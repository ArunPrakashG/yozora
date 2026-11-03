//! Now playing over MPRIS: a compact bar item and a player popover.

use std::time::Duration;

use gpui::{
    AnyElement, App, Context, Div, FontWeight, IntoElement, ParentElement, Pixels, SharedString,
    StatefulInteractiveElement, Styled, div, img, prelude::*, px, relative,
};
use yozora_ui::{
    Icon, Theme, controls, icon_sized,
    theme::{RADIUS_MD, RADIUS_SM, RADIUS_XS},
};

use crate::{
    app::Config,
    bar::bi,
    popover::Popover,
    services::{
        Services,
        media::{Cmd, Player, Status},
    },
};

pub fn playing(cx: &App) -> bool {
    Services::get(cx)
        .media
        .as_ref()
        .and_then(|m| m.read(cx).state.current().map(|p| p.status == Status::Playing))
        .unwrap_or(false)
}

/// Album art, or a music glyph on a fill when there is none.
pub fn cover(player: &Player, size: f32, radius: Pixels, theme: &Theme) -> AnyElement {
    match &player.art {
        Some(path) => img(path.clone()).size(px(size)).flex_none().rounded(radius).into_any_element(),
        None => div()
            .size(px(size))
            .flex_none()
            .rounded(radius)
            .bg(theme.fill_press)
            .flex()
            .items_center()
            .justify_center()
            .text_color(theme.ink_2)
            .child(icon_sized(Icon::Music, (size * 0.45).max(11.)))
            .into_any_element(),
    }
}

pub fn item(theme: &Theme, cx: &App) -> Option<Div> {
    let media = Services::get(cx).media.as_ref()?.read(cx);
    let player = media.state.current()?;
    if player.title.is_empty() {
        return None;
    }
    let max = Config::get(cx).settings.bar.media.max_length;
    let on = player.status == Status::Playing;
    Some(
        bi().pl(px(4.)).pr(px(10.)).child(cover(player, 18., RADIUS_XS, theme)).child(
            div()
                .flex()
                .gap(px(6.))
                .max_w(px(230.))
                .overflow_hidden()
                .text_size(px(12.5))
                .when(!on, |el| el.opacity(0.62))
                .child(div().font_weight(FontWeight::SEMIBOLD).flex_none().child(super::ellipsize(&player.title, max)))
                .when(!player.artist.is_empty(), |el| {
                    el.child(div().text_color(theme.ink_2).truncate().child(player.artist.clone()))
                }),
        ),
    )
}

pub fn time(d: Duration) -> String {
    let s = d.as_secs();
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

pub fn send(cmd: Cmd, cx: &App) {
    if let Some(m) = &Services::get(cx).media {
        m.read(cx).send(cmd);
    }
}

pub fn popover(_p: &mut Popover, theme: &Theme, cx: &mut Context<Popover>) -> Vec<AnyElement> {
    let Some(media) = Services::get(cx).media.clone() else {
        return vec![controls::empty(&["Media is not configured."], theme).into_any_element()];
    };
    let state = media.read(cx).state.clone();
    let Some(player) = state.current().cloned() else {
        return vec![
            controls::header("Now playing", div()).into_any_element(),
            controls::empty(&["Nothing is playing.", "Players that support MPRIS show up here."], theme)
                .into_any_element(),
        ];
    };
    let word = match player.status {
        Status::Playing => "Now playing",
        Status::Paused => "Paused",
        Status::Stopped => "Stopped",
    };
    let head = div().flex().gap(px(14.)).items_center().p(px(4.)).child(cover(&player, 88., RADIUS_MD, theme)).child(
        div()
            .min_w_0()
            .flex()
            .flex_col()
            .gap(px(2.))
            .child(super::calendar::eyebrow(word, theme))
            .child(
                div()
                    .text_size(px(15.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .mt(px(3.))
                    .child(if player.title.is_empty() { player.identity.clone() } else { player.title.clone() }),
            )
            .when(!player.artist.is_empty(), |el| el.child(controls::muted(player.artist.clone(), theme)))
            .when(!player.album.is_empty(), |el| el.child(controls::muted(player.album.clone(), theme))),
    );

    let pos = player.position_now();
    let progress = player.length.filter(|l| !l.is_zero()).map(|length| {
        let f = (pos.as_secs_f32() / length.as_secs_f32()).clamp(0., 1.);
        div()
            .px(px(4.))
            .child(
                div()
                    .relative()
                    .h(px(4.))
                    .mt(px(16.))
                    .rounded(px(2.))
                    .bg(theme.fill_press)
                    .overflow_hidden()
                    .child(div().absolute().top_0().bottom_0().left_0().w(relative(f)).rounded(px(2.)).bg(theme.ink)),
            )
            .child(
                div()
                    .flex()
                    .justify_between()
                    .mt(px(6.))
                    .text_size(px(11.))
                    .text_color(theme.ink_2)
                    .child(time(pos))
                    .child(format!("-{}", time(length.saturating_sub(pos)))),
            )
            .into_any_element()
    });

    let playing = player.status == Status::Playing;
    let ctl = div()
        .flex()
        .items_center()
        .justify_center()
        .gap(px(16.))
        .pt(px(8.))
        .pb(px(4.))
        .child(
            controls::media_button("previous", Icon::Previous, false, theme)
                .when(!player.can_previous, |el| el.opacity(0.4))
                .on_click(|_, _, cx| send(Cmd::Previous, cx)),
        )
        .child(
            controls::media_button("play", if playing { Icon::Pause } else { Icon::Play }, true, theme)
                .on_click(|_, _, cx| send(Cmd::PlayPause, cx)),
        )
        .child(
            controls::media_button("next", Icon::Next, false, theme)
                .when(!player.can_next, |el| el.opacity(0.4))
                .on_click(|_, _, cx| send(Cmd::Next, cx)),
        );

    let mut rows = vec![head.into_any_element()];
    rows.extend(progress);
    rows.push(ctl.into_any_element());
    if state.players.len() > 1 {
        rows.push(
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap(px(6.))
                .pt(px(10.))
                .mt(px(8.))
                .border_t_1()
                .border_color(theme.line)
                .child(div().px(px(4.)).text_size(px(11.5)).text_color(theme.ink_3).child("Players"))
                .children(state.players.iter().enumerate().map(|(i, pl)| {
                    let bus = pl.bus.clone();
                    controls::pill(("player", i), SharedString::from(pl.identity.clone()), pl.bus == player.bus, theme)
                        .on_click(move |_, _, cx| send(Cmd::Select(bus.clone()), cx))
                }))
                .into_any_element(),
        );
    }
    rows
}

/// The control center's one-line player.
pub fn mini(theme: &Theme, cx: &App) -> Option<AnyElement> {
    let media = Services::get(cx).media.as_ref()?.read(cx);
    let player = media.state.current()?.clone();
    let playing = player.status == Status::Playing;
    Some(
        div()
            .flex()
            .items_center()
            .gap(px(12.))
            .p(px(10.))
            .rounded(RADIUS_MD)
            .bg(theme.surface_2)
            .border_1()
            .border_color(theme.line)
            .child(cover(&player, 44., RADIUS_SM, theme))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .line_height(px(18.))
                    .child(div().font_weight(FontWeight::SEMIBOLD).truncate().child(if player.title.is_empty() {
                        player.identity.clone()
                    } else {
                        player.title.clone()
                    }))
                    .child(
                        div()
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(theme.ink_2)
                            .truncate()
                            .child(if player.artist.is_empty() {
                                player.identity.clone()
                            } else {
                                format!("{} · {}", player.artist, player.identity)
                            }),
                    ),
            )
            .child(mini_button("mini-prev", Icon::Previous, false, theme).on_click(|_, _, cx| send(Cmd::Previous, cx)))
            .child(
                mini_button("mini-play", if playing { Icon::Pause } else { Icon::Play }, true, theme)
                    .on_click(|_, _, cx| send(Cmd::PlayPause, cx)),
            )
            .child(mini_button("mini-next", Icon::Next, false, theme).on_click(|_, _, cx| send(Cmd::Next, cx)))
            .into_any_element(),
    )
}

/// A 28 px icon button in the mini player. The main one is full ink.
fn mini_button(id: &'static str, glyph: Icon, main: bool, theme: &Theme) -> gpui::Stateful<Div> {
    div()
        .id(id)
        .flex_none()
        .size(px(28.))
        .rounded(RADIUS_SM)
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .text_color(if main { theme.ink } else { theme.ink_3 })
        .hover(|s| s.bg(theme.fill_hover).text_color(theme.ink))
        .active(|s| s.bg(theme.fill_press))
        .child(icon_sized(glyph, 15.))
}

#[cfg(test)]
mod tests {
    #[test]
    fn formats_time() {
        assert_eq!(super::time(std::time::Duration::from_secs(74)), "1:14");
        assert_eq!(super::time(std::time::Duration::from_secs(3725)), "1:02:05");
    }
}
