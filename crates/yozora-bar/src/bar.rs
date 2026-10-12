//! One bar window: a layer-shell surface across the top of a monitor.

use std::time::{Duration, Instant};

use gpui::{
    App, Bounds, Context, FontWeight, IntoElement, MouseButton, MouseDownEvent, ParentElement,
    Render, Styled, Subscription, WeakEntity, Window, WindowBackgroundAppearance, WindowBounds,
    WindowKind, WindowOptions, div, layer_shell::*, point, prelude::*, px, size,
};

use crate::{
    frame_stats::FrameStats,
    popover::Popover,
    spring::{Spring, SpringConfig},
    theme,
};

const WORKSPACES: usize = 5;
const WS_WIDTH: f32 = 28.;
const WS_GAP: f32 = 2.;

pub struct Bar {
    output: String,
    active_workspace: usize,
    indicator: Spring,
    indicator_stats: FrameStats,
    /// Weak, so the popover is released when its window closes, including when the compositor
    /// dismisses it.
    popover: Option<(WeakEntity<Popover>, Subscription)>,
    first_frame_logged: bool,
}

impl Bar {
    pub fn open(
        display: &dyn gpui::PlatformDisplay,
        label: String,
        cx: &mut App,
    ) -> anyhow::Result<()> {
        cx.open_window(
            WindowOptions {
                titlebar: None,
                focus: false,
                window_bounds: Some(WindowBounds::Windowed(Bounds {
                    origin: point(px(0.), px(0.)),
                    // Zero width stretches between the left and right anchors. GPUI's display bounds
                    // ignore output rotation, so they can't be trusted for this.
                    size: size(px(0.), theme::BAR_HEIGHT),
                })),
                app_id: Some("yozora-bar".into()),
                window_background: WindowBackgroundAppearance::Transparent,
                display_id: Some(display.id()),
                // A bar never has keyboard focus. Without this GPUI caps it at 30 fps.
                inactive_frame_interval: None,
                kind: WindowKind::LayerShell(LayerShellOptions {
                    namespace: "yozora-bar".into(),
                    layer: Layer::Top,
                    anchor: Anchor::TOP | Anchor::LEFT | Anchor::RIGHT,
                    exclusive_zone: Some(theme::BAR_HEIGHT),
                    keyboard_interactivity: KeyboardInteractivity::None,
                    ..Default::default()
                }),
                ..Default::default()
            },
            |_, cx| {
                cx.new(|cx| {
                    spawn_clock(cx);
                    Self {
                        output: label,
                        active_workspace: 0,
                        indicator: Spring::new(0.0, SpringConfig::INDICATOR),
                        indicator_stats: FrameStats::default(),
                        popover: None,
                        first_frame_logged: false,
                    }
                })
            },
        )?;
        Ok(())
    }

    fn toggle_popover(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        tracing::debug!(output = %self.output, open = self.popover.is_some(), "control center pressed");
        if let Some(popover) = self.popover.as_ref().and_then(|(p, _)| p.upgrade())
            && !popover.read(cx).is_closing()
        {
            popover.update(cx, |popover, cx| popover.close(cx));
            return;
        }
        // Anchor to a small rectangle under the pointer, spanning the bar's height.
        let anchor = Bounds {
            origin: point(event.position.x - px(1.), px(0.)),
            size: size(px(2.), theme::BAR_HEIGHT),
        };
        match Popover::open(window.window_handle(), anchor, cx) {
            Ok(popover) => {
                let released = cx.observe_release(&popover, |bar: &mut Self, _, cx| {
                    tracing::debug!(output = %bar.output, "popover released");
                    bar.popover = None;
                    cx.notify();
                });
                self.popover = Some((popover.downgrade(), released));
            }
            Err(error) => tracing::error!("could not open popover on {}: {error:#}", self.output),
        }
    }

    fn select_workspace(&mut self, index: usize, cx: &mut Context<Self>) {
        tracing::debug!(output = %self.output, index, "workspace clicked");
        self.active_workspace = index;
        self.indicator.set_target(index as f32);
        cx.notify();
    }

    fn render_workspaces(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let dt = self.indicator_stats.frame(Instant::now());
        if self.indicator.step(dt) {
            let label = format!("workspace indicator on {}", self.output);
            self.indicator_stats
                .finish(&label, window.refresh_interval(cx));
        } else {
            window.request_animation_frame();
        }
        let x = 3. + self.indicator.value() * (WS_WIDTH + WS_GAP);

        div()
            .relative()
            .flex()
            .gap(px(WS_GAP))
            .p(px(3.))
            .ml(px(4.))
            .rounded(px(10.))
            .bg(theme::fill_2())
            .child(
                div()
                    .absolute()
                    .top(px(3.))
                    .left(px(x))
                    .w(px(WS_WIDTH))
                    .h(px(20.))
                    .rounded(px(7.))
                    .bg(theme::accent()),
            )
            .children((0..WORKSPACES).map(|i| {
                let active = i == self.active_workspace;
                div()
                    .id(("ws", i))
                    .w(px(WS_WIDTH))
                    .h(px(20.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(12.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(if active {
                        theme::accent_ink()
                    } else {
                        theme::ink_3()
                    })
                    .child((i + 1).to_string())
                    .on_click(cx.listener(move |bar, _, _, cx| bar.select_workspace(i, cx)))
            }))
    }
}

impl Render for Bar {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.first_frame_logged {
            self.first_frame_logged = true;
            if let Some(started) = crate::STARTED.get() {
                tracing::info!(
                    "first frame on {} after {:?}",
                    self.output,
                    started.elapsed()
                );
            }
        }
        let clock = chrono::Local::now().format("%a %-d %b   %H:%M").to_string();
        let popover_open = self
            .popover
            .as_ref()
            .and_then(|(p, _)| p.upgrade())
            .is_some_and(|p| !p.read(cx).is_closing());

        div()
            .size_full()
            .flex()
            .items_center()
            .justify_between()
            .px(px(6.))
            .bg(theme::glass_bar())
            .border_b_1()
            .border_color(theme::hairline())
            .text_color(theme::ink())
            .text_size(px(13.))
            .font_weight(FontWeight::MEDIUM)
            .child(self.render_workspaces(window, cx))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(2.))
                    .child(div().px(px(8.)).child(clock))
                    .child(
                        div()
                            .id("control-center")
                            .h(px(26.))
                            .px(px(10.))
                            .rounded(theme::ITEM_RADIUS)
                            .flex()
                            .items_center()
                            .when(popover_open, |el| el.bg(theme::accent_soft()))
                            .hover(|el| el.bg(theme::fill_2()))
                            .child("◐")
                            // Open from mouse-down: the popup grab needs the press still active.
                            .on_mouse_down(MouseButton::Left, cx.listener(Self::toggle_popover)),
                    ),
            )
    }
}

/// Redraws once at each minute boundary and never in between.
fn spawn_clock(cx: &mut Context<Bar>) {
    cx.spawn(async move |bar, cx| {
        loop {
            let now = chrono::Local::now();
            let into_minute = Duration::from_millis(
                u64::from(chrono::Timelike::second(&now)) * 1000
                    + u64::from(chrono::Timelike::nanosecond(&now) / 1_000_000),
            );
            let wait =
                Duration::from_secs(60).saturating_sub(into_minute) + Duration::from_millis(5);
            cx.background_executor().timer(wait).await;
            if bar.update(cx, |_, cx| cx.notify()).is_err() {
                break;
            }
        }
    })
    .detach();
}
