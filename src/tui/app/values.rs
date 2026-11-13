//! The value list of a column: its distinct values with a tick each, as in
//! the filter buttons of spreadsheets. What is ticked becomes the column's
//! rule in the `filter` layer.

use super::{App, Hit, ViewKind, FILTER};
use crate::layers::quote_value;
use crate::value::Value;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;

#[derive(Debug, Clone, PartialEq)]
pub struct ValueItem {
    /// As the cells show it (`1200.00 €`); `(empty)` for empty cells.
    pub label: String,
    /// As a rule names it (`1200`); empty for empty cells.
    pub rule: String,
    /// Rows with this value.
    pub count: usize,
    pub checked: bool,
}

/// The open value list.
#[derive(Debug, Clone, PartialEq)]
pub struct ValueList {
    pub col: usize,
    /// Name of the column, for the title.
    pub name: String,
    pub items: Vec<ValueItem>,
    /// Highlighted line: 0 is "(all)", then the listed values.
    pub selected: usize,
    pub scroll: usize,
    /// Typed text: only values containing it are listed.
    pub query: String,
    /// A tick was set or removed since the text was typed.
    touched: bool,
    /// The ticks when the list opened.
    initial: Vec<bool>,
    /// Where the list is on screen (set while drawing).
    pub rect: Rect,
    /// Top left corner asked for: below the column header.
    pub pos: (u16, u16),
}

impl ValueList {
    /// The items listed: all, or those containing the typed text.
    pub fn shown(&self) -> Vec<usize> {
        let q = self.query.to_lowercase();
        (0..self.items.len()).filter(|&i| q.is_empty() || self.items[i].label.to_lowercase().contains(&q)).collect()
    }

    /// Whether all / some of the listed items are ticked.
    pub fn all_checked(&self) -> (bool, bool) {
        let shown = self.shown();
        (shown.iter().all(|&i| self.items[i].checked), shown.iter().any(|&i| self.items[i].checked))
    }

    fn toggle(&mut self, line: usize) {
        let shown = self.shown();
        match line.checked_sub(1) {
            None => {
                let on = !self.all_checked().0;
                for i in shown {
                    self.items[i].checked = on;
                }
            }
            Some(k) => {
                if let Some(&i) = shown.get(k) {
                    self.items[i].checked = !self.items[i].checked;
                }
            }
        }
        self.touched = true;
    }

    /// The ticks to apply: with a text typed and no tick changed since, the
    /// listed values only (type, Enter – as in spreadsheets).
    fn result(&self) -> Vec<bool> {
        if !self.query.is_empty() && !self.touched {
            let shown = self.shown();
            return (0..self.items.len()).map(|i| shown.contains(&i)).collect();
        }
        self.items.iter().map(|i| i.checked).collect()
    }

    /// The rule for the ticked values; `None`: everything is ticked.
    fn rule(&self, checked: &[bool]) -> Option<String> {
        if checked.iter().all(|c| *c) {
            return None;
        }
        let ticked: Vec<&ValueItem> = self.items.iter().zip(checked).filter(|(_, c)| **c).map(|(i, _)| i).collect();
        let only_empty_off = self.items.iter().zip(checked).all(|(i, c)| *c != i.rule.is_empty());
        Some(match ticked.as_slice() {
            [one] if one.rule.is_empty() => "empty".to_string(),
            [one] => format!("= {}", quote_value(&one.rule)),
            _ if only_empty_off => "required".to_string(),
            many => format!("in({})", many.iter().map(|i| quote_value(&i.rule)).collect::<Vec<_>>().join(", ")),
        })
    }
}

impl App {
    /// Opens the value list of column `col` (without: the cursor column).
    pub fn open_values(&mut self, col: Option<usize>) {
        if self.view.kind != ViewKind::Table || self.view.pivot.is_some() {
            return self.error("the value list belongs to the columns of a table");
        }
        let (rows, cols) = self.dims();
        let c = col.unwrap_or(self.view.cursor.1);
        if c >= cols {
            return self.error("no data column here to filter");
        }
        let (sheet, z) = (self.view.sheet, self.shown_layer());
        let conditions: Vec<crate::layers::Check> =
            self.filters.conditions(&self.wb, sheet).into_iter().filter(|(col, _)| *col == c).flat_map(|(_, checks)| checks).collect();
        // Distinct values with their sort key: numbers by size, then text, empty cells last.
        let mut items: Vec<(ValueItem, (u8, f64, String))> = Vec::new();
        for r in 0..rows {
            let id = (sheet, r, crate::engine::in_layer(c, z));
            let v = self.wb.value(id);
            let (rule, key) = match &v {
                Value::Null => (String::new(), (2, 0.0, String::new())),
                Value::Text(t) if t.trim().is_empty() => (String::new(), (2, 0.0, String::new())),
                Value::Num(n, _) => (format!("{n}"), (0, *n, String::new())),
                other => {
                    let text = other.display(None, false);
                    (text.clone(), (1, 0.0, text.to_lowercase()))
                }
            };
            match items.iter_mut().find(|(i, _)| i.rule == rule) {
                Some((item, _)) => item.count += 1,
                None => {
                    let label = if rule.is_empty() { "(empty)".to_string() } else { self.wb.display(id) };
                    let checked = conditions.iter().all(|check| check.holds(&v));
                    items.push((ValueItem { label, rule, count: 1, checked }, key));
                }
            }
        }
        if items.is_empty() {
            return self.error("the column has no rows yet");
        }
        items.sort_by(|(_, a), (_, b)| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)).then(a.2.cmp(&b.2)));
        let items: Vec<ValueItem> = items.into_iter().map(|(i, _)| i).collect();
        let pos = self.hits.iter().find(|(_, h)| *h == Hit::ColHeader(c)).map(|(r, _)| (r.x, r.y + r.height)).unwrap_or((4, 4));
        let name = self.wb.sheets[sheet].columns[c].name.clone();
        let initial = items.iter().map(|i| i.checked).collect();
        self.menu = None;
        self.context = None;
        self.message = None;
        self.values = Some(ValueList { col: c, name, items, selected: 0, scroll: 0, query: String::new(), touched: false, initial, rect: Rect::ZERO, pos });
    }

    pub(super) fn key_values(&mut self, key: KeyEvent) {
        let Some(list) = &mut self.values else { return };
        let lines = list.shown().len() + 1;
        let page = (list.rect.height.saturating_sub(2) as usize).max(1);
        match key.code {
            KeyCode::Esc if !list.query.is_empty() => {
                list.query.clear();
                list.selected = 0;
            }
            KeyCode::Esc | KeyCode::F(10) => self.values = None,
            KeyCode::Enter | KeyCode::F(2) => self.apply_values(),
            KeyCode::Up => list.selected = list.selected.saturating_sub(1),
            KeyCode::Down => list.selected = (list.selected + 1).min(lines - 1),
            KeyCode::PageUp => list.selected = list.selected.saturating_sub(page),
            KeyCode::PageDown => list.selected = (list.selected + page).min(lines - 1),
            KeyCode::Home => list.selected = 0,
            KeyCode::End => list.selected = lines - 1,
            KeyCode::Char(' ') => list.toggle(list.selected),
            KeyCode::Backspace => {
                list.query.pop();
                list.selected = 0;
                list.touched = false;
            }
            KeyCode::Char(ch) if !key.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) => {
                list.query.push(ch);
                list.selected = 0;
                list.touched = false;
            }
            _ => {}
        }
    }

    /// A click while the list is open: a line ticks, the frame's bottom
    /// applies, anywhere else closes.
    pub(super) fn click_values(&mut self, hit: Option<Hit>, inside: bool) {
        let Some(list) = &mut self.values else { return };
        match hit {
            Some(Hit::ValueItem(line)) => {
                list.selected = line;
                list.toggle(line);
            }
            Some(Hit::ValuesApply) => self.apply_values(),
            _ if inside => {}
            _ => self.values = None,
        }
    }

    /// Enter: the ticks become the rule of the column.
    fn apply_values(&mut self) {
        let Some(list) = self.values.clone() else { return };
        let checked = list.result();
        if !checked.contains(&true) {
            return self.error("tick at least one value – or Esc");
        }
        self.values = None;
        if checked == list.initial {
            return;
        }
        self.set_filter(list.col, list.rule(&checked));
    }

    /// Sets the filter rule of column `c` (`None`: removes it), shows the
    /// filter layer and says what is shown now.
    pub(super) fn set_filter(&mut self, c: usize, body: Option<String>) {
        let sheet = self.view.sheet;
        let target = self.filter_target(c);
        // Further rules for the same column (written with its letter, say) go.
        let mut changes: Vec<(String, String)> = self
            .attached
            .iter()
            .filter(|a| a.sheet == sheet && a.layer.name == FILTER)
            .flat_map(|a| &a.layer.rules)
            .filter_map(|r| match r {
                crate::model::Rule::Rule { target: t, .. } if *t != target && crate::layers::column_of(&self.wb, sheet, t) == Some(c) => Some((t.clone(), String::new())),
                _ => None,
            })
            .collect();
        changes.push((target.clone(), body.clone().unwrap_or_default()));
        self.set_layer_rules(FILTER, sheet, &changes);
        // A filter that was set shall be seen.
        self.hidden_layers.remove(FILTER);
        self.layer_overrides.remove(&(sheet, FILTER.to_string()));
        self.revalidate();
        self.snap_to_shown();
        let all = self.dims().0;
        let shown = all - self.hidden_count().map_or(0, |(n, _)| n);
        match body {
            Some(body) => self.info(format!("filter {target}: {body} – {shown} of {all} rows shown · :filter off shows all")),
            None => self.info(format!("filter on {target} removed – {shown} of {all} rows shown")),
        }
    }
}
