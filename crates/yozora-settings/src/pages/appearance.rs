//! Appearance: `theme.toml`'s theme, accent, glass and font, shared by every app.

use gpui::{
    AnyElement, Context, IntoElement, ParentElement, StatefulInteractiveElement, Styled, Window, div, px, rgba,
};
use yozora_config::theme::{Accent, Glass, ThemeConfig, ThemeMode};
use yozora_ui::{Theme, controls::segmented};

use crate::{
    view::{InputTarget, SettingsView, THEME_FILE},
    widgets::{card, group_label, mini_bar, option_card, setting_row, swatch, thumb},
};

pub const SEARCH: &[(&str, &str)] = &[
    ("Theme", "dark light midnight daybreak automatic"),
    ("Accent", "color starlight aurora nebula dawn"),
    ("Glass", "transparency tint clear balanced frosted"),
    ("Font", "typeface family geist"),
];

const ACCENTS: [(Accent, &str, &str, u32, u32); 4] = [
    (Accent::Starlight, "starlight", "Starlight", 0x8BBEFFFF, 0x2E6CD9FF),
    (Accent::Aurora, "aurora", "Aurora", 0x8FE3CFFF, 0x0D7F69FF),
    (Accent::Nebula, "nebula", "Nebula", 0xBBA8FFFF, 0x6A4FDBFF),
    (Accent::Dawn, "dawn", "Dawn", 0xFFC58BFF, 0xAD5A0DFF),
];

pub fn render(view: &mut SettingsView, window: &mut Window, cx: &mut Context<SettingsView>) -> AnyElement {
    let theme = Theme::global(cx).clone();
    let config: ThemeConfig = view.doc(THEME_FILE).parse();
    let editable = view.doc(THEME_FILE).is_editable();

    let mode_card = |id: &'static str,
                     mode: ThemeMode,
                     value: &'static str,
                     title: &str,
                     detail: &str,
                     light: bool,
                     split: bool,
                     cx: &mut Context<SettingsView>| {
        let preview = thumb(light).child(mini_bar(false, light, theme.accent)).child(
            div()
                .absolute()
                .right(px(10.))
                .top(px(18.))
                .w(px(62.))
                .h(px(46.))
                .rounded(px(6.))
                .bg(if light { rgba(0xF8FAFDDB) } else { rgba(0x0E131FCC) })
                .border_1()
                .border_color(if light { rgba(0x14285A1F) } else { rgba(0xAAC8FF33) }),
        );
        let preview = if split {
            // Half night, half dawn.
            thumb(false).child(div().absolute().top_0().bottom_0().right_0().w(px(120.)).bg(crate::widgets::sky(true)))
        } else {
            preview
        };
        option_card(id, config.theme == mode && editable, preview, title.to_string(), detail.to_string(), &theme)
            .on_click(cx.listener(move |this, _, _, cx| this.edit(THEME_FILE, cx, |d| d.set("theme", value))))
    };

    let accent_name = ACCENTS.iter().find(|a| a.0 == config.accent).map_or("Starlight", |a| a.2);
    let glass_index = match config.glass {
        Glass::Clear => 0,
        Glass::Balanced => 1,
        Glass::Frosted => 2,
    };
    let weak = cx.entity().downgrade();
    let font_value = view.doc(THEME_FILE).value("font").and_then(|v| v.as_str().map(String::from)).unwrap_or_default();
    let font = view.input(
        "theme-font",
        "Geist",
        false,
        InputTarget::String { file: THEME_FILE.into(), key: "font".into() },
        &font_value,
        window,
        cx,
    );

    div()
        .flex()
        .flex_col()
        .child(group_label("Theme", true, &theme))
        .child(
            div()
                .flex()
                .gap(px(16.))
                .child(mode_card("theme-dark", ThemeMode::Midnight, "midnight", "Midnight", "Dark", false, false, cx))
                .child(mode_card("theme-light", ThemeMode::Daybreak, "daybreak", "Daybreak", "Light", true, false, cx))
                .child(mode_card(
                    "theme-auto",
                    ThemeMode::Auto,
                    "auto",
                    "Automatic",
                    "Follows the desktop",
                    false,
                    true,
                    cx,
                )),
        )
        .child(group_label("Color and glass", false, &theme))
        .child(
            card(&theme)
                .child(setting_row(
                    "Accent",
                    Some(format!("{accent_name} · workspaces, toggles, focus rings").into()),
                    div().flex().gap(px(6.)).children(ACCENTS.iter().map(|&(accent, value, _, dark, light)| {
                        let color = rgba(if theme.dark { dark } else { light });
                        swatch(SharedId::accent(value), color, config.accent == accent, &theme).on_click(
                            cx.listener(move |this, _, _, cx| this.edit(THEME_FILE, cx, |d| d.set("accent", value))),
                        )
                    })),
                    true,
                    &theme,
                ))
                .child(setting_row(
                    "Glass",
                    Some("How much of the wallpaper shows through bars and popovers.".into()),
                    div().w(px(260.)).child(segmented(
                        "glass",
                        &["Clear", "Balanced", "Frosted"],
                        glass_index,
                        &theme,
                        move |i, _, cx| {
                            let value = ["clear", "balanced", "frosted"][i];
                            weak.update(cx, |this, cx| this.edit(THEME_FILE, cx, |d| d.set("glass", value))).ok();
                        },
                    )),
                    false,
                    &theme,
                ))
                .child(setting_row(
                    "Font",
                    Some("Falls back to Inter, then the system font, when it isn't installed.".into()),
                    div().w(px(220.)).child(font),
                    false,
                    &theme,
                )),
        )
        .into_any_element()
}

struct SharedId;

impl SharedId {
    fn accent(value: &str) -> gpui::ElementId {
        gpui::ElementId::Name(format!("accent-{value}").into())
    }
}
