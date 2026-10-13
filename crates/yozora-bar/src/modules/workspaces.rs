//! Workspace buttons with a sprung indicator, and the focused window's title.

use gpui::{
    AnyElement, Context, FontWeight, InteractiveElement, IntoElement, MouseButton, ParentElement, SharedString, Styled,
    Window, div, prelude::*, px,
};
use yozora_ui::{Icon, Theme, icon_sized};

use crate::{
    app::Config,
    bar::Bar,
    services::{Services, hyprland},
};

const SLOT: f32 = 28.;
const GAP: f32 = 2.;
const PAD: f32 = 3.;

/// Workspace ids shown on `bar`'s monitor, in order.
pub fn slots(bar: &Bar, cx: &gpui::App) -> Vec<i32> {
    let config = &Config::get(cx).settings.bar.workspaces;
    let state = Services::get(cx).hyprland.as_ref().map(|h| &h.read(cx).state);
    let mut ids: Vec<i32> = if config.per_monitor {
        state
            .map(|s| s.workspaces.iter().filter(|w| w.monitor == bar.monitor && w.id > 0).map(|w| w.id).collect())
            .unwrap_or_default()
    } else {
        (1..=config.count.max(1) as i32).collect()
    };
    if let Some(s) = state {
        // Occupied workspaces past the configured count still get a button.
        for w in &s.workspaces {
            let mine = !config.per_monitor || w.monitor == bar.monitor;
            if w.id > 0 && w.windows > 0 && mine && !ids.contains(&w.id) {
                ids.push(w.id);
            }
        }
        if let Some(m) = s.monitor(&bar.monitor)
            && m.active_workspace > 0
            && !ids.contains(&m.active_workspace)
        {
            ids.push(m.active_workspace);
        }
    }
    ids.sort_unstable();
    ids.dedup();
    if ids.is_empty() {
        ids.push(1);
    }
    ids
}

pub fn render(bar: &mut Bar, theme: &Theme, window: &mut Window, cx: &mut Context<Bar>) -> Option<AnyElement> {
    let hypr = Services::get(cx).hyprland.clone()?;
    let ids = slots(bar, cx);
    let state = hypr.read(cx).state.clone();
    let active = state.monitor(&bar.monitor).map(|m| m.active_workspace);
    let target = active.and_then(|a| ids.iter().position(|&id| id == a));
    let pos = bar.indicator_position(target, theme.motion.reduced, window, cx);
    let current = target.unwrap_or(usize::MAX);
    let (fill_2, accent_glow) = (theme.fill_2, theme.accent_glow());

    let buttons = ids.iter().enumerate().map(|(i, &id)| {
        let cur = i == current;
        let occupied = state.occupied(id);
        let hypr = hypr.clone();
        div()
            .id(("workspace", i))
            .relative()
            .w(px(SLOT))
            .h(px(20.))
            .rounded(px(7.))
            .flex()
            .items_center()
            .justify_center()
            .text_size(px(12.))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(if cur {
                theme.accent_ink
            } else if occupied {
                theme.ink
            } else {
                theme.ink_3
            })
            .when(!cur, |el| el.hover(move |s| s.bg(fill_2)))
            .child(SharedString::from(id.to_string()))
            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                hypr.read(cx).send(hyprland::Cmd::Workspace(id));
            })
    });
    let hypr_scroll = hypr.clone();
    let scroll_ids = ids.clone();
    Some(
        div()
            .id("workspaces")
            .relative()
            .flex()
            .gap(px(GAP))
            .p(px(PAD))
            .mx(px(4.))
            .rounded(px(10.))
            .bg(theme.fill)
            .child(
                div()
                    .absolute()
                    .top(px(PAD))
                    .left(px(PAD + pos * (SLOT + GAP)))
                    .w(px(SLOT))
                    .h(px(20.))
                    .rounded(px(7.))
                    .bg(theme.accent)
                    .shadow(accent_glow),
            )
            .children(buttons)
            .on_scroll_wheel(move |e, _, cx| {
                let dy = match e.delta {
                    gpui::ScrollDelta::Lines(p) => p.y,
                    gpui::ScrollDelta::Pixels(p) => f32::from(p.y) / 40.,
                };
                if dy.abs() < 0.5 {
                    return;
                }
                let Some(i) = target else { return };
                let next = if dy > 0. { i.checked_sub(1) } else { Some(i + 1) };
                if let Some(&id) = next.and_then(|n| scroll_ids.get(n)) {
                    hypr_scroll.read(cx).send(hyprland::Cmd::Workspace(id));
                }
            })
            .into_any_element(),
    )
}

/// The focused window on this monitor: app icon, title and class.
pub fn window_title(bar: &mut Bar, theme: &Theme, cx: &mut Context<Bar>) -> Option<AnyElement> {
    let hypr = Services::get(cx).hyprland.clone()?;
    let state = &hypr.read(cx).state;
    let max = Config::get(cx).settings.bar.window.max_length;
    let Some(w) = state.window_on(&bar.monitor) else {
        let label = format!("Workspace {}", state.monitor(&bar.monitor)?.active_workspace);
        return Some(div().px(px(6.)).text_color(theme.ink_2).child(label).into_any_element());
    };
    let (title, class) = split_title(&w.title, &w.class);
    Some(
        div()
            .flex()
            .items_center()
            .gap(px(9.))
            .px(px(4.))
            .min_w_0()
            .overflow_hidden()
            .child(
                div()
                    .size(px(20.))
                    .flex_none()
                    .rounded(px(6.))
                    .bg(theme.fill_2)
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(theme.accent)
                    .child(icon_sized(Icon::App, 13.)),
            )
            .child(div().font_weight(FontWeight::MEDIUM).flex_none().child(super::ellipsize(&title, max)))
            .when_some(class, |el, c| el.child(div().text_color(theme.ink_2).truncate().child(format!("— {c}"))))
            .into_any_element(),
    )
}

/// Splits "bar.toml - yozora - Code" into the document and the app, when the title has one.
fn split_title(title: &str, class: &str) -> (String, Option<String>) {
    if title.is_empty() {
        return (class.to_string(), None);
    }
    for sep in [" — ", " - ", " – "] {
        if let Some((head, tail)) = title.split_once(sep) {
            return (head.to_string(), Some(tail.to_string()));
        }
    }
    // Reverse-DNS classes like "com.anthropic.Claude" read better as their last part.
    let class = class.rsplit('.').next().unwrap_or(class);
    (title.to_string(), (!class.is_empty() && !title.eq_ignore_ascii_case(class)).then(|| class.to_string()))
}

#[cfg(test)]
mod tests {
    #[test]
    fn splits_titles() {
        assert_eq!(super::split_title("bar.toml - yozora", "code"), ("bar.toml".into(), Some("yozora".into())));
        assert_eq!(super::split_title("", "kitty"), ("kitty".into(), None));
        assert_eq!(super::split_title("kitty", "kitty"), ("kitty".into(), None));
        assert_eq!(super::split_title("Claude", "com.anthropic.Claude"), ("Claude".into(), None));
    }
}
