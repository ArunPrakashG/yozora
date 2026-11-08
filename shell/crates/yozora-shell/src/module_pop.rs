//! `module/<name>` modules: what their bar item shows and their popover. The module sends a view
//! tree of a few semantic parts and the shell draws them in the look in force, so every module
//! looks like it belongs.

use rune::{Color, Drag, El, Tx, col, el, row};
use serde_json::{Value, json};
use yozora_module::{Node, Size, Tone, Update};
use yozora_services::modules::{ActionOutcome, ModuleState};
use yozora_style::{Look, Tokens};

use crate::kit::{cell, fig, glyph_el, label};
use crate::menu::pill;
use crate::shared::Shared;
use crate::sound::track;
use crate::stage::Stage;
use crate::wifi::{empty, head, section, shell, sep, text_el};

pub const W: f32 = 340.0;
/// Sections nest at most this deep; anything deeper shows the error state.
const MAX_DEPTH: usize = 3;
const TRACK_W: f32 = W - 36.0 - 28.0;

const ERROR_ICON: &str = "warning";
const ERROR_TEXT: &str = "Error";
/// The icon before a module has reported anything.
const PENDING_ICON: &str = "dot";

/// An action that asked to be confirmed and is waiting for the user.
#[derive(Clone, Debug)]
pub struct Pending {
    pub action: String,
    pub value: Option<Value>,
    pub label: String,
}

/// A bar item, ready to draw.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemView {
    pub icon: String,
    pub text: Option<String>,
    pub badge: Option<u32>,
    pub tone: Tone,
    /// The module has failed. Its last good update may still be what is shown.
    pub errored: bool,
}

/// The bar item for a module's state. With an error and a last good update, that update stays
/// on screen with the tone forced to warning. With an error and nothing good, the standard error
/// state is shown alone.
pub fn item_view(last_good: Option<&Update>, error: Option<&str>) -> ItemView {
    match (last_good, error) {
        (Some(u), None) => ItemView { icon: u.bar.icon.clone(), text: u.bar.text.clone(), badge: u.bar.badge, tone: u.bar.tone, errored: false },
        (Some(u), Some(_)) => ItemView { icon: u.bar.icon.clone(), text: u.bar.text.clone(), badge: u.bar.badge, tone: Tone::Warning, errored: true },
        (None, Some(_)) => ItemView { icon: ERROR_ICON.into(), text: Some(ERROR_TEXT.into()), badge: None, tone: Tone::Danger, errored: true },
        // Not reported yet: a quiet placeholder rather than nothing, so the zone doesn't jump.
        (None, None) => ItemView { icon: PENDING_ICON.into(), text: None, badge: None, tone: Tone::Neutral, errored: false },
    }
}

pub fn state_of(sh: &Shared, name: &str) -> ModuleState {
    sh.services.lock().unwrap().modules.as_ref().and_then(|m| m.states().get(name).cloned()).unwrap_or_default()
}

/// The glyph for an icon name a module supplied; names the set doesn't know show a dot.
pub fn glyph_name(name: &str) -> &'static str {
    match name {
        "logo" | "moon" => "moon",
        "wifi" | "wifi-1" | "wifi-2" => "wifi",
        "wifi-off" => "wifioff",
        "ethernet" => "ethernet",
        "network-off" => "netoff",
        "vpn" => "shield",
        "volume-high" => "vol",
        "volume-low" => "vol1",
        "volume-mute" => "vol0",
        "mic" => "mic",
        "mic-off" => "micoff",
        "speaker" => "speaker",
        "headphones" => "phones",
        "display" | "window" => "monitor",
        "bluetooth" => "bt",
        "bluetooth-off" => "btoff",
        "battery" => "battery",
        "battery-charging" => "battcharge",
        "bell" => "bell",
        "appearance" | "sun" => "sun",
        "check" => "check",
        "chevron-left" => "chevl",
        "chevron-right" => "chev",
        "play" => "play",
        "pause" => "pause",
        "previous" => "prev",
        "next" => "next",
        "close" => "x",
        "settings" => "gear",
        "lock" => "lock",
        "lock-open" | "unlock" => "unlock",
        "power" => "power",
        "keyboard" => "kbd",
        "mouse" => "mouse",
        "terminal" | "code" => "term",
        "globe" => "globe",
        "package" => "package",
        "sync" | "reset" => "sync",
        "cloud" => "cloud",
        "control-center" => "cc",
        "music" => "music",
        "bolt" => "bolt",
        "leaf" => "leaf",
        "gauge" => "gauge",
        "calendar" => "calendar",
        "warning" | "alert" => "alert",
        "info" => "info",
        "search" => "search",
        "folder" => "folder",
        "plus" => "plus",
        "star" => "star",
        _ => "dot",
    }
}

fn hex(v: u32) -> Color {
    Color { r: ((v >> 16) & 0xff) as f32 / 255.0, g: ((v >> 8) & 0xff) as f32 / 255.0, b: (v & 0xff) as f32 / 255.0, a: 1.0 }
}

/// A tone as a colour in the look in force. `neutral` is what the caller draws plain things in.
pub fn tone_color(t: &Tokens, tone: Tone, neutral: Color) -> Color {
    match tone {
        Tone::Neutral => neutral,
        Tone::Accent => if matches!(t.look, Look::Folio | Look::Phosphor) { t.ink } else { t.acc },
        Tone::Success => hex(if t.dark { 0x8FE3CF } else { 0x0D7F69 }),
        Tone::Warning => hex(if t.dark { 0xFFC58B } else { 0x9A4F0B }),
        Tone::Danger => t.alert,
    }
}

fn soft(c: Color) -> Color {
    Color { a: 0.16, ..c }
}

/// Sends an action to the module. A destructive one answers `NeedsConfirm` first, and is kept in
/// the popover until confirmed.
fn fire(s: &mut Stage, name: &str, pending: Pending, confirmed: bool) {
    let mut outcome = None;
    if let Some(m) = &s.sh.services.lock().unwrap().modules {
        m.send(name, |h| outcome = Some(h.send_action(&pending.action, pending.value.clone(), confirmed)));
    }
    match outcome {
        Some(ActionOutcome::NeedsConfirm) => s.bar.pending = Some(pending),
        other => {
            if other != Some(ActionOutcome::Sent) {
                tracing::warn!("module/{name}: action {:?} was not delivered: {other:?}", pending.action);
            }
            s.bar.pending = None;
        }
    }
}

fn on_action(name: &str, action: &str, value: Option<Value>, label: &str) -> impl Fn(&mut Stage) + 'static {
    let (name, pending) = (name.to_owned(), Pending { action: action.to_owned(), value, label: label.to_owned() });
    move |s| fire(s, &name, pending.clone(), false)
}

fn tone_chip(t: &Tokens, text: &str, tone: Tone) -> El {
    let c = tone_color(t, tone, t.ink2);
    let chip = row().h(20.0).px(8.0).center().radius(t.radii.pill).child(label(t, 11.0, 600.0, text, c));
    match t.look {
        Look::Folio | Look::Phosphor => chip.border(1.0, c),
        _ => chip.fill(soft(c)),
    }
}

/// The standard state for anything that can't be drawn.
fn error_state(t: &Tokens) -> El {
    empty(t, &["Something went wrong", "This part of the module could not be shown."])
}

fn note(t: &Tokens, text: &str, color: Color) -> El {
    row().w_full().px(10.0).py(4.0).child(label(t, 12.5, 400.0, text, color))
}

#[allow(clippy::too_many_arguments)]
fn row_node(t: &Tokens, key: String, name: &str, icon: Option<&str>, title: &str, subtitle: Option<&str>, value: Option<&str>, badge: Option<(&str, Tone)>, tone: Tone, size: Size, action: Option<&str>) -> El {
    let compact = size == Size::Compact;
    let main = tone_color(t, tone, t.ink);
    let mut body = col().grow(1.0).min_w(0.0).gap(2.0).child(label(t, if compact { 13.0 } else { 14.0 }, 500.0, title, main).truncate());
    if let Some(sub) = subtitle {
        body = body.child(label(t, 12.0, 400.0, sub, t.ink2).truncate());
    }
    let mut r = row().w_full().min_h(if compact { 36.0 } else { 50.0 }).px(14.0).gap(14.0).center_y().radius(t.radii.ctl);
    if let Some(i) = icon {
        r = r.child(cell(18.0, 18.0, glyph_el(t, glyph_name(i), 18.0, tone_color(t, tone, t.ink2))));
    }
    r = r.child(body);
    if let Some(v) = value {
        r = r.child(fig(t, 13.0, 500.0, v, tone_color(t, tone, t.ink2)));
    }
    if let Some((text, tone)) = badge {
        r = r.child(tone_chip(t, text, tone));
    }
    match action {
        Some(a) => r.hover(t.raised).key(key).role("button").label(title.to_owned()).hit().on_press(on_action(name, a, None, title)),
        None => r.key(key),
    }
}

fn slider_node(t: &Tokens, idx: usize, name: &str, title: &str, value: f32, action: &str, drag: Option<f32>) -> El {
    let shown = drag.unwrap_or(value).clamp(0.0, 1.0);
    let (module, action_id, what) = (name.to_owned(), action.to_owned(), title.to_owned());
    let head = row().w_full().gap(8.0).center_y()
        .child(label(t, 14.0, 500.0, title, t.ink).grow(1.0).truncate())
        .child(fig(t, 13.0, 500.0, format!("{}%", (shown * 100.0).round() as i32), t.ink2));
    let bar = row().w(TRACK_W).h(32.0).center_y().child(track(t, TRACK_W, shown, false))
        .key(format!("mn-slider{idx}")).role("slider").label(title.to_owned()).hit()
        .on_drag(move |s: &mut Stage, d: &mut Drag, _: &mut Tx| {
            if d.w <= 0.0 {
                return;
            }
            let f = ((d.x / d.w).clamp(0.0, 1.0) * 100.0).round() / 100.0;
            match d.phase {
                rune::Phase::Up if d.cancelled() => s.bar.slide = None,
                rune::Phase::Up => {
                    s.bar.slide = None;
                    fire(s, &module, Pending { action: action_id.clone(), value: Some(json!(f)), label: what.clone() }, false);
                }
                _ => s.bar.slide = Some((idx, f)),
            }
        });
    col().w_full().px(14.0).py(6.0).gap(4.0).child(head).child(bar)
}

fn node(s: &Stage, t: &Tokens, name: &str, n: &Node, next: &mut usize, depth: usize) -> Vec<El> {
    *next += 1;
    let idx = *next;
    match n {
        Node::Section { heading, rows } => {
            if depth >= MAX_DEPTH {
                return vec![error_state(t)];
            }
            let mut out = Vec::new();
            if let Some(h) = heading {
                out.push(section(t, h, None));
            }
            for r in rows {
                out.extend(node(s, t, name, r, next, depth + 1));
            }
            out
        }
        Node::Row { icon, title, subtitle, value, badge, tone, size, action } => vec![row_node(
            t,
            format!("mn-row{idx}"),
            name,
            icon.as_deref(),
            title,
            subtitle.as_deref(),
            value.as_deref(),
            badge.as_ref().map(|b| (b.text.as_str(), b.tone)),
            *tone,
            *size,
            action.as_deref(),
        )],
        Node::Toggle { title, on, action } => {
            let lead = row().w_full().min_h(50.0).px(14.0).gap(14.0).center_y().radius(t.radii.ctl)
                .child(label(t, 14.0, 500.0, title.as_str(), t.ink).grow(1.0).truncate())
                .child(crate::wifi::switch(t, *on));
            vec![lead.hover(t.raised).key(format!("mn-toggle{idx}")).role("switch").label(title.clone()).hit().on_press(on_action(name, action, Some(json!(!on)), title))]
        }
        Node::Slider { title, value, action } => {
            let drag = s.bar.slide.filter(|(i, _)| *i == idx).map(|(_, v)| v);
            vec![slider_node(t, idx, name, title, *value, action, drag)]
        }
        Node::Progress { title, subtitle, value } => {
            let v = if value.is_finite() { value.clamp(0.0, 1.0) } else { 0.0 };
            let mut c = col().w_full().px(14.0).py(6.0).gap(4.0).child(label(t, 14.0, 500.0, title.as_str(), t.ink).truncate());
            if let Some(sub) = subtitle {
                c = c.child(label(t, 12.0, 400.0, sub.as_str(), t.ink2).truncate());
            }
            vec![c.child(el().h(2.0)).child(track(t, TRACK_W, v, true))]
        }
        Node::Buttons { items } => {
            let mut r = row().w_full().px(2.0).pt(4.0).gap(10.0);
            for (i, b) in items.iter().enumerate() {
                r = r.child(pill(t, format!("mn-btn{idx}-{i}"), &b.label, b.primary || b.destructive, b.destructive, on_action(name, &b.action, None, &b.label)));
            }
            vec![r]
        }
        Node::Empty { title, message } => {
            let mut lines = vec![title.as_str()];
            lines.extend(message.as_deref());
            vec![empty(t, &lines)]
        }
        Node::Note { text } => vec![note(t, text, t.ink2)],
    }
}

fn confirm(t: &Tokens, name: &str, pending: &Pending) -> El {
    let (module, again) = (name.to_owned(), pending.clone());
    col().w_full().px(8.0).py(8.0).gap(10.0)
        .child(label(t, 14.0, 500.0, format!("{}?", pending.label), t.ink))
        .child(
            row().w_full().gap(10.0)
                .child(pill(t, "mn-cancel", "Cancel", false, false, |s| s.bar.pending = None))
                .child(pill(t, "mn-confirm", &pending.label, true, true, move |s| fire(s, &module, again.clone(), true))),
        )
}

pub fn view(s: &Stage, t: &Tokens) -> El {
    let name = s.bar.module.clone();
    let state = state_of(&s.sh, &name);
    let body = state.last_good.as_ref().and_then(|u| u.popover.as_ref());
    let mut p = shell(t, W, &name);
    p = p.child(head(t, body.map_or(name.as_str(), |b| b.title.as_str()), None));
    if let Some(c) = body.and_then(|b| b.caption.as_deref()) {
        p = p.child(note(t, c, t.ink2).mt(-4.0));
    }
    // The error goes on top; the last good body stays below it.
    if let Some(e) = &state.error {
        p = p.child(note(t, e, t.alert));
    }
    if let Some(pending) = &s.bar.pending {
        p = p.child(confirm(t, &name, pending));
    }
    match body {
        Some(b) => {
            let mut next = 0;
            for n in &b.body {
                for e in node(s, t, &name, n, &mut next, 0) {
                    p = p.child(e);
                }
            }
            if let Some(f) = &b.footer_note {
                p = p.child(sep(t)).child(match t.look {
                    Look::Folio | Look::Phosphor => row().w_full().px(10.0).child(text_el(t, 10.5, 400.0, f.to_uppercase(), 0.8, true)),
                    _ => note(t, f, t.ink3),
                });
            }
        }
        None if state.error.is_some() => p = p.child(error_state(t)),
        None => p = p.child(empty(t, &["Nothing to show"])),
    }
    p
}
