//! Text formatting of the selection, as in Excel: bold, italic, underline,
//! text and fill color, plain. Each writes rules into the table's `format`
//! layer: whole columns become column rules (`B: bold`), whole rows row
//! rules (`3: bold`), the table origin `*`, anything else the cell or range.

use super::{App, Header, ViewKind, FORMAT};
use crate::engine::{col_letters, CellId};
use crate::layers::{part_family, style_parts, CellStyle, Target, STYLE_FAMILIES};
use crate::model::{Block, Rule};

/// Colors offered for `:fg` / `:bg` (any `#rrggbb` works too).
pub const COLORS: &[&str] = &[
    "black", "red", "green", "yellow", "blue", "magenta", "cyan", "gray", "darkgray", "white", "lightred", "lightgreen",
    "lightyellow", "lightblue", "lightmagenta", "lightcyan", "none",
];

/// What a formatting command changes.
#[derive(Debug, Clone, PartialEq)]
pub enum StyleChange {
    /// Bold, italic or underline: on, or off.
    Flag(&'static str, bool),
    /// `fg` or `bg`: a color, or `None` for none.
    Color(&'static str, Option<String>),
    /// Line breaks in long text: `Some(width)` (0: the usual maximum), or off.
    Wrap(Option<u16>),
    /// All of the text style off.
    Plain,
}

/// Rows and columns a rule target covers; `None`: all of them.
type Region = (Option<(usize, usize)>, Option<(usize, usize)>);

fn within(inner: Option<(usize, usize)>, outer: Option<(usize, usize)>) -> bool {
    match (inner, outer) {
        (_, None) => true,
        (None, Some(_)) => false,
        (Some((a, b)), Some((c, d))) => c <= a && b <= d,
    }
}

/// Whether `color` is something the format layer and the terminal know.
pub fn valid_color(color: &str) -> bool {
    if let Some(hex) = color.strip_prefix('#') {
        return hex.len() == 6 && hex.chars().all(|c| c.is_ascii_hexdigit());
    }
    COLORS.contains(&color) || color == "grey" || color == "darkgrey"
}

impl App {
    /// The rule targets for the selection.
    fn format_targets(&self) -> Vec<String> {
        let (from, to) = self.selection();
        match self.on_header() {
            Some(Header::Column) => (from.1..=to.1).map(col_letters).collect(),
            Some(Header::Row) => (from.0..=to.0).map(|r| (r + 1).to_string()).collect(),
            Some(Header::Corner) => vec!["*".into()],
            None => {
                let a1 = |(r, c): (usize, usize)| format!("{}{}", col_letters(c), r + 1);
                vec![if from == to { a1(from) } else { format!("{}:{}", a1(from), a1(to)) }]
            }
        }
    }

    fn region(&self, sheet: usize, target: &Target) -> Option<Region> {
        let columns = &self.wb.sheets[sheet].columns;
        Some(match target {
            Target::All => (None, None),
            Target::Column(name) => {
                let c = columns.iter().position(|c| c.name == *name).or_else(|| {
                    crate::engine::letters_to_col(name).filter(|&c| c < columns.len())
                })?;
                (None, Some((c, c)))
            }
            Target::Row(n) if *n >= 1 => (Some((n - 1, n - 1)), None),
            Target::Row(_) => return None,
            Target::Cell(r, c) => (Some((*r, *r)), Some((*c, *c))),
            Target::Range((c1, r1), (c2, r2)) => (Some((*r1.min(r2), *r1.max(r2))), Some((*c1.min(c2), *c1.max(c2)))),
            Target::RowWhere { .. } => return None,
        })
    }

    /// The style of cell `id` from all format rules except those for `skip`.
    fn style_without(&mut self, id: CellId, skip: &[String]) -> CellStyle {
        let rules: Vec<(Target, CellStyle)> = self
            .attached
            .iter()
            .filter(|a| a.layer.name == FORMAT && a.sheet == id.0)
            .flat_map(|a| {
                a.layer.rules.iter().filter_map(|r| match r {
                    Rule::Rule { target, body } if !skip.contains(target) => Some((Target::parse(target), CellStyle::parse(body).0)),
                    _ => None,
                })
            })
            .collect();
        let mut st = CellStyle::default();
        for (t, s) in rules {
            if t.matches(&mut self.wb, id) {
                st.merge(&s);
            }
        }
        st
    }

    /// `:bold`, `:italic`, `:underline` (and Ctrl+B): switches the attribute
    /// of the selection – on unless the cursor cell has it already.
    pub fn toggle_style(&mut self, family: &'static str) {
        if !self.format_allowed() {
            return;
        }
        let (from, _) = self.selection();
        let on = if self.dims().0 == 0 {
            true
        } else {
            let st = self.formats.style(&mut self.wb, (self.view.sheet, from.0, from.1));
            !match family {
                "bold" => st.bold,
                "italic" => st.italic,
                _ => st.underline,
            }
        };
        self.apply_style(StyleChange::Flag(family, on));
    }

    fn format_allowed(&mut self) -> bool {
        if self.view.kind != ViewKind::Table || self.view.pivot.is_some() {
            self.error("formatting works in tables");
            return false;
        }
        true
    }

    /// Writes `change` for the selection into the format layer, as one undoable step.
    pub fn apply_style(&mut self, change: StyleChange) {
        if !self.format_allowed() {
            return;
        }
        let sheet = self.view.sheet;
        let targets = self.format_targets();
        let families: Vec<&str> = match &change {
            StyleChange::Flag(f, _) | StyleChange::Color(f, _) => vec![f],
            StyleChange::Wrap(_) => vec!["wrap"],
            StyleChange::Plain => STYLE_FAMILIES.to_vec(),
        };
        let rows = self.dims().0;
        let mut changes: Vec<(String, String)> = Vec::new();
        for target in &targets {
            let t = Target::parse(target);
            let Some(region) = self.region(sheet, &t) else { continue };
            // What the other rules give the region's first cell.
            let base = if rows == 0 {
                CellStyle::default()
            } else {
                let first = (region.0.map_or(0, |r| r.0).min(rows - 1), region.1.map_or(0, |c| c.0));
                self.style_without((sheet, first.0, first.1), &targets)
            };
            let body = self.layer_rule(FORMAT, sheet, target).unwrap_or_default();
            let mut parts: Vec<String> = style_parts(&body).into_iter().filter(|p| !families.contains(&part_family(p))).collect();
            for family in &families {
                let wanted = match (&change, *family) {
                    (StyleChange::Flag(_, on), _) => {
                        let has = match *family {
                            "bold" => base.bold,
                            "italic" => base.italic,
                            _ => base.underline,
                        };
                        match (on, has) {
                            (true, false) => Some(family.to_string()),
                            (false, true) => Some(format!("no{family}")),
                            _ => None,
                        }
                    }
                    (StyleChange::Color(_, color), _) => {
                        let has = if *family == "fg" { base.fg.is_some() } else { base.bg.is_some() };
                        match color {
                            Some(c) => Some(format!("{family} {c}")),
                            None if has => Some(format!("{family} none")),
                            None => None,
                        }
                    }
                    (StyleChange::Wrap(Some(0)), _) => Some("wrap".to_string()),
                    (StyleChange::Wrap(Some(n)), _) => Some(format!("wrap {n}")),
                    (StyleChange::Wrap(None), _) => base.wrap.is_some().then(|| "nowrap".to_string()),
                    (StyleChange::Plain, "bold") => base.bold.then(|| "nobold".to_string()),
                    (StyleChange::Plain, "italic") => base.italic.then(|| "noitalic".to_string()),
                    (StyleChange::Plain, "underline") => base.underline.then(|| "nounderline".to_string()),
                    (StyleChange::Plain, "fg") => base.fg.is_some().then(|| "fg none".to_string()),
                    (StyleChange::Plain, _) => base.bg.is_some().then(|| "bg none".to_string()),
                };
                parts.extend(wanted);
            }
            changes.push((target.clone(), parts.join(" ")));
            // Narrower rules inside the region lose what was just set, as in Excel.
            for (other, body) in self.format_rules(sheet) {
                if targets.contains(&other) {
                    continue;
                }
                let Some(inner) = self.region(sheet, &Target::parse(&other)) else { continue };
                if within(inner.0, region.0) && within(inner.1, region.1) {
                    let kept: Vec<String> = style_parts(&body).into_iter().filter(|p| !families.contains(&part_family(p))).collect();
                    if kept.len() != style_parts(&body).len() {
                        changes.push((other, kept.join(" ")));
                    }
                }
            }
        }
        self.set_layer_rules(FORMAT, sheet, &changes);
        let what = match &change {
            StyleChange::Flag(f, true) => f.to_string(),
            StyleChange::Flag(f, false) => format!("no{f}"),
            StyleChange::Color(f, Some(c)) => format!("{f} {c}"),
            StyleChange::Color(f, None) => format!("{f} none"),
            StyleChange::Wrap(Some(0)) => "wrap".into(),
            StyleChange::Wrap(Some(n)) => format!("wrap {n}"),
            StyleChange::Wrap(None) => "nowrap".into(),
            StyleChange::Plain => "plain".into(),
        };
        self.info(format!("{what}: {} – rule in the format layer", targets.join(", ")));
    }

    /// (target, body) of the rules in the format layer blocks of table `sheet`.
    fn format_rules(&self, sheet: usize) -> Vec<(String, String)> {
        self.attached
            .iter()
            .filter(|a| a.layer.name == FORMAT && a.sheet == sheet)
            .flat_map(|a| {
                a.layer.rules.iter().filter_map(|r| match r {
                    Rule::Rule { target, body } => Some((target.clone(), body.clone())),
                    Rule::Raw(_) => None,
                })
            })
            .collect()
    }

    /// Sets (or with an empty body removes) several rules of `layer` in table
    /// `sheet` as one undoable step; the block is created after the table if needed.
    pub(super) fn set_layer_rules(&mut self, layer: &str, sheet: usize, changes: &[(String, String)]) {
        use crate::model::Layer;
        if changes.is_empty() {
            return;
        }
        let (found, insert_at) = self.layer_block(layer, sheet);
        if found.is_none() && changes.iter().all(|(_, b)| b.is_empty()) {
            return;
        }
        // Rules of this table in other blocks of the layer (e.g. `@Table` further down).
        let owners: Vec<Option<usize>> = {
            let mut table = None;
            self.doc
                .blocks
                .iter()
                .map(|b| match b {
                    Block::Table(_) => {
                        table = Some(table.map_or(0, |t: usize| t + 1));
                        None
                    }
                    Block::Layer(l) if l.name == layer => match &l.target {
                        Some(name) => self.wb.sheet_index(name),
                        None => table,
                    },
                    _ => None,
                })
                .collect()
        };
        let layer = layer.to_string();
        let changes = changes.to_vec();
        self.with_doc(|doc| {
            let index = match found {
                Some(i) => i,
                None => {
                    let at = insert_at.unwrap_or(doc.blocks.len());
                    doc.blocks.insert(at, Block::Layer(Layer { line: Default::default(), name: layer.clone(), target: None, rules: Vec::new() }));
                    at
                }
            };
            let shift = |i: usize| if found.is_none() && i >= index { i + 1 } else { i };
            for (target, body) in changes {
                // The rule where it is (any block of the table), else in the first block.
                let mut done = false;
                for (i, owner) in owners.iter().enumerate() {
                    if *owner != Some(sheet) {
                        continue;
                    }
                    let Block::Layer(l) = &mut doc.blocks[shift(i)] else { continue };
                    if let Some(k) = l.rules.iter().position(|r| matches!(r, Rule::Rule { target: t, .. } if *t == target)) {
                        if body.is_empty() {
                            l.rules.remove(k);
                        } else {
                            l.rules[k] = Rule::Rule { target: target.clone(), body: body.clone() };
                        }
                        done = true;
                        break;
                    }
                }
                if !done
                    && !body.is_empty()
                    && let Block::Layer(l) = &mut doc.blocks[index]
                {
                    l.rules.push(Rule::Rule { target, body });
                }
            }
            // A format block that became empty goes.
            if let Block::Layer(l) = &doc.blocks[index]
                && l.rules.is_empty()
            {
                doc.blocks.remove(index);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colors() {
        assert!(valid_color("red") && valid_color("#a0b1c2") && valid_color("none"));
        assert!(!valid_color("redd") && !valid_color("#12345"));
    }

    #[test]
    fn regions() {
        assert!(within(Some((1, 2)), None));
        assert!(within(Some((1, 2)), Some((0, 3))));
        assert!(!within(None, Some((0, 3))));
        assert!(!within(Some((1, 4)), Some((0, 3))));
    }

    #[test]
    fn known_layer_names() {
        // The families the commands write are the ones the format layer reads.
        for f in STYLE_FAMILIES {
            let (st, errs) = crate::layers::CellStyle::parse(&format!("no{f}"));
            if f == "fg" || f == "bg" {
                assert!(!errs.is_empty());
            } else {
                assert!(errs.is_empty() && st.off != crate::layers::Off::default(), "{f}");
            }
        }
    }
}
