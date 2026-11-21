//! The list of recently opened files: a text file in the user's state folder,
//! one absolute path per line, the newest first – and where each document was
//! left (its table and cursor), so it opens there again.

use std::path::{Path, PathBuf};

/// How many files the list keeps.
pub const MAX: usize = 20;

/// Where the list lives.
fn file() -> Option<PathBuf> {
    Some(state_dir()?.join("recent.txt"))
}

/// hardcalc's own folder for what it keeps between runs (the list of recent
/// files, the examples); `HARDCALC_STATE_DIR` overrides the usual folder.
pub fn state_dir() -> Option<PathBuf> {
    let dir = if let Some(d) = std::env::var_os("HARDCALC_STATE_DIR").filter(|d| !d.is_empty()) {
        PathBuf::from(d)
    } else {
        let home = crate::tui::file_dialog::home_dir();
        if cfg!(windows) {
            std::env::var_os("APPDATA").map(PathBuf::from).or_else(|| home.map(|h| h.join("AppData").join("Roaming")))?.join("hardcalc")
        } else if cfg!(target_os = "macos") {
            home?.join("Library/Application Support/hardcalc")
        } else {
            std::env::var_os("XDG_STATE_HOME").filter(|d| !d.is_empty()).map(PathBuf::from).or_else(|| home.map(|h| h.join(".local/state")))?.join("hardcalc")
        }
    };
    Some(dir)
}

/// The recent files that still exist, newest first.
pub fn load() -> Vec<PathBuf> {
    let Some(text) = file().and_then(|f| std::fs::read_to_string(f).ok()) else { return Vec::new() };
    parse(&text).into_iter().filter(|p| p.exists() && !is_temporary(p)).collect()
}

/// A file in the system's temporary folder: scratch files (the tests write
/// theirs there) are not worth remembering.
fn is_temporary(path: &Path) -> bool {
    let tmp = std::env::temp_dir();
    let tmp = std::fs::canonicalize(&tmp).unwrap_or(tmp);
    path.starts_with(&tmp)
}

fn parse(text: &str) -> Vec<PathBuf> {
    text.lines().map(str::trim).filter(|l| !l.is_empty()).map(PathBuf::from).collect()
}

/// `list` with `path` moved to the front (no duplicates, at most [`MAX`]).
fn push(list: Vec<PathBuf>, path: &Path) -> Vec<PathBuf> {
    let mut out = vec![path.to_path_buf()];
    out.extend(list.into_iter().filter(|p| p != path));
    out.truncate(MAX);
    out
}

/// Notes `path` as opened or saved just now. Does nothing in unit tests, so
/// they leave the user's list alone; errors are ignored – the list is a
/// convenience.
pub fn add(path: &Path) {
    if cfg!(test) || path.as_os_str().is_empty() {
        return;
    }
    let Some(file) = file() else { return };
    let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    if is_temporary(&path) {
        return;
    }
    let text = std::fs::read_to_string(&file).unwrap_or_default();
    let list = push(parse(&text), &path);
    if let Some(dir) = file.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let body: String = list.iter().map(|p| format!("{}\n", p.display())).collect();
    let _ = std::fs::write(file, body);
}

/// Where each document was left: its table and the cell of the cursor.
fn views_file() -> Option<PathBuf> {
    Some(state_dir()?.join("views.txt"))
}

/// Where a document was left: the table's name and the cursor (row, column).
#[derive(Debug, Clone, PartialEq)]
pub struct Spot {
    pub table: String,
    pub cursor: (usize, usize),
}

/// Lines `path<TAB>table<TAB>row<TAB>column`, the newest first.
fn parse_views(text: &str) -> Vec<(PathBuf, Spot)> {
    text.lines()
        .filter_map(|line| {
            let mut parts = line.split('\t');
            let path = PathBuf::from(parts.next().filter(|p| !p.is_empty())?);
            let table = parts.next()?.to_string();
            let row = parts.next()?.parse().ok()?;
            let col = parts.next()?.parse().ok()?;
            Some((path, Spot { table, cursor: (row, col) }))
        })
        .collect()
}

/// `list` with `path` left at `spot` in front (no duplicates, at most [`MAX_VIEWS`]).
fn push_view(list: Vec<(PathBuf, Spot)>, path: &Path, spot: Spot) -> Vec<(PathBuf, Spot)> {
    let mut out = vec![(path.to_path_buf(), spot)];
    out.extend(list.into_iter().filter(|(p, _)| p != path));
    out.truncate(MAX_VIEWS);
    out
}

/// How many documents keep where they were left.
const MAX_VIEWS: usize = 100;

/// Notes where `path` was left. Does nothing in unit tests; errors are ignored.
pub fn remember(path: &Path, spot: Spot) {
    if cfg!(test) || path.as_os_str().is_empty() || spot.table.contains(['\t', '\n']) {
        return;
    }
    let Some(file) = views_file() else { return };
    let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    // Scratch files are not remembered – unless the state folder is a scratch one too (tests/spot.rs).
    if is_temporary(&path) && std::env::var_os("HARDCALC_STATE_DIR").is_none() {
        return;
    }
    let text = std::fs::read_to_string(&file).unwrap_or_default();
    let list = push_view(parse_views(&text), &path, spot);
    if let Some(dir) = file.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let body: String = list.iter().map(|(p, s)| format!("{}\t{}\t{}\t{}\n", p.display(), s.table, s.cursor.0, s.cursor.1)).collect();
    let _ = std::fs::write(file, body);
}

/// Where `path` was left last time.
pub fn spot(path: &Path) -> Option<Spot> {
    let path = std::fs::canonicalize(path).ok()?;
    let text = std::fs::read_to_string(views_file()?).ok()?;
    parse_views(&text).into_iter().find(|(p, _)| *p == path).map(|(_, s)| s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn views_are_kept_per_document() {
        let spot = |t: &str, r, c| Spot { table: t.into(), cursor: (r, c) };
        let list = push_view(Vec::new(), Path::new("/a.hcmd"), spot("Costs", 3, 1));
        let list = push_view(list, Path::new("/b.hcmd"), spot("Sheet1", 0, 0));
        let list = push_view(list, Path::new("/a.hcmd"), spot("Income", 7, 2));
        let text: String = list.iter().map(|(p, s)| format!("{}\t{}\t{}\t{}\n", p.display(), s.table, s.cursor.0, s.cursor.1)).collect();
        let back = parse_views(&format!("{text}broken line\n"));
        assert_eq!(back, [(PathBuf::from("/a.hcmd"), spot("Income", 7, 2)), (PathBuf::from("/b.hcmd"), spot("Sheet1", 0, 0))]);
    }

    #[test]
    fn newest_first_without_duplicates() {
        let list = push(vec!["/a".into(), "/b".into(), "/c".into()], Path::new("/b"));
        assert_eq!(list, [PathBuf::from("/b"), "/a".into(), "/c".into()]);
    }

    #[test]
    fn the_list_is_capped() {
        let many: Vec<PathBuf> = (0..MAX).map(|i| PathBuf::from(format!("/f{i}"))).collect();
        let list = push(many, Path::new("/new"));
        assert_eq!(list.len(), MAX);
        assert_eq!(list[0], PathBuf::from("/new"));
    }

    #[test]
    fn blank_lines_are_skipped() {
        assert_eq!(parse("/a\n\n  /b  \n"), [PathBuf::from("/a"), "/b".into()]);
    }
}
