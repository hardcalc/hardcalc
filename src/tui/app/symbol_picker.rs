//! The list of special characters (Ctrl+G while typing, `:symbol`,
//! Insert → Symbol…): type part of a name, a LaTeX name (`\le`) or an
//! ASCII spelling (`<=`), Enter inserts the character.

use super::{App, Mode};
use crate::symbols::{self, Symbol};
use ratatui::crossterm::event::{KeyCode, KeyEvent};

/// Where the picked character goes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SymbolTarget {
    /// The text being edited, at its cursor.
    Edit,
    /// The command line.
    Command,
    /// The current cell: it is opened for editing with the character at its end.
    Cell,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SymbolPicker {
    pub query: String,
    /// Index into the shown characters.
    pub selected: usize,
    pub scroll: usize,
    pub target: SymbolTarget,
}

/// How many characters the list keeps as recently used.
const RECENT: usize = 12;

impl App {
    /// The characters the picker lists for its query, the recently used ones first.
    pub fn symbols_shown(&self) -> Vec<&'static Symbol> {
        let query = self.symbols.as_ref().map_or("", |p| p.query.as_str());
        symbols::search(query, &self.recent_symbols)
    }

    /// Ctrl+G while typing, `:symbol`, Insert → Symbol…: opens the list.
    pub fn open_symbols(&mut self) {
        let target = match self.mode {
            Mode::Edit { .. } => SymbolTarget::Edit,
            Mode::Command { .. } => SymbolTarget::Command,
            Mode::Normal => SymbolTarget::Cell,
        };
        self.symbols = Some(SymbolPicker { query: String::new(), selected: 0, scroll: 0, target });
        self.message = None;
    }

    /// `:symbol Delta`: the best match for the name, at once.
    pub fn insert_symbol_named(&mut self, name: &str) {
        match symbols::search(name.trim(), &[]).first() {
            Some(s) => self.put_symbol(s.ch, SymbolTarget::Cell),
            None => self.error(format!("no character '{name}' – :symbol without a name lists them all")),
        }
    }

    pub(super) fn key_symbols(&mut self, key: KeyEvent) {
        let n = self.symbols_shown().len();
        let Some(p) = &mut self.symbols else { return };
        let last = n.saturating_sub(1);
        match key.code {
            KeyCode::Esc if !p.query.is_empty() => {
                p.query.clear();
                p.selected = 0;
            }
            KeyCode::Esc | KeyCode::F(10) => self.symbols = None,
            KeyCode::Up => p.selected = p.selected.saturating_sub(1),
            KeyCode::Down => p.selected = (p.selected + 1).min(last),
            KeyCode::PageUp => p.selected = p.selected.saturating_sub(10),
            KeyCode::PageDown => p.selected = (p.selected + 10).min(last),
            KeyCode::Home => p.selected = 0,
            KeyCode::End => p.selected = last,
            KeyCode::Backspace => {
                p.query.pop();
                p.selected = 0;
            }
            KeyCode::Char(c) => {
                p.query.push(c);
                p.selected = 0;
            }
            KeyCode::Enter | KeyCode::Tab | KeyCode::F(2) => {
                let i = p.selected;
                self.pick_symbol(i);
            }
            _ => {}
        }
    }

    /// Takes character `i` of the list (Enter, or a click on it).
    pub fn pick_symbol(&mut self, i: usize) {
        let ch = self.symbols_shown().get(i).map(|s| s.ch);
        let Some(p) = self.symbols.take() else { return };
        if let Some(ch) = ch {
            self.put_symbol(ch, p.target);
        }
    }

    fn put_symbol(&mut self, ch: &'static str, target: SymbolTarget) {
        self.recent_symbols.retain(|r| r != ch);
        self.recent_symbols.insert(0, ch.to_string());
        self.recent_symbols.truncate(RECENT);
        if target == SymbolTarget::Cell {
            self.start_edit(None);
            // The cell could not be edited: the reason is in the status line.
            let Mode::Edit { buf, cursor } = &mut self.mode else { return };
            *cursor = buf.chars().count();
        }
        self.insert_text(ch.to_string());
    }
}
