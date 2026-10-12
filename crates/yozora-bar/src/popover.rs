//! A spring-animated popover in its own `xdg_popup` surface.
//!
//! The surface is a fixed size with transparent margins for the shadow. Only the card itself
//! takes input. GPUI has no transform for a whole element tree, so the open animation is a fade
//! plus a short drop from above rather than the design's scale-in.

use std::time::{Duration, Instant};

use gpui::{
    AnyWindowHandle, App, Bounds, BoxShadow, Context, Entity, FontWeight, IntoElement,
    ParentElement, Pixels, Render, Size, Styled, Window, WindowBackgroundAppearance, WindowBounds,
    WindowKind, WindowOptions, div, point, popup::*, prelude::*, px, size,
};

use crate::{
    frame_stats::FrameStats,
    spring::{Spring, SpringConfig},
    theme,
};

const CARD: Size<Pixels> = size(px(344.), px(268.));
/// Room around the card for its shadow: top, sides, bottom.
const MARGIN_TOP: Pixels = px(8.);
const MARGIN_SIDE: Pixels = px(28.);
const MARGIN_BOTTOM: Pixels = px(48.);
/// How far the card drops while it fades in.
const DROP: f32 = 8.;
/// Reduced motion: a plain fade of this length.
const FADE: Duration = Duration::from_millis(120);

pub struct Popover {
    spring: Spring,
    reduce_motion: bool,
    stats: FrameStats,
    input_region_set: bool,
}

impl Popover {
    /// Opens a popover below `anchor`, a rectangle in `parent`'s coordinates. Must be called
    /// while the mouse button that triggered it is still down, or the grab is refused.
    pub fn open(
        parent: AnyWindowHandle,
        anchor: Bounds<Pixels>,
        cx: &mut App,
    ) -> anyhow::Result<Entity<Self>> {
        let window_size = size(
            CARD.width + MARGIN_SIDE * 2.,
            CARD.height + MARGIN_TOP + MARGIN_BOTTOM,
        );
        let mut entity = None;
        cx.open_window(
            WindowOptions {
                titlebar: None,
                focus: true,
                window_bounds: Some(WindowBounds::Windowed(Bounds {
                    origin: point(px(0.), px(0.)),
                    size: window_size,
                })),
                window_background: WindowBackgroundAppearance::Transparent,
                inactive_frame_interval: None,
                kind: WindowKind::AnchoredPopup(PopupOptions {
                    parent,
                    anchor_rect: anchor,
                    anchor: PopupAnchor::BottomRight,
                    gravity: PopupGravity::BottomLeft,
                    constraint_adjustment: PopupConstraintAdjustment::SLIDE_X
                        | PopupConstraintAdjustment::FLIP_X,
                    // Line the card's right edge up with the anchor despite the shadow margin.
                    offset: point(MARGIN_SIDE, px(0.)),
                    grab: true,
                }),
                ..Default::default()
            },
            |_, cx| {
                let popover = cx.new(|cx| {
                    let mut spring = Spring::new(0.0, SpringConfig::POPOVER);
                    spring.set_target(1.0);
                    Self {
                        spring,
                        reduce_motion: cx.reduce_motion(),
                        stats: FrameStats::default(),
                        input_region_set: false,
                    }
                });
                entity = Some(popover.clone());
                popover
            },
        )?;
        Ok(entity.expect("open_window builds the root view"))
    }

    /// Starts the closing animation. The window removes itself once it has faded out.
    pub fn close(&mut self, cx: &mut Context<Self>) {
        self.spring.set_target(0.0);
        cx.notify();
    }

    pub fn is_closing(&self) -> bool {
        self.spring.target() == 0.0
    }

    /// Advances the animation and returns the open fraction for this frame.
    fn advance(&mut self, window: &mut Window, cx: &mut Context<Self>) -> f32 {
        let now = Instant::now();
        let dt = self.stats.frame(now);

        let settled = if self.reduce_motion {
            // Linear fade toward the target.
            let step = dt.as_secs_f32() / FADE.as_secs_f32();
            let value = self.spring.value();
            let next = if self.spring.target() > value {
                (value + step).min(1.0)
            } else {
                (value - step).max(0.0)
            };
            self.spring = {
                let mut s = Spring::new(next, SpringConfig::POPOVER);
                s.set_target(self.spring.target());
                s
            };
            next == self.spring.target()
        } else {
            self.spring.step(dt)
        };

        if settled {
            let label = if self.is_closing() {
                "popover close"
            } else {
                "popover open"
            };
            self.stats.finish(label, window.refresh_interval(cx));
            if self.is_closing() {
                window.remove_window();
            }
        } else {
            window.request_animation_frame();
        }
        self.spring.value()
    }
}

impl Render for Popover {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.input_region_set {
            window.set_input_region(Some(&[Bounds {
                origin: point(MARGIN_SIDE, MARGIN_TOP),
                size: CARD,
            }]));
            self.input_region_set = true;
        }

        let t = self.advance(window, cx);
        let offset = if self.reduce_motion {
            0.
        } else {
            (1. - t) * -DROP
        };

        div().size_full().child(
            div()
                .absolute()
                .left(MARGIN_SIDE)
                .top(MARGIN_TOP + px(offset))
                .w(CARD.width)
                .h(CARD.height)
                .opacity(t.clamp(0., 1.))
                .rounded(theme::POPOVER_RADIUS)
                .bg(theme::glass_pop())
                .border_1()
                .border_color(theme::hairline())
                .shadow(vec![BoxShadow {
                    color: theme::shadow(),
                    offset: point(px(0.), px(20.)),
                    blur_radius: px(36.),
                    spread_radius: px(-14.),
                    inset: false,
                }])
                .p(px(16.))
                .flex()
                .flex_col()
                .gap(px(12.))
                .text_color(theme::ink())
                .child(
                    div()
                        .flex()
                        .justify_between()
                        .items_center()
                        .child(
                            div()
                                .text_size(px(15.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .child("Control center"),
                        )
                        .child(
                            div()
                                .text_size(px(12.))
                                .text_color(theme::ink_3())
                                .child("Spike"),
                        ),
                )
                .child(
                    div()
                        .grid()
                        .grid_cols(2)
                        .gap(px(8.))
                        .child(tile("Wi-Fi", "Home", true))
                        .child(tile("Bluetooth", "Off", false))
                        .child(tile("Do not disturb", "Off", false))
                        .child(tile("Night light", "Until 07:00", true)),
                )
                .child(slider("Volume", 0.62))
                .child(slider("Brightness", 0.8)),
        )
    }
}

fn tile(name: &'static str, detail: &'static str, on: bool) -> impl IntoElement {
    div()
        .h(px(56.))
        .px(px(12.))
        .rounded(px(12.))
        .flex()
        .flex_col()
        .justify_center()
        .gap(px(2.))
        .bg(if on { theme::accent() } else { theme::fill_2() })
        .text_color(if on {
            theme::accent_ink()
        } else {
            theme::ink()
        })
        .child(
            div()
                .text_size(px(13.))
                .font_weight(FontWeight::MEDIUM)
                .child(name),
        )
        .child(div().text_size(px(11.5)).opacity(0.75).child(detail))
}

fn slider(label: &'static str, value: f32) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap(px(6.))
        .child(
            div()
                .text_size(px(12.))
                .text_color(theme::ink_2())
                .child(label),
        )
        .child(
            div().h(px(6.)).rounded(px(3.)).bg(theme::fill_3()).child(
                div()
                    .h_full()
                    .w(gpui::relative(value))
                    .rounded(px(3.))
                    .bg(theme::ink()),
            ),
        )
}
