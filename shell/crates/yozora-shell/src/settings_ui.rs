//! The controls Settings pages are made of: headings, cards of rows, buttons, switches, segmented
//! choices and pills. Shapes, colours and type come from the look's tokens like every other screen.

use std::rc::Rc;

use rune::{Color, El, col, el, row};
use yozora_style::{Look, Tokens};

use crate::kit::{glyph_el, label};
use crate::stage::Stage;

/// A page's title and the line under it.
pub fn heading(t: &Tokens, title: &str, line: &str) -> El {
    col().gap(6.0).child(label(t, 22.0, 600.0, title, t.ink)).child(label(t, 13.0, 400.0, line, t.ink2).wrap())
}

/// A small heading over a group of rows, with an optional line of explanation.
pub fn group(t: &Tokens, title: &str, note: Option<&str>) -> El {
    let g = col().w_full().gap(4.0).pt(6.0).child(label(t, 12.0, 600.0, title.to_uppercase(), t.ink2));
    match note {
        Some(n) => g.child(label(t, 12.0, 400.0, n, t.ink3).wrap()),
        None => g,
    }
}

/// The surface rows sit on.
pub fn card(t: &Tokens) -> El {
    col().w_full().radius(t.radii.card).fill(t.fill)
}

/// One setting: its name and help on the left, the control on the right. `first` leaves out the
/// rule above it.
pub fn setting(t: &Tokens, first: bool, name: &str, help: Option<String>, tag: Option<El>, control: El) -> El {
    let mut text = col().grow(1.0).min_w(0.0).gap(3.0).child(label(t, 14.0, 500.0, name, t.ink).wrap());
    if let Some(h) = help.filter(|h| !h.is_empty()) {
        text = text.child(label(t, 12.0, 400.0, h, t.ink2).wrap());
    }
    if let Some(tag) = tag {
        text = text.child(tag);
    }
    let r = row().w_full().min_h(58.0).px(18.0).py(10.0).gap(16.0).items_center().child(text).child(control.shrink());
    if first { r } else { r.border_sides([1.0, 0.0, 0.0, 0.0], t.rule) }
}

/// A line of small print in the file's own voice.
pub fn mono(t: &Tokens, s: impl Into<String>, color: Color) -> El {
    crate::kit::fig(t, 11.0, 400.0, s, color)
}

pub fn button(t: &Tokens, key: String, glyph: Option<&str>, text: &str, primary: bool, f: impl Fn(&mut Stage) + 'static) -> El {
    let (bg, ink) = if primary { (t.acc, t.on_acc) } else { (t.fill, t.ink) };
    let mut b = row().h(36.0).px(14.0).gap(8.0).center().radius(t.radii.ctl).fill(bg).key(key).role("button").label(text).hit();
    if !primary {
        b = b.border(1.0, t.rule).hover(t.raised);
    }
    if let Some(g) = glyph {
        b = b.child(glyph_el(t, g, 15.0, ink));
    }
    b.child(label(t, 13.0, 600.0, text, ink)).on_press(f)
}

/// A square icon button, for the small actions on a row.
pub fn tool(t: &Tokens, key: String, glyph: &str, name: &str, enabled: bool, f: impl Fn(&mut Stage) + 'static) -> El {
    let b = row().size(30.0, 30.0).center().radius(t.radii.chip.min(10.0)).fill(t.fill).key(key).role("button").label(name)
        .child(glyph_el(t, glyph, 15.0, if enabled { t.ink } else { t.ink3 }));
    if enabled { b.hit().hover(t.raised).on_press(f) } else { b.opacity(0.5) }
}

/// A switch with a hit area around it.
pub fn toggle(t: &Tokens, key: String, name: &str, on: bool, f: impl Fn(&mut Stage) + 'static) -> El {
    row().size(48.0, 40.0).center().child(crate::wifi::switch(t, on)).key(key).role("switch").label(name).hit().on_press(f)
}

/// Choices side by side in one capsule, the way Appearance lays out its time of day.
pub fn segmented(t: &Tokens, key: &str, options: &[String], current: Option<usize>, width: f32, f: impl Fn(&mut Stage, usize) + 'static) -> El {
    let f: Rc<dyn Fn(&mut Stage, usize)> = Rc::new(f);
    let mut r = row().w(width).h(40.0).padding(3.0).radius(t.radii.pill).fill(t.fill);
    for (i, name) in options.iter().enumerate() {
        let on = current == Some(i);
        let seg = row().grow(1.0).h_full().center().radius(t.radii.pill).fill(if on { t.raised } else { Color::TRANSPARENT });
        let f = f.clone();
        r = r.child(
            (if on { seg.border(1.0, t.rule) } else { seg })
                .key(format!("{key}-{i}")).role("button").label(name.as_str()).hit()
                .child(label(t, 13.0, if on { 600.0 } else { 500.0 }, name.as_str(), if on { t.ink } else { t.ink2 }).truncate())
                .on_press(move |s: &mut Stage| f(s, i)),
        );
    }
    r
}

/// Many or long choices, wrapped over several lines.
pub fn pills(t: &Tokens, key: &str, options: &[String], current: Option<usize>, max_w: f32, f: impl Fn(&mut Stage, usize) + 'static) -> El {
    let f: Rc<dyn Fn(&mut Stage, usize)> = Rc::new(f);
    let mut r = row().max_w(max_w).flex_wrap().gap(6.0).justify_end();
    for (i, name) in options.iter().enumerate() {
        let on = current == Some(i);
        let f = f.clone();
        let (bg, ink) = if on { (t.acc, t.on_acc) } else { (t.fill, t.ink2) };
        r = r.child(
            row().h(30.0).px(12.0).center().radius(t.radii.pill).fill(bg).key(format!("{key}-{i}")).role("button").label(name.as_str()).hit()
                .child(label(t, 12.0, 600.0, name.as_str(), ink))
                .on_press(move |s: &mut Stage| f(s, i)),
        );
    }
    r
}

/// A message above a page: something to know, or something that went wrong.
pub fn notice(t: &Tokens, title: &str, body: impl Into<String>, bad: bool) -> El {
    let tint = if bad { t.alert } else { t.ink2 };
    row().w_full().padding(14.0).gap(12.0).radius(t.radii.card).fill(if bad { t.alert_bg } else { t.fill })
        .child(el().pt(1.0).child(glyph_el(t, if bad { "alert" } else { "check" }, 17.0, tint)))
        .child(
            col().grow(1.0).min_w(0.0).gap(3.0)
                .child(label(t, 13.0, 600.0, title, t.ink).wrap())
                .child(label(t, 12.0, 400.0, body, t.ink2).wrap()),
        )
}

/// The little capsule next to a page title: a dot and a few words.
pub fn status_pill(t: &Tokens, dot: Option<Color>, text: impl Into<String>) -> El {
    let mut r = row().h(28.0).px(12.0).gap(8.0).center_y().radius(t.radii.pill).fill(t.fill).border(1.0, t.rule);
    if let Some(c) = dot {
        r = r.child(el().size(6.0, 6.0).radius(3.0).fill(c));
    }
    r.child(label(t, 12.0, 500.0, text, t.ink2))
}

/// The chosen item of a look-specific list: kept for the sidebar and tabs, which mark the page
/// you are on in each look's own way.
pub fn on_mark(t: &Tokens, e: El) -> El {
    match t.look {
        Look::Folio => e.border_sides([0.0, 0.0, 0.0, 3.0], t.acc),
        Look::Prism => e.shadow(1.0, 4.0, Color { a: 0.12, ..t.ink }),
        _ => e,
    }
}
