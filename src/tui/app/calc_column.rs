//! Calculated columns, as in Excel tables: a column whose formulas are all
//! the same – up to relative references moving along with the row – keeps
//! them so. A new formula typed into one of its cells (or into an empty
//! column) goes into every row; values typed there stay exceptions. New
//! rows get the column's formula. The same holds in every slice and
//! scenario, for what it shows: its own cells, else the table's formulas
//! (and in a scenario the table's values).

use super::App;
use crate::model::{LayerKind, Shape, Table};
use crate::refs::shift_formula;

/// The formula of column `c` as (row, source) if it is calculated: at least
/// one formula, and all formulas (not counting row `skip`) equal when moved
/// to the same row. Values and empty cells are allowed between them.
pub(super) fn column_formula(rows: &[Vec<String>], c: usize, names: &[String], skip: Option<usize>) -> Option<(usize, String)> {
    let mut formulas = rows
        .iter()
        .enumerate()
        .filter(|&(r, _)| Some(r) != skip)
        .filter_map(|(r, row)| row.get(c).filter(|s| s.trim_start().starts_with('=')).map(|s| (r, s.trim())));
    let (base, first) = formulas.next()?;
    formulas
        .all(|(r, src)| shift_formula(first, (r - base) as isize, 0, names) == src)
        .then(|| (base, first.to_string()))
}

/// What slice or scenario `z` (≥ 1) shows of the table: its own cells, else
/// the table's formulas, in a scenario also the table's values.
pub(super) fn shown_grid(t: &Table, z: usize) -> Vec<Vec<String>> {
    let l = &t.layers[z - 1];
    t.rows
        .iter()
        .enumerate()
        .map(|(r, row)| {
            row.iter()
                .enumerate()
                .map(|(c, base)| match l.rows.get(r).and_then(|o| o.get(c)).filter(|o| !o.trim().is_empty()) {
                    Some(own) => own.clone(),
                    None if base.trim_start().starts_with('=') || l.kind == LayerKind::Scenario => base.clone(),
                    None => String::new(),
                })
                .collect()
        })
        .collect()
}

/// New rows `rows` of `t` (already inserted): slices and scenarios whose own
/// cells hold a calculated column get its formula there too.
pub(super) fn extend_layer_formulas(t: &mut Table, rows: std::ops::Range<usize>, names: &[String]) {
    super::data_layers::pad_layers(t);
    for l in &mut t.layers {
        for c in 0..names.len() {
            let Some((base, src)) = column_formula(&l.rows, c, names, None) else { continue };
            for r in rows.clone() {
                if l.rows[r][c].trim().is_empty() {
                    l.rows[r][c] = formula_in_row(&src, base, r, names);
                }
            }
        }
    }
}

/// The formula `src` of row `from` as it reads in row `to`.
pub(super) fn formula_in_row(src: &str, from: usize, to: usize, names: &[String]) -> String {
    shift_formula(src, to as isize - from as isize, 0, names)
}

impl App {
    /// Calculated columns belong to tables, not to the free playground sheet.
    pub(super) fn calculates(&self) -> bool {
        self.wb.sheets[self.view.sheet].shape != Shape::Playground && !self.fills()
    }

    fn column_names(&self) -> Vec<String> {
        self.wb.sheets[self.view.sheet].columns.iter().map(|c| c.name.clone()).collect()
    }

    /// The cells to write for `text` typed into (`r`, `c`): a formula in a
    /// calculated or empty column goes into every row that holds no value.
    pub(super) fn typed_cells(&mut self, r: usize, c: usize, text: String) -> Vec<(usize, usize, String)> {
        let (rows, cols) = self.dims();
        if !text.trim_start().starts_with('=') || c >= cols || !self.calculates() {
            return vec![(r, c, text)];
        }
        let z = self.shown_layer();
        let shown;
        let raw = match z {
            0 => &self.wb.sheets[self.view.sheet].raw,
            _ => {
                shown = shown_grid(self.table(), z);
                &shown
            }
        };
        let names = self.column_names();
        let blank = |row: usize| raw[row][c].trim().is_empty();
        let others: Vec<usize> = (0..rows).filter(|&row| row != r).collect();
        let calculated = column_formula(raw, c, &names, Some(r)).is_some();
        if others.is_empty() || !(calculated || others.iter().all(|&row| blank(row))) {
            return vec![(r, c, text)];
        }
        let targets: Vec<usize> =
            others.into_iter().filter(|&row| blank(row) || raw[row][c].trim_start().starts_with('=')).collect();
        let mut cells: Vec<(usize, usize, String)> =
            targets.iter().map(|&row| (row, c, formula_in_row(&text, r, row, &names))).collect();
        cells.push((r, c, text));
        let name = &self.wb.sheets[self.view.sheet].columns[c].name;
        self.info(format!("formula set for column {name} ({} rows)", targets.len() + 1));
        cells
    }

    /// Formulas of the calculated columns for the new rows `from..to`
    /// (appended), leaving out the cells in `set`.
    pub(super) fn inherited_cells(&self, from: usize, to: usize, set: &[(usize, usize, String)]) -> Vec<(usize, usize, String)> {
        if !self.calculates() || self.shown_layer() > 0 {
            return Vec::new();
        }
        let raw = &self.wb.sheets[self.view.sheet].raw;
        let names = self.column_names();
        let mut out = Vec::new();
        for c in 0..self.dims().1 {
            let Some((base, src)) = column_formula(raw, c, &names, None) else { continue };
            for r in from..to {
                if !set.iter().any(|&(sr, sc, _)| (sr, sc) == (r, c)) {
                    out.push((r, c, formula_in_row(&src, base, r, &names)));
                }
            }
        }
        out
    }
}
