//! A document opens at the table and cell it was left at. A test binary of
//! its own: it points hardcalc's state folder at a scratch folder.

use hardcalc::tui::app::App;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

const DOC: &str = "# Two tables\n\n## Costs :: matrix[*, 2]\n\n| Item | Amount |\n|------|--------|\n| a    | 1      |\n| b    | 2      |\n\n## Income :: matrix[*, 2]\n\n| Item | Amount |\n|------|--------|\n| x    | 10     |\n| y    | 20     |\n| z    | 30     |\n";

fn key(app: &mut App, code: KeyCode) {
    app.on_key(KeyEvent::new(code, KeyModifiers::NONE));
}

#[test]
fn a_document_opens_where_it_was_left() {
    let dir = std::env::temp_dir().join(format!("hardcalc-spot-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    // SAFETY: the only test of this binary; no other thread reads the environment yet.
    unsafe { std::env::set_var("HARDCALC_STATE_DIR", dir.join("state")) };
    let file = dir.join("two.hcmd");
    std::fs::write(&file, DOC).unwrap();

    let mut a = App::open(&file).unwrap();
    assert_eq!(a.view.sheet, 0);
    a.switch_sheet(1);
    key(&mut a, KeyCode::Down);
    key(&mut a, KeyCode::Down);
    key(&mut a, KeyCode::Right);
    let (sheet, cursor) = (a.view.sheet, a.view.cursor);
    a.remember_spot();

    let mut b = App::open(&file).unwrap();
    b.restore_spot();
    assert_eq!((b.view.sheet, b.view.cursor), (sheet, cursor));

    // A cell beyond the shortened table: the row for new entries below it;
    // a table that is gone: the first table.
    std::fs::write(&file, DOC.replace("| y    | 20     |\n| z    | 30     |\n", "")).unwrap();
    let mut s = App::open(&file).unwrap();
    s.restore_spot();
    assert_eq!((s.view.sheet, s.view.cursor), (1, (1, 1)));
    std::fs::write(&file, DOC.replace("## Income ::", "## Earnings ::")).unwrap();
    let mut c = App::open(&file).unwrap();
    c.restore_spot();
    assert_eq!(c.view.sheet, 0);
    std::fs::remove_dir_all(&dir).unwrap();
}
