//! Control components: Toggle, Slider, Button, Confirm, and the tone and size mapping.
//!
//! Builders return plain elements; callers attach their own click handlers.

use gpui::{
    Div, ElementId, FontWeight, InteractiveElement, ParentElement, Pixels, Rgba, SharedString, Stateful, Styled, div,
    px, relative, rgba,
};
use yozora_module::{Size, Tone};

use crate::{
    controls::switch,
    theme::{RADIUS_ROW, Theme},
};

/// Success colour in the dark theme (Aurora green).
const SUCCESS_DARK: u32 = 0x8FE3CFFF;
/// Success colour in the light theme (Aurora green).
const SUCCESS_LIGHT: u32 = 0x0D7F69FF;

/// The one place a semantic tone becomes a colour.
pub fn tone_color(tone: Tone, theme: &Theme) -> Rgba {
    match tone {
        Tone::Neutral => theme.ink_2,
        Tone::Accent => theme.accent,
        Tone::Success => rgba(if theme.dark { SUCCESS_DARK } else { SUCCESS_LIGHT }),
        Tone::Warning => theme.warn,
        Tone::Danger => theme.danger,
    }
}

/// The tone colour at about 16% alpha, for badge and icon backgrounds.
pub fn tone_soft(tone: Tone, theme: &Theme) -> Rgba {
    let mut c = tone_color(tone, theme);
    c.a = 0.16;
    c
}

/// The one place a semantic size becomes a row height: regular 40 px, compact 32 px.
pub fn row_height(size: Size) -> Pixels {
    match size {
        Size::Regular => px(40.),
        Size::Compact => px(32.),
    }
}

/// A toggle row: title on the left, switch on the right.
pub fn toggle(id: impl Into<ElementId>, title: impl Into<SharedString>, on: bool, theme: &Theme) -> Stateful<Div> {
    let id = id.into();
    div()
        .id((id.clone(), "toggle"))
        .flex()
        .items_center()
        .gap(px(12.))
        .w_full()
        .min_h(row_height(Size::Regular))
        .px(px(8.))
        .py(px(6.))
        .rounded(RADIUS_ROW)
        .text_size(px(13.))
        .child(div().flex_1().min_w_0().font_weight(FontWeight::MEDIUM).truncate().child(title.into()))
        .child(switch(id, on, theme))
}

/// A slider row: the title above a 22 px pill track filled to `value` (0..=1).
///
/// This is deliberately the design's static track rather than `crate::controls::slider`: that one
/// needs per-view drag state and a callback, which a declarative view tree does not carry. The
/// caller attaches pointer handlers to the returned element.
pub fn slider(id: impl Into<ElementId>, title: impl Into<SharedString>, value: f32, theme: &Theme) -> Stateful<Div> {
    let value = if value.is_finite() { value.clamp(0., 1.) } else { 0. };
    div()
        .id(id)
        .flex()
        .flex_col()
        .gap(px(6.))
        .w_full()
        .px(px(8.))
        .py(px(6.))
        .text_size(px(13.))
        .cursor_pointer()
        .child(div().font_weight(FontWeight::MEDIUM).truncate().child(title.into()))
        .child(
            div()
                .h(px(22.))
                .w_full()
                .rounded_full()
                .bg(theme.fill_2)
                .overflow_hidden()
                .child(div().h_full().w(relative(value)).bg(theme.accent)),
        )
}

/// A button. Primary is the accent fill with dark text; destructive has danger text.
pub fn button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    primary: bool,
    destructive: bool,
    theme: &Theme,
) -> Stateful<Div> {
    let base = div()
        .id(id)
        .flex_1()
        .flex()
        .items_center()
        .justify_center()
        .px(px(12.))
        .py(px(8.))
        .rounded(RADIUS_ROW)
        .text_size(px(13.))
        .font_weight(FontWeight::MEDIUM)
        .cursor_pointer()
        .border_1();
    let base = if primary {
        base.bg(theme.accent).text_color(theme.accent_ink).border_color(gpui::transparent_black())
    } else {
        base.bg(theme.fill_2).border_color(theme.hair).text_color(if destructive {
            tone_color(Tone::Danger, theme)
        } else {
            theme.ink
        })
    };
    base.child(label.into())
}

/// A row of buttons sharing the width equally.
pub fn buttons(items: Vec<Stateful<Div>>) -> Div {
    div().flex().gap(px(8.)).px(px(8.)).pt(px(12.)).pb(px(4.)).children(items)
}

/// A confirmation: the message, then Cancel and a destructive confirm button. The caller attaches
/// the click handlers to the buttons by id (`(id, "cancel")` and `(id, "confirm")`).
pub fn confirm(
    id: impl Into<ElementId>,
    message: impl Into<SharedString>,
    confirm_label: impl Into<SharedString>,
    theme: &Theme,
) -> Div {
    let id = id.into();
    div()
        .flex()
        .flex_col()
        .gap(px(12.))
        .p(px(12.))
        .child(div().text_size(px(13.)).text_color(theme.ink_2).text_center().child(message.into()))
        .child(
            div()
                .flex()
                .gap(px(8.))
                .child(button((id.clone(), "cancel"), "Cancel", false, false, theme))
                .child(button((id, "confirm"), confirm_label, false, true, theme)),
        )
}
