//! Settings' building blocks from the design: cards, rows, option cards, swatches and buttons.

use gpui::{
    Div, ElementId, FontWeight, Hsla, InteractiveElement, IntoElement, ParentElement, Rgba, SharedString, Stateful,
    Styled, div, linear_color_stop, linear_gradient, prelude::FluentBuilder, px, rgba,
};
use yozora_ui::{Icon, Theme, icon_sized};

/// The small uppercase label above a group of settings.
pub fn group_label(text: impl Into<SharedString>, first: bool, theme: &Theme) -> Div {
    div()
        .when(!first, |el| el.mt(px(24.)))
        .mb(px(10.))
        .ml(px(2.))
        .text_size(px(11.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme.ink_3)
        .child(text.into().to_uppercase())
}

pub fn card(theme: &Theme) -> Div {
    div().flex().flex_col().rounded(px(14.)).bg(theme.fill).border_1().border_color(theme.hair)
}

/// A setting: title and help on the left, its control on the right.
pub fn setting_row(
    title: impl Into<SharedString>,
    help: Option<SharedString>,
    control: impl IntoElement,
    first: bool,
    theme: &Theme,
) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(16.))
        .min_h(px(58.))
        .px(px(18.))
        .py(px(10.))
        .when(!first, |el| el.border_t_1().border_color(theme.hair))
        .child(text_block(title, help, theme))
        .child(control)
}

pub fn text_block(title: impl Into<SharedString>, help: Option<SharedString>, theme: &Theme) -> Div {
    div()
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .gap(px(2.))
        .child(div().text_size(px(13.5)).font_weight(FontWeight::MEDIUM).child(title.into()))
        .when_some(help, |el, help| el.child(div().text_size(px(12.5)).text_color(theme.ink_2).child(help)))
}

/// A selectable card with a thumbnail, a title and one line of detail.
pub fn option_card(
    id: impl Into<ElementId>,
    selected: bool,
    thumb: impl IntoElement,
    title: impl Into<SharedString>,
    detail: impl Into<SharedString>,
    theme: &Theme,
) -> Stateful<Div> {
    div()
        .id(id)
        .relative()
        .flex_1()
        .min_w_0()
        .p(px(8.))
        .rounded(px(14.))
        .bg(theme.fill)
        .cursor_pointer()
        .hover(|s| s.bg(theme.fill_2))
        .border_2()
        .border_color(if selected { theme.accent } else { theme.hair })
        .child(thumb)
        .child(
            div()
                .flex()
                .items_baseline()
                .justify_between()
                .gap(px(8.))
                .pt(px(10.))
                .px(px(4.))
                .pb(px(2.))
                .child(div().text_size(px(13.)).font_weight(FontWeight::SEMIBOLD).child(title.into()))
                .child(div().text_size(px(12.)).text_color(theme.ink_2).truncate().child(detail.into())),
        )
        .when(selected, |el| {
            el.child(
                div()
                    .absolute()
                    .top(px(14.))
                    .left(px(14.))
                    .size(px(20.))
                    .rounded_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(theme.accent)
                    .text_color(theme.accent_ink)
                    .shadow(theme.accent_glow())
                    .child(icon_sized(Icon::Check, 12.)),
            )
        })
}

/// The night or dawn sky behind thumbnails.
pub fn sky(light: bool) -> gpui::Background {
    if light {
        linear_gradient(160., linear_color_stop(rgba(0xE9EEF8FF), 0.), linear_color_stop(rgba(0xC9D5EAFF), 1.))
    } else {
        linear_gradient(160., linear_color_stop(rgba(0x1A2747FF), 0.), linear_color_stop(rgba(0x070B14FF), 0.8))
    }
}

/// A tiny bar for thumbnails: a few marks left, a few right.
pub fn mini_bar(floating: bool, light: bool, accent: Rgba) -> Div {
    let mark = |w: f32, color: Hsla| div().w(px(w)).h(px(4.)).rounded(px(2.)).bg(color);
    let ink: Hsla = if light { rgba(0x0F16264D).into() } else { rgba(0xE8F1FF66).into() };
    div()
        .absolute()
        .top(px(if floating { 6. } else { 0. }))
        .left(px(if floating { 8. } else { 0. }))
        .right(px(if floating { 8. } else { 0. }))
        .h(px(11.))
        .flex()
        .items_center()
        .justify_between()
        .px(px(5.))
        .bg(if light { rgba(0xF6F8FCB3) } else { rgba(0x0A0E179E) })
        .border_color(if light { rgba(0x14285A1F) } else { rgba(0x96B9FF24) })
        .when(floating, |el| el.rounded(px(4.)).border_1())
        .when(!floating, |el| el.border_b_1())
        .child(div().flex().gap(px(3.)).child(mark(8., accent.into())).child(mark(5., ink)).child(mark(5., ink)))
        .child(div().flex().gap(px(3.)).child(mark(5., ink)).child(mark(5., ink)).child(mark(22., ink)))
}

pub fn thumb(light: bool) -> Div {
    div().relative().h(px(92.)).rounded(px(9.)).overflow_hidden().bg(sky(light))
}

/// An accent swatch with a ring and check when selected.
pub fn swatch(id: impl Into<ElementId>, color: Rgba, selected: bool, theme: &Theme) -> Stateful<Div> {
    div()
        .id(id)
        .size(px(36.))
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .cursor_pointer()
        .border_2()
        .border_color(if selected { theme.ink } else { gpui::transparent_black().into() })
        .child(
            div()
                .size(px(28.))
                .rounded_full()
                .bg(color)
                .border_1()
                .border_color(rgba(0xFFFFFF2E))
                .flex()
                .items_center()
                .justify_center()
                .text_color(rgba(0x0A0E17FF))
                .when(selected, |el| el.child(icon_sized(Icon::Check, 16.))),
        )
}

pub fn button(
    id: impl Into<ElementId>,
    glyph: Option<Icon>,
    text: impl Into<SharedString>,
    primary: bool,
    theme: &Theme,
) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .flex_none()
        .items_center()
        .gap(px(7.))
        .h(px(32.))
        .px(px(13.))
        .rounded(px(9.))
        .text_size(px(12.5))
        .font_weight(FontWeight::MEDIUM)
        .cursor_pointer()
        .when(primary, |el| el.bg(theme.accent).text_color(theme.accent_ink))
        .when(!primary, |el| el.bg(theme.fill_2).border_1().border_color(theme.hair).hover(|s| s.bg(theme.fill_3)))
        .when_some(glyph, |el, g| el.child(icon_sized(g, 14.)))
        .child(text.into())
}

/// An accent text link with a chevron.
pub fn link(id: impl Into<ElementId>, text: impl Into<SharedString>, theme: &Theme) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .flex_none()
        .items_center()
        .gap(px(4.))
        .px(px(8.))
        .py(px(6.))
        .rounded(px(8.))
        .text_size(px(13.))
        .text_color(theme.accent)
        .cursor_pointer()
        .hover(|s| s.bg(theme.accent_soft))
        .child(text.into())
        .child(icon_sized(Icon::ChevronRight, 14.))
}

/// Mono text in the accent color, for keys, commands and paths.
pub fn code(text: impl Into<SharedString>, theme: &Theme) -> Div {
    div().font_family(theme.mono.clone()).text_size(px(11.5)).text_color(theme.accent).child(text.into())
}

/// A notice inside a page, for errors and empty states.
pub fn notice(
    glyph: Icon,
    title: impl Into<SharedString>,
    body: impl Into<SharedString>,
    danger: bool,
    theme: &Theme,
) -> Div {
    div()
        .flex()
        .gap(px(12.))
        .items_start()
        .p(px(14.))
        .rounded(px(14.))
        .bg(theme.fill)
        .border_1()
        .border_color(if danger { theme.danger } else { theme.hair })
        .child(
            div()
                .size(px(30.))
                .flex_none()
                .rounded(px(9.))
                .flex()
                .items_center()
                .justify_center()
                .bg(if danger { theme.danger } else { theme.accent })
                .text_color(theme.accent_ink)
                .child(icon_sized(glyph, 16.)),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(2.))
                .min_w_0()
                .child(div().text_size(px(13.)).font_weight(FontWeight::SEMIBOLD).child(title.into()))
                .child(div().text_size(px(12.)).text_color(theme.ink_2).child(body.into())),
        )
}
