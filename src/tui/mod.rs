//! Interactive terminal UI.

pub mod anim;
pub mod app;
pub mod chart_view;
pub mod config;
pub mod file_dialog;
pub mod commands;
pub mod graphics;
pub mod help;
pub mod keymap;
pub mod theme;
pub mod ui;

use app::App;
use ratatui::crossterm::event::{
    self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture, Event, KeyEventKind,
    KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use ratatui::crossterm::execute;
use std::io::{stdout, Write};
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
        Some(path) => App::open(path)?,
        None => App::untitled(),
    };
    if let Some(t) = std::env::var("HARDCALC_THEME").ok().and_then(|v| theme::ThemeSetting::parse(&v)) {
        app.theme = t;
    }
    // Asked before the UI takes over the terminal; again while idle, so an
    // automatic light/dark switch of the terminal is followed.
    app.detected_light = detect_light();
    let can_query = app.detected_light.is_some();
    let mut terminal = ratatui::init();
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
        terminal.draw(|f| ui::draw(f, &mut app))?;
        if app.quit {
            return Ok(());
        }
        if images.update(&mut app, &mut terminal)? {
            continue; // an error message needs a redraw
        }
        // While an animation runs, the next frame follows after ~16 ms.
        let animating = app.anim.active();
        let wait = if animating { std::time::Duration::from_millis(16) } else { std::time::Duration::from_secs(3) };
        if !event::poll(wait)? {
            if !animating && can_query && app.theme == theme::ThemeSetting::Auto {
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
        // click regions.
        for _ in 0..64 {
            match event::read()? {
                // Windows also reports key releases.
                Event::Key(k) if k.kind == KeyEventKind::Press => app.on_key(k),
                Event::Mouse(m) => {
                    app.on_mouse(m);
                    break;
                }
                Event::Paste(text) => app.on_paste(&text),
                _ => {}
            }
            if app.quit || !event::poll(std::time::Duration::ZERO)? {
                break;
            }
        }
    })();

    if kitty {
        let _ = execute!(stdout(), PopKeyboardEnhancementFlags);
    }
    let _ = execute!(stdout(), DisableMouseCapture, DisableBracketedPaste);
    ratatui::restore();
    result
}

/// The page image currently on screen, so it is only sent when it changes.
#[derive(Default)]
struct ImageState {
    /// Hash of (page PDF, slot) of the image on screen.
    shown: Option<u64>,
    /// Last rasterized page: hash of its PDF and the PNG.
    cache: Option<(u64, Vec<u8>)>,
}

impl ImageState {
    /// Shows, replaces or removes the preview image after a frame was drawn.
    /// Returns true when the frame must be redrawn.
    fn update(&mut self, app: &mut App, terminal: &mut ratatui::DefaultTerminal) -> std::io::Result<bool> {
        use std::hash::{Hash, Hasher};
        let (Some(protocol), Some(slot), Some(pdf)) = (app.graphics, app.image_slot, app.preview_page_pdf()) else {
            if self.shown.take().is_some() {
                if let Some(p) = app.graphics {
                    write!(stdout(), "{}", graphics::clear(p))?;
                }
                // Repaint everything so no image remains under the new content.
                terminal.clear()?;
                return Ok(true);
            }
            return Ok(false);
        };
        let mut h = std::collections::hash_map::DefaultHasher::new();
        pdf.hash(&mut h);
        let pdf_hash = h.finish();
        (slot.x, slot.y, slot.width, slot.height).hash(&mut h);
        let key = h.finish();
        if self.shown == Some(key) {
            return Ok(false);
        }
        let png = match &self.cache {
            Some((k, png)) if *k == pdf_hash => png.clone(),
            _ => match graphics::rasterize(&pdf, 1600) {
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
        let mut out = stdout();
        write!(out, "{}", graphics::clear(protocol))?;
        execute!(out, ratatui::crossterm::cursor::MoveTo(slot.x, slot.y))?;
        write!(out, "{}", graphics::escape(protocol, &png, slot.width, slot.height))?;
        out.flush()?;
        self.shown = Some(key);
        Ok(false)
    }
}
