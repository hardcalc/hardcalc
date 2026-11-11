//! State and keys of hardconf: tree on the left, form and documentation on the right.

use crate::config::edit::{self, choices, edit_text, parse_input, value_at};
use crate::config::view::{active, Row, RowKind, Shape, TreeNode};
use crate::config::{ConfigFile, Diagnostic, Field, FieldType, Path, Severity};
use crate::tui::file_dialog::{Action, FileDialog, Purpose};
use crate::tui::theme::ThemeSetting;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::{Duration, Instant};
use toml_edit::{DocumentMut, Value};

/// Files hardconf offers in the file dialog.
pub fn is_toml(name: &str) -> bool {
    name.to_lowercase().ends_with(".toml")
}

/// Label in the file dialog for files with a `#%hardconf` schema block.
pub fn format_tag(path: &std::path::Path) -> Option<&'static str> {
    // Only files of a size a configuration has.
    if std::fs::metadata(path).ok()?.len() > 4 << 20 {
        return None;
    }
    let text = std::fs::read_to_string(path).ok()?;
    crate::config::is_hardconf(&text).then_some("hardconf")
}

/// What kind of TOML the open file is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Format {
    /// A usable schema block of this version.
    Hardconf(u32),
    /// A `#%hardconf` block with problems, or one that cannot be used.
    Broken,
    /// No `#%hardconf` block.
    Plain,
}

const DOUBLE_CLICK: Duration = Duration::from_millis(400);

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Focus {
    Tree,
    Form,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Mode {
    Normal,
    /// `cursor` is a char index into `buf`; `row` the form row being edited.
    Edit { buf: String, cursor: usize, multiline: bool, row: usize },
    /// Choice list: `(value, meaning)`; `purpose` says what the choice is for.
    Choose { options: Vec<(String, String)>, cursor: usize, row: usize, purpose: ChooseFor },
    /// Typing the key of a new entry of the map (or, `generic`, the table) at `map`.
    NewKey { buf: String, cursor: usize, map: Path, generic: bool },
    /// Writing the comment of the table or key at `target`.
    Comment { buf: String, cursor: usize, target: Path },
    Confirm(Confirm),
}

/// What a choice list is for.
#[derive(Debug, Clone, PartialEq)]
pub enum ChooseFor {
    /// The value of the form row (enum, ref, bool).
    Value,
    /// The key of a new entry of the map at the path.
    NewKey(Path),
    /// The kind of the new key `key` in the table `parent` (without schema).
    NewKind { parent: Path, key: String },
    /// The new kind of the value at the path (without schema).
    Retype(Path),
}

/// Where F7 adds something.
#[derive(Debug, Clone)]
pub enum AddTarget {
    /// An entry of a map of the schema.
    Map(Path, Box<Field>),
    /// A key or table in a table without schema.
    Table(Path),
    /// An element of a `[[list]]` without schema.
    Tables(Path),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Confirm {
    SaveWithErrors,
    /// Unsaved changes before quitting or opening another file.
    Unsaved(Then),
    /// Deleting the map entry at the path.
    Delete(Path),
}

/// What happens once unsaved changes are settled.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Then {
    Quit,
    Open,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Hit {
    Tree(usize),
    Toggle(usize),
    Row(usize),
    Choice(usize),
    FKey(u8),
    /// An entry of the file dialog's list or a place of its sidebar.
    File(crate::tui::file_dialog::Spot),
}

pub struct Message {
    pub text: String,
    pub error: bool,
}

pub struct ConfApp {
    pub path: PathBuf,
    pub cfg: ConfigFile,
    /// The text as last loaded or saved (diagnostics already removed).
    saved: String,
    pub expanded: HashSet<Path>,
    /// The selected tree node.
    pub selected: Path,
    pub row: usize,
    pub focus: Focus,
    pub mode: Mode,
    pub message: Option<Message>,
    /// Show `secret` values in clear text.
    pub reveal: bool,
    pub show_help: bool,
    pub quit: bool,
    undo: Vec<DocumentMut>,
    redo: Vec<DocumentMut>,
    /// Findings of the schema check and of the file, refreshed after each change.
    pub diagnostics: Vec<Diagnostic>,
    pub comments: HashMap<Path, String>,
    pub theme: ThemeSetting,
    pub detected_light: Option<bool>,
    pub hits: Vec<(Rect, Hit)>,
    pub tree_scroll: usize,
    pub form_scroll: usize,
    /// Rows of the form that fit on screen, set while drawing.
    pub page: usize,
    pub file_dialog: Option<FileDialog>,
    last_click: Option<(Instant, u16, u16)>,
    /// Text copied with Ctrl+C.
    pub clipboard: Option<String>,
    /// Text for the system clipboard, sent to the terminal (OSC 52) after drawing.
    pub to_system_clipboard: Option<String>,
}

impl ConfApp {
    pub fn open(path: &std::path::Path) -> std::io::Result<ConfApp> {
        let cfg = ConfigFile::load(path).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.0))?;
        Ok(ConfApp::new(path.to_path_buf(), cfg))
    }

    pub fn new(path: PathBuf, cfg: ConfigFile) -> ConfApp {
        let saved = cfg.to_text();
        let mut app = ConfApp {
            path,
            cfg,
            saved,
            expanded: HashSet::new(),
            selected: Path::root(),
            row: 0,
            focus: Focus::Tree,
            mode: Mode::Normal,
            message: None,
            reveal: false,
            show_help: false,
            quit: false,
            undo: Vec::new(),
            redo: Vec::new(),
            diagnostics: Vec::new(),
            comments: HashMap::new(),
            theme: ThemeSetting::Auto,
            detected_light: None,
            hits: Vec::new(),
            tree_scroll: 0,
            form_scroll: 0,
            page: 10,
            file_dialog: None,
            last_click: None,
            clipboard: None,
            to_system_clipboard: None,
        };
        // Maps directly below the root start expanded: their entries are what one edits.
        let root = app.tree().remove(0);
        for c in app.cfg.children(&root) {
            if matches!(c.shape, Shape::Map { .. }) {
                app.expanded.insert(c.path);
            }
        }
        app.refresh();
        if !app.cfg.schema_errors.is_empty() {
            app.error(format!("schema: {} problem(s), see the root node", app.cfg.schema_errors.len()));
        }
        app
    }

    pub fn file_name(&self) -> String {
        self.path.file_name().map_or_else(|| self.path.display().to_string(), |n| n.to_string_lossy().into_owned())
    }

    pub fn light(&self) -> bool {
        match self.theme {
            ThemeSetting::Light => true,
            ThemeSetting::Dark => false,
            ThemeSetting::Auto => self.detected_light.unwrap_or(false),
        }
    }

    pub fn format(&self) -> Format {
        match &self.cfg.schema {
            Some(s) if self.cfg.schema_errors.is_empty() => Format::Hardconf(s.version),
            _ if !self.cfg.schema_errors.is_empty() => Format::Broken,
            _ => Format::Plain,
        }
    }

    pub fn modified(&self) -> bool {
        self.cfg.to_text() != self.saved
    }

    fn refresh(&mut self) {
        let mut d: Vec<Diagnostic> = self.cfg.file_diagnostics.iter().map(|f| f.diagnostic.clone()).collect();
        d.extend(self.cfg.validate());
        self.diagnostics = d;
        self.comments = self.cfg.comments();
    }

    pub fn info(&mut self, text: impl Into<String>) {
        self.message = Some(Message { text: text.into(), error: false });
    }

    pub fn error(&mut self, text: impl Into<String>) {
        self.message = Some(Message { text: text.into(), error: true });
    }

    pub fn tree(&self) -> Vec<TreeNode> {
        self.cfg.tree(&self.file_name(), &self.expanded)
    }

    /// Index of the selected node in the visible tree.
    pub fn tree_cursor(&self, tree: &[TreeNode]) -> usize {
        // The selection may have vanished (undo); fall back to its nearest visible ancestor.
        let mut p = self.selected.clone();
        loop {
            if let Some(i) = tree.iter().position(|n| n.path == p) {
                return i;
            }
            if p.0.pop().is_none() {
                return 0;
            }
        }
    }

    pub fn node(&self) -> TreeNode {
        let tree = self.tree();
        let i = self.tree_cursor(&tree);
        tree.into_iter().nth(i).expect("tree has a root")
    }

    pub fn rows(&self) -> Vec<Row> {
        self.cfg.rows(&self.node())
    }

    pub fn current_row(&self) -> Option<Row> {
        self.rows().into_iter().nth(self.row)
    }

    /// Worst finding at or below `path`.
    pub fn worst(&self, path: &Path) -> Option<Severity> {
        self.diagnostics.iter().filter(|d| d.path.starts_with(path)).map(|d| d.severity).max()
    }

    /// Findings exactly at `path`.
    pub fn findings(&self, path: &Path) -> Vec<&Diagnostic> {
        self.diagnostics.iter().filter(|d| &d.path == path).collect()
    }

    pub fn counts(&self) -> (usize, usize) {
        let n = |s| self.diagnostics.iter().filter(|d| d.severity == s).count();
        (n(Severity::Error), n(Severity::Warning))
    }

    /// Whether the form row is used, given its `when` condition.
    pub fn row_active(&self, row: &Row) -> bool {
        active(&self.cfg, row)
    }

    pub fn value(&self, path: &Path) -> Option<&Value> {
        value_at(self.cfg.doc.as_table(), path)
    }

    // ── Changes ──────────────────────────────────────────────────────────

    /// Runs a change on the document; on failure nothing changes.
    fn change(&mut self, f: impl FnOnce(&mut DocumentMut) -> Result<(), String>) -> bool {
        let before = self.cfg.doc.clone();
        match f(&mut self.cfg.doc) {
            Ok(()) => {
                if self.cfg.doc.to_string() != before.to_string() {
                    self.undo.push(before);
                    self.redo.clear();
                }
                self.refresh();
                true
            }
            Err(e) => {
                self.cfg.doc = before;
                self.error(e);
                false
            }
        }
    }

    pub fn undo(&mut self) {
        match self.undo.pop() {
            Some(doc) => {
                self.redo.push(std::mem::replace(&mut self.cfg.doc, doc));
                self.refresh();
                self.info("undone");
            }
            None => self.info("nothing to undo"),
        }
    }

    pub fn redo(&mut self) {
        match self.redo.pop() {
            Some(doc) => {
                self.undo.push(std::mem::replace(&mut self.cfg.doc, doc));
                self.refresh();
                self.info("redone");
            }
            None => self.info("nothing to redo"),
        }
    }

    pub fn save(&mut self, force: bool) {
        // Diagnostics from the file are about the old content and are dropped on save: they don't block it.
        let errors = self.cfg.validate().iter().filter(|d| d.severity == Severity::Error).count();
        if errors > 0 && !force {
            self.mode = Mode::Confirm(Confirm::SaveWithErrors);
            return;
        }
        let text = self.cfg.to_text();
        match std::fs::write(&self.path, &text) {
            Ok(()) => {
                self.saved = text.clone();
                self.cfg.original = text;
                // Written by another program for the old content (§6).
                self.cfg.file_diagnostics.clear();
                self.refresh();
                self.info(format!("saved {}", self.file_name()));
            }
            Err(e) => self.error(format!("cannot save: {e}")),
        }
    }

    /// Quits; asks first when there are unsaved changes. Never writes the file by itself.
    pub fn quit_key(&mut self) {
        if self.modified() {
            self.mode = Mode::Confirm(Confirm::Unsaved(Then::Quit));
        } else {
            self.quit = true;
        }
    }

    // ── Navigation ───────────────────────────────────────────────────────

    /// Selects the node at `path`, expanding its ancestors.
    pub fn select(&mut self, path: Path) {
        let mut p = path.clone();
        while p.0.pop().is_some() {
            if !p.is_root() {
                self.expanded.insert(p.clone());
            }
        }
        self.selected = path;
        self.row = 0;
        self.form_scroll = 0;
    }

    fn move_tree(&mut self, delta: isize) {
        let tree = self.tree();
        let i = self.tree_cursor(&tree) as isize + delta;
        let i = i.clamp(0, tree.len() as isize - 1) as usize;
        if tree[i].path != self.selected {
            self.selected = tree[i].path.clone();
            self.row = 0;
            self.form_scroll = 0;
        }
    }

    fn move_row(&mut self, delta: isize) {
        let n = self.rows().len();
        if n == 0 {
            return;
        }
        self.row = (self.row as isize + delta).clamp(0, n as isize - 1) as usize;
    }

    fn tree_left(&mut self) {
        let node = self.node();
        if self.expanded.contains(&node.path) {
            self.expanded.remove(&node.path);
        } else if !node.path.is_root() {
            let mut p = node.path.clone();
            p.0.pop();
            // A list element's parent is the list: one more step for `[n]` paths is not needed,
            // the list itself is a node.
            self.selected = p;
            self.row = 0;
        }
    }

    pub fn toggle(&mut self, path: &Path) {
        if !self.expanded.remove(path) {
            self.expanded.insert(path.clone());
        }
    }

    // ── Form actions ─────────────────────────────────────────────────────

    fn readonly(row: &Row) -> bool {
        row.field.as_ref().is_some_and(|f| f.readonly)
    }

    /// Enter on a form row.
    pub fn activate(&mut self) {
        let Some(row) = self.current_row() else { return };
        match row.kind {
            RowKind::Child => self.select(row.path),
            _ if Self::readonly(&row) => self.info(format!("{} is read-only", row.label)),
            RowKind::List | RowKind::Add => self.start_edit(None),
            RowKind::Value | RowKind::Element(_) => {
                let options = row.ty.as_ref().and_then(|t| choices(t, self.cfg.doc.as_table()));
                match options {
                    Some(o) if is_bool(row.ty.as_ref()) && o.len() == 2 => self.toggle_bool(&row),
                    Some(options) if !options.is_empty() => {
                        let current = self.value(&row.path).map(crate::config::validate::value_text).unwrap_or_default();
                        let cursor = options.iter().position(|(v, _)| *v == current).unwrap_or(0);
                        self.mode = Mode::Choose { options, cursor, row: self.row, purpose: ChooseFor::Value };
                    }
                    Some(_) => self.error("nothing to choose from: the referenced map is empty"),
                    None => self.start_edit(None),
                }
            }
        }
    }

    fn toggle_bool(&mut self, row: &Row) {
        let current = match self.value(&row.path) {
            Some(Value::Boolean(b)) => *b.value(),
            Some(_) => false,
            None => row.field.as_ref().and_then(|f| f.default.as_ref()).and_then(Value::as_bool).unwrap_or(false),
        };
        let path = row.path.clone();
        self.change(|d| edit::set(d, &path, Value::from(!current)));
    }

    /// Starts editing the current row, optionally replacing its text with a typed character.
    pub fn start_edit(&mut self, first: Option<char>) {
        let Some(mut row) = self.current_row() else { return };
        if row.kind == RowKind::Child {
            return;
        }
        if Self::readonly(&row) {
            return self.info(format!("{} is read-only", row.label));
        }
        // On a list heading, typing adds an element.
        if row.kind == RowKind::List {
            let rows = self.rows();
            if let Some(i) = rows.iter().skip(self.row).position(|r| r.kind == RowKind::Add) {
                self.row += i;
                row = rows[self.row].clone();
            }
        }
        if first.is_some() && row.ty.as_ref().and_then(|t| choices(t, self.cfg.doc.as_table())).is_some() {
            return self.activate();
        }
        let buf = match (first, row.kind) {
            (Some(c), _) => c.to_string(),
            (None, RowKind::Add) => String::new(),
            // A missing field starts from its default.
            (None, _) => match self.value(&row.path) {
                Some(v) => edit_text(v),
                None => row.field.as_ref().and_then(|f| f.default.as_ref()).filter(|_| row.kind == RowKind::Value).map(edit_text).unwrap_or_default(),
            },
        };
        let multiline = matches!(&row.ty, Some(FieldType::Scalar { ty, .. }) if ty.multiline)
            || (row.field.is_none() && row.kind == RowKind::Value && self.value(&row.path).is_some_and(edit::is_multiline));
        let cursor = buf.chars().count();
        self.mode = Mode::Edit { buf, cursor, multiline, row: self.row };
    }

    fn commit_edit(&mut self) {
        let Mode::Edit { buf, row, .. } = self.mode.clone() else { return };
        let Some(r) = self.rows().into_iter().nth(row) else {
            self.mode = Mode::Normal;
            return;
        };
        let old = match (r.kind, self.value(&r.path)) {
            // A new element takes the kind of the last one.
            (RowKind::Add, Some(Value::Array(a))) => a.iter().last().cloned(),
            (RowKind::Add, _) => None,
            (_, v) => v.cloned(),
        };
        match parse_input(&buf, r.ty.as_ref(), old.as_ref()) {
            Ok(v) => {
                let path = r.path.clone();
                let ok = if r.kind == RowKind::Add {
                    self.change(|d| edit::push(d, &path, v))
                } else {
                    self.change(|d| edit::set(d, &path, v))
                };
                if ok {
                    self.mode = Mode::Normal;
                    if r.kind == RowKind::Add {
                        // Stay on "+ add", which moved down by the new element.
                        self.row += 1;
                    }
                    self.report_row(&path);
                }
            }
            Err(e) => self.error(e),
        }
    }

    /// After a change: the first problem of the changed value, if any.
    fn report_row(&mut self, path: &Path) {
        if let Some(d) = self.findings(path).first() {
            let text = d.message.clone();
            if d.severity == Severity::Error { self.error(text) } else { self.info(text) }
        }
    }

    fn commit_choice(&mut self) {
        let Mode::Choose { options, cursor, row, purpose } = self.mode.clone() else { return };
        self.mode = Mode::Normal;
        let Some((choice, _)) = options.get(cursor) else { return };
        match purpose {
            ChooseFor::Value => {}
            ChooseFor::NewKey(map) => return self.create_entry(&map, choice),
            ChooseFor::NewKind { parent, key } => {
                if let Some(kind) = edit::Kind::from_label(choice) {
                    self.create_generic(&parent, &key, kind);
                }
                return;
            }
            ChooseFor::Retype(path) => {
                if let Some(kind) = edit::Kind::from_label(choice) {
                    self.retype(&path, kind);
                }
                return;
            }
        }
        let (Some(r), Some((value, _))) = (self.rows().into_iter().nth(row), options.get(cursor)) else { return };
        let old = self.value(&r.path).cloned();
        match parse_input(value, r.ty.as_ref(), old.as_ref()) {
            Ok(v) => {
                let path = r.path.clone();
                self.change(|d| edit::set(d, &path, v));
            }
            Err(e) => self.error(e),
        }
    }

    /// Del / F4: back to the default (the key is removed), or remove a list element.
    pub fn reset(&mut self) {
        let Some(row) = self.current_row() else { return };
        if Self::readonly(&row) {
            return self.info(format!("{} is read-only", row.label));
        }
        match row.kind {
            RowKind::Element(_) => {
                let path = row.path.clone();
                if self.change(|d| if edit::remove(d, &path) { Ok(()) } else { Err("nothing to remove".into()) }) {
                    self.info("element removed");
                    self.move_row(0);
                }
            }
            RowKind::Value | RowKind::List => {
                if self.value(&row.path).is_none() {
                    return self.info(format!("{} is not set", row.label));
                }
                if row.field.as_ref().is_some_and(|f| f.required()) {
                    return self.error(format!("{} is required and has no default", row.label));
                }
                let path = row.path.clone();
                if self.change(|d| if edit::remove(d, &path) { Ok(()) } else { Err("nothing to remove".into()) }) {
                    let text = match row.field.as_ref().and_then(|f| f.default.as_ref()) {
                        Some(d) => format!("{}: back to the default {}", row.label, edit::display(d)),
                        None => format!("{} removed", row.label),
                    };
                    self.info(text);
                    self.move_row(0);
                }
            }
            RowKind::Add => {}
            RowKind::Child if self.delete_target().is_ok() => self.ask_delete(),
            RowKind::Child => self.info("tables are removed with their entry (F8)"),
        }
    }

    // ── Keys ─────────────────────────────────────────────────────────────

    pub fn on_key(&mut self, key: KeyEvent) {
        if self.show_help {
            self.show_help = false;
            return;
        }
        if let Some(d) = &mut self.file_dialog {
            let action = d.key(key);
            return self.dialog_action(action);
        }
        match &self.mode {
            Mode::Normal => self.key_normal(key),
            Mode::Edit { .. } => self.key_edit(key),
            Mode::NewKey { .. } => self.key_new_key(key),
            Mode::Comment { .. } => self.key_comment(key),
            Mode::Choose { .. } => self.key_choose(key),
            Mode::Confirm(c) => {
                let c = c.clone();
                self.key_confirm(c, key)
            }
        }
    }

    fn key_normal(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        self.message = None;
        match key.code {
            KeyCode::Char('q') if ctrl => return self.quit_key(),
            KeyCode::Char('s') if ctrl => return self.save(false),
            KeyCode::Char('z') if ctrl => return self.undo(),
            KeyCode::Char('y') if ctrl => return self.redo(),
            KeyCode::Char('e') if ctrl => return self.open_dialog(),
            KeyCode::Char('c') if ctrl => return self.copy(),
            KeyCode::Char('v') if ctrl => {
                return match self.clipboard.clone() {
                    Some(text) => self.paste_value(&text),
                    None => self.info("nothing copied yet (Ctrl+C copies the value under the cursor)"),
                };
            }
            KeyCode::Insert => return self.new_entry(),
            KeyCode::F(n) => return self.fkey(n),
            KeyCode::Tab | KeyCode::BackTab => {
                self.focus = if self.focus == Focus::Tree { Focus::Form } else { Focus::Tree };
                return;
            }
            _ => {}
        }
        match self.focus {
            Focus::Tree => match key.code {
                KeyCode::Up => self.move_tree(-1),
                KeyCode::Down => self.move_tree(1),
                KeyCode::PageUp => self.move_tree(-(self.page as isize)),
                KeyCode::PageDown => self.move_tree(self.page as isize),
                KeyCode::Home => self.move_tree(-(1 << 20)),
                KeyCode::End => self.move_tree(1 << 20),
                // → always goes to the form, like ← comes back from it.
                KeyCode::Right | KeyCode::Enter => self.focus = Focus::Form,
                KeyCode::Left => self.tree_left(),
                KeyCode::Char('+') => {
                    let node = self.node();
                    if node.has_children && !node.path.is_root() {
                        self.expanded.insert(node.path);
                    }
                }
                KeyCode::Char('-') => {
                    let p = self.node().path;
                    self.expanded.remove(&p);
                }
                KeyCode::Char(' ') => {
                    let p = self.node().path;
                    if !p.is_root() {
                        self.toggle(&p);
                    }
                }
                _ => {}
            },
            Focus::Form => match key.code {
                KeyCode::Up => self.move_row(-1),
                KeyCode::Down => self.move_row(1),
                KeyCode::PageUp => self.move_row(-(self.page as isize)),
                KeyCode::PageDown => self.move_row(self.page as isize),
                KeyCode::Home => self.move_row(-(1 << 20)),
                KeyCode::End => self.move_row(1 << 20),
                KeyCode::Left | KeyCode::Esc => self.focus = Focus::Tree,
                KeyCode::Enter | KeyCode::Right => self.activate(),
                KeyCode::Char(' ') if is_bool(self.current_row().and_then(|r| r.ty).as_ref()) => self.activate(),
                KeyCode::Delete | KeyCode::Backspace => self.reset(),
                KeyCode::Char(c) if !ctrl => self.start_edit(Some(c)),
                _ => {}
            },
        }
    }

    pub fn fkey(&mut self, n: u8) {
        match n {
            1 => self.show_help = true,
            2 => self.save(false),
            3 => {
                self.reveal = !self.reveal;
                self.info(if self.reveal { "secret values shown" } else { "secret values hidden" });
            }
            4 => {
                self.focus = Focus::Form;
                self.reset();
            }
            5 => self.open_dialog(),
            6 => self.start_comment(),
            7 => self.new_entry(),
            8 => match self.current_row() {
                // Without schema, F8 on a value removes the key, like Del.
                Some(r) if self.delete_target().is_err() && self.focus == Focus::Form && r.field.is_none() && r.kind != RowKind::Child => self.reset(),
                _ => self.ask_delete(),
            },
            9 => self.ask_retype(),
            10 => self.quit_key(),
            _ => {}
        }
    }

    fn key_edit(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let Mode::Edit { buf, cursor, multiline, .. } = &mut self.mode else { return };
        match key.code {
            KeyCode::Esc => {
                self.mode = Mode::Normal;
                self.message = None;
            }
            KeyCode::Enter if !*multiline => self.commit_edit(),
            KeyCode::Tab => self.commit_edit(),
            KeyCode::Char('s') if ctrl => self.commit_edit(),
            KeyCode::Char('c') if ctrl => {
                let text = buf.clone();
                self.put_clipboard(text);
            }
            KeyCode::Char('v') if ctrl => {
                if let Some(text) = self.clipboard.clone() {
                    self.on_paste(&text);
                }
            }
            KeyCode::Enter => insert(buf, cursor, "\n"),
            _ => edit_line(buf, cursor, key),
        }
    }

    // ── Copy and paste ───────────────────────────────────────────────────

    fn put_clipboard(&mut self, text: String) {
        let n = text.chars().count();
        self.clipboard = Some(text.clone());
        self.to_system_clipboard = Some(text);
        self.info(format!("copied {n} character{}", if n == 1 { "" } else { "s" }));
    }

    /// Ctrl+C: the value under the cursor (in the tree: the key of the node).
    pub fn copy(&mut self) {
        let text = match (self.focus, self.current_row()) {
            (Focus::Form, Some(r)) => match (r.kind, self.value(&r.path)) {
                (RowKind::Value | RowKind::Element(_), Some(v)) => edit_text(v),
                (RowKind::Value, None) => match r.field.as_ref().and_then(|f| f.default.as_ref()) {
                    Some(d) => edit_text(d),
                    None => return self.info(format!("{} is not set", r.label)),
                },
                (RowKind::List, Some(v)) => edit::display(v),
                _ => return self.info("select a value to copy"),
            },
            _ => match self.node().path.0.last() {
                Some(crate::config::Segment::Key(k)) => k.clone(),
                _ => return self.info("select a value to copy"),
            },
        };
        self.put_clipboard(text);
    }

    /// Ctrl+V or a paste from the terminal outside editing: the text becomes
    /// the value under the cursor, as if typed and confirmed.
    pub fn paste_value(&mut self, text: &str) {
        let Some(row) = self.current_row().filter(|_| self.focus == Focus::Form) else {
            return self.info("select a value in the form to paste into");
        };
        if matches!(row.kind, RowKind::Child) {
            return self.info("paste into a value, not a table");
        }
        if Self::readonly(&row) {
            return self.info(format!("{} is read-only", row.label));
        }
        self.start_edit(None);
        if let Mode::Edit { buf, cursor, multiline, .. } = &mut self.mode {
            *buf = if *multiline { text.replace("\r\n", "\n") } else { text.trim_end_matches(['\r', '\n']).replace(['\r', '\n'], " ") };
            *cursor = buf.chars().count();
            self.commit_edit();
        }
    }

    fn key_comment(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let Mode::Comment { buf, cursor, target } = &mut self.mode else { return };
        match key.code {
            KeyCode::Esc => {
                self.mode = Mode::Normal;
                self.info("comment unchanged");
            }
            KeyCode::Tab => {
                let (target, text) = (target.clone(), buf.clone());
                self.commit_comment(&target, &text);
            }
            KeyCode::Char('s') if ctrl => {
                let (target, text) = (target.clone(), buf.clone());
                self.commit_comment(&target, &text);
            }
            KeyCode::Enter => insert(buf, cursor, "\n"),
            _ => edit_line(buf, cursor, key),
        }
    }

    // ── Comments in the file ─────────────────────────────────────────────

    /// The table or key whose comment F6 edits, or why there is none.
    pub fn comment_target(&self) -> Result<Path, String> {
        let path = match (self.focus, self.current_row()) {
            (Focus::Form, Some(r)) => match r.kind {
                // An element has no comment of its own: the list's.
                RowKind::Element(_) => {
                    let mut p = r.path;
                    p.0.pop();
                    p
                }
                _ => r.path,
            },
            _ => self.node().path,
        };
        crate::config::comments::can_comment(&self.cfg.doc, &path).map(|_| path)
    }

    /// F6: edits the comment above the selected table or key.
    pub fn start_comment(&mut self) {
        match self.comment_target() {
            Ok(target) => {
                let buf = self.comments.get(&target).cloned().unwrap_or_default();
                let cursor = buf.chars().count();
                self.mode = Mode::Comment { buf, cursor, target };
            }
            Err(e) => self.info(e),
        }
    }

    fn commit_comment(&mut self, target: &Path, text: &str) {
        let path = target.clone();
        if self.change(|d| crate::config::comments::set_comment(d, &path, text)) {
            self.mode = Mode::Normal;
            self.info(if text.trim().is_empty() { "comment removed" } else { "comment saved" });
        }
    }

    fn key_new_key(&mut self, key: KeyEvent) {
        let Mode::NewKey { buf, cursor, map, generic } = &mut self.mode else { return };
        match key.code {
            KeyCode::Esc => {
                self.mode = Mode::Normal;
                self.info("cancelled");
            }
            KeyCode::Enter | KeyCode::Tab => {
                let (map, key, generic) = (map.clone(), buf.trim().to_string(), *generic);
                if generic {
                    self.ask_kind(&map, &key);
                } else {
                    self.create_entry(&map, &key);
                }
            }
            _ => edit_line(buf, cursor, key),
        }
    }

    fn key_choose(&mut self, key: KeyEvent) {
        let Mode::Choose { options, cursor, .. } = &mut self.mode else { return };
        let n = options.len();
        match key.code {
            KeyCode::Esc => self.mode = Mode::Normal,
            KeyCode::Up => *cursor = cursor.saturating_sub(1),
            KeyCode::Down => *cursor = (*cursor + 1).min(n.saturating_sub(1)),
            KeyCode::Home => *cursor = 0,
            KeyCode::End => *cursor = n.saturating_sub(1),
            KeyCode::Enter | KeyCode::Tab => self.commit_choice(),
            // Jump to the first option starting with the typed letter.
            KeyCode::Char(c) => {
                if let Some(i) = options.iter().position(|(v, _)| v.starts_with(c)) {
                    *cursor = i;
                }
            }
            _ => {}
        }
    }

    fn key_confirm(&mut self, c: Confirm, key: KeyEvent) {
        let answer = match key.code {
            KeyCode::Char(ch) => ch.to_ascii_lowercase(),
            KeyCode::Enter => 'y',
            KeyCode::Esc => 'n',
            _ => return,
        };
        self.mode = Mode::Normal;
        match (c, answer) {
            (Confirm::SaveWithErrors, 'y') => self.save(true),
            (Confirm::Unsaved(then), 'd') => self.then(then),
            (Confirm::Unsaved(then), 's') => {
                self.save(false);
                // With errors, saving asks again; the action waits for the next try.
                if !self.modified() {
                    self.then(then);
                }
            }
            (Confirm::Delete(path), 'y') => self.delete_entry(&path),
            _ => self.info("cancelled"),
        }
    }

    fn then(&mut self, then: Then) {
        match then {
            Then::Quit => self.quit = true,
            Then::Open => self.show_dialog(),
        }
    }

    // ── Opening another file ─────────────────────────────────────────────

    /// The file dialog; unsaved changes are settled first.
    pub fn open_dialog(&mut self) {
        if self.modified() {
            self.mode = Mode::Confirm(Confirm::Unsaved(Then::Open));
        } else {
            self.show_dialog();
        }
    }

    fn show_dialog(&mut self) {
        let dir = match self.path.parent() {
            Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
            _ => PathBuf::from("."),
        };
        let dir = std::fs::canonicalize(&dir).unwrap_or(dir);
        self.file_dialog = Some(FileDialog::new(Purpose::Open, dir, is_toml).with_tag(format_tag));
        self.message = None;
    }

    fn dialog_action(&mut self, action: Action) {
        match action {
            Action::None => {}
            Action::Close => self.file_dialog = None,
            Action::Chosen(path) => {
                self.file_dialog = None;
                self.load(&path);
            }
        }
    }

    /// Replaces the open file; on failure the current one stays.
    pub fn load(&mut self, path: &std::path::Path) {
        match ConfigFile::load(path) {
            Ok(cfg) => {
                let (theme, light, clip) = (self.theme, self.detected_light, self.clipboard.take());
                *self = ConfApp::new(path.to_path_buf(), cfg);
                self.theme = theme;
                self.detected_light = light;
                self.clipboard = clip;
                if self.message.is_none() {
                    self.info(format!("opened {}", path.display()));
                }
            }
            Err(e) => self.error(format!("cannot open {}: {e}", path.display())),
        }
    }

    // ── Map entries ──────────────────────────────────────────────────────

    /// The map `node` is, or is an entry of, with the schema field of the map.
    fn map_of(node: &TreeNode) -> Option<(Path, Field)> {
        let f = node.field.clone()?;
        if !matches!(f.ty, FieldType::Map { .. }) {
            return None;
        }
        if matches!(node.shape, Shape::Map { .. }) && f.name == node.label {
            Some((node.path.clone(), f))
        } else {
            let mut p = node.path.clone();
            p.0.pop();
            Some((p, f))
        }
    }

    fn entries(&self, map: &Path) -> usize {
        edit::get(self.cfg.doc.as_table(), map)
            .and_then(crate::config::validate::Node::table_like)
            .map_or(0, |t| t.iter().filter(|(_, i)| !i.is_none()).count())
    }

    /// Where F7 adds something, or why nothing can be added here.
    pub fn add_target(&self) -> Result<AddTarget, String> {
        let node = self.node();
        // Without schema: keys and tables in any table, elements in a `[[list]]`.
        if node.shape == Shape::Generic {
            return match edit::get(self.cfg.doc.as_table(), &node.path) {
                Some(crate::config::validate::Node::Tables(_)) => Ok(AddTarget::Tables(node.path)),
                Some(n) if n.table_like().is_some() => Ok(AddTarget::Table(node.path)),
                _ => Err("a list of inline tables is edited as text".into()),
            };
        }
        let from_row = match (self.focus, self.current_row()) {
            (Focus::Form, Some(r)) if r.kind == RowKind::Child && matches!(r.ty, Some(FieldType::Map { .. })) => {
                r.field.map(|f| (r.path, f))
            }
            _ => None,
        };
        let (map, f) = from_row
            .or_else(|| Self::map_of(&node))
            .ok_or("select a map (or one of its entries) to add an entry")?;
        if f.readonly {
            return Err(format!("{} is read-only: no new entries", f.name));
        }
        if let FieldType::Map { length: Some(b), .. } = &f.ty
            && !b.contains((self.entries(&map) + 1) as f64)
        {
            return Err(format!("{map} allows {b} entries"));
        }
        Ok(AddTarget::Map(map, Box::new(f)))
    }

    /// What F8 deletes (a map entry, or a table without schema), or why nothing.
    pub fn delete_target(&self) -> Result<Path, String> {
        let node = self.node();
        let generic_row = match (self.focus, self.current_row()) {
            (Focus::Form, Some(r)) if r.field.is_none() && r.kind == RowKind::Child => Some(r.path),
            _ => None,
        };
        if let Some(p) = generic_row {
            return Ok(p);
        }
        if node.shape == Shape::Generic && !node.path.is_root() && self.focus == Focus::Tree {
            return Ok(node.path);
        }
        let (map, f) = Self::map_of(&node).ok_or("select an entry of a map or a table to delete it")?;
        let entry = if map == node.path {
            // The map's own form: the entry under the cursor.
            match (self.focus, self.current_row()) {
                (Focus::Form, Some(r)) if r.path.0.len() == map.0.len() + 1 => r.path,
                _ => return Err("select the entry to delete".into()),
            }
        } else {
            node.path.clone()
        };
        if f.readonly {
            return Err(format!("{} is read-only: entries cannot be deleted", f.name));
        }
        if let FieldType::Map { length: Some(b), .. } = &f.ty
            && !b.contains(self.entries(&map).saturating_sub(1) as f64)
        {
            return Err(format!("{map} needs {b} entries"));
        }
        Ok(entry)
    }

    /// F7 / Insert: asks for the key of a new entry or key (a choice for
    /// `ref` and `enum` keys); in a `[[list]]` adds an element.
    pub fn new_entry(&mut self) {
        match self.add_target() {
            Err(e) => self.info(e),
            Ok(AddTarget::Table(p)) => self.mode = Mode::NewKey { buf: String::new(), cursor: 0, map: p, generic: true },
            Ok(AddTarget::Tables(p)) => {
                let path = p.clone();
                if self.change(|d| edit::push_table(d, &path)) {
                    let n = edit::get(self.cfg.doc.as_table(), &p).and_then(crate::config::validate::Node::elements).map_or(1, |e| e.len());
                    self.select(p.index(n - 1));
                    self.focus = Focus::Form;
                    self.info(format!("new element [{}] of {p}", n - 1));
                }
            }
            Ok(AddTarget::Map(map, f)) => {
                let FieldType::Map { key, .. } = &f.ty else { return };
                let used = |k: &str| edit::get(self.cfg.doc.as_table(), &map).and_then(crate::config::validate::Node::table_like).is_some_and(|t| t.contains_key(k));
                if let Some(options) = choices(key, self.cfg.doc.as_table()) {
                    let free: Vec<(String, String)> = options.into_iter().filter(|(k, _)| !used(k)).collect();
                    if free.is_empty() {
                        return self.error(format!("no free key left for {map}"));
                    }
                    self.mode = Mode::Choose { options: free, cursor: 0, row: self.row, purpose: ChooseFor::NewKey(map) };
                } else {
                    self.mode = Mode::NewKey { buf: String::new(), cursor: 0, map, generic: false };
                }
            }
        }
    }

    /// Label of the key of new entries in `map`, e.g. "Name".
    pub fn key_label(&self, map: &Path) -> String {
        match (self.cfg.schema.as_ref(), self.add_target()) {
            (Some(s), Ok(AddTarget::Map(m, f))) if &m == map => match &f.ty {
                FieldType::Map { key, .. } => s.key_label(key),
                _ => "Key".into(),
            },
            _ => "Key".into(),
        }
    }

    fn create_entry(&mut self, map: &Path, key: &str) {
        let f = match self.add_target() {
            Ok(AddTarget::Map(m, f)) if &m == map => *f,
            // The cursor may have moved onto the new key's map in the meantime.
            _ => match Self::map_of(&self.node()).filter(|(m, _)| m == map) {
                Some((_, f)) => f,
                None => return self.error("cannot add here"),
            },
        };
        let FieldType::Map { value, key: key_ty, .. } = &f.ty else { return };
        let schema = self.cfg.schema.as_ref().expect("maps come from a schema");
        if let Some(problem) = edit::key_problem(schema, self.cfg.doc.as_table(), map, key_ty, key) {
            // Stays in the prompt so the name can be corrected.
            return self.error(problem);
        }
        let entry = edit::new_entry(schema, value);
        let struct_like = crate::config::view::shape_of(value).is_some();
        let label = match value.as_ref() {
            FieldType::Struct(s) => schema.struct_def(s).map_or(s.clone(), |st| st.label().to_string()),
            _ => "entry".into(),
        };
        let (map, key) = (map.clone(), key.to_string());
        if self.change(|d| edit::add_entry(d, &map, &key, entry)) {
            self.mode = Mode::Normal;
            self.expanded.insert(map.clone());
            if struct_like {
                self.select(map.key(&key));
                let missing = self.diagnostics.iter().filter(|d| d.path.starts_with(&map.key(&key)) && d.severity == Severity::Error).count();
                self.info(match missing {
                    0 => format!("new {label} '{key}'"),
                    n => format!("new {label} '{key}': fill in {n} required field{}", if n == 1 { "" } else { "s" }),
                });
            } else {
                self.select(map.clone());
                self.row = self.rows().iter().position(|r| r.path == map.key(&key)).unwrap_or(0);
                self.info(format!("new {label} '{key}'"));
            }
            self.focus = Focus::Form;
        }
    }

    /// Without schema: after the name, the kind of the new key.
    fn ask_kind(&mut self, parent: &Path, key: &str) {
        let exists = edit::get(self.cfg.doc.as_table(), parent).and_then(crate::config::validate::Node::table_like).is_some_and(|t| t.contains_key(key));
        if key.is_empty() {
            return self.error("the name must not be empty");
        }
        if exists {
            return self.error(format!("'{key}' exists already"));
        }
        let options = edit::Kind::ALL.iter().map(|k| (k.label().to_string(), String::new())).collect();
        self.mode = Mode::Choose { options, cursor: 0, row: self.row, purpose: ChooseFor::NewKind { parent: parent.clone(), key: key.to_string() } };
    }

    /// Without schema: creates `key` of the chosen kind and starts editing it.
    fn create_generic(&mut self, parent: &Path, key: &str, kind: edit::Kind) {
        use edit::Kind;
        let path = parent.key(key);
        let (p, k) = (parent.clone(), key.to_string());
        let ok = match kind {
            Kind::Table => self.change(|d| edit::add_entry(d, &p, &k, toml_edit::Item::Table(toml_edit::Table::new()))),
            Kind::Tables => {
                let mut list = toml_edit::ArrayOfTables::new();
                list.push(toml_edit::Table::new());
                self.change(|d| edit::add_entry(d, &p, &k, toml_edit::Item::ArrayOfTables(list)))
            }
            value => {
                let target = path.clone();
                self.change(|d| edit::set(d, &target, edit::empty_value(value)))
            }
        };
        if !ok {
            return;
        }
        self.info(format!("new {} '{key}'", kind.label()));
        match kind {
            Kind::Table => {
                self.select(path);
                self.focus = Focus::Form;
            }
            Kind::Tables => {
                self.select(path.index(0));
                self.focus = Focus::Form;
            }
            _ => {
                self.select(parent.clone());
                self.focus = Focus::Form;
                let rows = self.rows();
                let at = |pred: &dyn Fn(&Row) -> bool| rows.iter().position(pred);
                self.row = match kind {
                    // A new list: straight to "+ add".
                    Kind::List => at(&|r: &Row| r.path == path && r.kind == RowKind::Add),
                    _ => at(&|r: &Row| r.path == path),
                }
                .unwrap_or(0);
                if kind != Kind::Bool {
                    self.start_edit(None);
                }
            }
        }
    }

    /// The value F9 gives another kind (only without schema), or why none.
    pub fn retype_target(&self) -> Result<(Path, edit::Kind), String> {
        let row = match (self.focus, self.current_row()) {
            (Focus::Form, Some(r)) => r,
            _ => return Err("select a value in the form to change its type".into()),
        };
        if let Some(f) = &row.field {
            return Err(format!("the schema sets the type of {}", f.name));
        }
        if !matches!(row.kind, RowKind::Value | RowKind::Element(_) | RowKind::List) {
            return Err("select a value to change its type".into());
        }
        let kind = self.value(&row.path).and_then(edit::kind_of).ok_or("this value keeps its type")?;
        Ok((row.path, kind))
    }

    /// F9: choose the new kind of the value under the cursor.
    pub fn ask_retype(&mut self) {
        match self.retype_target() {
            Ok((path, kind)) => {
                let kinds: Vec<edit::Kind> = edit::Kind::ALL.into_iter().filter(|k| k.is_value()).collect();
                let cursor = kinds.iter().position(|k| *k == kind).unwrap_or(0);
                let options = kinds.iter().map(|k| (k.label().to_string(), String::new())).collect();
                self.mode = Mode::Choose { options, cursor, row: self.row, purpose: ChooseFor::Retype(path) };
            }
            Err(e) => self.info(e),
        }
    }

    fn retype(&mut self, path: &Path, kind: edit::Kind) {
        let Some(old) = self.value(path).cloned() else { return };
        if edit::kind_of(&old) == Some(kind) {
            return self.info(format!("already {}", kind.label()));
        }
        let (new, note) = edit::convert(&old, kind);
        let target = path.clone();
        if self.change(|d| edit::set(d, &target, new)) {
            match note {
                Some(n) => self.error(n),
                None => self.info(format!("now {}", kind.label())),
            }
        }
    }

    /// F8: asks before deleting the selected map entry.
    pub fn ask_delete(&mut self) {
        match self.delete_target() {
            Ok(entry) => self.mode = Mode::Confirm(Confirm::Delete(entry)),
            Err(e) => self.info(e),
        }
    }

    /// Places that will point nowhere once `entry` is deleted.
    pub fn references_to(&self, entry: &Path) -> Vec<Path> {
        let mut map = entry.clone();
        let Some(crate::config::Segment::Key(key)) = map.0.pop() else { return Vec::new() };
        self.cfg.references(&map, &key)
    }

    fn delete_entry(&mut self, entry: &Path) {
        let refs = self.references_to(entry).len();
        let path = entry.clone();
        if self.change(|d| edit::remove_entry(d, &path)) {
            let mut map = entry.clone();
            map.0.pop();
            let was_selected = self.selected.starts_with(entry);
            if was_selected {
                self.select(map);
                self.focus = Focus::Tree;
            } else {
                self.move_row(0);
            }
            self.info(match refs {
                0 => format!("deleted {entry}"),
                n => format!("deleted {entry} – {n} reference{} to it now point{} nowhere", if n == 1 { "" } else { "s" }, if n == 1 { "s" } else { "" }),
            });
        }
    }

    pub fn on_paste(&mut self, text: &str) {
        match &mut self.mode {
            Mode::Edit { buf, cursor, multiline: true, .. } | Mode::Comment { buf, cursor, .. } => {
                insert(buf, cursor, &text.replace("\r\n", "\n"))
            }
            Mode::Edit { buf, cursor, .. } | Mode::NewKey { buf, cursor, .. } => insert(buf, cursor, &text.replace(['\r', '\n'], " ")),
            Mode::Normal if self.file_dialog.is_none() => self.paste_value(text),
            _ => {}
        }
    }

    pub fn on_mouse(&mut self, m: MouseEvent) {
        let hit = self.hits.iter().rev().find(|(r, _)| r.contains((m.column, m.row).into())).map(|(_, h)| *h);
        if let Some(d) = &mut self.file_dialog {
            let action = match (m.kind, hit) {
                (MouseEventKind::Down(MouseButton::Left), Some(Hit::File(spot))) => {
                    let now = Instant::now();
                    let double = self.last_click.is_some_and(|(t, x, y)| now.duration_since(t) < DOUBLE_CLICK && (x, y) == (m.column, m.row));
                    self.last_click = Some((now, m.column, m.row));
                    d.click_spot(spot, double)
                }
                // A click on the bar is the key it shows.
                (MouseEventKind::Down(MouseButton::Left), Some(Hit::FKey(n))) => d.key(KeyEvent::new(KeyCode::F(n), KeyModifiers::NONE)),
                (MouseEventKind::ScrollDown, _) => {
                    d.scroll_by(3);
                    Action::None
                }
                (MouseEventKind::ScrollUp, _) => {
                    d.scroll_by(-3);
                    Action::None
                }
                _ => Action::None,
            };
            return self.dialog_action(action);
        }
        match m.kind {
            MouseEventKind::Down(MouseButton::Left) => match (hit, &self.mode) {
                (Some(Hit::Choice(i)), Mode::Choose { .. }) => {
                    if let Mode::Choose { cursor, .. } = &mut self.mode {
                        *cursor = i;
                    }
                    self.commit_choice();
                }
                (_, Mode::Choose { .. }) => self.mode = Mode::Normal,
                (_, Mode::Edit { .. } | Mode::NewKey { .. } | Mode::Comment { .. } | Mode::Confirm(_)) => {}
                (Some(Hit::Tree(i)), _) => {
                    let tree = self.tree();
                    if let Some(n) = tree.get(i) {
                        if n.path != self.selected {
                            self.select(n.path.clone());
                        }
                        self.focus = Focus::Tree;
                    }
                }
                (Some(Hit::Toggle(i)), _) => {
                    if let Some(n) = self.tree().get(i) {
                        let p = n.path.clone();
                        self.toggle(&p);
                    }
                }
                (Some(Hit::Row(i)), _) => {
                    let again = self.focus == Focus::Form && self.row == i;
                    self.row = i;
                    self.focus = Focus::Form;
                    // A second click on the selected row acts like Enter.
                    if again {
                        self.activate();
                    }
                }
                (Some(Hit::FKey(n)), _) => self.fkey(n),
                _ => {}
            },
            MouseEventKind::ScrollDown | MouseEventKind::ScrollUp if self.mode == Mode::Normal => {
                let d = if m.kind == MouseEventKind::ScrollDown { 1 } else { -1 };
                let over_tree = self.hits.iter().any(|(r, h)| matches!(h, Hit::Tree(_)) && r.y == m.row && r.contains((m.column, m.row).into()));
                if over_tree || (self.focus == Focus::Tree && hit.is_none()) {
                    self.move_tree(d);
                } else {
                    self.move_row(d);
                }
            }
            _ => {}
        }
    }
}

fn insert(buf: &mut String, cursor: &mut usize, text: &str) {
    let at = buf.char_indices().nth(*cursor).map_or(buf.len(), |(p, _)| p);
    buf.insert_str(at, text);
    *cursor += text.chars().count();
}

/// Cursor movement and typing in a one-line text field.
fn edit_line(buf: &mut String, cursor: &mut usize, key: KeyEvent) {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let len = buf.chars().count();
    let byte = |b: &String, i: usize| b.char_indices().nth(i).map_or(b.len(), |(p, _)| p);
    match key.code {
        KeyCode::Left => *cursor = cursor.saturating_sub(1),
        KeyCode::Right => *cursor = (*cursor + 1).min(len),
        KeyCode::Home => *cursor = 0,
        KeyCode::End => *cursor = len,
        KeyCode::Char('u') if ctrl => {
            buf.clear();
            *cursor = 0;
        }
        KeyCode::Backspace if *cursor > 0 => {
            let at = byte(buf, *cursor - 1);
            buf.remove(at);
            *cursor -= 1;
        }
        KeyCode::Delete if *cursor < len => {
            let at = byte(buf, *cursor);
            buf.remove(at);
        }
        KeyCode::Char(c) if !ctrl => insert(buf, cursor, &c.to_string()),
        _ => {}
    }
}

fn is_bool(ty: Option<&FieldType>) -> bool {
    matches!(ty, Some(FieldType::Scalar { ty, .. }) if ty.base == crate::types::Base::Bool)
}
