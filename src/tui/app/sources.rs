//! The sources of the formula being edited: the cells and ranges of this
//! table it refers to, so the grid can highlight them (and let data flow
//! from them to the formula cell), and the other tables it takes data
//! from, so their tabs can be marked.

use super::{App, Mode, ViewKind};
use crate::engine::RefParts;

/// A rectangle of cells: ((top, left), (bottom, right)).
pub type Area = ((usize, usize), (usize, usize));

/// What a formula takes data from, in the order it appears.
#[derive(Debug, Clone, PartialEq)]
pub enum Found {
    Cells(Area),
    /// Another table, by name.
    Table(String),
}

/// The sources of the formula being edited, each with the number of its
/// colour (the order of appearance).
#[derive(Debug, Default)]
pub struct Sources {
    pub cells: Vec<(Area, usize)>,
    /// Sheet indexes of other tables.
    pub tables: Vec<(usize, usize)>,
}

impl Sources {
    pub fn is_empty(&self) -> bool {
        self.cells.is_empty() && self.tables.is_empty()
    }

    /// The colour number of the source covering cell (`r`, `c`).
    pub fn at(&self, r: usize, c: usize) -> Option<usize> {
        self.cells.iter().find(|&&(((r0, c0), (r1, c1)), _)| (r0..=r1).contains(&r) && (c0..=c1).contains(&c)).map(|&(_, k)| k)
    }

    /// The colour number of a sheet the formula takes data from.
    pub fn table(&self, sheet: usize) -> Option<usize> {
        self.tables.iter().find(|&&(s, _)| s == sheet).map(|&(_, k)| k)
    }
}

impl App {
    /// The sources of the formula being edited (empty when no formula is
    /// edited in a table).
    pub fn formula_sources(&self) -> Sources {
        let Mode::Edit { buf, .. } = &self.mode else { return Sources::default() };
        if self.view.kind != ViewKind::Table || self.view.pivot.is_some() || self.solo.is_some() || self.on_header().is_some() {
            return Sources::default();
        }
        let Some(formula) = buf.trim_start().strip_prefix('=') else { return Sources::default() };
        let (rows, cols) = self.dims();
        let sheet = &self.wb.sheets[self.view.sheet];
        let names: Vec<String> = sheet.columns.iter().map(|c| c.name.clone()).collect();
        let tables: Vec<String> = self.wb.sheets.iter().filter_map(|s| s.name.clone()).collect();
        let mut out = Sources::default();
        for (k, found) in scan(formula, &names, rows, cols, &tables, sheet.name.as_deref()).into_iter().enumerate() {
            match found {
                Found::Cells(area) => out.cells.push((area, k)),
                Found::Table(name) => {
                    if let Some(i) = self.wb.sheets.iter().position(|s| s.name.as_deref() == Some(name.as_str())) {
                        out.tables.push((i, k));
                    }
                }
            }
        }
        out
    }
}

/// What `formula` (without `=`) takes data from: areas of this table (`B2`,
/// `B2:C5`, whole columns `B`, `B:C`, a column name) and other tables
/// (`Sales!B2`, `"My sheet"!B2`, `Sales[North, Value]`; `tables` are all
/// table names, `own` this one's). Not: function names, slices (`.x`),
/// indexes (`[…]`), strings and unit literals. Columns span `rows` rows;
/// areas beyond `cols` are left out.
pub fn scan(formula: &str, names: &[String], rows: usize, cols: usize, tables: &[String], own: Option<&str>) -> Vec<Found> {
    let text = crate::refs::expand_short_ranges(formula);
    let chars: Vec<char> = text.chars().collect();
    let next_char = |mut i: usize| {
        while i < chars.len() && chars[i] == ' ' {
            i += 1;
        }
        (i, chars.get(i).copied())
    };
    let ident_at = |i: usize| {
        let mut j = i;
        while j < chars.len() && (chars[j].is_alphanumeric() || chars[j] == '_' || chars[j] == '$') {
            j += 1;
        }
        (chars[i..j].iter().collect::<String>(), j)
    };
    // A reference as (column range, row range); `None` rows: the whole column.
    let reference = |ident: &str| -> Option<(usize, Option<usize>)> {
        if let Some(c) = names.iter().position(|n| n == ident) {
            return Some((c, None));
        }
        let p = RefParts::parse(ident)?;
        Some((p.col, p.row.map(|(r, _)| r)))
    };
    let mut out: Vec<Found> = Vec::new();
    let table = |out: &mut Vec<Found>, name: &str| {
        let found = Found::Table(name.to_string());
        if tables.iter().any(|t| t == name) && !out.contains(&found) {
            out.push(found);
        }
    };
    let push = |out: &mut Vec<Found>, a: (usize, Option<usize>), b: (usize, Option<usize>)| {
        let (c0, c1) = (a.0.min(b.0), a.0.max(b.0));
        let (r0, r1) = match (a.1, b.1) {
            (Some(x), Some(y)) => (x.min(y), x.max(y)),
            _ if rows == 0 => return,
            _ => (0, rows - 1),
        };
        if c0 >= cols {
            return;
        }
        let area = Found::Cells(((r0, c0), (r1, c1.min(cols - 1))));
        if !out.contains(&area) {
            out.push(area);
        }
    };
    let (mut i, mut brackets, mut last, mut other_table) = (0, 0usize, ' ', false);
    while i < chars.len() {
        let c = chars[i];
        if c == '"' {
            let start = i + 1;
            i += 1;
            while i < chars.len() {
                if chars[i] == '"' && chars.get(i + 1) == Some(&'"') {
                    i += 2;
                } else {
                    i += 1;
                    if chars[i - 1] == '"' {
                        break;
                    }
                }
            }
            if let (after, Some('!')) = next_char(i) {
                let name: String = chars[start..i.saturating_sub(1)].iter().collect::<String>().replace("\"\"", "\"");
                other_table = own != Some(name.as_str());
                if other_table {
                    table(&mut out, &name);
                }
                i = after + 1;
            }
            continue;
        }
        if c == '{' {
            i = chars[i..].iter().position(|&c| c == '}').map_or(chars.len(), |p| i + p + 1);
            continue;
        }
        if c.is_ascii_digit() {
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '.') {
                i += 1;
            }
            last = '0';
            continue;
        }
        if c.is_alphabetic() || c == '_' || c == '$' {
            let (ident, end) = ident_at(i);
            let (after, next) = next_char(end);
            i = end;
            match next {
                Some('!') => {
                    other_table = own != Some(ident.as_str());
                    if other_table {
                        table(&mut out, &ident);
                    }
                    i = after + 1;
                    continue;
                }
                // `Sales[North, Value]`: another table by key (a column name wins).
                Some('[') if last != '.' && !names.contains(&ident) && tables.contains(&ident) => {
                    if own != Some(ident.as_str()) {
                        table(&mut out, &ident);
                    }
                }
                Some('(') => {}
                _ if last == '.' || brackets > 0 => {}
                _ => {
                    let skip = std::mem::take(&mut other_table);
                    let first = reference(&ident);
                    // A range: the identifier after `:`.
                    let second = if next == Some(':') {
                        let (start, ch) = next_char(after + 1);
                        if ch.is_some_and(|ch| ch.is_alphanumeric() || ch == '$') {
                            let (b, end) = ident_at(start);
                            i = end;
                            reference(&b)
                        } else {
                            None
                        }
                    } else {
                        None
                    };
                    if !skip && let Some(a) = first {
                        push(&mut out, a, second.unwrap_or(a));
                    }
                }
            }
            last = 'a';
            continue;
        }
        match c {
            '[' => brackets += 1,
            ']' => brackets = brackets.saturating_sub(1),
            _ => {}
        }
        if !c.is_whitespace() {
            last = c;
        }
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names() -> Vec<String> {
        ["Item", "Amount", "Status"].iter().map(|s| s.to_string()).collect()
    }

    fn tables() -> Vec<String> {
        ["Expenses", "Sales", "My sheet"].iter().map(|s| s.to_string()).collect()
    }

    fn cells(formula: &str) -> Vec<Area> {
        scan(formula, &names(), 4, 5, &tables(), Some("Expenses"))
            .into_iter()
            .filter_map(|f| match f {
                Found::Cells(a) => Some(a),
                Found::Table(_) => None,
            })
            .collect()
    }

    fn other(formula: &str) -> Vec<String> {
        scan(formula, &names(), 4, 5, &tables(), Some("Expenses"))
            .into_iter()
            .filter_map(|f| match f {
                Found::Table(t) => Some(t),
                Found::Cells(_) => None,
            })
            .collect()
    }

    #[test]
    fn cells_ranges_and_columns() {
        assert_eq!(cells("B1/sum(B)"), vec![((0, 1), (0, 1)), ((0, 1), (3, 1))]);
        assert_eq!(cells("sum(B2:C3) + $A$1"), vec![((1, 1), (2, 2)), ((0, 0), (0, 0))]);
        assert_eq!(cells("C3:B2"), vec![((1, 1), (2, 2))]);
        assert_eq!(cells("Amount * 1.19"), vec![((0, 1), (3, 1))]);
        // Short range.
        assert_eq!(cells("sum(B2:4)"), vec![((1, 1), (3, 1))]);
        // The same area once.
        assert_eq!(cells("B1 + B1"), vec![((0, 1), (0, 1))]);
        // Qualified with this table's own name.
        assert_eq!(cells("Expenses!B1"), vec![((0, 1), (0, 1))]);
    }

    #[test]
    fn not_cells_of_this_table() {
        assert!(cells("Sales!B2 + \"B2\" + round(3) + x.B + y[B2] + 5{m}").is_empty());
        assert!(cells("\"My sheet\"!B2:C3").is_empty());
        // Beyond the table.
        assert!(cells("Z9").is_empty());
    }

    #[test]
    fn other_tables() {
        assert_eq!(other("Sales!B2 + \"My sheet\"!Amount + Sales!C1"), vec!["Sales", "My sheet"]);
        assert_eq!(other("Sales[North, Value] + @Sales!B"), vec!["Sales"]);
        // Not tables: its own name, unknown names, strings without `!`.
        assert!(other("Expenses!B2 + Nowhere!B2 + \"Sales\"").is_empty());
        // The order of all sources gives the colours.
        let all = scan("B1 + Sales!B2 + C1", &names(), 4, 5, &tables(), Some("Expenses"));
        assert_eq!(all, vec![Found::Cells(((0, 1), (0, 1))), Found::Table("Sales".into()), Found::Cells(((0, 2), (0, 2)))]);
    }
}
