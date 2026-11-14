//! The "∑ calc" tab: the calculation sheet – the lines of all ```hardcalc
//! blocks – with the way to each result next to them.

use super::{App, CellView, ViewKind};
use crate::layers::{CellStyle, Color};
use crate::model::{Align, Block, Document};
use crate::sheet;
use crate::sheet::expr::{Line, Parts};

/// Columns of the calculation view: the label of a line, its input, the
/// way to the result and the comment – all but the calculation are edited.
pub const CALC_COLS: [&str; 4] = ["Name", "Input", "Calculation", "Comment"];
pub const NAME: usize = 0;
pub const INPUT: usize = 1;
pub const CALCULATION: usize = 2;
pub const COMMENT: usize = 3;

/// One row of the calculation view.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CalcRow {
    /// Line `line` of the ```hardcalc block at `block`.
    Line { block: usize, line: usize },
    /// Placeholder for a new line at the end.
    New,
}

/// ASCII input as typed in a terminal, written the readable way:
/// `*` → ·, `<=` → ≤, `>=` → ≥, `->` → →. Names stay as typed (also
/// `\gamma_M`, which the sheet reads as γ_M).
pub fn tidy_input(line: &str) -> String {
    // The label is text: `[beta check]` stays.
    let (label, line) = line.split_at(sheet::expr::label_prefix(line));
    let (code, comment) = match line.find('\'') {
        Some(i) => line.split_at(i),
        None => (line, ""),
    };
    let mut out = String::new();
    let mut word = String::new();
    let mut quoted = false;
    let flush = |word: &mut String, out: &mut String| {
        if !word.is_empty() {
            out.push_str(word);
            word.clear();
        }
    };
    let chars: Vec<char> = code.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '"' {
            quoted = !quoted;
        }
        if !quoted && (c.is_alphanumeric() || c == '_') {
            word.push(c);
            i += 1;
            continue;
        }
        flush(&mut word, &mut out);
        let next = chars.get(i + 1).copied();
        // Not inside a reference like Loads!C12 or "My sheet"!B2.
        match (c, next) {
            _ if quoted => out.push(c),
            ('<', Some('=')) => {
                out.push('≤');
                i += 1;
            }
            ('>', Some('=')) => {
                out.push('≥');
                i += 1;
            }
            ('-', Some('>')) => {
                out.push('→');
                i += 1;
            }
            ('*', _) => out.push('·'),
            _ => out.push(c),
        }
        i += 1;
    }
    flush(&mut word, &mut out);
    // Numbers are written with a decimal point.
    format!("{label}{}", sheet::expr::point_decimals(&(out + comment)))
}

/// How formulas are shown in the preview (`:formulas`).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum FormulaStyle {
    /// Fractions, roots and indices drawn over several lines (term-maths).
    #[default]
    TwoD,
    /// One line per formula, with Unicode symbols.
    Text,
    /// One line per formula, plain ASCII.
    Ascii,
}

impl FormulaStyle {
    pub fn parse(s: &str) -> Option<FormulaStyle> {
        match s {
            "2d" => Some(FormulaStyle::TwoD),
            "text" => Some(FormulaStyle::Text),
            "ascii" => Some(FormulaStyle::Ascii),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            FormulaStyle::TwoD => "2d",
            FormulaStyle::Text => "text",
            FormulaStyle::Ascii => "ascii",
        }
    }
}

/// Kind of a preview line, for its colour.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PreviewKind {
    Heading,
    Comment,
    Math,
    /// A check that holds / fails.
    Ok,
    Fail,
    Error,
}

/// The sheet typeset: one or more screen lines per evaluated line.
pub fn preview_lines(out: &[sheet::Out], style: FormulaStyle) -> Vec<(String, PreviewKind)> {
    use sheet::Out;
    let mut lines = Vec::new();
    let render = match style {
        FormulaStyle::Ascii => sheet::Render { style: sheet::expr::TextStyle { ascii: true, point: true } },
        _ => sheet::Render::default(),
    };
    for o in out {
        let kind = match o {
            Out::Check { rel, sides: Ok((l, r)), .. } => {
                if sheet::verdict(*rel, l, r).ok { PreviewKind::Ok } else { PreviewKind::Fail }
            }
            _ => PreviewKind::Math,
        };
        match o {
            Out::Blank => lines.push((String::new(), PreviewKind::Comment)),
            Out::Heading { text, .. } => {
                lines.push((text.clone(), PreviewKind::Heading));
            }
            Out::Comment(c) => lines.push((c.clone(), PreviewKind::Comment)),
            Out::Error(_) => lines.push((render.line(o), PreviewKind::Error)),
            _ => {
                let comment = o.comment();
                let latex = (style == FormulaStyle::TwoD).then(|| sheet::notation::line(sheet::notation::Lang::Latex, o)).flatten();
                match latex {
                    Some(latex) => {
                        // The label above the formula drawn over several lines.
                        if let Some(l) = o.label() {
                            lines.push((format!("[{l}]"), PreviewKind::Comment));
                        }
                        let block = term_maths::render(&latex).to_string();
                        let rows: Vec<&str> = block.lines().collect();
                        let n = rows.len();
                        for (i, row) in rows.into_iter().enumerate() {
                            // The comment on the last line of the formula.
                            let row = match (comment, i + 1 == n) {
                                (Some(c), true) => format!("{}   {c}", row.trim_end()),
                                _ => row.trim_end().to_string(),
                            };
                            lines.push((row, kind));
                        }
                        lines.push((String::new(), PreviewKind::Comment));
                    }
                    // Text, ASCII, or a line that could not be calculated.
                    None => {
                        let failed = !sheet::problems(std::slice::from_ref(o)).is_empty();
                        lines.push((render.line(o), if failed { PreviewKind::Error } else { kind }));
                    }
                }
            }
        }
    }
    lines
}

/// Appends a line to the last ```hardcalc block, creating one at the end.
pub(super) fn add_calc_line(doc: &mut Document, line: String) {
    let last = doc.blocks.iter_mut().rev().find_map(|b| match b {
        Block::Code { info, body } if sheet::is_sheet_info(info) => Some(body),
        _ => None,
    });
    match last {
        Some(body) => body.push(line),
        None => doc.blocks.push(Block::Code { info: sheet::INFO.into(), body: vec![line] }),
    }
}

impl App {
    pub fn calc_view(&self) -> bool {
        self.view.kind == ViewKind::Calc
    }

    /// Whether the calculation tab is shown: the document has a sheet, or it is open.
    pub fn has_calc(&self) -> bool {
        self.calc_view() || self.doc.blocks.iter().any(|b| matches!(b, Block::Code { info, .. } if sheet::is_sheet_info(info)))
    }

    pub fn calc_rows(&self) -> Vec<CalcRow> {
        let mut rows = Vec::new();
        for (b, block) in self.doc.blocks.iter().enumerate() {
            if let Block::Code { info, body } = block
                && sheet::is_sheet_info(info)
            {
                rows.extend((0..body.len()).map(|line| CalcRow::Line { block: b, line }));
            }
        }
        rows.push(CalcRow::New);
        rows
    }

    fn calc_source(&self, row: CalcRow) -> Option<&String> {
        match row {
            CalcRow::Line { block, line } => match &self.doc.blocks[block] {
                Block::Code { body, .. } => body.get(line),
                _ => None,
            },
            CalcRow::New => None,
        }
    }

    /// The evaluated sheet, one entry per line (cached until the document changes).
    pub fn calc_results(&mut self) -> &[sheet::Out] {
        if self.calc_cache.is_none() {
            let out = sheet::evaluate_document(&self.doc, &mut self.wb);
            self.calc_cache = Some(out);
        }
        self.calc_cache.as_deref().unwrap_or(&[])
    }

    /// Writes the calculation report (PDF with typeset formulas) next to the file.
    pub(super) fn write_report(&mut self, out: Option<&std::path::Path>) {
        if !self.doc.blocks.iter().any(|b| matches!(b, Block::Code { info, .. } if sheet::is_sheet_info(info))) {
            return self.error("no calculation sheet – :calc starts one");
        }
        let out = match out {
            Some(o) => o.to_path_buf(),
            None => {
                let name = self.path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                let stem = crate::storage::document_stem(&name).to_string();
                self.path.with_file_name(format!("{stem}.report.pdf"))
            }
        };
        let doc = self.doc.clone();
        let source = crate::report::typst_source(&doc, &mut self.wb, &crate::report::title(&doc, &self.path));
        let result = if out.extension().is_some_and(|e| e == "typ") {
            std::fs::write(&out, source).map_err(|e| format!("{}: {e}", out.display()))
        } else {
            crate::report::compile(&source, &out)
        };
        match result {
            Ok(()) => self.info(format!("report written: {}", out.display())),
            Err(e) => self.error(e),
        }
    }

    /// The sheet typeset for the preview window.
    pub fn calc_preview(&mut self) -> Vec<(String, PreviewKind)> {
        let style = self.formula_style;
        preview_lines(self.calc_results(), style)
    }

    /// Index of row `r` among the evaluated lines (`#result` … lines have none).
    fn calc_index(&self, r: usize) -> Option<usize> {
        let rows = self.calc_rows();
        let is_mode = |row: &CalcRow| self.calc_source(*row).is_some_and(|s| matches!(sheet::expr::parse_line(s), sheet::expr::Line::Mode(_)));
        let row = rows.get(r)?;
        if self.calc_source(*row).is_none() || is_mode(row) {
            return None;
        }
        Some(r - rows[..r].iter().filter(|row| is_mode(row)).count())
    }

    /// The evaluated line for row `r`, if it has one.
    fn calc_out(&mut self, r: usize) -> Option<sheet::Out> {
        let i = self.calc_index(r)?;
        self.calc_results().get(i).cloned()
    }

    pub(super) fn calc_cell_view(&mut self, r: usize, c: usize) -> CellView {
        let mut style = CellStyle::default();
        let mut error = false;
        let row = self.calc_rows().get(r).copied();
        let parts = row.and_then(|row| self.calc_source(row)).map(|s| Parts::of(s));
        let text = match (row, c) {
            (Some(CalcRow::Line { .. }), NAME) => parts.and_then(|p| p.label).unwrap_or_default(),
            (Some(CalcRow::Line { .. }), INPUT) => parts.map(|p| p.code).unwrap_or_default(),
            (Some(CalcRow::Line { .. }), COMMENT) => {
                style.fg = Some(Color::Named("darkgray".into()));
                parts.and_then(|p| p.comment).unwrap_or_default()
            }
            (Some(CalcRow::Line { .. }), _) => match self.calc_out(r) {
                Some(out) => {
                    match &out {
                        // Headings and comment lines without their `##` and `'`.
                        sheet::Out::Heading { level, text } => {
                            style.bold = true;
                            style.underline = *level <= 2;
                            return CellView { text: text.clone(), align: Align::Left, style, error, violation: false };
                        }
                        sheet::Out::Comment(c) => {
                            style.fg = Some(Color::Named("darkgray".into()));
                            style.italic = true;
                            return CellView { text: c.clone(), align: Align::Left, style, error, violation: false };
                        }
                        _ if !sheet::problems(std::slice::from_ref(&out)).is_empty() => error = true,
                        sheet::Out::Def { .. } | sheet::Out::Plot { .. } => style.fg = Some(Color::Named("cyan".into())),
                        sheet::Out::Check { rel, sides: Ok((l, r)), .. } => {
                            let ok = sheet::verdict(*rel, l, r).ok;
                            style.fg = Some(Color::Named(if ok { "green" } else { "red" }.into()));
                            style.bold = true;
                        }
                        _ => {}
                    }
                    sheet::Render::default().body(&out)
                }
                None => String::new(),
            },
            _ => String::new(),
        };
        CellView { text, align: Align::Left, style, error, violation: false }
    }

    /// The calculation of row `r` as Typst math, for lines that are
    /// calculated without problems (not headings, comments, plots or errors).
    pub fn calc_math(&mut self, r: usize) -> Option<String> {
        let out = self.calc_out(r)?;
        if !sheet::problems(std::slice::from_ref(&out)).is_empty() {
            return None;
        }
        match out {
            sheet::Out::Assign { .. } | sheet::Out::Show { .. } | sheet::Out::Def { .. } | sheet::Out::Check { .. } => {
                sheet::notation::line(sheet::notation::Lang::Typst, &out)
            }
            _ => None,
        }
    }

    /// Typesets the formulas not set yet for cells `cell_h` pixels high, all in one run.
    pub fn typeset_formulas(&mut self, math: &[String], cell_h: u16) {
        let mut missing: Vec<String> = math.iter().filter(|m| !self.typeset_cache.contains_key(&((*m).clone(), cell_h))).cloned().collect();
        missing.dedup();
        if missing.is_empty() {
            return;
        }
        // An em of about seven tenths of a line: plain formulas fit one line,
        // fractions (display style, as in the report) two.
        let style = crate::typeset::Style { size: 10.0, scale: 0.07 * cell_h as f32, color: [255, 255, 255], display: true };
        for (m, img) in missing.iter().zip(crate::typeset::formulas(&missing, style)) {
            self.typeset_cache.insert((m.clone(), cell_h), img.map(std::rc::Rc::new));
        }
    }

    /// Formula `math` (set before with [`App::typeset_formulas`]) fitted into
    /// `width` pixels and coloured `ink`, with the lines it takes; `None`
    /// where it is not set or would be too small to read.
    pub fn typeset_fitted(&mut self, math: &str, cell_h: u16, width: usize, ink: [u8; 3]) -> Option<super::Fitted> {
        let key = (math.to_string(), cell_h, width, ink);
        if let Some(hit) = self.typeset_fit.get(&key) {
            return hit.clone();
        }
        let fitted = self.typeset_cache.get(&(math.to_string(), cell_h)).cloned().flatten().and_then(|img| {
            let ch = cell_h.max(1) as f64;
            let mut f = (width as f64 / img.width as f64).min(1.0);
            // Shrunk to less than two thirds the formula is hard to read: text instead.
            if f < 0.66 {
                return None;
            }
            // A little taller than whole lines: shrunk to fit them.
            let lines = ((img.height as f64 * f / ch) - 0.2).ceil().clamp(1.0, 6.0);
            f = f.min(lines * ch / img.height as f64);
            let mut out = img.shrink(f);
            for p in &mut out.pixels {
                p[0] = ink[0];
                p[1] = ink[1];
                p[2] = ink[2];
            }
            Some((std::rc::Rc::new(out), lines as u16))
        });
        // The cache holds what is on screen and a little more.
        if self.typeset_fit.len() > 2000 {
            self.typeset_fit.clear();
        }
        self.typeset_fit.insert(key, fitted.clone());
        fitted
    }

    /// Editable text of the cell at the cursor.
    pub(super) fn calc_raw(&self) -> String {
        let (r, c) = self.view.cursor;
        let row = self.calc_rows().get(r).copied();
        let parts = || row.and_then(|row| self.calc_source(row)).map(|s| Parts::of(s)).unwrap_or_default();
        match (row, c) {
            (Some(CalcRow::Line { .. }), NAME) => parts().label.unwrap_or_default(),
            (Some(CalcRow::Line { .. }), INPUT) => parts().code,
            (Some(CalcRow::Line { .. }), COMMENT) => parts().comment.unwrap_or_default(),
            // The grid was drawn before, so the sheet is evaluated.
            (Some(CalcRow::Line { .. }), _) => self
                .calc_index(r)
                .and_then(|i| self.calc_cache.as_ref()?.get(i))
                .map(|o| sheet::Render::default().body(o))
                .unwrap_or_default(),
            _ => String::new(),
        }
    }

    pub(super) fn calc_problem(&mut self) -> Option<String> {
        let out = self.calc_out(self.view.cursor.0)?;
        sheet::problems(std::slice::from_ref(&out)).into_iter().next()
    }

    /// Why the cell at the cursor cannot be edited, if it cannot.
    pub(super) fn calc_edit_problem(&self) -> Option<&'static str> {
        let (r, c) = self.view.cursor;
        let line = self.calc_rows().get(r).and_then(|row| self.calc_source(*row)).map(|s| sheet::expr::parse_line(s));
        match (c, line) {
            (CALCULATION, _) => Some("the calculation is computed – edit the input"),
            (NAME | COMMENT, Some(Line::Heading { .. } | Line::Mode(_))) => Some("a heading or a #mode line has no name and no comment"),
            (COMMENT, Some(Line::Stmt { stmt: None, .. })) => Some("the line is a comment – edit it under Input"),
            _ => None,
        }
    }

    pub(super) fn commit_calc(&mut self, text: String) {
        let (r, c) = self.view.cursor;
        let row = self.calc_rows().get(r).copied();
        let old = row.and_then(|row| self.calc_source(row)).map(|s| Parts::of(s)).unwrap_or_default();
        let text = text.trim().to_string();
        let parts = match c {
            NAME => Parts { label: Some(text), ..old },
            COMMENT => Parts { comment: Some(text), ..old },
            _ => {
                // Typed with a label or a comment, those win.
                let typed = Parts::of(&tidy_input(&text));
                match sheet::expr::parse_line(&typed.code) {
                    // Headings and #mode lines stand alone.
                    Line::Heading { .. } | Line::Mode(_) if typed.label.is_none() => typed,
                    // A comment line drops the comment it had beside.
                    Line::Stmt { stmt: None, .. } => Parts { label: typed.label.or(old.label), ..typed },
                    _ if typed.comment.is_some() => Parts { label: typed.label.or(old.label), ..typed },
                    _ => Parts { label: typed.label.or(old.label), code: typed.code, ..old },
                }
            }
        };
        let line = parts.line();
        let code = Parts::of(&line).code;
        match row {
            Some(CalcRow::Line { block, line: i }) => self.with_doc(|doc| {
                if let Block::Code { body, .. } = &mut doc.blocks[block] {
                    body[i] = line;
                }
            }),
            Some(CalcRow::New) if !line.trim().is_empty() => self.with_doc(|doc| add_calc_line(doc, line)),
            _ => return,
        }
        if let Some(p) = self.calc_problem() {
            self.error(p);
        } else if c == INPUT
            && let Some((word, g)) = sheet::expr::spelled_greek(&code)
        {
            // No longer turned into the letter: a hint, in case it was meant.
            let head = word.split('_').next().unwrap_or("");
            let tail = &word[head.len()..];
            self.info(format!("{word} is a name of its own – for {g}{tail} write \\{head}{tail} or press Ctrl+G"));
        }
    }

    /// Inserts an empty line below the cursor (at the end on the + row) and edits it.
    pub(super) fn insert_calc_line(&mut self) {
        let r = self.view.cursor.0;
        match self.calc_rows().get(r).copied() {
            Some(CalcRow::Line { block, line }) => {
                self.with_doc(|doc| {
                    if let Block::Code { body, .. } = &mut doc.blocks[block] {
                        body.insert(line + 1, String::new());
                    }
                });
                self.view.cursor = (r + 1, INPUT);
            }
            _ => self.view.cursor = (r, INPUT),
        }
        self.start_edit(None);
    }

    /// Del: in Name or Comment it clears them, elsewhere it deletes the lines.
    pub(super) fn clear_calc(&mut self) {
        if !self.has_range() && matches!(self.view.cursor.1, NAME | COMMENT) {
            return match self.calc_edit_problem() {
                Some(p) => self.error(p),
                None => self.commit_calc(String::new()),
            };
        }
        self.delete_calc_lines()
    }

    pub(super) fn delete_calc_lines(&mut self) {
        let ((r0, _), (r1, _)) = self.selection();
        let rows = self.calc_rows();
        let mut doomed: Vec<(usize, usize)> = (r0..=r1)
            .filter_map(|r| match rows.get(r) {
                Some(CalcRow::Line { block, line }) => Some((*block, *line)),
                _ => None,
            })
            .collect();
        if doomed.is_empty() {
            return self.error("no line selected");
        }
        doomed.sort_unstable_by(|a, b| b.cmp(a));
        let n = doomed.len();
        self.with_doc(|doc| {
            for (block, line) in doomed {
                if let Block::Code { body, .. } = &mut doc.blocks[block] {
                    body.remove(line);
                }
            }
            // An emptied block goes away.
            doc.blocks.retain(|b| !matches!(b, Block::Code { info, body } if sheet::is_sheet_info(info) && body.is_empty()));
        });
        self.info(format!("deleted {n} line{}", if n == 1 { "" } else { "s" }));
    }
}

#[cfg(test)]
mod tests {
    use super::tidy_input;

    #[test]
    fn ascii_input_is_tidied() {
        assert_eq!(tidy_input("q_d = 1,35*g_k + 1.5*q_k"), "q_d = 1.35·g_k + 1.5·q_k");
        assert_eq!(tidy_input("n_Ed <= f_d / gamma_M"), "n_Ed ≤ f_d / gamma_M", "names stay as typed");
        assert_eq!(tidy_input("n_Ed <= f_d / \\gamma_M"), "n_Ed ≤ f_d / \\gamma_M");
        assert_eq!(tidy_input("s = N/A -> N/mm^2 ' x*y"), "s = N/A → N/mm^2 ' x*y");
        assert_eq!(tidy_input("a = \"My*sheet\"!B2"), "a = \"My*sheet\"!B2");
        assert_eq!(tidy_input("[beta check, 1,5] b = 1,5*beta"), "[beta check, 1,5] b = 1.5·beta");
    }
}
