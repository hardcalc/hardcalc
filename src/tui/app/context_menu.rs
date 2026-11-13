//! The context menu: `/`, Menu key, Shift+F10, right-click or Ctrl+click.
//!
//! Two levels with a letter per entry, so menu sequences double as
//! shortcuts: `/ R I` inserts a row. Entries show their keys from the
//! KEYMAP – only those that work in this terminal.

use super::{App, Hit};
use crate::tui::keymap::{self, Action};
use ratatui::crossterm::event::{KeyCode, KeyEvent};

/// What an entry does.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Run {
    Action(Action),
    /// A `:` command; a trailing space asks for an argument.
    Command(&'static str),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Entry {
    pub letter: char,
    pub label: &'static str,
    pub run: Run,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Group {
    pub letter: char,
    pub label: &'static str,
    pub entries: &'static [Entry],
}

const fn a(letter: char, label: &'static str, action: Action) -> Entry {
    Entry { letter, label, run: Run::Action(action) }
}

const fn c(letter: char, label: &'static str, cmd: &'static str) -> Entry {
    Entry { letter, label, run: Run::Command(cmd) }
}

pub static CONTEXT_MENU: &[Group] = &[
    Group {
        letter: 'E',
        label: "Edit",
        entries: &[
            a('C', "Copy", Action::Copy),
            a('X', "Cut", Action::Cut),
            a('P', "Paste", Action::Paste),
            a('L', "Clear", Action::ClearCells),
            a('U', "Undo", Action::Undo),
            a('R', "Redo", Action::Redo),
        ],
    },
    Group {
        letter: 'F',
        label: "Format",
        entries: &[
            a('B', "Bold", Action::Bold),
            c('I', "Italic", "italic"),
            c('U', "Underline", "underline"),
            c('C', "Text color…", "fg "),
            c('F', "Fill color…", "bg "),
            c('P', "Plain", "plain"),
        ],
    },
    Group {
        letter: 'R',
        label: "Row",
        entries: &[
            a('I', "Insert", Action::InsertRow),
            a('D', "Delete", Action::DeleteLines),
            a('F', "Fill down", Action::FillDown),
            c('A', "Add footer row", "footer"),
        ],
    },
    Group {
        letter: 'C',
        label: "Column",
        entries: &[
            a('I', "Insert…", Action::InsertColumn),
            c('D', "Delete", "delcol"),
            c('T', "Type…", "type "),
            a('S', "Sort", Action::Sort),
            c('O', "Sort columns individually", "sortcol"),
            c('V', "Filter by this value", "filter"),
            c('L', "Filter: choose values…", "values"),
            c('N', "Filter by condition…", "filter "),
            c('R', "Remove filter", "filter off"),
            a('F', "Fill right", Action::FillRight),
        ],
    },
    Group {
        letter: 'T',
        label: "Table",
        entries: &[
            c('N', "New table…", "table "),
            a('P', "Pivot", Action::Pivot),
            c('H', "Chart", "chart"),
            c('S', "Slice…", "slice "),
            c('O', "Scenario…", "scenario "),
        ],
    },
    Group {
        letter: 'V',
        label: "View",
        entries: &[
            a('S', "Split window", Action::Split),
            a('W', "Swap windows", Action::SwapWindows),
            c('I', "Inspector", "inspector"),
            c('O', "Source", "source"),
            c('T', "Types", "types"),
            c('F', "Functions", "functions"),
        ],
    },
];

/// The open context menu.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ContextMenu {
    /// Top left corner on screen: the cursor cell, or where was clicked.
    pub pos: (u16, u16),
    /// The opened group; `None`: the group list.
    pub group: Option<usize>,
    pub selected: usize,
}

impl ContextMenu {
    /// (label, letter, keys) of the shown entries.
    pub fn rows(&self, caps: &keymap::Caps) -> Vec<(&'static str, char, String)> {
        match self.group {
            None => CONTEXT_MENU.iter().map(|g| (g.label, g.letter, "›".to_string())).collect(),
            Some(g) => CONTEXT_MENU[g]
                .entries
                .iter()
                .map(|e| {
                    let keys = match e.run {
                        Run::Action(act) => keymap::keys_for(act, caps),
                        Run::Command(cmd) => format!(":{}", cmd.trim_end()),
                    };
                    (e.label, e.letter, keys)
                })
                .collect(),
        }
    }
}

impl App {
    /// Opens the context menu at `pos`, or at the cursor cell.
    pub fn open_context_menu(&mut self, pos: Option<(u16, u16)>) {
        let pos = pos.unwrap_or_else(|| {
            let cursor = self.view.cursor;
            self.hits
                .iter()
                .find(|(_, h)| *h == Hit::Cell(cursor.0, cursor.1))
                .map(|(r, _)| (r.x, r.y + 1))
                .unwrap_or((2, 3))
        });
        self.menu = None;
        self.context = Some(ContextMenu { pos, group: None, selected: 0 });
        self.message = None;
    }

    pub(super) fn key_context(&mut self, key: KeyEvent) {
        let Some(mut m) = self.context else { return };
        let n = m.rows(&self.caps).len();
        match key.code {
            KeyCode::Esc | KeyCode::F(9) | KeyCode::F(10) => self.context = None,
            KeyCode::F(1) => {
                self.context = None;
                self.open_help();
            }
            KeyCode::Left | KeyCode::Backspace if m.group.is_some() => {
                m.selected = m.group.unwrap_or(0);
                m.group = None;
                self.context = Some(m);
            }
            KeyCode::Up => {
                m.selected = (m.selected + n - 1) % n;
                self.context = Some(m);
            }
            KeyCode::Down => {
                m.selected = (m.selected + 1) % n;
                self.context = Some(m);
            }
            KeyCode::Enter | KeyCode::Right | KeyCode::Char(' ') => self.choose_context(m.selected),
            KeyCode::Char(ch) => {
                let ch = ch.to_ascii_uppercase();
                if let Some(i) = m.rows(&self.caps).iter().position(|(_, l, _)| *l == ch) {
                    self.choose_context(i);
                }
            }
            _ => {}
        }
    }

    /// Opens group `i`, or runs entry `i` of the open group.
    pub(super) fn choose_context(&mut self, i: usize) {
        let Some(mut m) = self.context else { return };
        match m.group {
            None if i < CONTEXT_MENU.len() => {
                m.group = Some(i);
                m.selected = 0;
                self.context = Some(m);
            }
            Some(g) => {
                self.context = None;
                match CONTEXT_MENU[g].entries.get(i).map(|e| e.run) {
                    Some(Run::Action(act)) => self.handle(act),
                    Some(Run::Command(cmd)) if cmd.ends_with(' ') => {
                        self.mode = super::Mode::Command { buf: cmd.to_string() };
                        self.cmd_choice = None;
                    }
                    Some(Run::Command(cmd)) => self.run_command_line(cmd),
                    None => {}
                }
            }
            None => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::keymap::{Trigger, KEYMAP};

    #[test]
    fn letters_are_unique() {
        let groups: Vec<char> = CONTEXT_MENU.iter().map(|g| g.letter).collect();
        assert!(groups.iter().enumerate().all(|(i, l)| !groups[i + 1..].contains(l)));
        for g in CONTEXT_MENU {
            let ls: Vec<char> = g.entries.iter().map(|e| e.letter).collect();
            assert!(ls.iter().enumerate().all(|(i, l)| !ls[i + 1..].contains(l)), "{}", g.label);
        }
    }

    #[test]
    fn keymap_sequences_match_the_menu() {
        for b in KEYMAP.iter().filter(|b| b.trigger == Trigger::Other && b.label.starts_with("/ ")) {
            let letters: Vec<char> = b.label.split_whitespace().skip(1).filter_map(|s| s.chars().next()).collect();
            let [g, e] = letters[..] else { panic!("{}", b.label) };
            let group = CONTEXT_MENU.iter().find(|x| x.letter == g).unwrap_or_else(|| panic!("{}", b.label));
            let entry = group.entries.iter().find(|x| x.letter == e).unwrap_or_else(|| panic!("{}", b.label));
            assert_eq!(entry.run, Run::Action(b.action), "{}", b.label);
        }
    }
}
