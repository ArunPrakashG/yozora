//! Controls from the design: switches, tiles, rows, sliders and segmented pickers.
//!
//! These are plain element builders. Callers attach their own click handlers to the returned
//! `Stateful<Div>`. State that must survive frames, like spring positions, lives in GPUI element
//! state keyed by the id passed in.

use std::{cell::Cell, rc::Rc};

use gpui::{
    AnimationExt, App, Bounds, Div, ElementId, FontWeight, InteractiveElement, IntoElement, MouseButton,
    MouseMoveEvent, MouseUpEvent, ParentElement, Pixels, SharedString, SpringAnimation, SpringConfig, SpringPlayback,
    Stateful, StatefulInteractiveElement, Styled, Window, canvas, div, prelude::FluentBuilder, px, relative,
};

use crate::{
    icons::{Icon, icon, icon_sized},
    theme::{RADIUS_ROW, RADIUS_TILE, Theme},
};

/// The spring for small controls: the design's indicator spring, or a snap with reduced motion.
pub fn indicator_spring<T: gpui::SpringTarget>(theme: &Theme, target: T) -> SpringAnimation<T> {
    let p = theme.motion.indicator;
    SpringAnimation::new(SpringConfig::new(p.stiffness, p.damping, 1.0)).to(target).playback(if theme.motion.reduced {
        SpringPlayback::Completed
    } else {
        SpringPlayback::Running
    })
}

/// A 34×20 switch with a sprung knob.
pub fn switch(id: impl Into<ElementId>, on: bool, theme: &Theme) -> Stateful<Div> {
    let id = id.into();
    div()
        .id(id.clone())
        .relative()
        .flex_none()
        .w(px(34.))
        .h(px(20.))
        .rounded(px(10.))
        .cursor_pointer()
        .bg(if on { theme.accent } else { theme.switch_off })
        .child(
            div()
                .absolute()
                .top(px(2.))
                .size(px(16.))
                .rounded_full()
                .bg(gpui::white())
                .shadow(vec![gpui::BoxShadow {
                    color: gpui::hsla(0., 0., 0., 0.35),
                    offset: gpui::point(px(0.), px(1.)),
                    blur_radius: px(3.),
                    spread_radius: px(0.),
                    inset: false,
                }])
                .with_spring((id, "knob"), indicator_spring(theme, if on { px(16.) } else { px(2.) }), |el, x| {
                    el.left(x)
                }),
        )
}

/// A control-center tile: round icon, title and a line of detail.
pub fn tile(
    id: impl Into<ElementId>,
    glyph: Icon,
    title: impl Into<SharedString>,
    detail: impl Into<SharedString>,
    on: bool,
    theme: &Theme,
) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .gap(px(10.))
        .p(px(10.))
        .min_w_0()
        .rounded(RADIUS_TILE)
        .bg(theme.fill)
        .border_1()
        .border_color(theme.hair)
        .cursor_pointer()
        .hover(|s| s.bg(theme.fill_2))
        .child(
            div()
                .size(px(32.))
                .flex_none()
                .rounded_full()
                .flex()
                .items_center()
                .justify_center()
                .bg(if on { theme.accent } else { theme.fill_3 })
                .text_color(if on { theme.accent_ink } else { theme.ink })
                .when(on, |el| el.shadow(theme.accent_glow()))
                .child(icon(glyph)),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .min_w_0()
                .child(div().text_size(px(13.)).font_weight(FontWeight::SEMIBOLD).child(title.into()))
                .child(div().text_size(px(11.5)).text_color(theme.ink_2).truncate().child(detail.into())),
        )
}

/// A list row: 40 px tall, rounded, hover fill when interactive.
pub fn row(id: impl Into<ElementId>, theme: &Theme) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .gap(px(11.))
        .w_full()
        .min_h(px(40.))
        .px(px(8.))
        .py(px(6.))
        .rounded(RADIUS_ROW)
        .text_size(px(13.))
        .hover(|s| s.bg(theme.fill_2))
}

/// The two-line text block inside a row.
pub fn row_text(title: impl Into<SharedString>, sub: Option<SharedString>, theme: &Theme) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_w_0()
        .child(div().font_weight(FontWeight::MEDIUM).truncate().child(title.into()))
        .when_some(sub, |el, sub| el.child(div().text_size(px(12.)).text_color(theme.ink_2).truncate().child(sub)))
}

/// The 28 px rounded-square icon at the start of a row.
pub fn row_icon(glyph: Icon, on: bool, theme: &Theme) -> impl IntoElement {
    div()
        .size(px(28.))
        .flex_none()
        .rounded(px(8.))
        .flex()
        .items_center()
        .justify_center()
        .bg(if on { theme.accent } else { theme.fill_2 })
        .text_color(if on { theme.accent_ink } else { theme.ink })
        .child(icon(glyph))
}

/// The checkmark on a selected row. It springs in.
pub fn check(id: impl Into<ElementId>, on: bool, theme: &Theme) -> impl IntoElement {
    div().size(px(16.)).flex_none().text_color(theme.accent).child(icon(Icon::Check).with_spring(
        id,
        indicator_spring(theme, if on { 1.0f32 } else { 0.0 }),
        |el, t| {
            let t = t.clamp(0., 1.2);
            el.opacity(t.min(1.)).size(px(16. * (0.5 + 0.5 * t)))
        },
    ))
}

/// Uppercase section label, like "OTHER NETWORKS".
pub fn label(text: impl Into<SharedString>, theme: &Theme) -> impl IntoElement {
    div()
        .px(px(6.))
        .pt(px(12.))
        .pb(px(6.))
        .text_size(px(10.5))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme.ink_3)
        .child(text.into().to_uppercase())
}

/// A popover's title row.
pub fn header(title: impl Into<SharedString>, trailing: impl IntoElement) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap(px(12.))
        .px(px(4.))
        .pt(px(2.))
        .pb(px(10.))
        .child(div().text_size(px(13.)).font_weight(FontWeight::SEMIBOLD).child(title.into()))
        .child(trailing)
}

pub fn muted(text: impl Into<SharedString>, theme: &Theme) -> Div {
    div().text_size(px(12.5)).text_color(theme.ink_2).child(text.into())
}

/// A quiet text button, like "Network settings…".
pub fn link(id: impl Into<ElementId>, text: impl Into<SharedString>, theme: &Theme) -> Stateful<Div> {
    div()
        .id(id)
        .px(px(8.))
        .py(px(6.))
        .rounded(px(8.))
        .text_size(px(12.5))
        .text_color(theme.ink_2)
        .cursor_pointer()
        .hover(|s| s.bg(theme.fill_2).text_color(theme.ink))
        .child(text.into())
}

/// The footer row: a note on the left, an action on the right.
pub fn footer(note: impl Into<SharedString>, action: impl IntoElement, theme: &Theme) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap(px(8.))
        .pt(px(6.))
        .px(px(2.))
        .child(div().px(px(6.)).text_size(px(11.5)).text_color(theme.ink_3).child(note.into()))
        .child(action)
}

pub fn separator(theme: &Theme) -> impl IntoElement {
    div().h(px(1.)).mx(px(4.)).my(px(8.)).bg(theme.hair)
}

/// A centered message for empty states.
pub fn empty(lines: &[&str], theme: &Theme) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .items_center()
        .py(px(18.))
        .px(px(10.))
        .text_size(px(12.5))
        .text_color(theme.ink_2)
        .children(lines.iter().map(|l| SharedString::from(l.to_string())))
}

/// A round media button. `main` is the large filled play button.
pub fn media_button(id: impl Into<ElementId>, glyph: Icon, main: bool, theme: &Theme) -> Stateful<Div> {
    let size = if main { 46. } else { 38. };
    div()
        .id(id)
        .size(px(size))
        .flex_none()
        .rounded_full()
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .when(main, |el| el.bg(theme.ink).text_color(theme.on_ink))
        .when(!main, |el| el.hover(|s| s.bg(theme.fill_2)))
        .child(icon_sized(glyph, if main { 20. } else { 18. }))
}

/// A segmented picker with a sprung thumb. `on_select` gets the chosen index.
pub fn segmented(
    id: impl Into<ElementId>,
    options: &[&str],
    selected: usize,
    theme: &Theme,
    on_select: impl Fn(usize, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let id: ElementId = id.into();
    let count = options.len().max(1);
    let on_select = Rc::new(on_select);
    div()
        .id(id.clone())
        .relative()
        .flex()
        .p(px(3.))
        .rounded(px(11.))
        .bg(theme.fill)
        .border_1()
        .border_color(theme.hair)
        .child(
            div()
                .absolute()
                .top(px(3.))
                .bottom(px(3.))
                .w(relative((1. / count as f32) * 0.985))
                .rounded(px(8.))
                .bg(theme.fill_3)
                .with_spring((id.clone(), "thumb"), indicator_spring(theme, selected as f32), move |el, i| {
                    el.left(relative(0.008 + i / count as f32 * 0.985))
                }),
        )
        .children(options.iter().enumerate().map(|(i, name)| {
            let on_select = on_select.clone();
            div()
                .id(ElementId::NamedChild(std::sync::Arc::new(id.clone()), i.to_string().into()))
                .relative()
                .flex_1()
                .h(px(28.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(8.))
                .text_size(px(12.))
                .font_weight(FontWeight::MEDIUM)
                .cursor_pointer()
                .text_color(if i == selected { theme.ink } else { theme.ink_2 })
                .child(SharedString::from(name.to_string()))
                .on_click(move |_, window, cx| on_select(i, window, cx))
        }))
}

/// Drag state for a slider, owned by the view that shows it.
#[derive(Clone, Default)]
pub struct SliderState {
    bounds: Rc<Cell<Bounds<Pixels>>>,
    dragging: Rc<Cell<bool>>,
}

impl SliderState {
    pub fn is_dragging(&self) -> bool {
        self.dragging.get()
    }

    fn fraction(&self, x: Pixels) -> f32 {
        let b = self.bounds.get();
        let width = f32::from(b.size.width).max(1.0);
        ((f32::from(x - b.origin.x)) / width).clamp(0.0, 1.0)
    }
}

/// A 28 px pill slider with an icon in the fill. `value` and the callback use 0..=1.
pub fn slider(
    id: impl Into<ElementId>,
    value: f32,
    glyph: Option<Icon>,
    state: &SliderState,
    theme: &Theme,
    on_change: impl Fn(f32, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let on_change = Rc::new(on_change);
    let (bounds_state, down_state, drag_state) = (state.clone(), state.clone(), state.clone());
    let (drag_change, down_change, scroll_change) = (on_change.clone(), on_change.clone(), on_change);
    let value = value.clamp(0.0, 1.0);
    div()
        .id(id)
        .relative()
        .h(px(28.))
        .w_full()
        .rounded(px(14.))
        .bg(theme.slider_track)
        .border_1()
        .border_color(theme.hair)
        .overflow_hidden()
        .cursor_pointer()
        .child(
            div()
                .absolute()
                .top_0()
                .bottom_0()
                .left_0()
                .min_w(px(28.))
                .w(relative(value))
                .rounded(px(14.))
                .bg(theme.slider_fill),
        )
        .when_some(glyph, |el, glyph| {
            el.child(div().absolute().left(px(6.)).top(px(5.)).text_color(theme.slider_icon).child(icon(glyph)))
        })
        .child(
            canvas(
                move |bounds, _, _| bounds_state.bounds.set(bounds),
                move |_, _, window, _| {
                    // While dragging, follow the pointer even outside the slider.
                    if !drag_state.dragging.get() {
                        return;
                    }
                    let (move_state, up_state) = (drag_state.clone(), drag_state.clone());
                    let change = drag_change.clone();
                    window.on_mouse_event(move |e: &MouseMoveEvent, phase, window, cx| {
                        if phase == gpui::DispatchPhase::Bubble && move_state.dragging.get() {
                            if e.pressed_button == Some(MouseButton::Left) {
                                change(move_state.fraction(e.position.x), window, cx);
                            } else {
                                move_state.dragging.set(false);
                            }
                        }
                    });
                    window.on_mouse_event(move |_: &MouseUpEvent, phase, _, _| {
                        if phase == gpui::DispatchPhase::Bubble {
                            up_state.dragging.set(false);
                        }
                    });
                },
            )
            .absolute()
            .size_full(),
        )
        .on_mouse_down(MouseButton::Left, move |e, window, cx| {
            down_state.dragging.set(true);
            down_change(down_state.fraction(e.position.x), window, cx);
            window.refresh();
        })
        .on_scroll_wheel(move |e, window, cx| {
            let dy = match e.delta {
                gpui::ScrollDelta::Lines(p) => p.y * 0.05,
                gpui::ScrollDelta::Pixels(p) => f32::from(p.y) / 400.0,
            };
            scroll_change((value + dy).clamp(0.0, 1.0), window, cx);
        })
}

/// A small pill button, like a player chooser.
pub fn pill(id: impl Into<ElementId>, text: impl Into<SharedString>, on: bool, theme: &Theme) -> Stateful<Div> {
    div()
        .id(id)
        .h(px(26.))
        .px(px(10.))
        .flex()
        .items_center()
        .rounded(px(13.))
        .text_size(px(12.))
        .cursor_pointer()
        .border_1()
        .when(on, |el| el.bg(theme.accent_soft).text_color(theme.ink).border_color(theme.accent))
        .when(!on, |el| el.text_color(theme.ink_2).border_color(theme.hair_2))
        .child(text.into())
}

/// A footer button in the control center: icon plus label.
pub fn footer_button(
    id: impl Into<ElementId>,
    glyph: Icon,
    text: impl Into<SharedString>,
    theme: &Theme,
) -> Stateful<Div> {
    div()
        .id(id)
        .h(px(34.))
        .flex_1()
        .flex()
        .items_center()
        .justify_center()
        .gap(px(7.))
        .rounded(px(10.))
        .text_size(px(12.))
        .font_weight(FontWeight::MEDIUM)
        .text_color(theme.ink_2)
        .bg(theme.fill)
        .border_1()
        .border_color(theme.hair)
        .cursor_pointer()
        .hover(|s| s.bg(theme.fill_2).text_color(theme.ink))
        .child(icon_sized(glyph, 14.))
        .child(text.into())
}
