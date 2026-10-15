//! The lock screen: one window per monitor. The monitor under the pointer shows the password
//! field; the others show the time. Typing works on whichever has keyboard focus, because the
//! password lives in the shared `Lock`.

use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

use async_channel::Sender;
use gpui::{
    Animation, AnimationExt, AnyElement, AnyWindowHandle, App, BoxShadow, Context, DisplayId, Entity, FocusHandle,
    FontWeight, InteractiveElement, IntoElement, KeyDownEvent, ModifiersChangedEvent, ObjectFit, ParentElement, Render,
    Rgba, SharedString, StatefulInteractiveElement, Styled, StyledImage, Window, div, img, linear_color_stop,
    linear_gradient, point, prelude::*, px, rgba,
};
use yozora_config::lock::LockConfig;
use yozora_ui::{
    Icon, Theme, icon_sized,
    spring::{SpringConfig, settle_time, step_response},
};

use crate::{
    auth::{Auth, Phase, Tone, countdown},
    background, pam,
    services::{self, MediaCmd, Power, Snapshot},
    system::{Network, Status, User},
};

/// How long the content fades before the session unlocks.
const LEAVE: Duration = Duration::from_millis(160);
/// A second press on restart or shut down within this time confirms it.
const CONFIRM: Duration = Duration::from_secs(4);

/// State shared by every lock window.
pub struct Lock {
    pub auth: Auth,
    pub user: User,
    pub status: Status,
    pub snapshot: Snapshot,
    pub config: LockConfig,
    pub service: pam::Service,
    /// The display that shows the password field.
    pub main: Option<DisplayId>,
    pub windows: Vec<(DisplayId, AnyWindowHandle)>,
    pub media_cmds: Sender<MediaCmd>,
    /// Restart or shut down, waiting for a second press.
    confirm: Option<(Power, Instant)>,
    /// When the unlock started, for the fade.
    pub leaving: Option<Instant>,
    /// A layer-shell preview instead of a real lock.
    pub preview: bool,
    clock: String,
}

impl Lock {
    pub fn new(
        config: LockConfig,
        user: User,
        service: pam::Service,
        media_cmds: Sender<MediaCmd>,
        preview: bool,
    ) -> Self {
        Self {
            auth: Auth::new(config.auth.tries, config.auth.wait_s),
            user,
            status: Status::default(),
            snapshot: Snapshot::default(),
            service,
            main: None,
            windows: Vec::new(),
            media_cmds,
            confirm: None,
            leaving: None,
            preview,
            clock: String::new(),
            config,
        }
    }

    /// Called every half second: the clock, waits and confirmations.
    pub fn tick(&mut self, cx: &mut Context<Self>) {
        let now = Instant::now();
        let mut changed = self.auth.tick(now) || self.auth.phase == Phase::Waiting;
        if self.confirm.is_some_and(|(_, at)| now.duration_since(at) > CONFIRM) {
            self.confirm = None;
            changed = true;
        }
        let clock = chrono::Local::now().format(&self.config.clock.format).to_string();
        if clock != self.clock {
            self.clock = clock;
            changed = true;
        }
        if changed {
            cx.notify();
        }
    }

    fn key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        if self.leaving.is_some() {
            return;
        }
        let k = &event.keystroke;
        let m = k.modifiers;
        let changed = match k.key.as_str() {
            "enter" | "kp_enter" => {
                self.submit(cx);
                false
            }
            "escape" => {
                if self.preview && self.auth.dots() == 0 && self.auth.phase == Phase::Idle {
                    // Only a preview can be left without a password.
                    cx.quit();
                }
                self.auth.clear()
            }
            "backspace" if m.control || m.alt => self.auth.clear(),
            "backspace" => self.auth.backspace(),
            "u" if m.control => self.auth.clear(),
            _ if m.control || m.platform || m.function => false,
            _ => match &k.key_char {
                Some(text) => self.auth.push(text),
                None => false,
            },
        };
        if changed {
            cx.notify();
        }
    }

    fn submit(&mut self, cx: &mut Context<Self>) {
        let Some(password) = self.auth.submit() else { return };
        cx.notify();
        let service = self.service.clone();
        let login = self.user.login.clone();
        let check = cx.background_spawn(async move { pam::authenticate(&service, &login, &password) });
        cx.spawn(async move |this, cx| {
            let result = check.await;
            let _ = this.update(cx, |lock, cx| {
                match result {
                    Ok(()) => {
                        tracing::info!("password accepted");
                        lock.auth.succeeded();
                        lock.unlock(cx);
                    }
                    Err(failure) => {
                        tracing::warn!("password rejected: {} {:?}", failure.error, failure.messages);
                        lock.auth.failed(failure, Instant::now());
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Fades the content, then closes every lock window, which unlocks the session.
    fn unlock(&mut self, cx: &mut Context<Self>) {
        self.leaving = Some(Instant::now());
        let preview = self.preview;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(LEAVE + Duration::from_millis(40)).await;
            let windows = this.update(cx, |lock, _| std::mem::take(&mut lock.windows)).unwrap_or_default();
            if !preview {
                cx.background_spawn(async { services::set_locked_hint(false) }).detach();
            }
            for (_, handle) in windows {
                let _ = handle.update(cx, |_, window, _| window.remove_window());
            }
            // Give the compositor the unlock before the connection goes away.
            cx.background_executor().timer(Duration::from_millis(150)).await;
            cx.update(|cx| cx.quit());
        })
        .detach();
    }

    fn power(&mut self, action: Power, cx: &mut Context<Self>) {
        let needs_confirm = !matches!(action, Power::Suspend);
        let confirmed = self.confirm.is_some_and(|(a, at)| {
            std::mem::discriminant(&a) == std::mem::discriminant(&action) && at.elapsed() <= CONFIRM
        });
        if needs_confirm && !confirmed {
            self.confirm = Some((action, Instant::now()));
            cx.notify();
            return;
        }
        self.confirm = None;
        cx.notify();
        if self.preview {
            tracing::info!("preview: would {action:?}");
            return;
        }
        cx.background_spawn(async move {
            if let Err(e) = services::power(action) {
                tracing::error!("{action:?}: {e:#}");
            }
        })
        .detach();
    }

    fn confirming(&self, action: Power) -> bool {
        self.confirm.is_some_and(|(a, _)| std::mem::discriminant(&a) == std::mem::discriminant(&action))
    }
}

pub struct LockView {
    lock: Entity<Lock>,
    display: DisplayId,
    focus: FocusHandle,
    background: Option<PathBuf>,
    opened: Instant,
}

impl LockView {
    pub fn new(
        lock: Entity<Lock>,
        display: DisplayId,
        output: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.observe(&lock, |_, _, cx| cx.notify()).detach();
        let focus = cx.focus_handle();
        window.focus(&focus, cx);

        let (image, blur) = {
            let c = &lock.read(cx).config.background;
            (c.image.clone(), c.blur)
        };
        let found = cx.background_spawn(async move {
            let source = crate::system::wallpaper(&image, output.as_deref())?;
            background::prepare(&source, blur).map_err(|e| tracing::warn!("background: {e:#}")).ok()
        });
        cx.spawn(async move |this, cx| {
            if let Some(path) = found.await {
                let _ = this.update(cx, |view, cx| {
                    view.background = Some(path);
                    cx.notify();
                });
            }
        })
        .detach();

        Self { lock, display, focus, background: None, opened: Instant::now() }
    }
}

/// Sizes in the design are for 1080 px tall screens. Only shrunk as far as fits: a HiDPI
/// laptop can be 480 logical pixels tall.
fn scale(window: &Window) -> f32 {
    let size = window.viewport_size();
    (f32::from(size.width).min(f32::from(size.height)) / 1080.0).clamp(0.4, 2.0)
}

fn shadow(theme: &Theme) -> Vec<BoxShadow> {
    theme.popover_shadow()
}

fn ring(color: Rgba, width: f32) -> BoxShadow {
    BoxShadow {
        color: color.into(),
        offset: point(px(0.), px(0.)),
        blur_radius: px(0.),
        spread_radius: px(width),
        inset: false,
    }
}

fn with_alpha(c: Rgba, a: f32) -> Rgba {
    Rgba { a, ..c }
}

impl Render for LockView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::global(cx).clone();
        let s = scale(window);
        let lock = self.lock.read(cx);
        let main = lock.main.is_none_or(|d| d == self.display);
        let reduced = theme.motion.reduced;
        let leaving = lock.leaving.is_some();

        let (top, bottom) =
            if theme.dark { (rgba(0x05080F80), rgba(0x05080F9E)) } else { (rgba(0xFFFAF557), rgba(0xFFFAF54D)) };
        let base = if theme.dark {
            linear_gradient(165., linear_color_stop(rgba(0x1A2747FF), 0.), linear_color_stop(rgba(0x070B14FF), 1.))
        } else {
            linear_gradient(165., linear_color_stop(rgba(0xDDE6F5FF), 0.), linear_color_stop(rgba(0xF8FAFDFF), 1.))
        };

        let mut root = div()
            .id("lock")
            .track_focus(&self.focus)
            .size_full()
            .relative()
            .overflow_hidden()
            .bg(base)
            .font_family(theme.font.clone())
            .text_color(theme.ink)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                this.lock.update(cx, |lock, cx| lock.key(event, cx));
            }))
            .on_modifiers_changed(cx.listener(|_, _: &ModifiersChangedEvent, _, cx| cx.notify()))
            .on_mouse_move(cx.listener(|this, _, window, cx| {
                let display = this.display;
                this.lock.update(cx, |lock, cx| {
                    if lock.main != Some(display) {
                        lock.main = Some(display);
                        cx.notify();
                    }
                });
                // Keyboard focus follows, so typing goes where the field is.
                window.focus(&this.focus, cx);
            }));

        if let Some(path) = &self.background {
            let bg = img(path.clone()).absolute().inset_0().size_full().object_fit(ObjectFit::Cover);
            root = root.child(if reduced {
                bg.into_any_element()
            } else {
                bg.with_animation("bg", Animation::new(Duration::from_millis(300)), |el, t| el.opacity(t))
                    .into_any_element()
            });
        }
        root = root.child(div().absolute().inset_0().bg(linear_gradient(
            180.,
            linear_color_stop(top, 0.),
            linear_color_stop(bottom, 1.),
        )));

        let content = if main { self.main_content(s, &theme, window, cx) } else { self.clock_only(s, &theme, cx) };
        let content = div().absolute().inset_0().child(content);

        // The content rises into place on the popover spring, or fades when motion is reduced,
        // and fades out on unlock.
        let content = if leaving {
            content.with_animation("leave", Animation::new(LEAVE), |el, t| el.opacity(1. - t)).into_any_element()
        } else if self.opened.elapsed() > Duration::from_secs(2) {
            content.into_any_element()
        } else if reduced {
            content
                .with_animation("enter", Animation::new(Duration::from_millis(120)), |el, t| el.opacity(t))
                .into_any_element()
        } else {
            let spring = SpringConfig::POPOVER;
            let total = settle_time(spring);
            let secs = total.as_secs_f32();
            content
                .with_animation("enter", Animation::new(total), move |el, t| {
                    let v = step_response(spring, t * secs);
                    el.top(px((1. - v) * 12. * s)).opacity((t * secs / 0.18).min(1.))
                })
                .into_any_element()
        };
        root.child(content)
    }
}

impl LockView {
    fn clock(&self, s: f32, theme: &Theme, cx: &App) -> impl IntoElement {
        let lock = self.lock.read(cx);
        let now = chrono::Local::now();
        div()
            .flex()
            .flex_col()
            .items_center()
            .child(
                div()
                    .text_size(px(168. * s))
                    .line_height(px(168. * s))
                    .font_weight(FontWeight::LIGHT)
                    .child(SharedString::from(now.format(&lock.config.clock.format).to_string())),
            )
            .child(
                div()
                    .mt(px(14. * s))
                    .text_size(px(22. * s))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme.ink_2)
                    .child(SharedString::from(now.format(&lock.config.clock.date).to_string())),
            )
    }

    fn clock_only(&self, s: f32, theme: &Theme, cx: &App) -> AnyElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .pb(px(80. * s))
            .child(self.clock(s, theme, cx))
            .into_any_element()
    }

    fn main_content(&self, s: f32, theme: &Theme, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let lock = self.lock.read(cx);
        let caps = window.capslock().on;
        let mut el = div()
            .size_full()
            .relative()
            .child(self.top(s, theme, &lock.status))
            .child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .top(px(176. * s))
                    .flex()
                    .justify_center()
                    .child(self.clock(s, theme, cx)),
            )
            .child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .top(px(560. * s))
                    .flex()
                    .justify_center()
                    .child(self.auth(s, theme, caps, cx)),
            );
        let lock = self.lock.read(cx);
        if lock.config.show.media
            && let Some(media) = lock.snapshot.media.clone()
        {
            el = el.child(self.media(s, theme, media, cx));
        }
        let lock = self.lock.read(cx);
        if lock.config.show.notifications
            && let Some(n) = lock.snapshot.notifications.filter(|n| *n > 0)
        {
            el = el.child(self.notes(s, theme, n));
        }
        if self.lock.read(cx).config.show.power {
            el = el.child(self.power(s, theme, cx));
        }
        el.into_any_element()
    }

    fn top(&self, s: f32, theme: &Theme, status: &Status) -> impl IntoElement {
        let chip = |content: AnyElement| {
            div().h(px(26. * s)).px(px(8. * s)).flex().items_center().gap(px(6. * s)).child(content)
        };
        let mut pill = div()
            .h(px(34. * s))
            .px(px(6. * s))
            .flex()
            .items_center()
            .gap(px(4. * s))
            .rounded(px(12. * s))
            .bg(theme.glass_bar)
            .border_1()
            .border_color(theme.hair_2)
            .text_size(px(12.5 * s))
            .font_weight(FontWeight::MEDIUM)
            .text_color(theme.ink_2);
        if let Some(layout) = &status.layout {
            pill = pill.child(chip(
                div()
                    .h(px(17. * s))
                    .px(px(5. * s))
                    .flex()
                    .items_center()
                    .rounded(px(5. * s))
                    .border_1()
                    .border_color(theme.ink_2)
                    .text_size(px(10.5 * s))
                    .font_weight(FontWeight::BOLD)
                    .child(SharedString::from(layout.clone()))
                    .into_any_element(),
            ));
        }
        let net = match status.network {
            Network::Wireless => Icon::Wifi,
            Network::Wired => Icon::Ethernet,
            Network::Offline => Icon::NetworkOff,
        };
        pill = pill.child(chip(icon_sized(net, 16. * s).into_any_element()));
        if let Some((percent, charging)) = status.battery {
            let path = format!("icons/battery-level-{percent}{}.svg", if charging { "-charging" } else { "" });
            pill = pill.child(chip(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6. * s))
                    .child(yozora_ui::IconElement::from_path(path, 18. * s))
                    .child(format!("{percent}%"))
                    .into_any_element(),
            ));
        }
        div()
            .absolute()
            .left(px(48. * s))
            .right(px(48. * s))
            .top(px(36. * s))
            .flex()
            .justify_between()
            .items_center()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10. * s))
                    .text_size(px(13. * s))
                    .text_color(theme.ink_2)
                    .child(icon_sized(Icon::Logo, 16. * s).text_color(theme.accent))
                    .child(div().font_weight(FontWeight::SEMIBOLD).text_color(theme.ink).child("夜空"))
                    .child("Locked"),
            )
            .child(pill)
    }

    fn auth(&self, s: f32, theme: &Theme, caps: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let lock = self.lock.read(cx);
        let auth = &lock.auth;
        let now = Instant::now();
        let phase = auth.phase;
        let checking = matches!(phase, Phase::Checking | Phase::Unlocked);
        let waiting = auth.waiting_for(now);
        let dots = auth.dots();
        let can_submit = auth.can_submit();
        let (tone, hint) = auth.hint(caps);
        let shakes = auth.shakes;
        let reduced = theme.motion.reduced;

        let avatar = {
            let a = div()
                .size(px(84. * s))
                .rounded_full()
                .overflow_hidden()
                .flex()
                .items_center()
                .justify_center()
                .bg(theme.accent)
                .text_color(theme.accent_ink)
                .text_size(px(30. * s))
                .font_weight(FontWeight::SEMIBOLD)
                .shadow(vec![ring(with_alpha(theme.glass_bar, 1.), 4. * s), ring(theme.hair_2, 5. * s)]);
            match &lock.user.picture {
                Some(p) => a.child(img(p.clone()).size_full().object_fit(ObjectFit::Cover)),
                None => a.child(SharedString::from(lock.user.initial())),
            }
        };

        let border = match phase {
            Phase::Wrong => theme.danger,
            Phase::Waiting => theme.hair_2,
            _ => theme.accent,
        };
        let mut shadows = shadow(theme);
        if !matches!(phase, Phase::Waiting) {
            let soft = if phase == Phase::Wrong { with_alpha(theme.danger, 0.18) } else { theme.accent_soft };
            shadows.insert(0, ring(soft, 4. * s));
        }

        let text: AnyElement = if let Some(w) = waiting {
            div()
                .text_color(theme.ink_3)
                .text_size(px(14.5 * s))
                .child(format!("Try again in {}", countdown(w)))
                .into_any_element()
        } else if dots == 0 {
            div().text_color(theme.ink_3).text_size(px(14.5 * s)).child("Password").into_any_element()
        } else {
            let shown = "•".repeat(dots.min(22)).chars().map(String::from).collect::<Vec<_>>().join("\u{2009}");
            div()
                .flex()
                .items_center()
                .text_size(px(15. * s))
                .when(checking, |d| d.opacity(0.5))
                .child(shown)
                .when(!checking, |d| d.child(div().ml(px(2. * s)).w(px(1.5 * s)).h(px(17. * s)).bg(theme.accent)))
                .into_any_element()
        };

        let go_icon = if checking {
            let spinner = icon_sized(Icon::Spinner, 18. * s);
            if reduced {
                spinner.into_any_element()
            } else {
                spinner
                    .with_animation("spin", Animation::new(Duration::from_millis(800)).repeat(), |el, t| el.rotate(t))
                    .into_any_element()
            }
        } else if waiting.is_some() {
            icon_sized(Icon::Clock, 16. * s).into_any_element()
        } else {
            icon_sized(Icon::ArrowRight, 16. * s).into_any_element()
        };
        let lit = can_submit || checking;
        let go = div()
            .id("go")
            .size(px(36. * s))
            .flex_none()
            .rounded_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(if lit { theme.accent } else { theme.fill_3 })
            .text_color(if lit { theme.accent_ink } else { theme.ink_3 })
            .when(can_submit, |d| d.cursor_pointer())
            .child(go_icon)
            .on_click(cx.listener(|this, _, _, cx| this.lock.update(cx, |lock, cx| lock.submit(cx))));

        let field = div()
            .relative()
            .w(px(340. * s))
            .h(px(48. * s))
            .flex()
            .items_center()
            .gap(px(10. * s))
            .pl(px(16. * s))
            .pr(px(6. * s))
            .rounded(px(24. * s))
            .bg(theme.glass_pop)
            .border_1()
            .border_color(border)
            .shadow(shadows)
            .when(waiting.is_some(), |d| d.opacity(0.55))
            .child(icon_sized(Icon::Lock, 16. * s).text_color(theme.ink_2))
            .child(div().flex_1().min_w_0().overflow_hidden().child(text))
            .child(go);
        let field = if shakes > 0 && !reduced {
            field
                .with_animation(("shake", shakes as usize), Animation::new(Duration::from_millis(420)), move |el, t| {
                    let x = -9. * (1. - t) * (t * std::f32::consts::PI * 5.).sin();
                    el.left(px(x * s))
                })
                .into_any_element()
        } else {
            field.into_any_element()
        };

        let (hint_color, hint_icon) = match tone {
            Tone::Plain => (theme.ink_2, None),
            Tone::Warn => (theme.warn, Some(Icon::CapsLock)),
            Tone::Bad => (theme.danger, Some(Icon::Alert)),
        };
        let hint = div()
            .mt(px(14. * s))
            .min_h(px(18. * s))
            .max_w(px(380. * s))
            .flex()
            .items_center()
            .justify_center()
            .gap(px(7. * s))
            .text_size(px(13. * s))
            .text_color(hint_color)
            .when_some(hint_icon, |d, i| d.child(icon_sized(i, 15. * s)))
            .child(SharedString::from(hint));

        div()
            .w(px(380. * s))
            .flex()
            .flex_col()
            .items_center()
            .child(avatar)
            .child(
                div()
                    .mt(px(16. * s))
                    .text_size(px(17. * s))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(SharedString::from(lock.user.name.clone())),
            )
            .child(div().mt(px(22. * s)).child(field))
            .child(hint)
    }

    fn media(&self, s: f32, theme: &Theme, media: services::Media, cx: &mut Context<Self>) -> impl IntoElement {
        let button = |id: &'static str, icon: Icon, cmd: MediaCmd, cx: &mut Context<Self>| {
            div()
                .id(id)
                .size(px(32. * s))
                .rounded(px(9. * s))
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .hover(|d| d.bg(theme.fill_2))
                .child(icon_sized(icon, 15. * s))
                .on_click(cx.listener(move |this, _, _, cx| {
                    let _ = this.lock.read(cx).media_cmds.try_send(cmd);
                }))
        };
        let art = div().size(px(48. * s)).flex_none().rounded(px(10. * s)).overflow_hidden().bg(theme.fill_2);
        let art = match media.art {
            Some(p) => art.child(img(p).size_full().object_fit(ObjectFit::Cover)).into_any_element(),
            None => art
                .flex()
                .items_center()
                .justify_center()
                .text_color(theme.ink_2)
                .child(icon_sized(Icon::Music, 20. * s))
                .into_any_element(),
        };
        div()
            .absolute()
            .left(px(48. * s))
            .bottom(px(44. * s))
            .w(px(340. * s))
            .flex()
            .items_center()
            .gap(px(12. * s))
            .p(px(10. * s))
            .rounded(px(16. * s))
            .bg(theme.glass_pop)
            .border_1()
            .border_color(theme.hair_2)
            .shadow(shadow(theme))
            .child(art)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(div().text_size(px(13. * s)).font_weight(FontWeight::SEMIBOLD).truncate().child(media.title))
                    .child(
                        div()
                            .mt(px(2. * s))
                            .text_size(px(12. * s))
                            .text_color(theme.ink_2)
                            .truncate()
                            .child(media.artist),
                    ),
            )
            .child(button("prev", Icon::Previous, MediaCmd::Previous, cx))
            .child(button("play", if media.playing { Icon::Pause } else { Icon::Play }, MediaCmd::PlayPause, cx))
            .child(button("next", Icon::Next, MediaCmd::Next, cx))
    }

    fn notes(&self, s: f32, theme: &Theme, n: usize) -> impl IntoElement {
        let what = if n == 1 { "1 notification".to_string() } else { format!("{n} notifications") };
        div().absolute().left_0().right_0().bottom(px(52. * s)).flex().justify_center().child(
            div()
                .h(px(36. * s))
                .pl(px(10. * s))
                .pr(px(14. * s))
                .flex()
                .items_center()
                .gap(px(9. * s))
                .rounded(px(18. * s))
                .bg(theme.glass_bar)
                .border_1()
                .border_color(theme.hair_2)
                .text_size(px(12.5 * s))
                .text_color(theme.ink_2)
                .child(icon_sized(Icon::Bell, 16. * s).text_color(theme.ink))
                .child(div().font_weight(FontWeight::SEMIBOLD).text_color(theme.ink).child(what))
                .child("· contents hidden until you unlock"),
        )
    }

    fn power(&self, s: f32, theme: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let lock = self.lock.read(cx);
        let entries = [
            ("suspend", Icon::Moon, "Suspend", Power::Suspend),
            ("restart", Icon::Reset, "Restart", Power::Reboot),
            ("shutdown", Icon::Power, "Shut down", Power::ShutDown),
        ];
        let confirm: Vec<bool> = entries.iter().map(|e| lock.confirming(e.3)).collect();
        let mut row = div().absolute().right(px(48. * s)).bottom(px(44. * s)).flex().gap(px(10. * s));
        for ((id, icon, label, action), confirming) in entries.into_iter().zip(confirm) {
            row = row.child(
                div()
                    .id(id)
                    .min_w(px(64. * s))
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(7. * s))
                    .cursor_pointer()
                    .text_size(px(11.5 * s))
                    .group(id)
                    .child(
                        div()
                            .size(px(48. * s))
                            .rounded_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .bg(if confirming { theme.danger } else { theme.glass_bar })
                            .text_color(if confirming { theme.accent_ink } else { theme.ink })
                            .border_1()
                            .border_color(theme.hair_2)
                            .group_hover(id, |d| d.bg(if confirming { theme.danger } else { theme.fill_3 }))
                            .child(icon_sized(icon, 18. * s)),
                    )
                    .child(div().text_color(if confirming { theme.danger } else { theme.ink_2 }).child(if confirming {
                        "Again to confirm"
                    } else {
                        label
                    }))
                    .on_click(
                        cx.listener(move |this, _, _, cx| this.lock.update(cx, |lock, cx| lock.power(action, cx))),
                    ),
            );
        }
        row
    }
}
