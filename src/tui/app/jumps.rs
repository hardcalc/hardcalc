//! Jumps: the `links` layer, F4 / `:goto`, and the way back. A jump is a
//! step in the undo history like a change, so Ctrl+Z goes back and Ctrl+Y
//! forward again; `:back` and the ← in the status line go back over the
//! changes made since.

use super::{parse_a1, App, View, ViewKind, LINKS, UNDO_LIMIT};
use crate::engine::CellId;
use crate::links;
use crate::model::Document;

/// One step of the undo history.
#[derive(Debug, Clone)]
pub enum Step {
    /// The document before a change.
    Doc(Document),
    /// The window before a jump (`left`: the main window).
    Jump { left: bool, view: View },
}

impl App {
    /// Puts a step on the undo history; a new step ends what Ctrl+Y could redo.
    pub(super) fn record(&mut self, step: Step) {
        self.undo.push(step);
        if self.undo.len() > UNDO_LIMIT {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    /// Finds the cells with a link again (after every change, like `validate`).
    pub(super) fn relink(&mut self) {
        let shown: Vec<bool> = (0..self.wb.sheets.len()).map(|s| self.layer_visible_in(LINKS, s)).collect();
        self.links = links::link_cells(&self.attached, &mut self.wb, &|s| shown[s]);
    }

    /// Whether the main window's cursor is on a cell with a link.
    pub fn cursor_on_link(&self) -> bool {
        let main = self.main_view();
        main.kind == ViewKind::Table
            && main.pivot.is_none()
            && main.header.is_none()
            && self.solo.is_none()
            && self.links.contains_key(&(main.sheet, main.cursor.0, main.cursor.1))
    }

    /// F4: the go-to prompt; on a cell with a link it holds the destination,
    /// so Enter follows it.
    pub(super) fn goto_key(&mut self) {
        let dest = if self.cursor_on_link() {
            self.with_window(true, |app| {
                let id = (app.view.sheet, app.view.cursor.0, app.view.cursor.1);
                let text = app.links.get(&id).cloned().unwrap_or_default();
                links::concrete(&mut app.wb, id, &text)
            })
        } else {
            String::new()
        };
        self.ask_argument(&format!("goto {dest}"));
    }

    /// `:goto B5`, `:goto Expenses[Rent, Amount]` – in the main window while
    /// a helper is focused.
    pub(super) fn goto(&mut self, arg: &str) {
        if !self.view.is_data() {
            return self.with_window(true, |app| app.goto(arg));
        }
        let arg = arg.trim();
        if arg.is_empty() {
            return self.error("usage: :goto B5 · :goto Expenses · :goto Expenses!B5 · :goto Expenses[Rent, Amount]");
        }
        // A table name wins over an address it looks like (`Sheet2`).
        let plain = parse_a1(arg).filter(|_| self.wb.sheet_index(arg).is_none());
        if self.view.kind != ViewKind::Table || self.view.pivot.is_some() {
            return match plain {
                Some((r, c)) if self.view.pivot.is_none() => self.move_to(r, c),
                _ => self.error("usage: :goto B5"),
            };
        }
        // A cell of the own table: anywhere in the window, also beyond the data.
        if let Some((r, c)) = plain {
            return self.jump((self.view.sheet, r, c));
        }
        let here = (self.view.sheet, self.view.cursor.0, self.view.cursor.1);
        match links::resolve(&mut self.wb, here, arg) {
            Ok(dest) => self.jump(dest),
            Err(e) => self.error(e),
        }
    }

    /// A click on the ↗ of cell (r, c) in the focused window: the cursor
    /// goes there (so the way back leads to the link) and then follows it.
    pub(super) fn follow_link(&mut self, r: usize, c: usize) {
        let id = (self.view.sheet, r, c);
        let Some(text) = self.links.get(&id).cloned() else { return };
        self.view.anchor = None;
        self.view.header = None;
        self.view.cursor = (r, c);
        self.view.follow = true;
        match links::resolve(&mut self.wb, id, &text) {
            Ok(dest) => self.jump(dest),
            Err(e) => self.error(format!("link {text}: {e}")),
        }
    }

    /// Moves the focused window to cell `dest` and remembers where it was.
    pub(super) fn jump(&mut self, (sheet, r, c): CellId) {
        let from = self.view.clone();
        if self.view.kind != ViewKind::Table || self.view.sheet != sheet || self.view.pivot.is_some() {
            self.set_view(ViewKind::Table, sheet);
        }
        self.move_to(r, c);
        let moved = (self.view.kind, self.view.sheet, self.view.cursor) != (from.kind, from.sheet, from.cursor);
        if moved || from.anchor.is_some() || from.header.is_some() {
            self.message = None;
            self.record(Step::Jump { left: self.left_focused, view: from });
        }
    }

    /// Shows `view` again in its window (focusing it) and returns what that
    /// window showed.
    pub(super) fn restore_view(&mut self, left: bool, view: View) -> View {
        if left != self.left_focused && self.other.is_some() {
            self.swap_focus();
        }
        let old = std::mem::replace(&mut self.view, view);
        self.view.follow = true;
        self.clamp_view();
        old
    }

    /// `:back` and the ← in the status line: back from the last jump; the
    /// changes made since stay.
    pub(super) fn back(&mut self) {
        let Some(i) = self.undo.iter().rposition(|s| matches!(s, Step::Jump { .. })) else {
            return self.error("no jump to go back from");
        };
        let Step::Jump { left, view } = self.undo.remove(i) else { unreachable!() };
        let place = self.place(&view);
        self.restore_view(left, view);
        self.info(format!("back to {place}"));
    }

    /// Where the last jump came from, for the ← in the status line.
    pub fn back_label(&self) -> Option<String> {
        self.undo.iter().rev().find_map(|s| match s {
            Step::Jump { view, .. } => Some(self.place(view)),
            Step::Doc(_) => None,
        })
    }

    /// `Overview!A2`, or the name of a view without cells.
    pub(super) fn place(&self, view: &View) -> String {
        match view.kind {
            ViewKind::Table if view.sheet < self.wb.sheets.len() && view.pivot.is_none() => {
                links::cell_ref(&self.wb, usize::MAX, (view.sheet, view.cursor.0, view.cursor.1))
            }
            ViewKind::Table => "the pivot".into(),
            ViewKind::Types => "the types".into(),
            ViewKind::Functions => "the functions".into(),
            ViewKind::Calc => "the calculation".into(),
            _ => "the previous view".into(),
        }
    }
}
