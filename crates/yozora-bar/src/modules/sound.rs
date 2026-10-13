//! Output and input volume over PulseAudio, and choosing the output device.

use gpui::{
    AnyElement, App, Context, Div, IntoElement, ParentElement, StatefulInteractiveElement, Styled, div, prelude::*, px,
};
use yozora_ui::{Icon, Theme, controls, icon};

use crate::{
    bar::bi,
    popover::Popover,
    services::{
        Services,
        audio::{Cmd, Device, DeviceKind},
        run_shell,
    },
};

pub fn volume_icon(device: Option<&Device>) -> Icon {
    match device {
        None => Icon::VolumeMute,
        Some(d) if d.muted || d.volume <= 0.001 => Icon::VolumeMute,
        Some(d) if d.volume < 0.5 => Icon::VolumeLow,
        Some(_) => Icon::VolumeHigh,
    }
}

pub fn percent(v: f32) -> u32 {
    (v * 100.).round().max(0.) as u32
}

pub fn item(theme: &Theme, cx: &App) -> Option<Div> {
    let audio = Services::get(cx).audio.as_ref()?.read(cx);
    let sink = audio.state.sink();
    let dim = !audio.up() || sink.is_none_or(|s| s.muted);
    let entity = Services::get(cx).audio.clone()?;
    let volume = sink.map_or(0., |s| s.volume);
    Some(bi().child(icon(volume_icon(sink)).text_color(if dim { theme.ink_3 } else { theme.ink })).on_scroll_wheel(
        move |e, _, cx| {
            // Scrolling over the icon nudges the volume by 5%.
            let dy = match e.delta {
                gpui::ScrollDelta::Lines(p) => p.y,
                gpui::ScrollDelta::Pixels(p) => f32::from(p.y) / 40.,
            };
            if dy.abs() >= 0.5 {
                let v = (volume + dy.signum() * 0.05).clamp(0., 1.);
                entity.read(cx).send(Cmd::SinkVolume(v));
            }
        },
    ))
}

pub fn send(cmd: Cmd, cx: &App) {
    if let Some(a) = &Services::get(cx).audio {
        a.read(cx).send(cmd);
    }
}

fn kind_icon(kind: DeviceKind) -> Icon {
    match kind {
        DeviceKind::Speakers => Icon::Speaker,
        DeviceKind::Headphones => Icon::Headphones,
        DeviceKind::Display => Icon::Display,
        DeviceKind::Microphone => Icon::Mic,
    }
}

/// The volume slider with a mute toggle on its icon.
pub fn sink_slider(p: &Popover, theme: &Theme, cx: &App) -> Option<AnyElement> {
    let audio = Services::get(cx).audio.as_ref()?.read(cx);
    let sink = audio.state.sink()?;
    let (muted, volume) = (sink.muted, sink.volume);
    Some(
        div()
            .flex()
            .items_center()
            .gap(px(8.))
            .child(
                div()
                    .id("mute")
                    .flex_none()
                    .size(px(28.))
                    .rounded(px(8.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .hover(|s| s.bg(theme.fill_2))
                    .text_color(if muted { theme.ink_3 } else { theme.ink })
                    .child(icon(volume_icon(Some(sink))))
                    .on_click(move |_, _, cx| send(Cmd::SinkMute(!muted), cx)),
            )
            .child(controls::slider(
                "volume",
                if muted { 0. } else { volume },
                None,
                &p.ui.volume,
                theme,
                |v, _, cx| send(Cmd::SinkVolume(v), cx),
            ))
            .into_any_element(),
    )
}

pub fn source_slider(p: &Popover, theme: &Theme, cx: &App) -> Option<AnyElement> {
    let audio = Services::get(cx).audio.as_ref()?.read(cx);
    let source = audio.state.source()?;
    let (muted, volume) = (source.muted, source.volume);
    Some(
        div()
            .flex()
            .items_center()
            .gap(px(8.))
            .child(
                div()
                    .id("mic-mute")
                    .flex_none()
                    .size(px(28.))
                    .rounded(px(8.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .hover(|s| s.bg(theme.fill_2))
                    .text_color(if muted { theme.ink_3 } else { theme.ink })
                    .child(icon(if muted { Icon::MicOff } else { Icon::Mic }))
                    .on_click(move |_, _, cx| send(Cmd::SourceMute(!muted), cx)),
            )
            .child(controls::slider("mic", if muted { 0. } else { volume }, None, &p.ui.mic, theme, |v, _, cx| {
                send(Cmd::SourceVolume(v), cx)
            }))
            .into_any_element(),
    )
}

pub fn popover(p: &mut Popover, theme: &Theme, cx: &mut Context<Popover>) -> Vec<AnyElement> {
    let Some(audio) = Services::get(cx).audio.clone() else { return Vec::new() };
    let service = audio.read(cx);
    let state = service.state.clone();
    let mut rows: Vec<AnyElement> = Vec::new();
    let level = state.sink().map(|s| if s.muted { "Muted".to_string() } else { format!("{}%", percent(s.volume)) });
    rows.push(controls::header("Sound", controls::muted(level.unwrap_or_default(), theme)).into_any_element());
    if let Some(error) = &service.error {
        rows.push(controls::empty(&["PulseAudio is not available.", error.as_str()], theme).into_any_element());
        return rows;
    }
    rows.extend(sink_slider(p, theme, cx));
    if !state.sinks.is_empty() {
        rows.push(controls::label("Output", theme).into_any_element());
        let sinks = state.sinks.iter().enumerate().map(|(i, s)| {
            let on = s.name == state.default_sink;
            let name = s.name.clone();
            controls::row(("sink", i), theme)
                .cursor_pointer()
                .child(controls::row_icon(kind_icon(s.kind), on, theme))
                .child(controls::row_text(
                    s.description.clone(),
                    (!s.detail.is_empty()).then(|| s.detail.clone().into()),
                    theme,
                ))
                .child(controls::check(("sink-check", i), on, theme))
                .on_click(move |_, _, cx| send(Cmd::DefaultSink(name.clone()), cx))
                .into_any_element()
        });
        rows.push(div().flex().flex_col().children(sinks).into_any_element());
    }
    if let Some(source) = state.source() {
        rows.push(controls::label(format!("Input · {}", source.description), theme).into_any_element());
        rows.extend(source_slider(p, theme, cx));
    }
    let note = if service.fake { "PulseAudio · fake".to_string() } else { format!("PulseAudio · {}", state.server) };
    rows.push(
        controls::footer(
            note,
            controls::link("sound-settings", "Sound settings…", theme).on_click(cx.listener(|p, _, _, cx| {
                run_shell("pavucontrol");
                p.close(cx);
            })),
            theme,
        )
        .into_any_element(),
    );
    rows
}
