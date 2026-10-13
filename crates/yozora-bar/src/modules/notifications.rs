//! Notification history from mako, and Do Not Disturb.

use gpui::{
    AnyElement, App, Context, Div, FontWeight, InteractiveElement, IntoElement, ParentElement,
    StatefulInteractiveElement, Styled, div, img, prelude::*, px,
};
use yozora_ui::{Icon, IconElement, Theme, controls, icon, icon_sized};

use crate::{
    bar::bi,
    popover::Popover,
    services::{Services, icons, notifications::Cmd},
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

pub fn popover(_p: &mut Popover, theme: &Theme, cx: &mut Context<Popover>) -> Vec<AnyElement> {
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
        rows.push(controls::empty(&["mako is not running.", e.as_str()], theme).into_any_element());
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
                .child(icon_sized(Icon::Bell, 22.).text_color(theme.accent))
                .child(controls::muted("You're all caught up.", theme))
                .into_any_element(),
        );
    }
    for (i, n) in state.items.iter().take(30).enumerate() {
        let id = n.id;
        let glyph = n.icon_path.clone();
        let fallback = fallback_icon(&n.app, &n.desktop_entry);
        let urgent = n.urgency >= 2;
        let when = n.seen.filter(|s| *s > 0).map(super::ago);
        rows.push(
            div()
                .id(("notification", i))
                .group("nt")
                .relative()
                .flex()
                .gap(px(11.))
                .px(px(11.))
                .py(px(10.))
                .mb(px(6.))
                .rounded(px(12.))
                .bg(theme.fill)
                .border_1()
                .border_color(if urgent { theme.danger } else { theme.hair })
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
                        .on_click(move |_, _, cx| send(Cmd::Dismiss(id), cx)),
                )
                .into_any_element(),
        );
    }
    let note = if fake { "History from mako · fake" } else { "History from mako" };
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

/// A built-in icon for a notification with no icon of its own, guessed from the app's name.
fn fallback_icon(app: &str, desktop_entry: &str) -> Icon {
    let name = format!("{app} {desktop_entry}").to_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| name.contains(w));
    if has(&["terminal", "kitty", "alacritty", "foot", "wezterm", "ghostty", "konsole"]) {
        Icon::Terminal
    } else if has(&["firefox", "chrom", "browser", "brave", "zen", "vivaldi", "epiphany"]) {
        Icon::Globe
    } else if has(&["update", "pacman", "paru", "yay", "package", "pamac", "flatpak"]) {
        Icon::Package
    } else if has(&["sync", "nextcloud", "dropbox", "drive", "cloud"]) {
        Icon::Sync
    } else if has(&["spotify", "music", "player", "mpv", "vlc"]) {
        Icon::Music
    } else if has(&["bluetooth", "blueman"]) {
        Icon::Bluetooth
    } else if has(&["network", "nm-applet"]) {
        Icon::Wifi
    } else if has(&["battery", "power", "upower"]) {
        Icon::Battery
    } else {
        Icon::Bell
    }
}

#[cfg(test)]
mod tests {
    use yozora_ui::Icon;

    #[test]
    fn guesses_fallback_icons() {
        assert_eq!(super::fallback_icon("Terminal", ""), Icon::Terminal);
        assert_eq!(super::fallback_icon("", "org.mozilla.firefox"), Icon::Globe);
        assert_eq!(super::fallback_icon("Syncthing", ""), Icon::Sync);
        assert_eq!(super::fallback_icon("notify-send", ""), Icon::Bell);
    }
}
