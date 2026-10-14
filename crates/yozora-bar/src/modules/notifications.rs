//! The notification center and Do Not Disturb.
//!
//! With Jev on in Yozora Notify, pinned notifications come first, then the urgent ones, then
//! the rest by kind, and what can wait is folded at the bottom. Otherwise they're grouped by app.

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
    let sharing = Services::get(cx).hyprland.as_ref().is_some_and(|h| h.read(cx).state.sharing);
    let yozora = state.daemon == Daemon::Yozora;
    let mut shown = 0;
    for (g, group) in groups(&state.items).into_iter().enumerate() {
        if shown >= 30 {
            break;
        }
        let Group { key, title, glyph, items: group, always_open, folded } = group;
        let expanded = always_open || (group.len() == 1 && !folded) || p.ui.expanded.contains(&key);
        let toggle_key = key.clone();
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
                .child(div().flex().items_center().gap(px(8.)).child(icon_sized(glyph, 13.)).child(title))
                .child(div().font_weight(FontWeight::NORMAL).child(if expanded && !always_open && group.len() > 1 {
                    "Show less".to_string()
                } else {
                    group.len().to_string()
                }))
                .when(!always_open && (group.len() > 1 || folded), |el| {
                    el.cursor_pointer().hover(|s| s.text_color(theme.ink_2)).on_click(cx.listener(
                        move |p, _, _, cx| {
                            if !p.ui.expanded.remove(&toggle_key) {
                                p.ui.expanded.insert(toggle_key.clone());
                            }
                            cx.notify();
                        },
                    ))
                }),
        );
        let take = match (expanded, folded) {
            (true, _) => group.len(),
            (false, true) => 0,
            (false, false) => 1,
        };
        for n in group.iter().take(take) {
            el = el.child(card(n, shown, sharing, yozora, theme, cx));
            shown += 1;
        }
        if !expanded && !folded {
            // The rest peek out from under the newest, like a stack of cards.
            let peek_key = key.clone();
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
                        p.ui.expanded.insert(peek_key.clone());
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

struct Group<'a> {
    /// What remembers whether it's expanded.
    key: String,
    title: String,
    glyph: Icon,
    items: Vec<&'a Notification>,
    /// Pinned and urgent notifications are always all shown.
    always_open: bool,
    /// "Can wait" shows only its header until opened.
    folded: bool,
}

fn kind_group(kind: &str) -> Option<(&'static str, Icon)> {
    Some(match kind {
        "message" => ("MESSAGES", Icon::Smile),
        "alert" => ("ALERTS", Icon::Warning),
        "reminder" => ("REMINDERS", Icon::Calendar),
        "update" => ("UPDATES", Icon::Sync),
        "promotion" => ("PROMOTIONS", Icon::Star),
        _ => return None,
    })
}

/// Pinned, then urgent, then the rest by kind or app in order of their newest, then what can
/// wait. Items come newest first and stay that way within a group.
fn groups(items: &[Notification]) -> Vec<Group<'_>> {
    let fixed = |key: &str, title: &str, glyph, always_open, folded| Group {
        key: key.into(),
        title: title.into(),
        glyph,
        items: Vec::new(),
        always_open,
        folded,
    };
    let mut pinned = fixed("pinned", "PINNED", Icon::Star, true, false);
    let mut urgent = fixed("urgent", "NEEDS YOU", Icon::Alert, true, false);
    let mut quiet = fixed("quiet", "CAN WAIT", Icon::Clock, false, true);
    let mut rest: Vec<Group> = Vec::new();
    for n in items {
        if n.pinned {
            pinned.items.push(n);
        } else if n.quiet {
            quiet.items.push(n);
        } else if n.urgent {
            urgent.items.push(n);
        } else {
            let (key, title, glyph) = match kind_group(&n.kind) {
                Some((title, glyph)) => (format!("kind:{}", n.kind), title.to_string(), glyph),
                None => (
                    format!("app:{}", n.app),
                    if n.app.is_empty() { "NOTIFICATIONS".into() } else { n.app.to_uppercase() },
                    yozora_ui::icons::guess_app_icon(&n.app, &n.desktop_entry),
                ),
            };
            match rest.iter_mut().find(|g| g.key == key) {
                Some(g) => g.items.push(n),
                None => rest.push(Group { key, title, glyph, items: vec![n], always_open: false, folded: false }),
            }
        }
    }
    [pinned, urgent].into_iter().chain(rest).chain([quiet]).filter(|g| !g.items.is_empty()).collect()
}

/// A small round button on a card's edge, shown while the card is hovered.
fn corner(id: (&'static str, usize), glyph: Icon, on: bool, theme: &Theme) -> gpui::Stateful<Div> {
    div()
        .id(id)
        .absolute()
        .top(px(-6.))
        .size(px(20.))
        .rounded_full()
        .bg(theme.glass_pop)
        .border_1()
        .border_color(theme.hair_2)
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .text_color(if on { theme.accent } else { theme.ink })
        .opacity(if on { 1. } else { 0. })
        .group_hover("nt", |s| s.opacity(1.))
        .child(icon_sized(glyph, 11.))
}

fn card(
    n: &Notification,
    i: usize,
    sharing: bool,
    yozora: bool,
    theme: &Theme,
    cx: &mut Context<Popover>,
) -> AnyElement {
    let hidden = sharing && n.private;
    let (pinned, can_hold) = (n.pinned, yozora && (n.needs_action || n.pinned));
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
                .when(hidden, |el| {
                    el.child(
                        div()
                            .mt(px(1.))
                            .text_size(px(12.5))
                            .text_color(theme.ink_2)
                            .child("Hidden while you share your screen"),
                    )
                })
                .when(!hidden, |el| {
                    el.child(div().font_weight(FontWeight::SEMIBOLD).mt(px(1.)).child(n.summary.clone()))
                })
                .when(!hidden && !n.body.is_empty(), |el| {
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
        .when(can_hold, |el| {
            el.child(corner(("pin", i), Icon::Star, pinned, theme).right(px(20.)).on_click(move |_, _, cx| {
                cx.stop_propagation();
                send(Cmd::Pin(id, !pinned), cx);
            }))
            .child(corner(("snooze", i), Icon::Clock, false, theme).right(px(-6.)).on_click(
                move |_, _, cx| {
                    cx.stop_propagation();
                    send(Cmd::Snooze(id, 60), cx);
                },
            ))
        })
        .into_any_element()
}
