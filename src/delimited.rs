//! CSV / TSV import and export.

use crate::engine::{col_letters, Workbook};
use crate::model::{Align, Block, Column, Document, Shape, Table};
use std::path::{Path, PathBuf};

/// Decodes file bytes: UTF-8 (a BOM is dropped), otherwise Windows-1252 as
/// written by older spreadsheet exports.
pub fn decode(bytes: &[u8]) -> String {
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_string(),
        Err(_) => bytes.iter().map(|&b| windows_1252(b)).collect(),
    }
}

fn windows_1252(b: u8) -> char {
    const HIGH: [char; 32] = [
        '€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8d}', 'Ž', '\u{8f}', '\u{90}', '‘', '’', '“',
        '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ', '\u{9d}', 'ž', 'Ÿ',
    ];
    match b {
        0x80..=0x9f => HIGH[(b - 0x80) as usize],
        _ => b as char,
    }
}

/// Splits CSV text into records (RFC 4180: quotes, doubled quotes, line breaks in quotes).
pub fn parse(text: &str, sep: char) -> Vec<Vec<String>> {
    let mut records = Vec::new();
    let mut record = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if quoted {
            match c {
                '"' if chars.peek() == Some(&'"') => {
                    field.push('"');
                    chars.next();
                }
                '"' => quoted = false,
                _ => field.push(c),
            }
        } else if c == '"' && field.is_empty() {
            quoted = true;
        } else if c == sep {
            record.push(std::mem::take(&mut field));
        } else if c == '\n' || c == '\r' {
            if c == '\r' && chars.peek() == Some(&'\n') {
                chars.next();
            }
            record.push(std::mem::take(&mut field));
            records.push(std::mem::take(&mut record));
        } else {
            field.push(c);
        }
    }
    if !field.is_empty() || !record.is_empty() {
        record.push(field);
        records.push(record);
    }
    // Drop empty lines.
    records.retain(|r| r.iter().any(|f| !f.trim().is_empty()));
    records
}

/// Guesses the separator: the candidate that splits the first records into
/// the same number of fields (more than one), as many as possible.
pub fn detect_separator(text: &str) -> char {
    // The beginning of the file is enough (cut at a line end).
    let sample = match text.char_indices().nth(64 * 1024) {
        Some((i, _)) => &text[..text[..i].rfind('\n').unwrap_or(i)],
        None => text,
    };
    let score = |sep: char| {
        let records = parse(sample, sep);
        let first = records.first().map(Vec::len).unwrap_or(0);
        if first < 2 || records.iter().take(20).any(|r| r.len() != first) { 0 } else { first }
    };
    ['\t', ';', ',', '|'].into_iter().map(|s| (s, score(s))).filter(|(_, n)| *n > 0).max_by_key(|(_, n)| *n).map(|(s, _)| s).unwrap_or(',')
}

/// Reads a CSV/TSV file into a document with one table named after the file.
pub fn import(path: &Path) -> Result<(Document, crate::import::ImportReport), String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let text = decode(&bytes);
    let tsv = path.extension().is_some_and(|e| e.eq_ignore_ascii_case("tsv"));
    let sep = if tsv { '\t' } else { detect_separator(&text) };
    let mut records = parse(&text, sep);
    let width = records.iter().map(Vec::len).max().unwrap_or(0);
    for r in &mut records {
        r.resize(width, String::new());
    }
    // The first record is a header unless it is all numbers.
    let header = records
        .first()
        .is_some_and(|r| r.iter().any(|f| !f.trim().is_empty() && crate::value::parse_number(f).is_none()));
    let names: Vec<String> = if header { records.remove(0) } else { Vec::new() };
    let mut seen: Vec<String> = Vec::new();
    let columns = (0..width)
        .map(|c| {
            let mut name = names.get(c).map(|n| n.trim().replace(':', "-")).filter(|n| !n.is_empty()).unwrap_or_else(|| col_letters(c));
            while seen.contains(&name) {
                name.push('_');
            }
            seen.push(name.clone());
            Column { name, ty: None, align: Align::None }
        })
        .collect();
    let rows: Vec<Vec<String>> = records.into_iter().map(|r| r.into_iter().map(|f| f.replace(['\r', '\n'], " ")).collect()).collect();
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "Import".into());
    let source = path.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let report = crate::import::ImportReport {
        tables: 1,
        cells: rows.iter().flatten().filter(|f| !f.is_empty()).count(),
        formulas: 0,
        kept: 0,
    };
    let doc = Document {
        blocks: vec![
            Block::Text(vec![format!("# {stem}"), String::new(), format!("Imported from {source}.")]),
            Block::Table(Table { heading_level: Some(2), name: Some(stem), shape: Shape::Matrix(Vec::new()), columns, rows, footer: Vec::new(), layer_name: None, layers: Vec::new(), fill: None }),
        ],
    };
    Ok((doc, report))
}

fn quote(field: &str, sep: char) -> String {
    if field.contains(sep) || field.contains('"') || field.contains('\n') || field.starts_with(' ') || field.ends_with(' ') {
        format!("\"{}\"", field.replace('"', "\"\""))
    } else {
        field.to_string()
    }
}

/// Table `sheet` as CSV text with computed values (header first).
pub fn table_csv(wb: &mut Workbook, sheet: usize, sep: char) -> String {
    let mut out = String::new();
    let line = |fields: Vec<String>| fields.iter().map(|f| quote(f, sep)).collect::<Vec<_>>().join(&sep.to_string()) + "\r\n";
    out.push_str(&line(wb.sheets[sheet].columns.iter().map(|c| c.name.clone()).collect()));
    for r in 0..wb.sheets[sheet].cells.len() {
        let fields = (0..wb.sheets[sheet].columns.len()).map(|c| wb.display((sheet, r, c))).collect();
        out.push_str(&line(fields));
    }
    out
}

/// Separator for a target file: tab for .tsv, `;` where decimals are written
/// with a comma (as spreadsheets there expect), `,` otherwise.
pub fn separator_for(path: &Path, decimal_comma: bool) -> char {
    if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("tsv")) {
        '\t'
    } else if decimal_comma {
        ';'
    } else {
        ','
    }
}

/// Writes table `sheet` (or every table, one file each, when `None` and the
/// document has several) and returns the files written.
pub fn export(doc: &Document, path: &Path, sheet: Option<usize>) -> Result<Vec<PathBuf>, String> {
    let (mut wb, _) = Workbook::from_document(doc);
    if wb.sheets.is_empty() {
        return Err("the document has no table".into());
    }
    let sep = separator_for(path, wb.decimal_comma);
    let targets: Vec<(usize, PathBuf)> = match sheet {
        Some(s) => vec![(s, path.to_path_buf())],
        None if wb.sheets.len() == 1 => vec![(0, path.to_path_buf())],
        None => {
            let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            let ext = path.extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_else(|| "csv".into());
            (0..wb.sheets.len())
                .map(|s| {
                    let name = wb.sheets[s].name.clone().unwrap_or_else(|| format!("table{}", s + 1));
                    let safe: String = name.chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
                    (s, path.with_file_name(format!("{stem}-{safe}.{ext}")))
                })
                .collect()
        }
    };
    let mut written = Vec::new();
    for (s, target) in targets {
        // A BOM lets spreadsheets recognise UTF-8 (umlauts, €).
        let text = format!("\u{feff}{}", table_csv(&mut wb, s, sep));
        std::fs::write(&target, text).map_err(|e| format!("{}: {e}", target.display()))?;
        written.push(target);
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parsing() {
        let text = "a;\"b;c\";\"d \"\"e\"\"\"\r\n1;\"zwei\nZeilen\";3\n\n";
        assert_eq!(parse(text, ';'), vec![vec!["a", "b;c", "d \"e\""], vec!["1", "zwei\nZeilen", "3"]]);
        assert_eq!(detect_separator(text), ';');
        assert_eq!(detect_separator("x,y\n1,2\n"), ',');
        assert_eq!(detect_separator("x\ty\n1\t2\n"), '\t');
        assert_eq!(detect_separator("single column\nvalue\n"), ',');
    }

    #[test]
    fn decoding() {
        assert_eq!(decode(b"\xEF\xBB\xBFCaf\xC3\xA9"), "Café");
        assert_eq!(decode(b"Na\xEFve \x80"), "Naïve €");
    }

    #[test]
    fn quoting() {
        assert_eq!(quote("a;b", ';'), "\"a;b\"");
        assert_eq!(quote("say \"hi\"", ','), "\"say \"\"hi\"\"\"");
        assert_eq!(quote("plain", ','), "plain");
    }
}
