//! Light and dark terminals. The UI is drawn with colours meant for a dark
//! background; for a light one, [`apply`] adjusts the finished frame: text
//! colours on the terminal's own background become darker variants, and
//! default-coloured text on coloured areas gets a contrasting colour.

use ratatui::buffer::Buffer;
use ratatui::style::Color;

/// How the theme is chosen.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ThemeSetting {
    /// Follow the terminal's background (queried at start and while idle).
    Auto,
    Light,
    Dark,
}

impl ThemeSetting {
    pub fn parse(s: &str) -> Option<ThemeSetting> {
        match s.to_lowercase().as_str() {
            "auto" => Some(ThemeSetting::Auto),
            "light" | "hell" => Some(ThemeSetting::Light),
            "dark" | "dunkel" => Some(ThemeSetting::Dark),
            _ => None,
        }
    }
}

/// Approximate RGB of a colour (`None` for the terminal's default).
fn rgb(c: Color) -> Option<(u8, u8, u8)> {
    Some(match c {
        Color::Reset => return None,
        Color::Black => (0, 0, 0),
        Color::Red => (205, 0, 0),
        Color::Green => (0, 205, 0),
        Color::Yellow => (205, 205, 0),
        Color::Blue => (0, 0, 238),
        Color::Magenta => (205, 0, 205),
        Color::Cyan => (0, 205, 205),
        Color::Gray => (229, 229, 229),
        Color::DarkGray => (127, 127, 127),
        Color::LightRed => (255, 85, 85),
        Color::LightGreen => (85, 255, 85),
        Color::LightYellow => (255, 255, 85),
        Color::LightBlue => (92, 92, 255),
        Color::LightMagenta => (255, 85, 255),
        Color::LightCyan => (85, 255, 255),
        Color::White => (255, 255, 255),
        Color::Rgb(r, g, b) => (r, g, b),
        Color::Indexed(i) => indexed(i),
    })
}

/// xterm 256-colour palette.
fn indexed(i: u8) -> (u8, u8, u8) {
    match i {
        0..=15 => {
            const BASE: [(u8, u8, u8); 16] = [
                (0, 0, 0), (205, 0, 0), (0, 205, 0), (205, 205, 0), (0, 0, 238), (205, 0, 205), (0, 205, 205), (229, 229, 229),
                (127, 127, 127), (255, 0, 0), (0, 255, 0), (255, 255, 0), (92, 92, 255), (255, 0, 255), (0, 255, 255), (255, 255, 255),
            ];
            BASE[i as usize]
        }
        16..=231 => {
            const LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];
            let n = i - 16;
            (LEVELS[(n / 36) as usize], LEVELS[(n / 6 % 6) as usize], LEVELS[(n % 6) as usize])
        }
        _ => {
            let v = 8 + 10 * (i - 232);
            (v, v, v)
        }
    }
}

/// Relative brightness 0 … 1.
pub fn luminance(c: Color) -> Option<f32> {
    let (r, g, b) = rgb(c)?;
    Some((0.2126 * r as f32 + 0.7152 * g as f32 + 0.0722 * b as f32) / 255.0)
}

/// A text colour that stays readable on a light background.
pub fn for_light_background(c: Color) -> Color {
    match c {
        // Secondary text stays darker than "dim" text, as on dark terminals.
        Color::Gray => Color::Indexed(240),
        Color::DarkGray => Color::Indexed(247),
        Color::White => Color::Black,
        Color::Yellow | Color::LightYellow => Color::Indexed(136),
        Color::Cyan | Color::LightCyan => Color::Indexed(30),
        Color::Green | Color::LightGreen => Color::Indexed(28),
        Color::Magenta | Color::LightMagenta => Color::Indexed(127),
        Color::Red | Color::LightRed => Color::Indexed(160),
        Color::Blue | Color::LightBlue => Color::Indexed(25),
        other => match rgb(other) {
            // Bright custom colours (e.g. `fg #ffee00`) are darkened.
            Some((r, g, b)) if luminance(other).unwrap_or(0.0) > 0.55 => {
                let d = |v: u8| (v as f32 * 0.55) as u8;
                Color::Rgb(d(r), d(g), d(b))
            }
            _ => other,
        },
    }
}

/// Light counterparts of the dark surfaces the UI is drawn with (bars,
/// desk around the paper, highlighted source block).
fn light_surface(bg: Color) -> Option<Color> {
    match bg {
        Color::Black => Some(Color::Indexed(254)),
        Color::DarkGray => Some(Color::Indexed(251)),
        Color::Rgb(40, 44, 60) => Some(Color::Rgb(222, 228, 242)),
        _ => None,
    }
}

/// Banded cell backgrounds: odd rows and odd columns get a slightly
/// different shade, both together a little more. Subtle greys, also
/// visible with the 256-colour palette. `None`: the sheet background.
pub fn band(row_odd: bool, col_odd: bool, light: bool, truecolor: bool) -> Option<Color> {
    let steps = usize::from(row_odd) * 2 + usize::from(col_odd);
    if steps == 0 {
        return None;
    }
    // [col only, row only, both]
    const DARK: [Color; 3] = [Color::Rgb(44, 44, 50), Color::Rgb(54, 54, 61), Color::Rgb(66, 66, 74)];
    const LIGHT: [Color; 3] = [Color::Rgb(240, 240, 245), Color::Rgb(230, 230, 237), Color::Rgb(218, 218, 228)];
    const DARK_256: [Color; 3] = [Color::Indexed(236), Color::Indexed(237), Color::Indexed(238)];
    const LIGHT_256: [Color; 3] = [Color::Indexed(255), Color::Indexed(254), Color::Indexed(253)];
    let set = match (light, truecolor) {
        (false, true) => DARK,
        (true, true) => LIGHT,
        (false, false) => DARK_256,
        (true, false) => LIGHT_256,
    };
    Some(set[steps - 1])
}

/// Whether `c` is one of the band shades (treated like the sheet background).
fn is_band(c: Color) -> bool {
    [(false, true), (true, false), (true, true)]
        .into_iter()
        .flat_map(|(r, c)| [(r, c, false, false), (r, c, false, true), (r, c, true, false), (r, c, true, true)])
        .any(|(r, col, l, t)| band(r, col, l, t) == Some(c))
}

/// Sheet background and text colour when the theme is chosen explicitly.
pub const DARK_PAPER: Color = Color::Rgb(28, 28, 30);
pub const DARK_INK: Color = Color::Rgb(220, 220, 220);
pub const LIGHT_PAPER: Color = Color::Rgb(255, 255, 255);
pub const LIGHT_INK: Color = Color::Rgb(20, 20, 20);

/// Adjusts a finished frame to the theme. With `explicit` (theme chosen with
/// `:theme light|dark` rather than taken from the terminal) the whole screen,
/// sheet included, gets the theme's own background and text colour instead of
/// the terminal's.
pub fn apply(buf: &mut Buffer, light: bool, explicit: bool) {
    for cell in buf.content.iter_mut() {
        adjust(cell, light);
        if explicit {
            if cell.bg == Color::Reset {
                cell.bg = if light { LIGHT_PAPER } else { DARK_PAPER };
            }
            if cell.fg == Color::Reset {
                cell.fg = if light { LIGHT_INK } else { DARK_INK };
            }
        }
    }
}

/// Adapts one cell's colours to a light or dark theme.
fn adjust(cell: &mut ratatui::buffer::Cell, light: bool) {
    // Dark surfaces turn light; their text is then treated like text on
    // the terminal's own background.
    // Bands are part of the sheet: text on them is adapted like on the sheet.
    if is_band(cell.bg) {
        if light {
            cell.fg = for_light_background(cell.fg);
        }
        return;
    }
    if light && let Some(surface) = light_surface(cell.bg) {
        cell.bg = surface;
        cell.fg = for_light_background(cell.fg);
        return;
    }
    match (cell.bg, cell.fg) {
        // Text on the terminal's own background.
        (Color::Reset, fg) if light => cell.fg = for_light_background(fg),
        (Color::Reset, _) => {}
        // Default text colour on a coloured area: make it contrast with that area.
        (bg, Color::Reset) => match luminance(bg) {
            Some(l) if light && l < 0.45 => cell.fg = Color::White,
            Some(l) if !light && l > 0.6 => cell.fg = Color::Black,
            _ => {}
        },
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::layout::Rect;
    use ratatui::style::Style;

    #[test]
    fn light_background_darkens_text_only_on_the_default_background() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 1));
        buf.set_string(0, 0, "a", Style::new().fg(Color::Yellow));
        buf.set_string(1, 0, "b", Style::new().fg(Color::Yellow).bg(Color::Blue));
        buf.set_string(2, 0, "c", Style::new().bg(Color::Red));
        buf.set_string(3, 0, "d", Style::new().fg(Color::Rgb(255, 238, 0)));
        apply(&mut buf, true, false);
        assert_eq!(buf[(0, 0)].fg, Color::Indexed(136));
        assert_eq!(buf[(1, 0)].fg, Color::Yellow, "explicit background: unchanged");
        assert_eq!(buf[(2, 0)].fg, Color::White, "default text on a dark accent");
        assert!(matches!(buf[(3, 0)].fg, Color::Rgb(r, _, _) if r < 160));

        let mut dark = Buffer::empty(Rect::new(0, 0, 2, 1));
        dark.set_string(0, 0, "a", Style::new().fg(Color::Yellow));
        dark.set_string(1, 0, "b", Style::new().bg(Color::Rgb(220, 220, 220)));
        apply(&mut dark, false, false);
        assert_eq!(dark[(0, 0)].fg, Color::Yellow, "dark theme keeps the colours");
        assert_eq!(dark[(1, 0)].fg, Color::Black, "default text on a light area");
    }

    #[test]
    fn dark_surfaces_turn_light() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 3, 1));
        buf.set_string(0, 0, "a", Style::new().fg(Color::White).bg(Color::DarkGray));
        buf.set_string(1, 0, "b", Style::new().fg(Color::Gray).bg(Color::Black));
        buf.set_string(2, 0, "c", Style::new().bg(Color::Rgb(40, 44, 60)));
        apply(&mut buf, true, false);
        assert_eq!((buf[(0, 0)].bg, buf[(0, 0)].fg), (Color::Indexed(251), Color::Black));
        assert_eq!((buf[(1, 0)].bg, buf[(1, 0)].fg), (Color::Indexed(254), Color::Indexed(240)));
        assert_eq!(buf[(2, 0)].bg, Color::Rgb(222, 228, 242));
        let mut dark = buf.clone();
        dark.set_string(0, 0, "a", Style::new().fg(Color::White).bg(Color::DarkGray));
        apply(&mut dark, false, false);
        assert_eq!(dark[(0, 0)].bg, Color::DarkGray, "unchanged on dark terminals");
    }

    #[test]
    fn explicit_theme_paints_the_whole_screen() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 2, 1));
        buf.set_string(0, 0, "a", Style::new());
        buf.set_string(1, 0, "b", Style::new().fg(Color::Yellow).bg(Color::Blue));
        let mut dark = buf.clone();
        apply(&mut dark, false, true);
        assert_eq!((dark[(0, 0)].bg, dark[(0, 0)].fg), (DARK_PAPER, DARK_INK));
        assert_eq!((dark[(1, 0)].bg, dark[(1, 0)].fg), (Color::Blue, Color::Yellow), "explicit colours stay");
        apply(&mut buf, true, true);
        assert_eq!((buf[(0, 0)].bg, buf[(0, 0)].fg), (LIGHT_PAPER, LIGHT_INK));
    }

    #[test]
    fn luminance_and_palette() {
        assert!(luminance(Color::White).unwrap() > 0.99);
        assert!(luminance(Color::Indexed(16)).unwrap() < 0.01);
        assert_eq!(indexed(231), (255, 255, 255));
        assert_eq!(indexed(244), (128, 128, 128));
        assert_eq!(ThemeSetting::parse("Hell"), Some(ThemeSetting::Light));
    }
}
