//! Modules: which bar modules show in each zone, in what order. Drag rows to move them.

use gpui::{
    AnyElement, AppContext, Context, FontWeight, InteractiveElement, IntoElement, ParentElement, Render, SharedString,
    StatefulInteractiveElement, Styled, Window, div, prelude::FluentBuilder, px,
};
use yozora_config::bar::{BarConfig, Zones};
use yozora_ui::{
    Icon, Theme,
    controls::{segmented, switch},
    icon_sized,
};

use crate::{
    apps::AppEntry,
    doc::string_array,
    view::{SettingsView, with_alpha},
    widgets::{card, code},
};

pub const SEARCH: &[(&str, &str)] = &[
    ("Preset", "minimal standard full modules"),
    ("Left zone", "modules order launcher workspaces window"),
    ("Right zone", "modules order tray media network sound clock control center"),
    ("Custom modules", "script command json"),
];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Zone {
    Left,
    Center,
    Right,
}

impl Zone {
    const ALL: [Zone; 3] = [Zone::Left, Zone::Center, Zone::Right];

    fn key(self) -> &'static str {
        match self {
            Zone::Left => "modules.left",
            Zone::Center => "modules.center",
            Zone::Right => "modules.right",
        }
    }

    fn name(self) -> &'static str {
        match self {
            Zone::Left => "Left",
            Zone::Center => "Center",
            Zone::Right => "Right",
        }
    }

    fn list(self, zones: &mut Zones) -> &mut Vec<String> {
        match self {
            Zone::Left => &mut zones.left,
            Zone::Center => &mut zones.center,
            Zone::Right => &mut zones.right,
        }
    }
}

/// Every built-in module: id, label, what it shows, icon, and where it goes when switched on.
const CATALOG: &[(&str, &str, &str, Icon, Zone)] = &[
    ("launcher", "Menu", "About, settings, session", Icon::Logo, Zone::Left),
    ("workspaces", "Workspaces", "Hyprland workspaces", Icon::Modules, Zone::Left),
    ("window", "Window title", "The focused window", Icon::Window, Zone::Left),
    ("tray", "Tray", "StatusNotifier icons", Icon::Package, Zone::Right),
    ("media", "Media", "MPRIS players", Icon::Music, Zone::Right),
    ("network", "Network", "NetworkManager", Icon::Wifi, Zone::Right),
    ("sound", "Sound", "Outputs and inputs", Icon::VolumeHigh, Zone::Right),
    ("devices", "Bluetooth", "Devices and their batteries", Icon::Bluetooth, Zone::Right),
    ("battery", "Battery", "UPower and power profiles", Icon::Battery, Zone::Right),
    ("notifications", "Notifications", "mako history, Do Not Disturb", Icon::Bell, Zone::Right),
    ("clock", "Clock", "Time and calendar", Icon::Calendar, Zone::Right),
    ("control-center", "Control center", "Quick settings", Icon::ControlCenter, Zone::Right),
];

const MINIMAL: (&[&str], &[&str]) = (&["workspaces"], &["clock", "control-center"]);
const STANDARD: (&[&str], &[&str]) =
    (&["launcher", "workspaces", "window"], &["media", "network", "sound", "clock", "control-center"]);
const FULL: (&[&str], &[&str]) = (
    &["launcher", "workspaces", "window"],
    &["tray", "media", "network", "sound", "devices", "battery", "notifications", "clock", "control-center"],
);
const PRESETS: [(&[&str], &[&str]); 3] = [MINIMAL, STANDARD, FULL];

struct Info {
    label: String,
    detail: String,
    icon: Icon,
    home: Zone,
}

fn info(name: &str) -> Info {
    if let Some(&(_, label, detail, icon, home)) = CATALOG.iter().find(|c| c.0 == name) {
        return Info { label: label.into(), detail: detail.into(), icon, home };
    }
    if let Some(custom) = name.strip_prefix("custom/") {
        return Info { label: custom.into(), detail: "Custom command".into(), icon: Icon::Terminal, home: Zone::Right };
    }
    Info {
        label: name.into(),
        detail: "Unknown module; the bar skips it".into(),
        icon: Icon::Warning,
        home: Zone::Right,
    }
}

/// A module being dragged.
#[derive(Clone)]
pub struct Dragged {
    name: String,
    label: String,
}

struct Ghost {
    label: String,
    icon: Icon,
}

impl Render for Ghost {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::global(cx);
        div()
            .flex()
            .items_center()
            .gap(px(9.))
            .h(px(36.))
            .px(px(12.))
            .rounded(px(10.))
            .bg(theme.glass_pop)
            .border_1()
            .border_color(theme.accent)
            .shadow(theme.popover_shadow())
            .text_size(px(13.))
            .font_weight(FontWeight::MEDIUM)
            .text_color(theme.ink)
            .font_family(theme.font.clone())
            .child(div().text_color(theme.accent).child(icon_sized(self.icon, 15.)))
            .child(self.label.clone())
    }
}

/// Moves `name` to `zone`, before `before` when given, and enables it.
pub fn place(zones: &mut Zones, name: &str, zone: Zone, before: Option<&str>) {
    for z in Zone::ALL {
        z.list(zones).retain(|n| n != name);
    }
    let list = zone.list(zones);
    let at = before.and_then(|b| list.iter().position(|n| n == b)).unwrap_or(list.len());
    list.insert(at, name.to_string());
}

fn save_zones(view: &mut SettingsView, file: &str, zones: Zones, cx: &mut Context<SettingsView>) {
    view.edit(file, cx, |d| {
        d.edit(|f| {
            f.set(Zone::Left.key(), string_array(&zones.left));
            f.set(Zone::Center.key(), string_array(&zones.center));
            f.set(Zone::Right.key(), string_array(&zones.right));
        })
    });
}

pub fn render(view: &mut SettingsView, app: &AppEntry, cx: &mut Context<SettingsView>) -> AnyElement {
    let theme = Theme::global(cx).clone();
    let file = app.manifest.config.clone();
    let config: BarConfig = view.doc(&file).parse();
    let zones = config.modules.clone();

    let preset = PRESETS.iter().position(|(l, r)| {
        zones.center.is_empty()
            && zones.left.iter().map(String::as_str).eq(l.iter().copied())
            && zones.right.iter().map(String::as_str).eq(r.iter().copied())
    });
    let weak = cx.entity().downgrade();
    let preset_file = file.clone();

    let enabled: Vec<String> = zones.left.iter().chain(&zones.center).chain(&zones.right).cloned().collect();
    let mut known: Vec<String> = CATALOG.iter().map(|c| c.0.to_string()).collect();
    known.extend(config.custom.keys().map(|k| format!("custom/{k}")));

    let zone_card = |zone: Zone, cx: &mut Context<SettingsView>| {
        let mut zones_for_zone = zones.clone();
        let on: Vec<String> = zone.list(&mut zones_for_zone).clone();
        let off: Vec<String> = known.iter().filter(|n| !enabled.contains(n) && info(n).home == zone).cloned().collect();
        let count = on.len();
        let rows: Vec<AnyElement> = on
            .iter()
            .map(|n| (n.clone(), true))
            .chain(off.iter().map(|n| (n.clone(), false)))
            .map(|(name, is_on)| {
                let i = info(&name);
                let (file_drop, file_toggle) = (file.clone(), file.clone());
                let (zones_drop, zones_toggle) = (zones.clone(), zones.clone());
                let (name_drop, name_toggle) = (name.clone(), name.clone());
                let row_id = SharedString::from(format!("mod-{name}"));
                let row = div()
                    .id(row_id)
                    .flex()
                    .items_center()
                    .gap(px(9.))
                    .h(px(40.))
                    .pl(px(6.))
                    .pr(px(8.))
                    .rounded(px(10.))
                    .hover(|s| s.bg(theme.fill))
                    .child(
                        div()
                            .w(px(14.))
                            .flex_none()
                            .text_color(theme.ink_3)
                            .when(is_on, |el| el.cursor_grab())
                            .when(!is_on, |el| el.opacity(0.4))
                            .child(icon_sized(Icon::Grip, 14.)),
                    )
                    .child(
                        div()
                            .size(px(26.))
                            .flex_none()
                            .rounded(px(8.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .bg(if is_on { theme.accent_soft } else { theme.fill_2 })
                            .text_color(if is_on { theme.accent } else { theme.ink_2 })
                            .child(icon_sized(i.icon, 15.)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(if is_on { theme.ink } else { theme.ink_2 })
                                    .truncate()
                                    .child(i.label.clone()),
                            )
                            .child(
                                div()
                                    .font_family(theme.mono.clone())
                                    .text_size(px(10.5))
                                    .text_color(theme.ink_3)
                                    .truncate()
                                    .child(format!("{name} · {}", i.detail)),
                            ),
                    )
                    .child(switch(SharedString::from(format!("sw-{name}")), is_on, &theme).on_click(cx.listener(
                        move |this, _, _, cx| {
                            let mut zones = zones_toggle.clone();
                            if is_on {
                                for z in Zone::ALL {
                                    z.list(&mut zones).retain(|n| n != &name_toggle);
                                }
                            } else {
                                place(&mut zones, &name_toggle, info(&name_toggle).home, None);
                            }
                            save_zones(this, &file_toggle, zones, cx);
                        },
                    )))
                    .drag_over::<Dragged>(move |s, _, _, cx| s.bg(with_alpha(Theme::global(cx).accent, 0x1F)))
                    .on_drop(cx.listener(move |this, dragged: &Dragged, _, cx| {
                        if dragged.name == name_drop {
                            return;
                        }
                        let mut zones = zones_drop.clone();
                        place(&mut zones, &dragged.name, zone, is_on.then_some(name_drop.as_str()));
                        save_zones(this, &file_drop, zones, cx);
                    }));
                let row = if is_on {
                    let dragged = Dragged { name: name.clone(), label: i.label.clone() };
                    let icon = i.icon;
                    row.on_drag(dragged, move |d, _, _, cx| cx.new(|_| Ghost { label: d.label.clone(), icon }))
                } else {
                    row
                };
                row.into_any_element()
            })
            .collect();
        let empty = rows.is_empty();
        let (file_zone, zones_zone) = (file.clone(), zones.clone());
        div()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .justify_between()
                    .items_baseline()
                    .mx(px(2.))
                    .mb(px(8.))
                    .child(
                        div()
                            .text_size(px(11.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme.ink_3)
                            .child(zone.name().to_uppercase()),
                    )
                    .child(
                        div()
                            .font_family(theme.mono.clone())
                            .text_size(px(10.5))
                            .text_color(theme.ink_3)
                            .child(if count == 0 { "empty".to_string() } else { format!("{count} on") }),
                    ),
            )
            .child(
                div()
                    .id(SharedString::from(format!("zone-{}", zone.name())))
                    .flex()
                    .flex_col()
                    .when(!empty, |el| el.p(px(4.)).rounded(px(14.)).bg(theme.fill).border_1().border_color(theme.hair))
                    .when(empty, |el| {
                        el.h(px(46.))
                            .items_center()
                            .justify_center()
                            .flex_row()
                            .gap(px(8.))
                            .rounded(px(12.))
                            .border_1()
                            .border_dashed()
                            .border_color(theme.hair_2)
                            .text_size(px(12.5))
                            .text_color(theme.ink_3)
                            .child(icon_sized(Icon::Plus, 15.))
                            .child("Drag a module here")
                    })
                    .drag_over::<Dragged>(|s, _, _, cx| s.border_color(Theme::global(cx).accent))
                    .on_drop(cx.listener(move |this, dragged: &Dragged, _, cx| {
                        let mut zones = zones_zone.clone();
                        place(&mut zones, &dragged.name, zone, None);
                        save_zones(this, &file_zone, zones, cx);
                    }))
                    .children(rows),
            )
    };

    let custom_names: Vec<String> = config.custom.keys().cloned().collect();
    div()
        .flex()
        .flex_col()
        .child(
            card(&theme).child(crate::widgets::setting_row(
                "Preset",
                Some("A starting point. Switch any module below to customise.".into()),
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .when(preset.is_none(), |el| {
                        el.child(div().text_size(px(12.)).text_color(theme.ink_3).child("Custom"))
                    })
                    .child(div().w(px(270.)).child(segmented(
                        "preset",
                        &["Minimal", "Standard", "Full"],
                        preset.unwrap_or(1),
                        &theme,
                        move |i, _, cx| {
                            let (l, r) = PRESETS[i];
                            let zones = Zones {
                                left: l.iter().map(|s| s.to_string()).collect(),
                                center: Vec::new(),
                                right: r.iter().map(|s| s.to_string()).collect(),
                            };
                            let file = preset_file.clone();
                            weak.update(cx, |this, cx| save_zones(this, &file, zones, cx)).ok();
                        },
                    ))),
                true,
                &theme,
            )),
        )
        .child(
            div()
                .mt(px(20.))
                .flex()
                .gap(px(16.))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(16.))
                        .child(zone_card(Zone::Left, cx))
                        .child(zone_card(Zone::Center, cx))
                        .child(add_card(&custom_names, &theme)),
                )
                .child(div().flex_1().min_w_0().child(zone_card(Zone::Right, cx))),
        )
        .into_any_element()
}

fn add_card(custom: &[String], theme: &Theme) -> impl IntoElement + use<> {
    div()
        .flex()
        .gap(px(12.))
        .items_start()
        .p(px(14.))
        .rounded(px(14.))
        .bg(theme.fill)
        .border_1()
        .border_color(theme.hair)
        .child(
            div()
                .size(px(30.))
                .flex_none()
                .rounded(px(9.))
                .flex()
                .items_center()
                .justify_center()
                .bg(theme.accent)
                .text_color(theme.accent_ink)
                .child(icon_sized(Icon::Plus, 16.)),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(2.))
                .min_w_0()
                .child(div().text_size(px(13.)).font_weight(FontWeight::SEMIBOLD).child("Add a module"))
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(theme.ink_2)
                        .child("Any command that prints one JSON object per line can be a module. Add it to bar.toml and it shows up here:"),
                )
                .child(div().mt(px(4.)).child(code("[custom.load]", theme)))
                .child(code("exec = \"cut -d' ' -f1 /proc/loadavg\"", theme))
                .child(code("interval = 5", theme))
                .when(!custom.is_empty(), |el| {
                    el.child(div().mt(px(9.)).flex().flex_wrap().gap(px(6.)).children(custom.iter().map(|c| {
                        div()
                            .px(px(8.))
                            .py(px(3.))
                            .rounded(px(7.))
                            .bg(theme.fill_2)
                            .text_color(theme.ink_2)
                            .font_family(theme.mono.clone())
                            .text_size(px(11.))
                            .child(format!("custom/{c}"))
                    })))
                }),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn place_moves_and_orders() {
        let mut zones = Zones::default();
        place(&mut zones, "clock", Zone::Center, None);
        assert_eq!(zones.center, vec!["clock".to_string()]);
        assert_eq!(zones.right, vec!["control-center".to_string()]);
        place(&mut zones, "window", Zone::Left, Some("workspaces"));
        assert_eq!(zones.left, vec!["window".to_string(), "workspaces".to_string()]);
        place(&mut zones, "window", Zone::Left, None);
        assert_eq!(zones.left, vec!["workspaces".to_string(), "window".to_string()]);
    }
}
