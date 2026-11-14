//! Toasts: the cards that announce a new notification at the top right.
//!
//! The daemon decides what is a toast and keeps each one's clock; the shell draws the ones it
//! says are visible, holds their clocks while the pointer is on one or a reply is typed, and
//! sends clicks back. The line along the bottom drains on a render-time track, so a running
//! toast costs no redraws.

use std::time::{Duration, Instant};

use rune::{El, Key, KeyEvent, SurfaceFit, Tween, col, el, row};
use yozora_services::notifications::{Cmd, Notification, Toast};
use yozora_style::{Fx, Look, Tokens};

use super::{HIDDEN, actions, ink, picture, read_state, reply_row, send, tile, when};
use crate::bar;
use crate::kit::{cell, enter, glyph_el, label};
use crate::stage::{Pop, Stage};

pub const TOAST_W: f32 = 384.0;
/// The draining line, inset from the corners.
const LINE_W: f32 = TOAST_W - 16.0;
const PAD: f32 = 20.0;

/// How one toast's line is drawn.
#[derive(Default)]
pub struct Line {
    /// When a running line reaches the end.
    end: Option<Instant>,
    running: bool,
    /// How much was left when it last stopped.
    frac: f32,
    tween: Option<Tween<[f32; 4]>>,
}

/// Which stage shows toasts: the configured monitor, else the focused one, else the first.
fn here(s: &Stage, monitor: &str) -> bool {
    let outputs = s.sh.outputs();
    let target = if monitor != "focused" && outputs.iter().any(|o| o == monitor) {
        Some(monitor.to_string())
    } else {
        crate::desktop::focused_output(&s.sh).filter(|o| outputs.contains(o))
    };
    match target {
        Some(o) => o == s.output,
        None => outputs.first().is_none_or(|o| *o == s.output),
    }
}

/// Keeps the lines, holds and reply in step with the daemon. Runs on every redraw.
pub fn sync(s: &mut Stage) {
    let (state, _, _) = read_state(s);
    let now = Instant::now();
    let toasts: Vec<(u32, Toast)> = state.items.iter().filter_map(|n| n.toast.map(|t| (n.id, t))).collect();
    s.notes.lines.retain(|id, _| toasts.iter().any(|(i, t)| i == id && t.visible));
    for (id, tw) in toasts.iter().filter(|(_, t)| t.visible) {
        let line = s.notes.lines.entry(*id).or_default();
        match (tw.running, tw.remaining(now), tw.fraction(now)) {
            (true, Some(left), Some(frac)) => {
                let end = now + left;
                // The same run, as long as the daemon's end has not moved.
                let moved = line.end.is_none_or(|e| if e > end { e - end } else { end - e } > Duration::from_millis(250));
                if !line.running || moved {
                    line.tween = Some(Tween::linear([0.0, 0.0, LINE_W * frac, 2.0], [0.0, 0.0, 0.0, 2.0], left));
                    line.end = Some(end);
                    line.running = true;
                }
            }
            (_, _, frac) => {
                line.running = false;
                line.tween = None;
                line.end = None;
                line.frac = frac.unwrap_or(1.0);
            }
        }
    }
    s.notes.hovered.retain(|id| toasts.iter().any(|(i, _)| i == id));
    if let Some((id, _)) = &s.notes.reply
        && !state.items.iter().any(|n| n.id == *id)
    {
        s.notes.reply = None;
    }
    hold(s);
}

/// Tells the daemon which toasts to keep still.
fn hold(s: &mut Stage) {
    let mut ids: Vec<u32> = s.notes.hovered.iter().copied().collect();
    if let Some((id, _)) = &s.notes.reply {
        ids.push(*id);
    }
    ids.sort_unstable();
    ids.dedup();
    if ids != s.notes.held {
        s.notes.held = ids.clone();
        send(s, Cmd::Hold(ids));
    }
}

pub(super) fn start_reply(s: &mut Stage, id: u32) {
    s.notes.reply = Some((id, String::new()));
    hold(s);
    // A click gives an on-demand surface the keys on some compositors and not on others.
    let out = s.output.clone();
    s.sh.on_stage(Some(&out), |_| {});
}

pub(super) fn send_reply(s: &mut Stage, id: u32) {
    let Some((rid, text)) = s.notes.reply.clone() else { return };
    let text = text.trim().to_string();
    if rid != id || text.is_empty() {
        return;
    }
    s.notes.reply = None;
    s.notes.hovered.remove(&id);
    send(s, Cmd::Reply(id, text));
    hold(s);
}

/// What a key does while a reply is open. Returns whether it was used.
pub fn on_key(s: &mut Stage, e: &KeyEvent) -> bool {
    let Some((id, _)) = s.notes.reply.clone() else { return false };
    match &e.key {
        Key::Escape => {
            s.notes.reply = None;
            hold(s);
        }
        Key::Enter => send_reply(s, id),
        Key::Backspace => {
            if let Some((_, text)) = &mut s.notes.reply {
                if e.mods.cmd || e.mods.alt {
                    text.clear();
                } else {
                    text.pop();
                }
            }
        }
        Key::Text(t) if !e.mods.cmd => {
            if let Some((_, text)) = &mut s.notes.reply {
                text.push_str(t);
            }
        }
        _ => return false,
    }
    true
}

/// The stack of visible toasts, with a chip for the ones waiting, or none on this stage.
pub fn toasts(s: &Stage, t: &Tokens, fx: Fx) -> Option<El> {
    if s.open == Some(Pop::Notifications) {
        return None;
    }
    let (state, error, sharing) = read_state(s);
    if error.is_some() || !here(s, &state.toast_monitor) {
        return None;
    }
    let mut shown: Vec<(&Notification, Toast)> = state.items.iter().filter_map(|n| n.toast.filter(|t| t.visible).map(|t| (n, t))).collect();
    shown.sort_by_key(|(n, _)| std::cmp::Reverse(n.id));
    let waiting = state.items.iter().filter(|n| n.toast.is_some_and(|t| !t.visible)).count();
    if shown.is_empty() {
        return None;
    }
    let left = s.screen_w - bar::SIDE - TOAST_W;
    let bell = s.sh.anchor(&s.output, "bell").unwrap_or(s.screen_w - 80.0) - left;
    let mut c = col().key("toasts").w(TOAST_W).pin(None, Some(t.panel_top), Some(bar::SIDE), None).gap(10.0);
    for (n, tw) in shown {
        c = c.child(card(s, t, n, tw, sharing).key(format!("t{}", n.id)).transition(enter(t, fx, bell, TOAST_W)));
    }
    if waiting > 0 {
        c = c.child(more(t, waiting).key("more").transition(enter(t, fx, bell, TOAST_W)));
    }
    Some(c)
}

/// "2 more", for toasts waiting for room.
fn more(t: &Tokens, n: usize) -> El {
    let square = matches!(t.look, Look::Folio | Look::Phosphor);
    let chip = row().h(28.0).px(14.0).center().self_end().radius(if square { 0.0 } else { t.radii.pill }).fill(t.surf)
        .child(label(t, 12.0, 500.0, format!("{n} more"), t.ink2));
    match t.look {
        Look::Folio | Look::Phosphor => chip.border(1.0, t.ink),
        _ => chip.border(1.0, t.rule),
    }
}

fn card(s: &Stage, t: &Tokens, n: &Notification, tw: Toast, sharing: bool) -> El {
    let id = n.id;
    let critical = n.urgency >= 2;
    let ik = ink(t, critical);
    let square = matches!(t.look, Look::Folio | Look::Phosphor);
    let hidden = sharing;

    let lead = match (&n.image, n.preview, hidden) {
        (Some(path), false, false) => {
            let base = el().size(40.0, 40.0).radius(if square { 0.0 } else { 20.0 });
            picture(&s.sh, base, path, 40.0, 2.0, SurfaceFit::Fill)
        }
        _ => tile(&s.sh, t, n, 40.0, hidden),
    };

    let app = if n.app.is_empty() { "Notification".to_string() } else { n.app.clone() };
    let close = cell(24.0, 24.0, glyph_el(t, "x", 14.0, ik.faint)).key(format!("close{id}")).role("button").label("Dismiss").hit().radius(t.radii.pill).hover(t.fill)
        .on_press(move |s: &mut Stage| send(s, Cmd::Dismiss(id)));
    let head = row().w_full().h(24.0).gap(6.0).center_y()
        .child(label(t, 12.0, 500.0, app.clone(), if critical && !square { t.alert } else { ik.dim }).truncate().min_w(0.0))
        .child(match n.progress.filter(|_| !hidden) {
            Some(p) => crate::kit::fig(t, 12.0, 600.0, format!("\u{b7} {p}%"), t.acc),
            None => label(t, 12.0, 500.0, format!("\u{b7} {}", when(n.seen)), ik.faint).max_lines(1),
        })
        .child(el().grow(1.0))
        .child(close);

    let mut text = col().grow(1.0).min_w(0.0).gap(4.0).child(head);
    if hidden {
        text = text.child(
            row().gap(8.0).center_y().child(glyph_el(t, "eye", 14.0, ik.faint)).child(label(t, 14.0, 400.0, HIDDEN, ik.dim).wrap()),
        );
    } else {
        if !n.summary.is_empty() {
            text = text.child(label(t, 15.0, 600.0, n.summary.clone(), ik.main).truncate());
        }
        if !n.body.is_empty() {
            text = text.child(label(t, 14.0, 400.0, n.body.clone(), ik.dim).wrap().max_lines(3));
        }
        if n.preview && let Some(path) = &n.image {
            let base = el().w(TOAST_W - 2.0 * PAD - 40.0 - 16.0).h(132.0).radius(if square { 0.0 } else { t.radii.chip }).mt(6.0);
            text = text.child(picture(&s.sh, base, path, 132.0, 1.0, SurfaceFit::Fill));
        }
    }
    if !hidden {
        match &s.notes.reply {
            Some((rid, typed)) if *rid == id => {
                text = text.child(el().h(6.0)).child(reply_row(t, id, typed, n.reply.as_deref().unwrap_or("Reply")));
            }
            _ => {
                if let Some(a) = actions(s, t, n, 34.0) {
                    text = text.child(el().h(6.0)).child(a);
                }
            }
        }
    }

    let mut c = crate::kit::card(t, TOAST_W).padding(PAD).gap(0.0).child(row().w_full().gap(16.0).items_start().child(lead).child(text));
    c = match (t.look, critical) {
        (Look::Folio, true) => c.fill(t.ink),
        (Look::Phosphor, true) => c.border(1.0, t.alert),
        (Look::Tension | Look::Daybreak | Look::Prism, true) => c.fill(t.alert_bg).border(1.0, t.alert),
        _ => c,
    };

    if let Some(line) = line(s, t, id, tw, critical) {
        c = c.child(line);
    }
    let has_default = n.default_action;
    c.key(format!("toast{id}")).hit().role("button").label(format!("{app}: {}", n.summary))
        .on_hover_change(move |s: &mut Stage, inside, _| {
            if inside {
                s.notes.hovered.insert(id);
            } else {
                s.notes.hovered.remove(&id);
            }
            hold(s);
        })
        .on_press(move |s: &mut Stage| {
            if s.notes.reply.as_ref().is_some_and(|(r, _)| *r == id) {
                return;
            }
            // A click opens what the app attached, or else files the toast in the center.
            send(s, if has_default { Cmd::Activate(id) } else { Cmd::Expire(id) });
        })
}

/// The line along the bottom edge: it drains while the clock runs and stands still when it is
/// held. Toasts that stay until closed have none.
fn line(s: &Stage, t: &Tokens, id: u32, tw: Toast, critical: bool) -> Option<El> {
    tw.total?;
    let _ = critical;
    let state = s.notes.lines.get(&id);
    let bar = el().w(LINE_W).h(2.0).fill(t.acc);
    let bar = match state.and_then(|l| l.tween.as_ref().filter(|_| l.running)) {
        Some(tween) => bar.opacity(0.55).live_rect(tween),
        None => {
            let frac = state.map_or_else(|| tw.fraction(Instant::now()).unwrap_or(1.0), |l| l.frac);
            bar.w(LINE_W * frac).opacity(0.25)
        }
    };
    Some(row().pin(Some(8.0), None, Some(8.0), Some(1.0)).h(2.0).child(bar))
}
