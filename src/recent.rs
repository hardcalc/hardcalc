//! The list of recently opened files: a text file in the user's state folder,
//! one absolute path per line, the newest first.

use std::path::{Path, PathBuf};

/// How many files the list keeps.
pub const MAX: usize = 20;

/// Where the list lives; `HARDCALC_STATE_DIR` overrides the usual folder.
fn file() -> Option<PathBuf> {
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
    Some(dir.join("recent.txt"))
}

/// The recent files that still exist, newest first.
pub fn load() -> Vec<PathBuf> {
    let Some(text) = file().and_then(|f| std::fs::read_to_string(f).ok()) else { return Vec::new() };
    parse(&text).into_iter().filter(|p| p.exists()).collect()
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
    let text = std::fs::read_to_string(&file).unwrap_or_default();
    let list = push(parse(&text), &path);
    if let Some(dir) = file.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let body: String = list.iter().map(|p| format!("{}\n", p.display())).collect();
    let _ = std::fs::write(file, body);
}

#[cfg(test)]
mod tests {
    use super::*;

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
