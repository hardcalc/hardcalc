//! `:shape matrix|playground`: turns a playground into a normal table and
//! back. A playground becomes a matrix only if its content is one connected
//! area (as Excel's current region); the empty edges are dropped and
//! references follow.

use std::collections::HashSet;

use super::{table_mut, App, ViewKind, COMMENTS};
use crate::engine::{col_letters, parse_a1};
use crate::model::{Shape, Table};

/// A cell of the content: row 0 is the column headers, row r + 1 data row r.
type Cell = (usize, usize);

impl App {
    pub(super) fn set_shape(&mut self, arg: Option<&str>) {
        if self.view.kind != ViewKind::Table || self.view.pivot.is_some() {
            return self.error(":shape works in tables");
        }
        let current = self.table().shape.clone();
        match (arg, &current) {
            (None, _) => self.info(format!("shape: {current} – :shape matrix / :shape playground changes it")),
            (Some("playground"), Shape::Playground) | (Some("matrix"), Shape::Matrix(_)) => self.info(format!("the table is a {current} already")),
            (Some("playground"), Shape::Matrix(_)) => {
                self.with_table(|t| reshape(t, Shape::Playground));
                self.info("the table is a playground now");
            }
            (Some("matrix"), Shape::Playground) => self.playground_to_matrix(),
            (Some("playground" | "matrix"), _) => self.error(format!(":shape changes between matrix and playground, not a {current}")),
            (Some(other), _) => self.error(format!("unknown shape '{other}' – matrix or playground")),
        }
    }

    fn playground_to_matrix(&mut self) {
        let sheet = self.view.sheet;
        let areas = areas(self.content());
        if areas.len() > 1 {
            let list: Vec<String> = areas.iter().map(|a| a1_range(bounds(a))).collect();
            return self.error(format!("{} separate areas ({}) – a matrix needs one connected area", areas.len(), list.join(", ")));
        }
        let Some(area) = areas.first() else {
            self.with_table(|t| reshape(t, Shape::Matrix(Vec::new())));
            return self.info("the table is a matrix now");
        };
        let ((top, left), (bottom, right)) = bounds(area);
        // Footer cells keep their columns.
        let t = self.table();
        let used = |c: usize| t.footer.iter().any(|f| f.cells.get(c).is_some_and(|s| !s.trim().is_empty()));
        let left = (0..left).find(|&c| used(c)).unwrap_or(left);
        let right = (right + 1..t.columns.len()).rev().find(|&c| used(c)).unwrap_or(right);
        let (rows, cols) = (t.rows.len(), t.columns.len());
        // Data rows from the first one with content (the header row is no data row).
        let first = top.saturating_sub(1);
        if crate::refs::refers_to_edge(&self.doc, sheet, first, left) {
            return self.error("a formula or layer rule refers to the empty rows or columns at the top / left – change it first");
        }
        let row_map: Vec<usize> = (0..rows).map(|r| r.saturating_sub(first)).collect();
        let col_map: Vec<usize> = (0..cols).map(|c| c.saturating_sub(left)).collect();
        self.with_doc(|doc| {
            crate::refs::update_document(doc, sheet, &crate::refs::Moves { rows: &row_map, cols: &col_map });
            let Some(t) = table_mut(doc, sheet) else { return };
            let keep = left..(right + 1).min(t.columns.len());
            let auto: Vec<bool> = t.columns.iter().enumerate().map(|(i, c)| c.name == col_letters(i)).collect();
            t.columns = t.columns.drain(keep.clone()).collect();
            // Columns named by their letter keep matching their position.
            for (i, col) in t.columns.iter_mut().enumerate() {
                if auto[left + i] {
                    col.name = col_letters(i);
                }
            }
            for grid in t.grids_mut() {
                grid.truncate(bottom);
                grid.drain(..first.min(grid.len()));
                for row in grid.iter_mut() {
                    row.truncate(keep.end);
                    row.drain(..keep.start.min(row.len()));
                }
            }
            for f in &mut t.footer {
                f.cells.truncate(keep.end);
                f.cells.drain(..keep.start.min(f.cells.len()));
            }
            reshape(t, Shape::Matrix(Vec::new()));
        });
        let (r, c) = self.view.cursor;
        self.view.cursor = (r.saturating_sub(first), c.saturating_sub(left));
        self.clamp_cursor();
        self.info(format!("the table is a matrix now ({} rows, {} columns)", bottom - first, right + 1 - left));
    }

    /// The cells with content: values in the table and its slices /
    /// scenarios, commented cells and named column headers.
    fn content(&self) -> HashSet<Cell> {
        let t = self.table();
        let filled = |s: &String| !s.trim().is_empty();
        let mut cells: HashSet<Cell> = t
            .columns
            .iter()
            .enumerate()
            .filter(|(i, c)| c.name != col_letters(*i))
            .map(|(i, _)| (0, i))
            .collect();
        for grid in std::iter::once(&t.rows).chain(t.layers.iter().map(|l| &l.rows)) {
            for (r, row) in grid.iter().enumerate() {
                cells.extend(row.iter().enumerate().filter(|(_, s)| filled(s)).map(|(c, _)| (r + 1, c)));
            }
        }
        for a in self.attached.iter().filter(|a| a.sheet == self.view.sheet && a.layer.name == COMMENTS) {
            for rule in &a.layer.rules {
                if let crate::model::Rule::Rule { target, .. } = rule
                    && let Some((c, r)) = parse_a1(target.trim())
                {
                    cells.insert((r + 1, c));
                }
            }
        }
        cells
    }
}

/// The new shape; `fill` / `nofill` stays only where it differs from it.
fn reshape(t: &mut Table, shape: Shape) {
    t.shape = shape;
    let default = t.shape == Shape::Playground && t.footer.is_empty();
    if t.fill == Some(default) {
        t.fill = None;
    }
}

/// Connected areas (neighbours also diagonally), top left first.
fn areas(mut cells: HashSet<Cell>) -> Vec<Vec<Cell>> {
    let mut out = Vec::new();
    let mut starts: Vec<Cell> = cells.iter().copied().collect();
    starts.sort_unstable();
    for start in starts {
        if !cells.remove(&start) {
            continue;
        }
        let (mut area, mut todo) = (vec![start], vec![start]);
        while let Some((r, c)) = todo.pop() {
            for nr in r.saturating_sub(1)..=r + 1 {
                for nc in c.saturating_sub(1)..=c + 1 {
                    if cells.remove(&(nr, nc)) {
                        area.push((nr, nc));
                        todo.push((nr, nc));
                    }
                }
            }
        }
        out.push(area);
    }
    out
}

fn bounds(area: &[Cell]) -> (Cell, Cell) {
    let (r0, r1) = (area.iter().map(|p| p.0).min().unwrap_or(0), area.iter().map(|p| p.0).max().unwrap_or(0));
    let (c0, c1) = (area.iter().map(|p| p.1).min().unwrap_or(0), area.iter().map(|p| p.1).max().unwrap_or(0));
    ((r0, c0), (r1, c1))
}

/// `A1:C5` (the header row counts as row 1 of the data).
fn a1_range(((r0, c0), (r1, c1)): (Cell, Cell)) -> String {
    let cell = |r: usize, c: usize| format!("{}{}", col_letters(c), r.max(1));
    if (r0, c0) == (r1, c1) { cell(r0, c0) } else { format!("{}:{}", cell(r0, c0), cell(r1, c1)) }
}
