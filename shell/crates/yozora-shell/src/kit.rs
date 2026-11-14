//! Shared pieces every screen is made of. Shapes, colours and type come from the look's tokens, so
//! a screen is written once and wears every look.

use rune::tok::TextStyle;
use rune::{Color, Edge, El, Spring, SpringPair, Transition, Vec2, col, icon, row, text};
use yozora_style::{Entrance, Fx, Motion, Tokens, glyph};

pub fn label(t: &Tokens, size: f32, weight: f32, s: impl Into<String>, color: Color) -> El {
    text(s.into()).font(TextStyle::new(size, weight).family(t.fonts.body)).color(color)
}

pub fn fig(t: &Tokens, size: f32, weight: f32, s: impl Into<String>, color: Color) -> El {
    text(s.into()).font(TextStyle::new(size, weight).family(t.fonts.fig).features("'tnum' 1")).color(color)
}

pub fn display(t: &Tokens, size: f32, s: impl Into<String>, color: Color) -> El {
    text(s.into()).font(TextStyle::new(size, 400.0).family(t.fonts.display)).color(color)
}

pub fn glyph_el(t: &Tokens, name: &str, size: f32, color: Color) -> El {
    icon(glyph(t.look, name), size).color(color)
}

pub fn cell(w: f32, h: f32, child: El) -> El {
    row().size(w, h).center().child(child)
}

/// How a panel arrives and leaves, per look. `anchor_x` is the icon it grows from, relative to
/// the panel's own left edge.
pub fn enter(t: &Tokens, fx: Fx, anchor_x: f32, panel_w: f32) -> Transition {
    let m = Motion::of(t.look, fx);
    if m.open == 0.0 {
        return Transition::none();
    }
    let ax = (anchor_x / panel_w).clamp(0.0, 1.0);
    let base = match m.entrance {
        Entrance::Bud => Transition::scale_anchor(0.5, Vec2 { x: ax, y: 0.0 }).allow_small_scale(),
        Entrance::Unfold => Transition::reveal(Edge::Top),
        Entrance::Scan => Transition::reveal(Edge::Top),
        Entrance::Iris => Transition::scale_anchor(0.6, Vec2 { x: ax, y: 0.0 }).allow_small_scale().combined(Transition::blur(6.0)),
    };
    base.combined(Transition::opacity()).springs(springs(&m))
}

/// The look's `Motion` as an enter/exit spring pair. Durations are the perceived time to settle;
/// the look's stiffness and damping set how much it overshoots, capped so a scaling panel never
/// swells past its neighbours. Phosphor's frame-stepped scan has no step control in the spring
/// model, so it takes a short critically damped spring instead.
fn springs(m: &Motion) -> SpringPair {
    let bounce = match m.spring {
        Some((k, c)) => Spring::from_stiffness_damping(k, c, 1.0).bounce.clamp(0.0, 0.22),
        None => 0.0,
    };
    SpringPair::new(Spring::new(m.open * 0.7, bounce), Spring::new(m.close * 0.6, 0.0))
}

/// Settles a panel's rows in one after another, the look's `row_stagger` apart. Rows keep their
/// place in layout; each starts a little low and rises as it fades in.
pub fn stagger(t: &Tokens, fx: Fx, panel: El) -> El {
    let m = Motion::of(t.look, fx);
    if m.row_stagger == 0.0 {
        return panel;
    }
    let rise = if t.look == yozora_style::Look::Phosphor { 0.0 } else { 8.0 };
    let first = m.row_stagger;
    panel.map_children(|i, c| {
        c.transition(Transition::offset(0.0, rise).combined(Transition::opacity()).springs(SpringPair::new(Spring::new(m.open * 0.6, 0.0), Spring::new(0.15, 0.0))))
            .enter_delay(first + m.row_stagger * i as f32)
    })
}

/// A bar popover's surface. In the liquid looks the drop under it is its body (`liquid`), so it
/// only holds the content. Content goes in as children.
pub fn panel(t: &Tokens, w: f32) -> El {
    if crate::liquid::applies(t) {
        return col().w(w).padding(18.0).gap(10.0).radius(t.radii.panel);
    }
    card(t, w)
}

/// A surface that draws its own body in every look, for what doesn't hang from the bar: the
/// launcher and toasts.
pub fn card(t: &Tokens, w: f32) -> El {
    let p = col().w(w).padding(18.0).gap(10.0).radius(t.radii.panel).fill(t.surf).border(1.0, t.rule);
    if t.look == yozora_style::Look::Phosphor || t.look == yozora_style::Look::Folio {
        p.border(1.0, t.ink)
    } else {
        p.shadow(24.0, 64.0, Color { r: 0.0, g: 0.0, b: 0.0, a: 0.45 })
    }
}

/// Title row at the top of a panel.
pub fn header(t: &Tokens, title: impl Into<String>, right: Option<El>) -> El {
    let r = row().w_full().h(48.0).center_y().px(8.0).gap(6.0)
        .child(display_or_title(t, title.into()).grow(1.0));
    match right {
        Some(e) => r.child(e),
        None => r,
    }
}

fn display_or_title(t: &Tokens, s: String) -> El {
    label(t, 22.0, 600.0, s, t.ink)
}

/// A small round icon button.
pub fn icon_button(t: &Tokens, key: &'static str, name: &str, on_press: impl Fn(&mut crate::stage::Stage) + 'static) -> El {
    cell(44.0, 44.0, glyph_el(t, name, 17.0, t.ink)).key(key).role("button").label(key).hit().radius(t.radii.pill).on_press(on_press)
}

/// A one-line section heading.
pub fn kicker(t: &Tokens, s: impl Into<String>) -> El {
    label(t, 12.0, 500.0, s, t.ink2).padding(10.0)
}
