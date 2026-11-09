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
fn cursor_moves_and_scrolling_draw_at_once() {
    let mut a = app(true);
    let mut t = Terminal::new(TestBackend::new(32, 20)).unwrap();
    frames(&mut a, &mut t); // switching on
    // Moving right scrolls this narrow screen.
    for code in [KeyCode::Down, KeyCode::Right, KeyCode::Right, KeyCode::Right, KeyCode::Right, KeyCode::PageDown] {
        a.on_key(k(code, KeyModifiers::NONE));
        t.draw(|f| ui::draw(f, &mut a)).unwrap();
        assert!(!a.anim.active(), "{code:?} animates");
    }
}

#[test]
fn formula_sources_are_highlighted_and_data_flows() {
    use ratatui::style::Color;
    for animated in [true, false] {
        let mut a = app(animated);
        let mut t = Terminal::new(TestBackend::new(80, 20)).unwrap();
        frames(&mut a, &mut t);
        // A free cell below the table: =sum(B1:B3) + D1
        for _ in 0..5 {
            a.on_key(k(KeyCode::Down, KeyModifiers::NONE));
        }
        for _ in 0..4 {
            a.on_key(k(KeyCode::Right, KeyModifiers::NONE));
        }
        for ch in "=sum(B1:B3) + D1".chars() {
            a.on_key(k(KeyCode::Char(ch), KeyModifiers::NONE));
        }
        t.draw(|f| ui::draw(f, &mut a)).unwrap();
        let buf = t.backend().buffer().clone();
        let count = |bg: Color| (0..20).flat_map(|y| (0..80).map(move |x| (x, y))).filter(|&p| buf[p].bg == bg).count();
        assert!(count(Color::LightBlue) >= 3, "B1:B3 highlighted");
        assert!(count(Color::LightMagenta) >= 1, "D1 highlighted");
        // Dots flow only with animations; they keep running while editing.
        assert_eq!(a.anim.flowing(), animated);
        let dots = (0..20).flat_map(|y| (0..80).map(move |x| (x, y))).filter(|&p| buf[p].symbol() == "●").count();
        assert_eq!(dots > 0, animated, "dots: {dots}");
        // After Esc nothing is highlighted and nothing flows.
        a.on_key(k(KeyCode::Esc, KeyModifiers::NONE));
        t.draw(|f| ui::draw(f, &mut a)).unwrap();
        assert!(!a.anim.flowing());
        let buf = t.backend().buffer();
        assert!((0..20).flat_map(|y| (0..80).map(move |x| (x, y))).all(|p| buf[p].bg != Color::LightMagenta));
    }
}

#[test]
fn switching_tables_turns_a_cube() {
    let all = check(&[k(KeyCode::PageDown, KeyModifiers::CONTROL)]);
    assert!(all.len() > 5);
}

#[test]
fn animations_command_switches_them_off_and_on() {
    let mut a = app(true);
    let mut t = Terminal::new(TestBackend::new(60, 20)).unwrap();
    let run = |a: &mut App, cmd: &str| {
        a.on_key(k(KeyCode::Char(':'), KeyModifiers::NONE));
        for ch in cmd.chars() {
            a.on_key(k(KeyCode::Char(ch), KeyModifiers::NONE));
        }
        a.on_key(k(KeyCode::Enter, KeyModifiers::NONE));
    };
    run(&mut a, "animations off");
    assert!(!a.anim.enabled);
    t.draw(|f| ui::draw(f, &mut a)).unwrap();
    a.on_key(k(KeyCode::F(9), KeyModifiers::NONE));
    t.draw(|f| ui::draw(f, &mut a)).unwrap();
    assert!(!a.anim.active(), "the menu opens at once");
    a.on_key(k(KeyCode::Esc, KeyModifiers::NONE));
    // Without an argument it toggles; the short form works too.
    run(&mut a, "anim");
    assert!(a.anim.enabled);
    run(&mut a, "animations");
    assert!(!a.anim.enabled);
    run(&mut a, "animations on");
    assert!(a.anim.enabled);
}
