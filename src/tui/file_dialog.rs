//! File dialog (Midnight Commander style), shared by hardcalc and hardconf:
//! a list of folders and files to walk through, with quick search.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::widgets::{Block, Clear};
use ratatui::Frame;
use std::path::PathBuf;
use std::time::SystemTime;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// What the chosen file is used for.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Purpose {
    Open,
    Append,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub name: String,
    pub dir: bool,
    pub size: u64,
    pub modified: Option<SystemTime>,
    /// Short label from the program, e.g. the format of the file.
    pub tag: Option<&'static str>,
}

/// Result of a key or click in the dialog.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    None,
    Close,
    Chosen(PathBuf),
}

#[derive(Debug, Clone)]
pub struct FileDialog {
    pub purpose: Purpose,
    pub dir: PathBuf,
    /// All entries of `dir` ("..", folders, then files).
    entries: Vec<Entry>,
    /// Quick search: only entries whose name contains it.
    pub filter: String,
    /// Index into [`FileDialog::shown`].
    pub selected: usize,
    pub scroll: usize,
    /// Visible list rows (set while drawing).
    pub rows: usize,
    /// Also files the program cannot read.
    pub all_files: bool,
    /// Files the program can read; the others show with Ctrl+A.
    accept: fn(&str) -> bool,
    /// Optional label per readable file, shown next to its name.
    tag: Option<fn(&std::path::Path) -> Option<&'static str>>,
}

impl FileDialog {
    pub fn new(purpose: Purpose, dir: PathBuf, accept: fn(&str) -> bool) -> FileDialog {
        let mut d = FileDialog {
            purpose,
            dir,
            entries: Vec::new(),
            filter: String::new(),
            selected: 0,
            scroll: 0,
            rows: 10,
            all_files: false,
            accept,
            tag: None,
        };
        d.read();
        d
    }

    /// Labels readable files, e.g. with their format.
    pub fn with_tag(mut self, tag: fn(&std::path::Path) -> Option<&'static str>) -> FileDialog {
        self.tag = Some(tag);
        self.read();
        self
    }

    /// Whether the program can read the file.
    pub fn accepts(&self, name: &str) -> bool {
        (self.accept)(name)
    }

    fn read(&mut self) {
        let mut entries: Vec<Entry> = std::fs::read_dir(&self.dir)
            .map(|rd| {
                rd.filter_map(|e| e.ok())
                    .filter_map(|e| {
                        let name = e.file_name().to_string_lossy().into_owned();
                        let meta = e.metadata().ok()?;
                        // A .textbundle is a folder but opens like a file.
                        let dir = e.path().is_dir() && crate::bundle::Kind::from_path(&e.path()).is_none();
                        let readable = !dir && (self.accept)(&name);
                        let tag = self.tag.filter(|_| readable).and_then(|t| t(&e.path()));
                        (!name.starts_with('.') && (dir || self.all_files || readable))
                            .then(|| Entry { name, dir, size: meta.len(), modified: meta.modified().ok(), tag })
                    })
                    .collect()
            })
            .unwrap_or_default();
        entries.sort_by(|a, b| b.dir.cmp(&a.dir).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
        if self.dir.parent().is_some() {
            entries.insert(0, Entry { name: "..".into(), dir: true, size: 0, modified: None, tag: None });
        }
        self.entries = entries;
        self.selected = 0;
        self.scroll = 0;
    }

    /// Entries matching the quick search.
    pub fn shown(&self) -> Vec<&Entry> {
        let f = self.filter.to_lowercase();
        self.entries.iter().filter(|e| f.is_empty() || (e.name != ".." && e.name.to_lowercase().contains(&f))).collect()
    }

    pub fn current(&self) -> Option<Entry> {
        self.shown().get(self.selected).map(|e| (*e).clone())
    }

    pub fn select(&mut self, i: isize) {
        let n = self.shown().len();
        self.selected = i.clamp(0, n.saturating_sub(1) as isize) as usize;
        if self.selected < self.scroll {
            self.scroll = self.selected;
        } else if self.selected >= self.scroll + self.rows {
            self.scroll = self.selected + 1 - self.rows;
        }
    }

    fn enter_dir(&mut self, name: &str) {
        let previous = self.dir.file_name().map(|n| n.to_string_lossy().into_owned());
        if name == ".." {
            if let Some(parent) = self.dir.parent() {
                self.dir = parent.to_path_buf();
            }
        } else {
            self.dir = self.dir.join(name);
        }
        self.filter.clear();
        self.read();
        // Going up: the folder we came from stays selected.
        if name == ".."
            && let Some(prev) = previous
            && let Some(i) = self.shown().iter().position(|e| e.name == prev)
        {
            self.select(i as isize);
        }
    }

    fn up(&mut self) {
        if self.dir.parent().is_some() {
            self.enter_dir("..");
        }
    }

    /// Takes the selected file, or enters the selected folder.
    pub fn choose(&mut self) -> Action {
        let Some(entry) = self.current() else { return Action::None };
        if entry.dir {
            self.enter_dir(&entry.name);
            return Action::None;
        }
        Action::Chosen(self.dir.join(&entry.name))
    }

    pub fn key(&mut self, key: KeyEvent) -> Action {
        let page = self.rows.max(1) as isize;
        let sel = self.selected as isize;
        match key.code {
            KeyCode::Esc if !self.filter.is_empty() => {
                self.filter.clear();
                self.select(0);
            }
            KeyCode::Esc | KeyCode::F(10) => return Action::Close,
            KeyCode::Up => self.select(sel - 1),
            KeyCode::Down => self.select(sel + 1),
            KeyCode::PageUp => self.select(sel - page),
            KeyCode::PageDown => self.select(sel + page),
            KeyCode::Home => self.select(0),
            KeyCode::End => self.select(isize::MAX / 2),
            KeyCode::Left => self.up(),
            KeyCode::Right if self.current().is_some_and(|e| e.dir) => return self.choose(),
            KeyCode::Enter => return self.choose(),
            KeyCode::Backspace if !self.filter.is_empty() => {
                self.filter.pop();
                self.select(0);
            }
            KeyCode::Backspace => self.up(),
            // Ctrl+A: all files, not only the readable ones.
            KeyCode::Char('a') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.all_files = !self.all_files;
                self.read();
            }
            KeyCode::Char('~') if self.filter.is_empty() => {
                if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
                    self.dir = PathBuf::from(home);
                    self.read();
                }
            }
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.filter.push(c);
                self.select(0);
            }
            _ => {}
        }
        Action::None
    }

    /// Click on entry `i` of the list (double click: choose it).
    pub fn click(&mut self, i: usize, double: bool) -> Action {
        self.select(i as isize);
        if double { self.choose() } else { Action::None }
    }

    pub fn scroll_by(&mut self, lines: isize) {
        let sel = self.selected as isize;
        self.select(sel + lines);
    }

    /// Draws the dialog over the frame; returns the screen rows of the entries.
    pub fn draw(&mut self, f: &mut Frame, title: &str, filter_label: &str) -> Vec<(Rect, usize)> {
        let area = f.area();
        let w = (area.width * 4 / 5).clamp(40.min(area.width), 100.min(area.width));
        let h = (area.height * 4 / 5).max(8.min(area.height));
        let rect = Rect::new(area.x + (area.width - w) / 2, area.y + (area.height - h) / 2, w, h);
        f.render_widget(Clear, rect);
        f.render_widget(Block::bordered().title(title.to_string()), rect);
        let inner = Rect::new(rect.x + 1, rect.y + 1, rect.width.saturating_sub(2), rect.height.saturating_sub(2));
        let iw = inner.width as usize;
        let mut hits = Vec::new();
        if inner.height < 4 {
            return hits;
        }
        let buf = f.buffer_mut();
        // Folder, shortened from the left.
        let dir = self.dir.display().to_string();
        let skip = dir.width().saturating_sub(iw.saturating_sub(1));
        buf.set_stringn(inner.x, inner.y, format!(" {}", skip_width(&dir, skip)), iw, Style::new().bold().fg(Color::Cyan));
        let rows = inner.height as usize - 3;
        let list_y = inner.y + 1;
        let date = |t: Option<SystemTime>| {
            t.and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|s| {
                    let (y, m, day) = crate::value::civil_from_days((s.as_secs() / 86_400) as i64);
                    format!("{y:04}-{m:02}-{day:02}")
                })
                .unwrap_or_default()
        };
        let size = |n: u64| match n {
            n if n < 1024 => format!("{n} B"),
            n if n < 1024 * 1024 => format!("{:.1} kB", n as f64 / 1024.0),
            n => format!("{:.1} MB", n as f64 / 1024.0 / 1024.0),
        };
        let shown = self.shown();
        for (k, e) in shown.iter().enumerate().skip(self.scroll).take(rows) {
            let y = list_y + (k - self.scroll) as u16;
            let tag = e.tag.map(|t| format!("{t}  ")).unwrap_or_default();
            let right = if e.dir { String::new() } else { format!("{tag}{:>9}  {}", size(e.size), date(e.modified)) };
            let name = if e.dir { format!("▸ {}/", e.name) } else { format!("  {}", e.name) };
            let name_w = iw.saturating_sub(right.width() + 2);
            let text = format!(" {}{} ", pad(&name, name_w), right);
            let style = match (k == self.selected, e.dir) {
                (true, _) => Style::new().reversed(),
                (false, true) => Style::new().bold(),
                (false, false) if (self.accept)(&e.name) => Style::new(),
                (false, false) => Style::new().fg(Color::DarkGray),
            };
            let row = Rect::new(inner.x, y, inner.width, 1);
            buf.set_stringn(row.x, y, pad(&text, iw), iw, style);
            hits.push((row, k));
        }
        if shown.is_empty() {
            buf.set_stringn(inner.x, list_y, "  (no matching files)", iw, Style::new().fg(Color::DarkGray));
        }
        let search = if self.filter.is_empty() { String::new() } else { format!(" search: {}▏", self.filter) };
        buf.set_stringn(inner.x, inner.bottom() - 2, &search, iw, Style::new().fg(Color::Yellow));
        let all = if self.all_files { format!("Ctrl+A: {filter_label} only") } else { "Ctrl+A: all files".to_string() };
        let keys = format!(" ↑↓ Enter · ← up · ~ home · type to search · {all} · Esc");
        buf.set_stringn(inner.x, inner.bottom() - 1, &keys, iw, Style::new().fg(Color::Gray));
        self.rows = rows.max(1);
        hits
    }
}

/// `text` cut or padded to exactly `w` columns.
fn pad(text: &str, w: usize) -> String {
    let tw = text.width();
    if tw <= w {
        return format!("{text}{}", " ".repeat(w - tw));
    }
    let mut out = String::new();
    let mut used = 0;
    for c in text.chars() {
        let cw = c.width().unwrap_or(0);
        if used + cw + 1 > w {
            break;
        }
        out.push(c);
        used += cw;
    }
    out.push('…');
    format!("{out}{}", " ".repeat(w.saturating_sub(used + 1)))
}

fn skip_width(s: &str, skip: usize) -> String {
    let mut w = 0;
    s.chars()
        .skip_while(|c| {
            let keep_skipping = w < skip;
            w += c.width().unwrap_or(0);
            keep_skipping
        })
        .collect()
}
