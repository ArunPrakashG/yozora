//! Toasts: one layer-shell surface on the overlay layer, top right of a monitor, holding a
//! stack of cards. The surface is transparent and takes input only on the cards.
//!
//! The window opens on the focused monitor when the first toast arrives and closes when the
//! last one has left, so an idle daemon has no surface at all.

use std::{
    cell::Cell,
    rc::Rc,
    time::{Duration, Instant},
};

use async_channel::Sender;
use gpui::{
    AnyElement, App, AppContext, Bounds, BoxShadow, Context, Entity, Focusable, FontWeight, Global, InteractiveElement,
    IntoElement, ObjectFit, ParentElement, Pixels, Render, Rgba, SharedString, StatefulInteractiveElement, Styled,
    StyledImage, Subscription, Task, Window, WindowBackgroundAppearance, WindowBounds, WindowHandle, WindowKind,
    WindowOptions, canvas, div, img, layer_shell::*, point, prelude::*, px, size,
};
use yozora_config::notify::NotifyConfig;
use yozora_ui::{
    Icon, IconElement, Theme, icon_sized,
    icon_theme::is_symbolic,
    icons::guess_app_icon,
    input::{InputEvent, TextInput},
    spring::{Spring, SpringConfig},
};

use crate::{
    server::Cmd,
    store::{Notification, Urgency},
};

const WIDTH: Pixels = px(380.);
/// Room left of the cards for their shadow.
const SHADOW: Pixels = px(28.);
/// From the bar and from the screen's right edge.
const EDGE: Pixels = px(12.);
/// What is left of a toast once Jev says it can wait.
const QUIET_LEFT: Duration = Duration::from_millis(1500);
const GAP: f32 = 12.;
/// How far a card slides while it arrives or leaves.
const SLIDE: f32 = 28.;
const FADE_IN: f32 = 0.18;
const FADE_OUT: Duration = Duration::from_millis(220);
/// After fading, the space a card held closes over this long.
const COLLAPSE: Duration = Duration::from_millis(200);
const REDUCED: Duration = Duration::from_millis(120);
/// How often the timer bars redraw while nothing else is moving.
const TIMER_FRAME: Duration = Duration::from_millis(33);

/// The open toast window, if any.
pub struct Stack {
    window: Option<WindowHandle<Toasts>>,
    config: NotifyConfig,
    cmds: Sender<Cmd>,
    /// The screen is being shared: private toasts hide what they say.
    sharing: bool,
}

impl Global for Stack {}

impl Stack {
    pub fn new(config: NotifyConfig, cmds: Sender<Cmd>) -> Self {
        Self { window: None, config, cmds, sharing: false }
    }

    fn view(cx: &App) -> Option<(WindowHandle<Toasts>, Entity<Toasts>)> {
        let handle = cx.global::<Stack>().window?;
        let view = handle.entity(cx).ok()?;
        Some((handle, view))
    }

    /// Shows `n`, opening the window on the right monitor if there is none.
    pub fn show(n: Notification, cx: &mut App) {
        if let Some((handle, _)) = Self::view(cx) {
            let _ = handle.update(cx, |toasts, window, cx| toasts.add(n, window, cx));
            return;
        }
        let started = Instant::now();
        match Toasts::open(n, cx) {
            Ok(handle) => {
                tracing::debug!("opened the toast window in {:?}", started.elapsed());
                cx.global_mut::<Stack>().window = Some(handle);
            }
            Err(e) => tracing::error!("could not open the toast window: {e:#}"),
        }
    }

    pub fn close(id: u32, cx: &mut App) {
        if let Some((_, view)) = Self::view(cx) {
            view.update(cx, |t, cx| t.leave(id, None, cx));
        }
    }

    pub fn close_all(cx: &mut App) {
        if let Some((_, view)) = Self::view(cx) {
            view.update(cx, |t, cx| {
                let ids: Vec<u32> = t.items.iter().map(|i| i.n.id).collect();
                for id in ids {
                    t.leave(id, None, cx);
                }
            });
        }
    }

    pub fn expire_all(cx: &mut App) {
        if let Some((_, view)) = Self::view(cx) {
            view.update(cx, |t, cx| {
                let ids: Vec<u32> =
                    t.items.iter().filter(|i| i.n.urgency != Urgency::Critical).map(|i| i.n.id).collect();
                for id in ids {
                    t.leave(id, Some(Cmd::Expired(id)), cx);
                }
            });
        }
    }

    /// A toast that can wait leaves in a moment, unless it is being read.
    pub fn shorten(id: u32, cx: &mut App) {
        if let Some((_, view)) = Self::view(cx) {
            view.update(cx, |t, cx| {
                if let Some(t) = t.items.iter_mut().find(|t| t.n.id == id && t.active()) {
                    t.left = t.left.map(|left| left.min(QUIET_LEFT));
                    cx.notify();
                }
            });
        }
    }

    /// New details for an open toast, such as Jev's answers. Opens nothing.
    pub fn refresh(n: Notification, cx: &mut App) {
        if let Some((_, view)) = Self::view(cx) {
            view.update(cx, |t, cx| {
                if let Some(t) = t.items.iter_mut().find(|t| t.n.id == n.id && t.active()) {
                    t.n = n;
                    cx.notify();
                }
            });
        }
    }

    pub fn set_sharing(on: bool, cx: &mut App) {
        cx.global_mut::<Stack>().sharing = on;
        if let Some((_, view)) = Self::view(cx) {
            view.update(cx, |t, cx| {
                t.sharing = on;
                cx.notify();
            });
        }
    }

    pub fn set_config(config: NotifyConfig, cx: &mut App) {
        cx.global_mut::<Stack>().config = config.clone();
        if let Some((_, view)) = Self::view(cx) {
            view.update(cx, |t, cx| {
                t.config = config;
                cx.notify();
            });
        }
    }

    fn send(cmd: Cmd, cx: &App) {
        let _ = cx.global::<Stack>().cmds.try_send(cmd);
    }
}

/// Opens a 1 px surface and closes it after its first frame. GPUI sets up the GPU with the
/// first window and keeps it, so this takes half a second off the first toast.
pub fn warm_up(cx: &mut App) {
    let opened = cx.open_window(
        WindowOptions {
            titlebar: None,
            focus: false,
            window_bounds: Some(WindowBounds::Windowed(Bounds {
                origin: point(px(0.), px(0.)),
                size: size(px(1.), px(1.)),
            })),
            app_id: Some("yozora-notify".into()),
            window_background: WindowBackgroundAppearance::Transparent,
            kind: WindowKind::LayerShell(LayerShellOptions {
                namespace: "yozora-notify-warmup".into(),
                layer: Layer::Background,
                anchor: Anchor::TOP | Anchor::LEFT,
                keyboard_interactivity: KeyboardInteractivity::None,
                ..Default::default()
            }),
            ..Default::default()
        },
        |window, cx| {
            window.set_input_region(Some(&[]));
            cx.new(|_| WarmUp)
        },
    );
    if let Err(e) = opened {
        tracing::debug!("warm-up window: {e:#}");
    }
}

struct WarmUp;

impl Render for WarmUp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let handle = window.window_handle();
        cx.defer(move |cx| {
            let _ = handle.update(cx, |_, window, _| window.remove_window());
        });
        div()
    }
}

struct Reply {
    input: Entity<TextInput>,
    _events: Subscription,
}

struct Toast {
    n: Notification,
    arrived: Instant,
    enter: Spring,
    leaving: Option<Instant>,
    /// Time left before it times out, or `None` if it stays until closed.
    left: Option<Duration>,
    total: Duration,
    hovered: bool,
    /// The card as drawn last frame, for its height and the input region.
    bounds: Rc<Cell<Bounds<Pixels>>>,
    reply: Option<Reply>,
}

impl Toast {
    fn new(n: Notification, config: &NotifyConfig) -> Self {
        let mut enter = Spring::new(0.0, SpringConfig::POPOVER);
        enter.set_target(1.0);
        let total = timeout(&n, config);
        Self {
            n,
            arrived: Instant::now(),
            enter,
            leaving: None,
            left: total,
            total: total.unwrap_or(Duration::ZERO),
            hovered: false,
            bounds: Rc::default(),
            reply: None,
        }
    }

    fn active(&self) -> bool {
        self.leaving.is_none()
    }
}

/// How long a toast stays: the app's timeout, the configured default, or forever for critical
/// ones and apps that ask for it.
fn timeout(n: &Notification, config: &NotifyConfig) -> Option<Duration> {
    if n.urgency == Urgency::Critical {
        return None;
    }
    match n.expire_ms {
        0 => None,
        ms if ms > 0 => Some(Duration::from_millis(ms as u64)),
        _ => Some(Duration::from_millis(config.toasts.timeout_ms as u64)),
    }
}

pub struct Toasts {
    items: Vec<Toast>,
    config: NotifyConfig,
    monitor: Option<String>,
    last: Instant,
    input_region: Vec<Bounds<Pixels>>,
    tick: Option<Task<()>>,
    sharing: bool,
}

impl Toasts {
    fn open(n: Notification, cx: &mut App) -> anyhow::Result<WindowHandle<Toasts>> {
        let config = cx.global::<Stack>().config.clone();
        let sharing = cx.global::<Stack>().sharing;
        let (display, monitor) = crate::hypr::display_for(&config.toasts.monitor, cx);
        let screen_height =
            display.and_then(|id| cx.find_display(id)).map(|d| f32::from(d.bounds().size.height)).unwrap_or(1080.);
        let height = px((screen_height - 80.).clamp(300., 1100.));
        let handle = cx.open_window(
            WindowOptions {
                titlebar: None,
                focus: false,
                window_bounds: Some(WindowBounds::Windowed(Bounds {
                    origin: point(px(0.), px(0.)),
                    size: size(SHADOW + WIDTH + EDGE, height),
                })),
                app_id: Some("yozora-notify".into()),
                window_background: WindowBackgroundAppearance::Transparent,
                display_id: display,
                inactive_frame_interval: None,
                kind: WindowKind::LayerShell(LayerShellOptions {
                    namespace: "yozora-notify".into(),
                    layer: Layer::Overlay,
                    anchor: Anchor::TOP | Anchor::RIGHT,
                    // Keyboard only after a click, for inline replies.
                    keyboard_interactivity: KeyboardInteractivity::OnDemand,
                    ..Default::default()
                }),
                ..Default::default()
            },
            |window, cx| {
                // Nothing takes input until the first card has been laid out.
                window.set_input_region(Some(&[]));
                cx.new(|cx| {
                    let mut toasts = Self {
                        items: Vec::new(),
                        config,
                        monitor,
                        last: Instant::now(),
                        input_region: Vec::new(),
                        tick: None,
                        sharing,
                    };
                    toasts.add(n, window, cx);
                    toasts
                })
            },
        )?;
        Ok(handle)
    }

    fn add(&mut self, n: Notification, _window: &mut Window, cx: &mut Context<Self>) {
        if let Some(t) = self.items.iter_mut().find(|t| t.n.id == n.id && t.active()) {
            // An update: new content, a fresh timer, no second entrance.
            t.total = timeout(&n, &self.config).unwrap_or(Duration::ZERO);
            t.left = timeout(&n, &self.config);
            t.n = n;
        } else {
            self.items.retain(|t| t.n.id != n.id);
            self.items.insert(0, Toast::new(n, &self.config));
        }
        // Too many: the oldest that can time out goes to the center early.
        let max = self.config.toasts.max_visible as usize;
        while self.items.iter().filter(|t| t.active()).count() > max {
            let Some(oldest) = self.items.iter().rev().find(|t| t.active() && t.n.urgency != Urgency::Critical) else {
                break;
            };
            let id = oldest.n.id;
            self.leave(id, Some(Cmd::Expired(id)), cx);
        }
        cx.notify();
    }

    /// Starts a toast's exit, telling the server why if `cmd` is set.
    fn leave(&mut self, id: u32, cmd: Option<Cmd>, cx: &mut Context<Self>) {
        if let Some(t) = self.items.iter_mut().find(|t| t.n.id == id && t.active()) {
            t.leaving = Some(Instant::now());
            t.reply = None;
            if let Some(cmd) = cmd {
                Stack::send(cmd, cx);
            }
            cx.notify();
        }
    }

    fn act(&mut self, id: u32, key: String, cx: &mut Context<Self>) {
        self.leave(id, Some(Cmd::Action(id, key)), cx);
    }

    fn start_reply(&mut self, id: u32, window: &mut Window, cx: &mut Context<Self>) {
        let Some(t) = self.items.iter_mut().find(|t| t.n.id == id) else { return };
        let placeholder = match t.n.reply_label() {
            Some(l) if !l.is_empty() && !l.eq_ignore_ascii_case("reply") && l != "inline-reply" => l.to_string(),
            _ => format!("Reply to {}", if t.n.summary.is_empty() { &t.n.app } else { &t.n.summary }),
        };
        let input = cx.new(|cx| TextInput::new(placeholder, window, cx));
        let events = cx.subscribe(&input, move |this: &mut Self, input, event: &InputEvent, cx| match event {
            InputEvent::Submit => {
                let text = input.read(cx).text().trim().to_string();
                if !text.is_empty() {
                    this.leave(id, Some(Cmd::Reply(id, text)), cx);
                }
            }
            InputEvent::Cancel => {
                if let Some(t) = this.items.iter_mut().find(|t| t.n.id == id) {
                    t.reply = None;
                }
                cx.notify();
            }
            _ => {}
        });
        window.focus(&input.focus_handle(cx), cx);
        t.reply = Some(Reply { input, _events: events });
        cx.notify();
    }

    /// Advances timers and springs. Returns whether anything is still moving.
    fn advance(&mut self, reduced: bool, window: &mut Window, cx: &mut Context<Self>) -> (bool, bool) {
        let now = Instant::now();
        let dt = (now - self.last).min(Duration::from_millis(250));
        self.last = now;
        let max = self.config.toasts.max_visible as usize;
        let mut moving = false;
        let mut timing = false;
        let mut expired = Vec::new();
        let mut shown = 0;
        for t in &mut self.items {
            if let Some(since) = t.leaving {
                moving |= now - since < leave_duration(reduced);
                continue;
            }
            shown += 1;
            if !reduced {
                moving |= !t.enter.step(dt);
            } else {
                moving |= now - t.arrived < REDUCED;
            }
            // Waiting toasts beyond the limit, and ones being read, keep their time.
            if shown > max || t.hovered || t.reply.is_some() {
                continue;
            }
            if let Some(left) = t.left.as_mut() {
                *left = left.saturating_sub(dt);
                timing = true;
                if left.is_zero() {
                    expired.push(t.n.id);
                }
            }
        }
        for id in expired {
            self.leave(id, Some(Cmd::Expired(id)), cx);
            moving = true;
        }
        let before = self.items.len();
        self.items.retain(|t| t.leaving.is_none_or(|since| now - since < leave_duration(reduced)));
        if self.items.is_empty() && before > 0 {
            // The last one has gone: close the window, and forget it so the next toast opens a
            // new one on whichever monitor is focused then.
            cx.defer(|cx| cx.global_mut::<Stack>().window = None);
            yozora_ui::close_after_frame(window, cx);
        }
        (moving, timing)
    }

    fn schedule(&mut self, moving: bool, timing: bool, window: &mut Window, cx: &mut Context<Self>) {
        if moving {
            window.request_animation_frame();
        } else if timing && self.tick.is_none() {
            self.tick = Some(cx.spawn(async move |this, cx| {
                cx.background_executor().timer(TIMER_FRAME).await;
                let _ = this.update(cx, |t, cx| {
                    t.tick = None;
                    cx.notify();
                });
            }));
        }
    }
}

fn leave_duration(reduced: bool) -> Duration {
    if reduced { REDUCED } else { FADE_OUT + COLLAPSE }
}

/// "now", "4m", "2h": how long ago a toast arrived.
fn ago(arrived: Instant) -> String {
    let s = arrived.elapsed().as_secs();
    match s {
        0..60 => "now".into(),
        60..3600 => format!("{}m", s / 60),
        _ => format!("{}h", s / 3600),
    }
}

fn with_alpha(c: Rgba, a: f32) -> Rgba {
    Rgba { a, ..c }
}

impl Render for Toasts {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::global(cx).clone();
        let reduced = theme.motion.reduced;
        let (moving, timing) = self.advance(reduced, window, cx);
        self.schedule(moving, timing, window, cx);

        // Input only on the cards, as drawn last frame.
        let region: Vec<Bounds<Pixels>> = self
            .items
            .iter()
            .filter(|t| t.active())
            .map(|t| t.bounds.get())
            .filter(|b| b.size.height > px(0.))
            .collect();
        if region != self.input_region {
            window.set_input_region(Some(&region));
            self.input_region = region;
        }

        let max = self.config.toasts.max_visible as usize;
        let now = Instant::now();
        let mut cards: Vec<AnyElement> = Vec::new();
        let mut shown = 0;
        let mut waiting = 0;
        for (i, t) in self.items.iter().enumerate() {
            if t.active() {
                shown += 1;
                if shown > max {
                    waiting += 1;
                    continue;
                }
            }
            let measured = f32::from(t.bounds.get().size.height);
            // Opacity, horizontal offset and how much of its height the card takes up.
            let (opacity, x, height) = match t.leaving {
                Some(since) if reduced => {
                    (1.0 - ((now - since).as_secs_f32() / REDUCED.as_secs_f32()).min(1.0), 0.0, 1.0)
                }
                Some(since) => {
                    let fade = ((now - since).as_secs_f32() / FADE_OUT.as_secs_f32()).min(1.0);
                    let collapse =
                        ((now - since).saturating_sub(FADE_OUT).as_secs_f32() / COLLAPSE.as_secs_f32()).min(1.0);
                    let ease = 1.0 - (1.0 - collapse).powi(3);
                    (1.0 - fade * fade, SLIDE * fade * fade, 1.0 - ease)
                }
                None if reduced => ((now - t.arrived).as_secs_f32() / REDUCED.as_secs_f32(), 0.0, 1.0),
                None => {
                    let v = t.enter.value();
                    ((now - t.arrived).as_secs_f32() / FADE_IN, (1.0 - v) * SLIDE, v.clamp(0.0, 1.0))
                }
            };
            let card = self.card(i, t, &theme, cx);
            let mut slot = div().relative().w(WIDTH).flex_none();
            if height < 1.0 {
                // Until the card has been measured once, assume a typical height.
                let full = if measured > 0. { measured } else { 92. };
                slot = slot.h(px((full + GAP) * height)).overflow_hidden();
            } else {
                slot = slot.pb(px(GAP));
            }
            cards.push(
                slot.child(div().relative().left(px(x)).opacity(opacity.clamp(0.0, 1.0)).child(card))
                    .into_any_element(),
            );
        }

        div().size_full().font_family(theme.font.clone()).text_size(px(13.)).text_color(theme.ink).child(
            div()
                .absolute()
                .top(EDGE)
                .left(SHADOW)
                .w(WIDTH)
                .flex()
                .flex_col()
                .children(cards)
                .when(waiting > 0, |el| el.child(self.more(waiting, &theme))),
        )
    }
}

impl Toasts {
    fn more(&self, waiting: usize, theme: &Theme) -> impl IntoElement {
        let monitor = self.monitor.clone().unwrap_or_default();
        div().flex().justify_end().child(
            div()
                .id("more")
                .h(px(28.))
                .px(px(12.))
                .rounded(px(14.))
                .flex()
                .items_center()
                .bg(theme.glass_pop)
                .border_1()
                .border_color(theme.hair_2)
                .text_size(px(12.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme.ink_2)
                .cursor_pointer()
                .hover(|s| s.text_color(theme.ink))
                .child(format!("+{waiting} more in the center"))
                .on_click(move |_, _, _| {
                    let _ = std::process::Command::new("yozora-bar")
                        .args(["msg", "toggle", "notifications", &monitor])
                        .stdin(std::process::Stdio::null())
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .spawn()
                        .map(|mut c| std::thread::spawn(move || c.wait()));
                }),
        )
    }

    fn card(&self, i: usize, t: &Toast, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let n = &t.n;
        let id = n.id;
        let critical = n.urgency == Urgency::Critical;
        let mut shadow = theme.popover_shadow();
        if critical {
            shadow.push(BoxShadow {
                color: with_alpha(theme.danger, 0.16).into(),
                offset: point(px(0.), px(0.)),
                blur_radius: px(0.),
                spread_radius: px(3.),
                inset: false,
            });
        }
        let bounds = t.bounds.clone();
        let has_default = n.has_action("default");
        // While the screen is shared, a private toast shows only its app. So does one Jev hasn't
        // answered about yet.
        let hidden = self.sharing && n.private != Some(false);

        let mut body = div().flex_1().min_w_0().flex().flex_col().child(
            div()
                .flex()
                .justify_between()
                .gap(px(8.))
                .pr(px(18.))
                .text_size(px(11.5))
                .text_color(theme.ink_2)
                .child(div().truncate().child(if n.app.is_empty() {
                    "Notification".to_string()
                } else {
                    n.app.clone()
                }))
                .child(div().flex_none().child(ago(t.arrived))),
        );
        if hidden {
            body = body.child(
                div().mt(px(2.)).text_size(px(13.)).text_color(theme.ink_2).child("Hidden while you share your screen"),
            );
        } else if !n.summary.is_empty() {
            body = body.child(
                div()
                    .mt(px(2.))
                    .text_size(px(13.5))
                    .font_weight(FontWeight::SEMIBOLD)
                    .line_clamp(2)
                    .child(n.summary.clone()),
            );
        }
        if !hidden && !n.body.is_empty() {
            body = body.child(
                div()
                    .mt(px(2.))
                    .text_size(px(12.5))
                    .line_height(px(17.5))
                    .text_color(theme.ink_2)
                    .line_clamp(3)
                    .child(n.body.clone()),
            );
        }
        if n.preview
            && !hidden
            && let Some(path) = &n.image
        {
            body = body
                .child(img(path.clone()).mt(px(9.)).w_full().h(px(132.)).rounded(px(10.)).object_fit(ObjectFit::Cover));
        }
        if let Some(p) = n.progress {
            body =
                body.child(
                    div()
                        .mt(px(9.))
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .child(
                            div().flex_1().h(px(6.)).rounded(px(3.)).bg(theme.fill_2).child(
                                div().h_full().w(gpui::relative(p as f32 / 100.)).rounded(px(3.)).bg(theme.accent),
                            ),
                        )
                        .child(div().text_size(px(11.5)).text_color(theme.ink_3).child(format!("{p}%"))),
                );
        }
        if hidden {
            // Action labels can say as much as the body.
        } else if let Some(reply) = &t.reply {
            let input = reply.input.clone();
            body = body.child(
                div()
                    .mt(px(10.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(div().flex_1().min_w_0().child(input.clone()))
                    .child(
                        div()
                            .id("send")
                            .size(px(32.))
                            .flex_none()
                            .rounded(px(9.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .bg(theme.accent)
                            .text_color(theme.accent_ink)
                            .cursor_pointer()
                            .hover(|s| s.opacity(0.9))
                            .child(icon_sized(Icon::ChevronRight, 15.))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                let text = input.read(cx).text().trim().to_string();
                                if !text.is_empty() {
                                    this.leave(id, Some(Cmd::Reply(id, text)), cx);
                                }
                            })),
                    ),
            );
        } else {
            let mut buttons: Vec<(SharedString, Option<String>)> =
                n.buttons().take(3).map(|(k, l)| (l.clone().into(), Some(k.clone()))).collect();
            if n.reply_label().is_some() {
                buttons.insert(0, ("Reply".into(), None));
                buttons.truncate(3);
            }
            if !buttons.is_empty() {
                let row = buttons.into_iter().enumerate().map(|(b, (label, key))| {
                    let primary = b == 0;
                    div()
                        .id(("action", b))
                        .flex_1()
                        .min_w_0()
                        .h(px(30.))
                        .px(px(8.))
                        .rounded(px(8.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(12.5))
                        .font_weight(FontWeight::SEMIBOLD)
                        .cursor_pointer()
                        .map(|el| {
                            if primary {
                                el.bg(theme.accent_soft).text_color(theme.accent).hover(|s| s.opacity(0.85))
                            } else {
                                el.bg(theme.fill_2).border_1().border_color(theme.hair).hover(|s| s.bg(theme.fill_3))
                            }
                        })
                        .child(div().truncate().child(label))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            cx.stop_propagation();
                            match &key {
                                Some(key) => this.act(id, key.clone(), cx),
                                None => this.start_reply(id, window, cx),
                            }
                        }))
                });
                body = body.child(div().mt(px(10.)).flex().gap(px(8.)).children(row));
            }
        }

        let timer = t.left.filter(|_| !t.total.is_zero()).map(|left| {
            let f = left.as_secs_f32() / t.total.as_secs_f32();
            div().absolute().left(px(14.)).right(px(14.)).bottom(px(5.)).h(px(2.)).rounded(px(1.)).bg(theme.fill).child(
                div().h_full().w(gpui::relative(f.clamp(0., 1.))).rounded(px(1.)).bg(with_alpha(theme.accent, 0.7)),
            )
        });

        div()
            .id(("toast", id as usize))
            .group("toast")
            .relative()
            .flex_none()
            .w(WIDTH)
            .flex()
            .gap(px(12.))
            .pl(px(12.))
            .pr(px(14.))
            .pt(px(12.))
            .pb(px(14.))
            .rounded(px(16.))
            .bg(theme.glass_pop)
            .border_1()
            .border_color(if critical { with_alpha(theme.danger, 0.55) } else { theme.hair_2 })
            .shadow(shadow)
            .overflow_hidden()
            .cursor_pointer()
            .child(canvas(move |b, _, _| bounds.set(b), |_, _, _, _| {}).absolute().top_0().left_0().size_full())
            .child(if hidden {
                // An attached image is often a photo of whoever wrote.
                self.icon(&Notification { image: None, ..n.clone() }, theme).into_any_element()
            } else {
                self.icon(n, theme).into_any_element()
            })
            .child(body)
            .child(
                div()
                    .id(("close", i))
                    .absolute()
                    .top(px(9.))
                    .right(px(9.))
                    .size(px(22.))
                    .rounded(px(7.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(theme.ink_2)
                    .opacity(0.)
                    .group_hover("toast", |s| s.opacity(1.))
                    .hover(|s| s.bg(theme.fill_2).text_color(theme.ink))
                    .child(icon_sized(Icon::Close, 12.))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        this.leave(id, Some(Cmd::Dismissed(id)), cx);
                    })),
            )
            .children(timer)
            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                if let Some(t) = this.items.iter_mut().find(|t| t.n.id == id) {
                    t.hovered = *hovered;
                    cx.notify();
                }
            }))
            .on_click(cx.listener(move |this, _, _, cx| {
                if this.items.iter().any(|t| t.n.id == id && t.reply.is_some()) {
                    return;
                }
                if has_default {
                    this.act(id, "default".into(), cx);
                } else {
                    // Nothing to open: send it to the center.
                    this.leave(id, Some(Cmd::Expired(id)), cx);
                }
            }))
            .into_any_element()
    }

    fn icon(&self, n: &Notification, theme: &Theme) -> impl IntoElement {
        let critical = n.urgency == Urgency::Critical;
        let tile = div()
            .size(px(36.))
            .flex_none()
            .rounded(px(10.))
            .overflow_hidden()
            .flex()
            .items_center()
            .justify_center()
            .bg(if critical { with_alpha(theme.danger, 0.16) } else { theme.fill_3 });
        let glyph = if critical { theme.danger } else { theme.ink };
        match (&n.image, &n.icon) {
            (Some(image), _) if !n.preview => {
                tile.child(img(image.clone()).size(px(36.)).object_fit(ObjectFit::Cover)).into_any_element()
            }
            (_, Some(icon)) if is_symbolic(icon) => tile
                .child(IconElement::from_path(icon.to_string_lossy().into_owned(), 18.).text_color(glyph))
                .into_any_element(),
            (_, Some(icon)) => tile.child(img(icon.clone()).size(px(24.))).into_any_element(),
            _ => {
                let fallback = if critical { Icon::Warning } else { guess_app_icon(&n.app, &n.desktop_entry) };
                tile.child(icon_sized(fallback, 17.).text_color(glyph)).into_any_element()
            }
        }
    }
}
