//! The notification center, grouped by app, and Do Not Disturb.

use gpui::{
    AnyElement, App, Context, Div, FontWeight, InteractiveElement, IntoElement, ParentElement,
    StatefulInteractiveElement, Styled, div, img, prelude::*, px,
};
use yozora_ui::{Icon, IconElement, Theme, controls, icon, icon_sized};

use crate::{
    bar::bi,
    popover::Popover,
    services::{
        Services, icons,
        notifications::{Cmd, Daemon, Notification},
    },
};

pub fn item(theme: &Theme, cx: &App) -> Option<Div> {
    let n = Services::get(cx).notifications.as_ref()?.read(cx);
    let (dnd, unread) = (n.state.dnd, n.state.unread);
    Some(
        bi().relative()
            .child(icon(if dnd { Icon::Moon } else { Icon::Bell }).text_color(if n.up() {
                theme.ink
            } else {
                theme.ink_3
            }))
            .when(unread && !dnd, |el| {
                el.child(
                    div()
                        .absolute()
                        .top(px(6.))
                        .right(px(6.))
                        .size(px(6.))
                        .rounded_full()
                        .bg(theme.accent)
                        .border_2()
                        .border_color(theme.glass_bar),
                )
            }),
    )
}

pub fn send(cmd: Cmd, cx: &App) {
    if let Some(n) = &Services::get(cx).notifications {
        n.read(cx).send(cmd);
    }
}

pub fn dnd_switch(theme: &Theme, cx: &App) -> Option<AnyElement> {
    let n = Services::get(cx).notifications.as_ref()?.read(cx);
    let on = n.state.dnd;
    Some(controls::switch("dnd", on, theme).on_click(move |_, _, cx| send(Cmd::SetDnd(!on), cx)).into_any_element())
}

pub fn popover(p: &mut Popover, theme: &Theme, cx: &mut Context<Popover>) -> Vec<AnyElement> {
    let Some(service) = Services::get(cx).notifications.as_ref().map(|n| n.read(cx)) else { return Vec::new() };
    let state = service.state.clone();
    let error = service.error.clone();
    let fake = service.fake;
    let mut rows: Vec<AnyElement> = Vec::new();
    rows.push(
        controls::header(
            "Notifications",
            div()
                .flex()
                .items_center()
                .gap(px(9.))
                .text_size(px(12.))
                .text_color(theme.ink_2)
                .child("Do Not Disturb")
                .children(dnd_switch(theme, cx)),
        )
        .into_any_element(),
    );
    if let Some(e) = error {
        rows.push(controls::empty(&["No notification server is running.", e.as_str()], theme).into_any_element());
        return rows;
    }
    if state.items.is_empty() {
        rows.push(
            div()
                .flex()
                .flex_col()
                .items_center()
                .py(px(26.))
                .gap(px(10.))
                .child(icon_sized(Icon::Check, 22.).text_color(theme.accent))
                .child(controls::muted("You're all caught up.", theme))
                .into_any_element(),
        );
    }
    let mut shown = 0;
    for (g, group) in groups(&state.items).into_iter().enumerate() {
        if shown >= 30 {
            break;
        }
        let app = group[0].app.clone();
        let expanded = group.len() == 1 || p.ui.expanded.contains(&app);
        let glyph = yozora_ui::icons::guess_app_icon(&app, &group[0].desktop_entry);
        let toggle_app = app.clone();
        let mut el = div().flex().flex_col().pt(px(if g == 0 { 0. } else { 8. })).child(
            div()
                .id(("group", g))
                .flex()
                .items_center()
                .justify_between()
                .px(px(6.))
                .pt(px(4.))
                .pb(px(6.))
                .text_size(px(11.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme.ink_3)
                .child(div().flex().items_center().gap(px(8.)).child(icon_sized(glyph, 13.)).child(if app.is_empty() {
                    "NOTIFICATIONS".to_string()
                } else {
                    app.to_uppercase()
                }))
                .child(div().font_weight(FontWeight::NORMAL).child(if group.len() > 1 && expanded {
                    "Show less".to_string()
                } else {
                    group.len().to_string()
                }))
                .when(group.len() > 1, |el| {
                    el.cursor_pointer().hover(|s| s.text_color(theme.ink_2)).on_click(cx.listener(
                        move |p, _, _, cx| {
                            if !p.ui.expanded.remove(&toggle_app) {
                                p.ui.expanded.insert(toggle_app.clone());
                            }
                            cx.notify();
                        },
                    ))
                }),
        );
        let take = if expanded { group.len() } else { 1 };
        for n in group.iter().take(take) {
            el = el.child(card(n, shown, theme, cx));
            shown += 1;
        }
        if !expanded {
            // The rest peek out from under the newest, like a stack of cards.
            let peek_app = app.clone();
            el = el.child(
                div()
                    .id(("peek", g))
                    .flex()
                    .flex_col()
                    .items_center()
                    .cursor_pointer()
                    .child(div().h(px(7.)).w_full().px(px(10.)).child(peek(theme, 1.)))
                    .when(group.len() > 2, |el| el.child(div().h(px(6.)).w_full().px(px(20.)).child(peek(theme, 0.7))))
                    .on_click(cx.listener(move |p, _, _, cx| {
                        p.ui.expanded.insert(peek_app.clone());
                        cx.notify();
                    })),
            );
        }
        rows.push(el.into_any_element());
    }
    let note = match (fake, state.daemon) {
        (true, _) => "History · fake",
        (false, Daemon::Yozora) => "History from Yozora Notify",
        (false, Daemon::Mako) => "History from mako",
    };
    rows.push(
        controls::footer(
            note,
            controls::link("clear", "Clear all", theme).on_click(|_, _, cx| send(Cmd::ClearAll, cx)),
            theme,
        )
        .into_any_element(),
    );
    rows
}

/// The edge of a card hidden under the one above it.
fn peek(theme: &Theme, opacity: f32) -> impl IntoElement {
    div()
        .size_full()
        .rounded_b(px(10.))
        .bg(theme.fill_2)
        .border_1()
        .border_t_0()
        .border_color(theme.hair_2)
        .opacity(opacity)
}

/// Notifications grouped by app, groups ordered by their newest notification.
fn groups(items: &[Notification]) -> Vec<Vec<&Notification>> {
    let mut groups: Vec<Vec<&Notification>> = Vec::new();
    for n in items {
        match groups.iter_mut().find(|g| g[0].app == n.app) {
            Some(g) => g.push(n),
            None => groups.push(vec![n]),
        }
    }
    groups
}

fn card(n: &Notification, i: usize, theme: &Theme, cx: &mut Context<Popover>) -> AnyElement {
    let id = n.id;
    let glyph = n.icon_path.clone();
    let fallback = yozora_ui::icons::guess_app_icon(&n.app, &n.desktop_entry);
    let urgent = n.urgency >= 2;
    let when = n.seen.filter(|s| *s > 0).map(super::ago);
    let activate = n.default_action;
    div()
        .id(("notification", i))
        .group("nt")
        .relative()
        .flex()
        .gap(px(11.))
        .px(px(11.))
        .py(px(10.))
        .mt(px(if i == 0 { 0. } else { 6. }))
        .rounded(px(12.))
        .bg(theme.fill)
        .border_1()
        .border_color(if urgent { theme.danger } else { theme.hair })
        .when(activate, |el| {
            el.cursor_pointer().hover(|s| s.bg(theme.fill_2)).on_click(cx.listener(move |p, _, _, cx| {
                send(Cmd::Activate(id), cx);
                p.close(cx);
            }))
        })
        .child(
            div()
                .size(px(32.))
                .flex_none()
                .rounded(px(9.))
                .flex()
                .items_center()
                .justify_center()
                .bg(theme.fill_3)
                .overflow_hidden()
                .child(match glyph {
                    Some(path) if icons::is_symbolic(&path) => {
                        IconElement::from_path(path.to_string_lossy().into_owned(), 18.)
                            .text_color(theme.ink)
                            .into_any_element()
                    }
                    Some(path) => img(path).size(px(22.)).into_any_element(),
                    None => icon_sized(fallback, 17.).text_color(theme.ink_2).into_any_element(),
                }),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .child(
                    div()
                        .flex()
                        .justify_between()
                        .text_size(px(11.5))
                        .text_color(theme.ink_2)
                        .child(n.app.clone())
                        .children(when),
                )
                .child(div().font_weight(FontWeight::SEMIBOLD).mt(px(1.)).child(n.summary.clone()))
                .when(!n.body.is_empty(), |el| {
                    el.child(
                        div()
                            .mt(px(1.))
                            .text_size(px(12.5))
                            .text_color(theme.ink_2)
                            .line_height(px(17.5))
                            .child(super::ellipsize(&n.body, 280)),
                    )
                }),
        )
        .child(
            div()
                .id(("dismiss", i))
                .absolute()
                .top(px(-6.))
                .left(px(-6.))
                .size(px(20.))
                .rounded_full()
                .bg(theme.glass_pop)
                .border_1()
                .border_color(theme.hair_2)
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .opacity(0.)
                .group_hover("nt", |s| s.opacity(1.))
                .child(icon_sized(Icon::Close, 11.))
                .on_click(move |_, _, cx| {
                    cx.stop_propagation();
                    send(Cmd::Dismiss(id), cx);
                }),
        )
        .into_any_element()
}
