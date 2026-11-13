//! Filling cells like Excel: Ctrl+D / Ctrl+R copy the first row / column of
//! the selection into the rest, the fill handle (▪ at the selection's
//! corner) and `:autofill` continue series. Formulas move their relative
//! references along.

use super::{App, ViewKind};
use crate::fill::{series, Fill};

type Rect = ((usize, usize), (usize, usize));

impl App {
    fn fill_allowed(&mut self) -> bool {
        if self.view.kind != ViewKind::Table || self.view.pivot.is_some() || self.on_header().is_some() {
            self.error("filling works on cells of a table");
            return false;
        }
        true
    }

    fn raw_at(&self, r: usize, c: usize) -> String {
        self.effective_raw(r, c)
    }

    /// Fills from `src` towards `to` (down, up, right or left – whichever
    /// reaches further out). `smart`: continue series, else repeat.
    pub(super) fn autofill(&mut self, src: Rect, to: (usize, usize), smart: bool) {
        let ((r0, c0), (r1, c1)) = src;
        // Rows the filter hides neither give nor take: a series runs on
        // over the rows that are shown.
        let from_rows = self.shown_in(r0, r1);
        if from_rows.is_empty() {
            return;
        }
        let down = to.0.saturating_sub(r1);
        let up = r0.saturating_sub(to.0);
        let right = to.1.saturating_sub(c1);
        let left = c0.saturating_sub(to.1);
        let names: Vec<String> = self.wb.sheets[self.view.sheet].columns.iter().map(|c| c.name.clone()).collect();
        let fill_line = |source: Vec<String>, count: usize| if smart { series(&source, count) } else { (0..count).map(|k| Fill::From(k % source.len())).collect() };
        let mut cells = Vec::new();
        // (target, source) positions along one line, for vertical / horizontal fills.
        let mut put = |fills: Vec<Fill>, source: &[String], targets: Vec<(usize, usize)>, sources: &dyn Fn(usize) -> (usize, usize)| {
            for (fill, (tr, tc)) in fills.into_iter().zip(targets) {
                let text = match fill {
                    Fill::Text(t) => t,
                    Fill::From(i) => {
                        let (sr, sc) = sources(i);
                        let raw = &source[i];
                        if raw.trim_start().starts_with('=') {
                            crate::refs::shift_formula(raw, tr as isize - sr as isize, tc as isize - sc as isize, &names)
                        } else {
                            raw.clone()
                        }
                    }
                };
                cells.push((tr, tc, text));
            }
        };
        let filled: Rect = if down.max(up) >= right.max(left) && down.max(up) > 0 {
            let targets: Vec<usize> = if down >= up { self.shown_in(r1 + 1, r1 + down) } else { self.shown_in(r0 - up, r0 - 1).into_iter().rev().collect() };
            let from: Vec<usize> = if down >= up { from_rows.clone() } else { from_rows.iter().rev().copied().collect() };
            for c in c0..=c1 {
                let source: Vec<String> = from.iter().map(|&r| self.raw_at(r, c)).collect();
                let fills = fill_line(source.clone(), targets.len());
                put(fills, &source, targets.iter().map(|&r| (r, c)).collect(), &|i| (from[i], c));
            }
            if down >= up { ((r0, c0), (r1 + down, c1)) } else { ((r0 - up, c0), (r1, c1)) }
        } else if right.max(left) > 0 {
            for &r in &from_rows {
                if right >= left {
                    let source: Vec<String> = (c0..=c1).map(|c| self.raw_at(r, c)).collect();
                    let fills = fill_line(source.clone(), right);
                    put(fills, &source, (1..=right).map(|k| (r, c1 + k)).collect(), &|i| (r, c0 + i));
                } else {
                    let source: Vec<String> = (c0..=c1).rev().map(|c| self.raw_at(r, c)).collect();
                    let fills = fill_line(source.clone(), left);
                    put(fills, &source, (1..=left).map(|k| (r, c0 - k)).collect(), &|i| (r, c1 - i));
                }
            }
            if right >= left { ((r0, c0), (r1, c1 + right)) } else { ((r0, c0 - left), (r1, c1)) }
        } else {
            return;
        };
        let n = cells.len();
        self.set_cells(cells);
        self.view.anchor = Some(filled.0);
        self.view.cursor = filled.1;
        self.view.follow = true;
        self.info(format!("filled {n} cell(s){}", if smart { "" } else { " (copied)" }));
    }

    /// Ctrl+D: the first row of the selection into the others (a single row:
    /// the row above into it).
    pub(super) fn fill_down(&mut self) {
        if !self.fill_allowed() {
            return;
        }
        let ((r0, c0), (r1, c1)) = self.selection();
        match (r0 == r1, r0) {
            (true, 0) => self.error("nothing above to fill down from"),
            (true, _) => self.autofill(((r0 - 1, c0), (r0 - 1, c1)), (r1, c1), false),
            (false, _) => self.autofill(((r0, c0), (r0, c1)), (r1, c1), false),
        }
    }

    /// Ctrl+R: the first column of the selection into the others.
    pub(super) fn fill_right(&mut self) {
        if !self.fill_allowed() {
            return;
        }
        let ((r0, c0), (r1, c1)) = self.selection();
        match (c0 == c1, c0) {
            (true, 0) => self.error("nothing to the left to fill right from"),
            (true, _) => self.autofill(((r0, c0 - 1), (r1, c0 - 1)), (r1, c1), false),
            (false, _) => self.autofill(((r0, c0), (r1, c0)), (r1, c1), false),
        }
    }

    /// `:autofill B10`: continues the selection up to that cell.
    pub(super) fn autofill_to(&mut self, cell: Option<(usize, usize)>) {
        if !self.fill_allowed() {
            return;
        }
        match cell {
            Some((c, r)) => {
                let sel = self.selection();
                self.autofill(sel, (r, c), true)
            }
            None => self.error("usage: :autofill B10 – continues the selection up to that cell"),
        }
    }

    /// The cells the fill handle would fill while dragging (for drawing).
    pub fn fill_preview(&self) -> Option<Rect> {
        let (((r0, c0), (r1, c1)), to) = (self.fill_drag?, self.fill_to?);
        let (down, up) = (to.0.saturating_sub(r1), r0.saturating_sub(to.0));
        let (right, left) = (to.1.saturating_sub(c1), c0.saturating_sub(to.1));
        if down.max(up) >= right.max(left) && down.max(up) > 0 {
            Some(if down >= up { ((r1 + 1, c0), (r1 + down, c1)) } else { ((r0 - up, c0), (r0 - 1, c1)) })
        } else if right.max(left) > 0 {
            Some(if right >= left { ((r0, c1 + 1), (r1, c1 + right)) } else { ((r0, c0 - left), (r1, c0 - 1)) })
        } else {
            None
        }
    }
}
