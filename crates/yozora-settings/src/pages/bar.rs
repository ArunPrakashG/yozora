//! The bar's own page: style, height, reserved space and compositor blur.

use gpui::{AnyElement, Context, IntoElement, ParentElement, StatefulInteractiveElement, Styled, div, px};
use yozora_config::bar::{BarConfig, BarStyle};
use yozora_ui::{
    Theme,
    controls::{segmented, switch},
};

use crate::{
    apps::AppEntry,
    view::{AppPage, Page, SettingsView},
    widgets::{card, group_label, link, mini_bar, option_card, setting_row, thumb},
};

pub const SEARCH: &[(&str, &str)] = &[
    ("Style", "flush floating rounded edge"),
    ("Height", "size tall 30 34 38"),
    ("Reserve space", "exclusive zone windows overlap"),
    ("Blur behind the bar", "hyprland compositor glass layer rule"),
];

const HEIGHTS: [u32; 3] = [30, 34, 38];

pub fn render(view: &mut SettingsView, app: &AppEntry, cx: &mut Context<SettingsView>) -> AnyElement {
    let theme = Theme::global(cx).clone();
    let file = app.manifest.config.clone();
    let config: BarConfig = view.doc(&file).parse();
    let bar = config.bar.clone();

    let style_card = |id: &'static str, style: BarStyle, value: &'static str, title: &str, detail: &str, cx: &mut Context<SettingsView>| {
        let file = file.clone();
        option_card(
            id,
            bar.style == style,
            thumb(!theme.dark).child(mini_bar(style == BarStyle::Floating, !theme.dark, theme.accent)),
            title.to_string(),
            detail.to_string(),
            &theme,
        )
        .on_click(cx.listener(move |this, _, _, cx| this.edit(&file, cx, |d| d.set("bar.style", value))))
    };

    let mut labels: Vec<String> = HEIGHTS.iter().map(|h| h.to_string()).collect();
    let height_index = match HEIGHTS.iter().position(|&h| h == bar.height) {
        Some(i) => i,
        None => {
            labels.push(bar.height.to_string());
            labels.len() - 1
        }
    };
    let label_refs: Vec<&str> = labels.iter().map(String::as_str).collect();
    let weak = cx.entity().downgrade();
    let height_file = file.clone();

    let monitors = match &view.monitors {
        Ok(list) => {
            let on: Vec<&str> = list.iter().filter(|m| config.shows_on(&m.name)).map(|m| m.name.as_str()).collect();
            match on.len() {
                0 => "Hidden on every monitor".to_string(),
                n if n == list.len() => format!("On every monitor: {}", on.join(", ")),
                _ => format!("On {}", on.join(", ")),
            }
        }
        Err(_) if config.bar.monitors.is_empty() => "On every monitor".into(),
        Err(_) => format!("On {}", config.bar.monitors.join(", ")),
    };
    let app_id = app.id().to_string();

    let toggle = |id: &'static str, key: &'static str, on: bool, cx: &mut Context<SettingsView>| {
        let file = file.clone();
        switch(id, on, &theme).on_click(cx.listener(move |this, _, _, cx| this.edit(&file, cx, |d| d.set(key, !on))))
    };

    div()
        .flex()
        .flex_col()
        .child(group_label("Style", true, &theme))
        .child(
            div()
                .flex()
                .gap(px(16.))
                .child(style_card("style-flush", BarStyle::Flush, "flush", "Flush", "Edge to edge", cx))
                .child(style_card("style-floating", BarStyle::Floating, "floating", "Floating", "Inset, rounded", cx)),
        )
        .child(group_label("Layout", false, &theme))
        .child(
            card(&theme)
                .child(setting_row(
                    "Height",
                    Some("Icons stay 16 px at every height.".into()),
                    div().w(px(60. * label_refs.len() as f32)).child(segmented("height", &label_refs, height_index, &theme, move |i, _, cx| {
                        let Some(&h) = HEIGHTS.get(i) else { return };
                        let file = height_file.clone();
                        weak.update(cx, |this, cx| this.edit(&file, cx, |d| d.set("bar.height", h as i64))).ok();
                    })),
                    true,
                    &theme,
                ))
                .child(setting_row(
                    "Reserve space",
                    Some("Tiled windows stop below the bar instead of under it.".into()),
                    toggle("reserve", "bar.reserve_space", bar.reserve_space, cx),
                    false,
                    &theme,
                ))
                .child(setting_row(
                    "Blur behind the bar",
                    Some("Asks Hyprland to blur behind the bar. Off makes the tint more opaque instead.".into()),
                    toggle("blur", "bar.blur", bar.blur, cx),
                    false,
                    &theme,
                ))
                .child(setting_row(
                    "Monitors",
                    Some(monitors.into()),
                    link("manage-monitors", "Manage", &theme).on_click(cx.listener(move |this, _, _, cx| {
                        this.go(Page::App(app_id.clone(), AppPage::Monitors), cx)
                    })),
                    false,
                    &theme,
                )),
        )
        .into_any_element()
}
