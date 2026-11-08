//! Settings: a page list on the left, the page on the right. Appearance is where the look,
//! its time of day and the effects level are chosen; they apply as soon as they are pressed.

use rune::{Color, El, col, el, row};
use yozora_style::{Look, Tokens, tokens};

use crate::kit::{cell, glyph_el, label};
use crate::stage::Stage;

pub const W: f32 = 1080.0;
pub const H: f32 = 660.0;
const SIDEBAR: f32 = 247.0;

const PAGES: [(&str, &str); 8] = [
    ("Appearance", "sun"),
    ("Bar", "cc"),
    ("Notifications", "bell"),
    ("Launcher", "search"),
    ("Lock and idle", "lock"),
    ("Displays", "monitor"),
    ("Motion", "gauge"),
    ("About Yozora", "globe"),
];

fn sidebar(t: &Tokens, page: usize) -> El {
    let mut list = col().w_full().gap(4.0).padding(12.0).child(label(t, 13.0, 600.0, "Settings", t.ink2).padding(12.0));
    for (i, (name, glyph)) in PAGES.iter().enumerate() {
        let on = i == page;
        list = list.child(
            row().w_full().h(42.0).px(14.0).gap(14.0).center_y().radius(t.radii.ctl).fill(if on { t.fill } else { Color::TRANSPARENT })
                .key(format!("page{i}")).role("button").label(*name).hit()
                .child(glyph_el(t, glyph, 17.0, if on { t.ink } else { t.ink2 }))
                .child(label(t, 14.0, if on { 600.0 } else { 500.0 }, *name, if on { t.ink } else { t.ink2 }))
                .on_press(move |s: &mut Stage| s.settings_page = i),
        );
    }
    col().w(SIDEBAR).h_full().child(list).border_sides([0.0, 0.0, 1.0, 0.0], t.rule)
}

/// A miniature of a look: its sky, its bar and one open panel, in its own colours.
fn preview(look: Look) -> El {
    let t = tokens(look);
    let line = |w: f32, c: Color| el().w(w).h(5.0).radius(t.radii.pill.min(3.0)).fill(c);
    el().w_full().h(140.0).radius(t.radii.card).fill(t.bg).clip_rounded(t.radii.card)
        .child(el().pin(Some(12.0), Some(8.0), Some(12.0), None).h(14.0).radius(t.radii.pill.min(7.0)).fill(t.surf).border(1.0, t.rule))
        .child(
            col().pin(None, Some(28.0), Some(26.0), None).w(140.0).h(100.0).padding(14.0).gap(8.0).radius(t.radii.card.min(14.0)).fill(t.surf).border(1.0, t.rule)
                .child(line(90.0, t.ink2)).child(line(60.0, t.ink3)).child(line(110.0, t.ink2)).child(line(50.0, t.ink3)),
        )
}

fn style_card(t: &Tokens, look: Look, on: bool, tag: &str) -> El {
    let slug = look.slug();
    let card = col().w(366.0).gap(14.0).padding(8.0).radius(t.radii.card).fill(t.raised).key(format!("look-{slug}")).role("button").label(look.name()).hit()
        .child(preview(look))
        .child(
            row().px(8.0).pb(4.0).gap(10.0).center_y()
                .child(label(t, 15.0, 600.0, look.name(), t.ink))
                .child(label(t, 13.0, 400.0, tag, t.ink2)),
        )
        .on_press(move |s: &mut Stage| {
            // Daybreak is Surface tension in its day sky.
            if look == Look::Daybreak {
                crate::config::set(&s.sh, "time", "day");
                crate::config::set(&s.sh, "look", "tension");
            } else {
                crate::config::set(&s.sh, "look", slug);
            }
        });
    if on { card.border(2.0, t.acc) } else { card }
}

fn segmented(t: &Tokens, key: &'static str, options: &[(&'static str, &'static str)], current: &str, width: f32) -> El {
    let mut r = row().w(width).h(40.0).padding(3.0).radius(t.radii.pill).fill(t.fill);
    for (value, name) in options {
        let on = *value == current;
        let value = *value;
        let seg = row().grow(1.0).h_full().center().radius(t.radii.pill).fill(if on { t.raised } else { Color::TRANSPARENT });
        r = r.child(
            (if on { seg.border(1.0, t.rule) } else { seg })
                .key(format!("{key}-{value}")).role("button").label(*name).hit()
                .child(label(t, 13.0, if on { 600.0 } else { 500.0 }, *name, if on { t.ink } else { t.ink2 }))
                .on_press(move |s: &mut Stage| crate::config::set(&s.sh, key, value)),
        );
    }
    r
}

fn appearance(s: &Stage, t: &Tokens) -> El {
    let c = s.sh.config();
    let tension_on = |l: Look| match l {
        Look::Tension => c.look == "tension" && c.time != "day",
        Look::Daybreak => c.look == "tension" && c.time == "day",
        other => c.look == other.slug(),
    };
    let cards = col().gap(16.0)
        .child(row().gap(16.0)
            .child(style_card(t, Look::Tension, tension_on(Look::Tension), "Liquid. Quiet. Alive."))
            .child(style_card(t, Look::Folio, tension_on(Look::Folio), "Printed. Plain. Considered.")))
        .child(row().gap(16.0)
            .child(style_card(t, Look::Phosphor, tension_on(Look::Phosphor), "Lit. Terse. Mechanical."))
            .child(style_card(t, Look::Prism, tension_on(Look::Prism), "Light. Clear. Precise.")));
    let time = col().gap(10.0).child(label(t, 13.0, 500.0, "Time of day", t.ink2))
        .child(segmented(t, "time", &[("night", "Night"), ("day", "Daybreak"), ("follow", "Follow the sun")], &c.time, 400.0));
    let fx = col().gap(10.0).child(label(t, 13.0, 500.0, "Effects", t.ink2))
        .child(segmented(t, "fx", &[("full", "Full"), ("calm", "Calm"), ("still", "Still")], &c.fx, 300.0));
    col().gap(18.0)
        .child(label(t, 22.0, 600.0, "Appearance", t.ink))
        .child(label(t, 13.0, 500.0, "Style", t.ink2))
        .child(cards)
        .child(row().gap(32.0).child(time).child(fx))
}

pub fn view(s: &Stage, t: &Tokens) -> El {
    let page = s.settings_page.min(PAGES.len() - 1);
    let body = if page == 0 { appearance(s, t) } else { crate::settings_pages::view(s, t, PAGES[page].0) };
    let _ = cell;
    row().w(W).h(H).radius(t.radii.panel).fill(t.surf).border(1.0, t.rule).clip_rounded(t.radii.panel)
        .shadow(24.0, 64.0, Color { r: 0.0, g: 0.0, b: 0.0, a: 0.45 })
        .child(sidebar(t, page))
        .child(col().grow(1.0).h_full().padding(28.0).child(body))
}
