//! Export to xlsx via rust_xlsxwriter.
//!
//! Every table becomes a sheet: header in row 1, data below. Formulas are
//! translated back to Excel where possible (the computed value is stored as
//! the cached result); otherwise the value is written and the hardcalc
//! formula becomes a note. Types become number formats, the `format` layer
//! cell formats, `comments` notes, enum types and `validate` rules data
//! validation, and charts native Excel charts.

use crate::chart::{self, ChartSpec, ChartType};
use crate::engine::{col_letters, Cell, Workbook};
use crate::formula::{parse_formula, BinOp, Expr};
use crate::layers::{self, Attached, CellStyle, Check, CmpOp, Formats, Target};
use crate::model::{Align, Block, Document, Rule};
use crate::types::Base;
use crate::value::{civil_from_days, Value};
use rust_xlsxwriter as xl;
use std::path::Path;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ExportReport {
    pub sheets: usize,
    pub formulas: usize,
    /// Formulas written as value (with the hardcalc formula as note).
    pub kept: usize,
    pub notes: usize,
    pub validations: usize,
    pub charts: usize,
    /// CSV/TSV: the files written (values only).
    pub files: Vec<std::path::PathBuf>,
    /// A TextBundle / TextPack was written (`files` holds it).
    pub bundle: bool,
}

impl std::fmt::Display for ExportReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.bundle {
            let names: Vec<String> = self.files.iter().map(|p| p.display().to_string()).collect();
            return write!(f, "document and tables written to {}", names.join(", "));
        }
        if !self.files.is_empty() {
            let names: Vec<String> = self.files.iter().map(|p| p.display().to_string()).collect();
            return write!(f, "values written to {}", names.join(", "));
        }
        write!(f, "{} sheet(s), {} formulas", self.sheets, self.formulas)?;
        if self.kept > 0 {
            write!(f, " ({} as value with note)", self.kept)?;
        }
        write!(f, ", {} notes, {} validations, {} charts", self.notes, self.validations, self.charts)
    }
}

/// Excel sheet names: at most 31 characters, none of `[]:*?/\`, unique.
fn sheet_names(wb: &Workbook) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for (i, s) in wb.sheets.iter().enumerate() {
        let base: String = s
            .name
            .clone()
            .unwrap_or_else(|| format!("Table {}", i + 1))
            .chars()
            .map(|c| if "[]:*?/\\".contains(c) { '_' } else { c })
            .take(31)
            .collect();
        let mut name = base.trim_matches('\'').to_string();
        let mut k = 2;
        while out.iter().any(|n| n.eq_ignore_ascii_case(&name)) {
            let suffix = format!(" {k}");
            name = format!("{}{suffix}", base.chars().take(31 - suffix.len()).collect::<String>());
            k += 1;
        }
        out.push(name);
    }
    out
}

fn quote_sheet(name: &str) -> String {
    format!("'{}'", name.replace('\'', "''"))
}

/// Context for translating the formulas of one table.
struct Translator<'a> {
    wb: &'a Workbook,
    names: &'a [String],
    sheet: usize,
    /// Nesting of inlined user functions (recursion cannot be inlined).
    depth: usize,
}

impl Translator<'_> {
    fn find_sheet(&self, name: &str) -> Result<usize, String> {
        self.wb.sheet_index(name).ok_or_else(|| format!("unknown table '{name}'"))
    }

    fn column(&self, sheet: usize, name: &str) -> Option<usize> {
        let cols = &self.wb.sheets[sheet].columns;
        cols.iter().position(|c| c.name == name).or_else(|| {
            if !name.is_empty() && name.chars().all(|c| c.is_ascii_uppercase()) {
                let i = name.chars().fold(0, |acc, c| acc * 26 + (c as usize - 'A' as usize + 1)) - 1;
                (i < cols.len()).then_some(i)
            } else {
                None
            }
        })
    }

    fn prefix(&self, sheet: usize) -> String {
        if sheet == self.sheet { String::new() } else { format!("{}!", quote_sheet(&self.names[sheet])) }
    }

    /// Excel address of a data cell (hardcalc rows exclude the header).
    fn cell(&self, sheet: usize, row: usize, col: usize) -> String {
        format!("{}{}{}", self.prefix(sheet), col_letters(col), row + 2)
    }

    fn column_range(&self, sheet: usize, col: usize) -> Result<String, String> {
        let rows = self.wb.sheets[sheet].cells.len();
        if rows == 0 {
            return Err("empty table".into());
        }
        let l = col_letters(col);
        Ok(format!("{}${l}$2:${l}${}", self.prefix(sheet), rows + 1))
    }

    /// `Column[Label]`: the cell in footer row `Label` (footer rows follow
    /// the data directly) or in the data row whose first cell is `Label`.
    fn row_of(&self, sheet: usize, col: &str, label: &str) -> Result<String, String> {
        let s = &self.wb.sheets[sheet];
        let c = self.column(sheet, col).ok_or_else(|| format!("unknown column '{col}'"))?;
        let r = match s.footer.iter().position(|f| f.label == label) {
            Some(k) => s.cells.len() + k,
            None => s.raw.iter().position(|row| row.first().is_some_and(|x| x.trim() == label)).ok_or("row not found")?,
        };
        Ok(self.cell(sheet, r, c))
    }

    /// `B2` / `$B$2` → (row, col).
    fn a1(s: &str) -> Option<(usize, usize)> {
        crate::engine::parse_a1(s).map(|(c, r)| (r, c))
    }

    /// `name` as seen from table `sheet`: column (whole, or this row's cell
    /// outside aggregates), cell, or row label.
    fn name(&self, sheet: usize, name: &str, row: usize, in_aggregate: bool) -> Result<String, String> {
        let s = &self.wb.sheets[sheet];
        if let Some(c) = s.columns.iter().position(|c| c.name == name) {
            return self.column_ref(sheet, c, row, in_aggregate);
        }
        if let Some((r, c)) = Self::a1(name) {
            return Ok(self.cell(sheet, r, c));
        }
        if let Some(c) = self.column(sheet, name) {
            return self.column_ref(sheet, c, row, in_aggregate);
        }
        if let Some(r) = s.raw.iter().position(|row| row[0].trim() == name) {
            return Ok(self.cell(sheet, r, s.columns.len() - 1));
        }
        Err(format!("unknown name '{name}'"))
    }

    /// A whole column inside aggregates; elsewhere hardcalc takes the cell
    /// of the formula's own row (implicit intersection).
    fn column_ref(&self, sheet: usize, col: usize, row: usize, in_aggregate: bool) -> Result<String, String> {
        if in_aggregate {
            self.column_range(sheet, col)
        } else if sheet == self.sheet {
            Ok(self.cell(sheet, row, col))
        } else {
            Err("a column of another table outside an aggregate".into())
        }
    }

    fn expr(&self, e: &Expr, row: usize, agg: bool) -> Result<String, String> {
        let wrap = |e: &Expr, s: String| if matches!(e, Expr::Binary(..)) { format!("({s})") } else { s };
        Ok(match e {
            Expr::Num(n) => format!("{n}"),
            Expr::Quantity(n, _) => format!("{n}"),
            Expr::Str(s) => format!("\"{}\"", s.replace('"', "\"\"")),
            Expr::Bool(b) => if *b { "TRUE" } else { "FALSE" }.into(),
            Expr::Name(n) => self.name(self.sheet, n, row, agg)?,
            // `@Col`: this row's cell, also inside aggregates.
            Expr::At(inner) => match &**inner {
                Expr::Name(n) => self.name(self.sheet, n, row, false)?,
                Expr::Member(t, col) => {
                    let s = self.find_sheet(t)?;
                    let c = self.column(s, col).ok_or_else(|| format!("unknown column '{t}.{col}'"))?;
                    self.cell(s, row, c)
                }
                Expr::Remote(t, inner) if matches!(&**inner, Expr::Name(_)) => {
                    let Expr::Name(col) = &**inner else { unreachable!() };
                    let s = self.find_sheet(t)?;
                    let c = self.column(s, col).ok_or_else(|| format!("unknown column '{t}!{col}'"))?;
                    self.cell(s, row, c)
                }
                _ => return Err("unsupported @ reference".into()),
            },
            Expr::Range(a, b) => {
                // Also the short forms within the table (B2:4, B2:D); not across layers.
                let (Some(a), Some((col, row, None))) = (Self::a1(a), crate::engine::parse_partial(b)) else {
                    return Err("bad range or one across data layers".into());
                };
                let (r2, c2) = (row.map_or(a.0, |r| r.0), col.map_or(a.1, |c| c.0));
                format!("{}:{}{}", self.cell(self.sheet, a.0, a.1), col_letters(c2), r2 + 2)
            }
            Expr::Member(t, _) if self.wb.sheet_index(t).is_none() => return Err("data layers are exported as values".into()),
            Expr::Layer(..) => return Err("data layers are exported as values".into()),
            Expr::Member(t, col) => {
                let s = self.find_sheet(t)?;
                let c = self.column(s, col).ok_or_else(|| format!("unknown column '{t}.{col}'"))?;
                self.column_range(s, c)?
            }
            Expr::Remote(t, inner) => {
                let s = self.find_sheet(t)?;
                let sub = Translator { sheet: s, ..*self };
                // Names inside are resolved in the other table but written with its prefix.
                match &**inner {
                    Expr::Name(n) => match sub.column(s, n) {
                        Some(c) if Self::a1(n).is_none() => {
                            let r = sub.column_range(s, c)?;
                            format!("{}{}", self.prefix(s), r)
                        }
                        _ => format!("{}{}", self.prefix(s), sub.name(s, n, row, true)?),
                    },
                    Expr::Range(a, b) => format!("{}{}", self.prefix(s), sub.expr(&Expr::Range(a.clone(), b.clone()), row, agg)?),
                    Expr::Index(col, args) => match args.as_slice() {
                        [Expr::Name(label) | Expr::Str(label)] => format!("{}{}", self.prefix(s), sub.row_of(s, col, label)?),
                        _ => return Err("unsupported remote reference".into()),
                    },
                    _ => return Err("unsupported remote reference".into()),
                }
            }
            Expr::Index(t, args) if self.wb.sheet_index(t).is_none() => match args.as_slice() {
                [Expr::Name(label) | Expr::Str(label)] => self.row_of(self.sheet, t, label)?,
                _ => return Err(format!("unknown table '{t}'")),
            },
            Expr::Index(t, args) => {
                let s = self.find_sheet(t)?;
                match args.as_slice() {
                    [r, c] if !matches!(r, Expr::Binary(BinOp::Eq, ..)) => {
                        let row = match r {
                            Expr::Num(n) if *n >= 1.0 => *n as usize - 1,
                            Expr::Name(k) | Expr::Str(k) => {
                                self.wb.sheets[s].raw.iter().position(|row| row[0].trim() == k).ok_or("row key not found")?
                            }
                            _ => return Err("unsupported row key".into()),
                        };
                        let col = match c {
                            Expr::Name(n) | Expr::Str(n) => self.column(s, n).ok_or("unknown column")?,
                            Expr::Num(n) if *n >= 1.0 => *n as usize - 1,
                            _ => return Err("unsupported column".into()),
                        };
                        self.cell(s, row, col)
                    }
                    _ => return Err("filters are only exported inside sum/avg/count".into()),
                }
            }
            Expr::Neg(x) => format!("-{}", wrap(x, self.expr(x, row, agg)?)),
            Expr::Percent(x) => format!("{}%", wrap(x, self.expr(x, row, agg)?)),
            Expr::Binary(op, l, r) => {
                let sym = match op {
                    BinOp::Add => "+",
                    BinOp::Sub => "-",
                    BinOp::Mul => "*",
                    BinOp::Div => "/",
                    BinOp::Pow => "^",
                    BinOp::Concat => "&",
                    BinOp::Eq => "=",
                    BinOp::Ne => "<>",
                    BinOp::Lt => "<",
                    BinOp::Le => "<=",
                    BinOp::Gt => ">",
                    BinOp::Ge => ">=",
                };
                format!("{}{sym}{}", wrap(l, self.expr(l, row, agg)?), wrap(r, self.expr(r, row, agg)?))
            }
            Expr::Call(name, args) => self.call(name, args, row)?,
        })
    }

    fn call(&self, name: &str, args: &[Expr], row: usize) -> Result<String, String> {
        // User functions are inlined: their body with the arguments put in.
        if let Some(f) = self.wb.functions.get(name) {
            if self.depth >= 16 || args.len() != f.params.len() {
                return Err(format!("{name} cannot be inlined"));
            }
            let body = crate::functions::substitute(&f.body, &f.params, args);
            let inner = Translator { depth: self.depth + 1, ..*self };
            return Ok(format!("({})", inner.expr(&body, row, false)?));
        }
        // sum/avg/count over a filtered tensor → SUMIFS / AVERAGEIFS / COUNTIFS.
        if let [Expr::Index(t, conds)] = args
            && !conds.is_empty()
            && conds.iter().all(|c| matches!(c, Expr::Binary(BinOp::Eq, ..)))
        {
            let s = self.find_sheet(t)?;
            let last = self.wb.sheets[s].columns.len() - 1;
            let mut parts = Vec::new();
            for c in conds {
                let Expr::Binary(_, l, r) = c else { unreachable!() };
                let (Expr::Name(col), value) = (&**l, &**r) else { return Err("unsupported filter".into()) };
                let col = self.column(s, col).ok_or("unknown filter column")?;
                let value = match value {
                    Expr::Name(v) | Expr::Str(v) => format!("\"{}\"", v.replace('"', "\"\"")),
                    Expr::Num(n) => format!("{n}"),
                    _ => return Err("unsupported filter value".into()),
                };
                parts.push(format!("{},{value}", self.column_range(s, col)?));
            }
            let values = self.column_range(s, last)?;
            return Ok(match name {
                "sum" | "summe" => format!("SUMIFS({values},{})", parts.join(",")),
                "mean" | "avg" | "average" | "mittelwert" => format!("AVERAGEIFS({values},{})", parts.join(",")),
                "count" | "anzahl" => format!("COUNTIFS({})", parts.join(",")),
                _ => return Err(format!("{name} over a filter")),
            });
        }
        let builtin = crate::builtins::find(name).ok_or_else(|| format!("unknown function {name}"))?;
        let excel = builtin.excel.ok_or_else(|| format!("function {name} has no Excel equivalent"))?;
        // Range arguments take whole columns, the others this row's cell.
        let mut parts: Vec<String> =
            args.iter().enumerate().map(|(i, a)| self.expr(a, row, builtin.range_arg(i))).collect::<Result<_, _>>()?;
        if excel == "ROUND" && parts.len() == 1 {
            parts.push("0".into());
        }
        Ok(format!("{excel}({})", parts.join(",")))
    }
}

/// Translates the hardcalc formula of cell (`sheet`, `row`) to Excel (with `=`).
pub fn formula_to_excel(wb: &Workbook, names: &[String], sheet: usize, row: usize, src: &str) -> Result<String, String> {
    let expr = parse_formula(src)?;
    Ok(format!("={}", Translator { wb, names, sheet, depth: 0 }.expr(&expr, row, false)?))
}

// ---------------------------------------------------------------- formats

fn rgb(c: &layers::Color) -> Option<u32> {
    let (r, g, b) = crate::print::print_color(c)?;
    Some((r as u32) << 16 | (g as u32) << 8 | b as u32)
}

/// Light background colours for `bg` (the print colours are for text).
fn bg_rgb(c: &layers::Color) -> Option<u32> {
    crate::print::print_bg(c).map(|(r, g, b)| (r as u32) << 16 | (g as u32) << 8 | b as u32)
}

/// Excel number format for a type: decimals and unit, e.g. `#,##0.00 "€"`.
fn type_format(ty: &crate::types::Type, unit: &crate::units::Unit, percent: Option<u8>, decimals: Option<u8>) -> Option<String> {
    let digits = |n: u8| if n == 0 { "#,##0".to_string() } else { format!("#,##0.{}", "0".repeat(n as usize)) };
    if let Some(p) = percent {
        return Some(if p == 0 { "0%".into() } else { format!("0.{}%", "0".repeat(p as usize)) });
    }
    let base = match (decimals, &ty.base) {
        (Some(d), _) | (None, &Base::Decimal(d)) => Some(digits(d)),
        (None, Base::Int) => Some("#,##0".into()),
        _ => None,
    };
    match (base, unit.is_none()) {
        (b, true) => b,
        (b, false) => Some(format!("{} \"{}\"", b.unwrap_or_else(|| "General".into()), unit.to_string().replace('"', ""))),
    }
}

fn cell_format(style: &CellStyle, number: Option<String>, header: bool) -> xl::Format {
    let mut f = xl::Format::new();
    if header || style.bold {
        f = f.set_bold();
    }
    if header {
        f = f.set_background_color(xl::Color::RGB(0xDDDDDD)).set_border_bottom(xl::FormatBorder::Thin);
    }
    if style.italic {
        f = f.set_italic();
    }
    if style.underline {
        f = f.set_underline(xl::FormatUnderline::Single);
    }
    if let Some(c) = style.fg.as_ref().and_then(rgb) {
        f = f.set_font_color(xl::Color::RGB(c));
    }
    if let Some(c) = style.bg.as_ref().and_then(bg_rgb) {
        f = f.set_background_color(xl::Color::RGB(c));
    }
    match style.align {
        Some(Align::Left) => f = f.set_align(xl::FormatAlign::Left),
        Some(Align::Right) => f = f.set_align(xl::FormatAlign::Right),
        Some(Align::Center) => f = f.set_align(xl::FormatAlign::Center),
        _ => {}
    }
    if let Some(n) = number {
        f = f.set_num_format(n);
    }
    f
}

// ---------------------------------------------------------------- export

fn err(e: xl::XlsxError) -> String {
    e.to_string()
}

/// Cells (data rows/cols, 0-based) a layer target covers in a table.
fn target_area(t: &Target, wb: &Workbook, sheet: usize) -> Option<(usize, usize, usize, usize)> {
    let s = &wb.sheets[sheet];
    let (rows, cols) = (s.cells.len(), s.columns.len());
    if rows == 0 {
        return None;
    }
    let col = |name: &str| s.columns.iter().position(|c| c.name == name).or_else(|| Translator::a1(&format!("{name}1")).map(|(_, c)| c));
    match t {
        Target::All => Some((0, 0, rows - 1, cols - 1)),
        Target::Column(n) => col(n).filter(|c| *c < cols).map(|c| (0, c, rows - 1, c)),
        Target::Row(n) => (*n >= 1 && *n <= rows).then(|| (n - 1, 0, n - 1, cols - 1)),
        Target::Cell(r, c) => (*r < rows && *c < cols).then_some((*r, *c, *r, *c)),
        Target::Range((c1, r1), (c2, r2)) => Some(((*r1).min(*r2), (*c1).min(*c2), (*r1).max(*r2).min(rows - 1), (*c1).max(*c2).min(cols - 1))),
        Target::RowWhere { .. } => None,
    }
}

/// A data validation in a writer-independent form.
#[derive(Debug, Clone, PartialEq)]
enum Validation {
    List(Vec<String>),
    Compare(CmpOp, f64),
}

fn validation(check: &Check) -> Option<Validation> {
    match check {
        Check::Compare(op, v) => Some(Validation::Compare(*op, crate::value::parse_number(v)?.0)),
        Check::OneOf(options) => Some(Validation::List(options.clone())),
        Check::Required | Check::Unique => None,
    }
}

/// Everything a spreadsheet writer needs about one cell.
#[derive(Debug, Clone)]
struct CellOut {
    value: Value,
    style: CellStyle,
    ty: crate::types::Type,
    unit: crate::units::Unit,
    /// Translated formula in Excel syntax (with `=`).
    formula: Option<String>,
    note: Option<String>,
}

/// A table prepared for writing; rows and columns as in the table (the
/// writers put the header in front).
struct SheetOut {
    name: String,
    headers: Vec<String>,
    cells: Vec<Vec<CellOut>>,
    /// Data area (rows, cols inclusive) and rule.
    validations: Vec<((usize, usize, usize, usize), Validation)>,
}

/// Computes all values and translates formulas, formats, comments and
/// validation rules – shared by the xlsx and ods writers.
fn prepare(doc: &Document) -> (Workbook, Vec<SheetOut>, ExportReport) {
    let (mut wb, _) = Workbook::from_document(doc);
    wb.eval_all();
    let attached: Vec<Attached> = layers::attach(doc, &wb);
    let formats = Formats::new(&attached, "format");
    let names = sheet_names(&wb);
    let mut report = ExportReport::default();
    let mut sheets = Vec::new();
    for s in 0..wb.sheets.len() {
        let (rows, cols) = (wb.sheets[s].cells.len(), wb.sheets[s].columns.len());
        let mut cells = Vec::with_capacity(rows);
        for r in 0..rows {
            let mut row = Vec::with_capacity(cols);
            for c in 0..cols {
                let id = (s, r, c);
                let value = wb.value(id);
                let style = formats.style(&mut wb, id);
                let ty = wb.sheets[s].columns[c].ty.clone();
                let unit = match &value {
                    Value::Num(_, u) => u.clone(),
                    _ => ty.unit.clone(),
                };
                let mut out = CellOut { value, style, ty, unit, formula: None, note: None };
                if let Cell::Formula { src, .. } = &wb.sheets[s].cells[r][c] {
                    let src = src.clone();
                    report.formulas += 1;
                    match formula_to_excel(&wb, &names, s, r, &src) {
                        // decimal(n) rounds the value itself in hardcalc; spreadsheets
                        // only format it – ROUND keeps both computing the same.
                        Ok(excel) => {
                            out.formula = Some(match out.ty.base {
                                Base::Decimal(n) => format!("=ROUND({},{n})", &excel[1..]),
                                _ => excel,
                            })
                        }
                        Err(_) => {
                            report.kept += 1;
                            out.note = Some(format!("hardcalc formula: {src}"));
                        }
                    }
                }
                row.push(out);
            }
            cells.push(row);
        }
        // Footer rows directly below the data, in bold; the label goes into the
        // first column (or, if that has a value, into a note there).
        for k in 0..wb.sheets[s].footer.len() {
            let label = wb.sheets[s].footer[k].label.clone();
            let mut row = Vec::with_capacity(cols);
            for c in 0..cols {
                let id = (s, crate::engine::FOOTER_ROW + k, c);
                let value = wb.value(id);
                // Formatted like the column only if it is an amount of it (not a count).
                let ty = match &value {
                    Value::Num(_, u) if *u != wb.sheets[s].columns[c].ty.unit => crate::types::Type::auto(),
                    _ => wb.sheets[s].columns[c].ty.clone(),
                };
                let unit = match &value {
                    Value::Num(_, u) => u.clone(),
                    _ => ty.unit.clone(),
                };
                let style = CellStyle { bold: true, ..CellStyle::default() };
                let mut out = CellOut { value, style, ty, unit, formula: None, note: None };
                if let Some(Cell::Formula { src, expr }) = wb.sheets[s].footer[k].cells.get(c) {
                    report.formulas += 1;
                    let excel = match expr {
                        Ok(expr) => (Translator { wb: &wb, names: &names, sheet: s, depth: 0 }).expr(expr, rows + k, false),
                        Err(e) => Err(e.clone()),
                    };
                    match excel {
                        Ok(excel) => out.formula = Some(format!("={excel}")),
                        Err(_) => {
                            report.kept += 1;
                            out.note = Some(format!("hardcalc formula: {src}"));
                        }
                    }
                }
                row.push(out);
            }
            if let Some(first) = row.first_mut() {
                if matches!(first.value, Value::Null) && first.formula.is_none() {
                    first.value = Value::Text(label);
                } else {
                    first.note = Some(match first.note.take() {
                        Some(n) => format!("{label}\n{n}"),
                        None => label,
                    });
                }
            }
            cells.push(row);
        }
        // Comments → notes on the first cell a target covers.
        for a in attached.iter().filter(|a| a.sheet == s && a.layer.name == "comments") {
            for rule in &a.layer.rules {
                let Rule::Rule { target, body } = rule else { continue };
                if let Some((r, c, _, _)) = target_area(&Target::parse(target), &wb, s) {
                    let note = &mut cells[r][c].note;
                    *note = Some(match note.take() {
                        Some(prev) => format!("{prev}\n{body}"),
                        None => body.clone(),
                    });
                }
            }
        }
        report.notes += cells.iter().flatten().filter(|c| c.note.is_some()).count();
        // enum types → lists; validate rules → comparisons and lists.
        let mut validations = Vec::new();
        for c in 0..cols {
            if let Base::Enum(options) = &wb.sheets[s].columns[c].ty.base
                && rows > 0
            {
                validations.push(((0, c, rows - 1, c), Validation::List(options.clone())));
            }
        }
        for a in attached.iter().filter(|a| a.sheet == s && a.layer.name == "validate") {
            for rule in &a.layer.rules {
                let Rule::Rule { target, body } = rule else { continue };
                let Some(area) = target_area(&Target::parse(target), &wb, s) else { continue };
                for check in Check::parse_all(body).unwrap_or_default() {
                    if let Some(v) = validation(&check) {
                        validations.push((area, v));
                    }
                }
            }
        }
        report.validations += validations.len();
        let headers = wb.sheets[s].columns.iter().map(|c| c.name.clone()).collect();
        sheets.push(SheetOut { name: names[s].clone(), headers, cells, validations });
        report.sheets += 1;
    }
    (wb, sheets, report)
}

fn xl_validation(v: &Validation) -> Result<xl::DataValidation, xl::XlsxError> {
    Ok(match v {
        Validation::List(options) => xl::DataValidation::new().allow_list_strings(options)?,
        Validation::Compare(op, n) => xl::DataValidation::new().allow_decimal_number(match op {
            CmpOp::Eq => xl::DataValidationRule::EqualTo(*n),
            CmpOp::Ne => xl::DataValidationRule::NotEqualTo(*n),
            CmpOp::Lt => xl::DataValidationRule::LessThan(*n),
            CmpOp::Le => xl::DataValidationRule::LessThanOrEqualTo(*n),
            CmpOp::Gt => xl::DataValidationRule::GreaterThan(*n),
            CmpOp::Ge => xl::DataValidationRule::GreaterThanOrEqualTo(*n),
        }),
    })
}

/// Writes `doc` in the format given by the file extension.
pub fn export_file(doc: &Document, path: &Path) -> Result<ExportReport, String> {
    match path.extension().map(|e| e.to_string_lossy().to_lowercase()).as_deref() {
        Some("xlsx") => export_xlsx(doc, path),
        Some("ods") => export_ods(doc, path),
        Some("csv" | "tsv") => {
            let files = crate::delimited::export(doc, path, None)?;
            Ok(ExportReport { files, ..ExportReport::default() })
        }
        // A TextPack for passing on is marked transient (only for the exchange).
        Some(ext @ ("textbundle" | "textpack" | "hcbundle" | "hcpack")) => {
            let kind = crate::bundle::Kind::parse(ext).ok_or("unknown bundle")?;
            let bundle = crate::bundle::build(doc, None, kind, kind == crate::bundle::Kind::TextPack);
            bundle.write(path, kind).map_err(|e| format!("{}: {e}", path.display()))?;
            Ok(ExportReport { files: vec![path.to_path_buf()], bundle: true, ..ExportReport::default() })
        }
        _ => Err(format!("{}: export supports .xlsx, .ods, .csv, .tsv, .textbundle, .textpack, .hcbundle and .hcpack", path.display())),
    }
}

/// Writes `doc` as an xlsx workbook.
pub fn export_xlsx(doc: &Document, path: &Path) -> Result<ExportReport, String> {
    let (mut wb, sheets, mut report) = prepare(doc);
    let names: Vec<String> = sheets.iter().map(|s| s.name.clone()).collect();
    let comma = wb.decimal_comma;
    let mut book = xl::Workbook::new();
    for sheet in &sheets {
        let ws = book.add_worksheet();
        ws.set_name(&sheet.name).map_err(err)?;
        for (c, h) in sheet.headers.iter().enumerate() {
            ws.write_string_with_format(0, c as u16, h, &cell_format(&CellStyle::default(), None, true)).map_err(err)?;
        }
        for (r, row) in sheet.cells.iter().enumerate() {
            for (c, cell) in row.iter().enumerate() {
                let (xr, xc) = (r as u32 + 1, c as u16);
                let number = type_format(&cell.ty, &cell.unit, cell.style.percent, cell.style.decimals);
                let format = cell_format(&cell.style, number, false);
                if let Some(note) = &cell.note {
                    ws.insert_note(xr, xc, &xl::Note::new(note).set_author("hardcalc")).map_err(err)?;
                }
                if let Some(f) = &cell.formula {
                    let result = match &cell.value {
                        Value::Num(n, _) => format!("{n}"),
                        other => other.display(None, false),
                    };
                    ws.write_formula_with_format(xr, xc, xl::Formula::new(f).set_result(result), &format).map_err(err)?;
                    continue;
                }
                match &cell.value {
                    Value::Null => {}
                    Value::Num(n, _) => {
                        ws.write_number_with_format(xr, xc, *n, &format).map_err(err)?;
                    }
                    Value::Bool(b) => {
                        ws.write_boolean_with_format(xr, xc, *b, &format).map_err(err)?;
                    }
                    Value::Date(d) => {
                        let (y, m, day) = civil_from_days(*d);
                        let date_format = if comma { "dd.mm.yyyy" } else { "yyyy-mm-dd" };
                        let dt = xl::ExcelDateTime::from_ymd(y as u16, m as u8, day as u8).map_err(err)?;
                        ws.write_datetime_with_format(xr, xc, dt, &format.clone().set_num_format(date_format)).map_err(err)?;
                    }
                    Value::Text(t) => {
                        ws.write_string_with_format(xr, xc, t, &format).map_err(err)?;
                    }
                    Value::Error(e) => {
                        ws.write_string_with_format(xr, xc, format!("#ERR: {e}"), &format).map_err(err)?;
                    }
                    v @ Value::Vector(_) => {
                        ws.write_string(xr, xc, v.display(None, comma)).map_err(err)?;
                    }
                }
            }
        }
        for ((r0, c0, r1, c1), v) in &sheet.validations {
            ws.add_data_validation(*r0 as u32 + 1, *c0 as u16, *r1 as u32 + 1, *c1 as u16, &xl_validation(v).map_err(err)?).map_err(err)?;
        }
        ws.set_freeze_panes(1, 0).map_err(err)?;
        ws.autofit();
    }

    // Charts: next to their table; tensor and histogram charts get their data
    // on a sheet of their own, because the values are aggregated.
    let charts: Vec<(String, Vec<String>)> = doc
        .blocks
        .iter()
        .filter_map(|b| if let Block::Chart { name, lines } = b { Some((name.clone(), lines.clone())) } else { None })
        .collect();
    let mut placed_in_sheet = vec![0u32; wb.sheets.len()];
    for (name, lines) in charts {
        let Ok(spec) = ChartSpec::parse(&name, &lines) else { continue };
        let Ok(data) = chart::resolve(&spec, &mut wb) else { continue };
        let Some(kind) = chart_type(data.kind) else { continue };
        let mut xc = xl::Chart::new(kind);
        xc.title().set_name(&data.title);
        let host = data.sheet;
        let columns: Option<Vec<usize>> = spec.data.iter().map(|d| d.split_once('.').and_then(|(_, c)| wb.sheets[host].columns.iter().position(|x| x.name == c.trim()))).collect();
        match columns.filter(|_| data.rows_are_categories && data.xs.is_none()) {
            Some(cols) => {
                let rows = wb.sheets[host].cells.len() as u32;
                let label = spec.labels.as_ref().and_then(|l| l.split_once('.')).and_then(|(_, c)| wb.sheets[host].columns.iter().position(|x| x.name == c.trim()));
                for (k, c) in cols.iter().enumerate() {
                    let series = xc.add_series();
                    series.set_values((names[host].as_str(), 1, *c as u16, rows, *c as u16)).set_name(data.series[k].name.as_str());
                    if let Some(l) = label {
                        series.set_categories((names[host].as_str(), 1, l as u16, rows, l as u16));
                    }
                }
            }
            None => {
                // Aggregated data (tensor, histogram, scatter): a sheet with the values.
                let sheet = format!("{} data", name.chars().take(26).collect::<String>());
                let ds = book.add_worksheet();
                ds.set_name(&sheet).map_err(err)?;
                ds.write_string(0, 0, "").map_err(err)?;
                for (i, cat) in data.categories.iter().enumerate() {
                    match &data.xs {
                        Some(xs) => ds.write_number(i as u32 + 1, 0, xs[i]).map_err(err)?,
                        None => ds.write_string(i as u32 + 1, 0, cat).map_err(err)?,
                    };
                }
                for (k, s) in data.series.iter().enumerate() {
                    ds.write_string(0, k as u16 + 1, &s.name).map_err(err)?;
                    for (i, v) in s.values.iter().enumerate() {
                        if let Some(v) = v {
                            ds.write_number(i as u32 + 1, k as u16 + 1, *v).map_err(err)?;
                        }
                    }
                }
                let n = data.categories.len() as u32;
                for (k, s) in data.series.iter().enumerate() {
                    xc.add_series()
                        .set_values((sheet.as_str(), 1, k as u16 + 1, n, k as u16 + 1))
                        .set_categories((sheet.as_str(), 1, 0, n, 0))
                        .set_name(s.name.as_str());
                }
            }
        }
        let ws = book.worksheet_from_name(&names[host]).map_err(err)?;
        let col = wb.sheets[host].columns.len() as u16 + 1;
        ws.insert_chart(1 + placed_in_sheet[host] * 16, col, &xc).map_err(err)?;
        placed_in_sheet[host] += 1;
        report.charts += 1;
    }
    book.save(path).map_err(err)?;
    Ok(report)
}

fn chart_type(kind: ChartType) -> Option<xl::ChartType> {
    Some(match kind {
        ChartType::Bar | ChartType::Histogram => xl::ChartType::Bar,
        ChartType::Line => xl::ChartType::Line,
        ChartType::Area => xl::ChartType::Area,
        ChartType::Scatter => xl::ChartType::Scatter,
    })
}

// ---------------------------------------------------------------- ods

/// Excel formula (with `=`) → OpenFormula as used in ODS (`of:=SUM([.B2:.B4])`).
pub fn excel_to_odf(excel: &str) -> String {
    let body = excel.strip_prefix('=').unwrap_or(excel);
    let chars: Vec<char> = body.chars().collect();
    let is_ref = |s: &str| {
        let t = s.replace('$', "");
        let split = t.find(|c: char| c.is_ascii_digit()).unwrap_or(t.len());
        let (l, d) = t.split_at(split);
        !l.is_empty() && l.len() <= 3 && l.chars().all(|c| c.is_ascii_uppercase()) && !d.is_empty() && d.chars().all(|c| c.is_ascii_digit())
    };
    let word = |i: usize| {
        let mut j = i;
        while j < chars.len() && (chars[j].is_alphanumeric() || chars[j] == '$' || chars[j] == '_' || chars[j] == '.') {
            j += 1;
        }
        (chars[i..j].iter().collect::<String>(), j)
    };
    // A reference (cell or range) starting at i, written as [sheet.a:.b].
    let reference = |sheet: Option<String>, i: usize| -> Option<(String, usize)> {
        let (a, j) = word(i);
        if !is_ref(&a) {
            return None;
        }
        let prefix = sheet.map(|s| format!("$'{}'", s.replace('\'', "''"))).unwrap_or_default();
        if chars.get(j) == Some(&':') {
            let (b, k) = word(j + 1);
            if is_ref(&b) {
                return Some((format!("[{prefix}.{a}:.{b}]"), k));
            }
        }
        Some((format!("[{prefix}.{a}]"), j))
    };
    let mut out = String::from("of:=");
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '"' {
            out.push(c);
            i += 1;
            while i < chars.len() {
                out.push(chars[i]);
                if chars[i] == '"' && chars.get(i + 1) != Some(&'"') {
                    i += 1;
                    break;
                }
                if chars[i] == '"' {
                    out.push('"');
                    i += 1;
                }
                i += 1;
            }
        } else if c == '\'' {
            // 'Sheet'!ref
            let mut j = i + 1;
            let mut name = String::new();
            while j < chars.len() {
                if chars[j] == '\'' && chars.get(j + 1) == Some(&'\'') {
                    name.push('\'');
                    j += 2;
                } else if chars[j] == '\'' {
                    break;
                } else {
                    name.push(chars[j]);
                    j += 1;
                }
            }
            match reference(Some(name), j + 2) {
                Some((r, end)) if chars.get(j + 1) == Some(&'!') => {
                    out.push_str(&r);
                    i = end;
                }
                _ => {
                    out.push(c);
                    i += 1;
                }
            }
        } else if c.is_ascii_alphabetic() || c == '$' {
            let (w, j) = word(i);
            if chars.get(j) == Some(&'(') || !is_ref(&w) {
                out.push_str(&w);
                i = j;
            } else {
                let (r, end) = reference(None, i).unwrap();
                out.push_str(&r);
                i = end;
            }
        } else if c == ',' {
            out.push(';');
            i += 1;
        } else {
            out.push(c);
            i += 1;
        }
    }
    out
}

fn xml(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn hex(c: u32) -> String {
    format!("#{c:06x}")
}

/// Collects automatic number and cell styles, deduplicated by their XML.
#[derive(Default)]
struct OdsStyles {
    number: Vec<String>,
    cell: Vec<String>,
}

impl OdsStyles {
    fn intern(list: &mut Vec<String>, prefix: &str, body: String) -> String {
        let pos = list.iter().position(|b| *b == body).unwrap_or_else(|| {
            list.push(body);
            list.len() - 1
        });
        format!("{prefix}{}", pos + 1)
    }

    /// Number style for a cell: decimals, unit, percent or date.
    fn number(&mut self, cell: &CellOut, comma_dates: bool) -> Option<String> {
        let digits = |d: u8, grouping: bool| {
            format!(
                "<number:number number:decimal-places=\"{d}\" number:min-decimal-places=\"{d}\" number:min-integer-digits=\"1\"{}/>",
                if grouping { " number:grouping=\"true\"" } else { "" }
            )
        };
        let body = match &cell.value {
            Value::Date(_) => {
                let (a, sep, b, c) = if comma_dates { ("day", ".", "month", "year") } else { ("year", "-", "month", "day") };
                format!(
                    "number:date-style§<number:{a} number:style=\"long\"/><number:text>{sep}</number:text><number:{b} number:style=\"long\"/><number:text>{sep}</number:text><number:{c} number:style=\"long\"/>"
                )
            }
            Value::Num(..) => {
                if let Some(p) = cell.style.percent {
                    format!("number:percentage-style§{}<number:text>%</number:text>", digits(p, false))
                } else {
                    let decimals = cell.style.decimals.or(match cell.ty.base {
                        Base::Decimal(d) => Some(d),
                        Base::Int => Some(0),
                        _ => None,
                    });
                    if decimals.is_none() && cell.unit.is_none() {
                        return None;
                    }
                    let number = match decimals {
                        Some(d) => digits(d, true),
                        None => "<number:number number:min-integer-digits=\"1\"/>".into(),
                    };
                    let unit = if cell.unit.is_none() { String::new() } else { format!("<number:text> {}</number:text>", xml(&cell.unit.to_string())) };
                    format!("number:number-style§{number}{unit}")
                }
            }
            _ => return None,
        };
        Some(Self::intern(&mut self.number, "N", body))
    }

    fn cell(&mut self, style: &CellStyle, number: Option<String>, header: bool) -> Option<String> {
        let mut cell_props = String::new();
        let mut text_props = String::new();
        let mut para = String::new();
        if header || style.bold {
            text_props.push_str(" fo:font-weight=\"bold\"");
        }
        if style.italic {
            text_props.push_str(" fo:font-style=\"italic\"");
        }
        if style.underline {
            text_props.push_str(" style:text-underline-style=\"solid\" style:text-underline-width=\"auto\" style:text-underline-color=\"font-color\"");
        }
        if let Some(c) = style.fg.as_ref().and_then(rgb) {
            text_props.push_str(&format!(" fo:color=\"{}\"", hex(c)));
        }
        let bg = if header { Some(0xDDDDDD) } else { style.bg.as_ref().and_then(bg_rgb) };
        if let Some(c) = bg {
            cell_props.push_str(&format!(" fo:background-color=\"{}\"", hex(c)));
        }
        match style.align {
            Some(Align::Left) => para.push_str(" fo:text-align=\"start\""),
            Some(Align::Right) => para.push_str(" fo:text-align=\"end\""),
            Some(Align::Center) => para.push_str(" fo:text-align=\"center\""),
            _ => {}
        }
        if cell_props.is_empty() && text_props.is_empty() && para.is_empty() && number.is_none() {
            return None;
        }
        let mut body = String::new();
        if let Some(n) = &number {
            body.push_str(&format!(" style:data-style-name=\"{n}\""));
        }
        body.push('§');
        if !cell_props.is_empty() {
            body.push_str(&format!("<style:table-cell-properties{cell_props}/>"));
        }
        if !para.is_empty() {
            body.push_str(&format!("<style:paragraph-properties{para}/>"));
        }
        if !text_props.is_empty() {
            body.push_str(&format!("<style:text-properties{text_props}/>"));
        }
        Some(Self::intern(&mut self.cell, "ce", body))
    }

    fn xml(&self) -> String {
        let mut out = String::new();
        for (i, b) in self.number.iter().enumerate() {
            let (element, inner) = b.split_once('§').unwrap();
            out.push_str(&format!("<{element} style:name=\"N{}\">{inner}</{element}>", i + 1));
        }
        for (i, b) in self.cell.iter().enumerate() {
            let (attrs, inner) = b.split_once('§').unwrap();
            out.push_str(&format!("<style:style style:name=\"ce{}\" style:family=\"table-cell\"{attrs}>{inner}</style:style>", i + 1));
        }
        out
    }
}

/// `'Table 4'.B2` – a cell address for ODS attributes.
fn ods_address(sheet: &str, row: usize, col: usize) -> String {
    format!("'{}'.{}{}", sheet.replace('\'', "''"), col_letters(col), row + 1)
}

/// Writes `doc` as an OpenDocument spreadsheet (charts are not exported).
pub fn export_ods(doc: &Document, path: &Path) -> Result<ExportReport, String> {
    use std::io::Write;
    let (wb, sheets, report) = prepare(doc);
    let comma = wb.decimal_comma;
    let mut styles = OdsStyles::default();
    let mut validations = String::new();
    let mut tables = String::new();
    let mut count = 0;
    for sheet in &sheets {
        // Validation names per cell (later rules win, as one cell has one validation).
        let mut cell_validation: std::collections::HashMap<(usize, usize), String> = Default::default();
        for ((r0, c0, r1, c1), v) in &sheet.validations {
            count += 1;
            let name = format!("val{count}");
            let condition = match v {
                Validation::List(options) => {
                    let items: Vec<String> = options.iter().map(|o| format!("\"{}\"", o.replace('"', "\"\""))).collect();
                    format!("of:cell-content-is-in-list({})", items.join(";"))
                }
                Validation::Compare(op, n) => {
                    let sym = match op {
                        CmpOp::Eq => "=",
                        CmpOp::Ne => "!=",
                        CmpOp::Lt => "<",
                        CmpOp::Le => "<=",
                        CmpOp::Gt => ">",
                        CmpOp::Ge => ">=",
                    };
                    format!("of:cell-content-is-decimal-number() and cell-content(){sym}{n}")
                }
            };
            validations.push_str(&format!(
                "<table:content-validation table:name=\"{name}\" table:condition=\"{}\" table:allow-empty-cell=\"true\" table:base-cell-address=\"{}\"/>",
                xml(&condition),
                xml(&ods_address(&sheet.name, r0 + 1, *c0))
            ));
            for r in *r0..=*r1 {
                for c in *c0..=*c1 {
                    cell_validation.insert((r, c), name.clone());
                }
            }
        }

        tables.push_str(&format!("<table:table table:name=\"{}\">", xml(&sheet.name)));
        tables.push_str(&format!("<table:table-column table:number-columns-repeated=\"{}\"/>", sheet.headers.len().max(1)));
        let header_style = styles.cell(&CellStyle::default(), None, true).unwrap();
        tables.push_str("<table:table-row>");
        for h in &sheet.headers {
            tables.push_str(&format!("<table:table-cell table:style-name=\"{header_style}\" office:value-type=\"string\"><text:p>{}</text:p></table:table-cell>", xml(h)));
        }
        tables.push_str("</table:table-row>");
        for (r, row) in sheet.cells.iter().enumerate() {
            tables.push_str("<table:table-row>");
            for (c, cell) in row.iter().enumerate() {
                let number = styles.number(cell, comma);
                let mut attrs = String::new();
                if let Some(st) = styles.cell(&cell.style, number, false) {
                    attrs.push_str(&format!(" table:style-name=\"{st}\""));
                }
                if let Some(v) = cell_validation.get(&(r, c)) {
                    attrs.push_str(&format!(" table:content-validation-name=\"{v}\""));
                }
                if let Some(f) = &cell.formula {
                    attrs.push_str(&format!(" table:formula=\"{}\"", xml(&excel_to_odf(f))));
                }
                let display = |v: &Value| v.display(cell.ty.decimals(), comma);
                let (typed, text) = match &cell.value {
                    Value::Null => (String::new(), String::new()),
                    Value::Num(n, _) if cell.style.percent.is_some() => {
                        (format!(" office:value-type=\"percentage\" office:value=\"{n}\""), cell.style.display(&cell.value, comma).unwrap_or_default())
                    }
                    Value::Num(n, _) => (format!(" office:value-type=\"float\" office:value=\"{n}\""), display(&cell.value)),
                    Value::Bool(b) => (format!(" office:value-type=\"boolean\" office:boolean-value=\"{b}\""), b.to_string()),
                    Value::Date(d) => {
                        let (y, m, day) = civil_from_days(*d);
                        (format!(" office:value-type=\"date\" office:date-value=\"{y:04}-{m:02}-{day:02}\""), display(&cell.value))
                    }
                    other => (" office:value-type=\"string\"".to_string(), display(other)),
                };
                tables.push_str(&format!("<table:table-cell{attrs}{typed}>"));
                if let Some(note) = &cell.note {
                    tables.push_str("<office:annotation><dc:creator>hardcalc</dc:creator>");
                    for line in note.lines() {
                        tables.push_str(&format!("<text:p>{}</text:p>", xml(line)));
                    }
                    tables.push_str("</office:annotation>");
                }
                if !text.is_empty() {
                    tables.push_str(&format!("<text:p>{}</text:p>", xml(&text)));
                }
                tables.push_str("</table:table-cell>");
            }
            tables.push_str("</table:table-row>");
        }
        tables.push_str("</table:table>");
    }

    const NS: &str = "xmlns:office=\"urn:oasis:names:tc:opendocument:xmlns:office:1.0\" \
        xmlns:style=\"urn:oasis:names:tc:opendocument:xmlns:style:1.0\" \
        xmlns:text=\"urn:oasis:names:tc:opendocument:xmlns:text:1.0\" \
        xmlns:table=\"urn:oasis:names:tc:opendocument:xmlns:table:1.0\" \
        xmlns:number=\"urn:oasis:names:tc:opendocument:xmlns:datastyle:1.0\" \
        xmlns:fo=\"urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0\" \
        xmlns:dc=\"http://purl.org/dc/elements/1.1/\" \
        xmlns:of=\"urn:oasis:names:tc:opendocument:xmlns:of:1.2\" office:version=\"1.2\"";
    let validations = if validations.is_empty() { String::new() } else { format!("<table:content-validations>{validations}</table:content-validations>") };
    let content = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<office:document-content {NS}><office:automatic-styles>{}</office:automatic-styles>\
         <office:body><office:spreadsheet>{validations}{tables}</office:spreadsheet></office:body></office:document-content>",
        styles.xml()
    );
    let styles_xml = format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<office:document-styles {NS}/>");
    let manifest = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<manifest:manifest xmlns:manifest=\"urn:oasis:names:tc:opendocument:xmlns:manifest:1.0\" manifest:version=\"1.2\">\
        <manifest:file-entry manifest:full-path=\"/\" manifest:version=\"1.2\" manifest:media-type=\"application/vnd.oasis.opendocument.spreadsheet\"/>\
        <manifest:file-entry manifest:full-path=\"content.xml\" manifest:media-type=\"text/xml\"/>\
        <manifest:file-entry manifest:full-path=\"styles.xml\" manifest:media-type=\"text/xml\"/>\
        </manifest:manifest>";

    let file = std::fs::File::create(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut zip = zip::ZipWriter::new(file);
    let stored = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let deflated = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let io = |e: std::io::Error| e.to_string();
    // The mimetype must come first and uncompressed.
    zip.start_file("mimetype", stored).map_err(|e| e.to_string())?;
    zip.write_all(b"application/vnd.oasis.opendocument.spreadsheet").map_err(io)?;
    for (name, data) in [("META-INF/manifest.xml", manifest.to_string()), ("styles.xml", styles_xml), ("content.xml", content)] {
        zip.start_file(name, deflated).map_err(|e| e.to_string())?;
        zip.write_all(data.as_bytes()).map_err(io)?;
    }
    zip.finish().map_err(|e| e.to_string())?;
    Ok(report)
}
