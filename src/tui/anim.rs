//! Soft animations, applied to the drawn frame afterwards.
//!
//! Drawing stays as it is: every frame shows the final state, and the
//! animations here rearrange the cells of that frame while they run.
//! - Menus and dialogs slide in from above.
//! - Switching tables turns the window like a cube (pseudo-3D).
//! - While a formula is edited, dots flow from its sources to its cell.
//! - Opening and closing look like an old tube screen switching on and
//!   off: a line of light opens into the picture, and collapses back
//!   into a line and a dot.
//!
//! Disabled unless `enabled` is set (the interactive UI sets it, tests do
//! not), and with `HARDCALC_ANIMATIONS=0`. They switch themselves off when
//! frames take too long (slow machine or terminal), unless switched on by
//! hand.

use ratatui::buffer::{Buffer, Cell};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

/// How long popups take to slide in.
const POPUP: Duration = Duration::from_millis(110);
/// How long the cube turn between tables takes.
const CUBE: Duration = Duration::from_millis(280);
/// Switching on (start) and off (quit).
const OPEN: Duration = Duration::from_millis(520);
const CLOSE: Duration = Duration::from_millis(420);
/// Flowing dots: distance between them (in cell widths) and speed (cell
/// widths per second); rows count twice, as cells are about twice as high.
const FLOW_SPACING: f32 = 8.0;
const FLOW_SPEED: f32 = 12.0;
/// Time between frames while only dots flow (30 per second).
pub const FLOW_FRAME: Duration = Duration::from_millis(33);
/// Animated frames that take longer than this (median of the last
/// `SAMPLES`, drawing and output) switch the animations off.
const SLOW_FRAME: Duration = Duration::from_millis(35);
const SAMPLES: usize = 8;
/// Camera distance of the cube, in half widths of the window.
const CAMERA: f32 = 3.2;

/// Running animations and what they start from.
#[derive(Default)]
pub struct Anim {
    pub enabled: bool,
    /// Time of the frame being drawn.
    now: Option<Instant>,
    /// Set while drawing when an animation still runs: the next frame is due soon.
    active: bool,
    /// Set while drawing when dots flow (runs as long as a formula is edited).
    flowing: bool,
    /// Time origin of the flowing dots.
    flow_epoch: Option<Instant>,
    /// Popup name → (identity, shown since).
    popups: HashMap<&'static str, (u64, Instant)>,
    /// Per window (true: left / only window).
    windows: HashMap<bool, WindowState>,
    /// When the first frame was drawn (switching on).
    opened: Option<Instant>,
    /// When quitting started (switching off), and whether its last frame was drawn.
    closing: Option<Instant>,
    closed: bool,
    /// Switched on or off with `:animations`: no automatic switching off.
    by_hand: bool,
    /// Durations of the last animated frames.
    samples: VecDeque<Duration>,
}

/// What a window showed last, and a running cube turn.
struct WindowState {
    ident: (u32, usize),
    area: Rect,
    last: Buffer,
    turn: Option<Turn>,
    /// The picture on screen while a turn runs.
    shown: Option<Buffer>,
}

struct Turn {
    old: Buffer,
    start: Instant,
    /// The new table lies right of the old one: the cube turns left.
    forward: bool,
}

/// How far 0..1 an animation of `dur` started at `start` is at `now`.
fn fraction(now: Instant, start: Instant, dur: Duration) -> f32 {
    if dur.is_zero() {
        return 1.0;
    }
    (now.saturating_duration_since(start).as_secs_f32() / dur.as_secs_f32()).min(1.0)
}

pub fn ease_out(t: f32) -> f32 {
    1.0 - (1.0 - t).powi(3)
}

pub fn ease_in_out(t: f32) -> f32 {
    if t < 0.5 { 4.0 * t * t * t } else { 1.0 - (-2.0 * t + 2.0).powi(3) / 2.0 }
}

impl Anim {
    /// Starts a frame. Animations are on only when enabled and not switched off.
    pub fn begin_frame(&mut self) {
        let now = Instant::now();
        self.now = Some(now);
        self.active = false;
        self.flowing = false;
        if self.enabled && self.opened.is_none() {
            self.opened = Some(now);
        }
    }

    /// `:animations on|off`: also keeps them from switching off by themselves.
    pub fn set_by_hand(&mut self, on: bool) {
        self.enabled = on;
        self.by_hand = true;
        self.samples.clear();
    }

    /// Notes how long an animated frame took (drawing and output). Returns
    /// true when this switched the animations off: the machine or the
    /// terminal is too slow for them.
    pub fn record(&mut self, took: Duration) -> bool {
        if !self.enabled || self.by_hand {
            return false;
        }
        self.samples.push_back(took);
        if self.samples.len() > SAMPLES {
            self.samples.pop_front();
        }
        if self.samples.len() < SAMPLES {
            return false;
        }
        let mut sorted: Vec<Duration> = self.samples.iter().copied().collect();
        sorted.sort();
        if sorted[SAMPLES / 2] <= SLOW_FRAME {
            return false;
        }
        self.enabled = false;
        self.samples.clear();
        true
    }

    /// Quitting: starts switching off. True while frames of it remain to be
    /// drawn; false when it is done (or animations are off).
    pub fn close(&mut self) -> bool {
        if !self.enabled {
            return false;
        }
        if self.closing.is_none() {
            self.closing = Some(self.now());
            self.closed = false;
            return true;
        }
        !self.closed
    }

    /// After everything was drawn: switching on or off, over the whole frame.
    pub fn finish(&mut self, buf: &mut Buffer) {
        if !self.enabled {
            return;
        }
        if let Some(start) = self.closing {
            let t = self.progress(start, CLOSE);
            switch_off(buf, t);
            self.closed = t >= 1.0;
        } else if let Some(start) = self.opened {
            let t = self.progress(start, OPEN);
            if t < 1.0 {
                switch_on(buf, t);
            }
        }
    }

    /// Whether an animation with an end still runs (not counting the
    /// flowing dots), so the next frame should come soon.
    pub fn active(&self) -> bool {
        self.active
    }

    /// Whether dots flow: frames should keep coming, at a calmer pace.
    pub fn flowing(&self) -> bool {
        self.flowing
    }

    /// Dots flowing from each source (screen rectangle, colour, arrow for a
    /// source out of sight) to the `target` cell, on a right-angled way from
    /// edge to edge (`route`), over `buf` within `area`. The arrow stands on
    /// the first cell of the way within `area`.
    pub fn flow(&mut self, buf: &mut Buffer, area: Rect, target: Rect, sources: &[(Rect, Color, Option<char>)]) {
        if !self.enabled || sources.is_empty() || self.closing.is_some() {
            return;
        }
        let now = self.now();
        let epoch = *self.flow_epoch.get_or_insert(now);
        self.flowing = true;
        let travelled = now.saturating_duration_since(epoch).as_secs_f32() * FLOW_SPEED;
        for &(source, color, arrow) in sources {
            let way = route(source, target);
            for &((x, y), at, step) in &way {
                // A dot is on the cell it reaches in this step.
                if (at - travelled).rem_euclid(FLOW_SPACING) < step && area.contains((x, y).into()) {
                    buf[(x, y)].set_char('•').set_style(Style::new().fg(color));
                }
            }
            if let Some(arrow) = arrow
                && let Some(&((x, y), ..)) = way.iter().find(|((x, y), ..)| area.contains((*x, *y).into()))
            {
                buf[(x, y)].set_char(arrow).set_style(Style::new().fg(color).add_modifier(Modifier::BOLD));
            }
        }
    }

    fn now(&self) -> Instant {
        self.now.unwrap_or_else(Instant::now)
    }

    /// Progress 0..1 of an animation started at `start`; marks the frame active while < 1.
    fn progress(&mut self, start: Instant, dur: Duration) -> f32 {
        let t = fraction(self.now(), start, dur);
        if t < 1.0 {
            self.active = true;
        }
        t
    }

    /// Lets the popup `name` slide in from above: `base` is the frame before it
    /// was drawn, `buf` the frame with it; `key` identifies what it shows
    /// (`None`: not shown). Only a popup that appears slides; when it shows
    /// something else (the next menu, a submenu) a running slide goes on and
    /// a finished one does not start again. Only rows from `clip_top` on
    /// take part (the popup comes out from behind that line).
    pub fn popup(&mut self, name: &'static str, key: Option<u64>, buf: &mut Buffer, base: &Buffer, clip_top: u16) {
        let Some(key) = key.filter(|_| self.enabled) else {
            self.popups.remove(name);
            return;
        };
        let now = self.now();
        let start = match self.popups.get(name) {
            Some(&(_, start)) => start,
            None => now,
        };
        self.popups.insert(name, (key, start));
        let t = self.progress(start, POPUP);
        if t >= 1.0 {
            return;
        }
        // The popup: the cells that differ from the frame before it.
        let area = buf.area;
        let (mut x0, mut x1, mut y0, mut y1) = (u16::MAX, 0, u16::MAX, 0);
        for y in clip_top.max(area.y)..area.bottom() {
            for x in area.x..area.right() {
                if buf[(x, y)] != base[(x, y)] {
                    x0 = x0.min(x);
                    x1 = x1.max(x + 1);
                    y0 = y0.min(y);
                    y1 = y1.max(y + 1);
                }
            }
        }
        if x0 >= x1 {
            return;
        }
        let clip = clip_top.max(area.y);
        // At the start the popup's bottom edge is at the clip line.
        // It starts 40 % out, so the first frame already shows it.
        let travel = (y1 - clip) as f32 * 0.6;
        let dy = ((1.0 - ease_out(t)) * travel).round() as u16;
        let popup: Vec<Vec<Cell>> = (y0..y1).map(|y| (x0..x1).map(|x| buf[(x, y)].clone()).collect()).collect();
        for y in clip..y1 {
            for x in x0..x1 {
                let src = y + dy;
                let cell = if src >= y0 && src < y1 { popup[(src - y0) as usize][(x - x0) as usize].clone() } else { base[(x, y)].clone() };
                buf[(x, y)] = cell;
            }
        }
    }

    /// After a window was drawn into `area` of `buf`: when it now shows
    /// another table (`ident`: order of its tab, sheet), turns the old
    /// content away like a cube with the new one on its next face.
    pub fn window(&mut self, side: bool, ident: (u32, usize), buf: &mut Buffer, area: Rect) {
        if !self.enabled || area.width < 4 || area.height < 2 {
            self.windows.remove(&side);
            return;
        }
        let now = self.now();
        let fresh = snapshot(buf, area);
        let Some(state) = self.windows.get_mut(&side) else {
            self.windows.insert(side, WindowState { ident, area, last: fresh, turn: None, shown: None });
            return;
        };
        if state.area != area {
            *state = WindowState { ident, area, last: fresh, turn: None, shown: None };
            return;
        }
        if state.ident != ident {
            // A turn that still runs goes on from the picture on screen
            // (the cube half turned), so nothing jumps.
            let old = state.shown.take().unwrap_or_else(|| std::mem::replace(&mut state.last, Buffer::empty(area)));
            state.turn = Some(Turn { old, start: now, forward: ident > state.ident });
            state.ident = ident;
        }
        state.last = fresh;
        let Some(turn) = self.windows.get(&side).and_then(|s| s.turn.as_ref()).map(|t| (t.start, t.forward)) else { return };
        let t = self.progress(turn.0, CUBE);
        let state = self.windows.get_mut(&side).unwrap();
        if t >= 1.0 {
            state.turn = None;
            state.shown = None;
            return;
        }
        let p = ease_in_out(t);
        let old = &state.turn.as_ref().unwrap().old;
        let new = &state.last;
        if turn.1 { cube(buf, area, old, new, p) } else { cube(buf, area, new, old, 1.0 - p) }
        state.shown = Some(snapshot(buf, area));
    }
}

/// The cells of `area` in `buf`.
fn snapshot(buf: &Buffer, area: Rect) -> Buffer {
    let mut copy = Buffer::empty(area);
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            copy[(x, y)] = buf[(x, y)].clone();
        }
    }
    copy
}

/// The cells of a right-angled way from the edge of `source` to the edge of
/// `target`, each with its distance from the start and the length of the
/// step to it (rows count twice). Straight when both share columns or
/// rows, else sideways along the source's middle row, then up or down.
fn route(source: Rect, target: Rect) -> Vec<((u16, u16), f32, f32)> {
    let mut cells = Vec::new();
    let mut at = 0.0;
    let mut go = |cells: &mut Vec<_>, cell: (u16, u16), step: f32| {
        at += step;
        cells.push((cell, at, step));
    };
    // Cells strictly between `a` and `b` (a < b), in the order walked.
    let forward = |a: u16, b: u16| -> Vec<u16> { (a..b).collect() };
    let back = |a: u16, b: u16| -> Vec<u16> { (a..b).rev().collect() };
    let (cols0, cols1) = (source.x.max(target.x), source.right().min(target.right()));
    let (rows0, rows1) = (source.y.max(target.y), source.bottom().min(target.bottom()));
    if cols0 < cols1 {
        // Above or below each other.
        let x = (cols0 + cols1 - 1) / 2;
        let ys = if target.y >= source.bottom() { forward(source.bottom(), target.y) } else { back(target.bottom(), source.y) };
        for y in ys {
            go(&mut cells, (x, y), 2.0);
        }
    } else if rows0 < rows1 {
        // Side by side.
        let y = (rows0 + rows1 - 1) / 2;
        let xs = if target.x >= source.right() { forward(source.right(), target.x) } else { back(target.right(), source.x) };
        for x in xs {
            go(&mut cells, (x, y), 1.0);
        }
    } else {
        // Sideways to the target's column, then up or down to it.
        let y = source.y + source.height / 2;
        let x = target.x + target.width / 2;
        let xs = if x >= source.right() { forward(source.right(), x + 1) } else { back(x, source.x) };
        for cx in xs {
            go(&mut cells, (cx, y), 1.0);
        }
        let ys = if target.y > y { forward(y + 1, target.y) } else { back(target.bottom(), y) };
        for cy in ys {
            go(&mut cells, (x, cy), 2.0);
        }
    }
    cells
}

/// Switching on at `t` (0..1): a line of light grows from the middle, then
/// opens up and down into the picture.
fn switch_on(buf: &mut Buffer, t: f32) {
    const LINE: f32 = 0.3;
    if t < LINE {
        tube(buf, 0.0, ease_out(t / LINE), 1.0);
    } else {
        tube(buf, ease_out((t - LINE) / (1.0 - LINE)), 1.0, 1.0);
    }
}

/// Switching off at `t` (0..1): the picture collapses into a line, the
/// line into a dot, and the dot fades.
fn switch_off(buf: &mut Buffer, t: f32) {
    let ease_in = |x: f32| x * x * x;
    if t < 0.5 {
        tube(buf, 1.0 - ease_in(t / 0.5), 1.0, 1.0);
    } else if t < 0.8 {
        tube(buf, 0.0, 1.0 - ease_in((t - 0.5) / 0.3), 1.0);
    } else {
        tube(buf, 0.0, 0.0, 1.0 - (t - 0.8) / 0.2);
    }
}

/// The picture squeezed to `height` (0..1) of the screen around its middle,
/// edged by lines of light; at height 0 a line of `width` (0..1), at width
/// 0 a dot of brightness `glow` (0..1).
fn tube(buf: &mut Buffer, height: f32, width: f32, glow: f32) {
    let area = buf.area;
    if area.width == 0 || area.height == 0 {
        return;
    }
    let picture = buf.clone();
    let (w, h) = (area.width as f32, area.height as f32);
    let cy = area.y as f32 + h / 2.0;
    let mid = area.y + area.height / 2;
    let light = Style::new().fg(Color::White).add_modifier(Modifier::BOLD);
    let half = height * h / 2.0;
    for y in area.top()..area.bottom() {
        let dist = (y as f32 + 0.5 - cy).abs();
        for x in area.left()..area.right() {
            let mut cell = Cell::default();
            if half >= 1.0 {
                if dist < half - 0.5 {
                    let v = (y as f32 + 0.5 - cy) / height + cy;
                    if v >= area.y as f32 && v < area.bottom() as f32 {
                        cell = picture[(x, v as u16)].clone();
                    }
                } else if dist < half + 0.5 {
                    cell.set_char('━').set_style(light);
                }
            } else if y == mid {
                let reach = width * w / 2.0;
                let dx = (x as f32 + 0.5 - (area.x as f32 + w / 2.0)).abs();
                if reach >= 0.5 && dx < reach {
                    cell.set_char(if dx > reach - 1.5 { '─' } else { '━' }).set_style(light);
                } else if reach < 0.5 && x == area.x + area.width / 2 && glow > 0.05 {
                    let dot = if glow > 0.5 { light } else { light.remove_modifier(Modifier::BOLD).add_modifier(Modifier::DIM) };
                    cell.set_char(if glow > 0.6 { '●' } else { '•' }).set_style(dot);
                }
            }
            buf[(x, y)] = cell;
        }
    }
}

/// Draws a cube turned by `p` (0..1 of a quarter turn) into `area`: the face
/// `front` turns away to the left, `right` comes in from the right.
fn cube(buf: &mut Buffer, area: Rect, front: &Buffer, right: &Buffer, p: f32) {
    let theta = p * std::f32::consts::FRAC_PI_2;
    let (sin, cos) = theta.sin_cos();
    // Corners of the cube's cross-section seen from above, turned: front
    // left, front right (the shared edge), back right.
    let turn = |x: f32, z: f32| (x * cos + z * sin, -x * sin + z * cos);
    // Perspective scale, 1 for the front face at rest.
    let scale = |z: f32| (CAMERA - 1.0) / (CAMERA + z);
    let project = |(x, z): (f32, f32)| (x * scale(z), scale(z));
    let l = project(turn(-1.0, -1.0));
    let s = project(turn(1.0, -1.0));
    let r = project(turn(1.0, 1.0));
    let (w, h) = (area.width as f32, area.height as f32);
    let cy = area.y as f32 + h / 2.0;
    for x in area.left()..area.right() {
        let u = (x - area.x) as f32 + 0.5;
        let u = u / w * 2.0 - 1.0;
        // Face, its edges, and how much it faces the viewer.
        let (face, a, b, light) = if u >= l.0 && u < s.0 {
            (front, l, s, cos)
        } else if u >= s.0 && u < r.0 {
            (right, s, r, sin)
        } else {
            for y in area.top()..area.bottom() {
                buf[(x, y)] = Cell::default();
            }
            continue;
        };
        let k = ((u - a.0) / (b.0 - a.0)).clamp(0.0, 1.0);
        // Perspective-correct position along the face, and its height there.
        let along = k * b.1 / ((1.0 - k) * a.1 + k * b.1);
        let height = (1.0 - k) * a.1 + k * b.1;
        let sx = area.x + ((along * w) as u16).min(area.width - 1);
        for y in area.top()..area.bottom() {
            let v = (y as f32 + 0.5 - cy) / height + cy;
            let cell = if v >= area.y as f32 && v < area.bottom() as f32 {
                let mut cell = face[(sx, v as u16)].clone();
                if light < 0.6 {
                    cell.modifier.insert(Modifier::DIM);
                }
                cell
            } else {
                Cell::default()
            };
            buf[(x, y)] = cell;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn easing_ends() {
        for f in [ease_out, ease_in_out] {
            assert!(f(0.0).abs() < 1e-6);
            assert!((f(1.0) - 1.0).abs() < 1e-6);
        }
    }

    #[test]
    fn cube_at_rest_shows_front_then_right() {
        let area = Rect::new(0, 0, 20, 6);
        let mut a = Buffer::empty(area);
        let mut b = Buffer::empty(area);
        for y in 0..6 {
            a.set_string(0, y, "a".repeat(20), ratatui::style::Style::new());
            b.set_string(0, y, "b".repeat(20), ratatui::style::Style::new());
        }
        let mut out = Buffer::empty(area);
        cube(&mut out, area, &a, &b, 0.0);
        assert_eq!(out, a);
        cube(&mut out, area, &a, &b, 1.0);
        assert_eq!(out[(10, 3)].symbol(), "b");
    }

    #[test]
    fn interrupted_turn_goes_on_from_the_picture_on_screen() {
        let area = Rect::new(0, 0, 20, 6);
        let table = |ch: &str| {
            let mut b = Buffer::empty(area);
            for y in 0..6 {
                b.set_string(0, y, ch.repeat(20), Style::new());
            }
            b
        };
        let mut anim = Anim { enabled: true, ..Anim::default() };
        let t0 = Instant::now();
        let frame = |anim: &mut Anim, at: Duration, sheet: usize, ch: &str| {
            anim.now = Some(t0 + at);
            let mut buf = table(ch);
            anim.window(true, (0, sheet), &mut buf, area);
            buf
        };
        frame(&mut anim, Duration::ZERO, 0, "a");
        assert_eq!(frame(&mut anim, Duration::ZERO, 1, "b"), table("a"), "the turn starts at the old table");
        let half = frame(&mut anim, CUBE / 2, 1, "b");
        assert!(half != table("a") && half != table("b"));
        // The next table while the cube is half turned: no jump.
        assert_eq!(frame(&mut anim, CUBE / 2, 2, "c"), half);
        // And it ends at the newest table.
        assert_eq!(frame(&mut anim, CUBE * 2, 2, "c"), table("c"));
    }

    #[test]
    fn switching_on_and_off() {
        let area = Rect::new(0, 0, 20, 9);
        let mut full = Buffer::empty(area);
        for y in 0..9 {
            full.set_string(0, y, "x".repeat(20), Style::new());
        }
        let count = |b: &Buffer, s: &str| (0..9).flat_map(|y| (0..20).map(move |x| (x, y))).filter(|&p| b[p].symbol() == s).count();
        // Early: a line in the middle, no picture yet.
        let mut b = full.clone();
        switch_on(&mut b, 0.1);
        assert_eq!(count(&b, "x"), 0);
        assert!(count(&b, "━") > 0);
        // At the end of switching on the picture is complete.
        let mut b = full.clone();
        switch_on(&mut b, 0.9999);
        assert!(count(&b, "x") >= 20 * 7, "{}", count(&b, "x"));
        // Switching off ends dark.
        let mut b = full.clone();
        switch_off(&mut b, 1.0);
        assert_eq!(count(&b, " "), 20 * 9);
        let mut b = full.clone();
        switch_off(&mut b, 0.85);
        assert_eq!(count(&b, "x"), 0);
    }

    #[test]
    fn slow_frames_switch_off_unless_by_hand() {
        let mut anim = Anim { enabled: true, ..Anim::default() };
        for _ in 0..SAMPLES - 1 {
            assert!(!anim.record(Duration::from_millis(80)));
        }
        assert!(anim.record(Duration::from_millis(80)));
        assert!(!anim.enabled);
        let mut fast = Anim { enabled: true, ..Anim::default() };
        for _ in 0..3 * SAMPLES {
            assert!(!fast.record(Duration::from_millis(5)));
        }
        assert!(fast.enabled);
        let mut manual = Anim::default();
        manual.set_by_hand(true);
        for _ in 0..3 * SAMPLES {
            assert!(!manual.record(Duration::from_millis(80)));
        }
        assert!(manual.enabled);
    }

    #[test]
    fn routes_are_right_angled_and_outside_both_ends() {
        let check = |source: Rect, target: Rect| {
            let cells: Vec<(u16, u16)> = route(source, target).into_iter().map(|(c, ..)| c).collect();
            for w in cells.windows(2) {
                let (a, b) = (w[0], w[1]);
                assert_eq!(a.0.abs_diff(b.0) + a.1.abs_diff(b.1), 1, "steps of one cell, no diagonals: {cells:?}");
            }
            assert!(cells.iter().all(|&c| !source.contains(c.into()) && !target.contains(c.into())), "{cells:?}");
            cells
        };
        // Below, beside, apart (an L), and touching (no way at all).
        assert_eq!(check(Rect::new(2, 1, 6, 1), Rect::new(2, 5, 6, 1)).len(), 3);
        assert_eq!(check(Rect::new(2, 1, 6, 1), Rect::new(12, 1, 6, 1)).len(), 4);
        let l = check(Rect::new(2, 1, 6, 2), Rect::new(30, 8, 6, 1));
        assert!(l.first().unwrap().1 == l[1].1 && l.last().unwrap().0 == l[l.len() - 2].0, "sideways, then down");
        assert!(check(Rect::new(2, 1, 6, 1), Rect::new(2, 2, 6, 1)).is_empty());
        // Up and to the left.
        check(Rect::new(30, 8, 6, 1), Rect::new(2, 1, 6, 2));
    }

    #[test]
    fn dots_flow_between_source_and_target() {
        let area = Rect::new(0, 0, 40, 10);
        let mut anim = Anim { enabled: true, ..Anim::default() };
        let dots = |anim: &mut Anim| {
            anim.begin_frame();
            let mut buf = Buffer::empty(area);
            anim.flow(&mut buf, area, Rect::new(30, 8, 6, 1), &[(Rect::new(2, 1, 6, 2), Color::Blue, None)]);
            (0..10).flat_map(|y| (0..40).map(move |x| (x, y))).filter(|&p| buf[p].symbol() != " ").collect::<Vec<_>>()
        };
        let first = dots(&mut anim);
        assert!(!first.is_empty() && anim.flowing() && !anim.active());
        // None inside the source or the target.
        assert!(first.iter().all(|&(x, y)| !(2..8).contains(&x) || !(1..3).contains(&y)));
        assert!(first.iter().all(|&(x, y)| !((30..36).contains(&x) && y == 8)));
        std::thread::sleep(Duration::from_millis(120));
        assert_ne!(dots(&mut anim), first, "they move");
    }

    #[test]
    fn disabled_does_nothing() {
        let mut anim = Anim::default();
        anim.begin_frame();
        let area = Rect::new(0, 0, 10, 4);
        let base = Buffer::empty(area);
        let mut buf = Buffer::empty(area);
        buf.set_string(2, 1, "menu", ratatui::style::Style::new());
        let before = buf.clone();
        anim.popup("m", Some(1), &mut buf, &base, 0);
        assert_eq!(buf, before);
        assert!(!anim.active());
    }
}
