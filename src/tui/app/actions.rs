//! Program functions reached through the KEYMAP, and what the terminal can
//! do: observed capabilities and the key test.

use super::{App, Header, Hit, Purpose};
use crate::tui::keymap::{Action, Cap, Cond};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// The inputs the key test asks for, in order.
const KEY_TEST: [(Cond, &str); 3] = [
    (Cond::MenuKey, "press the Menu key (≣, next to the right Ctrl)"),
    (Cond::ShiftF10, "press Shift+F10"),
    (Cond::Mouse, "right-click anywhere"),
];

impl App {
    /// Runs a program function, whatever input asked for it.
    pub fn handle(&mut self, action: Action) {
        // Cell functions act on the main window while a helper is focused.
        let global = matches!(
            action,
            Action::ContextMenu | Action::Menu | Action::Help | Action::Save | Action::Quit | Action::CommandLine | Action::OpenFile | Action::Goto | Action::Split | Action::SwapWindows
        );
        if !global && !self.view.is_data() {
            return self.with_window(true, |app| app.handle(action));
        }
        match action {
            Action::ContextMenu => self.open_context_menu(None),
            Action::Menu => self.open_menu(0),
            Action::Help => self.open_help(),
            Action::Save => self.save(None),
            Action::Quit => self.quit_key(),
            Action::CommandLine => self.ask_argument(""),
            Action::OpenFile => self.open_file_dialog(Purpose::Open),
            Action::Goto => self.goto_key(),
            Action::EditCell if self.on_header() == Some(Header::Row) && self.cursor_footer().is_none() => self.leave_header(),
            Action::EditCell => self.start_edit(None),
            Action::ClearCells => self.clear_selection(),
            Action::Copy => self.copy(),
            Action::Cut => self.cut(),
            Action::Paste => {
                let grid = self.clipboard.clone();
                self.paste_grid(grid);
            }
            Action::Undo => self.undo(),
            Action::Redo => self.redo(),
            Action::FillDown => self.fill_down(),
            Action::FillRight => self.fill_right(),
            Action::InsertRow => self.command("addrow"),
            Action::InsertColumn => self.insert_column_key(),
            Action::DeleteLines => match self.on_header() {
                Some(Header::Column) => self.command("delcol"),
                Some(Header::Corner) => self.error("F8 deletes rows or columns, not the whole table"),
                _ => self.command("delrow"),
            },
            Action::Sort => self.command("sort"),
            Action::Pivot => self.toggle_pivot(None),
            Action::Split => self.toggle_split(),
            Action::SwapWindows => self.swap_windows(),
            Action::NextTable => self.switch_sheet(self.view.sheet + 1),
            Action::PrevTable => self.switch_sheet(self.view.sheet.saturating_sub(1)),
            Action::Bold => self.toggle_style("bold"),
        }
    }

    /// Right-click or Ctrl+click: on a cell the cursor goes there first.
    pub(super) fn context_click(&mut self, hit: Option<Hit>, pos: (u16, u16)) {
        if let Some(Hit::Cell(r, c)) = hit
            && !self.selection_contains(r, c)
        {
            self.view.cursor = (r, c);
            self.view.anchor = None;
        }
        self.open_context_menu(Some((pos.0, pos.1 + 1)));
    }

    fn selection_contains(&self, r: usize, c: usize) -> bool {
        let ((r0, c0), (r1, c1)) = self.selection();
        (r0..=r1).contains(&r) && (c0..=c1).contains(&c)
    }

    /// Remembers what this terminal can do, for the next start.
    pub(super) fn save_caps(&self) {
        if let Some(p) = &self.caps_path {
            if let Some(dir) = p.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = std::fs::write(p, self.caps.to_lines());
        }
    }

    /// Starts the key test (help page Keys, `t`, or `:keytest`).
    pub fn start_key_test(&mut self) {
        self.show_help = false;
        self.key_test = Some(0);
        self.ask_key_test();
    }

    fn ask_key_test(&mut self) {
        match self.key_test.and_then(|i| KEY_TEST.get(i)) {
            Some((_, ask)) => {
                let i = self.key_test.unwrap_or(0);
                self.info(format!("key test {}/{}: {ask} · n: pressed, nothing happened · Esc: skip", i + 1, KEY_TEST.len()));
            }
            None => {
                self.key_test = None;
                self.save_caps();
                let state = |c: Cond| match self.caps.get(c) {
                    Cap::Guaranteed | Cap::Confirmed => "works",
                    Cap::Impossible => "not here",
                    Cap::Unknown => "unknown",
                };
                self.info(format!(
                    "key test done – Menu key: {}, Shift+F10: {}, right-click: {} (F1 → Keys shows what works)",
                    state(Cond::MenuKey),
                    state(Cond::ShiftF10),
                    state(Cond::Mouse)
                ));
            }
        }
    }

    pub(super) fn key_test_answer(&mut self, cap: Cap) {
        let Some(i) = self.key_test else { return };
        if let Some((cond, _)) = KEY_TEST.get(i) {
            self.caps.set(*cond, cap);
        }
        self.key_test = Some(i + 1);
        self.ask_key_test();
    }

    pub(super) fn key_test_key(&mut self, key: KeyEvent) {
        let Some(i) = self.key_test else { return };
        let expected = match KEY_TEST.get(i).map(|t| t.0) {
            Some(Cond::MenuKey) => key.code == KeyCode::Menu,
            Some(Cond::ShiftF10) => key.code == KeyCode::F(10) && key.modifiers == KeyModifiers::SHIFT,
            _ => false,
        };
        match key.code {
            _ if expected => self.key_test_answer(Cap::Confirmed),
            KeyCode::Char('n') => self.key_test_answer(Cap::Impossible),
            KeyCode::Esc => {
                self.key_test = Some(i + 1);
                self.ask_key_test();
            }
            // Something else arrived: ask again.
            _ => self.ask_key_test(),
        }
    }
}
