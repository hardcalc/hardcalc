//! How columns are shown: the `view` layer hides columns, fixes their width
//! and freezes the first ones while scrolling sideways. Like the filter it
//! changes nothing in the data – only one rule per column in the file.

use super::{App, Header, ViewKind, FILTER, VIEW};
use crate::engine::col_letters;
use crate::layers::{self, ColumnView};

impl App {
    /// The column settings that act in the shown table: in a table (not its
    /// pivot), outside the layer view, the `view` layer switched on.
    fn column_view(&self) -> Option<&ColumnView> {
        if self.view.kind != ViewKind::Table || self.view.pivot.is_some() || self.solo.is_some() || !self.layer_visible_in(VIEW, self.view.sheet) {
            return None;
        }
        self.column_views.get(&self.view.sheet)
    }

    /// Whether the view hides column `c` of the shown table.
    pub fn col_hidden(&self, c: usize) -> bool {
        self.column_view().is_some_and(|v| v.is_hidden(c))
    }

    /// Whether column `c` is drawn: the column with the cursor always is.
    pub fn col_shown(&self, c: usize) -> bool {
        let in_column = !matches!(self.view.header, Some(Header::Row | Header::Corner));
        !self.col_hidden(c) || in_column && (c == self.view.cursor.1 || c == self.focus_cell().1)
    }

    /// The fixed width of column `c`, if it has one.
    pub fn col_width(&self, c: usize) -> Option<usize> {
        self.column_view().and_then(|v| v.width.get(c).copied().flatten()).map(|w| w as usize)
    }

    /// How many of the first columns stay in place while scrolling sideways.
    pub fn frozen_cols(&self) -> usize {
        self.column_view().map_or(0, |v| v.frozen)
    }

    /// Number of columns the view hides in the shown table.
    pub fn hidden_cols(&self) -> usize {
        self.column_view().map_or(0, |v| v.hidden_count())
    }

    /// The column `d` shown columns away from `from`, stopping at the edges.
    pub fn step_col(&self, from: usize, d: isize) -> usize {
        let last = self.view_dims().1.saturating_sub(1);
        if self.hidden_cols() == 0 {
            return (from as isize).saturating_add(d).clamp(0, last as isize) as usize;
        }
        let mut at = from.min(last);
        for _ in 0..d.unsigned_abs() {
            let next = if d > 0 { (at + 1..=last).find(|&c| !self.col_hidden(c)) } else { (0..at).rev().find(|&c| !self.col_hidden(c)) };
            match next {
                Some(c) => at = c,
                None => break,
            }
        }
        at
    }

    /// Moves the cursor off a hidden column, to the next shown one.
    pub(super) fn snap_to_shown_col(&mut self) {
        let c = self.view.cursor.1;
        if self.col_hidden(c) {
            let last = self.view_dims().1.saturating_sub(1);
            if let Some(next) = (c..=last).find(|&x| !self.col_hidden(x)).or_else(|| (0..c).rev().find(|&x| !self.col_hidden(x))) {
                self.view.cursor.1 = next;
                self.view.anchor = None;
                self.view.follow = true;
            }
        }
    }

    /// The target a rule of `layer` for column `c` is written with: that of
    /// its rule if there is one, else the column's name (or letter).
    pub(super) fn column_target(&self, layer: &str, c: usize) -> String {
        let sheet = self.view.sheet;
        let existing = self.attached.iter().filter(|a| a.sheet == sheet && a.layer.name == layer).flat_map(|a| &a.layer.rules).find_map(|r| match r {
            crate::model::Rule::Rule { target, .. } if layers::column_of(&self.wb, sheet, target) == Some(c) => Some(target.clone()),
            _ => None,
        });
        existing.unwrap_or_else(|| match self.wb.sheets[sheet].columns.get(c) {
            Some(col) if layers::column_of(&self.wb, sheet, &col.name) == Some(c) => col.name.clone(),
            _ => col_letters(c),
        })
    }

    /// The target a filter rule for column `c` is written with.
    pub(super) fn filter_target(&self, c: usize) -> String {
        self.column_target(FILTER, c)
    }

    /// The columns a column command applies to: those of the selection (a
    /// range or column headers), else the cursor column – data columns only.
    fn chosen_columns(&self) -> Vec<usize> {
        let cols = self.dims().1;
        let ((_, c0), (_, c1)) = self.selection();
        (c0..=c1).filter(|&c| c < cols).collect()
    }

    /// Sets (`Some`) or removes (`None`) one setting – `hide`, `freeze` or
    /// `width` – in the view rules of `columns`, keeping their other settings.
    fn set_view_setting(&mut self, columns: &[usize], family: &str, value: Option<String>) {
        let sheet = self.view.sheet;
        let mut changes = Vec::new();
        for &c in columns {
            let target = self.column_target(VIEW, c);
            let body = self.layer_rule(VIEW, sheet, &target).unwrap_or_default();
            // `freeze width 12 hide` → its settings, the one of `family` replaced.
            let mut parts: Vec<String> = Vec::new();
            let mut words = body.split_whitespace();
            while let Some(w) = words.next() {
                let part = if w == "width" { format!("width {}", words.next().unwrap_or("")) } else { w.to_string() };
                if part.split(' ').next() != Some(family) {
                    parts.push(part);
                }
            }
            parts.extend(value.clone());
            let new = parts.join(" ");
            if new != body {
                changes.push((target, new));
            }
        }
        if changes.is_empty() {
            return;
        }
        self.set_layer_rules(VIEW, sheet, &changes);
        // What was set shall be seen.
        self.hidden_layers.remove(VIEW);
        self.layer_overrides.remove(&(sheet, VIEW.to_string()));
        self.revalidate();
    }

    fn names_of(&self, columns: &[usize]) -> String {
        let cols = &self.wb.sheets[self.view.sheet].columns;
        columns.iter().map(|&c| cols[c].name.clone()).collect::<Vec<_>>().join(", ")
    }

    fn in_table(&mut self, command: &str) -> bool {
        let ok = self.view.kind == ViewKind::Table && self.view.pivot.is_none();
        if !ok {
            self.error(format!(":{command} works in tables"));
        }
        ok
    }

    /// `:hide [Column …]`: the named columns, else those of the selection.
    pub(super) fn hide_columns(&mut self, args: &[&str]) {
        if !self.in_table("hide") {
            return;
        }
        let columns = match self.named_columns(args) {
            Ok(Some(named)) => named,
            Ok(None) => self.chosen_columns(),
            Err(e) => return self.error(e),
        };
        if columns.is_empty() {
            return self.error("no data column here to hide");
        }
        let cols = self.dims().1;
        if (0..cols).all(|c| columns.contains(&c) || self.col_hidden(c)) {
            return self.error("one column has to stay – :unhide shows the hidden ones");
        }
        self.set_view_setting(&columns, "hide", Some("hide".into()));
        self.view.anchor = None;
        self.view.header = None;
        self.snap_to_shown_col();
        self.info(format!("hidden: {} · :unhide shows all columns, a click on the view layer for now", self.names_of(&columns)));
    }

    /// `:unhide [Column …]`: the named columns, else all hidden ones.
    pub(super) fn unhide_columns(&mut self, args: &[&str]) {
        if !self.in_table("unhide") {
            return;
        }
        let hidden: Vec<usize> = self.column_views.get(&self.view.sheet).map(|v| (0..v.hidden.len()).filter(|&c| v.is_hidden(c)).collect()).unwrap_or_default();
        let columns = match self.named_columns(args) {
            Ok(Some(named)) => named.into_iter().filter(|c| hidden.contains(c)).collect(),
            Ok(None) => hidden,
            Err(e) => return self.error(e),
        };
        if columns.is_empty() {
            return self.info("no hidden column");
        }
        self.set_view_setting(&columns, "hide", None);
        self.info(format!("shown again: {}", self.names_of(&columns)));
    }

    /// `:width n | auto`: the width of the selected columns in characters.
    pub(super) fn set_width(&mut self, args: &[&str]) {
        if !self.in_table("width") {
            return;
        }
        let columns = self.chosen_columns();
        if columns.is_empty() {
            return self.error("no data column here");
        }
        let value = match args {
            ["auto" | "off" | "none"] => None,
            [n] => match n.parse::<u16>() {
                Ok(n) if (1..=200).contains(&n) => Some(format!("width {n}")),
                _ => return self.error("usage: :width 20 (characters, 1–200) or :width auto"),
            },
            _ => return self.error("usage: :width 20 (characters) or :width auto – for the selected columns"),
        };
        let auto = value.is_none();
        self.set_view_setting(&columns, "width", value);
        let names = self.names_of(&columns);
        self.info(if auto { format!("{names}: width as the content needs") } else { format!("{names}: width {}", args[0]) });
    }

    /// `:freeze [Column | off]`: the columns up to this one (without a
    /// name: the cursor column) stay in place while scrolling sideways.
    pub(super) fn freeze_columns(&mut self, args: &[&str]) {
        if !self.in_table("freeze") {
            return;
        }
        let cols = self.dims().1;
        let frozen: Vec<usize> = (0..cols).filter(|&c| self.layer_rule(VIEW, self.view.sheet, &self.column_target(VIEW, c)).is_some_and(|b| b.split_whitespace().any(|w| w == "freeze"))).collect();
        if let ["off" | "none"] = args {
            if frozen.is_empty() {
                return self.info("no column is frozen");
            }
            self.set_view_setting(&frozen, "freeze", None);
            return self.info("no column is frozen any more");
        }
        let c = match self.named_columns(args) {
            Ok(Some(named)) if named.len() == 1 => named[0],
            Ok(None) if self.view.cursor.1 < cols => self.view.cursor.1,
            Ok(_) => return self.error("usage: :freeze [Column] – the columns up to it stay while scrolling sideways · :freeze off"),
            Err(e) => return self.error(e),
        };
        // One rule says it: that of the last frozen column.
        let others: Vec<usize> = frozen.into_iter().filter(|&f| f != c).collect();
        if !others.is_empty() {
            self.set_view_setting(&others, "freeze", None);
        }
        self.set_view_setting(&[c], "freeze", Some("freeze".into()));
        self.info(format!("columns A–{} stay in place while scrolling sideways · :freeze off", col_letters(c)));
    }

    /// The columns named in `args` (each a name or letter; a name of
    /// several words as a whole); `None` without arguments.
    fn named_columns(&self, args: &[&str]) -> Result<Option<Vec<usize>>, String> {
        if args.is_empty() {
            return Ok(None);
        }
        if let Some(c) = self.find_column(&args.join(" ")) {
            return Ok(Some(vec![c]));
        }
        args.iter().map(|a| self.find_column(a).ok_or_else(|| format!("no column '{a}'"))).collect::<Result<Vec<_>, _>>().map(Some)
    }
}
