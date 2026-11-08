//! The control center: quick toggles, volume and brightness, a glance at what is playing, and
//! the session buttons.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use rune::{Color, El, col, el, row};
use yozora_services::audio::Cmd as AudioCmd;
use yozora_services::brightness::{self, Change};
use yozora_services::media::{Cmd as MediaCmd, Status};
use yozora_services::network::Cmd as NetCmd;
use yozora_services::notifications::Cmd as NotifyCmd;
use yozora_services::power;
use yozora_services::{bluetooth, run_shell};
use yozora_style::{Look, Tokens};

use crate::kit::{cell, glyph_el, label};
use crate::stage::{Pop, Stage};
use crate::wifi::{ink, shell, text_el};

pub const W: f32 = 380.0;

/// What the session buttons run. These are the bar's defaults; there is no config in this
/// workspace yet to override them.
const LOCK: &str = "loginctl lock-session";
const SETTINGS: &str = "yozora-settings";
const LOGOUT: &str = "uwsm stop || loginctl terminate-session \"$XDG_SESSION_ID\"";
const REBOOT: &str = "systemctl reboot";
const POWEROFF: &str = "systemctl poweroff";

/// Unix seconds until which the footer offers log out, restart and shut down. Pressing the power
/// button arms it, so a single stray press never ends the session.
static POWER_UNTIL: AtomicU64 = AtomicU64::new(0);

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

fn power_open() -> bool {
    POWER_UNTIL.load(Ordering::Relaxed) > now()
}

// Brightness is written through logind, which blocks, so a worker applies the latest request.
const NONE: u32 = u32::MAX;
static WANT: AtomicU32 = AtomicU32::new(NONE);
static BUSY: AtomicBool = AtomicBool::new(false);

fn set_brightness(s: &Stage, fraction: f32) {
    WANT.store((fraction.clamp(0.0, 1.0) * 1000.0).round() as u32, Ordering::Relaxed);
    if BUSY.swap(true, Ordering::AcqRel) {
        return;
    }
    let sh = s.sh.clone();
    std::thread::spawn(move || {
        loop {
            let w = WANT.load(Ordering::Relaxed);
            if w != NONE {
                let _ = brightness::apply(Change::Set(w as f32 / 1000.0));
            }
            // Nothing newer arrived while that was written: done.
            if WANT.compare_exchange(w, NONE, Ordering::AcqRel, Ordering::Relaxed).is_ok() {
                break;
            }
        }
        BUSY.store(false, Ordering::Release);
        sh.redraw();
    });
}

/// The screen's brightness, 0 to 1, or None without a backlight. A request in flight wins, so
/// the slider follows the pointer.
fn brightness_now() -> Option<f32> {
    let b = brightness::find().ok()?;
    match WANT.load(Ordering::Relaxed) {
        NONE => brightness::get(&b).ok(),
        w => Some(w as f32 / 1000.0),
    }
}

/// A button-sized drag track. `f` gets the pointer's position along it, 0 to 1.
fn slider(t: &Tokens, key: &'static str, name: &str, value: f32, f: impl Fn(&mut Stage, f32) + 'static) -> El {
    let v = value.clamp(0.0, 1.0);
    let track = match t.look {
        Look::Folio => {
            let ticks = row().pin(Some(0.0), None, Some(0.0), Some(0.0)).h(5.0).justify_between()
                .children((0..11).map(|_| el().size(1.0, 5.0).fill(t.ink3)));
            let knob = el().pin(None, Some(-8.0), Some(-5.0), None).size(10.0, 20.0).fill(t.acc);
            el().grow(1.0).h(20.0)
                .child(el().pin(Some(0.0), Some(9.5), Some(0.0), None).h(1.0).fill(t.ink))
                .child(ticks)
                .child(el().pin(Some(0.0), Some(8.0), None, None).h(4.0).w_frac(v).fill(t.ink).child(knob))
        }
        Look::Phosphor => {
            const N: usize = 25;
            let lit = (v * N as f32).round() as usize;
            row().grow(1.0).h(16.0).gap(3.0)
                .children((0..N).map(|i| el().grow(1.0).h(16.0).fill(if i < lit { t.ink } else { t.ink3 })))
        }
        Look::Prism => {
            let knob = el().pin(None, Some(0.0), Some(-12.0), None).size(24.0, 24.0).radius(t.radii.pill)
                .fill(t.on_ink).shadow(2.0, 8.0, Color { a: 0.3, ..t.ink });
            let fill = el().w_frac(v).h_full().radius(t.radii.pill)
                .linear_gradient(std::f32::consts::FRAC_PI_2, [(0.0, Color { a: 0.65, ..t.acc }), (1.0, t.acc)])
                .child(knob);
            el().grow(1.0).h(24.0).radius(t.radii.pill).fill(Color { a: 0.07, ..t.ink }).child(fill)
        }
        Look::Daybreak => el().grow(1.0).h(28.0).radius(t.radii.pill).fill(t.raised).child(
            el().w_frac(v).h_full().radius(t.radii.pill)
                .linear_gradient(std::f32::consts::FRAC_PI_2, [(0.0, Color { a: 0.72, ..t.ink }), (1.0, t.ink)]),
        ),
        _ => el().grow(1.0).h(28.0).radius(t.radii.pill).fill(t.raised)
            .child(el().w_frac(v).h_full().radius(t.radii.pill).fill(t.ink)),
    };
    track.key(key).role("slider").label(name).hit().on_drag(move |s: &mut Stage, d, _| {
        if !d.cancelled() && d.w > 0.0 {
            f(s, (d.x / d.w).clamp(0.0, 1.0));
        }
    })
}

/// One slider row: an icon, the track, and a spot on the right for an action or nothing.
fn slider_row(left: El, track: El, right: Option<El>) -> El {
    row().w_full().h(48.0).px(5.0).gap(14.0).center_y()
        .child(left)
        .child(track)
        .child(right.unwrap_or_else(|| el().size(28.0, 28.0)))
}

fn tile(t: &Tokens, key: &'static str, name: &'static str, glyph: &str, sub: &str, on: bool, f: impl Fn(&mut Stage) + 'static) -> El {
    let (main, dim) = if on && t.look == Look::Phosphor { ink(t, true) } else { ink(t, false) };
    let ti = cell(36.0, 36.0, glyph_el(t, glyph, 18.0, if on && matches!(t.look, Look::Tension | Look::Daybreak | Look::Prism) { t.on_ink } else { t.ink }));
    let ti = match t.look {
        Look::Folio => ti.border(1.0, t.ink),
        Look::Phosphor if on => ti.fill(t.on_ink),
        Look::Phosphor => ti.border(1.0, t.rule),
        _ => ti.radius(t.radii.pill.min(18.0)).fill(if on { t.on } else { t.fill }),
    };
    let sub = match t.look {
        Look::Folio => text_el(t, 10.5, 500.0, sub.to_uppercase(), 0.8, true),
        Look::Phosphor => label(t, 12.0, 400.0, sub, dim),
        _ => label(t, 12.0, 500.0, sub, dim),
    };
    let body = col().grow(1.0).gap(3.0).child(label(t, 14.0, 600.0, name, main).truncate()).child(sub.color(dim).truncate());
    let r = row().grow(1.0).h(72.0).px(16.0).gap(12.0).center_y().radius(t.radii.card).child(ti).child(body);
    let r = match t.look {
        Look::Folio => r.border(1.0, if on { t.ink } else { t.rule }),
        Look::Phosphor if on => r.fill(t.ink),
        Look::Phosphor => r.border(1.0, t.rule),
        _ => r.fill(t.raised),
    };
    r.key(key).role("button").label(name).hit().hover(t.raised).on_press(f)
}

fn round_button(t: &Tokens, key: &'static str, name: &str, glyph: &str, size: f32, filled: bool, f: impl Fn(&mut Stage) + 'static) -> El {
    let b = cell(size, size, glyph_el(t, glyph, 18.0, t.ink)).radius(t.radii.pill.min(size / 2.0));
    let b = if filled { b.fill(t.raised) } else { b };
    b.key(key).role("button").label(name).hit().hover(t.raised).on_press(f)
}

fn pill_button(t: &Tokens, key: &'static str, name: &'static str, color: Color, f: impl Fn(&mut Stage) + 'static) -> El {
    let words = match t.look {
        Look::Folio => text_el(t, 11.0, 500.0, name.to_uppercase(), 0.9, false).color(color),
        _ => label(t, 13.0, 500.0, name, color),
    };
    let b = row().h(34.0).px(16.0).center().radius(t.radii.pill).child(words);
    let b = match t.look {
        Look::Folio => b.border(1.0, t.ink),
        Look::Phosphor => b.border(1.0, t.ink),
        _ => b.fill(t.raised),
    };
    b.key(key).role("button").label(name).hit().on_press(f)
}

fn run(s: &mut Stage, command: &str) {
    run_shell(command);
    s.open = None;
}

pub fn view(s: &Stage, t: &Tokens) -> El {
    let svc = s.sh.services.lock().unwrap();
    let net = svc.network.as_ref().map(|n| n.read().state.clone());
    let bt = svc.bluetooth.as_ref().map(|b| b.read().state.clone());
    let dnd = svc.notifications.as_ref().map(|n| n.read().state.dnd);
    let pw = svc.power.as_ref().map(|p| p.read().state.clone());
    let sink = svc.audio.as_ref().and_then(|a| a.read().state.sink().cloned());
    let player = svc.media.as_ref().and_then(|m| m.read().state.current().cloned());
    drop(svc);

    let mut tiles: Vec<El> = Vec::new();
    if let Some(st) = net.filter(|n| n.wifi_device) {
        let on = st.wifi_enabled;
        let sub = match &st.primary {
            Some(p) => p.name.clone(),
            None if st.activating.is_some() => "Connecting\u{2026}".into(),
            None if !on => "Off".into(),
            None => "Not connected".into(),
        };
        tiles.push(tile(t, "tile-wifi", "Wi-Fi", "wifi", &sub, on, move |s| {
            if let Some(n) = &s.sh.services.lock().unwrap().network {
                n.send(NetCmd::SetWifi(!on));
            }
        }));
    }
    if let Some(st) = bt.filter(|b| b.adapter.is_some()) {
        let on = st.powered;
        let n = st.connected().count();
        let sub = match (on, n) {
            (false, _) => "Off".to_owned(),
            (true, 0) => "On".to_owned(),
            (true, 1) => st.connected().next().map(|d| d.name.clone()).unwrap_or_default(),
            (true, n) => format!("{n} devices"),
        };
        tiles.push(tile(t, "tile-bt", "Bluetooth", "bt", &sub, on, move |s| {
            if let Some(b) = &s.sh.services.lock().unwrap().bluetooth {
                b.send(bluetooth::Cmd::SetPowered(!on));
            }
        }));
    }
    if let Some(on) = dnd {
        tiles.push(tile(t, "tile-dnd", "Focus", "moon", if on { "On" } else { "Off" }, on, move |s| {
            if let Some(n) = &s.sh.services.lock().unwrap().notifications {
                n.send(NotifyCmd::SetDnd(!on));
            }
        }));
    }
    if let Some(p) = pw.as_ref().filter(|p| !p.profiles.is_empty()) {
        let current = p.profile.clone().unwrap_or_default();
        let next = p.profiles.iter().position(|n| *n == current).map_or(0, |i| (i + 1) % p.profiles.len());
        let next = p.profiles[next].clone();
        tiles.push(tile(t, "tile-profile", "Profile", "gauge", power::profile_label(&current), false, move |s| {
            if let Some(p) = &s.sh.services.lock().unwrap().power {
                p.send(power::Cmd::SetProfile(next.clone()));
            }
        }));
    }

    let mut out = shell(t, W, "control");
    let mut tiles = tiles.into_iter();
    while let Some(a) = tiles.next() {
        out = out.child(row().w_full().gap(10.0).child(a).child(tiles.next().unwrap_or_else(|| el().grow(1.0))));
    }

    if let Some(d) = sink {
        let glyph = if d.muted || d.volume <= 0.0 { "vol0" } else { "vol" };
        let mute = cell(28.0, 28.0, glyph_el(t, glyph, 18.0, t.ink)).key("mute").role("button").label("Mute").hit().radius(t.radii.pill)
            .on_press(move |s: &mut Stage| {
                if let Some(a) = &s.sh.services.lock().unwrap().audio {
                    a.send(AudioCmd::SinkMute(!d.muted));
                }
            });
        let output = cell(28.0, 28.0, glyph_el(t, "chev", 16.0, t.ink2)).key("output").role("button").label("Output devices").hit().radius(t.radii.pill)
            .on_press(|s: &mut Stage| {
                let x = s.anchor_x;
                s.toggle(Pop::Sound, x);
            });
        let track = slider(t, "volume", "Volume", d.volume, |s, v| {
            if let Some(a) = &s.sh.services.lock().unwrap().audio {
                a.send(AudioCmd::SinkVolume(v));
            }
        });
        out = out.child(slider_row(mute, track, Some(output)));
    }
    if let Some(b) = brightness_now() {
        let track = slider(t, "brightness", "Brightness", b, |s, v| set_brightness(s, v));
        out = out.child(slider_row(cell(28.0, 28.0, glyph_el(t, "sun", 18.0, t.ink)), track, None));
    }

    if let Some(p) = player {
        let playing = p.status == Status::Playing;
        let name = if p.title.is_empty() { p.identity.clone() } else { p.title.clone() };
        let by = if p.artist.is_empty() { p.identity.clone() } else { p.artist.clone() };
        let art = cell(44.0, 44.0, glyph_el(t, "music", 20.0, t.ink2)).radius(t.radii.chip).fill(t.fill);
        let art = if matches!(t.look, Look::Folio | Look::Phosphor) { art.border(1.0, t.ink) } else { art };
        let (size, sub) = match t.look {
            Look::Folio => (16.0, text_el(t, 10.5, 500.0, by.to_uppercase(), 0.8, true)),
            Look::Phosphor => (13.0, label(t, 12.0, 400.0, by, t.ink2)),
            _ => (15.0, label(t, 12.0, 500.0, by, t.ink2)),
        };
        let words = col().grow(1.0).gap(2.0).child(label(t, size, 600.0, name, t.ink).truncate()).child(sub.truncate());
        let glance = row().grow(1.0).gap(14.0).center_y().child(art).child(words)
            .key("glance").role("button").label("Now playing").hit()
            .on_press(|s: &mut Stage| {
                let x = s.anchor_x;
                s.toggle(Pop::Media, x);
            });
        let send = |c: MediaCmd| move |s: &mut Stage| {
            if let Some(m) = &s.sh.services.lock().unwrap().media {
                m.send(c.clone());
            }
        };
        let card = row().w_full().px(14.0).py(12.0).gap(14.0).center_y().radius(t.radii.card)
            .child(glance)
            .child(round_button(t, "play", if playing { "Pause" } else { "Play" }, if playing { "pause" } else { "play" }, 36.0, false, send(MediaCmd::PlayPause)))
            .child(round_button(t, "next", "Next", "next", 36.0, false, send(MediaCmd::Next)));
        let card = match t.look {
            Look::Folio => card.border_sides([1.0, 0.0, 0.0, 0.0], t.rule),
            Look::Phosphor => card.border(1.0, t.rule),
            _ => card.fill(t.raised),
        };
        out = out.child(card);
    }

    let foot = if power_open() {
        let c = t.ink;
        row().w_full().mt(8.0).pt(6.0).gap(10.0).justify_end().center_y()
            .child(pill_button(t, "logout", "Log out", c, |s| run(s, LOGOUT)))
            .child(pill_button(t, "reboot", "Restart", c, |s| run(s, REBOOT)))
            .child(pill_button(t, "poweroff", "Shut down", t.alert, |s| run(s, POWEROFF)))
            .child(round_button(t, "cancel", "Cancel", "x", 44.0, true, |_| POWER_UNTIL.store(0, Ordering::Relaxed)))
    } else {
        row().w_full().mt(8.0).pt(6.0).gap(10.0).justify_end()
            .child(round_button(t, "settings", "Settings", "gear", 44.0, true, |s| run(s, SETTINGS)))
            .child(round_button(t, "lock", "Lock", "lock", 44.0, true, |s| run(s, LOCK)))
            .child(round_button(t, "power", "Power", "power", 44.0, true, |_| POWER_UNTIL.store(now() + 8, Ordering::Relaxed)))
    };
    out.child(foot)
}
