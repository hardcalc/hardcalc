//! The menu bar (F9, or a click on `hardcalc`) and the suggestions while
//! typing a `:` command.

use super::{App, Mode, Shape};
use crate::tui::commands::{self, MENUS};
use ratatui::crossterm::event::{KeyCode, KeyEvent};

impl App {
    pub fn open_menu(&mut self, top: usize) {
        if matches!(self.mode, Mode::Edit { .. }) {
            self.commit_edit(0, 0);
        }
        self.mode = Mode::Normal;
        self.menu = Some((top.min(MENUS.len() - 1), 0));
        self.message = None;
    }

    /// Runs entry `item` of menu `top`.
    pub(super) fn run_menu_item(&mut self, top: usize, item: usize) {
        self.menu = None;
        let Some(entry) = commands::items(top).get(item).map(|i| i.run) else { return };
        if entry.ends_with(' ') {
            // The command needs an argument: the command line opens with it.
            self.mode = Mode::Command { buf: entry.to_string() };
            self.cmd_choice = None;
        } else {
            self.run_command_line(entry);
        }
    }

    /// Runs a command line (in the main window when a helper is focused).
    pub(super) fn run_command_line(&mut self, cmd: &str) {
        let left = if self.view.is_data() { self.left_focused } else { true };
        self.with_window(left, |app| app.command(cmd));
    }

    pub(super) fn key_menu(&mut self, key: KeyEvent) {
        let Some((top, item)) = self.menu else { return };
        let n = commands::items(top).len();
        match key.code {
            KeyCode::Esc | KeyCode::F(9) | KeyCode::F(10) => self.menu = None,
            KeyCode::Left => self.menu = Some(((top + MENUS.len() - 1) % MENUS.len(), 0)),
            KeyCode::Right | KeyCode::Tab => self.menu = Some(((top + 1) % MENUS.len(), 0)),
            KeyCode::Up => self.menu = Some((top, (item + n - 1) % n)),
            KeyCode::Down => self.menu = Some((top, (item + 1) % n)),
            KeyCode::Home | KeyCode::PageUp => self.menu = Some((top, 0)),
            KeyCode::End | KeyCode::PageDown => self.menu = Some((top, n - 1)),
            KeyCode::Enter | KeyCode::Char(' ') => self.run_menu_item(top, item),
            // A letter: the next entry starting with it; the only one runs.
            KeyCode::Char(ch) => {
                let ch = ch.to_ascii_lowercase();
                let items = commands::items(top);
                let hits: Vec<usize> = (0..n).filter(|&i| items[i].label.to_lowercase().starts_with(ch)).collect();
                match hits.as_slice() {
                    [] => {}
                    [only] => self.run_menu_item(top, *only),
                    _ => {
                        let next = hits.iter().find(|&&i| i > item).unwrap_or(&hits[0]);
                        self.menu = Some((top, *next));
                    }
                }
            }
            _ => {}
        }
    }

    /// Suggestions for the command line being typed.
    pub fn command_suggestions(&self) -> Vec<(String, String, String)> {
        let Mode::Command { buf } = &self.mode else { return Vec::new() };
        commands::suggestions(buf, |cmd| match cmd {
            "layer" | "solo" => self.layer_names.clone(),
            "sort" => self.wb.sheets[self.view.sheet].columns.iter().map(|c| c.name.clone()).collect(),
            // Own types besides the built-in ones.
            "type" => self
                .type_rows()
                .into_iter()
                .filter_map(|r| match r {
                    super::TypeRow::User { name, .. } => Some(name),
                    _ => None,
                })
                .collect(),
            "pivot" => match &self.wb.sheets[self.view.sheet].shape {
                Shape::Tensor(axes) => axes.clone(),
                _ => Vec::new(),
            },
            _ => Vec::new(),
        })
    }

    /// Up / Down in the command line: walks through the suggestions.
    pub(super) fn choose_suggestion(&mut self, step: isize) {
        let n = self.command_suggestions().len();
        if n == 0 {
            return;
        }
        self.cmd_choice = Some(match self.cmd_choice {
            None if step > 0 => 0,
            None => n - 1,
            Some(i) => (i as isize + step).rem_euclid(n as isize) as usize,
        });
    }

    /// Tab (or Enter on a chosen suggestion): completes the command line.
    /// Returns whether a suggestion was applied.
    pub(super) fn apply_suggestion(&mut self) -> bool {
        let list = self.command_suggestions();
        let done = match (self.cmd_choice, list.as_slice()) {
            (Some(i), _) if i < list.len() => list[i].0.clone(),
            (_, [(only, ..)]) => only.clone(),
            (_, [first, rest @ ..]) => {
                // The common beginning of all suggestions.
                let common: String = rest.iter().fold(first.0.clone(), |acc, (s, ..)| {
                    acc.chars().zip(s.chars()).take_while(|(a, b)| a == b).map(|(a, _)| a).collect()
                });
                let Mode::Command { buf } = &self.mode else { return false };
                if common.len() <= buf.len() {
                    return false;
                }
                common
            }
            _ => return false,
        };
        self.mode = Mode::Command { buf: done };
        self.cmd_choice = None;
        true
    }
}
