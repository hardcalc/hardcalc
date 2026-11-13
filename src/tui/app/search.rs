//! Find and replace in tables: `:find text` (Ctrl+F), Ctrl+G for the next
//! match, `:replace old new`.

use super::{App, ViewKind};
use crate::engine::{col_letters, in_layer};

/// The words of a command line, `"several words"` in quotes as one
/// (`""` for a quote inside).
pub fn split_args(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut chars = line.trim().chars().peekable();
    while chars.peek().is_some() {
        let mut word = String::new();
        if chars.peek() == Some(&'"') {
            chars.next();
            while let Some(ch) = chars.next() {
                match ch {
                    '"' if chars.peek() == Some(&'"') => {
                        chars.next();
                        word.push('"');
                    }
                    '"' => break,
                    _ => word.push(ch),
                }
            }
        } else {
            while let Some(ch) = chars.next_if(|c| !c.is_whitespace()) {
                word.push(ch);
            }
        }
        out.push(word);
        while chars.next_if(|c| c.is_whitespace()).is_some() {}
    }
    out
}

/// Replaces every `needle` in `hay` – upper and lower case alike – and
/// counts the replacements.
pub fn replace_all(hay: &str, needle: &str, with: &str) -> (String, usize) {
    let (low, low_needle) = (hay.to_lowercase(), needle.to_lowercase());
    // Lower-casing keeps the positions for all but a few letters; else case counts.
    if low.len() != hay.len() || low_needle.is_empty() {
        let n = if needle.is_empty() { 0 } else { hay.matches(needle).count() };
        return (if n > 0 { hay.replace(needle, with) } else { hay.to_string() }, n);
    }
    let mut out = String::new();
    let (mut at, mut n) = (0, 0);
    while let Some(i) = low[at..].find(&low_needle) {
        let start = at + i;
        if !hay.is_char_boundary(start) || !hay.is_char_boundary(start + low_needle.len()) {
            break;
        }
        out.push_str(&hay[at..start]);
        out.push_str(with);
        at = start + low_needle.len();
        n += 1;
    }
    out.push_str(&hay[at..]);
    (out, n)
}

impl App {
    /// The level of table `sheet` a search looks at: the one last shown.
    fn search_level(&self, sheet: usize) -> usize {
        if sheet == self.view.sheet { self.shown_layer() } else { self.levels.get(&sheet).copied().unwrap_or(0).min(self.wb.sheets[sheet].layers.len()) }
    }

    /// Every cell of the document that contains `text` (upper and lower case
    /// alike) – in what it shows or in what it holds (a formula).
    fn matches(&mut self, text: &str) -> Vec<(usize, usize, usize)> {
        let needle = text.to_lowercase();
        let mut out = Vec::new();
        for s in 0..self.wb.sheets.len() {
            let z = self.search_level(s);
            let (rows, cols) = (self.wb.sheets[s].cells.len(), self.wb.sheets[s].columns.len());
            for r in 0..rows {
                for c in 0..cols {
                    let raw = self.wb.sheets[s].raw[r].get(c).is_some_and(|x| x.to_lowercase().contains(&needle));
                    if raw || self.wb.display((s, r, in_layer(c, z))).to_lowercase().contains(&needle) {
                        out.push((s, r, c));
                    }
                }
            }
        }
        out
    }

    /// `:find text`: the cursor goes to the next cell containing it – on
    /// through the table and the tables after it, then from the start.
    /// Without a text (Ctrl+G, `:next`, `:prev`): the last one again.
    pub(super) fn find(&mut self, text: Option<String>, back: bool) {
        if !self.view.is_data() {
            return self.with_window(true, |app| app.find(text, back));
        }
        if self.view.kind != ViewKind::Table {
            return self.error(":find searches the tables – go to one first");
        }
        let Some(text) = text.filter(|t| !t.is_empty()).or_else(|| self.last_find.clone()) else {
            return self.error("usage: :find text – then Ctrl+G (or :next, :prev) for the next match");
        };
        self.last_find = Some(text.clone());
        if self.view.pivot.is_some() {
            self.toggle_pivot(None);
        }
        let found = self.matches(&text);
        if found.is_empty() {
            return self.error(format!("'{text}' not found"));
        }
        let here = (self.view.sheet, self.view.cursor.0, self.view.cursor.1);
        let k = if back {
            found.iter().rposition(|m| *m < here).unwrap_or(found.len() - 1)
        } else {
            found.iter().position(|m| *m > here).unwrap_or(0)
        };
        let (s, r, c) = found[k];
        if s != self.view.sheet {
            self.switch_sheet(s);
        }
        self.move_to(r, c);
        let place = match self.wb.sheets[s].name.as_deref().filter(|_| s != here.0) {
            Some(name) => format!("{name}!{}{}", col_letters(c), r + 1),
            None => format!("{}{}", col_letters(c), r + 1),
        };
        self.info(format!("'{text}': match {} of {} at {place} · Ctrl+G next · :prev back", k + 1, found.len()));
    }

    /// `:replace old new`: in the selection, else in the whole table – in
    /// what the cells hold (values and formulas). One undo step.
    pub(super) fn replace(&mut self, args: &[String]) {
        if self.view.kind != ViewKind::Table || self.view.pivot.is_some() {
            return self.error(":replace works in tables");
        }
        let (old, new) = match args {
            [old, new] if !old.is_empty() => (old, new),
            [old] if !old.is_empty() => (old, &String::new()),
            _ => return self.error("usage: :replace old new – words with spaces in quotes: :replace \"due soon\" open · in the selection, else in the whole table"),
        };
        let (rows, cols) = self.dims();
        if rows == 0 {
            return self.error("nothing to replace in");
        }
        let ((r0, c0), (r1, c1)) = if self.has_range() || self.on_header().is_some() { self.selection() } else { ((0, 0), (rows - 1, cols - 1)) };
        let mut cells = Vec::new();
        let mut count = 0;
        // Rows the filter hides stay as they are.
        for r in self.shown_in(r0, r1.min(rows - 1)) {
            for c in c0..=c1.min(cols.saturating_sub(1)) {
                let raw = self.effective_raw(r, c);
                let (text, n) = replace_all(&raw, old, new);
                if n > 0 {
                    count += n;
                    cells.push((r, c, text));
                }
            }
        }
        if cells.is_empty() {
            return self.error(format!("'{old}' not found"));
        }
        let n = cells.len();
        self.set_cells(cells);
        self.info(format!("replaced {count} × '{old}' by '{new}' in {n} cell(s) · Ctrl+Z takes it back"));
    }
}

impl App {
    /// Deletes the data rows `doomed` (ascending) as one step: references
    /// and rules follow the rows below, those of the deleted rows go.
    pub(super) fn delete_rows(&mut self, doomed: &[usize]) {
        let (rows, cols) = self.dims();
        let mut kept = 0;
        let moved: Vec<usize> = (0..rows)
            .map(|r| {
                if doomed.contains(&r) {
                    crate::refs::GONE
                } else {
                    kept += 1;
                    kept - 1
                }
            })
            .collect();
        let same: Vec<usize> = (0..cols).collect();
        let sheet = self.view.sheet;
        let doomed = doomed.to_vec();
        self.with_doc(|doc| {
            crate::refs::update_document(doc, sheet, &crate::refs::Moves { rows: &moved, cols: &same });
            if let Some(t) = super::table_mut(doc, sheet) {
                super::data_layers::pad_layers(t);
                for grid in t.grids_mut() {
                    for &r in doomed.iter().rev() {
                        if r < grid.len() {
                            grid.remove(r);
                        }
                    }
                }
            }
        });
    }

    /// The key columns of `:duplicates` / `:dedupe`: those named, else `default`.
    fn key_columns(&self, args: &[&str], default: Vec<usize>) -> Result<Vec<usize>, String> {
        if args.is_empty() {
            return Ok(default);
        }
        if let Some(c) = self.find_column(&args.join(" ")) {
            return Ok(vec![c]);
        }
        args.iter().map(|a| self.find_column(a.trim_end_matches(',')).ok_or_else(|| format!("no column '{a}'"))).collect()
    }

    /// Per data row its key: the values of `columns` as shown (`None`: all empty).
    fn row_keys(&mut self, columns: &[usize]) -> Vec<Option<Vec<String>>> {
        let (sheet, z) = (self.view.sheet, self.shown_layer());
        (0..self.dims().0)
            .map(|r| {
                let key: Vec<String> = columns.iter().map(|&c| self.wb.value((sheet, r, in_layer(c, z))).display(None, false).trim().to_string()).collect();
                key.iter().any(|k| !k.is_empty()).then_some(key)
            })
            .collect()
    }

    /// `:duplicates [Column]`: filters the column to the values that occur
    /// more than once.
    pub(super) fn show_duplicates(&mut self, args: &[&str]) {
        if self.view.kind != ViewKind::Table || self.view.pivot.is_some() {
            return self.error(":duplicates works in tables");
        }
        let cols = self.dims().1;
        let c = match self.key_columns(args, vec![self.view.cursor.1]) {
            Ok(columns) if columns.len() == 1 && columns[0] < cols => columns[0],
            Ok(_) => return self.error("usage: :duplicates [Column] – shows the rows whose value in the column occurs more than once"),
            Err(e) => return self.error(e),
        };
        let (sheet, z) = (self.view.sheet, self.shown_layer());
        // As a filter rule names them: numbers as numbers.
        let mut seen: Vec<(String, usize)> = Vec::new();
        for r in 0..self.dims().0 {
            let rule = match self.wb.value((sheet, r, in_layer(c, z))) {
                crate::value::Value::Null => continue,
                crate::value::Value::Text(t) if t.trim().is_empty() => continue,
                crate::value::Value::Num(n, _) => format!("{n}"),
                v => v.display(None, false),
            };
            match seen.iter_mut().find(|(k, _)| *k == rule) {
                Some((_, n)) => *n += 1,
                None => seen.push((rule, 1)),
            }
        }
        let twice: Vec<String> = seen.into_iter().filter(|(_, n)| *n > 1).map(|(k, _)| crate::layers::quote_value(&k)).collect();
        let name = self.wb.sheets[sheet].columns[c].name.clone();
        if twice.is_empty() {
            return self.info(format!("no value occurs twice in {name}"));
        }
        let n = twice.len();
        self.set_filter(c, Some(format!("in({})", twice.join(", "))));
        self.info(format!("{n} value(s) of {name} occur more than once – shown by a filter · :sort {name} puts them together · :filter off shows all"));
    }

    /// `:dedupe [Column …]`: deletes the rows that repeat an earlier one –
    /// in the named columns, else in all of them. The first one stays.
    pub(super) fn dedupe(&mut self, args: &[&str]) {
        if self.view.kind != ViewKind::Table || self.view.pivot.is_some() {
            return self.error(":dedupe works in tables");
        }
        let all: Vec<usize> = (0..self.dims().1).collect();
        let columns = match self.key_columns(args, all) {
            Ok(c) => c,
            Err(e) => return self.error(e),
        };
        let keys = self.row_keys(&columns);
        let mut seen = std::collections::HashSet::new();
        let doomed: Vec<usize> = keys.into_iter().enumerate().filter_map(|(r, key)| key.filter(|k| !seen.insert(k.clone())).map(|_| r)).collect();
        if doomed.is_empty() {
            return self.info("no row repeats another");
        }
        self.delete_rows(&doomed);
        self.view.anchor = None;
        self.clamp_cursor();
        self.snap_to_shown();
        self.info(format!("deleted {} row(s) that repeated an earlier one · Ctrl+Z takes it back", doomed.len()));
    }
}

impl App {
    /// `:group Column`: a new table with one row per value of the column –
    /// how many rows have it and the sum of every number column. Its cells
    /// are formulas over the table, so the sums follow the data.
    pub(super) fn group_by(&mut self, args: &[&str]) {
        use crate::model::{Block, Column, FooterRow, Shape, Table};
        if self.view.kind != ViewKind::Table || self.view.pivot.is_some() {
            return self.error(":group works in tables");
        }
        let (rows, cols) = self.dims();
        let c = match self.key_columns(args, vec![self.view.cursor.1]) {
            Ok(columns) if columns.len() == 1 && columns[0] < cols => columns[0],
            Ok(_) => return self.error("usage: :group Column – a table with the count and sums per value of the column"),
            Err(e) => return self.error(e),
        };
        let (sheet, z) = (self.view.sheet, self.shown_layer());
        let source = self.table().clone();
        let Some(source_name) = source.name.clone() else { return self.error("name the table first (type on its origin, top left) – the group table refers to it") };
        // The values of the column in order of appearance, as a cell holds them.
        let mut keys: Vec<String> = Vec::new();
        for r in 0..rows {
            let raw = match self.wb.value((sheet, r, in_layer(c, z))) {
                crate::value::Value::Null => continue,
                crate::value::Value::Num(n, _) => format!("{n}"),
                v => v.display(None, false),
            };
            if !raw.trim().is_empty() && !keys.contains(&raw) {
                keys.push(raw);
            }
        }
        if keys.is_empty() {
            return self.error("the column is empty – nothing to group by");
        }
        // Number columns: every value in them is a number.
        let numeric: Vec<usize> = (0..cols)
            .filter(|&k| k != c)
            .filter(|&k| {
                let values: Vec<_> = (0..rows).map(|r| self.wb.value((sheet, r, in_layer(k, z)))).filter(|v| !matches!(v, crate::value::Value::Null)).collect();
                !values.is_empty() && values.iter().all(|v| matches!(v, crate::value::Value::Num(..)))
            })
            .collect();
        let mut name = super::table_name(&format!("{source_name} by {}", source.columns[c].name));
        if !super::valid_table_name(&name) {
            name = format!("{source_name} groups");
        }
        let base = name.clone();
        let mut n = 2;
        while self.wb.sheet_index(&name).is_some() {
            name = format!("{base} {n}");
            n += 1;
        }
        let table_ref = if source_name.chars().all(|ch| ch.is_alphanumeric() || ch == '_') { source_name.clone() } else { format!("\"{source_name}\"") };
        let col_ref = |k: usize| format!("{table_ref}!{}", col_letters(k));
        let mut columns = vec![source.columns[c].clone(), Column { name: "Rows".into(), ty: None, align: crate::model::Align::None }];
        columns.extend(numeric.iter().map(|&k| source.columns[k].clone()));
        // An enum type would refuse nothing here, but a type with a unit suits the sums.
        let grid: Vec<Vec<String>> = keys
            .iter()
            .enumerate()
            .map(|(r, key)| {
                let mut row = vec![key.clone(), format!("=countif({}; A{})", col_ref(c), r + 1)];
                row.extend(numeric.iter().map(|&k| format!("=sumif({}; A{}; {})", col_ref(c), r + 1, col_ref(k))));
                row
            })
            .collect();
        let mut total = vec![String::new(), "=sum".to_string()];
        total.extend(numeric.iter().map(|_| "=sum".to_string()));
        let table = Table {
            compact: true,
            line: Default::default(),
            heading_level: Some(source.heading_level.unwrap_or(2)),
            name: Some(name.clone()),
            shape: Shape::Matrix(Vec::new()),
            columns,
            rows: grid,
            footer: vec![FooterRow { label: "Total".into(), cells: total }],
            layer_name: None,
            layers: Vec::new(),
            fill: None,
        };
        self.snapshot();
        self.doc.blocks.push(Block::Table(table));
        self.dirty = true;
        self.rebuild();
        self.switch_sheet(self.wb.sheets.len() - 1);
        self.info(format!("table {name}: {} group(s) with count and sums – formulas over {source_name}, they follow its data; a new value needs a row of its own", keys.len()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_and_quotes() {
        assert_eq!(split_args("a  b"), ["a", "b"]);
        assert_eq!(split_args("\"due soon\" open"), ["due soon", "open"]);
        assert_eq!(split_args("x \"\""), ["x", ""]);
        assert_eq!(split_args("\"say \"\"hi\"\"\""), ["say \"hi\""]);
        assert!(split_args("  ").is_empty());
    }

    #[test]
    fn replacing() {
        assert_eq!(replace_all("Rent rent RENT", "rent", "x"), ("x x x".to_string(), 3));
        assert_eq!(replace_all("=B1*Rate", "rate", "Tax"), ("=B1*Tax".to_string(), 1));
        assert_eq!(replace_all("Straße", "ß", "ss"), ("Strasse".to_string(), 1));
        assert_eq!(replace_all("abc", "x", "y"), ("abc".to_string(), 0));
        assert_eq!(replace_all("aaa", "a", ""), (String::new(), 3));
    }
}
