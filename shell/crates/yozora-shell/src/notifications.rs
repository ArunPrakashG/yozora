//! The notification center, the toast that announces a new notification, and the volume key
//! display. All three read the notification and audio snapshots; none keeps history of its own.

use std::time::{SystemTime, UNIX_EPOCH};

use rune::{El, col, el, row, spacer};
use yozora_services::notifications::{Cmd, Notification};
use yozora_style::Tokens;

use crate::kit::{cell, fig, glyph_el, label, panel};
use crate::stage::Stage;

pub const W: f32 = 392.0;
const CARD_W: f32 = W - 36.0;

fn ago(seen: Option<u64>) -> String {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let Some(s) = seen else { return String::new() };
    match now.saturating_sub(s) {
        0..=59 => "now".into(),
        m @ 60..=3599 => format!("{} min", m / 60),
        h @ 3600..=86399 => format!("{} h", h / 3600),
        d => format!("{} d", d / 86400),
    }
}

/// The app's mark: its icon tile in the look's own shapes.
fn tile(t: &Tokens, n: &Notification) -> El {
    let name = if n.urgency >= 2 { "disk" } else if n.app.to_lowercase().contains("mail") { "mail" } else { "bell" };
    let (bg, ink) = if n.urgency >= 2 { (t.alert_bg, t.alert) } else { (t.fill, t.ink) };
    cell(40.0, 40.0, glyph_el(t, name, 20.0, ink)).radius(t.radii.ctl).fill(bg)
}

fn button(t: &Tokens, key: String, text: String, primary: bool, on: impl Fn(&mut Stage) + 'static) -> El {
    let (bg, ink) = if primary { (t.acc, t.on_acc) } else { (t.fill, t.ink) };
    row().h(32.0).px(14.0).center().radius(t.radii.chip).fill(bg).key(key).role("button").label(text.clone()).hit()
        .child(label(t, 13.0, 600.0, text, ink)).on_press(on)
}

fn card(t: &Tokens, n: &Notification, toast: bool) -> El {
    let id = n.id;
    let mut text = col().grow(1.0).gap(4.0);
    text = text.child(
        row().w_full().center_y().gap(8.0)
            .child(label(t, 15.0, 600.0, n.summary.clone(), t.ink).w(190.0).max_lines(1))
            .child(spacer())
            .child(if n.unread && !toast { el().size(7.0, 7.0).radius(t.radii.pill).fill(t.acc) } else { el().w(0.0) })
            .child(fig(t, 12.0, 400.0, ago(n.seen), t.ink2)),
    );
    if !n.body.is_empty() {
        text = text.child(label(t, 14.0, 400.0, n.body.clone(), t.ink2).w(CARD_W - 40.0 - 14.0 - 32.0).max_lines(3));
    }
    if !n.actions.is_empty() {
        let mut r = row().gap(8.0).pt(6.0);
        for (i, (key, name)) in n.actions.iter().cloned().enumerate() {
            r = r.child(button(t, format!("n{id}a{i}"), name, i == 0, move |s| {
                if let Some(x) = &s.sh.services.lock().unwrap().notifications {
                    x.send(Cmd::Action(id, key.clone()));
                    x.send(Cmd::Dismiss(id));
                }
            }));
        }
        text = text.child(r);
    }
    let surface = if toast { t.surf } else if n.urgency >= 2 { t.alert_bg } else { t.raised };
    let c = row().w(CARD_W).gap(14.0).padding(16.0).radius(t.radii.card).fill(surface).key(format!("n{id}")).role("button")
        .label(n.summary.clone()).hit()
        .child(tile(t, n))
        .child(text)
        .on_press(move |s: &mut Stage| {
            if let Some(x) = &s.sh.services.lock().unwrap().notifications {
                if toast { x.send(Cmd::MarkRead) }
                x.send(Cmd::Activate(id));
                x.send(Cmd::Dismiss(id));
            }
        });
    if toast {
        c.border(1.0, t.rule).shadow(18.0, 48.0, rune::Color { r: 0.0, g: 0.0, b: 0.0, a: 0.4 })
    } else {
        c
    }
}

pub fn view(s: &Stage, t: &Tokens) -> El {
    let (items, dnd) = s.sh.services.lock().unwrap().notifications.as_ref()
        .map(|n| { let g = n.read(); (g.state.items.clone(), g.state.dnd) })
        .unwrap_or_default();
    let moon = cell(44.0, 44.0, glyph_el(t, "moon", 17.0, if dnd { t.on_acc } else { t.ink })).radius(t.radii.pill)
        .fill(if dnd { t.acc } else { t.fill }).key("dnd").role("button").label("Do not disturb").hit()
        .on_press(move |s: &mut Stage| {
            if let Some(x) = &s.sh.services.lock().unwrap().notifications { x.send(Cmd::SetDnd(!dnd)) }
        });
    let clear = row().h(36.0).px(14.0).center().radius(t.radii.pill).fill(t.fill).key("clear").role("button").label("Clear all").hit()
        .child(label(t, 13.0, 500.0, "Clear all", t.ink))
        .on_press(|s: &mut Stage| {
            if let Some(x) = &s.sh.services.lock().unwrap().notifications { x.send(Cmd::ClearAll) }
        });
    let head = row().w_full().h(48.0).center_y().px(8.0).gap(6.0)
        .child(label(t, 22.0, 600.0, "Notifications", t.ink).grow(1.0))
        .child(moon)
        .child(if items.is_empty() { el().w(0.0) } else { clear });
    let mut list = col().w_full().gap(10.0);
    if items.is_empty() {
        list = list.child(
            col().w(CARD_W).h(140.0).center().gap(10.0)
                .child(glyph_el(t, "bell", 24.0, t.ink3))
                .child(label(t, 14.0, 400.0, if dnd { "Do Not Disturb is on" } else { "Nothing new" }, t.ink2)),
        );
    }
    for n in items.iter().take(6) {
        list = list.child(card(t, n, false));
    }
    panel(t, W).child(head).child(list)
}

/// The card that slides in when something arrives.
pub fn toast(t: &Tokens, n: &Notification) -> El {
    card(t, n, true)
}

/// The volume display: a speaker, a bar, the number.
pub fn osd(t: &Tokens, level: f32, muted: bool) -> El {
    let pct = (level * 100.0).round().clamp(0.0, 100.0);
    let fillw = 150.0 * (if muted { 0.0 } else { pct / 100.0 });
    row().w(240.0).h(46.0).px(16.0).gap(12.0).center_y().radius(t.radii.pill).fill(t.surf).border(1.0, t.rule)
        .shadow(14.0, 40.0, rune::Color { r: 0.0, g: 0.0, b: 0.0, a: 0.35 })
        .child(glyph_el(t, if muted || pct == 0.0 { "vol0" } else { "vol" }, 17.0, t.ink))
        .child(el().w(150.0).h(6.0).radius(3.0).fill(t.fill).child(el().w(fillw).h(6.0).radius(3.0).fill(t.acc)))
        .child(fig(t, 13.0, 500.0, if muted { "mute".into() } else { format!("{pct:.0}") }, t.ink2))
}
