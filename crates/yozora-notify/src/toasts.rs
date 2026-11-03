//! Toasts: one layer-shell surface on the overlay layer, top right of a monitor, holding a
//! stack of cards, newest on top. The surface is transparent and takes input only on the cards.
//!
//! The window opens on the focused monitor when the first toast arrives. Opening one costs a
//! few hundred milliseconds, so it stays, empty and taking no input, for a while after the last
//! toast leaves, then closes, so an idle daemon has no surface at all.
//!
//! Motion follows the design language. A card arrives on the settle spring, fading in as it
//! drops the last 8 px into place. It leaves in 140 ms, fading and sliding 12 px toward the
//! edge, and never waits for anything: the cards below move up at once, on the same spring,
//! retargeted rather than restarted if something else arrives meanwhile. Timers run on the
//! wall clock. At rest nothing draws but a draining timer line, at 30 fps.

use std::{
    cell::Cell,
    rc::Rc,
    time::{Duration, Instant},
};

use async_channel::Sender;
use gpui::{
    AnyElement, App, AppContext, Bounds, Context, Entity, Focusable, FontWeight, Global, InteractiveElement,
    IntoElement, ObjectFit, ParentElement, Pixels, Render, SharedString, StatefulInteractiveElement, Styled,
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
    theme::{CONTROL_H, RADIUS_LG, RADIUS_MD, RADIUS_SM, SHADOW_REACH, SPACE, TOAST_W, motion},
};

use crate::{
    server::Cmd,
    store::{Notification, Urgency, now},
};

/// From the bar and from the screen's right edge.
const EDGE: Pixels = px(16.);
const GAP: f32 = 8.;
/// The height assumed for a card that hasn't been laid out yet.
const GUESS_H: f32 = 96.;
/// How often a draining timer line redraws.
const TIMER_FRAME: Duration = Duration::from_millis(33);
/// While a card is hovered, how often to check the pointer is still on it. A pointer that
/// leaves the surface doesn't move GPUI's idea of where it is, so this asks the window too.
const HOVER_POLL: Duration = Duration::from_millis(100);
/// Leaving a hovered card resumes its timer with at least this long left.
const MIN_RESUME: Duration = Duration::from_millis(1500);
/// With nothing moving, how often the "2 min" ages refresh.
const AGE_TICK: Duration = Duration::from_secs(30);
/// How long the empty window stays open for the next toast.
const LINGER: Duration = Duration::from_secs(30);

/// The open toast window, if any.
pub struct Stack {
    window: Option<WindowHandle<Toasts>>,
    config: NotifyConfig,
    cmds: Sender<Cmd>,
    /// The screen is being shared: toasts hide what they say.
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
        if let Some((handle, view)) = Self::view(cx) {
            // An empty window left open on a monitor that no longer has focus makes way for one
            // where the person is looking.
            let (idle, monitor) = {
                let t = view.read(cx);
                (t.items.is_empty(), t.monitor.clone())
            };
            let wanted = cx.global::<Stack>().config.toasts.monitor.clone();
            if !idle || crate::hypr::display_for(&wanted, cx).1 == monitor {
                view.update(cx, |toasts, cx| toasts.add(n, cx));
                return;
            }
            let _ = handle.update(cx, |_, window, _| window.remove_window());
            cx.global_mut::<Stack>().window = None;
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

    /// The center opened: everything that can time out goes there now.
    pub fn expire_all(cx: &mut App) {
        if let Some((_, view)) = Self::view(cx) {
            view.update(cx, |t, cx| {
                for id in t.ids(|t| t.n.urgency != Urgency::Critical) {
                    t.leave(id, Some(Cmd::Expired(id)), cx);
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

/// How long a toast has left, on the wall clock.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Clock {
    /// Critical, or the app asked it to stay until closed.
    Stays,
    Running {
        until: Instant,
        total: Duration,
    },
    /// Hovered, being replied to, or waiting below the visible ones.
    Paused {
        left: Duration,
        total: Duration,
    },
}

impl Clock {
    /// A clock that starts when the toast is first on screen.
    fn new(total: Option<Duration>) -> Self {
        total.map_or(Clock::Stays, |total| Clock::Paused { left: total, total })
    }

    fn pause(&mut self, now: Instant) {
        if let Clock::Running { until, total } = *self {
            *self = Clock::Paused { left: until.saturating_duration_since(now), total };
        }
    }

    /// Runs again with what was left, but never less than `floor`.
    fn resume(&mut self, now: Instant, floor: Duration) {
        if let Clock::Paused { left, total } = *self {
            *self = Clock::Running { until: now + left.max(floor), total };
        }
    }

    /// The share of its time left, or `None` if it stays.
    fn fraction(&self, now: Instant) -> Option<f32> {
        let (left, total) = match *self {
            Clock::Stays => return None,
            Clock::Running { until, total } => (until.saturating_duration_since(now), total),
            Clock::Paused { left, total } => (left, total),
        };
        Some(if total.is_zero() { 0. } else { (left.as_secs_f32() / total.as_secs_f32()).clamp(0., 1.) })
    }
}

/// How long a toast stays: the app's timeout, or the configured one (two thirds of it for low
/// urgency), or until closed for critical ones and apps that ask.
fn timeout(n: &Notification, config: &NotifyConfig) -> Option<Duration> {
    if n.urgency == Urgency::Critical {
        return None;
    }
    let ms = match n.expire_ms {
        0 => return None,
        ms if ms > 0 => ms as u64,
        _ if n.urgency == Urgency::Low => u64::from(config.toasts.timeout_ms) * 2 / 3,
        _ => u64::from(config.toasts.timeout_ms),
    };
    Some(Duration::from_millis(ms))
}

struct Reply {
    input: Entity<TextInput>,
    _events: Subscription,
}

struct Toast {
    /// Tells cards apart even when an app reuses an id while the old card is still leaving.
    key: usize,
    n: Notification,
    /// When it last came on screen; `None` while it waits below the visible ones.
    shown: Option<Instant>,
    /// When it went from on screen to waiting, for the fade.
    hiding: Option<Instant>,
    leaving: Option<Instant>,
    /// The card's top, relative to the first card's.
    y: Spring,
    clock: Clock,
    hovered: bool,
    /// The card as drawn last frame, for its height and the input region.
    bounds: Rc<Cell<Bounds<Pixels>>>,
    reply: Option<Reply>,
}

impl Toast {
    fn active(&self) -> bool {
        self.leaving.is_none()
    }

    fn height(&self) -> f32 {
        let h = f32::from(self.bounds.get().size.height);
        if h > 0. { h } else { GUESS_H }
    }
}

pub struct Toasts {
    /// Newest first.
    items: Vec<Toast>,
    next_key: usize,
    config: NotifyConfig,
    monitor: Option<String>,
    last: Instant,
    input_region: Vec<Bounds<Pixels>>,
    tick: Option<Task<()>>,
    sharing: bool,
    /// Whether the surface currently asks for the keyboard (only while a reply is open).
    keyboard: bool,
    /// The "2 more" chip under the visible cards.
    more_y: Spring,
    more_bounds: Rc<Cell<Bounds<Pixels>>>,
}

/// What the stack needs next to keep its picture current.
enum Wake {
    /// Something is moving: draw the next frame.
    Frame,
    After(Duration),
    /// Nothing left: close the window if nothing arrives for a while.
    Close,
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
                    size: size(SHADOW_REACH + TOAST_W + EDGE, height),
                })),
                app_id: Some("yozora-notify".into()),
                window_background: WindowBackgroundAppearance::Transparent,
                display_id: display,
                inactive_frame_interval: None,
                kind: WindowKind::LayerShell(LayerShellOptions {
                    namespace: "yozora-notify".into(),
                    layer: Layer::Overlay,
                    anchor: Anchor::TOP | Anchor::RIGHT,
                    // No keyboard until an inline reply starts, so a toast never takes focus
                    // from the window being typed in.
                    keyboard_interactivity: KeyboardInteractivity::None,
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
                        next_key: 0,
                        config,
                        monitor,
                        last: Instant::now(),
                        input_region: Vec::new(),
                        tick: None,
                        sharing,
                        keyboard: false,
                        more_y: Spring::new(0., SpringConfig::SETTLE),
                        more_bounds: Rc::default(),
                    };
                    toasts.add(n, cx);
                    toasts
                })
            },
        )?;
        Ok(handle)
    }

    fn ids(&self, which: impl Fn(&Toast) -> bool) -> Vec<u32> {
        self.items.iter().filter(|t| t.active() && which(t)).map(|t| t.n.id).collect()
    }

    fn add(&mut self, n: Notification, cx: &mut Context<Self>) {
        let clock = Clock::new(timeout(&n, &self.config));
        if let Some(t) = self.items.iter_mut().find(|t| t.n.id == n.id && t.active()) {
            // An update: new content and a fresh timer, in the same place, with no second
            // entrance.
            t.clock = clock;
            t.n = n;
        } else {
            self.items.insert(
                0,
                Toast {
                    key: self.next_key,
                    n,
                    shown: None,
                    hiding: None,
                    leaving: None,
                    y: Spring::new(0., SpringConfig::SETTLE),
                    clock,
                    hovered: false,
                    bounds: Rc::default(),
                    reply: None,
                },
            );
            self.next_key += 1;
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
        let Some(t) = self.items.iter_mut().find(|t| t.n.id == id && t.active()) else { return };
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
                if let Some(t) = this.items.iter_mut().find(|t| t.n.id == id && t.active()) {
                    t.reply = None;
                }
                cx.notify();
            }
            _ => {}
        });
        window.set_keyboard_interactivity(KeyboardInteractivity::OnDemand);
        self.keyboard = true;
        window.focus(&input.focus_handle(cx), cx);
        t.reply = Some(Reply { input, _events: events });
        cx.notify();
    }

    /// Lays the stack out, advances springs and clocks, and says when to look again.
    fn advance(&mut self, reduced: bool, window: &mut Window, cx: &mut Context<Self>) -> Option<Wake> {
        let now = Instant::now();
        // Springs take a clamped step so a stalled frame doesn't jump; clocks read the real
        // time, so a throttled window never stretches how long a toast stays.
        let dt = (now - self.last).min(Duration::from_millis(100));
        self.last = now;
        let (enter, exit) = durations(reduced);
        let pointer = window.mouse_position();
        let in_window = window.is_window_hovered();
        let max = self.config.toasts.max_visible.max(1) as usize;

        let mut frame = false;
        let mut after: Option<Duration> = None;
        let mut soon = |d: Duration| after = Some(after.map_or(d, |a| a.min(d)));
        let mut expired = Vec::new();
        let mut slot = 0.;
        let mut visible = 0;
        for t in &mut self.items {
            if let Some(since) = t.leaving {
                frame |= now - since < exit;
                continue;
            }
            if visible == max {
                // Waiting below the visible ones, with its time kept.
                if t.shown.take().is_some() {
                    t.hiding = Some(now);
                }
                frame |= t.hiding.is_some_and(|h| now - h < exit);
                t.clock.pause(now);
                t.hovered = false;
                continue;
            }
            visible += 1;
            if t.shown.is_none() {
                t.shown = Some(now);
                t.hiding = None;
                t.y = Spring::new(if reduced { slot } else { slot - motion::RISE }, SpringConfig::SETTLE);
            }
            if reduced {
                t.y = Spring::new(slot, SpringConfig::SETTLE);
            } else {
                t.y.set_target(slot);
                frame |= !t.y.step(dt);
            }
            frame |= t.shown.is_some_and(|s| now - s < enter);
            slot += t.height() + GAP;

            t.hovered = in_window && t.bounds.get().contains(&pointer);
            if t.hovered || t.reply.is_some() {
                t.clock.pause(now);
            } else {
                t.clock.resume(now, MIN_RESUME);
            }
            if t.hovered {
                soon(HOVER_POLL);
            }
            if let Clock::Running { until, .. } = t.clock {
                if until <= now {
                    expired.push(t.n.id);
                } else {
                    soon(TIMER_FRAME.min(until - now));
                }
            }
        }
        if reduced {
            self.more_y = Spring::new(slot, SpringConfig::SETTLE);
        } else {
            self.more_y.set_target(slot);
            frame |= !self.more_y.step(dt);
        }
        for id in expired {
            self.leave(id, Some(Cmd::Expired(id)), cx);
            frame = true;
        }

        if self.keyboard && !self.items.iter().any(|t| t.reply.is_some()) {
            window.set_keyboard_interactivity(KeyboardInteractivity::None);
            self.keyboard = false;
        }
        let before = self.items.len();
        self.items.retain(|t| t.leaving.is_none_or(|since| now - since < exit));
        if self.items.is_empty() {
            // Keep the close already scheduled when nothing has changed since.
            return (before > 0 || self.tick.is_none()).then_some(Wake::Close);
        }
        Some(if frame { Wake::Frame } else { Wake::After(after.unwrap_or(AGE_TICK)) })
    }

    fn schedule(&mut self, wake: Option<Wake>, window: &mut Window, cx: &mut Context<Self>) {
        match wake {
            Some(Wake::Frame) => {
                self.tick = None;
                window.request_animation_frame();
            }
            Some(Wake::After(d)) => {
                // Replacing the task cancels the one before.
                self.tick = Some(cx.spawn(async move |this, cx| {
                    cx.background_executor().timer(d).await;
                    let _ = this.update(cx, |_, cx| cx.notify());
                }));
            }
            Some(Wake::Close) => {
                let Some(handle) = window.window_handle().downcast::<Toasts>() else { return };
                self.tick = Some(cx.spawn(async move |_, cx| {
                    cx.background_executor().timer(LINGER).await;
                    let _ = handle.update(cx, |toasts, window, cx| {
                        if toasts.items.is_empty() {
                            // Forget it, so the next toast opens a window on whichever monitor
                            // is focused then.
                            cx.defer(|cx| cx.global_mut::<Stack>().window = None);
                            window.remove_window();
                        }
                    });
                }));
            }
            None => {}
        }
    }
}

/// How long arriving and leaving take.
fn durations(reduced: bool) -> (Duration, Duration) {
    if reduced { (motion::REDUCED, motion::REDUCED) } else { (motion::ENTER, motion::EXIT) }
}

/// How far along `elapsed` is through `total`, 0 to 1.
fn progress(elapsed: Duration, total: Duration) -> f32 {
    (elapsed.as_secs_f32() / total.as_secs_f32()).clamp(0., 1.)
}

/// The settle curve for fades: fast, then gently home.
fn ease_out(p: f32) -> f32 {
    1. - (1. - p).powi(3)
}

/// The exit curve: slow to start, then gone.
fn ease_in(p: f32) -> f32 {
    p * p
}

/// "now", "4 min", "2 h": how long ago it arrived.
fn age(n: &Notification) -> String {
    let s = now().saturating_sub(n.time);
    match s {
        0..60 => "now".into(),
        60..3600 => format!("{} min", s / 60),
        _ => format!("{} h", s / 3600),
    }
}

impl Render for Toasts {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::global(cx).clone();
        let reduced = theme.motion.reduced;
        let wake = self.advance(reduced, window, cx);
        self.schedule(wake, window, cx);

        // Input only on the cards on screen and the chip, as drawn last frame.
        let waiting = self.items.iter().filter(|t| t.active() && t.shown.is_none()).count();
        let region: Vec<Bounds<Pixels>> = self
            .items
            .iter()
            .filter(|t| t.active() && t.shown.is_some())
            .map(|t| t.bounds.get())
            .chain((waiting > 0).then(|| self.more_bounds.get()))
            .filter(|b| b.size.height > px(0.))
            .collect();
        if region != self.input_region {
            window.set_input_region(Some(&region));
            self.input_region = region;
        }

        let now = Instant::now();
        let (enter, exit) = durations(reduced);
        // Leaving and hiding cards go underneath, so the ones moving into their place cover
        // them.
        let mut under: Vec<AnyElement> = Vec::new();
        let mut over: Vec<AnyElement> = Vec::new();
        for t in &self.items {
            let y = t.y.value();
            let (opacity, x, y, list) = if let Some(since) = t.leaving {
                let p = progress(now - since, exit);
                let travel = if reduced { 0. } else { motion::SLIDE_OUT * ease_in(p) };
                (1. - if reduced { p } else { ease_in(p) }, travel, y, &mut under)
            } else if let Some(shown) = t.shown {
                let p = progress(now - shown, enter);
                (if reduced { p } else { ease_out(p) }, 0., y, &mut over)
            } else if let Some(since) = t.hiding.filter(|h| now - *h < exit) {
                let p = progress(now - since, exit);
                let sink = if reduced { 0. } else { motion::RISE * ease_in(p) };
                (1. - p, 0., y + sink, &mut under)
            } else {
                continue;
            };
            list.push(
                div()
                    .absolute()
                    .top(px(y))
                    .left(px(x))
                    .opacity(opacity)
                    .child(self.card(t, &theme, cx))
                    .into_any_element(),
            );
        }

        div().size_full().font_family(theme.font.clone()).text_size(px(13.)).text_color(theme.ink).child(
            div().absolute().top(EDGE).left(SHADOW_REACH).w(TOAST_W).h_full().children(under).children(over).when(
                waiting > 0,
                |el| {
                    el.child(div().absolute().top(px(self.more_y.value())).right_0().child(self.more(waiting, &theme)))
                },
            ),
        )
    }
}

impl Toasts {
    fn more(&self, waiting: usize, theme: &Theme) -> impl IntoElement {
        let monitor = self.monitor.clone().unwrap_or_default();
        let bounds = self.more_bounds.clone();
        div()
            .id("more")
            .relative()
            .h(px(24.))
            .px(SPACE[3])
            .rounded_full()
            .flex()
            .items_center()
            .bg(theme.surface)
            .border_1()
            .border_color(theme.line_strong)
            .text_size(px(12.))
            .line_height(px(16.))
            .font_weight(FontWeight::MEDIUM)
            .text_color(theme.ink_2)
            .cursor_pointer()
            .hover(|s| s.text_color(theme.ink).bg(theme.surface_2))
            .child(canvas(move |b, _, _| bounds.set(b), |_, _, _, _| {}).absolute().top_0().left_0().size_full())
            .child(format!("{waiting} more"))
            .on_click(move |_, _, _| {
                let _ = std::process::Command::new("yozora-bar")
                    .args(["msg", "toggle", "notifications", &monitor])
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn()
                    .map(|mut c| std::thread::spawn(move || c.wait()));
            })
    }

    fn card(&self, t: &Toast, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let n = &t.n;
        let id = n.id;
        let critical = n.urgency == Urgency::Critical;
        let bounds = t.bounds.clone();
        let has_default = n.has_action("default");
        // While the screen is shared, a toast shows only its app.
        let hidden = self.sharing;
        let paused = t.hovered || t.reply.is_some();

        let close = div()
            .id("close")
            .flex_none()
            .size(px(28.))
            .my(px(-4.))
            .mr(px(-6.))
            .rounded(RADIUS_SM)
            .flex()
            .items_center()
            .justify_center()
            .text_color(theme.ink_3)
            .cursor_pointer()
            .group_hover("toast", |s| s.bg(theme.fill_hover).text_color(theme.ink))
            .hover(|s| s.bg(theme.fill_press).text_color(theme.ink))
            .child(icon_sized(Icon::Close, 14.))
            .on_click(cx.listener(move |this, _, _, cx| {
                cx.stop_propagation();
                this.leave(id, Some(Cmd::Dismissed(id)), cx);
            }));
        let head = div()
            .flex()
            .items_center()
            .gap(SPACE[3])
            .h(px(20.))
            .text_size(px(12.))
            .line_height(px(16.))
            .font_weight(FontWeight::MEDIUM)
            .child(self.glyph(n, theme))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .gap(px(4.))
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_color(if critical { theme.danger } else { theme.ink_2 })
                            .child(if n.app.is_empty() { "Notification".to_string() } else { n.app.clone() }),
                    )
                    .child(div().flex_none().text_color(theme.ink_3).child(format!("· {}", age(n)))),
            )
            .child(close);

        let mut card = div()
            .id(("toast", t.key))
            .group("toast")
            .relative()
            .w(TOAST_W)
            .flex()
            .flex_col()
            .gap(SPACE[4])
            .p(SPACE[4])
            .rounded(RADIUS_LG)
            .bg(theme.surface)
            .border_1()
            .border_color(if critical { theme.danger } else { theme.line_strong })
            .shadow(theme.raised_shadow())
            .cursor_pointer()
            .child(canvas(move |b, _, _| bounds.set(b), |_, _, _, _| {}).absolute().top_0().left_0().size_full())
            .child(head);

        if hidden {
            card = card.child(
                div()
                    .flex()
                    .items_center()
                    .gap(SPACE[3])
                    .child(icon_sized(Icon::Display, 14.).text_color(theme.ink_3))
                    .child(
                        div().text_color(theme.ink_2).line_height(px(18.)).child("Hidden while you share your screen"),
                    ),
            );
        } else {
            let text = (!n.summary.is_empty() || !n.body.is_empty()).then(|| {
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(SPACE[0])
                    .line_height(px(18.))
                    .when(!n.summary.is_empty(), |el| {
                        el.child(div().truncate().font_weight(FontWeight::SEMIBOLD).child(n.summary.clone()))
                    })
                    .when(!n.body.is_empty(), |el| {
                        el.child(div().text_color(theme.ink_2).line_clamp(3).child(n.body.clone()))
                    })
            });
            // A small picture from the app is usually who wrote: it goes beside the text.
            let avatar = n.image.clone().filter(|_| !n.preview).map(|p| {
                img(p).size(px(32.)).flex_none().rounded_full().overflow_hidden().object_fit(ObjectFit::Cover)
            });
            if text.is_some() || avatar.is_some() {
                card = card.child(div().flex().items_start().gap(SPACE[4]).children(avatar).children(text));
            }
            if n.preview
                && let Some(path) = &n.image
            {
                card =
                    card.child(img(path.clone()).w_full().h(px(132.)).rounded(RADIUS_MD).object_fit(ObjectFit::Cover));
            }
        }
        if let Some(p) = n.progress {
            card =
                card.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .child(div().flex_1().h(px(4.)).rounded(px(2.)).bg(theme.fill_press).child(
                            div().h_full().w(gpui::relative(f32::from(p) / 100.)).rounded(px(2.)).bg(theme.accent),
                        ))
                        .child(
                            div()
                                .font_family(theme.mono.clone())
                                .text_size(px(12.))
                                .line_height(px(16.))
                                .text_color(theme.ink_2)
                                .child(format!("{p}%")),
                        ),
                );
        }
        if hidden {
            // Action labels can say as much as the body.
        } else if let Some(reply) = &t.reply {
            let input = reply.input.clone();
            card = card.child(
                div().flex().items_center().gap(SPACE[2]).child(div().flex_1().min_w_0().child(input.clone())).child(
                    div()
                        .id("send")
                        .size(CONTROL_H)
                        .flex_none()
                        .rounded(RADIUS_SM)
                        .flex()
                        .items_center()
                        .justify_center()
                        .bg(theme.accent)
                        .text_color(theme.accent_ink)
                        .cursor_pointer()
                        .hover(|s| s.opacity(0.9))
                        .child(icon_sized(Icon::ArrowRight, 14.))
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
                    div()
                        .id(("action", b))
                        .flex_1()
                        .min_w_0()
                        .h(CONTROL_H)
                        .px(SPACE[4])
                        .rounded(RADIUS_SM)
                        .flex()
                        .items_center()
                        .justify_center()
                        .font_weight(FontWeight::SEMIBOLD)
                        .bg(theme.fill)
                        .hover(|s| s.bg(theme.fill_hover))
                        .active(|s| s.bg(theme.fill_press))
                        .cursor_pointer()
                        .child(div().truncate().child(label))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            cx.stop_propagation();
                            match &key {
                                Some(key) => this.act(id, key.clone(), cx),
                                None => this.start_reply(id, window, cx),
                            }
                        }))
                });
                card = card.child(div().flex().gap(SPACE[2]).children(row));
            }
        }

        let timer =
            t.clock.fraction(Instant::now()).map(|f| {
                // Inset so the line stays inside the rounded corners.
                div().absolute().left(px(6.)).right(px(6.)).bottom(px(1.)).h(px(2.)).child(
                    div().h_full().w(gpui::relative(f)).bg(theme.accent).opacity(if paused { 0.25 } else { 0.55 }),
                )
            });

        card.children(timer)
            .on_hover(cx.listener(|_, _: &bool, _, cx| cx.notify()))
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

    /// The 16 px app glyph at the start of the head.
    fn glyph(&self, n: &Notification, theme: &Theme) -> AnyElement {
        let tile = |bg| div().size(px(16.)).flex_none().rounded(px(3.)).flex().items_center().justify_center().bg(bg);
        if n.urgency == Urgency::Critical {
            return tile(theme.danger)
                .child(icon_sized(Icon::Alert, 11.).text_color(theme.accent_ink))
                .into_any_element();
        }
        match &n.icon {
            Some(icon) if is_symbolic(icon) => tile(theme.fill_press)
                .child(IconElement::from_path(icon.to_string_lossy().into_owned(), 11.).text_color(theme.ink))
                .into_any_element(),
            Some(icon) => img(icon.clone()).size(px(16.)).flex_none().into_any_element(),
            None => tile(theme.fill_press)
                .child(icon_sized(guess_app_icon(&n.app, &n.desktop_entry), 11.).text_color(theme.ink_2))
                .into_any_element(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_pauses_and_resumes_with_a_floor() {
        let t0 = Instant::now();
        let mut c = Clock::new(Some(Duration::from_secs(6)));
        assert_eq!(c.fraction(t0), Some(1.));
        c.resume(t0, Duration::ZERO);
        c.pause(t0 + Duration::from_millis(5500));
        assert_eq!(c, Clock::Paused { left: Duration::from_millis(500), total: Duration::from_secs(6) });
        // Leaving the card gives it at least 1.5 s back.
        let t1 = t0 + Duration::from_secs(60);
        c.resume(t1, MIN_RESUME);
        assert_eq!(c, Clock::Running { until: t1 + MIN_RESUME, total: Duration::from_secs(6) });
        assert_eq!(Clock::new(None).fraction(t1), None);
    }

    #[test]
    fn timeouts_follow_urgency() {
        let config = NotifyConfig::default();
        let n = |urgency, expire_ms| Notification { urgency, expire_ms, ..Default::default() };
        let normal = Duration::from_millis(u64::from(config.toasts.timeout_ms));
        assert_eq!(timeout(&n(Urgency::Normal, -1), &config), Some(normal));
        assert_eq!(timeout(&n(Urgency::Low, -1), &config), Some(normal * 2 / 3));
        assert_eq!(timeout(&n(Urgency::Normal, 2500), &config), Some(Duration::from_millis(2500)));
        assert_eq!(timeout(&n(Urgency::Normal, 0), &config), None);
        assert_eq!(timeout(&n(Urgency::Critical, 9000), &config), None);
    }
}
