//! Interactive terminal UI.

pub mod anim;
pub mod app;
pub mod chart_image;
pub mod chart_view;
pub mod config;
pub mod file_dialog;
pub mod commands;
pub mod graphics;
pub mod help;
pub mod keymap;
pub mod output;
pub mod theme;
pub mod ui;

use app::App;
use ratatui::crossterm::event::{
    self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture, Event, KeyEventKind,
    KeyboardEnhancementFlags, MouseEventKind, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use ratatui::crossterm::execute;
use std::io::{stdout, Stdout, Write};
use std::time::{Duration, Instant};

/// The terminal, drawn in synchronized frames.
type Term = ratatui::Terminal<ratatui::backend::CrosstermBackend<output::Output<Stdout>>>;

/// Shortest time between animation frames (60 per second).
const FRAME: Duration = Duration::from_millis(16);
/// Longest time between animation frames (12 per second).
const SLOWEST_FRAME: Duration = Duration::from_millis(80);
/// Bytes per second animations send at most: big frames (large windows)
/// come less often, so the terminal keeps up and keys are not delayed.
const BYTES_PER_SECOND: f64 = 1_500_000.0;
use std::path::Path;

/// Asks the terminal whether its background is light (`None`: no answer).
pub(crate) fn detect_light() -> Option<bool> {
    let mut options = terminal_colorsaurus::QueryOptions::default();
    options.timeout = std::time::Duration::from_millis(300);
    terminal_colorsaurus::theme_mode(options).ok().map(|m| m == terminal_colorsaurus::ThemeMode::Light)
}

/// Runs the UI on `path`, or on a new untitled document.
pub fn run(path: Option<&Path>) -> std::io::Result<()> {
    let mut app = match path {
        Some(path) => {
            let app = App::open(path)?;
            if path.exists() {
                crate::recent::add(path);
            }
            app
        }
        None => App::untitled(),
    };
    if let Some(t) = std::env::var("HARDCALC_THEME").ok().and_then(|v| theme::ThemeSetting::parse(&v)) {
        app.theme = t;
    }
    // Asked before the UI takes over the terminal; again while idle, so an
    // automatic light/dark switch of the terminal is followed.
    app.detected_light = detect_light();
    let can_query = app.detected_light.is_some();
    // Raw mode, alternate screen and the panic hook; then a terminal that
    // draws in synchronized frames.
    drop(ratatui::init());
    let out = output::Output::new(stdout());
    let frame_bytes = out.counter();
    let mut terminal: Term = ratatui::Terminal::new(ratatui::backend::CrosstermBackend::new(out))?;
    execute!(stdout(), EnableMouseCapture, EnableBracketedPaste)?;

    // Inputs that only some terminals deliver: Menu key (kitty protocol), Shift+F10.
    let kitty = ratatui::crossterm::terminal::supports_keyboard_enhancement().unwrap_or(false);
    if kitty {
        execute!(stdout(), PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES))?;
    }
    app.caps = keymap::Caps::detect(|k| std::env::var(k).ok(), cfg!(target_os = "macos"), kitty);
    app.caps_path = keymap::caps_file(|k| std::env::var(k).ok());
    if let Some(text) = app.caps_path.as_ref().and_then(|p| std::fs::read_to_string(p).ok()) {
        app.caps.merge_lines(&text);
    }
    app.graphics = graphics::detect(|k| std::env::var(k).ok());
    app.truecolor = std::env::var("COLORTERM").is_ok_and(|v| v == "truecolor" || v == "24bit");
    app.anim.enabled = std::env::var("HARDCALC_ANIMATIONS").map_or(true, |v| v != "0" && v != "off");
    let mut images = ImageState::default();

    let result = (|| loop {
        frame_bytes.take();
        let animated = app.anim.active();
        let start = Instant::now();
        terminal.draw(|f| ui::draw(f, &mut app))?;
        let drawn = Instant::now();
        if animated && app.anim.record(drawn - start) {
            app.message = Some(app::Message { text: "animations off: frames take too long here (:anim on)".into(), error: false });
        }
        // Animation frames come in a steady beat: the next one is due a
        // fixed time after this one began (not after it was drawn, which
        // takes a different time for every frame). The time fits the budget.
        let pause = Duration::from_secs_f64(frame_bytes.take() as f64 / BYTES_PER_SECOND).clamp(FRAME, SLOWEST_FRAME);
        let until = |due: Instant| due.saturating_duration_since(Instant::now());
        if app.quit {
            // Switching off, keys are no longer read.
            if app.anim.close() {
                std::thread::sleep(until(start + pause));
                continue;
            }
            return Ok(());
        }
        if images.update(&mut app, &mut terminal)? {
            continue; // an error message needs a redraw
        }
        let animating = app.anim.active();
        let due = if animating {
            Some(start + pause)
        } else if app.anim.flowing() {
            Some(start + pause.max(anim::FLOW_FRAME))
        } else {
            None
        };
        if !event::poll(due.map_or(Duration::from_secs(3), until))? {
            if due.is_none() && can_query && app.theme == theme::ThemeSetting::Auto {
                let light = detect_light();
                if light.is_some() && light != app.detected_light {
                    app.detected_light = light;
                    terminal.clear()?;
                }
            }
            continue;
        }
        // Keys that are already waiting (key repeat, fast typing) are all
        // handled before the next frame, so the screen does not lag behind.
        // A mouse event ends the batch: the next one needs the new frame's
        // click regions. Not so the mouse merely moving, which changes nothing.
        let mut typed = false;
        for _ in 0..64 {
            match event::read()? {
                // Windows also reports key releases.
                Event::Key(k) if k.kind == KeyEventKind::Press => {
                    app.on_key(k);
                    typed = true;
                }
                Event::Mouse(m) => {
                    app.on_mouse(m);
                    if m.kind != MouseEventKind::Moved {
                        break;
                    }
                }
                Event::Paste(text) => {
                    app.on_paste(&text);
                    typed = true;
                }
                _ => {}
            }
            if app.quit || !event::poll(Duration::ZERO)? {
                break;
            }
        }
        // While an animation runs, events are handled at once but the frame
        // keeps to the beat (and to the budget). Typing while only dots flow
        // is drawn at once.
        if let Some(due) = due
            && !app.quit
            && (animating || !typed)
        {
            std::thread::sleep(until(due));
        }
    })();

    if kitty {
        let _ = execute!(stdout(), PopKeyboardEnhancementFlags);
    }
    let _ = execute!(stdout(), DisableMouseCapture, DisableBracketedPaste);
    ratatui::restore();
    result
}

/// The images on screen – the page of the print preview, charts – so they
/// are only sent when something changes.
#[derive(Default)]
struct ImageState {
    /// Hash of everything shown (page PDF, charts, their places).
    shown: Option<u64>,
    /// Where images are on screen.
    rects: Vec<ratatui::layout::Rect>,
    /// Last rasterized page: hash of its PDF and the PNG.
    cache: Option<(u64, Vec<u8>)>,
    /// The PNGs of the charts on screen, by what they show.
    charts: std::collections::HashMap<u64, Vec<u8>>,
}

/// Pixels of a terminal cell; a usual size where the terminal does not say.
fn cell_pixels() -> (u16, u16) {
    match ratatui::crossterm::terminal::window_size() {
        Ok(w) if w.width > 0 && w.height > 0 && w.columns > 0 && w.rows > 0 => ((w.width / w.columns).max(1), (w.height / w.rows).max(1)),
        _ => (10, 20),
    }
}

impl ImageState {
    /// Shows, replaces or removes the images after a frame was drawn.
    /// Returns true when the frame must be redrawn.
    fn update(&mut self, app: &mut App, terminal: &mut Term) -> std::io::Result<bool> {
        use std::hash::{Hash, Hasher};
        let Some(protocol) = app.graphics else { return Ok(false) };
        let page = match (app.image_slot, app.preview_page_pdf()) {
            (Some(slot), Some(pdf)) => Some((slot, pdf)),
            _ => None,
        };
        if page.is_none() && app.images.is_empty() {
            if self.shown.take().is_some() {
                write!(stdout(), "{}", graphics::clear(protocol))?;
                self.rects.clear();
                // Repaint everything so no image remains under the new content.
                terminal.clear()?;
                return Ok(true);
            }
            return Ok(false);
        }
        let cell = cell_pixels();
        let mut h = std::collections::hash_map::DefaultHasher::new();
        let mut pdf_hash = 0;
        if let Some((slot, pdf)) = &page {
            pdf.hash(&mut h);
            pdf_hash = h.finish();
            (slot.x, slot.y, slot.width, slot.height).hash(&mut h);
        }
        let charts: Vec<(u64, &chart_image::ChartImage)> = app.images.iter().map(|i| (i.key() ^ ((cell.0 as u64) << 48 | (cell.1 as u64) << 32), i)).collect();
        for (k, _) in &charts {
            k.hash(&mut h);
        }
        let key = h.finish();
        if self.shown == Some(key) {
            return Ok(false);
        }
        let rects: Vec<ratatui::layout::Rect> = page.iter().map(|(s, _)| *s).chain(charts.iter().map(|(_, i)| i.rect)).collect();
        // iTerm2 images are part of the cells: one that moved away is only
        // gone once its cells are written again.
        if protocol == graphics::Protocol::Iterm && self.rects.iter().any(|r| !rects.contains(r)) {
            self.shown = None;
            self.rects.clear();
            terminal.clear()?;
            return Ok(true);
        }
        let mut pngs: Vec<(ratatui::layout::Rect, Vec<u8>)> = Vec::new();
        if let Some((slot, pdf)) = &page {
            let png = match &self.cache {
                Some((k, png)) if *k == pdf_hash => png.clone(),
                _ => match graphics::rasterize(pdf, 1600) {
                    Ok(png) => {
                        self.cache = Some((pdf_hash, png.clone()));
                        png
                    }
                    Err(e) => {
                        app.image_mode = false;
                        app.message = Some(app::Message { text: e, error: true });
                        return Ok(true);
                    }
                },
            };
            pngs.push((*slot, png));
        }
        let wanted: Vec<u64> = charts.iter().map(|(k, _)| *k).collect();
        self.charts.retain(|k, _| wanted.contains(k));
        for (k, img) in &charts {
            let png = self.charts.entry(*k).or_insert_with(|| chart_image::plot(&img.data, img.rect.width, img.rect.height, cell, img.paper).png());
            pngs.push((img.rect, png.clone()));
        }
        let mut out = stdout();
        write!(out, "{}", graphics::clear(protocol))?;
        for (rect, png) in &pngs {
            execute!(out, ratatui::crossterm::cursor::MoveTo(rect.x, rect.y))?;
            write!(out, "{}", graphics::escape(protocol, png, rect.width, rect.height))?;
        }
        out.flush()?;
        self.shown = Some(key);
        self.rects = rects;
        Ok(false)
    }
}
