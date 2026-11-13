//! Dialogs of the calculation sheet: the settings of a chart drawn in it
//! (`plot(…)`, F7) and the formula library (F6).

use super::calc_view::INPUT;
use super::{App, Block, CalcRow, ViewKind};
use crate::sheet::expr::{self, Line, Parts, Stmt, TextStyle};
use crate::sheet::library::{self, Entry};
use ratatui::crossterm::event::{KeyCode, KeyEvent};

/// The chart types a plot can have.
pub const PLOT_TYPES: [&str; 4] = ["line", "area", "bar", "scatter"];

/// The settings of a plot line, while they are edited.
#[derive(Debug, Clone, PartialEq)]
pub struct PlotDialog {
    /// The calculation row of the plot line; `None`: a new line after `after`.
    pub row: Option<usize>,
    pub after: usize,
    /// Name, curves, x, type, title.
    pub fields: [String; 5],
    pub selected: usize,
    pub error: Option<String>,
}

impl PlotDialog {
    pub const LABELS: [&'static str; 5] = ["Name", "Curves", "x", "Type", "Title"];
    pub const TYPE: usize = 3;

    fn kind_step(&mut self, d: isize) {
        let k = PLOT_TYPES.iter().position(|t| *t == self.fields[Self::TYPE]).unwrap_or(0) as isize;
        self.fields[Self::TYPE] = PLOT_TYPES[(k + d).rem_euclid(PLOT_TYPES.len() as isize) as usize].to_string();
    }

    /// The line the settings make, keeping the comment of `old`.
    fn line(&self, old: &Parts) -> String {
        let ys: Vec<String> = self.fields[1].split(';').map(str::to_string).collect();
        let opts = [("type".to_string(), self.fields[3].clone()), ("title".to_string(), self.fields[4].clone())];
        let code = expr::plot_code(&ys, Some(&self.fields[2]), &opts);
        Parts { label: Some(self.fields[0].clone()), code, ..old.clone() }.line()
    }
}

/// The formula library, to pick a formula from.
#[derive(Debug, Clone, PartialEq)]
pub struct LibraryDialog {
    pub entries: Vec<Entry>,
    /// Typed text: only formulas containing it.
    pub query: String,
    /// Index into [`LibraryDialog::shown`].
    pub selected: usize,
    pub scroll: usize,
}

impl LibraryDialog {
    /// The formulas listed: all, or those whose group, name, formula or
    /// note contain the typed text.
    pub fn shown(&self) -> Vec<&Entry> {
        let q = self.query.to_lowercase();
        self.entries
            .iter()
            .filter(|e| q.is_empty() || [&e.group, &e.title, &e.line].iter().any(|t| t.to_lowercase().contains(&q)) || e.note.as_ref().is_some_and(|n| n.to_lowercase().contains(&q)))
            .collect()
    }
}

/// The variable a formula's result gets: the function name up to its first
/// `_` (`M_ss` → `M`).
fn result_name(f: &str) -> String {
    f.split('_').next().filter(|s| !s.is_empty()).unwrap_or(f).to_string()
}

impl App {
    /// The calculation row of plot `k` (in document order).
    fn plot_row(&self, k: usize) -> Option<usize> {
        let rows = self.calc_rows();
        (0..rows.len()).filter(|&r| self.calc_line(r).is_some_and(|l| crate::sheet::is_plot_line(&l))).nth(k)
    }

    /// The source of calculation row `r`.
    pub(super) fn calc_line(&self, r: usize) -> Option<String> {
        match self.calc_rows().get(r)? {
            CalcRow::Line { block, line } => match &self.doc.blocks[*block] {
                Block::Code { body, .. } => body.get(*line).cloned(),
                _ => None,
            },
            CalcRow::New => None,
        }
    }

    /// Replaces calculation row `r` by `text` (one undoable step).
    fn set_calc_line(&mut self, r: usize, text: String) {
        if let Some(CalcRow::Line { block, line }) = self.calc_rows().get(r).copied() {
            self.with_doc(|doc| {
                if let Block::Code { body, .. } = &mut doc.blocks[block] {
                    body[line] = text;
                }
            });
        }
    }

    /// Inserts `lines` after calculation row `r` (at the end on the + row),
    /// as one undoable step; returns the row of the first.
    fn insert_calc_lines(&mut self, r: usize, lines: Vec<String>) -> usize {
        match self.calc_rows().get(r).copied() {
            Some(CalcRow::Line { block, line }) => {
                self.with_doc(|doc| {
                    if let Block::Code { body, .. } = &mut doc.blocks[block] {
                        for (k, l) in lines.into_iter().enumerate() {
                            body.insert(line + 1 + k, l);
                        }
                    }
                });
                r + 1
            }
            _ => {
                let first = self.calc_rows().len() - 1;
                self.with_doc(|doc| {
                    for l in lines {
                        super::calc_view::add_calc_line(doc, l);
                    }
                });
                first
            }
        }
    }

    /// Sets option `key` of plot `k` (`type: area`); `line` and empty remove it.
    pub(super) fn set_plot_option(&mut self, k: usize, key: &str, value: &str) {
        let Some(r) = self.plot_row(k) else { return self.error("the chart no longer exists") };
        let Some(src) = self.calc_line(r) else { return };
        let parts = Parts::of(&src);
        let Line::Stmt { stmt: Some(Stmt::Plot { ys, x, mut opts }), .. } = expr::parse_line(&src) else { return };
        let style = TextStyle { ascii: false, point: true };
        let ys: Vec<String> = ys.iter().map(|y| expr::to_text(y, style, &|_| None)).collect();
        let x = x.map(|x| expr::to_text(&x, style, &|_| None));
        opts.retain(|(k, _)| k != key);
        opts.push((key.to_string(), value.to_string()));
        let code = expr::plot_code(&ys, x.as_deref(), &opts);
        self.set_calc_line(r, Parts { code, ..parts }.line());
    }

    /// F7 / `:plot` in the calculation sheet: the settings of the plot at
    /// the cursor, or a new plot of the variable there.
    pub(super) fn open_plot_dialog(&mut self) {
        if self.view.kind != ViewKind::Calc {
            return self.error(":plot works in the calculation sheet (∑ calc)");
        }
        let r = self.view.cursor.0;
        let src = self.calc_line(r).unwrap_or_default();
        let style = TextStyle { ascii: false, point: true };
        let dialog = match expr::parse_line(&src) {
            Line::Stmt { stmt: Some(Stmt::Plot { ys, x, opts }), label, .. } => {
                let ys: Vec<String> = ys.iter().map(|y| expr::to_text(y, style, &|_| None)).collect();
                let opt = |k: &str| opts.iter().find(|(o, _)| o == k).map(|(_, v)| v.clone()).unwrap_or_default();
                let kind = Some(opt("type")).filter(|t| !t.is_empty()).unwrap_or_else(|| "line".into());
                PlotDialog {
                    row: Some(r),
                    after: r,
                    fields: [label.unwrap_or_default(), ys.join("; "), x.map(|x| expr::to_text(&x, style, &|_| None)).unwrap_or_default(), kind, opt("title")],
                    selected: 0,
                    error: None,
                }
            }
            line => {
                // A new chart of the variable at the cursor.
                let curve = match line {
                    Line::Stmt { stmt: Some(Stmt::Assign { name, .. }), .. } => name,
                    _ => String::new(),
                };
                PlotDialog { row: None, after: r, fields: [String::new(), curve, String::new(), "line".into(), String::new()], selected: 0, error: None }
            }
        };
        self.plot_dialog = Some(dialog);
    }

    pub(super) fn key_plot_dialog(&mut self, key: KeyEvent) {
        let Some(d) = &mut self.plot_dialog else { return };
        let n = PlotDialog::LABELS.len();
        match key.code {
            KeyCode::Esc | KeyCode::F(10) => self.plot_dialog = None,
            KeyCode::Up | KeyCode::BackTab => d.selected = (d.selected + n - 1) % n,
            KeyCode::Down | KeyCode::Tab => d.selected = (d.selected + 1) % n,
            KeyCode::Left | KeyCode::Right if d.selected == PlotDialog::TYPE => d.kind_step(if key.code == KeyCode::Left { -1 } else { 1 }),
            KeyCode::Char(' ') if d.selected == PlotDialog::TYPE => d.kind_step(1),
            KeyCode::Backspace if d.selected != PlotDialog::TYPE => {
                d.fields[d.selected].pop();
            }
            KeyCode::Char(c) if d.selected != PlotDialog::TYPE => d.fields[d.selected].push(c),
            KeyCode::Enter | KeyCode::F(2) => self.apply_plot_dialog(),
            _ => {}
        }
    }

    /// Writes the plot line the dialog describes; it stays open on an error.
    fn apply_plot_dialog(&mut self) {
        let Some(d) = self.plot_dialog.clone() else { return };
        if d.fields[1].trim().is_empty() {
            return self.plot_error("name what to draw under Curves, e.g. M");
        }
        let old = d.row.and_then(|r| self.calc_line(r)).map(|s| Parts::of(&s)).unwrap_or_default();
        let line = super::calc_view::tidy_input(&d.line(&old));
        if let Line::Error { msg } = expr::parse_line(&line) {
            return self.plot_error(&msg);
        }
        self.plot_dialog = None;
        let r = match d.row {
            Some(r) => {
                self.set_calc_line(r, line);
                r
            }
            None => self.insert_calc_lines(d.after, vec![line]),
        };
        self.view.cursor = (r, INPUT);
        match self.calc_problem() {
            Some(p) => self.error(p),
            None => self.info("chart written – it is shown in ▥, in layouts and when printed"),
        }
    }

    fn plot_error(&mut self, msg: &str) {
        if let Some(d) = &mut self.plot_dialog {
            d.error = Some(msg.to_string());
        }
    }

    /// F6 / `:library` in the calculation sheet: the formula library.
    pub(super) fn open_library(&mut self) {
        if self.view.kind != ViewKind::Calc {
            return self.error(":library works in the calculation sheet (∑ calc)");
        }
        self.library = Some(LibraryDialog { entries: library::entries(), query: String::new(), selected: 0, scroll: 0 });
    }

    pub(super) fn key_library(&mut self, key: KeyEvent) {
        let Some(d) = &mut self.library else { return };
        let n = d.shown().len();
        let last = n.saturating_sub(1);
        match key.code {
            KeyCode::Esc if !d.query.is_empty() => {
                d.query.clear();
                d.selected = 0;
            }
            KeyCode::Esc | KeyCode::F(10) => self.library = None,
            KeyCode::Up => d.selected = d.selected.saturating_sub(1),
            KeyCode::Down => d.selected = (d.selected + 1).min(last),
            KeyCode::PageUp => d.selected = d.selected.saturating_sub(10),
            KeyCode::PageDown => d.selected = (d.selected + 10).min(last),
            KeyCode::Home => d.selected = 0,
            KeyCode::End => d.selected = last,
            KeyCode::Backspace => {
                d.query.pop();
                d.selected = 0;
            }
            KeyCode::Char(c) => {
                d.query.push(c);
                d.selected = 0;
            }
            KeyCode::Enter | KeyCode::F(2) => {
                let entry = d.shown().get(d.selected).map(|e| (*e).clone());
                self.library = None;
                if let Some(e) = entry {
                    self.insert_formula(&e);
                }
            }
            _ => {}
        }
    }

    /// Inserts a library formula after the cursor: its definition (unless
    /// the sheet has it), a line for each input the sheet lacks and the
    /// line that uses it.
    pub fn insert_formula(&mut self, e: &Entry) {
        let sources = crate::sheet::source_lines(&self.doc);
        let (mut defined, mut known) = (false, Vec::new());
        for l in &sources {
            match expr::parse_line(l) {
                Line::Stmt { stmt: Some(Stmt::Def { name, .. }), .. } if name == e.name => defined = true,
                Line::Stmt { stmt: Some(Stmt::Assign { name, .. }), .. } => known.push(name),
                _ => {}
            }
        }
        let mut lines = Vec::new();
        if !defined {
            lines.push(e.line.clone());
        }
        let missing: Vec<&String> = e.params.iter().filter(|p| !known.contains(p)).collect();
        lines.extend(missing.iter().map(|p| format!("{p} =")));
        let result = format!("{} = {}({})", result_name(&e.name), e.name, e.params.join("; "));
        let use_line = match &e.note {
            Some(n) => format!("[{}] {result}   ' {n}", e.title),
            None => format!("[{}] {result}", e.title),
        };
        lines.push(use_line);
        let first = self.insert_calc_lines(self.view.cursor.0, lines);
        // The cursor on the first input to fill in, else on the result:
        // both come right after the definition.
        self.view.cursor = (first + usize::from(!defined), INPUT);
        self.view.anchor = None;
        if missing.is_empty() {
            self.info(format!("{} inserted", e.title));
        } else {
            let names: Vec<&str> = missing.iter().map(|s| s.as_str()).collect();
            self.info(format!("{} inserted – enter {}", e.title, names.join(", ")));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::result_name;

    #[test]
    fn result_names() {
        assert_eq!(result_name("M_ss"), "M");
        assert_eq!(result_name("c_hyp"), "c");
        assert_eq!(result_name("f"), "f");
    }
}
