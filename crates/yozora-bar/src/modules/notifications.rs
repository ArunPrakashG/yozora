//! The notification center and Do Not Disturb.
//!
//! Pinned notifications come first, then one group per app in order of their newest, then the
//! snoozed ones. A group of three or more collapses into a stack until it's opened.

use chrono::{Local, TimeZone};
use gpui::{
    AnyElement, App, Context, Div, FontWeight, InteractiveElement, IntoElement, ParentElement, SharedString, Stateful,
    StatefulInteractiveElement, Styled, div, img, prelude::*, px,
};
use yozora_ui::{
    Icon, IconElement, Theme, icon, icon_sized,
    theme::{CONTROL_H, RADIUS_MD, RADIUS_SM},
};

use crate::{
    bar::bi,
    popover::Popover,
    services::{
        Services, icons,
        notifications::{Cmd, Daemon, Notification},
    },
};

/// A group this long collapses into a stack.
const STACK_AT: usize = 3;
/// What the snooze button on a card asks for.
const SNOOZE_MIN: u32 = 60;

pub fn item(theme: &Theme, cx: &App) -> Option<Div> {
    let n = Services::get(cx).notifications.as_ref()?.read(cx);
    let dnd = n.state.dnd;
    let unread = n.state.items.iter().filter(|n| n.unread && n.snoozed_until == 0).count();
    Some(
        bi().text_color(if n.up() { theme.ink } else { theme.ink_3 })
            .gap(px(6.))
            .child(icon(if dnd { Icon::Moon } else { Icon::Bell }))
            .when(unread > 0 && !dnd, |el| {
                el.child(
                    div()
                        .font_family(theme.mono.clone())
                        .text_size(px(12.))
                        .font_weight(FontWeight::MEDIUM)
                        .child(unread.to_string()),
                )
            }),
    )
}

pub fn send(cmd: Cmd, cx: &App) {
    if let Some(n) = &Services::get(cx).notifications {
        n.read(cx).send(cmd);
    }
}

pub fn popover(p: &mut Popover, theme: &Theme, cx: &mut Context<Popover>) -> Vec<AnyElement> {
    let Some(service) = Services::get(cx).notifications.as_ref().map(|n| n.read(cx)) else { return Vec::new() };
    let state = service.state.clone();
    let error = service.error.clone();
    let yozora = state.daemon == Daemon::Yozora;
    let sharing = Services::get(cx).hyprland.as_ref().is_some_and(|h| h.read(cx).state.sharing);
    let now = now();
    let (snoozed, live): (Vec<&Notification>, Vec<&Notification>) =
        state.items.iter().partition(|n| n.snoozed_until > now);

    let mut rows: Vec<AnyElement> =
        vec![head(state.dnd, !live.is_empty() && error.is_none(), theme).into_any_element()];
    if let Some(e) = error {
        rows.push(empty("No notification server", Some(e), theme).into_any_element());
        return rows;
    }
    if state.dnd {
        rows.push(dnd_banner(theme).into_any_element());
    }
    if live.is_empty() && snoozed.is_empty() {
        rows.push(empty("No notifications", None, theme).into_any_element());
        return rows;
    }

    let ctx = CardCtx { sharing, yozora };
    for (g, group) in groups(&live).into_iter().enumerate() {
        let Group { key, title, glyph, items } = group;
        let pinned = key == "pinned";
        let stacked = !pinned && items.len() >= STACK_AT && !p.ui.expanded.contains(&key);
        let mut el = group_el().child(group_head(
            glyph,
            title.clone(),
            (items.len() >= STACK_AT && !pinned).then_some(items.len()),
            theme,
        ));
        let show = if stacked { 1 } else { items.len() };
        for n in &items[..show] {
            el = el.child(card(n, pinned, None, &ctx, theme, cx));
        }
        if stacked {
            let (more, open) = (items.len() - 1, key.clone());
            el = el.child(under(8., 1., theme)).child(under(16., 0.6, theme)).child(
                ghost(("more", g), format!("{more} more from {title}").into(), theme).h(px(26.)).ml(px(4.)).on_click(
                    cx.listener(move |p, _, _, cx| {
                        p.ui.expanded.insert(open.clone());
                        cx.notify();
                    }),
                ),
            );
        } else if items.len() >= STACK_AT && !pinned {
            el = el.child(ghost(("less", g), "Show less".into(), theme).h(px(26.)).ml(px(4.)).on_click(cx.listener(
                move |p, _, _, cx| {
                    p.ui.expanded.remove(&key);
                    cx.notify();
                },
            )));
        }
        rows.push(el.into_any_element());
    }
    if !snoozed.is_empty() {
        let mut el = group_el().child(group_head(Glyph::Icon(Icon::Clock), "Snoozed".into(), None, theme));
        for n in snoozed {
            el = el.child(card(n, true, Some(n.snoozed_until), &ctx, theme, cx));
        }
        rows.push(el.into_any_element());
    }
    rows
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// Title, the Do Not Disturb button and Clear.
fn head(dnd: bool, can_clear: bool, theme: &Theme) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .gap(px(4.))
        .pt(px(8.))
        .pr(px(6.))
        .pb(px(8.))
        .pl(px(10.))
        .child(
            div()
                .flex_1()
                .text_size(px(15.))
                .line_height(px(20.))
                .font_weight(FontWeight::SEMIBOLD)
                .child("Notifications"),
        )
        .child(
            icon_button("dnd", Icon::Moon, 28., 15., theme)
                .when(dnd, |el| el.bg(theme.accent_soft).text_color(theme.accent))
                .on_click(move |_, _, cx| send(Cmd::SetDnd(!dnd), cx)),
        )
        .when(can_clear, |el| {
            el.child(
                ghost("clear", "Clear".into(), theme)
                    .h(px(28.))
                    .px(px(10.))
                    .text_size(px(13.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .on_click(|_, _, cx| send(Cmd::ClearAll, cx)),
            )
        })
}

fn dnd_banner(theme: &Theme) -> impl IntoElement {
    div()
        .mx(px(2.))
        .mt(px(4.))
        .py(px(10.))
        .px(px(12.))
        .rounded(RADIUS_MD)
        .bg(theme.accent_soft)
        .flex()
        .items_center()
        .gap(px(10.))
        .child(
            div()
                .flex_1()
                .flex()
                .flex_col()
                .gap(px(1.))
                .child(div().font_weight(FontWeight::SEMIBOLD).line_height(px(18.)).child("Do Not Disturb"))
                .child(meta().text_color(theme.ink_2).child("Critical alerts still show")),
        )
        .child(
            button("dnd-off", "Turn off".into(), false, theme)
                .h(px(28.))
                .on_click(|_, _, cx| send(Cmd::SetDnd(false), cx)),
        )
}

fn empty(title: &str, detail: Option<String>, theme: &Theme) -> impl IntoElement {
    div()
        .h(px(220.))
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(12.))
        .child(
            div()
                .size(px(44.))
                .rounded_full()
                .bg(theme.fill)
                .flex()
                .items_center()
                .justify_center()
                .text_color(theme.ink_3)
                .child(icon_sized(Icon::Bell, 18.)),
        )
        .child(div().font_weight(FontWeight::SEMIBOLD).text_color(theme.ink_2).child(title.to_string()))
        .children(detail.map(|d| meta().px(px(24.)).text_color(theme.ink_3).text_center().child(d)))
}

fn group_el() -> Div {
    div().flex().flex_col().gap(px(4.)).pt(px(10.))
}

fn group_head(glyph: Glyph, title: SharedString, count: Option<usize>, theme: &Theme) -> impl IntoElement {
    meta()
        .flex()
        .items_center()
        .gap(px(8.))
        .h(px(24.))
        .px(px(10.))
        .text_color(theme.ink_2)
        .child(glyph.render(14., theme))
        .child(div().flex_1().min_w_0().truncate().child(title))
        .children(count.map(|c| div().font_family(theme.mono.clone()).text_color(theme.ink_3).child(c.to_string())))
}

/// The edge of a card hidden under the one above it.
fn under(inset: f32, opacity: f32, theme: &Theme) -> impl IntoElement {
    div()
        .h(px(5.))
        .mx(px(inset))
        .mt(px(-4.))
        .rounded_b(px(7.))
        .bg(theme.surface_2)
        .border_1()
        .border_t_0()
        .border_color(theme.line)
        .opacity(opacity)
}

struct CardCtx {
    sharing: bool,
    yozora: bool,
}

/// One notification. `with_glyph` puts the app's glyph before the summary, for groups that mix
/// apps. `until` is set for a snoozed one.
fn card(
    n: &Notification,
    with_glyph: bool,
    until: Option<u64>,
    ctx: &CardCtx,
    theme: &Theme,
    cx: &mut Context<Popover>,
) -> AnyElement {
    let id = n.id;
    let urgent = n.urgency >= 2;
    let hidden = ctx.sharing;
    let activate = n.default_action && until.is_none();
    let time = match until {
        Some(t) => div()
            .font_family(theme.mono.clone())
            .text_size(px(12.))
            .font_weight(FontWeight::MEDIUM)
            .text_color(theme.accent)
            .child(format!("until {}", clock(t))),
        None => meta().text_color(theme.ink_3).child(n.seen.filter(|s| *s > 0).map(when).unwrap_or_default()),
    };
    let summary = if hidden {
        "Hidden while you share your screen".to_string()
    } else if n.summary.is_empty() {
        n.app.clone()
    } else {
        n.summary.clone()
    };

    let mut tools = div().flex().gap(px(2.));
    if ctx.yozora && until.is_none() {
        let pinned = n.pinned;
        tools = tools
            .child(
                icon_button(("pin", id as usize), Icon::Pin, 26., 13., theme)
                    .when(pinned, |el| el.text_color(theme.accent))
                    .on_click(move |_, _, cx| {
                        cx.stop_propagation();
                        send(Cmd::Pin(id, !pinned), cx);
                    }),
            )
            .child(icon_button(("snooze", id as usize), Icon::Snooze, 26., 13., theme).on_click(move |_, _, cx| {
                cx.stop_propagation();
                send(Cmd::Snooze(id, SNOOZE_MIN), cx);
            }));
    }
    if ctx.yozora && until.is_some() {
        // Back into its group now.
        tools =
            tools.child(icon_button(("wake", id as usize), Icon::Bell, 26., 13., theme).on_click(move |_, _, cx| {
                cx.stop_propagation();
                send(Cmd::Snooze(id, 0), cx);
            }));
    }
    tools =
        tools.child(icon_button(("dismiss", id as usize), Icon::Close, 26., 13., theme).on_click(move |_, _, cx| {
            cx.stop_propagation();
            send(Cmd::Dismiss(id), cx);
        }));

    let top = div()
        .relative()
        .flex()
        .items_center()
        .gap(px(8.))
        .h(px(18.))
        .when(with_glyph, |el| el.child(Glyph::of(n).render(14., theme)))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .font_weight(FontWeight::SEMIBOLD)
                .when(hidden, |el| el.font_weight(FontWeight::NORMAL).text_color(theme.ink_2))
                .child(summary),
        )
        .child(time.flex_none().group_hover("nt", |s| s.opacity(0.)))
        // Over the time while hovered, on the card's hover color so a long summary doesn't
        // show through.
        .child(
            div()
                .absolute()
                .top(px(-4.))
                .right(px(-8.))
                .rounded(RADIUS_SM)
                .bg(theme.surface)
                .opacity(0.)
                .group_hover("nt", |s| s.opacity(1.))
                .child(div().rounded(RADIUS_SM).bg(theme.fill_hover).child(tools)),
        );

    let body = (!hidden && !n.body.is_empty() && until.is_none()).then(|| {
        div()
            .text_color(theme.ink_2)
            .line_height(px(18.))
            .line_clamp(2)
            .when(with_glyph, |el| el.pl(px(22.)))
            .child(super::ellipsize(&n.body, 400))
    });

    let actions = (!hidden && until.is_none() && !n.actions.is_empty()).then(|| {
        div().flex().gap(px(6.)).pt(px(8.)).children(n.actions.iter().take(3).enumerate().map(|(i, (key, label))| {
            let key = key.clone();
            button(("action", id as usize * 4 + i), label.clone().into(), i == 0, theme)
                .flex_1()
                .min_w_0()
                .h(px(28.))
                .on_click(move |_, _, cx| {
                    cx.stop_propagation();
                    send(Cmd::Action(id, key.clone()), cx);
                })
        }))
    });

    div()
        .id(("notification", id as usize))
        .group("nt")
        .relative()
        .flex()
        .flex_col()
        .gap(px(2.))
        .py(px(10.))
        .px(px(12.))
        .rounded(RADIUS_MD)
        .bg(theme.surface_2)
        .border_1()
        .border_color(if urgent { theme.danger } else { theme.line })
        .hover(|s| s.bg(theme.fill_hover))
        .when(activate, |el| {
            el.cursor_pointer().on_click(cx.listener(move |p, _, _, cx| {
                send(Cmd::Activate(id), cx);
                p.close(cx);
            }))
        })
        .child(top)
        .children(body)
        .children(actions)
        .into_any_element()
}

/// The app's glyph: its own icon when it sent one, else a guess from its name.
enum Glyph {
    Icon(Icon),
    Symbolic(String),
    Image(std::path::PathBuf),
}

impl Glyph {
    fn of(n: &Notification) -> Self {
        match &n.icon_path {
            Some(p) if icons::is_symbolic(p) => Glyph::Symbolic(p.to_string_lossy().into_owned()),
            Some(p) => Glyph::Image(p.clone()),
            None => Glyph::Icon(yozora_ui::icons::guess_app_icon(&n.app, &n.desktop_entry)),
        }
    }

    fn render(self, size: f32, theme: &Theme) -> AnyElement {
        match self {
            Glyph::Icon(i) => icon_sized(i, size).flex_none().into_any_element(),
            Glyph::Symbolic(p) => IconElement::from_path(p, size).text_color(theme.ink_2).into_any_element(),
            Glyph::Image(p) => img(p).size(px(size)).flex_none().into_any_element(),
        }
    }
}

struct Group<'a> {
    /// What remembers whether it's expanded.
    key: String,
    title: SharedString,
    glyph: Glyph,
    items: Vec<&'a Notification>,
}

/// Pinned, then the rest by app in order of their newest. Items come newest first and stay that
/// way within a group.
fn groups<'a>(items: &[&'a Notification]) -> Vec<Group<'a>> {
    let mut pinned =
        Group { key: "pinned".into(), title: "Pinned".into(), glyph: Glyph::Icon(Icon::Pin), items: Vec::new() };
    let mut rest: Vec<Group> = Vec::new();
    for &n in items {
        if n.pinned {
            pinned.items.push(n);
            continue;
        }
        let key = format!("app:{}", n.app);
        match rest.iter_mut().find(|g| g.key == key) {
            Some(g) => g.items.push(n),
            None => rest.push(Group {
                key,
                title: if n.app.is_empty() { "Notifications".into() } else { n.app.clone().into() },
                glyph: Glyph::of(n),
                items: vec![n],
            }),
        }
    }
    [pinned].into_iter().chain(rest).filter(|g| !g.items.is_empty()).collect()
}

/// "now", "5 min", "3 h", then the weekday, then the date.
fn when(seen: u64) -> String {
    let s = now().saturating_sub(seen);
    match s {
        0..60 => "now".into(),
        60..3600 => format!("{} min", s / 60),
        3600..86400 => format!("{} h", s / 3600),
        _ => match Local.timestamp_opt(seen as i64, 0).single() {
            Some(t) if s < 6 * 86400 => t.format("%a").to_string(),
            Some(t) => t.format("%-d %b").to_string(),
            None => format!("{} d", s / 86400),
        },
    }
}

fn clock(at: u64) -> String {
    Local.timestamp_opt(at as i64, 0).single().map(|t| t.format("%H:%M").to_string()).unwrap_or_default()
}

/// The meta text style: 12/16, medium.
fn meta() -> Div {
    div().text_size(px(12.)).line_height(px(16.)).font_weight(FontWeight::MEDIUM)
}

fn icon_button(
    id: impl Into<gpui::ElementId>,
    glyph: Icon,
    size: f32,
    glyph_size: f32,
    theme: &Theme,
) -> Stateful<Div> {
    div()
        .id(id)
        .flex_none()
        .size(px(size))
        .rounded(RADIUS_SM)
        .flex()
        .items_center()
        .justify_center()
        .text_color(theme.ink_3)
        .cursor_pointer()
        .hover(|s| s.bg(theme.fill_hover).text_color(theme.ink))
        .active(|s| s.bg(theme.fill_press))
        .child(icon_sized(glyph, glyph_size))
}

fn button(id: impl Into<gpui::ElementId>, label: SharedString, primary: bool, theme: &Theme) -> Stateful<Div> {
    div()
        .id(id)
        .h(CONTROL_H)
        .px(px(12.))
        .rounded(RADIUS_SM)
        .flex()
        .items_center()
        .justify_center()
        .font_weight(FontWeight::SEMIBOLD)
        .cursor_pointer()
        .map(|el| {
            if primary {
                el.bg(theme.accent).text_color(theme.accent_ink).hover(|s| s.opacity(0.92))
            } else {
                el.bg(theme.fill).text_color(theme.ink).hover(|s| s.bg(theme.fill_hover))
            }
        })
        .active(|s| s.bg(theme.fill_press))
        .child(div().truncate().child(label))
}

fn ghost(id: impl Into<gpui::ElementId>, label: SharedString, theme: &Theme) -> Stateful<Div> {
    div()
        .id(id)
        .flex_none()
        .self_start()
        .px(px(8.))
        .rounded(RADIUS_SM)
        .flex()
        .items_center()
        .text_size(px(12.))
        .font_weight(FontWeight::MEDIUM)
        .text_color(theme.ink_2)
        .cursor_pointer()
        .hover(|s| s.bg(theme.fill_hover).text_color(theme.ink))
        .active(|s| s.bg(theme.fill_press))
        .child(label)
}
