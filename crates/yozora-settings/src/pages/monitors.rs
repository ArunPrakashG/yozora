//! Monitors: the arrangement from Hyprland, and per-monitor bar settings.

use gpui::{
    AnyElement, Context, FontWeight, InteractiveElement, IntoElement, ParentElement, SharedString,
    StatefulInteractiveElement, Styled, div, prelude::FluentBuilder, px,
};
use yozora_config::bar::BarConfig;
use yozora_ui::{
    Icon, Theme,
    controls::{segmented, switch},
};

use crate::{
    apps::AppEntry,
    doc::string_array,
    hypr::Monitor,
    view::SettingsView,
    widgets::{button, card, group_label, mini_bar, notice, setting_row},
};

pub const SEARCH: &[(&str, &str)] = &[
    ("Show the bar", "monitor screen display hide"),
    ("Monitor modules", "second screen minimal same as main"),
    ("New monitors get a bar", "hotplug plug in screens"),
];

const MINIMAL_LEFT: &[&str] = &["workspaces"];
const MINIMAL_RIGHT: &[&str] = &["clock", "control-center"];

fn monitor_key(name: &str, rest: &str) -> String {
    format!("monitor.\"{name}\".{rest}")
}

pub fn render(view: &mut SettingsView, app: &AppEntry, cx: &mut Context<SettingsView>) -> AnyElement {
    let theme = Theme::global(cx).clone();
    let file = app.manifest.config.clone();
    let config: BarConfig = view.doc(&file).parse();

    let monitors: Vec<Monitor> = match &view.monitors {
        Ok(m) if !m.is_empty() => m.clone(),
        Ok(_) | Err(_) => {
            let reason = view.monitors.as_ref().err().cloned().unwrap_or_else(|| "Hyprland reports no monitors".into());
            return div()
                .flex()
                .flex_col()
                .gap(px(14.))
                .child(notice(Icon::Display, "Can't read the monitor layout", reason, false, &theme))
                .child(
                    button("retry", Some(Icon::Sync), "Try again", false, &theme)
                        .on_click(cx.listener(|this, _, _, cx| this.refresh_monitors(cx))),
                )
                .into_any_element();
        }
    };
    let selected = view
        .selected_monitor
        .clone()
        .filter(|s| monitors.iter().any(|m| &m.name == s))
        .unwrap_or_else(|| monitors[0].name.clone());
    let monitor = monitors.iter().find(|m| m.name == selected).cloned().expect("selected exists");

    // Fit the arrangement into the stage.
    let (min_x, min_y) =
        (monitors.iter().map(|m| m.x).min().unwrap_or(0), monitors.iter().map(|m| m.y).min().unwrap_or(0));
    let max_x = monitors.iter().map(|m| m.x + m.width).max().unwrap_or(1);
    let max_y = monitors.iter().map(|m| m.y + m.height).max().unwrap_or(1);
    let (span_w, span_h) = ((max_x - min_x).max(1) as f32, (max_y - min_y).max(1) as f32);
    let (stage_w, stage_h) = (440., 170.);
    let scale = (stage_w / span_w).min(stage_h / span_h);
    let (offset_x, offset_y) = ((stage_w - span_w * scale) / 2., (stage_h - span_h * scale) / 2.);

    let screens = monitors.iter().map(|m| {
        let on = config.shows_on(&m.name);
        let is_selected = m.name == selected;
        let name = m.name.clone();
        let (w, h) = (m.width as f32 * scale, m.height as f32 * scale);
        div()
            .id(SharedString::from(format!("mon-{}", m.name)))
            .absolute()
            .left(px(offset_x + (m.x - min_x) as f32 * scale))
            .top(px(offset_y + (m.y - min_y) as f32 * scale))
            .w(px(w))
            .h(px(h))
            .rounded(px(8.))
            .overflow_hidden()
            .bg(theme.midnight)
            .border_2()
            .border_color(if is_selected { theme.accent } else { theme.hair_2 })
            .when(is_selected, |el| el.shadow(theme.accent_glow()))
            .cursor_pointer()
            .child(
                div()
                    .absolute()
                    .inset(px(4.))
                    .rounded(px(4.))
                    .overflow_hidden()
                    .bg(crate::widgets::sky(false))
                    .when(on, |el| el.child(mini_bar(false, false, theme.accent)))
                    .child(
                        div()
                            .absolute()
                            .bottom(px(6.))
                            .left(px(8.))
                            .right(px(6.))
                            .whitespace_nowrap()
                            .truncate()
                            .text_size(px(11.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(gpui::rgba(0xE8F1FFFF))
                            .child(m.name.clone()),
                    ),
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                this.selected_monitor = Some(name.clone());
                cx.notify();
            }))
    });

    let on = config.shows_on(&monitor.name);
    let over = config.monitor.get(&monitor.name).cloned().unwrap_or_default();
    let mods = &over.modules;
    let mods_index = if mods.left.is_none() && mods.center.is_none() && mods.right.is_none() {
        Some(0)
    } else if mods.left.as_deref().is_some_and(|l| l.iter().map(String::as_str).eq(MINIMAL_LEFT.iter().copied()))
        && mods.right.as_deref().is_some_and(|r| r.iter().map(String::as_str).eq(MINIMAL_RIGHT.iter().copied()))
        && mods.center.as_deref().is_none_or(|c| c.is_empty())
    {
        Some(1)
    } else {
        None
    };
    let new_get_bar = config.bar.monitors.is_empty();
    let showing: Vec<String> = monitors.iter().filter(|m| config.shows_on(&m.name)).map(|m| m.name.clone()).collect();

    let (file_show, file_mods, file_new) = (file.clone(), file.clone(), file.clone());
    let (name_show, name_mods) = (monitor.name.clone(), monitor.name.clone());
    let weak = cx.entity().downgrade();
    let spec = format!(
        "{} × {} · {:.0} Hz{}",
        monitor.mode.0,
        monitor.mode.1,
        monitor.refresh,
        if monitor.description.is_empty() { String::new() } else { format!(" · {}", monitor.description) }
    );

    div()
        .flex()
        .flex_col()
        .child(
            div()
                .relative()
                .h(px(196.))
                .rounded(px(14.))
                .bg(theme.fill)
                .border_1()
                .border_color(theme.hair)
                .flex()
                .items_center()
                .justify_center()
                .child(div().relative().w(px(stage_w)).h(px(stage_h)).children(screens)),
        )
        .child(group_label(format!("{} · {}", monitor.name, spec), false, &theme))
        .child(
            card(&theme)
                .child(setting_row(
                    "Show the bar",
                    Some(if on { "The bar is on this monitor.".into() } else { "Windows use the full height.".into() }),
                    switch("mon-on", on, &theme).on_click(cx.listener(move |this, _, _, cx| {
                        let key = monitor_key(&name_show, "enabled");
                        this.edit(&file_show, cx, |d| d.set(&key, !on));
                    })),
                    true,
                    &theme,
                ))
                .child(setting_row(
                    "Modules",
                    Some("Minimal keeps a second screen quiet: workspaces, clock, control center.".into()),
                    div()
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .when(mods_index.is_none(), |el| {
                            el.child(div().text_size(px(12.)).text_color(theme.ink_3).child("Custom"))
                        })
                        .child(div().w(px(240.)).child(segmented(
                            "mon-mods",
                            &["Same as main", "Minimal"],
                            mods_index.unwrap_or(0),
                            &theme,
                            move |i, _, cx| {
                                let name = name_mods.clone();
                                let file = file_mods.clone();
                                weak.update(cx, |this, cx| {
                                    this.edit(&file, cx, |d| {
                                        d.edit(|f| {
                                            let key = |z: &str| monitor_key(&name, &format!("modules.{z}"));
                                            if i == 0 {
                                                for z in ["left", "center", "right"] {
                                                    f.remove(&key(z));
                                                }
                                            } else {
                                                f.set(&key("left"), string_array(MINIMAL_LEFT));
                                                f.set(&key("center"), string_array::<&str>(&[]));
                                                f.set(&key("right"), string_array(MINIMAL_RIGHT));
                                            }
                                        })
                                    })
                                })
                                .ok();
                            },
                        ))),
                    false,
                    &theme,
                ))
                .child(setting_row(
                    "New monitors get a bar",
                    Some(if new_get_bar {
                        "Screens you plug in show the bar right away.".into()
                    } else {
                        format!("Only {} show the bar.", config.bar.monitors.join(", ")).into()
                    }),
                    switch("new-mon", new_get_bar, &theme).on_click(cx.listener(move |this, _, _, cx| {
                        let showing = showing.clone();
                        this.edit(&file_new, cx, |d| {
                            if new_get_bar {
                                // Pin the bar to the monitors that show it now.
                                d.set("bar.monitors", string_array(&showing))
                            } else {
                                d.remove(&["bar.monitors"])
                            }
                        });
                    })),
                    false,
                    &theme,
                )),
        )
        .into_any_element()
}
