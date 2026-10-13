//! Now playing over MPRIS: a compact bar item and a player popover.

use std::time::Duration;

use gpui::{
    AnyElement, App, Context, Div, FontWeight, IntoElement, ParentElement, SharedString,
    StatefulInteractiveElement, Styled, div, img, prelude::*, px, relative,
};
use yozora_ui::{Icon, Theme, controls, icon_sized};

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
pub fn cover(player: &Player, size: f32, radius: f32, theme: &Theme) -> AnyElement {
    match &player.art {
        Some(path) => img(path.clone()).size(px(size)).flex_none().rounded(px(radius)).into_any_element(),
        None => div()
            .size(px(size))
            .flex_none()
            .rounded(px(radius))
            .bg(theme.fill_3)
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
        bi().pl(px(4.)).pr(px(10.)).child(cover(player, 18., 5., theme)).child(
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
            controls::empty(&["Nothing is playing.", "Players that support MPRIS show up here."], theme).into_any_element(),
        ];
    };
    let word = match player.status {
        Status::Playing => "Now playing",
        Status::Paused => "Paused",
        Status::Stopped => "Stopped",
    };
    let head = div()
        .flex()
        .gap(px(14.))
        .items_center()
        .p(px(4.))
        .child(cover(&player, 88., 12., theme))
        .child(
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
                    .bg(theme.fill_3)
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
                .border_color(theme.hair)
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
            .gap(px(11.))
            .p(px(8.))
            .rounded(px(12.))
            .bg(theme.fill)
            .border_1()
            .border_color(theme.hair)
            .child(cover(&player, 40., 8., theme))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(div().font_weight(FontWeight::SEMIBOLD).truncate().child(if player.title.is_empty() {
                        player.identity.clone()
                    } else {
                        player.title.clone()
                    }))
                    .child(div().text_size(px(12.)).text_color(theme.ink_2).truncate().child(player.artist.clone())),
            )
            .child(
                controls::media_button("mini-play", if playing { Icon::Pause } else { Icon::Play }, false, theme)
                    .size(px(32.))
                    .on_click(|_, _, cx| send(Cmd::PlayPause, cx)),
            )
            .child(
                controls::media_button("mini-next", Icon::Next, false, theme)
                    .size(px(32.))
                    .on_click(|_, _, cx| send(Cmd::Next, cx)),
            )
            .into_any_element(),
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn formats_time() {
        assert_eq!(super::time(std::time::Duration::from_secs(74)), "1:14");
        assert_eq!(super::time(std::time::Duration::from_secs(3725)), "1:02:05");
    }
}
