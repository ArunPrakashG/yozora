//! StatusNotifierItem tray icons and their DBusMenu menus.

use gpui::{
    AnyElement, Context, FontWeight, InteractiveElement, IntoElement, MouseButton, ParentElement, SharedString,
    StatefulInteractiveElement, Styled, div, img, prelude::*, px,
};
use yozora_ui::{Icon, IconElement, Theme, controls, icon, icon_sized};

use super::Pop;
use crate::{
    bar::{Bar, bi},
    popover::Popover,
    services::{
        Services,
        tray::{self, Cmd, MenuItem, Status, Toggle},
    },
};

fn send(cmd: Cmd, cx: &gpui::App) {
    if let Some(t) = &Services::get(cx).tray {
        t.read(cx).send(cmd);
    }
}

pub fn render(bar: &mut Bar, theme: &Theme, cx: &mut Context<Bar>) -> Option<AnyElement> {
    let tray = Services::get(cx).tray.clone()?;
    let items: Vec<_> = tray.read(cx).state.items.iter().filter(|i| i.status != Status::Passive).cloned().collect();
    if items.is_empty() {
        return None;
    }
    let buttons = items.into_iter().enumerate().map(|(i, item)| {
        let key = item.key.clone();
        let active = bar.popover_open(&Pop::Tray(key.clone()), cx);
        let glyph = match &item.icon {
            Some(tray::Icon::Builtin(glyph)) => icon(*glyph).text_color(theme.ink).into_any_element(),
            Some(tray::Icon::Symbolic(path)) => IconElement::from_path(path.to_string_lossy().into_owned(), 16.)
                .text_color(theme.ink)
                .into_any_element(),
            Some(tray::Icon::File(path)) => img(path.clone()).size(px(16.)).flex_none().into_any_element(),
            Some(tray::Icon::Pixmap(image)) => img(image.clone()).size(px(16.)).flex_none().into_any_element(),
            None => icon(Icon::App).into_any_element(),
        };
        let (left_key, right_key, middle_key, scroll_key) = (key.clone(), key.clone(), key.clone(), key.clone());
        let menu_only = item.item_is_menu;
        let has_menu = item.has_menu;
        let fill_2 = theme.fill_2;
        bi().id(("tray", i))
            .relative()
            .px(px(6.))
            .when(active, |el| el.bg(theme.fill_3))
            .when(!active, |el| el.hover(move |s| s.bg(fill_2)))
            .when(item.status == Status::NeedsAttention, |el| el.border_1().border_color(theme.accent))
            .child(glyph)
            .child(bar.track(format!("tray/{key}")))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |bar, _, window, cx| {
                    if menu_only && has_menu {
                        bar.toggle(Pop::Tray(left_key.clone()), true, window, cx);
                    } else {
                        send(Cmd::Activate(left_key.clone()), cx);
                    }
                }),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |bar, _, window, cx| {
                    if has_menu {
                        bar.toggle(Pop::Tray(right_key.clone()), true, window, cx);
                    } else {
                        send(Cmd::SecondaryActivate(right_key.clone()), cx);
                    }
                }),
            )
            .on_mouse_down(MouseButton::Middle, move |_, _, cx| send(Cmd::SecondaryActivate(middle_key.clone()), cx))
            .on_scroll_wheel(move |e, _, cx| {
                let dy = match e.delta {
                    gpui::ScrollDelta::Lines(p) => p.y,
                    gpui::ScrollDelta::Pixels(p) => f32::from(p.y) / 40.,
                };
                if dy.abs() >= 0.5 {
                    send(Cmd::Scroll(scroll_key.clone(), dy.signum() as i32), cx);
                }
            })
            .into_any_element()
    });
    Some(div().flex().items_center().gap(px(2.)).children(buttons.collect::<Vec<_>>()).into_any_element())
}

/// Menu labels mark mnemonics with underscores: "_File" and "Save __as" become "File", "Save _as".
fn label(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '_' {
            if chars.peek() == Some(&'_') {
                out.push('_');
                chars.next();
            }
        } else {
            out.push(c);
        }
    }
    out
}

pub fn popover(p: &mut Popover, key: &str, theme: &Theme, cx: &mut Context<Popover>) -> Vec<AnyElement> {
    let Some(tray) = Services::get(cx).tray.clone() else { return Vec::new() };
    let state = tray.read(cx).state.clone();
    let title = state.items.iter().find(|i| i.key == key).map(|i| i.title.clone()).unwrap_or_default();
    let Some((_, root)) = state.menu.as_ref().filter(|(k, _)| k == key) else {
        return vec![controls::empty(&["Loading menu…"], theme).into_any_element()];
    };
    // Walk down the submenu path, dropping parts that no longer exist.
    let mut items: &Vec<MenuItem> = root;
    let mut depth = 0;
    for id in &p.ui.submenu {
        match items.iter().find(|i| i.id == *id && !i.children.is_empty()) {
            Some(i) => {
                items = &i.children;
                depth += 1;
            }
            None => break,
        }
    }
    p.ui.submenu.truncate(depth);

    let mut rows: Vec<AnyElement> = Vec::new();
    if depth > 0 {
        rows.push(
            div()
                .id("tray-back")
                .h(px(30.))
                .px(px(6.))
                .flex()
                .items_center()
                .gap(px(6.))
                .rounded(px(8.))
                .text_color(theme.ink_2)
                .cursor_pointer()
                .hover(|s| s.bg(theme.fill_2))
                .child(icon_sized(Icon::ChevronLeft, 14.))
                .child("Back")
                .on_click(cx.listener(|p, _, _, cx| {
                    p.ui.submenu.pop();
                    cx.notify();
                }))
                .into_any_element(),
        );
    } else if !title.is_empty() {
        rows.push(
            div()
                .px(px(10.))
                .pt(px(4.))
                .pb(px(6.))
                .text_size(px(11.5))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme.ink_3)
                .child(title)
                .into_any_element(),
        );
    }
    let key = key.to_string();
    let entries = items.iter().enumerate().map(|(i, item)| {
        if item.separator {
            return div().h(px(1.)).mx(px(6.)).my(px(4.)).bg(theme.hair).into_any_element();
        }
        let (id, enabled, submenu) = (item.id, item.enabled, !item.children.is_empty());
        let key = key.clone();
        let mark = match item.toggle {
            Toggle::Check(true) | Toggle::Radio(true) => Some(Icon::Check),
            Toggle::Radio(false) => Some(Icon::Dot),
            _ => None,
        };
        let has_marks = items.iter().any(|i| i.toggle != Toggle::None);
        div()
            .id(("tray-item", i))
            .h(px(30.))
            .px(px(10.))
            .flex()
            .items_center()
            .gap(px(8.))
            .rounded(px(8.))
            .when(!enabled, |el| el.text_color(theme.ink_3))
            .when(enabled, |el| el.cursor_pointer().hover(|s| s.bg(theme.accent_soft)))
            .when(has_marks, |el| {
                el.child(
                    div().size(px(14.)).flex_none().text_color(theme.accent).children(mark.map(|m| icon_sized(m, 14.))),
                )
            })
            .child(div().flex_1().truncate().child(SharedString::from(label(&item.label))))
            .when(submenu, |el| el.child(icon_sized(Icon::ChevronRight, 14.).text_color(theme.ink_3)))
            .on_click(cx.listener(move |p, _, _, cx| {
                if !enabled {
                    return;
                }
                if submenu {
                    p.ui.submenu.push(id);
                    cx.notify();
                } else {
                    send(Cmd::MenuClick(key.clone(), id), cx);
                    p.close(cx);
                }
            }))
            .into_any_element()
    });
    rows.push(div().flex().flex_col().children(entries.collect::<Vec<_>>()).into_any_element());
    rows
}

#[cfg(test)]
mod tests {
    #[test]
    fn strips_mnemonics() {
        assert_eq!(super::label("_File"), "File");
        assert_eq!(super::label("Save __as"), "Save _as");
    }
}
