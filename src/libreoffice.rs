//! The LibreOffice extension (`libreoffice/extension`) built into hardcalc:
//! `hardcalc libreoffice install`, `uninstall` and `export`.
//!
//! The extension files are embedded; the `.oxt` (a zip) is assembled when
//! needed and handed to LibreOffice's `unopkg`, which installs it in the
//! user's profile – no administrator rights needed.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::{env, fs, io::Write};

/// Identifier of the extension (`description.xml`).
pub const IDENTIFIER: &str = "org.hardcalc.libreoffice";

macro_rules! files {
    ($($name:literal),* $(,)?) => {
        /// Path inside the `.oxt` and content of every file of the extension.
        pub const FILES: &[(&str, &[u8])] = &[
            $(($name, include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/libreoffice/extension/", $name))),)*
        ];
    };
}

files!(
    "META-INF/manifest.xml",
    "description.xml",
    "description/de.txt",
    "description/en.txt",
    "hardcalc_filter.py",
    "pythonpath/hardcalc_lo/__init__.py",
    "pythonpath/hardcalc_lo/builtins.py",
    "pythonpath/hardcalc_lo/convert.py",
    "pythonpath/hardcalc_lo/detect.py",
    "pythonpath/hardcalc_lo/formula.py",
    "pythonpath/hardcalc_lo/markdown.py",
    "pythonpath/hardcalc_lo/styles.py",
    "pythonpath/hardcalc_lo/translate.py",
    "registry/Filters.xcu",
    "registry/Types.xcu",
);

/// Version of the extension, from its `description.xml`.
pub fn version() -> String {
    let xml = FILES.iter().find(|(n, _)| *n == "description.xml").map(|(_, b)| String::from_utf8_lossy(b).into_owned()).unwrap_or_default();
    xml.split("<version value=\"").nth(1).and_then(|r| r.split('"').next()).unwrap_or("0").to_string()
}

/// The `.oxt` file as bytes.
pub fn oxt() -> Result<Vec<u8>, String> {
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for (name, bytes) in FILES {
        zip.start_file(*name, options).map_err(|e| e.to_string())?;
        zip.write_all(bytes).map_err(|e| e.to_string())?;
    }
    Ok(zip.finish().map_err(|e| e.to_string())?.into_inner())
}

/// Default file name of the exported extension.
pub fn file_name() -> String {
    format!("hardcalc-libreoffice-{}.oxt", version())
}

/// The LibreOffice program `unopkg`, if LibreOffice is installed.
pub fn find_unopkg() -> Option<PathBuf> {
    if let Some(p) = env::var_os("HARDCALC_UNOPKG").map(PathBuf::from).filter(|p| p.is_file()) {
        return Some(p);
    }
    let mut candidates: Vec<PathBuf> = Vec::new();
    if cfg!(target_os = "macos") {
        candidates.push("/Applications/LibreOffice.app/Contents/MacOS/unopkg".into());
    } else if cfg!(windows) {
        for var in ["ProgramFiles", "ProgramFiles(x86)", "ProgramW6432"] {
            if let Some(dir) = env::var_os(var) {
                for exe in ["unopkg.com", "unopkg.exe"] {
                    candidates.push(Path::new(&dir).join("LibreOffice").join("program").join(exe));
                }
            }
        }
    } else {
        for dir in ["/usr/lib/libreoffice/program", "/usr/lib64/libreoffice/program", "/opt/libreoffice/program", "/usr/local/lib/libreoffice/program"] {
            candidates.push(Path::new(dir).join("unopkg"));
        }
        candidates.push("/snap/bin/libreoffice.unopkg".into());
    }
    if let Some(found) = candidates.into_iter().find(|p| p.is_file()) {
        return Some(found);
    }
    let paths = env::var_os("PATH")?;
    env::split_paths(&paths).flat_map(|d| [d.join("unopkg"), d.join("unopkg.com"), d.join("unopkg.exe")]).find(|p| p.is_file())
}

/// Whether `unopkg` belongs to the Mac App Store build of LibreOffice, which
/// cannot install extensions with Python components (sandbox).
fn app_store_build(unopkg: &Path) -> bool {
    cfg!(target_os = "macos") && unopkg.ancestors().any(|a| a.join("Contents/_MASReceipt").exists())
}

const APP_STORE: &str = "this is the Mac App Store build of LibreOffice, which cannot install extensions with Python parts. \
    Use LibreOffice from libreoffice.org, or a Windows or Linux installation.";

/// The error when `unopkg` is missing: LibreOffice from the Mac App Store has none.
fn missing() -> String {
    if cfg!(target_os = "macos") && Path::new("/Applications/LibreOffice.app/Contents/_MASReceipt").exists() {
        return APP_STORE.into();
    }
    NOT_FOUND.into()
}

const NOT_FOUND: &str = "LibreOffice not found (no `unopkg`). Install LibreOffice, or write the extension with \
    `hardcalc libreoffice export` and add it in LibreOffice under Tools → Extensions. \
    HARDCALC_UNOPKG=<path> names unopkg explicitly.";

fn run(unopkg: &Path, args: &[&str]) -> Result<String, String> {
    let out = Command::new(unopkg).args(args).output().map_err(|e| format!("{}: {e}", unopkg.display()))?;
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)).trim().to_string();
    if out.status.success() {
        Ok(text)
    } else {
        let hint = if text.contains("lock") || text.contains("already running") || text.contains("pipe") {
            "\nClose LibreOffice (also the quickstarter) and try again."
        } else {
            ""
        };
        Err(format!("unopkg failed: {text}{hint}"))
    }
}

/// `hardcalc libreoffice install`: returns one line per thing done.
pub fn install() -> Result<Vec<String>, String> {
    let unopkg = find_unopkg().ok_or_else(missing)?;
    if app_store_build(&unopkg) {
        return Err(APP_STORE.into());
    }
    let dir = env::temp_dir().join(format!("hardcalc-oxt-{}", std::process::id()));
    fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let file = dir.join(file_name());
    let result = fs::write(&file, oxt()?).map_err(|e| format!("{}: {e}", file.display())).and_then(|()| {
        // `add` replaces an installed version of the extension.
        run(&unopkg, &["add", "--suppress-license", &file.to_string_lossy()])
    });
    let _ = fs::remove_dir_all(&dir);
    result?;
    Ok(vec![
        format!("installed hardcalc Markdown for Calc {} with {}", version(), unopkg.display()),
        "restart LibreOffice; Calc then offers “hardcalc Markdown (.hcmd)” in the Open and Save As dialogs".into(),
    ])
}

/// `hardcalc libreoffice uninstall`.
pub fn uninstall() -> Result<Vec<String>, String> {
    let unopkg = find_unopkg().ok_or_else(missing)?;
    run(&unopkg, &["remove", IDENTIFIER])?;
    Ok(vec![format!("removed the extension {IDENTIFIER} with {}", unopkg.display())])
}

/// `hardcalc libreoffice export [file]`: writes the `.oxt`.
pub fn export(target: Option<&str>) -> Result<Vec<String>, String> {
    let path = PathBuf::from(target.map(String::from).unwrap_or_else(file_name));
    fs::write(&path, oxt()?).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(vec![path.display().to_string()])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn walk(dir: &Path, base: &Path, out: &mut Vec<String>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                if path.file_name().is_some_and(|n| n != "__pycache__") {
                    walk(&path, base, out);
                }
            } else if !path.extension().is_some_and(|e| e == "pyc" || e == "pyo") {
                out.push(path.strip_prefix(base).unwrap().to_string_lossy().replace('\\', "/"));
            }
        }
    }

    #[test]
    fn every_extension_file_is_embedded() {
        let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("libreoffice/extension");
        let mut on_disk = Vec::new();
        walk(&base, &base, &mut on_disk);
        on_disk.sort();
        let mut embedded: Vec<String> = FILES.iter().map(|(n, _)| n.to_string()).collect();
        embedded.sort();
        assert_eq!(on_disk, embedded, "add new extension files to files!(…) in src/libreoffice.rs");
    }

    #[test]
    fn oxt_is_a_zip_with_the_description() {
        let bytes = oxt().unwrap();
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        assert_eq!(zip.len(), FILES.len());
        assert!(zip.by_name("description.xml").is_ok() && zip.by_name("META-INF/manifest.xml").is_ok());
        assert!(version().split('.').count() == 3, "{}", version());
    }
}
