//! The control center: quick toggles, power profile, volume, media and session buttons.

use gpui::{
    AnyElement, App, Context, Div, IntoElement, ParentElement, StatefulInteractiveElement, Styled, Window, div,
    prelude::*, px,
};
use yozora_ui::{Icon, Theme, controls, icon};

use super::{SessionAction, devices, media, network, notifications, sound};
use crate::{
    app::{self, Config},
    bar::bi,
    popover::Popover,
    services::{self, Services, bluetooth, run_shell},
};

pub fn item(_theme: &Theme) -> Div {
    bi().child(icon(Icon::ControlCenter))
}

fn tiles(theme: &Theme, cx: &App) -> AnyElement {
    let s = Services::get(cx);
    let mut tiles: Vec<AnyElement> = Vec::new();

    if let Some(net) = s.network.as_ref().map(|n| n.read(cx)) {
        let st = &net.state;
        if st.wifi_device {
            let on = st.wifi_enabled;
            tiles.push(
                controls::tile("tile-wifi", Icon::Wifi, "Wi‑Fi", network::summary(st), on, theme)
                    .on_click(move |_, _, cx| network::send(services::network::Cmd::SetWifi(!on), cx))
                    .into_any_element(),
            );
        } else {
            tiles.push(
                controls::tile("tile-net", network::glyph(st), "Network", network::summary(st), st.connected(), theme)
                    .cursor_default()
                    .into_any_element(),
            );
        }
    }

    if let Some(bt) = s.bluetooth.as_ref().map(|b| b.read(cx)) {
        let st = &bt.state;
        let on = st.powered;
        let detail = if st.adapter.is_none() {
            "Unavailable".to_string()
        } else if !on {
            "Off".into()
        } else {
            st.connected().next().map(|d| d.name.clone()).unwrap_or_else(|| "On".into())
        };
        let available = st.adapter.is_some();
        tiles.push(
            controls::tile(
                "tile-bt",
                if on { Icon::Bluetooth } else { Icon::BluetoothOff },
                "Bluetooth",
                detail,
                on,
                theme,
            )
            .when(!available, |el| el.opacity(0.6).cursor_default())
            .on_click(move |_, _, cx| {
                if available && let Some(b) = &Services::get(cx).bluetooth {
                    b.read(cx).send(bluetooth::Cmd::SetPowered(!on));
                }
            })
            .into_any_element(),
        );
    }

    if let Some(n) = s.notifications.as_ref().map(|n| n.read(cx)) {
        let on = n.state.dnd;
        tiles.push(
            controls::tile("tile-dnd", Icon::Moon, "Do Not Disturb", if on { "On" } else { "Off" }, on, theme)
                .on_click(move |_, _, cx| notifications::send(services::notifications::Cmd::SetDnd(!on), cx))
                .into_any_element(),
        );
    }

    let dark = theme.dark;
    tiles.push(
        controls::tile("tile-dark", Icon::Appearance, "Dark mode", if dark { "On" } else { "Off" }, dark, theme)
            .on_click(move |_, _, cx| app::set_dark_override(Some(!dark), cx))
            .into_any_element(),
    );
    div().grid().grid_cols(2).gap(px(8.)).children(tiles).into_any_element()
}

pub fn popover(p: &mut Popover, theme: &Theme, _window: &mut Window, cx: &mut Context<Popover>) -> Vec<AnyElement> {
    if let Some(action) = p.ui.confirm {
        return super::launcher::confirm(action, theme, cx);
    }
    let mut rows: Vec<AnyElement> = vec![tiles(theme, cx)];
    if let Some(audio) = Services::get(cx).audio.as_ref().map(|a| a.read(cx)) {
        let sliders: Vec<AnyElement> = [
            audio.state.sink().and_then(|_| sound::sink_slider(p, theme, cx)),
            audio.state.source().and_then(|_| sound::source_slider(p, theme, cx)),
        ]
        .into_iter()
        .flatten()
        .collect();
        if !sliders.is_empty() {
            rows.push(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .mt(px(8.))
                    .py(px(6.))
                    .px(px(4.))
                    .children(sliders)
                    .into_any_element(),
            );
        }
    }
    if let Some(mini) = media::mini(theme, cx) {
        rows.push(div().mt(px(8.)).child(mini).into_any_element());
    }
    if let Some(picker) = devices::profile_picker(theme, cx) {
        rows.push(div().mt(px(8.)).child(picker).into_any_element());
    }
    rows.push(div().h(px(8.)).into_any_element());

    let footer = if p.ui.power_choices {
        div().flex().gap(px(6.)).children([SessionAction::Logout, SessionAction::Reboot, SessionAction::Poweroff].map(
            |action| {
                let glyph = match action {
                    SessionAction::Logout => Icon::Close,
                    SessionAction::Reboot => Icon::Sync,
                    _ => Icon::Power,
                };
                controls::footer_button(action.verb(), glyph, action.verb(), theme).on_click(cx.listener(
                    move |p, _, _, cx| {
                        p.ui.confirm = Some(action);
                        cx.notify();
                    },
                ))
            },
        ))
    } else {
        div()
            .flex()
            .gap(px(6.))
            .child(controls::footer_button("cc-settings", Icon::Settings, "Settings", theme).on_click(cx.listener(
                |p, _, _, cx| {
                    super::launcher::open_settings(None, cx);
                    p.close(cx);
                },
            )))
            .child(controls::footer_button("cc-lock", Icon::Lock, "Lock", theme).on_click(cx.listener(
                |p, _, _, cx| {
                    run_shell(&SessionAction::Lock.command(&Config::get(cx).settings.bar.session));
                    p.close(cx);
                },
            )))
            .child(controls::footer_button("cc-power", Icon::Power, "Power", theme).on_click(cx.listener(
                |p, _, _, cx| {
                    p.ui.power_choices = true;
                    cx.notify();
                },
            )))
    };
    rows.push(footer.into_any_element());
    rows
}
