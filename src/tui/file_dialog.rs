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
    /// The whole path, for entries that do not live in the shown folder
    /// (the list of recent files).
    pub path: Option<PathBuf>,
}

/// The "folder" that stands for the list of recent files.
const RECENT: &str = "\u{0}recent";

/// A place in the sidebar, as in the Finder: a favourite folder or a location
/// (disk, volume, cloud storage).
#[derive(Debug, Clone, PartialEq)]
pub struct Place {
    pub name: String,
    pub path: PathBuf,
    pub section: Section,
}

/// The sidebar's sections.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Section {
    Recent,
    Favorites,
    Locations,
}

impl Section {
    fn title(self) -> &'static str {
        match self {
            Section::Recent => "Recent",
            Section::Favorites => "Favorites",
            Section::Locations => "Locations",
        }
    }
}

/// What the list is sorted by.
#[derive(Debug, Clone, Copy, PartialEq)]
enum SortKey {
    Name,
    Date,
}

/// What a screen area of the dialog stands for.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Spot {
    /// Entry `n` of the list.
    Entry(usize),
    /// Place `n` of the sidebar.
    Place(usize),
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
    /// Sidebar: favourites, then locations.
    pub places: Vec<Place>,
    /// Keys go to the sidebar (Tab switches).
    pub in_places: bool,
    /// Index into [`FileDialog::places`].
    pub place: usize,
    /// The probe result the places were taken from; `None`: the places were
    /// set by hand and stay as they are.
    probed: Option<u64>,
    /// The sidebar covers the list (narrow window, set while drawing).
    side_only: bool,
    /// Recently opened files, newest first; empty: no such list.
    recent: Vec<PathBuf>,
    /// The folder shown before the list of recent files.
    before_recent: PathBuf,
    /// Chosen order (key, descending); `None`: by name, recent files by use.
    sort: Option<(SortKey, bool)>,
}

impl FileDialog {
    pub fn new(purpose: Purpose, dir: PathBuf, accept: fn(&str) -> bool) -> FileDialog {
        let mut d = FileDialog {
            purpose,
            dir: plain(dir),
            entries: Vec::new(),
            filter: String::new(),
            selected: 0,
            scroll: 0,
            rows: 10,
            all_files: false,
            accept,
            tag: None,
            places: Vec::new(),
            in_places: false,
            place: 0,
            probed: Some(0),
            side_only: false,
            recent: Vec::new(),
            before_recent: PathBuf::new(),
            sort: None,
        };
        probe::start(std::time::Duration::from_millis(150));
        d.take_places();
        d.read();
        d
    }

    /// Offers the list of recently opened files: a place "Recent" in the
    /// sidebar and Ctrl+R. `show` opens the dialog on that list.
    pub fn with_recent(mut self, recent: Vec<PathBuf>, show: bool) -> FileDialog {
        self.recent = recent;
        self.add_recent_place();
        if show {
            self.show_recent();
        }
        self
    }

    fn add_recent_place(&mut self) {
        if !self.recent.is_empty() && !self.places.iter().any(|p| p.path.as_os_str() == RECENT) {
            self.places.insert(0, Place { name: "Recent".into(), path: PathBuf::from(RECENT), section: Section::Recent });
        }
    }

    fn in_recent(&self) -> bool {
        self.dir.as_os_str() == RECENT
    }

    /// Ctrl+R: the list of recent files, and back to the folder.
    fn toggle_recent(&mut self) {
        if self.in_recent() {
            self.dir = std::mem::take(&mut self.before_recent);
            self.filter.clear();
            self.read();
        } else if !self.recent.is_empty() {
            self.show_recent();
        }
    }

    fn show_recent(&mut self) {
        if !self.in_recent() {
            self.before_recent = self.dir.clone();
        }
        self.dir = PathBuf::from(RECENT);
        self.filter.clear();
        self.in_places = false;
        self.read();
    }

    /// Takes over the places once the probe in the background has found them.
    fn take_places(&mut self) {
        let Some(seen) = self.probed else { return };
        if let Some((n, places)) = probe::newer(seen) {
            // The selected place stays selected if it is still there.
            let selected = self.places.get(self.place).map(|p| p.path.clone());
            self.places = places;
            self.add_recent_place();
            self.probed = Some(n);
            self.place = selected.and_then(|s| self.places.iter().position(|p| p.path == s)).unwrap_or(0);
            if self.places.is_empty() {
                self.in_places = false;
            }
        }
    }

    /// Goes to the home folder.
    fn go_home(&mut self) {
        if let Some(home) = home_dir() {
            // As the places are written, so the home folder's place lights up.
            self.dir = plain(std::fs::canonicalize(&home).unwrap_or(home));
            self.filter.clear();
            self.read();
        }
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
        if self.in_recent() {
            self.entries = self
                .recent
                .iter()
                .filter_map(|p| {
                    let meta = std::fs::metadata(p).ok()?;
                    let name = p.file_name()?.to_string_lossy().into_owned();
                    let tag = self.tag.and_then(|t| t(p));
                    Some(Entry { name, dir: false, size: meta.len(), modified: meta.modified().ok(), tag, path: Some(p.clone()) })
                })
                .collect();
            self.sort_entries();
            self.selected = 0;
            self.scroll = 0;
            return;
        }
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
                            .then(|| Entry { name, dir, size: meta.len(), modified: meta.modified().ok(), tag, path: None })
                    })
                    .collect()
            })
            .unwrap_or_default();
        if self.dir.parent().is_some() {
            entries.insert(0, Entry { name: "..".into(), dir: true, size: 0, modified: None, tag: None, path: None });
        }
        self.entries = entries;
        self.sort_entries();
        self.selected = 0;
        self.scroll = 0;
    }

    /// Orders the entries: ".." and the folders first, then the files by the
    /// chosen key (recent files: by use unless a key was chosen).
    fn sort_entries(&mut self) {
        let recent = self.in_recent();
        let (key, desc) = match self.sort {
            Some(s) => s,
            None if recent => return,
            None => (SortKey::Name, false),
        };
        self.entries.sort_by(|a, b| {
            let by_key = match key {
                SortKey::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
                SortKey::Date => a.modified.cmp(&b.modified),
            };
            let by_key = if desc { by_key.reverse() } else { by_key };
            (a.name != "..").cmp(&(b.name != "..")).then(b.dir.cmp(&a.dir)).then(by_key).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
    }

    /// Ctrl+N / Ctrl+D: sorts by name or date; the same key again reverses.
    /// The selected entry stays selected.
    fn sort_by(&mut self, key: SortKey) {
        self.sort = match self.sort {
            Some((k, desc)) if k == key => Some((k, !desc)),
            // Names run A–Z, dates newest first.
            _ => Some((key, key == SortKey::Date)),
        };
        let keep = self.current().map(|e| e.name);
        self.sort_entries();
        self.selected = keep.and_then(|n| self.shown().iter().position(|e| e.name == n)).unwrap_or(0);
        self.select(self.selected as isize);
    }

    /// What the order is, for the status line.
    fn sort_label(&self) -> String {
        match self.sort {
            Some((SortKey::Name, desc)) => format!("sorted by name {}", if desc { "↓" } else { "↑" }),
            Some((SortKey::Date, desc)) => format!("sorted by date {}", if desc { "newest first" } else { "oldest first" }),
            None if self.in_recent() => "sorted by use".to_string(),
            None => "sorted by name ↑".to_string(),
        }
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

    /// Goes to place `i` of the sidebar; keys go back to the list.
    pub fn go_place(&mut self, i: usize) {
        let Some(p) = self.places.get(i) else { return };
        self.place = i;
        self.dir = p.path.clone();
        self.filter.clear();
        self.in_places = false;
        self.read();
    }

    /// The place showing the current folder.
    fn current_place(&self) -> Option<usize> {
        self.places.iter().position(|p| p.path == self.dir)
    }

    fn key_places(&mut self, key: KeyEvent) -> Action {
        let n = self.places.len() as isize;
        let sel = self.place as isize;
        let to = |i: isize| i.clamp(0, (n - 1).max(0)) as usize;
        match key.code {
            KeyCode::Esc | KeyCode::Tab | KeyCode::BackTab | KeyCode::Left => self.in_places = false,
            KeyCode::F(10) => return Action::Close,
            KeyCode::Up => self.place = to(sel - 1),
            KeyCode::Down => self.place = to(sel + 1),
            KeyCode::Home | KeyCode::PageUp => self.place = 0,
            KeyCode::End | KeyCode::PageDown => self.place = to(n - 1),
            KeyCode::Enter | KeyCode::Right => self.go_place(self.place),
            // Keys of the list that mean the same here: home, all files, one folder up.
            KeyCode::Char('~') | KeyCode::Backspace => {
                self.in_places = false;
                return self.key(key);
            }
            KeyCode::Char('a') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.in_places = false;
                return self.key(key);
            }
            // Typing searches the list again.
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.in_places = false;
                self.filter.push(c);
                self.select(0);
            }
            _ => {}
        }
        Action::None
    }

    fn up(&mut self) {
        if self.in_recent() {
            return;
        }
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
        Action::Chosen(entry.path.unwrap_or_else(|| self.dir.join(&entry.name)))
    }

    pub fn key(&mut self, key: KeyEvent) -> Action {
        if self.in_places {
            return self.key_places(key);
        }
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
            KeyCode::Tab | KeyCode::BackTab if !self.places.is_empty() => {
                self.in_places = true;
                self.place = self.current_place().unwrap_or(self.place.min(self.places.len() - 1));
            }
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
            KeyCode::Char('r') if key.modifiers.contains(KeyModifiers::CONTROL) => self.toggle_recent(),
            KeyCode::Char('n') if key.modifiers.contains(KeyModifiers::CONTROL) => self.sort_by(SortKey::Name),
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => self.sort_by(SortKey::Date),
            KeyCode::Char('~') if self.filter.is_empty() => self.go_home(),
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
        // The keys follow the click: Enter opens the clicked entry.
        self.in_places = false;
        self.select(i as isize);
        if double { self.choose() } else { Action::None }
    }

    pub fn scroll_by(&mut self, lines: isize) {
        if self.in_places && self.side_only {
            // Only the sidebar is on screen: the wheel moves its selection.
            let last = self.places.len().saturating_sub(1) as isize;
            self.place = (self.place as isize + lines.signum()).clamp(0, last) as usize;
            return;
        }
        self.in_places = false;
        let sel = self.selected as isize;
        self.select(sel + lines);
    }

    /// Click on place `i` of the sidebar: goes there, like in the Finder.
    pub fn click_place(&mut self, i: usize) -> Action {
        self.go_place(i);
        Action::None
    }

    /// Click on a spot the dialog reported while drawing.
    pub fn click_spot(&mut self, spot: Spot, double: bool) -> Action {
        match spot {
            Spot::Entry(i) => self.click(i, double),
            Spot::Place(i) => self.click_place(i),
        }
    }

    /// Draws the dialog over the frame; returns the screen rows of the entries
    /// and of the places in the sidebar.
    pub fn draw(&mut self, f: &mut Frame, title: &str, filter_label: &str) -> Vec<(Rect, Spot)> {
        self.take_places();
        let area = f.area();
        let w = (area.width * 4 / 5).clamp(40.min(area.width), 120.min(area.width));
        let h = (area.height * 4 / 5).max(8.min(area.height));
        let rect = Rect::new(area.x + (area.width - w) / 2, area.y + (area.height - h) / 2, w, h);
        f.render_widget(Clear, rect);
        f.render_widget(Block::bordered().title(title.to_string()), rect);
        let inner = Rect::new(rect.x + 1, rect.y + 1, rect.width.saturating_sub(2), rect.height.saturating_sub(2));
        let full_w = inner.width as usize;
        let mut hits = Vec::new();
        if inner.height < 4 {
            return hits;
        }
        let buf = f.buffer_mut();
        // Folder, shortened from the left.
        let dir = if self.in_recent() { "Recent files".to_string() } else { self.dir.display().to_string() };
        let skip = dir.width().saturating_sub(full_w.saturating_sub(1));
        buf.set_stringn(inner.x, inner.y, format!(" {}", skip_width(&dir, skip)), full_w, Style::new().bold().fg(Color::Cyan));
        let rows = inner.height as usize - 3;
        let list_y = inner.y + 1;
        // Sidebar on the left when there is room – or across the list while it has the keys.
        let side_w = if self.places.is_empty() {
            0
        } else if self.in_places && full_w < 60 {
            full_w
        } else if full_w >= 60 {
            let longest = self.places.iter().map(|p| p.name.width()).max().unwrap_or(0).max("Locations".len());
            (longest + 4).clamp(14, 28).min(full_w / 3)
        } else {
            0
        };
        self.side_only = side_w > 0 && side_w >= full_w;
        if side_w > 0 {
            hits.extend(self.draw_places(buf, Rect::new(inner.x, list_y, side_w as u16, rows as u16)));
        }
        let (list_x, iw) = match side_w {
            0 => (inner.x, full_w),
            w if w >= full_w => (inner.x, 0),
            w => {
                for y in list_y..list_y + rows as u16 {
                    buf.set_stringn(inner.x + w as u16, y, "│", 1, Style::new().fg(Color::DarkGray));
                }
                (inner.x + w as u16 + 1, full_w - w - 1)
            }
        };
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
        let shown = if iw == 0 { Vec::new() } else { self.shown() };
        for (k, e) in shown.iter().enumerate().skip(self.scroll).take(rows) {
            let y = list_y + (k - self.scroll) as u16;
            let tag = e.tag.map(|t| format!("{t}  ")).unwrap_or_default();
            let right = if e.dir { String::new() } else { format!("{tag}{:>9}  {}", size(e.size), date(e.modified)) };
            let name = if e.dir { format!("▸ {}/", e.name) } else { format!("  {}", e.name) };
            let name_w = iw.saturating_sub(right.width() + 2);
            let text = format!(" {}{} ", pad(&name, name_w), right);
            let style = match (k == self.selected, e.dir) {
                (true, _) if self.in_places => Style::new().underlined(),
                (true, _) => Style::new().reversed(),
                (false, true) => Style::new().bold(),
                (false, false) if (self.accept)(&e.name) => Style::new(),
                (false, false) => Style::new().fg(Color::DarkGray),
            };
            let row = Rect::new(list_x, y, iw as u16, 1);
            buf.set_stringn(row.x, y, pad(&text, iw), iw, style);
            hits.push((row, Spot::Entry(k)));
        }
        if shown.is_empty() && iw > 0 {
            buf.set_stringn(list_x, list_y, "  (no matching files)", iw, Style::new().fg(Color::DarkGray));
        }
        let search = if self.filter.is_empty() { String::new() } else { format!(" search: {}▏", self.filter) };
        buf.set_stringn(inner.x, inner.bottom() - 2, &search, full_w, Style::new().fg(Color::Yellow));
        // Right-aligned on the same line: the order and its keys.
        let order = format!("{} · Ctrl+N name · Ctrl+D date ", self.sort_label());
        if order.width() + search.width() < full_w {
            buf.set_stringn(inner.x + (full_w - order.width()) as u16, inner.bottom() - 2, &order, full_w, Style::new().fg(Color::DarkGray));
        }
        let all = if self.all_files { format!("Ctrl+A: {filter_label} only") } else { "Ctrl+A: all files".to_string() };
        let all = if self.recent.is_empty() { all } else { format!("Ctrl+R: recent · {all}") };
        let keys = if self.in_places {
            " ↑↓ Enter: go there · Tab / Esc: back to the list".to_string()
        } else if self.places.is_empty() {
            format!(" ↑↓ Enter · ← up · ~ home · type to search · {all} · Esc")
        } else {
            format!(" ↑↓ Enter · ← up · Tab places · ~ home · type to search · {all} · Esc")
        };
        buf.set_stringn(inner.x, inner.bottom() - 1, &keys, full_w, Style::new().fg(Color::Gray));
        self.rows = rows.max(1);
        hits
    }

    /// The sidebar: headings "Favorites" and "Locations" with their places.
    fn draw_places(&self, buf: &mut ratatui::buffer::Buffer, area: Rect) -> Vec<(Rect, Spot)> {
        // Lines: a heading (`None`) or place `i`.
        let mut lines: Vec<Option<usize>> = Vec::new();
        for (i, p) in self.places.iter().enumerate() {
            if i == 0 || p.section != self.places[i - 1].section {
                if i > 0 {
                    lines.push(None);
                }
                lines.push(None);
            }
            lines.push(Some(i));
        }
        let rows = area.height as usize;
        let at = lines.iter().position(|l| *l == Some(self.place)).unwrap_or(0);
        let skip = if self.in_places && at >= rows { at + 1 - rows } else { 0 };
        let current = self.current_place();
        let w = area.width as usize;
        let mut hits = Vec::new();
        for (k, line) in lines.iter().enumerate().skip(skip).take(rows) {
            let y = area.y + (k - skip) as u16;
            match line {
                None => {
                    // A heading: the section of the place below it.
                    let Some(Some(next)) = lines.get(k + 1) else { continue };
                    let title = self.places[*next].section.title();
                    buf.set_stringn(area.x, y, format!(" {title}"), w, Style::new().fg(Color::DarkGray).bold());
                }
                Some(i) => {
                    let style = if self.in_places && *i == self.place {
                        Style::new().reversed()
                    } else if current == Some(*i) {
                        Style::new().bold().fg(Color::Cyan)
                    } else {
                        Style::new()
                    };
                    let row = Rect::new(area.x, y, area.width, 1);
                    buf.set_stringn(area.x, y, pad(&format!("  {}", self.places[*i].name), w), w, style);
                    hits.push((row, Spot::Place(*i)));
                }
            }
        }
        hits
    }
}

/// The sidebar's places, like the Finder's: favourites (the home folder and
/// its standard folders), then locations (disks, volumes, cloud storage).
pub fn places() -> Vec<Place> {
    let mut out: Vec<Place> = Vec::new();
    let mut add = |name: String, path: PathBuf, section: Section| {
        if path.is_dir() {
            let path = plain(std::fs::canonicalize(&path).unwrap_or(path));
            if !out.iter().any(|p| p.path == path) {
                out.push(Place { name, path, section });
            }
        }
    };
    let home = home_dir();
    if let Some(home) = &home {
        for (name, path) in standard_folders(home) {
            add(name, path, Section::Favorites);
        }
        let user = home.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "Home".into());
        add(user, home.clone(), Section::Favorites);
    }
    #[cfg(target_os = "macos")]
    {
        if let Some(home) = &home {
            add("iCloud Drive".into(), home.join("Library/Mobile Documents/com~apple~CloudDocs"), Section::Locations);
        }
        // Volumes: the startup disk ("Macintosh HD" → /) first, then the others.
        let is_root = |p: &PathBuf| std::fs::canonicalize(p).is_ok_and(|p| p == std::path::Path::new("/"));
        let mut volumes = subfolders(std::path::Path::new("/Volumes"));
        volumes.retain(|(name, _)| !name.starts_with("com.apple."));
        volumes.sort_by_key(|(_, path)| !is_root(path));
        if !volumes.iter().any(|(_, p)| is_root(p)) {
            add("Macintosh HD".into(), PathBuf::from("/"), Section::Locations);
        }
        for (name, path) in volumes {
            add(name, path, Section::Locations);
        }
        // Cloud storage (OneDrive, Synology Drive, Dropbox, Google Drive …).
        if let Some(home) = &home {
            for (name, path) in subfolders(&home.join("Library/CloudStorage")) {
                add(name, path, Section::Locations);
            }
        }
    }
    #[cfg(windows)]
    {
        for letter in 'A'..='Z' {
            add(format!("{letter}:"), PathBuf::from(format!("{letter}:\\")), Section::Locations);
        }
        // Cloud storage: OneDrive (private and business), then the usual
        // folders of other providers in the user's folder.
        let onedrive = ["OneDriveConsumer", "OneDriveCommercial", "OneDrive"].into_iter().filter_map(std::env::var_os).map(PathBuf::from);
        for path in onedrive {
            let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "OneDrive".into());
            add(name, path, Section::Locations);
        }
        if let Some(home) = &home {
            for (name, path) in subfolders(home) {
                if is_cloud_folder(&name) {
                    add(name, path, Section::Locations);
                }
            }
        }
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        add("Computer".into(), PathBuf::from("/"), Section::Locations);
        // Removable media are mounted below a folder named after the user;
        // without the name there is nothing to list (not the other users' folders).
        let user = ["USER", "LOGNAME"].into_iter().filter_map(|k| std::env::var(k).ok()).find(|u| !u.is_empty() && !u.contains('/'));
        for base in user.iter().flat_map(|user| [format!("/media/{user}"), format!("/run/media/{user}")]) {
            for (name, path) in subfolders(std::path::Path::new(&base)) {
                add(name, path, Section::Locations);
            }
        }
    }
    out
}

/// The user's home folder, as the environment names it.
pub fn home_dir() -> Option<PathBuf> {
    ["HOME", "USERPROFILE"].into_iter().filter_map(std::env::var_os).find(|h| !h.is_empty()).map(PathBuf::from)
}

/// Whether a folder in the user's folder is the one of a cloud storage:
/// its usual name, alone or with an addition (`OneDrive - Company`,
/// `Dropbox (Personal)`) – not `Boxes` or `Dropbox-backup`.
#[cfg(any(test, windows))]
fn is_cloud_folder(name: &str) -> bool {
    let cloud = ["OneDrive", "Dropbox", "iCloudDrive", "SynologyDrive", "Google Drive", "pCloud", "Nextcloud", "ownCloud", "Box"];
    cloud.iter().any(|c| name.strip_prefix(c).is_some_and(|rest| rest.is_empty() || rest.starts_with([' ', '('])))
}

/// The places are looked up in the background: a disconnected network drive
/// or a stale mount can make a single look take many seconds, and the dialog
/// must not wait for that. It shows the places found last time meanwhile.
mod probe {
    use super::Place;
    use std::sync::{Condvar, Mutex};
    use std::time::{Duration, Instant};

    #[derive(Default)]
    struct State {
        /// Counts the finished probes; 0: none yet.
        n: u64,
        places: Vec<Place>,
        /// When the probe that is still running started.
        running: Option<Instant>,
    }

    /// After this time a probe hangs (a dead network drive): a new one may start.
    const GIVE_UP: Duration = Duration::from_secs(10);

    static STATE: Mutex<Option<State>> = Mutex::new(None);
    static DONE: Condvar = Condvar::new();

    /// Starts a probe and waits up to `wait` for it, so the dialog usually
    /// opens with fresh places. While an earlier probe is still running
    /// (it hangs), the dialog does not wait again; a probe that hangs for
    /// long is given up and a new one starts.
    pub fn start(wait: Duration) {
        let mut guard = STATE.lock().unwrap_or_else(|e| e.into_inner());
        let state = guard.get_or_insert_with(State::default);
        let before = state.n;
        match state.running {
            Some(since) if since.elapsed() < GIVE_UP => return,
            _ => {}
        }
        let started = Instant::now();
        state.running = Some(started);
        std::thread::spawn(move || {
            let places = super::places();
            let mut guard = STATE.lock().unwrap_or_else(|e| e.into_inner());
            let state = guard.get_or_insert_with(State::default);
            state.places = places;
            state.n += 1;
            // A newer probe may be running by now; only this one is done.
            if state.running == Some(started) {
                state.running = None;
            }
            DONE.notify_all();
        });
        let until = Instant::now() + wait;
        while guard.as_ref().is_some_and(|s| s.n == before) {
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break;
            }
            guard = DONE.wait_timeout(guard, left).unwrap_or_else(|e| e.into_inner()).0;
        }
    }

    /// The places of a probe finished after probe `seen`, with its number.
    pub fn newer(seen: u64) -> Option<(u64, Vec<Place>)> {
        let guard = STATE.lock().unwrap_or_else(|e| e.into_inner());
        guard.as_ref().filter(|s| s.n > seen).map(|s| (s.n, s.places.clone()))
    }
}

/// Visible subfolders of `dir`, sorted by name.
fn subfolders(dir: &std::path::Path) -> Vec<(String, PathBuf)> {
    let mut list: Vec<(String, PathBuf)> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .map(|e| (e.file_name().to_string_lossy().into_owned(), e.path()))
                .filter(|(name, path)| !name.starts_with('.') && path.is_dir())
                .collect()
        })
        .unwrap_or_default();
    list.sort_by_key(|(name, _)| name.to_lowercase());
    list
}

/// Desktop, documents and downloads – where the system keeps them.
fn standard_folders(home: &std::path::Path) -> Vec<(String, PathBuf)> {
    let english = || ["Desktop", "Documents", "Downloads"].map(|n| (n.to_string(), home.join(n))).to_vec();
    #[cfg(windows)]
    {
        // Explorer's folders, also when OneDrive or the user moved them.
        let moved = windows_shell_folders();
        english().into_iter().map(|(name, path)| moved.iter().find(|(n, _)| *n == name).cloned().unwrap_or((name, path))).collect()
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        // XDG user folders, often localized ("Schreibtisch", "Dokumente").
        let config = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or_else(|| home.join(".config"));
        match std::fs::read_to_string(config.join("user-dirs.dirs")) {
            Ok(text) => {
                let dirs = xdg_user_dirs(&text, home);
                english()
                    .into_iter()
                    .filter_map(|(name, path)| match dirs.iter().find(|(n, _)| *n == name) {
                        Some((_, Some(p))) => Some((p.file_name().map_or(name, |n| n.to_string_lossy().into_owned()), p.clone())),
                        // Switched off by the user: no such folder.
                        Some((_, None)) => None,
                        None => Some((name, path)),
                    })
                    .collect()
            }
            Err(_) => english(),
        }
    }
    #[cfg(not(any(windows, all(unix, not(target_os = "macos")))))]
    english()
}

/// The desktop, documents and download folders of an XDG `user-dirs.dirs`
/// (`XDG_DESKTOP_DIR="$HOME/Schreibtisch"`), under their English names;
/// `None` for a folder the user switched off.
#[cfg(any(test, all(unix, not(target_os = "macos"))))]
fn xdg_user_dirs(text: &str, home: &std::path::Path) -> Vec<(String, Option<PathBuf>)> {
    let keys = [("XDG_DESKTOP_DIR", "Desktop"), ("XDG_DOCUMENTS_DIR", "Documents"), ("XDG_DOWNLOAD_DIR", "Downloads")];
    text.lines()
        .filter_map(|line| {
            let (key, value) = line.trim().split_once('=')?;
            let (_, name) = keys.iter().find(|(k, _)| *k == key.trim())?;
            let value = value.trim().trim_matches('"');
            let path = match value.strip_prefix("$HOME") {
                Some(rest) => home.join(rest.trim_start_matches('/')),
                None if value.starts_with('/') => PathBuf::from(value),
                None => return None,
            };
            // A folder set to the home folder itself means "none".
            Some((name.to_string(), (path != home).then_some(path)))
        })
        .collect()
}

/// Explorer's desktop, documents and download folders (also when OneDrive
/// or the user moved them), under their English names.
#[cfg(windows)]
fn windows_shell_folders() -> Vec<(String, PathBuf)> {
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::System::Com::CoTaskMemFree;
    use windows_sys::Win32::UI::Shell::{FOLDERID_Desktop, FOLDERID_Documents, FOLDERID_Downloads, SHGetKnownFolderPath};
    let known = [(&FOLDERID_Desktop, "Desktop"), (&FOLDERID_Documents, "Documents"), (&FOLDERID_Downloads, "Downloads")];
    known
        .into_iter()
        .filter_map(|(id, name)| {
            let mut ptr: *mut u16 = std::ptr::null_mut();
            // SAFETY: `id` is a valid GUID; on success `ptr` is a NUL-terminated
            // wide string that we free with CoTaskMemFree (also on failure, as documented).
            unsafe {
                let ok = SHGetKnownFolderPath(id, 0, std::ptr::null_mut(), &mut ptr) >= 0 && !ptr.is_null();
                let path = ok.then(|| {
                    let len = (0..).take_while(|&i| *ptr.add(i) != 0).count();
                    PathBuf::from(std::ffi::OsString::from_wide(std::slice::from_raw_parts(ptr, len)))
                });
                CoTaskMemFree(ptr as *const _);
                path.map(|p| (name.to_string(), p))
            }
        })
        .collect()
}

/// The path without Windows' `\\?\` prefix that `canonicalize` adds
/// (`\\?\C:\Users` → `C:\Users`, `\\?\UNC\server\share` → `\\server\share`).
pub fn plain(path: PathBuf) -> PathBuf {
    let text = path.to_string_lossy();
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        return PathBuf::from(format!(r"\\{rest}"));
    }
    match text.strip_prefix(r"\\?\") {
        Some(rest) if rest.as_bytes().get(1) == Some(&b':') => PathBuf::from(rest),
        _ => path,
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

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::crossterm::event::KeyEvent;

    fn key(d: &mut FileDialog, code: KeyCode) -> Action {
        d.key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    fn dialog(root: &std::path::Path) -> FileDialog {
        let mut d = FileDialog::new(Purpose::Open, root.to_path_buf(), |n| n.ends_with(".md"));
        d.places = vec![
            Place { name: "a".into(), path: root.join("a"), section: Section::Favorites },
            Place { name: "b".into(), path: root.join("b"), section: Section::Locations },
        ];
        // Places set by hand: the probe must not replace them.
        d.probed = None;
        d.place = 0;
        d
    }

    #[test]
    fn sorting_by_name_and_date_keeps_folders_first() {
        let root = std::env::temp_dir().join(format!("hardcalc-sort-{}", std::process::id()));
        std::fs::create_dir_all(root.join("zdir")).unwrap();
        std::fs::write(root.join("b.md"), "").unwrap();
        std::fs::write(root.join("a.md"), "").unwrap();
        let old = std::fs::File::options().write(true).open(root.join("b.md")).unwrap();
        old.set_modified(std::time::SystemTime::UNIX_EPOCH).unwrap();
        let mut d = dialog(&root);
        let names = |d: &FileDialog| d.shown().iter().map(|e| e.name.clone()).collect::<Vec<_>>();
        assert_eq!(names(&d), ["..", "zdir", "a.md", "b.md"]);
        d.key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL));
        assert_eq!(names(&d), ["..", "zdir", "a.md", "b.md"], "newest first: a.md");
        d.key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL));
        assert_eq!(names(&d), ["..", "zdir", "b.md", "a.md"]);
        d.key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::CONTROL));
        d.key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::CONTROL));
        assert_eq!(names(&d), ["..", "zdir", "b.md", "a.md"], "name descending");
    }

    #[test]
    fn tab_goes_to_the_places_and_enter_opens_one() {
        let root = std::env::temp_dir().join(format!("hardcalc-places-{}", std::process::id()));
        std::fs::create_dir_all(root.join("a")).unwrap();
        std::fs::create_dir_all(root.join("b")).unwrap();
        std::fs::write(root.join("b/x.md"), "").unwrap();
        let mut d = dialog(&root);
        key(&mut d, KeyCode::Tab);
        assert!(d.in_places);
        key(&mut d, KeyCode::Down);
        key(&mut d, KeyCode::Enter);
        assert!(!d.in_places);
        assert_eq!(d.dir, root.join("b"));
        assert_eq!(d.current().unwrap().name, "..");
        // Back in the sidebar, the current folder's place is selected.
        key(&mut d, KeyCode::Tab);
        assert_eq!(d.place, 1);
        // Typing searches the list again.
        key(&mut d, KeyCode::Char('x'));
        assert!(!d.in_places);
        assert_eq!(d.current().unwrap().name, "x.md");
        assert_eq!(d.click_spot(Spot::Place(0), false), Action::None);
        assert_eq!(d.dir, root.join("a"));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn xdg_user_dirs_are_read() {
        let text = "# comment\nXDG_DESKTOP_DIR=\"$HOME/Schreibtisch\"\nXDG_DOWNLOAD_DIR=\"$HOME/\"\nXDG_DOCUMENTS_DIR=\"/data/Dokumente\"\nXDG_MUSIC_DIR=\"$HOME/Musik\"\n";
        let home = std::path::Path::new("/home/m");
        assert_eq!(
            xdg_user_dirs(text, home),
            vec![
                ("Desktop".to_string(), Some(PathBuf::from("/home/m/Schreibtisch"))),
                // Set to the home folder: switched off.
                ("Downloads".to_string(), None),
                ("Documents".to_string(), Some(PathBuf::from("/data/Dokumente"))),
            ]
        );
    }

    #[test]
    fn cloud_folders_by_their_names() {
        for name in ["OneDrive", "OneDrive - Company", "Dropbox (Personal)", "Box", "Box Sync", "Google Drive"] {
            assert!(is_cloud_folder(name), "{name}");
        }
        for name in ["Boxes", "BoxSync-old", "Dropbox-backup-2019", "OneDriveTemp", "Documents"] {
            assert!(!is_cloud_folder(name), "{name}");
        }
    }

    #[test]
    fn a_click_in_the_list_takes_the_keys_back() {
        let root = std::env::temp_dir().join(format!("hardcalc-places-click-{}", std::process::id()));
        std::fs::create_dir_all(root.join("a")).unwrap();
        std::fs::create_dir_all(root.join("b")).unwrap();
        std::fs::write(root.join("x.md"), "").unwrap();
        let mut d = dialog(&root);
        key(&mut d, KeyCode::Tab);
        assert!(d.in_places);
        let i = d.shown().iter().position(|e| e.name == "x.md").unwrap();
        d.click(i, false);
        assert!(!d.in_places);
        assert_eq!(key(&mut d, KeyCode::Enter), Action::Chosen(root.join("x.md")));
        // In the sidebar, Ctrl+A and Backspace act as in the list instead of doing nothing.
        key(&mut d, KeyCode::Tab);
        d.key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL));
        assert!(d.all_files && !d.in_places);
        // The wheel scrolls the list, also while the sidebar has the keys.
        key(&mut d, KeyCode::Tab);
        d.scroll_by(1);
        assert!(!d.in_places);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn verbatim_prefix_is_removed() {
        assert_eq!(plain(PathBuf::from(r"\\?\C:\Users\m")), PathBuf::from(r"C:\Users\m"));
        assert_eq!(plain(PathBuf::from(r"\\?\UNC\nas\share")), PathBuf::from(r"\\nas\share"));
        assert_eq!(plain(PathBuf::from(r"\\?\Volume{1234}\x")), PathBuf::from(r"\\?\Volume{1234}\x"));
        assert_eq!(plain(PathBuf::from("/home/m")), PathBuf::from("/home/m"));
    }

    #[test]
    fn sidebar_is_drawn_with_headings() {
        let root = std::env::temp_dir();
        let mut d = dialog(&root);
        let backend = ratatui::backend::TestBackend::new(100, 20);
        let mut t = ratatui::Terminal::new(backend).unwrap();
        let mut hits = Vec::new();
        t.draw(|f| hits = d.draw(f, " open file ", "documents")).unwrap();
        assert!(hits.iter().any(|(_, s)| *s == Spot::Place(1)));
        let text: String = t.backend().buffer().content().iter().map(|c| c.symbol()).collect();
        assert!(text.contains("Favorites") && text.contains("Locations"));
    }
}
