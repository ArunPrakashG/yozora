//! Bluetooth devices, peripheral batteries and the system battery.

use std::sync::OnceLock;

use gpui::{
    AnyElement, App, Context, Div, FontWeight, IntoElement, ParentElement, SharedString, StatefulInteractiveElement,
    Styled, div, prelude::*, px,
};
use yozora_ui::{
    Icon, Theme, controls, icon,
    icons::{IconElement, battery_path},
};

use crate::{
    bar::bi,
    popover::Popover,
    services::{
        Services, bluetooth,
        power::{self, Charge, DeviceKind},
        run_shell,
    },
};

fn bt_icon(name: &str) -> Icon {
    match name {
        n if n.contains("headphone") || n.contains("headset") || n.contains("audio") => Icon::Headphones,
        n if n.contains("keyboard") => Icon::Keyboard,
        n if n.contains("mouse") || n.contains("pointing") => Icon::Mouse,
        n if n.contains("gaming") || n.contains("joystick") => Icon::Gamepad,
        n if n.contains("phone") => Icon::Phone,
        _ => Icon::Bluetooth,
    }
}

fn peripheral_icon(kind: DeviceKind) -> Icon {
    match kind {
        DeviceKind::Mouse => Icon::Mouse,
        DeviceKind::Keyboard => Icon::Keyboard,
        DeviceKind::Headphones => Icon::Headphones,
        DeviceKind::Gamepad => Icon::Gamepad,
        DeviceKind::Phone => Icon::Phone,
        DeviceKind::Other => Icon::Battery,
    }
}

pub fn battery_glyph(percent: f32, charging: bool, size: f32) -> impl IntoElement {
    IconElement::from_path(battery_path(percent, charging), size)
}

/// The lowest battery among connected Bluetooth devices and UPower peripherals.
fn lowest_battery(cx: &App) -> Option<f32> {
    let s = Services::get(cx);
    let bt = s.bluetooth.as_ref().map(|b| &b.read(cx).state);
    let from_bt = bt.into_iter().flat_map(|b| b.connected().filter_map(|d| d.battery.map(f32::from)));
    let from_power = s.power.as_ref().into_iter().flat_map(|p| p.read(cx).state.peripherals.iter().map(|p| p.percent));
    from_bt.chain(from_power).min_by(f32::total_cmp)
}

pub fn item(theme: &Theme, cx: &App) -> Option<Div> {
    let bt = Services::get(cx).bluetooth.as_ref().map(|b| b.read(cx));
    let has_adapter = bt.is_some_and(|b| b.state.adapter.is_some());
    let on = bt.is_some_and(|b| b.state.powered);
    let low = lowest_battery(cx);
    if !has_adapter && low.is_none() {
        return None;
    }
    Some(
        bi().when(has_adapter, |el| {
            el.child(icon(if on { Icon::Bluetooth } else { Icon::BluetoothOff }).text_color(if on {
                theme.ink
            } else {
                theme.ink_3
            }))
        })
        .when_some(low, |el, pct| {
            el.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(5.))
                    .text_size(px(12.5))
                    .text_color(if pct <= 15. { theme.danger } else { theme.ink_2 })
                    .child(battery_glyph(pct, false, 18.))
                    .child(format!("{}%", pct.round())),
            )
        }),
    )
}

pub fn battery_item(theme: &Theme, cx: &App) -> Option<Div> {
    let power = Services::get(cx).power.as_ref()?.read(cx);
    let b = power.state.battery.as_ref()?;
    let charging = matches!(b.charge, Charge::Charging | Charge::Full);
    Some(
        bi().gap(px(5.))
            .text_size(px(12.5))
            .text_color(if b.percent <= 15. && !charging { theme.danger } else { theme.ink })
            .child(battery_glyph(b.percent, charging, 20.))
            .child(format!("{}%", b.percent.round())),
    )
}

fn send_bt(cmd: bluetooth::Cmd, cx: &App) {
    if let Some(b) = &Services::get(cx).bluetooth {
        b.read(cx).send(cmd);
    }
}

fn which(program: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|paths| std::env::split_paths(&paths).any(|d| d.join(program).is_file()))
}

/// A Bluetooth manager to hand off to, if one is installed.
fn bluetooth_manager() -> Option<&'static str> {
    static FOUND: OnceLock<Option<&'static str>> = OnceLock::new();
    *FOUND.get_or_init(|| ["blueman-manager", "overskride", "blueberry"].into_iter().find(|p| which(p)))
}

pub fn popover(_p: &mut Popover, theme: &Theme, cx: &mut Context<Popover>) -> Vec<AnyElement> {
    let services = Services::get(cx);
    let bt = services.bluetooth.as_ref().map(|b| b.read(cx));
    let state = bt.map(|b| b.state.clone()).unwrap_or_default();
    let fake = bt.is_some_and(|b| b.fake);
    let peripherals = services.power.as_ref().map(|p| p.read(cx).state.peripherals.clone()).unwrap_or_default();
    let mut rows: Vec<AnyElement> = Vec::new();

    let on = state.powered;
    let header_switch = state.adapter.is_some().then(|| {
        controls::switch("bt", on, theme).on_click(move |_, _, cx| send_bt(bluetooth::Cmd::SetPowered(!on), cx))
    });
    rows.push(controls::header("Bluetooth", div().children(header_switch)).into_any_element());

    if state.adapter.is_none() {
        rows.push(
            controls::empty(&["No Bluetooth adapter.", "BlueZ is not running or found no hardware."], theme)
                .into_any_element(),
        );
    } else if !on {
        rows.push(
            controls::empty(&["Bluetooth is off.", "Wired devices that report a battery still show here."], theme)
                .into_any_element(),
        );
    } else {
        let connected: Vec<_> = state.devices.iter().filter(|d| d.connected).cloned().collect();
        let paired: Vec<_> = state.devices.iter().filter(|d| !d.connected && d.paired).cloned().collect();
        let device_row = |i: usize, d: &bluetooth::Device, prefix: &'static str| {
            let path = d.path.clone();
            let connected = d.connected;
            let sub: SharedString = if d.busy {
                if connected { "Disconnecting…" } else { "Connecting…" }.into()
            } else if connected {
                match d.battery {
                    Some(b) => format!("Connected · {b}%").into(),
                    None => "Connected".into(),
                }
            } else {
                "Not connected".into()
            };
            controls::row((prefix, i), theme)
                .cursor_pointer()
                .child(controls::row_icon(bt_icon(&d.icon), connected, theme))
                .child(controls::row_text(d.name.clone(), Some(sub), theme))
                .when_some(d.battery.filter(|_| connected), |el, b| {
                    el.child(div().text_color(theme.ink_2).child(battery_glyph(f32::from(b), false, 18.)))
                })
                .on_click(move |_, _, cx| {
                    let cmd = if connected {
                        bluetooth::Cmd::Disconnect(path.clone())
                    } else {
                        bluetooth::Cmd::Connect(path.clone())
                    };
                    send_bt(cmd, cx)
                })
                .into_any_element()
        };
        if !connected.is_empty() {
            let list: Vec<_> = connected.iter().enumerate().map(|(i, d)| device_row(i, d, "bt-on")).collect();
            rows.push(div().flex().flex_col().children(list).into_any_element());
        }
        if !paired.is_empty() {
            rows.push(controls::label("Paired", theme).into_any_element());
            let list: Vec<_> = paired.iter().enumerate().map(|(i, d)| device_row(i, d, "bt-paired")).collect();
            rows.push(div().flex().flex_col().children(list).into_any_element());
        }
        if connected.is_empty() && paired.is_empty() {
            rows.push(
                controls::empty(&["No paired devices.", "Pair new devices with a Bluetooth manager."], theme)
                    .into_any_element(),
            );
        }
    }

    // UPower peripherals Bluetooth didn't already show.
    let shown: Vec<&bluetooth::Device> = state.devices.iter().filter(|d| d.connected).collect();
    let extra: Vec<_> = peripherals
        .iter()
        .filter(|p| {
            !shown.iter().any(|d| {
                p.address.as_ref().is_some_and(|a| a.eq_ignore_ascii_case(&d.address))
                    || p.name.eq_ignore_ascii_case(&d.name)
            })
        })
        .collect();
    if !extra.is_empty() {
        rows.push(controls::label("Batteries", theme).into_any_element());
        let list = extra.iter().enumerate().map(|(i, p)| {
            controls::row(("peripheral", i), theme)
                .child(controls::row_icon(peripheral_icon(p.kind), false, theme))
                .child(controls::row_text(p.name.clone(), Some(format!("{}%", p.percent.round()).into()), theme))
                .child(div().text_color(theme.ink_2).child(battery_glyph(p.percent, false, 18.)))
                .into_any_element()
        });
        rows.push(div().flex().flex_col().children(list).into_any_element());
    }

    let note = if fake { "BlueZ · UPower · fake" } else { "BlueZ · UPower" };
    let action = bluetooth_manager().map(|program| {
        controls::link("bt-settings", "Bluetooth settings…", theme).on_click(cx.listener(move |p, _, _, cx| {
            run_shell(program);
            p.close(cx);
        }))
    });
    rows.push(controls::footer(note, div().children(action), theme).into_any_element());
    rows
}

fn duration_label(seconds: i64) -> String {
    let m = seconds / 60;
    if m >= 60 { format!("{} h {} min", m / 60, m % 60) } else { format!("{m} min") }
}

pub fn send_power(cmd: power::Cmd, cx: &App) {
    if let Some(p) = &Services::get(cx).power {
        p.read(cx).send(cmd);
    }
}

/// The power profile picker, shared with the control center.
pub fn profile_picker(theme: &Theme, cx: &App) -> Option<AnyElement> {
    let power = Services::get(cx).power.as_ref()?.read(cx);
    let profiles = power.state.profiles.clone();
    if profiles.is_empty() {
        return None;
    }
    let current = power.state.profile.clone().unwrap_or_default();
    let selected = profiles.iter().position(|p| *p == current).unwrap_or(0);
    let names: Vec<String> = profiles.iter().map(|p| power::profile_label(p).to_string()).collect();
    let labels: Vec<&str> = names.iter().map(String::as_str).collect();
    Some(
        div()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .justify_between()
                    .px(px(4.))
                    .pb(px(7.))
                    .text_size(px(12.))
                    .text_color(theme.ink_2)
                    .child("Power profile")
                    .child(
                        div()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(theme.ink)
                            .child(power::profile_label(&current).to_string()),
                    ),
            )
            .child(controls::segmented("profile", &labels, selected, theme, move |i, _, cx| {
                if let Some(p) = profiles.get(i) {
                    send_power(power::Cmd::SetProfile(p.clone()), cx);
                }
            }))
            .into_any_element(),
    )
}

pub fn battery_popover(_p: &mut Popover, theme: &Theme, cx: &mut Context<Popover>) -> Vec<AnyElement> {
    let Some(power) = Services::get(cx).power.as_ref().map(|p| p.read(cx)) else { return Vec::new() };
    let state = power.state.clone();
    let mut rows: Vec<AnyElement> = Vec::new();
    match &state.battery {
        Some(b) => {
            let status = match b.charge {
                Charge::Charging => "Charging",
                Charge::Discharging => "On battery",
                Charge::Full => "Fully charged",
                Charge::NotCharging => "Plugged in, not charging",
            };
            let left = b.seconds_left.filter(|s| *s > 0).map(|s| match b.charge {
                Charge::Charging => format!("{} until full", duration_label(s)),
                _ => format!("{} left", duration_label(s)),
            });
            rows.push(
                div()
                    .flex()
                    .items_end()
                    .justify_between()
                    .px(px(6.))
                    .pt(px(4.))
                    .pb(px(12.))
                    .child(
                        div().flex().flex_col().child(super::calendar::eyebrow(status, theme)).child(
                            div()
                                .text_size(px(22.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .mt(px(3.))
                                .child(format!("{}%", b.percent.round())),
                        ),
                    )
                    .children(left.map(|l| controls::muted(l, theme).pb(px(2.))))
                    .into_any_element(),
            );
        }
        None => rows.push(controls::header("Power", div()).into_any_element()),
    }
    rows.extend(profile_picker(theme, cx));
    if !state.peripherals.is_empty() {
        rows.push(controls::label("Devices", theme).into_any_element());
        let list = state.peripherals.iter().enumerate().map(|(i, p)| {
            controls::row(("peripheral", i), theme)
                .child(controls::row_icon(peripheral_icon(p.kind), false, theme))
                .child(controls::row_text(p.name.clone(), Some(format!("{}%", p.percent.round()).into()), theme))
                .into_any_element()
        });
        rows.push(div().flex().flex_col().children(list).into_any_element());
    }
    rows.push(controls::footer(if power.fake { "UPower · fake" } else { "UPower" }, div(), theme).into_any_element());
    rows
}
