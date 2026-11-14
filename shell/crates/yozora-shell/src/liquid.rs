//! Surface tension's signature: a bar popover buds out of the bar as one drop of the bar's own
//! liquid. A neck stretches down from the icon, the panel swells out of it with a little
//! overshoot, and concave joins round off where the neck meets the bar and the panel. Closing
//! runs it back: the neck stretches, the panel is drawn up into the bar.
//!
//! The panel's own box only holds its content; the liquid underneath is drawn here, on
//! render-time tracks (`Tween`), so no view runs while it moves.

use std::cell::RefCell;
use std::time::{Duration, Instant};

use rune::{Color, El, Spring, SpringPair, Transition, Tween, el};
use yozora_style::{Entrance, Fx, Motion, Tokens};

use crate::bar_config::Geo;

const NECK_W: f32 = 28.0;
/// Radius of the concave joins.
const JOIN: f32 = 8.0;
/// How far the neck tucks under the bar and into the panel, so no seam shows.
const TUCK: f32 = 2.0;
const INTO_PANEL: f32 = 12.0;

/// The mockup's `te-bud`: an overshoot in, an ease-in out.
const BUD_IN: [f32; 4] = [0.34, 1.42, 0.64, 1.0];
const BUD_OUT: [f32; 4] = [0.5, 0.0, 0.75, 0.0];
/// `te-neck`: in, it overshoots to about 1.25 and settles; out, it stretches before snapping up.
const NECK_IN: [f32; 4] = [0.3, 1.9, 0.6, 1.0];
const NECK_OUT: [f32; 4] = [0.4, -0.6, 0.8, 1.0];
const EASE: [f32; 4] = [0.2, 0.0, 0.0, 1.0];

/// How a panel's content comes and goes over the drop: in once the drop has mostly swelled, out
/// in a quick fade, so it is gone before the drop is drawn back up. Its rows still settle in one by one (`kit::stagger`).
pub fn content(panel: El, t: &Tokens, fx: Fx) -> El {
    let m = Motion::of(t.look, fx);
    panel.transition(Transition::opacity().springs(SpringPair::new(Spring::new(m.open * 0.35, 0.0), Spring::new(0.12, 0.0))))
        .enter_delay(m.open * 0.4)
}

/// Where the drop is: the panel it fills and the icon it hangs from.
#[derive(Clone, Copy, PartialEq)]
struct Shape {
    panel: [f32; 4],
    neck_x: f32,
    /// The neck runs flush down the panel's left or right side, whose top corner is square.
    flush: [bool; 2],
}

struct Tracks {
    body: Tween<[f32; 4]>,
    neck: Tween<[f32; 4]>,
    /// Opacity of the joins at the bar, and at the panel.
    bar_joins: Tween<f32>,
    panel_joins: Tween<f32>,
}

struct State {
    shape: Shape,
    tracks: Tracks,
    /// Set while it runs back into the bar; the drop goes once this has passed.
    gone_at: Option<Instant>,
}

/// The drop on one stage. Views take the stage by reference, so it keeps its tracks in a cell.
#[derive(Default)]
pub struct Liquid(RefCell<Option<State>>);

/// Whether the look draws its popovers as liquid. Every effects level does; Still only drops the
/// motion.
pub fn applies(t: &Tokens) -> bool {
    Motion::of(t.look, Fx::Full).entrance == Entrance::Bud
}

/// The in and out curves, without the overshoot when the effects level has none.
fn curves(m: &Motion) -> ([f32; 4], [f32; 4], [f32; 4]) {
    if m.spring.is_some() { (BUD_IN, NECK_IN, NECK_OUT) } else { (EASE, EASE, BUD_OUT) }
}

fn bar_bottom(geo: &Geo) -> f32 {
    geo.top + geo.h
}

/// The bud the panel swells from (`h` 12, a nub under the icon) and is drawn back into (`h` 0,
/// gone under the bar).
fn bud(geo: &Geo, s: &Shape, h: f32) -> [f32; 4] {
    let w = (s.panel[2] * 0.07).max(NECK_W - 4.0);
    [s.neck_x - w / 2.0, bar_bottom(geo) - TUCK, w, h + TUCK]
}

fn neck(geo: &Geo, s: &Shape, k: f32) -> [f32; 4] {
    let top = bar_bottom(geo) - TUCK;
    let h = (s.panel[1] + INTO_PANEL - top).max(0.0);
    [s.neck_x - NECK_W / 2.0, top, NECK_W, h * k]
}

impl Liquid {
    /// Shows the drop under a panel laid out at `panel`, growing from the icon at `anchor_x`.
    /// Called on every view; it starts the tracks only when the shape changes.
    pub fn open(&self, t: &Tokens, fx: Fx, geo: &Geo, anchor_x: f32, panel: [f32; 4]) {
        // The neck hangs straight from the icon. Near one of the panel's rounded corners there is
        // no room for a join: it moves the few points to run flush down that side instead.
        let reach = t.radii.panel + JOIN + NECK_W / 2.0;
        let (lo, hi) = (panel[0] + NECK_W / 2.0, panel[0] + panel[2] - NECK_W / 2.0);
        let (neck_x, flush) = if panel[2] < reach * 2.0 {
            (panel[0] + panel[2] / 2.0, [false, false])
        } else if anchor_x < panel[0] + reach {
            (lo, [true, false])
        } else if anchor_x > panel[0] + panel[2] - reach {
            (hi, [false, true])
        } else {
            (anchor_x, [false, false])
        };
        let shape = Shape { panel, neck_x, flush };
        let mut cur = self.0.borrow_mut();
        if let Some(st) = cur.as_ref()
            && st.gone_at.is_none()
            && near(&st.shape, &shape)
        {
            return;
        }
        let m = Motion::of(t.look, fx);
        let open = Duration::from_secs_f32(m.open);
        let (bud_in, neck_in, _) = curves(&m);
        let tracks = match cur.as_ref() {
            // Already out under another panel: the drop flows over to this one.
            Some(st) if st.gone_at.is_none() => Tracks {
                neck: Tween::curve(neck(geo, &st.shape, 1.0), neck(geo, &shape, 1.0), open.mul_f32(0.6), EASE),
                body: Tween::curve(st.shape.panel, shape.panel, open.mul_f32(0.6), EASE),
                bar_joins: Tween::curve(1.0, 1.0, Duration::ZERO, EASE),
                panel_joins: Tween::curve(1.0, 1.0, Duration::ZERO, EASE),
            },
            // Otherwise it buds from the bar.
            _ => Tracks {
                neck: Tween::curve(neck(geo, &shape, 0.25), neck(geo, &shape, 1.0), open.mul_f32(0.8), neck_in),
                body: Tween::curve(bud(geo, &shape, 12.0), shape.panel, open, bud_in).delay(open.mul_f32(0.14)),
                bar_joins: Tween::curve(0.0, 1.0, open.mul_f32(0.2), EASE),
                panel_joins: Tween::curve(0.0, 1.0, open.mul_f32(0.2), EASE).delay(open.mul_f32(0.55)),
            },
        };
        *cur = Some(State { shape, tracks, gone_at: None });
    }

    /// Runs an open drop back into the bar. Returns how long until `view` stops drawing it, so
    /// the caller can redraw then.
    pub fn close(&self, t: &Tokens, fx: Fx, geo: &Geo) -> Option<Duration> {
        let mut cur = self.0.borrow_mut();
        let st = cur.as_mut()?;
        if let Some(at) = st.gone_at {
            if at <= Instant::now() {
                *cur = None;
            }
            return None;
        }
        let m = Motion::of(t.look, fx);
        let close = Duration::from_secs_f32(m.close);
        let (_, _, neck_out) = curves(&m);
        let s = st.shape;
        st.tracks = Tracks {
            body: Tween::curve(s.panel, bud(geo, &s, 0.0), close, BUD_OUT),
            neck: Tween::curve(neck(geo, &s, 1.0), neck(geo, &s, 0.0), close.mul_f32(0.9), neck_out),
            bar_joins: Tween::curve(1.0, 0.0, close.mul_f32(0.25), EASE).delay(close.mul_f32(0.75)),
            panel_joins: Tween::curve(1.0, 0.0, close.mul_f32(0.15), EASE),
        };
        st.gone_at = Some(Instant::now() + close);
        Some(close + Duration::from_millis(40))
    }

    /// The drop, or nothing when there is none.
    pub fn view(&self, t: &Tokens, geo: &Geo) -> Option<El> {
        let cur = self.0.borrow();
        let st = cur.as_ref()?;
        if st.gone_at.is_some_and(|at| at <= Instant::now()) {
            return None;
        }
        let s = &st.shape;
        let p = s.panel;
        let liq = t.liq;
        let n = neck(geo, s, 1.0);
        let yb = bar_bottom(geo);
        let left = s.neck_x - NECK_W / 2.0;
        let right = s.neck_x + NECK_W / 2.0;
        let r = t.radii.panel;
        let [flush_left, flush_right] = s.flush;
        let corner = |flush: bool| if flush { 0.0 } else { r };
        let mut root = el().w_full().h_full()
            .child(
                el().pin(Some(p[0]), Some(p[1]), None, None).size(p[2], p[3]).corners([corner(flush_left), corner(flush_right), r, r]).fill(liq)
                    .shadow(24.0, 64.0, Color { r: 0.0, g: 0.0, b: 0.0, a: 0.45 })
                    .live_rect(&st.tracks.body),
            )
            .child(el().pin(Some(n[0]), Some(n[1]), None, None).size(n[2], n[3]).corners([0.0, 0.0, NECK_W / 2.0, NECK_W / 2.0]).fill(liq).live_rect(&st.tracks.neck));
        // Each join is a square of liquid with a quarter circle cut from the corner away from the
        // neck: centre in unit coordinates of the square.
        let join = |x: f32, y: f32, centre: [f32; 2], o: &Tween<f32>| {
            let clear = Color { a: 0.0, ..liq };
            el().pin(Some(x), Some(y), None, None).size(JOIN, JOIN)
                .radial_gradient(centre, JOIN, [(0.0, clear), (1.0 - 1.0 / JOIN, clear), (1.0, liq)])
                .live_opacity(o)
        };
        root = root
            .child(join(left - JOIN, yb, [0.0, 1.0], &st.tracks.bar_joins))
            .child(join(right, yb, [1.0, 1.0], &st.tracks.bar_joins));
        if !flush_left {
            root = root.child(join(left - JOIN, p[1] - JOIN, [0.0, 0.0], &st.tracks.panel_joins));
        }
        if !flush_right {
            root = root.child(join(right, p[1] - JOIN, [1.0, 0.0], &st.tracks.panel_joins));
        }
        Some(root.key("liquid"))
    }
}

fn near(a: &Shape, b: &Shape) -> bool {
    a.flush == b.flush && (a.neck_x - b.neck_x).abs() < 0.5 && a.panel.iter().zip(b.panel).all(|(x, y)| (x - y).abs() < 0.5)
}
