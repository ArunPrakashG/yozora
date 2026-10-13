//! The Yozora menu: settings, reload and session actions, with a highlight that follows the
//! pointer between rows.

use gpui::{
    AnimationExt, AnyElement, Context, Div, FontWeight, InteractiveElement, IntoElement, MouseButton, ParentElement,
    SharedString, StatefulInteractiveElement, Styled, Window, div, prelude::*, px,
};
use yozora_ui::{Icon, Theme, controls, icon_sized};

use super::SessionAction;
use crate::{app::Config, bar::bi, popover::Popover, services::run_shell};

pub fn item(theme: &Theme) -> Div {
    bi().px(px(9.)).text_color(theme.accent).child(icon_sized(Icon::Logo, 16.))
}

enum Entry {
    Settings(Option<&'static str>),
    Reload,
    Session(SessionAction),
}

const HEAD: f32 = 56.;
const SEP: f32 = 9.;
const ROW: f32 = 30.;

/// Menu layout: `None` is a separator.
const ENTRIES: &[Option<(&str, Option<&str>, u8)>] = &[
    Some(("About Yozora", None, 0)),
    Some(("Bar settings…", Some("Super ,"), 1)),
    Some(("Reload bar", Some("Super ⇧ R"), 2)),
    None,
    Some(("Lock screen", Some("Super L"), 3)),
    Some(("Log out…", None, 4)),
    None,
    Some(("Restart…", None, 5)),
    Some(("Shut down…", None, 6)),
];

fn entry(code: u8) -> Entry {
    match code {
        0 => Entry::Settings(Some("about")),
        1 => Entry::Settings(Some("bar")),
        2 => Entry::Reload,
        3 => Entry::Session(SessionAction::Lock),
        4 => Entry::Session(SessionAction::Logout),
        5 => Entry::Session(SessionAction::Reboot),
        _ => Entry::Session(SessionAction::Poweroff),
    }
}

/// Opens the Settings app, on `page` if given.
pub fn open_settings(page: Option<&str>, cx: &gpui::App) {
    let command = &Config::get(cx).settings.bar.session.settings;
    match page {
        Some(p) => run_shell(&format!("{command} --page {p}")),
        None => run_shell(command),
    }
}

pub fn popover(p: &mut Popover, theme: &Theme, _window: &mut Window, cx: &mut Context<Popover>) -> Vec<AnyElement> {
    if let Some(action) = p.ui.confirm {
        return confirm(action, theme, cx);
    }
    // Where each row sits, for the highlight.
    let mut y = HEAD + SEP;
    let mut rows: Vec<AnyElement> = Vec::new();
    let mut tops = Vec::new();
    for e in ENTRIES {
        match e {
            None => {
                rows.push(div().h(px(1.)).mx(px(6.)).my(px(4.)).bg(theme.hair).into_any_element());
                y += SEP;
            }
            Some((label, key, code)) => {
                let index = tops.len();
                tops.push(y);
                let code = *code;
                rows.push(
                    div()
                        .id(("menu", index))
                        .relative()
                        .h(px(ROW))
                        .px(px(10.))
                        .flex()
                        .items_center()
                        .justify_between()
                        .rounded(px(8.))
                        .child(SharedString::from(*label))
                        .when_some(*key, |el, k| el.child(div().text_size(px(11.5)).text_color(theme.ink_3).child(k)))
                        .on_hover(cx.listener(move |p, hovered: &bool, _, cx| {
                            if *hovered {
                                p.ui.hovered = Some(index);
                            } else if p.ui.hovered == Some(index) {
                                p.ui.hovered = None;
                            }
                            cx.notify();
                        }))
                        .on_mouse_up(MouseButton::Left, cx.listener(move |p, _, _, cx| activate(p, code, cx)))
                        .into_any_element(),
                );
                y += ROW;
            }
        }
    }
    let hovered = p.ui.hovered;
    let hi_top = hovered.and_then(|i| tops.get(i).copied()).unwrap_or(tops[0]);
    let highlight = div()
        .absolute()
        .left(px(6.))
        .right(px(6.))
        .top_0()
        .h(px(ROW))
        .rounded(px(8.))
        .bg(theme.accent_soft)
        .border_1()
        .border_color(gpui::Rgba { a: 0.24, ..theme.accent })
        .with_spring("menu-highlight", controls::indicator_spring(theme, hi_top), |el, top| el.top(px(top)));
    let highlight = div().absolute().top_0().left_0().size_full().child(highlight).with_spring(
        "menu-highlight-opacity",
        controls::indicator_spring(theme, if hovered.is_some() { 1.0f32 } else { 0.0 }),
        |el, o| el.opacity(o.clamp(0., 1.)),
    );

    let head = div()
        .h(px(HEAD))
        .flex()
        .items_center()
        .gap(px(11.))
        .px(px(8.))
        .child(
            div()
                .size(px(34.))
                .rounded(px(10.))
                .flex()
                .items_center()
                .justify_center()
                .bg(theme.midnight)
                .text_color(theme.accent)
                .shadow(theme.accent_glow())
                .child(icon_sized(Icon::Logo, 18.)),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .child(div().text_size(px(13.5)).font_weight(FontWeight::SEMIBOLD).child("Yozora"))
                .child(
                    div()
                        .text_size(px(11.5))
                        .text_color(theme.ink_2)
                        .child(format!("夜空 · Bar {}", env!("CARGO_PKG_VERSION"))),
                ),
        );
    let mut all =
        vec![head.into_any_element(), div().h(px(1.)).mx(px(6.)).my(px(4.)).bg(theme.hair).into_any_element()];
    all.extend(rows);
    // The highlight sits behind the rows, in one block so the stagger doesn't separate it.
    vec![div().relative().p(px(0.)).child(highlight).children(all).into_any_element()]
}

fn activate(p: &mut Popover, code: u8, cx: &mut Context<Popover>) {
    match entry(code) {
        Entry::Settings(page) => {
            open_settings(page, cx);
            p.close(cx);
        }
        Entry::Reload => {
            p.close(cx);
            cx.defer(|cx| {
                if let Err(e) = crate::app::reload(cx) {
                    tracing::error!("reload failed: {e:#}");
                }
            });
        }
        Entry::Session(SessionAction::Lock) => {
            run_shell(&SessionAction::Lock.command(&Config::get(cx).settings.bar.session));
            p.close(cx);
        }
        Entry::Session(action) => {
            p.ui.confirm = Some(action);
            cx.notify();
        }
    }
}

/// "Restart now?" with the action and Cancel. Destructive session actions always ask.
pub fn confirm(action: SessionAction, theme: &Theme, cx: &mut Context<Popover>) -> Vec<AnyElement> {
    let question = match action {
        SessionAction::Logout => "Log out now?",
        SessionAction::Reboot => "Restart now?",
        SessionAction::Poweroff => "Shut down now?",
        SessionAction::Lock => "Lock now?",
    };
    let detail = "Unsaved work in open apps may be lost.";
    vec![
        div()
            .p(px(8.))
            .flex()
            .flex_col()
            .gap(px(4.))
            .child(div().text_size(px(14.)).font_weight(FontWeight::SEMIBOLD).child(question))
            .child(controls::muted(detail, theme))
            .into_any_element(),
        div()
            .flex()
            .gap(px(6.))
            .p(px(6.))
            .child(controls::footer_button("cancel", Icon::Close, "Cancel", theme).on_click(cx.listener(
                |p, _, _, cx| {
                    p.ui.confirm = None;
                    p.ui.power_choices = false;
                    cx.notify();
                },
            )))
            .child(
                div()
                    .id("confirm")
                    .h(px(34.))
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(10.))
                    .text_size(px(12.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .bg(theme.danger)
                    .text_color(gpui::white())
                    .cursor_pointer()
                    .child(action.verb())
                    .on_click(cx.listener(move |p, _, _, cx| {
                        run_shell(&action.command(&Config::get(cx).settings.bar.session));
                        p.close(cx);
                    })),
            )
            .into_any_element(),
    ]
}
