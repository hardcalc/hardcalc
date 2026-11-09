//! Soft animations: popups slide in, scrolling glides, tables turn like a
//! cube; every animation ends on exactly the frame drawn without it.

use hardcalc::tui::app::App;
use hardcalc::tui::ui;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Terminal;
use std::path::PathBuf;
use std::time::{Duration, Instant};

const BUDGET: &str = include_str!("../examples/budget.hcmd");

fn app(animated: bool) -> App {
    let mut a = App::new(PathBuf::from("budget.hcmd"), hardcalc::parse(BUDGET).unwrap());
    a.anim.enabled = animated;
    a
}

fn screen(term: &Terminal<TestBackend>) -> String {
    let buf = term.backend().buffer();
    (0..buf.area.height)
        .map(|y| (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Frames drawn until the animation ends (at most 2 s), and the last one.
fn frames(a: &mut App, term: &mut Terminal<TestBackend>) -> (Vec<String>, String) {
    let start = Instant::now();
    let mut out = Vec::new();
    loop {
        term.draw(|f| ui::draw(f, a)).unwrap();
        out.push(screen(term));
        if !a.anim.active() || start.elapsed() > Duration::from_secs(2) {
            break;
        }
        std::thread::sleep(Duration::from_millis(16));
    }
    let last = out.last().cloned().unwrap();
    (out, last)
}

/// Runs `keys` on an animated and a plain app: the animation shows frames
/// in between and ends on the plain frame. Returns the in-between frames.
fn check(keys: &[KeyEvent]) -> Vec<String> {
    check_at(60, keys)
}

fn check_at(width: u16, keys: &[KeyEvent]) -> Vec<String> {
    let mut plain = app(false);
    let mut smooth = app(true);
    let mut tp = Terminal::new(TestBackend::new(width, 20)).unwrap();
    let mut ts = Terminal::new(TestBackend::new(width, 20)).unwrap();
    tp.draw(|f| ui::draw(f, &mut plain)).unwrap();
    frames(&mut smooth, &mut ts);
    for k in keys {
        plain.on_key(*k);
        smooth.on_key(*k);
    }
    tp.draw(|f| ui::draw(f, &mut plain)).unwrap();
    let (all, last) = frames(&mut smooth, &mut ts);
    if std::env::var_os("HARDCALC_DUMP_FRAMES").is_some() {
        for f in &all {
            eprintln!("{f}\n{}", "-".repeat(width as usize));
        }
    }
    assert_eq!(last, screen(&tp), "the animation ends on the plain frame");
    assert!(all.len() > 2, "frames in between: {}", all.len());
    all
}

fn k(code: KeyCode, mods: KeyModifiers) -> KeyEvent {
    KeyEvent::new(code, mods)
}

#[test]
fn menu_slides_in_from_above() {
    let all = check(&[k(KeyCode::F(9), KeyModifiers::NONE)]);
    // The menu's lower border comes down line by line.
    let bottom = |s: &String| s.lines().collect::<Vec<_>>().iter().rposition(|l| l.contains('└')).unwrap_or(0);
    let rows: Vec<usize> = all.iter().map(bottom).collect();
    assert!(rows.windows(2).all(|w| w[0] <= w[1]), "{rows:?}");
    assert!(rows[0] < *rows.last().unwrap(), "{rows:?}");
}

#[test]
fn scrolling_sideways_glides_by_characters() {
    let right = k(KeyCode::Right, KeyModifiers::NONE);
    let all = check_at(32, &[right; 4]);
    let distinct: std::collections::HashSet<&String> = all.iter().collect();
    assert!(distinct.len() > 3, "several positions on the way: {}", distinct.len());
}

#[test]
fn switching_tables_turns_a_cube() {
    let all = check(&[k(KeyCode::PageDown, KeyModifiers::CONTROL)]);
    assert!(all.len() > 5);
}
