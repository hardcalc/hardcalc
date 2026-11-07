//! The `#%` schema block: types, root and structs (CONFIG-SPEC §2, §3).

use super::path::{is_bare, Path, Segment};
use crate::types::{matching_paren, split_top_level, take_string, Base, Bounds, Type, TypeRegistry};

/// Type of a field after resolution.
#[derive(Debug, Clone, PartialEq)]
pub enum FieldType {
    /// A hardcalc type; `name` is the user type it was written as, e.g. `Port`.
    Scalar { ty: Type, name: Option<String> },
    List { item: Box<FieldType>, length: Option<Bounds> },
    /// Table with arbitrary keys; `key` is a text-like scalar or a `Ref`;
    /// `length` limits the number of entries.
    Map { value: Box<FieldType>, key: Box<FieldType>, length: Option<Bounds> },
    /// Key of an entry of the map at the path.
    Ref(Path),
    Struct(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Test {
    /// `when flag`: bool is true, other values are set.
    Set,
    /// `when f = a` or `when f in (a, b)`.
    In(Vec<String>),
    /// `when f != a`.
    NotIn(Vec<String>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Condition {
    pub field: String,
    pub test: Test,
}

impl std::fmt::Display for Condition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.test {
            Test::Set => write!(f, "{}", self.field),
            Test::In(v) if v.len() == 1 => write!(f, "{} = {}", self.field, v[0]),
            Test::In(v) => write!(f, "{} in ({})", self.field, v.join(", ")),
            Test::NotIn(v) => write!(f, "{} != {}", self.field, v.join(", ")),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Field {
    pub name: String,
    pub optional: bool,
    pub ty: FieldType,
    /// The type expression as written.
    pub ty_src: String,
    pub default: Option<toml_edit::Value>,
    pub readonly: bool,
    pub when: Option<Condition>,
    pub hint: Option<String>,
    pub doc: Vec<String>,
    /// Line in the file.
    pub line: usize,
}

impl Field {
    /// Neither optional nor defaulted: must be present.
    pub fn required(&self) -> bool {
        !self.optional && self.default.is_none()
    }
}

#[derive(Debug, Clone)]
pub struct Struct {
    /// Empty for the root.
    pub name: String,
    pub title: Option<String>,
    pub doc: Vec<String>,
    pub fields: Vec<Field>,
    pub line: usize,
}

impl Struct {
    pub fn field(&self, name: &str) -> Option<&Field> {
        self.fields.iter().find(|f| f.name == name)
    }

    /// Heading in the form: the title, otherwise the name.
    pub fn label(&self) -> &str {
        self.title.as_deref().unwrap_or(&self.name)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TypeDef {
    pub name: String,
    pub src: String,
    pub hint: Option<String>,
    pub doc: Vec<String>,
    pub line: usize,
}

#[derive(Debug)]
pub struct Schema {
    pub version: u32,
    pub types: TypeRegistry,
    pub type_defs: Vec<TypeDef>,
    pub root: Struct,
    pub structs: Vec<Struct>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SchemaError {
    pub line: usize,
    pub message: String,
}

impl std::fmt::Display for SchemaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}

pub const VERSION: u32 = 1;

impl Schema {
    pub fn struct_def(&self, name: &str) -> Option<&Struct> {
        self.structs.iter().find(|s| s.name == name)
    }

    pub fn type_def(&self, name: &str) -> Option<&TypeDef> {
        self.type_defs.iter().find(|t| t.name == name)
    }

    /// The field's hint, otherwise the hint of its named type (§2.1).
    pub fn hint<'a>(&'a self, field: &'a Field) -> Option<&'a str> {
        field.hint.as_deref().or_else(|| match &field.ty {
            FieldType::Scalar { name: Some(n), .. } => self.type_def(n)?.hint.as_deref(),
            _ => None,
        })
    }

    /// Label of a map key, e.g. for "New entry – Name:" (§3.3).
    pub fn key_label(&self, key: &FieldType) -> String {
        match key {
            FieldType::Scalar { name: Some(n), .. } => {
                self.type_def(n).and_then(|t| t.hint.clone()).unwrap_or_else(|| n.clone())
            }
            FieldType::Ref(p) => match self.field_at(p).map(|f| &f.ty) {
                Some(FieldType::Map { value, .. }) => match value.as_ref() {
                    FieldType::Struct(s) => self.struct_def(s).map_or(s.as_str(), |s| s.label()).to_string(),
                    _ => p.to_string(),
                },
                _ => p.to_string(),
            },
            _ => "Key".into(),
        }
    }

    /// The field a schema path leads to, through structs only (maps have no fixed keys).
    pub fn field_at(&self, path: &Path) -> Option<&Field> {
        let mut st = &self.root;
        let mut field: Option<&Field> = None;
        for seg in &path.0 {
            if let Some(f) = field {
                st = match &f.ty {
                    FieldType::Struct(s) => self.struct_def(s)?,
                    _ => return None,
                };
            }
            let Segment::Key(k) = seg else { return None };
            field = Some(st.field(k)?);
        }
        field
    }

    /// Parses the schema lines `(line number, content after #%)`, diagnostics
    /// excluded. The first line must be `hardconf <version>`. Bad lines are
    /// reported and skipped; `None` when there is no usable schema.
    pub fn parse(lines: &[(usize, &str)]) -> (Option<Schema>, Vec<SchemaError>) {
        let mut errors = Vec::new();
        let Some(&(first_line, first)) = lines.first() else { return (None, errors) };
        let version = match first.strip_prefix("hardconf").map(|v| v.trim().parse::<u32>()) {
            Some(Ok(v)) if v == VERSION => v,
            Some(Ok(v)) => {
                errors.push(SchemaError { line: first_line, message: format!("unsupported schema version {v}") });
                return (None, errors);
            }
            _ => {
                errors.push(SchemaError { line: first_line, message: "expected `#%hardconf 1`".into() });
                return (None, errors);
            }
        };

        let raw = read_sections(&lines[1..], &mut errors);
        let Some(raw_root) = raw.root else {
            errors.push(SchemaError { line: first_line, message: "schema has no `root` section".into() });
            return (None, errors);
        };

        let mut types = TypeRegistry::default();
        let mut type_defs: Vec<TypeDef> = Vec::new();
        for t in raw.types {
            let line = t.line;
            let err = |m: String| SchemaError { line, message: format!("type {}: {m}", t.name) };
            if crate::types::is_builtin(&t.name) || matches!(t.name.as_str(), "list" | "map") {
                errors.push(err("is a built-in type".into()));
            } else if raw.structs.iter().any(|s| s.name == t.name) {
                errors.push(err("is also a struct".into()));
            } else if type_defs.iter().any(|d| d.name == t.name) {
                errors.push(err("defined twice".into()));
            } else {
                match types.add_block(&[format!("{} = {}", t.name, t.src)]).pop() {
                    Some(e) => errors.push(SchemaError { line, message: e }),
                    None => type_defs.push(t),
                }
            }
        }
        // Undefined parents and cycles only show on resolution.
        type_defs.retain(|t| match types.resolve(&t.name) {
            Ok(_) => true,
            Err(e) => {
                errors.push(SchemaError { line: t.line, message: format!("type {}: {e}", t.name) });
                false
            }
        });

        let struct_names: Vec<String> = raw.structs.iter().map(|s| s.name.clone()).collect();
        let mut schema = Schema {
            version,
            types,
            type_defs,
            root: Struct { name: String::new(), title: raw_root.title, doc: raw_root.doc, fields: Vec::new(), line: raw_root.line },
            structs: Vec::new(),
        };
        let mut pending = vec![(None, raw_root.fields)];
        for s in raw.structs {
            if schema.structs.iter().any(|d| d.name == s.name) {
                errors.push(SchemaError { line: s.line, message: format!("struct {} defined twice", s.name) });
                continue;
            }
            pending.push((Some(schema.structs.len()), s.fields));
            schema.structs.push(Struct { name: s.name, title: s.title, doc: s.doc, fields: Vec::new(), line: s.line });
        }
        for (idx, fields) in pending {
            let mut resolved: Vec<Field> = Vec::new();
            for f in fields {
                if resolved.iter().any(|r| r.name == f.name) {
                    errors.push(SchemaError { line: f.line, message: format!("field {} defined twice", f.name) });
                    continue;
                }
                match schema.resolve_field(f, &struct_names) {
                    Ok(f) => resolved.push(f),
                    Err(e) => errors.push(e),
                }
            }
            match idx {
                None => schema.root.fields = resolved,
                Some(i) => schema.structs[i].fields = resolved,
            }
        }
        schema.check_references(&mut errors);
        errors.sort_by_key(|e| e.line);
        (Some(schema), errors)
    }

    fn resolve_field(&self, f: RawField, structs: &[String]) -> Result<Field, SchemaError> {
        let line = f.line;
        let err = |m: String| SchemaError { line, message: format!("{}: {m}", f.name) };
        let ty = self.resolve_type(&f.ty_src, structs).map_err(err)?;
        let default = match &f.default {
            Some(raw) => Some(raw.parse::<toml_edit::Value>().map_err(|_| err(format!("bad default value {raw}")))?),
            None => None,
        };
        Ok(Field {
            name: f.name,
            optional: f.optional,
            ty,
            ty_src: f.ty_src,
            default,
            readonly: f.readonly,
            when: f.when,
            hint: f.hint,
            doc: f.doc,
            line,
        })
    }

    /// Resolves a field type expression such as `map(Agent, Name)` or `Port special("0": "off")`.
    pub fn resolve_type(&self, src: &str, structs: &[String]) -> Result<FieldType, String> {
        let src = src.trim();
        let head_end = src.find(|c: char| c == '(' || c.is_whitespace()).unwrap_or(src.len());
        let head = &src[..head_end];
        let mut rest = &src[head_end..];
        let mut args = None;
        if rest.starts_with('(') {
            let close = matching_paren(rest).ok_or("missing ')'")?;
            args = Some(split_top_level(&rest[1..close]));
            rest = &rest[close + 1..];
        }
        let modifiers = rest.trim();
        let no_modifiers = |what: &str| {
            if modifiers.is_empty() { Ok(()) } else { Err(format!("{what} takes no modifiers: {modifiers}")) }
        };
        match (head, args) {
            ("list", Some(a)) => {
                let [item] = a.as_slice() else { return Err("list expects 1 argument".into()) };
                let length = length_only("list", modifiers)?;
                Ok(FieldType::List { item: Box::new(self.resolve_type(item, structs)?), length })
            }
            ("map", Some(a)) => {
                let length = length_only("map", modifiers)?;
                let (value, key) = match a.as_slice() {
                    [v] => (v, None),
                    [v, k] => (v, Some(k)),
                    _ => return Err("map expects 1 or 2 arguments".into()),
                };
                let key = match key {
                    Some(k) => self.resolve_type(k, structs)?,
                    None => FieldType::Scalar { ty: Type { base: Base::Text, ..Type::auto() }, name: None },
                };
                match &key {
                    FieldType::Scalar { ty, .. } if matches!(ty.base, Base::Text | Base::Enum(_)) => {}
                    FieldType::Ref(_) => {}
                    _ => return Err("map keys must be text, enum or ref(…)".into()),
                }
                Ok(FieldType::Map { value: Box::new(self.resolve_type(value, structs)?), key: Box::new(key), length })
            }
            ("ref", Some(a)) => {
                no_modifiers("ref")?;
                let [p] = a.as_slice() else { return Err("ref expects 1 argument".into()) };
                Ok(FieldType::Ref(Path::parse(p)?))
            }
            (name, None) if structs.iter().any(|s| s == name) => {
                no_modifiers("a struct")?;
                Ok(FieldType::Struct(name.to_string()))
            }
            _ => {
                let ty = self.types.resolve(src)?;
                let name = self.type_def(head).map(|t| t.name.clone());
                Ok(FieldType::Scalar { ty, name })
            }
        }
    }

    /// `ref(…)` targets, `when` fields and default values.
    fn check_references(&self, errors: &mut Vec<SchemaError>) {
        for st in std::iter::once(&self.root).chain(&self.structs) {
            for f in &st.fields {
                let err = |m: String| SchemaError { line: f.line, message: format!("{}: {m}", f.name) };
                let mut refs = Vec::new();
                collect_refs(&f.ty, &mut refs);
                for p in refs {
                    if !matches!(self.field_at(p), Some(Field { ty: FieldType::Map { .. }, .. })) {
                        errors.push(err(format!("ref({p}) does not lead to a map")));
                    }
                }
                if let Some(c) = &f.when
                    && st.field(&c.field).is_none() {
                        errors.push(err(format!("when: no field {} in this struct", c.field)));
                    }
                if let Some(d) = &f.default {
                    let item = toml_edit::Item::Value(d.clone());
                    for diag in super::validate::Validator::new(self, None).check_default(&item, &f.ty) {
                        errors.push(err(format!("default value: {}", diag.message)));
                    }
                }
            }
        }
    }
}

/// `length a..b` as the only modifier of a list or map.
fn length_only(what: &str, modifiers: &str) -> Result<Option<Bounds>, String> {
    match modifiers.strip_prefix("length") {
        Some(b) => Ok(Some(Bounds::parse(b.trim())?)),
        None if modifiers.is_empty() => Ok(None),
        None => Err(format!("{what} only takes `length`: {modifiers}")),
    }
}

fn collect_refs<'a>(ty: &'a FieldType, out: &mut Vec<&'a Path>) {
    match ty {
        FieldType::Ref(p) => out.push(p),
        FieldType::List { item, .. } => collect_refs(item, out),
        FieldType::Map { value, key, .. } => {
            collect_refs(value, out);
            collect_refs(key, out);
        }
        _ => {}
    }
}

// ── Reading the lines ──────────────────────────────────────────────────────

struct RawField {
    name: String,
    optional: bool,
    ty_src: String,
    default: Option<String>,
    readonly: bool,
    when: Option<Condition>,
    hint: Option<String>,
    doc: Vec<String>,
    line: usize,
}

struct RawStruct {
    name: String,
    title: Option<String>,
    doc: Vec<String>,
    fields: Vec<RawField>,
    line: usize,
}

#[derive(Default)]
struct RawSchema {
    types: Vec<TypeDef>,
    root: Option<RawStruct>,
    structs: Vec<RawStruct>,
}

#[derive(Clone, Copy, PartialEq)]
enum Section {
    None,
    Types,
    Root,
    Struct,
}

fn read_sections(lines: &[(usize, &str)], errors: &mut Vec<SchemaError>) -> RawSchema {
    let mut raw = RawSchema::default();
    let mut section = Section::None;
    // Whether `///` lines currently attach to something.
    let mut doc_open = false;
    for &(line, content) in lines {
        let t = content.trim();
        if t.is_empty() || (t.starts_with('#')) {
            continue;
        }
        let err = |m: String| SchemaError { line, message: m };
        if let Some(d) = t.strip_prefix("///") {
            let d = d.strip_prefix(' ').unwrap_or(d).trim_end().to_string();
            let target = match section {
                _ if !doc_open => None,
                Section::Types => raw.types.last_mut().map(|t| &mut t.doc),
                Section::Root => raw.root.as_mut().map(|s| s.fields.last_mut().map_or(&mut s.doc, |f| &mut f.doc)),
                Section::Struct => raw.structs.last_mut().map(|s| s.fields.last_mut().map_or(&mut s.doc, |f| &mut f.doc)),
                Section::None => None,
            };
            match target {
                Some(doc) => doc.push(d),
                None => errors.push(err("`///` without a field, type or struct before it".into())),
            }
            continue;
        }
        doc_open = true;
        match header(t) {
            Some(Ok(("types", None, None))) => {
                section = Section::Types;
                doc_open = false;
            }
            Some(Ok(("root", None, title))) => {
                if raw.root.is_some() {
                    errors.push(err("second `root` section".into()));
                }
                raw.root = Some(RawStruct { name: String::new(), title, doc: Vec::new(), fields: Vec::new(), line });
                section = Section::Root;
            }
            Some(Ok(("struct", Some(name), title))) => {
                raw.structs.push(RawStruct { name, title, doc: Vec::new(), fields: Vec::new(), line });
                section = Section::Struct;
            }
            Some(Ok((kw, _, _))) => {
                errors.push(err(format!("malformed `{kw}` line")));
                section = Section::None;
            }
            Some(Err(e)) => {
                errors.push(err(e));
                section = Section::None;
            }
            None => {
                let parsed = match section {
                    Section::Types => parse_type_def(t, line).map(|d| raw.types.push(d)),
                    Section::Root | Section::Struct => parse_field(t, line).map(|f| {
                        let st = if section == Section::Root { raw.root.as_mut() } else { raw.structs.last_mut() };
                        st.expect("section has a struct").fields.push(f)
                    }),
                    Section::None => Err("line outside of `types`, `root` or `struct`".into()),
                };
                if let Err(e) = parsed {
                    errors.push(err(e));
                    doc_open = false;
                }
            }
        }
    }
    raw
}

/// `types`, `root ["title"]` or `struct Name ["title"]`; `None` for other lines.
#[allow(clippy::type_complexity)]
fn header(t: &str) -> Option<Result<(&'static str, Option<String>, Option<String>), String>> {
    let end = t.find(|c: char| !is_bare(c)).unwrap_or(t.len());
    let kw = match &t[..end] {
        "types" => "types",
        "root" => "root",
        "struct" => "struct",
        _ => return None,
    };
    let rest = t[end..].trim_start();
    // A field that happens to be called `root` or `struct`.
    if rest.starts_with([':', '?']) {
        return None;
    }
    Some((|| {
        let mut name = None;
        let mut rest = rest;
        if kw == "struct" {
            let end = rest.find(|c: char| !is_bare(c)).unwrap_or(rest.len());
            if end == 0 {
                return Err("struct needs a name".into());
            }
            name = Some(rest[..end].to_string());
            rest = rest[end..].trim_start();
        }
        let title = if rest.is_empty() {
            None
        } else {
            let (s, used) = take_string(rest)?;
            if !rest[used..].trim().is_empty() {
                return Err(format!("unexpected '{}'", rest[used..].trim()));
            }
            Some(s)
        };
        if kw == "types" && title.is_some() {
            return Err("`types` takes no title".into());
        }
        Ok((kw, name, title))
    })())
}

/// `Name = type expression ["hint"]`
fn parse_type_def(t: &str, line: usize) -> Result<TypeDef, String> {
    let (name, rest) = t.split_once('=').ok_or_else(|| format!("expected `Name = type`: {t}"))?;
    let name = name.trim();
    if name.is_empty() || !name.chars().all(is_bare) {
        return Err(format!("bad type name '{name}'"));
    }
    let toks = tokenize(rest)?;
    let (src, mut i) = type_expr(&toks)?;
    let mut hint = None;
    if let Some(Token { kind: Tok::Str(s), .. }) = toks.get(i) {
        hint = Some(s.clone());
        i += 1;
    }
    if let Some(tok) = toks.get(i) {
        return Err(format!("unexpected '{}'", tok.raw));
    }
    Ok(TypeDef { name: name.to_string(), src, hint, doc: Vec::new(), line })
}

/// `name ["?"] ":" type ["=" default] {readonly | when …} ["hint"]`
fn parse_field(t: &str, line: usize) -> Result<RawField, String> {
    let (name, mut rest) = if t.starts_with('"') {
        let (n, used) = take_string(t)?;
        (n, &t[used..])
    } else {
        let end = t.find(|c: char| !is_bare(c)).unwrap_or(t.len());
        if end == 0 {
            return Err(format!("expected a field: {t}"));
        }
        (t[..end].to_string(), &t[end..])
    };
    rest = rest.trim_start();
    let optional = rest.starts_with('?');
    if optional {
        rest = rest[1..].trim_start();
    }
    let rest = rest.strip_prefix(':').ok_or_else(|| format!("expected `{name} : type`"))?;
    let toks = tokenize(rest)?;
    let (ty_src, mut i) = type_expr(&toks)?;
    let mut f = RawField {
        name,
        optional,
        ty_src,
        default: None,
        readonly: false,
        when: None,
        hint: None,
        doc: Vec::new(),
        line,
    };
    if toks.get(i).is_some_and(|t| t.kind == Tok::Eq) {
        let v = toks.get(i + 1).ok_or("missing default value after `=`")?;
        if matches!(v.kind, Tok::Eq | Tok::Ne | Tok::Group) {
            return Err(format!("bad default value '{}'", v.raw));
        }
        f.default = Some(v.raw.to_string());
        i += 2;
    }
    while let Some(tok) = toks.get(i) {
        i += 1;
        match &tok.kind {
            Tok::Word if tok.raw == "readonly" => f.readonly = true,
            Tok::Word if tok.raw == "when" => {
                let field = match toks.get(i) {
                    Some(Token { kind: Tok::Word, raw }) if raw.chars().all(is_bare) => raw.to_string(),
                    _ => return Err("`when` needs a field name".into()),
                };
                i += 1;
                let test = match toks.get(i).map(|t| (&t.kind, t.raw)) {
                    Some((Tok::Eq, _)) => Test::In(vec![cond_value(toks.get(i + 1))?]),
                    Some((Tok::Ne, _)) => Test::NotIn(vec![cond_value(toks.get(i + 1))?]),
                    Some((Tok::Word, "in")) => match toks.get(i + 1) {
                        Some(Token { kind: Tok::Group, raw }) => Test::In(
                            split_top_level(&raw[1..raw.len() - 1])
                                .iter()
                                .map(|v| if v.starts_with('"') { take_string(v).map(|s| s.0) } else { Ok(v.clone()) })
                                .collect::<Result<_, _>>()?,
                        ),
                        _ => return Err("`in` needs a list: in (a, b)".into()),
                    },
                    _ => Test::Set,
                };
                if test != Test::Set {
                    i += 2;
                }
                f.when = Some(Condition { field, test });
            }
            Tok::Str(s) if i == toks.len() => f.hint = Some(s.clone()),
            _ => return Err(format!("unexpected '{}'", tok.raw)),
        }
    }
    Ok(f)
}

fn cond_value(tok: Option<&Token>) -> Result<String, String> {
    match tok {
        Some(Token { kind: Tok::Str(s), .. }) => Ok(s.clone()),
        Some(Token { kind: Tok::Word | Tok::Lit, raw }) => Ok(raw.to_string()),
        _ => Err("`when` needs a value".into()),
    }
}

/// Head and modifiers of a type expression; returns the expression and the tokens used.
fn type_expr(toks: &[Token]) -> Result<(String, usize), String> {
    let head = match toks.first() {
        Some(t @ Token { kind: Tok::Word, .. }) => t.raw,
        _ => return Err("missing type".into()),
    };
    let mut parts = vec![head];
    let mut i = 1;
    while let Some(t) = toks.get(i) {
        if t.kind != Tok::Word {
            break;
        }
        let arg = match t.raw {
            "unit" => Some(matches!(toks.get(i + 1), Some(Token { kind: Tok::Word | Tok::Str(_), .. }))),
            "check" | "range" | "length" => Some(matches!(toks.get(i + 1), Some(Token { kind: Tok::Word, .. }))),
            "pattern" => Some(matches!(toks.get(i + 1), Some(Token { kind: Tok::Str(_), .. }))),
            "special" => Some(matches!(toks.get(i + 1), Some(Token { kind: Tok::Group, .. }))),
            "secret" | "multiline" => None,
            w if w.starts_with("special(") => None,
            _ => break,
        };
        parts.push(t.raw);
        i += 1;
        match arg {
            Some(true) => {
                parts.push(toks[i].raw);
                i += 1;
            }
            Some(false) => return Err(format!("`{}` needs an argument", t.raw)),
            None => {}
        }
    }
    Ok((parts.join(" "), i))
}

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Word,
    Str(String),
    /// `(…)` standing alone.
    Group,
    /// `[…]` or `{…}`, a TOML literal.
    Lit,
    Eq,
    Ne,
}

#[derive(Debug, Clone)]
struct Token<'a> {
    kind: Tok,
    raw: &'a str,
}

fn tokenize(s: &str) -> Result<Vec<Token<'_>>, String> {
    let mut out = Vec::new();
    let mut pos = 0;
    loop {
        pos += s[pos..].len() - s[pos..].trim_start().len();
        let rest = &s[pos..];
        let Some(c) = rest.chars().next() else { break };
        let (kind, len) = match c {
            '"' => {
                let (text, used) = take_string(rest)?;
                (Tok::Str(text), used)
            }
            '[' | '{' => (Tok::Lit, closing(rest).ok_or_else(|| format!("missing closing bracket: {rest}"))? + 1),
            '(' => (Tok::Group, matching_paren(rest).ok_or("missing ')'")? + 1),
            '=' => (Tok::Eq, 1),
            '!' if rest.starts_with("!=") => (Tok::Ne, 2),
            _ => (Tok::Word, word_len(rest)),
        };
        out.push(Token { kind, raw: &rest[..len] });
        pos += len;
    }
    Ok(out)
}

/// A word runs to whitespace, `=` or `!=`, but includes `(…)` groups with spaces in them.
fn word_len(s: &str) -> usize {
    let mut depth = 0;
    let mut in_str = false;
    let mut prev = ' ';
    for (i, c) in s.char_indices() {
        if in_str {
            in_str = !(c == '"' && prev != '\\');
        } else {
            match c {
                '"' if depth > 0 => in_str = true,
                '(' => depth += 1,
                ')' => depth -= 1,
                c if depth == 0 && (c.is_whitespace() || c == '=' || (c == '!' && s[i..].starts_with("!="))) => {
                    return i.max(1);
                }
                _ => {}
            }
        }
        prev = c;
    }
    s.len()
}

/// Index of the bracket closing the `[` or `{` at the start, skipping strings.
fn closing(s: &str) -> Option<usize> {
    let mut depth = 0;
    let mut in_str: Option<char> = None;
    let mut prev = ' ';
    for (i, c) in s.char_indices() {
        if let Some(q) = in_str {
            if c == q && !(q == '"' && prev == '\\') {
                in_str = None;
            }
        } else {
            match c {
                '"' | '\'' => in_str = Some(c),
                '[' | '{' => depth += 1,
                ']' | '}' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(i);
                    }
                }
                _ => {}
            }
        }
        prev = c;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn schema(src: &str) -> (Option<Schema>, Vec<SchemaError>) {
        let lines: Vec<(usize, &str)> = src.lines().enumerate().map(|(i, l)| (i + 1, l)).collect();
        Schema::parse(&lines)
    }

    #[test]
    fn fields_and_modifiers() {
        let (s, errs) = schema(
            r#"hardconf 1
types
  Port = int range 1..65535   "a port"
  TlsMode = enum(verify, ca, pin)
root "Top"
  /// Overview.
  servers : map(Server, ref(names)) = {}
  names : map(text) = {}
struct Server "Server"
  smb_port : Port = 445
  tls : TlsMode = "verify" "check"
    /// "verify": public CA.
    ///   indented
  tls_name? : text when tls in (verify, ca)
  tls_ca? : text multiline when tls = ca "PEM"
  cc? : text special("": "off") readonly when tls != "pin"
  "odd key" : list(text) length ..3 = []
"#,
        );
        assert!(errs.is_empty(), "{errs:?}");
        let s = s.unwrap();
        assert_eq!(s.root.title.as_deref(), Some("Top"));
        assert_eq!(s.root.doc, ["Overview."]);
        let srv = s.struct_def("Server").unwrap();
        let port = srv.field("smb_port").unwrap();
        assert!(matches!(&port.ty, FieldType::Scalar { name: Some(n), ty } if n == "Port" && ty.range.is_some()));
        assert_eq!(s.hint(port), Some("a port"));
        let tls = srv.field("tls").unwrap();
        assert_eq!(tls.hint.as_deref(), Some("check"));
        assert_eq!(tls.doc, ["\"verify\": public CA.", "  indented"]);
        assert_eq!(tls.default.as_ref().unwrap().as_str(), Some("verify"));
        let name = srv.field("tls_name").unwrap();
        assert_eq!(name.when.as_ref().unwrap().test, Test::In(vec!["verify".into(), "ca".into()]));
        let ca = srv.field("tls_ca").unwrap();
        assert!(matches!(&ca.ty, FieldType::Scalar { ty, .. } if ty.multiline));
        assert_eq!(ca.hint.as_deref(), Some("PEM"));
        let cc = srv.field("cc").unwrap();
        assert!(cc.readonly && cc.optional);
        assert_eq!(cc.when.as_ref().unwrap().test, Test::NotIn(vec!["pin".into()]));
        assert!(matches!(&cc.ty, FieldType::Scalar { ty, .. } if ty.special_meaning("") == Some("off")));
        assert!(matches!(&srv.field("odd key").unwrap().ty, FieldType::List { length: Some(_), .. }));
        let servers = s.root.field("servers").unwrap();
        let FieldType::Map { key, .. } = &servers.ty else { panic!() };
        assert_eq!(s.key_label(key), "names");
    }

    #[test]
    fn reports_bad_lines() {
        let (s, errs) = schema(
            "hardconf 1\ntypes\n  A = int range x..\n  B = Nope\nroot\n  a : Missing\n  b : int = \"x\"\n  c : map(int, ref(a))\n  d? : int when nope\n  e : text when\nstray\n",
        );
        assert!(s.is_some());
        let msgs: Vec<String> = errs.iter().map(|e| e.to_string()).collect();
        assert_eq!(msgs.len(), 8, "{msgs:#?}");
        assert!(msgs[0].starts_with("line 3:"));
        assert!(msgs.iter().any(|m| m.contains("unknown type 'Nope'")));
        assert!(msgs.iter().any(|m| m.contains("default value")));
        assert!(msgs.iter().any(|m| m.contains("ref(a) does not lead to a map")));
        assert!(msgs.iter().any(|m| m.contains("no field nope")));
        assert!(msgs.iter().any(|m| m.contains("`when` needs a field name")));
    }

    #[test]
    fn needs_header_and_root() {
        assert!(schema("hardconf 2\nroot\n").0.is_none());
        assert!(schema("hardconf 1\ntypes\n").0.is_none());
        assert!(schema("root\n").0.is_none());
    }
}
