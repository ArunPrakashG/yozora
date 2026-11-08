//! The control center: quick toggles, volume and brightness, a glance at what is playing, and
//! the session buttons.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use rune::{Color, El, col, el, row};
use yozora_services::audio::Cmd as AudioCmd;
use yozora_services::brightness::{self, Change};
use yozora_services::media::{Cmd as MediaCmd, Status};
use yozora_services::network::Cmd as NetCmd;
use yozora_services::notifications::Cmd as NotifyCmd;
use yozora_services::{bluetooth, run_shell};
use yozora_style::{Look, Tokens};

use crate::kit::{cell, glyph_el, label};
use crate::shared::Shared;
use crate::stage::{Pop, Stage};
use crate::wifi::{ink, shell, text_el, title};

pub const W: f32 = 380.0;

/// Unix seconds until which the footer offers log out, restart and shut down. Pressing the power
/// button arms it, so a single stray press never ends the session.
static POWER_UNTIL: AtomicU64 = AtomicU64::new(0);

/// What the confirmation step is asking about, and until when.
#[derive(Clone, Copy, PartialEq)]
enum Action {
    Logout,
    Reboot,
    Poweroff,
}

impl Action {
    fn verb(self) -> &'static str {
        match self {
            Action::Logout => "Log out",
            Action::Reboot => "Restart",
            Action::Poweroff => "Shut down",
        }
    }

    fn command(self, s: &Stage) -> String {
        let c = s.sh.bar_conf();
        match self {
            Action::Logout => c.bar.session.logout.clone(),
            Action::Reboot => c.bar.session.reboot.clone(),
            Action::Poweroff => c.bar.session.poweroff.clone(),
        }
    }
}

static CONFIRM: Mutex<Option<(Action, u64)>> = Mutex::new(None);

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

fn power_open() -> bool {
    POWER_UNTIL.load(Ordering::Relaxed) > now()
}

fn confirming() -> Option<Action> {
    CONFIRM.lock().unwrap().filter(|(_, until)| *until > now()).map(|(a, _)| a)
}

/// The portal's colour scheme and when it was read. It is read off the drawing thread and kept
/// for a few seconds, since asking the portal blocks.
static DARK: Mutex<Option<(Instant, bool)>> = Mutex::new(None);

fn portal_dark(sh: &Arc<Shared>) -> bool {
    static BUSY: AtomicBool = AtomicBool::new(false);
    let cached = *DARK.lock().unwrap();
    if cached.is_none_or(|(at, _)| at.elapsed() > Duration::from_secs(3)) && !BUSY.swap(true, Ordering::AcqRel) {
        let sh = sh.clone();
        std::thread::spawn(move || {
            let dark = yozora_lockd::system_dark();
            let before = DARK.lock().unwrap().map(|(_, d)| d);
            *DARK.lock().unwrap() = Some((Instant::now(), dark));
            BUSY.store(false, Ordering::Release);
            if before != Some(dark) {
                sh.redraw();
            }
        });
    }
    cached.is_none_or(|(_, d)| d)
}

/// Whether the desktop is dark. Surface tension answers for itself, since its day look is the
/// light one; the other looks have one palette, so this is the system setting.
fn is_dark(sh: &Arc<Shared>, t: &Tokens) -> bool {
    match t.look {
        Look::Tension => true,
        Look::Daybreak => false,
        _ => portal_dark(sh),
    }
}

fn set_dark(s: &Stage, t_look: Look, dark: bool) {
    *DARK.lock().unwrap() = Some((Instant::now(), dark));
    let scheme = if dark { "prefer-dark" } else { "prefer-light" };
    run_shell(&format!(
        "gsettings set org.gnome.desktop.interface color-scheme {scheme} || dconf write /org/gnome/desktop/interface/color-scheme \"'{scheme}'\""
    ));
    if matches!(t_look, Look::Tension | Look::Daybreak) {
        crate::config::set(&s.sh, "time", if dark { "night" } else { "day" });
    }
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

/// The question before a session ends.
fn confirm(t: &Tokens, action: Action) -> El {
    let ask = match action {
        Action::Logout => "Log out now?",
        Action::Reboot => "Restart now?",
        Action::Poweroff => "Shut down now?",
    };
    shell(t, W, "control")
        .child(title(t, ask).px(8.0).py(10.0))
        .child(label(t, 13.0, 400.0, "Unsaved work in open apps may be lost.", t.ink2).px(8.0).pb(14.0))
        .child(
            row().w_full().gap(10.0).justify_end()
                .child(pill_button(t, "cancel", "Cancel", t.ink, |_| *CONFIRM.lock().unwrap() = None))
                .child(pill_button(t, "confirm", action.verb(), t.alert, move |s| {
                    *CONFIRM.lock().unwrap() = None;
                    POWER_UNTIL.store(0, Ordering::Relaxed);
                    let command = action.command(s);
                    run(s, &command);
                })),
        )
}

/// Asks to see the power choices, or the confirmation, for a few seconds.
fn arm(s: &Stage, until: u64) {
    POWER_UNTIL.store(until, Ordering::Relaxed);
    s.sh.redraw_after(Duration::from_secs(until.saturating_sub(now()) + 1));
}

pub fn view(s: &Stage, t: &Tokens) -> El {
    if let Some(action) = confirming() {
        return confirm(t, action);
    }
    let svc = s.sh.services.lock().unwrap();
    let net = svc.network.as_ref().map(|n| n.read().state.clone());
    let bt = svc.bluetooth.as_ref().map(|b| b.read().state.clone());
    let dnd = svc.notifications.as_ref().map(|n| n.read().state.dnd);
    let pw = svc.power.as_ref().map(|p| p.read().state.clone());
    let (sink, source) = match svc.audio.as_ref() {
        Some(a) => {
            let snap = a.read();
            (snap.state.sink().cloned(), snap.state.source().cloned())
        }
        None => (None, None),
    };
    let player = svc.media.as_ref().and_then(|m| m.read().state.current().cloned());
    drop(svc);

    let mut tiles: Vec<El> = Vec::new();
    if let Some(st) = net {
        if st.wifi_device {
            let on = st.wifi_enabled;
            let sub = match &st.primary {
                Some(p) => p.name.clone(),
                None if st.activating.is_some() => "Connecting\u{2026}".into(),
                None if !on => "Off".into(),
                None => "Not connected".into(),
            };
            tiles.push(tile(t, "tile-wifi", "Wi-Fi", if on { "wifi" } else { "wifioff" }, &sub, on, move |s| {
                if let Some(n) = &s.sh.services.lock().unwrap().network {
                    n.send(NetCmd::SetWifi(!on));
                }
            }));
        } else if st.wired.is_some() || st.primary.is_some() {
            let up = st.primary.is_some() || st.wired.as_ref().is_some_and(|w| w.connected);
            let sub = st.primary.as_ref().map_or_else(|| if up { "Connected".to_owned() } else { "Not connected".to_owned() }, |p| p.name.clone());
            tiles.push(tile(t, "tile-net", "Network", if st.wired.is_some() { "ethernet" } else { "globe" }, &sub, up, |_| {}));
        }
    }
    if let Some(st) = bt {
        let available = st.adapter.is_some();
        let on = st.powered;
        let sub = if !available {
            "Unavailable".to_owned()
        } else {
            match (on, st.connected().count()) {
                (false, _) => "Off".to_owned(),
                (true, 0) => "On".to_owned(),
                (true, 1) => st.connected().next().map(|d| d.name.clone()).unwrap_or_default(),
                (true, n) => format!("{n} devices"),
            }
        };
        let b = tile(t, "tile-bt", "Bluetooth", "bt", &sub, on, move |s| {
            if available && let Some(b) = &s.sh.services.lock().unwrap().bluetooth {
                b.send(bluetooth::Cmd::SetPowered(!on));
            }
        });
        tiles.push(if available { b } else { b.opacity(0.6) });
    }
    if let Some(on) = dnd {
        tiles.push(tile(t, "tile-dnd", "Focus", "moon", if on { "On" } else { "Off" }, on, move |s| {
            if let Some(n) = &s.sh.services.lock().unwrap().notifications {
                n.send(NotifyCmd::SetDnd(!on));
            }
        }));
    }
    let dark = is_dark(&s.sh, t);
    let look = t.look;
    tiles.push(tile(t, "tile-dark", "Dark mode", "contrast", if dark { "On" } else { "Off" }, dark, move |s| {
        set_dark(s, look, !dark);
        s.sh.redraw();
    }));

    let mut out = shell(t, W, "control");
    let mut tiles = tiles.into_iter();
    while let Some(a) = tiles.next() {
        out = out.child(row().w_full().gap(10.0).child(a).child(tiles.next().unwrap_or_else(|| el().grow(1.0))));
    }

    if let Some(d) = sink {
        let glyph = if d.muted || d.volume <= 0.0 { "vol0" } else { "vol" };
        let muted = d.muted;
        let mute = cell(28.0, 28.0, glyph_el(t, glyph, 18.0, t.ink)).key("mute").role("button").label("Mute").hit().radius(t.radii.pill)
            .on_press(move |s: &mut Stage| {
                if let Some(a) = &s.sh.services.lock().unwrap().audio {
                    a.send(AudioCmd::SinkMute(!muted));
                }
            });
        let output = cell(28.0, 28.0, glyph_el(t, "chev", 16.0, t.ink2)).key("output").role("button").label("Output devices").hit().radius(t.radii.pill)
            .on_press(|s: &mut Stage| {
                let x = s.anchor_x;
                s.toggle(Pop::Sound, x);
            });
        let track = slider(t, "volume", "Volume", if muted { 0.0 } else { d.volume }, move |s, v| {
            if let Some(a) = &s.sh.services.lock().unwrap().audio {
                if muted && v > 0.0 {
                    a.send(AudioCmd::SinkMute(false));
                }
                a.send(AudioCmd::SinkVolume(v));
            }
        });
        out = out.child(slider_row(mute, track, Some(output)));
    }
    if let Some(d) = source {
        let muted = d.muted;
        let off = muted || d.volume <= 0.0;
        let mute = cell(28.0, 28.0, glyph_el(t, if off { "micoff" } else { "mic" }, 18.0, t.ink)).key("mic-mute").role("button")
            .label(if muted { "Unmute microphone" } else { "Mute microphone" }).hit().radius(t.radii.pill)
            .on_press(move |s: &mut Stage| {
                if let Some(a) = &s.sh.services.lock().unwrap().audio {
                    a.send(AudioCmd::SourceMute(!muted));
                }
            });
        let track = slider(t, "mic", "Microphone", if muted { 0.0 } else { d.volume }, move |s, v| {
            if let Some(a) = &s.sh.services.lock().unwrap().audio {
                if muted && v > 0.0 {
                    a.send(AudioCmd::SourceMute(false));
                }
                a.send(AudioCmd::SourceVolume(v));
            }
        });
        out = out.child(slider_row(mute, track, None));
    }
    if let Some(b) = brightness_now() {
        let track = slider(t, "brightness", "Brightness", b, |s, v| set_brightness(s, v));
        out = out.child(slider_row(cell(28.0, 28.0, glyph_el(t, "sun", 18.0, t.ink)), track, None));
    }

    if let Some(p) = player {
        let playing = p.status == Status::Playing;
        let name = if p.title.is_empty() { p.identity.clone() } else { p.title.clone() };
        let by = if p.artist.is_empty() { p.identity.clone() } else { p.artist.clone() };
        let base = row().size(44.0, 44.0).center().radius(t.radii.chip).fill(t.fill);
        let base = if matches!(t.look, Look::Folio | Look::Phosphor) { base.border(1.0, t.ink) } else { base };
        let art = crate::media::cover(t, &s.sh, &p, base, 44.0);
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
        if playing {
            crate::media::keep_time(&s.sh);
        }
    }

    if let Some(p) = pw.as_ref().filter(|p| !p.profiles.is_empty()) {
        let current = p.profile.clone().unwrap_or_default();
        out = out.child(col().w_full().px(2.0).child(crate::bluetooth::profiles(t, &current, &p.profiles)));
    }

    let foot = if power_open() {
        let c = t.ink;
        let ask = |a: Action| move |s: &mut Stage| {
            POWER_UNTIL.store(0, Ordering::Relaxed);
            *CONFIRM.lock().unwrap() = Some((a, now() + 30));
            s.sh.redraw_after(Duration::from_secs(31));
        };
        row().w_full().mt(8.0).pt(6.0).gap(10.0).justify_end().center_y()
            .child(pill_button(t, "logout", "Log out", c, ask(Action::Logout)))
            .child(pill_button(t, "reboot", "Restart", c, ask(Action::Reboot)))
            .child(pill_button(t, "poweroff", "Shut down", t.alert, ask(Action::Poweroff)))
            .child(round_button(t, "cancel", "Cancel", "x", 44.0, true, |_| POWER_UNTIL.store(0, Ordering::Relaxed)))
    } else {
        row().w_full().mt(8.0).pt(6.0).gap(10.0).justify_end()
            .child(round_button(t, "settings", "Settings", "gear", 44.0, true, |s| {
                let c = s.sh.bar_conf().bar.session.settings.clone();
                run(s, &c)
            }))
            .child(round_button(t, "lock", "Lock", "lock", 44.0, true, |s| {
                let c = s.sh.bar_conf().bar.session.lock.clone();
                run(s, &c)
            }))
            .child(round_button(t, "power", "Power", "power", 44.0, true, |s| arm(s, now() + 8)))
    };
    out.child(foot)
}
