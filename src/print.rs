//! Paged print layout and PDF output.
//!
//! Pages are laid out on a monospaced character grid and the PDF uses the
//! built-in Courier fonts, so a terminal preview of the grid shows exactly
//! what the PDF contains. No external dependencies: the PDF is written by hand.

use crate::chart::{ChartData, ChartType};
use crate::layers::Color;
use crate::model::Align;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Paper {
    A4,
    A3,
    Letter,
    Legal,
    /// 11 × 17 in; in landscape also called Ledger.
    Tabloid,
}

impl Paper {
    pub const ALL: [Paper; 5] = [Paper::A4, Paper::A3, Paper::Letter, Paper::Legal, Paper::Tabloid];

    /// Portrait size in PDF points.
    pub fn size(self) -> (f32, f32) {
        match self {
            Paper::A4 => (595.28, 841.89),
            Paper::A3 => (841.89, 1190.55),
            Paper::Letter => (612.0, 792.0),
            Paper::Legal => (612.0, 1008.0),
            Paper::Tabloid => (792.0, 1224.0),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Paper::A4 => "A4",
            Paper::A3 => "A3",
            Paper::Letter => "Letter",
            Paper::Legal => "Legal",
            Paper::Tabloid => "Tabloid",
        }
    }

    /// Paper name, case-insensitive; `ledger` is Tabloid in landscape, so it
    /// also returns the orientation it implies.
    pub fn parse(s: &str) -> Option<(Paper, Option<bool>)> {
        match s.to_lowercase().as_str() {
            "ledger" => Some((Paper::Tabloid, Some(true))),
            other => Paper::ALL.into_iter().find(|p| p.name().to_lowercase() == other).map(|p| (p, None)),
        }
    }

    /// Display name of a paper in an orientation (`Ledger` for Tabloid landscape).
    pub fn label(self, landscape: bool) -> String {
        match (self, landscape) {
            (Paper::Tabloid, true) => "Ledger (Tabloid landscape)".into(),
            (p, true) => format!("{} landscape", p.name()),
            (p, false) => format!("{} portrait", p.name()),
        }
    }
}

/// All paper / orientation combinations, as offered in the paper menu.
pub fn paper_choices() -> Vec<(Paper, bool)> {
    Paper::ALL.into_iter().flat_map(|p| [(p, false), (p, true)]).collect()
}

/// What gets printed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Scope {
    /// The current table with its charts.
    Table,
    /// The whole document as flow layout.
    Document,
    /// Raster layout `n` (the n-th ```layout block).
    Layout(usize),
}

#[derive(Debug, Clone, PartialEq)]
pub struct PageSetup {
    pub paper: Paper,
    pub landscape: bool,
    /// Font size in points; `None` fits the widest table to the page width.
    pub font_size: Option<f32>,
    pub scope: Scope,
}

impl Default for PageSetup {
    fn default() -> Self {
        PageSetup { paper: Paper::A4, landscape: false, font_size: None, scope: Scope::Table }
    }
}

/// Page margin on all sides (15 mm).
pub const MARGIN: f32 = 42.52;
pub const MIN_FONT: f32 = 5.0;
pub const MAX_FONT: f32 = 10.0;
/// Courier glyph width as a fraction of the font size.
pub const CHAR_W: f32 = 0.6;
/// Line height as a multiple of the font size.
pub const LINE_H: f32 = 1.3;
const COL_GAP: usize = 2;
const MAX_COL: usize = 40;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct SpanStyle {
    pub bold: bool,
    pub italic: bool,
    pub color: Option<(u8, u8, u8)>,
    /// Light gray background (table headers and notes in raster layouts).
    pub shade: bool,
    /// Background color of the cell (layer `bg`), already light for paper.
    pub bg: Option<(u8, u8, u8)>,
    /// Break the text into lines (layer `wrap`): the width, 0 for the usual maximum.
    pub wrap: Option<u16>,
    /// Text standing in for a picture (a typeset formula): shown in the
    /// text preview where the picture cannot be, never on the PDF page.
    pub stand_in: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Span {
    /// Character column on the page.
    pub col: usize,
    pub text: String,
    pub style: SpanStyle,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LineKind {
    Blank,
    Title,
    Header,
    Row,
    Footer,
    /// Space taken by a chart (see [`Layout::graphics`]).
    Chart,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub kind: LineKind,
    pub spans: Vec<Span>,
    /// Width in characters of a header's shaded band.
    pub width: usize,
}

impl Line {
    fn blank() -> Line {
        Line { kind: LineKind::Blank, spans: Vec::new(), width: 0 }
    }

    fn text(kind: LineKind, text: String, style: SpanStyle) -> Line {
        Line { kind, spans: vec![Span { col: 0, text, style }], width: 0 }
    }

    /// The line as plain text (for tests and debugging).
    pub fn plain(&self) -> String {
        let mut out = String::new();
        for s in &self.spans {
            let w = out.width();
            if s.col > w {
                out.push_str(&" ".repeat(s.col - w));
            }
            out.push_str(&s.text);
        }
        out
    }
}

/// Input: one table, already rendered to display text.
#[derive(Debug, Clone)]
pub struct PrintTable {
    pub title: String,
    pub headers: Vec<String>,
    pub rows: Vec<Vec<PrintCell>>,
    /// Footnotes printed below the table (`¹ text`).
    pub footnotes: Vec<String>,
    /// How many of the last `rows` are footer rows (totals).
    pub footer_rows: usize,
}

/// Something to print, in document order.
#[derive(Debug, Clone)]
pub enum PrintItem {
    Table(PrintTable),
    Chart(ChartData),
    /// Markdown text (live values already filled in).
    Text(Vec<String>),
    /// A code block, printed verbatim.
    Code(Vec<String>),
    /// The lines of a calculation sheet, the calculations typeset where Typst is built in.
    Calc(Vec<CalcLine>),
}

/// A line of a calculation sheet for printing.
#[derive(Debug, Clone, Default)]
pub struct CalcLine {
    pub label: Option<String>,
    /// The calculation (or heading, comment …) as text.
    pub text: String,
    /// The calculation as Typst math, for those that are typeset.
    pub math: Option<String>,
    pub comment: Option<String>,
    /// A check that fails: in red.
    pub failed: bool,
}

/// A typeset formula on a page: from line `line` and character column
/// `col` on, `width` × `height` points.
#[derive(Debug, Clone)]
pub struct Picture {
    pub line: usize,
    pub col: usize,
    pub width: f32,
    pub height: f32,
    /// Identifies what it shows.
    pub key: u64,
    pub image: std::sync::Arc<crate::typeset::Image>,
}

/// Pixels per point of typeset formulas on paper (about 300 dpi).
const PICTURE_SCALE: f32 = 4.0;

/// Lines a printed chart takes, including its title line.
pub const CHART_LINES: usize = 18;

/// A chart placed on a page: `height` lines from `line` on, `width`
/// characters from column `col` on.
#[derive(Debug, Clone)]
pub struct Graphic {
    pub line: usize,
    pub height: usize,
    pub col: usize,
    pub width: usize,
    pub data: ChartData,
}

/// Content of a raster-layout element, already sized to its area.
#[derive(Debug, Clone)]
pub enum Placed {
    Lines(Vec<Line>),
    Chart(ChartData),
}

/// An element of a raster layout: rows count on across pages.
#[derive(Debug, Clone)]
pub struct PlacedElement {
    pub row: usize,
    pub col: usize,
    pub width: usize,
    pub height: usize,
    pub content: Placed,
    /// `height: auto`: the lines run on over page ends, repeating the first
    /// `n` of them (a table's title and header) on every further page.
    pub flow: Option<usize>,
    /// Table rows that did not fit into the element.
    pub hidden: usize,
}

/// Where the `n` lines of a flowing element land when it starts in the
/// (0-based) row `row`: for every row it takes, the index of the line shown
/// there. The first `head` lines are repeated at the top of each further page.
pub fn flow_rows(row: usize, n: usize, head: usize, body: usize) -> Vec<usize> {
    let head = if head + 1 < body && head < n { head } else { 0 };
    let mut out = Vec::with_capacity(n);
    let mut next = 0;
    while next < n {
        if !out.is_empty() && (row + out.len()).is_multiple_of(body) {
            out.extend(0..head);
        }
        out.push(next);
        next += 1;
    }
    out
}

#[derive(Debug, Clone)]
pub struct PrintCell {
    pub text: String,
    pub align: Align,
    pub style: SpanStyle,
}

#[derive(Debug, Clone)]
pub struct Layout {
    pub pages: Vec<Vec<Line>>,
    /// Charts per page.
    pub graphics: Vec<Vec<Graphic>>,
    /// Typeset formulas per page.
    pub pictures: Vec<Vec<Picture>>,
    /// Characters per line and lines per page (including the footer).
    pub cols: usize,
    pub rows: usize,
    pub font_size: f32,
    /// Page size in points (orientation applied).
    pub width: f32,
    pub height: f32,
}

impl Layout {
    /// Margins expressed in characters / lines, for the preview.
    pub fn margin_cells(&self) -> (usize, usize) {
        let mx = (MARGIN / (CHAR_W * self.font_size)).round() as usize;
        let my = (MARGIN / (LINE_H * self.font_size)).round() as usize;
        (mx, my)
    }
}

pub fn fit(text: &str, w: usize, align: Align) -> String {
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
        return out;
    }
    let fill = w - tw;
    match align {
        Align::Right => format!("{}{text}", " ".repeat(fill)),
        Align::Center => format!("{}{text}", " ".repeat(fill / 2)),
        _ => text.to_string(),
    }
}

/// Column width in characters of a `wrap` rule (0: the usual maximum).
fn wrap_width(n: u16) -> usize {
    if n == 0 { MAX_COL } else { n as usize }
}

fn column_widths(t: &PrintTable) -> Vec<usize> {
    (0..t.headers.len())
        .map(|c| {
            let cells = || t.rows.iter().filter_map(|r| r.get(c));
            let widest = cells().filter_map(|cell| cell.style.wrap).map(wrap_width).fold(MAX_COL, usize::max);
            std::iter::once(t.headers[c].width())
                .chain(cells().map(|cell| match cell.style.wrap {
                    Some(n) => cell.text.width().min(wrap_width(n)),
                    None => cell.text.width(),
                }))
                .max()
                .unwrap_or(0)
                .clamp(3, widest)
        })
        .collect()
}

/// Splits columns into bands that fit into `cols` characters each.
fn bands(widths: &[usize], cols: usize) -> Vec<std::ops::Range<usize>> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut used = 0;
    for (c, &w) in widths.iter().enumerate() {
        let need = if c == start { w } else { used + COL_GAP + w };
        if c > start && need > cols {
            out.push(start..c);
            start = c;
            used = w;
        } else {
            used = need;
        }
    }
    if start < widths.len() {
        out.push(start..widths.len());
    }
    out
}

fn table_width(widths: &[usize]) -> usize {
    widths.iter().sum::<usize>() + COL_GAP * widths.len().saturating_sub(1)
}

/// Characters per line and lines per page for a setup with a fixed font size.
pub fn page_grid(setup: &PageSetup, font_size: f32) -> (usize, usize, f32, f32) {
    let (mut width, mut height) = setup.paper.size();
    if setup.landscape {
        std::mem::swap(&mut width, &mut height);
    }
    let cols = ((width - 2.0 * MARGIN) / (CHAR_W * font_size)).floor() as usize;
    let rows = ((height - 2.0 * MARGIN) / (LINE_H * font_size)).floor() as usize;
    (cols, rows, width, height)
}

/// Lines per page available to a raster layout (the last two hold the footer).
pub fn layout_rows_per_page(setup: &PageSetup) -> usize {
    let (_, rows, _, _) = page_grid(setup, setup.font_size.unwrap_or(9.0));
    rows.saturating_sub(2).max(4)
}

/// Places raster-layout elements on pages. Elements that cross a page end
/// are cut there.
pub fn compose(setup: &PageSetup, elements: &[PlacedElement], footer: &str) -> Layout {
    let font_size = setup.font_size.unwrap_or(9.0);
    let (cols, rows, width, height) = page_grid(setup, font_size);
    let body = rows.saturating_sub(2).max(4);
    let span = |e: &PlacedElement| match (&e.content, e.flow) {
        (Placed::Lines(lines), Some(head)) => flow_rows(e.row, lines.len(), head, body).len().max(1),
        _ => e.height,
    };
    let n_pages = elements.iter().map(|e| (e.row + span(e)).div_ceil(body)).max().unwrap_or(1).max(1);
    let blank_page = || (0..body).map(|_| Line::blank()).collect::<Vec<_>>();
    let mut pages: Vec<Vec<Line>> = (0..n_pages).map(|_| blank_page()).collect();
    let mut graphics: Vec<Vec<Graphic>> = vec![Vec::new(); n_pages];

    for e in elements {
        let (page, line) = (e.row / body, e.row % body);
        match &e.content {
            Placed::Chart(data) => graphics[page].push(Graphic {
                line,
                height: e.height.min(body - line),
                col: e.col,
                width: e.width.min(cols.saturating_sub(e.col)),
                data: data.clone(),
            }),
            Placed::Lines(lines) => {
                // A flowing element continues on the next pages; any other is cut.
                let order: Vec<usize> = match e.flow {
                    Some(head) => flow_rows(e.row, lines.len(), head, body),
                    None => (0..lines.len().min(e.height)).collect(),
                };
                for (i, l) in order.into_iter().map(|k| &lines[k]).enumerate() {
                    let abs = e.row + i;
                    // Cut at the page end: an element belongs to the page it starts on.
                    if abs / body != page && e.flow.is_none() {
                        break;
                    }
                    let target = &mut pages[abs / body][abs % body];
                    if l.kind != LineKind::Blank {
                        target.kind = LineKind::Row;
                    }
                    for span in &l.spans {
                        let room = e.width.saturating_sub(span.col);
                        if room == 0 {
                            continue;
                        }
                        let text = fit(&span.text, room, Align::Left);
                        target.spans.push(Span { col: e.col + span.col, text, style: span.style.clone() });
                    }
                }
            }
        }
    }
    for p in &mut pages {
        for l in p.iter_mut() {
            l.spans.sort_by_key(|s| s.col);
        }
    }
    add_footers(&mut pages, rows, cols, footer);
    let pictures = vec![Vec::new(); pages.len()];
    Layout { pages, graphics, pictures, cols, rows, font_size, width, height }
}

/// Lays out `items` on pages. `title` heads the first page, `footer` is
/// printed left on every page next to the page number.
pub fn paginate(title: &str, items: &[PrintItem], setup: &PageSetup, footer: &str) -> Layout {
    let tables: Vec<&PrintTable> = items.iter().filter_map(|i| if let PrintItem::Table(t) = i { Some(t) } else { None }).collect();
    let (mut width, mut height) = setup.paper.size();
    if setup.landscape {
        std::mem::swap(&mut width, &mut height);
    }
    let usable_w = width - 2.0 * MARGIN;
    let usable_h = height - 2.0 * MARGIN;

    let all_widths: Vec<Vec<usize>> = tables.iter().map(|t| column_widths(t)).collect();
    let widest = all_widths.iter().map(|w| table_width(w)).max().unwrap_or(1).max(1);
    let font_size = setup
        .font_size
        .unwrap_or_else(|| (usable_w / (CHAR_W * widest as f32)).clamp(MIN_FONT, MAX_FONT))
        .clamp(MIN_FONT, 24.0);
    let cols = (usable_w / (CHAR_W * font_size)).floor() as usize;
    let rows = (usable_h / (LINE_H * font_size)).floor() as usize;
    // The last two lines of every page hold a gap and the footer.
    let body = rows.saturating_sub(2).max(4);

    let bold = SpanStyle { bold: true, ..SpanStyle::default() };
    let mut pages: Vec<Vec<Line>> = Vec::new();
    let mut graphics: Vec<Vec<Graphic>> = Vec::new();
    let mut pictures: Vec<Vec<Picture>> = Vec::new();
    let mut page: Vec<Line> = Vec::new();
    if !title.is_empty() {
        page.push(Line::text(LineKind::Title, fit(title, cols, Align::Left), bold.clone()));
        page.push(Line::blank());
    }

    let mut table_index = 0;
    for item in items {
        let t = match item {
            PrintItem::Table(t) => t,
            PrintItem::Text(lines) => {
                for line in text_lines(lines, cols) {
                    // Keep a heading together with what follows.
                    let need = if line.kind == LineKind::Title { 3 } else { 1 };
                    if !page.is_empty() && page.len() + need > body {
                        pages.push(std::mem::take(&mut page));
                    }
                    if page.is_empty() && line.kind == LineKind::Blank {
                        continue;
                    }
                    page.push(line);
                }
                if page.len() < body && page.last().is_some_and(|l| l.kind != LineKind::Blank) {
                    page.push(Line::blank());
                }
                continue;
            }
            PrintItem::Code(lines) => {
                let gray = SpanStyle { color: Some((70, 70, 70)), ..SpanStyle::default() };
                for l in lines {
                    if page.len() >= body {
                        pages.push(std::mem::take(&mut page));
                    }
                    page.push(Line::text(LineKind::Row, fit(l, cols, Align::Left), gray.clone()));
                }
                if page.len() < body {
                    page.push(Line::blank());
                }
                continue;
            }
            PrintItem::Calc(lines) => {
                calc_lines(lines, font_size, cols, body, &mut page, &mut pages, &mut pictures);
                if page.len() < body {
                    page.push(Line::blank());
                }
                continue;
            }
            PrintItem::Chart(data) => {
                // Bar charts take as many lines as they have bars; plots a fixed height.
                let h = match data.kind {
                    ChartType::Bar | ChartType::Histogram => {
                        let n = data.series.len();
                        data.categories.len() * (n + usize::from(n > 1)) + 2
                    }
                    _ => CHART_LINES,
                }
                .clamp(4, body);
                if !page.is_empty() && page.len() + h > body {
                    pages.push(std::mem::take(&mut page));
                }
                graphics.resize(pages.len() + 1, Vec::new());
                graphics[pages.len()].push(Graphic { line: page.len(), height: h, col: 0, width: cols, data: data.clone() });
                page.extend((0..h).map(|_| Line { kind: LineKind::Chart, spans: Vec::new(), width: 0 }));
                if page.len() < body {
                    page.push(Line::blank());
                }
                continue;
            }
        };
        let widths = &all_widths[table_index];
        table_index += 1;
        for band in bands(widths, cols) {
            let band_widths = &widths[band.clone()];
            let shown = table_width(band_widths).min(cols);
            let mut name = t.title.clone();
            if band.start > 0 || band.end < widths.len() {
                name = format!("{name} (columns {}–{})", col_name(band.start), col_name(band.end - 1));
            }
            let heading = |cont: bool| {
                let text = if cont { format!("{name} (continued)") } else { name.clone() };
                Line::text(LineKind::Title, fit(&text, cols, Align::Left), bold.clone())
            };
            let header = {
                let mut spans = Vec::new();
                let mut x = 0;
                for (i, c) in band.clone().enumerate() {
                    let w = band_widths[i];
                    if x >= cols {
                        break;
                    }
                    spans.push(Span { col: x, text: fit(&t.headers[c], w.min(cols - x), Align::Left), style: bold.clone() });
                    x += w + COL_GAP;
                }
                Line { kind: LineKind::Header, spans, width: shown }
            };

            // Start a table on a new page if not even its heading and two rows fit.
            if !page.is_empty() && page.len() + 4 > body {
                pages.push(std::mem::take(&mut page));
            }
            page.push(heading(false));
            page.push(header.clone());
            for row in &t.rows {
                // Wrapped cells make a row several lines high; it stays together.
                let cell_lines: Vec<Vec<String>> = band
                    .clone()
                    .enumerate()
                    .map(|(i, c)| match row.get(c) {
                        Some(cell) if cell.style.wrap.is_some() => crate::text::wrap_plain(&cell.text, band_widths[i].min(cols)),
                        Some(cell) => vec![cell.text.clone()],
                        None => vec![String::new()],
                    })
                    .collect();
                let height = cell_lines.iter().map(Vec::len).max().unwrap_or(1);
                if page.len() + height > body {
                    pages.push(std::mem::take(&mut page));
                    page.push(heading(true));
                    page.push(header.clone());
                }
                for k in 0..height {
                    let mut spans = Vec::new();
                    let mut x = 0;
                    for (i, c) in band.clone().enumerate() {
                        let w = band_widths[i];
                        if x >= cols {
                            break;
                        }
                        if let Some(cell) = row.get(c) {
                            let part = cell_lines[i].get(k).map_or("", String::as_str);
                            if !part.is_empty() || cell.style.bg.is_some() {
                                let cw = w.min(cols - x);
                                let mut text = fit(part, cw, cell.align);
                                // A coloured cell is coloured to its edge, not only under its text.
                                if cell.style.bg.is_some() {
                                    text.push_str(&" ".repeat(cw.saturating_sub(text.width())));
                                }
                                spans.push(Span { col: x, text, style: cell.style.clone() });
                            }
                        }
                        x += w + COL_GAP;
                    }
                    page.push(Line { kind: LineKind::Row, spans, width: shown });
                }
            }
            if band.end == widths.len() {
                let gray = SpanStyle { color: Some((80, 80, 80)), ..SpanStyle::default() };
                for note in &t.footnotes {
                    for line in crate::text::wrap(&crate::text::inline(note), cols, 2) {
                        if page.len() >= body {
                            pages.push(std::mem::take(&mut page));
                        }
                        page.push(styled_line(LineKind::Row, &line, Some(&gray)));
                    }
                }
            }
            if page.len() < body {
                page.push(Line::blank());
            }
        }
    }
    if !page.is_empty() || pages.is_empty() {
        pages.push(page);
    }

    add_footers(&mut pages, rows, cols, footer);
    graphics.resize(pages.len(), Vec::new());
    pictures.resize(pages.len(), Vec::new());
    Layout { pages, graphics, pictures, cols, rows, font_size, width, height }
}

/// The lines of a calculation sheet onto the pages: each calculation
/// typeset as a picture (its text stands in for it in the text preview),
/// the label left of it, the comment right of it or below; as text where
/// it cannot be typeset.
fn calc_lines(lines: &[CalcLine], fs: f32, cols: usize, body: usize, page: &mut Vec<Line>, pages: &mut Vec<Vec<Line>>, pictures: &mut Vec<Vec<Picture>>) {
    use std::hash::{Hash, Hasher};
    let (cw, lh) = (CHAR_W * fs, LINE_H * fs);
    let gray = SpanStyle { color: Some((70, 70, 70)), ..SpanStyle::default() };
    let red = SpanStyle { color: Some((190, 0, 0)), ..SpanStyle::default() };
    let note = SpanStyle { color: Some((90, 90, 90)), italic: true, ..SpanStyle::default() };
    // All formulas in two runs: black, and red for the failing checks.
    let mut images: Vec<Option<crate::typeset::Image>> = vec![None; lines.len()];
    for failed in [false, true] {
        let which: Vec<usize> = (0..lines.len()).filter(|&i| lines[i].failed == failed && lines[i].math.is_some()).collect();
        if which.is_empty() {
            continue;
        }
        let math: Vec<String> = which.iter().filter_map(|&i| lines[i].math.clone()).collect();
        let color = if failed { [190, 0, 0] } else { [0, 0, 0] };
        let style = crate::typeset::Style { size: fs, scale: PICTURE_SCALE, color, display: true };
        for (&i, img) in which.iter().zip(crate::typeset::formulas(&math, style)) {
            images[i] = img;
        }
    }
    // The formulas start in one column, after the widest label.
    let label_w = lines.iter().filter_map(|l| l.label.as_ref()).map(|l| l.width() + 3).max().unwrap_or(0).min(cols / 3);
    let label_span = |l: &CalcLine| l.label.as_ref().map(|t| Span { col: 0, text: fit(&format!("[{t}]"), label_w.saturating_sub(1), Align::Left), style: gray.clone() });
    let new_page = |page: &mut Vec<Line>, pages: &mut Vec<Vec<Line>>| pages.push(std::mem::take(page));
    for (l, img) in lines.iter().zip(images) {
        let text_style = if l.failed { red.clone() } else { gray.clone() };
        let Some(img) = img else {
            // As text: the label, the calculation, the comment.
            let mut text = match &l.label {
                Some(t) => format!("[{t}] {}", l.text),
                None => l.text.clone(),
            };
            if let Some(c) = &l.comment {
                text.push_str(&format!("   ' {c}"));
            }
            if page.len() >= body {
                new_page(page, pages);
            }
            page.push(Line::text(LineKind::Row, fit(&text, cols, Align::Left), text_style));
            continue;
        };
        let col = label_w;
        let (mut w, mut h) = (img.width as f32 / PICTURE_SCALE, img.height as f32 / PICTURE_SCALE);
        let room = cols.saturating_sub(col).max(1) as f32 * cw;
        if w > room {
            h *= room / w;
            w = room;
        }
        let n = ((h / lh) - 0.05).ceil().max(1.0) as usize;
        let after = col + (w / cw).ceil() as usize + 3;
        let comment = l.comment.as_ref().map(|c| (c, after + c.width() <= cols));
        let need = n + usize::from(comment.is_some_and(|(_, beside)| !beside));
        if !page.is_empty() && page.len() + need > body {
            new_page(page, pages);
        }
        let start = page.len();
        // Label, stand-in and comment on the middle line, beside the formula.
        for k in 0..n {
            let mut spans = Vec::new();
            if k == (n - 1) / 2 {
                spans.extend(label_span(l));
                spans.push(Span { col, text: fit(&l.text, cols - col, Align::Left), style: SpanStyle { stand_in: true, ..text_style.clone() } });
                if let Some((c, true)) = comment {
                    spans.push(Span { col: after, text: c.clone(), style: note.clone() });
                }
            }
            page.push(Line { kind: LineKind::Row, spans, width: 0 });
        }
        if let Some((c, false)) = comment {
            page.push(Line::text(LineKind::Row, fit(c, cols.saturating_sub(col), Align::Left), note.clone()));
            if let Some(last) = page.last_mut() {
                last.spans[0].col = col;
            }
        }
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        (&l.math, l.failed, fs.to_bits()).hash(&mut hash);
        pictures.resize(pages.len() + 1, Vec::new());
        pictures[pages.len()].push(Picture { line: start, col, width: w, height: h, key: hash.finish(), image: std::sync::Arc::new(img) });
    }
}

/// Lines a picture of `height` points takes at font size `fs`.
pub fn picture_lines(height: f32, fs: f32) -> usize {
    ((height / (LINE_H * fs)) - 0.05).ceil().max(1.0) as usize
}

/// Turns wrapped, styled text into a print line; `base` overrides the color.
pub fn styled_line(kind: LineKind, pieces: &[crate::text::Styled], base: Option<&SpanStyle>) -> Line {
    let mut spans = Vec::new();
    let mut col = 0;
    for p in pieces {
        let style = SpanStyle {
            bold: p.bold || kind == LineKind::Title,
            italic: p.italic,
            color: base.and_then(|b| b.color),
            shade: base.is_some_and(|b| b.shade),
            bg: base.and_then(|b| b.bg),
            wrap: base.and_then(|b| b.wrap),
            stand_in: false,
        };
        spans.push(Span { col, text: p.text.clone(), style });
        col += p.text.width();
    }
    Line { kind, spans, width: 0 }
}

/// Markdown-lite: headings, paragraphs (wrapped), list items, blank lines.
fn text_lines(lines: &[String], cols: usize) -> Vec<Line> {
    use crate::text::{inline, wrap, Styled};
    let mut out = Vec::new();
    let mut para: Vec<String> = Vec::new();
    let flush = |para: &mut Vec<String>, out: &mut Vec<Line>| {
        if !para.is_empty() {
            let text = para.join(" ");
            for l in wrap(&inline(&text), cols, 0) {
                out.push(styled_line(LineKind::Row, &l, None));
            }
            para.clear();
        }
    };
    for raw in lines {
        let t = raw.trim();
        let level = t.chars().take_while(|&c| c == '#').count();
        let list = ["- ", "* ", "+ "].iter().find_map(|p| t.strip_prefix(p)).map(|rest| ("• ".to_string(), rest)).or_else(|| {
            let digits = t.chars().take_while(char::is_ascii_digit).count();
            (digits > 0 && t[digits..].starts_with(". ")).then(|| (format!("{} ", &t[..digits + 1]), &t[digits + 2..]))
        });
        if t.is_empty() {
            flush(&mut para, &mut out);
            if out.last().is_some_and(|l: &Line| l.kind != LineKind::Blank) {
                out.push(Line::blank());
            }
        } else if (1..=6).contains(&level) && t[level..].starts_with(' ') {
            flush(&mut para, &mut out);
            for l in wrap(&inline(t[level..].trim()), cols, 0) {
                out.push(styled_line(LineKind::Title, &l, None));
            }
        } else if let Some((bullet, rest)) = list {
            flush(&mut para, &mut out);
            let mut pieces = vec![Styled { text: bullet.clone(), bold: false, italic: false }];
            pieces.extend(inline(rest));
            for l in wrap(&pieces, cols, bullet.width()) {
                out.push(styled_line(LineKind::Row, &l, None));
            }
        } else {
            para.push(t.to_string());
        }
    }
    flush(&mut para, &mut out);
    out
}

/// A table squeezed into `w` × `h` characters for a raster layout: title,
/// shaded header, rows (with a "more rows" note if cut; the footer rows stay)
/// and footnotes. `columns` selects and orders columns by name (all if
/// empty). Also returns the number of data rows left out.
pub fn table_lines(t: &PrintTable, columns: &[String], w: usize, h: usize) -> (Vec<Line>, usize) {
    let picked: Vec<usize> = if columns.is_empty() {
        (0..t.headers.len()).collect()
    } else {
        // Headers may carry a footnote marker; match on the name before it.
        columns.iter().filter_map(|c| t.headers.iter().position(|hd| hd.trim_end_matches(|ch: char| "⁰¹²³⁴⁵⁶⁷⁸⁹".contains(ch)) == c)).collect()
    };
    let mut widths: Vec<usize> = picked
        .iter()
        .map(|&c| {
            let widest = t.rows.iter().filter_map(|r| r[c].style.wrap).map(wrap_width).fold(MAX_COL, usize::max);
            let cell_width = |r: &Vec<PrintCell>| match r[c].style.wrap {
                Some(n) => r[c].text.width().min(wrap_width(n)),
                None => r[c].text.width(),
            };
            std::iter::once(t.headers[c].width()).chain(t.rows.iter().map(cell_width)).max().unwrap_or(3).clamp(3, widest)
        })
        .collect();
    // Shrink the widest columns until the table fits.
    while table_width(&widths) > w {
        let (i, &max) = widths.iter().enumerate().max_by_key(|(_, w)| **w).unwrap();
        if max <= 3 {
            break;
        }
        widths[i] -= 1;
    }
    let gray = SpanStyle { color: Some((100, 100, 100)), ..SpanStyle::default() };
    let bold = SpanStyle { bold: true, ..SpanStyle::default() };
    let row_line = |cells: Vec<(String, Align, SpanStyle)>, shade: bool| {
        let mut spans = Vec::new();
        let mut x = 0;
        for (k, (text, align, mut style)) in cells.into_iter().enumerate() {
            if x >= w {
                break;
            }
            let cw = widths[k].min(w - x);
            // Header cells include the gap so the shading is continuous.
            let pad = if shade && k + 1 < widths.len() { COL_GAP.min(w - x - cw) } else { 0 };
            let mut text = fit(&text, cw, align);
            if shade {
                text = format!("{text:<width$}", width = cw + pad);
                style.shade = true;
            } else if style.bg.is_some() {
                text = format!("{text:<width$}", width = cw);
            }
            spans.push(Span { col: x, text, style });
            x += widths[k] + COL_GAP;
        }
        Line { kind: LineKind::Row, spans, width: 0 }
    };
    let mut out = vec![Line::text(LineKind::Title, fit(&t.title, w, Align::Left), bold.clone())];
    out.push(row_line(picked.iter().map(|&c| (t.headers[c].clone(), Align::Left, bold.clone())).collect(), true));
    let room = h.saturating_sub(out.len());
    // A row with wrapped cells is several lines high.
    let groups: Vec<Vec<Line>> = t
        .rows
        .iter()
        .map(|row| {
            let parts: Vec<Vec<String>> = picked
                .iter()
                .enumerate()
                .map(|(k, &c)| match row[c].style.wrap {
                    Some(_) => crate::text::wrap_plain(&row[c].text, widths[k]),
                    None => vec![row[c].text.clone()],
                })
                .collect();
            let height = parts.iter().map(Vec::len).max().unwrap_or(1);
            (0..height)
                .map(|i| {
                    row_line(
                        picked
                            .iter()
                            .enumerate()
                            .map(|(k, &c)| (parts[k].get(i).cloned().unwrap_or_default(), row[c].align, row[c].style.clone()))
                            .collect(),
                        false,
                    )
                })
                .collect()
        })
        .collect();
    let fits = groups.iter().map(Vec::len).sum::<usize>() <= room;
    let (data, footer) = groups.split_at(groups.len() - t.footer_rows.min(groups.len()));
    // When rows are left out, the totals stay if there is room for them.
    let footer_lines: usize = footer.iter().map(Vec::len).sum();
    let keep_footer = !fits && footer_lines > 0 && footer_lines + 2 <= room;
    let data_room = if fits { room } else { room.saturating_sub(1 + if keep_footer { footer_lines } else { 0 }) };
    let mut shown = 0;
    let mut used = 0;
    for g in if fits { &groups[..] } else { data } {
        if !fits && used + g.len() > data_room {
            break;
        }
        out.extend(g.iter().cloned());
        used += g.len();
        shown += 1;
    }
    let hidden = if fits { 0 } else { data.len() - shown };
    if !fits {
        let rows = if hidden == 1 { "row" } else { "rows" };
        out.push(Line::text(LineKind::Row, format!("… {hidden} more {rows}"), gray.clone()));
        if keep_footer {
            out.extend(footer.iter().flatten().cloned());
        }
    }
    for note in &t.footnotes {
        if out.len() < h {
            out.push(Line::text(LineKind::Row, fit(note, w, Align::Left), gray.clone()));
        }
    }
    (out, hidden)
}

/// Pads every page to `rows - 1` lines and adds the footer line:
/// left `footer`, right "page i/n".
fn add_footers(pages: &mut [Vec<Line>], rows: usize, cols: usize, footer: &str) {
    let n = pages.len();
    for (i, p) in pages.iter_mut().enumerate() {
        while p.len() < rows.saturating_sub(1) {
            p.push(Line::blank());
        }
        let right = format!("page {}/{n}", i + 1);
        let left = fit(footer, cols.saturating_sub(right.width() + 2), Align::Left);
        let gray = SpanStyle { color: Some((110, 110, 110)), ..SpanStyle::default() };
        p.push(Line {
            kind: LineKind::Footer,
            spans: vec![
                Span { col: 0, text: left, style: gray.clone() },
                Span { col: cols.saturating_sub(right.width()), text: right, style: gray },
            ],
            width: 0,
        });
    }
}

fn col_name(c: usize) -> String {
    crate::engine::col_letters(c)
}

/// Print-friendly RGB for a layer color (light colors are darkened for white paper).
pub fn print_color(c: &Color) -> Option<(u8, u8, u8)> {
    match c {
        Color::Rgb(r, g, b) => Some((*r, *g, *b)),
        Color::Named(n) => Some(match n.replace(['-', '_', ' '], "").as_str() {
            "black" => (0, 0, 0),
            "red" | "lightred" => (190, 0, 0),
            "green" | "lightgreen" => (0, 130, 0),
            "yellow" | "lightyellow" => (160, 120, 0),
            "blue" | "lightblue" => (0, 60, 200),
            "magenta" | "lightmagenta" => (150, 0, 150),
            "cyan" | "lightcyan" => (0, 120, 140),
            "gray" | "grey" => (110, 110, 110),
            "darkgray" | "darkgrey" => (80, 80, 80),
            "white" => (150, 150, 150),
            _ => return None,
        }),
    }
}

/// Light background for a layer `bg` color on paper: named colors are
/// lightened so black text stays readable, RGB values are taken as written.
pub fn print_bg(c: &Color) -> Option<(u8, u8, u8)> {
    match c {
        Color::Rgb(r, g, b) => Some((*r, *g, *b)),
        Color::Named(_) => {
            let (r, g, b) = print_color(c)?;
            let lighten = |x: u8| (x as u32 + (255 - x as u32) * 3 / 4) as u8;
            Some((lighten(r), lighten(g), lighten(b)))
        }
    }
}

// ---------------------------------------------------------------- PDF

/// Maps a char to the WinAnsi encoding of the standard PDF fonts.
fn win_ansi(c: char) -> u8 {
    match c as u32 {
        0x20..=0x7e | 0xa0..=0xff => c as u32 as u8,
        _ => match c {
            '€' => 0x80,
            '‚' => 0x82,
            '„' => 0x84,
            '…' => 0x85,
            '‘' => 0x91,
            '’' => 0x92,
            '“' => 0x93,
            '”' => 0x94,
            '•' => 0x95,
            '–' => 0x96,
            '—' => 0x97,
            '™' => 0x99,
            // Superscripts beyond ¹²³ are not in WinAnsi; print plain digits.
            '⁰' => b'0',
            '⁴'..='⁹' => b'4' + (c as u32 - '⁴' as u32) as u8,
            _ => b'?',
        },
    }
}

fn pdf_string(s: &str) -> Vec<u8> {
    let mut out = vec![b'('];
    for c in s.chars() {
        let b = win_ansi(c);
        if matches!(b, b'(' | b')' | b'\\') {
            out.push(b'\\');
        }
        out.push(b);
    }
    out.push(b')');
    out
}

/// The content stream of page `index`; the pictures it shows are added to
/// `images` and drawn as `/Im<n>` by their place there.
fn page_content(layout: &Layout, index: usize, images: &mut Vec<std::sync::Arc<crate::typeset::Image>>) -> Vec<u8> {
    let page = &layout.pages[index];
    let fs = layout.font_size;
    let lh = LINE_H * fs;
    let cw = CHAR_W * fs;
    // Opaque white paper, so rasterized previews are not transparent.
    let mut out: Vec<u8> = format!("1 g 0 0 {:.2} {:.2} re f\n", layout.width, layout.height).into_bytes();
    for (i, line) in page.iter().enumerate() {
        let top = layout.height - MARGIN - i as f32 * lh;
        let baseline = top - fs;
        if line.kind == LineKind::Header {
            // Shaded band with a rule underneath.
            let w = line.width as f32 * cw;
            out.extend(format!("0.9 g {:.2} {:.2} {:.2} {:.2} re f\n", MARGIN, top - lh, w, lh).bytes());
            out.extend(format!("0.4 G 0.5 w {:.2} {:.2} m {:.2} {:.2} l S\n", MARGIN, top - lh, MARGIN + w, top - lh).bytes());
        }
        for span in line.spans.iter().filter(|s| !s.style.stand_in) {
            if let Some(bg) = span.style.bg {
                rect_op(&mut out, bg, MARGIN + span.col as f32 * cw, top - lh, span.text.width() as f32 * cw, lh);
            }
            if span.style.shade {
                let x = MARGIN + span.col as f32 * cw;
                out.extend(format!("0.9 g {:.2} {:.2} {:.2} {:.2} re f\n", x, top - lh, span.text.width() as f32 * cw, lh).bytes());
            }
            let font = match (span.style.bold, span.style.italic) {
                (false, false) => "F1",
                (true, false) => "F2",
                (false, true) => "F3",
                (true, true) => "F4",
            };
            let (r, g, b) = span.style.color.unwrap_or((0, 0, 0));
            out.extend(
                format!(
                    "BT /{font} {fs:.2} Tf {:.3} {:.3} {:.3} rg {:.2} {:.2} Td ",
                    r as f32 / 255.0,
                    g as f32 / 255.0,
                    b as f32 / 255.0,
                    MARGIN + span.col as f32 * cw,
                    baseline
                )
                .bytes(),
            );
            out.extend(pdf_string(&span.text));
            out.extend(b" Tj ET\n");
        }
    }
    for g in &layout.graphics[index] {
        chart_ops(&mut out, layout, g);
    }
    for p in layout.pictures.get(index).into_iter().flatten() {
        // Centred from top to bottom in the lines it takes.
        let n = picture_lines(p.height, fs) as f32;
        let top = layout.height - MARGIN - p.line as f32 * lh;
        let y = top - n * lh + (n * lh - p.height) / 2.0;
        let x = MARGIN + p.col as f32 * cw;
        out.extend(format!("q {:.3} 0 0 {:.3} {x:.2} {y:.2} cm /Im{} Do Q\n", p.width, p.height, images.len()).bytes());
        images.push(p.image.clone());
    }
    out
}

/// A picture as a PDF image object: RGB on white paper, deflated.
fn image_object(img: &crate::typeset::Image) -> Vec<u8> {
    use std::io::Write;
    let raw: Vec<u8> = img.over([255, 255, 255]).into_iter().flatten().collect();
    let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    let _ = z.write_all(&raw);
    let data = z.finish().unwrap_or_default();
    let mut out = format!(
        "<< /Type /XObject /Subtype /Image /Width {} /Height {} /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /FlateDecode /Length {} >>\nstream\n",
        img.width,
        img.height,
        data.len()
    )
    .into_bytes();
    out.extend(data);
    out.extend(b"\nendstream");
    out
}

// ---------------------------------------------------------------- charts in PDF

/// Series colors on paper (same as the terminal preview).
const SERIES: [(u8, u8, u8); 6] = [(0, 90, 170), (200, 110, 0), (150, 0, 150), (0, 130, 60), (60, 60, 200), (190, 0, 0)];

fn rgb(c: (u8, u8, u8)) -> String {
    format!("{:.3} {:.3} {:.3}", c.0 as f32 / 255.0, c.1 as f32 / 255.0, c.2 as f32 / 255.0)
}

fn text_op(out: &mut Vec<u8>, x: f32, y: f32, fs: f32, color: (u8, u8, u8), bold: bool, text: &str) {
    let font = if bold { "F2" } else { "F1" };
    out.extend(format!("BT /{font} {fs:.2} Tf {} rg {x:.2} {y:.2} Td ", rgb(color)).bytes());
    out.extend(pdf_string(text));
    out.extend(b" Tj ET\n");
}

fn rect_op(out: &mut Vec<u8>, color: (u8, u8, u8), x: f32, y: f32, w: f32, h: f32) {
    out.extend(format!("{} rg {x:.2} {y:.2} {w:.2} {h:.2} re f\n", rgb(color)).bytes());
}

fn path_op(out: &mut Vec<u8>, color: (u8, u8, u8), width: f32, pts: &[(f32, f32)], fill: bool) {
    let Some(((x0, y0), rest)) = pts.split_first() else { return };
    let mut s = format!("{x0:.2} {y0:.2} m ");
    for (x, y) in rest {
        s.push_str(&format!("{x:.2} {y:.2} l "));
    }
    if fill {
        s.push_str(&format!("h {} rg f\n", rgb(color)));
    } else {
        s.push_str(&format!("{} RG {width:.2} w S\n", rgb(color)));
    }
    out.extend(s.bytes());
}

/// A lighter tint of `c` for filled areas.
fn tint(c: (u8, u8, u8)) -> (u8, u8, u8) {
    let t = |v: u8| (v as f32 + (255.0 - v as f32) * 0.7) as u8;
    (t(c.0), t(c.1), t(c.2))
}

/// Draws a chart into its box. The geometry follows the terminal preview
/// (same character grid), drawn as vectors.
fn chart_ops(out: &mut Vec<u8>, layout: &Layout, g: &Graphic) {
    let fs = layout.font_size;
    let cw = CHAR_W * fs;
    let lh = LINE_H * fs;
    let d = &g.data;
    let cols = g.width;
    let left = MARGIN + g.col as f32 * cw;
    let top = layout.height - MARGIN - g.line as f32 * lh;
    let line_y = |l: usize| top - l as f32 * lh;
    let baseline = |l: usize| line_y(l) - fs;
    let gray = (110, 110, 110);

    text_op(out, left, baseline(0), fs, (0, 0, 0), true, &format!("{} ({})", d.title, d.kind.name()));
    if d.series.len() > 1 {
        let mut x = left + cols as f32 * cw;
        for (i, s) in d.series.iter().enumerate().rev() {
            let item = format!("{} ", s.name);
            x -= (item.width() + 2) as f32 * cw;
            rect_op(out, SERIES[i % SERIES.len()], x, baseline(0), cw, fs * 0.7);
            text_op(out, x + 1.5 * cw, baseline(0), fs, (0, 0, 0), false, &item);
        }
    }
    if d.categories.is_empty() {
        return;
    }
    let (lo, hi) = d.value_range();
    match d.kind {
        ChartType::Bar | ChartType::Histogram => {
            let n_series = d.series.len();
            let label_w = d.categories.iter().map(|c| c.width()).max().unwrap_or(0).min(cols / 3);
            let value_w = d.series.iter().flat_map(|s| s.values.iter().flatten()).map(|v| d.fmt(*v, false).width()).max().unwrap_or(1);
            let bar_w = cols.saturating_sub(label_w + value_w + 3).max(1) as f32;
            let zero = (-lo) as f32 / (hi - lo) as f32 * bar_w;
            let per_category = n_series + usize::from(n_series > 1);
            for (i, cat) in d.categories.iter().enumerate() {
                let l0 = 1 + i * per_category;
                if l0 + n_series > g.height {
                    break;
                }
                text_op(out, left, baseline(l0), fs, (0, 0, 0), false, &fit(cat, label_w, Align::Left));
                for (j, s) in d.series.iter().enumerate() {
                    let Some(v) = s.values.get(i).copied().flatten() else { continue };
                    let len = (v.abs() / (hi - lo)) as f32 * bar_w;
                    let x0 = left + (label_w + 1) as f32 * cw + if v >= 0.0 { zero } else { zero - len } * cw;
                    let y = line_y(l0 + j) - lh * 0.85;
                    rect_op(out, SERIES[j % SERIES.len()], x0, y, len * cw, lh * 0.7);
                    let tx = x0 + len * cw + cw;
                    text_op(out, tx, baseline(l0 + j), fs, (0, 0, 0), false, &d.fmt(v, false));
                }
            }
            let used = d.categories.len() * per_category + 1;
            if !d.unit.is_none() && used < g.height {
                text_op(out, left + (label_w + 1) as f32 * cw, baseline(used), fs, gray, false, &format!("values in {}", d.unit));
            }
        }
        ChartType::Line | ChartType::Area | ChartType::Scatter => {
            let labels = [d.fmt(lo, false), d.fmt((lo + hi) / 2.0, false), d.fmt(hi, true)];
            let label_w = labels.iter().map(|l| l.width()).max().unwrap_or(1) + 1;
            let x0 = left + label_w as f32 * cw;
            let x1 = left + cols as f32 * cw;
            let y_top = line_y(1) - lh * 0.5;
            let y_bot = line_y(g.height - 2);
            let (x_lo, x_hi) = match &d.xs {
                Some(xs) => {
                    let f: Vec<f64> = xs.iter().copied().filter(|x| x.is_finite()).collect();
                    let lo = f.iter().copied().fold(f64::INFINITY, f64::min);
                    let hi = f.iter().copied().fold(f64::NEG_INFINITY, f64::max);
                    if lo < hi { (lo, hi) } else { (lo - 1.0, lo + 1.0) }
                }
                None => (0.0, (d.categories.len().max(2) - 1) as f64),
            };
            let px = |x: f64| x0 + ((x - x_lo) / (x_hi - x_lo)) as f32 * (x1 - x0);
            let py = |y: f64| y_bot + ((y - lo) / (hi - lo)) as f32 * (y_top - y_bot);
            path_op(out, gray, 0.6, &[(x0, y_top), (x0, y_bot), (x1, y_bot)], false);
            for (k, l) in labels.iter().enumerate() {
                let y = [lo, (lo + hi) / 2.0, hi][k];
                text_op(out, left, py(y) - fs * 0.35, fs, gray, false, l);
            }
            let n = d.categories.len();
            let x_labels: Vec<(f64, String)> = match &d.xs {
                Some(_) => vec![(x_lo, d.fmt(x_lo, false)), (x_hi, d.fmt(x_hi, false))],
                None => [0, n / 2, n - 1].into_iter().map(|i| (i as f64, d.categories[i].clone())).collect(),
            };
            for (x, l) in x_labels {
                let w = l.width() as f32 * cw;
                let tx = (px(x) - w / 2.0).clamp(x0, x1 - w);
                text_op(out, tx, baseline(g.height - 1), fs, gray, false, &l);
            }
            for (j, s) in d.series.iter().enumerate() {
                let color = SERIES[j % SERIES.len()];
                let pts: Vec<(f32, f32)> = s
                    .values
                    .iter()
                    .enumerate()
                    .filter_map(|(i, v)| {
                        let x = match &d.xs {
                            Some(xs) => xs.get(i).copied().filter(|x| x.is_finite())?,
                            None => i as f64,
                        };
                        v.map(|y| (px(x), py(y)))
                    })
                    .collect();
                match d.kind {
                    ChartType::Scatter => {
                        for (x, y) in &pts {
                            rect_op(out, color, x - 1.5, y - 1.5, 3.0, 3.0);
                        }
                    }
                    ChartType::Area if pts.len() > 1 => {
                        let base = py(lo.max(0.0).min(hi));
                        let mut poly = pts.clone();
                        poly.push((pts[pts.len() - 1].0, base));
                        poly.push((pts[0].0, base));
                        path_op(out, tint(color), 0.0, &poly, true);
                        path_op(out, color, 1.2, &pts, false);
                    }
                    _ if pts.len() == 1 => rect_op(out, color, pts[0].0 - 1.5, pts[0].1 - 1.5, 3.0, 3.0),
                    _ => path_op(out, color, 1.2, &pts, false),
                }
            }
        }
    }
}

/// Serializes the layout as a PDF document.
pub fn to_pdf(layout: &Layout, title: &str) -> Vec<u8> {
    let pages = (0..layout.pages.len())
        .map(|i| {
            let mut images = Vec::new();
            let content = page_content(layout, i, &mut images);
            (layout.width, layout.height, content, images)
        })
        .collect();
    assemble(pages, title)
}

/// Gap between the pages of an overview sheet, in points.
const SHEET_GAP: f32 = 24.0;

/// Size of the overview sheet for `across` × `down` pages.
pub fn sheet_size(layout: &Layout, across: usize, down: usize) -> (f32, f32) {
    (
        SHEET_GAP + across as f32 * (layout.width + SHEET_GAP),
        SHEET_GAP + down as f32 * (layout.height + SHEET_GAP),
    )
}

/// One PDF page showing the pages from `first` on, `across` × `down` of them
/// side by side on a dark ground – an overview, as the print preview zooms out.
pub fn to_pdf_sheet(layout: &Layout, first: usize, across: usize, down: usize, title: &str) -> Vec<u8> {
    let (w, h) = sheet_size(layout, across, down);
    let mut out: Vec<u8> = format!("0.35 g 0 0 {w:.2} {h:.2} re f\n").into_bytes();
    let mut images = Vec::new();
    for k in 0..across * down {
        let index = first + k;
        if index >= layout.pages.len() {
            break;
        }
        let x = SHEET_GAP + (k % across) as f32 * (layout.width + SHEET_GAP);
        let y = h - (SHEET_GAP + (k / across + 1) as f32 * (layout.height + SHEET_GAP) - SHEET_GAP);
        out.extend(format!("q 1 0 0 1 {x:.2} {y:.2} cm\n").bytes());
        out.extend(page_content(layout, index, &mut images));
        out.extend(b"Q\n");
    }
    assemble(vec![(w, h, out, images)], title)
}

/// A sheet for [`assemble`]: width, height, content stream and the pictures it draws.
type Sheet = (f32, f32, Vec<u8>, Vec<std::sync::Arc<crate::typeset::Image>>);

/// A PDF file from pages.
fn assemble(sheets: Vec<Sheet>, title: &str) -> Vec<u8> {
    let mut objects: Vec<Vec<u8>> = Vec::new();
    // 1 catalog, 2 pages, 3–6 fonts, 7 info, then a page and its content per
    // page, then the pictures.
    let n = sheets.len();
    let page_ids: Vec<usize> = (0..n).map(|i| 8 + 2 * i).collect();
    let mut next_image = 8 + 2 * n;
    let mut image_objects: Vec<Vec<u8>> = Vec::new();
    objects.push(b"<< /Type /Catalog /Pages 2 0 R >>".to_vec());
    let kids: Vec<String> = page_ids.iter().map(|id| format!("{id} 0 R")).collect();
    objects.push(format!("<< /Type /Pages /Kids [{}] /Count {n} >>", kids.join(" ")).into_bytes());
    for font in ["Courier", "Courier-Bold", "Courier-Oblique", "Courier-BoldOblique"] {
        objects.push(
            format!("<< /Type /Font /Subtype /Type1 /BaseFont /{font} /Encoding /WinAnsiEncoding >>").into_bytes(),
        );
    }
    let mut info = b"<< /Producer (hardcalc) /Title ".to_vec();
    info.extend(pdf_string(title));
    info.extend(b" >>");
    objects.push(info);
    for (&page_id, (width, height, content, images)) in page_ids.iter().zip(sheets) {
        let mut xobjects = String::new();
        for (k, img) in images.iter().enumerate() {
            xobjects.push_str(&format!("/Im{k} {next_image} 0 R "));
            next_image += 1;
            image_objects.push(image_object(img));
        }
        let xobjects = if xobjects.is_empty() { String::new() } else { format!(" /XObject << {xobjects}>>") };
        objects.push(
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {width:.2} {height:.2}] \
                 /Resources << /Font << /F1 3 0 R /F2 4 0 R /F3 5 0 R /F4 6 0 R >>{xobjects} >> /Contents {} 0 R >>",
                page_id + 1
            )
            .into_bytes(),
        );
        let mut stream = format!("<< /Length {} >>\nstream\n", content.len()).into_bytes();
        stream.extend(content);
        stream.extend(b"\nendstream");
        objects.push(stream);
    }
    objects.extend(image_objects);

    let mut pdf = b"%PDF-1.4\n%\xe2\xe3\xcf\xd3\n".to_vec();
    let mut offsets = Vec::new();
    for (i, obj) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.extend(format!("{} 0 obj\n", i + 1).bytes());
        pdf.extend(obj);
        pdf.extend(b"\nendobj\n");
    }
    let xref = pdf.len();
    pdf.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).bytes());
    for off in offsets {
        pdf.extend(format!("{off:010} 00000 n \n").bytes());
    }
    pdf.extend(format!("trailer\n<< /Size {} /Root 1 0 R /Info 7 0 R >>\nstartxref\n{xref}\n%%EOF\n", objects.len() + 1).bytes());
    pdf
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(rows: usize, cols: usize) -> PrintTable {
        PrintTable {
            title: "T".into(),
            headers: (0..cols).map(|c| format!("Col{c}")).collect(),
            rows: (0..rows)
                .map(|r| {
                    (0..cols)
                        .map(|c| PrintCell { text: format!("r{r}c{c}"), align: Align::Left, style: SpanStyle::default() })
                        .collect()
                })
                .collect(),
            footnotes: Vec::new(),
            footer_rows: 0,
        }
    }

    #[test]
    fn long_tables_break_pages_and_repeat_headers() {
        let l = paginate("Doc", &[PrintItem::Table(table(200, 3))], &PageSetup::default(), "file");
        assert!(l.pages.len() > 1);
        let second = &l.pages[1];
        assert_eq!(second[0].plain(), "T (continued)");
        assert_eq!(second[1].kind, LineKind::Header);
        assert!(l.pages.iter().all(|p| p.len() == l.rows));
        let footer = l.pages[0].last().unwrap().plain();
        assert!(footer.starts_with("file") && footer.ends_with(&format!("page 1/{}", l.pages.len())), "{footer}");
        // Every row appears exactly once.
        let rows: usize = l.pages.iter().flatten().filter(|line| line.kind == LineKind::Row).count();
        assert_eq!(rows, 200);
    }

    #[test]
    fn wide_tables_shrink_then_split_into_bands() {
        let narrow = paginate("", &[PrintItem::Table(table(3, 3))], &PageSetup::default(), "");
        assert_eq!(narrow.font_size, MAX_FONT);
        let wide = paginate("", &[PrintItem::Table(table(3, 60))], &PageSetup::default(), "");
        assert_eq!(wide.font_size, MIN_FONT);
        let titles: Vec<String> =
            wide.pages[0].iter().filter(|l| l.kind == LineKind::Title).map(|l| l.plain()).collect();
        assert!(titles.len() >= 2 && titles[0].contains("(columns A–"), "{titles:?}");
        let landscape = paginate("", &[PrintItem::Table(table(3, 60))], &PageSetup { landscape: true, ..PageSetup::default() }, "");
        assert!(landscape.cols > wide.cols);
    }

    #[test]
    fn pdf_structure() {
        let l = paginate("Budget €", &[PrintItem::Table(table(120, 2))], &PageSetup::default(), "x");
        let pdf = to_pdf(&l, "Budget €");
        assert!(pdf.starts_with(b"%PDF-1.4"));
        assert!(pdf.ends_with(b"%%EOF\n"));
        let text = String::from_utf8_lossy(&pdf);
        assert!(text.contains(&format!("/Count {}", l.pages.len())));
        assert!(text.contains("/BaseFont /Courier-Bold"));
        // `€` is encoded as WinAnsi 0x80.
        assert!(pdf.windows(9).any(|w| w == b"(Budget \x80"));
        // xref offsets point at the objects.
        let xref = text.rfind("startxref\n").unwrap();
        let start: usize = text[xref + 10..].lines().next().unwrap().parse().unwrap();
        assert!(pdf[start..].starts_with(b"xref"));
    }

    #[test]
    fn overview_sheet_is_one_page_with_all_pages() {
        let layout = paginate("T", &[PrintItem::Table(table(120, 3))], &PageSetup::default(), "f");
        assert!(layout.pages.len() >= 3);
        let pdf = to_pdf_sheet(&layout, 0, 3, 1, "t");
        let text = String::from_utf8_lossy(&pdf);
        assert!(text.contains("/Count 1"));
        assert_eq!(text.matches(" cm\n").count(), 3);
        let (w, h) = sheet_size(&layout, 3, 1);
        assert!(w > 3.0 * layout.width && h > layout.height);
    }

    #[test]
    fn escapes_and_unknown_chars() {
        assert_eq!(pdf_string("a(b)\\ é ⚠"), b"(a\\(b\\)\\\\ \xe9 ?)".to_vec());
    }
}
