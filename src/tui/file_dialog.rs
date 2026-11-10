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

/// A place in the sidebar, as in the Finder: a favourite folder or a location
/// (disk, volume, cloud storage).
#[derive(Debug, Clone, PartialEq)]
pub struct Place {
    pub name: String,
    pub path: PathBuf,
    /// Under "Locations" rather than "Favorites".
    pub location: bool,
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
            places: places(),
            in_places: false,
            place: 0,
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
        let dir = self.dir.display().to_string();
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
        let all = if self.all_files { format!("Ctrl+A: {filter_label} only") } else { "Ctrl+A: all files".to_string() };
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
            if i == 0 || p.location != self.places[i - 1].location {
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
                    let title = if self.places[*next].location { "Locations" } else { "Favorites" };
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
    let mut add = |name: String, path: PathBuf, location: bool| {
        if path.is_dir() {
            let path = plain(std::fs::canonicalize(&path).unwrap_or(path));
            if !out.iter().any(|p| p.path == path) {
                out.push(Place { name, path, location });
            }
        }
    };
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from);
    if let Some(home) = &home {
        for (name, path) in standard_folders(home) {
            add(name, path, false);
        }
        let user = home.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "Home".into());
        add(user, home.clone(), false);
    }
    #[cfg(target_os = "macos")]
    {
        if let Some(home) = &home {
            add("iCloud Drive".into(), home.join("Library/Mobile Documents/com~apple~CloudDocs"), true);
        }
        // Volumes: the startup disk ("Macintosh HD" → /) first, then the others.
        let is_root = |p: &PathBuf| std::fs::canonicalize(p).is_ok_and(|p| p == std::path::Path::new("/"));
        let mut volumes = subfolders(std::path::Path::new("/Volumes"));
        volumes.retain(|(name, _)| !name.starts_with("com.apple."));
        volumes.sort_by_key(|(_, path)| !is_root(path));
        if !volumes.iter().any(|(_, p)| is_root(p)) {
            add("Macintosh HD".into(), PathBuf::from("/"), true);
        }
        for (name, path) in volumes {
            add(name, path, true);
        }
        // Cloud storage (OneDrive, Synology Drive, Dropbox, Google Drive …).
        if let Some(home) = &home {
            for (name, path) in subfolders(&home.join("Library/CloudStorage")) {
                add(name, path, true);
            }
        }
    }
    #[cfg(windows)]
    {
        for letter in 'A'..='Z' {
            add(format!("{letter}:"), PathBuf::from(format!("{letter}:\\")), true);
        }
        // Cloud storage: OneDrive (private and business), then the usual
        // folders of other providers in the user's folder.
        let onedrive = ["OneDriveConsumer", "OneDriveCommercial", "OneDrive"].into_iter().filter_map(std::env::var_os).map(PathBuf::from);
        for path in onedrive {
            let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "OneDrive".into());
            add(name, path, true);
        }
        if let Some(home) = &home {
            for (name, path) in subfolders(home) {
                let cloud = ["OneDrive", "Dropbox", "iCloudDrive", "SynologyDrive", "Google Drive", "pCloud", "Nextcloud", "ownCloud", "Box"];
                if cloud.iter().any(|c| name.starts_with(c)) {
                    add(name, path, true);
                }
            }
        }
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        add("Computer".into(), PathBuf::from("/"), true);
        let user = std::env::var("USER").unwrap_or_default();
        for base in [format!("/media/{user}"), format!("/run/media/{user}")] {
            for (name, path) in subfolders(std::path::Path::new(&base)) {
                add(name, path, true);
            }
        }
    }
    out
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
                    .map(|(name, path)| match dirs.iter().find(|(n, _)| *n == name) {
                        Some((_, p)) => (p.file_name().map_or(name, |n| n.to_string_lossy().into_owned()), p.clone()),
                        None => (name, path),
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
/// (`XDG_DESKTOP_DIR="$HOME/Schreibtisch"`), under their English names.
#[cfg(any(test, all(unix, not(target_os = "macos"))))]
fn xdg_user_dirs(text: &str, home: &std::path::Path) -> Vec<(String, PathBuf)> {
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
            (path != home).then(|| (name.to_string(), path))
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
            Place { name: "a".into(), path: root.join("a"), location: false },
            Place { name: "b".into(), path: root.join("b"), location: true },
        ];
        d
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
            vec![("Desktop".to_string(), PathBuf::from("/home/m/Schreibtisch")), ("Documents".to_string(), PathBuf::from("/data/Dokumente"))]
        );
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
