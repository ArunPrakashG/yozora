//! A popover in its own `xdg_popup` surface, with the design's spring open and staggered rows.
//!
//! The surface is a fixed size with transparent margins for the shadow; only the card takes
//! input. GPUI has no transform for a whole element tree, so the open animation is a fade plus
//! a short drop rather than the design's scale-in.

use std::{
    cell::Cell,
    rc::Rc,
    time::{Duration, Instant},
};

use gpui::{
    AnyElement, AnyWindowHandle, App, Bounds, Context, Entity, FocusHandle, InteractiveElement, IntoElement,
    KeyDownEvent, ParentElement, Pixels, Render, StatefulInteractiveElement, Styled, Subscription, WeakEntity, Window,
    WindowBackgroundAppearance, WindowBounds, WindowKind, WindowOptions, canvas, div, point, popup::*, prelude::*, px,
    size,
};
use yozora_ui::{
    Theme,
    frame_stats::FrameStats,
    spring::{Spring, SpringConfig, step_response},
    theme::RADIUS_POPOVER,
};

use crate::{
    bar::Bar,
    modules::{self, Pop, PopUi},
    services::Services,
};

/// Room around the card for its shadow.
const MARGIN_TOP: Pixels = px(8.);
const MARGIN_SIDE: Pixels = px(28.);
const MARGIN_BOTTOM: Pixels = px(48.);
/// The tallest a card gets before it scrolls.
const MAX_HEIGHT: Pixels = px(680.);
/// How far the card drops while it fades in, and how far each row rises.
const DROP: f32 = 8.;
const RISE: f32 = 7.;
/// When each row starts rising, from the design.
const STAGGER_MS: [f32; 8] = [0., 22., 44., 66., 88., 110., 128., 144.];
const FADE_IN: Duration = Duration::from_millis(160);
const FADE_OUT: Duration = Duration::from_millis(130);
const SLIDE_OUT: Duration = Duration::from_millis(160);
/// Reduced motion: a plain fade.
const REDUCED: Duration = Duration::from_millis(120);

pub struct Popover {
    pub pop: Pop,
    pub bar: WeakEntity<Bar>,
    pub ui: PopUi,
    window: AnyWindowHandle,
    opened: Instant,
    closing: Option<Instant>,
    /// The open animation's drop, as a spring so it shares the design's feel.
    drop: Spring,
    open_settled: bool,
    stats: FrameStats,
    card: Rc<Cell<Bounds<Pixels>>>,
    input_region: Option<Bounds<Pixels>>,
    focus: FocusHandle,
    /// The focused window when an unfocusable popover opened; it closes when that changes.
    focused_window: Option<(String, String)>,
    grab: bool,
    tick: Option<gpui::Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl Popover {
    /// Opens a popover under `anchor`, a rectangle in `parent`'s coordinates. With `grab`, it
    /// must be called while the mouse button that triggered it is still down.
    #[allow(clippy::too_many_arguments)]
    pub fn open(
        parent: AnyWindowHandle,
        anchor: Bounds<Pixels>,
        pop: Pop,
        monitor: String,
        left: bool,
        grab: bool,
        bar: WeakEntity<Bar>,
        cx: &mut App,
    ) -> anyhow::Result<Entity<Self>> {
        let width = pop.width();
        let window_size = size(width + MARGIN_SIDE * 2., MAX_HEIGHT + MARGIN_TOP + MARGIN_BOTTOM);
        let (anchor_edge, gravity, offset) = if left {
            (PopupAnchor::BottomLeft, PopupGravity::BottomRight, point(-MARGIN_SIDE, px(6.) - MARGIN_TOP))
        } else {
            (PopupAnchor::BottomRight, PopupGravity::BottomLeft, point(MARGIN_SIDE + px(2.), px(6.) - MARGIN_TOP))
        };
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
                    anchor: anchor_edge,
                    gravity,
                    constraint_adjustment: PopupConstraintAdjustment::SLIDE_X | PopupConstraintAdjustment::FLIP_X,
                    offset,
                    grab,
                }),
                ..Default::default()
            },
            |window, cx| {
                let handle = window.window_handle();
                let view = cx.new(|cx| {
                    let mut drop = Spring::new(0.0, SpringConfig::POPOVER);
                    drop.set_target(1.0);
                    let focus = cx.focus_handle();
                    tracing::debug!("opening {pop:?} on {monitor}");
                    let mut popover = Self {
                        pop,
                        bar,
                        ui: PopUi::default(),
                        window: handle,
                        opened: Instant::now(),
                        closing: None,
                        drop,
                        open_settled: false,
                        stats: FrameStats::default(),
                        card: Rc::default(),
                        input_region: None,
                        focus,
                        focused_window: None,
                        grab,
                        tick: None,
                        _subscriptions: Vec::new(),
                    };
                    popover.observe(cx);
                    popover.on_open(cx);
                    popover
                });
                window.focus(&view.read(cx).focus.clone(), cx);
                entity = Some(view.clone());
                view
            },
        )?;
        Ok(entity.expect("open_window builds the root view"))
    }

    fn observe(&mut self, cx: &mut Context<Self>) {
        let mut subs = Vec::new();
        macro_rules! watch {
            ($($field:ident),*) => {$(
                if let Some(e) = Services::get(cx).$field.clone() {
                    subs.push(cx.observe(&e, |_, _, cx| cx.notify()));
                }
            )*};
        }
        watch!(audio, bluetooth, media, network, notifications, power, tray);
        if let Some(hypr) = Services::get(cx).hyprland.clone() {
            // Without a grab nothing tells us about clicks elsewhere. Close when focus moves.
            subs.push(cx.observe(&hypr, |p, hypr, cx| {
                if p.grab {
                    return;
                }
                let now = hypr.read(cx).state.window.as_ref().map(|w| (w.class.clone(), w.title.clone()));
                if now != p.focused_window {
                    p.close(cx);
                }
            }));
            if !self.grab {
                self.focused_window = hypr.read(cx).state.window.as_ref().map(|w| (w.class.clone(), w.title.clone()));
            }
        }
        self._subscriptions = subs;
    }

    /// Asks services for fresh data the popover shows.
    fn on_open(&mut self, cx: &mut Context<Self>) {
        let services = Services::get(cx);
        match &self.pop {
            Pop::Network => {
                if let Some(n) = &services.network {
                    n.read(cx).send(crate::services::network::Cmd::Scan);
                }
            }
            Pop::Notifications => {
                if let Some(n) = &services.notifications {
                    n.read(cx).send(crate::services::notifications::Cmd::MarkRead);
                }
            }
            Pop::Tray(key) => {
                if let Some(t) = &services.tray {
                    t.read(cx).send(crate::services::tray::Cmd::LoadMenu(key.clone()));
                }
            }
            _ => {}
        }
    }

    /// Starts the close animation. The window removes itself when it finishes.
    pub fn close(&mut self, cx: &mut Context<Self>) {
        if self.closing.is_none() {
            self.closing = Some(Instant::now());
            if !self.open_settled {
                // Interrupted mid-open: the open timing is not a clean sample.
                self.stats = FrameStats::default();
                self.open_settled = true;
            }
            cx.notify();
        }
    }

    pub fn is_closing(&self) -> bool {
        self.closing.is_some()
    }

    pub fn window(&self) -> AnyWindowHandle {
        self.window
    }

    /// Removes a popover window immediately, without animating. Call outside the popover's
    /// own update.
    pub fn remove(window: AnyWindowHandle, cx: &mut App) {
        let _ = window.update(cx, |_, window, _| window.remove_window());
    }

    fn settled(&mut self, label: &'static str, window: &mut Window, cx: &mut Context<Self>) {
        let mut stats = std::mem::take(&mut self.stats);
        let refresh = window.refresh_interval(cx);
        let bar = self.bar.clone();
        // The bar may close or reopen popovers in response, so not while this one is leased.
        cx.defer(move |cx| {
            let _ = bar.update(cx, |bar, cx| bar.popover_settled(label, &mut stats, refresh, cx));
        });
    }

    /// Opacity, card offset and elapsed open time for this frame.
    fn advance(&mut self, theme: &Theme, window: &mut Window, cx: &mut Context<Self>) -> (f32, f32, f32) {
        let now = Instant::now();
        let dt = self.stats.frame(now);
        let reduced = theme.motion.reduced;
        if let Some(closing) = self.closing {
            let t = now - closing;
            let (opacity, offset, done) = if reduced {
                let f = 1.0 - (t.as_secs_f32() / REDUCED.as_secs_f32()).min(1.0);
                (f, 0.0, t >= REDUCED)
            } else {
                // Ease-in, so the card leaves quickly at the end.
                let o = (t.as_secs_f32() / FADE_OUT.as_secs_f32()).min(1.0);
                let s = (t.as_secs_f32() / SLIDE_OUT.as_secs_f32()).min(1.0);
                (1.0 - o * o, -DROP * s * s, t >= SLIDE_OUT)
            };
            if done {
                self.settled("popover close", window, cx);
                window.remove_window();
            } else {
                window.request_animation_frame();
            }
            return (opacity, offset, f32::MAX);
        }

        let elapsed = (now - self.opened).as_secs_f32();
        let (opacity, offset, done) = if reduced {
            let f = (elapsed / REDUCED.as_secs_f32()).min(1.0);
            (f, 0.0, f >= 1.0)
        } else {
            let settled = self.drop.step(dt);
            let o = (elapsed / FADE_IN.as_secs_f32()).min(1.0);
            let rows_done = elapsed * 1000.0 > STAGGER_MS[STAGGER_MS.len() - 1] + 500.0;
            (1.0 - (1.0 - o).powi(3), (self.drop.value() - 1.0) * DROP, settled && o >= 1.0 && rows_done)
        };
        if !self.open_settled {
            if done {
                self.open_settled = true;
                self.settled("popover open", window, cx);
            } else {
                window.request_animation_frame();
            }
        } else {
            self.stats.pause();
        }
        (opacity, offset, if reduced { f32::MAX } else { elapsed })
    }
}

/// Wraps a popover's rows so each rises in after its stagger delay.
fn stagger(rows: Vec<AnyElement>, elapsed: f32) -> Vec<AnyElement> {
    rows.into_iter()
        .enumerate()
        .map(|(i, row)| {
            if elapsed == f32::MAX {
                return row;
            }
            let delay = STAGGER_MS[i.min(STAGGER_MS.len() - 1)] / 1000.0;
            let t = step_response(SpringConfig::POPOVER, elapsed - delay);
            let fade = ((elapsed - delay) / 0.18).clamp(0.0, 1.0);
            div().relative().top(px((1.0 - t) * RISE)).opacity(fade).child(row).into_any_element()
        })
        .collect()
}

impl Render for Popover {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::global(cx).clone();
        // Input goes only to the card, as measured last frame.
        let card = self.card.get();
        if card.size.width > px(0.) && self.input_region != Some(card) {
            window.set_input_region(Some(&[card]));
            self.input_region = Some(card);
        }
        let (opacity, offset, elapsed) = self.advance(&theme, window, cx);
        let rows = match self.pop.clone() {
            Pop::Menu => modules::launcher::popover(self, &theme, window, cx),
            Pop::Media => modules::media::popover(self, &theme, cx),
            Pop::Network => modules::network::popover(self, &theme, cx),
            Pop::Sound => modules::sound::popover(self, &theme, cx),
            Pop::Devices => modules::devices::popover(self, &theme, cx),
            Pop::Battery => modules::devices::battery_popover(self, &theme, cx),
            Pop::Notifications => modules::notifications::popover(self, &theme, cx),
            Pop::Calendar => modules::calendar::popover(self, &theme, cx),
            Pop::ControlCenter => modules::control_center::popover(self, &theme, window, cx),
            Pop::Tray(key) => modules::tray::popover(self, &key, &theme, cx),
        };
        let card_bounds = self.card.clone();
        // Keep the media position ticking.
        if self.tick.is_none() && matches!(self.pop, Pop::Media | Pop::ControlCenter) && modules::media::playing(cx) {
            self.tick = Some(cx.spawn(async move |this, cx| {
                cx.background_executor().timer(Duration::from_millis(500)).await;
                let _ = this.update(cx, |p, cx| {
                    p.tick = None;
                    cx.notify();
                });
            }));
        }

        div()
            .size_full()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|p, e: &KeyDownEvent, _, cx| {
                if e.keystroke.key == "escape" {
                    p.close(cx);
                }
            }))
            .child(
                div()
                    .id("card")
                    .absolute()
                    .left(MARGIN_SIDE)
                    .top(MARGIN_TOP + px(offset))
                    .w(self.pop.width())
                    .max_h(MAX_HEIGHT)
                    .overflow_y_scroll()
                    .opacity(opacity.clamp(0., 1.))
                    .rounded(RADIUS_POPOVER)
                    .bg(theme.glass_pop)
                    .border_1()
                    .border_color(theme.hair_2)
                    .shadow(theme.popover_shadow())
                    .p(px(10.))
                    .text_color(theme.ink)
                    .text_size(px(13.))
                    .font_family(theme.font.clone())
                    .flex()
                    .flex_col()
                    .child(
                        canvas(move |b, _, _| card_bounds.set(b), |_, _, _, _| {})
                            .absolute()
                            .top_0()
                            .left_0()
                            .size_full(),
                    )
                    .children(stagger(rows, elapsed)),
            )
    }
}
