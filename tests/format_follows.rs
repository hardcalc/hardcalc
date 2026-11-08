//! Format rules (colors, bold …) and references move with their cells when
//! rows and columns are inserted, deleted or sorted.

use hardcalc::tui::app::App;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::path::PathBuf;

const SRC: &str = "## T :: matrix\n\n| Name | V | W |\n|---|---|---|\n| a | 3 | x |\n| b | 1 | y |\n| c | 2 | z |\n| d | 0 | w |\n\n";

fn app(layer: &str) -> App {
    let src = format!("{SRC}```layer format\n{layer}\n```\n");
    App::new(PathBuf::from("t.hcmd"), hardcalc::parse(&src).unwrap())
}

fn cmd(a: &mut App, text: &str) {
    for ch in text.chars() {
        a.on_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
    }
    a.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
}

/// Which cells have a background, one line per row: `#` or `.`.
fn colored(a: &App) -> String {
    let doc = a.doc.clone();
    let (mut wb, _) = hardcalc::Workbook::from_document(&doc);
    let att = hardcalc::layers::attach(&doc, &wb);
    let formats = hardcalc::layers::Formats::new(&att, "format");
    let (rows, cols) = (wb.sheets[0].cells.len(), wb.sheets[0].columns.len());
    (0..rows)
        .map(|r| (0..cols).map(|c| if formats.style(&mut wb, (0, r, c)).bg.is_some() { '#' } else { '.' }).collect::<String>())
        .collect::<Vec<_>>()
        .join("/")
}

fn rules(a: &App) -> String {
    let text = hardcalc::write(&a.doc);
    text.split("```layer format\n").nth(1).map(|r| r.split("```").next().unwrap().trim().replace('\n', " | ")).unwrap_or_default()
}

#[test]
fn inserting_moves_the_rules() {
    let mut a = app("A3: bg red\nB2:C3: bg blue\n4: bg green");
    assert_eq!(colored(&a), ".../.##/###/###");
    cmd(&mut a, ":A1");
    cmd(&mut a, ":addrow");
    assert_eq!(rules(&a), "A4: bg red | B3:C4: bg blue | 5: bg green");
    assert_eq!(colored(&a), ".../.../.##/###/###");
    cmd(&mut a, ":A1");
    cmd(&mut a, ":addcol X");
    assert_eq!(rules(&a), "A4: bg red | C3:D4: bg blue | 5: bg green");
}

#[test]
fn deleting_a_row_moves_the_rules_below_it() {
    let mut a = app("A3: bg red\nB2:C3: bg blue\n4: bg green");
    cmd(&mut a, ":A1");
    cmd(&mut a, ":delrow");
    // c is the second row now, d the third.
    assert_eq!(rules(&a), "A2: bg red | B1:C2: bg blue | 3: bg green");
    assert_eq!(colored(&a), ".##/###/###");
}

#[test]
fn rules_of_a_deleted_row_go() {
    let mut a = app("A3: bg red\n3: bold\nB2:B3: bg blue\nA2:A3: bg green");
    cmd(&mut a, ":A3");
    cmd(&mut a, ":delrow");
    assert_eq!(rules(&a), "B2: bg blue | A2: bg green");
}

#[test]
fn a_deleted_range_goes_and_a_hit_range_shrinks() {
    let mut a = app("B2:B4: bg blue\nA2:A3: bg green");
    cmd(&mut a, ":A2");
    cmd(&mut a, ":delrow");
    assert_eq!(rules(&a), "B2:B3: bg blue | A2: bg green");
    cmd(&mut a, ":A2");
    cmd(&mut a, ":delrow");
    assert_eq!(rules(&a), "B2: bg blue");
}

#[test]
fn deleting_a_column_moves_the_rules_to_its_right() {
    let mut a = app("B2: bg red\nC1:C2: bg blue\nW: bold\nV: fg green\nA: bg yellow");
    cmd(&mut a, ":B1");
    cmd(&mut a, ":delcol");
    // V is gone with its rules; W is the second column now.
    assert_eq!(rules(&a), "B1:B2: bg blue | W: bold | A: bg yellow");
    assert_eq!(colored(&a), "##/##/#./#.");
}

#[test]
fn deleting_moves_references_in_formulas() {
    let src = "## T :: matrix\n\n| N | V | W |\n|---|---|---|\n| a | 3 | =B3*2 |\n| b | 1 | =B1+B2 |\n| c | 2 | =sum(B1:B3) |\n| d | 0 | =sum(B2:B3) |\n";
    let mut a = App::new(PathBuf::from("t.hcmd"), hardcalc::parse(src).unwrap());
    cmd(&mut a, ":A1");
    cmd(&mut a, ":delrow");
    let text = hardcalc::write(&a.doc);
    assert!(text.contains("| b | 1 | =#REF!+B1 |"), "{text}");
    assert!(text.contains("| c | 2 | =sum(B1:B2) |"), "a range shrinks\n{text}");
    assert!(text.contains("| d | 0 | =sum(B1:B2) |"), "{text}");
}

#[test]
fn deleting_a_column_moves_column_references() {
    let src = "## T :: matrix\n\n| N | V | W |\n|---|---|---|\n| a | 3 | =C1*2 |\n| b | 1 | =B2+3 |\n";
    let mut a = App::new(PathBuf::from("t.hcmd"), hardcalc::parse(src).unwrap());
    cmd(&mut a, ":B1");
    cmd(&mut a, ":delcol");
    let text = hardcalc::write(&a.doc);
    assert!(text.contains("| a | =B1*2 |"), "{text}");
    assert!(text.contains("| b | =#REF!+3 |"), "a reference to the deleted column\n{text}");
}

#[test]
fn sorting_takes_the_rules_along() {
    let mut a = app("A1: bg red");
    cmd(&mut a, ":sort V");
    // a (V = 3) is the last row after the sort.
    assert_eq!(rules(&a), "A4: bg red");
    assert_eq!(colored(&a), ".../.../.../#..");
}
