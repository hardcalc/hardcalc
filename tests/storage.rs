use hardcalc::storage::{read_text, write_text, Compression};
use hardcalc::tui::app::App;

const BUDGET: &str = include_str!("../examples/budget.hcmd");

fn dir(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("hardcalc-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn compressed_documents_stay_compressed() {
    let d = dir("compressed");
    let path = d.join("budget.hcmd.gz");
    write_text(&path, BUDGET, Compression::Gzip).unwrap();
    let mut app = App::open(&path).unwrap();
    assert_eq!(app.compression, Compression::Gzip);
    app.save(None);
    let (text, c) = read_text(&path).unwrap();
    assert_eq!(c, Compression::Gzip);
    // Saved by hardcalc: the same document, now with its format version.
    let saved = hardcalc::parse(&text).unwrap();
    assert_eq!(saved.blocks, hardcalc::parse(BUDGET).unwrap().blocks);
    assert_eq!(saved.version, Some(hardcalc::model::FORMAT_VERSION));
    assert!(text.starts_with("<!-- hardcalc 1 -->\n\n# Budget 2026"), "{text}");
    // :compress zstd saves as budget.hcmd.zst.
    app.command("compress zstd");
    let zst = d.join("budget.hcmd.zst");
    assert_eq!(app.path, zst);
    assert_eq!(read_text(&zst).unwrap().1, Compression::Zstd);
    // :compress none: plain Markdown again.
    app.command("compress none");
    assert_eq!(app.path, d.join("budget.hcmd"));
    assert!(std::fs::read_to_string(d.join("budget.hcmd")).unwrap().starts_with("<!-- hardcalc 1 -->"));
    std::fs::remove_dir_all(&d).unwrap();
}

#[test]
fn misnamed_file_keeps_its_compression() {
    let d = dir("misnamed");
    let path = d.join("budget.hcmd");
    write_text(&path, BUDGET, Compression::Xz).unwrap();
    let mut app = App::open(&path).unwrap();
    assert_eq!(app.compression, Compression::Xz);
    app.save(None);
    assert_eq!(read_text(&path).unwrap().1, Compression::Xz);
    std::fs::remove_dir_all(&d).unwrap();
}

#[test]
fn cli_reads_compressed_files() {
    let d = dir("cli");
    let path = d.join("budget.hcmd.zst");
    write_text(&path, BUDGET, Compression::Zstd).unwrap();
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_hardcalc")).arg("fmt").arg(&path).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let mut canonical = hardcalc::parse(BUDGET).unwrap();
    canonical.stamp();
    assert_eq!(String::from_utf8(out.stdout).unwrap(), hardcalc::write(&canonical));
    std::fs::remove_dir_all(&d).unwrap();
}
