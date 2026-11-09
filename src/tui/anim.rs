//! Soft animations, applied to the drawn frame afterwards.
//!
//! Drawing stays as it is: every frame shows the final state, and the
//! animations here rearrange the cells of that frame while they run.
//! - Popups (menus, dialogs) slide in from above.
//! - Scrolling the grid glides character by character (sideways) and line
//!   by line (up and down) to the new position.
//! - Switching tables turns the window like a cube (pseudo-3D).
//!
//! Disabled unless `enabled` is set (the interactive UI sets it, tests do
//! not), and with `HARDCALC_ANIMATIONS=0`.

use ratatui::buffer::{Buffer, Cell};
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// How long popups take to slide in.
const POPUP: Duration = Duration::from_millis(100);
/// How long the cube turn between tables takes.
const CUBE: Duration = Duration::from_millis(260);
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
    /// Popup name → (identity, shown since).
    popups: HashMap<&'static str, (u64, Instant)>,
    /// Per window (true: left / only window).
    windows: HashMap<bool, WindowState>,
    scrolls: HashMap<bool, ScrollState>,
}

/// What a window showed last, and a running cube turn.
struct WindowState {
    ident: (u32, usize),
    area: Rect,
    last: Buffer,
    turn: Option<Turn>,
}

struct Turn {
    old: Buffer,
    start: Instant,
    /// The new table lies right of the old one: the cube turns left.
    forward: bool,
}

/// Displayed and target scroll position of a grid, in rows and characters.
struct ScrollState {
    ident: u64,
    from: (f32, f32),
    to: (f32, f32),
    start: Instant,
    dur: Duration,
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
        self.now = Some(Instant::now());
        self.active = false;
    }

    /// Whether an animation still runs, so the next frame should come soon.
    pub fn active(&self) -> bool {
        self.active
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
    /// (`None`: not shown), a new identity slides in again. Only rows from
    /// `clip_top` on take part (the popup comes out from behind that line).
    pub fn popup(&mut self, name: &'static str, key: Option<u64>, buf: &mut Buffer, base: &Buffer, clip_top: u16) {
        let Some(key) = key.filter(|_| self.enabled) else {
            self.popups.remove(name);
            return;
        };
        let now = self.now();
        let start = match self.popups.get(name) {
            Some(&(k, start)) if k == key => start,
            _ => {
                self.popups.insert(name, (key, now));
                now
            }
        };
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
        let travel = (y1 - clip) as f32;
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

    /// The scroll position a grid shows: `None` when it is at `target` (rows,
    /// characters from the left of the table), else the position on the way
    /// there. A new `ident` (other table) jumps without animation.
    pub fn scroll(&mut self, side: bool, ident: u64, target: (usize, usize), screen: Rect) -> Option<(f32, f32)> {
        let to = (target.0 as f32, target.1 as f32);
        let now = self.now();
        if !self.enabled {
            self.scrolls.remove(&side);
            return None;
        }
        let state = self.scrolls.entry(side).or_insert(ScrollState { ident, from: to, to, start: now, dur: Duration::ZERO });
        if state.ident != ident {
            *state = ScrollState { ident, from: to, to, start: now, dur: Duration::ZERO };
        }
        if state.to != to {
            // From where it is now, at most a screen away.
            let (start, dur, from, old_to) = (state.start, state.dur, state.from, state.to);
            let t = fraction(now, start, dur);
            let e = ease_out(t);
            let cur = (from.0 + (old_to.0 - from.0) * e, from.1 + (old_to.1 - from.1) * e);
            let max_rows = screen.height.max(1) as f32;
            let max_chars = screen.width.max(1) as f32 * 1.5;
            let cur = (cur.0.clamp(to.0 - max_rows, to.0 + max_rows).max(0.0), cur.1.clamp(to.1 - max_chars, to.1 + max_chars).max(0.0));
            let (dr, dc) = ((to.0 - cur.0).abs(), (to.1 - cur.1).abs());
            // Short glides; quicker still when one is caught mid-way (keys
            // held down), so the screen keeps up with the cursor. A single
            // line cannot move more smoothly than at once.
            let mut ms = (50.0 + 4.0 * dc).max(if dr > 1.0 { 60.0 + 6.0 * dr } else { 0.0 }).min(150.0);
            if t < 1.0 {
                ms *= 0.5;
            }
            if dc < 0.5 && dr <= 1.0 {
                ms = 0.0;
            }
            let state = self.scrolls.get_mut(&side).unwrap();
            *state = ScrollState { ident, from: cur, to, start: now, dur: Duration::from_millis(ms as u64) };
        }
        let state = &self.scrolls[&side];
        let (start, dur, from) = (state.start, state.dur, state.from);
        let t = self.progress(start, dur);
        if t >= 1.0 {
            return None;
        }
        let e = ease_out(t);
        Some((from.0 + (to.0 - from.0) * e, from.1 + (to.1 - from.1) * e))
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
        let mut fresh = Buffer::empty(area);
        for y in area.top()..area.bottom() {
            for x in area.left()..area.right() {
                fresh[(x, y)] = buf[(x, y)].clone();
            }
        }
        let Some(state) = self.windows.get_mut(&side) else {
            self.windows.insert(side, WindowState { ident, area, last: fresh, turn: None });
            return;
        };
        if state.area != area {
            *state = WindowState { ident, area, last: fresh, turn: None };
            return;
        }
        if state.ident != ident {
            let old = std::mem::replace(&mut state.last, Buffer::empty(area));
            state.turn = Some(Turn { old, start: now, forward: ident > state.ident });
            state.ident = ident;
        }
        state.last = fresh;
        let Some(turn) = self.windows.get(&side).and_then(|s| s.turn.as_ref()).map(|t| (t.start, t.forward)) else { return };
        let t = self.progress(turn.0, CUBE);
        let state = self.windows.get_mut(&side).unwrap();
        if t >= 1.0 {
            state.turn = None;
            return;
        }
        let p = ease_in_out(t);
        let old = &state.turn.as_ref().unwrap().old;
        let new = &state.last;
        if turn.1 { cube(buf, area, old, new, p) } else { cube(buf, area, new, old, 1.0 - p) }
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
        assert!(anim.scroll(true, 0, (5, 5), area).is_none());
        assert!(!anim.active());
    }
}
