//! Network status from NetworkManager: Wi‑Fi networks, wired links and VPNs.

use gpui::{
    AnyElement, App, Context, Div, IntoElement, ParentElement, SharedString, StatefulInteractiveElement, Styled, div,
    prelude::*, px,
};
use yozora_ui::{Icon, Theme, controls, icon};

use crate::{
    bar::bi,
    popover::Popover,
    services::{
        Services,
        network::{Cmd, Kind, State},
        run_shell,
    },
};

pub fn glyph(state: &State) -> Icon {
    match state.primary.as_ref().map(|p| p.kind) {
        Some(Kind::Wifi) => Icon::Wifi,
        Some(Kind::Ethernet) => Icon::Ethernet,
        Some(Kind::Vpn) => Icon::Vpn,
        Some(Kind::Other) => Icon::Globe,
        None if state.wifi_device && !state.wifi_enabled => Icon::WifiOff,
        None => Icon::NetworkOff,
    }
}

/// A one-line description, for the control center tile.
pub fn summary(state: &State) -> String {
    match &state.primary {
        Some(p) => p.name.clone(),
        None if state.activating.is_some() => "Connecting…".into(),
        None if state.wifi_device && !state.wifi_enabled => "Off".into(),
        None => "Not connected".into(),
    }
}

pub fn item(theme: &Theme, cx: &App) -> Option<Div> {
    let net = Services::get(cx).network.as_ref()?.read(cx);
    let state = &net.state;
    let dim = !net.up() || state.primary.is_none();
    Some(bi().child(icon(glyph(state)).text_color(if dim { theme.ink_3 } else { theme.ink })))
}

pub fn send(cmd: Cmd, cx: &App) {
    if let Some(n) = &Services::get(cx).network {
        n.read(cx).send(cmd);
    }
}

/// Signal strength as an opacity, so weak networks look weak.
fn strength_opacity(strength: u8) -> f32 {
    match strength {
        0..25 => 0.35,
        25..50 => 0.55,
        50..75 => 0.8,
        _ => 1.0,
    }
}

pub fn popover(_p: &mut Popover, theme: &Theme, cx: &mut Context<Popover>) -> Vec<AnyElement> {
    let Some(net) = Services::get(cx).network.clone() else { return Vec::new() };
    let service = net.read(cx);
    let state = service.state.clone();
    let mut rows: Vec<AnyElement> = Vec::new();
    if let Some(error) = &service.error {
        rows.push(controls::header("Network", div()).into_any_element());
        rows.push(controls::empty(&["NetworkManager is not available.", error.as_str()], theme).into_any_element());
        return rows;
    }

    if state.wifi_device {
        let on = state.wifi_enabled;
        rows.push(
            controls::header(
                "Wi‑Fi",
                controls::switch("wifi", on, theme).on_click(move |_, _, cx| send(Cmd::SetWifi(!on), cx)),
            )
            .into_any_element(),
        );
        if on {
            let (active, others): (Vec<_>, Vec<_>) = state.access_points.iter().partition(|a| a.active);
            for ap in active {
                rows.push(
                    controls::row("wifi-active", theme)
                        .bg(theme.fill)
                        .child(controls::row_icon(Icon::Wifi, true, theme))
                        .child(controls::row_text(
                            ap.ssid.clone(),
                            Some(security(ap.secured, &ap.security, true).into()),
                            theme,
                        ))
                        .child(controls::check("wifi-check", true, theme))
                        .on_click(|_, _, cx| send(Cmd::Disconnect, cx))
                        .into_any_element(),
                );
            }
            if !others.is_empty() {
                rows.push(controls::label("Other networks", theme).into_any_element());
            }
            let list = others.into_iter().take(8).enumerate().map(|(i, ap)| {
                let ssid = ap.ssid.clone();
                let connecting = state.activating.as_deref() == Some(ap.ssid.as_str());
                let sub = if connecting {
                    Some(SharedString::from("Connecting…"))
                } else if !ap.secured {
                    Some("Open network".into())
                } else if ap.known {
                    Some("Saved".into())
                } else {
                    None
                };
                controls::row(("ap", i), theme)
                    .cursor_pointer()
                    .child(controls::row_text(ap.ssid.clone(), sub, theme))
                    .when(ap.secured, |el| el.child(icon(Icon::Lock).size(px(12.)).text_color(theme.ink_3)))
                    .child(icon(Icon::Wifi).opacity(strength_opacity(ap.strength)))
                    .on_click(move |_, _, cx| send(Cmd::Connect(ssid.clone()), cx))
                    .into_any_element()
            });
            rows.push(div().flex().flex_col().children(list).into_any_element());
            if state.access_points.is_empty() {
                rows.push(controls::empty(&["Looking for networks…"], theme).into_any_element());
            }
        } else {
            rows.push(controls::empty(&["Wi‑Fi is off.", "Wired connections stay up."], theme).into_any_element());
        }
    }

    if let Some(wired) = &state.wired {
        if state.wifi_device {
            rows.push(controls::separator(theme).into_any_element());
        } else {
            rows.push(controls::header("Network", div()).into_any_element());
        }
        let sub = if wired.connected { format!("Connected · {}", wired.interface) } else { "Cable unplugged".into() };
        rows.push(
            controls::row("wired", theme)
                .child(controls::row_icon(Icon::Ethernet, wired.connected, theme))
                .child(controls::row_text("Ethernet", Some(sub.into()), theme))
                .into_any_element(),
        );
    }
    if !state.wifi_device && state.wired.is_none() {
        rows.push(controls::header("Network", div()).into_any_element());
        rows.push(controls::empty(&["No network devices."], theme).into_any_element());
    }

    if !state.vpns.is_empty() {
        rows.push(controls::separator(theme).into_any_element());
        let vpns = state.vpns.iter().enumerate().map(|(i, vpn)| {
            let (uuid, on) = (vpn.uuid.clone(), vpn.active);
            let sub = if vpn.connecting {
                "Connecting…"
            } else if on {
                "Connected"
            } else {
                "Not connected"
            };
            controls::row(("vpn", i), theme)
                .child(controls::row_icon(Icon::Vpn, on, theme))
                .child(controls::row_text(vpn.name.clone(), Some(sub.into()), theme))
                .child(controls::switch(("vpn-switch", i), on || vpn.connecting, theme))
                .on_click(move |_, _, cx| send(Cmd::SetVpn(uuid.clone(), !on), cx))
                .into_any_element()
        });
        rows.push(div().flex().flex_col().children(vpns).into_any_element());
    }

    rows.push(
        controls::footer(
            if service.fake { "NetworkManager · fake" } else { "NetworkManager" },
            controls::link("net-settings", "Network settings…", theme).on_click(cx.listener(|p, _, _, cx| {
                run_shell("nm-connection-editor");
                p.close(cx);
            })),
            theme,
        )
        .into_any_element(),
    );
    rows
}

fn security(secured: bool, security: &str, active: bool) -> String {
    let mut s = if active { "Connected".to_string() } else { String::new() };
    if secured && !security.is_empty() {
        if !s.is_empty() {
            s.push_str(" · ");
        }
        s.push_str(security);
    }
    s
}
