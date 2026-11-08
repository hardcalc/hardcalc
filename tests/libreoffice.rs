#![cfg(unix)]
//! `hardcalc libreoffice …` with a stand-in for LibreOffice's `unopkg`.

use std::process::Command;

fn fake_unopkg(dir: &std::path::Path) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let script = dir.join("unopkg");
    // Records the arguments and checks that the file given to `add` is a zip.
    std::fs::write(&script, format!("#!/bin/sh\necho \"$@\" >> {0}/calls\nif [ \"$1\" = add ]; then head -c 2 \"$3\" >> {0}/magic; fi\n", dir.display())).unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    script
}

fn hardcalc(args: &[&str], unopkg: &std::path::Path) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_hardcalc")).args(args).env("HARDCALC_UNOPKG", unopkg).output().unwrap()
}

#[test]
fn install_and_uninstall_call_unopkg() {
    let dir = std::env::temp_dir().join(format!("hardcalc-lo-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let unopkg = fake_unopkg(&dir);
    assert!(hardcalc(&["libreoffice", "install"], &unopkg).status.success());
    assert!(hardcalc(&["libreoffice", "uninstall"], &unopkg).status.success());
    let calls = std::fs::read_to_string(dir.join("calls")).unwrap();
    assert!(calls.contains("add --suppress-license") && calls.contains(".oxt"), "{calls}");
    assert!(calls.contains("remove org.hardcalc.libreoffice"), "{calls}");
    assert_eq!(std::fs::read(dir.join("magic")).unwrap(), b"PK");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn export_writes_the_oxt() {
    let dir = std::env::temp_dir().join(format!("hardcalc-lo-export-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let out = dir.join("x.oxt");
    let result = Command::new(env!("CARGO_BIN_EXE_hardcalc")).args(["libreoffice", "export", out.to_str().unwrap()]).output().unwrap();
    assert!(result.status.success());
    assert_eq!(&std::fs::read(&out).unwrap()[..2], b"PK");
    let _ = std::fs::remove_dir_all(&dir);
}
