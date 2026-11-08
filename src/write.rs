//! Writes a [`Document`] in canonical, git-friendly form.

use crate::model::*;
use unicode_width::UnicodeWidthStr;

pub fn write(doc: &Document) -> String {
    // Formulas typed as `Table.Column` are written as `Table!Column`.
    let mut doc = doc.clone();
    crate::refs::normalize_table_dots(&mut doc);
    // Numbers are written with a decimal point, whatever was typed or read.
    crate::engine::normalize_numbers(&mut doc);
    let parts: Vec<String> = doc.blocks.iter().map(write_block).collect();
    let mut out = parts.join("\n\n");
    out.push('\n');
    out
}

fn write_block(block: &Block) -> String {
    match block {
        Block::Text(lines) => lines.join("\n"),
        Block::Table(t) => {
            let mut parts = vec![write_table(t)];
            if !t.footer.is_empty() {
                parts.push(write_footer(t));
            }
            parts.extend(t.layers.iter().map(|l| write_layer(t, l)));
            parts.join("\n\n")
        }
        Block::Types(lines) => fence("types", lines),
        Block::Functions(lines) => fence("functions", lines),
        Block::Layer(l) => {
            let mut info = format!("layer {}", l.name);
            if let Some(t) = &l.target {
                info.push_str(&format!(" @{t}"));
            }
            let lines: Vec<String> = l
                .rules
                .iter()
                .map(|r| match r {
                    Rule::Rule { target, body } => format!("{target}: {body}"),
                    Rule::Raw(s) => s.clone(),
                })
                .collect();
            fence(&info, &lines)
        }
        Block::Chart { name, lines } => fence(&format!("chart {name}"), lines),
        Block::Layout { name, lines } => fence(&format!("layout {name}"), lines),
        Block::Code { info, body } => fence(info, body),
    }
}

/// The table's footer block: a table of the labels and the cells of every column.
fn write_footer(t: &Table) -> String {
    let header: Vec<String> = std::iter::once(String::new()).chain(t.columns.iter().map(|c| escape_cell(&c.name))).collect();
    let rows: Vec<Vec<String>> = t
        .footer
        .iter()
        .map(|r| {
            std::iter::once(escape_cell(&r.label))
                .chain((0..t.columns.len()).map(|c| escape_cell(r.cells.get(c).map_or("", |s| s.as_str()))))
                .collect()
        })
        .collect();
    let lines = grid_lines(&header, &rows, t.is_compact());
    let info = match &t.name {
        Some(name) => format!("footer {name}"),
        None => "footer".into(),
    };
    fence(&info, &lines)
}

/// A data layer: its cells as a table with the table's columns; trailing
/// empty rows are left out.
fn write_layer(t: &Table, l: &DataLayer) -> String {
    let used = l.rows.iter().rposition(|r| r.iter().any(|c| !c.trim().is_empty())).map_or(0, |r| r + 1);
    let header: Vec<String> = t.columns.iter().map(|c| escape_cell(&c.name)).collect();
    let rows: Vec<Vec<String>> = l.rows[..used]
        .iter()
        .map(|r| (0..t.columns.len()).map(|c| escape_cell(r.get(c).map_or("", |s| s.as_str()))).collect())
        .collect();
    fence(&format!("{} {}", l.kind.name(), l.name), &grid_lines(&header, &rows, t.is_compact()))
}

/// Header, separator and rows of a table inside a block, columns padded
/// (`compact`: not).
fn grid_lines(header: &[String], rows: &[Vec<String>], compact: bool) -> Vec<String> {
    let widths: Vec<usize> = (0..header.len())
        .map(|i| if compact { 1 } else { std::iter::once(&header[i]).chain(rows.iter().map(|r| &r[i])).map(|s| s.width()).max().unwrap_or(0).max(3) })
        .collect();
    let line = |cells: &[String]| {
        let parts: Vec<String> =
            cells.iter().zip(&widths).map(|(c, &w)| if compact { c.clone() } else { format!("{c}{}", " ".repeat(w - c.width())) }).collect();
        format!("| {} |", parts.join(" | "))
    };
    let mut lines = vec![line(header)];
    lines.push(format!("|{}|", widths.iter().map(|w| "-".repeat(w + 2)).collect::<Vec<_>>().join("|")));
    lines.extend(rows.iter().map(|r| line(r)));
    lines
}

fn fence(info: &str, body: &[String]) -> String {
    // Use a longer fence if the body itself contains ``` lines.
    let marker = if body.iter().any(|l| l.trim_start().starts_with("```")) { "````" } else { "```" };
    let mut out = format!("{marker}{info}\n");
    for l in body {
        out.push_str(l);
        out.push('\n');
    }
    out.push_str(marker);
    out
}

fn escape_cell(s: &str) -> String {
    s.replace('|', "\\|")
}

fn write_table(t: &Table) -> String {
    if t.shape == Shape::Playground {
        // Only the used area: empty rows and columns at the edges are dropped.
        let filled = |s: &String| !s.trim().is_empty();
        let rows = t.rows.iter().rposition(|r| r.iter().any(filled)).map_or(0, |r| r + 1);
        let cols = t.rows.iter().filter_map(|r| r.iter().rposition(filled)).max().map_or(1, |c| c + 1);
        if rows < t.rows.len() || cols < t.columns.len() {
            let mut used = t.clone();
            used.columns.truncate(cols);
            used.rows.truncate(rows);
            for r in &mut used.rows {
                r.truncate(cols);
            }
            return write_table_as_is(&used);
        }
    }
    write_table_as_is(t)
}

fn write_table_as_is(t: &Table) -> String {
    let header: Vec<String> = t
        .columns
        .iter()
        .map(|c| match &c.ty {
            Some(ty) => escape_cell(&format!("{}:{}", c.name, ty)),
            None => escape_cell(&c.name),
        })
        .collect();
    let rows: Vec<Vec<String>> = t.rows.iter().map(|r| r.iter().map(|c| escape_cell(c)).collect()).collect();

    let widths: Vec<usize> = (0..header.len())
        .map(|i| {
            let w = std::iter::once(&header[i])
                .chain(rows.iter().filter_map(|r| r.get(i)))
                .map(|s| s.width())
                .max()
                .unwrap_or(0);
            if t.is_compact() { 1 } else { w.max(3) }
        })
        .collect();

    let mut out = String::new();
    if let (Some(level), Some(name)) = (t.heading_level, &t.name) {
        let layer = t.layer_name.as_ref().map(|l| format!(" [{l}]")).unwrap_or_default();
        let fill = match t.fill {
            Some(true) => " fill",
            Some(false) => " nofill",
            None => "",
        };
        let aligned = if t.is_compact() { "" } else { " aligned" };
        out.push_str(&format!("{} {} :: {}{fill}{aligned}{layer}\n\n", "#".repeat(level), name, t.shape));
    }

    let pad = |s: &str, w: usize, align: Align| {
        if t.is_compact() {
            return s.to_string();
        }
        let fill = w.saturating_sub(s.width());
        match align {
            Align::Right => format!("{}{}", " ".repeat(fill), s),
            Align::Center => format!("{}{}{}", " ".repeat(fill / 2), s, " ".repeat(fill - fill / 2)),
            _ => format!("{}{}", s, " ".repeat(fill)),
        }
    };
    let row_line = |cells: &[String]| {
        let parts: Vec<String> = cells
            .iter()
            .enumerate()
            .map(|(i, c)| pad(c, widths[i], t.columns[i].align))
            .collect();
        format!("| {} |", parts.join(" | "))
    };

    out.push_str(&row_line(&header));
    out.push('\n');
    // Each separator segment spans the cell width plus its two padding spaces.
    let seps: Vec<String> = t
        .columns
        .iter()
        .zip(&widths)
        .map(|(c, &w)| match c.align {
            Align::None => "-".repeat(w + 2),
            Align::Left => format!(":{}", "-".repeat(w + 1)),
            Align::Right => format!("{}:", "-".repeat(w + 1)),
            Align::Center => format!(":{}:", "-".repeat(w)),
        })
        .collect();
    out.push_str(&format!("|{}|", seps.join("|")));
    for r in &rows {
        out.push('\n');
        out.push_str(&row_line(r));
    }
    out
}
