//! The `links` layer: cells that jump to another place of the document.
//!
//! Every rule names cells (like any layer) and a destination written like a
//! reference in a formula:
//!
//! | Destination               | Leads to                                         |
//! |---------------------------|--------------------------------------------------|
//! | `Expenses`                | the first cell of the table                      |
//! | `B5`, `Amount`            | cell / top of the column in the own table        |
//! | `Expenses!B5`, `Expenses!Amount` | cell / top of the column of another table |
//! | `Expenses[Rent]`          | the row with this key (1st column)               |
//! | `Expenses[Rent, Amount]`  | the cell by key and column                       |
//! | `Expenses[@Amount]`       | the same row of the other table (as in formulas) |
//! | `Expenses[@Item, Amount]` | the row whose key is this row's `Item`           |
//!
//! UI-independent; the terminal UI draws the marker and does the jumping.

use crate::engine::{letters_to_col, CellId, Workbook};
use crate::layers::{Attached, LayerError, Target};
use crate::model::Rule;
use std::collections::HashMap;

/// Name of the layer.
pub const LAYER: &str = "links";

/// All cells with a link and their destination text (the last rule that
/// meets a cell wins, as in `format`). A rule for a cell or range also counts
/// beyond the data (an empty row below it); columns and rows only within.
pub fn link_cells(attached: &[Attached], wb: &mut Workbook, shown: &dyn Fn(usize) -> bool) -> HashMap<CellId, String> {
    let mut out = HashMap::new();
    for a in attached.iter().filter(|a| a.layer.name == LAYER && shown(a.sheet)) {
        for rule in &a.layer.rules {
            let Rule::Rule { target, body } = rule else { continue };
            let target = Target::parse(target);
            let cells = match target {
                Target::Cell(r, c) => vec![(a.sheet, r, c)],
                Target::Range((c1, r1), (c2, r2)) => {
                    (r1.min(r2)..=r1.max(r2)).flat_map(|r| (c1.min(c2)..=c1.max(c2)).map(move |c| (a.sheet, r, c))).collect()
                }
                _ => cells_of(wb, a.sheet, &target),
            };
            for id in cells {
                out.insert(id, body.clone());
            }
        }
    }
    out
}

fn cells_of(wb: &mut Workbook, sheet: usize, target: &Target) -> Vec<CellId> {
    let (rows, cols) = (wb.sheets[sheet].cells.len(), wb.sheets[sheet].columns.len());
    (0..rows).flat_map(|r| (0..cols).map(move |c| (sheet, r, c))).filter(|&id| target.matches(wb, id)).collect()
}

/// Destinations that lead nowhere, for `hardcalc check`. A destination with
/// `@` is tried from every cell of its rule; the first failure is reported.
pub fn problems(attached: &[Attached], wb: &mut Workbook) -> Vec<LayerError> {
    let mut out = Vec::new();
    for a in attached.iter().filter(|a| a.layer.name == LAYER) {
        for (i, rule) in a.layer.rules.iter().enumerate() {
            let Rule::Rule { target, body } = rule else { continue };
            let cells = if body.contains('@') { cells_of(wb, a.sheet, &Target::parse(target)) } else { vec![(a.sheet, 0, 0)] };
            if let Some(e) = cells.into_iter().find_map(|here| resolve(wb, here, body).err()) {
                out.push(LayerError { line: a.layer.line.0 + 1 + i, sheet: a.sheet, layer: LAYER.into(), message: format!("{target}: {e}") });
            }
        }
    }
    out
}

/// The cell the destination `text` leads to, seen from cell `here`.
pub fn resolve(wb: &mut Workbook, here: CellId, text: &str) -> Result<CellId, String> {
    let text = text.trim();
    if text.is_empty() {
        return Err("the link has no destination".into());
    }
    let (name, rest) = split_table(text)?;
    let sheet = match name.as_deref() {
        Some(n) => wb.sheet_index(n),
        None => Some(here.0),
    };
    // A cell address may lie beyond the data: the cursor goes there (in a
    // playground) or as near as the window lets it (the + row / column).
    let mut address = false;
    let dest = match (rest, sheet) {
        // A plain word: a table, else a cell or column of the own table.
        (None, Some(s)) if name.is_some() => (s, 0, 0),
        (None, _) => {
            let word = name.unwrap_or_default();
            match parse_a1(&word) {
                Some((r, c)) => {
                    address = true;
                    (here.0, r, c)
                }
                None => (here.0, 0, column(wb, here.0, &word).ok_or_else(|| format!("no table or column '{word}'"))?),
            }
        }
        (Some(_), None) => return Err(format!("unknown table '{}'", name.unwrap_or_default())),
        (Some(Rest::Bang(r)), Some(s)) => {
            if let Some(col) = r.strip_prefix('@') {
                (s, here.1, column(wb, s, col.trim()).ok_or_else(|| unknown_column(wb, s, col.trim()))?)
            } else {
                let first = r.split(':').next().unwrap_or_default().trim();
                match parse_a1(first) {
                    Some((row, c)) => {
                        address = true;
                        (s, row, c)
                    }
                    None => (s, 0, column(wb, s, first).ok_or_else(|| unknown_column(wb, s, first))?),
                }
            }
        }
        (Some(Rest::Index(args)), Some(s)) => index(wb, here, s, &args)?,
    };
    let (rows, cols) = (wb.sheets[dest.0].cells.len(), wb.sheets[dest.0].columns.len());
    if !address && (dest.1 >= rows || dest.2 >= cols) {
        return Err(format!("{} is beyond the table – it has {rows} rows and {cols} columns", sheet_label(wb, dest.0)));
    }
    Ok(dest)
}

/// The destination as the go-to prompt shows it: a `@` reference becomes the
/// cell it means from `here`, everything else stays as written.
pub fn concrete(wb: &mut Workbook, here: CellId, text: &str) -> String {
    let text = text.trim();
    if !text.contains('@') {
        return text.to_string();
    }
    match resolve(wb, here, text) {
        Ok(dest) => cell_ref(wb, here.0, dest),
        Err(_) => text.to_string(),
    }
}

/// `B5` in the own table, `Expenses!B5` / `"My costs"!B5` in another one.
pub fn cell_ref(wb: &Workbook, here: usize, (s, r, c): CellId) -> String {
    let a1 = format!("{}{}", crate::engine::col_letters(c), r + 1);
    match &wb.sheets[s].name {
        Some(name) if s != here => format!("{}!{a1}", crate::refs::table_ref(name)),
        _ => a1,
    }
}

fn sheet_label(wb: &Workbook, s: usize) -> String {
    wb.sheets[s].name.clone().unwrap_or_else(|| format!("table {}", s + 1))
}

fn unknown_column(wb: &Workbook, s: usize, name: &str) -> String {
    format!("no column '{name}' in {}", sheet_label(wb, s))
}

enum Rest {
    /// After `!`.
    Bang(String),
    /// The arguments in `[…]`.
    Index(Vec<String>),
}

/// `"My costs"!B2` → (Some("My costs"), Bang("B2")); `[@Item]` → (None, Index).
/// Without `!` or `[`, the whole text is the name.
fn split_table(text: &str) -> Result<(Option<String>, Option<Rest>), String> {
    let (name, after) = if let Some(quoted) = text.strip_prefix('"') {
        let end = quoted.find('"').ok_or("a quote is not closed")?;
        (Some(quoted[..end].to_string()), quoted[end + 1..].trim_start())
    } else {
        match text.find(['!', '[']) {
            Some(i) => {
                let n = text[..i].trim();
                ((!n.is_empty()).then(|| n.to_string()), &text[i..])
            }
            None => (Some(text.to_string()), ""),
        }
    };
    let rest = if after.is_empty() {
        None
    } else if let Some(r) = after.strip_prefix('!') {
        Some(Rest::Bang(r.trim().to_string()))
    } else if let Some(inner) = after.strip_prefix('[').and_then(|a| a.trim_end().strip_suffix(']')) {
        Some(Rest::Index(split_args(inner)))
    } else {
        return Err(format!("cannot read '{text}' – e.g. Expenses, Expenses!B5, Expenses[Rent, Amount]"));
    };
    Ok((name, rest))
}

/// Splits at the commas outside quotes.
fn split_args(s: &str) -> Vec<String> {
    let mut out = vec![String::new()];
    let mut quoted = false;
    for ch in s.chars() {
        match ch {
            '"' => {
                quoted = !quoted;
                out.last_mut().unwrap().push(ch);
            }
            ',' if !quoted => out.push(String::new()),
            _ => out.last_mut().unwrap().push(ch),
        }
    }
    out.into_iter().map(|a| a.trim().to_string()).collect()
}

/// `[Key]`, `[Key, Column]`, `[@Column]`, `[@Column, Column]`, `[3, Amount]`.
fn index(wb: &mut Workbook, here: CellId, s: usize, args: &[String]) -> Result<CellId, String> {
    match args {
        [one] if one.starts_with('@') => {
            let col = one[1..].trim();
            Ok((s, here.1, column(wb, s, col).ok_or_else(|| unknown_column(wb, s, col))?))
        }
        [key] => Ok((s, row(wb, here, s, key)?, 0)),
        [key, col] => {
            let r = row(wb, here, s, key)?;
            let c = match col.parse::<usize>() {
                Ok(n) if n >= 1 => n - 1,
                _ => column(wb, s, col.trim_matches('"')).ok_or_else(|| unknown_column(wb, s, col))?,
            };
            Ok((s, r, c))
        }
        _ => Err("expected Table[Key] or Table[Key, Column]".into()),
    }
}

/// The row of a key: a number is the row itself (as in formulas), `@Column`
/// takes the key from that column in `here`'s row.
fn row(wb: &mut Workbook, here: CellId, s: usize, key: &str) -> Result<usize, String> {
    if let Ok(n) = key.parse::<usize>() {
        return if n >= 1 { Ok(n - 1) } else { Err("rows count from 1".into()) };
    }
    let key = match key.strip_prefix('@') {
        Some(col) => {
            let c = column(wb, here.0, col.trim()).ok_or_else(|| unknown_column(wb, here.0, col.trim()))?;
            if here.1 >= wb.sheets[here.0].cells.len() {
                return Err(format!("the row of the link has no {} yet", col.trim()));
            }
            wb.display((here.0, here.1, c)).trim().to_string()
        }
        None => key.trim_matches('"').to_string(),
    };
    wb.sheets[s]
        .raw
        .iter()
        .position(|r| r.first().map(|x| x.trim()) == Some(key.as_str()))
        .ok_or_else(|| format!("no row '{key}' in {}", sheet_label(wb, s)))
}

/// A column by name, else by letters.
fn column(wb: &Workbook, s: usize, name: &str) -> Option<usize> {
    let cols = &wb.sheets[s].columns;
    cols.iter().position(|c| c.name == name).or_else(|| letters_to_col(name).filter(|&c| c < cols.len()))
}

/// `B12` → (row 11, col 1)
fn parse_a1(s: &str) -> Option<(usize, usize)> {
    let split = s.find(|c: char| c.is_ascii_digit())?;
    let (letters, digits) = s.split_at(split);
    if letters.is_empty() || !letters.chars().all(|c| c.is_ascii_uppercase()) {
        return None;
    }
    let row: usize = digits.parse().ok()?;
    (row >= 1).then(|| (row - 1, letters_to_col(letters).unwrap_or(0)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::parse;

    const DOC: &str = "\
## Overview :: matrix

| Area     | Total |
|----------|-------|
| Expenses | 1     |
| Income   | 2     |

```layer links
A: Details[@Area, Amount]
B1: Expenses
B2: \"My costs\"!B2
```

## Details :: matrix

| Area     | Amount |
|----------|--------|
| Income   | 5      |
| Expenses | 7      |

## Expenses :: matrix

| Item | Amount |
|------|--------|
| Rent | 1200   |

## My costs :: matrix

| Item | Amount |
|------|--------|
| Tea  | 3      |
| Milk | 2      |
";

    fn setup() -> (Workbook, Vec<Attached>) {
        let doc = parse(DOC).unwrap();
        let wb = Workbook::from_document(&doc).0;
        let att = crate::layers::attach(&doc, &wb);
        (wb, att)
    }

    #[test]
    fn destinations() {
        let (mut wb, _) = setup();
        let here = (0, 0, 0);
        assert_eq!(resolve(&mut wb, here, "Expenses"), Ok((2, 0, 0)));
        assert_eq!(resolve(&mut wb, here, "B2"), Ok((0, 1, 1)));
        assert_eq!(resolve(&mut wb, here, "Total"), Ok((0, 0, 1)));
        assert_eq!(resolve(&mut wb, here, "Details!B2"), Ok((1, 1, 1)));
        assert_eq!(resolve(&mut wb, here, "Details!Amount"), Ok((1, 0, 1)));
        assert_eq!(resolve(&mut wb, here, "Details[Expenses]"), Ok((1, 1, 0)));
        assert_eq!(resolve(&mut wb, here, "Details[Expenses, Amount]"), Ok((1, 1, 1)));
        assert_eq!(resolve(&mut wb, here, "\"My costs\"[Milk, Amount]"), Ok((3, 1, 1)));
        assert_eq!(resolve(&mut wb, (0, 1, 0), "Details[@Area]"), Ok((1, 1, 0)));
        assert_eq!(resolve(&mut wb, (0, 1, 0), "Details[@Area, Amount]"), Ok((1, 0, 1)));
        assert_eq!(resolve(&mut wb, (0, 0, 0), "Details[@Area, Amount]"), Ok((1, 1, 1)));
        assert!(resolve(&mut wb, here, "Nowhere!B2").is_err());
        assert!(resolve(&mut wb, here, "Details[Rent]").is_err());
        // A cell address may lie beyond the data, a row number may not.
        assert_eq!(resolve(&mut wb, here, "Details!B9"), Ok((1, 8, 1)));
        assert!(resolve(&mut wb, here, "Details[9, Amount]").is_err());
    }

    #[test]
    fn cells_and_prompt() {
        let (mut wb, att) = setup();
        let cells = link_cells(&att, &mut wb, &|_| true);
        assert_eq!(cells.get(&(0, 1, 0)).map(String::as_str), Some("Details[@Area, Amount]"));
        assert_eq!(cells.get(&(0, 0, 1)).map(String::as_str), Some("Expenses"));
        assert_eq!(cells.len(), 4);
        assert_eq!(concrete(&mut wb, (0, 1, 0), "Details[@Area, Amount]"), "Details!B1");
        assert_eq!(concrete(&mut wb, (0, 1, 1), "\"My costs\"!B2"), "\"My costs\"!B2");
        assert!(problems(&att, &mut wb).is_empty());
    }

    #[test]
    fn renaming_a_table_renames_its_links() {
        let mut doc = parse(DOC).unwrap();
        crate::refs::rename_table(&mut doc, 1, "Info");
        crate::refs::rename_table(&mut doc, 2, "All costs");
        let text = crate::write::write(&doc);
        assert!(text.contains("A: Info[@Area, Amount]"), "{text}");
        assert!(text.contains("B1: \"All costs\""), "{text}");
    }

    #[test]
    fn broken_destinations_are_reported() {
        let doc = parse("## T :: matrix\n\n| A |\n|---|\n| x |\n| y |\n\n```layer links\nA: T[@A, Nope]\nA1: Gone\n```\n").unwrap();
        let mut wb = Workbook::from_document(&doc).0;
        let att = crate::layers::attach(&doc, &wb);
        let p = problems(&att, &mut wb);
        assert_eq!(p.len(), 2, "{p:?}");
        assert!(p[0].message.contains("Nope"));
        assert!(p[1].message.contains("Gone"));
    }
}
