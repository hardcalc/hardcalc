//! Terminal rendering of charts – on screen and on the paper of the print preview.

use crate::chart::{ChartData, ChartType};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::symbols::Marker;
use ratatui::text::Span;
use ratatui::widgets::{Axis, Chart, Dataset, GraphType, Widget};
use unicode_width::UnicodeWidthStr;

const SCREEN: [Color; 6] = [Color::Cyan, Color::Yellow, Color::Magenta, Color::Green, Color::LightBlue, Color::LightRed];
/// Darker series colors for white paper (same order as on screen).
pub const PAPER: [(u8, u8, u8); 6] = [(0, 90, 170), (200, 110, 0), (150, 0, 150), (0, 130, 60), (60, 60, 200), (190, 0, 0)];

fn series_color(i: usize, paper: bool) -> Color {
    if paper {
        let (r, g, b) = PAPER[i % PAPER.len()];
        Color::Rgb(r, g, b)
    } else {
        SCREEN[i % SCREEN.len()]
    }
}

fn truncate(s: &str, w: usize) -> String {
    if s.width() <= w {
        return s.to_string();
    }
    let mut out: String = s.chars().take(w.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// Draws `data` into `area`. `highlight` marks a category (linked to the table
/// cursor). Returns clickable regions per category (bar charts).
pub fn render(buf: &mut Buffer, area: Rect, data: &ChartData, paper: bool, highlight: Option<usize>) -> Vec<(Rect, usize)> {
    if area.height < 3 || area.width < 10 {
        return Vec::new();
    }
    let base = if paper { Style::new().fg(Color::Black).bg(Color::White) } else { Style::new() };
    buf.set_style(area, base);
    let title = format!("{} ({})", data.title, data.kind.name());
    buf.set_stringn(area.x, area.y, truncate(&title, area.width as usize), area.width as usize, base.bold());

    // Legend on the title line when there are several series.
    if data.series.len() > 1 {
        let mut x = area.right();
        for (i, s) in data.series.iter().enumerate().rev() {
            let item = format!(" ■ {}", s.name);
            let w = item.width() as u16;
            if x < area.x + title.width() as u16 + w + 1 {
                break;
            }
            x -= w;
            buf.set_string(x, area.y, &item, base.fg(series_color(i, paper)));
        }
    }
    let plot = Rect { y: area.y + 1, height: area.height - 1, ..area };
    if data.categories.is_empty() || data.series.iter().all(|s| s.values.iter().all(Option::is_none)) {
        buf.set_string(plot.x, plot.y, "(no numeric data)", base.italic());
        return Vec::new();
    }
    match data.kind {
        ChartType::Bar | ChartType::Histogram => bars(buf, plot, data, paper, highlight, base),
        ChartType::Line | ChartType::Area | ChartType::Scatter => {
            xy(buf, plot, data, paper, base);
            Vec::new()
        }
    }
}

/// Horizontal bars: one line per series and category, eighth blocks for precision.
fn bars(buf: &mut Buffer, area: Rect, data: &ChartData, paper: bool, highlight: Option<usize>, base: Style) -> Vec<(Rect, usize)> {
    const EIGHTHS: [&str; 8] = ["", "▏", "▎", "▍", "▌", "▋", "▊", "▉"];
    let (lo, hi) = data.value_range();
    let n_series = data.series.len();
    let label_w = data.categories.iter().map(|c| c.width()).max().unwrap_or(0).min(area.width as usize / 3);
    let value_w = data
        .series
        .iter()
        .flat_map(|s| s.values.iter().flatten())
        .map(|v| data.fmt(*v, false).width())
        .max()
        .unwrap_or(1);
    let bar_w = (area.width as usize).saturating_sub(label_w + value_w + 3).max(1) as f64;
    let zero = ((-lo) / (hi - lo) * bar_w).round() as usize;
    let per_category = n_series + usize::from(n_series > 1);

    let mut hits = Vec::new();
    for (i, cat) in data.categories.iter().enumerate() {
        let y0 = area.y as usize + i * per_category;
        if y0 + n_series > area.bottom() as usize {
            break;
        }
        let selected = highlight == Some(i);
        let label_style = if selected { base.bold().reversed() } else { base };
        buf.set_stringn(area.x, y0 as u16, format!("{:<label_w$}", truncate(cat, label_w)), label_w, label_style);
        for (j, s) in data.series.iter().enumerate() {
            let y = (y0 + j) as u16;
            let Some(v) = s.values.get(i).copied().flatten() else { continue };
            let color = base.fg(series_color(j, paper));
            let x0 = area.x as usize + label_w + 1;
            let len = (v.abs() / (hi - lo)) * bar_w;
            let text = if v >= 0.0 {
                let full = len.floor() as usize;
                let frac = ((len - full as f64) * 8.0).round() as usize;
                format!("{}{}", "█".repeat(full), if frac == 8 { "█" } else { EIGHTHS[frac.min(7)] })
            } else {
                "█".repeat(len.round().max(1.0) as usize)
            };
            let start = if v >= 0.0 { x0 + zero } else { x0 + zero - text.chars().count().min(zero) };
            buf.set_string(start as u16, y, &text, color);
            let after = (start + text.chars().count() + 1).min(area.right() as usize);
            let value_style = if selected { base.bold() } else { base.fg(Color::Gray) };
            let value_style = if paper { base } else { value_style };
            buf.set_stringn(after as u16, y, data.fmt(v, false), area.right() as usize - after, value_style);
        }
        hits.push((Rect::new(area.x, y0 as u16, area.width, n_series as u16), i));
    }
    if !data.unit.is_none() && area.height as usize > data.categories.len() * per_category {
        let y = area.y + (data.categories.len() * per_category) as u16;
        let note = format!("values in {}", data.unit);
        buf.set_stringn(area.x + label_w as u16 + 1, y, &note, area.width as usize, base.italic().fg(Color::Gray));
    }
    hits
}

/// Line, area and scatter charts with braille resolution.
fn xy(buf: &mut Buffer, area: Rect, data: &ChartData, paper: bool, base: Style) {
    let (lo, hi) = data.value_range();
    let n = data.categories.len();
    let points: Vec<Vec<(f64, f64)>> = data
        .series
        .iter()
        .map(|s| {
            s.values
                .iter()
                .enumerate()
                .filter_map(|(i, v)| {
                    let x = match &data.xs {
                        Some(xs) => xs.get(i).copied().filter(|x| x.is_finite())?,
                        None => i as f64,
                    };
                    v.map(|y| (x, y))
                })
                .collect()
        })
        .collect();
    let (x_lo, x_hi) = match &data.xs {
        Some(xs) => {
            let f = xs.iter().filter(|x| x.is_finite());
            let lo = f.clone().fold(f64::INFINITY, |a, &b| a.min(b));
            let hi = f.fold(f64::NEG_INFINITY, |a, &b| a.max(b));
            if lo < hi { (lo, hi) } else { (lo - 1.0, lo + 1.0) }
        }
        None => (0.0, (n.max(2) - 1) as f64),
    };
    let graph = match data.kind {
        ChartType::Scatter => GraphType::Scatter,
        ChartType::Area => GraphType::Bar,
        _ => GraphType::Line,
    };
    let datasets: Vec<Dataset> = points
        .iter()
        .enumerate()
        .map(|(i, p)| Dataset::default().marker(Marker::Braille).graph_type(graph).style(base.fg(series_color(i, paper))).data(p))
        .collect();
    let x_labels: Vec<Span> = match &data.xs {
        Some(_) => vec![data.fmt(x_lo, false).into(), data.fmt(x_hi, false).into()],
        None if n > 2 => vec![data.categories[0].clone().into(), data.categories[n / 2].clone().into(), data.categories[n - 1].clone().into()],
        None => data.categories.iter().map(|c| Span::from(c.clone())).collect(),
    };
    let y_labels: Vec<Span> =
        vec![data.fmt(lo, false).into(), data.fmt((lo + hi) / 2.0, false).into(), data.fmt(hi, true).into()];
    let axis_style = if paper { base } else { Style::new().fg(Color::Gray) };
    Chart::new(datasets)
        .style(base)
        .x_axis(Axis::default().bounds([x_lo, x_hi]).labels(x_labels).style(axis_style))
        .y_axis(Axis::default().bounds([lo, hi]).labels(y_labels).style(axis_style))
        .hidden_legend_constraints((ratatui::layout::Constraint::Length(0), ratatui::layout::Constraint::Length(0)))
        .render(area, buf);
}
