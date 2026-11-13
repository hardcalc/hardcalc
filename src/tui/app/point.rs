//! Pointing while editing a formula, as in Excel: arrows (Shift: a range) or
//! a click / drag pick cells, and the reference is written at the caret.

use super::{App, Mode, ViewKind};
use crate::engine::col_letters;

/// The reference being picked.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    /// Where the reference starts in the input (in characters).
    pub start: usize,
    /// Its current length (in characters).
    pub len: usize,
    /// Other corner of a range.
    pub anchor: Option<(usize, usize)>,
    pub cell: (usize, usize),
    /// First corner typed before `:` (`B1:` + arrows → `B1:B3`), for the highlight.
    pub base: Option<(usize, usize)>,
}

fn span(a: (usize, usize), b: (usize, usize)) -> ((usize, usize), (usize, usize)) {
    ((a.0.min(b.0), a.1.min(b.1)), (a.0.max(b.0), a.1.max(b.1)))
}

impl Point {
    /// Normalized range ((top, left), (bottom, right)) that is highlighted.
    pub fn range(&self) -> ((usize, usize), (usize, usize)) {
        span(self.base.or(self.anchor).unwrap_or(self.cell), self.cell)
    }

    fn reference(&self) -> String {
        let cell = |(r, c): (usize, usize)| format!("{}{}", col_letters(c), r + 1);
        let (a, b) = span(self.anchor.unwrap_or(self.cell), self.cell);
        if a == b { cell(a) } else { format!("{}:{}", cell(a), cell(b)) }
    }
}

impl App {
    /// Whether arrows / clicks pick a reference now: a formula is edited in
    /// a table and the caret follows `=`, `(`, `,`, an operator or a
    /// reference being picked.
    pub(super) fn can_point(&self) -> bool {
        let Mode::Edit { buf, cursor } = &self.mode else { return false };
        if self.view.kind != ViewKind::Table || self.view.pivot.is_some() || self.solo.is_some() || self.on_header().is_some() {
            return false;
        }
        if !buf.trim_start().starts_with('=') {
            return false;
        }
        if self.point.is_some_and(|p| p.start + p.len == *cursor) {
            return true;
        }
        let before = buf.chars().take(*cursor).collect::<String>();
        // Inside a finished formula (`=A1 + |B1`, spaces included) the arrows
        // move the caret: only before the end, a closing bracket or an operator
        // is a reference picked.
        let after = buf.chars().skip(*cursor).collect::<String>();
        let open_ahead = after.trim_start().chars().next().is_none_or(|c| matches!(c, ')' | ',' | ';' | '+' | '-' | '*' | '/' | '^' | '&' | '<' | '>' | '=' | ':'));
        open_ahead && before.trim_end().ends_with(['=', '(', ',', ';', '+', '-', '*', '/', '^', '&', '<', '>', ':'])
    }

    /// The range being picked (for drawing).
    pub fn point_range(&self) -> Option<((usize, usize), (usize, usize))> {
        self.point.filter(|_| matches!(self.mode, Mode::Edit { .. })).map(|p| p.range())
    }

    /// The cell the window should keep visible.
    pub fn focus_cell(&self) -> (usize, usize) {
        match self.point.filter(|_| matches!(self.mode, Mode::Edit { .. })) {
            Some(p) => p.cell,
            None => self.view.cursor,
        }
    }

    /// Arrow key while pointing: from the edited cell (or the picked one).
    pub(super) fn point_move(&mut self, dr: isize, dc: isize, extend: bool) {
        let (_, cols) = self.view_dims();
        let active = self.active_point();
        // After `B1:` the second corner starts at B1 (as in Excel).
        let from = active.map_or_else(|| self.range_start().unwrap_or(self.view.cursor), |p| p.cell);
        let r = self.step_row(from.0, dr);
        let c = (from.1 as isize + dc).clamp(0, cols.saturating_sub(1) as isize) as usize;
        let anchor = if extend { Some(active.and_then(|p| p.anchor).unwrap_or(from)) } else { None };
        self.point_at((r, c), anchor);
    }

    /// The picked reference, if the caret is still right behind it.
    fn active_point(&self) -> Option<Point> {
        let Mode::Edit { cursor, .. } = &self.mode else { return None };
        self.point.filter(|p| p.start + p.len == *cursor)
    }

    /// The cell before a `:` right in front of the caret (`=sum(B1:`).
    fn range_start(&self) -> Option<(usize, usize)> {
        let Mode::Edit { buf, cursor } = &self.mode else { return None };
        let before: String = buf.chars().take(*cursor).collect();
        let before = before.trim_end().strip_suffix(':')?.trim_end();
        let start = before.rfind(|c: char| !c.is_ascii_alphanumeric()).map_or(0, |i| i + 1);
        crate::engine::parse_a1(&before[start..]).map(|(c, r)| (r, c))
    }

    /// Picks `cell` (with `anchor`: the range between them) and writes the
    /// reference at the caret, replacing the one picked before.
    pub(super) fn point_at(&mut self, cell: (usize, usize), anchor: Option<(usize, usize)>) {
        let base = self.range_start();
        let Mode::Edit { buf, cursor } = &mut self.mode else { return };
        let mut p = match self.point.filter(|p| p.start + p.len == *cursor) {
            Some(p) => p,
            None => Point { start: *cursor, len: 0, anchor: None, cell, base },
        };
        p.cell = cell;
        p.anchor = anchor.filter(|a| *a != cell);
        let text = p.reference();
        let byte_at = |s: &str, i: usize| s.char_indices().nth(i).map(|(b, _)| b).unwrap_or(s.len());
        let (from, to) = (byte_at(buf, p.start), byte_at(buf, p.start + p.len));
        buf.replace_range(from..to, &text);
        p.len = text.chars().count();
        *cursor = p.start + p.len;
        self.point = Some(p);
        self.view.follow = true;
    }
}
