use hardcalc::bundle::{Bundle, Kind, CREATOR};
use hardcalc::tui::app::App;
use std::path::PathBuf;

const BUDGET: &str = include_str!("../examples/budget.hcmd");

fn dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("hardcalc-bundle-it-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn save_as_bundle_and_reopen_keeps_assets() {
    let d = dir("save");
    let mut app = App::new(d.join("budget.hcmd"), hardcalc::parse(BUDGET).unwrap());
    app.command("bundle hcbundle");
    let path = d.join("budget.hcbundle");
    assert_eq!(app.path, path);
    assert!(path.join("info.json").exists() && path.join("text.md").exists());
    assert!(path.join("assets/raster/Expenses.tsv").exists());
    // An image added by another tool survives the next save.
    std::fs::create_dir_all(path.join("assets/bilder")).unwrap();
    std::fs::write(path.join("assets/bilder/skizze.png"), [1, 2, 3]).unwrap();
    let mut app = App::open(&path).unwrap();
    assert_eq!(app.doc, hardcalc::parse(BUDGET).unwrap());
    app.save(None);
    assert_eq!(std::fs::read(path.join("assets/bilder/skizze.png")).unwrap(), [1, 2, 3]);
    let info: serde_json::Value = serde_json::from_slice(&std::fs::read(path.join("info.json")).unwrap()).unwrap();
    assert_eq!(info["creatorIdentifier"], CREATOR);
    assert_eq!(info[CREATOR]["raster"]["Expenses"]["file"], "assets/raster/Expenses.tsv");
    std::fs::remove_dir_all(&d).unwrap();
}

#[test]
fn tsv_edited_outside_is_taken_over() {
    let d = dir("tsv");
    let mut app = App::new(d.join("b.hcmd"), hardcalc::parse(BUDGET).unwrap());
    app.command("bundle textbundle");
    let path = d.join("b.textbundle");
    let tsv = path.join("assets/raster/Expenses.tsv");
    let edited = std::fs::read_to_string(&tsv).unwrap().replace("1200", "1300");
    std::fs::write(&tsv, edited).unwrap();
    let app = App::open(&path).unwrap();
    assert!(app.dirty);
    assert!(hardcalc::write(&app.doc).contains("1300"));
    std::fs::remove_dir_all(&d).unwrap();
}

#[test]
fn foreign_bundle_is_saved_as_new_file() {
    let d = dir("foreign");
    let path = d.join("Note.textpack");
    let mut b = Bundle { text: "# Note\n\nAus Bear.\n".into(), ..Default::default() };
    b.info.insert("version".into(), 2.into());
    b.info.insert("creatorIdentifier".into(), "net.shinyfrog.bear".into());
    b.write(&path, Kind::TextPack).unwrap();
    let before = std::fs::read(&path).unwrap();
    let mut app = App::open(&path).unwrap();
    assert_eq!(app.path, d.join("Note (hardcalc).textpack"));
    app.save(None);
    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert!(Bundle::read(&d.join("Note (hardcalc).textpack")).unwrap().text.contains("Aus Bear."));
    std::fs::remove_dir_all(&d).unwrap();
}

#[test]
fn export_textpack_is_transient_and_cli_reads_bundles() {
    let d = dir("export");
    let src = d.join("budget.hcmd");
    std::fs::write(&src, BUDGET).unwrap();
    let pack = d.join("budget.textpack");
    let exe = env!("CARGO_BIN_EXE_hardcalc");
    let out = std::process::Command::new(exe).arg("export").arg(&src).arg(&pack).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(Bundle::read(&pack).unwrap().transient());
    let out = std::process::Command::new(exe).arg("fmt").arg(&pack).output().unwrap();
    assert_eq!(String::from_utf8(out.stdout).unwrap(), hardcalc::write(&hardcalc::parse(BUDGET).unwrap()));
    std::fs::remove_dir_all(&d).unwrap();
}
