//! The on-screen display: a capsule on the focused monitor that shows volume, the microphone,
//! brightness, the output device and the keyboard layout as they change.
//!
//! Audio and layout changes are noticed from the services the shell already runs, so they show
//! whatever made them: a key, an app, `pactl` or the sound popover. Brightness has no change
//! events and comes from `yozora-shell osd brightness`. One capsule at a time: a new change
//! replaces what it shows and restarts its timer. Where it sits, whether it shows and how long it
//! stays come from `[osd]` in `bar.toml`.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime};

use rune::tok::TextStyle;
use rune::{Color, Edge, El, Transition, Vec2, col, row, text};
use yozora_config::bar::{OsdConfig, OsdPosition};
use yozora_config::paths;
use yozora_services::audio::{self, DeviceKind};
use yozora_services::brightness;
use yozora_services::hyprland::Keyboard;
use yozora_style::{Entrance, Fx, Look, Motion, Tokens};

use crate::kit::{self, fig, glyph_el, label};
use crate::shared::Shared;
use crate::stage::Pop;

/// A capsule that only tracks a level is this wide; one with words is wider.
const W: f32 = 240.0;
const WIDE: f32 = 304.0;
const H: f32 = 48.0;
const PAD: f32 = 18.0;
const GAP: f32 = 14.0;
/// The number, or `Muted`, on the right.
const VALUE_W: f32 = 40.0;
/// From the bottom edge of the screen to the capsule.
const BOTTOM: f32 = 72.0;
/// How far a capsule travels while it comes in.
const RISE: f32 = 10.0;

/// What the capsule shows.
#[derive(Clone, Debug, PartialEq)]
pub enum Shown {
    Volume { level: f32, muted: bool },
    /// The default output changed.
    Output { name: String, kind: DeviceKind, level: f32, muted: bool },
    /// `toggled` when the change was muting or unmuting, which reads better as words.
    Mic { name: String, level: f32, muted: bool, toggled: bool },
    Brightness(f32),
    Layout(Keyboard),
}

impl Shown {
    fn audio(&self) -> bool {
        matches!(self, Shown::Volume { .. } | Shown::Output { .. } | Shown::Mic { .. })
    }
}

/// A capsule on a stage: what it shows, until when, and where.
#[derive(Clone, Debug)]
pub struct Slot {
    pub shown: Shown,
    pub until: Instant,
    pub position: OsdPosition,
}

fn pct(v: f32) -> u32 {
    (v * 100.0).round().max(0.0) as u32
}

/// The audio facts the capsule reacts to, rounded to what it shows.
#[derive(Clone, Debug, Default, PartialEq)]
struct AudioKey {
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
    fn of(state: &audio::State) -> Self {
        let (sink, source) = (state.sink(), state.source());
        Self {
            sink: sink.map(|d| d.name.clone()).unwrap_or_default(),
            sink_name: sink.map(|d| d.description.clone()).unwrap_or_default(),
            sink_kind: sink.map(|d| d.kind).unwrap_or_default(),
            volume: sink.map_or(0, |d| pct(d.volume)),
            muted: sink.is_some_and(|d| d.muted),
            source: source.map(|d| d.name.clone()).unwrap_or_default(),
            source_name: source.map(|d| d.description.clone()).unwrap_or_default(),
            mic: source.map_or(0, |d| pct(d.volume)),
            mic_muted: source.is_some_and(|d| d.muted),
        }
    }
}

/// What to show for a change from `prev` to `next`, if anything.
fn diff(prev: &AudioKey, next: &AudioKey) -> Option<Shown> {
    let level = next.volume as f32 / 100.0;
    if next.sink != prev.sink && !prev.sink.is_empty() && !next.sink.is_empty() {
        return Some(Shown::Output { name: next.sink_name.clone(), kind: next.sink_kind, level, muted: next.muted });
    }
    if next.sink == prev.sink && (next.volume != prev.volume || next.muted != prev.muted) {
        return Some(Shown::Volume { level, muted: next.muted });
    }
    if next.source == prev.source && !next.source.is_empty() {
        let mic = |toggled| Shown::Mic { name: next.source_name.clone(), level: next.mic as f32 / 100.0, muted: next.mic_muted, toggled };
        if next.mic_muted != prev.mic_muted {
            return Some(mic(true));
        }
        if next.mic != prev.mic {
            return Some(mic(false));
        }
    }
    None
}

#[derive(Default)]
struct Inner {
    audio: Option<AudioKey>,
    keyboard: Option<Keyboard>,
    /// What each monitor's stage has open, so a volume change doesn't repeat the sound controls.
    popups: HashMap<String, Option<Pop>>,
    config: Option<(Option<SystemTime>, OsdConfig)>,
    /// Stands in for the backlight under `YZ_FAKE=brightness`.
    fake_brightness: Option<f32>,
}

/// Watches the services for changes worth showing and puts the capsule on the focused monitor.
#[derive(Default)]
pub struct Hub {
    inner: Mutex<Inner>,
}

impl Hub {
    /// `[osd]` from `bar.toml`, read again when the file changes.
    fn config(&self) -> OsdConfig {
        let stamp = std::fs::metadata(paths::config_dir().join(yozora_config::bar::FILE)).and_then(|m| m.modified()).ok();
        let mut g = self.inner.lock().unwrap();
        if let Some((seen, c)) = &g.config && *seen == stamp {
            return c.clone();
        }
        let c = match yozora_config::bar::load(&paths::config_dir()) {
            Ok(l) => l.value.bar.osd,
            Err(e) => {
                tracing::warn!("osd: {e:#}");
                OsdConfig::default()
            }
        };
        g.config = Some((stamp, c.clone()));
        c
    }

    /// Records what a monitor's stage has open. Called as each stage draws.
    pub fn note_popover(&self, output: &str, pop: Option<Pop>) {
        self.inner.lock().unwrap().popups.insert(output.to_owned(), pop);
    }

    /// Whether the sound controls are on screen, which already show what the capsule would.
    fn sound_open(&self) -> bool {
        self.inner.lock().unwrap().popups.values().any(|p| matches!(p, Some(Pop::Sound | Pop::Control)))
    }

    /// Looks for a change in audio or keyboard layout since the last look. Runs on every redraw
    /// request, so it only reads snapshots.
    pub fn observe(&self, sh: &Shared) {
        let (audio, keyboard) = {
            let svc = sh.services.lock().unwrap();
            let audio = svc.audio.as_ref().filter(|a| a.up()).map(|a| AudioKey::of(&a.read().state));
            let keyboard = svc.hyprland.as_ref().and_then(|h| h.read().state.keyboard.clone());
            (audio, keyboard)
        };
        let (mut heard, mut layout) = (None, None);
        {
            let mut g = self.inner.lock().unwrap();
            if let Some(key) = audio {
                // The first snapshot is the baseline, not a change.
                if let Some(prev) = g.audio.replace(key.clone()) {
                    heard = diff(&prev, &key);
                }
            }
            if let Some(k) = keyboard {
                if g.keyboard.replace(k.clone()).is_some_and(|p| p.layout != k.layout || p.index != k.index) {
                    layout = Some(Shown::Layout(k));
                }
            }
        }
        if let Some(shown) = layout.or(heard) {
            self.show(sh, shown, true);
        }
    }

    /// Shows `shown` on the focused monitor, replacing what the capsule shows now. `watched`
    /// is a change the shell noticed itself, as opposed to one somebody asked to see.
    pub fn show(&self, sh: &Shared, shown: Shown, watched: bool) {
        let cfg = self.config();
        if !cfg.enabled || (shown.audio() && self.sound_open()) || (watched && matches!(shown, Shown::Layout(_)) && !cfg.keyboard_layout) {
            return;
        }
        tracing::debug!("osd: {shown:?}");
        let slot = Slot { shown, until: Instant::now() + Duration::from_millis(cfg.timeout_ms as u64), position: cfg.position };
        let stages = sh.stage_handles();
        let focused = crate::desktop::focused_output(sh);
        let target = stages.iter().position(|(n, _)| Some(n) == focused.as_ref()).or(if stages.is_empty() { None } else { Some(0) });
        for (i, (_, stage)) in stages.into_iter().enumerate() {
            if Some(i) == target {
                let slot = slot.clone();
                stage.update(move |st, _| st.show_osd(slot));
            } else {
                // Focus moved to another monitor: the capsule follows it.
                stage.update_if(|st, _| st.hide_osd());
            }
        }
    }

    /// The current value of `what` (volume, mic or layout), for `osd volume`.
    pub fn current(&self, sh: &Shared, what: &str) -> Result<Shown, String> {
        let svc = sh.services.lock().unwrap();
        match what {
            "volume" | "mic" => {
                let audio = svc.audio.as_ref().ok_or("sound isn't running")?;
                let key = AudioKey::of(&audio.read().state);
                Ok(if what == "volume" {
                    Shown::Volume { level: key.volume as f32 / 100.0, muted: key.muted }
                } else {
                    Shown::Mic { name: key.source_name, level: key.mic as f32 / 100.0, muted: key.mic_muted, toggled: false }
                })
            }
            "layout" => svc
                .hyprland
                .as_ref()
                .and_then(|h| h.read().state.keyboard.clone())
                .map(Shown::Layout)
                .ok_or_else(|| "no keyboard layout from Hyprland".into()),
            other => Err(format!("unknown OSD value {other:?}; try volume, mic, layout or brightness")),
        }
    }

    /// Changes the brightness by `arg` (`+5%`, `-5%`, `40%`, or nothing to just look), shows it,
    /// and returns the new level. Goes through logind; `YZ_FAKE=brightness` keeps a made-up level
    /// instead, for machines without a backlight.
    pub fn brightness(&self, sh: &Shared, arg: Option<&str>) -> Result<f32, String> {
        let change = brightness::parse(arg)?;
        let fake = sh.services.lock().unwrap().fakes.has("brightness");
        let level = if fake {
            let mut g = self.inner.lock().unwrap();
            let now = g.fake_brightness.unwrap_or(0.6);
            let next = brightness::target(now, change);
            g.fake_brightness = Some(next);
            next
        } else {
            brightness::apply(change).map_err(|e| format!("{e:#}"))?
        };
        self.show(sh, Shown::Brightness(level), false);
        Ok(level)
    }
}

// ---- drawing ----

fn words(t: &Tokens, s: &str) -> String {
    if t.look == Look::Phosphor { s.to_lowercase() } else { s.to_owned() }
}

fn kind_glyph(kind: DeviceKind) -> &'static str {
    match kind {
        DeviceKind::Speakers => "speaker",
        DeviceKind::Headphones => "phones",
        DeviceKind::Display => "monitor",
        DeviceKind::Microphone => "mic",
    }
}

fn volume_glyph(level: f32, muted: bool) -> &'static str {
    if muted || level <= 0.001 {
        "vol0"
    } else if level < 0.5 {
        "vol1"
    } else {
        "vol"
    }
}

fn shadow_color(t: &Tokens) -> Color {
    match t.look {
        Look::Prism => Color { r: 0.16, g: 0.2, b: 0.47, a: 0.35 },
        _ if t.dark => Color { r: 0.0, g: 0.0, b: 0.0, a: 0.4 },
        _ => Color { r: 0.0, g: 0.0, b: 0.0, a: 0.18 },
    }
}

/// The right-hand number, or `Muted`.
fn value(t: &Tokens, s: String, off: bool, hot: bool) -> El {
    let ink = if hot { t.alert } else if off { t.ink3 } else { t.ink2 };
    row().w(VALUE_W).justify_end().child(fig(t, 13.0, 500.0, s, ink)).key("value")
}

/// A line of the capsule's words: a title over a detail.
fn lines(t: &Tokens, title: String, detail: Option<String>, width: f32) -> El {
    col().w(width).gap(1.0).key("words")
        .child(label(t, 13.0, 600.0, title, t.ink).truncate())
        .children(detail.map(|d| label(t, 12.0, 400.0, d, t.ink2).truncate()))
}

fn track(t: &Tokens, w: f32, level: f32, off: bool) -> El {
    crate::sound::track(t, w, if off { 0.0 } else { level }, true).key("track")
}

/// The capsule's width and its contents.
fn content(t: &Tokens, shown: &Shown) -> (f32, Vec<El>) {
    let ink = |off: bool| if off { t.ink3 } else { t.ink };
    let icon = |name: &str, color: Color| glyph_el(t, name, 18.0, color).key("glyph");
    let bar = W - 2.0 * PAD - 18.0 - 2.0 * GAP - VALUE_W;
    match shown {
        Shown::Volume { level, muted } => {
            let num = if *muted { words(t, "Muted") } else { pct(*level).to_string() };
            (W, vec![icon(volume_glyph(*level, *muted), ink(*muted)), track(t, bar, *level, *muted), value(t, num, *muted, *level > 1.005)])
        }
        Shown::Brightness(level) => (W, vec![icon("sun", t.ink), track(t, bar, *level, false), value(t, pct(*level).to_string(), false, false)]),
        Shown::Mic { level, muted, toggled: false, .. } => {
            let num = if *muted { words(t, "Off") } else { pct(*level).to_string() };
            (W, vec![icon(if *muted { "micoff" } else { "mic" }, if *muted { t.alert } else { t.ink }), track(t, bar, *level, *muted), value(t, num, *muted, false)])
        }
        Shown::Output { name, kind, level, muted } => {
            let num = if *muted { words(t, "Muted") } else { pct(*level).to_string() };
            let text_w = WIDE - 2.0 * PAD - 18.0 - 2.0 * GAP - VALUE_W;
            let body = col().w(text_w).gap(6.0).key("words")
                .child(label(t, 13.0, 600.0, name.clone(), t.ink).truncate())
                .child(track(t, text_w, *level, *muted));
            (WIDE, vec![icon(kind_glyph(*kind), ink(*muted)), body, value(t, num, *muted, *level > 1.005)])
        }
        Shown::Mic { name, muted, toggled: true, .. } => {
            let title = words(t, if *muted { "Microphone off" } else { "Microphone on" });
            let detail = (!name.is_empty()).then(|| name.clone());
            let text_w = WIDE - 2.0 * PAD - 18.0 - GAP;
            (WIDE, vec![icon(if *muted { "micoff" } else { "mic" }, if *muted { t.alert } else { t.ink }), lines(t, title, detail, text_w)])
        }
        Shown::Layout(k) => {
            let detail = if k.count > 1 { format!("{} of {} \u{b7} Keyboard layout", k.index + 1, k.count) } else { "Keyboard layout".into() };
            let text_w = WIDE - 2.0 * PAD - 18.0 - 2.0 * GAP - VALUE_W;
            let code = text(k.code.clone()).font(TextStyle::new(13.0, 600.0).family(t.fonts.fig)).color(t.ink);
            (WIDE, vec![icon("kbd", t.ink), lines(t, k.layout.clone(), Some(words(t, &detail)), text_w), row().w(VALUE_W).justify_end().child(code).key("value")])
        }
    }
}

/// How the capsule arrives and leaves. It comes in from the edge it sits against, in the look's
/// own motion, on the springs the panels use.
fn arrive(t: &Tokens, fx: Fx, bottom: bool) -> Transition {
    let m = Motion::of(t.look, fx);
    if m.open == 0.0 {
        return Transition::none();
    }
    let springs = kit::enter(t, fx, 0.0, 1.0).springs;
    let from = Vec2 { x: 0.5, y: if bottom { 1.0 } else { 0.0 } };
    let rise = if bottom { RISE } else { -RISE };
    let base = match m.entrance {
        Entrance::Bud => Transition::scale_anchor(0.6, from).allow_small_scale().combined(Transition::offset(0.0, rise)),
        Entrance::Unfold | Entrance::Scan => Transition::reveal(if bottom { Edge::Bottom } else { Edge::Top }),
        Entrance::Iris => Transition::scale_anchor(0.6, from).allow_small_scale().combined(Transition::blur(6.0)),
    };
    base.combined(Transition::opacity()).springs(springs)
}

/// The capsule, placed. `anchor_x` is the volume icon on the bar, which a capsule under the bar
/// sits beneath.
pub fn view(t: &Tokens, fx: Fx, slot: &Slot, anchor_x: f32, screen_w: f32) -> El {
    let (w, body) = content(t, &slot.shown);
    let bottom = slot.position == OsdPosition::Bottom;
    let mut c = row().w(w).h(H).px(PAD).gap(GAP).center_y().radius(t.radii.pill).fill(t.surf).children(body);
    c = match t.look {
        Look::Folio | Look::Phosphor => c.border(1.0, t.ink),
        _ => c.border(1.0, t.rule).shadow(14.0, 40.0, shadow_color(t)),
    };
    let c = c.key("osd").transition(arrive(t, fx, bottom));
    if bottom {
        c.pin(Some(((screen_w - w) / 2.0).max(0.0)), None, None, Some(BOTTOM))
    } else {
        let left = (anchor_x - w / 2.0).clamp(crate::bar::SIDE, (screen_w - w - crate::bar::SIDE).max(crate::bar::SIDE));
        c.pin(Some(left), Some(t.panel_top), None, None)
    }
}
