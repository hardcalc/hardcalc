//! Column filters: the rules of the `filter` layer hide the rows that do not
//! meet them. The file keeps every row and only the rules; hiding happens
//! here, in the view (and when printing).

use super::{App, ViewKind, FILTER};
use crate::engine::col_letters;
use crate::layers::{self, Check};
use crate::value::Value;

impl App {
    /// Recomputes which rows the filters hide – for every table with filter
    /// rules, on each of its levels (a slice is judged by its own values).
    pub(super) fn refilter(&mut self) {
        self.hidden_rows.clear();
        for sheet in 0..self.wb.sheets.len() {
            if self.filters.applies(sheet) {
                for z in 0..=self.wb.sheets[sheet].layers.len() {
                    let hidden = self.filters.hidden(&mut self.wb, sheet, z);
                    self.hidden_rows.insert((sheet, z), hidden);
                }
            }
        }
    }

    /// The rows the filter hides in the shown table, while it acts: in a
    /// table (not its pivot), outside the layer view, the layer switched on.
    fn hidden_flags(&self) -> Option<&Vec<bool>> {
        if self.view.kind != ViewKind::Table || self.view.pivot.is_some() || self.solo.is_some() || !self.layer_visible_in(FILTER, self.view.sheet) {
            return None;
        }
        self.hidden_rows.get(&(self.view.sheet, self.shown_layer()))
    }

    /// Whether the filter hides data row `r` of the shown table.
    pub fn row_hidden(&self, r: usize) -> bool {
        self.hidden_flags().is_some_and(|h| h.get(r) == Some(&true))
    }

    /// Whether row `r` is drawn: the row with the cursor always is, so that
    /// a row being typed does not vanish under the cursor.
    pub fn row_shown(&self, r: usize) -> bool {
        // On a column header or the table origin the cursor is in no row.
        let in_row = !matches!(self.view.header, Some(super::Header::Column | super::Header::Corner));
        !self.row_hidden(r) || in_row && (r == self.view.cursor.0 || r == self.focus_cell().0)
    }

    /// (hidden, all) data rows of the shown table, if a filter hides some.
    pub fn hidden_count(&self) -> Option<(usize, usize)> {
        let flags = self.hidden_flags()?;
        let n = flags.iter().filter(|h| **h).count();
        (n > 0).then_some((n, flags.len()))
    }

    /// Whether column `c` of the shown table has a filter rule that acts.
    pub fn column_filtered(&self, c: usize) -> bool {
        self.hidden_flags().is_some() && self.filters.columns(&self.wb, self.view.sheet).contains(&c)
    }

    /// Whether the header of column `c` offers the value list with a ▾: the
    /// named data column the cursor is in, in a table shown as such.
    pub fn values_button(&self, c: usize) -> bool {
        let sheet = &self.wb.sheets[self.view.sheet];
        self.view.kind == ViewKind::Table
            && self.view.pivot.is_none()
            && self.solo.is_none()
            && c == self.view.cursor.1
            && !sheet.cells.is_empty()
            && sheet.columns.get(c).is_some_and(|col| col.name != col_letters(c))
    }

    /// Up to `n` drawn rows from view row `start` on.
    pub fn rows_from(&self, start: usize, n: usize) -> Vec<usize> {
        let rows = self.view_dims().0;
        if self.hidden_flags().is_none() {
            return (start..rows.min(start.saturating_add(n))).collect();
        }
        (start..rows).filter(|&r| self.row_shown(r)).take(n).collect()
    }

    /// The row `d` shown rows away from `from` (hidden rows do not count),
    /// stopping at the edges of the view.
    pub fn step_row(&self, from: usize, d: isize) -> usize {
        let last = self.view_dims().0.saturating_sub(1);
        if self.hidden_flags().is_none() {
            return (from as isize).saturating_add(d).clamp(0, last as isize) as usize;
        }
        let mut at = from.min(last);
        for _ in 0..d.unsigned_abs() {
            let next = if d > 0 { (at + 1..=last).find(|&r| !self.row_hidden(r)) } else { (0..at).rev().find(|&r| !self.row_hidden(r)) };
            match next {
                Some(r) => at = r,
                None => break,
            }
        }
        at
    }

    /// The rows of `r0..=r1` an edit applies to: not those the filter hides.
    pub(super) fn shown_in(&self, r0: usize, r1: usize) -> Vec<usize> {
        (r0..=r1).filter(|&r| self.row_shown(r)).collect()
    }

    /// Whether the filter hides rows between `r0` and `r1`.
    pub(super) fn hides_between(&self, r0: usize, r1: usize) -> bool {
        self.hidden_flags().is_some() && (r0..=r1).any(|r| !self.row_shown(r))
    }

    /// Moves the cursor off a hidden row, to the next shown one (after the
    /// filter changed; a row reached on purpose – go to, a link – stays).
    pub(super) fn snap_to_shown(&mut self) {
        let r = self.view.cursor.0;
        if self.row_hidden(r) {
            let last = self.view_dims().0.saturating_sub(1);
            let next = (r..=last).find(|&x| !self.row_hidden(x)).or_else(|| (0..r).rev().find(|&x| !self.row_hidden(x)));
            if let Some(next) = next {
                self.view.cursor.0 = next;
                self.view.anchor = None;
                self.view.follow = true;
            }
        }
    }

    /// The target a filter rule for column `c` is written with: that of its
    /// rule if there is one, else the column's name (or letter).
    pub(super) fn filter_target(&self, c: usize) -> String {
        let sheet = self.view.sheet;
        let existing = self.attached.iter().filter(|a| a.sheet == sheet && a.layer.name == FILTER).flat_map(|a| &a.layer.rules).find_map(|r| match r {
            crate::model::Rule::Rule { target, .. } if layers::column_of(&self.wb, sheet, target) == Some(c) => Some(target.clone()),
            _ => None,
        });
        existing.unwrap_or_else(|| match self.wb.sheets[sheet].columns.get(c) {
            Some(col) if layers::column_of(&self.wb, sheet, &col.name) == Some(c) => col.name.clone(),
            _ => col_letters(c),
        })
    }

    /// The condition that keeps the rows with the value of cell (r, c).
    fn condition_for(&mut self, r: usize, c: usize) -> String {
        let id = (self.view.sheet, r, crate::engine::in_layer(c, self.shown_layer()));
        match self.wb.value(id) {
            Value::Null => "empty".to_string(),
            Value::Text(s) if s.trim().is_empty() => "empty".to_string(),
            Value::Num(n, _) => format!("= {n}"),
            v => format!("= {}", layers::quote_value(&v.display(None, false))),
        }
    }

    /// `:filter` – see the command list.
    pub(super) fn filter_command(&mut self, args: &[&str]) {
        if self.view.kind != ViewKind::Table || self.view.pivot.is_some() {
            return self.error(":filter works in tables");
        }
        let (rows, cols) = self.dims();
        let sheet = self.view.sheet;
        let cursor = self.view.cursor;
        // `:filter off [Column]`: all rules of this table, or the column's.
        if let Some(("off" | "none" | "clear", rest)) = args.split_first().map(|(a, rest)| (*a, rest)) {
            let targets: Vec<String> = if rest.is_empty() {
                self.filters.columns(&self.wb, sheet).into_iter().map(|c| self.filter_target(c)).collect()
            } else {
                match self.find_column(&rest.join(" ")) {
                    Some(c) => vec![self.filter_target(c)],
                    None => return self.error(format!("no column '{}'", rest.join(" "))),
                }
            };
            let had = self.attached.iter().any(|a| a.sheet == sheet && a.layer.name == FILTER && a.layer.rules.iter().any(|r| matches!(r, crate::model::Rule::Rule { target, .. } if targets.contains(target))));
            if !had {
                return self.info("no filter to remove");
            }
            let changes: Vec<(String, String)> = targets.into_iter().map(|t| (t, String::new())).collect();
            self.set_layer_rules(FILTER, sheet, &changes);
            return self.info(if rest.is_empty() { "filter removed – all rows shown".to_string() } else { format!("filter on {} removed", rest.join(" ")) });
        }
        // The column: the longest beginning that names one, or what stands
        // before a comparison (`Amount>=100`).
        let text = args.join(" ");
        let named = (1..=args.len()).rev().find_map(|k| self.find_column(&args[..k].join(" ")).map(|c| (c, args[k..].join(" "))));
        let split = || {
            let i = text.find(['<', '>', '=', '!'])?;
            self.find_column(text[..i].trim()).map(|c| (c, text[i..].to_string()))
        };
        let (c, body) = match named.or_else(split) {
            Some(found) => found,
            None if args.is_empty() && cursor.1 < cols => (cursor.1, String::new()),
            None if args.is_empty() => return self.error("usage: :filter Column condition – e.g. :filter Status = open, :filter Amount >= 100, :filter off"),
            None => return self.error(format!("'{text}' does not start with a column – e.g. :filter Status = open, :filter Amount >= 100, :filter off")),
        };
        // Without a condition: the value in the cursor row.
        let body = if !body.trim().is_empty() {
            normalize(body.trim())
        } else if cursor.0 < rows {
            Ok(self.condition_for(cursor.0, c))
        } else {
            Err("place the cursor on a value to filter by, or name a condition (:filter Status = open)".to_string())
        };
        let body = match body {
            Ok(b) => b,
            Err(e) => return self.error(e),
        };
        self.set_filter(c, Some(body));
    }
}

/// A condition as typed, in the form the layer keeps: `open` → `= open`,
/// `= due soon` → `= "due soon"`.
fn normalize(body: &str) -> Result<String, String> {
    let ok = |b: &str| Check::parse_all(b).is_ok_and(|c| !c.is_empty() && !c.contains(&Check::Unique));
    if ok(body) {
        return Ok(body.to_string());
    }
    let ops = ["<=", ">=", "<>", "!=", "=", "<", ">"];
    let candidate = match ops.iter().find(|op| body.starts_with(**op)) {
        Some(op) => format!("{op} {}", layers::quote_value(body[op.len()..].trim())),
        None => format!("= {}", layers::quote_value(body)),
    };
    if ok(&candidate) { Ok(candidate) } else { Err(format!("'{body}' is no condition – e.g. = open, >= 100, in(a, b), contains(text), empty, required")) }
}

#[cfg(test)]
mod tests {
    use super::normalize;

    #[test]
    fn conditions_as_typed() {
        assert_eq!(normalize(">= 100").unwrap(), ">= 100");
        assert_eq!(normalize("open").unwrap(), "= open");
        assert_eq!(normalize("= due soon").unwrap(), "= \"due soon\"");
        assert_eq!(normalize("due soon").unwrap(), "= \"due soon\"");
        assert_eq!(normalize("in(a, b)").unwrap(), "in(a, b)");
        assert_eq!(normalize("<>").unwrap(), "<> \"\"");
        assert!(normalize("in(a").is_ok(), "read as text");
    }
}
