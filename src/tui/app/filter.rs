//! Column filters: the rules of the `filter` layer hide the rows that do not
//! meet them. The file keeps every row and only the rules; hiding happens
//! here, in the view (and when printing).

use super::{App, ViewKind, FILTER};
use crate::engine::col_letters;
use crate::layers;
use crate::value::Value;

impl App {
    /// Recomputes which rows the filters hide – for every table with filter
    /// rules, on each of its levels (a slice is judged by its own values).
    /// `subtotal` counts what the view shows: all rows while the filter layer
    /// of a table is switched off.
    pub(super) fn sync_filter_switch(&mut self) {
        for sheet in 0..self.wb.sheets.len() {
            self.wb.set_filter_off(sheet, !self.layer_visible_in(FILTER, sheet));
        }
    }

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
        let text = args.join(" ");
        let (c, body) = match layers::split_column(&text, |name| self.find_column(name)) {
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

impl App {
    /// `:when Amount > 1000 fg red bold`: formats the cells of a column that
    /// meet a condition (a rule `Amount[> 1000]: fg red bold` in the format
    /// layer); `off` instead of a format removes the rule.
    pub(super) fn when_command(&mut self, args: &[&str]) {
        const USAGE: &str = "usage: :when Column condition format – e.g. :when Amount > 1000 fg red bold · :when Status = open bg yellow · :when Amount > 1000 off";
        if self.view.kind != ViewKind::Table || self.view.pivot.is_some() {
            return self.error(":when works in tables");
        }
        let Some((c, rest)) = layers::split_column(&args.join(" "), |name| self.find_column(name)) else {
            return self.error(USAGE);
        };
        let rest: Vec<&str> = rest.split_whitespace().collect();
        // The format begins where a condition stands before it (more than a
        // comparison) and the rest reads as a format: `= none bg red`.
        let complete = |k: usize| rest[..k].iter().any(|w| w.chars().any(|ch| !matches!(ch, '<' | '>' | '=' | '!')));
        let fits = |k: usize| matches!(rest[k..], ["off"] | ["none"]) || layers::CellStyle::parse(&rest[k..].join(" ")).1.is_empty();
        // Else the first word that starts a format, so that its error is shown.
        let starts_format = |k: usize| matches!(rest[k], "fg" | "bg" | "bold" | "italic" | "underline" | "align" | "wrap") || ["percent", "decimals", "no"].iter().any(|p| rest[k].starts_with(p));
        let candidates = || (1..rest.len()).filter(|&k| complete(k));
        let Some(k) = candidates().find(|&k| fits(k)).or_else(|| candidates().find(|&k| starts_format(k))) else { return self.error(USAGE) };
        let condition = match normalize(&rest[..k].join(" ")) {
            Ok(c) => c,
            Err(e) => return self.error(e),
        };
        let format = rest[k..].join(" ");
        let remove = matches!(format.as_str(), "off" | "none");
        if !remove {
            let (_, errors) = layers::CellStyle::parse(&format);
            if let Some(e) = errors.first() {
                return self.error(format!("{e} – formats: fg red, bg yellow, bold, italic, underline"));
            }
        }
        let sheet = self.view.sheet;
        let name = self.column_target(super::FORMAT, c);
        let target = format!("{name}[{condition}]");
        let had = self.layer_rule(super::FORMAT, sheet, &target).is_some();
        if remove && !had {
            return self.info(format!("no rule {target} to remove"));
        }
        self.set_layer_rules(super::FORMAT, sheet, &[(target.clone(), if remove { String::new() } else { format.clone() })]);
        self.hidden_layers.remove(super::FORMAT);
        self.layer_overrides.remove(&(sheet, super::FORMAT.to_string()));
        self.revalidate();
        self.info(if remove { format!("rule {target} removed") } else { format!("format rule {target}: {format} · :when … off removes it") });
    }
}

use crate::layers::normalize_condition as normalize;

#[cfg(test)]
mod tests {
    use crate::layers::normalize_condition as normalize;

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
