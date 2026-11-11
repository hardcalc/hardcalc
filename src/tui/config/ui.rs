//! Drawing hardconf: title bar, tree, form, documentation panel, status and F-keys.

use super::app::{ChooseFor, ConfApp, Confirm, Focus, Format, Hit, Mode};
use crate::config::edit::{describe, display, get};
use crate::config::validate::Node;
use crate::config::view::{reflow, shape_of, Row, RowKind, Shape};
use crate::config::{Diagnostic, FieldType, Severity};
use crate::types::Base;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph, Wrap};
use ratatui::Frame;
use toml_edit::Value;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

const DIM: Style = Style::new().fg(Color::DarkGray);
const HINT: Style = Style::new().fg(Color::Gray);
/// Hints from the schema: muted blue-grey and italic, apart from values.
const HINT_TEXT: Style = Style::new().fg(Color::Indexed(109)).add_modifier(Modifier::ITALIC);

/// Below this size only a note is shown.
const MIN_W: u16 = 40;
const MIN_H: u16 = 8;

pub fn draw(f: &mut Frame, app: &mut ConfApp) {
    app.hits.clear();
    let area = f.area();
    if area.width < MIN_W || area.height < MIN_H {
        let text = format!("terminal too small for hardconf (min. {MIN_W}×{MIN_H})");
        f.render_widget(Paragraph::new(text).wrap(Wrap { trim: true }), area);
        return;
    }
    let [top, body, status, fkeys] =
        Layout::vertical([Constraint::Length(1), Constraint::Min(3), Constraint::Length(1), Constraint::Length(1)])
            .areas(f.area());
    draw_top(f, app, top);
    let tree_w = (body.width / 3).clamp(18.min(body.width), 40);
    let [left, sep, right] =
        Layout::horizontal([Constraint::Length(tree_w), Constraint::Length(1), Constraint::Fill(1)]).areas(body);
    for y in sep.top()..sep.bottom() {
        f.buffer_mut()[(sep.x, y)].set_char('│').set_style(DIM);
    }
    draw_tree(f, app, left);
    let form_h = (right.height * 3 / 5).max(3.min(right.height));
    let [form, doc] = Layout::vertical([Constraint::Length(form_h), Constraint::Fill(1)]).areas(right);
    draw_form(f, app, form);
    draw_doc(f, app, doc);
    draw_status(f, app, status);
    draw_fkeys(f, app, fkeys);
    draw_choose(f, app);
    draw_multiline(f, app);
    draw_confirm(f, app);
    if let Some(d) = &mut app.file_dialog {
        let hits = d.draw(f, " open file ", "TOML files");
        app.hits.extend(hits.into_iter().map(|(r, spot)| (r, Hit::File(spot))));
    }
    if app.show_help {
        draw_help(f);
    }
    crate::tui::theme::apply(f.buffer_mut(), app.light(), app.theme != crate::tui::theme::ThemeSetting::Auto);
}

fn severity_style(s: Severity) -> Style {
    match s {
        Severity::Error => Style::new().fg(Color::LightRed),
        Severity::Warning => Style::new().fg(Color::Yellow),
        Severity::Note => DIM,
    }
}

/// Cuts `text` to `w` columns, ending in `…` when cut.
fn fit(text: &str, w: usize) -> String {
    if text.width() <= w {
        return text.to_string();
    }
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
    out
}

fn draw_top(f: &mut Frame, app: &ConfApp, area: Rect) {
    let buf = f.buffer_mut();
    buf.set_style(area, Style::new().bg(Color::DarkGray));
    let modified = if app.modified() { " ●" } else { "" };
    let name = format!(" hardconf · {}{modified}  ", app.file_name());
    buf.set_stringn(area.x, area.y, &name, area.width as usize, Style::new().fg(Color::White).bg(Color::DarkGray).bold());
    // The format of the file, at first glance.
    let (badge, badge_style) = match app.format() {
        Format::Hardconf(v) => (format!(" ✓ hardconf v{v} "), Style::new().fg(Color::Black).bg(Color::Green).bold()),
        Format::Broken => (" ✗ hardconf schema problems ".to_string(), Style::new().fg(Color::White).bg(Color::Red).bold()),
        Format::Plain => (" plain TOML ".to_string(), Style::new().fg(Color::Black).bg(Color::Gray)),
    };
    let bx = area.x + name.width() as u16;
    if bx < area.right() {
        buf.set_stringn(bx, area.y, &badge, (area.right() - bx) as usize, badge_style);
    }
    let left = format!("{name}{badge}");
    let (e, w) = app.counts();
    let mut right = Vec::new();
    if e > 0 {
        right.push((format!("● {e} error{} ", if e == 1 { "" } else { "s" }), Style::new().fg(Color::LightRed)));
    }
    if w > 0 {
        right.push((format!("● {w} warning{} ", if w == 1 { "" } else { "s" }), Style::new().fg(Color::Yellow)));
    }
    let total: usize = right.iter().map(|(t, _)| t.width()).sum();
    let mut x = area.right().saturating_sub(total as u16);
    if x > area.x + left.width() as u16 {
        for (t, s) in right {
            buf.set_string(x, area.y, &t, s.bg(Color::DarkGray));
            x += t.width() as u16;
        }
    }
}

fn draw_tree(f: &mut Frame, app: &mut ConfApp, area: Rect) {
    let tree = app.tree();
    let cursor = app.tree_cursor(&tree);
    let h = area.height as usize;
    if cursor < app.tree_scroll {
        app.tree_scroll = cursor;
    } else if cursor >= app.tree_scroll + h {
        app.tree_scroll = cursor + 1 - h;
    }
    app.tree_scroll = app.tree_scroll.min(tree.len().saturating_sub(h));
    let w = area.width as usize;
    for (i, n) in tree.iter().enumerate().skip(app.tree_scroll).take(h) {
        let y = area.y + (i - app.tree_scroll) as u16;
        let selected = i == cursor;
        let base = match (selected, app.focus) {
            (true, Focus::Tree) => Style::new().fg(Color::Black).bg(Color::Cyan),
            (true, Focus::Form) => Style::new().bg(Color::DarkGray),
            _ => Style::new(),
        };
        let buf = f.buffer_mut();
        buf.set_style(Rect::new(area.x, y, area.width, 1), base);
        let indent = n.depth * 2;
        let arrow = if !n.has_children || n.path.is_root() {
            " "
        } else if app.expanded.contains(&n.path) {
            "▾"
        } else {
            "▸"
        };
        let mut x = area.x + indent as u16;
        if (x as usize) < area.x as usize + w {
            buf.set_string(x, y, arrow, base);
            app.hits.push((Rect::new(x, y, 1, 1), Hit::Toggle(i)));
        }
        x += 2;
        let marker = app.worst(&n.path).filter(|s| *s > Severity::Note);
        let room = (area.right().saturating_sub(x) as usize).saturating_sub(if marker.is_some() { 2 } else { 0 });
        let label_style = match (n.present, n.path.is_root()) {
            (_, true) => base.bold(),
            (true, _) => base,
            (false, _) if selected => base,
            (false, _) => base.patch(DIM),
        };
        let label = fit(&n.label, room);
        buf.set_stringn(x, y, &label, room, label_style);
        let used = label.width();
        if let Some(h) = &n.hint
            && used + 3 < room
        {
            let style = if selected { base } else { HINT_TEXT };
            buf.set_stringn(x + used as u16 + 2, y, fit(h, room - used - 2), room - used - 2, style);
        }
        if let Some(s) = marker {
            buf.set_string(area.right() - 2, y, "●", severity_style(s).bg(base.bg.unwrap_or(Color::Reset)));
        }
        app.hits.push((Rect::new(area.x + indent as u16 + 2, y, area.width.saturating_sub(indent as u16 + 2), 1), Hit::Tree(i)));
    }
}

/// The text of a row's value and its state ("default", "not set" …).
struct Cell {
    value: String,
    value_style: Style,
    state: String,
    state_style: Style,
}

fn row_cell(app: &ConfApp, row: &Row) -> Cell {
    let field = row.field.as_ref();
    let hint = field.and_then(|f| app.cfg.schema.as_ref()?.hint(f)).map(str::to_string);
    let readonly = field.is_some_and(|f| f.readonly);
    let active = app.row_active(row);
    let value = app.value(&row.path);
    let mut cell = Cell { value: String::new(), value_style: Style::new(), state: String::new(), state_style: DIM };
    let scalar_ty = match &row.ty {
        Some(FieldType::Scalar { ty, .. }) => Some(ty),
        _ => None,
    };
    match row.kind {
        RowKind::Child => {
            let node = get(app.cfg.doc.as_table(), &row.path);
            let entries = node.and_then(Node::table_like).map_or(0, |t| t.iter().filter(|(_, i)| !i.is_none()).count());
            let elements = node.and_then(Node::elements).map(|e| e.len());
            let plural = |n: usize, one: &str, many: &str| format!("› {n} {}", if n == 1 { one } else { many });
            let present = node.is_some();
            cell.value = match (row.ty.as_ref().and_then(shape_of), elements) {
                (Some(Shape::Map { .. }), _) => plural(entries, "entry", "entries"),
                (Some(Shape::List(_)), n) | (_, n @ Some(_)) => plural(n.unwrap_or(0), "element", "elements"),
                _ if present => "› table".into(),
                _ => "›".into(),
            };
            cell.value_style = Style::new().fg(Color::Cyan);
            if !present {
                cell.state = if field.is_some_and(|f| f.default.is_some()) { "default".into() } else { "not set".into() };
            }
        }
        RowKind::List => {
            let n = match value {
                Some(Value::Array(a)) => a.len(),
                _ => 0,
            };
            cell.value = if value.is_some() { format!("{n} element{}", if n == 1 { "" } else { "s" }) } else { String::new() };
            cell.value_style = DIM;
            if value.is_none() {
                cell.state = match field.and_then(|f| f.default.as_ref()) {
                    Some(d) => format!("default {}", display(d)),
                    None if field.is_some_and(|f| f.required()) => "required".into(),
                    None => "not set".into(),
                };
            }
        }
        RowKind::Add => {
            cell.value = String::new();
        }
        RowKind::Value | RowKind::Element(_) => match value {
            Some(v) => {
                let text = display(v);
                let special = scalar_ty.and_then(|t| t.special_meaning(&crate::config::validate::value_text(v)));
                let secret = scalar_ty.is_some_and(|t| t.secret) && special.is_none() && !app.reveal;
                cell.value = if secret {
                    "•".repeat(text.chars().count().clamp(6, 12))
                } else if matches!(v, Value::String(s) if s.value().is_empty()) {
                    "\"\"".into()
                } else {
                    text
                };
                if let Some(m) = special.filter(|m| !m.is_empty()) {
                    cell.state = m.to_string();
                }
                if matches!(v, Value::String(s) if s.value().is_empty()) && special.is_none() {
                    cell.value_style = DIM;
                }
            }
            None => match field.and_then(|f| f.default.as_ref()) {
                Some(d) if row.kind == RowKind::Value => {
                    cell.value = display(d);
                    cell.value_style = DIM;
                    cell.state = "default".into();
                }
                _ if field.is_some_and(|f| f.required()) => {
                    cell.state = "required".into();
                    cell.state_style = Style::new().fg(Color::LightRed);
                }
                _ => {
                    cell.value = "—".into();
                    cell.value_style = DIM;
                    cell.state = "not set".into();
                }
            },
        },
    }
    if readonly && cell.state.is_empty() {
        cell.state = "fixed".into();
    }
    if !active {
        cell.value_style = DIM;
        if value.is_some() {
            cell.state = "no effect".into();
            cell.state_style = Style::new().fg(Color::Yellow);
        }
    }
    if let Some(d) = worst_at(app, row) {
        cell.state = d.message.clone();
        cell.state_style = severity_style(d.severity);
    } else if cell.state.is_empty()
        && let Some(h) = hint
        && matches!(row.kind, RowKind::Value | RowKind::List | RowKind::Child)
    {
        cell.state = h;
        cell.state_style = HINT_TEXT;
    }
    cell
}

/// The most severe finding of the row itself (not of its children).
fn worst_at<'a>(app: &'a ConfApp, row: &Row) -> Option<&'a Diagnostic> {
    if row.kind == RowKind::Add {
        return None;
    }
    let mut found = app.findings(&row.path);
    if row.kind == RowKind::Child {
        found.retain(|d| d.severity > Severity::Note);
    }
    found.into_iter().max_by_key(|d| d.severity)
}

fn draw_form(f: &mut Frame, app: &mut ConfApp, area: Rect) {
    if area.height < 2 {
        return;
    }
    let node = app.node();
    let buf = f.buffer_mut();
    let title_style = Style::new().bold().fg(if app.focus == Focus::Form { Color::Cyan } else { Color::White });
    buf.set_stringn(area.x + 1, area.y, fit(&node.title, area.width as usize - 1), area.width as usize - 1, title_style);
    if !node.present {
        let t = "  (not in the file: defaults)";
        let x = area.x + 1 + node.title.width() as u16;
        if x + 4 < area.right() {
            buf.set_stringn(x, area.y, t, (area.right() - x) as usize, DIM);
        }
    }
    let rows = app.rows();
    let list = Rect::new(area.x, area.y + 2.min(area.height - 1), area.width, area.height.saturating_sub(2));
    app.page = (list.height as usize).max(1);
    if rows.is_empty() {
        let text = match node.shape {
            Shape::Map { .. } => "no entries",
            _ => "nothing to edit here",
        };
        f.buffer_mut().set_stringn(list.x + 2, list.y, text, list.width as usize, DIM);
        return;
    }
    app.row = app.row.min(rows.len() - 1);
    let h = list.height as usize;
    let areas: Vec<Option<Vec<String>>> = rows.iter().map(|r| text_area(app, r)).collect();
    let height = |i: usize| areas[i].as_ref().map_or(1, |l| l.len());
    if app.row < app.form_scroll {
        app.form_scroll = app.row;
    }
    while app.form_scroll < app.row && (app.form_scroll..=app.row).map(height).sum::<usize>() > h {
        app.form_scroll += 1;
    }
    let label_w = rows
        .iter()
        .map(|r| r.label.width() + if matches!(r.kind, RowKind::Element(_) | RowKind::Add) { 2 } else { 0 })
        .max()
        .unwrap_or(0)
        .clamp(6, 28);
    let value_x = list.x + 2 + label_w as u16 + 2;
    let value_w = (list.right().saturating_sub(value_x) as usize * 3 / 5).max(10);
    let mut y = list.y;
    for (i, row) in rows.iter().enumerate().skip(app.form_scroll) {
        if y >= list.bottom() {
            break;
        }
        let rh = (height(i) as u16).min(list.bottom() - y);
        let selected = i == app.row;
        let line = Rect::new(list.x, y, list.width, rh);
        let next_y = y + rh;
        let base = match (selected, app.focus) {
            (true, Focus::Form) => Style::new().bg(Color::Rgb(40, 44, 60)),
            _ => Style::new(),
        };
        let cell = row_cell(app, row);
        let active = app.row_active(row);
        let buf = f.buffer_mut();
        buf.set_style(line, base);
        if selected {
            let mark_style = if app.focus == Focus::Form { Style::new().fg(Color::Cyan) } else { DIM };
            for yy in y..next_y {
                buf.set_string(list.x, yy, "▌", mark_style.bg(base.bg.unwrap_or(Color::Reset)));
            }
        }
        let indent = if matches!(row.kind, RowKind::Element(_) | RowKind::Add) { 2 } else { 0 };
        let mut label_style = match row.kind {
            RowKind::Add => DIM,
            RowKind::Element(_) => HINT,
            _ if !active => DIM,
            _ => Style::new().bold(),
        };
        if row.field.is_none() && app.cfg.schema.is_some() && row.kind != RowKind::Add {
            label_style = Style::new().fg(Color::Yellow);
        }
        buf.set_stringn(list.x + 2 + indent, y, fit(&row.label, label_w - indent as usize), label_w, label_style.patch(base));
        let editing = matches!(&app.mode, Mode::Edit { row: r, multiline: false, .. } if *r == i);
        if editing {
            let Mode::Edit { buf: text, cursor, .. } = &app.mode else { unreachable!() };
            let w = list.right().saturating_sub(value_x) as usize;
            let before: String = text.chars().take(*cursor).collect();
            // Scroll the field so the cursor stays visible.
            let skip = before.width().saturating_sub(w.saturating_sub(2));
            let shown: String = text.chars().scan(0, |acc, c| { *acc += c.width().unwrap_or(0); Some((*acc, c)) }).filter(|(a, _)| *a > skip).map(|(_, c)| c).collect();
            let field = Rect::new(value_x, y, w as u16, 1);
            buf.set_style(field, Style::new().bg(Color::Black).fg(Color::White));
            buf.set_stringn(value_x, y, &shown, w, Style::new().bg(Color::Black).fg(Color::White));
            let cx = value_x + (before.width() - skip) as u16;
            f.set_cursor_position((cx.min(list.right() - 1), y));
            y = next_y;
            continue;
        }
        if let Some(text) = &areas[i] {
            // A text area: the lines under each other, the state below the label.
            let w = list.right().saturating_sub(value_x) as usize;
            for (k, l) in text.iter().enumerate().take(rh as usize) {
                buf.set_stringn(value_x, y + k as u16, fit(l, w), w, cell.value_style.patch(base));
            }
            if rh > 1 && !cell.state.is_empty() {
                let lw = label_w - indent as usize;
                buf.set_stringn(list.x + 2 + indent, y + 1, fit(&cell.state, lw), lw, cell.state_style.patch(Style::new().bg(base.bg.unwrap_or(Color::Reset))));
            }
            app.hits.push((line, Hit::Row(i)));
            y = next_y;
            continue;
        }
        buf.set_stringn(value_x, y, fit(&cell.value, value_w), value_w, cell.value_style.patch(base));
        let state_x = value_x + (cell.value.width().min(value_w) as u16) + 2;
        if !cell.state.is_empty() && state_x + 2 < list.right() {
            let w = (list.right() - state_x) as usize;
            buf.set_stringn(state_x, y, fit(&cell.state, w), w, cell.state_style.patch(Style::new().bg(base.bg.unwrap_or(Color::Reset))));
        }
        app.hits.push((line, Hit::Row(i)));
        y = next_y;
    }
}

/// Most lines a multi-line text shows in the form.
const AREA_LINES: usize = 5;

/// The lines of a `multiline` text shown as a text area in the form; `None`
/// for one-line rows (also an empty or missing text).
fn text_area(app: &ConfApp, row: &Row) -> Option<Vec<String>> {
    let schema_multiline = matches!(&row.ty, Some(FieldType::Scalar { ty, .. }) if ty.multiline && !ty.secret);
    let generic_multiline = row.field.is_none() && app.value(&row.path).is_some_and(crate::config::edit::is_multiline);
    if row.kind != RowKind::Value || !(schema_multiline || generic_multiline) {
        return None;
    }
    let text = app.value(&row.path)?.as_str()?.trim_end();
    if text.is_empty() {
        return None;
    }
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    if lines.len() > AREA_LINES {
        let more = lines.len() - (AREA_LINES - 1);
        lines.truncate(AREA_LINES - 1);
        lines.push(format!("… {more} more lines – Enter shows all"));
    }
    Some(lines)
}

fn type_lines(ty: &FieldType, out: &mut Vec<Line<'static>>) {
    let field = |k: &str, v: String| Line::from(vec![Span::styled(format!("{k:<9}"), DIM), Span::raw(v)]);
    match ty {
        FieldType::Scalar { ty, name } => {
            let base = match &ty.base {
                Base::Enum(v) => format!("one of {}", v.join(", ")),
                b => b.to_string(),
            };
            out.push(field("type", match name { Some(n) => format!("{n} = {base}"), None => base }));
            if let Some(r) = ty.range {
                out.push(field("range", r.to_string()));
            }
            if let Some(r) = ty.length {
                out.push(field("length", r.to_string()));
            }
            if let Some(p) = &ty.pattern {
                out.push(field("pattern", p.clone()));
            }
            if let Some(c) = &ty.check {
                out.push(field("check", c.clone()));
            }
            if !ty.unit.is_none() {
                out.push(field("unit", ty.unit.to_string()));
            }
            for (v, m) in &ty.special {
                let v = if v.is_empty() { "\"\"".to_string() } else { v.clone() };
                out.push(field("special", if m.is_empty() { v } else { format!("{v} = {m}") }));
            }
            if ty.secret {
                out.push(field("", "secret: hidden, F3 shows it".into()));
            }
        }
        FieldType::List { item, length } => {
            out.push(field("type", format!("list of {}", describe(item))));
            if let Some(l) = length {
                out.push(field("length", l.to_string()));
            }
            type_lines(item, out);
        }
        FieldType::Map { value, key, length } => {
            out.push(field("type", format!("map of {}", describe(value))));
            out.push(field("keys", describe(key)));
            if let Some(l) = length {
                out.push(field("entries", l.to_string()));
            }
        }
        FieldType::Ref(p) => out.push(field("type", format!("an entry of {p}"))),
        FieldType::Struct(s) => out.push(field("type", s.clone())),
    }
}

fn draw_doc(f: &mut Frame, app: &mut ConfApp, area: Rect) {
    if area.height < 2 {
        return;
    }
    let top = Rect::new(area.x, area.y, area.width, 1);
    f.buffer_mut().set_stringn(top.x, top.y, "─".repeat(area.width as usize), area.width as usize, DIM);
    let mut lines: Vec<Line<'static>> = Vec::new();
    let heading = |s: String| Line::from(Span::styled(s, Style::new().bold()));
    let section = |s: &str| Line::from(Span::styled(s.to_string(), Style::new().fg(Color::Cyan)));
    let node = app.node();
    let row = (app.focus == Focus::Form).then(|| app.current_row()).flatten();
    let path = row.as_ref().map_or(node.path.clone(), |r| r.path.clone());
    match (&row, app.cfg.schema.as_ref()) {
        (Some(r), Some(schema)) if r.field.is_some() => {
            let fd = r.field.as_ref().expect("checked");
            lines.push(heading(format!("{} : {}", fd.name, fd.ty_src)));
            if let Some(h) = schema.hint(fd) {
                lines.push(Line::styled(h.to_string(), HINT_TEXT));
            }
            let mut t = Vec::new();
            match (&r.kind, &r.ty) {
                (RowKind::Element(_) | RowKind::Add | RowKind::List, _) => type_lines(&fd.ty, &mut t),
                (_, Some(ty)) => type_lines(ty, &mut t),
                _ => type_lines(&fd.ty, &mut t),
            }
            lines.extend(t);
            let field = |k: &str, v: String| Line::from(vec![Span::styled(format!("{k:<9}"), DIM), Span::raw(v)]);
            match &fd.default {
                Some(d) => lines.push(field("default", display(d))),
                None if fd.optional => lines.push(field("", "optional".into())),
                None => lines.push(field("", "required".into())),
            }
            if let Some(c) = &fd.when {
                lines.push(field("only if", c.to_string()));
            }
            if fd.readonly {
                lines.push(field("", "read-only".into()));
            }
            if !fd.doc.is_empty() {
                lines.push(Line::raw(""));
                lines.extend(reflow(&fd.doc).into_iter().map(Line::raw));
            }
        }
        (Some(r), _) => {
            lines.push(heading(r.path.to_string()));
            if app.cfg.schema.is_some() {
                lines.push(Line::styled("not in the schema", Style::new().fg(Color::Yellow)));
            }
            if let Some(v) = app.value(&r.path) {
                if let Some(k) = crate::config::edit::kind_of(v) {
                    lines.push(Line::from(vec![
                        Span::styled(format!("{:<9}", "type"), DIM),
                        Span::raw(k.label().to_string()),
                        Span::styled("  F9 changes it", DIM),
                    ]));
                }
                lines.push(Line::styled(format!("TOML: {}", v.to_string().trim()), DIM));
            }
        }
        (None, _) => {
            lines.push(heading(node.title.clone()));
            if node.path.is_root() {
                let (text, style) = match (app.format(), &app.cfg.schema) {
                    (Format::Hardconf(v), Some(s)) => (
                        format!("hardconf schema v{v} · {} types · {} structs", s.type_defs.len(), s.structs.len()),
                        Style::new().fg(Color::Green),
                    ),
                    (Format::Broken, _) => ("#%hardconf block with problems, see below".to_string(), Style::new().fg(Color::LightRed)),
                    _ => ("plain TOML · no #%hardconf block: types follow the values".to_string(), HINT),
                };
                lines.push(Line::from(vec![Span::styled(format!("{:<9}", "format"), DIM), Span::styled(text, style)]));
            }
            let st = match &node.shape {
                Shape::Struct(s) => app.cfg.struct_of(s),
                Shape::Map { value, .. } => match value.as_ref() {
                    FieldType::Struct(s) => app.cfg.struct_of(s),
                    _ => None,
                },
                Shape::List(FieldType::Struct(s)) => app.cfg.struct_of(s),
                Shape::List(_) => None,
                Shape::Generic => None,
            };
            // The field's own documentation, not repeated for each entry of a map.
            if let Some(fd) = &node.field
                && !fd.doc.is_empty()
                && fd.name == node.label
            {
                lines.push(Line::raw(""));
                lines.extend(reflow(&fd.doc).into_iter().map(Line::raw));
            }
            if let Some(st) = st
                && !st.doc.is_empty()
            {
                lines.push(Line::raw(""));
                lines.extend(reflow(&st.doc).into_iter().map(Line::raw));
            }
            if node.path.is_root() && !app.cfg.schema_errors.is_empty() {
                lines.push(Line::raw(""));
                lines.push(Line::styled("Schema problems", Style::new().fg(Color::LightRed)));
                for e in &app.cfg.schema_errors {
                    lines.push(Line::styled(format!("  {e}"), Style::new().fg(Color::LightRed)));
                }
            }
        }
    }
    if let Some(c) = app.comments.get(&path) {
        lines.push(Line::raw(""));
        lines.push(Line::from(vec![
            Span::styled("Comment in the file", Style::new().fg(Color::Cyan)),
            Span::styled("  F6 edit", DIM),
        ]));
        lines.extend(c.lines().map(|l| Line::styled(l.to_string(), Style::new().fg(Color::Magenta))));
    }
    // Findings: of the row itself, or everything below the node.
    let found: Vec<&Diagnostic> = if row.is_some() {
        app.diagnostics.iter().filter(|d| d.path.starts_with(&path)).collect()
    } else {
        app.diagnostics.iter().filter(|d| d.path.starts_with(&node.path)).collect()
    };
    if !found.is_empty() {
        lines.push(Line::raw(""));
        lines.push(section("Problems"));
        for d in found.iter().take(20) {
            let at = if d.path == path || d.path.is_root() { String::new() } else { format!("{}: ", d.path) };
            lines.push(Line::styled(format!("{} {at}{}", match d.severity { Severity::Error => "✗", Severity::Warning => "!", Severity::Note => "·" }, d.message), severity_style(d.severity)));
        }
        if found.len() > 20 {
            lines.push(Line::styled(format!("… {} more", found.len() - 20), DIM));
        }
    }
    let body = Rect::new(area.x + 1, area.y + 1, area.width.saturating_sub(2), area.height - 1);
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), body);
}

fn draw_status(f: &mut Frame, app: &ConfApp, area: Rect) {
    if let Mode::NewKey { buf, cursor, map, .. } = &app.mode {
        let place = if map.is_root() { app.file_name() } else { map.to_string() };
        let prompt = format!(" new entry in {place} – {}: ", app.key_label(map));
        let style = Style::new().fg(Color::Yellow);
        let b = f.buffer_mut();
        b.set_stringn(area.x, area.y, &prompt, area.width as usize, style);
        let x = area.x + (prompt.width() as u16).min(area.width);
        b.set_stringn(x, area.y, buf, area.right().saturating_sub(x) as usize, Style::new());
        if let Some(m) = app.message.as_ref().filter(|m| m.error) {
            let mx = x + buf.width() as u16 + 3;
            if mx < area.right() {
                b.set_stringn(mx, area.y, &m.text, (area.right() - mx) as usize, Style::new().fg(Color::LightRed));
            }
        }
        let before: String = buf.chars().take(*cursor).collect();
        f.set_cursor_position(((x + before.width() as u16).min(area.right() - 1), area.y));
        return;
    }
    let (text, style) = match (&app.mode, &app.message) {
        (Mode::Comment { .. }, _) => ("comment – Enter new line · Tab / Ctrl+S save · empty removes it · Esc cancel".to_string(), Style::new().fg(Color::Yellow)),
        (Mode::Edit { multiline: true, .. }, _) => ("editing – Enter new line · Tab / Ctrl+S commit · Esc cancel".to_string(), Style::new().fg(Color::Yellow)),
        (Mode::Edit { .. }, Some(m)) if m.error => (m.text.clone(), Style::new().fg(Color::LightRed).bold()),
        (Mode::Edit { .. }, _) => ("editing – Enter/Tab commit · Esc cancel".to_string(), Style::new().fg(Color::Yellow)),
        (Mode::Choose { purpose: ChooseFor::NewKey(map), .. }, _) => (format!("new entry in {map} – choose its {} · Enter take · Esc cancel", app.key_label(map)), Style::new().fg(Color::Yellow)),
        (Mode::Choose { purpose: ChooseFor::NewKind { key, .. }, .. }, _) => (format!("what is '{key}'? · Enter take · Esc cancel"), Style::new().fg(Color::Yellow)),
        (Mode::Choose { purpose: ChooseFor::Retype(path), .. }, _) => (format!("new type of {path} – the value is converted where possible · Esc cancel"), Style::new().fg(Color::Yellow)),
        (Mode::Choose { .. }, _) => ("choose – ↑↓ · Enter take · Esc cancel".to_string(), Style::new().fg(Color::Yellow)),
        (_, Some(m)) => (m.text.clone(), if m.error { Style::new().fg(Color::LightRed).bold() } else { Style::new().fg(Color::Green) }),
        _ => (
            match app.focus {
                Focus::Tree => "↑↓ select · Space + − open/close · ← parent · → form · F1 help",
                Focus::Form if app.current_row().is_some_and(|r| r.field.is_none()) => {
                    "Enter edit · type to replace · Del remove · F9 type · ← tree · F1 help"
                }
                Focus::Form => "Enter edit/choose · type to replace · Del default · ← tree · F1 help",
            }
            .to_string(),
            DIM,
        ),
    };
    f.buffer_mut().set_stringn(area.x, area.y, format!(" {text}"), area.width as usize, style);
}

fn draw_fkeys(f: &mut Frame, app: &mut ConfApp, area: Rect) {
    // The file dialog has its own keys.
    let dialog = app.file_dialog.as_ref().map(|d| d.fkeys());
    let keys: [(u8, &str); 10] = if let Some(labels) = dialog {
        std::array::from_fn(|i| (i as u8 + 1, labels[i]))
    } else {
        [
        (1, "Help"),
        (2, "Save"),
        (3, if app.reveal { "Hide" } else { "Show" }),
        (4, if app.focus == Focus::Form && app.current_row().is_some_and(|r| r.field.is_none()) { "Remove" } else { "Default" }),
        (5, "Open"),
        (6, if app.comment_target().is_ok() { "Comment" } else { "" }),
        (7, if app.add_target().is_ok() { "New" } else { "" }),
        (8, if app.delete_target().is_ok() { "Delete" } else { "" }),
        (9, if app.retype_target().is_ok() { "Type" } else { "" }),
        (10, "Quit"),
        ]
    };
    let slot = (area.width / keys.len() as u16).max(1);
    for (i, (key, label)) in keys.iter().enumerate() {
        let x = area.x + i as u16 * slot;
        if x >= area.right() {
            break;
        }
        let w = if i + 1 == keys.len() { area.right() - x } else { slot.min(area.right() - x) };
        let num = key.to_string();
        let buf = f.buffer_mut();
        buf.set_stringn(x, area.y, &num, w as usize, Style::new().fg(Color::White));
        let lw = (w as usize).saturating_sub(num.len());
        if lw > 0 {
            buf.set_stringn(x + num.len() as u16, area.y, format!("{:<lw$}", fit(label, lw)), lw, Style::new().fg(Color::Black).bg(Color::Cyan));
        }
        if !label.is_empty() {
            app.hits.push((Rect::new(x, area.y, w, 1), Hit::FKey(*key)));
        }
    }
}

/// Position of the selected form row on screen, for popups.
fn row_rect(app: &ConfApp, row: usize) -> Option<Rect> {
    app.hits.iter().find(|(_, h)| *h == Hit::Row(row)).map(|(r, _)| *r)
}

fn draw_choose(f: &mut Frame, app: &mut ConfApp) {
    let Mode::Choose { options, cursor, row, .. } = &app.mode else { return };
    let area = f.area();
    let w = options.iter().map(|(v, m)| v.width().max(2) + if m.is_empty() { 0 } else { m.width() + 3 }).max().unwrap_or(10).max(12) as u16 + 4;
    let w = w.min(area.width);
    let h = (options.len() as u16 + 2).min(area.height.saturating_sub(2)).max(3);
    let anchor = row_rect(app, *row).unwrap_or(Rect::new(area.width / 2, area.height / 3, 1, 1));
    let x = (anchor.x + anchor.width / 3).min(area.right().saturating_sub(w));
    let y = if anchor.y + 1 + h <= area.bottom() { anchor.y + 1 } else { anchor.y.saturating_sub(h) };
    let rect = Rect::new(x, y, w, h);
    f.render_widget(Clear, rect);
    f.render_widget(Block::bordered().border_style(Style::new().fg(Color::Cyan)), rect);
    let inner = rect.inner(ratatui::layout::Margin::new(1, 1));
    let scroll = cursor.saturating_sub(inner.height as usize - 1);
    let mut hits = Vec::new();
    for (i, (v, m)) in options.iter().enumerate().skip(scroll).take(inner.height as usize) {
        let yy = inner.y + (i - scroll) as u16;
        let style = if i == *cursor { Style::new().fg(Color::Black).bg(Color::Cyan) } else { Style::new() };
        let buf = f.buffer_mut();
        buf.set_style(Rect::new(inner.x, yy, inner.width, 1), style);
        let shown = if v.is_empty() { "\"\"" } else { v };
        buf.set_stringn(inner.x + 1, yy, shown, inner.width as usize - 1, style);
        if !m.is_empty() {
            let mx = inner.x + 1 + shown.width() as u16 + 2;
            if mx < inner.right() {
                buf.set_stringn(mx, yy, m, (inner.right() - mx) as usize, if i == *cursor { style } else { DIM });
            }
        }
        hits.push((Rect::new(inner.x, yy, inner.width, 1), Hit::Choice(i)));
    }
    app.hits.extend(hits);
}

fn draw_multiline(f: &mut Frame, app: &ConfApp) {
    let (buf, cursor, title, color) = match &app.mode {
        Mode::Edit { buf, cursor, multiline: true, row } => {
            (buf, cursor, app.rows().get(*row).map(|r| r.label.clone()).unwrap_or_default(), Color::Cyan)
        }
        Mode::Comment { buf, cursor, target } => {
            let what = if target.is_root() { app.file_name() } else { target.to_string() };
            (buf, cursor, format!("comment · {what}"), Color::Magenta)
        }
        _ => return,
    };
    let area = f.area();
    let w = area.width.saturating_sub(8).clamp(20.min(area.width), 90);
    let h = area.height.saturating_sub(6).clamp(5.min(area.height), 20);
    let rect = Rect::new(area.x + (area.width - w) / 2, area.y + (area.height - h) / 2, w, h);
    f.render_widget(Clear, rect);
    f.render_widget(Block::bordered().title(format!(" {title} ")).border_style(Style::new().fg(color)), rect);
    let inner = rect.inner(ratatui::layout::Margin::new(1, 1));
    // Cursor line and column.
    let before: String = buf.chars().take(*cursor).collect();
    let cy = before.matches('\n').count();
    let cx = before.rsplit('\n').next().unwrap_or("").width();
    let scroll = cy.saturating_sub(inner.height as usize - 1);
    for (i, line) in buf.split('\n').enumerate().skip(scroll).take(inner.height as usize) {
        f.buffer_mut().set_stringn(inner.x, inner.y + (i - scroll) as u16, line, inner.width as usize, Style::new());
    }
    f.set_cursor_position(((inner.x + cx as u16).min(inner.right() - 1), inner.y + (cy - scroll) as u16));
}

fn draw_confirm(f: &mut Frame, app: &ConfApp) {
    let Mode::Confirm(c) = &app.mode else { return };
    let mut extra = None;
    let (title, text, keys) = match c {
        Confirm::SaveWithErrors => {
            let e = app.cfg.validate().iter().filter(|d| d.severity == Severity::Error).count();
            (" save with errors ", format!("The file has {e} error{}.", if e == 1 { "" } else { "s" }), "[Y]es, save   [N]o")
        }
        Confirm::Unsaved(_) => (" unsaved changes ", format!("{} has unsaved changes.", app.file_name()), "[S]ave   [D]iscard   [Esc] Cancel"),
        Confirm::Delete(entry) => {
            let refs = app.references_to(entry);
            if !refs.is_empty() {
                let list: Vec<String> = refs.iter().take(3).map(|p| p.to_string()).collect();
                let more = if refs.len() > 3 { format!(" and {} more", refs.len() - 3) } else { String::new() };
                extra = Some(format!("Still used by {}{more}.", list.join(", ")));
            }
            (" delete entry ", format!("Delete {entry} with everything in it?"), "[Y]es, delete   [N]o")
        }
    };
    let area = f.area();
    let extra_w = extra.as_ref().map_or(0, |e| e.width());
    let w = (text.width().max(keys.width()).max(extra_w) as u16 + 6).min(area.width);
    let h = (if extra.is_some() { 7 } else { 6 }).min(area.height);
    let rect = Rect::new(area.x + (area.width - w) / 2, area.y + (area.height - h) / 2, w, h);
    f.render_widget(Clear, rect);
    f.render_widget(Block::bordered().title(title), rect);
    let buf = f.buffer_mut();
    buf.set_stringn(rect.x + 3, rect.y + 2, &text, (w - 4) as usize, Style::new().bold());
    if let Some(e) = &extra {
        buf.set_stringn(rect.x + 3, rect.y + 3, e, (w - 4) as usize, Style::new().fg(Color::Yellow));
    }
    buf.set_stringn(rect.x + 3, rect.bottom() - 2, keys, (w - 4) as usize, Style::new().fg(Color::Cyan));
}

const HELP: &[(&str, &str)] = &[
    ("Tab", "switch between tree and form"),
    ("↑ ↓  PgUp PgDn", "move"),
    ("→ ←", "→ to the form · ← back to the tree; in the tree ← closes / goes to the parent"),
    ("+ −", "tree: open / close a node"),
    ("Enter", "form: edit, choose from a list, toggle a bool, open a table"),
    ("typing", "replaces the value under the cursor"),
    ("Space", "toggle a bool · tree: open / close"),
    ("Del  F4", "back to the default (removes the key) · removes a list element"),
    ("F3", "show / hide secret values"),
    ("F7  Insert", "new map entry (agents …) or, without schema, new key / table / list element"),
    ("F8", "delete the selected map entry or table with all its tables"),
    ("F9", "without schema: change the type of a value (text, number, date, list …)"),
    ("F5  Ctrl+E", "open another file"),
    ("F6", "write the comment above the selected table or key (# lines in the file)"),
    ("Ctrl+Z  Ctrl+Y", "undo / redo"),
    ("Ctrl+C  Ctrl+V", "copy the value (also to the system clipboard) / paste it as value"),
    ("F2  Ctrl+S", "save (diagnostic lines in the file are removed)"),
    ("F10  Ctrl+Q", "quit – without saving, the file stays untouched"),
    ("", ""),
    ("multi-line text", "Enter new line · Tab or Ctrl+S commit · Esc cancel"),
];

fn draw_help(f: &mut Frame) {
    let area = f.area();
    let w = 90.min(area.width);
    let h = (HELP.len() as u16 + 4).min(area.height);
    let rect = Rect::new(area.x + (area.width - w) / 2, area.y + (area.height - h) / 2, w, h);
    f.render_widget(Clear, rect);
    f.render_widget(Block::bordered().title(" hardconf – keys ").title_bottom(" any key closes "), rect);
    let lines: Vec<Line> = HELP
        .iter()
        .map(|(k, d)| Line::from(vec![Span::styled(format!(" {k:<16}"), Style::new().fg(Color::Cyan)), Span::raw(*d)]))
        .collect();
    f.render_widget(Paragraph::new(lines), rect.inner(ratatui::layout::Margin::new(1, 1)));
}
