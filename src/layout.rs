//! Raster layouts: the ```layout block.
//!
//! ```text
//! page: A4 portrait            paper and orientation
//! font: 9                      point size; one grid row is one text line
//! grid: 12                     number of columns
//! 1-12 / 1-2    heading  Budget 2026
//! 1-6  / 3-15   table    Expenses   columns: Item, Amount   rows: top 5 by Amount
//! 7-12 / 3-15   chart    Spending
//! 1-12 / 16-18  text     Rent is {=Expenses[Rent, Share] | percent(1)} of the expenses.
//! 1-4  / 20-21  value    sum(Expenses.Gross)   label: Total expenses
//! 5-12 / 20-21  note     Figures are provisional
//! ```
//!
//! Placement is `columns / rows`, both 1-based and inclusive. Rows count on
//! across pages; where a page ends depends on paper and font.
//!
//! `rows:` shows only part of a table, `height: auto` makes an element as
//! tall as its content (what lies below keeps its distance).

use crate::print::{PageSetup, Paper, Scope};

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum ElementKind {
    Heading,
    #[default]
    Text,
    Value,
    Note,
    Table,
    Chart,
}

impl ElementKind {
    pub const ALL: [ElementKind; 6] =
        [ElementKind::Heading, ElementKind::Text, ElementKind::Value, ElementKind::Note, ElementKind::Table, ElementKind::Chart];

    pub fn name(self) -> &'static str {
        match self {
            ElementKind::Heading => "heading",
            ElementKind::Text => "text",
            ElementKind::Value => "value",
            ElementKind::Note => "note",
            ElementKind::Table => "table",
            ElementKind::Chart => "chart",
        }
    }

    pub fn parse(s: &str) -> Option<ElementKind> {
        ElementKind::ALL.into_iter().find(|k| k.name() == s)
    }
}

/// The rows of a table an element shows (`rows:`).
#[derive(Debug, Clone, PartialEq, Default)]
pub enum RowPick {
    #[default]
    All,
    /// `rows: 10` – the first rows.
    First(usize),
    /// `rows: last 12`
    Last(usize),
    /// `rows: top 5 by Amount` (largest first) / `rows: bottom 5 by Amount`.
    Ranked { n: usize, by: String, bottom: bool },
    /// `rows: total` – only the footer rows.
    Total,
}

impl RowPick {
    pub fn parse(s: &str) -> Result<RowPick, String> {
        let bad = || format!("rows: expected `10`, `last 10`, `top 5 by Column`, `bottom 5 by Column`, `total` or `all`, not '{s}'");
        let count = |w: &str| w.parse::<usize>().ok().filter(|n| *n > 0).ok_or_else(bad);
        let words: Vec<&str> = s.split_whitespace().collect();
        match words.as_slice() {
            ["all"] => Ok(RowPick::All),
            ["total"] | ["totals"] | ["footer"] => Ok(RowPick::Total),
            [n] | ["first", n] => Ok(RowPick::First(count(n)?)),
            ["last", n] => Ok(RowPick::Last(count(n)?)),
            [dir @ ("top" | "bottom"), n, "by", by @ ..] if !by.is_empty() => {
                Ok(RowPick::Ranked { n: count(n)?, by: by.join(" "), bottom: *dir == "bottom" })
            }
            _ => Err(bad()),
        }
    }

    /// The value after `rows:` (None for all rows).
    pub fn text(&self) -> Option<String> {
        match self {
            RowPick::All => None,
            RowPick::First(n) => Some(n.to_string()),
            RowPick::Last(n) => Some(format!("last {n}")),
            RowPick::Ranked { n, by, bottom } => Some(format!("{} {n} by {by}", if *bottom { "bottom" } else { "top" })),
            RowPick::Total => Some("total".into()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Element {
    /// First and last grid column (1-based, inclusive).
    pub cols: (usize, usize),
    /// First and last row (1-based, inclusive, counted across pages).
    pub rows: (usize, usize),
    pub kind: ElementKind,
    /// Text, value expression, or the name of a table / chart.
    pub content: String,
    /// Tables: the columns to show (all if empty).
    pub columns: Vec<String>,
    /// Values: the caption above the number.
    pub label: Option<String>,
    /// Tables: the slice or scenario shown (name or letter; a if none).
    pub z: Option<String>,
    /// Tables: the rows to show.
    pub pick: RowPick,
    /// `height: auto`: as tall as the content needs; `rows` is the designed
    /// size, and what lies below keeps its distance to the real one.
    pub auto: bool,
}

fn parse_span(s: &str) -> Result<(usize, usize), String> {
    let (a, b) = s.split_once('-').unwrap_or((s, s));
    let a: usize = a.trim().parse().map_err(|_| format!("bad range '{s}'"))?;
    let b: usize = b.trim().parse().map_err(|_| format!("bad range '{s}'"))?;
    if a == 0 || b < a {
        return Err(format!("bad range '{s}'"));
    }
    Ok((a, b))
}

impl Element {
    pub fn parse(line: &str) -> Result<Element, String> {
        let start = line.find(|c: char| c.is_ascii_alphabetic()).ok_or("missing element type")?;
        let (place, rest) = line.split_at(start);
        let (cols, rows) = place.split_once('/').ok_or_else(|| format!("expected `columns / rows`: {line}"))?;
        let rest = rest.trim();
        let (kind, content) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
        let kind = ElementKind::parse(kind).ok_or_else(|| format!("unknown element '{kind}'"))?;
        let mut e = Element { cols: parse_span(cols.trim())?, rows: parse_span(rows.trim())?, kind, ..Element::default() };
        e.set_text(content)?;
        Ok(e)
    }

    /// Options an element of this kind understands (`key: value` after the content).
    fn option_keys(kind: ElementKind) -> &'static [&'static str] {
        match kind {
            // A value or chart keeps the options of a table, so that F6 (kind) loses nothing.
            ElementKind::Table | ElementKind::Value | ElementKind::Chart => &["columns:", "rows:", "label:", "z:", "height:"],
            // Running text: only a closing `height: auto`, so that a colon in the text stays text.
            ElementKind::Heading | ElementKind::Text | ElementKind::Note => &[],
        }
    }

    /// Sets content and options from the text after the kind, as written in
    /// the file and typed in the editor: `Expenses columns: Item z: b`.
    pub fn set_text(&mut self, text: &str) -> Result<(), String> {
        let mut content = text.trim().to_string();
        (self.columns, self.label, self.z, self.pick, self.auto) = (Vec::new(), None, None, RowPick::All, false);
        if matches!(self.kind, ElementKind::Heading | ElementKind::Text | ElementKind::Note) {
            let words: Vec<&str> = content.split_whitespace().collect();
            if let [.., "height:", "auto"] = words.as_slice() {
                let i = content.rfind("height:").unwrap();
                content.truncate(i);
                self.auto = true;
            }
            self.content = content.trim().to_string();
            return Ok(());
        }
        // Options in any order; each value runs up to the next option.
        let padded = format!(" {content}");
        let mut found: Vec<(usize, &str)> =
            Self::option_keys(self.kind).iter().filter_map(|key| padded.find(&format!(" {key}")).map(|i| (i, *key))).collect();
        found.sort();
        for (k, &(i, key)) in found.iter().enumerate() {
            let end = found.get(k + 1).map_or(padded.len(), |n| n.0);
            let value = padded[i + 1 + key.len()..end].trim();
            match key {
                "z:" => self.z = Some(value.to_string()).filter(|v| !v.is_empty()),
                "label:" => self.label = Some(value.to_string()),
                "rows:" => self.pick = RowPick::parse(value)?,
                "height:" if value == "auto" => self.auto = true,
                "height:" if value == "fixed" || value.is_empty() => {}
                "height:" => return Err(format!("height: expected `auto`, not '{value}'")),
                _ => self.columns = value.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect(),
            }
        }
        if let Some(&(i, _)) = found.first() {
            content = padded[..i].to_string();
        }
        self.content = content.trim().to_string();
        Ok(())
    }

    /// Content and options with `gap` between them (see [`Element::set_text`]).
    fn text_with(&self, gap: &str) -> String {
        let mut text = self.content.clone();
        let mut add = |key: &str, value: &str| {
            if !text.is_empty() {
                text.push_str(gap);
            }
            text.push_str(key);
            text.push(' ');
            text.push_str(value);
        };
        let text_kind = Self::option_keys(self.kind).is_empty();
        if !self.columns.is_empty() && !text_kind {
            add("columns:", &self.columns.join(", "));
        }
        if let Some(rows) = self.pick.text().filter(|_| !text_kind) {
            add("rows:", &rows);
        }
        if let Some(l) = self.label.as_ref().filter(|_| !text_kind) {
            add("label:", l);
        }
        if let Some(z) = self.z.as_ref().filter(|_| !text_kind) {
            add("z:", z);
        }
        if self.auto {
            add("height:", "auto");
        }
        text
    }

    /// What the editor shows for typing: content and options.
    pub fn text(&self) -> String {
        self.text_with(" ")
    }

    pub fn to_line(&self) -> String {
        let line = format!(
            "{:<7} / {:<7} {:<8} {}",
            format!("{}-{}", self.cols.0, self.cols.1),
            format!("{}-{}", self.rows.0, self.rows.1),
            self.kind.name(),
            self.text_with("   ")
        );
        line.trim_end().to_string()
    }

    /// `height: auto` in effect (a chart has no height of its own).
    pub fn flows(&self) -> bool {
        self.auto && self.kind != ElementKind::Chart
    }

    pub fn height(&self) -> usize {
        self.rows.1 - self.rows.0 + 1
    }
}

/// A parsed layout block. `element_lines[i]` is the block line of element `i`.
#[derive(Debug, Clone, PartialEq)]
pub struct RasterSpec {
    pub paper: Paper,
    pub landscape: bool,
    pub font: f32,
    pub grid: usize,
    pub elements: Vec<Element>,
    pub element_lines: Vec<usize>,
    pub errors: Vec<String>,
}

impl RasterSpec {
    pub fn parse(lines: &[String]) -> RasterSpec {
        let mut spec = RasterSpec {
            paper: Paper::A4,
            landscape: false,
            font: 9.0,
            grid: 12,
            elements: Vec::new(),
            element_lines: Vec::new(),
            errors: Vec::new(),
        };
        for (i, line) in lines.iter().enumerate() {
            let t = line.trim();
            if t.is_empty() || t.starts_with('#') {
                continue;
            }
            if let Some(v) = t.strip_prefix("page:") {
                for word in v.split_whitespace() {
                    match word.to_lowercase().as_str() {
                        "portrait" => spec.landscape = false,
                        "landscape" => spec.landscape = true,
                        other => match Paper::parse(other) {
                            Some((p, orientation)) => {
                                spec.paper = p;
                                if let Some(landscape) = orientation {
                                    spec.landscape = landscape;
                                }
                            }
                            None => spec.errors.push(format!("line {}: unknown paper '{word}'", i + 1)),
                        },
                    }
                }
            } else if let Some(v) = t.strip_prefix("font:") {
                match v.trim().parse::<f32>() {
                    Ok(f) if (4.0..=24.0).contains(&f) => spec.font = f,
                    _ => spec.errors.push(format!("line {}: font must be 4–24", i + 1)),
                }
            } else if let Some(v) = t.strip_prefix("grid:") {
                match v.trim().parse::<usize>() {
                    Ok(g) if (1..=48).contains(&g) => spec.grid = g,
                    _ => spec.errors.push(format!("line {}: grid must be 1–48", i + 1)),
                }
            } else {
                match Element::parse(t) {
                    Ok(e) => {
                        spec.elements.push(e);
                        spec.element_lines.push(i);
                    }
                    Err(e) => spec.errors.push(format!("line {}: {e}", i + 1)),
                }
            }
        }
        spec
    }

    pub fn page_line(&self) -> String {
        format!("page: {} {}", self.paper.name(), if self.landscape { "landscape" } else { "portrait" })
    }

    pub fn setup(&self, index: usize) -> PageSetup {
        PageSetup { paper: self.paper, landscape: self.landscape, font_size: Some(self.font), scope: Scope::Layout(index) }
    }

    /// Character columns (x, width) of an element for a page `cols` characters wide.
    pub fn span(&self, cols: usize, e: &Element) -> (usize, usize) {
        let unit = cols as f64 / self.grid as f64;
        let c1 = e.cols.0.clamp(1, self.grid);
        let c2 = e.cols.1.clamp(c1, self.grid);
        let x = ((c1 - 1) as f64 * unit).round() as usize;
        let end = (c2 as f64 * unit).round() as usize;
        // One character of gutter between neighbouring columns.
        let gap = usize::from(c2 < self.grid);
        (x, end.saturating_sub(x + gap).max(1))
    }

    /// Last row used by any element.
    pub fn last_row(&self) -> usize {
        self.elements.iter().map(|e| e.rows.1).max().unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(src: &str) -> Vec<String> {
        src.lines().map(str::to_string).collect()
    }

    #[test]
    fn parse_and_write_elements() {
        let spec = RasterSpec::parse(&lines(
            "page: A4 landscape\nfont: 8\n\
             1-6 / 3-15   table    Expenses   columns: Item, Amount\n\
             1-4 / 20-21  value    sum(Expenses.Gross)   label: Total expenses\n\
             7 / 1        heading  Budget 2026",
        ));
        assert!(spec.errors.is_empty(), "{:?}", spec.errors);
        assert!(spec.landscape && spec.font == 8.0);
        let t = &spec.elements[0];
        assert_eq!((t.cols, t.rows, t.kind), ((1, 6), (3, 15), ElementKind::Table));
        assert_eq!(t.content, "Expenses");
        assert_eq!(t.columns, ["Item", "Amount"]);
        assert_eq!(spec.elements[1].label.as_deref(), Some("Total expenses"));
        assert_eq!(spec.elements[2].cols, (7, 7));
        assert_eq!(spec.element_lines, [2, 3, 4]);
        // Writing and reading again gives the same element.
        for e in &spec.elements {
            assert_eq!(&Element::parse(&e.to_line()).unwrap(), e);
        }
        assert_eq!(t.to_line(), "1-6     / 3-15    table    Expenses   columns: Item, Amount");
    }

    #[test]
    fn options_in_any_order() {
        let e = Element::parse("1-6 / 4-12 table Expenses z: February rows: top 5 by Net amount columns: Item, Amount height: auto").unwrap();
        assert_eq!(e.content, "Expenses");
        assert_eq!(e.columns, ["Item", "Amount"]);
        assert_eq!(e.z.as_deref(), Some("February"));
        assert_eq!(e.pick, RowPick::Ranked { n: 5, by: "Net amount".into(), bottom: false });
        assert!(e.auto);
        assert_eq!(
            e.to_line(),
            "1-6     / 4-12    table    Expenses   columns: Item, Amount   rows: top 5 by Net amount   z: February   height: auto"
        );
        assert_eq!(&Element::parse(&e.to_line()).unwrap(), &e);
        for (text, pick) in [("10", RowPick::First(10)), ("first 3", RowPick::First(3)), ("last 12", RowPick::Last(12)), ("total", RowPick::Total)] {
            assert_eq!(RowPick::parse(text).unwrap(), pick);
            let again = Element::parse(&format!("1 / 1 table T rows: {text}")).unwrap();
            assert_eq!(Element::parse(&again.to_line()).unwrap().pick, pick);
        }
        assert!(Element::parse("1 / 1 table T rows: many").is_err());
        assert!(Element::parse("1 / 1 table T rows: top 0 by A").is_err());
        // Running text keeps its colons; only a closing `height: auto` is an option.
        let t = Element::parse("1-12 / 3-4 text Total rows: 5, label: none height: auto").unwrap();
        assert_eq!((t.content.as_str(), t.auto, t.label.as_deref()), ("Total rows: 5, label: none", true, None));
        assert_eq!(&Element::parse(&t.to_line()).unwrap(), &t);
    }

    #[test]
    fn errors_are_collected() {
        let ledger = RasterSpec::parse(&lines("page: Ledger"));
        assert!(ledger.errors.is_empty() && ledger.paper == Paper::Tabloid && ledger.landscape);
        assert_eq!(RasterSpec::parse(&lines("page: a3 landscape")).paper, Paper::A3);
        let spec = RasterSpec::parse(&lines("page: A5\n1-2 / x  text hi\n3 / 1 bogus"));
        assert_eq!(spec.errors.len(), 3, "{:?}", spec.errors);
    }

    #[test]
    fn grid_geometry() {
        let spec = RasterSpec::parse(&lines("grid: 12"));
        let e = |c1, c2| Element { cols: (c1, c2), rows: (1, 1), ..Element::default() };
        assert_eq!(spec.span(96, &e(1, 6)), (0, 47));
        assert_eq!(spec.span(96, &e(7, 12)), (48, 48));
        assert_eq!(spec.span(96, &e(1, 12)), (0, 96));
    }
}
