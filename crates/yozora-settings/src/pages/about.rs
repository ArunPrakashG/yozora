//! About an app: what it is, whether it's running, and where its files are.

use gpui::{
    AnyElement, Context, FontWeight, IntoElement, ParentElement, StatefulInteractiveElement, Styled,
    div, prelude::FluentBuilder, px,
};
use yozora_ui::{Icon, Theme, icon_sized};

use crate::{
    apps::{self, AppEntry, Status},
    view::{SettingsView, tilde},
    widgets::{button, card, code, group_label, setting_row},
};

const REPO: &str = "https://github.com/ArunPrakashG/yozora";

pub fn render(view: &mut SettingsView, app: &AppEntry, cx: &mut Context<SettingsView>) -> AnyElement {
    let theme = Theme::global(cx).clone();
    let m = &app.manifest;
    let (status, version, warnings) = match &app.status {
        Status::Running { version, warnings } => ("Running", version.clone(), warnings.clone()),
        Status::Stopped => ("Not running", None, Vec::new()),
        Status::NoSocket => ("No control socket, so Settings can't tell", None, Vec::new()),
        Status::Unknown => ("Checking…", None, Vec::new()),
    };
    let subtitle = [
        version.clone(),
        hyprland_version(),
        Some("Built with GPUI".to_string()).filter(|_| app.is_bar()),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" · ");
    let manifest = m.clone();
    let stopped = app.status == Status::Stopped;

    let mut running = card(&theme).child(setting_row(
        "Status",
        Some(status.into()),
        div().when(stopped, |el| {
            el.child(button("about-start", Some(Icon::Power), "Start", true, &theme).on_click(cx.listener(move |this, _, _, cx| {
                this.error = apps::start(&manifest).err().map(|e| format!("{e:#}"));
                cx.notify();
            })))
        }),
        true,
        &theme,
    ));
    if app.is_bar() {
        running = running
            .child(setting_row(
                "Sources",
                Some("hyprland socket2 · pipewire-pulse · NetworkManager · UPower · BlueZ · MPRIS · mako · StatusNotifier".into()),
                div(),
                false,
                &theme,
            ))
            .child(setting_row(
                "Control socket",
                Some("Bind keys to popovers, or check that everything is wired up.".into()),
                div().flex().flex_col().items_end().gap(px(2.)).child(code("yozora-bar msg toggle control-center", &theme)).child(code("yozora-bar check", &theme)),
                false,
                &theme,
            ));
    } else if let Some(socket) = m.socket_path() {
        running = running.child(setting_row("Control socket", Some(tilde(&socket).into()), div(), false, &theme));
    }
    running = running.child(setting_row(
        "Configuration",
        Some(tilde(&view.config_dir.join(&m.config)).into()),
        div(),
        false,
        &theme,
    ));
    running = running.child(setting_row("Manifest", Some(tilde(&m.dir.join(format!("{}.toml", m.id))).into()), div(), false, &theme));

    div()
        .flex()
        .flex_col()
        .child(
            card(&theme)
                .flex_row()
                .items_center()
                .gap(px(22.))
                .p(px(22.))
                .child(
                    div()
                        .size(px(72.))
                        .flex_none()
                        .rounded(px(20.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .bg(theme.midnight)
                        .text_color(theme.accent)
                        .border_1()
                        .border_color(theme.hair_2)
                        .shadow(theme.accent_glow())
                        .child(icon_sized(Icon::Logo, 36.)),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .min_w_0()
                        .child(div().text_size(px(22.)).font_weight(FontWeight::SEMIBOLD).child(m.name.clone()))
                        .when(!subtitle.is_empty(), |el| {
                            el.child(div().mt(px(4.)).text_size(px(13.5)).text_color(theme.ink_2).child(subtitle))
                        })
                        .child(
                            div()
                                .mt(px(4.))
                                .text_size(px(13.5))
                                .text_color(theme.ink_2)
                                .child("Part of 夜空 Yozora, an open-source desktop for Hyprland."),
                        )
                        .child(
                            div()
                                .flex()
                                .gap(px(8.))
                                .mt(px(14.))
                                .child(button("source", Some(Icon::Code), "Source", true, &theme).on_click(|_, _, _| apps::open(REPO)))
                                .child(
                                    button("docs", Some(Icon::File), "Docs", false, &theme)
                                        .on_click(|_, _, _| apps::open(&format!("{REPO}/tree/main/docs"))),
                                ),
                        ),
                ),
        )
        .child(group_label("Running", false, &theme))
        .child(running)
        .when(!warnings.is_empty(), |el| {
            el.child(group_label("Configuration warnings", false, &theme)).child(card(&theme).children(
                warnings.iter().enumerate().map(|(i, w)| {
                    setting_row(w.clone(), None, div().text_color(theme.danger).child(icon_sized(Icon::Warning, 16.)), i == 0, &theme)
                }),
            ))
        })
        .into_any_element()
}

fn hyprland_version() -> Option<String> {
    static VERSION: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    VERSION
        .get_or_init(|| {
            let out = std::process::Command::new("hyprctl").args(["version", "-j"]).output().ok()?;
            let v: serde_json::Value = serde_json::from_slice(&out.stdout).ok()?;
            v["tag"].as_str().map(|t| format!("Hyprland {}", t.trim_start_matches('v')))
        })
        .clone()
}
