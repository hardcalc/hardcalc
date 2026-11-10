//! Documents and files from within the UI: a new untitled document, opening
//! another file, appending one, and file name completion in the command line.

use super::{new_table, tables, App, Purpose};
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use crate::model::{Block, Document, Shape};
use std::path::{Path, PathBuf};

/// What to do once unsaved changes are saved or discarded.
#[derive(Debug, Clone, PartialEq)]
pub enum Pending {
    /// The file dialog for opening.
    Dialog,
    Open(String),
    New,
}

/// `~/x` → home directory; a name without extension gets `.hcmd`.
pub fn document_path(name: &str) -> PathBuf {
    let path = expand_home(name.trim());
    if path.extension().is_none() { path.with_extension(crate::storage::EXTENSION) } else { path }
}

fn expand_home(name: &str) -> PathBuf {
    let home = crate::tui::file_dialog::home_dir();
    match (name.strip_prefix("~/").or_else(|| name.strip_prefix("~\\")), home) {
        (Some(rest), Some(home)) => home.join(rest),
        (None, Some(home)) if name == "~" => home,
        _ => PathBuf::from(name),
    }
}

/// A new document: one empty playground.
pub fn new_document() -> Document {
    let mut table = new_table("Sheet1");
    table.shape = Shape::Playground;
    table.columns.truncate(1);
    table.rows.clear();
    Document { blocks: vec![Block::Table(table)], version: Some(crate::model::FORMAT_VERSION) }
}

impl App {
    /// A new, untitled document (`hardcalc` without a file).
    pub fn untitled() -> App {
        let mut app = App::new(PathBuf::new(), new_document());
        app.info("new document – :w name saves it, :e file opens one, :append file adds one (Tab completes names)");
        app
    }

    /// Column `c` of table `sheet` as written in the document.
    pub fn doc_column(&self, sheet: usize, c: usize) -> crate::model::Column {
        tables(&self.doc).nth(sheet).and_then(|t| t.columns.get(c)).cloned().expect("column")
    }

    /// File name of the document (without folder).
    pub fn doc_file_name(&self) -> String {
        self.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "untitled".into())
    }

    /// Not saved to a file yet.
    pub fn is_untitled(&self) -> bool {
        self.path.as_os_str().is_empty()
    }

    /// Replaces the document, keeping the screen settings.
    fn replace_with(&mut self, mut next: App) {
        next.theme = self.theme;
        next.detected_light = self.detected_light;
        next.graphics = self.graphics;
        next.truecolor = self.truecolor;
        next.bands = self.bands;
        next.anim = std::mem::take(&mut self.anim);
        next.show_keys = self.show_keys;
        *self = next;
    }

    /// Runs `action` – after asking to save or discard unsaved changes.
    pub(super) fn after_unsaved(&mut self, action: Pending, force: bool) {
        if self.dirty && !force {
            self.confirm = Some(action);
            self.message = None;
        } else {
            self.run_pending(action);
        }
    }

    pub(super) fn run_pending(&mut self, action: Pending) {
        match action {
            Pending::Dialog => self.show_file_dialog(Purpose::Open),
            Pending::Open(name) => self.open_file(&name, true),
            Pending::New => self.replace_with(App::untitled()),
        }
    }

    /// Answer to the unsaved-changes question: 0 save, 1 discard, 2 cancel.
    pub(super) fn answer_confirm(&mut self, choice: usize) {
        let Some(action) = self.confirm.take() else { return };
        match choice {
            0 if self.is_untitled() => {
                // The name first; the action follows the successful save.
                self.after_save = Some(action);
                self.save(None);
            }
            0 => {
                self.save(None);
                if !self.dirty {
                    self.run_pending(action);
                }
            }
            1 => self.run_pending(action),
            _ => {}
        }
    }

    pub(super) fn key_confirm(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('s' | 'S' | 'w' | 'W') | KeyCode::Enter | KeyCode::F(2) => self.answer_confirm(0),
            KeyCode::Char('d' | 'D' | 'n' | 'N') => self.answer_confirm(1),
            KeyCode::Esc | KeyCode::Char('c' | 'C') | KeyCode::F(10) => self.answer_confirm(2),
            _ => {}
        }
    }

    /// `:e file` / `:open file`: another document (spreadsheets are imported).
    pub(super) fn open_file(&mut self, name: &str, force: bool) {
        if self.dirty && !force {
            return self.after_unsaved(Pending::Open(name.to_string()), false);
        }
        let path = expand_home(name.trim());
        let path = if path.exists() || crate::import::is_spreadsheet(&path) { path } else { document_path(name) };
        match App::open(&path) {
            Ok(next) => {
                let existed = path.exists();
                let message = next.message.clone();
                self.replace_with(next);
                match message {
                    Some(m) => self.message = Some(m),
                    None if existed => self.info(format!("opened {}", path.display())),
                    None => self.info(format!("new file {} – F2 creates it", path.display())),
                }
            }
            Err(e) => self.error(format!("cannot open {}: {e}", path.display())),
        }
    }

    /// `:new`: an untitled document with a playground.
    pub(super) fn new_file(&mut self, force: bool) {
        self.after_unsaved(Pending::New, force);
    }

    /// `:append file`: adds the tables, layers, charts, layouts, types,
    /// functions and text of another document (or a spreadsheet's sheets).
    /// Tables whose name is taken get a number; references follow.
    pub(super) fn append_file(&mut self, name: &str) {
        let path = expand_home(name.trim());
        if crate::import::is_spreadsheet(&path) {
            return self.import_sheets(&path);
        }
        let path = if path.exists() { path } else { document_path(name) };
        let mut other = match crate::storage::read_text(&path).map(|(s, _)| s).map_err(|e| e.to_string()).and_then(|s| crate::parse(&s).map_err(|e| e.to_string())) {
            Ok(doc) => doc,
            Err(e) => return self.error(format!("cannot read {}: {e}", path.display())),
        };
        let mut taken: Vec<String> = self.wb.sheets.iter().filter_map(|s| s.name.clone()).collect();
        let names: Vec<Option<String>> = tables(&other).map(|t| t.name.clone()).collect();
        let mut renamed = Vec::new();
        for (i, name) in names.into_iter().enumerate() {
            let Some(name) = name else { continue };
            if taken.contains(&name) {
                let new = (2..).map(|k| format!("{name} {k}")).find(|n| !taken.contains(n)).unwrap();
                crate::refs::rename_table(&mut other, i, &new);
                renamed.push(format!("{name} → {new}"));
                taken.push(new);
            } else {
                taken.push(name);
            }
        }
        // The other document's title is left out.
        if let Some(Block::Text(lines)) = other.blocks.first_mut()
            && lines.first().is_some_and(|l| l.starts_with("# "))
        {
            lines.remove(0);
            while lines.first().is_some_and(|l| l.trim().is_empty()) {
                lines.remove(0);
            }
            if lines.is_empty() {
                other.blocks.remove(0);
            }
        }
        let added = tables(&other).count();
        let first_new = self.wb.sheets.len();
        self.with_doc(|doc| doc.blocks.extend(other.blocks));
        if added > 0 {
            self.switch_sheet(first_new);
        }
        let file = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let note = if renamed.is_empty() { String::new() } else { format!(" (renamed {})", renamed.join(", ")) };
        self.info(format!("appended {file}: {added} table(s){note}"));
    }

    /// Tab in the command line: completes the file name of commands that take one.
    pub(super) fn complete_command(&mut self, buf: &str) -> Option<String> {
        let (cmd, arg) = buf.split_once(' ').unwrap_or((buf, ""));
        if !crate::tui::commands::FILE_COMMANDS.contains(&cmd) {
            return None;
        }
        let (dir_part, prefix) = match arg.rfind(['/', '\\']) {
            Some(i) => (&arg[..=i], &arg[i + 1..]),
            None => ("", arg),
        };
        let dir = if dir_part.is_empty() { PathBuf::from(".") } else { expand_home(dir_part) };
        let mut names: Vec<(String, bool)> = std::fs::read_dir(&dir)
            .ok()?
            .filter_map(|e| e.ok())
            .map(|e| (e.file_name().to_string_lossy().into_owned(), e.path().is_dir()))
            .filter(|(n, _)| n.starts_with(prefix) && (!n.starts_with('.') || prefix.starts_with('.')))
            .collect();
        names.sort();
        match names.as_slice() {
            [] => {
                self.error(format!("no file matches '{arg}'"));
                None
            }
            [(name, dir)] => Some(format!("{cmd} {dir_part}{name}{}", if *dir { std::path::MAIN_SEPARATOR_STR } else { "" })),
            _ => {
                let common = names.iter().skip(1).fold(names[0].0.clone(), |acc, (n, _)| {
                    acc.chars().zip(n.chars()).take_while(|(a, b)| a == b).map(|(a, _)| a).collect()
                });
                let shown: Vec<String> = names.iter().take(12).map(|(n, d)| if *d { format!("{n}/") } else { n.clone() }).collect();
                let more = if names.len() > 12 { format!(" … +{}", names.len() - 12) } else { String::new() };
                self.info(format!("{}{more}", shown.join("  ")));
                Some(format!("{cmd} {dir_part}{common}"))
            }
        }
    }
}

/// Default PDF name of an untitled document.
pub fn untitled_pdf() -> &'static Path {
    Path::new("untitled.pdf")
}
