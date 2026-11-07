//! Workbook evaluation: typed cells, formulas and incremental recalculation.
//!
//! Formula results are computed lazily and memoized. While a formula is being
//! evaluated, every cell it reads is recorded as a dependency, so changing a
//! cell only invalidates its transitive dependents.

use crate::formula::{parse_formula, BinOp, Expr, LayerSel};
use crate::functions::UserFn;
use crate::model::{Block, Document, Shape};
use crate::types::{parse_cell, Type, TypeRegistry};
use crate::units::Unit;
use crate::value::{round_to, Value};
use std::collections::{HashMap, HashSet};

/// (sheet, row, column), all zero-based; rows exclude the header.
/// Footer cells have rows from [`FOOTER_ROW`] on; the column carries the
/// data layer in its upper bits ([`in_layer`]), 0 being the table itself.
pub type CellId = (usize, usize, usize);

const LAYER_SHIFT: u32 = 32;

/// Column `c` in data layer `z` (z = 0: the table itself).
pub fn in_layer(c: usize, z: usize) -> usize {
    c | (z << LAYER_SHIFT)
}

/// (column, data layer) of the column part of a [`CellId`].
pub fn split_layer(c: usize) -> (usize, usize) {
    (c & ((1 << LAYER_SHIFT) - 1), c >> LAYER_SHIFT)
}

/// Data layer letters: a, b, … z, aa, ab …
pub fn layer_letters(z: usize) -> String {
    col_letters(z).to_lowercase()
}

pub fn letters_to_layer(s: &str) -> Option<usize> {
    (!s.is_empty() && s.chars().all(|c| c.is_ascii_lowercase())).then(|| letters_to_col(&s.to_uppercase())).flatten()
}

/// Row of the first footer row in a [`CellId`]; footer row `k` is `FOOTER_ROW + k`.
pub const FOOTER_ROW: usize = usize::MAX / 2;

/// Aggregates a footer formula may name alone: `=sum` stands for `=sum(Column)`.
pub const FOOTER_AGGREGATES: &[&str] = &[
    "sum", "avg", "average", "mean", "min", "max", "count", "counta", "countblank", "median", "stdev", "product", "summe",
    "mittelwert", "anzahl",
];

#[derive(Debug, Clone, PartialEq)]
pub enum Cell {
    Value(Value),
    Formula { src: String, expr: Result<Expr, String> },
}

#[derive(Debug, Clone)]
pub struct SheetColumn {
    pub name: String,
    pub ty: Type,
}

#[derive(Debug, Clone)]
pub struct Sheet {
    pub name: Option<String>,
    pub shape: Shape,
    pub columns: Vec<SheetColumn>,
    pub raw: Vec<Vec<String>>,
    pub cells: Vec<Vec<Cell>>,
    pub footer: Vec<FooterLine>,
    /// Name of data layer a (the table itself), if given.
    pub layer_name: Option<String>,
    /// Data layers b, c, …
    pub layers: Vec<SheetLayer>,
}

/// A data layer of a sheet: its own cells (`None` where it takes the table's).
#[derive(Debug, Clone)]
pub struct SheetLayer {
    pub name: String,
    /// Empty cells show the table's value too (else only its formula).
    pub scenario: bool,
    pub cells: Vec<Vec<Option<Cell>>>,
}

impl Sheet {
    /// Data layer `z` by name (a: the table itself).
    pub fn layer_index(&self, name: &str) -> Option<usize> {
        if self.layer_name.as_deref() == Some(name) {
            return Some(0);
        }
        self.layers.iter().position(|l| l.name == name).map(|i| i + 1)
    }

    /// The name of data layer `z`; the letter if it has none.
    pub fn layer_label(&self, z: usize) -> String {
        match z {
            0 => self.layer_name.clone(),
            _ => self.layers.get(z - 1).map(|l| l.name.clone()),
        }
        .unwrap_or_else(|| layer_letters(z))
    }
}

/// A footer row: its label and per column a cell.
#[derive(Debug, Clone)]
pub struct FooterLine {
    pub label: String,
    pub cells: Vec<Cell>,
}

/// Reads a footer cell of column `column`, written as in the data: a value
/// or text, or a formula – where an aggregate alone (`=sum`) applies to the
/// column. A value that does not fit the column type is taken as it reads.
pub fn footer_cell(raw: &str, column: &SheetColumn) -> Cell {
    let raw = raw.trim();
    if let Some(src) = raw.strip_prefix('=') {
        let lower = src.trim().to_lowercase();
        let expr = if FOOTER_AGGREGATES.contains(&lower.as_str()) {
            Ok(Expr::Call(lower, vec![Expr::Name(column.name.clone())]))
        } else {
            parse_formula(src)
        };
        return Cell::Formula { src: raw.to_string(), expr };
    }
    match parse_cell(raw, &column.ty).value {
        Value::Error(_) => Cell::Value(parse_cell(raw, &Type::auto()).value),
        v => Cell::Value(v),
    }
}

/// Whether a footer cell counts (its result is no amount of the column).
fn counts(cell: &Cell) -> bool {
    matches!(cell, Cell::Formula { expr: Ok(Expr::Call(name, _)), .. } if name.starts_with("count") || name == "anzahl")
}

#[derive(Debug, Clone, PartialEq)]
pub struct Diagnostic {
    /// Sheet name or `types`.
    pub location: String,
    pub message: String,
}

#[derive(Debug, Default)]
struct EvalState {
    values: HashMap<CellId, Value>,
    in_progress: HashSet<CellId>,
    dependents: HashMap<CellId, HashSet<CellId>>,
}

#[derive(Debug)]
pub struct Workbook {
    pub types: TypeRegistry,
    pub sheets: Vec<Sheet>,
    /// User functions by (lower-case) name.
    pub functions: HashMap<String, UserFn>,
    /// Whether the document writes decimals with `,`.
    pub decimal_comma: bool,
    state: EvalState,
}

impl Workbook {
    pub fn from_document(doc: &Document) -> (Workbook, Vec<Diagnostic>) {
        let mut diags = Vec::new();
        let mut types = TypeRegistry::default();
        for block in &doc.blocks {
            if let Block::Types(lines) = block {
                for e in types.add_block(lines) {
                    diags.push(Diagnostic { location: "types".into(), message: e });
                }
            }
        }

        let mut sheets = Vec::new();
        for block in &doc.blocks {
            let Block::Table(t) = block else { continue };
            let location = t.name.clone().unwrap_or_else(|| format!("table #{}", sheets.len() + 1));
            let columns: Vec<SheetColumn> = t
                .columns
                .iter()
                .map(|c| {
                    let ty = match &c.ty {
                        None => Type::auto(),
                        Some(expr) => types.resolve(expr).unwrap_or_else(|e| {
                            diags.push(Diagnostic { location: location.clone(), message: format!("column {}: {e}", c.name) });
                            Type::auto()
                        }),
                    };
                    SheetColumn { name: c.name.clone(), ty }
                })
                .collect();

            let mut cells = Vec::new();
            for (r, row) in t.rows.iter().enumerate() {
                let mut out = Vec::new();
                for (c, raw) in row.iter().enumerate() {
                    let cell = make_cell(raw, &columns[c].ty);
                    match &cell {
                        Cell::Value(Value::Error(e)) | Cell::Formula { expr: Err(e), .. } => {
                            diags.push(Diagnostic {
                                location: location.clone(),
                                message: format!("{}{}: {e}", col_letters(c), r + 1),
                            });
                        }
                        _ => {}
                    }
                    out.push(cell);
                }
                cells.push(out);
            }
            let footer = t
                .footer
                .iter()
                .map(|f| FooterLine {
                    label: f.label.clone(),
                    cells: columns
                        .iter()
                        .enumerate()
                        .map(|(c, col)| {
                            let cell = footer_cell(f.cells.get(c).map_or("", String::as_str), col);
                            if let Cell::Formula { expr: Err(e), .. } = &cell {
                                diags.push(Diagnostic { location: location.clone(), message: format!("footer {} {}: {e}", f.label, col.name) });
                            }
                            cell
                        })
                        .collect(),
                })
                .collect();
            let layers = t
                .layers
                .iter()
                .map(|l| SheetLayer {
                    name: l.name.clone(),
                    scenario: l.kind == crate::model::LayerKind::Scenario,
                    cells: (0..t.rows.len())
                        .map(|r| {
                            (0..columns.len())
                                .map(|c| {
                                    let raw = l.rows.get(r).and_then(|row| row.get(c)).map_or("", |s| s.as_str());
                                    if raw.trim().is_empty() {
                                        return None;
                                    }
                                    let cell = make_cell(raw, &columns[c].ty);
                                    if let Cell::Value(Value::Error(e)) | Cell::Formula { expr: Err(e), .. } = &cell {
                                        diags.push(Diagnostic {
                                            location: location.clone(),
                                            message: format!("layer {}: {}{}: {e}", l.name, col_letters(c), r + 1),
                                        });
                                    }
                                    Some(cell)
                                })
                                .collect()
                        })
                        .collect(),
                })
                .collect();
            sheets.push(Sheet {
                name: t.name.clone(),
                shape: t.shape.clone(),
                columns,
                raw: t.rows.clone(),
                cells,
                footer,
                layer_name: t.layer_name.clone(),
                layers,
            });
        }
        // A footer block that did not attach to the table before it.
        let mut previous: Option<&crate::model::Table> = None;
        for block in &doc.blocks {
            match block {
                Block::Table(t) => {
                    previous = Some(t);
                    continue;
                }
                Block::Code { info, .. } if crate::parse::is_data_layer_like(info) => {
                    let reason = match crate::parse::data_layer_info(info) {
                        None => "names are no references or lower-case letters".to_string(),
                        Some(_) => "its columns or rows do not match the table before it, or the name is taken".to_string(),
                    };
                    diags.push(Diagnostic { location: info.clone(), message: format!("block ignored: {reason}") });
                }
                Block::Code { info, body } if crate::parse::is_footer_info(info) => {
                    let reason = match previous {
                        Some(t) if !t.footer.is_empty() => "the table before it has a footer already".to_string(),
                        Some(t) => crate::parse::parse_footer(body, &t.columns).err().unwrap_or_default(),
                        None => "no table before it".into(),
                    };
                    diags.push(Diagnostic { location: "footer".into(), message: format!("block ignored: {reason}") });
                }
                _ => {}
            }
            previous = None;
        }

        // User functions: later definitions of the same name win; errors are reported.
        let mut functions = HashMap::new();
        for block in &doc.blocks {
            if let Block::Functions(lines) = block {
                let (defs, errors) = crate::functions::parse_block(lines);
                diags.extend(errors.into_iter().map(|message| Diagnostic { location: "functions".into(), message }));
                for (_, def) in defs {
                    match def.compile() {
                        Ok(f) => {
                            functions.insert(def.name.clone(), f);
                        }
                        Err(message) => diags.push(Diagnostic { location: "functions".into(), message }),
                    }
                }
            }
        }
        let wb = Workbook { types, sheets, functions, decimal_comma: crate::locale::decimal_comma(), state: EvalState::default() };
        (wb, diags)
    }

    pub fn sheet_index(&self, name: &str) -> Option<usize> {
        self.sheets.iter().position(|s| s.name.as_deref() == Some(name))
    }

    /// The (computed) value of a cell.
    pub fn value(&mut self, id: CellId) -> Value {
        let mut ev = Evaluator { sheets: &self.sheets, functions: &self.functions, state: &mut self.state, current: None, env: Vec::new(), layer: 0, pinned: false };
        ev.cell(id)
    }

    /// Evaluates a formula that is not stored in a cell (e.g. `{=…}` in text).
    /// Plain names refer to table `sheet`.
    pub fn eval_formula(&mut self, src: &str, sheet: usize) -> Value {
        if self.sheets.is_empty() {
            return Value::err("no tables");
        }
        let expr = match parse_formula(src) {
            Ok(e) => e,
            Err(e) => return Value::err(format!("syntax: {e}")),
        };
        let mut ev = Evaluator { sheets: &self.sheets, functions: &self.functions, state: &mut self.state, current: None, env: Vec::new(), layer: 0, pinned: false };
        match ev.eval(&expr, sheet.min(self.sheets.len() - 1)) {
            Value::Vector(mut items) if items.len() == 1 => items.remove(0),
            v => v,
        }
    }

    /// Evaluates a formula as if it stood in data layer `z` of table `sheet`.
    pub fn eval_formula_in(&mut self, src: &str, sheet: usize, z: usize) -> Value {
        let expr = match parse_formula(src) {
            Ok(e) => e,
            Err(e) => return Value::err(format!("syntax: {e}")),
        };
        let mut ev = Evaluator { sheets: &self.sheets, functions: &self.functions, state: &mut self.state, current: None, env: Vec::new(), layer: z, pinned: false };
        match ev.eval(&expr, sheet) {
            Value::Vector(mut items) if items.len() == 1 => items.remove(0),
            v => v,
        }
    }

    /// Replaces cell (`r`, `c`) of data layer `z` ≥ 1 of table `s` (empty: the table's again).
    pub fn set_layer_cell(&mut self, s: usize, z: usize, r: usize, c: usize, raw: &str) {
        let cell = (!raw.trim().is_empty()).then(|| make_cell(raw, &self.sheets[s].columns[c].ty));
        self.sheets[s].layers[z - 1].cells[r][c] = cell;
        self.invalidate((s, r, in_layer(c, z)));
    }

    /// Replaces a cell's content and invalidates everything that depends on it.
    pub fn set_cell(&mut self, id: CellId, raw: &str) {
        let (s, r, c) = id;
        let ty = &self.sheets[s].columns[c].ty;
        // Files hold numbers with a decimal point, whatever was typed.
        let raw = if raw.trim_start().starts_with('=') { None } else { crate::types::point_cell(raw, ty) }.unwrap_or_else(|| raw.to_string());
        let cell = make_cell(&raw, ty);
        self.sheets[s].raw[r][c] = raw;
        self.sheets[s].cells[r][c] = cell;
        self.invalidate(id);
    }

    /// Replaces footer cell (`k`, `c`) of table `s` and recalculates what depends on it.
    pub fn set_footer(&mut self, s: usize, k: usize, c: usize, raw: &str) {
        self.sheets[s].footer[k].cells[c] = footer_cell(raw, &self.sheets[s].columns[c]);
        self.invalidate((s, FOOTER_ROW + k, c));
    }

    fn invalidate(&mut self, id: CellId) {
        let mut stack = vec![id];
        let mut seen = HashSet::new();
        while let Some(id) = stack.pop() {
            if !seen.insert(id) {
                continue;
            }
            self.state.values.remove(&id);
            if let Some(deps) = self.state.dependents.remove(&id) {
                stack.extend(deps);
            }
        }
    }

    /// Evaluates every formula, which also completes the dependency graph.
    pub fn eval_all(&mut self) {
        for s in 0..self.sheets.len() {
            for k in 0..self.sheets[s].footer.len() {
                for c in 0..self.sheets[s].columns.len() {
                    self.value((s, FOOTER_ROW + k, c));
                }
            }
            for z in 0..=self.sheets[s].layers.len() {
                for r in 0..self.sheets[s].cells.len() {
                    for c in 0..self.sheets[s].columns.len() {
                        let own = z > 0 && matches!(self.sheets[s].layers[z - 1].cells[r][c], Some(Cell::Formula { .. }));
                        if own || matches!(self.sheets[s].cells[r][c], Cell::Formula { .. }) {
                            self.value((s, r, in_layer(c, z)));
                        }
                    }
                }
                if z > 0 {
                    for k in 0..self.sheets[s].footer.len() {
                        for c in 0..self.sheets[s].columns.len() {
                            self.value((s, FOOTER_ROW + k, in_layer(c, z)));
                        }
                    }
                }
            }
        }
    }

    /// Cells whose formulas read `id` directly (call [`eval_all`](Self::eval_all) first).
    pub fn dependents_of(&self, id: CellId) -> Vec<CellId> {
        let mut out: Vec<CellId> = self.state.dependents.get(&id).map(|d| d.iter().copied().collect()).unwrap_or_default();
        out.sort();
        out
    }

    /// Cells read directly by the formula in `id` (call [`eval_all`](Self::eval_all) first).
    pub fn precedents_of(&self, id: CellId) -> Vec<CellId> {
        let mut out: Vec<CellId> =
            self.state.dependents.iter().filter(|(_, d)| d.contains(&id)).map(|(k, _)| *k).collect();
        out.sort();
        out
    }

    /// Whether a formula result is currently memoized (observes incremental recalculation).
    pub fn is_cached(&self, id: CellId) -> bool {
        self.state.values.contains_key(&id)
    }

    /// Display text of a cell, using the column's decimals and omitting the
    /// unit when it matches the column type (the header already shows it).
    pub fn display(&mut self, id: CellId) -> String {
        let v = self.value(id);
        let col = split_layer(id.2).0;
        let ty = &self.sheets[id.0].columns[col].ty;
        if let Some(k) = id.1.checked_sub(FOOTER_ROW) {
            // Footers show amounts of the column like the column; counts and
            // results in another unit as they are.
            let counting = self.sheets[id.0].footer.get(k).and_then(|f| f.cells.get(col)).is_some_and(counts);
            return match v {
                Value::Num(n, u) if u == ty.unit && !counting => Value::num(n).display(ty.decimals(), self.decimal_comma),
                v => v.display(None, self.decimal_comma),
            };
        }
        let v = match v {
            Value::Num(n, u) if u == ty.unit => Value::num(n),
            v => v,
        };
        v.display(ty.decimals(), self.decimal_comma)
    }

    /// Copy of `doc` with every table cell replaced by its computed display value.
    pub fn render(&mut self, doc: &Document) -> Document {
        let mut out = doc.clone();
        let mut s = 0;
        for block in &mut out.blocks {
            match block {
                Block::Table(t) => {
                    for r in 0..t.rows.len() {
                        for c in 0..t.columns.len() {
                            t.rows[r][c] = self.display((s, r, c));
                        }
                    }
                    s += 1;
                }
                Block::Text(lines) => {
                    for line in lines.iter_mut() {
                        *line = crate::text::live_values(line, self, s.saturating_sub(1));
                    }
                }
                _ => {}
            }
        }
        out
    }
}

fn make_cell(raw: &str, ty: &Type) -> Cell {
    if raw.trim_start().starts_with('=') {
        return Cell::Formula { src: raw.to_string(), expr: parse_formula(raw) };
    }
    Cell::Value(parse_cell(raw, ty).value)
}

/// Rewrites numbers written with a decimal comma to the decimal point, as
/// files hold them: data cells, footers, slices and scenarios by their column
/// type, and the numbers of calculation sheets (```hardcalc blocks).
pub fn normalize_numbers(doc: &mut Document) {
    let mut types = TypeRegistry::default();
    for block in &doc.blocks {
        if let Block::Types(lines) = block {
            let _ = types.add_block(lines);
        }
    }
    let point = |raw: &mut String, ty: &Type| {
        if !raw.trim_start().starts_with('=')
            && let Some(p) = crate::types::point_cell(raw, ty)
        {
            *raw = p;
        }
    };
    for block in &mut doc.blocks {
        match block {
            Block::Table(t) => {
                let tys: Vec<Type> = t
                    .columns
                    .iter()
                    .map(|c| c.ty.as_deref().and_then(|e| types.resolve(e).ok()).unwrap_or_else(Type::auto))
                    .collect();
                let rows = t.rows.iter_mut().chain(t.layers.iter_mut().flat_map(|l| l.rows.iter_mut()));
                for row in rows.chain(t.footer.iter_mut().map(|f| &mut f.cells)) {
                    for (c, raw) in row.iter_mut().enumerate() {
                        if let Some(ty) = tys.get(c) {
                            point(raw, ty);
                        }
                    }
                }
            }
            Block::Code { info, body } if crate::sheet::is_sheet_info(info) => {
                for line in body.iter_mut() {
                    *line = crate::sheet::expr::point_decimals(line);
                }
            }
            _ => {}
        }
    }
}

pub fn col_letters(mut c: usize) -> String {
    let mut s = String::new();
    loop {
        s.insert(0, (b'A' + (c % 26) as u8) as char);
        if c < 26 {
            return s;
        }
        c = c / 26 - 1;
    }
}

/// A cell or column reference with its `$` anchors: `$B$2`, `B2`, `$B`,
/// and a data layer after the row: `B2b`, `$B$2$b`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RefParts {
    pub col: usize,
    pub col_abs: bool,
    /// Row and whether it is anchored; `None` for a whole column.
    pub row: Option<(usize, bool)>,
    /// Data layer (only after a row); `None`: the formula's own.
    pub layer: Option<(usize, bool)>,
}

/// The parts of a (partial) reference: `[$]COL [$]ROW [$]layer`, each
/// optional, in this order – `B2b`, `4c`, `c`, `D`.
pub type Partial = (Option<(usize, bool)>, Option<(usize, bool)>, Option<(usize, bool)>);

pub fn parse_partial(s: &str) -> Option<Partial> {
    let mut rest = s;
    let mut part = |pred: fn(char) -> bool| -> Option<(String, bool)> {
        let (abs, r) = rest.strip_prefix('$').map_or((false, rest), |r| (true, r));
        let end = r.find(|c: char| !pred(c)).unwrap_or(r.len());
        if end == 0 {
            return None;
        }
        let text = r[..end].to_string();
        rest = &r[end..];
        Some((text, abs))
    };
    let col = part(|c| c.is_ascii_uppercase()).map(|(t, abs)| letters_to_col(&t).map(|c| (c, abs)));
    let row = part(|c| c.is_ascii_digit()).map(|(t, abs)| t.parse::<usize>().ok().filter(|r| *r >= 1).map(|r| (r - 1, abs)));
    let layer = part(|c| c.is_ascii_lowercase()).map(|(t, abs)| letters_to_layer(&t).map(|z| (z, abs)));
    if !rest.is_empty() || (col.is_none() && row.is_none() && layer.is_none()) {
        return None;
    }
    let flat = |p: Option<Option<(usize, bool)>>| match p {
        None => Some(None),
        Some(None) => None,
        Some(v) => Some(v),
    };
    Some((flat(col)?, flat(row)?, flat(layer)?))
}

impl RefParts {
    pub fn parse(s: &str) -> Option<RefParts> {
        let (col, row, layer) = parse_partial(s)?;
        let (col, col_abs) = col?;
        // A layer only after a row: `Rent` or `Nr` are no references.
        if layer.is_some() && row.is_none() {
            return None;
        }
        Some(RefParts { col, col_abs, row, layer })
    }

    pub fn write(&self) -> String {
        let d = |abs: bool| if abs { "$" } else { "" };
        let mut out = format!("{}{}", d(self.col_abs), col_letters(self.col));
        if let Some((r, abs)) = self.row {
            out.push_str(&format!("{}{}", d(abs), r + 1));
        }
        if let Some((z, abs)) = self.layer {
            out.push_str(&format!("{}{}", d(abs), layer_letters(z)));
        }
        out
    }
}

pub fn letters_to_col(s: &str) -> Option<usize> {
    let s = s.strip_prefix('$').unwrap_or(s);
    if s.is_empty() || !s.chars().all(|c| c.is_ascii_uppercase()) {
        return None;
    }
    Some(s.chars().fold(0, |acc, c| acc * 26 + (c as usize - 'A' as usize + 1)) - 1)
}

/// `B12` → (col 1, row 11)
/// Also `$B$12` (anchors make no difference for the value); not with a layer.
pub fn parse_a1(s: &str) -> Option<(usize, usize)> {
    match RefParts::parse(s)? {
        RefParts { col, row: Some((row, _)), layer: None, .. } => Some((col, row)),
        _ => None,
    }
}

/// `B12c` → (col 1, row 11, Some(layer 2)); `B12` → (1, 11, None).
pub fn parse_cell_ref(s: &str) -> Option<(usize, usize, Option<usize>)> {
    match RefParts::parse(s)? {
        RefParts { col, row: Some((row, _)), layer, .. } => Some((col, row, layer.map(|(z, _)| z))),
        _ => None,
    }
}

struct Evaluator<'a> {
    sheets: &'a [Sheet],
    functions: &'a HashMap<String, UserFn>,
    state: &'a mut EvalState,
    current: Option<CellId>,
    /// Parameters of the user functions being evaluated (innermost last).
    env: Vec<HashMap<String, Value>>,
    /// The data layer references without one read (that of the formula).
    layer: usize,
    /// The layer was named in the reference (`Plan!B2.February`), so it also
    /// applies to another table; else other tables are read in layer a.
    pinned: bool,
}

/// How deep user functions may call each other (recursion guard).
const MAX_CALL_DEPTH: usize = 64;

impl Evaluator<'_> {
    fn cell(&mut self, id: CellId) -> Value {
        if let Some(cur) = self.current {
            self.state.dependents.entry(id).or_default().insert(cur);
        }
        if id.1 >= FOOTER_ROW {
            return self.footer_cell(id);
        }
        let (c, z) = split_layer(id.2);
        let Some(sheet) = self.sheets.get(id.0) else { return Value::err("reference out of range") };
        if z > sheet.layers.len() {
            return Value::err(format!("no layer {} in {}", layer_letters(z), sheet.name.as_deref().unwrap_or("this table")));
        }
        let Some(base) = sheet.cells.get(id.1).and_then(|r| r.get(c)) else {
            return Value::err("reference out of range");
        };
        // In a slice or scenario: its own cell, else the table's formula (and
        // in a scenario all of the table's values, in a slice its row names
        // in the first column).
        let cell = match z {
            0 => base,
            _ => match &sheet.layers[z - 1].cells[id.1][c] {
                Some(own) => own,
                None => {
                    let base_id = (id.0, id.1, c);
                    self.state.dependents.entry(base_id).or_default().insert(id);
                    match base {
                        Cell::Value(v) if sheet.layers[z - 1].scenario || c == 0 => return v.clone(),
                        Cell::Value(_) => return Value::Null,
                        formula => formula,
                    }
                }
            },
        };
        let expr = match cell {
            Cell::Value(v) => return v.clone(),
            Cell::Formula { expr: Err(e), .. } => return Value::err(format!("syntax: {e}")),
            Cell::Formula { expr: Ok(expr), .. } => expr,
        };
        if let Some(v) = self.state.values.get(&id) {
            return v.clone();
        }
        if !self.state.in_progress.insert(id) {
            return Value::err("circular reference");
        }
        let outer = self.current.replace(id);
        let outer_layer = std::mem::replace(&mut self.layer, z);
        let outer_pinned = std::mem::replace(&mut self.pinned, false);
        let mut v = self.eval(expr, id.0);
        self.pinned = outer_pinned;
        self.layer = outer_layer;
        self.current = outer;
        self.state.in_progress.remove(&id);

        // Implicit intersection: a column-sized vector yields this row's element,
        // so `=Amount * 1.19` works in every row.
        if let Value::Vector(items) = &v {
            v = if items.len() == self.sheets[id.0].cells.len() {
                items[id.1].clone()
            } else if items.len() == 1 {
                items[0].clone()
            } else {
                Value::err(format!("vector of length {} does not fit into a cell", items.len()))
            };
        }
        let v = conform(v, &self.sheets[id.0].columns[c].ty);
        self.state.values.insert(id, v.clone());
        v
    }

    /// A footer cell: its expression evaluated over the table; it must give one value.
    fn footer_cell(&mut self, id: CellId) -> Value {
        let k = id.1 - FOOTER_ROW;
        let (c, z) = split_layer(id.2);
        let expr = match self.sheets.get(id.0).and_then(|s| s.footer.get(k)).and_then(|f| f.cells.get(c)) {
            None => return Value::err("no such footer cell"),
            Some(Cell::Value(v)) => return v.clone(),
            Some(Cell::Formula { expr: Err(e), .. }) => return Value::err(format!("syntax: {e}")),
            Some(Cell::Formula { expr: Ok(e), .. }) => e,
        };
        if let Some(v) = self.state.values.get(&id) {
            return v.clone();
        }
        if !self.state.in_progress.insert(id) {
            return Value::err("circular reference");
        }
        let outer = self.current.replace(id);
        let outer_layer = std::mem::replace(&mut self.layer, z);
        let outer_pinned = std::mem::replace(&mut self.pinned, false);
        let v = match self.eval(expr, id.0) {
            Value::Vector(mut items) if items.len() == 1 => items.remove(0),
            Value::Vector(_) => Value::err("a footer needs one value – e.g. sum(Column)"),
            v => v,
        };
        self.pinned = outer_pinned;
        self.layer = outer_layer;
        self.current = outer;
        self.state.in_progress.remove(&id);
        self.state.values.insert(id, v.clone());
        v
    }

    fn column(&mut self, sheet: usize, col: usize) -> Value {
        let rows = self.sheets[sheet].cells.len();
        let col = in_layer(col, self.layer);
        Value::Vector((0..rows).map(|r| self.cell((sheet, r, col))).collect())
    }

    /// Evaluates `e` with references reading data layer `z`.
    fn in_layer(&mut self, e: &Expr, sheet: usize, z: usize) -> Value {
        let outer = std::mem::replace(&mut self.layer, z);
        let v = self.eval(e, sheet);
        self.layer = outer;
        v
    }

    /// The table a reference expression reads (for its layer names).
    fn sheet_of(&self, e: &Expr, sheet: usize) -> usize {
        match e {
            Expr::Remote(t, _) => self.find_sheet(t).unwrap_or(sheet),
            Expr::Layer(inner, _) | Expr::At(inner) => self.sheet_of(inner, sheet),
            _ => sheet,
        }
    }

    /// `ref.Name` / `ref[±n]`: the reference in another data layer. Beyond
    /// the stack (`[-1]` in layer a) it is empty.
    fn layer_ref(&mut self, inner: &Expr, sel: &LayerSel, sheet: usize) -> Value {
        let target = self.sheet_of(inner, sheet);
        let z = match sel {
            LayerSel::Name(name) => match self.sheets[target].layer_index(name) {
                Some(z) => z,
                None => return Value::err(format!("unknown layer '{name}'")),
            },
            LayerSel::Offset(n) => {
                let z = self.layer as i64 + n;
                if z < 0 || z as usize > self.sheets[target].layers.len() {
                    return Value::Null;
                }
                z as usize
            }
        };
        let outer = std::mem::replace(&mut self.pinned, true);
        let v = self.in_layer(inner, sheet, z);
        self.pinned = outer;
        v
    }

    /// The layer to read in table `s` from a formula of table `sheet`: the
    /// formula's own in its table, else layer a – unless the reference names
    /// the layer (`Plan!B2.February`).
    fn layer_for(&self, s: usize, sheet: usize) -> usize {
        if s == sheet || self.pinned { self.layer.min(self.sheets[s].layers.len()) } else { 0 }
    }

    /// A reference cell: (sheet, row, col, layer) of `B2`, `B2c`, `Table!B2`.
    fn ref_cell(&self, e: &Expr, sheet: usize) -> Option<(usize, usize, usize, usize)> {
        self.ref_cell_in(e, sheet, self.layer)
    }

    fn ref_cell_in(&self, e: &Expr, sheet: usize, z: usize) -> Option<(usize, usize, usize, usize)> {
        match e {
            Expr::Name(n) => parse_cell_ref(n).map(|(c, r, own)| (sheet, r, c, own.unwrap_or(z))),
            Expr::Remote(t, inner) => {
                let s = self.find_sheet(t)?;
                self.ref_cell_in(inner, s, self.layer_for(s, sheet))
            }
            _ => None,
        }
    }

    /// The cells of range `a:b` – with the short forms `B2:4`, `B2a:c`, `B2:4c`.
    fn range(&mut self, a: &str, b: &str, sheet: usize) -> Value {
        let (Some(start), Some((col, row, layer))) = (RefParts::parse(a), parse_partial(b)) else {
            return Value::err(format!("bad range {a}:{b}"));
        };
        let Some((r1, _)) = start.row else { return Value::err(format!("bad range {a}:{b}")) };
        let (c1, c2) = (start.col, col.map_or(start.col, |c| c.0));
        let r2 = row.map_or(r1, |r| r.0);
        // A layer at the end only applies to the start too (B2:4c = B2c:B4c).
        let z1 = start.layer.or(layer).map_or(self.layer, |z| z.0);
        let z2 = layer.or(start.layer).map_or(self.layer, |z| z.0);
        let mut items = Vec::new();
        for z in z1.min(z2)..=z1.max(z2) {
            for r in r1.min(r2)..=r1.max(r2) {
                for c in c1.min(c2)..=c1.max(c2) {
                    items.push(self.cell((sheet, r, in_layer(c, z))));
                }
            }
        }
        Value::Vector(items)
    }

    fn find_column(&self, sheet: usize, name: &str) -> Option<usize> {
        let cols = &self.sheets[sheet].columns;
        cols.iter().position(|c| c.name == name).or_else(|| letters_to_col(name).filter(|&c| c < cols.len()))
    }

    fn find_sheet(&self, name: &str) -> Option<usize> {
        self.sheets.iter().position(|s| s.name.as_deref() == Some(name))
    }

    fn eval(&mut self, e: &Expr, sheet: usize) -> Value {
        match e {
            Expr::Num(n) => Value::num(*n),
            Expr::Quantity(n, u) => match Unit::parse(u) {
                Some(u) => Value::Num(*n, u),
                None => Value::err(format!("bad unit '{u}'")),
            },
            Expr::Str(s) => Value::Text(s.clone()),
            Expr::Bool(b) => Value::Bool(*b),
            Expr::Name(name) if self.env.last().is_some_and(|e| e.contains_key(name)) => self.env.last().unwrap()[name].clone(),
            Expr::Name(name) => {
                if let Some(c) = self.sheets[sheet].columns.iter().position(|c| &c.name == name) {
                    self.column(sheet, c)
                } else if let Some((c, r, z)) = parse_cell_ref(name) {
                    self.cell((sheet, r, in_layer(c, z.unwrap_or(self.layer))))
                } else if let Some(c) = self.find_column(sheet, name) {
                    self.column(sheet, c)
                } else if let Some(r) = self.sheets[sheet].raw.iter().position(|row| row[0].trim() == name) {
                    // Row label: the value in the last column of the row named `name`.
                    self.cell((sheet, r, in_layer(self.sheets[sheet].columns.len() - 1, self.layer)))
                } else {
                    Value::err(format!("unknown name '{name}'"))
                }
            }
            Expr::Range(a, b) => self.range(a, b, sheet),
            Expr::Member(t, col) => match self.find_sheet(t) {
                // `Table.Column`, the old form of `Table!Column`.
                Some(s) => match self.find_column(s, col) {
                    Some(c) => {
                        let z = self.layer_for(s, sheet);
                        let outer = std::mem::replace(&mut self.layer, z);
                        let v = self.column(s, c);
                        self.layer = outer;
                        v
                    }
                    None => Value::err(format!("unknown column '{t}.{col}'")),
                },
                // `B2.February`, `Amount.February`: another data layer.
                None => self.layer_ref(&Expr::Name(t.clone()), &LayerSel::Name(col.clone()), sheet),
            },
            Expr::Layer(inner, sel) => self.layer_ref(inner, sel, sheet),
            Expr::Index(t, args) => self.index(t, args, sheet),
            Expr::Remote(t, inner) => match self.find_sheet(t) {
                Some(s) => {
                    let z = self.layer_for(s, sheet);
                    self.in_layer(inner, s, z)
                }
                None => Value::err(format!("unknown table '{t}'")),
            },
            Expr::At(inner) => {
                let Some((_, row, _)) = self.current.filter(|id| id.1 < FOOTER_ROW) else { return Value::err("@ needs the row of a cell") };
                match self.eval(inner, sheet) {
                    Value::Vector(items) => items.get(row).cloned().unwrap_or_else(|| Value::err(format!("row {} is beyond that column", row + 1))),
                    v => v,
                }
            }
            Expr::Neg(x) => {
                let v = self.eval(x, sheet);
                map1(v, &|v| binary(BinOp::Mul, v, Value::num(-1.0)))
            }
            Expr::Percent(x) => {
                let v = self.eval(x, sheet);
                map1(v, &|v| binary(BinOp::Div, v, Value::num(100.0)))
            }
            Expr::Binary(op, l, r) => {
                let l = self.eval(l, sheet);
                let r = self.eval(r, sheet);
                broadcast(*op, l, r)
            }
            // `offset(ref, rows, cols[, layers])` moves a reference (as in Excel, plus layers).
            Expr::Call(name, args) if name == "offset" && (3..=4).contains(&args.len()) && !self.functions.contains_key(name) => {
                let Some((s, r, c, z)) = self.ref_cell(&args[0], sheet) else {
                    return Value::err("offset needs a cell reference first, e.g. offset(B2, 1, 0)");
                };
                let mut moved = [r as i64, c as i64, z as i64];
                for (k, a) in args[1..].iter().enumerate() {
                    match self.eval(a, sheet) {
                        Value::Num(n, _) if n.fract() == 0.0 => moved[k] += n as i64,
                        Value::Null => {}
                        v if v.is_error() => return v,
                        _ => return Value::err("offset takes whole numbers"),
                    }
                }
                let [r, c, z] = moved;
                if r < 0 || c < 0 || z < 0 || r as usize >= self.sheets[s].cells.len() || c as usize >= self.sheets[s].columns.len() {
                    return Value::err("offset leaves the table");
                }
                self.cell((s, r as usize, in_layer(c as usize, z as usize)))
            }
            // `if` only evaluates the branch it takes (needed for recursive functions).
            Expr::Call(name, args) if (name == "if" || name == "wenn") && args.len() == 3 && !self.functions.contains_key(name) => {
                match self.eval(&args[0], sheet) {
                    Value::Vector(_) => {
                        let all: Vec<Value> = args.iter().map(|a| self.eval(a, sheet)).collect();
                        call(name, all)
                    }
                    c => match c.truthy() {
                        Ok(true) => self.eval(&args[1], sheet),
                        Ok(false) => self.eval(&args[2], sheet),
                        Err(e) => e,
                    },
                }
            }
            Expr::Call(name, args) => {
                let args: Vec<Value> = args.iter().map(|a| self.eval(a, sheet)).collect();
                match self.functions.get(name) {
                    Some(f) => self.call_user(name, f, args, sheet),
                    None => call(name, args),
                }
            }
        }
    }

    /// Calls a user function: parameters become local names in its body.
    fn call_user(&mut self, name: &str, f: &UserFn, args: Vec<Value>, sheet: usize) -> Value {
        if args.len() != f.params.len() {
            return Value::err(format!("{name} expects {} argument(s), got {}", f.params.len(), args.len()));
        }
        if self.env.len() >= MAX_CALL_DEPTH {
            return Value::err(format!("{name}: calls nested too deeply"));
        }
        self.env.push(f.params.iter().cloned().zip(args).collect());
        let v = self.eval(&f.body, sheet);
        self.env.pop();
        v
    }

    /// `Column[Label]`: the cell of column `col` in the footer row `Label`,
    /// else in the data row whose first cell is `Label`.
    fn row_of(&mut self, sheet: usize, col: usize, label: &str) -> Value {
        let s = &self.sheets[sheet];
        let col = in_layer(col, self.layer);
        if let Some(k) = s.footer.iter().position(|f| f.label == label) {
            return self.cell((sheet, FOOTER_ROW + k, col));
        }
        match s.raw.iter().position(|row| row.first().is_some_and(|x| x.trim() == label)) {
            Some(r) => self.cell((sheet, r, col)),
            None => Value::err(format!("no row '{label}'")),
        }
    }

    /// `Table[rowkey, col]` or `Table[Col=value, …]` (filter; yields the last
    /// column), or `Column[Label]` of table `sheet`.
    fn index(&mut self, t: &str, args: &[Expr], sheet: usize) -> Value {
        let Some(s) = self.find_sheet(t) else {
            return match (self.find_column(sheet, t), args) {
                (Some(c), [Expr::Name(label) | Expr::Str(label)]) => self.row_of(sheet, c, label),
                (Some(_), _) => Value::err(format!("expected {t}[Label] – the name of a row or footer row")),
                (None, _) => Value::err(format!("unknown table or column '{t}'")),
            };
        };
        let key_text = |e: &Expr| match e {
            Expr::Name(n) | Expr::Str(n) => Some(n.clone()),
            Expr::Num(n) => Some(Value::num(*n).display(None, false)),
            _ => None,
        };

        let is_filter = args.iter().all(|a| matches!(a, Expr::Binary(BinOp::Eq, ..)));
        if is_filter && !args.is_empty() {
            let mut conds = Vec::new();
            for a in args {
                let Expr::Binary(_, l, r) = a else { unreachable!() };
                let (Expr::Name(col), Some(want)) = (&**l, key_text(r)) else {
                    return Value::err("filter must look like Column=value");
                };
                let Some(c) = self.find_column(s, col) else { return Value::err(format!("unknown column '{col}'")) };
                conds.push((c, want));
            }
            let last = self.sheets[s].columns.len() - 1;
            let rows: Vec<usize> = (0..self.sheets[s].raw.len())
                .filter(|&r| conds.iter().all(|(c, want)| self.sheets[s].raw[r][*c].trim() == want))
                .collect();
            let last = in_layer(last, self.layer_for(s, sheet));
            return Value::Vector(rows.into_iter().map(|r| self.cell((s, r, last))).collect());
        }

        let [row, col] = args else { return Value::err("expected Table[row, column] or Table[Column=value]") };
        let row = match row {
            Expr::Num(n) if *n >= 1.0 => Some(*n as usize - 1),
            e => key_text(e).and_then(|k| self.sheets[s].raw.iter().position(|r| r.first().map(|x| x.trim()) == Some(&k))),
        };
        let col = match col {
            Expr::Name(n) | Expr::Str(n) => self.find_column(s, n),
            Expr::Num(n) if *n >= 1.0 => Some(*n as usize - 1),
            _ => None,
        };
        match (row, col) {
            (Some(r), Some(c)) => self.cell((s, r, in_layer(c, self.layer_for(s, sheet)))),
            _ => Value::err(format!("no such cell in '{t}'")),
        }
    }
}

/// Checks a formula result against the declared column type: units must match
/// and `decimal(n)` results are rounded to `n` places.
fn conform(v: Value, ty: &Type) -> Value {
    use crate::types::Base;
    let Value::Num(n, u) = v else { return v };
    if ty.base == Base::Auto {
        return Value::Num(n, u);
    }
    if u != ty.unit {
        return Value::err(format!("expected unit {}, got {}", unit_name(&ty.unit), unit_name(&u)));
    }
    match ty.base {
        Base::Decimal(d) => Value::Num(round_to(n, d as i32), u),
        Base::Int if n.fract() != 0.0 => Value::err(format!("not an integer: {n}")),
        _ => Value::Num(n, u),
    }
}

pub(crate) fn map1(v: Value, f: &dyn Fn(Value) -> Value) -> Value {
    match v {
        Value::Vector(items) => Value::Vector(items.into_iter().map(|x| map1(x, f)).collect()),
        v => f(v),
    }
}

fn broadcast(op: BinOp, l: Value, r: Value) -> Value {
    match (l, r) {
        (Value::Vector(a), Value::Vector(b)) => {
            if a.len() != b.len() {
                return Value::err(format!("vector lengths differ: {} vs {}", a.len(), b.len()));
            }
            Value::Vector(a.into_iter().zip(b).map(|(x, y)| broadcast(op, x, y)).collect())
        }
        (Value::Vector(a), s) => Value::Vector(a.into_iter().map(|x| broadcast(op, x, s.clone())).collect()),
        (s, Value::Vector(b)) => Value::Vector(b.into_iter().map(|y| broadcast(op, s.clone(), y)).collect()),
        (a, b) => binary(op, a, b),
    }
}

/// Scalar binary operation with unit checking.
pub(crate) fn binary(op: BinOp, l: Value, r: Value) -> Value {
    use Value::*;
    if let Error(_) = l {
        return l;
    }
    if let Error(_) = r {
        return r;
    }
    if op == BinOp::Concat {
        return Text(format!("{}{}", l.display(None, false), r.display(None, false)));
    }
    // Empty cells act as zero; for + - and comparisons in the unit of the other side.
    let additive = matches!(op, BinOp::Add | BinOp::Sub) || is_comparison(op);
    let zero_like = |u: &Unit| Num(0.0, if additive { u.clone() } else { Unit::none() });
    let (l, r) = match (l, r) {
        (Null, Num(n, u)) => (zero_like(&u), Num(n, u)),
        (Num(n, u), Null) => {
            let z = zero_like(&u);
            (Num(n, u), z)
        }
        (Null, Null) if !is_comparison(op) => (Value::num(0.0), Value::num(0.0)),
        other => other,
    };
    match (op, l, r) {
        (BinOp::Add | BinOp::Sub, Num(a, ua), Num(b, ub)) => {
            if ua != ub {
                return Value::err(format!("unit mismatch: {} vs {}", unit_name(&ua), unit_name(&ub)));
            }
            Num(if op == BinOp::Add { a + b } else { a - b }, ua)
        }
        (BinOp::Mul, Num(a, ua), Num(b, ub)) => Num(a * b, ua.mul(&ub)),
        (BinOp::Div, Num(_, _), Num(0.0, _)) => Value::err("division by zero"),
        (BinOp::Div, Num(a, ua), Num(b, ub)) => Num(a / b, ua.div(&ub)),
        (BinOp::Pow, Num(a, ua), Num(b, ub)) => {
            if !ub.is_none() {
                return Value::err("exponent must be dimensionless");
            }
            if !ua.is_none() && b.fract() != 0.0 {
                return Value::err("non-integer power of a unit");
            }
            Num(a.powf(b), ua.powi(b as i32))
        }
        (BinOp::Sub, Date(a), Date(b)) => Num((a - b) as f64, Unit::symbol("d")),
        (BinOp::Add | BinOp::Sub, Date(a), Num(n, u)) if u.is_none() || u == Unit::symbol("d") => {
            Date(if op == BinOp::Add { a + n as i64 } else { a - n as i64 })
        }
        (op, l, r) if is_comparison(op) => compare(op, l, r),
        (op, l, r) => Value::err(format!(
            "cannot apply {op:?} to {} and {}",
            l.display(None, false),
            r.display(None, false)
        )),
    }
}

fn is_comparison(op: BinOp) -> bool {
    matches!(op, BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge)
}

pub(crate) fn compare(op: BinOp, l: Value, r: Value) -> Value {
    use std::cmp::Ordering;
    let ord: Option<Ordering> = match (&l, &r) {
        (Value::Num(a, ua), Value::Num(b, ub)) => {
            if ua != ub {
                return Value::err(format!("unit mismatch: {} vs {}", unit_name(ua), unit_name(ub)));
            }
            a.partial_cmp(b)
        }
        (Value::Text(a), Value::Text(b)) => Some(a.cmp(b)),
        (Value::Bool(a), Value::Bool(b)) => Some(a.cmp(b)),
        (Value::Date(a), Value::Date(b)) => Some(a.cmp(b)),
        (Value::Null, Value::Null) => Some(Ordering::Equal),
        _ => None,
    };
    let Some(ord) = ord else {
        return match op {
            BinOp::Eq => Value::Bool(false),
            BinOp::Ne => Value::Bool(true),
            _ => Value::err("cannot compare values of different kinds"),
        };
    };
    Value::Bool(match op {
        BinOp::Eq => ord == Ordering::Equal,
        BinOp::Ne => ord != Ordering::Equal,
        BinOp::Lt => ord == Ordering::Less,
        BinOp::Le => ord != Ordering::Greater,
        BinOp::Gt => ord == Ordering::Greater,
        BinOp::Ge => ord != Ordering::Less,
        _ => unreachable!(),
    })
}

pub(crate) fn unit_name(u: &Unit) -> String {
    if u.is_none() { "no unit".into() } else { u.to_string() }
}

fn call(name: &str, args: Vec<Value>) -> Value {
    crate::builtins::call(name, args)
}
