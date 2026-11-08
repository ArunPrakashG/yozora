//! The battery popover: charge and time left, the power mode, and batteries in peripherals.

use rune::{El, col, row};
use yozora_services::power::{self, Charge, DeviceKind};
use yozora_style::{Look, Tokens};

use crate::bluetooth::{profiles, ring};
use crate::kit::{fig, glyph_el, label};
use crate::stage::Stage;
use crate::wifi::{empty, head, ink, item, lead, section, shell, text_el};

pub const W: f32 = 340.0;

/// At or under this charge a battery that isn't charging is drawn as an alert.
pub const LOW: f32 = 15.0;

fn duration(seconds: i64) -> String {
    let m = seconds / 60;
    if m >= 60 { format!("{} h {} min", m / 60, m % 60) } else { format!("{m} min") }
}

pub fn status(charge: Charge) -> &'static str {
    match charge {
        Charge::Charging => "Charging",
        Charge::Discharging => "On battery",
        Charge::Full => "Fully charged",
        Charge::NotCharging => "Plugged in, not charging",
    }
}

/// "On battery \u{b7} 2 h 10 min left", for the popover and the tooltip.
pub fn summary(b: &power::Battery) -> String {
    let status = status(b.charge);
    match b.seconds_left.filter(|s| *s > 0) {
        Some(s) if b.charge == Charge::Charging => format!("{status} \u{b7} {} until full", duration(s)),
        Some(s) => format!("{status} \u{b7} {} left", duration(s)),
        None => status.to_owned(),
    }
}

pub fn is_low(b: &power::Battery) -> bool {
    b.percent <= LOW && !matches!(b.charge, Charge::Charging | Charge::Full)
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

/// The big number: a ring, the charge and what it is doing.
fn hero(t: &Tokens, b: &power::Battery) -> El {
    let charging = matches!(b.charge, Charge::Charging);
    let color = if is_low(b) { t.alert } else { t.ink };
    let figure = match t.look {
        Look::Folio => fig(t, 34.0, 500.0, format!("{}%", b.percent.round()), color),
        _ => fig(t, 30.0, 600.0, format!("{}%", b.percent.round()), color),
    };
    let words = col().grow(1.0).min_w(0.0).gap(2.0).child(figure).child(label(t, 13.0, 400.0, summary(b), t.ink2).truncate());
    let icon = glyph_el(t, if charging { "battcharge" } else { "battery" }, 26.0, color);
    row().w_full().min_h(64.0).px(14.0).py(6.0).gap(14.0).center_y().radius(t.radii.card)
        .child(icon)
        .child(words)
        .key("battery-level").role("status").label(format!("Battery {}%", b.percent.round()))
}

pub fn view(s: &Stage, t: &Tokens) -> El {
    let (state, fake, error) = match s.sh.services.lock().unwrap().power.as_ref() {
        Some(p) => {
            let snap = p.read();
            (snap.state.clone(), p.fake, snap.error.clone())
        }
        None => (power::State::default(), false, Some("no service".into())),
    };
    let mut p = shell(t, W, "battery").child(head(t, "Battery", None));

    if let Some(e) = error {
        return p.child(empty(t, &["Power information is not available.", &e]));
    }
    match &state.battery {
        Some(b) => p = p.child(hero(t, b)),
        None => p = p.child(empty(t, &["No battery found.", "This computer runs on mains power."])),
    }

    if !state.profiles.is_empty() {
        let current = state.profile.clone().unwrap_or_default();
        p = p.child(section(t, "Power mode", None).mt(6.0)).child(col().w_full().px(2.0).child(profiles(t, &current, &state.profiles)));
    }

    if !state.peripherals.is_empty() {
        p = p.child(section(t, "Devices", None).mt(6.0));
        let (main, _) = ink(t, false);
        for (i, q) in state.peripherals.iter().enumerate() {
            p = p.child(item(t, false, lead(t, peripheral_glyph(q.kind), main), &q.name, None, Some(ring(t, q.percent))).key(format!("peripheral{i}")));
        }
    }

    let note = if fake { "UPower \u{b7} fake".to_owned() } else { "UPower".to_owned() };
    let note = match t.look {
        Look::Folio | Look::Phosphor => text_el(t, 10.5, 400.0, note.to_uppercase(), 1.0, true),
        _ => label(t, 12.0, 400.0, note, t.ink3),
    };
    p.child(row().w_full().pt(8.0).justify_end().child(note))
}
