//! File dialog (Midnight Commander style) for opening and appending files:
//! a list of folders and documents to walk through, with quick search.

use super::App;
pub use crate::tui::file_dialog::{FileDialog, Purpose};
use crate::tui::file_dialog::Action;
use ratatui::crossterm::event::KeyEvent;
use std::path::{Path, PathBuf};

/// Files hardcalc can open or append.
pub fn is_document(name: &str) -> bool {
    let lower = name.to_lowercase();
    crate::storage::is_document_name(&lower)
        || crate::bundle::Kind::from_path(Path::new(&lower)).is_some()
        || crate::import::is_spreadsheet(Path::new(&lower))
}

impl App {
    /// The file dialog; for opening, unsaved changes are settled first.
    pub fn open_file_dialog(&mut self, purpose: Purpose) {
        match purpose {
            Purpose::Open => self.after_unsaved(super::files::Pending::Dialog, false),
            Purpose::Append => self.show_file_dialog(purpose),
        }
    }

    /// Shows the file dialog in the folder of the current file.
    pub(super) fn show_file_dialog(&mut self, purpose: Purpose) {
        let dir = match self.path.parent() {
            Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
            _ => PathBuf::from("."),
        };
        let dir = std::fs::canonicalize(&dir).unwrap_or(dir);
        self.file_dialog = Some(FileDialog::new(purpose, dir, is_document));
        self.message = None;
    }

    /// Acts on what the dialog reports: closing it or taking a file.
    fn dialog_action(&mut self, action: Action) {
        match action {
            Action::None => {}
            Action::Close => self.file_dialog = None,
            Action::Chosen(path) => {
                let Some(d) = self.file_dialog.take() else { return };
                let name = path.display().to_string();
                match d.purpose {
                    // Unsaved changes were saved or discarded before the dialog opened.
                    Purpose::Open => self.open_file(&name, true),
                    Purpose::Append => self.append_file(&name),
                }
            }
        }
    }

    pub(super) fn key_file_dialog(&mut self, key: KeyEvent) {
        let Some(d) = &mut self.file_dialog else { return };
        let action = d.key(key);
        self.dialog_action(action);
    }

    /// Click on entry `i` of the list (double click: choose it).
    pub(super) fn click_file_entry(&mut self, i: usize, double: bool) {
        let Some(d) = &mut self.file_dialog else { return };
        let action = d.click(i, double);
        self.dialog_action(action);
    }

    pub(super) fn scroll_file_dialog(&mut self, lines: isize) {
        if let Some(d) = &mut self.file_dialog {
            d.scroll_by(lines);
        }
    }
}
