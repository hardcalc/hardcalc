//! The sources of the formula being edited: the cells and ranges of this
//! table it refers to, so the grid can highlight them (and let data flow
//! from them to the formula cell).

use super::{App, Mode, ViewKind};
use crate::engine::RefParts;

/// A rectangle of cells: ((top, left), (bottom, right)).
pub type Area = ((usize, usize), (usize, usize));

impl App {
    /// The areas of this table the formula being edited refers to, in the
    /// order they appear (empty when no formula is edited in a table).
    pub fn formula_sources(&self) -> Vec<Area> {
        let Mode::Edit { buf, .. } = &self.mode else { return Vec::new() };
        if self.view.kind != ViewKind::Table || self.view.pivot.is_some() || self.solo.is_some() || self.on_header().is_some() {
            return Vec::new();
        }
        let Some(formula) = buf.trim_start().strip_prefix('=') else { return Vec::new() };
        let (rows, cols) = self.dims();
        let names: Vec<String> = self.wb.sheets[self.view.sheet].columns.iter().map(|c| c.name.clone()).collect();
        scan(formula, &names, rows, cols)
    }
}

/// The areas referred to in `formula` (without `=`): `B2`, `B2:C5`, whole
/// columns (`B`, `B:C`, a column name). Not: references to other tables
/// (`Sales!B2`), function names, members (`.x`), indexes (`[…]`), strings
/// and unit literals. Columns span `rows` rows; areas beyond `cols` are left out.
pub fn scan(formula: &str, names: &[String], rows: usize, cols: usize) -> Vec<Area> {
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
    let mut out: Vec<Area> = Vec::new();
    let mut push = |a: (usize, Option<usize>), b: (usize, Option<usize>)| {
        let (c0, c1) = (a.0.min(b.0), a.0.max(b.0));
        let (r0, r1) = match (a.1, b.1) {
            (Some(x), Some(y)) => (x.min(y), x.max(y)),
            _ if rows == 0 => return,
            _ => (0, rows - 1),
        };
        if c0 >= cols {
            return;
        }
        let area = ((r0, c0), (r1, c1.min(cols - 1)));
        if !out.contains(&area) {
            out.push(area);
        }
    };
    let (mut i, mut brackets, mut last, mut other_table) = (0, 0usize, ' ', false);
    while i < chars.len() {
        let c = chars[i];
        if c == '"' {
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
            other_table = next_char(i).1 == Some('!');
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
                    other_table = true;
                    i = after + 1;
                    continue;
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
                        push(a, second.unwrap_or(a));
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

    #[test]
    fn cells_ranges_and_columns() {
        let n = names();
        assert_eq!(scan("B1/sum(B)", &n, 4, 5), vec![((0, 1), (0, 1)), ((0, 1), (3, 1))]);
        assert_eq!(scan("sum(B2:C3) + $A$1", &n, 4, 5), vec![((1, 1), (2, 2)), ((0, 0), (0, 0))]);
        assert_eq!(scan("C3:B2", &n, 4, 5), vec![((1, 1), (2, 2))]);
        assert_eq!(scan("Amount * 1.19", &n, 4, 5), vec![((0, 1), (3, 1))]);
        // Short range.
        assert_eq!(scan("sum(B2:4)", &n, 4, 5), vec![((1, 1), (3, 1))]);
        // The same area once.
        assert_eq!(scan("B1 + B1", &n, 4, 5), vec![((0, 1), (0, 1))]);
    }

    #[test]
    fn not_sources() {
        let n = names();
        assert!(scan("Sales!B2 + \"B2\" + round(3) + x.B + y[B2] + 5{m}", &n, 4, 5).is_empty());
        assert!(scan("\"Other table\"!B2", &n, 4, 5).is_empty());
        // Beyond the table.
        assert!(scan("Z9", &n, 4, 5).is_empty());
    }
}
