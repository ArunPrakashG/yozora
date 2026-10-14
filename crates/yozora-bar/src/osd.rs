//! The on-screen display: a capsule at the bottom of the focused monitor that shows volume,
//! the microphone, brightness, the output device and the keyboard layout as they change.
//!
//! Audio and layout changes are noticed from the services the bar already runs, so they show
//! whatever made them: a key, an app, `pactl` or the bar. Brightness has no change events and
//! comes from `yozora-bar osd brightness`. One capsule at a time: a new change replaces what it
//! shows and restarts its timer. The window exists only while the capsule is visible.

use std::time::{Duration, Instant};

use gpui::{
    AnyElement, App, AppContext, Bounds, Context, FontWeight, Global, IntoElement, ParentElement, Render, Styled,
    Subscription, Task, Window, WindowBackgroundAppearance, WindowBounds, WindowHandle, WindowKind, WindowOptions, div,
    layer_shell::*, point, prelude::*, px, relative, size,
};
use yozora_config::bar::OsdPosition;
use yozora_ui::{
    Icon, Theme, icon_sized,
    spring::{Spring, SpringConfig},
};

use crate::{
    app::{self, Config, Shell},
    modules::{Pop, sound},
    services::{
        Services,
        audio::{self, DeviceKind},
        hyprland::Keyboard,
    },
};

const WIDTH: f32 = 320.;
const HEIGHT: f32 = 56.;
/// Room around the capsule for its shadow and the rise.
const PAD_SIDE: f32 = 36.;
const PAD_TOP: f32 = 20.;
const PAD_BOTTOM: f32 = 60.;
/// From the bottom edge of the screen to the capsule.
const BOTTOM: f32 = 72.;
/// How far it rises while it fades in.
const RISE: f32 = 10.;
const FADE_IN: f32 = 0.16;
const LEAVE: Duration = Duration::from_millis(160);
const REDUCED: Duration = Duration::from_millis(120);
/// Above this the track runs to 150% and the extra turns warm.
const BOOST_MAX: f32 = 1.5;

/// What the capsule shows.
#[derive(Clone, Debug, PartialEq)]
pub enum Shown {
    Volume {
        level: f32,
        muted: bool,
    },
    /// The default output changed.
    Output {
        name: String,
        kind: DeviceKind,
        level: f32,
        muted: bool,
    },
    /// `toggled` when the change was muting or unmuting, which reads better as words.
    Mic {
        name: String,
        level: f32,
        muted: bool,
        toggled: bool,
    },
    Brightness(f32),
    Layout(Keyboard),
}

impl Shown {
    /// Values with the same track share it, so the fill springs between them instead of jumping.
    fn track(&self) -> Option<(u8, f32)> {
        match self {
            Shown::Volume { level, .. } | Shown::Output { level, .. } => Some((0, *level)),
            Shown::Mic { level, toggled: false, .. } => Some((1, *level)),
            Shown::Brightness(level) => Some((2, *level)),
            Shown::Mic { .. } | Shown::Layout(_) => None,
        }
    }

    fn audio(&self) -> bool {
        matches!(self, Shown::Volume { .. } | Shown::Output { .. } | Shown::Mic { .. })
    }
}

/// The audio facts the capsule reacts to, rounded to what it shows.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AudioKey {
    sink: String,
    sink_name: String,
    sink_kind: DeviceKind,
    volume: u32,
    muted: bool,
    source: String,
    source_name: String,
    mic: u32,
    mic_muted: bool,
}

impl AudioKey {
    pub fn of(state: &audio::State) -> Self {
        let (sink, source) = (state.sink(), state.source());
        Self {
            sink: sink.map(|d| d.name.clone()).unwrap_or_default(),
            sink_name: sink.map(|d| d.description.clone()).unwrap_or_default(),
            sink_kind: sink.map(|d| d.kind).unwrap_or_default(),
            volume: sink.map_or(0, |d| sound::percent(d.volume)),
            muted: sink.is_some_and(|d| d.muted),
            source: source.map(|d| d.name.clone()).unwrap_or_default(),
            source_name: source.map(|d| d.description.clone()).unwrap_or_default(),
            mic: source.map_or(0, |d| sound::percent(d.volume)),
            mic_muted: source.is_some_and(|d| d.muted),
        }
    }
}

/// What to show for a change from `prev` to `next`, if anything.
pub fn diff(prev: &AudioKey, next: &AudioKey) -> Option<Shown> {
    let level = next.volume as f32 / 100.;
    if next.sink != prev.sink && !prev.sink.is_empty() && !next.sink.is_empty() {
        return Some(Shown::Output { name: next.sink_name.clone(), kind: next.sink_kind, level, muted: next.muted });
    }
    if next.sink == prev.sink && (next.volume != prev.volume || next.muted != prev.muted) {
        return Some(Shown::Volume { level, muted: next.muted });
    }
    if next.source == prev.source && !next.source.is_empty() {
        let mic = |toggled| Shown::Mic {
            name: next.source_name.clone(),
            level: next.mic as f32 / 100.,
            muted: next.mic_muted,
            toggled,
        };
        if next.mic_muted != prev.mic_muted {
            return Some(mic(true));
        }
        if next.mic != prev.mic {
            return Some(mic(false));
        }
    }
    None
}

/// The open capsule, and what was last seen so only changes show.
#[derive(Default)]
pub struct Osd {
    window: Option<WindowHandle<OsdView>>,
    audio: Option<AudioKey>,
    keyboard: Option<Keyboard>,
    observing_audio: bool,
    observing_hyprland: bool,
    subscriptions: Vec<Subscription>,
}

impl Global for Osd {}

/// Starts watching the services the capsule reacts to. Safe to call again after they start.
pub fn observe(cx: &mut App) {
    if !cx.has_global::<Osd>() {
        cx.set_global(Osd::default());
    }
    if !cx.global::<Osd>().observing_audio
        && let Some(audio) = Services::get(cx).audio.clone()
    {
        let sub = cx.observe(&audio, |audio, cx| {
            let service = audio.read(cx);
            if !service.up() {
                return;
            }
            let key = AudioKey::of(&service.state);
            let prev = cx.global_mut::<Osd>().audio.replace(key.clone());
            // The first snapshot is the baseline, not a change.
            if let Some(shown) = prev.and_then(|p| diff(&p, &key)) {
                show(shown, cx);
            }
        });
        let osd = cx.global_mut::<Osd>();
        osd.observing_audio = true;
        osd.subscriptions.push(sub);
    }
    if !cx.global::<Osd>().observing_hyprland
        && let Some(hypr) = Services::get(cx).hyprland.clone()
    {
        let sub = cx.observe(&hypr, |hypr, cx| {
            let Some(kbd) = hypr.read(cx).state.keyboard.clone() else { return };
            let prev = cx.global_mut::<Osd>().keyboard.replace(kbd.clone());
            if prev.is_some_and(|p| p.layout != kbd.layout || p.index != kbd.index)
                && Config::get(cx).settings.bar.osd.keyboard_layout
            {
                show(Shown::Layout(kbd), cx);
            }
        });
        let osd = cx.global_mut::<Osd>();
        osd.observing_hyprland = true;
        osd.subscriptions.push(sub);
    }
}

/// The current value of `what` (volume, mic or layout), for `yozora-bar osd volume`.
pub fn current(what: &str, cx: &App) -> Result<Shown, String> {
    match what {
        "volume" | "mic" => {
            let audio = Services::get(cx).audio.as_ref().ok_or("sound isn't running")?.read(cx);
            let key = AudioKey::of(&audio.state);
            Ok(if what == "volume" {
                Shown::Volume { level: key.volume as f32 / 100., muted: key.muted }
            } else {
                Shown::Mic { name: key.source_name, level: key.mic as f32 / 100., muted: key.mic_muted, toggled: false }
            })
        }
        "layout" => Services::get(cx)
            .hyprland
            .as_ref()
            .and_then(|h| h.read(cx).state.keyboard.clone())
            .map(Shown::Layout)
            .ok_or_else(|| "no keyboard layout from Hyprland".into()),
        other => Err(format!("unknown OSD value {other:?}; try volume, mic, layout or brightness")),
    }
}

/// Whether a bar already shows the sound controls, so the capsule would repeat them.
fn sound_open(cx: &App) -> bool {
    Shell::bars(cx).into_iter().any(|bar| {
        bar.read(cx).is_ok_and(|b| b.popover_open(&Pop::Sound, cx) || b.popover_open(&Pop::ControlCenter, cx))
    })
}

/// Shows `shown` on the focused monitor, replacing what the capsule shows now.
pub fn show(shown: Shown, cx: &mut App) {
    if !cx.has_global::<Osd>() {
        cx.set_global(Osd::default());
    }
    let config = Config::get(cx).settings.bar.osd.clone();
    if !config.enabled || (shown.audio() && sound_open(cx)) {
        return;
    }
    let Some((monitor, display)) = app::focused_display(cx) else { return };
    tracing::debug!("osd: {shown:?} on {monitor}");
    if let Some(handle) = cx.global::<Osd>().window {
        let same = handle.read(cx).is_ok_and(|v| v.monitor == monitor && !v.done);
        if same {
            let _ = handle.update(cx, |view, _, cx| view.show(shown, cx));
            return;
        }
        // Focus moved to another monitor: the capsule follows it.
        let _ = handle.update(cx, |_, window, _| window.remove_window());
        cx.global_mut::<Osd>().window = None;
    }
    let (anchor, margin) = match config.position {
        OsdPosition::Bottom => (Anchor::BOTTOM, (px(0.), px(0.), px(BOTTOM - PAD_BOTTOM), px(0.))),
        // A zero exclusive zone keeps it clear of the bar's.
        OsdPosition::Top => (Anchor::TOP, (px(0.), px(0.), px(0.), px(0.))),
    };
    let timeout = Duration::from_millis(config.timeout_ms as u64);
    let opened = cx.open_window(
        WindowOptions {
            titlebar: None,
            focus: false,
            window_bounds: Some(WindowBounds::Windowed(Bounds {
                origin: point(px(0.), px(0.)),
                size: size(px(WIDTH + PAD_SIDE * 2.), px(HEIGHT + PAD_TOP + PAD_BOTTOM)),
            })),
            app_id: Some("yozora-bar".into()),
            window_background: WindowBackgroundAppearance::Transparent,
            display_id: Some(display),
            // Never focused; without this GPUI caps it at 30 fps.
            inactive_frame_interval: None,
            kind: WindowKind::LayerShell(LayerShellOptions {
                namespace: "yozora-osd".into(),
                layer: Layer::Overlay,
                anchor,
                exclusive_zone: None,
                margin: Some(margin),
                keyboard_interactivity: KeyboardInteractivity::None,
                ..Default::default()
            }),
            ..Default::default()
        },
        |window, cx| {
            // It never takes a click.
            window.set_input_region(Some(&[]));
            cx.new(|_| OsdView::new(shown, monitor, config.position, timeout))
        },
    );
    match opened {
        Ok(handle) => cx.global_mut::<Osd>().window = Some(handle),
        Err(e) => tracing::warn!("could not open the OSD: {e:#}"),
    }
}

pub struct OsdView {
    shown: Shown,
    monitor: String,
    position: OsdPosition,
    timeout: Duration,
    appear: Spring,
    fill: Spring,
    opened: Instant,
    hide_at: Instant,
    leaving: Option<Instant>,
    last: Instant,
    timer: Option<Task<()>>,
    /// Set once the window is being removed.
    done: bool,
}

impl OsdView {
    fn new(shown: Shown, monitor: String, position: OsdPosition, timeout: Duration) -> Self {
        let mut appear = Spring::new(0.0, SpringConfig::POPOVER);
        appear.set_target(1.0);
        let level = shown.track().map_or(0.0, |t| t.1);
        let now = Instant::now();
        Self {
            shown,
            monitor,
            position,
            timeout,
            appear,
            fill: Spring::new(level, SpringConfig::POPOVER),
            opened: now,
            hide_at: now + timeout,
            leaving: None,
            last: now,
            timer: None,
            done: false,
        }
    }

    fn show(&mut self, shown: Shown, cx: &mut Context<Self>) {
        match (self.shown.track(), shown.track()) {
            (Some((a, _)), Some((b, level))) if a == b => self.fill.set_target(level),
            (_, Some((_, level))) => self.fill = Spring::new(level, SpringConfig::POPOVER),
            _ => {}
        }
        if let Some(since) = self.leaving.take() {
            // Caught on its way out: fade back in from where it got to.
            let left = (since.elapsed().as_secs_f32() / LEAVE.as_secs_f32()).min(1.0);
            let back = Duration::from_secs_f32((1.0 - left) * FADE_IN);
            self.opened = Instant::now().checked_sub(back).unwrap_or_else(Instant::now);
        }
        self.shown = shown;
        self.hide_at = Instant::now() + self.timeout;
        cx.notify();
    }

    /// Opacity and vertical offset for this frame, scheduling the next one.
    fn advance(&mut self, reduced: bool, window: &mut Window, cx: &mut Context<Self>) -> (f32, f32) {
        let now = Instant::now();
        let dt = (now - self.last).min(Duration::from_millis(100));
        self.last = now;
        // Bottom rises up into place; top drops down from the bar.
        let dir = if self.position == OsdPosition::Bottom { 1.0 } else { -1.0 };

        if let Some(since) = self.leaving {
            let span = if reduced { REDUCED } else { LEAVE };
            let t = ((now - since).as_secs_f32() / span.as_secs_f32()).min(1.0);
            if t >= 1.0 {
                self.done = true;
                let me = window.window_handle().window_id();
                cx.defer(move |cx| {
                    let osd = cx.global_mut::<Osd>();
                    if osd.window.is_some_and(|w| w.window_id() == me) {
                        osd.window = None;
                    }
                });
                yozora_ui::close_after_frame(window, cx);
                return (0.0, 0.0);
            }
            window.request_animation_frame();
            let eased = t * t;
            return (1.0 - eased, if reduced { 0.0 } else { eased * 6.0 * dir });
        }

        let elapsed = (now - self.opened).as_secs_f32();
        let (opacity, offset, moving) = if reduced {
            self.fill = Spring::new(self.fill.target(), SpringConfig::POPOVER);
            let o = (elapsed / REDUCED.as_secs_f32()).min(1.0);
            (o, 0.0, o < 1.0)
        } else {
            let appear_settled = self.appear.step(dt);
            let fill_settled = self.fill.step(dt);
            let o = (elapsed / FADE_IN).min(1.0);
            let eased = 1.0 - (1.0 - o).powi(3);
            (eased, (1.0 - self.appear.value()) * RISE * dir, !(appear_settled && fill_settled && o >= 1.0))
        };

        if moving {
            window.request_animation_frame();
        } else if now >= self.hide_at {
            self.leaving = Some(now);
            window.request_animation_frame();
        } else if self.timer.is_none() {
            let wait = self.hide_at - now;
            self.timer = Some(cx.spawn(async move |this, cx| {
                cx.background_executor().timer(wait).await;
                let _ = this.update(cx, |v, cx| {
                    v.timer = None;
                    cx.notify();
                });
            }));
        }
        (opacity, offset)
    }
}

/// A 6 px track filled to `level`, running to 150% with the extra warm when boosted.
fn track(level: f32, target: f32, off: bool, theme: &Theme) -> AnyElement {
    let boosted = target > 1.0 + 0.005;
    let max = if boosted { BOOST_MAX } else { 1.0 };
    let level = level.clamp(0.0, max);
    let fill = if off { theme.ink_3 } else { theme.accent };
    let mut t = div().relative().flex_1().h(px(6.)).rounded(px(3.)).bg(theme.fill_3).overflow_hidden().child(
        div()
            .absolute()
            .left_0()
            .top_0()
            .bottom_0()
            .w(relative(level.min(1.0) / max))
            .rounded(px(3.))
            .bg(fill)
            .when(off, |d| d.opacity(0.45)),
    );
    if level > 1.0 {
        t = t.child(
            div()
                .absolute()
                .left(relative(1.0 / max))
                .top_0()
                .bottom_0()
                .w(relative((level - 1.0) / max))
                .rounded_r(px(3.))
                .bg(theme.warn)
                .when(off, |d| d.opacity(0.45)),
        );
    }
    t.into_any_element()
}

fn value_label(text: String, off: bool, theme: &Theme) -> impl IntoElement {
    div()
        .w(px(52.))
        .flex_none()
        .flex()
        .justify_end()
        .text_size(px(13.5))
        .font_weight(if off { FontWeight::MEDIUM } else { FontWeight::SEMIBOLD })
        .text_color(if off { theme.ink_2 } else { theme.ink })
        .child(text)
}

fn words(title: String, detail: Option<String>, theme: &Theme) -> gpui::Div {
    div()
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .child(div().text_size(px(13.5)).font_weight(FontWeight::SEMIBOLD).truncate().child(title))
        .when_some(detail, |d, detail| {
            d.child(div().mt(px(1.)).text_size(px(11.5)).text_color(theme.ink_2).truncate().child(detail))
        })
}

impl Render for OsdView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::global(cx).clone();
        if self.last == self.opened {
            tracing::debug!("osd: first frame {:?} after opening", self.opened.elapsed());
        }
        let (opacity, offset) = self.advance(theme.motion.reduced, window, cx);
        let (level, target) = (self.fill.value(), self.fill.target());
        let pct = |v: f32| sound::percent(v).to_string();

        // The chip's look: normal, off (muted) or alarm (mic muted).
        let (icon, chip_bg, chip_fg, body): (Icon, gpui::Rgba, gpui::Rgba, Vec<AnyElement>) = match &self.shown {
            Shown::Volume { muted, .. } => {
                let icon = if *muted || target <= 0.001 {
                    Icon::VolumeMute
                } else if target < 0.5 {
                    Icon::VolumeLow
                } else {
                    Icon::VolumeHigh
                };
                let (bg, fg) = if *muted { (theme.fill_3, theme.ink_2) } else { (theme.accent_soft, theme.accent) };
                let label = if *muted { "Muted".to_string() } else { pct(target) };
                (
                    icon,
                    bg,
                    fg,
                    vec![track(level, target, *muted, &theme), value_label(label, *muted, &theme).into_any_element()],
                )
            }
            Shown::Output { name, kind, muted, .. } => {
                let (bg, fg) = if *muted { (theme.fill_3, theme.ink_2) } else { (theme.accent_soft, theme.accent) };
                let label = if *muted { "Muted".to_string() } else { pct(target) };
                (
                    sound::kind_icon(*kind),
                    bg,
                    fg,
                    vec![
                        words(name.clone(), None, &theme)
                            .child(div().mt(px(6.)).flex().child(track(level, target, *muted, &theme)))
                            .into_any_element(),
                        value_label(label, *muted, &theme).into_any_element(),
                    ],
                )
            }
            Shown::Mic { name, muted, toggled: true, .. } => {
                let title = if *muted { "Microphone off" } else { "Microphone on" };
                let (icon, bg, fg) = if *muted {
                    (Icon::MicOff, gpui::Rgba { a: 0.16, ..theme.danger }, theme.danger)
                } else {
                    (Icon::Mic, theme.accent_soft, theme.accent)
                };
                let detail = (!name.is_empty()).then(|| name.clone());
                (icon, bg, fg, vec![words(title.into(), detail, &theme).into_any_element()])
            }
            Shown::Mic { muted, .. } => {
                let (bg, fg) = if *muted {
                    (gpui::Rgba { a: 0.16, ..theme.danger }, theme.danger)
                } else {
                    (theme.accent_soft, theme.accent)
                };
                let label = if *muted { "Off".to_string() } else { pct(target) };
                (
                    if *muted { Icon::MicOff } else { Icon::Mic },
                    bg,
                    fg,
                    vec![track(level, target, *muted, &theme), value_label(label, *muted, &theme).into_any_element()],
                )
            }
            Shown::Brightness(_) => (
                Icon::Sun,
                theme.accent_soft,
                theme.accent,
                vec![track(level, target, false, &theme), value_label(pct(target), false, &theme).into_any_element()],
            ),
            Shown::Layout(k) => {
                let detail = if k.count > 1 {
                    format!("{} of {} · Keyboard layout", k.index + 1, k.count)
                } else {
                    "Keyboard layout".into()
                };
                (
                    Icon::Keyboard,
                    theme.accent_soft,
                    theme.accent,
                    vec![
                        words(k.layout.clone(), Some(detail), &theme).into_any_element(),
                        value_label(k.code.clone(), false, &theme).into_any_element(),
                    ],
                )
            }
        };

        div().size_full().font_family(theme.font.clone()).text_color(theme.ink).child(
            div()
                .absolute()
                .left(px(PAD_SIDE))
                .top(px(PAD_TOP + offset))
                .w(px(WIDTH))
                .h(px(HEIGHT))
                .opacity(opacity.clamp(0.0, 1.0))
                .rounded(px(HEIGHT / 2.))
                .bg(theme.glass_pop)
                .border_1()
                .border_color(theme.hair_2)
                .shadow(theme.popover_shadow())
                .pl(px(10.))
                .pr(px(18.))
                .flex()
                .items_center()
                .gap(px(12.))
                .child(
                    div()
                        .size(px(36.))
                        .flex_none()
                        .rounded_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .bg(chip_bg)
                        .child(icon_sized(icon, 18.).text_color(chip_fg)),
                )
                .children(body),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key() -> AudioKey {
        AudioKey {
            sink: "speakers".into(),
            sink_name: "Speakers".into(),
            sink_kind: DeviceKind::Speakers,
            volume: 60,
            muted: false,
            source: "mic".into(),
            source_name: "Microphone".into(),
            mic: 40,
            mic_muted: false,
        }
    }

    #[test]
    fn nothing_changed_shows_nothing() {
        assert_eq!(diff(&key(), &key()), None);
    }

    #[test]
    fn volume_and_mute() {
        let next = AudioKey { volume: 65, ..key() };
        assert_eq!(diff(&key(), &next), Some(Shown::Volume { level: 0.65, muted: false }));
        let next = AudioKey { muted: true, ..key() };
        assert_eq!(diff(&key(), &next), Some(Shown::Volume { level: 0.6, muted: true }));
    }

    #[test]
    fn output_switch_names_the_device() {
        let next = AudioKey {
            sink: "bt".into(),
            sink_name: "Headphones".into(),
            sink_kind: DeviceKind::Headphones,
            volume: 48,
            ..key()
        };
        assert_eq!(
            diff(&key(), &next),
            Some(Shown::Output { name: "Headphones".into(), kind: DeviceKind::Headphones, level: 0.48, muted: false })
        );
        // The first device appearing isn't a switch.
        assert_eq!(diff(&AudioKey { sink: String::new(), ..key() }, &next), None);
    }

    #[test]
    fn microphone() {
        let next = AudioKey { mic_muted: true, ..key() };
        assert!(matches!(diff(&key(), &next), Some(Shown::Mic { muted: true, toggled: true, .. })));
        let next = AudioKey { mic: 55, ..key() };
        assert!(matches!(diff(&key(), &next), Some(Shown::Mic { toggled: false, .. })));
    }

    #[test]
    fn tracks_are_shared_by_kind() {
        let v = Shown::Volume { level: 0.5, muted: false };
        let o = Shown::Output { name: "x".into(), kind: DeviceKind::Speakers, level: 0.7, muted: false };
        assert_eq!(v.track().unwrap().0, o.track().unwrap().0);
        assert_ne!(v.track().unwrap().0, Shown::Brightness(0.5).track().unwrap().0);
        assert_eq!(Shown::Mic { name: String::new(), level: 0., muted: true, toggled: true }.track(), None);
    }
}
