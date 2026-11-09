//! The cube turn between tables, applied to the drawn frame afterwards.
//!
//! Drawing stays as it is: every frame shows the final state, and while the
//! window turns, its cells are rearranged into a pseudo-3D cube with the
//! old table on one face and the new one on the next.
//!
//! Disabled unless `enabled` is set (the interactive UI sets it, tests do
//! not), and with `HARDCALC_ANIMATIONS=0`.

use ratatui::buffer::{Buffer, Cell};
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// How long the cube turn between tables takes.
const CUBE: Duration = Duration::from_millis(260);
/// Camera distance of the cube, in half widths of the window.
const CAMERA: f32 = 3.2;

/// Running cube turns and what they start from.
#[derive(Default)]
pub struct Anim {
    pub enabled: bool,
    /// Time of the frame being drawn.
    now: Option<Instant>,
    /// Set while drawing when an animation still runs: the next frame is due soon.
    active: bool,
    /// Per window (true: left / only window).
    windows: HashMap<bool, WindowState>,
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

/// How far 0..1 an animation of `dur` started at `start` is at `now`.
fn fraction(now: Instant, start: Instant, dur: Duration) -> f32 {
    if dur.is_zero() {
        return 1.0;
    }
    (now.saturating_duration_since(start).as_secs_f32() / dur.as_secs_f32()).min(1.0)
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
        assert!(ease_in_out(0.0).abs() < 1e-6);
        assert!((ease_in_out(1.0) - 1.0).abs() < 1e-6);
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
        let mut buf = Buffer::empty(area);
        buf.set_string(2, 1, "one", ratatui::style::Style::new());
        anim.window(true, (0, 0), &mut buf, area);
        let before = buf.clone();
        anim.window(true, (0, 1), &mut buf, area);
        assert_eq!(buf, before);
        assert!(!anim.active());
    }
}
