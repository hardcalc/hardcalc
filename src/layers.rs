//! Interpretation of layer blocks: rule targets, the `format` layer and the
//! `validate` layer. UI-independent; the terminal UI maps [`CellStyle`] to colors.

use crate::engine::{CellId, Workbook};
use crate::model::{Align, Block, Document, Layer, Rule};
use crate::value::{parse_number, Value};
use std::cmp::Ordering;

/// A layer together with the sheet it applies to.
#[derive(Debug, Clone)]
pub struct Attached {
    pub sheet: usize,
    pub layer: Layer,
}

/// Binds every layer block to its table: the explicit `@Name` or the
/// nearest table above it. Layers without any table are dropped.
pub fn attach(doc: &Document, wb: &Workbook) -> Vec<Attached> {
    let mut out = Vec::new();
    let mut last_sheet: Option<usize> = None;
    let mut sheet_count = 0;
    for block in &doc.blocks {
        match block {
            Block::Table(_) => {
                last_sheet = Some(sheet_count);
                sheet_count += 1;
            }
            Block::Layer(l) => {
                let sheet = match &l.target {
                    Some(name) => wb.sheet_index(name),
                    None => last_sheet,
                };
                if let Some(sheet) = sheet {
                    out.push(Attached { sheet, layer: l.clone() });
                }
            }
            _ => {}
        }
    }
    out
}

/// Names of all layers in document order, without duplicates.
pub fn layer_names(doc: &Document) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for block in &doc.blocks {
        if let Block::Layer(l) = block
            && !names.contains(&l.name)
        {
            names.push(l.name.clone());
        }
    }
    names
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CmpOp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

impl CmpOp {
    fn holds(self, ord: Ordering) -> bool {
        match self {
            CmpOp::Eq => ord == Ordering::Equal,
            CmpOp::Ne => ord != Ordering::Equal,
            CmpOp::Lt => ord == Ordering::Less,
            CmpOp::Le => ord != Ordering::Greater,
            CmpOp::Gt => ord == Ordering::Greater,
            CmpOp::Ge => ord != Ordering::Less,
        }
    }

    fn symbol(self) -> &'static str {
        match self {
            CmpOp::Eq => "=",
            CmpOp::Ne => "<>",
            CmpOp::Lt => "<",
            CmpOp::Le => "<=",
            CmpOp::Gt => ">",
            CmpOp::Ge => ">=",
        }
    }
}

/// Splits `>= 0` / `Status=open` at the first comparison operator.
fn split_comparison(s: &str) -> Option<(&str, CmpOp, &str)> {
    for (sym, op) in [
        ("<=", CmpOp::Le),
        (">=", CmpOp::Ge),
        ("<>", CmpOp::Ne),
        ("!=", CmpOp::Ne),
        ("=", CmpOp::Eq),
        ("<", CmpOp::Lt),
        (">", CmpOp::Gt),
    ] {
        if let Some(i) = s.find(sym) {
            return Some((s[..i].trim(), op, s[i + sym.len()..].trim()));
        }
    }
    None
}

/// Compares a cell value against literal text: numerically when both sides
/// are numbers, otherwise by display text.
fn compare_value(v: &Value, op: CmpOp, rhs: &str) -> bool {
    let ord = match (v, parse_number(rhs)) {
        (Value::Num(n, _), Some((r, _))) => n.partial_cmp(&r),
        _ => Some(v.display(None, false).as_str().cmp(rhs)),
    };
    ord.is_some_and(|o| op.holds(o))
}

#[derive(Debug, Clone, PartialEq)]
pub enum Target {
    All,
    /// Column by name or letter.
    Column(String),
    /// 1-based data row.
    Row(usize),
    Cell(usize, usize),
    Range((usize, usize), (usize, usize)),
    RowWhere { column: String, op: CmpOp, value: String },
    /// The cells of a column that meet conditions: `Amount[> 1000]`.
    ColumnWhere { column: String, checks: Vec<Check> },
}

impl Target {
    pub fn parse(s: &str) -> Target {
        let s = s.trim();
        if s == "*" {
            return Target::All;
        }
        if let Some(cond) = s.strip_prefix("row[").and_then(|r| r.strip_suffix(']'))
            && let Some((column, op, value)) = split_comparison(cond)
        {
            return Target::RowWhere { column: column.into(), op, value: value.into() };
        }
        // `Amount[> 1000]`, `Status[in(open, due)]`: the cells of the column that meet it.
        if let Some((column, cond)) = s.strip_suffix(']').and_then(|r| r.split_once('['))
            && !column.trim().is_empty()
            && let Ok(checks) = Check::parse_all(cond)
            && !checks.is_empty()
            && !checks.contains(&Check::Unique)
        {
            return Target::ColumnWhere { column: column.trim().to_string(), checks };
        }
        if let Ok(n) = s.parse::<usize>() {
            return Target::Row(n);
        }
        if let Some((a, b)) = s.split_once(':')
            && let (Some(a), Some(b)) = (parse_a1(a.trim()), parse_a1(b.trim()))
        {
            return Target::Range(a, b);
        }
        if let Some((c, r)) = parse_a1(s) {
            return Target::Cell(r, c);
        }
        Target::Column(s.to_string())
    }

    pub fn matches(&self, wb: &mut Workbook, (sheet, row, col): CellId) -> bool {
        match self {
            Target::All => true,
            Target::Column(name) => column_index(wb, sheet, name) == Some(col),
            Target::Row(n) => *n == row + 1,
            Target::Cell(r, c) => (*r, *c) == (row, col),
            Target::Range((c1, r1), (c2, r2)) => {
                (*r1.min(r2)..=*r1.max(r2)).contains(&row) && (*c1.min(c2)..=*c1.max(c2)).contains(&col)
            }
            Target::RowWhere { column, op, value } => match column_index(wb, sheet, column) {
                Some(c) => {
                    let v = wb.value((sheet, row, c));
                    compare_value(&v, *op, value)
                }
                None => false,
            },
            Target::ColumnWhere { column, checks } => {
                column_index(wb, sheet, column) == Some(col) && {
                    let v = wb.value((sheet, row, col));
                    checks.iter().all(|check| check.holds(&v))
                }
            }
        }
    }
}

/// Why the target of a rule meets nothing in its table: a column that is
/// not there, or a row or cell beyond the table. `open`: the table grows
/// with the window (a playground, `fill`), so cells beyond it are fine.
pub fn target_problem(wb: &Workbook, sheet: usize, target: &str, open: bool) -> Option<String> {
    let s = &wb.sheets[sheet];
    let (rows, cols) = (s.cells.len(), s.columns.len());
    let beyond = |r: usize, c: usize| !open && (r >= rows || c >= cols);
    let size = format!("the table has {rows} rows and {cols} columns");
    match Target::parse(target) {
        Target::All => None,
        Target::Column(name) | Target::RowWhere { column: name, .. } | Target::ColumnWhere { column: name, .. } => {
            let letters = !name.is_empty() && name.chars().all(|c| c.is_ascii_uppercase());
            match column_index(wb, sheet, &name) {
                Some(_) => None,
                None if letters && open => None,
                None if letters => Some(format!("column {name} is beyond the table – {size}")),
                None => Some(format!("unknown column '{name}'")),
            }
        }
        Target::Row(n) => (!open && n > rows).then(|| format!("row {n} is beyond the table – {size}")),
        Target::Cell(r, c) => beyond(r, c).then(|| format!("the cell is beyond the table – {size}")),
        Target::Range((c1, r1), (c2, r2)) => (beyond(r1, c1) || beyond(r2, c2)).then(|| format!("the range reaches beyond the table – {size}")),
    }
}

/// The column of `sheet` a rule target names, if it names a column that is there.
pub fn column_of(wb: &Workbook, sheet: usize, target: &str) -> Option<usize> {
    match Target::parse(target) {
        Target::Column(name) => column_index(wb, sheet, &name),
        _ => None,
    }
}

fn column_index(wb: &Workbook, sheet: usize, name: &str) -> Option<usize> {
    let cols = &wb.sheets[sheet].columns;
    cols.iter().position(|c| c.name == name).or_else(|| {
        if !name.is_empty() && name.chars().all(|c| c.is_ascii_uppercase()) {
            let i = name.chars().fold(0, |acc, c| acc * 26 + (c as usize - 'A' as usize + 1)) - 1;
            (i < cols.len()).then_some(i)
        } else {
            None
        }
    })
}

/// `B12` → (col 1, row 11)
fn parse_a1(s: &str) -> Option<(usize, usize)> {
    let split = s.find(|c: char| c.is_ascii_digit())?;
    let letters = &s[..split];
    if letters.is_empty() || !letters.chars().all(|c| c.is_ascii_uppercase()) {
        return None;
    }
    let col = letters.chars().fold(0, |acc, c| acc * 26 + (c as usize - 'A' as usize + 1)) - 1;
    let row: usize = s[split..].parse().ok()?;
    (row >= 1).then(|| (col, row - 1))
}

// ---------------------------------------------------------------- format

#[derive(Debug, Clone, PartialEq)]
pub enum Color {
    Named(String),
    Rgb(u8, u8, u8),
}

impl Color {
    fn parse(s: &str) -> Option<Color> {
        if let Some(hex) = s.strip_prefix('#') {
            if hex.len() != 6 {
                return None;
            }
            let p = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
            return Some(Color::Rgb(p(0)?, p(2)?, p(4)?));
        }
        // The names the screen and the printer both know (`light-blue` = `lightblue`).
        const NAMES: &[&str] = &[
            "black", "red", "green", "yellow", "blue", "magenta", "cyan", "gray", "grey", "darkgray", "darkgrey", "white", "lightred",
            "lightgreen", "lightyellow", "lightblue", "lightmagenta", "lightcyan",
        ];
        let name = s.to_lowercase();
        NAMES.contains(&name.replace(['-', '_'], "").as_str()).then_some(Color::Named(name))
    }
}

/// Visual attributes from the format layer. `None` means "not set".
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CellStyle {
    pub align: Option<Align>,
    pub fg: Option<Color>,
    pub bg: Option<Color>,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    /// Show as percentage with n decimals.
    pub percent: Option<u8>,
    pub decimals: Option<u8>,
    /// Break long text into lines (`wrap`, `wrap 30`): the column width in
    /// characters, 0 for the usual maximum.
    pub wrap: Option<u16>,
    /// What a rule switches off for the rules before it: `nobold`,
    /// `noitalic`, `nounderline`, `fg none`, `bg none`. Only in parsed
    /// rules; a merged style has these false.
    pub off: Off,
}

/// Attributes a rule switches off (see [`CellStyle::off`]).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Off {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub fg: bool,
    pub bg: bool,
    pub wrap: bool,
}

/// The rule words that set the text style, as named in the format layer.
pub const STYLE_FAMILIES: [&str; 5] = ["bold", "italic", "underline", "fg", "bg"];

/// Splits a rule body into its parts: `fg red bold align right` →
/// `["fg red", "bold", "align right"]` (words with an argument stay together).
pub fn style_parts(body: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut words = body.split_whitespace().peekable();
    while let Some(w) = words.next() {
        match w {
            "align" | "fg" | "bg" => match words.next() {
                Some(arg) => out.push(format!("{w} {arg}")),
                None => out.push(w.to_string()),
            },
            "wrap" => match words.next_if(|n| n.parse::<u16>().is_ok()) {
                Some(n) => out.push(format!("wrap {n}")),
                None => out.push(w.to_string()),
            },
            _ => out.push(w.to_string()),
        }
    }
    out
}

/// The family of a rule part: `nobold` → `bold`, `fg red` → `fg`, `align left` → `align`.
pub fn part_family(part: &str) -> &str {
    let first = part.split_whitespace().next().unwrap_or("");
    first.strip_prefix("no").filter(|f| STYLE_FAMILIES.contains(f) || *f == "wrap").unwrap_or(first)
}

impl CellStyle {
    /// Parses `fg yellow bold percent(1)`. Unknown words are reported.
    pub fn parse(body: &str) -> (CellStyle, Vec<String>) {
        let mut st = CellStyle::default();
        let mut errors = Vec::new();
        let mut words = body.split_whitespace().peekable();
        let arg = |w: &str, name: &str| -> Option<Option<u8>> {
            let rest = w.strip_prefix(name)?;
            if rest.is_empty() {
                return Some(None);
            }
            rest.strip_prefix('(')?.strip_suffix(')')?.trim().parse().ok().map(Some)
        };
        while let Some(w) = words.next() {
            match w {
                "align" => match words.next() {
                    Some("left") => st.align = Some(Align::Left),
                    Some("right") => st.align = Some(Align::Right),
                    Some("center") => st.align = Some(Align::Center),
                    other => errors.push(format!("align: unknown value {other:?}")),
                },
                "fg" | "bg" => match words.next() {
                    Some("none") if w == "fg" => st.off.fg = true,
                    Some("none") => st.off.bg = true,
                    Some(c) => match Color::parse(c) {
                        Some(c) if w == "fg" => st.fg = Some(c),
                        Some(c) => st.bg = Some(c),
                        None => errors.push(format!("{w}: bad color '{c}'")),
                    },
                    None => errors.push(format!("{w}: missing color")),
                },
                "wrap" => {
                    let width = words.next_if(|n| n.parse::<u16>().is_ok()).and_then(|n| n.parse::<u16>().ok());
                    match width {
                        Some(0) => errors.push("wrap: the width must be at least 1".to_string()),
                        w => st.wrap = Some(w.unwrap_or(0)),
                    }
                }
                "nowrap" => st.off.wrap = true,
                "bold" => st.bold = true,
                "italic" => st.italic = true,
                "underline" => st.underline = true,
                "nobold" => st.off.bold = true,
                "noitalic" => st.off.italic = true,
                "nounderline" => st.off.underline = true,
                _ => {
                    if let Some(n) = arg(w, "percent") {
                        st.percent = Some(n.unwrap_or(0));
                    } else if let Some(Some(n)) = arg(w, "decimals") {
                        st.decimals = Some(n);
                    } else {
                        errors.push(format!("unknown format '{w}'"));
                    }
                }
            }
        }
        (st, errors)
    }

    /// Overlays `other` on top of `self` (later rules win).
    pub fn merge(&mut self, other: &CellStyle) {
        if other.align.is_some() {
            self.align = other.align;
        }
        if other.off.fg {
            self.fg = None;
        }
        if other.off.bg {
            self.bg = None;
        }
        if other.fg.is_some() {
            self.fg = other.fg.clone();
        }
        if other.bg.is_some() {
            self.bg = other.bg.clone();
        }
        self.bold = (self.bold && !other.off.bold) || other.bold;
        self.italic = (self.italic && !other.off.italic) || other.italic;
        self.underline = (self.underline && !other.off.underline) || other.underline;
        if other.off.wrap {
            self.wrap = None;
        }
        if other.wrap.is_some() {
            self.wrap = other.wrap;
        }
        if other.percent.is_some() {
            self.percent = other.percent;
        }
        if other.decimals.is_some() {
            self.decimals = other.decimals;
        }
    }

    /// Renders a value according to percent/decimals; `None` if the style
    /// does not change the default display.
    pub fn display(&self, v: &Value, comma: bool) -> Option<String> {
        let Value::Num(n, u) = v else { return None };
        if let Some(d) = self.percent {
            return Some(format!("{} %", Value::Num(n * 100.0, u.clone()).display(Some(d), comma)));
        }
        self.decimals.map(|d| v.display(Some(d), comma))
    }
}

/// Precompiled format rules of all `format` layers.
#[derive(Debug, Default)]
pub struct Formats {
    rules: Vec<(usize, Target, CellStyle)>,
    pub errors: Vec<LayerError>,
}

impl Formats {
    pub fn new(attached: &[Attached], layer: &str) -> Formats {
        let mut f = Formats::default();
        for a in attached.iter().filter(|a| a.layer.name == layer) {
            for (i, rule) in a.layer.rules.iter().enumerate() {
                if let Rule::Rule { target, body } = rule {
                    let (style, errs) = CellStyle::parse(body);
                    f.errors.extend(errs.into_iter().map(|e| LayerError {
                        line: a.layer.line.0 + 1 + i,
                        sheet: a.sheet,
                        layer: layer.to_string(),
                        message: format!("{target}: {e}"),
                    }));
                    f.rules.push((a.sheet, Target::parse(target), style));
                }
            }
        }
        f
    }

    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    pub fn style(&self, wb: &mut Workbook, id: CellId) -> CellStyle {
        let mut st = CellStyle::default();
        for (sheet, target, style) in &self.rules {
            if *sheet == id.0 && target.matches(wb, id) {
                st.merge(style);
            }
        }
        st
    }
}

// ---------------------------------------------------------------- validate

#[derive(Debug, Clone, PartialEq)]
pub enum Check {
    Required,
    /// The cell is empty (`empty`).
    Empty,
    Unique,
    Compare(CmpOp, String),
    OneOf(Vec<String>),
    /// The text contains this, upper and lower case alike (`contains(rent)`).
    Contains(String),
}

/// A value at the start of `s` – a word, or `"several words"` (`""` for a
/// quote inside) – and the rest behind it. `stop`: characters that end a word.
fn take_value<'a>(s: &'a str, stop: &[char]) -> Option<(String, &'a str)> {
    let Some(quoted) = s.strip_prefix('"') else {
        let end = s.find(|c: char| c.is_whitespace() || stop.contains(&c)).unwrap_or(s.len());
        return Some((s[..end].to_string(), &s[end..]));
    };
    let mut out = String::new();
    let mut chars = quoted.char_indices();
    while let Some((i, ch)) = chars.next() {
        match ch {
            '"' if quoted[i + 1..].starts_with('"') => {
                out.push('"');
                chars.next();
            }
            '"' => return Some((out, &quoted[i + 1..])),
            _ => out.push(ch),
        }
    }
    None
}

/// The values of `a, "b, c", d)` up to the closing bracket, and the rest.
fn take_list(s: &str) -> Option<(Vec<String>, &str)> {
    let mut out = Vec::new();
    let mut rest = s;
    loop {
        rest = rest.trim_start();
        let (value, after) = if rest.starts_with('"') {
            take_value(rest, &[])?
        } else {
            let end = rest.find([',', ')'])?;
            (rest[..end].trim().to_string(), &rest[end..])
        };
        out.push(value);
        let after = after.trim_start();
        if let Some(r) = after.strip_prefix(')') {
            return Some((out, r));
        }
        rest = after.strip_prefix(',')?;
    }
}

/// Writes a value so that [`Check::parse_all`] reads it back: quoted where
/// a space, comma, bracket or quote would end it early.
pub fn quote_value(v: &str) -> String {
    if v.is_empty() || v.contains(|c: char| c.is_whitespace() || matches!(c, ',' | '(' | ')' | '"')) {
        format!("\"{}\"", v.replace('"', "\"\""))
    } else {
        v.to_string()
    }
}

impl Check {
    /// Parses `required unique >= 0 in(a, b)`.
    pub fn parse_all(body: &str) -> Result<Vec<Check>, String> {
        let mut checks = Vec::new();
        let mut rest = body.trim();
        let word = |rest: &str, w: &str| rest.strip_prefix(w).filter(|r| r.is_empty() || r.starts_with(char::is_whitespace)).map(|r| r.trim_start().to_string());
        while !rest.is_empty() {
            if let Some(r) = rest.strip_prefix("required") {
                checks.push(Check::Required);
                rest = r.trim_start();
            } else if let Some(r) = rest.strip_prefix("unique") {
                checks.push(Check::Unique);
                rest = r.trim_start();
            } else if word(rest, "empty").is_some() {
                checks.push(Check::Empty);
                rest = rest["empty".len()..].trim_start();
            } else if let Some(r) = rest.strip_prefix("in(") {
                let (values, after) = take_list(r).ok_or("in(…) missing ')'")?;
                checks.push(Check::OneOf(values));
                rest = after.trim_start();
            } else if let Some(r) = rest.strip_prefix("contains(") {
                let (values, after) = take_list(r).ok_or("contains(…) missing ')'")?;
                let [value] = values.as_slice() else { return Err("contains(…) takes one text".to_string()) };
                checks.push(Check::Contains(value.clone()));
                rest = after.trim_start();
            } else if let Some((_, op, after)) = split_comparison(rest).filter(|(lhs, ..)| lhs.is_empty()) {
                let quoted = after.starts_with('"');
                let (value, after) = take_value(after, &[]).ok_or("missing closing quote")?;
                if value.is_empty() && !quoted {
                    return Err(format!("missing value after {}", op.symbol()));
                }
                checks.push(Check::Compare(op, value));
                rest = after.trim_start();
            } else {
                return Err(format!("unknown check '{rest}'"));
            }
        }
        Ok(checks)
    }

    /// Whether a value passes – for the checks that look at one cell
    /// (`unique` looks at all and passes here).
    pub fn holds(&self, v: &Value) -> bool {
        let empty = is_empty(v);
        match self {
            Check::Required => !empty,
            Check::Empty => empty,
            Check::Unique => true,
            // An empty cell is neither smaller nor larger than anything: it
            // only differs from a value (and equals the empty text).
            Check::Compare(op, rhs) if empty => rhs.is_empty() != (*op == CmpOp::Ne) && !matches!(op, CmpOp::Lt | CmpOp::Gt),
            Check::Compare(op, rhs) => compare_value(v, *op, rhs),
            Check::OneOf(options) if empty => options.iter().any(|o| o.is_empty()),
            Check::OneOf(options) => options.iter().any(|o| compare_value(v, CmpOp::Eq, o)),
            Check::Contains(text) => v.display(None, false).to_lowercase().contains(&text.to_lowercase()),
        }
    }
}

fn is_empty(v: &Value) -> bool {
    matches!(v, Value::Null) || matches!(v, Value::Text(s) if s.trim().is_empty())
}

/// A rule that cannot be read, with where it is.
#[derive(Debug, Clone, PartialEq)]
pub struct LayerError {
    /// Line of the rule in the file (0 = unknown).
    pub line: usize,
    /// Index of the table the layer belongs to.
    pub sheet: usize,
    /// Name of the layer (`format`, `validate`, …).
    pub layer: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Violation {
    pub cell: CellId,
    pub message: String,
}

/// Runs all rules of the layer named `layer` (normally `validate`).
/// Returns violations and rule syntax errors.
pub fn validate(attached: &[Attached], wb: &mut Workbook, layer: &str) -> (Vec<Violation>, Vec<LayerError>) {
    let mut out = Vec::new();
    let mut errors = Vec::new();
    for a in attached.iter().filter(|a| a.layer.name == layer) {
        for (i, rule) in a.layer.rules.iter().enumerate() {
            let Rule::Rule { target, body } = rule else { continue };
            let checks = match Check::parse_all(body) {
                Ok(c) => c,
                Err(e) => {
                    errors.push(LayerError {
                        line: a.layer.line.0 + 1 + i,
                        sheet: a.sheet,
                        layer: layer.to_string(),
                        message: format!("{target}: {e}"),
                    });
                    continue;
                }
            };
            let t = Target::parse(target);
            let sheet = a.sheet;
            let (rows, cols) = (wb.sheets[sheet].cells.len(), wb.sheets[sheet].columns.len());
            let cells: Vec<CellId> = (0..rows)
                .flat_map(|r| (0..cols).map(move |c| (sheet, r, c)))
                .filter(|&id| t.matches(wb, id))
                .collect();
            for check in &checks {
                check_cells(wb, check, &cells, &mut out);
            }
        }
    }
    (out, errors)
}

fn check_cells(wb: &mut Workbook, check: &Check, cells: &[CellId], out: &mut Vec<Violation>) {
    let mut seen: Vec<(String, CellId)> = Vec::new();
    for &id in cells {
        let v = wb.value(id);
        let empty = is_empty(&v);
        let message = match check {
            Check::Required if empty => Some("required".to_string()),
            Check::Empty if !empty => Some("must be empty".to_string()),
            Check::Contains(text) if !empty && !check.holds(&v) => Some(format!("must contain {text}")),
            Check::Unique if !empty => {
                let key = v.display(None, false);
                let dup = seen.iter().find(|(k, _)| *k == key).map(|(_, first)| *first);
                seen.push((key.clone(), id));
                dup.map(|(_, r, _)| format!("duplicate of row {}: {key}", r + 1))
            }
            Check::Compare(op, rhs) if !empty && !v.is_error() && !compare_value(&v, *op, rhs) => {
                Some(format!("must be {} {rhs}", op.symbol()))
            }
            Check::OneOf(options) if !empty && !options.contains(&v.display(None, false)) => {
                Some(format!("must be one of {}", options.join(", ")))
            }
            _ => None,
        };
        if let Some(message) = message {
            out.push(Violation { cell: id, message });
        }
    }
}

// ---------------------------------------------------------------- filter

/// Name of the layer whose rules hide rows.
pub const FILTER: &str = "filter";

/// The rules of all `filter` layers: `Status: in(open, paid)`, `Amount: >= 100`.
/// A row is shown when it meets every rule of its table.
#[derive(Debug, Default)]
pub struct Filters {
    /// (sheet, column as written, checks)
    rules: Vec<(usize, String, Vec<Check>)>,
    pub errors: Vec<LayerError>,
}

impl Filters {
    pub fn new(attached: &[Attached], layer: &str) -> Filters {
        let mut f = Filters::default();
        for a in attached.iter().filter(|a| a.layer.name == layer) {
            for (i, rule) in a.layer.rules.iter().enumerate() {
                let Rule::Rule { target, body } = rule else { continue };
                let mut error = |message: String| {
                    f.errors.push(LayerError { line: a.layer.line.0 + 1 + i, sheet: a.sheet, layer: layer.to_string(), message: format!("{target}: {message}") })
                };
                let Target::Column(column) = Target::parse(target) else {
                    error("a filter rule names a column, e.g. `Status: = open`".to_string());
                    continue;
                };
                match Check::parse_all(body) {
                    Ok(checks) if checks.contains(&Check::Unique) => error("unique is a check, not a filter".to_string()),
                    Ok(checks) if checks.is_empty() => error("missing condition, e.g. `= open`, `>= 100`, `in(a, b)`".to_string()),
                    Ok(checks) => f.rules.push((a.sheet, column, checks)),
                    Err(e) => error(e),
                }
            }
        }
        f
    }

    /// Whether table `sheet` has filter rules.
    pub fn applies(&self, sheet: usize) -> bool {
        self.rules.iter().any(|(s, ..)| *s == sheet)
    }

    /// The columns of `sheet` that are filtered.
    pub fn columns(&self, wb: &Workbook, sheet: usize) -> Vec<usize> {
        self.rules.iter().filter(|(s, ..)| *s == sheet).filter_map(|(_, name, _)| column_index(wb, sheet, name)).collect()
    }

    /// The filtered columns of `sheet`, each with all its conditions (rules
    /// for the same column joined), in the order of the rules.
    pub fn conditions(&self, wb: &Workbook, sheet: usize) -> Vec<(usize, Vec<Check>)> {
        let mut out: Vec<(usize, Vec<Check>)> = Vec::new();
        for (_, name, checks) in self.rules.iter().filter(|(s, ..)| *s == sheet) {
            let Some(c) = column_index(wb, sheet, name) else { continue };
            match out.iter_mut().find(|(col, _)| *col == c) {
                Some((_, all)) => all.extend(checks.iter().cloned()),
                None => out.push((c, checks.clone())),
            }
        }
        out
    }

    /// Per data row of `sheet`, whether the filter hides it – judged by the
    /// values of data layer `z` (0: the table itself). Empty without rules.
    /// A rule for a column the table does not have hides nothing.
    pub fn hidden(&self, wb: &mut Workbook, sheet: usize, z: usize) -> Vec<bool> {
        if !self.applies(sheet) {
            return Vec::new();
        }
        let mut hidden = vec![false; wb.sheets[sheet].cells.len()];
        for (_, name, checks) in self.rules.iter().filter(|(s, ..)| *s == sheet) {
            let Some(c) = column_index(wb, sheet, name) else { continue };
            for (r, h) in hidden.iter_mut().enumerate() {
                if !*h {
                    let v = wb.value((sheet, r, crate::engine::in_layer(c, z)));
                    *h = !checks.iter().all(|check| check.holds(&v));
                }
            }
        }
        hidden
    }
}

/// A condition as typed, in the form the layer keeps: `open` → `= open`,
/// `= due soon` → `= "due soon"`.
pub fn normalize_condition(body: &str) -> Result<String, String> {
    let ok = |b: &str| Check::parse_all(b).is_ok_and(|c| !c.is_empty() && !c.contains(&Check::Unique));
    if ok(body) {
        return Ok(body.to_string());
    }
    let ops = ["<=", ">=", "<>", "!=", "=", "<", ">"];
    let candidate = match ops.iter().find(|op| body.starts_with(**op)) {
        Some(op) => format!("{op} {}", quote_value(body[op.len()..].trim())),
        None => format!("= {}", quote_value(body)),
    };
    if ok(&candidate) { Ok(candidate) } else { Err(format!("'{body}' is no condition – e.g. = open, >= 100, in(a, b), contains(text), empty, required")) }
}


/// `Status = open`, `Amount >= 100`: a column of `sheet` and the conditions
/// behind it (the longest beginning that names a column is the column).
pub fn row_condition(wb: &Workbook, sheet: usize, text: &str) -> Result<(usize, Vec<Check>), String> {
    let words: Vec<&str> = text.split_whitespace().collect();
    let named = (1..=words.len()).rev().find_map(|k| column_index(wb, sheet, &words[..k].join(" ")).map(|c| (c, words[k..].join(" "))));
    let split = || {
        let i = text.find(['<', '>', '=', '!'])?;
        column_index(wb, sheet, text[..i].trim()).map(|c| (c, text[i..].to_string()))
    };
    let (c, body) = named.or_else(split).ok_or_else(|| format!("'{text}' does not start with a column of the table"))?;
    if body.trim().is_empty() {
        return Err(format!("'{text}': a condition is missing, e.g. = open, >= 100"));
    }
    let checks = Check::parse_all(&normalize_condition(body.trim())?)?;
    Ok((c, checks))
}

/// Per data row of `sheet` on data layer `z`, whether it fails `checks` in column `col`.
pub fn failing_rows(wb: &mut Workbook, sheet: usize, z: usize, col: usize, checks: &[Check]) -> Vec<bool> {
    (0..wb.sheets[sheet].cells.len())
        .map(|r| {
            let v = wb.value((sheet, r, crate::engine::in_layer(col, z)));
            !checks.iter().all(|check| check.holds(&v))
        })
        .collect()
}

// ---------------------------------------------------------------- view

/// Name of the layer that says how columns are shown.
pub const VIEW: &str = "view";

/// How the columns of one table are shown.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ColumnView {
    /// Per column, whether it is hidden.
    pub hidden: Vec<bool>,
    /// Per column, its fixed width in characters.
    pub width: Vec<Option<u16>>,
    /// The first columns that stay in place while scrolling sideways.
    pub frozen: usize,
}

impl ColumnView {
    pub fn is_hidden(&self, c: usize) -> bool {
        self.hidden.get(c) == Some(&true)
    }

    pub fn hidden_count(&self) -> usize {
        self.hidden.iter().filter(|h| **h).count()
    }
}

/// The rules of all `view` layers: `Note: hide`, `Item: width 20`,
/// `B: freeze` (the columns up to B stay while scrolling sideways).
#[derive(Debug, Default)]
pub struct Views {
    /// (sheet, column as written, setting)
    rules: Vec<(usize, String, ViewSetting)>,
    pub errors: Vec<LayerError>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum ViewSetting {
    Hide,
    Width(u16),
    Freeze,
}

impl Views {
    pub fn new(attached: &[Attached], layer: &str) -> Views {
        let mut v = Views::default();
        for a in attached.iter().filter(|a| a.layer.name == layer) {
            for (i, rule) in a.layer.rules.iter().enumerate() {
                let Rule::Rule { target, body } = rule else { continue };
                let mut error = |message: String| {
                    v.errors.push(LayerError { line: a.layer.line.0 + 1 + i, sheet: a.sheet, layer: layer.to_string(), message: format!("{target}: {message}") })
                };
                let Target::Column(column) = Target::parse(target) else {
                    error("a view rule names a column, e.g. `Note: hide`".to_string());
                    continue;
                };
                let mut words = body.split_whitespace().peekable();
                if words.peek().is_none() {
                    error("missing setting: hide, width n or freeze".to_string());
                }
                while let Some(w) = words.next() {
                    let setting = match w {
                        "hide" => Some(ViewSetting::Hide),
                        "freeze" => Some(ViewSetting::Freeze),
                        "width" => words.next().and_then(|n| n.parse::<u16>().ok()).filter(|n| *n >= 1).map(ViewSetting::Width),
                        _ => None,
                    };
                    match setting {
                        Some(s) => v.rules.push((a.sheet, column.clone(), s)),
                        None if w == "width" => error("width: a number of characters is missing, e.g. `width 20`".to_string()),
                        None => error(format!("unknown setting '{w}' – hide, width n or freeze")),
                    }
                }
            }
        }
        v
    }

    pub fn applies(&self, sheet: usize) -> bool {
        self.rules.iter().any(|(s, ..)| *s == sheet)
    }

    /// How the columns of `sheet` are shown. Rules for columns the table
    /// does not have do nothing.
    pub fn columns(&self, wb: &Workbook, sheet: usize) -> ColumnView {
        let cols = wb.sheets[sheet].columns.len();
        let mut view = ColumnView { hidden: vec![false; cols], width: vec![None; cols], frozen: 0 };
        for (_, name, setting) in self.rules.iter().filter(|(s, ..)| *s == sheet) {
            let Some(c) = column_index(wb, sheet, name) else { continue };
            match setting {
                ViewSetting::Hide => view.hidden[c] = true,
                ViewSetting::Width(n) => view.width[c] = Some(*n),
                ViewSetting::Freeze => view.frozen = view.frozen.max(c + 1),
            }
        }
        view
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse;

    const SRC: &str = "\
| Name | Value | Status |
|------|------|--------|
| a    | 5    | open  |
| b    | -1   | zu     |
| a    |      | open  |

```layer format
B: align right decimals(2)
row[Status=open]: fg yellow bold
B2: bg #ff0000
```

```layer validate
Name: required unique
B: required >= 0
Status: in(open, zu)
```
";

    fn setup() -> (Vec<Attached>, Workbook) {
        let doc = parse(SRC).unwrap();
        let (wb, diags) = Workbook::from_document(&doc);
        assert!(diags.is_empty());
        (attach(&doc, &wb), wb)
    }

    #[test]
    fn targets() {
        assert_eq!(Target::parse("B"), Target::Column("B".into()));
        assert_eq!(Target::parse("3"), Target::Row(3));
        assert_eq!(Target::parse("B2"), Target::Cell(1, 1));
        assert_eq!(Target::parse("A1:B2"), Target::Range((0, 0), (1, 1)));
        assert_eq!(
            Target::parse("row[Status=open]"),
            Target::RowWhere { column: "Status".into(), op: CmpOp::Eq, value: "open".into() }
        );
        assert_eq!(Target::parse("Value[> 3]"), Target::ColumnWhere { column: "Value".into(), checks: vec![Check::Compare(CmpOp::Gt, "3".into())] });
        assert_eq!(Target::parse("Status[in(open, zu)]"), Target::ColumnWhere { column: "Status".into(), checks: vec![Check::OneOf(vec!["open".into(), "zu".into()])] });
        assert_eq!(Target::parse("Value[what]"), Target::Column("Value[what]".into()), "no condition: a column name");
        let (att, mut wb) = setup();
        let _ = att;
        let t = Target::parse("Value[> 3]");
        assert!(t.matches(&mut wb, (0, 0, 1)) && !t.matches(&mut wb, (0, 1, 1)) && !t.matches(&mut wb, (0, 0, 0)));
        assert!(!t.matches(&mut wb, (0, 2, 1)), "an empty cell is not larger than 3");
    }

    #[test]
    fn format_rules() {
        let (att, mut wb) = setup();
        let f = Formats::new(&att, "format");
        assert!(f.errors.is_empty(), "{:?}", f.errors);
        let st = f.style(&mut wb, (0, 0, 1));
        assert_eq!(st.align, Some(Align::Right));
        assert!(st.bold);
        assert_eq!(st.fg, Some(Color::Named("yellow".into())));
        assert_eq!(st.display(&Value::num(5.0), false).as_deref(), Some("5.00"));
        let st = f.style(&mut wb, (0, 1, 1));
        assert!(!st.bold);
        assert_eq!(st.bg, Some(Color::Rgb(255, 0, 0)));
    }

    #[test]
    fn switching_off() {
        let (column, _) = CellStyle::parse("bold fg red bg #00ff00");
        let (cell, errs) = CellStyle::parse("nobold fg none");
        assert!(errs.is_empty(), "{errs:?}");
        let mut st = CellStyle::default();
        st.merge(&column);
        st.merge(&cell);
        assert!(!st.bold);
        assert_eq!(st.fg, None);
        assert_eq!(st.bg, Some(Color::Rgb(0, 255, 0)));
        assert_eq!(st.off, Off::default(), "a merged style switches nothing off");
        assert_eq!(style_parts("fg red bold  align right nobold"), ["fg red", "bold", "align right", "nobold"]);
        assert_eq!(style_parts("wrap 20 bold wrap"), ["wrap 20", "bold", "wrap"]);
        assert_eq!(part_family("nowrap"), "wrap");
        assert_eq!(part_family("wrap 20"), "wrap");
        assert_eq!(part_family("nobold"), "bold");
        assert_eq!(part_family("fg none"), "fg");
        assert_eq!(part_family("normal"), "normal");
    }

    #[test]
    fn percent_display() {
        let (st, errs) = CellStyle::parse("percent(1)");
        assert!(errs.is_empty());
        assert_eq!(st.display(&Value::num(0.905), true).as_deref(), Some("90,5 %"));
    }

    #[test]
    fn validation() {
        let (att, mut wb) = setup();
        let (v, errs) = validate(&att, &mut wb, "validate");
        assert!(errs.is_empty(), "{errs:?}");
        let msgs: Vec<(usize, usize, &str)> = v.iter().map(|v| (v.cell.1, v.cell.2, v.message.as_str())).collect();
        assert!(msgs.contains(&(2, 0, "duplicate of row 1: a")));
        assert!(msgs.contains(&(1, 1, "must be >= 0")));
        assert!(msgs.contains(&(2, 1, "required")));
        assert_eq!(v.len(), 3, "{msgs:?}");
    }

    #[test]
    fn bad_checks() {
        assert!(Check::parse_all("positive").is_err());
        assert_eq!(Check::parse_all(">=0").unwrap(), vec![Check::Compare(CmpOp::Ge, "0".into())]);
    }

    #[test]
    fn view_rules() {
        let src = "\
| Name | Value | Status | Note |
|------|------|--------|---|
| a | 5 | open | x |

```layer view
Name: freeze width 12
D: hide
Status: hide
B2: hide
Value: wide
Value: width
Nowhere: hide
```
";
        let doc = parse(src).unwrap();
        let (wb, _) = Workbook::from_document(&doc);
        let v = Views::new(&attach(&doc, &wb), VIEW);
        let view = v.columns(&wb, 0);
        assert_eq!(view, ColumnView { hidden: vec![false, false, true, true], width: vec![Some(12), None, None, None], frozen: 1 });
        assert_eq!(v.errors.len(), 3, "{:?}", v.errors);
    }

    #[test]
    fn quoted_values() {
        assert_eq!(Check::parse_all("= \"New York\"").unwrap(), vec![Check::Compare(CmpOp::Eq, "New York".into())]);
        assert_eq!(
            Check::parse_all("in(open, \"a, b\", \"say \"\"hi\"\"\") required").unwrap(),
            vec![Check::OneOf(vec!["open".into(), "a, b".into(), "say \"hi\"".into()]), Check::Required]
        );
        assert_eq!(Check::parse_all("contains(rent) empty").unwrap(), vec![Check::Contains("rent".into()), Check::Empty]);
        assert!(Check::parse_all("in(a, b").is_err());
        assert!(Check::parse_all("= \"open").is_err());
        assert!(Check::parse_all(">=").is_err());
        for v in ["open", "New York", "a, b", "say \"hi\"", ""] {
            assert_eq!(Check::parse_all(&format!("= {}", quote_value(v))).unwrap(), vec![Check::Compare(CmpOp::Eq, v.into())], "{v}");
            assert_eq!(Check::parse_all(&format!("in({})", quote_value(v))).unwrap(), vec![Check::OneOf(vec![v.into()])], "{v}");
        }
    }

    #[test]
    fn filters_hide_rows() {
        let src = "\
| Name | Value | Status |
|------|------|--------|
| Rent | 1200 | open |
| Power | 85.5 | paid |
| Phone |  | open |
| Car | 300 | due soon |

```layer filter
Status: in(open, \"due soon\")
B: >= 100
```
";
        let doc = parse(src).unwrap();
        let (mut wb, _) = Workbook::from_document(&doc);
        let f = Filters::new(&attach(&doc, &wb), FILTER);
        assert!(f.errors.is_empty(), "{:?}", f.errors);
        // Power is paid, Phone has no value.
        assert_eq!(f.hidden(&mut wb, 0, 0), [false, true, true, false]);
        assert_eq!(f.columns(&wb, 0), [2, 1]);

        let hidden = |rules: &str| {
            let doc = parse(&src.replace("Status: in(open, \"due soon\")\nB: >= 100", rules)).unwrap();
            let (mut wb, _) = Workbook::from_document(&doc);
            let f = Filters::new(&attach(&doc, &wb), FILTER);
            (f.hidden(&mut wb, 0, 0), f.errors.len())
        };
        assert_eq!(hidden("Value: empty"), (vec![true, true, false, true], 0));
        // An empty cell is not "less than 100"; it differs from a value and equals "".
        assert_eq!(hidden("Value: < 100"), (vec![true, false, true, true], 0));
        assert_eq!(hidden("Value: <> 300"), (vec![false, false, false, true], 0));
        assert_eq!(hidden("Value: = \"\""), (vec![true, true, false, true], 0));
        assert_eq!(hidden("Value: in(300, \"\")"), (vec![true, true, false, false], 0));
        assert_eq!(hidden("Name: contains(R)"), (vec![false, false, true, false], 0));
        assert_eq!(hidden("Status: <> open"), (vec![true, false, true, false], 0));
        // Rules that cannot be read hide nothing and are reported.
        assert_eq!(hidden("B2: = 1\nName: unique\nName: big\nStatus:"), (vec![], 4));
        assert_eq!(hidden("Nowhere: = 1"), (vec![false; 4], 0));
    }
}
