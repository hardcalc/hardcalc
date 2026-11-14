//! The example files, built into the program and written to a folder of
//! their own, so the file dialog can offer them wherever hardcalc was
//! installed from. Files already there are left alone.

use std::path::PathBuf;

/// The examples: file name and content.
const FILES: [(&str, &str); 5] = [
    ("budget.hcmd", include_str!("../examples/budget.hcmd")),
    ("beam.hcmd", include_str!("../examples/beam.hcmd")),
    ("calc-features.hcmd", include_str!("../examples/calc-features.hcmd")),
    ("backup.toml", include_str!("../examples/backup.toml")),
    ("general.toml", include_str!("../examples/general.toml")),
];

/// The folder with the examples; files missing there are written first.
/// Files already there are never overwritten: they may be the user's own
/// changes. `None`: no such folder could be made.
pub fn dir() -> Option<PathBuf> {
    let dir = crate::recent::state_dir()?.join("examples");
    write_into(&dir).then_some(dir)
}

fn write_into(dir: &std::path::Path) -> bool {
    if std::fs::create_dir_all(dir).is_err() {
        return false;
    }
    FILES.iter().all(|(name, text)| {
        let path = dir.join(name);
        path.exists() || std::fs::write(&path, text).is_ok()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn examples_are_written_but_never_overwritten() {
        let dir = std::env::temp_dir().join(format!("hardcalc-examples-{}", std::process::id()));
        assert!(write_into(&dir));
        for (name, text) in FILES {
            assert_eq!(std::fs::read_to_string(dir.join(name)).unwrap(), text);
        }
        // An edited example stays; a deleted one comes back.
        std::fs::write(dir.join("beam.hcmd"), "mine").unwrap();
        std::fs::remove_file(dir.join("budget.hcmd")).unwrap();
        assert!(write_into(&dir));
        assert_eq!(std::fs::read_to_string(dir.join("beam.hcmd")).unwrap(), "mine");
        assert_eq!(std::fs::read_to_string(dir.join("budget.hcmd")).unwrap(), FILES[0].1);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
