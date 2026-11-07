//! hardconf: terminal editor for TOML files with a built-in schema (CONFIG-SPEC.md).

pub mod app;
pub mod ui;

use app::ConfApp;
use ratatui::crossterm::event::{
    self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture, Event, KeyEventKind,
};
use ratatui::crossterm::execute;
use std::io::stdout;
use std::path::{Path, PathBuf};

/// Runs the editor on `path`, or on a file chosen in the file dialog.
/// Quitting without saving leaves the file untouched.
pub fn run(path: Option<&Path>) -> std::io::Result<()> {
    let path = match path {
        Some(p) => p.to_path_buf(),
        None => match pick()? {
            Some(p) => p,
            None => return Ok(()),
        },
    };
    let mut app = ConfApp::open(&path)?;
    let theme = std::env::var("HARDCONF_THEME").or_else(|_| std::env::var("HARDCALC_THEME")).ok();
    if let Some(t) = theme.and_then(|v| super::theme::ThemeSetting::parse(&v)) {
        app.theme = t;
    }
    app.detected_light = super::detect_light();
    let can_query = app.detected_light.is_some();
    let mut terminal = ratatui::init();
    execute!(stdout(), EnableMouseCapture, EnableBracketedPaste)?;

    let result = (|| loop {
        terminal.draw(|f| ui::draw(f, &mut app))?;
        if let Some(text) = app.to_system_clipboard.take() {
            // OSC 52: the terminal puts the text on the system clipboard (if it supports it).
            use std::io::Write;
            let mut out = stdout();
            write!(out, "\x1b]52;c;{}\x07", base64(text.as_bytes()))?;
            out.flush()?;
        }
        if app.quit {
            return Ok(());
        }
        if !event::poll(std::time::Duration::from_secs(3))? {
            if can_query && app.theme == super::theme::ThemeSetting::Auto {
                let light = super::detect_light();
                if light.is_some() && light != app.detected_light {
                    app.detected_light = light;
                    terminal.clear()?;
                }
            }
            continue;
        }
        match event::read()? {
            Event::Key(k) if k.kind == KeyEventKind::Press => app.on_key(k),
            Event::Mouse(m) => app.on_mouse(m),
            Event::Paste(text) => app.on_paste(&text),
            _ => {}
        }
    })();

    let _ = execute!(stdout(), DisableMouseCapture, DisableBracketedPaste);
    ratatui::restore();
    result
}

/// Standard base64, for OSC 52.
fn base64(data: &[u8]) -> String {
    const ABC: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ABC[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// The file dialog on its own, in the current folder: the file to open, or
/// `None` when it is closed.
fn pick() -> std::io::Result<Option<PathBuf>> {
    use super::file_dialog::{Action, FileDialog, Purpose};
    use ratatui::style::{Color, Style};
    let dir = std::env::current_dir()?;
    let mut dialog = FileDialog::new(Purpose::Open, dir, app::is_toml).with_tag(app::format_tag);
    let mut error: Option<String> = None;
    let mut terminal = ratatui::init();
    let result = (|| loop {
        terminal.draw(|f| {
            let area = f.area();
            f.buffer_mut().set_stringn(0, 0, " hardconf – choose a TOML file", area.width as usize, Style::new().fg(Color::Cyan));
            if let Some(e) = &error {
                let y = area.height.saturating_sub(1);
                f.buffer_mut().set_stringn(0, y, format!(" {e}"), area.width as usize, Style::new().fg(Color::LightRed));
            }
            dialog.draw(f, " open file ", "TOML files");
        })?;
        if let Event::Key(k) = event::read()?
            && k.kind == KeyEventKind::Press
        {
            match dialog.key(k) {
                Action::Close => return Ok(None),
                // Only files hardconf can read end the dialog.
                Action::Chosen(p) => match crate::config::ConfigFile::load(&p) {
                    Ok(_) => return Ok(Some(p)),
                    Err(e) => error = Some(format!("{}: {e}", p.display())),
                },
                Action::None => {}
            }
        }
    })();
    ratatui::restore();
    result
}

#[cfg(test)]
mod tests {
    #[test]
    fn base64() {
        assert_eq!(super::base64(b""), "");
        assert_eq!(super::base64(b"f"), "Zg==");
        assert_eq!(super::base64(b"fo"), "Zm8=");
        assert_eq!(super::base64(b"foo"), "Zm9v");
        assert_eq!(super::base64("Café".as_bytes()), "Q2Fmw6k=");
    }
}
