//! Base types, user-defined types and typed parsing of cell text.

use crate::units::Unit;
use crate::value::{parse_date, parse_number, Value};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub enum Base {
    /// No declared type: inferred per cell.
    Auto,
    Int,
    Decimal(u8),
    Float,
    Text,
    Bool,
    Date,
    Time,
    DateTime,
    Duration,
    Ref,
    Enum(Vec<String>),
    Record(Vec<(String, String)>),
}

impl std::fmt::Display for Base {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Base::Auto => write!(f, "auto"),
            Base::Int => write!(f, "int"),
            Base::Decimal(n) => write!(f, "decimal({n})"),
            Base::Float => write!(f, "float"),
            Base::Text => write!(f, "text"),
            Base::Bool => write!(f, "bool"),
            Base::Date => write!(f, "date"),
            Base::Time => write!(f, "time"),
            Base::DateTime => write!(f, "datetime"),
            Base::Duration => write!(f, "duration"),
            Base::Ref => write!(f, "ref"),
            Base::Enum(v) => write!(f, "enum({})", v.join(", ")),
            Base::Record(fields) => {
                let parts: Vec<String> = fields.iter().map(|(n, t)| format!("{n}: {t}")).collect();
                write!(f, "record({})", parts.join(", "))
            }
        }
    }
}

/// Built-in types and modifiers with a short description, for the type overview.
pub const BUILTINS: &[(&str, &str)] = &[
    ("int", "whole number"),
    ("decimal(n)", "fixed-point number, rounded to n decimals"),
    ("float", "floating-point number"),
    ("text", "any text"),
    ("bool", "true/false, yes/no (also ja/nein, wahr/falsch)"),
    ("date", "2026-10-08 or 08.10.2026"),
    ("time", "time of day (stored as text for now)"),
    ("datetime", "date and time (stored as text for now)"),
    ("duration", "length of time (stored as text for now)"),
    ("ref", "reference to a cell or table (stored as text for now)"),
    ("enum(a, b, …)", "one of the listed values"),
    ("record(f: type, …)", "structured value (stored as text for now)"),
    ("… unit \"€\"", "modifier: attach a unit, e.g. decimal(2) unit \"€\""),
    ("… check iban", "modifier: validate with a named check (iban, email, host, ip, cidr, url)"),
    ("… range 1..65535", "modifier: allowed numbers, open with 0.. or ..100"),
    ("… pattern \"re\"", "modifier: regular expression the whole text must match"),
    ("… length ..100", "modifier: number of characters"),
    ("… secret", "modifier: shown masked"),
    ("… multiline", "modifier: edited as multi-line text"),
    ("… special(\"w\": \"meaning\")", "modifier: extra value with a fixed meaning, always valid"),
];

/// Whether `name` is a built-in type name (cannot be redefined).
pub fn is_builtin(name: &str) -> bool {
    matches!(
        name,
        "auto" | "int" | "decimal" | "float" | "text" | "bool" | "date" | "time" | "datetime" | "duration" | "ref"
            | "enum" | "record"
    )
}

/// Inclusive bounds such as `1..65535`, `0..` or `..100`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Bounds {
    pub min: Option<f64>,
    pub max: Option<f64>,
}

impl Bounds {
    pub fn parse(s: &str) -> Result<Bounds, String> {
        let (a, b) = s.split_once("..").ok_or_else(|| format!("expected a..b: {s}"))?;
        let num = |t: &str| -> Result<Option<f64>, String> {
            if t.is_empty() { Ok(None) } else { t.parse().map(Some).map_err(|_| format!("bad bound '{t}'")) }
        };
        let bounds = Bounds { min: num(a.trim())?, max: num(b.trim())? };
        if bounds.min.is_none() && bounds.max.is_none() {
            return Err("range needs at least one bound".into());
        }
        Ok(bounds)
    }

    pub fn contains(&self, n: f64) -> bool {
        self.min.is_none_or(|m| n >= m) && self.max.is_none_or(|m| n <= m)
    }
}

impl std::fmt::Display for Bounds {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(m) = self.min {
            write!(f, "{m}")?;
        }
        write!(f, "..")?;
        if let Some(m) = self.max {
            write!(f, "{m}")?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Type {
    pub base: Base,
    pub unit: Unit,
    /// Named check, e.g. `iban`.
    pub check: Option<String>,
    pub range: Option<Bounds>,
    /// Regular expression the whole text must match.
    pub pattern: Option<String>,
    /// Allowed number of characters.
    pub length: Option<Bounds>,
    pub secret: bool,
    pub multiline: bool,
    /// Extra values with a fixed meaning, always valid: `(value, meaning)`.
    pub special: Vec<(String, String)>,
}

impl Type {
    pub fn auto() -> Type {
        Type {
            base: Base::Auto,
            unit: Unit::none(),
            check: None,
            range: None,
            pattern: None,
            length: None,
            secret: false,
            multiline: false,
            special: Vec::new(),
        }
    }

    /// Meaning of `raw` if it is one of the `special` values.
    pub fn special_meaning(&self, raw: &str) -> Option<&str> {
        self.special.iter().find(|(v, _)| v == raw).map(|(_, m)| m.as_str())
    }

    pub fn decimals(&self) -> Option<u8> {
        match self.base {
            Base::Decimal(n) => Some(n),
            _ => None,
        }
    }
}

/// Unresolved definition as written: base spec plus modifiers.
#[derive(Debug, Clone, PartialEq, Default)]
struct Spec {
    head: String,
    args: Vec<String>,
    unit: Option<Unit>,
    check: Option<String>,
    range: Option<Bounds>,
    pattern: Option<String>,
    length: Option<Bounds>,
    secret: bool,
    multiline: bool,
    special: Vec<(String, String)>,
}

#[derive(Debug, Default)]
pub struct TypeRegistry {
    specs: HashMap<String, Spec>,
}

impl TypeRegistry {
    /// Adds the definitions of a ```types block. Returns one message per bad line.
    pub fn add_block(&mut self, lines: &[String]) -> Vec<String> {
        let mut errors = Vec::new();
        for line in lines {
            let t = line.trim();
            if t.is_empty() || t.starts_with('#') {
                continue;
            }
            let Some((name, spec)) = t.split_once('=') else {
                errors.push(format!("expected `Name = type`: {t}"));
                continue;
            };
            match parse_spec(spec) {
                Ok(s) => {
                    self.specs.insert(name.trim().to_string(), s);
                }
                Err(e) => errors.push(format!("{}: {e}", name.trim())),
            }
        }
        errors
    }

    /// Resolves a type expression such as `Euro`, `decimal(2)` or `int unit "m"`.
    pub fn resolve(&self, expr: &str) -> Result<Type, String> {
        self.resolve_spec(&parse_spec(expr)?, 0)
    }

    fn resolve_spec(&self, spec: &Spec, depth: usize) -> Result<Type, String> {
        if depth > 32 {
            return Err(format!("type definition cycle at '{}'", spec.head));
        }
        let arity = |n: usize| {
            if spec.args.len() == n {
                Ok(())
            } else {
                Err(format!("{} expects {n} argument(s)", spec.head))
            }
        };
        let simple = |b: Base| arity(0).map(|_| b);
        let mut ty = match spec.head.as_str() {
            "auto" => Type { base: simple(Base::Auto)?, ..Type::auto() },
            "int" => Type { base: simple(Base::Int)?, ..Type::auto() },
            "float" => Type { base: simple(Base::Float)?, ..Type::auto() },
            "text" => Type { base: simple(Base::Text)?, ..Type::auto() },
            "bool" => Type { base: simple(Base::Bool)?, ..Type::auto() },
            "date" => Type { base: simple(Base::Date)?, ..Type::auto() },
            "time" => Type { base: simple(Base::Time)?, ..Type::auto() },
            "datetime" => Type { base: simple(Base::DateTime)?, ..Type::auto() },
            "duration" => Type { base: simple(Base::Duration)?, ..Type::auto() },
            "ref" => Type { base: simple(Base::Ref)?, ..Type::auto() },
            "decimal" => {
                arity(1)?;
                let n = spec.args[0].parse().map_err(|_| "decimal(n) needs a number".to_string())?;
                Type { base: Base::Decimal(n), ..Type::auto() }
            }
            "enum" => {
                if spec.args.is_empty() {
                    return Err("enum needs at least one variant".into());
                }
                Type { base: Base::Enum(spec.args.clone()), ..Type::auto() }
            }
            "record" => {
                let fields = spec
                    .args
                    .iter()
                    .map(|a| match a.split_once(':') {
                        Some((n, t)) => Ok((n.trim().to_string(), t.trim().to_string())),
                        None => Err(format!("record field needs `name: type`: {a}")),
                    })
                    .collect::<Result<_, _>>()?;
                Type { base: Base::Record(fields), ..Type::auto() }
            }
            name => {
                let parent = self.specs.get(name).ok_or_else(|| format!("unknown type '{name}'"))?;
                arity(0)?;
                self.resolve_spec(parent, depth + 1)?
            }
        };
        if let Some(u) = &spec.unit {
            ty.unit = u.clone();
        }
        if let Some(c) = &spec.check {
            ty.check = Some(c.clone());
        }
        if spec.range.is_some() {
            ty.range = spec.range;
        }
        if let Some(p) = &spec.pattern {
            ty.pattern = Some(p.clone());
        }
        if spec.length.is_some() {
            ty.length = spec.length;
        }
        ty.secret |= spec.secret;
        ty.multiline |= spec.multiline;
        ty.special.extend(spec.special.iter().cloned());
        Ok(ty)
    }
}

/// `decimal(2) unit "€" check iban`
fn parse_spec(src: &str) -> Result<Spec, String> {
    let src = src.trim();
    let head_end = src.find(|c: char| c == '(' || c.is_whitespace()).unwrap_or(src.len());
    let head = src[..head_end].to_string();
    if head.is_empty() {
        return Err("missing type".into());
    }
    let mut rest = &src[head_end..];
    let mut args = Vec::new();
    if rest.starts_with('(') {
        let close = matching_paren(rest).ok_or("missing ')'")?;
        args = split_top_level(&rest[1..close]);
        rest = &rest[close + 1..];
    }

    let mut spec = Spec { head, args, ..Spec::default() };
    let mut rest = rest.trim();
    while !rest.is_empty() {
        let kw_end = rest.find(|c: char| c == '(' || c.is_whitespace()).unwrap_or(rest.len());
        let (kw, r) = (&rest[..kw_end], rest[kw_end..].trim_start());
        // A modifier argument: a "string" or a bare word up to the next space.
        let word = |r: &str| -> (String, usize) {
            let end = r.find(char::is_whitespace).unwrap_or(r.len());
            (r[..end].to_string(), end)
        };
        rest = match kw {
            "unit" => {
                let (sym, used) = if r.starts_with('"') { take_string(r)? } else { word(r) };
                spec.unit = Some(Unit::parse(&sym).ok_or_else(|| format!("bad unit '{sym}'"))?);
                &r[used..]
            }
            "check" => {
                let (name, used) = word(r);
                if !CHECKS.contains(&name.as_str()) {
                    return Err(format!("unknown check '{name}'"));
                }
                spec.check = Some(name);
                &r[used..]
            }
            "range" | "length" => {
                let (b, used) = word(r);
                let b = Bounds::parse(&b)?;
                if kw == "range" { spec.range = Some(b) } else { spec.length = Some(b) }
                &r[used..]
            }
            "pattern" => {
                let (re, used) = take_string(r)?;
                regex::Regex::new(&re).map_err(|e| format!("bad pattern: {e}"))?;
                spec.pattern = Some(re);
                &r[used..]
            }
            "secret" => {
                spec.secret = true;
                r
            }
            "multiline" => {
                spec.multiline = true;
                r
            }
            "special" => {
                let close = matching_paren(r).filter(|_| r.starts_with('(')).ok_or("special needs (\"value\": \"meaning\")")?;
                for part in split_top_level(&r[1..close]) {
                    let (value, used) = take_string(&part)?;
                    let after = part[used..].trim_start();
                    let meaning = match after.strip_prefix(':') {
                        Some(m) => take_string(m.trim_start())?.0,
                        None if after.is_empty() => String::new(),
                        None => return Err(format!("expected \"value\": \"meaning\": {part}")),
                    };
                    spec.special.push((value, meaning));
                }
                &r[close + 1..]
            }
            _ => return Err(format!("unexpected '{rest}'")),
        }
        .trim_start();
    }
    Ok(spec)
}

/// Names accepted by `check`.
pub const CHECKS: &[&str] = &["iban", "email", "host", "ip", "cidr", "url"];

/// Reads a `"…"` string at the start of `s`; only `\"` is an escape, other
/// backslashes stay as written (so patterns read naturally). Returns the
/// content and the number of bytes consumed.
pub fn take_string(s: &str) -> Result<(String, usize), String> {
    let body = s.strip_prefix('"').ok_or_else(|| format!("expected \"…\": {s}"))?;
    let mut out = String::new();
    let mut chars = body.char_indices();
    while let Some((i, c)) = chars.next() {
        match c {
            '"' => return Ok((out, i + 2)),
            '\\' if body[i + 1..].starts_with('"') => {
                out.push('"');
                chars.next();
            }
            c => out.push(c),
        }
    }
    Err("unterminated string".into())
}

pub(crate) fn matching_paren(s: &str) -> Option<usize> {
    let mut depth = 0;
    let mut in_str = false;
    let mut prev = ' ';
    for (i, c) in s.char_indices() {
        if in_str {
            in_str = !(c == '"' && prev != '\\');
            prev = c;
            continue;
        }
        prev = c;
        match c {
            '"' => in_str = true,
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

pub(crate) fn split_top_level(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth = 0;
    let mut in_str = false;
    let mut prev = ' ';
    let mut cur = String::new();
    for c in s.chars() {
        if in_str {
            in_str = !(c == '"' && prev != '\\');
            prev = c;
            cur.push(c);
            continue;
        }
        prev = c;
        match c {
            '"' => in_str = true,
            '(' => depth += 1,
            ')' => depth -= 1,
            ',' if depth == 0 => {
                out.push(cur.trim().to_string());
                cur.clear();
                continue;
            }
            _ => {}
        }
        cur.push(c);
    }
    if !cur.trim().is_empty() {
        out.push(cur.trim().to_string());
    }
    out
}

/// Result of reading one cell: its value and whether it used `,` as decimal separator.
pub struct Parsed {
    pub value: Value,
    pub comma_decimal: bool,
}

/// The number at the start of a cell and the rest (unit, `%`): `1.200,50 €`
/// → (`1.200,50`, ` €`).
fn split_number(raw: &str) -> (&str, &str) {
    let t = raw.trim();
    let end = t
        .char_indices()
        .find(|(_, c)| !(c.is_ascii_digit() || matches!(c, '.' | ',' | '+' | '-' | ' ' | '\'')))
        .map_or(t.len(), |(i, _)| i);
    let num = t[..end].trim_end();
    (num, &t[num.len()..])
}

/// The raw text of a number cell as written in files, with a decimal point:
/// `1200,50` → `1200.50`, `5,5 m/s` → `5.5 m/s`. `None` if the cell is no
/// number or already written with a point.
pub fn point_cell(raw: &str, ty: &Type) -> Option<String> {
    let parsed = parse_cell(raw, ty);
    if !matches!(parsed.value, Value::Num(..)) || !parsed.comma_decimal {
        return None;
    }
    let (num, rest) = split_number(raw);
    Some(format!("{}{rest}", crate::value::point_number(num)?))
}

/// The raw text of a number cell as shown for editing: with the decimal comma
/// when the display uses it (`1200.50` → `1200,50`). Other cells as they are.
pub fn shown_cell(raw: &str, ty: &Type) -> String {
    if !crate::locale::decimal_comma() || !matches!(parse_cell(raw, ty).value, Value::Num(..)) {
        return raw.to_string();
    }
    let (num, rest) = split_number(raw);
    if num.contains(',') {
        return raw.to_string();
    }
    format!("{}{rest}", num.replace('.', ","))
}

/// Interprets the raw text of a (non-formula) cell according to `ty`.
pub fn parse_cell(raw: &str, ty: &Type) -> Parsed {
    let raw = raw.trim();
    let plain = |value| Parsed { value, comma_decimal: false };
    if ty.special_meaning(raw).is_some() {
        return plain(Value::Text(raw.to_string()));
    }
    if raw.is_empty() {
        return plain(Value::Null);
    }
    let number = |unit: &Unit| -> Option<Parsed> {
        // Accept an optional trailing unit symbol matching the type, e.g. `1200 €`.
        let unit_str = unit.to_string();
        let digits = if unit.is_none() { raw } else { raw.strip_suffix(unit_str.as_str()).unwrap_or(raw) };
        parse_number(digits).map(|(n, comma)| Parsed { value: Value::Num(n, unit.clone()), comma_decimal: comma })
    };

    let parsed = match &ty.base {
        Base::Auto => {
            // Dates first: `08.10.2026` would otherwise read as a number with thousands dots.
            if let Some(d) = parse_date(raw) {
                plain(Value::Date(d))
            } else if raw.len() > 1 && raw.starts_with('0') && raw.chars().all(|c| c.is_ascii_digit()) {
                // `01234`, `007`: a code (postal code, number of an item) whose
                // zeros matter. A column of type `int` reads it as a number.
                plain(Value::Text(raw.to_string()))
            } else if let Some(p) = number(&ty.unit) {
                p
            } else if let Some(b) = parse_bool(raw) {
                plain(Value::Bool(b))
            } else if let Some(p) = number_with_unit(raw) {
                p
            } else {
                plain(Value::Text(raw.to_string()))
            }
        }
        Base::Int => match number(&ty.unit) {
            Some(p) if matches!(p.value, Value::Num(n, _) if n.fract() == 0.0) => p,
            _ => plain(Value::err(format!("not an integer: {raw}"))),
        },
        Base::Decimal(_) | Base::Float => {
            number(&ty.unit).unwrap_or_else(|| plain(Value::err(format!("not a number: {raw}"))))
        }
        Base::Bool => plain(parse_bool(raw).map(Value::Bool).unwrap_or_else(|| Value::err(format!("not a bool: {raw}")))),
        Base::Date => plain(parse_date(raw).map(Value::Date).unwrap_or_else(|| Value::err(format!("not a date: {raw}")))),
        Base::Enum(variants) => {
            if variants.iter().any(|v| v == raw) {
                plain(Value::Text(raw.to_string()))
            } else {
                plain(Value::err(format!("'{raw}' not in enum({})", variants.join(", "))))
            }
        }
        // Not yet interpreted beyond text.
        Base::Text | Base::Time | Base::DateTime | Base::Duration | Base::Ref | Base::Record(_) => {
            plain(Value::Text(raw.to_string()))
        }
    };

    if parsed.value.is_error() {
        return parsed;
    }
    match check_constraints(ty, raw, &parsed.value) {
        Ok(()) => parsed,
        Err(e) => plain(Value::err(e)),
    }
}

/// Checks `range`, `length`, `pattern` and `check` of `ty` against a value
/// that already has the right base type. `raw` is the text as written.
pub fn check_constraints(ty: &Type, raw: &str, value: &Value) -> Result<(), String> {
    if ty.special_meaning(raw).is_some() {
        return Ok(());
    }
    if let (Some(b), Value::Num(n, _)) = (&ty.range, value)
        && !b.contains(*n) {
            return Err(format!("{raw} outside {b}"));
        }
    if let (Some(b), Value::Text(t)) = (&ty.length, value) {
        let n = t.chars().count();
        if !b.contains(n as f64) {
            return Err(format!("length {n} outside {b}"));
        }
    }
    if let (Some(p), Value::Text(t)) = (&ty.pattern, value) {
        let re = regex::Regex::new(&format!("^(?:{p})$")).map_err(|e| format!("bad pattern: {e}"))?;
        if !re.is_match(t) {
            return Err(format!("'{t}' does not match {p}"));
        }
    }
    match &ty.check {
        Some(check) => run_check(check, raw),
        None => Ok(()),
    }
}

fn number_with_unit(raw: &str) -> Option<Parsed> {
    let (num, unit) = raw.rsplit_once(' ')?;
    let unit = Unit::parse(unit)?;
    let (n, comma) = parse_number(num)?;
    Some(Parsed { value: Value::Num(n, unit), comma_decimal: comma })
}

pub fn parse_bool(s: &str) -> Option<bool> {
    match s.to_lowercase().as_str() {
        "true" | "yes" | "wahr" | "ja" => Some(true),
        "false" | "no" | "falsch" | "nein" => Some(false),
        _ => None,
    }
}

fn run_check(check: &str, raw: &str) -> Result<(), String> {
    match check {
        "iban" => {
            if iban_valid(raw) {
                Ok(())
            } else {
                Err(format!("invalid IBAN: {raw}"))
            }
        }
        "email" if email_valid(raw) => Ok(()),
        "email" => Err(format!("invalid e-mail address: {raw}")),
        "host" if host_valid(raw) => Ok(()),
        "host" => Err(format!("invalid host name: {raw}")),
        "ip" if raw.parse::<std::net::IpAddr>().is_ok() => Ok(()),
        "ip" => Err(format!("invalid IP address: {raw}")),
        "cidr" if cidr_valid(raw) => Ok(()),
        "cidr" => Err(format!("invalid network (a.b.c.d/n): {raw}")),
        "url" if url_valid(raw) => Ok(()),
        "url" => Err(format!("invalid URL: {raw}")),
        other => Err(format!("unknown check '{other}'")),
    }
}

/// Host name (RFC 1123 labels) or IP address.
fn host_valid(s: &str) -> bool {
    if s.parse::<std::net::IpAddr>().is_ok() {
        return true;
    }
    let label = |l: &str| {
        !l.is_empty()
            && l.len() <= 63
            && l.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
            && !l.starts_with('-')
            && !l.ends_with('-')
    };
    s.len() <= 253 && s.trim_end_matches('.').split('.').all(label)
}

fn email_valid(s: &str) -> bool {
    match s.rsplit_once('@') {
        Some((local, domain)) => {
            !local.is_empty() && !local.chars().any(|c| c.is_whitespace() || c == '@') && domain.contains('.') && host_valid(domain)
        }
        None => false,
    }
}

fn cidr_valid(s: &str) -> bool {
    let Some((ip, bits)) = s.split_once('/') else { return false };
    let Ok(bits) = bits.parse::<u8>() else { return false };
    match ip.parse::<std::net::IpAddr>() {
        Ok(std::net::IpAddr::V4(_)) => bits <= 32,
        Ok(std::net::IpAddr::V6(_)) => bits <= 128,
        Err(_) => false,
    }
}

/// `scheme://host[:port][/…]`.
fn url_valid(s: &str) -> bool {
    let Some((scheme, rest)) = s.split_once("://") else { return false };
    let scheme_ok = scheme.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
        && scheme.chars().all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c));
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    let host = match host.strip_prefix('[') {
        Some(v6) => return scheme_ok && v6.split(']').next().is_some_and(|a| a.parse::<std::net::Ipv6Addr>().is_ok()),
        None => host,
    };
    let (host, port_ok) = match host.rsplit_once(':') {
        Some((h, p)) => (h, p.parse::<u16>().is_ok()),
        None => (host, true),
    };
    scheme_ok && port_ok && host_valid(host)
}

fn iban_valid(s: &str) -> bool {
    let s: String = s.chars().filter(|c| !c.is_whitespace()).collect::<String>().to_uppercase();
    if s.len() < 15 || s.len() > 34 || !s.chars().all(|c| c.is_ascii_alphanumeric()) {
        return false;
    }
    let rearranged = format!("{}{}", &s[4..], &s[..4]);
    let mut rem: u32 = 0;
    for c in rearranged.chars() {
        let digits = if c.is_ascii_digit() { c.to_string() } else { (c as u32 - 'A' as u32 + 10).to_string() };
        for d in digits.chars() {
            rem = (rem * 10 + d.to_digit(10).unwrap()) % 97;
        }
    }
    rem == 1
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registry() -> TypeRegistry {
        let mut r = TypeRegistry::default();
        let errs = r.add_block(&[
            "Euro = decimal(2) unit \"€\"".into(),
            "Price = Euro".into(),
            "Status = enum(open, paid)".into(),
            "IBAN = text check iban".into(),
            "Speed = float unit m/s".into(),
        ]);
        assert!(errs.is_empty(), "{errs:?}");
        r
    }

    #[test]
    fn aliases_inherit() {
        let t = registry().resolve("Price").unwrap();
        assert_eq!(t.base, Base::Decimal(2));
        assert_eq!(t.unit, Unit::symbol("€"));
    }

    #[test]
    fn typed_cells() {
        let r = registry();
        let euro = r.resolve("Euro").unwrap();
        assert_eq!(parse_cell("1200 €", &euro).value, Value::Num(1200.0, Unit::symbol("€")));
        assert!(parse_cell("viel", &euro).value.is_error());
        let status = r.resolve("Status").unwrap();
        assert!(parse_cell("cancelled", &status).value.is_error());
        let iban = r.resolve("IBAN").unwrap();
        assert!(!parse_cell("DE89 3704 0044 0532 0130 00", &iban).value.is_error());
        assert!(parse_cell("DE89 3704 0044 0532 0130 01", &iban).value.is_error());
    }

    #[test]
    fn inline_and_unknown() {
        let r = registry();
        assert_eq!(r.resolve("int unit \"m\"").unwrap().unit, Unit::symbol("m"));
        assert!(r.resolve("Dollar").is_err());
    }

    #[test]
    fn config_modifiers() {
        let mut r = TypeRegistry::default();
        let errs = r.add_block(&[
            "Port = int range 1..65535".into(),
            r#"Name = text pattern "^[a-z][a-z0-9_-]*$" length ..8"#.into(),
            r#"Muster = text pattern "^(@[^@\s]+|[^@\s]+@[^@\s]+)$""#.into(),
            r#"Password = text secret special("********": "keep", "new")"#.into(),
            "Netz = text check cidr".into(),
        ]);
        assert!(errs.is_empty(), "{errs:?}");
        let port = r.resolve("Port").unwrap();
        assert!(!parse_cell("445", &port).value.is_error());
        assert!(parse_cell("0", &port).value.is_error());
        assert!(parse_cell("70000", &port).value.is_error());
        let name = r.resolve("Name").unwrap();
        assert!(!parse_cell("nas-1", &name).value.is_error());
        assert!(parse_cell("Nas", &name).value.is_error());
        assert!(parse_cell("abcdefghi", &name).value.is_error());
        let muster = r.resolve("Muster").unwrap();
        assert!(!parse_cell("@example.com", &muster).value.is_error());
        assert!(parse_cell("a b@example.com", &muster).value.is_error());
        let pw = r.resolve("Password").unwrap();
        assert!(pw.secret);
        assert_eq!(pw.special_meaning("********"), Some("keep"));
        assert_eq!(pw.special_meaning("new"), Some(""));
        // A field can add modifiers to a named type.
        let field = r.resolve(r#"Port special("0": "off")"#).unwrap();
        assert!(!parse_cell("0", &field).value.is_error());
        let netz = r.resolve("Netz").unwrap();
        assert!(!parse_cell("192.168.178.0/24", &netz).value.is_error());
        assert!(parse_cell("192.168.178.0/33", &netz).value.is_error());
        assert!(r.resolve("text check nope").is_err());
        assert!(r.resolve(r#"text pattern "(""#).is_err());
    }

    #[test]
    fn named_checks() {
        for (check, good, bad) in [
            ("email", "a@example.com", "a@b"),
            ("host", "nas.example.lan", "-nas"),
            ("host", "192.168.1.2", "nas_1"),
            ("ip", "::1", "1.2.3"),
            ("url", "https://example.com:8443/x", "example.com"),
        ] {
            assert!(run_check(check, good).is_ok(), "{check} {good}");
            assert!(run_check(check, bad).is_err(), "{check} {bad}");
        }
    }

    #[test]
    fn numbers_in_files_use_the_point() {
        let euro = TypeRegistry::default().resolve("decimal(2) unit \"€\"").unwrap();
        assert_eq!(point_cell("1200,50", &Type::auto()).as_deref(), Some("1200.50"));
        assert_eq!(point_cell("1.200,50 €", &euro).as_deref(), Some("1200.50 €"));
        assert_eq!(point_cell("5,5 m/s", &Type::auto()).as_deref(), Some("5.5 m/s"));
        assert_eq!(point_cell("1200.50", &Type::auto()), None, "already a point");
        assert_eq!(point_cell("08.10.2026", &Type::auto()), None, "a date");
        assert_eq!(point_cell("a,b", &Type::auto()), None, "text");
        assert_eq!(shown_cell("1200.50", &Type::auto()), "1200.50", "the display uses the point by default");
        crate::locale::set_decimal_comma_for_this_thread(true);
        assert_eq!(shown_cell("1200.50", &Type::auto()), "1200,50");
        assert_eq!(shown_cell("5.5 m/s", &Type::auto()), "5,5 m/s");
        assert_eq!(shown_cell("1,200.50", &Type::auto()), "1,200.50", "thousands separators stay");
        assert_eq!(shown_cell("08.10.2026", &Type::auto()), "08.10.2026");
    }

    #[test]
    fn auto_infers() {
        let t = Type::auto();
        assert_eq!(parse_cell("5 m/s", &t).value, Value::Num(5.0, Unit::parse("m/s").unwrap()));
        assert_eq!(parse_cell("ja", &t).value, Value::Bool(true));
        assert_eq!(parse_cell("No", &t).value, Value::Bool(false));
        assert_eq!(parse_cell("Rent", &t).value, Value::Text("Rent".into()));
    }
}
