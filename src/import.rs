//! Import of spreadsheet files (xlsx, xlsm, xlsb, xls, ods) via calamine.
//!
//! Every non-empty sheet becomes a table: the first non-empty row is the
//! header (unless it holds numbers only), the rows below are data. Formulas
//! are translated to hardcalc where possible; otherwise the cell keeps the
//! value last computed by the spreadsheet and the original formula goes into
//! the `comments` layer.

use crate::engine::col_letters;
use crate::model::{Align, Block, Column, Document, Layer, Rule, Shape, Table};
use crate::value::civil_from_days;
use calamine::{open_workbook_auto, Data, Range, Reader};
use std::collections::HashMap;
use std::path::Path;

/// What happened during an import.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ImportReport {
    pub tables: usize,
    pub cells: usize,
    pub formulas: usize,
    /// Formulas kept as value + comment.
    pub kept: usize,
}

impl std::fmt::Display for ImportReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} table(s), {} cells, {} formulas", self.tables, self.cells, self.formulas)?;
        if self.kept > 0 {
            write!(f, " ({} kept as value – original formula in the comments layer)", self.kept)?;
        }
        Ok(())
    }
}

/// Where a sheet's table lies in the sheet (0-based absolute positions).
#[derive(Debug, Clone)]
struct Geometry {
    name: String,
    /// First data row; the header (if any) is the row above.
    data_row: u32,
    rows: u32,
    first_col: u32,
    cols: u32,
}

impl Geometry {
    /// hardcalc A1 address of an absolute sheet position inside the table.
    fn cell(&self, row: u32, col: u32) -> Option<String> {
        let r = row.checked_sub(self.data_row)?;
        let c = col.checked_sub(self.first_col)?;
        (r < self.rows && c < self.cols).then(|| format!("{}{}", col_letters(c as usize), r + 1))
    }
}

/// Whether `path` is a spreadsheet file this module can import.
pub fn is_spreadsheet(path: &Path) -> bool {
    let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    matches!(ext.as_str(), "xlsx" | "xlsm" | "xlsb" | "xls" | "ods" | "csv" | "tsv")
}

/// `mappe.xlsx` → `mappe.hcmd` next to it (`mappe-2.hcmd` … if taken).
pub fn target_path(path: &Path) -> std::path::PathBuf {
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "import".into());
    (1..)
        .map(|k| path.with_file_name(if k == 1 { format!("{stem}.hcmd") } else { format!("{stem}-{k}.hcmd") }))
        .find(|p| !p.exists())
        .unwrap()
}

/// Reads a spreadsheet file into a document; numbers are written with a
/// decimal point.
pub fn import_file(path: &Path) -> Result<(Document, ImportReport), String> {
    import_file_avoiding(path, &[])
}

/// Like [`import_file`]; table names in `taken` are avoided (also in the
/// translated formulas), for adding sheets to an existing document.
pub fn import_file_avoiding(path: &Path, taken: &[String]) -> Result<(Document, ImportReport), String> {
    let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    if ext == "csv" || ext == "tsv" {
        let (mut doc, report) = crate::delimited::import(path)?;
        // Avoid clashing table names (CSV has no formulas to adjust).
        for block in &mut doc.blocks {
            if let Block::Table(t) = block
                && let Some(name) = &mut t.name
            {
                while taken.contains(name) {
                    name.push('_');
                }
            }
        }
        return Ok((doc, report));
    }
    let mut book = open_workbook_auto(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut sheets = Vec::new();
    for name in book.sheet_names() {
        let values = book.worksheet_range(&name).map_err(|e| format!("sheet {name}: {e}"))?;
        // Not every format has formulas (or they may fail to load): import the values then.
        let formulas = book.worksheet_formula(&name).unwrap_or_default();
        sheets.push((name, values, formulas));
    }
    let title = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let source = path.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    Ok(build_document(&title, &source, &sheets, taken))
}

type Sheet = (String, Range<Data>, Range<String>);

fn non_empty_formula(f: &str) -> bool {
    !f.trim().is_empty()
}

/// Bounding box (rows, cols, inclusive) of all non-empty values and formulas.
fn bounds(values: &Range<Data>, formulas: &Range<String>) -> Option<((u32, u32), (u32, u32))> {
    let mut cells: Vec<(u32, u32)> = Vec::new();
    if let Some((r0, c0)) = values.start() {
        cells.extend(values.used_cells().filter(|(_, _, v)| **v != Data::Empty).map(|(r, c, _)| (r0 + r as u32, c0 + c as u32)));
    }
    if let Some((r0, c0)) = formulas.start() {
        cells.extend(formulas.used_cells().filter(|(_, _, f)| non_empty_formula(f)).map(|(r, c, _)| (r0 + r as u32, c0 + c as u32)));
    }
    let rows = (cells.iter().map(|c| c.0).min()?, cells.iter().map(|c| c.0).max()?);
    let cols = (cells.iter().map(|c| c.1).min()?, cells.iter().map(|c| c.1).max()?);
    Some((rows, cols))
}

fn formula_at(formulas: &Range<String>, row: u32, col: u32) -> Option<&str> {
    formulas.get_value((row, col)).map(String::as_str).filter(|f| non_empty_formula(f))
}

fn build_document(title: &str, source: &str, sheets: &[Sheet], taken: &[String]) -> (Document, ImportReport) {
    let mut report = ImportReport::default();
    // Pass 1: where each sheet's table lies, so formulas can refer across sheets.
    let mut geometry: HashMap<String, Geometry> = HashMap::new();
    let mut order = Vec::new();
    let mut used_names: Vec<String> = taken.to_vec();
    for (sheet, values, formulas) in sheets {
        let Some(((r0, r1), (c0, c1))) = bounds(values, formulas) else { continue };
        // The first row is a header unless it holds numbers only.
        let first: Vec<&Data> = (c0..=c1).filter_map(|c| values.get_value((r0, c))).collect();
        let header = first.iter().any(|v| matches!(v, Data::String(_))) && formula_count(formulas, r0, c0, c1) == 0;
        let data_row = if header { r0 + 1 } else { r0 };
        let mut name = sheet.trim().replace("::", ":").replace('|', "/");
        if name.is_empty() {
            name = format!("Sheet{}", order.len() + 1);
        }
        while used_names.contains(&name) {
            name.push('_');
        }
        used_names.push(name.clone());
        let g = Geometry { name, data_row, rows: (r1 + 1).saturating_sub(data_row), first_col: c0, cols: c1 - c0 + 1 };
        geometry.insert(sheet.clone(), g);
        order.push((sheet, values, formulas, header, r0));
    }

    // Pass 2: tables, values, translated formulas.
    let mut doc = Document::default();
    doc.blocks.push(Block::Text(vec![format!("# {title}"), String::new(), format!("Imported from {source}.")]));
    for (sheet, values, formulas, header, r0) in order {
        let g = &geometry[sheet];
        let mut names: Vec<String> = Vec::new();
        let columns: Vec<Column> = (0..g.cols)
            .map(|c| {
                let raw = if header { values.get_value((r0, g.first_col + c)).map(header_text).unwrap_or_default() } else { String::new() };
                // `:` separates name and type in hardcalc headers.
                let mut name = raw.trim().replace(':', "-").replace('\n', " ");
                if name.is_empty() {
                    name = col_letters(c as usize);
                }
                while names.contains(&name) {
                    name.push('_');
                }
                names.push(name.clone());
                Column { name, ty: None, align: Align::None }
            })
            .collect();
        let mut rows = Vec::new();
        let mut notes = Vec::new();
        for r in 0..g.rows {
            let row = g.data_row + r;
            let mut out = Vec::new();
            for c in 0..g.cols {
                let col = g.first_col + c;
                let value = values.get_value((row, col)).cloned().unwrap_or(Data::Empty);
                let text = match formula_at(formulas, row, col) {
                    Some(f) => {
                        report.formulas += 1;
                        let excel = if f.starts_with("of:") || f.starts_with("oooc:") { odf_to_excel(f) } else { f.trim_start_matches('=').to_string() };
                        match translate(&excel, sheet, &geometry) {
                            Ok(formula) => format!("={formula}"),
                            Err(_) => {
                                report.kept += 1;
                                notes.push(Rule::Rule {
                                    target: format!("{}{}", col_letters(c as usize), r + 1),
                                    body: format!("original formula: ={excel}"),
                                });
                                cell_text(&value)
                            }
                        }
                    }
                    None => cell_text(&value),
                };
                if !text.is_empty() {
                    report.cells += 1;
                }
                out.push(text);
            }
            rows.push(out);
        }
        doc.blocks.push(Block::Table(Table {
            heading_level: Some(2),
            name: Some(g.name.clone()),
            shape: Shape::Matrix(Vec::new()),
            columns,
            rows,
            footer: Vec::new(),
            layer_name: None,
            layers: Vec::new(),
            fill: None,
        }));
        if !notes.is_empty() {
            doc.blocks.push(Block::Layer(Layer { name: "comments".into(), target: None, rules: notes }));
        }
        report.tables += 1;
    }
    (doc, report)
}

fn formula_count(formulas: &Range<String>, row: u32, c0: u32, c1: u32) -> usize {
    (c0..=c1).filter(|&c| formula_at(formulas, row, c).is_some()).count()
}

fn header_text(v: &Data) -> String {
    match v {
        Data::String(s) => s.clone(),
        other => cell_text(other),
    }
}

fn number_text(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 { format!("{n:.0}") } else { format!("{n}") }
}

/// Excel serial date/time → `2026-10-08`, `2026-10-08 14:30` or `14:30`.
fn serial_text(serial: f64) -> String {
    let days = serial.floor() as i64;
    let minutes = ((serial - serial.floor()) * 1440.0).round() as i64;
    let time = format!("{:02}:{:02}", minutes / 60 % 24, minutes % 60);
    if days == 0 {
        return time;
    }
    // Day 25569 is 1970-01-01 (Excel counts from 1899-12-30).
    let (y, m, d) = civil_from_days(days - 25569);
    if minutes == 0 { format!("{y:04}-{m:02}-{d:02}") } else { format!("{y:04}-{m:02}-{d:02} {time}") }
}

/// The text a value gets in the hardcalc table.
fn cell_text(v: &Data) -> String {
    match v {
        Data::Empty => String::new(),
        Data::Int(i) => i.to_string(),
        Data::Float(f) => number_text(*f),
        Data::Bool(b) => b.to_string(),
        Data::String(s) => s.replace(['\r', '\n'], " "),
        Data::DateTime(dt) if dt.is_duration() => {
            let minutes = (dt.as_f64() * 1440.0).round() as i64;
            format!("{}:{:02}", minutes / 60, minutes % 60)
        }
        Data::DateTime(dt) => serial_text(dt.as_f64()),
        Data::DateTimeIso(s) => s.strip_suffix("T00:00:00").unwrap_or(s).replace('T', " "),
        Data::DurationIso(s) => s.clone(),
        Data::Error(e) => format!("#{e:?}"),
    }
}

// ---------------------------------------------------------------- formulas

/// `$AB$12` → (row 11, col 27); `AB` alone → (None, col 27).
fn parse_cell_ref(s: &str) -> Option<(Option<u32>, u32)> {
    let s = s.replace('$', "");
    let split = s.find(|c: char| c.is_ascii_digit()).unwrap_or(s.len());
    let (letters, digits) = s.split_at(split);
    if letters.is_empty() || !letters.chars().all(|c| c.is_ascii_alphabetic()) || letters.len() > 3 {
        return None;
    }
    let col = letters.to_ascii_uppercase().chars().fold(0u32, |acc, c| acc * 26 + (c as u32 - 'A' as u32 + 1)) - 1;
    if digits.is_empty() {
        return Some((None, col));
    }
    let row: u32 = digits.parse().ok()?;
    (row >= 1).then_some((Some(row - 1), col))
}

/// Writes a table name for use before `!` (quoted unless it is a plain name).
fn table_prefix(name: &str) -> String {
    if name.chars().all(|c| c.is_alphanumeric() || c == '_') && name.chars().next().is_some_and(char::is_alphabetic) {
        format!("{name}!")
    } else {
        format!("\"{name}\"!")
    }
}

/// Translates a reference (`B2`, `B2:B9`, `B:B`, optionally on `sheet`).
fn translate_ref(sheet: &str, a: &str, b: Option<&str>, here: &str, geometry: &HashMap<String, Geometry>) -> Result<String, String> {
    let g = geometry.get(sheet).ok_or_else(|| format!("unknown sheet '{sheet}'"))?;
    let prefix = if sheet == here { String::new() } else { table_prefix(&g.name) };
    let (ra, ca) = parse_cell_ref(a).ok_or_else(|| format!("bad reference '{a}'"))?;
    let column = |c: u32| c.checked_sub(g.first_col).filter(|c| *c < g.cols).map(|c| col_letters(c as usize));
    match (ra, b) {
        (Some(r), None) => Ok(format!("{prefix}{}", g.cell(r, ca).ok_or_else(|| format!("{a} is outside the table"))?)),
        // Whole columns: `B:B` → the column of the table.
        (None, Some(b)) => {
            let (rb, cb) = parse_cell_ref(b).ok_or_else(|| format!("bad reference '{b}'"))?;
            if rb.is_some() || ca != cb {
                return Err("only single whole columns are supported".into());
            }
            Ok(format!("{prefix}{}", column(ca).ok_or("column outside the table")?))
        }
        (Some(r1), Some(b)) => {
            let (Some(r2), c2) = parse_cell_ref(b).ok_or_else(|| format!("bad reference '{b}'"))? else {
                return Err("mixed range".into());
            };
            // Ranges reaching into the header or past the data are clipped to the data.
            let lo = r1.min(r2).max(g.data_row);
            let hi = r1.max(r2).min(g.data_row + g.rows.saturating_sub(1));
            if lo > hi || g.rows == 0 {
                return Err("range holds no data".into());
            }
            let from = g.cell(lo, ca.min(c2)).ok_or("range outside the table")?;
            let to = g.cell(hi, ca.max(c2)).ok_or("range outside the table")?;
            Ok(format!("{prefix}{from}:{to}"))
        }
        (None, None) => Err(format!("bad reference '{a}'")),
    }
}

/// Translates an Excel formula (without `=`) of sheet `here` to hardcalc;
/// `tables` gives (name, data row, rows, first column, columns) per sheet.
#[cfg(test)]
fn translate_formula(excel: &str, here: &str, tables: &[(&str, u32, u32, u32, u32)]) -> Result<String, String> {
    let geometry: HashMap<String, Geometry> = tables
        .iter()
        .map(|&(n, data_row, rows, first_col, cols)| (n.to_string(), Geometry { name: n.to_string(), data_row, rows, first_col, cols }))
        .collect();
    translate(excel, here, &geometry)
}

fn translate(excel: &str, here: &str, geometry: &HashMap<String, Geometry>) -> Result<String, String> {
    // Newer Excel functions are stored as `_xlfn.XLOOKUP(…)`.
    let excel = excel.replace("_xlfn._xlws.", "").replace("_xlfn.", "").replace("_xlws.", "");
    let chars: Vec<char> = excel.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    let word_char = |c: char| c.is_alphanumeric() || c == '_' || c == '$' || c == '.';
    // Reads `A1` or `A1:B2` / `A:A` starting at i; returns the end position.
    let read_ref = |i: usize| -> (String, Option<String>, usize) {
        let mut j = i;
        while j < chars.len() && word_char(chars[j]) {
            j += 1;
        }
        let a: String = chars[i..j].iter().collect();
        if chars.get(j) == Some(&':') {
            let mut k = j + 1;
            while k < chars.len() && word_char(chars[k]) {
                k += 1;
            }
            if k > j + 1 {
                return (a, Some(chars[j + 1..k].iter().collect()), k);
            }
        }
        (a, None, j)
    };
    while i < chars.len() {
        let c = chars[i];
        match c {
            '"' => {
                // String literal ("" escapes a quote) – the same in hardcalc.
                let start = i;
                i += 1;
                while i < chars.len() {
                    if chars[i] == '"' && chars.get(i + 1) == Some(&'"') {
                        i += 2;
                    } else if chars[i] == '"' {
                        i += 1;
                        break;
                    } else {
                        i += 1;
                    }
                }
                out.extend(&chars[start..i]);
            }
            '\'' => {
                // 'Sheet name'!ref
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
                if chars.get(j + 1) != Some(&'!') {
                    return Err("quoted name without '!'".into());
                }
                let (a, b, end) = read_ref(j + 2);
                out.push_str(&translate_ref(&name, &a, b.as_deref(), here, geometry)?);
                i = end;
            }
            c if c.is_ascii_digit() || (c == '.' && chars.get(i + 1).is_some_and(|d| d.is_ascii_digit())) => {
                // Number – or a row range like 1:1 (not supported).
                let start = i;
                while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.' || matches!(chars[i], 'e' | 'E')) {
                    if matches!(chars[i], 'e' | 'E') && chars.get(i + 1).is_some_and(|n| *n == '+' || *n == '-') {
                        i += 1;
                    }
                    i += 1;
                }
                if chars.get(i) == Some(&':') {
                    return Err("row ranges are not supported".into());
                }
                out.extend(&chars[start..i]);
            }
            c if c.is_alphabetic() || c == '_' || c == '$' => {
                let mut j = i;
                while j < chars.len() && word_char(chars[j]) {
                    j += 1;
                }
                let word: String = chars[i..j].iter().collect();
                if chars.get(j) == Some(&'(') {
                    let upper = word.to_ascii_uppercase();
                    let name = crate::builtins::from_excel(&upper).map(|b| b.name).ok_or_else(|| format!("function {word} has no hardcalc equivalent"))?;
                    out.push_str(name);
                    i = j;
                } else if chars.get(j) == Some(&'!') {
                    let (a, b, end) = read_ref(j + 1);
                    out.push_str(&translate_ref(&word, &a, b.as_deref(), here, geometry)?);
                    i = end;
                } else if word.eq_ignore_ascii_case("TRUE") || word.eq_ignore_ascii_case("FALSE") {
                    out.push_str(&word.to_lowercase());
                    i = j;
                } else if parse_cell_ref(&word).is_some() {
                    let (a, b, end) = read_ref(i);
                    out.push_str(&translate_ref(here, &a, b.as_deref(), here, geometry)?);
                    i = end;
                } else {
                    return Err(format!("unknown name '{word}'"));
                }
            }
            ';' => {
                out.push(',');
                i += 1;
            }
            '{' | '#' | '@' | '[' => return Err(format!("'{c}' is not supported")),
            _ => {
                out.push(c);
                i += 1;
            }
        }
    }
    // Make sure the result is valid hardcalc syntax.
    crate::formula::parse_formula(&out).map(|_| out)
}

/// ODF formula (`of:=SUM([.B2:.B3])`) → Excel syntax (`SUM(B2:B3)`).
pub fn odf_to_excel(f: &str) -> String {
    let body = f.split_once('=').map(|(_, b)| b).unwrap_or(f);
    let mut out = String::new();
    let mut chars = body.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                out.push(c);
                for d in chars.by_ref() {
                    out.push(d);
                    if d == '"' {
                        break;
                    }
                }
            }
            '[' => {
                let mut inner = String::new();
                for d in chars.by_ref() {
                    if d == ']' {
                        break;
                    }
                    inner.push(d);
                }
                // [Sheet.B2:.B9] / [.B2] / [$'My Sheet'.$B$2]
                let parts: Vec<String> = inner
                    .split(':')
                    .map(|part| {
                        let part = part.trim();
                        match part.rfind('.') {
                            Some(dot) => {
                                let sheet = part[..dot].trim_start_matches('$');
                                let cell = &part[dot + 1..];
                                if sheet.is_empty() {
                                    cell.to_string()
                                } else if sheet.starts_with('\'') {
                                    format!("{sheet}!{cell}")
                                } else {
                                    format!("'{sheet}'!{cell}")
                                }
                            }
                            None => part.to_string(),
                        }
                    })
                    .collect();
                // The sheet is only needed once, in front of the range.
                let second = parts.get(1).map(|p| p.rsplit_once('!').map(|(_, c)| c.to_string()).unwrap_or(p.clone()));
                out.push_str(&parts[0]);
                if let Some(second) = second {
                    out.push(':');
                    out.push_str(&second);
                }
            }
            ';' => out.push(','),
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // Sheet "S": header in row 1 (0-based 0), data rows 2..=4, columns A..C.
    // Sheet "T x": no header, data from row 1, columns B..C.
    const TABLES: &[(&str, u32, u32, u32, u32)] = &[("S", 1, 3, 0, 3), ("T x", 0, 5, 1, 2)];

    fn t(f: &str) -> Result<String, String> {
        translate_formula(f, "S", TABLES)
    }

    #[test]
    fn references_shift_by_the_header() {
        assert_eq!(t("B2*1.19").unwrap(), "B1*1.19");
        assert_eq!(t("SUM($B$2:$B$4)").unwrap(), "sum(B1:B3)");
        assert_eq!(t("SUM(B1:B100)").unwrap(), "sum(B1:B3)", "clipped to the data");
        assert_eq!(t("AVERAGE(B:B)").unwrap(), "avg(B)");
        assert_eq!(t("IF(A2>0,\"ja\",\"nein\")").unwrap(), "if(A1>0,\"ja\",\"nein\")");
        assert_eq!(t("ROUND(C3;2)").unwrap(), "round(C2,2)");
        assert_eq!(t("A2=TRUE").unwrap(), "A1=true");
    }

    #[test]
    fn other_sheets() {
        // 'T x' starts in column B: its B1 is the table's A1.
        assert_eq!(t("'T x'!B1+1").unwrap(), "\"T x\"!A1+1");
        assert_eq!(t("SUM('T x'!C1:C5)").unwrap(), "sum(\"T x\"!B1:B5)");
        assert_eq!(translate_formula("S!B2", "T x", TABLES).unwrap(), "S!B1");
    }

    #[test]
    fn untranslatable() {
        assert!(t("VLOOKUP(A2,A2:C4,2,FALSE)").is_err());
        assert!(t("B1").is_err(), "the header row is not data");
        assert!(t("Z9").is_err(), "outside the table");
        assert!(t("SUM(1:1)").is_err());
        assert!(t("Unknown!A1").is_err());
        assert!(t("MyName*2").is_err());
    }

    #[test]
    fn odf_formulas() {
        assert_eq!(odf_to_excel("of:=SUM([.B2:.B3])"), "SUM(B2:B3)");
        assert_eq!(odf_to_excel("of:=[$Tab.$B$2]*2"), "'Tab'!$B$2*2");
        assert_eq!(odf_to_excel("of:=IF([.A2]>0;\"a;b\";[.A3])"), "IF(A2>0,\"a;b\",A3)");
        assert_eq!(odf_to_excel("of:=SUM(['T x'.C1:.C5])"), "SUM('T x'!C1:C5)");
    }

    #[test]
    fn values() {
        assert_eq!(serial_text(46303.0), "2026-10-08");
        assert_eq!(serial_text(46303.5), "2026-10-08 12:00");
        assert_eq!(serial_text(0.25), "06:00");
        assert_eq!(number_text(1200.5), "1200.5");
        assert_eq!(number_text(3.0), "3");
    }
}
