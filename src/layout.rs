//! Raster layouts: the ```layout block.
//!
//! ```text
//! page: A4 portrait            paper and orientation
//! font: 9                      point size; one grid row is one text line
//! grid: 12                     number of columns
//! 1-12 / 1-2    heading  Budget 2026
//! 1-6  / 3-15   table    Expenses   columns: Item, Amount
//! 7-12 / 3-15   chart    Spending
//! 1-12 / 16-18  text     Rent is {=Expenses[Rent, Share] | percent(1)} of the expenses.
//! 1-4  / 20-21  value    sum(Expenses.Gross)   label: Total expenses
//! 5-12 / 20-21  note     Figures are provisional
//! ```
//!
//! Placement is `columns / rows`, both 1-based and inclusive. Rows count on
//! across pages; where a page ends depends on paper and font.

use crate::print::{PageSetup, Paper, Scope};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ElementKind {
    Heading,
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

#[derive(Debug, Clone, PartialEq)]
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
        let mut content = content.trim().to_string();
        let mut columns = Vec::new();
        let mut label = None;
        let mut z = None;
        for key in ["z:", "label:", "columns:"] {
            if let Some(i) = content.find(&format!(" {key}")) {
                let value = content[i + key.len() + 1..].trim().to_string();
                content.truncate(i);
                match key {
                    "z:" => z = Some(value).filter(|v| !v.is_empty()),
                    "label:" => label = Some(value),
                    _ => columns = value.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect(),
                }
            }
        }
        Ok(Element { cols: parse_span(cols.trim())?, rows: parse_span(rows.trim())?, kind, content: content.trim().to_string(), columns, label, z })
    }

    pub fn to_line(&self) -> String {
        let mut line = format!(
            "{:<7} / {:<7} {:<8} {}",
            format!("{}-{}", self.cols.0, self.cols.1),
            format!("{}-{}", self.rows.0, self.rows.1),
            self.kind.name(),
            self.content
        );
        if !self.columns.is_empty() {
            line.push_str(&format!("   columns: {}", self.columns.join(", ")));
        }
        if let Some(l) = &self.label {
            line.push_str(&format!("   label: {l}"));
        }
        if let Some(z) = &self.z {
            line.push_str(&format!("   z: {z}"));
        }
        line.trim_end().to_string()
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
        let e = |c1, c2| Element { cols: (c1, c2), rows: (1, 1), kind: ElementKind::Text, content: String::new(), columns: vec![], label: None, z: None };
        assert_eq!(spec.span(96, &e(1, 6)), (0, 47));
        assert_eq!(spec.span(96, &e(7, 12)), (48, 48));
        assert_eq!(spec.span(96, &e(1, 12)), (0, 96));
    }
}
