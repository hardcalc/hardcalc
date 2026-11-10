//! Slices and scenarios in the UI – the z axis of a table: each window
//! shows one level of its table (a: the table itself). The table origin is
//! the head of the stack – ↑/↓ there, Alt+↑/↓ anywhere, a click in the
//! line above the layer bar or `:z` change it. In b, c, … typing writes the
//! slice's or scenario's own cells; cleared cells take the table's again.

use super::{table_mut, App, ViewKind};
use crate::engine::{layer_letters, letters_to_layer};
use crate::model::{DataLayer, LayerKind, Table};

/// Brings every data layer to the table's size, so that lines can be
/// inserted, deleted and moved in all grids alike.
pub(super) fn pad_layers(t: &mut Table) {
    let (rows, cols) = (t.rows.len(), t.columns.len());
    for l in &mut t.layers {
        l.rows.resize(rows, Vec::new());
        for row in &mut l.rows {
            row.resize(cols, String::new());
        }
    }
}

impl App {
    /// Data layers b, c, … of the current table (0: only the table itself).
    pub fn layer_count(&self) -> usize {
        self.wb.sheets.get(self.view.sheet).map_or(0, |s| s.layers.len())
    }

    /// The data layer shown: 0 for the table itself, and in views without layers.
    pub fn shown_layer(&self) -> usize {
        if self.view.kind == ViewKind::Table && self.view.pivot.is_none() { self.view.layer.min(self.layer_count()) } else { 0 }
    }

    /// `b February` – the letter and the name of level `z` of the shown
    /// table; an unnamed a is `a table`.
    pub fn layer_title(&self, z: usize) -> String {
        self.level_title(self.view.sheet, z)
    }

    /// The same for table `sheet`.
    pub fn level_title(&self, sheet: usize, z: usize) -> String {
        let s = &self.wb.sheets[sheet];
        let name = s.layer_label(z);
        let letter = layer_letters(z);
        match (name == letter, z) {
            (true, 0) => "a table".into(),
            (true, _) => letter,
            (false, _) => format!("{letter} {name}"),
        }
    }

    pub fn set_layer(&mut self, z: usize) {
        if z > self.layer_count() {
            return;
        }
        self.view.layer = z;
        self.levels.insert(self.view.sheet, z);
        let kind = match z {
            0 => "the table itself".to_string(),
            _ => match self.table().layers[z - 1].kind {
                LayerKind::Slice => "slice – empty cells stay empty".to_string(),
                LayerKind::Scenario => "scenario – empty cells show a".to_string(),
            },
        };
        let back = if z > 0 { "Esc back to a · " } else { "" };
        self.info(format!("{} ({kind}) · {back}:z b, Alt+↑/↓ or a click below change it", self.layer_title(z)));
    }

    /// One layer up (-1) or down (+1) the stack.
    pub(super) fn step_layer(&mut self, d: isize) {
        if self.layer_count() == 0 {
            return self.info("this table has no slices or scenarios – :slice Name or :scenario Name adds one");
        }
        let z = (self.shown_layer() as isize + d).clamp(0, self.layer_count() as isize) as usize;
        self.set_layer(z);
    }

    /// The layer's own raw cell (empty: it takes the table's).
    pub(super) fn own_raw(&self, z: usize, r: usize, c: usize) -> String {
        self.table().layers.get(z.wrapping_sub(1)).and_then(|l| l.rows.get(r)).and_then(|row| row.get(c)).cloned().unwrap_or_default()
    }

    /// What a cell of the shown layer holds: its own content, else the table's.
    pub(super) fn effective_raw(&self, r: usize, c: usize) -> String {
        let base = || self.wb.sheets[self.view.sheet].raw.get(r).and_then(|row| row.get(c)).cloned().unwrap_or_default();
        match self.shown_layer() {
            0 => base(),
            z => {
                let own = self.own_raw(z, r, c);
                if own.trim().is_empty() { base() } else { own }
            }
        }
    }

    /// Whether (`r`, `c`) of the shown layer b, c, … has no content of its own.
    pub(super) fn inherited(&self, r: usize, c: usize) -> bool {
        self.shown_layer() > 0 && self.own_raw(self.shown_layer(), r, c).trim().is_empty()
    }

    /// Writes cells of the shown data layer (≥ 1) as one undoable step; beyond
    /// the data the table grows.
    pub(super) fn set_layer_cells(&mut self, z: usize, cells: Vec<(usize, usize, String)>) {
        let (rows, cols) = self.dims();
        let cells: Vec<_> = cells.into_iter().filter(|(r, c, t)| (*r < rows && *c < cols) || !t.trim().is_empty()).collect();
        if cells.is_empty() {
            return;
        }
        let need_rows = cells.iter().map(|(r, _, _)| r + 1).max().unwrap_or(0).max(rows);
        let need_cols = cells.iter().map(|(_, c, _)| c + 1).max().unwrap_or(0).max(cols);
        let calculates = self.calculates();
        self.with_table(|t| {
            super::grow(t, need_rows, need_cols);
            pad_layers(t);
            if calculates && need_rows > rows {
                use super::calc_column::{column_formula, extend_layer_formulas, formula_in_row};
                let names: Vec<String> = t.columns.iter().map(|c| c.name.clone()).collect();
                // The table's calculated columns reach the new rows too.
                for c in 0..names.len() {
                    if let Some((base, src)) = column_formula(&t.rows, c, &names, None) {
                        for r in rows..need_rows {
                            t.rows[r][c] = formula_in_row(&src, base, r, &names);
                        }
                    }
                }
                extend_layer_formulas(t, rows..need_rows, &names);
            }
            for (r, c, text) in cells {
                t.layers[z - 1].rows[r][c] = text;
            }
        });
    }

    /// `:slice Name`, `:scenario Name` – a new level on the current table.
    pub(super) fn new_data_layer(&mut self, name: &str, kind: LayerKind) {
        if self.view.kind != ViewKind::Table || self.view.pivot.is_some() {
            return self.error("slices and scenarios belong to tables");
        }
        if let Err(e) = self.check_layer_name(name, None) {
            return self.error(e);
        }
        let layer = DataLayer { name: name.to_string(), kind, rows: Vec::new() };
        self.with_table(|t| t.layers.push(layer));
        let z = self.layer_count();
        self.set_layer(z);
    }

    fn check_layer_name(&self, name: &str, current: Option<usize>) -> Result<(), String> {
        if !crate::parse::valid_layer_name(name) {
            return Err(format!("'{name}' is not a valid name (no , : . \" [ ], not like B2 or the letters of a level such as b)"));
        }
        let s = &self.wb.sheets[self.view.sheet];
        if s.layer_index(name).is_some_and(|z| Some(z) != current) {
            return Err(format!("{name} exists already"));
        }
        if self.table().name.is_none() {
            return Err("name the table first (type on the table origin) – the name of a lives in its heading".into());
        }
        Ok(())
    }

    /// `:z rename Name` – the shown slice or scenario (also a, the table itself).
    pub(super) fn rename_data_layer(&mut self, name: &str) {
        let z = self.shown_layer();
        if let Err(e) = self.check_layer_name(name, Some(z)) {
            return self.error(e);
        }
        let old = self.wb.sheets[self.view.sheet].layer_label(z);
        let had_name = old != layer_letters(z);
        let sheet = self.view.sheet;
        let new = name.to_string();
        self.with_doc(|doc| {
            if had_name {
                crate::refs::rename_layer(doc, sheet, &old, &new);
            }
            if let Some(t) = table_mut(doc, sheet) {
                match z {
                    0 => t.layer_name = Some(new),
                    _ => t.layers[z - 1].name = new,
                }
            }
        });
        self.info(format!("{} named {name}; references updated", layer_letters(z)));
    }

    /// `:z delete` – the shown slice or scenario; letters of the later
    /// ones move up in formulas.
    pub(super) fn delete_data_layer(&mut self) {
        let z = self.shown_layer();
        if z == 0 {
            return self.error("a is the table itself – it cannot be deleted");
        }
        let title = self.layer_title(z);
        let sheet = self.view.sheet;
        self.with_doc(|doc| {
            crate::refs::remove_layer(doc, sheet, z);
            if let Some(t) = table_mut(doc, sheet) {
                t.layers.remove(z - 1);
            }
        });
        self.view.layer = z - 1;
        self.levels.insert(sheet, z - 1);
        self.info(format!("{title} deleted (Ctrl+Z restores it)"));
    }

    /// `-` / `+` on the table origin: the shown slice or scenario one place
    /// up / down the stack (a, the table itself, stays first); references by
    /// letter follow.
    pub(super) fn move_level(&mut self, d: isize) {
        let z = self.shown_layer();
        let to = z as isize + d;
        if z == 0 {
            return self.error("a is the table itself and stays first – show a slice or scenario to move it");
        }
        if to < 1 || to as usize > self.layer_count() {
            return;
        }
        let to = to as usize;
        let sheet = self.view.sheet;
        self.with_doc(|doc| {
            crate::refs::swap_layers(doc, sheet, z, to);
            if let Some(t) = table_mut(doc, sheet) {
                t.layers.swap(z - 1, to - 1);
            }
        });
        self.set_layer(to);
        self.info(format!("moved to {} – references by letter follow", self.layer_title(to)));
    }

    /// `:z b`, `:z February`, `:z` (list).
    pub(super) fn goto_layer(&mut self, arg: Option<&str>) {
        let Some(arg) = arg else {
            let list: Vec<String> = (0..=self.layer_count()).map(|z| self.layer_title(z)).collect();
            return self.info(format!("z: {} – :z rename Name, :z delete", list.join(" · ")));
        };
        // A name first: `plan` is a name, although it could be read as letters.
        let z = self.wb.sheets[self.view.sheet].layer_index(arg).or_else(|| letters_to_layer(arg));
        match z {
            Some(z) if z <= self.layer_count() => self.set_layer(z),
            _ => self.error(format!("no '{arg}' on the z axis – :z lists them")),
        }
    }
}
