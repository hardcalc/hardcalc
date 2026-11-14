//! Drawing. Every clickable region is recorded in `app.hits` for mouse handling.

use super::app::{App, CellView, Header, Hit, InspectorLine, Mode, ViewKind, VALIDATE};
use crate::print::LineKind;
use crate::layers::{CellStyle, Color as LayerColor};
use crate::model::Align;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::Line;
use ratatui::widgets::{Block, Clear, Paragraph};
use ratatui::Frame;
use std::collections::HashMap;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

const MIN_COL: usize = 3;
const FILL_MIN_COL: usize = 8;
const MAX_COL: usize = 40;
const HEADER_LINES: u16 = 2;

pub fn draw(f: &mut Frame, app: &mut App) {
    app.anim.begin_frame();
    app.hits.clear();
    app.image_slot = None;
    app.images.clear();
    app.pictures.clear();
    // Kitty puts images over the text: none while something lies over the windows.
    // The commands shown above the command line may cover a chart too.
    app.images_allowed = app.graphics.is_some() && !app.overlay_open() && !matches!(app.mode, Mode::Command { .. });
    app.other_hits.clear();
    app.window_areas.clear();
    if app.preview.is_some() {
        let [tabs, page, status, fkeys] =
            Layout::vertical([Constraint::Length(1), Constraint::Min(3), Constraint::Length(1), Constraint::Length(1)])
                .areas(f.area());
        draw_tabs(f, app, tabs);
        draw_preview(f, app, page);
        draw_preview_status(f, app, status);
        draw_fkeys(f, app, fkeys);
        draw_paper_menu(f, app);
        if app.show_help {
            draw_help(f, app);
        }
        app.anim.finish(f.buffer_mut());
        super::theme::apply(f.buffer_mut(), app.light(), app.theme != super::theme::ThemeSetting::Auto);
        return;
    }
    // The status line names the rows of a layout element as they are drawn.
    app.layout_rows = match app.view.kind {
        ViewKind::Layout(i) => app.shown_spec(i).elements.iter().map(|e| e.rows).collect(),
        _ => Vec::new(),
    };
    // Formula bar and status follow the focused data window (the main window
    // while a helper is focused).
    let data_left = if app.view.is_data() { app.left_focused } else { true };
    // The data layers of that window's table get a line of their own.
    let stacked = app.with_window(data_left, |app| app.view.kind == ViewKind::Table && app.view.pivot.is_none() && app.layer_count() > 0);
    let screen = f.area();
    // The bar grows only while text is entered; navigating keeps it one line.
    let bar_height = app.with_window(data_left, |app| {
        if matches!(app.mode, Mode::Edit { .. }) {
            bar_text(app, screen.width).0.len().clamp(1, (screen.height as usize / 4).clamp(1, 8)) as u16
        } else {
            1
        }
    });
    let [top, bar, body, stack, layers, status, fkeys] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(bar_height),
        Constraint::Min(3),
        Constraint::Length(u16::from(stacked)),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(f.area());
    // While a type definition is edited, the lower part of the screen explains the syntax.
    let mut body = body;
    if matches!(app.mode, Mode::Edit { .. }) && app.types_view() && body.height >= TYPE_HELP.len() as u16 + 2 + 6 {
        let [rest, help] = Layout::vertical([Constraint::Min(6), Constraint::Length(TYPE_HELP.len() as u16 + 2)]).areas(body);
        draw_type_help(f, help);
        body = rest;
    }
    // The hint of a focused helper window (inspector, source, chart).
    let helper = helper_hint(app);
    app.split_body = None;
    if app.is_split() {
        draw_split_title(f, app, top);
        let [left, sep, right] =
            Layout::horizontal([Constraint::Percentage(app.split_percent), Constraint::Length(1), Constraint::Fill(1)]).areas(body);
        app.split_body = Some(body);
        app.split_sep = sep;
        let sep_style = if app.split_drag { Style::new().fg(Color::Black).bg(Color::Cyan) } else { Style::new().fg(Color::DarkGray) };
        for y in sep.top()..sep.bottom() {
            f.buffer_mut()[(sep.x, y)].set_char('│').set_style(sep_style);
        }
        for (is_left, area) in [(true, left), (false, right)] {
            app.window_areas.push((is_left, area));
            let focused = is_left == app.left_focused;
            let start = app.hits.len();
            app.with_window(is_left, |app| {
                draw_window(f, app, area, focused, true);
                turn_window(f, app, is_left, area);
            });
            if !focused {
                let moved = app.hits.split_off(start);
                app.other_hits.extend(moved);
            }
        }
    } else {
        draw_tabs(f, app, top);
        app.window_areas.push((true, body));
        draw_window(f, app, body, true, false);
        turn_window(f, app, true, body);
    }
    app.with_window(data_left, |app| draw_formula_bar(f, app, bar));
    if stacked {
        app.with_window(data_left, |app| draw_data_layers(f, app, stack));
    }
    draw_layers(f, app, layers);
    app.with_window(data_left, |app| draw_status(f, app, status, helper));
    draw_fkeys(f, app, fkeys);
    let paper = app.paper_menu.map(|_| 0);
    slide(f, app, "paper", paper, 0, draw_paper_menu);
    draw_suggestions(f, app, status);
    let menu = app.menu.map(|(open, _)| open as u64);
    slide(f, app, "menu", menu, top.y + 1, |f, app| draw_menu(f, app, top));
    let context = app.context.map(|m| hash_of((m.pos, m.group)));
    slide(f, app, "context", context, 0, draw_context_menu);
    let values = app.values.as_ref().map(|v| v.col as u64);
    slide(f, app, "values", values, 0, draw_values);
    let plot = app.plot_dialog.as_ref().map(|_| 0);
    slide(f, app, "plot", plot, 0, draw_plot_dialog);
    let library = app.library.as_ref().map(|_| 0);
    slide(f, app, "library", library, 0, draw_library);
    let symbols = app.symbols.as_ref().map(|_| 0);
    slide(f, app, "symbols", symbols, 0, draw_symbols);
    let dialog = app.file_dialog.as_ref().map(|_| 0);
    slide(f, app, "files", dialog, 0, draw_file_dialog);
    let confirm = app.confirm.as_ref().map(|_| 0);
    slide(f, app, "confirm", confirm, 0, draw_confirm);
    let help = app.show_help.then_some(0);
    slide(f, app, "help", help, 0, |f, app| {
        if app.show_help {
            draw_help(f, app);
        }
    });
    app.anim.finish(f.buffer_mut());
    super::theme::apply(f.buffer_mut(), app.light(), app.theme != super::theme::ThemeSetting::Auto);
}

/// Draws a popup with `draw` and lets it slide in from above (from behind
/// line `clip_top`); `key` identifies what it shows (`None`: not shown).
fn slide(f: &mut Frame, app: &mut App, name: &'static str, key: Option<u64>, clip_top: u16, draw: impl FnOnce(&mut Frame, &mut App)) {
    let base = if key.is_some() && app.anim.enabled { Some(f.buffer_mut().clone()) } else { None };
    draw(f, app);
    match base {
        Some(base) => app.anim.popup(name, key, f.buffer_mut(), &base, clip_top),
        None => app.anim.popup(name, None, f.buffer_mut(), &ratatui::buffer::Buffer::empty(Rect::ZERO), clip_top),
    }
}

fn hash_of(v: impl std::hash::Hash) -> u64 {
    use std::hash::Hasher;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    v.hash(&mut h);
    h.finish()
}

/// Turns the window like a cube when it switched to another table or view.
fn turn_window(f: &mut Frame, app: &mut App, side: bool, area: Rect) {
    let ident = (tab_order(app.view.kind), app.view.sheet);
    app.anim.window(side, ident, f.buffer_mut(), area);
}

/// Popup listing all paper formats and orientations.
fn draw_paper_menu(f: &mut Frame, app: &mut App) {
    let Some(sel) = app.paper_menu else { return };
    let choices = crate::print::paper_choices();
    let current = app.current_paper();
    let area = f.area();
    let w = 48.min(area.width);
    let h = (choices.len() as u16 + 2).min(area.height);
    let rect = Rect::new(area.x + (area.width - w) / 2, area.y + (area.height - h) / 2, w, h);
    f.render_widget(Clear, rect);
    f.render_widget(Block::bordered().title(" paper · ↑↓ Enter · Esc "), rect);
    for (i, &(paper, landscape)) in choices.iter().enumerate() {
        let y = rect.y + 1 + i as u16;
        if y >= rect.bottom() - 1 {
            break;
        }
        let (pw, ph) = paper.size();
        let (pw, ph) = if landscape { (ph, pw) } else { (pw, ph) };
        let mm = |pt: f32| (pt / 72.0 * 25.4).round();
        let mark = if (paper, landscape) == current { "●" } else { " " };
        let size = format!("{}×{} mm", mm(pw), mm(ph));
        let text = format!(" {mark} {:<27}{size:>14}", paper.label(landscape));
        let style = if i == sel { Style::new().reversed() } else { Style::new() };
        let inner = Rect::new(rect.x + 1, y, w - 2, 1);
        f.buffer_mut().set_stringn(inner.x, y, fit(&text, inner.width as usize, Align::Left), inner.width as usize, style);
        app.hits.push((inner, Hit::PaperChoice(i)));
    }
}

/// The menu bar in place of the top line, with the open menu below it.
fn draw_menu(f: &mut Frame, app: &mut App, top: Rect) {
    use super::commands::{items, MENUS};
    let Some((open, selected)) = app.menu else { return };
    f.buffer_mut().set_stringn(top.x, top.y, " ".repeat(top.width as usize), top.width as usize, Style::new().bg(Color::DarkGray));
    let mut chips_list = vec![(" ≡ hardcalc ".to_string(), Style::new().bold().fg(Color::Cyan).bg(Color::Black), Some(Hit::Logo))];
    let mut xs = Vec::new();
    let mut x = top.x + " ≡ hardcalc ".width() as u16;
    for (i, name) in MENUS.iter().enumerate() {
        let style = if i == open { Style::new().bold().fg(Color::Black).bg(Color::Cyan) } else { Style::new().fg(Color::White).bg(Color::DarkGray) };
        let text = format!("  {name}  ");
        xs.push(x);
        x += text.width() as u16;
        chips_list.push((text, style, Some(Hit::MenuTop(i))));
    }
    chips_list.push(("   ←→ ↑↓ Enter · Esc".into(), Style::new().fg(Color::Gray).bg(Color::DarkGray), None));
    chips(f, top, chips_list, &mut app.hits);

    let entries = items(open);
    let label_w = entries.iter().map(|e| e.label.width()).max().unwrap_or(0);
    let key_w = entries.iter().map(|e| e.key.width()).max().unwrap_or(0);
    let area = f.area();
    let w = ((label_w + key_w + 6) as u16).min(area.width);
    let h = (entries.len() as u16 + 2).min(area.height.saturating_sub(1));
    let x = xs[open].min(area.right().saturating_sub(w));
    let rect = Rect::new(x, top.y + 1, w, h);
    f.render_widget(Clear, rect);
    f.render_widget(Block::bordered(), rect);
    for (i, e) in entries.iter().enumerate() {
        let y = rect.y + 1 + i as u16;
        if y >= rect.bottom() - 1 {
            break;
        }
        let gap = (w as usize - 4).saturating_sub(e.label.width() + e.key.width());
        let text = format!(" {}{}{} ", e.label, " ".repeat(gap), e.key);
        let style = if i == selected { Style::new().reversed().bold() } else { Style::new() };
        let row = Rect::new(rect.x + 1, y, w - 2, 1);
        f.buffer_mut().set_stringn(row.x, y, &text, row.width as usize, style);
        // The key in grey, unless selected.
        if i != selected && !e.key.is_empty() {
            let kx = row.right().saturating_sub(e.key.width() as u16 + 1);
            f.buffer_mut().set_string(kx, y, e.key, Style::new().fg(Color::Gray));
        }
        app.hits.push((row, Hit::MenuItem(i)));
    }
}

/// The context menu at the cursor cell or the click: groups, then entries
/// with their letters and keys.
fn draw_context_menu(f: &mut Frame, app: &mut App) {
    let Some(m) = app.context else { return };
    let rows = m.rows(&app.caps);
    let title = match m.group {
        Some(g) => format!(" {} ", super::app::CONTEXT_MENU[g].label),
        None => " / ".to_string(),
    };
    let label_w = rows.iter().map(|(l, ..)| l.width()).max().unwrap_or(0);
    let key_w = rows.iter().map(|(.., k)| k.width()).max().unwrap_or(0);
    let area = f.area();
    let w = ((label_w + key_w + 9) as u16).max(title.width() as u16 + 4).min(area.width);
    let h = (rows.len() as u16 + 2).min(area.height);
    let x = m.pos.0.min(area.right().saturating_sub(w));
    let y = if m.pos.1 + h > area.bottom() { m.pos.1.saturating_sub(h + 1) } else { m.pos.1 };
    let rect = Rect::new(x, y, w, h);
    f.render_widget(Clear, rect);
    f.render_widget(Block::bordered().title(title).title_bottom(" letter · ↑↓ Enter · ← Esc "), rect);
    for (i, (label, letter, keys)) in rows.iter().enumerate() {
        let y = rect.y + 1 + i as u16;
        if y >= rect.bottom() - 1 {
            break;
        }
        let row = Rect::new(rect.x + 1, y, w - 2, 1);
        let selected = i == m.selected;
        let base = if selected { Style::new().reversed().bold() } else { Style::new() };
        f.buffer_mut().set_stringn(row.x, y, " ".repeat(row.width as usize), row.width as usize, base);
        f.buffer_mut().set_string(row.x + 1, y, letter.to_string(), if selected { base } else { Style::new().bold().fg(Color::Cyan) });
        f.buffer_mut().set_string(row.x + 3, y, *label, base);
        let kx = row.right().saturating_sub(keys.width() as u16 + 1);
        f.buffer_mut().set_string(kx, y, keys, if selected { base } else { Style::new().fg(Color::Gray) });
        app.hits.push((row, Hit::ContextItem(i)));
    }
}

/// The value list of a column: a tick per distinct value.
/// A box of `w` × `h` in the middle of the screen, cleared, with a frame.
fn centered_box(f: &mut Frame, w: u16, h: u16, title: String, bottom: String) -> Rect {
    let area = f.area();
    let (w, h) = (w.min(area.width), h.min(area.height));
    let rect = Rect::new(area.x + (area.width - w) / 2, area.y + (area.height - h) / 2, w, h);
    f.render_widget(Clear, rect);
    f.render_widget(Block::bordered().title(title).title_bottom(bottom), rect);
    rect
}

/// The settings of a chart in the calculation sheet (F7).
fn draw_plot_dialog(f: &mut Frame, app: &mut App) {
    use super::app::PlotDialog;
    let Some(d) = app.plot_dialog.clone() else { return };
    let title = if d.row.is_some() { " chart settings " } else { " new chart " };
    let rect = centered_box(f, 72, 10, title.into(), " ↑↓ field · type to edit · ←→ type · Enter writes · Esc ".into());
    let inner = rect.width.saturating_sub(2) as usize;
    let label_w = PlotDialog::LABELS.iter().map(|l| l.width()).max().unwrap_or(0);
    let hints = ["the chart's name, for layouts and ▥", "ranges to draw, separated by ;  (M; V)", "the x values; empty: the range they run over", "", "above the chart; empty: the name"];
    for (i, label) in PlotDialog::LABELS.iter().enumerate() {
        let y = rect.y + 1 + i as u16;
        let selected = i == d.selected;
        let value = if i == PlotDialog::TYPE {
            format!("◂ {} ▸", d.fields[i])
        } else if selected {
            format!("{}▏", d.fields[i])
        } else {
            d.fields[i].clone()
        };
        let text = format!(" {label:>label_w$}  {value}");
        let style = if selected { Style::new().reversed() } else { Style::new() };
        f.buffer_mut().set_stringn(rect.x + 1, y, fit(&text, inner, Align::Left), inner, style);
        // What the field is for, dim, after a short value.
        let used = text.width() + 2;
        if !selected && d.fields[i].is_empty() && used < inner {
            f.buffer_mut().set_stringn(rect.x + 1 + used as u16, y, hints[i], inner - used, Style::new().fg(Color::DarkGray));
        }
    }
    let y = rect.y + 1 + PlotDialog::LABELS.len() as u16 + 1;
    match &d.error {
        Some(e) => f.buffer_mut().set_stringn(rect.x + 2, y, format!("⚠ {e}"), inner - 1, Style::new().fg(Color::Red)),
        None => f.buffer_mut().set_stringn(rect.x + 2, y, "types: line, area, bar, scatter", inner - 1, Style::new().fg(Color::DarkGray)),
    };
}

/// The formula library (F6): pick a formula to insert.
/// The list of special characters: the character, its name, its LaTeX
/// name and ASCII spellings; typing narrows it.
fn draw_symbols(f: &mut Frame, app: &mut App) {
    let shown = app.symbols_shown();
    let Some(mut p) = app.symbols.clone() else { return };
    let area = f.area();
    let title = if p.query.is_empty() { " special characters – type a name, \\le or <= ".to_string() } else { format!(" special characters: {}▏ ", p.query) };
    let rect = centered_box(f, 64.min(area.width), (area.height * 3 / 4).clamp(8, 24), title, " ↑↓ · Enter inserts · Esc ".into());
    let inner = rect.width.saturating_sub(2) as usize;
    let body = rect.height.saturating_sub(2) as usize;
    p.selected = p.selected.min(shown.len().saturating_sub(1));
    if p.selected < p.scroll {
        p.scroll = p.selected;
    } else if body > 0 && p.selected >= p.scroll + body {
        p.scroll = p.selected + 1 - body;
    }
    for (k, s) in shown.iter().enumerate().skip(p.scroll).take(body) {
        let y = rect.y + 1 + (k - p.scroll) as u16;
        let latex: Vec<String> = s.latex.iter().map(|l| format!("\\{l}")).collect();
        let text = format!("  {}   {:<26} {:<16} {}", s.ch, s.name, latex.join(" "), s.ascii.join(" "));
        let style = if k == p.selected { Style::new().reversed() } else { Style::new() };
        f.buffer_mut().set_stringn(rect.x + 1, y, fit(&clip(&text, inner), inner, Align::Left), inner, style);
        app.hits.push((Rect::new(rect.x + 1, y, inner as u16, 1), Hit::Symbol(k)));
    }
    if shown.is_empty() {
        f.buffer_mut().set_stringn(rect.x + 2, rect.y + 1, "(no character matches)", inner - 1, Style::new().fg(Color::DarkGray));
    }
    app.symbols = Some(p);
}

fn draw_library(f: &mut Frame, app: &mut App) {
    let Some(mut d) = app.library.clone() else { return };
    let area = f.area();
    let title = if d.query.is_empty() { " formula library – type to search ".to_string() } else { format!(" formula library: {}▏ ", d.query) };
    let rect = centered_box(f, (area.width * 9 / 10).max(40), (area.height * 4 / 5).max(8), title, " ↑↓ · Enter inserts the formula with its inputs · Esc ".into());
    let inner = rect.width.saturating_sub(2) as usize;
    // The last two lines: the note of the selected formula.
    let body = rect.height.saturating_sub(4) as usize;
    let shown = d.shown().into_iter().cloned().collect::<Vec<_>>();
    d.selected = d.selected.min(shown.len().saturating_sub(1));
    if d.selected < d.scroll {
        d.scroll = d.selected;
    } else if body > 0 && d.selected >= d.scroll + body {
        d.scroll = d.selected + 1 - body;
    }
    let left_w = shown.iter().map(|e| e.title.width() + 2).max().unwrap_or(0).min(inner / 3);
    for (k, e) in shown.iter().enumerate().skip(d.scroll).take(body) {
        let y = rect.y + 1 + (k - d.scroll) as u16;
        let selected = k == d.selected;
        let own = if e.own { " ★" } else { "" };
        let left = fit(&clip(&format!("{}{own}", e.title), left_w), left_w, Align::Left);
        let text = format!(" {left}  {}", crate::sheet::expr::Parts::of(&e.line).code);
        let style = if selected { Style::new().reversed() } else { Style::new() };
        f.buffer_mut().set_stringn(rect.x + 1, y, fit(&clip(&text, inner), inner, Align::Left), inner, style);
    }
    if shown.is_empty() {
        f.buffer_mut().set_stringn(rect.x + 2, rect.y + 1, "(no formula matches)", inner - 1, Style::new().fg(Color::DarkGray));
    }
    // Where the selected formula belongs and what it is for.
    let note = shown.get(d.selected).map(|e| [Some(e.group.clone()), e.note.clone()].into_iter().flatten().collect::<Vec<_>>().join(" · ")).unwrap_or_default();
    let file = crate::sheet::library::own_file().map(|p| format!("own formulas: {}", p.display())).unwrap_or_default();
    f.buffer_mut().set_stringn(rect.x + 2, rect.bottom().saturating_sub(3), &note, inner - 1, Style::new().italic());
    f.buffer_mut().set_stringn(rect.x + 2, rect.bottom().saturating_sub(2), &file, inner - 1, Style::new().fg(Color::DarkGray));
    app.library = Some(d);
}

fn draw_values(f: &mut Frame, app: &mut App) {
    let Some(mut list) = app.values.clone() else { return };
    let shown = list.shown();
    let lines = shown.len() + 1;
    let area = f.area();
    let title = if list.query.is_empty() { format!(" {} ", list.name) } else { format!(" {}: {}▏", list.name, list.query) };
    let bottom = " Space ✓ · type: search · Enter ";
    let label_w = list.items.iter().map(|i| i.label.width()).max().unwrap_or(0).max("(all listed)".width());
    let count_w = list.items.iter().map(|i| i.count.to_string().len()).max().unwrap_or(1);
    let w = ((label_w + count_w + 9).max(title.width() + 2).max(bottom.width() + 2).min(48) as u16).min(area.width);
    let h = (list.items.len() as u16 + 3).min(14).min(area.height.saturating_sub(2)).max(3);
    let x = list.pos.0.min(area.right().saturating_sub(w));
    let y = if list.pos.1 + h > area.bottom() { area.bottom().saturating_sub(h) } else { list.pos.1 };
    let rect = Rect::new(x, y, w, h);
    let body = h.saturating_sub(2) as usize;
    // Keep the highlighted line in sight.
    list.selected = list.selected.min(lines - 1);
    if list.selected < list.scroll {
        list.scroll = list.selected;
    } else if body > 0 && list.selected >= list.scroll + body {
        list.scroll = list.selected + 1 - body;
    }
    list.scroll = list.scroll.min(lines.saturating_sub(body));
    list.rect = rect;
    f.render_widget(Clear, rect);
    f.render_widget(Block::bordered().title(title).title_bottom(bottom), rect);
    app.hits.push((Rect::new(rect.x, rect.bottom().saturating_sub(1), rect.width, 1), Hit::ValuesApply));
    let inner = (w as usize).saturating_sub(2);
    for (k, line) in (list.scroll..lines).take(body).enumerate() {
        let y = rect.y + 1 + k as u16;
        let (tick, label, count) = match line.checked_sub(1) {
            None => {
                let tick = match list.all_checked() {
                    (true, _) => "[x]",
                    (false, true) => "[-]",
                    _ => "[ ]",
                };
                (tick, if list.query.is_empty() { "(all)" } else { "(all listed)" }.to_string(), String::new())
            }
            Some(i) => {
                let item = &list.items[shown[i]];
                (if item.checked { "[x]" } else { "[ ]" }, item.label.clone(), item.count.to_string())
            }
        };
        let selected = line == list.selected;
        let base = if selected { Style::new().reversed().bold() } else { Style::new() };
        let room = inner.saturating_sub(6 + count.width());
        let text = format!(" {tick} {}", fit(&clip(&label, room), room, Align::Left));
        f.buffer_mut().set_stringn(rect.x + 1, y, fit(&text, inner, Align::Left), inner, base);
        let cx = rect.right().saturating_sub(2 + count.width() as u16);
        f.buffer_mut().set_string(cx, y, &count, if selected { base } else { Style::new().fg(Color::Gray) });
        app.hits.push((Rect::new(rect.x + 1, y, inner as u16, 1), Hit::ValueItem(line)));
    }
    app.values = Some(list);
}

/// While a `:` command is typed (like Helix): the matching commands or
/// values as a compact grid above the command line, and above that the
/// documentation of the chosen or typed command.
fn draw_suggestions(f: &mut Frame, app: &mut App, status: Rect) {
    use super::commands;
    let Mode::Command { buf } = &app.mode else { return };
    if app.menu.is_some() {
        return;
    }
    let buf = buf.clone();
    let list = app.command_suggestions();
    let typed = buf.split_whitespace().next().unwrap_or("").to_string();
    let values = buf.contains(' ');
    // The command documented: the chosen suggestion, else the typed one, else the first.
    let documented = match (app.cmd_choice.and_then(|i| list.get(i)), values) {
        (Some((done, ..)), false) => done.trim().to_string(),
        _ if commands::find(&typed).is_some() => typed.clone(),
        _ if values => typed.clone(),
        // Nothing typed yet: only the list.
        _ if buf.is_empty() => String::new(),
        _ => list.first().map(|(done, ..)| done.trim().to_string()).unwrap_or_default(),
    };
    let doc = commands::doc(&documented);
    if list.is_empty() && doc.is_none() {
        return;
    }
    let width = status.width;
    let mut bottom = status.y;

    // Grid of names (or values), row by row.
    if !list.is_empty() {
        let names: Vec<String> = list
            .iter()
            .map(|(done, ..)| if values { done.split_once(' ').map_or(done.clone(), |(_, v)| v.to_string()) } else { done.trim().to_string() })
            .collect();
        let col_w = names.iter().map(|n| n.width()).max().unwrap_or(1) + 3;
        let cols = (width as usize / col_w).max(1);
        let total_rows = names.len().div_ceil(cols);
        let rows = total_rows.min(6).min(bottom.saturating_sub(2) as usize);
        if rows > 0 {
            let chosen_row = app.cmd_choice.map_or(0, |i| i / cols);
            let first_row = chosen_row.saturating_sub(rows - 1);
            let top = bottom - rows as u16;
            let surface = Style::new().bg(Color::Black).fg(Color::Gray);
            for y in top..bottom {
                f.buffer_mut().set_stringn(status.x, y, " ".repeat(width as usize), width as usize, surface);
            }
            for (i, name) in names.iter().enumerate() {
                let (row, col) = (i / cols, i % cols);
                if row < first_row || row >= first_row + rows {
                    continue;
                }
                let x = status.x + (col * col_w) as u16 + 1;
                let y = top + (row - first_row) as u16;
                let style = if app.cmd_choice == Some(i) {
                    Style::new().bg(Color::Cyan).fg(Color::Black).bold()
                } else {
                    Style::new().bg(Color::Black).fg(Color::White)
                };
                let text = format!(" {name} ");
                f.buffer_mut().set_stringn(x, y, &text, col_w.saturating_sub(1), style);
                app.hits.push((Rect::new(x, y, (col_w - 1) as u16, 1), Hit::Suggestion(i)));
            }
            if total_rows > rows {
                let more = format!(" {} more ↓ ", names.len().saturating_sub((first_row + rows) * cols));
                if first_row + rows < total_rows {
                    let w = more.width() as u16;
                    f.buffer_mut().set_string(status.right().saturating_sub(w), bottom - 1, &more, surface);
                }
            }
            bottom = top;
        }
    }

    // Documentation of the command.
    if let Some((syntax, help, more)) = doc {
        let lines = 2 + more.len() as u16;
        let h = lines + 2;
        let w = width.min(78);
        if bottom < h + 2 {
            return;
        }
        let rect = Rect::new(status.x, bottom - h, w, h);
        f.render_widget(Clear, rect);
        f.render_widget(Block::bordered().border_style(Style::new().fg(Color::DarkGray)), rect);
        let iw = (w - 4) as usize;
        let buf = f.buffer_mut();
        buf.set_stringn(rect.x + 2, rect.y + 1, &syntax, iw, Style::new().bold().fg(Color::Cyan));
        buf.set_stringn(rect.x + 2, rect.y + 2, &help, iw, Style::new());
        for (k, line) in more.iter().enumerate() {
            buf.set_stringn(rect.x + 2, rect.y + 3 + k as u16, line, iw, Style::new().fg(Color::Gray));
        }
    }
}

/// Syntax of a type definition: (label, text) per line.
const TYPE_HELP: &[(&str, &str)] = &[
    ("Syntax", "Name = base [unit \"…\"] [modifiers]   (base may also be another own type)"),
    ("Bases", "int · decimal(n) · float · text · bool · date · time · datetime · duration · ref"),
    ("", "enum(a, b, …) · record(field: type, …)"),
    ("Unit", "unit \"€\" · unit \"%\" · unit \"m/s\"   (shown in the column header, checked in formulas)"),
    ("Modifiers", "range 1..100 (also 1.. / ..100) · length ..4 · pattern \"^[A-Z]+$\" (whole text)"),
    ("", "special(\"-\": \"not collected\") · secret · multiline"),
    ("Checks", "check iban · email · host · ip · cidr · url"),
    ("Examples", "Euro = decimal(2) unit \"€\"     Status = enum(open, paid, cancelled)"),
    ("", "Port = int range 1..65535       Code = text pattern \"^[A-Z]{2,4}$\""),
];

/// Quick reference shown below the types view while a definition is edited.
fn draw_type_help(f: &mut Frame, area: Rect) {
    f.render_widget(Clear, area);
    let block = Block::bordered()
        .title(" type definition – Enter/Tab commit · Esc cancel ")
        .border_style(Style::new().fg(Color::DarkGray));
    let inner = block.inner(area);
    f.render_widget(block, area);
    let label_w = TYPE_HELP.iter().map(|(l, _)| l.width()).max().unwrap_or(0) + 2;
    let buf = f.buffer_mut();
    for (k, (label, text)) in TYPE_HELP.iter().enumerate() {
        let y = inner.y + k as u16;
        if y >= inner.bottom() {
            break;
        }
        buf.set_stringn(inner.x + 1, y, label, label_w, Style::new().bold().fg(Color::Cyan));
        let x = inner.x + 1 + label_w as u16;
        buf.set_stringn(x, y, text, inner.right().saturating_sub(x) as usize, Style::new());
    }
}

/// "Save or discard unsaved changes?" with three buttons.
fn draw_confirm(f: &mut Frame, app: &mut App) {
    if app.confirm.is_none() {
        return;
    }
    let area = f.area();
    let file = if app.is_untitled() { "The new document".to_string() } else { app.doc_file_name() };
    let text = format!("{file} has unsaved changes.");
    let buttons = [" [S]ave ", " [D]iscard ", " [Esc] Cancel "];
    let bw: usize = buttons.iter().map(|b| b.width() + 2).sum();
    let w = (text.width().max(bw) as u16 + 6).min(area.width);
    let h = 6.min(area.height);
    let rect = Rect::new(area.x + (area.width - w) / 2, area.y + (area.height - h) / 2, w, h);
    f.render_widget(Clear, rect);
    f.render_widget(Block::bordered().title(" unsaved changes "), rect);
    let buf = f.buffer_mut();
    buf.set_stringn(rect.x + 3, rect.y + 2, &text, (w - 4) as usize, Style::new().bold());
    let mut x = rect.x + (w.saturating_sub(bw as u16)) / 2;
    let y = rect.y + 4;
    for (i, b) in buttons.iter().enumerate() {
        let style = if i == 0 { Style::new().fg(Color::Black).bg(Color::Cyan).bold() } else { Style::new().reversed() };
        buf.set_string(x, y, b, style);
        app.hits.push((Rect::new(x, y, b.width() as u16, 1), Hit::Confirm(i)));
        x += b.width() as u16 + 2;
    }
}

/// The file dialog: folder, list (name, size, date), quick search and keys.
fn draw_file_dialog(f: &mut Frame, app: &mut App) {
    let Some(d) = &mut app.file_dialog else { return };
    let title = match d.purpose {
        super::app::Purpose::Open => " open file ",
        super::app::Purpose::Append => " append file ",
    };
    let hits = d.draw(f, title, "documents");
    app.hits.extend(hits.into_iter().map(|(r, spot)| (r, Hit::File(spot))));
}

fn draw_split_title(f: &mut Frame, app: &mut App, area: Rect) {
    f.buffer_mut().set_style(area, Style::new().bg(Color::DarkGray));
    let dim = Style::new().fg(Color::Gray).bg(Color::DarkGray);
    let items = vec![
        (" ≡ hardcalc ".to_string(), Style::new().bold().fg(Color::Black).bg(Color::Cyan), Some(Hit::Logo)),
        (" Tab: other window · Ctrl+U: swap · Ctrl+O: close split · drag │: width".to_string(), dim, None),
    ];
    let end = chips(f, area, items, &mut app.hits);
    draw_file_name(f, app, Rect { x: end, width: area.right().saturating_sub(end), ..area });
}

/// File name right-aligned in `area` (the free rest of a bar), if it fits.
fn draw_file_name(f: &mut Frame, app: &App, area: Rect) {
    let file = format!(" {}{} ", app.path.display(), if app.dirty { " [+]" } else { "" });
    let w = file.width() as u16;
    if w < area.width {
        f.buffer_mut().set_string(area.right() - w, area.y, file, Style::new().fg(Color::White).bg(Color::DarkGray));
    }
}

/// One window: its tab line (when split), pivot filters and content.
fn draw_window(f: &mut Frame, app: &mut App, area: Rect, focused: bool, split: bool) {
    let mut area = area;
    if split {
        draw_window_tabs(f, app, Rect { height: 1, ..area }, focused);
        area = Rect { y: area.y + 1, height: area.height.saturating_sub(1), ..area };
    }
    if app.view.pivot.is_some() && area.height > 1 {
        draw_filters(f, app, Rect { height: 1, ..area });
        area = Rect { y: area.y + 1, height: area.height - 1, ..area };
    }
    match app.view.kind {
        ViewKind::Table | ViewKind::Types | ViewKind::Functions | ViewKind::Calc => draw_grid(f, app, area, focused),
        ViewKind::Inspector => draw_inspector(f, app, area, focused),
        ViewKind::Source => draw_source(f, app, area, focused),
        ViewKind::CalcPreview => draw_calc_preview(f, app, area),
        ViewKind::Chart(i) => draw_chart(f, app, area, i),
        ViewKind::Layout(i) => draw_layout(f, app, area, i),
    }
}

/// What the overview writes into an element's box.
fn element_label(e: &crate::layout::Element) -> String {
    use crate::layout::ElementKind;
    let mut label = match e.kind {
        ElementKind::Heading => format!("# {}", e.content),
        ElementKind::Text => format!("¶ {}", e.content),
        ElementKind::Note => format!("▒ {}", e.content),
        ElementKind::Value => format!("= {}", e.label.as_ref().unwrap_or(&e.content)),
        ElementKind::Chart => format!("▥ {}", e.content),
        ElementKind::Table => match &e.z {
            Some(z) => format!("▦ {} · {z}", e.content),
            None => format!("▦ {}", e.content),
        },
    };
    if let Some(rows) = e.pick.text() {
        label.push_str(&format!(" · rows: {rows}"));
    }
    if e.flows() {
        label.push_str(" · auto");
    }
    label
}

/// The layout editor: ruler, rows of all pages with page breaks, elements –
/// as printed, or in the overview as labelled boxes with several rows per line.
fn draw_layout(f: &mut Frame, app: &mut App, area: Rect, i: usize) {
    use super::app::LayoutGeom;
    use crate::layout::{Element, ElementKind};
    if area.height < 3 {
        return;
    }
    let (spec, placed, layout) = app.layout_parts(i);
    let setup = spec.setup(i);
    let body = crate::print::layout_rows_per_page(&setup);
    let pages = layout.pages.len();
    let gutter: u16 = 5;
    let canvas = Rect { y: area.y + 1, height: area.height - 1, ..area };
    let x0 = area.x + gutter;
    let paper_w = (layout.cols as u16).min(area.width.saturating_sub(gutter));
    // Overview: a whole page (and the break below it) fits the window.
    let overview = app.layout_overview;
    let zoom = if overview { body.div_ceil((canvas.height as usize).saturating_sub(1).max(1)).max(2) } else { 1 };

    // Keep the selected element (or a running drag) in view.
    let (rows, lines) = LayoutGeom::canvas(body, pages, zoom, &spec.elements);
    let mut geom = LayoutGeom { area: canvas, x0, cols: layout.cols, grid: spec.grid, body, scroll: app.view.scroll.0, rows, lines };
    let total_lines = geom.lines.len();
    let focus_rows = app.layout_drag.map(|d| d.rows).or_else(|| spec.elements.get(app.view.cursor.0).map(|e| e.rows));
    if app.view.follow
        && let Some((r0, r1)) = focus_rows
    {
        let (l0, l1) = (geom.line_of(r0), geom.line_of(r1));
        let h = canvas.height as usize;
        if l0 < app.view.scroll.0 {
            app.view.scroll.0 = l0;
        } else if l1 >= app.view.scroll.0 + h {
            app.view.scroll.0 = (l1 + 1).saturating_sub(h).min(l0);
        }
    }
    app.view.scroll.0 = app.view.scroll.0.min(total_lines.saturating_sub(1));
    geom.scroll = app.view.scroll.0;
    app.layout_geom = Some(geom.clone());
    let visible = |line: usize| (line >= geom.scroll && line < geom.scroll + canvas.height as usize).then(|| canvas.y + (line - geom.scroll) as u16);

    let buf = f.buffer_mut();
    buf.set_style(area, Style::new().bg(Color::DarkGray));
    // Ruler: grid column numbers at their start.
    for g in 1..=spec.grid {
        let e = Element { cols: (g, g), rows: (1, 1), ..Default::default() };
        let (x, _) = spec.span(layout.cols, &e);
        if (x as u16) < paper_w {
            buf.set_string(x0 + x as u16, area.y, g.to_string(), Style::new().fg(Color::Gray).bg(Color::DarkGray));
        }
    }

    let paper = Style::new().bg(Color::White).fg(Color::Black);
    let right = x0 + paper_w;
    for y in 0..canvas.height {
        let line = geom.scroll + y as usize;
        if line >= total_lines {
            break;
        }
        let sy = canvas.y + y;
        let (first, _, page_break) = geom.lines[line];
        let (page, within) = (first / body, first % body);
        if page_break {
            let label = format!(" page {} ", page + 2);
            let dash = "╌".repeat(paper_w as usize);
            buf.set_stringn(x0, sy, &dash, paper_w as usize, Style::new().fg(Color::Yellow).bg(Color::DarkGray));
            buf.set_string(x0 + 2, sy, label, Style::new().fg(Color::Yellow).bg(Color::DarkGray));
            continue;
        }
        buf.set_stringn(area.x, sy, format!("{:>4} ", first + 1), gutter as usize, Style::new().fg(Color::Gray).bg(Color::DarkGray));
        buf.set_stringn(x0, sy, " ".repeat(paper_w as usize), paper_w as usize, paper);
        if overview {
            continue;
        }
        for span in &layout.pages[page][within].spans {
            let mut style = paper;
            if let Some((r, g, b)) = span.style.color {
                style = style.fg(Color::Rgb(r, g, b));
            }
            if span.style.bold {
                style = style.bold();
            }
            if span.style.italic {
                style = style.italic();
            }
            if span.style.shade {
                style = style.bg(Color::Rgb(220, 220, 220));
            }
            put_clipped(buf, x0 as i32 + span.col as i32, sy, &span.text, style, x0, right);
        }
    }
    // Charts, rendered off-screen and copied row by row; one wholly in view
    // gets its plot as an image where the terminal can show one.
    let mut images = Vec::new();
    for (page, graphics) in layout.graphics.iter().enumerate().filter(|_| !overview) {
        for g in graphics {
            let size = Rect::new(0, 0, g.width as u16, g.height as u16);
            let rows: Vec<Option<u16>> = (0..size.height).map(|y| visible(geom.line_of(page * body + g.line + y as usize + 1))).collect();
            let whole = rows.iter().all(Option::is_some) && rows.windows(2).all(|w| w[1] == w[0].map(|y| y + 1)) && x0 + g.col as u16 + size.width <= right;
            let mut tmp = ratatui::buffer::Buffer::empty(size);
            let (_, plot) = super::chart_view::render_with(&mut tmp, size, &g.data, true, None, app.images_allowed && whole);
            for y in 0..size.height {
                let Some(sy) = rows[y as usize] else { continue };
                for x in 0..size.width {
                    let sx = x0 + g.col as u16 + x;
                    if sx < right {
                        buf[(sx, sy)] = tmp[(x, y)].clone();
                    }
                }
            }
            if let (Some(p), Some(Some(top))) = (plot, rows.first()) {
                let rect = Rect { x: x0 + g.col as u16 + p.x, y: top + p.y, ..p };
                images.push(super::chart_image::ChartImage { rect, data: g.data.clone(), paper: true });
            }
        }
    }
    app.images.extend(images);
    // Element areas: light tint, selection, drag rectangle, page-break overflow.
    let tint = |buf: &mut ratatui::buffer::Buffer, cols: (usize, usize), rows: (usize, usize), color: Color| {
        let e = Element { cols, rows, ..Default::default() };
        let (x, w) = spec.span(layout.cols, &e);
        for r in rows.0..=rows.1 {
            let Some(sy) = visible(geom.line_of(r)) else { continue };
            for sx in (x0 + x as u16)..(x0 + (x + w) as u16).min(right) {
                buf[(sx, sy)].set_bg(color);
            }
        }
    };
    let selected = app.view.cursor.0;
    let amber = Color::Rgb(255, 214, 150);
    for (n, e) in spec.elements.iter().enumerate() {
        let crosses = !e.flows() && (e.rows.0 - 1) / body != (e.rows.1 - 1) / body;
        let hidden = placed.get(n).map_or(0, |p| p.hidden);
        let color = match (n == selected, crosses) {
            (_, true) => Color::Rgb(255, 215, 215),
            (true, false) if app.layout_grab => Color::Rgb(200, 240, 200),
            (true, false) => Color::Rgb(210, 228, 255),
            // The overview tells the kinds apart by colour.
            (false, false) if overview => match e.kind {
                ElementKind::Heading => Color::Rgb(255, 240, 190),
                ElementKind::Text => Color::Rgb(236, 236, 226),
                ElementKind::Value => Color::Rgb(215, 240, 215),
                ElementKind::Note => Color::Rgb(222, 222, 222),
                ElementKind::Table => Color::Rgb(226, 232, 245),
                ElementKind::Chart => Color::Rgb(250, 226, 205),
            },
            (false, false) => Color::Rgb(246, 246, 236),
        };
        tint(buf, e.cols, e.rows, color);
        let (x, w) = spec.span(layout.cols, e);
        let (left, room) = (x0 + x as u16, (w as u16).min(right.saturating_sub(x0 + x as u16)) as usize);
        if overview {
            // First line: what it is; second line: where, or what does not fit.
            let (l0, l1) = (geom.line_of(e.rows.0), geom.line_of(e.rows.1));
            let mut label = element_label(e);
            if hidden > 0 && l1 == l0 {
                label.push_str(&format!(" · ✂ {hidden} rows hidden"));
            }
            if let Some(sy) = visible(l0) {
                let fg = if hidden > 0 && l1 == l0 { Color::Rgb(170, 60, 0) } else { Color::Black };
                buf.set_stringn(left, sy, label, room, Style::new().fg(fg).bg(color).bold());
            }
            if l1 > l0
                && geom.lines.get(l0 + 1).is_some_and(|l| !l.2)
                && let Some(sy) = visible(l0 + 1)
            {
                let (info, fg) = if hidden > 0 {
                    (format!("✂ {hidden} rows hidden"), Color::Rgb(170, 60, 0))
                } else if crosses {
                    ("cut at the page break".to_string(), Color::Rgb(170, 0, 0))
                } else {
                    (format!("columns {}–{} · rows {}–{}", e.cols.0, e.cols.1, e.rows.0, e.rows.1), Color::Rgb(110, 110, 110))
                };
                buf.set_stringn(left, sy, info, room, Style::new().fg(fg).bg(color));
            }
        } else if hidden > 0
            && let Some(crate::print::Placed::Lines(lines)) = placed.get(n).map(|p| &p.content)
        {
            // The line that says how many rows do not fit.
            let at = lines.iter().position(|l| l.plain().starts_with('…')).unwrap_or(e.height() - 1).min(e.height() - 1);
            tint(buf, e.cols, (e.rows.0 + at, e.rows.0 + at), amber);
        }
    }
    if let Some(d) = app.layout_drag {
        tint(buf, d.cols, d.rows, Color::Rgb(205, 245, 205));
    }
    // Resize handle at the selected element's bottom-right corner.
    if let Some(e) = spec.elements.get(selected) {
        let (x, w) = spec.span(layout.cols, e);
        let sx = x0 + (x + w) as u16 - 1;
        if let Some(sy) = visible(geom.line_of(e.rows.1))
            && sx < right
        {
            buf[(sx, sy)].set_char('◢').set_fg(Color::Blue);
        }
    }
}

fn draw_chart(f: &mut Frame, app: &mut App, area: Rect, i: usize) {
    let area = Rect { x: area.x + 1, width: area.width.saturating_sub(2), ..area };
    match app.chart_data(i) {
        Ok(data) => {
            let highlight = app.chart_highlight(&data);
            let (hits, plot) = super::chart_view::render_with(f.buffer_mut(), area, &data, false, highlight, app.images_allowed);
            app.hits.extend(hits.into_iter().map(|(r, k)| (r, Hit::ChartItem(k))));
            if let Some(rect) = plot {
                app.images.push(super::chart_image::ChartImage { rect, data, paper: false });
            }
        }
        Err(e) => {
            f.buffer_mut().set_stringn(area.x, area.y, format!("⚠ chart: {e}"), area.width as usize, Style::new().fg(Color::LightRed));
            f.buffer_mut().set_stringn(area.x, area.y + 1, "edit the chart block in ‹› source", area.width as usize, Style::new().fg(Color::Gray));
        }
    }
}

fn draw_window_tabs(f: &mut Frame, app: &mut App, area: Rect, focused: bool) {
    let bg = if focused { Color::Blue } else { Color::DarkGray };
    f.buffer_mut().set_style(area, Style::new().bg(bg));
    let mut items = Vec::new();
    let sources = if focused { app.formula_sources() } else { Default::default() };
    for (label, hit, active) in app.window_tabs(app.left_focused) {
        let style = match (active, focused, hit) {
            (_, _, Hit::Tab(i)) if sources.table(i).is_some() => source_tab_style(sources.table(i).unwrap_or(0)),
            (true, true, _) => Style::new().bold().fg(Color::Black).bg(Color::White),
            (true, false, _) => Style::new().bold().fg(Color::Black).bg(Color::Gray),
            (false, ..) => Style::new().fg(Color::White).bg(bg),
        };
        items.push((format!(" {label} "), style, Some(hit)));
    }
    if app.can_pivot() {
        let style = if app.view.pivot.is_some() {
            Style::new().bold().fg(Color::Black).bg(Color::Yellow)
        } else {
            Style::new().fg(Color::Yellow).bg(bg)
        };
        items.push((" ⊞ pivot ".into(), style, Some(Hit::PivotButton)));
    }
    chips(f, area, items, &mut app.hits);
}

fn draw_inspector(f: &mut Frame, app: &mut App, area: Rect, focused: bool) {
    let lines = app.inspector();
    let h = area.height as usize;
    app.view.page_rows = h.max(1);
    app.view.scroll.0 = app.view.scroll.0.min(lines.len().saturating_sub(h));
    let scroll = app.view.scroll.0;
    let width = area.width as usize;
    let mut choice = 0;
    for (i, line) in lines.iter().enumerate() {
        let visible = i >= scroll && i < scroll + h;
        let y = area.y + i.saturating_sub(scroll) as u16;
        let buf = f.buffer_mut();
        match line {
            InspectorLine::Heading(t) if visible => {
                buf.set_stringn(area.x, y, format!(" {t}"), width, Style::new().bold().fg(Color::Cyan));
            }
            InspectorLine::Field(label, value) if visible => {
                let label = format!(" {label:<8} ");
                buf.set_stringn(area.x, y, &label, width, Style::new().fg(Color::Gray));
                let x = area.x + (label.width() as u16).min(area.width);
                buf.set_stringn(x, y, value, (area.right() - x) as usize, Style::new());
            }
            InspectorLine::Error(e) if visible => {
                buf.set_stringn(area.x, y, format!(" ⚠ {e}"), width, Style::new().fg(Color::LightRed));
            }
            InspectorLine::Choice { value, current } => {
                if visible {
                    let mark = if *current { "(•)" } else { "( )" };
                    let mut style = if *current { Style::new().bold().fg(Color::Green) } else { Style::new() };
                    if focused && choice == app.view.cursor.0 {
                        style = style.add_modifier(Modifier::REVERSED);
                    }
                    buf.set_stringn(area.x, y, fit(&format!("   {mark} {value}"), width, Align::Left), width, style);
                    app.hits.push((Rect::new(area.x, y, area.width, 1), Hit::Choice(choice)));
                }
                choice += 1;
            }
            _ => {}
        }
    }
}

fn draw_source(f: &mut Frame, app: &mut App, area: Rect, focused: bool) {
    let (lines, marked) = app.source_lines();
    let h = area.height as usize;
    app.view.page_rows = h.max(1);
    // Follow the main window when it moves to another block.
    if app.view.mark != Some(marked.start) {
        app.view.mark = Some(marked.start);
        app.view.follow = true;
    }
    let scroll = &mut app.view.scroll.0;
    if app.view.follow && !marked.is_empty() && (marked.start < *scroll || marked.start >= *scroll + h) {
        *scroll = marked.start.saturating_sub(2);
    }
    *scroll = (*scroll).min(lines.len().saturating_sub(h));
    let scroll = *scroll;
    let gutter = lines.len().to_string().len() + 1;
    let buf = f.buffer_mut();
    for (i, line) in lines.iter().enumerate().skip(scroll).take(h) {
        let y = area.y + (i - scroll) as u16;
        let in_block = marked.contains(&i);
        let base = if in_block { Style::new().bg(Color::Rgb(40, 44, 60)) } else { Style::new() };
        if in_block {
            buf.set_style(Rect::new(area.x, y, area.width, 1), base);
        }
        buf.set_stringn(area.x, y, format!("{:>w$} ", i + 1, w = gutter - 1), gutter, base.fg(Color::DarkGray));
        let style = if line.starts_with('#') {
            base.bold().fg(Color::Cyan)
        } else if line.starts_with("```") {
            base.fg(Color::Magenta)
        } else if line.starts_with('|') {
            base
        } else {
            base.fg(Color::Gray)
        };
        let x = area.x + gutter as u16;
        buf.set_stringn(x, y, line, (area.right().saturating_sub(x)) as usize, style);
    }
    let _ = focused;
}

/// The calculation sheet typeset: formulas in 2D (or one line each).
fn draw_calc_preview(f: &mut Frame, app: &mut App, area: Rect) {
    use super::app::PreviewKind;
    let lines = app.calc_preview();
    let h = area.height as usize;
    app.view.page_rows = h.max(1);
    let scroll = &mut app.view.scroll.0;
    *scroll = (*scroll).min(lines.len().saturating_sub(h));
    let scroll = *scroll;
    let buf = f.buffer_mut();
    if lines.is_empty() {
        buf.set_stringn(area.x + 1, area.y, "no calculation yet – :calc opens the sheet", area.width.saturating_sub(1) as usize, Style::new().fg(Color::DarkGray));
    }
    for (i, (line, kind)) in lines.iter().enumerate().skip(scroll).take(h) {
        let y = area.y + (i - scroll) as u16;
        let style = match kind {
            PreviewKind::Heading => Style::new().bold().fg(Color::Cyan),
            PreviewKind::Comment => Style::new().fg(Color::DarkGray),
            PreviewKind::Math => Style::new(),
            PreviewKind::Ok => Style::new().fg(Color::Green),
            PreviewKind::Fail => Style::new().fg(Color::Red),
            PreviewKind::Error => Style::new().fg(Color::Red).italic(),
        };
        buf.set_stringn(area.x + 1, y, line, area.width.saturating_sub(1) as usize, style);
    }
}

/// Draws chips left to right; records a hit region for those that have one.
fn chips(f: &mut Frame, area: Rect, items: Vec<(String, Style, Option<Hit>)>, hits: &mut Vec<(Rect, Hit)>) -> u16 {
    let mut x = area.x;
    for (text, style, hit) in items {
        if x >= area.right() {
            break;
        }
        let w = (text.width() as u16).min(area.right() - x);
        f.buffer_mut().set_stringn(x, area.y, &text, w as usize, style);
        if let Some(h) = hit {
            hits.push((Rect::new(x, area.y, w, 1), h));
        }
        x += w;
    }
    x
}

fn draw_tabs(f: &mut Frame, app: &mut App, area: Rect) {
    f.buffer_mut().set_style(area, Style::new().bg(Color::DarkGray));
    let mut items = vec![(" ≡ hardcalc ".to_string(), Style::new().bold().fg(Color::Black).bg(Color::Cyan), Some(Hit::Logo))];
    // Tables the edited formula takes data from, in their source colour.
    let sources = app.formula_sources();
    for (i, s) in app.wb.sheets.iter().enumerate() {
        let name = s.name.clone().unwrap_or_else(|| format!("#{}", i + 1));
        let active = i == app.view.sheet && app.view.kind == ViewKind::Table;
        let style = match sources.table(i) {
            Some(k) => source_tab_style(k),
            None => tab_style(active),
        };
        items.push((format!(" {name} "), style, Some(Hit::Tab(i))));
        items.push(("│".into(), Style::new().fg(Color::Gray).bg(Color::DarkGray), None));
    }
    items.push((" ⚙ types ".into(), tab_style(app.types_view()).italic(), Some(Hit::TypesTab)));
    items.push((" ƒ functions ".into(), tab_style(app.functions_view()).italic(), Some(Hit::ViewTab(ViewKind::Functions))));
    if app.has_calc() {
        items.push((" ∑ calc ".into(), tab_style(app.calc_view()).italic(), Some(Hit::ViewTab(ViewKind::Calc))));
    }
    for (i, (name, _)) in app.layout_blocks().into_iter().enumerate() {
        let active = app.view.kind == ViewKind::Layout(i);
        items.push((format!(" ▤ {name} "), tab_style(active), Some(Hit::ViewTab(ViewKind::Layout(i)))));
    }
    if app.can_pivot() {
        let style = if app.view.pivot.is_some() {
            Style::new().bold().fg(Color::Black).bg(Color::Yellow)
        } else {
            Style::new().fg(Color::Yellow).bg(Color::DarkGray)
        };
        items.push(("  ".into(), Style::new().bg(Color::DarkGray), None));
        items.push((" ⊞ pivot ".into(), style, Some(Hit::PivotButton)));
    }
    for (i, (name, _)) in app.chart_blocks().into_iter().enumerate() {
        items.push((format!(" ▥ {name} "), tab_style(false).fg(Color::LightGreen), Some(Hit::ViewTab(ViewKind::Chart(i)))));
    }
    let end = chips(f, area, items, &mut app.hits);
    draw_file_name(f, app, Rect { x: end, width: area.right().saturating_sub(end), ..area });
}

/// The tab of a table the edited formula takes data from.
fn source_tab_style(k: usize) -> Style {
    Style::new().bold().fg(Color::Black).bg(SOURCE_COLORS[k % SOURCE_COLORS.len()])
}

fn tab_style(active: bool) -> Style {
    if active {
        Style::new().bold().fg(Color::Black).bg(Color::White)
    } else {
        Style::new().fg(Color::White).bg(Color::DarkGray)
    }
}

/// The formula bar's text broken into lines of the room it has (by
/// characters, so every edit position has a place), and the caret as
/// (line, column) while editing.
fn bar_text(app: &App, width: u16) -> (Vec<String>, Option<(usize, usize)>) {
    let label = format!(" {:<6}", app.cursor_label());
    let x = label.width() + 2;
    let info_w = app.cursor_column_info().width() + 1;
    let room = (width as usize).saturating_sub(x + info_w + 1).max(8);
    let (text, caret) = match &app.mode {
        Mode::Edit { buf, cursor } => (buf.clone(), Some(*cursor)),
        _ => (app.cursor_raw(), None),
    };
    let mut lines = vec![String::new()];
    let mut used = 0;
    let mut at = None;
    let place = |lines: &Vec<String>, used: usize| (lines.len() - 1, used);
    for (i, c) in text.chars().enumerate() {
        if caret == Some(i) {
            at = Some(place(&lines, used));
        }
        let cw = c.width().unwrap_or(0);
        if used + cw > room {
            lines.push(String::new());
            used = 0;
            if caret == Some(i) {
                at = Some(place(&lines, used));
            }
        }
        lines.last_mut().unwrap().push(c);
        used += cw;
    }
    if caret.is_some() && at.is_none() {
        if used >= room {
            lines.push(String::new());
            used = 0;
        }
        at = Some(place(&lines, used));
    }
    (lines, at)
}

fn draw_formula_bar(f: &mut Frame, app: &mut App, area: Rect) {
    let label = format!(" {:<6}", app.cursor_label());
    f.buffer_mut().set_string(area.x, area.y, &label, Style::new().bold());
    let x = area.x + label.width() as u16;
    let info = app.cursor_column_info();
    let info_w = info.width() as u16 + 1;
    let (lines, caret) = bar_text(app, area.width);
    let height = area.height as usize;
    // Long contents: the lines around the caret.
    let first = caret.map_or(0, |(l, _)| (l + 1).saturating_sub(height));
    let editing = matches!(app.mode, Mode::Edit { .. });
    let style = if editing {
        Style::new().fg(Color::Yellow)
    } else if app.cursor_raw().starts_with('=') {
        Style::new().fg(Color::Cyan)
    } else {
        Style::new()
    };
    // Text that does not fit ends in an ellipsis.
    let cut = lines.len() > first + height;
    for (row, line) in lines.iter().skip(first).take(height).enumerate() {
        let y = area.y + row as u16;
        let mut line = line.clone();
        if cut && row + 1 == height {
            let room = (area.width as usize).saturating_sub(x as usize + 2 + info_w as usize + 1).max(8);
            while line.width() + 1 > room {
                line.pop();
            }
            line.push('…');
        }
        f.buffer_mut().set_string(x, y, "│ ", Style::new().fg(Color::DarkGray));
        f.buffer_mut().set_string(x + 2, y, &line, style);
    }
    if let Some((l, c)) = caret {
        let cx = x + 2 + c as u16;
        f.set_cursor_position((cx.min(area.right() - 1), area.y + (l - first) as u16));
    }
    if area.width > info_w + 10 {
        f.buffer_mut().set_string(area.right() - info_w, area.y, &info, Style::new().fg(Color::DarkGray));
    }
}

fn skip_width(s: &str, skip: usize) -> String {
    let mut w = 0;
    s.chars()
        .skip_while(|c| {
            let keep_skipping = w < skip;
            w += c.width().unwrap_or(0);
            keep_skipping
        })
        .collect()
}

fn draw_filters(f: &mut Frame, app: &mut App, area: Rect) {
    let Some(p) = app.view.pivot.clone() else { return };
    let dim = Style::new().fg(Color::DarkGray);
    let mut items = vec![(
        format!(" pivot  rows: {}  ×  columns: {}", app.axis_name(p.row_axis), app.axis_name(p.col_axis)),
        dim,
        None,
    )];
    for axis in 0..p.values.len() {
        if axis == p.row_axis || axis == p.col_axis {
            continue;
        }
        let value = p.values[axis].get(p.fixed[axis]).cloned().unwrap_or_default();
        items.push(("   ".into(), Style::new(), None));
        items.push((
            format!(" {} = {} ▸ ", app.axis_name(axis), value),
            Style::new().fg(Color::Black).bg(Color::Yellow),
            Some(Hit::Filter(axis)),
        ));
    }
    chips(f, area, items, &mut app.hits);
}

fn fit(text: &str, w: usize, align: Align) -> String {
    let tw = text.width();
    if tw > w {
        let mut out = String::new();
        let mut used = 0;
        for c in text.chars() {
            let cw = c.width().unwrap_or(0);
            if used + cw + 1 > w {
                break;
            }
            out.push(c);
            used += cw;
        }
        out.push('…');
        return format!("{out}{}", " ".repeat(w.saturating_sub(used + 1)));
    }
    let fill = w - tw;
    match align {
        Align::Right => format!("{}{text}", " ".repeat(fill)),
        Align::Center => format!("{}{text}{}", " ".repeat(fill / 2), " ".repeat(fill - fill / 2)),
        _ => format!("{text}{}", " ".repeat(fill)),
    }
}

fn to_color(c: &LayerColor) -> Option<Color> {
    match c {
        LayerColor::Named(n) => n.parse().ok(),
        LayerColor::Rgb(r, g, b) => Some(Color::Rgb(*r, *g, *b)),
    }
}

fn cell_style(st: &CellStyle) -> Style {
    let mut s = Style::new();
    if let Some(c) = st.fg.as_ref().and_then(to_color) {
        s = s.fg(c);
    }
    if let Some(c) = st.bg.as_ref().and_then(to_color) {
        s = s.bg(c);
    }
    if st.bold {
        s = s.add_modifier(Modifier::BOLD);
    }
    if st.italic {
        s = s.add_modifier(Modifier::ITALIC);
    }
    if st.underline {
        s = s.add_modifier(Modifier::UNDERLINED);
    }
    s
}

/// Order of a view's tab in the tab line, so a table switch knows its direction.
fn tab_order(kind: ViewKind) -> u32 {
    match kind {
        ViewKind::Table => 0,
        ViewKind::Types => 10_000,
        ViewKind::Functions => 10_001,
        ViewKind::Calc => 10_002,
        ViewKind::Layout(i) => 20_000 + i as u32,
        ViewKind::Chart(i) => 30_000 + i as u32,
        ViewKind::Inspector => 40_000,
        ViewKind::Source => 40_001,
        ViewKind::CalcPreview => 40_002,
    }
}

/// Colours of the sources of an edited formula, in the order they appear.
const SOURCE_COLORS: [Color; 5] = [Color::LightBlue, Color::LightMagenta, Color::LightGreen, Color::LightYellow, Color::LightRed];

/// Colour of a typeset calculation: that of its text, or one that reads on
/// the highlight of the cursor or the selection.
fn typeset_ink(style: Style, view: &CellView, light: bool) -> [u8; 3] {
    if style.add_modifier.contains(Modifier::REVERSED) {
        return if light { [250, 250, 250] } else { [28, 28, 30] };
    }
    match style.bg {
        Some(Color::Blue) => return [255, 255, 255],
        Some(Color::Cyan) | Some(Color::Yellow) => return [0, 0, 0],
        _ => {}
    }
    let named = match &view.style.fg {
        Some(crate::layers::Color::Named(n)) => n.as_str(),
        _ => "",
    };
    match (named, light) {
        _ if view.error => if light { [190, 0, 0] } else { [255, 110, 110] },
        ("cyan", false) => [0, 190, 210],
        ("cyan", true) => [0, 115, 145],
        ("green", false) => [80, 200, 100],
        ("green", true) => [0, 130, 40],
        ("red", false) => [240, 95, 95],
        ("red", true) => [190, 0, 0],
        (_, false) => [220, 220, 220],
        (_, true) => [20, 20, 20],
    }
}

fn draw_grid(f: &mut Frame, app: &mut App, area: Rect, focused: bool) {
    // The cells the formula being edited refers to.
    let sources = if focused { app.formula_sources() } else { Default::default() };
    let (rows, cols) = app.view_dims();
    let vis_rows = area.height.saturating_sub(HEADER_LINES) as usize;
    app.view.page_rows = vis_rows.max(1);

    // Vertical scrolling (to the cursor, or the cell picked for a formula).
    let focus = if focused { app.focus_cell() } else { app.view.cursor };
    if app.view.follow {
        if focus.0 < app.view.scroll.0 {
            app.view.scroll.0 = focus.0;
        } else if vis_rows > 0 && !app.rows_from(app.view.scroll.0, vis_rows).contains(&focus.0) {
            // The cursor row becomes the last one shown (rows the filter hides do not count).
            app.view.scroll.0 = app.step_row(focus.0, 1 - vis_rows as isize);
        }
    }
    app.view.scroll.0 = app.view.scroll.0.min(rows.saturating_sub(1));
    // Frozen columns stay at the left; the others scroll behind them.
    let frozen = app.frozen_cols().min(cols.saturating_sub(1));
    app.view.scroll.1 = app.view.scroll.1.min(cols.saturating_sub(1)).max(frozen);

    // Filling tables get Excel-like column widths.
    let min_col = if app.fills() { FILL_MIN_COL } else { MIN_COL };
    // The calculation sheet with typeset calculations: the formula of each row.
    let cell_px = app.cell_px;
    let typeset = app.calc_view() && app.typeset && app.images_allowed && crate::typeset::available();
    let math: HashMap<usize, String> = if typeset {
        let m: HashMap<usize, String> = (0..app.calc_rows().len()).filter_map(|r| app.calc_math(r).map(|m| (r, m))).collect();
        let list: Vec<String> = m.values().cloned().collect();
        app.typeset_formulas(&list, cell_px.1);
        m
    } else {
        HashMap::new()
    };
    let calc_col = crate::tui::app::CALCULATION;
    // Rows with wrapped cells are several lines high, so fewer rows fit; the
    // scroll position moves on until the cursor row is shown completely.
    let (row_list, cache, visible, heights, gutter) = loop {
        // The rows drawn: without those the filter hides.
        let mut row_list = app.rows_from(app.view.scroll.0, vis_rows);
        let gutter = {
            let headers = row_list.iter().map(|&r| app.row_header(r).width()).max().unwrap_or(1);
            headers.max(app.corner().width()).max(2) + 1
        };
        let avail = (area.width as usize).saturating_sub(gutter);
        // Column views are computed lazily, only for visible rows.
        let mut cache: HashMap<usize, (usize, Vec<CellView>)> = HashMap::new();
        // A cell with a link needs one column more for its ↗.
        let marks = app.view.kind == ViewKind::Table && app.view.pivot.is_none() && app.solo.is_none();
        let (sheet, links) = (app.view.sheet, app.links.clone());
        let mark = move |r: usize, c: usize| usize::from(marks && links.contains_key(&(sheet, r, c)));
        // In the calculation sheet the way to the result takes the room the
        // other columns leave.
        let calc_room = app.calc_view().then(|| {
            use crate::tui::app::{CALC_COLS, CALCULATION};
            let others: usize = (0..CALC_COLS.len())
                .filter(|&c| c != CALCULATION)
                .map(|c| {
                    let content = row_list.iter().map(|&r| app.cell_view(r, c).text.width()).max().unwrap_or(0);
                    content.max(app.col_header(c).1.width()).clamp(min_col, MAX_COL) + 1
                })
                .sum();
            avail.saturating_sub(others + 1).max(MAX_COL)
        });
        let mut column = |app: &mut App, c: usize| -> usize {
            if let Some((w, _)) = cache.get(&c) {
                return *w;
            }
            let cap = match calc_room {
                Some(room) if c == crate::tui::app::CALCULATION => room,
                _ => MAX_COL,
            };
            let views: Vec<CellView> = row_list.iter().map(|&r| app.cell_view(r, c)).collect();
            let (top, name) = app.col_header(c);
            let widest = views.iter().filter_map(|v| v.style.wrap).map(wrap_width).fold(cap, usize::max);
            let w = views
                .iter()
                .zip(row_list.iter().copied())
                .map(|(v, r)| match v.style.wrap {
                    Some(n) => v.text.width().min(wrap_width(n)) + mark(r, c),
                    None => v.text.width().min(cap) + mark(r, c),
                })
                .chain([top.width(), name.width()])
                .max()
                .unwrap_or(0)
                .clamp(min_col, widest);
            // A width set in the view layer wins over what the content needs.
            let w = app.col_width(c).unwrap_or(w);
            cache.insert(c, (w, views));
            w
        };
        // Columns from `start` that fit: (col, x offset, width, fully visible).
        let layout = |app: &mut App, column: &mut dyn FnMut(&mut App, usize) -> usize, start: usize| {
            let mut out = Vec::new();
            let mut x = 0;
            // The frozen columns first, then from `start`; hidden ones are left out.
            for c in (0..frozen).chain(start.max(frozen)..cols) {
                if x >= avail {
                    break;
                }
                if !app.col_shown(c) {
                    continue;
                }
                let w = column(app, c);
                out.push((c, x, w.min(avail - x), x + w <= avail));
                x += w + 1;
            }
            out
        };

        // Horizontal scrolling: move right until the cursor column fits completely.
        if app.view.follow {
            if focus.1 < app.view.scroll.1 && focus.1 >= frozen {
                app.view.scroll.1 = focus.1;
            }
            while app.view.scroll.1 < focus.1 {
                let l = layout(app, &mut column, app.view.scroll.1);
                if l.iter().any(|&(c, _, _, full)| c == focus.1 && full) {
                    break;
                }
                app.view.scroll.1 += 1;
            }
        }
        let visible = layout(app, &mut column, app.view.scroll.1);

        // Height of each row in lines; the rows that fit on the screen.
        // Every column counts, also those scrolled out of view, so that a row
        // keeps its height while scrolling sideways.
        let drawn: Vec<usize> = (0..cols).filter(|&c| app.col_shown(c)).collect();
        let all: Vec<(usize, usize)> = drawn.into_iter().map(|c| (c, column(app, c))).collect();
        let mut heights = Vec::new();
        let mut used = 0;
        for (i, &r) in row_list.iter().enumerate() {
            let h = all
                .iter()
                .map(|&(c, w)| {
                    let v = &cache[&c].1[i];
                    let w = w.saturating_sub(mark(r, c)).max(1);
                    if v.style.wrap.is_some() { crate::text::wrap_plain(&v.text, w).len() } else { 1 }
                })
                .max()
                .unwrap_or(1);
            // A typeset calculation takes the lines its picture needs.
            let typeset_h = math
                .get(&r)
                .zip(all.iter().find(|(c, _)| *c == calc_col))
                .and_then(|(m, &(_, w))| app.typeset_fitted(m, cell_px.1, w * cell_px.0 as usize, [255, 255, 255]))
                .map_or(1, |(_, lines)| lines as usize);
            let h = h.max(typeset_h);
            if used + h > vis_rows && !heights.is_empty() {
                break;
            }
            used += h;
            heights.push(h);
        }
        let shown = heights.len();
        if app.view.follow && focus.0 < rows && !row_list[..shown].contains(&focus.0) && app.view.scroll.0 < focus.0 {
            app.view.scroll.0 = row_list.get(1).copied().unwrap_or(app.view.scroll.0 + 1);
            continue;
        }
        row_list.truncate(shown);
        break (row_list, cache, visible, heights, gutter);
    };

    let ((r0, c0), (r1, c1)) = app.selection();
    let has_range = app.has_range();
    // The reference being picked for the formula in the focused window.
    let pointed = if focused { app.point_range() } else { None };
    // The cells the fill handle is being dragged over.
    let filling = if focused { app.fill_preview() } else { None };
    // The fill handle sits right of the selection's bottom-right cell.
    let handle = (focused
        && matches!(app.mode, Mode::Normal)
        && app.on_header().is_none()
        && app.view.kind == ViewKind::Table
        && app.view.pivot.is_none()
        && app.footer_at(app.selection().1 .0).is_none())
        .then(|| app.selection().1);
    let on_header = app.on_header();
    // Header cells of selected columns / rows, and the one with the cursor.
    let header_sel = Style::new().bold().fg(Color::White).bg(Color::Blue);
    let header_cursor = header_sel.add_modifier(Modifier::REVERSED);
    let edit = match &app.mode {
        Mode::Edit { buf, cursor } if focused => Some((buf.clone(), *cursor)),
        _ => None,
    };
    let editing = edit.is_some();
    let mut edit_cursor = None;
    let header_style = Style::new().fg(Color::Gray);
    let (light, truecolor, bands) = (app.light(), app.truecolor, app.bands);
    let (data_rows, data_cols) = app.dims();
    // Filling tables are banded to the edges.
    let with_placeholders = app.view.pivot.is_none() && app.view.kind == ViewKind::Table && !app.fills();
    let placeholder = |r: usize, c: usize| with_placeholders && (r >= data_rows || c >= data_cols);
    // Cells with a link carry a ↗ at their right edge; a click on it jumps.
    let links_on = app.view.kind == ViewKind::Table && app.view.pivot.is_none() && app.solo.is_none();
    let sheet = app.view.sheet;
    let buf = f.buffer_mut();

    // Corner and column headers.
    buf.set_stringn(area.x, area.y + 1, fit(&app.corner(), gutter - 1, Align::Left), gutter - 1, header_style.italic());
    if on_header == Some(Header::Corner) {
        // The table origin: yellow while its name is edited (in the formula bar).
        let style = if editing { Style::new().fg(Color::Black).bg(Color::Yellow) } else { header_cursor };
        for y in area.y..area.y + HEADER_LINES {
            buf.set_stringn(area.x, y, " ".repeat(gutter - 1), gutter - 1, style);
        }
    }
    if app.view.kind == ViewKind::Table && app.view.pivot.is_none() {
        app.hits.push((Rect::new(area.x, area.y, (gutter - 1) as u16, HEADER_LINES), Hit::Corner));
    }
    for &(c, x, w, _) in &visible {
        let x = area.x + (gutter + x) as u16;
        let (top, name) = app.col_header(c);
        let current = c == app.view.cursor.1;
        let style = match on_header {
            Some(Header::Column) if current => header_cursor,
            Some(Header::Column) if (c0..=c1).contains(&c) => header_sel,
            Some(Header::Corner) => header_sel,
            _ if current => Style::new().bold().fg(Color::Black).bg(Color::Gray),
            _ => header_style,
        };
        buf.set_stringn(x, area.y, fit(&top, w, Align::Left), w, style.remove_modifier(Modifier::BOLD).italic());
        buf.set_stringn(x, area.y + 1, fit(&name, w, Align::Left), w, style.bold().underlined());
        // The ▾ / ▼ behind the letter opens the value list of the column.
        if top.ends_with(['▾', '▼']) && top.width() <= w {
            let at = top.width().saturating_sub(2);
            app.hits.push((Rect::new(x + at as u16, area.y, (top.width() - at) as u16, 1), Hit::Values(c)));
        }
        app.hits.push((Rect::new(x, area.y, w as u16, HEADER_LINES), Hit::ColHeader(c)));
    }

    let first_row = row_list.first().copied().unwrap_or(app.view.scroll.0);
    // With hidden rows, the bands alternate over the rows that are shown.
    let band_base = if app.hidden_count().is_some() { (0..first_row).filter(|&r| !app.row_hidden(r)).count() } else { first_row };
    // A row the filter would hide, shown because the cursor is in it.
    let strays: Vec<bool> = row_list.iter().map(|&r| app.row_hidden(r)).collect();
    let mut y_off = 0u16;
    for (i, &r) in row_list.iter().enumerate() {
        let odd = if strays.contains(&true) || band_base != first_row { (band_base + i) % 2 == 1 } else { r % 2 == 1 };
        let y = area.y + HEADER_LINES + y_off;
        let lines = heights[i] as u16;
        y_off += lines;
        let current_row = r == app.view.cursor.0;
        let gstyle = match on_header {
            Some(Header::Row) if current_row => header_cursor,
            Some(Header::Row) if (r0..=r1).contains(&r) => header_sel,
            Some(Header::Corner) => header_sel,
            _ if current_row => Style::new().bold().fg(Color::Black).bg(Color::Gray),
            _ => header_style,
        };
        let gstyle = if strays[i] { gstyle.italic() } else { gstyle };
        buf.set_stringn(area.x, y, fit(&app.row_header(r), gutter - 1, Align::Right), gutter - 1, gstyle);
        for extra in 1..lines {
            buf.set_stringn(area.x, y + extra, " ".repeat(gutter - 1), gutter - 1, gstyle);
        }
        app.hits.push((Rect::new(area.x, y, (gutter - 1) as u16, lines), Hit::RowHeader(r)));

        let shade = |c: usize| bands.then(|| super::theme::band(odd, c % 2 == 1, light, truecolor)).flatten().filter(|_| !placeholder(r, c));
        for (k, &(c, x, w, _)) in visible.iter().enumerate() {
            let x = area.x + (gutter + x) as u16;
            // The gap to the next data column carries the row's band.
            if let Some(&(next, ..)) = visible.get(k + 1)
                && bands
                && !placeholder(r, c)
                && !placeholder(r, next)
                && let Some(bg) = super::theme::band(odd, false, light, truecolor)
            {
                for extra in 0..lines {
                    buf.set_string(x + w as u16, y + extra, " ", Style::new().bg(bg));
                }
            }
            let view = &cache[&c].1[i];
            let mut style = cell_style(&view.style);
            if view.style.bg.is_none()
                && let Some(bg) = shade(c)
            {
                style = style.bg(bg);
            }
            if view.error {
                style = style.fg(Color::LightRed);
            }
            if view.violation {
                style = style.fg(Color::White).bg(Color::Red);
            }
            // On a header the whole shown column / row is highlighted, also
            // beyond the data (the selection itself covers the data only).
            let selected = match on_header {
                Some(Header::Column) => (c0..=c1).contains(&c),
                Some(Header::Row) => (r0..=r1).contains(&r),
                Some(Header::Corner) => true,
                None => has_range && (r0..=r1).contains(&r) && (c0..=c1).contains(&c),
            };
            if selected {
                style = style.bg(Color::Blue).fg(Color::White);
            }
            if let Some(k) = sources.at(r, c) {
                style = style.bg(SOURCE_COLORS[k % SOURCE_COLORS.len()]).fg(Color::Black);
            }
            if let Some(((p0, q0), (p1, q1))) = pointed
                && (p0..=p1).contains(&r)
                && (q0..=q1).contains(&c)
            {
                style = style.bg(Color::Cyan).fg(Color::Black);
            }
            if let Some(((p0, q0), (p1, q1))) = filling
                && (p0..=p1).contains(&r)
                && (q0..=q1).contains(&c)
            {
                style = style.bg(Color::Cyan).fg(Color::Black);
            }
            if (r, c) == app.view.cursor && on_header.is_some() {
                // The cursor is on the header.
            } else if (r, c) == app.view.cursor && !focused {
                style = style.add_modifier(Modifier::UNDERLINED | Modifier::BOLD);
            } else if (r, c) == app.view.cursor {
                style = if editing { style.fg(Color::Black).bg(Color::Yellow) } else { style.add_modifier(Modifier::REVERSED) };
            }
            let edited = editing && (r, c) == app.view.cursor && on_header.is_none();
            // A link keeps the last column for its ↗, so the text (also a
            // right-aligned number) ends before it.
            let marked = links_on && w >= 2 && !edited && app.links.contains_key(&(sheet, r, c));
            let tw = if marked { w - 1 } else { w };
            let wrapped = view.style.wrap.is_some() && !edited;
            // The calculation typeset, in a colour that reads on the cell's background.
            let picture = math.get(&r).filter(|_| c == calc_col && !edited).and_then(|m| {
                let ink = typeset_ink(style, view, light);
                app.typeset_fitted(m, cell_px.1, w * cell_px.0 as usize, ink).map(|(img, _)| {
                    use std::hash::{Hash, Hasher};
                    let mut h = std::collections::hash_map::DefaultHasher::new();
                    (m, ink, w).hash(&mut h);
                    super::Picture { rect: Rect::new(x, y, w as u16, lines), key: h.finish(), image: img }
                })
            });
            let text = match &edit {
                // While editing, the cell shows the input; if it is too long,
                // the part around the cursor (normally the end).
                Some((input, pos)) if (r, c) == app.view.cursor && on_header.is_none() => {
                    let before: String = input.chars().take(*pos).collect();
                    let skip = (before.width() + 1).saturating_sub(w);
                    edit_cursor = Some((x + (before.width() - skip) as u16, y));
                    fit(&clip(&skip_width(input, skip), w), w, Align::Left)
                }
                _ => fit(&view.text, tw, view.align) + if marked { " " } else { "" },
            };
            if let Some(p) = picture {
                for extra in 0..lines {
                    buf.set_stringn(x, y + extra, " ".repeat(w), w, style);
                }
                app.pictures.push(p);
            } else if wrapped {
                let mut parts = crate::text::wrap_plain(&view.text, tw).into_iter();
                for extra in 0..lines {
                    let part = parts.next().unwrap_or_default();
                    let gap = if marked { " " } else { "" };
                    buf.set_stringn(x, y + extra, fit(&part, tw, view.align) + gap, w, style);
                }
            } else {
                buf.set_stringn(x, y, text, w, style);
                for extra in 1..lines {
                    buf.set_stringn(x, y + extra, " ".repeat(w), w, style);
                }
            }
            if marked {
                let mx = x + w as u16 - 1;
                // In the cell's own colours, so it reads like its text.
                buf.set_string(mx, y, "↗", style);
                app.hits.push((Rect::new(mx, y, 1, 1), Hit::Link(r, c)));
            }
            app.hits.push((Rect::new(x, y, w as u16, lines), Hit::Cell(r, c)));
            if handle == Some((r, c)) && x + (w as u16) < area.right() {
                let hx = x + w as u16;
                buf.set_string(hx, y + lines - 1, "▪", Style::new().fg(Color::Cyan).bold());
                app.hits.push((Rect::new(hx, y + lines - 1, 1, 1), Hit::FillHandle));
            }
        }
    }
    // A line behind the frozen columns.
    if frozen > 0
        && let Some(&(_, x, w, _)) = visible.iter().rfind(|v| v.0 < frozen)
    {
        let sx = area.x + (gutter + x + w) as u16;
        if sx < area.right() {
            for y in area.y..(area.y + HEADER_LINES + y_off).min(area.bottom()) {
                f.buffer_mut()[(sx, y)].set_symbol("│").set_fg(Color::DarkGray);
            }
        }
    }
    // Data flows from the sources to the formula cell; from sources out of
    // sight it comes in at the edge on their side.
    if !sources.cells.is_empty() && app.anim.enabled && !visible.is_empty() {
        let grid = Rect {
            x: area.x + gutter as u16,
            y: area.y + HEADER_LINES,
            width: area.width.saturating_sub(gutter as u16),
            height: area.height.saturating_sub(HEADER_LINES),
        };
        // Screen columns / lines of cells `a..=b` on one axis: the shown part,
        // or one line just outside the edge they lie beyond.
        enum Span {
            Shown(u16, u16),
            Before,
            After,
        }
        let col_span = |c0: usize, c1: usize| {
            let shown: Vec<(u16, u16)> =
                visible.iter().filter(|&&(c, ..)| (c0..=c1).contains(&c)).map(|&(_, x, w, _)| (grid.x + x as u16, grid.x + (x + w) as u16)).collect();
            match (shown.first(), shown.last()) {
                (Some(a), Some(b)) => Span::Shown(a.0, b.1),
                _ if c1 < visible[0].0 => Span::Before,
                _ => Span::After,
            }
        };
        let mut lines = Vec::new(); // (row, first line, height)
        let mut y = grid.y;
        for (i, &r) in row_list.iter().enumerate() {
            lines.push((r, y, heights[i] as u16));
            y += heights[i] as u16;
        }
        let row_span = |r0: usize, r1: usize| {
            let shown: Vec<(u16, u16)> = lines.iter().filter(|&&(r, ..)| (r0..=r1).contains(&r)).map(|&(_, y, h)| (y, y + h)).collect();
            match (shown.first(), shown.last()) {
                (Some(a), Some(b)) => Span::Shown(a.0, b.1),
                _ if r1 < first_row => Span::Before,
                _ => Span::After,
            }
        };
        let rect = |(r0, c0): (usize, usize), (r1, c1): (usize, usize)| {
            let (x, w, x_side) = match col_span(c0, c1) {
                Span::Shown(a, b) => (a, b - a, 0),
                Span::Before => (grid.x.saturating_sub(1), 1, -1),
                Span::After => (grid.right(), 1, 1),
            };
            let (y, h, y_side) = match row_span(r0, r1) {
                Span::Shown(a, b) => (a, b - a, 0),
                Span::Before => (grid.y.saturating_sub(1), 1, -1),
                Span::After => (grid.bottom(), 1, 1),
            };
            // Out of sight: an arrow at the edge points to it.
            let arrow = match (y_side, x_side) {
                (-1, _) => Some('▴'),
                (1, _) => Some('▾'),
                (_, -1) => Some('◂'),
                (_, 1) => Some('▸'),
                _ => None,
            };
            (Rect::new(x, y, w, h), arrow)
        };
        let cursor = app.view.cursor;
        if let (target, None) = rect(cursor, cursor) {
            let flows: Vec<(Rect, Color, Option<char>)> = sources
                .cells
                .iter()
                .filter_map(|&((a, b), k)| {
                    let (r, arrow) = rect(a, b);
                    (!r.intersects(target)).then_some((r, SOURCE_COLORS[k % SOURCE_COLORS.len()], arrow))
                })
                .collect();
            app.anim.flow(f.buffer_mut(), grid, target, &flows);
        }
    }
    if let Some(pos) = edit_cursor {
        f.set_cursor_position(pos);
    }
}

/// Column width in characters of a `wrap` rule (0: the usual maximum).
fn wrap_width(n: u16) -> usize {
    if n == 0 { MAX_COL } else { n as usize }
}

/// The longest prefix of `s` that fits into `w` columns.
fn clip(s: &str, w: usize) -> String {
    let mut used = 0;
    s.chars()
        .take_while(|c| {
            used += c.width().unwrap_or(0);
            used <= w
        })
        .collect()
}

fn draw_fkeys(f: &mut Frame, app: &mut App, area: Rect) {
    let keys = app.fkeys();
    let n = keys.len() as u16;
    let slot = (area.width / n).max(1);
    let label_style = Style::new().fg(Color::Black).bg(Color::Cyan);
    for (i, (key, label)) in keys.iter().enumerate() {
        let x = area.x + i as u16 * slot;
        if x >= area.right() {
            break;
        }
        let w = if i + 1 == keys.len() { area.right() - x } else { slot.min(area.right() - x) };
        let num = key.to_string();
        let buf = f.buffer_mut();
        // A key without a function here: its number steps back.
        let num_style = Style::new().fg(if label.is_empty() { Color::DarkGray } else { Color::White });
        buf.set_stringn(x, area.y, &num, w as usize, num_style);
        let lw = (w as usize).saturating_sub(num.len());
        if lw > 0 {
            buf.set_stringn(x + num.len() as u16, area.y, fit(label, lw, Align::Left), lw, label_style);
        }
        if !label.is_empty() {
            app.hits.push((Rect::new(x, area.y, w, 1), Hit::FKey(*key)));
        }
    }
}

fn draw_layers(f: &mut Frame, app: &mut App, area: Rect) {
    f.buffer_mut().set_style(area, Style::new().bg(Color::Black));
    let mut items = vec![(" layers ".to_string(), Style::new().fg(Color::Gray).bg(Color::Black), None)];
    let scope = if app.layers_global {
        (" ⊕ all tables ".to_string(), Style::new().fg(Color::Black).bg(Color::Cyan))
    } else {
        (" ⊙ this table ".to_string(), Style::new().fg(Color::Black).bg(Color::Yellow))
    };
    items.push((scope.0, scope.1, Some(Hit::LayerScope)));
    items.push(("  ".into(), Style::new().bg(Color::Black), None));
    if app.layer_names.is_empty() {
        items.push(("(none)".into(), Style::new().fg(Color::DarkGray).bg(Color::Black), None));
    }
    for (i, name) in app.layer_names.iter().enumerate() {
        let (mark, style) = if app.solo.as_deref() == Some(name.as_str()) {
            ("◉", Style::new().bold().fg(Color::White).bg(Color::Magenta))
        } else if app.layer_empty(name) {
            // No rules for this table yet: nothing to show or hide.
            ("○", Style::new().fg(Color::DarkGray).bg(Color::Black))
        } else if app.layer_visible(name) {
            ("■", Style::new().fg(Color::Black).bg(Color::Green))
        } else {
            ("□", Style::new().fg(Color::Gray).bg(Color::DarkGray))
        };
        // `*`: this table has its own setting for the layer.
        let local = if app.layer_is_local(name) { "*" } else { "" };
        // The filter says how many rows it shows: `filter 2/6`.
        let count = match app.hidden_count().filter(|_| name == crate::tui::app::FILTER) {
            Some((hidden, all)) => format!(" {}/{all}", all - hidden),
            // The view says how many columns it hides: `view −2`.
            None if name == crate::tui::app::VIEW && app.hidden_cols() > 0 => format!(" −{}", app.hidden_cols()),
            None => String::new(),
        };
        items.push((format!(" {mark} {name}{local}{count} "), style, Some(Hit::Layer(i))));
        items.push((" ".into(), Style::new().bg(Color::Black), None));
    }
    // Too many for the line: layers without rules for this table make room
    // (`:layer new` still adds them), the last ones first.
    let width = |items: &[(String, Style, Option<Hit>)]| items.iter().map(|(t, ..)| t.width()).sum::<usize>();
    while width(&items) > area.width as usize {
        let empty = items.iter().rposition(|(text, _, hit)| matches!(hit, Some(Hit::Layer(_))) && text.starts_with(" ○ "));
        match empty {
            Some(k) => {
                items.drain(k..=(k + 1).min(items.len() - 1));
            }
            None => break,
        }
    }
    chips(f, area, items, &mut app.hits);

    let n = app.violations.len();
    if n > 0 && app.layer_visible(VALIDATE) {
        let text = format!(" ⚠ {n} violation{} ", if n == 1 { "" } else { "s" });
        let w = text.width() as u16;
        if w < area.width / 2 {
            f.buffer_mut().set_string(area.right() - w, area.y, text, Style::new().fg(Color::White).bg(Color::Red));
        }
    }
}

/// The slices and scenarios of the shown table (its z axis), a line above the
/// layer bar: a click shows one; scenarios are marked ◇.
fn draw_data_layers(f: &mut Frame, app: &mut App, area: Rect) {
    f.buffer_mut().set_style(area, Style::new().bg(Color::Black));
    let table = app.table().name.clone().unwrap_or_default();
    let mut items = vec![(format!(" ▤ {table} "), Style::new().fg(Color::Gray).bg(Color::Black), None)];
    for z in 0..=app.layer_count() {
        let style = if z == app.shown_layer() {
            Style::new().bold().fg(Color::Black).bg(Color::Yellow)
        } else {
            Style::new().fg(Color::White).bg(Color::DarkGray)
        };
        let scenario = z > 0 && app.table().layers[z - 1].kind == crate::model::LayerKind::Scenario;
        let mark = if scenario { "◇ " } else { "" };
        items.push((format!(" {mark}{} ", app.layer_title(z)), style, Some(Hit::DataLayer(z))));
        items.push((" ".into(), Style::new().bg(Color::Black), None));
    }
    chips(f, area, items, &mut app.hits);
}

/// Status hint of the focused window when it is a helper, not a data window.
fn helper_hint(app: &App) -> Option<&'static str> {
    if !app.is_split() {
        return None;
    }
    match app.view.kind {
        ViewKind::Inspector => Some("inspector: ↑↓ choose · Enter applies the choice to the cell · Tab: back to the table"),
        ViewKind::Source => Some("source: ↑↓ PgUp PgDn Home End scroll · Tab: back to the table"),
        ViewKind::Chart(_) => Some("chart: t changes the type · ↑↓ move the table cursor · Tab: back to the table"),
        ViewKind::CalcPreview => Some("formulas: f switches 2d / text / ascii · ↑↓ PgUp PgDn scroll · Tab: back to the table"),
        _ => None,
    }
}

/// `helper`: hint of a focused helper window; it replaces the data window's
/// hint and notes (messages still come first).
fn draw_status(f: &mut Frame, app: &mut App, area: Rect, helper: Option<&str>) {
    match &app.mode {
        Mode::Command { buf } => {
            let text = format!(":{buf}");
            f.buffer_mut().set_stringn(area.x, area.y, &text, area.width as usize, Style::new());
            f.set_cursor_position((area.x + (text.width() as u16).min(area.width - 1), area.y));
            return;
        }
        Mode::Edit { .. } => {
            let (hint, style) = match &app.message {
                Some(m) if m.error => (m.text.clone(), Style::new().fg(Color::LightRed).bold()),
                _ => ("editing – Enter/Tab commit · Esc cancel · Ctrl+G special characters (α ≤ ² →)".to_string(), Style::new().fg(Color::Yellow)),
            };
            f.buffer_mut().set_stringn(area.x, area.y, hint, area.width as usize, style);
            return;
        }
        Mode::Normal => {}
    }
    let (text, style) = if let Some(m) = &app.message {
        (m.text.clone(), if m.error { Style::new().fg(Color::LightRed).bold() } else { Style::new().fg(Color::Green) })
    } else if let Some(hint) = helper {
        (hint.to_string(), Style::new().fg(Color::DarkGray))
    } else if let Some(problem) = app.cursor_problem() {
        (problem, Style::new().fg(Color::LightRed))
    } else if let Some(comment) = app.cursor_comment() {
        (format!("✎ {comment}"), Style::new().fg(Color::Magenta))
    } else {
        (
            if let Some(layer) = &app.solo {
                format!("layer view: {layer} · type to edit the {layer} rule of the cell · right-click the layer or Esc: back")
            } else if matches!(app.view.kind, ViewKind::Layout(_)) && app.layout_grab {
                "moving: arrows move · + − taller/shorter (rest follows) · > < wider/narrower · = fit height · a auto height · Enter / Esc / F3 put it down".to_string()
            } else if matches!(app.view.kind, ViewKind::Layout(_)) {
                "arrows: select · F3: move and resize · Space: overview · mouse: drag, ◢ resize, draw new".to_string()
            } else if app.calc_view() {
                "calculation: f_d = f_tk / γ_M · checks n ≤ f · cells Loads!C12 · Name and Comment beside the input · ## heading · #result/#formula/#full".into()
            } else if app.functions_view() {
                "own functions: name, parameters, definition, description · + row adds one · F8 deletes".into()
            } else if app.types_view() {
                "your types: edit Type or Definition · + row adds a type · F8 deletes · Esc back".into()
            } else if let Some((hidden, all)) = app.hidden_count() {
                format!("✂ filter: {} of {all} rows shown · click the filter layer or :filter off for all rows · : command", all - hidden)
            } else {
                "type or double-click to edit · Shift+arrows or drag to select · F9 menu · : command".into()
            },
            Style::new().fg(Color::DarkGray),
        )
    };
    // After a jump, the way back: ← and where it leads.
    let back = app.back_label().map(|label| format!(" ← {label} "));
    let room = back.as_ref().map_or(0, |b| b.width() as u16 + 1);
    let room = if room * 2 <= area.width { room } else { 0 };
    f.buffer_mut().set_stringn(area.x, area.y, format!(" {text}"), (area.width - room) as usize, style);
    if let Some(chip) = back.filter(|_| room > 0) {
        let x = area.right() - room + 1;
        f.buffer_mut().set_string(x, area.y, &chip, Style::new().fg(Color::White).bg(Color::DarkGray));
        app.hits.push((Rect::new(x, area.y, room - 1, 1), Hit::Back));
    }
}

/// Writes `text` starting at screen column `x` (may be left of `left`),
/// clipped to [left, right).
fn put_clipped(buf: &mut ratatui::buffer::Buffer, x: i32, y: u16, text: &str, style: Style, left: u16, right: u16) {
    let mut cx = x;
    for ch in text.chars() {
        let w = ch.width().unwrap_or(0) as i32;
        if cx >= right as i32 {
            break;
        }
        if cx >= left as i32 && cx + w <= right as i32 {
            buf[(cx as u16, y)].set_char(ch).set_style(style);
        }
        cx += w;
    }
}

fn draw_preview(f: &mut Frame, app: &mut App, area: Rect) {
    if app.image_mode {
        if app.graphics.is_some() {
            return draw_preview_image_slot(f, app, area);
        }
        if draw_preview_blocks(f, app, area) {
            return;
        }
    }
    let Some(p) = app.preview.as_mut() else { return };
    let l = &p.layout;
    let (mx, my) = l.margin_cells();
    let paper_w = l.cols + 2 * mx;
    let paper_h = l.rows + 2 * my;
    let (aw, ah) = (area.width as usize, area.height as usize);

    // Center the paper if it fits, otherwise scroll.
    p.scroll.0 = p.scroll.0.min(paper_w.saturating_sub(aw));
    p.scroll.1 = p.scroll.1.min((paper_h + 2).saturating_sub(ah));
    let (pad_x, sx) = if paper_w + 2 <= aw { ((aw - paper_w) / 2, 0) } else { (0, p.scroll.0) };
    let (pad_y, sy) = if paper_h + 2 <= ah { (1, 0) } else { (0, p.scroll.1) };

    let buf = f.buffer_mut();
    buf.set_style(area, Style::new().bg(Color::DarkGray));
    let paper = Style::new().bg(Color::White).fg(Color::Black);
    let left = area.x;
    let right = area.right();
    let page = &l.pages[p.page];
    for y in 0..ah {
        let Some(py) = (y + sy).checked_sub(pad_y) else { continue };
        if py >= paper_h {
            continue;
        }
        let sy_screen = area.y + y as u16;
        let x0 = left as i32 + pad_x as i32 - sx as i32;
        put_clipped(buf, x0, sy_screen, &" ".repeat(paper_w), paper, left, right);
        let Some(li) = py.checked_sub(my) else { continue };
        let Some(line) = page.get(li) else { continue };
        let tx = x0 + mx as i32;
        if line.kind == LineKind::Header {
            let band = Style::new().bg(Color::Rgb(220, 220, 220)).fg(Color::Black).underlined();
            put_clipped(buf, tx, sy_screen, &" ".repeat(line.width), band, left, right);
        }
        for span in &line.spans {
            let mut style = if line.kind == LineKind::Header {
                Style::new().bg(Color::Rgb(220, 220, 220)).underlined()
            } else {
                paper
            };
            let (r, g, b) = span.style.color.unwrap_or((0, 0, 0));
            style = style.fg(Color::Rgb(r, g, b));
            // Same fills as on the PDF page: the cell colour, then the gray shade on top.
            if let Some((r, g, b)) = span.style.bg {
                style = style.bg(Color::Rgb(r, g, b));
            }
            if span.style.shade {
                style = style.bg(Color::Rgb(230, 230, 230));
            }
            if span.style.bold {
                style = style.bold();
            }
            if span.style.italic {
                style = style.italic();
            }
            put_clipped(buf, tx + span.col as i32, sy_screen, &span.text, style, left, right);
        }
    }

    // Charts: rendered off-screen at full size, then the visible part is copied.
    let top = area.y as i32 + pad_y as i32 - sy as i32;
    let x0 = left as i32 + pad_x as i32 - sx as i32 + mx as i32;
    for g in &l.graphics[p.page] {
        let size = Rect::new(0, 0, l.cols as u16, g.height as u16);
        let mut tmp = ratatui::buffer::Buffer::empty(size);
        super::chart_view::render(&mut tmp, size, &g.data, true, None);
        let y0 = top + (my + g.line) as i32;
        for y in 0..size.height {
            for x in 0..size.width {
                let (sx2, sy2) = (x0 + x as i32, y0 + y as i32);
                if sx2 >= left as i32 && sx2 < right as i32 && sy2 >= area.y as i32 && sy2 < area.bottom() as i32 {
                    buf[(sx2 as u16, sy2 as u16)] = tmp[(x, y)].clone();
                }
            }
        }
    }

    // Typeset formulas as pictures over the text standing in for them, where
    // the terminal shows images and the picture is wholly in view.
    if !app.images_allowed {
        return;
    }
    let cw = crate::print::CHAR_W * l.font_size;
    let wanted: Vec<(Rect, u64, std::sync::Arc<crate::typeset::Image>, f32)> = l
        .pictures
        .get(p.page)
        .into_iter()
        .flatten()
        .filter_map(|pic| {
            let (x, y) = (x0 + pic.col as i32, top + (my + pic.line) as i32);
            let (w, h) = ((pic.width / cw).ceil() as i32, crate::print::picture_lines(pic.height, l.font_size) as i32);
            let inside = x >= left as i32 && x + w <= right as i32 && y >= area.y as i32 && y + h <= area.bottom() as i32;
            inside.then(|| (Rect::new(x as u16, y as u16, w as u16, h as u16), pic.key, pic.image.clone(), pic.width / cw))
        })
        .collect();
    let cell = app.cell_px;
    if app.typeset_preview.len() > 500 {
        app.typeset_preview.clear();
    }
    for (rect, key, image, cells_wide) in wanted {
        buf.set_style(rect, paper);
        for y in rect.y..rect.bottom() {
            buf.set_stringn(rect.x, y, " ".repeat(rect.width as usize), rect.width as usize, paper);
        }
        let shrunk = app
            .typeset_preview
            .entry((key, cell))
            .or_insert_with(|| std::rc::Rc::new(image.shrink(cells_wide as f64 * cell.0 as f64 / image.width as f64)))
            .clone();
        app.pictures.push(super::Picture { rect, key, image: shrunk });
    }
}

/// Page area of the given size in cells, centered, with the page's aspect
/// ratio (cells are about twice as high as wide).
fn page_slot(app: &App, area: Rect) -> Option<Rect> {
    let p = app.preview.as_ref()?;
    let (across, down) = p.grid();
    let (sw, sh) = if p.zoom > 0 { crate::print::sheet_size(&p.layout, across, down) } else { (p.layout.width, p.layout.height) };
    let aspect = sw / sh;
    let mut rows = area.height.saturating_sub(2).max(1);
    let mut cols = (rows as f32 * 2.0 * aspect).round() as u16;
    if cols > area.width.saturating_sub(2) {
        cols = area.width.saturating_sub(2).max(1);
        rows = ((cols as f32 / (2.0 * aspect)).round() as u16).max(1);
    }
    Some(Rect::new(area.x + (area.width - cols) / 2, area.y + (area.height - rows) / 2, cols, rows))
}

/// Overview of the real PDF page in block graphics (`▀`: upper pixel in the
/// foreground colour, lower one in the background) for terminals without an
/// image protocol. Returns false if the page could not be rendered.
fn draw_preview_blocks(f: &mut Frame, app: &mut App, area: Rect) -> bool {
    let Some(slot) = page_slot(app, area) else { return false };
    let img = match app.preview_thumbnail(slot.width as usize, slot.height as usize * 2) {
        Ok(img) => img,
        Err(e) => {
            app.image_mode = false;
            app.message = Some(super::app::Message { text: e, error: true });
            return false;
        }
    };
    let color = |p: [u8; 3]| {
        if app.truecolor { Color::Rgb(p[0], p[1], p[2]) } else { Color::Indexed(super::graphics::ansi256(p)) }
    };
    let buf = f.buffer_mut();
    buf.set_style(area, Style::new().bg(Color::DarkGray));
    for y in 0..slot.height {
        for x in 0..slot.width {
            let top = img.get(x as usize, 2 * y as usize);
            let bottom = img.get(x as usize, 2 * y as usize + 1);
            buf[(slot.x + x, slot.y + y)].set_char('▀').set_fg(color(top)).set_bg(color(bottom));
        }
    }
    true
}

/// Reserves the area for the page image (drawn by the event loop after the
/// frame): the page's aspect ratio, assuming cells twice as high as wide.
fn draw_preview_image_slot(f: &mut Frame, app: &mut App, area: Rect) {
    let Some(slot) = page_slot(app, area) else { return };
    let buf = f.buffer_mut();
    buf.set_style(area, Style::new().bg(Color::DarkGray));
    // Blank paper until the image arrives (and stays stable under it).
    buf.set_style(slot, Style::new().bg(Color::White));
    if app.paper_menu.is_none() && !app.show_help {
        app.image_slot = Some(slot);
    }
}

fn draw_preview_status(f: &mut Frame, app: &mut App, area: Rect) {
    let (text, style) = match &app.message {
        Some(m) => (m.text.clone(), if m.error { Style::new().fg(Color::LightRed).bold() } else { Style::new().fg(Color::Green) }),
        None => {
            let Some(p) = &app.preview else { return };
            let s = &app.print_setup;
            (
                format!(
                    "page {}/{} · {} · {:.0} pt{} · {} · PgUp/PgDn pages · +/- zoom{} · Esc closes",
                    if p.zoom > 0 {
                        let (a, d) = p.grid();
                        format!("{}–{}", p.page + 1, (p.page + a * d).min(p.layout.pages.len()))
                    } else {
                        (p.page + 1).to_string()
                    },
                    p.layout.pages.len(),
                    paper_label(app),
                    p.layout.font_size,
                    if s.font_size.is_none() { " (auto)" } else { "" },
                    app.scope_name(s.scope),
                    match (app.graphics.is_some(), app.image_mode) {
                        _ if p.zoom > 0 => " · 0 or i: back to the normal view",
                        (true, true) => " · i: text view",
                        (true, false) => " · i: image view · arrows scroll",
                        (false, true) => " · overview (no image protocol) · i: text view",
                        (false, false) => " · i: page overview · arrows scroll",
                    },
                ),
                Style::new().fg(Color::Gray),
            )
        }
    };
    f.buffer_mut().set_stringn(area.x, area.y, format!(" {text}"), area.width as usize, style);
}

fn paper_label(app: &App) -> String {
    let (paper, landscape) = app.current_paper();
    paper.label(landscape)
}


/// The help as pages; the page titles at the bottom are clickable.
fn draw_help(f: &mut Frame, app: &mut App) {
    let pages = super::help::pages(&app.caps);
    // The function-key bar below stays visible: it shows the keys of the help.
    let mut area = f.area();
    area.height = area.height.saturating_sub(1);
    let page = app.help_page.min(pages.len() - 1);
    let (title, entries) = &pages[page];
    // The same size for every page, so paging does not jump around.
    let tallest = pages.iter().map(|(_, e)| e.len()).max().unwrap_or(0) as u16;
    let w = 92.min(area.width);
    let h = (tallest + 5).min(area.height);
    let rect = Rect::new(area.x + (area.width - w) / 2, area.y + (area.height - h) / 2, w, h);
    let key_w = entries.iter().map(|(k, _)| k.width()).max().unwrap_or(0).max(18) + 2;
    let lines: Vec<Line> = entries
        .iter()
        .map(|(k, v)| Line::from(vec![format!(" {k:<key_w$}").bold().cyan(), v.to_string().into()]))
        .collect();
    f.render_widget(Clear, rect);
    let heading = format!(" hardcalc help · {}/{} · {title} ", page + 1, pages.len());
    f.render_widget(
        Paragraph::new(lines).block(Block::bordered().title(heading).title_bottom(" ← → / 1–7 pages · Esc closes ")),
        rect,
    );
    // Page tabs on the last inner line.
    let y = rect.bottom().saturating_sub(2);
    let mut x = rect.x + 2;
    for (i, (name, _)) in pages.iter().enumerate() {
        let label = format!(" {} {name} ", i + 1);
        let width = label.width() as u16;
        if x + width >= rect.right() {
            break;
        }
        let style = if i == page { Style::new().bold().fg(Color::Black).bg(Color::Cyan) } else { Style::new().fg(Color::Gray) };
        f.buffer_mut().set_string(x, y, &label, style);
        app.hits.push((Rect::new(x, y, width, 1), Hit::HelpPage(i)));
        x += width + 1;
    }
}
