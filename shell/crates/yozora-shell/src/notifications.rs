//! The notification center, the toasts that announce new notifications, and the volume key
//! display. The daemon keeps the history and the toast clocks; this draws what it serves.
//!
//! The center lists pinned notifications first, then one group per app in order of its newest,
//! then the snoozed ones. A group of three or more collapses into a stack until it is opened.

mod toasts;

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::{Local, TimeZone};
use rune::{Color, El, Image, ScrollSpec, SurfaceFit, col, el, row};
use yozora_services::notifications::{Cmd, Daemon, Notification, State};
use yozora_style::{Look, Tokens};

use crate::kit::{self, cell, fig, glyph_el, label};
use crate::shared::Shared;
use crate::stage::Stage;
use crate::wifi::{head, section, shell, text_el};

pub use toasts::{on_key, sync, toasts};

pub const W: f32 = 400.0;
/// A group this long collapses into a stack.
const STACK_AT: usize = 3;
/// What the snooze button on a card asks for.
const SNOOZE_MIN: u32 = 60;
/// The tallest the list gets before it scrolls.
const LIST_H: f32 = 560.0;
const HIDDEN: &str = "Hidden while you share your screen";

/// What the notification screens remember between frames.
#[derive(Default)]
pub struct Ui {
    /// Groups opened out of their stack, by key.
    open_groups: HashSet<String>,
    /// Toasts under the pointer.
    hovered: HashSet<u32>,
    /// The notification a reply is being typed to.
    reply: Option<(u32, String)>,
    /// The toasts the daemon was last told to hold.
    held: Vec<u32>,
    /// The draining line of each toast on screen.
    lines: HashMap<u32, toasts::Line>,
}

pub(crate) fn send(s: &Stage, cmd: Cmd) {
    if let Some(n) = &s.sh.services.lock().unwrap().notifications {
        n.send(cmd);
    }
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// "now", "5 min", "3 h", then the weekday, then the date.
pub(crate) fn when(seen: Option<u64>) -> String {
    let Some(seen) = seen.filter(|s| *s > 0) else { return String::new() };
    let s = now().saturating_sub(seen);
    match s {
        0..60 => "now".into(),
        60..3600 => format!("{} min", s / 60),
        3600..86400 => format!("{} h", s / 3600),
        _ => match Local.timestamp_opt(seen as i64, 0).single() {
            Some(t) if s < 6 * 86400 => t.format("%a").to_string(),
            Some(t) => t.format("%-d %b").to_string(),
            None => format!("{} d", s / 86400),
        },
    }
}

fn clock(at: u64) -> String {
    Local.timestamp_opt(at as i64, 0).single().map(|t| t.format("%H:%M").to_string()).unwrap_or_default()
}

/// The glyph for an app that sent no icon, guessed from its name.
fn guess(n: &Notification) -> &'static str {
    if n.urgency >= 2 {
        return "alert";
    }
    let k = format!("{} {}", n.app, n.desktop_entry).to_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| k.contains(w));
    if has(&["mail", "signal", "message", "chat", "telegram", "discord", "slack", "whatsapp", "element"]) {
        "mail"
    } else if has(&["screenshot", "grim", "flameshot", "spectacle"]) {
        "shot"
    } else if has(&["calendar", "clock", "alarm", "remind"]) {
        "clock"
    } else if has(&["download", "transmission", "torrent"]) {
        "down"
    } else if has(&["firefox", "chrom", "browser", "brave", "zen"]) {
        "globe"
    } else if has(&["music", "spotify", "player"]) {
        "music"
    } else if has(&["terminal", "kitty", "alacritty", "foot"]) {
        "term"
    } else if has(&["bluetooth"]) {
        "bt"
    } else if has(&["network", "wifi", "wi-fi"]) {
        "wifi"
    } else if has(&["battery", "power"]) {
        "battery"
    } else if has(&["backup", "disk", "system", "update", "package", "pacman"]) {
        "disk"
    } else {
        "bell"
    }
}

/// Keeps decoded pictures alive for as long as they are drawn.
fn held(img: Image) {
    static HELD: Mutex<Vec<Image>> = Mutex::new(Vec::new());
    let mut h = HELD.lock().unwrap();
    if h.len() > 96 {
        h.clear();
    }
    h.push(img);
}

/// `base` filled with the picture at `path`.
fn picture(sh: &Arc<Shared>, base: El, path: &Path, size: f32, scale: f32, fit: SurfaceFit) -> El {
    let img = crate::media::images(sh).file(path, size, scale);
    let e = base.pixels_fit(img.pixels(), fit);
    held(img);
    e
}

/// The colours a card writes in: its own, or inverted for a critical one in the looks that invert.
#[derive(Clone, Copy)]
pub(crate) struct Ink {
    pub main: Color,
    pub dim: Color,
    pub faint: Color,
}

pub(crate) fn ink(t: &Tokens, critical: bool) -> Ink {
    let a = |c: Color, a: f32| Color { a, ..c };
    match (t.look, critical) {
        (Look::Folio, true) => Ink { main: t.surf, dim: a(t.surf, 0.72), faint: a(t.surf, 0.5) },
        (Look::Phosphor, true) => Ink { main: t.alert, dim: a(t.alert, 0.75), faint: a(t.alert, 0.5) },
        _ => Ink { main: t.ink, dim: t.ink2, faint: t.ink3 },
    }
}

/// A button: filled in the look's accent when primary.
pub(crate) fn btn(t: &Tokens, text: &str, primary: bool, h: f32) -> El {
    let (fg, bg) = if primary { (t.on_acc, t.acc) } else { (t.ink, t.raised) };
    match t.look {
        Look::Folio => {
            let b = row().h(h).px(14.0).center().radius(t.radii.ctl).child(text_el(t, 11.0, 500.0, text.to_uppercase(), 0.9, false).color(fg).truncate());
            if primary { b.fill(bg) } else { b.border(1.0, t.ink).hover(t.fill) }
        }
        Look::Phosphor => {
            let b = row().h(h).px(10.0).center().child(label(t, 13.0, 400.0, format!("[{}]", text.to_lowercase()), if primary { t.on_ink } else { t.ink }).truncate());
            if primary { b.fill(t.ink) } else { b.border(1.0, t.ink).hover(t.fill) }
        }
        Look::Prism => {
            let b = row().h(h).px(if primary { 18.0 } else { 16.0 }).center().radius(t.radii.pill).child(label(t, 13.0, if primary { 600.0 } else { 500.0 }, text, fg).truncate());
            if primary {
                b.fill(bg).shadow(6.0, 16.0, Color { r: 0.18, g: 0.36, b: 1.0, a: 0.35 })
            } else {
                b.fill(Color { a: 0.9, ..Color::WHITE }).shadow(1.0, 3.0, Color { r: 0.16, g: 0.2, b: 0.47, a: 0.12 })
            }
        }
        _ => {
            let b = row().h(h).px(if primary { 18.0 } else { 16.0 }).center().radius(t.radii.pill).child(label(t, 13.0, if primary { 600.0 } else { 500.0 }, text, fg).truncate());
            b.fill(bg).hover(if primary { t.acc } else { t.fill })
        }
    }
}

/// The square holding an app's icon, `size` wide.
pub(crate) fn tile(sh: &Arc<Shared>, t: &Tokens, n: &Notification, size: f32, hidden: bool) -> El {
    let critical = n.urgency >= 2;
    let square = matches!(t.look, Look::Folio | Look::Phosphor);
    let radius = if square { 0.0 } else { size * 0.3 };
    let ik = ink(t, critical);
    let base = cell(size, size, el()).radius(radius);
    let base = match (t.look, critical) {
        (Look::Folio, true) => base.border(1.0, t.surf),
        (Look::Folio, false) => base.border(1.0, t.ink),
        (Look::Phosphor, true) => base.border(1.0, t.alert),
        (Look::Phosphor, false) => base.border(1.0, t.ink),
        (Look::Prism, true) => base.fill(Color { a: 0.12, ..t.alert }),
        (Look::Prism, false) => base.fill(Color { a: 0.95, ..Color::WHITE }).shadow(1.0, 3.0, Color { r: 0.16, g: 0.2, b: 0.47, a: 0.12 }),
        (_, true) => base.fill(Color { a: 0.16, ..t.alert }),
        (_, false) => base.fill(t.fill),
    };
    let glyph_color = if critical && !square { t.alert } else { ik.main };
    let inner = size * 0.55;
    // An app's progress fills the tile from the bottom, under its icon.
    let base = match n.progress.filter(|_| !hidden) {
        Some(p) => {
            let rise = size * f32::from(p.min(100)) / 100.0;
            let fill = if square { Color { a: 0.28, ..t.ink } } else { Color { a: 0.4, ..t.acc } };
            base.clip_rounded(radius).child(el().pin(Some(0.0), None, Some(0.0), Some(0.0)).h(rise).fill(fill).key("rise"))
        }
        None => base,
    };
    match (&n.icon_path, hidden) {
        (Some(path), false) => {
            let icon = picture(sh, el().size(inner, inner), path, inner, 2.0, SurfaceFit::Fit);
            base.child(icon)
        }
        (_, true) => base.child(glyph_el(t, "eye", inner * 0.9, glyph_color)),
        _ => base.child(glyph_el(t, guess(n), inner * 0.9, glyph_color)),
    }
}

/// A small round control, like the tools on a card.
pub(crate) fn tool(t: &Tokens, key: String, name: &str, glyph: &str, color: Color, f: impl Fn(&mut Stage) + 'static) -> El {
    cell(26.0, 26.0, glyph_el(t, glyph, 13.0, color)).key(key).role("button").label(name.to_string()).hit().radius(t.radii.pill).hover(t.fill).on_press(f)
}

/// Buttons for a notification's actions, the first one primary. A reply goes first when the app
/// takes one. At most three.
pub(crate) fn actions(s: &Stage, t: &Tokens, n: &Notification, h: f32) -> Option<El> {
    let id = n.id;
    let mut items: Vec<(String, Option<String>)> = n.actions.iter().take(3).map(|(k, l)| (l.clone(), Some(k.clone()))).collect();
    if n.reply.is_some() {
        items.insert(0, ("Reply".into(), None));
        items.truncate(3);
    }
    if items.is_empty() {
        return None;
    }
    let _ = s;
    let mut r = row().w_full().gap(8.0);
    for (i, (text, key)) in items.into_iter().enumerate() {
        let b = btn(t, &text, i == 0, h).grow(1.0).min_w(0.0).key(format!("act{id}-{i}")).role("button").label(text.clone()).hit();
        r = r.child(b.on_press(move |s: &mut Stage| match &key {
            Some(k) => send(s, Cmd::Action(id, k.clone())),
            None => toasts::start_reply(s, id),
        }));
    }
    Some(r)
}

/// The text field a reply is typed in, and its send button.
pub(crate) fn reply_row(t: &Tokens, id: u32, text: &str, hint: &str) -> El {
    let square = matches!(t.look, Look::Folio | Look::Phosphor);
    let mut field = row().grow(1.0).min_w(0.0).h(40.0).px(12.0).gap(2.0).center_y().radius(t.radii.ctl);
    field = match t.look {
        Look::Folio | Look::Phosphor => field.border(1.0, t.ink),
        Look::Prism => field.fill(Color { a: 0.9, ..Color::WHITE }).shadow(1.0, 3.0, Color { r: 0.16, g: 0.2, b: 0.47, a: 0.12 }),
        _ => field.fill(t.raised),
    };
    let caret = el().w(if t.look == Look::Phosphor { 8.0 } else { 2.0 }).h(18.0).radius(if square { 0.0 } else { 1.0 }).fill(t.acc);
    field = if text.is_empty() {
        field.child(caret).child(label(t, 14.0, 400.0, hint, t.ink3).truncate())
    } else {
        field.child(label(t, 14.0, 400.0, text, t.ink).truncate()).child(caret)
    };
    let send_btn = cell(40.0, 40.0, glyph_el(t, "arrow", 16.0, t.on_acc)).key(format!("send{id}")).role("button").label("Send reply").hit()
        .radius(if square { 0.0 } else { t.radii.pill }).fill(t.acc).on_press(move |s: &mut Stage| toasts::send_reply(s, id));
    row().w_full().gap(8.0).center_y().child(field).child(send_btn)
}

/// The app's name and age, over a notification's text.
fn read_state(s: &Stage) -> (State, Option<String>, bool) {
    let svc = s.sh.services.lock().unwrap();
    let sharing = svc.hyprland.as_ref().is_some_and(|h| h.read().state.sharing);
    match svc.notifications.as_ref() {
        Some(n) => {
            let g = n.read();
            (g.state.clone(), g.error.clone(), sharing)
        }
        None => (State::default(), Some("The notification service did not start.".into()), sharing),
    }
}

struct Group<'a> {
    /// What remembers whether it is open.
    key: String,
    title: String,
    items: Vec<&'a Notification>,
}

/// Pinned first, then one group per app in order of its newest. Items stay newest first.
fn groups<'a>(items: &[&'a Notification]) -> Vec<Group<'a>> {
    let mut pinned = Group { key: "pinned".into(), title: "Pinned".into(), items: Vec::new() };
    let mut rest: Vec<Group> = Vec::new();
    for &n in items {
        if n.pinned {
            pinned.items.push(n);
            continue;
        }
        let key = format!("app:{}", n.app);
        match rest.iter_mut().find(|g| g.key == key) {
            Some(g) => g.items.push(n),
            None => rest.push(Group { key, title: if n.app.is_empty() { "Notifications".into() } else { n.app.clone() }, items: vec![n] }),
        }
    }
    [pinned].into_iter().chain(rest).filter(|g| !g.items.is_empty()).collect()
}

pub fn view(s: &Stage, t: &Tokens) -> El {
    let (_, fx) = s.sh.look();
    let (state, error, sharing) = read_state(s);
    let yozora = state.daemon == Daemon::Yozora;
    let dnd = state.dnd;
    let now = now();
    let (snoozed, live): (Vec<&Notification>, Vec<&Notification>) = state.items.iter().partition(|n| n.snoozed_until > now);

    let square = matches!(t.look, Look::Folio | Look::Phosphor);
    let moon = {
        let (fg, bg) = if dnd { (t.on_ink, Some(t.on)) } else { (t.ink, None) };
        let e = cell(44.0, 44.0, glyph_el(t, "moon", 17.0, fg)).key("dnd").role("switch").label(if dnd { "Do Not Disturb, on" } else { "Do Not Disturb, off" }).hit()
            .radius(if square { t.radii.ctl } else { t.radii.pill }).on_press(move |s: &mut Stage| send(s, Cmd::SetDnd(!dnd)));
        match bg {
            Some(c) => e.fill(c),
            None => e.hover(t.fill),
        }
    };
    let clear = btn(t, "Clear all", false, 34.0).key("clear").role("button").label("Clear all notifications").hit().on_press(|s: &mut Stage| send(s, Cmd::ClearAll));
    let mut controls = row().gap(6.0).center_y().child(moon);
    if error.is_none() && live.iter().any(|n| !n.pinned) {
        controls = controls.child(clear);
    }
    let mut p = shell(t, W, "notifications").child(head(t, "Notifications", Some(controls)));

    if let Some(e) = error {
        return p.child(empty(t, "No notification server", Some(&e)));
    }
    if dnd {
        p = p.child(dnd_banner(t));
    }
    if live.is_empty() && snoozed.is_empty() {
        return p.child(empty(t, "No notifications", None));
    }

    let mut list = col().w_full().gap(8.0);
    for (g, group) in groups(&live).into_iter().enumerate() {
        list = list.child(group_view(s, t, g, &group, sharing, yozora));
    }
    if !snoozed.is_empty() {
        let mut g = col().w_full().gap(8.0).child(section(t, "Snoozed", None));
        for n in snoozed {
            g = g.child(card(s, t, n, &Mode { sharing, yozora, snoozed: true, stacked: false }, None));
        }
        list = list.child(g);
    }
    let total = live.len() + 1;
    let list = kit::stagger(t, fx, list);
    // Past a handful, the list scrolls inside a fixed height.
    p.child(if total > 4 { list.h(LIST_H).scroll(ScrollSpec { stick_bottom: false }) } else { list })
}

struct Mode {
    sharing: bool,
    yozora: bool,
    snoozed: bool,
    /// The first card of a collapsed stack: it says how many more there are.
    stacked: bool,
}

fn group_view(s: &Stage, t: &Tokens, index: usize, group: &Group, sharing: bool, yozora: bool) -> El {
    let pinned = group.key == "pinned";
    let long = group.items.len() >= STACK_AT && !pinned;
    let stacked = long && !s.notes.open_groups.contains(&group.key);
    let right = if long {
        if stacked {
            Some(fig(t, 12.0, 500.0, group.items.len().to_string(), t.ink3))
        } else {
            let key = group.key.clone();
            Some(
                label(t, 12.0, 500.0, "Show less", t.ink2).key(format!("less{index}")).role("button").label("Show less").hit().px(8.0).py(4.0).radius(t.radii.chip).hover(t.fill)
                    .on_press(move |s: &mut Stage| {
                        s.notes.open_groups.remove(&key);
                    }),
            )
        }
    } else {
        None
    };
    let mut g = col().w_full().gap(8.0).child(section(t, &group.title, right));
    if stacked {
        let more = group.items.len() - 1;
        let key = group.key.clone();
        let open = Box::new(move |s: &mut Stage| {
            s.notes.open_groups.insert(key.clone());
        });
        let first = card(s, t, group.items[0], &Mode { sharing, yozora, snoozed: false, stacked: true }, Some((format!("{more} more from {}", group.title), open)));
        g = g.child(stack(t, first));
    } else {
        for n in &group.items {
            g = g.child(card(s, t, n, &Mode { sharing, yozora, snoozed: false, stacked: false }, None));
        }
    }
    g
}

/// A card with the edges of the ones under it showing below.
fn stack(t: &Tokens, top: El) -> El {
    let under = |inset: f32, bottom: f32, alpha: f32| {
        let e = el().pin(Some(inset), None, Some(inset), Some(bottom)).h(24.0);
        match t.look {
            Look::Folio => e.border_sides([0.0, 0.0, 1.0, 0.0], t.rule),
            Look::Phosphor => e.border_sides([0.0, 1.0, 1.0, 1.0], t.rule),
            Look::Prism => e.radius(16.0).fill(Color { a: 0.5 * alpha.max(0.6), ..Color::WHITE }),
            _ => e.radius(14.0).fill(Color { a: t.raised.a * alpha, ..t.raised }),
        }
    };
    col().w_full().pb(12.0).child(under(16.0, 0.0, 0.6)).child(under(8.0, 6.0, 1.0)).child(top)
}

/// One notification in the center. `more` makes it the top of a stack, with its opener.
fn card(s: &Stage, t: &Tokens, n: &Notification, mode: &Mode, more: Option<(String, Box<dyn Fn(&mut Stage)>)>) -> El {
    let id = n.id;
    let critical = n.urgency >= 2;
    let hidden = mode.sharing;
    let ik = ink(t, critical);
    let square = matches!(t.look, Look::Folio | Look::Phosphor);

    let when_text = if mode.snoozed { format!("until {}", clock(n.snoozed_until)) } else { when(n.seen) };
    let time = match n.progress.filter(|_| !hidden && !mode.snoozed) {
        Some(p) => crate::kit::fig(t, 12.0, 600.0, format!("{p}%"), t.acc),
        None => label(t, 12.0, 500.0, when_text, if mode.snoozed { t.acc } else { ik.dim }),
    };

    let summary = if hidden {
        HIDDEN.to_string()
    } else if n.summary.is_empty() {
        n.app.clone()
    } else {
        n.summary.clone()
    };
    let mut top = row().w_full().gap(8.0).center_y().min_h(22.0)
        .child(label(t, 15.0, if hidden { 400.0 } else { 600.0 }, summary, if hidden { ik.dim } else { ik.main }).truncate().grow(1.0).min_w(0.0));
    if n.unread && !mode.snoozed {
        top = top.child(el().size(7.0, 7.0).radius(if square { 0.0 } else { 3.5 }).fill(t.acc).key(format!("unread{id}")));
    }
    top = top.child(time.key(format!("time{id}")).group_opacity(1.0, 0.0));

    let mut tools = row().gap(2.0).center_y().px(2.0).radius(t.radii.pill).fill(card_bg_under(t));
    if mode.yozora && !mode.snoozed {
        let pinned = n.pinned;
        tools = tools
            .child(tool(t, format!("pin{id}"), if pinned { "Unpin" } else { "Pin" }, "pin", if pinned { t.acc } else { ik.dim }, move |s: &mut Stage| send(s, Cmd::Pin(id, !pinned))))
            .child(tool(t, format!("snooze{id}"), "Snooze for an hour", "clock", ik.dim, move |s: &mut Stage| send(s, Cmd::Snooze(id, SNOOZE_MIN))));
    }
    if mode.yozora && mode.snoozed {
        tools = tools.child(tool(t, format!("wake{id}"), "Bring back now", "bell", ik.dim, move |s: &mut Stage| send(s, Cmd::Snooze(id, 0))));
    }
    tools = tools.child(tool(t, format!("dismiss{id}"), "Dismiss", "x", ik.dim, move |s: &mut Stage| send(s, Cmd::Dismiss(id))));
    let tools = tools.pin(None, Some(10.0), Some(10.0), None).group_opacity(0.0, 1.0);

    let mut text = col().grow(1.0).min_w(0.0).gap(5.0).child(top);
    if !hidden && !mode.snoozed && !n.body.is_empty() {
        text = text.child(label(t, 14.0, 400.0, n.body.clone(), ik.dim).wrap().max_lines(2));
    }
    if !hidden && !mode.snoozed && let Some(a) = actions(s, t, n, 32.0) {
        text = text.child(el().h(4.0)).child(a);
    }
    if !hidden && !mode.snoozed && let Some((id2, txt)) = s.notes.reply.as_ref().map(|(i, x)| (*i, x.as_str())) && id2 == id {
        text = text.child(el().h(4.0)).child(reply_row(t, id, txt, n.reply.as_deref().unwrap_or("Reply")));
    }
    if let Some((line, open)) = more {
        text = text.child(
            label(t, 12.0, 500.0, line.clone(), ik.dim).key(format!("more{id}")).role("button").label(line).hit().mt(2.0).self_start().radius(t.radii.chip).hover(t.fill).on_press(move |s: &mut Stage| open(s)),
        );
    }

    let mut body = row().w_full().gap(14.0).items_start().child(tile(&s.sh, t, n, 32.0, hidden)).child(text);
    if n.preview && !hidden && !mode.snoozed && let Some(path) = &n.image {
        let thumb = el().size(64.0, 40.0).radius(if square { 0.0 } else { 8.0 });
        body = body.child(picture(&s.sh, thumb, path, 64.0, 2.0, SurfaceFit::Fill));
    }

    let mut c = col().w_full().hover_group().padding(18.0).radius(t.radii.card).child(body).child(tools);
    c = match (t.look, critical) {
        (Look::Folio, true) => c.fill(t.ink).radius(0.0),
        (Look::Folio, false) => c.border_sides([1.0, 0.0, 0.0, 0.0], t.rule).radius(0.0).px(10.0),
        (Look::Phosphor, true) => c.border(1.0, t.alert),
        (Look::Phosphor, false) => c.border(1.0, t.rule),
        (Look::Prism, true) => c.fill(t.alert_bg),
        (Look::Prism, false) => c.fill(t.raised).shadow(1.0, 2.0, Color { r: 0.16, g: 0.2, b: 0.47, a: 0.06 }),
        (_, true) => c.fill(t.alert_bg),
        (_, false) => c.fill(t.raised),
    };
    if mode.stacked && matches!(t.look, Look::Tension | Look::Daybreak) {
        c = c.fill(t.surf2);
    }
    if n.default_action && !mode.snoozed && !hidden {
        c = c.hit().role("button").label(format!("Open {}", n.summary)).on_press(move |s: &mut Stage| {
            send(s, Cmd::Activate(id));
            s.open = None;
        });
    }
    c.key(format!("n{id}"))
}

/// The tint under the tools, so a long summary does not show through them.
fn card_bg_under(t: &Tokens) -> Color {
    Color { a: 1.0, ..t.surf }
}

fn dnd_banner(t: &Tokens) -> El {
    let square = matches!(t.look, Look::Folio | Look::Phosphor);
    let off = btn(t, "Turn off", false, 30.0).key("dnd-off").role("button").label("Turn off Do Not Disturb").hit().on_press(|s: &mut Stage| send(s, Cmd::SetDnd(false)));
    let b = row().w_full().px(14.0).py(10.0).gap(12.0).center_y().radius(if square { 0.0 } else { t.radii.ctl })
        .child(glyph_el(t, "moon", 16.0, t.acc))
        .child(
            col().grow(1.0).min_w(0.0).gap(1.0)
                .child(label(t, 14.0, 600.0, "Do Not Disturb", t.ink))
                .child(label(t, 12.0, 400.0, "Critical alerts still show", t.ink2).truncate()),
        )
        .child(off);
    match t.look {
        Look::Folio | Look::Phosphor => b.border(1.0, t.rule),
        _ => b.fill(Color { a: 0.12, ..t.acc }),
    }
}

fn empty(t: &Tokens, title: &str, detail: Option<&str>) -> El {
    let square = matches!(t.look, Look::Folio | Look::Phosphor);
    let ring = cell(44.0, 44.0, glyph_el(t, "bell", 18.0, t.ink3)).radius(if square { 0.0 } else { 22.0 });
    let ring = if square { ring.border(1.0, t.rule) } else { ring.fill(t.fill) };
    let mut c = col().w_full().h(220.0).center().gap(12.0).child(ring)
        .child(label(t, 14.0, 600.0, if t.look == Look::Phosphor { format!("> {}", title.to_lowercase()) } else { title.to_string() }, t.ink2));
    if let Some(d) = detail {
        c = c.child(label(t, 12.0, 400.0, d, t.ink3).wrap().px(24.0));
    }
    c
}

/// Starts decoding the wide previews, so a one-frame render has them. For the snapshot tool.
pub fn preview_warm(s: &Stage) {
    let (state, _, _) = read_state(s);
    for n in state.items.iter().filter(|n| n.preview) {
        if let Some(path) = &n.image {
            held(crate::media::images(&s.sh).file(path, 132.0, 1.0));
        }
    }
    std::thread::sleep(std::time::Duration::from_millis(300));
}

/// Opens a reply field on `id` with `text` typed, for the snapshot tool.
pub fn preview_reply(s: &mut Stage, id: u32, text: &str) {
    s.notes.reply = Some((id, text.to_string()));
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
