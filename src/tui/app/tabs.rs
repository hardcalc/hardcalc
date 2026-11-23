//! The tabs of tables and calculations: moving them (`:movetab`, `-` / `+`
//! on the table origin, dragging a tab), remembering where each one was
//! left, and the warning when a move breaks a calculation.

use std::collections::HashMap;

use super::{App, ViewKind};
use crate::model::{Block, Document};

/// Where a table or calculation was left: cursor and first visible row / column.
pub(super) type Spot = ((usize, usize), (usize, usize));

/// One tab of the bar: a table or a calculation, by its place among its kind.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TabItem {
    Table(usize),
    Calc(usize),
}

/// The tables and calculations in the order of the document.
pub fn tab_items(doc: &Document) -> Vec<TabItem> {
    let (mut tables, mut calcs) = (0, 0);
    doc.blocks
        .iter()
        .filter_map(|b| match b {
            Block::Table(_) => {
                tables += 1;
                Some(TabItem::Table(tables - 1))
            }
            Block::Code { info, .. } if crate::sheet::is_sheet_info(info) => {
                calcs += 1;
                Some(TabItem::Calc(calcs - 1))
            }
            _ => None,
        })
        .collect()
}

/// Moves tab `from` to place `to`. A table takes the layers that follow it
/// without a target along; everything else stays where it is in the
/// document. Returns for each table and for each calculation its new place.
pub fn reorder_tabs(doc: &mut Document, from: usize, to: usize) -> (Vec<usize>, Vec<usize>) {
    let items = tab_items(doc);
    let n = items.len();
    let identity = |items: &[TabItem]| {
        let tables = items.iter().filter(|i| matches!(i, TabItem::Table(_))).count();
        ((0..tables).collect(), (0..items.len() - tables).collect())
    };
    if from >= n || to >= n {
        return identity(&items);
    }
    let blocks: Vec<usize> = doc
        .blocks
        .iter()
        .enumerate()
        .filter(|(_, b)| matches!(b, Block::Table(_)) || matches!(b, Block::Code { info, .. } if crate::sheet::is_sheet_info(info)))
        .map(|(i, _)| i)
        .collect();
    // The blocks of each tab: a calculation is one, a table has its layers too.
    let units: Vec<Vec<usize>> = (0..n)
        .map(|k| {
            let mut unit = vec![blocks[k]];
            if matches!(items[k], TabItem::Table(_)) {
                let end = blocks[k + 1..].iter().zip(&items[k + 1..]).find(|(_, i)| matches!(i, TabItem::Table(_))).map_or(doc.blocks.len(), |(b, _)| *b);
                unit.extend((blocks[k] + 1..end).filter(|&i| matches!(&doc.blocks[i], Block::Layer(l) if l.target.is_none())));
            }
            unit
        })
        .collect();
    let mut order: Vec<usize> = (0..n).collect();
    let moved = order.remove(from);
    order.insert(to, moved);
    // The places of all these blocks take them in the new order.
    let mut slots: Vec<usize> = units.iter().flatten().copied().collect();
    slots.sort_unstable();
    let old = doc.blocks.clone();
    for (slot, from) in slots.into_iter().zip(order.iter().flat_map(|&k| units[k].iter().copied())) {
        doc.blocks[slot] = old[from].clone();
    }
    let (mut sheets, mut calcs) = (Vec::new(), Vec::new());
    let (tables, calc_count) = (items.iter().filter(|i| matches!(i, TabItem::Table(_))).count(), items.iter().filter(|i| matches!(i, TabItem::Calc(_))).count());
    sheets.resize(tables, 0);
    calcs.resize(calc_count, 0);
    let (mut next_table, mut next_calc) = (0, 0);
    for &old in &order {
        match items[old] {
            TabItem::Table(i) => {
                sheets[i] = next_table;
                next_table += 1;
            }
            TabItem::Calc(k) => {
                calcs[k] = next_calc;
                next_calc += 1;
            }
        }
    }
    (sheets, calcs)
}

impl App {
    /// The tab the focused window shows, if it is a table or a calculation.
    pub(super) fn current_tab(&self) -> Option<TabItem> {
        match self.view.kind {
            ViewKind::Table => Some(TabItem::Table(self.view.sheet)),
            ViewKind::Calc => Some(TabItem::Calc(self.view.calc)),
            _ => None,
        }
    }

    /// The place of a tab in the bar.
    pub(super) fn tab_position(&self, tab: TabItem) -> Option<usize> {
        tab_items(&self.doc).iter().position(|t| *t == tab)
    }

    /// Whether there are charts or layouts for a second row of tabs.
    pub fn has_second_tab_row(&self) -> bool {
        !self.layout_blocks().is_empty() || !self.chart_blocks().is_empty()
    }

    /// The name of a tab.
    pub fn tab_title(&self, tab: TabItem) -> String {
        match tab {
            TabItem::Table(i) => self.wb.sheets.get(i).and_then(|s| s.name.clone()).unwrap_or_else(|| format!("#{}", i + 1)),
            TabItem::Calc(k) => crate::sheet::calc_title(&self.doc, k),
        }
    }

    /// Notes where the shown table or calculation is, before another view takes the window.
    pub(super) fn remember_table_view(&mut self) {
        let spot = (self.view.cursor, self.view.scroll);
        match self.view.kind {
            ViewKind::Table => {
                self.table_views.insert(self.view.sheet, spot);
            }
            ViewKind::Calc => {
                self.calc_spots.insert(self.view.calc, spot);
            }
            _ => {}
        }
    }

    /// Back where table `sheet` was left, if it was shown before.
    pub(super) fn restore_table_view(&mut self, sheet: usize) {
        if let Some(&(cursor, scroll)) = self.table_views.get(&sheet) {
            self.view.cursor = cursor;
            self.view.scroll = scroll;
            self.view.follow = true;
        }
    }

    /// Shows calculation `k` in the main window, where it was left.
    pub fn show_calc(&mut self, k: usize) {
        self.with_window(true, |app| {
            app.set_view(ViewKind::Calc, app.view.sheet);
            app.view.calc = k;
            app.last_calc = k;
            if let Some(&(cursor, scroll)) = app.calc_spots.get(&k) {
                app.view.cursor = cursor;
                app.view.scroll = scroll;
            }
            app.refresh_calc_hidden();
            app.clamp_cursor();
        });
    }

    /// Moves the tab at place `from` of the bar to place `to`; the views, the
    /// remembered places and the slices shown follow. A move that breaks a
    /// calculation (a name used before it is defined) is warned about.
    pub(super) fn move_tab(&mut self, from: usize, to: usize) {
        let items = tab_items(&self.doc);
        if from >= items.len() || to >= items.len() || from == to {
            return;
        }
        let name = self.tab_title(items[from]);
        self.remember_table_view();
        let before = crate::sheet::problems(self.calc_results());
        let mut maps = (Vec::new(), Vec::new());
        self.with_doc(|doc| maps = reorder_tabs(doc, from, to));
        let (sheets, calcs) = maps;
        let sheet = |i: usize| sheets.get(i).copied().unwrap_or(i);
        let calc = |k: usize| calcs.get(k).copied().unwrap_or(k);
        self.table_views = std::mem::take(&mut self.table_views).into_iter().map(|(s, v)| (sheet(s), v)).collect::<HashMap<_, _>>();
        self.calc_spots = std::mem::take(&mut self.calc_spots).into_iter().map(|(k, v)| (calc(k), v)).collect::<HashMap<_, _>>();
        self.levels = std::mem::take(&mut self.levels).into_iter().map(|(s, v)| (sheet(s), v)).collect();
        self.layer_overrides = std::mem::take(&mut self.layer_overrides).into_iter().map(|((s, n), v)| ((sheet(s), n), v)).collect();
        self.calc_folds = std::mem::take(&mut self.calc_folds).into_iter().map(|(k, t, n)| (calc(k), t, n)).collect();
        self.view.sheet = sheet(self.view.sheet);
        self.view.calc = calc(self.view.calc);
        if let Some(other) = &mut self.other {
            other.sheet = sheet(other.sheet);
            other.calc = calc(other.calc);
        }
        self.refilter();
        self.refresh_calc_hidden();
        // The order of the calculations is the order of evaluation.
        let after = crate::sheet::problems(self.calc_results());
        let new: Vec<&String> = after.iter().filter(|p| !before.contains(p)).collect();
        self.order_warning = (!new.is_empty()).then(|| {
            let shown: Vec<&str> = new.iter().take(3).map(|s| s.as_str()).collect();
            let more = if new.len() > 3 { format!(" … and {} more", new.len() - 3) } else { String::new() };
            format!("Moving \"{name}\" broke the calculation: {}{more}", shown.join(" · "))
        });
    }

    /// Ctrl+PgDn / Ctrl+PgUp: the next / previous table or calculation in the
    /// bar (not further than the first and the last).
    pub(super) fn step_tab(&mut self, d: isize) {
        let items = tab_items(&self.doc);
        let here = self.current_tab().unwrap_or(TabItem::Table(self.view.sheet));
        let Some(at) = items.iter().position(|t| *t == here) else { return };
        let to = (at as isize + d).clamp(0, items.len() as isize - 1) as usize;
        match items[to] {
            TabItem::Table(i) => self.switch_sheet(i),
            TabItem::Calc(k) => self.show_calc(k),
        }
    }

    /// `:movetab left|right|first|last|<place>`: moves the shown table or calculation.
    pub(super) fn movetab_command(&mut self, args: &[&str]) {
        let items = tab_items(&self.doc);
        let n = items.len();
        let Some(at) = self.current_tab().and_then(|t| self.tab_position(t)).filter(|_| n >= 2) else {
            return self.error(":movetab moves the shown table or calculation among the others");
        };
        let to = match args.first().copied() {
            Some("left" | "-") => at.saturating_sub(1),
            Some("right" | "+") => (at + 1).min(n - 1),
            Some("first") => 0,
            Some("last") => n - 1,
            Some(p) => match p.parse::<usize>() {
                Ok(p) if (1..=n).contains(&p) => p - 1,
                _ => return self.error(format!("no place '{p}' – 1 to {n}")),
            },
            None => return self.error("usage: :movetab left|right|first|last|<place>"),
        };
        if to == at {
            return self.info("it is there already");
        }
        self.move_tab(at, to);
        if self.order_warning.is_none() {
            let name = self.tab_title(items[at]);
            self.info(format!("{name} is tab {} of {n}", to + 1));
        }
    }

    /// `:calc [N|name]` shows a calculation, `:calc new [name]` starts one.
    pub(super) fn calc_command(&mut self, args: &[&str]) {
        let blocks = crate::sheet::calc_blocks(&self.doc);
        match args.first().copied() {
            None => self.show_calc(if self.calc_view() { self.view.calc } else { self.last_calc.min(blocks.len().saturating_sub(1)) }),
            Some("new") => {
                let name = args[1..].join(" ");
                let info = if name.trim().is_empty() { crate::sheet::INFO.to_string() } else { format!("{} {}", crate::sheet::INFO, name.trim()) };
                self.snapshot();
                self.doc.blocks.push(Block::Code { info, body: Vec::new() });
                self.dirty = true;
                self.rebuild();
                self.show_calc(blocks.len());
            }
            Some(first) => {
                let wanted = args.join(" ");
                let by_number = first.parse::<usize>().ok().filter(|k| (1..=blocks.len()).contains(k)).map(|k| k - 1);
                let by_name = (0..blocks.len()).find(|&k| crate::sheet::calc_title(&self.doc, k).eq_ignore_ascii_case(&wanted));
                match by_number.or(by_name) {
                    Some(k) => self.show_calc(k),
                    None => self.error(format!("no calculation '{wanted}' – :calc new {wanted} starts one")),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::reorder_tabs;
    use crate::engine::Workbook;

    const DOC: &str = "<!-- hardcalc 1 -->\n\nIntro.\n\n## A :: matrix\n\n| x | y |\n|---|---|\n| 1 | 2 |\n\n```layer format\nB1: bold\n```\n\nBetween.\n\n## B :: matrix\n\n| x | y |\n|---|---|\n| 3 | 4 |\n\n```layer comments\nA1: note\n```\n\n## C :: matrix\n\n| x | y |\n|---|---|\n| 5 | 6 |\n\nEnd.\n";

    fn layers_by_table(doc: &crate::model::Document) -> Vec<(String, String)> {
        let wb = Workbook::from_document(doc).0;
        let mut out: Vec<(String, String)> = crate::layers::attach(doc, &wb).into_iter().map(|a| (wb.sheets[a.sheet].name.clone().unwrap_or_default(), a.layer.name)).collect();
        out.sort();
        out
    }

    #[test]
    fn layers_stay_with_their_table() {
        let mut doc = crate::parse(DOC).unwrap();
        let before = layers_by_table(&doc);
        assert_eq!(before, [("A".to_string(), "format".to_string()), ("B".to_string(), "comments".to_string())]);
        let (map, calcs) = reorder_tabs(&mut doc, 0, 2);
        assert_eq!(map, [2, 0, 1]);
        assert!(calcs.is_empty());
        let names: Vec<String> = Workbook::from_document(&doc).0.sheets.iter().map(|s| s.name.clone().unwrap_or_default()).collect();
        assert_eq!(names, ["B", "C", "A"]);
        assert_eq!(layers_by_table(&doc), before, "each layer is still attached to the same table");
        // What is not a table stays: the text keeps its place around them.
        let text = crate::write(&doc);
        assert!(text.contains("Intro.") && text.contains("Between.") && text.trim_end().ends_with("End."), "{text}");
        assert!(text.find("Intro.") < text.find("## B") && text.find("## B") < text.find("Between."), "{text}");
    }
}
