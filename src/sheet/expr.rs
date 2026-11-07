//! Calculation-sheet lines: syntax tree and recursive-descent parser.
//!
//! ```text
//! line    := heading | mode | statement? comment?
//! heading := "#"+ " " text
//! mode    := "#result" | "#formula" | "#full"
//! comment := "'" text
//! stmt    := name "=" expr convert?           assignment
//!          | expr rel expr                    check: ≤ ≥ < > (also <= >=)
//!          | expr convert?                    show a value
//! convert := ("→" | "->") unit                σ = N/A → N/mm²
//! expr    := mul (("+" | "-") mul)*
//! mul     := unary (("·" | "*" | "/") unary)*
//! unary   := "-" unary | power
//! power   := primary ("^" unary)?
//! primary := number unit? | name | name "(" expr (";" expr)* ")"
//!          | cell | "(" expr ")"
//! cell    := table "!" ref | name "[" … "]"   read from the grid: Loads!C12
//! ```
//!
//! Numbers use a decimal comma (a point works too), arguments are separated
//! by `;`. A unit follows a number directly or after a space: `84 kN/m`,
//! `30°`. ASCII input is accepted: `gamma_M` → γ_M, `*` → ·, `<=` → ≤,
//! `->` → →.

use crate::quantity::{is_unit_symbol, parse_unit};
use crate::units::Unit;

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    /// A number as written (`0,60`) with its value and unit.
    Num { value: f64, text: String, unit: Option<Unit> },
    Var(String),
    /// A grid reference, kept as written: `Loads!C12`, `Loads[Wind, q]`.
    Cell(String),
    Neg(Box<Expr>),
    Bin(Op, Box<Expr>, Box<Expr>),
    Call(String, Vec<Expr>),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Op {
    Add,
    Sub,
    Mul,
    Div,
    Pow,
}

impl Op {
    fn prec(self) -> u8 {
        match self {
            Op::Add | Op::Sub => 1,
            Op::Mul | Op::Div => 2,
            Op::Pow => 3,
        }
    }
}

/// Relation of a check line.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Rel {
    Le,
    Ge,
    Lt,
    Gt,
}

impl Rel {
    pub fn symbol(self, ascii: bool) -> &'static str {
        match (self, ascii) {
            (Rel::Le, false) => "≤",
            (Rel::Ge, false) => "≥",
            (Rel::Le, true) => "<=",
            (Rel::Ge, true) => ">=",
            (Rel::Lt, _) => "<",
            (Rel::Gt, _) => ">",
        }
    }

    /// Whether a lower left side is the safe one (≤, <).
    pub fn upper_bound(self) -> bool {
        matches!(self, Rel::Le | Rel::Lt)
    }
}

/// How much of the calculation a line shows.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum Mode {
    /// `q_d = 1,24 kN/m²`
    Result,
    /// `q_d = 1,35·g_k + 1,5·q_k = 1,24 kN/m²`
    Formula,
    /// `q_d = 1,35·g_k + 1,5·q_k = 1,35·0,25 + 1,5·0,60 = 1,24 kN/m²`
    #[default]
    Full,
}

impl Mode {
    pub fn name(self) -> &'static str {
        match self {
            Mode::Result => "result",
            Mode::Formula => "formula",
            Mode::Full => "full",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    Assign { name: String, expr: Expr, to: Option<Unit> },
    Check { lhs: Expr, rel: Rel, rhs: Expr },
    Show { expr: Expr, to: Option<Unit> },
}

#[derive(Debug, Clone, PartialEq)]
pub enum Line {
    Blank,
    Heading { level: usize, text: String },
    /// `#result`, `#formula`, `#full`: the mode of the following lines.
    Mode(Mode),
    /// A line with a statement, a comment or both. `mode` overrides the
    /// block's mode for this line (suffix `#result` …).
    Stmt { stmt: Option<Stmt>, comment: Option<String>, mode: Option<Mode> },
    Error { msg: String },
}

/// Greek letters typed in ASCII.
const GREEK: &[(&str, &str)] = &[
    ("alpha", "α"),
    ("beta", "β"),
    ("gamma", "γ"),
    ("delta", "δ"),
    ("epsilon", "ε"),
    ("zeta", "ζ"),
    ("eta", "η"),
    ("theta", "θ"),
    ("kappa", "κ"),
    ("lambda", "λ"),
    ("mu", "μ"),
    ("nu", "ν"),
    ("xi", "ξ"),
    ("pi", "π"),
    ("rho", "ρ"),
    ("sigma", "σ"),
    ("tau", "τ"),
    ("phi", "φ"),
    ("chi", "χ"),
    ("psi", "ψ"),
    ("omega", "ω"),
    ("Gamma", "Γ"),
    ("Delta", "Δ"),
    ("Theta", "Θ"),
    ("Lambda", "Λ"),
    ("Sigma", "Σ"),
    ("Phi", "Φ"),
    ("Psi", "Ψ"),
    ("Omega", "Ω"),
];

/// `gamma_M` → `γ_M`: a Greek name before the first `_` (or alone).
pub fn greek(name: &str) -> String {
    let (head, tail) = name.split_once('_').map(|(h, t)| (h, Some(t))).unwrap_or((name, None));
    match GREEK.iter().find(|(a, _)| *a == head) {
        Some((_, g)) => match tail {
            Some(t) => format!("{g}_{t}"),
            None => g.to_string(),
        },
        None => name.to_string(),
    }
}

/// `γ_M` → `gamma_M`, for ASCII output.
pub fn ascii_name(name: &str) -> String {
    let (head, tail) = name.split_once('_').map(|(h, t)| (h, Some(t))).unwrap_or((name, None));
    match GREEK.iter().find(|(_, g)| *g == head) {
        Some((a, _)) => match tail {
            Some(t) => format!("{a}_{t}"),
            None => a.to_string(),
        },
        None => name.to_string(),
    }
}

/// A line for the standard PDF fonts (Latin-1): Greek letters by name,
/// `≤` → `<=`, `✓` → `ok` …; `²`, `³`, `·`, `°` stay.
pub fn latin1(line: &str) -> String {
    let mut out = String::new();
    for c in line.chars() {
        let s = c.to_string();
        match GREEK.iter().find(|(_, g)| *g == s) {
            Some((a, _)) => out.push_str(a),
            None => match c {
                '≤' => out.push_str("<="),
                '≥' => out.push_str(">="),
                '→' => out.push_str("->"),
                '✓' => out.push_str("ok"),
                '✗' => out.push_str("NOT OK"),
                '⚠' => out.push('!'),
                '⋅' => out.push('·'),
                '⁻' => out.push('-'),
                c => out.push(c),
            },
        }
    }
    out
}

/// Parses one line of a ```hardcalc block.
pub fn parse_line(src: &str) -> Line {
    let t = src.trim();
    if t.is_empty() {
        return Line::Blank;
    }
    if let Some(m) = mode_directive(t) {
        return Line::Mode(m);
    }
    let hashes = t.chars().take_while(|c| *c == '#').count();
    if hashes > 0 && t[hashes..].starts_with(' ') {
        return Line::Heading { level: hashes, text: t[hashes..].trim().to_string() };
    }
    let (code, comment) = split_comment(t);
    let (code, mode) = match code.rfind('#') {
        Some(i) if mode_directive(code[i..].trim()).is_some() => (&code[..i], mode_directive(code[i..].trim())),
        _ => (code, None),
    };
    let code = code.trim();
    if code.is_empty() {
        return Line::Stmt { stmt: None, comment, mode };
    }
    match Parser::new(code).statement() {
        Ok(stmt) => Line::Stmt { stmt: Some(stmt), comment, mode },
        Err(msg) => Line::Error { msg },
    }
}

fn mode_directive(t: &str) -> Option<Mode> {
    match t {
        "#result" => Some(Mode::Result),
        "#formula" => Some(Mode::Formula),
        "#full" => Some(Mode::Full),
        _ => None,
    }
}

/// Splits at the first `'` outside double quotes.
fn split_comment(t: &str) -> (&str, Option<String>) {
    let mut quoted = false;
    for (i, ch) in t.char_indices() {
        match ch {
            '"' => quoted = !quoted,
            '\'' if !quoted => return (&t[..i], Some(t[i + 1..].trim().to_string())),
            _ => {}
        }
    }
    (t, None)
}

struct Parser {
    chars: Vec<char>,
    pos: usize,
}

fn is_name_start(c: char) -> bool {
    c.is_alphabetic()
}

fn is_name_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn is_unit_char(c: char) -> bool {
    c.is_alphabetic() || matches!(c, '%' | '‰' | '°' | 'µ')
}

fn is_sup(c: char) -> bool {
    matches!(c, '⁰' | '¹' | '²' | '³' | '⁴' | '⁵' | '⁶' | '⁷' | '⁸' | '⁹' | '⁻')
}

impl Parser {
    fn new(src: &str) -> Parser {
        Parser { chars: src.chars().collect(), pos: 0 }
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn peek_at(&self, n: usize) -> Option<char> {
        self.chars.get(self.pos + n).copied()
    }

    fn skip_ws(&mut self) {
        while self.peek().is_some_and(char::is_whitespace) {
            self.pos += 1;
        }
    }

    fn eat(&mut self, s: &str) -> bool {
        self.skip_ws();
        let n = s.chars().count();
        if self.chars[self.pos..].iter().take(n).copied().eq(s.chars()) {
            self.pos += n;
            true
        } else {
            false
        }
    }

    fn rest(&self) -> String {
        self.chars[self.pos..].iter().collect()
    }

    fn at_end(&mut self) -> bool {
        self.skip_ws();
        self.pos >= self.chars.len()
    }

    fn statement(&mut self) -> Result<Stmt, String> {
        // name = expr
        let start = self.pos;
        if let Some(name) = self.name() {
            self.skip_ws();
            if self.peek() == Some('=') && !matches!(self.peek_at(1), Some('=' | '>')) {
                self.pos += 1;
                let expr = self.expr()?;
                let to = self.convert()?;
                self.finish()?;
                return Ok(Stmt::Assign { name: greek(&name), expr, to });
            }
        }
        self.pos = start;
        let lhs = self.expr()?;
        if let Some(rel) = self.rel() {
            let rhs = self.expr()?;
            self.finish()?;
            return Ok(Stmt::Check { lhs, rel, rhs });
        }
        let to = self.convert()?;
        self.finish()?;
        Ok(Stmt::Show { expr: lhs, to })
    }

    fn finish(&mut self) -> Result<(), String> {
        if self.at_end() { Ok(()) } else { Err(format!("unexpected '{}'", self.rest())) }
    }

    fn rel(&mut self) -> Option<Rel> {
        for (s, r) in [("<=", Rel::Le), (">=", Rel::Ge), ("≤", Rel::Le), ("≥", Rel::Ge), ("<", Rel::Lt), (">", Rel::Gt)] {
            if self.eat(s) {
                return Some(r);
            }
        }
        None
    }

    fn convert(&mut self) -> Result<Option<Unit>, String> {
        if !(self.eat("→") || self.eat("->")) {
            return Ok(None);
        }
        let text = self.rest();
        self.pos = self.chars.len();
        parse_unit(&text).map(Some).ok_or_else(|| format!("not a unit: {}", text.trim()))
    }

    fn name(&mut self) -> Option<String> {
        self.skip_ws();
        if !self.peek().is_some_and(is_name_start) {
            return None;
        }
        let start = self.pos;
        while self.peek().is_some_and(is_name_char) {
            self.pos += 1;
        }
        Some(self.chars[start..self.pos].iter().collect())
    }

    fn expr(&mut self) -> Result<Expr, String> {
        let mut lhs = self.mul()?;
        loop {
            let op = if self.eat("+") {
                Op::Add
            } else if self.peek_op_minus() {
                Op::Sub
            } else {
                return Ok(lhs);
            };
            let rhs = self.mul()?;
            lhs = Expr::Bin(op, Box::new(lhs), Box::new(rhs));
        }
    }

    /// A `-` that is subtraction, not the `->` of a conversion.
    fn peek_op_minus(&mut self) -> bool {
        self.skip_ws();
        if self.peek() == Some('-') && self.peek_at(1) != Some('>') {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn mul(&mut self) -> Result<Expr, String> {
        let mut lhs = self.unary()?;
        loop {
            let op = if self.eat("·") || self.eat("*") || self.eat("⋅") {
                Op::Mul
            } else if self.eat("/") {
                Op::Div
            } else {
                return Ok(lhs);
            };
            let rhs = self.unary()?;
            lhs = Expr::Bin(op, Box::new(lhs), Box::new(rhs));
        }
    }

    fn unary(&mut self) -> Result<Expr, String> {
        if self.peek_op_minus() {
            return Ok(Expr::Neg(Box::new(self.unary()?)));
        }
        let base = self.primary()?;
        if self.eat("^") {
            let exp = self.unary()?;
            return Ok(Expr::Bin(Op::Pow, Box::new(base), Box::new(exp)));
        }
        Ok(base)
    }

    fn primary(&mut self) -> Result<Expr, String> {
        self.skip_ws();
        let Some(c) = self.peek() else { return Err("expression expected".into()) };
        if c.is_ascii_digit() {
            return self.number();
        }
        if c == '(' {
            self.pos += 1;
            let e = self.expr()?;
            if !self.eat(")") {
                return Err("')' expected".into());
            }
            return Ok(e);
        }
        if c == '"' {
            // "My sheet"!C12
            let start = self.pos;
            self.pos += 1;
            while self.peek().is_some_and(|c| c != '"') {
                self.pos += 1;
            }
            self.pos += 1;
            if self.peek() != Some('!') {
                return Err("text is not a value".into());
            }
            return Ok(self.cell_ref(start));
        }
        let start = self.pos;
        let Some(name) = self.name() else { return Err(format!("unexpected '{}'", self.rest())) };
        match self.peek() {
            Some('!') => Ok(self.cell_ref(start)),
            Some('[') => {
                let mut depth = 0;
                while let Some(c) = self.peek() {
                    self.pos += 1;
                    match c {
                        '[' => depth += 1,
                        ']' => {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        _ => {}
                    }
                }
                if depth != 0 {
                    return Err("']' expected".into());
                }
                Ok(Expr::Cell(self.chars[start..self.pos].iter().collect()))
            }
            Some('(') => {
                self.pos += 1;
                let mut args = Vec::new();
                if !self.eat(")") {
                    loop {
                        args.push(self.expr()?);
                        if self.eat(")") {
                            break;
                        }
                        if !self.eat(";") {
                            return Err("';' or ')' expected".into());
                        }
                    }
                }
                Ok(Expr::Call(name.to_lowercase(), args))
            }
            _ => Ok(Expr::Var(greek(&name))),
        }
    }

    /// After `Table!` (cursor on `!`): the reference up to the next operator.
    fn cell_ref(&mut self, start: usize) -> Expr {
        self.pos += 1;
        while self.peek().is_some_and(|c| is_name_char(c) || matches!(c, '$' | ':' | '.')) {
            self.pos += 1;
        }
        Expr::Cell(self.chars[start..self.pos].iter().collect())
    }

    fn number(&mut self) -> Result<Expr, String> {
        let start = self.pos;
        while self.peek().is_some_and(|c| c.is_ascii_digit()) {
            self.pos += 1;
        }
        if matches!(self.peek(), Some(',' | '.')) && self.peek_at(1).is_some_and(|c| c.is_ascii_digit()) {
            self.pos += 1;
            while self.peek().is_some_and(|c| c.is_ascii_digit()) {
                self.pos += 1;
            }
        }
        if matches!(self.peek(), Some('e' | 'E'))
            && (self.peek_at(1).is_some_and(|c| c.is_ascii_digit())
                || (matches!(self.peek_at(1), Some('-' | '+')) && self.peek_at(2).is_some_and(|c| c.is_ascii_digit())))
        {
            self.pos += 2;
            while self.peek().is_some_and(|c| c.is_ascii_digit()) {
                self.pos += 1;
            }
        }
        let text: String = self.chars[start..self.pos].iter().collect();
        let value: f64 = text.replace(',', ".").parse().map_err(|_| format!("not a number: {text}"))?;
        let unit = self.unit();
        Ok(Expr::Num { value, text, unit })
    }

    /// A unit right after a number: known symbols joined by `/`, `·`, `*`
    /// without spaces, each with an exponent (`^2`, `²`).
    fn unit(&mut self) -> Option<Unit> {
        let save = self.pos;
        while self.peek() == Some(' ') {
            self.pos += 1;
        }
        let mut text = String::new();
        loop {
            let sym_start = self.pos;
            while self.peek().is_some_and(is_unit_char) {
                self.pos += 1;
            }
            let sym: String = self.chars[sym_start..self.pos].iter().collect();
            if !is_unit_symbol(&sym) {
                self.pos = sym_start;
                break;
            }
            text.push_str(&sym);
            // Exponent.
            if self.peek() == Some('^') {
                let e_start = self.pos;
                self.pos += 1;
                if self.peek() == Some('-') {
                    self.pos += 1;
                }
                while self.peek().is_some_and(|c| c.is_ascii_digit()) {
                    self.pos += 1;
                }
                text.extend(&self.chars[e_start..self.pos]);
            }
            while self.peek().is_some_and(is_sup) {
                text.push(self.peek().unwrap());
                self.pos += 1;
            }
            match self.peek() {
                Some(op @ ('/' | '·' | '*' | '⋅')) if self.peek_at(1).is_some_and(is_unit_char) => {
                    // Only when another unit symbol follows.
                    let mut end = self.pos + 1;
                    while self.chars.get(end).is_some_and(|c| is_unit_char(*c)) {
                        end += 1;
                    }
                    let next: String = self.chars[self.pos + 1..end].iter().collect();
                    if !is_unit_symbol(&next) {
                        break;
                    }
                    text.push(op);
                    self.pos += 1;
                }
                _ => break,
            }
        }
        // Drop a trailing operator that was not followed by a symbol.
        let unit = (!text.is_empty()).then(|| parse_unit(&text)).flatten();
        if unit.is_none() {
            self.pos = save;
        }
        unit
    }
}

/// A calculation line with its numbers written with a decimal point, as files
/// hold them: `q = 1,35·g_k` → `q = 1.35·g_k`. Arguments are separated by
/// `;`, so a comma between digits is always a decimal comma; comments (`'`)
/// and quoted names stay as they are.
pub fn point_decimals(line: &str) -> String {
    let chars: Vec<char> = line.chars().collect();
    let mut out = String::with_capacity(line.len());
    let mut quoted = false;
    for (i, &c) in chars.iter().enumerate() {
        if c == '\'' && !quoted {
            out.extend(&chars[i..]);
            return out;
        }
        if c == '"' {
            quoted = !quoted;
        }
        let between_digits = i > 0 && chars[i - 1].is_ascii_digit() && chars.get(i + 1).is_some_and(|d| d.is_ascii_digit());
        // Not in a name like `f_1,2` – the number must start the token.
        let starts_number = chars[..i].iter().rev().take_while(|d| d.is_ascii_digit()).count() == i
            || chars[..i].iter().rev().find(|d| !d.is_ascii_digit()).is_some_and(|d| !(d.is_alphanumeric() || *d == '_'));
        out.push(if c == ',' && !quoted && between_digits && starts_number { '.' } else { c });
    }
    out
}

/// Options for writing an expression as one line of text.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextStyle {
    /// Plain ASCII: `*`, `<=`, `gamma_M`, `m^2`.
    pub ascii: bool,
    /// Decimal point instead of the decimal comma (`1.35`).
    pub point: bool,
}

impl Default for TextStyle {
    /// Unicode, numbers as the display shows them.
    fn default() -> Self {
        TextStyle { ascii: false, point: !crate::locale::decimal_comma() }
    }
}

impl TextStyle {
    /// A number as written, with the chosen decimal separator.
    pub fn number(self, text: &str) -> String {
        if self.point { text.replace(',', ".") } else { text.replace('.', ",") }
    }
}

/// Writes an expression as one line; `leaf` may replace variables and grid
/// references (by their values, for the substituted step).
pub fn to_text(e: &Expr, style: TextStyle, leaf: &dyn Fn(&Expr) -> Option<String>) -> String {
    fn go(e: &Expr, style: TextStyle, leaf: &dyn Fn(&Expr) -> Option<String>, parent: u8) -> String {
        if let Some(s) = leaf(e) {
            return s;
        }
        match e {
            Expr::Num { text, unit, .. } => match unit {
                Some(u) if style.ascii => format!("{} {u}", style.number(text)),
                // 30°, but 5 kN.
                Some(u) if u.to_string() == "°" => format!("{}°", style.number(text)),
                Some(u) => format!("{} {}", style.number(text), crate::quantity::pretty(u)),
                None => style.number(text),
            },
            Expr::Var(n) if style.ascii => ascii_name(n),
            Expr::Var(n) | Expr::Cell(n) => n.clone(),
            Expr::Neg(x) => {
                let s = format!("-{}", go(x, style, leaf, 3));
                if parent >= 2 { format!("({s})") } else { s }
            }
            Expr::Bin(op, a, b) => {
                let p = op.prec();
                let sym = match op {
                    Op::Add => " + ",
                    Op::Sub => " - ",
                    Op::Mul if style.ascii => "*",
                    Op::Mul => "·",
                    Op::Div => "/",
                    Op::Pow => "^",
                };
                // Left-associative: a right operand of equal precedence needs
                // parentheses (a - (b - c)); powers are right-associative.
                let (pl, pr) = if *op == Op::Pow { (p + 1, p) } else { (p, p + 1) };
                let s = format!("{}{sym}{}", go(a, style, leaf, pl), go(b, style, leaf, pr));
                if p < parent { format!("({s})") } else { s }
            }
            Expr::Call(f, args) => {
                let parts: Vec<String> = args.iter().map(|a| go(a, style, leaf, 0)).collect();
                format!("{f}({})", parts.join("; "))
            }
        }
    }
    go(e, style, leaf, 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stmt(s: &str) -> Stmt {
        match parse_line(s) {
            Line::Stmt { stmt: Some(st), .. } => st,
            other => panic!("{s}: {other:?}"),
        }
    }

    fn text(e: &Expr) -> String {
        to_text(e, TextStyle::default(), &|_| None)
    }

    #[test]
    fn assignment_with_unit_and_comment() {
        let line = parse_line("f_tk   = 84 kN/m            ' Festigkeit");
        let Line::Stmt { stmt: Some(Stmt::Assign { name, expr, .. }), comment, .. } = line else { panic!() };
        assert_eq!(name, "f_tk");
        assert_eq!(comment.as_deref(), Some("Festigkeit"));
        assert_eq!(text(&expr), "84 kN/m");
    }

    #[test]
    fn decimals_are_written_with_a_point() {
        assert_eq!(point_decimals("q_d = 1,35·g_k + 1,5·q_k   ' 1,5 stays in comments"), "q_d = 1.35·g_k + 1.5·q_k   ' 1,5 stays in comments");
        assert_eq!(point_decimals("x = max(1,5; 2)"), "x = max(1.5; 2)");
        assert_eq!(point_decimals("y = Loads[Wind, q] + \"a 1,5\"!B2"), "y = Loads[Wind, q] + \"a 1,5\"!B2");
        assert_eq!(point_decimals("z = 4.0 kN/m"), "z = 4.0 kN/m");
    }

    #[test]
    fn decimal_comma_and_ascii_input() {
        // Shown with the decimal comma, as with a German locale.
        crate::locale::set_decimal_comma_for_this_thread(true);
        let Stmt::Assign { name, expr, .. } = stmt("gamma_M = 1,4") else { panic!() };
        assert_eq!(name, "γ_M");
        assert_eq!(expr, Expr::Num { value: 1.4, text: "1,4".into(), unit: None });
        let Stmt::Assign { expr, .. } = stmt("q_d = 1.35*g_k + 1,5·q_k") else { panic!() };
        assert_eq!(text(&expr), "1,35·g_k + 1,5·q_k");
    }

    #[test]
    fn units_after_numbers_only() {
        // Shown with the decimal comma, as with a German locale.
        crate::locale::set_decimal_comma_for_this_thread(true);
        let Stmt::Assign { expr, .. } = stmt("g_k = 0,25 kN/m²") else { panic!() };
        assert_eq!(text(&expr), "0,25 kN/m²");
        // h is a variable here, not hours.
        let Stmt::Assign { expr, .. } = stmt("A = b·h") else { panic!() };
        assert_eq!(text(&expr), "b·h");
        let Stmt::Assign { expr, .. } = stmt("x = 10 kN / 2") else { panic!() };
        assert_eq!(text(&expr), "10 kN/2");
        let Stmt::Assign { expr, .. } = stmt("w = 2 kN·m") else { panic!() };
        assert_eq!(text(&expr), "2 kN·m");
        let Stmt::Assign { expr, .. } = stmt("a = 30°") else { panic!() };
        assert_eq!(text(&expr), "30°");
    }

    #[test]
    fn checks_and_cells() {
        let Stmt::Check { lhs, rel, rhs } = stmt("n_Ed <= f_d") else { panic!() };
        assert_eq!((text(&lhs), rel, text(&rhs)), ("n_Ed".into(), Rel::Le, "f_d".into()));
        let Stmt::Assign { expr, .. } = stmt("n_Ed = Loads!C12") else { panic!() };
        assert_eq!(expr, Expr::Cell("Loads!C12".into()));
        let Stmt::Assign { expr, .. } = stmt("q = Loads[Wind, q] * 2") else { panic!() };
        assert_eq!(text(&expr), "Loads[Wind, q]·2");
        let Stmt::Assign { expr, .. } = stmt("q = \"My sheet\"!B2") else { panic!() };
        assert_eq!(expr, Expr::Cell("\"My sheet\"!B2".into()));
    }

    #[test]
    fn conversion_and_functions() {
        let Stmt::Assign { to, expr, .. } = stmt("σ = N/A -> N/mm^2") else { panic!() };
        assert_eq!(to, parse_unit("N/mm²"));
        assert_eq!(text(&expr), "N/A");
        let Stmt::Assign { expr, .. } = stmt("m = max(a; b) - 1") else { panic!() };
        assert_eq!(text(&expr), "max(a; b) - 1");
    }

    #[test]
    fn parentheses_where_needed() {
        let Stmt::Show { expr, .. } = stmt("(a + b)·c - (d - e) / (f·g)") else { panic!() };
        assert_eq!(text(&expr), "(a + b)·c - (d - e)/(f·g)");
        let Stmt::Show { expr, .. } = stmt("a^2^3") else { panic!() };
        assert_eq!(text(&expr), "a^2^3");
    }

    #[test]
    fn latin1_for_pdf() {
        assert_eq!(latin1("n = 38,2 kN/m ≤ f_d = 60,0 kN/m   η = 0,64 ✓"), "n = 38,2 kN/m <= f_d = 60,0 kN/m   eta = 0,64 ok");
        assert_eq!(latin1("γ_M = 1,4 · m²"), "gamma_M = 1,4 · m²");
    }

    #[test]
    fn headings_modes_comments() {
        assert_eq!(parse_line("## Check"), Line::Heading { level: 2, text: "Check".into() });
        assert_eq!(parse_line("#result"), Line::Mode(Mode::Result));
        assert_eq!(parse_line("' just text"), Line::Stmt { stmt: None, comment: Some("just text".into()), mode: None });
        let Line::Stmt { mode, .. } = parse_line("a = 1 #formula ' x") else { panic!() };
        assert_eq!(mode, Some(Mode::Formula));
        assert!(matches!(parse_line("a = = 1"), Line::Error { .. }));
    }
}
