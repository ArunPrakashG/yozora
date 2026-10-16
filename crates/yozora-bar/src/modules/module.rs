//! `module/<name>` modules: the bar item and popover for a module run by the module runtime.
//!
//! The bar item comes from [`super::module_view`]. The popover is the module's view tree turned
//! into kit components, so every module looks the same. Nothing here styles anything itself.

use std::{cell::Cell, rc::Rc};

use gpui::{
    AnyElement, App, Bounds, ClickEvent, Context, Div, ElementId, InteractiveElement, IntoElement, MouseButton,
    MouseDownEvent, ParentElement, Pixels, StatefulInteractiveElement, Styled, canvas, div, prelude::*, px,
};
use serde_json::{Value, json};
use yozora_module::{Node, Tone};
use yozora_ui::{
    Icon, Theme, icon,
    kit::{
        controls::{self, tone_color},
        display::{self, RowProps},
    },
};

use super::module_view::{ERROR_ICON, item_view};
use crate::{
    bar::bi,
    popover::Popover,
    services::{Services, modules::ActionOutcome},
};

/// Sections nest at most this deep; anything deeper shows the error state.
const MAX_DEPTH: usize = 3;

/// An action that asked to be confirmed and is waiting for the user.
#[derive(Clone, Debug)]
pub struct Pending {
    pub action: String,
    pub value: Option<Value>,
    pub label: String,
}

/// The icon for a module-supplied name. The standard error icon is not in the kit's list of
/// module icons, so it is mapped here.
fn glyph(name: &str) -> Icon {
    if name == ERROR_ICON { Icon::Warning } else { display::icon_for(name) }
}

fn state_of(name: &str, cx: &App) -> crate::services::modules::ModuleState {
    Services::get(cx).modules.as_ref().and_then(|h| h.read(cx).states.get(name)).cloned().unwrap_or_default()
}

/// The bar item. Clicking it is wired by the bar, the same way as the built-in items.
pub fn item(name: &str, theme: &Theme, cx: &App) -> Div {
    let state = state_of(name, cx);
    let v = item_view(state.last_good.as_ref(), state.error.as_deref());
    let color = if v.tone == Tone::Neutral { theme.ink } else { tone_color(v.tone, theme) };
    bi().text_color(color)
        .text_size(px(12.5))
        .child(icon(glyph(&v.icon)))
        .when_some(v.text, |el, t| el.child(div().max_w(px(260.)).truncate().child(t)))
        .when_some(v.badge, |el, n| el.child(display::badge(n.to_string(), v.tone, theme)))
}

/// The standard error state, for anything that can't be drawn.
fn error_state(theme: &Theme) -> AnyElement {
    display::empty("Something went wrong", Some("This part of the module could not be shown.".into()), theme)
        .into_any_element()
}

/// Sends an action to the module. A destructive one answers `NeedsConfirm` first, and is kept in
/// the popover until confirmed.
fn send(p: &mut Popover, name: &str, pending: Pending, confirmed: bool, cx: &mut Context<Popover>) {
    let outcome = Services::get(cx).modules.as_ref().and_then(|h| {
        h.read(cx).handle(name).map(|h| h.send_action(&pending.action, pending.value.clone(), confirmed))
    });
    match outcome {
        Some(ActionOutcome::NeedsConfirm) => p.ui.module_confirm = Some(pending),
        other => {
            if other != Some(ActionOutcome::Sent) {
                tracing::warn!("module/{name}: action {:?} was not delivered: {other:?}", pending.action);
            }
            p.ui.module_confirm = None;
        }
    }
    cx.notify();
}

fn on_action(
    name: &str,
    action: &str,
    value: Option<Value>,
    label: &str,
    cx: &mut Context<Popover>,
) -> impl Fn(&ClickEvent, &mut gpui::Window, &mut App) + 'static {
    let (name, pending) = (name.to_string(), Pending { action: action.into(), value, label: label.into() });
    cx.listener(move |p, _: &ClickEvent, _, cx| send(p, &name, pending.clone(), false, cx))
}

/// The fraction of the track a press at `x` lands on, to two decimals.
fn slider_fraction(x: f32, left: f32, width: f32) -> f32 {
    let f = ((x - left) / width.max(1.)).clamp(0., 1.);
    (f * 100.).round() / 100.
}

/// Everything inside the card for one module.
pub fn popover(p: &mut Popover, name: &str, theme: &Theme, cx: &mut Context<Popover>) -> Vec<AnyElement> {
    let state = state_of(name, cx);
    let body = state.last_good.as_ref().and_then(|u| u.popover.as_ref());
    let mut out: Vec<AnyElement> = Vec::new();
    match body {
        Some(b) => out.push(display::title(b.title.clone(), b.caption.as_deref(), theme).into_any_element()),
        None => out.push(display::title(name.to_string(), None, theme).into_any_element()),
    }
    // The error goes on top; the last good body stays below it.
    if let Some(e) = &state.error {
        out.push(display::note(e.clone(), theme).into_any_element());
    }
    if let Some(pending) = p.ui.module_confirm.clone() {
        out.push(confirm_view(name, pending, theme, cx));
    }
    match body {
        Some(b) => {
            let mut next = 0;
            for n in &b.body {
                out.push(node(n, name, theme, cx, &mut next, 0));
            }
            if let Some(f) = &b.footer_note {
                out.push(display::note(f.clone(), theme).into_any_element());
            }
        }
        None if state.error.is_some() => out.push(error_state(theme)),
        None => out.push(display::empty("Nothing to show", None, theme).into_any_element()),
    }
    out
}

/// The kit's `confirm` has no way to attach click handlers to its buttons, so this lays out the
/// same message and the same two kit buttons itself.
fn confirm_view(name: &str, pending: Pending, theme: &Theme, cx: &mut Context<Popover>) -> AnyElement {
    let name = name.to_string();
    let (label, again) = (pending.label.clone(), pending);
    div()
        .flex()
        .flex_col()
        .gap(px(12.))
        .p(px(12.))
        .child(div().text_size(px(13.)).text_color(theme.ink_2).text_center().child(format!("{label}?")))
        .child(
            div()
                .flex()
                .gap(px(8.))
                .child(controls::button("module-cancel", "Cancel", false, false, theme).on_click(cx.listener(
                    |p, _: &ClickEvent, _, cx| {
                        p.ui.module_confirm = None;
                        cx.notify();
                    },
                )))
                .child(
                    controls::button("module-confirm", label, false, true, theme)
                        .on_click(cx.listener(move |p, _: &ClickEvent, _, cx| send(p, &name, again.clone(), true, cx))),
                ),
        )
        .into_any_element()
}

fn node(n: &Node, name: &str, theme: &Theme, cx: &mut Context<Popover>, next: &mut usize, depth: usize) -> AnyElement {
    *next += 1;
    let id = ElementId::from(("module-node", *next));
    match n {
        Node::Section { heading, rows } => {
            if depth >= MAX_DEPTH {
                return error_state(theme);
            }
            let rows = rows.iter().map(|r| node(r, name, theme, cx, next, depth + 1)).collect();
            display::section(heading.as_deref(), rows, theme).into_any_element()
        }
        Node::Row { icon, title, subtitle, value, badge, tone, size, action } => {
            let props = RowProps {
                icon: icon.as_deref().map(glyph),
                title: title.clone().into(),
                subtitle: subtitle.clone().map(Into::into),
                value: value.clone().map(Into::into),
                badge: badge.as_ref().map(|b| (b.text.clone().into(), b.tone)),
                tone: *tone,
                size: *size,
                clickable: action.is_some(),
            };
            let row = display::row(id, props, theme);
            match action {
                Some(a) => row.on_click(on_action(name, a, None, title, cx)).into_any_element(),
                None => row.into_any_element(),
            }
        }
        Node::Toggle { title, on, action } => controls::toggle(id, title.clone(), *on, theme)
            .on_click(on_action(name, action, Some(json!(!on)), title, cx))
            .into_any_element(),
        Node::Slider { title, value, action } => {
            let bounds = Rc::new(Cell::new(Bounds::<Pixels>::default()));
            let track = bounds.clone();
            let (name, action, label) = (name.to_string(), action.clone(), title.clone());
            controls::slider(id, title.clone(), *value, theme)
                .relative()
                .child(canvas(move |b, _, _| track.set(b), |_, _, _, _| {}).absolute().top_0().left_0().size_full())
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |p, e: &MouseDownEvent, _, cx| {
                        // The kit's track is inset by the row padding of 8 px on each side.
                        let b = bounds.get();
                        let (left, width) = (f32::from(b.origin.x) + 8., f32::from(b.size.width) - 16.);
                        let v = slider_fraction(f32::from(e.position.x), left, width);
                        let pending = Pending { action: action.clone(), value: Some(json!(v)), label: label.clone() };
                        send(p, &name, pending, false, cx);
                    }),
                )
                .into_any_element()
        }
        Node::Progress { title, subtitle, value } => {
            display::progress(title.clone(), subtitle.clone().map(Into::into), *value, theme).into_any_element()
        }
        Node::Buttons { items } => {
            let items = items
                .iter()
                .map(|b| {
                    *next += 1;
                    controls::button(("module-button", *next), b.label.clone(), b.primary, b.destructive, theme)
                        .on_click(on_action(name, &b.action, None, &b.label, cx))
                })
                .collect();
            controls::buttons(items).into_any_element()
        }
        Node::Empty { title, message } => {
            display::empty(title.clone(), message.clone().map(Into::into), theme).into_any_element()
        }
        Node::Note { text } => display::note(text.clone(), theme).into_any_element(),
    }
}

#[cfg(test)]
mod tests {
    use yozora_module::{Popover, Update};

    use super::*;

    const UPDATE: &str = r#"{
        "bar": {"icon": "star", "text": "14", "badge": 2, "tone": "accent"},
        "popover": {
            "title": "Updates",
            "caption": "14 pending",
            "body": [
                {"type": "section", "heading": "Needs attention", "rows": [
                    {"type": "row", "icon": "warning", "title": "Manual step", "subtitle": "see news",
                     "badge": {"text": "news", "tone": "warning"}, "tone": "warning", "action": "open"},
                    {"type": "row", "title": "mesa", "value": "26.1.3-1"}
                ]},
                {"type": "toggle", "title": "Notify", "on": true, "action": "notify"},
                {"type": "slider", "title": "Level", "value": 0.4, "action": "level"},
                {"type": "progress", "title": "Download", "subtitle": "3 of 5", "value": 0.6},
                {"type": "buttons", "items": [
                    {"label": "Check now", "action": "check"},
                    {"label": "Update", "action": "update", "primary": true}
                ]},
                {"type": "empty", "title": "Up to date", "message": "Nothing pending."},
                {"type": "note", "text": "Opens your terminal."}
            ],
            "footer_note": "checked 2m ago"
        }
    }"#;

    fn popover() -> Popover {
        Update::parse_line(&UPDATE.split_whitespace().collect::<Vec<_>>().join(" ")).unwrap().popover.unwrap()
    }

    fn names(nodes: &[Node], out: &mut Vec<&'static str>) {
        for n in nodes {
            out.push(display::component_name(n));
            if let Node::Section { rows, .. } = n {
                names(rows, out);
            }
        }
    }

    #[test]
    fn module_render_popover_nodes() {
        let mut got = Vec::new();
        names(&popover().body, &mut got);
        assert_eq!(got, ["section", "row", "row", "toggle", "slider", "progress", "buttons", "empty", "note"]);
        // Every name is a documented component.
        assert!(got.iter().all(|n| yozora_module::COMPONENTS.contains(n)));
    }

    #[test]
    fn module_render_icons_map() {
        assert_eq!(glyph("warning"), Icon::Warning);
        assert_eq!(glyph("star"), Icon::Star);
        assert_eq!(glyph("no-such-icon"), Icon::Dot);
    }

    #[test]
    fn module_render_slider_fraction() {
        assert_eq!(slider_fraction(58., 8., 100.), 0.5);
        assert_eq!(slider_fraction(-5., 8., 100.), 0.);
        assert_eq!(slider_fraction(500., 8., 100.), 1.);
        assert_eq!(slider_fraction(10., 8., 0.), 1.);
    }
}
