//! The tables as tabs: moving them (`:movetab`, `-` / `+` on the table origin,
//! dragging a tab) and remembering where each one was left.

use std::collections::HashMap;

use super::{App, ViewKind};
use crate::model::{Block, Document};

/// Where a table was left: cursor and first visible row / column.
pub(super) type Spot = ((usize, usize), (usize, usize));

/// Moves table `from` to place `to` (both counted among the tables). A table
/// takes the layers that follow it without a target along; everything else
/// stays where it is in the document. Returns for each old place the new one.
pub fn reorder_tables(doc: &mut Document, from: usize, to: usize) -> Vec<usize> {
    let tables: Vec<usize> = doc.blocks.iter().enumerate().filter(|(_, b)| matches!(b, Block::Table(_))).map(|(i, _)| i).collect();
    let n = tables.len();
    if from >= n || to >= n {
        return (0..n).collect();
    }
    // The blocks of each table: it, and the layers after it that belong to it.
    let units: Vec<Vec<usize>> = (0..n)
        .map(|k| {
            let end = tables.get(k + 1).copied().unwrap_or(doc.blocks.len());
            let mut unit = vec![tables[k]];
            unit.extend((tables[k] + 1..end).filter(|&i| matches!(&doc.blocks[i], Block::Layer(l) if l.target.is_none())));
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
    let mut map = vec![0; n];
    for (new, &old) in order.iter().enumerate() {
        map[old] = new;
    }
    map
}

impl App {
    /// Notes where the shown table is, before another view takes the window.
    pub(super) fn remember_table_view(&mut self) {
        if self.view.kind == ViewKind::Table {
            self.table_views.insert(self.view.sheet, (self.view.cursor, self.view.scroll));
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

    /// Moves the table in the tab bar from `from` to `to`; the view, the
    /// remembered places and the slices shown follow it.
    pub(super) fn move_table(&mut self, from: usize, to: usize) {
        let n = self.wb.sheets.len();
        if from >= n || to >= n || from == to {
            return;
        }
        self.remember_table_view();
        let mut map = Vec::new();
        self.with_doc(|doc| map = reorder_tables(doc, from, to));
        let at = |i: usize| map.get(i).copied().unwrap_or(i);
        self.table_views = std::mem::take(&mut self.table_views).into_iter().map(|(s, v)| (at(s), v)).collect::<HashMap<_, _>>();
        self.levels = std::mem::take(&mut self.levels).into_iter().map(|(s, v)| (at(s), v)).collect();
        self.layer_overrides = std::mem::take(&mut self.layer_overrides).into_iter().map(|((s, n), v)| ((at(s), n), v)).collect();
        self.view.sheet = at(self.view.sheet);
        if let Some(other) = &mut self.other {
            other.sheet = at(other.sheet);
        }
        self.refilter();
    }

    /// `:movetab left|right|first|last|<place>`: moves the shown table.
    pub(super) fn movetab_command(&mut self, args: &[&str]) {
        let n = self.wb.sheets.len();
        if self.view.kind != ViewKind::Table || n < 2 {
            return self.error(":movetab moves the shown table among the others");
        }
        let at = self.view.sheet;
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
            return self.info("the table is there already");
        }
        self.move_table(at, to);
        let name = self.wb.sheets[self.view.sheet].name.clone().unwrap_or_default();
        self.info(format!("{name} is table {} of {n}", self.view.sheet + 1));
    }
}

#[cfg(test)]
mod tests {
    use super::reorder_tables;
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
        let map = reorder_tables(&mut doc, 0, 2);
        assert_eq!(map, [2, 0, 1]);
        let names: Vec<String> = Workbook::from_document(&doc).0.sheets.iter().map(|s| s.name.clone().unwrap_or_default()).collect();
        assert_eq!(names, ["B", "C", "A"]);
        assert_eq!(layers_by_table(&doc), before, "each layer is still attached to the same table");
        // What is not a table stays: the text keeps its place around them.
        let text = crate::write(&doc);
        assert!(text.contains("Intro.") && text.contains("Between.") && text.trim_end().ends_with("End."), "{text}");
        assert!(text.find("Intro.") < text.find("## B") && text.find("## B") < text.find("Between."), "{text}");
    }
}
