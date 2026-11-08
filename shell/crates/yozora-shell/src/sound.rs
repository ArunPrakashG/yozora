//! The Sound popover: output volume and device, input volume. Its track is also what the
//! now-playing popover draws progress with.

use std::f32::consts::FRAC_PI_2;

use rune::tok::TextStyle;
use rune::{Color, Drag, El, Tx, col, el, row, text};
use yozora_services::audio::{Cmd, Device, DeviceKind};
use yozora_style::{Look, Tokens};

use crate::kit::{cell, glyph_el, label, panel};
use crate::stage::Stage;

pub const W: f32 = 360.0;
/// A volume track: the panel's inside, the row's padding, the mute button and the gap after it.
const TRACK_W: f32 = W - 36.0 - 20.0 - 28.0 - 14.0;
pub(crate) const CLEAR: Color = Color { r: 0.0, g: 0.0, b: 0.0, a: 0.0 };

pub(crate) fn alpha(c: Color, a: f32) -> Color {
    Color { a, ..c }
}

/// A progress or volume track, filled to `v`. `thin` is the slim one under a song.
pub(crate) fn track(t: &Tokens, w: f32, v: f32, thin: bool) -> El {
    let v = v.clamp(0.0, 1.0);
    let x = w * v;
    match t.look {
        Look::Folio => {
            let mut e = el().w(w).h(20.0)
                .child(el().pin(Some(0.0), Some(9.5), Some(0.0), None).h(1.0).fill(t.ink))
                .child(el().pin(Some(0.0), Some(8.0), None, None).size(x, 4.0).fill(t.ink));
            for i in 0..10 {
                e = e.child(el().pin(Some(w * i as f32 / 10.0), Some(13.0), None, None).size(1.0, 7.0).fill(t.ink3));
            }
            e.child(el().pin(Some(x - 5.0), Some(0.0), None, None).size(10.0, 20.0).fill(t.acc))
        }
        Look::Phosphor => {
            let h = if thin { 10.0 } else { 16.0 };
            let n = ((w + 3.0) / 10.0) as usize;
            let on = (v * n as f32).round() as usize;
            row().w(w).h(h).gap(3.0).children((0..n).map(|i| el().size(7.0, h).fill(if i < on { t.ink } else { t.ink3 })))
        }
        Look::Prism => {
            let (h, k) = if thin { (6.0, 14.0) } else { (24.0, 24.0) };
            let white = Color { r: 1.0, g: 1.0, b: 1.0, a: 1.0 };
            let light = Color { r: t.acc.r + (1.0 - t.acc.r) * 0.4, g: t.acc.g + (1.0 - t.acc.g) * 0.4, b: t.acc.b + (1.0 - t.acc.b) * 0.4, a: 1.0 };
            el().w(w).h(h).radius(t.radii.pill).fill(alpha(t.ink, 0.07))
                .child(el().pin(Some(0.0), Some(0.0), None, None).size(x, h).radius(t.radii.pill).linear_gradient(FRAC_PI_2, [(0.0, light), (1.0, t.acc)]))
                .child(
                    el().pin(Some(x - k / 2.0), Some((h - k) / 2.0), None, None).size(k, k).radius(t.radii.pill).fill(white)
                        .shadow(2.0, 8.0, Color { r: 0.02, g: 0.03, b: 0.12, a: 0.3 }),
                )
        }
        _ => {
            let h = if thin { 4.0 } else { 28.0 };
            el().w(w).h(h).radius(t.radii.pill).fill(t.raised)
                .child(el().pin(Some(0.0), Some(0.0), None, None).size(x, h).radius(t.radii.pill).fill(t.ink))
        }
    }
}

/// A track that follows the pointer while pressed and the arrow keys while focused. `set` gets
/// the new value, 0 to 1.
fn slider(t: &Tokens, key: String, name: String, v: f32, set: impl Fn(&mut Stage, f32) + Clone + 'static) -> El {
    let (nudge, end) = (set.clone(), set.clone());
    row().w(TRACK_W).h(36.0).center_y().child(track(t, TRACK_W, v, false))
        .key(key).role("slider").label(name).hit()
        .on_drag(move |s: &mut Stage, d: &mut Drag, _: &mut Tx| {
            if !d.cancelled() && d.w > 0.0 {
                set(s, ((d.x / d.w).clamp(0.0, 1.0) * 100.0).round() / 100.0);
            }
        })
        .on_step(move |s: &mut Stage, by, _| nudge(s, (v + by as f32 * 0.02).clamp(0.0, 1.0)))
        .on_step_limit(move |s: &mut Stage, max, _| end(s, if max { 1.0 } else { 0.0 }))
}

fn send(s: &Stage, cmd: Cmd) {
    if let Some(a) = &s.sh.services.lock().unwrap().audio {
        a.send(cmd);
    }
}

/// Mute button and slider for the default output or input.
fn volume_row(t: &Tokens, d: &Device, input: bool) -> El {
    let id = if input { "mic" } else { "vol" };
    let (vol, mute): (fn(f32) -> Cmd, fn(bool) -> Cmd) = if input { (Cmd::SourceVolume, Cmd::SourceMute) } else { (Cmd::SinkVolume, Cmd::SinkMute) };
    let muted = d.muted;
    let off = muted || d.volume <= 0.001;
    let glyph = if input { "mic" } else if off { "vol0" } else { "vol" };
    let name = if input { "Input volume" } else { "Output volume" };
    let button = cell(28.0, 28.0, glyph_el(t, glyph, 18.0, if off { t.ink3 } else { t.ink }))
        .key(format!("{id}-mute")).role("button").label(if muted { "Unmute" } else { "Mute" }).hit()
        .radius(t.radii.pill).hover(t.fill)
        .on_press(move |s: &mut Stage| send(s, mute(!muted)));
    // Moving a muted slider is asking to hear it.
    let set = move |s: &mut Stage, v: f32| {
        if muted && v > 0.0 {
            send(s, mute(false));
        }
        send(s, vol(v));
    };
    let shown = if muted { 0.0 } else { d.volume.min(1.0) };
    row().w_full().h(48.0).px(10.0).gap(14.0).center_y()
        .child(button)
        .child(slider(t, id.to_string(), name.to_string(), shown, set))
}

fn kind_glyph(kind: DeviceKind) -> &'static str {
    match kind {
        DeviceKind::Speakers => "speaker",
        DeviceKind::Headphones => "phones",
        DeviceKind::Display => "monitor",
        DeviceKind::Microphone => "mic",
    }
}

fn device_row(t: &Tokens, i: usize, d: &Device, on: bool) -> El {
    let text_col = col().grow(1.0).min_w(0.0).gap(2.0)
        .child(label(t, 14.0, 500.0, d.description.clone(), t.ink).truncate())
        .children((!d.detail.is_empty()).then(|| label(t, 12.0, 400.0, d.detail.clone(), t.ink2).truncate()));
    let name = d.name.clone();
    let mut r = row().w_full().h(50.0).px(14.0).gap(14.0).center_y().radius(t.radii.ctl)
        .child(cell(18.0, 18.0, glyph_el(t, kind_glyph(d.kind), 18.0, t.ink)))
        .child(text_col)
        .key(format!("sink{i}")).role("button").label(if on { format!("{}, selected", d.description) } else { d.description.clone() }).hit()
        .on_press(move |s: &mut Stage| send(s, Cmd::DefaultSink(name.clone())));
    r = match (on, t.look) {
        (false, _) => r.hover(t.raised),
        (true, Look::Folio) => r.border_sides([0.0, 0.0, 0.0, 3.0], t.acc),
        (true, Look::Phosphor) => r.border(1.0, t.ink),
        (true, Look::Prism) => r.fill(t.fill).shadow(1.0, 4.0, Color { r: 0.02, g: 0.03, b: 0.12, a: 0.12 }),
        (true, _) => r.fill(t.fill),
    };
    r
}

/// A screen's title, in the look's own voice.
pub(crate) fn title(t: &Tokens, s: &str) -> El {
    let e = match t.look {
        Look::Folio => text(s.to_string()).font(TextStyle::new(26.0, 500.0).family(t.fonts.display).italic().tracking(-0.4)),
        Look::Phosphor => text(format!("> {}", s.to_lowercase())).font(TextStyle::new(15.0, 600.0).family(t.fonts.body).tracking(0.3)),
        Look::Prism => text(s.to_string()).font(TextStyle::new(22.0, 600.0).family(t.fonts.body).tracking(-0.55)),
        _ => text(s.to_string()).font(TextStyle::new(22.0, 600.0).family(t.fonts.body).tracking(-0.22)),
    };
    row().w_full().h(48.0).center_y().pl(8.0).child(e.color(t.ink))
}

/// A small heading over a group of rows.
pub(crate) fn section(t: &Tokens, s: &str) -> El {
    match t.look {
        Look::Folio => {
            let e = text(s.to_uppercase()).font(TextStyle::new(10.5, 500.0).family(t.fonts.fig).tracking(1.5)).color(t.ink2).truncate();
            row().w_full().px(10.0).pt(6.0)
                .child(col().grow(1.0).min_w(0.0).pt(14.0).pb(6.0).border_sides([1.0, 0.0, 0.0, 0.0], t.ink).child(e))
        }
        Look::Phosphor => label(t, 12.0, 400.0, format!("# {}", s.to_lowercase()), t.ink).truncate().px(10.0).pt(12.0).pb(4.0),
        _ => label(t, 12.0, 500.0, s, t.ink2).truncate().px(10.0).pt(12.0).pb(4.0),
    }
}

/// A line of explanation where a list would be.
pub(crate) fn note(t: &Tokens, s: impl Into<String>) -> El {
    label(t, 13.0, 400.0, s, t.ink2).wrap().px(8.0).py(6.0)
}

pub fn view(s: &Stage, t: &Tokens) -> El {
    let audio = s.sh.services.lock().unwrap().audio.clone();
    let mut p = panel(t, W).child(title(t, "Sound"));
    let Some(audio) = audio else { return p.child(note(t, "Sound is not available.")) };
    let (state, error) = {
        let snap = audio.read();
        (snap.state.clone(), snap.error.clone())
    };
    if let Some(error) = error {
        return p.child(note(t, "PulseAudio is not available.")).child(label(t, 12.0, 400.0, error, t.ink3).wrap().px(8.0));
    }
    if state.sinks.is_empty() && state.sources.is_empty() {
        return p.child(note(t, "No audio devices found."));
    }
    if let Some(sink) = state.sink() {
        p = p.child(volume_row(t, sink, false));
    }
    if !state.sinks.is_empty() {
        let current = state.sink().map(|d| d.name.clone());
        p = p.child(section(t, "Output")).child(
            col().w_full().children(state.sinks.iter().enumerate().map(|(i, d)| device_row(t, i, d, Some(&d.name) == current.as_ref()))),
        );
    }
    if let Some(source) = state.source() {
        p = p.child(section(t, &format!("Input · {}", source.description))).child(volume_row(t, source, true));
    }
    p
}
