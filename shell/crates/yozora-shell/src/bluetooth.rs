//! Bluetooth and power: the adapter switch, paired devices with their batteries, the system
//! battery, and the power profile.

use rune::{Color, El, LineCap, Shape, canvas, col, row};
use yozora_services::bluetooth::{Cmd, Device, State};
use yozora_services::power::{self, Charge, DeviceKind};
use yozora_style::{Look, Tokens};

use crate::kit::{cell, fig, glyph_el, label};
use crate::stage::Stage;
use crate::wifi::{empty, head, ink, item, lead, section, sep, shell, switch_button, text_el, tool_row};

pub const W: f32 = 340.0;

fn device_glyph(icon: &str) -> &'static str {
    match icon {
        n if n.contains("headphone") || n.contains("headset") => "phones",
        n if n.contains("keyboard") => "kbd",
        n if n.contains("mouse") || n.contains("pointing") => "mouse",
        n if n.contains("speaker") => "speaker",
        n if n.contains("audio") => "phones",
        _ => "bt",
    }
}

fn peripheral_glyph(kind: DeviceKind) -> &'static str {
    match kind {
        DeviceKind::Mouse => "mouse",
        DeviceKind::Keyboard => "kbd",
        DeviceKind::Headphones => "phones",
        DeviceKind::Gamepad | DeviceKind::Phone => "bt",
        DeviceKind::Other => "battery",
    }
}

/// A battery level as a ring with the number in it.
pub(crate) fn ring(t: &Tokens, percent: f32) -> El {
    let (r, c) = (12.0_f32, 15.0_f32);
    let at = |a: f32| [c + r * a.cos(), c + r * a.sin()];
    let start = -std::f32::consts::FRAC_PI_2;
    let frac = (percent / 100.0).clamp(0.0, 1.0);
    let n = (frac * 48.0).ceil().max(2.0) as usize;
    let arc: Vec<[f32; 2]> = (0..=n).map(|i| at(start + frac * std::f32::consts::TAU * i as f32 / n as f32)).collect();
    let track: Vec<[f32; 2]> = (0..48).map(|i| at(start + std::f32::consts::TAU * i as f32 / 48.0)).collect();
    let (cap, level) = match t.look {
        Look::Folio | Look::Phosphor => (LineCap::Butt, t.ink),
        _ => (LineCap::Round, t.acc),
    };
    let shape = Shape::new().stroke_with(track, true, 2.5, LineCap::Butt, t.rule).stroke_with(arc, false, 2.5, cap, level);
    canvas(shape).size(30.0, 30.0).center().child(fig(t, 9.0, 600.0, format!("{}", percent.round() as i32), t.ink2))
}

fn send(s: &Stage, cmd: Cmd) {
    if let Some(b) = &s.sh.services.lock().unwrap().bluetooth {
        b.send(cmd);
    }
}

fn device_row(t: &Tokens, i: usize, d: &Device, near: bool) -> El {
    let connected = d.connected;
    let sub = if d.busy {
        if connected { "Disconnecting\u{2026}" } else { "Connecting\u{2026}" }
    } else if connected {
        "Connected"
    } else if near {
        "Ready to pair"
    } else {
        "Not connected"
    };
    let (main, _) = ink(t, false);
    let faint = Color { a: main.a * 0.45, ..main };
    let trail = match (d.busy, connected, d.battery) {
        (true, ..) => None,
        (_, true, Some(b)) => Some(ring(t, f32::from(b))),
        (_, true, None) => None,
        _ => Some(label(t, 13.0, 400.0, if near { "Pair" } else { "Connect" }, t.ink2)),
    };
    let path = d.path.clone();
    item(t, false, lead(t, device_glyph(&d.icon), if connected { main } else { faint }), &d.name, Some(sub), trail)
        .key(format!("{}{i}", if near { "near" } else { "dev" })).role("button")
        .label(format!("{} {}", if connected { "Disconnect" } else if near { "Pair" } else { "Connect" }, d.name)).hit()
        .on_press(move |s: &mut Stage| send(s, if connected { Cmd::Disconnect(path.clone()) } else { Cmd::Connect(path.clone()) }))
}

fn duration(seconds: i64) -> String {
    let m = seconds / 60;
    if m >= 60 { format!("{} h {} min", m / 60, m % 60) } else { format!("{m} min") }
}

/// The power profile picker, shared with the control center.
pub(crate) fn profiles(t: &Tokens, current: &str, names: &[String]) -> El {
    let mut r = row().w_full().padding(3.0).gap(2.0).radius(t.radii.pill);
    r = match t.look {
        Look::Folio => r.padding(0.0).gap(0.0).border(1.0, t.ink),
        Look::Phosphor => r.padding(0.0).gap(0.0).border(1.0, t.rule),
        Look::Prism => r.fill(Color { a: 0.06, ..t.ink }),
        _ => r.fill(t.raised),
    };
    for (i, name) in names.iter().enumerate() {
        let on = name == current;
        let text_color = if !on {
            t.ink2
        } else if matches!(t.look, Look::Folio | Look::Phosphor) {
            t.on_ink
        } else {
            t.ink
        };
        let words = power::profile_label(name);
        let words = match t.look {
            Look::Folio => text_el(t, 11.0, 500.0, words.to_uppercase(), 0.9, false).color(text_color),
            Look::Phosphor => label(t, 12.0, 400.0, words, text_color),
            _ => label(t, 13.0, 500.0, words, text_color),
        };
        let mut b = row().grow(1.0).h(30.0).center().radius(t.radii.pill).child(words);
        if on {
            b = match t.look {
                Look::Folio | Look::Phosphor => b.fill(t.ink),
                Look::Prism => b.fill(t.fill).shadow(1.0, 4.0, Color { a: 0.18, ..t.ink }),
                _ => b.fill(t.fill),
            };
        }
        let pick = name.clone();
        r = r.child(
            b.key(format!("profile{i}")).role("button").label(format!("Power mode {}", power::profile_label(name))).hit()
                .on_press(move |s: &mut Stage| {
                    if let Some(p) = &s.sh.services.lock().unwrap().power {
                        p.send(power::Cmd::SetProfile(pick.clone()));
                    }
                }),
        );
    }
    r
}

pub fn view(s: &Stage, t: &Tokens) -> El {
    let (state, error) = match s.sh.services.lock().unwrap().bluetooth.as_ref() {
        Some(b) => {
            let snap = b.read();
            (snap.state.clone(), snap.error.clone())
        }
        None => (State::default(), None),
    };
    let power = s.sh.services.lock().unwrap().power.as_ref().map(|p| p.read().state.clone()).unwrap_or_default();

    let mut p = shell(t, W, "bluetooth");
    let on = state.powered;
    let toggle = (state.adapter.is_some() && error.is_none())
        .then(|| switch_button(t, "bt-toggle", "Bluetooth", on, move |s| send(s, Cmd::SetPowered(!on))));
    p = p.child(head(t, "Bluetooth", toggle));

    if let Some(e) = error {
        p = p.child(empty(t, &["Bluetooth is not available.", &e]));
    } else if state.adapter.is_none() {
        p = p.child(empty(t, &["No Bluetooth adapter.", "BlueZ is not running or found no hardware."]));
    } else if !on {
        p = p.child(empty(t, &["Bluetooth is off.", "Wired devices that report a battery still show here."]));
    } else {
        let scanning = state.scanning;
        let paired: Vec<&Device> = state.devices.iter().filter(|d| d.connected).chain(state.devices.iter().filter(|d| !d.connected && d.paired)).collect();
        let nearby: Vec<&Device> = state.devices.iter().filter(|d| !d.connected && !d.paired).collect();
        let scan = {
            let button = cell(28.0, 28.0, glyph_el(t, "sync", 15.0, if scanning { t.acc } else { t.ink2 }))
                .key("bt-scan").role("button").label(if scanning { "Stop scanning" } else { "Scan for devices" }).hit()
                .radius(t.radii.pill).on_press(move |s: &mut Stage| send(s, Cmd::Scan(!scanning)));
            row().gap(4.0).center_y()
                .children(scanning.then(|| label(t, 12.0, 400.0, "Scanning\u{2026}", t.ink2)))
                .child(button)
        };
        p = p.child(section(t, "Paired", Some(scan)));
        if paired.is_empty() {
            p = p.child(empty(t, &["No paired devices.", "Scan to find a device to pair."]));
        } else {
            let mut list = col().w_full().gap(10.0);
            for (i, d) in paired.into_iter().enumerate() {
                list = list.child(device_row(t, i, d, false));
            }
            p = p.child(list);
        }
        if scanning || !nearby.is_empty() {
            p = p.child(section(t, "Nearby", None));
            if nearby.is_empty() {
                p = p.child(empty(t, &["Looking for devices\u{2026}"]));
            } else {
                let mut list = col().w_full().gap(10.0);
                for (i, d) in nearby.into_iter().enumerate() {
                    list = list.child(device_row(t, i, d, true));
                }
                p = p.child(list);
            }
        }
    }

    // The system battery, and peripherals that report through UPower but not through Bluetooth.
    let shown: Vec<&Device> = state.devices.iter().filter(|d| d.connected).collect();
    let extra: Vec<_> = power.peripherals.iter().filter(|q| {
        !shown.iter().any(|d| q.address.as_ref().is_some_and(|a| a.eq_ignore_ascii_case(&d.address)) || q.name.eq_ignore_ascii_case(&d.name))
    }).collect();
    if power.battery.is_some() || !extra.is_empty() {
        p = p.child(section(t, "Battery", None));
        let (main, _) = ink(t, false);
        if let Some(b) = &power.battery {
            let status = match b.charge {
                Charge::Charging => "Charging",
                Charge::Discharging => "On battery",
                Charge::Full => "Fully charged",
                Charge::NotCharging => "Plugged in, not charging",
            };
            let sub = match b.seconds_left.filter(|s| *s > 0) {
                Some(s) if b.charge == Charge::Charging => format!("{status} \u{b7} {} until full", duration(s)),
                Some(s) => format!("{status} \u{b7} {} left", duration(s)),
                None => status.to_owned(),
            };
            p = p.child(item(t, false, glyph_el(t, "battery", 18.0, main), &format!("{}%", b.percent.round()), Some(&sub), None).key("battery"));
        }
        for (i, q) in extra.into_iter().enumerate() {
            p = p.child(item(t, false, lead(t, peripheral_glyph(q.kind), main), &q.name, None, Some(ring(t, q.percent))).key(format!("peripheral{i}")));
        }
    }

    if !power.profiles.is_empty() {
        let current = power.profile.clone().unwrap_or_default();
        p = p.child(section(t, "Power mode", None).mt(6.0)).child(col().w_full().px(2.0).child(profiles(t, &current, &power.profiles)));
    }
    if state.adapter.is_some() && let Some(row) = tool_row(t, "bt-manager", "Bluetooth manager\u{2026}", &["blueman-manager", "overskride", "blueberry"]) {
        p = p.child(sep(t)).child(row);
    }
    p
}
