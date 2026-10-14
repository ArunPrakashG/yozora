//! The desktop's state as JSON, and the few controls scripts and agents get, over the control
//! socket: `state`, `media`, `volume` and `profile`. The `yozora` command reads these.

use std::time::{Duration, Instant};

use gpui::App;
use serde_json::{Value, json};
use yozora_ipc::{Incoming, Response};

use crate::services::{Kind, Services, audio, media, power};

/// The sections `state` knows, in the order it lists them.
pub const SECTIONS: [&str; 7] = ["hyprland", "audio", "media", "network", "bluetooth", "power", "tray"];

/// How long `state` waits for a service it had to start.
const WARM_UP: Duration = Duration::from_secs(3);

fn kind(section: &str) -> Option<Kind> {
    Some(match section {
        "hyprland" => Kind::Hyprland,
        "audio" => Kind::Audio,
        "media" => Kind::Media,
        "network" => Kind::Network,
        "bluetooth" => Kind::Bluetooth,
        "power" => Kind::Power,
        "tray" => Kind::Tray,
        _ => return None,
    })
}

/// `state [SECTION...]`: starts any service the bar doesn't run yet, waits a moment for its
/// first snapshot, then answers with every section asked for, or all of them.
pub fn state(args: Vec<String>, incoming: Incoming, cx: &mut App) {
    let sections: Vec<String> = if args.is_empty() { SECTIONS.iter().map(|s| s.to_string()).collect() } else { args };
    if let Some(bad) = sections.iter().find(|s| kind(s).is_none()) {
        return incoming.reply(Response::error(format!("no section {bad:?}; try {}", SECTIONS.join(", "))));
    }
    let kinds: Vec<Kind> = sections.iter().filter_map(|s| kind(s)).collect();
    let customs = crate::app::Config::get(cx).settings.bar.custom.clone();
    Services::ensure(kinds.iter().copied(), &customs, cx);
    let started = Instant::now();
    cx.spawn(async move |cx| {
        loop {
            let settled = cx.update(|cx| kinds.iter().all(|k| settled(*k, cx)));
            if settled || started.elapsed() >= WARM_UP {
                break;
            }
            cx.background_executor().timer(Duration::from_millis(50)).await;
        }
        cx.update(|cx| {
            let mut out = serde_json::Map::new();
            for s in &sections {
                out.insert(s.clone(), section(s, cx));
            }
            incoming.reply(Response::data(Value::Object(out)));
        });
    })
    .detach();
}

/// Whether a service has its first snapshot, or has failed.
fn settled(kind: Kind, cx: &App) -> bool {
    macro_rules! up {
        ($field:ident) => {
            Services::get(cx).$field.as_ref().is_none_or(|e| {
                let s = e.read(cx);
                s.ready || s.error.is_some()
            })
        };
    }
    match kind {
        Kind::Hyprland => up!(hyprland),
        Kind::Audio => up!(audio),
        Kind::Media => up!(media),
        Kind::Network => up!(network),
        Kind::Bluetooth => up!(bluetooth),
        Kind::Power => up!(power),
        Kind::Tray => up!(tray),
        Kind::Custom | Kind::Notifications => true,
    }
}

/// One service's snapshot, with `error` set while it can't reach its system service.
fn section(name: &str, cx: &App) -> Value {
    let s = Services::get(cx);
    macro_rules! with {
        ($field:ident, $f:expr) => {
            match s.$field.as_ref().map(|e| e.read(cx)) {
                None => json!({ "error": "not running" }),
                Some(svc) if !svc.ready => {
                    json!({ "error": svc.error.clone().unwrap_or_else(|| "still starting".into()) })
                }
                Some(svc) => {
                    let mut v: Value = $f(&svc.state);
                    if let (Some(e), Some(o)) = (&svc.error, v.as_object_mut()) {
                        o.insert("error".into(), json!(e));
                    }
                    if svc.fake && let Some(o) = v.as_object_mut() {
                        o.insert("fake".into(), json!(true));
                    }
                    v
                }
            }
        };
    }
    match name {
        "hyprland" => with!(hyprland, hyprland),
        "audio" => with!(audio, audio_json),
        "media" => with!(media, media_json),
        "network" => with!(network, network),
        "bluetooth" => with!(bluetooth, bluetooth),
        "power" => with!(power, power_json),
        "tray" => with!(tray, tray),
        _ => Value::Null,
    }
}

fn lower(v: impl std::fmt::Debug) -> String {
    format!("{v:?}").to_lowercase()
}

fn pct(v: f32) -> u32 {
    (v * 100.).round().max(0.) as u32
}

fn hyprland(s: &crate::services::hyprland::State) -> Value {
    json!({
        "monitors": s.monitors.iter().map(|m| json!({
            "name": m.name, "focused": m.focused, "workspace": m.active_workspace,
            "x": m.x, "y": m.y, "width": m.width, "height": m.height,
        })).collect::<Vec<_>>(),
        "workspaces": s.workspaces.iter().map(|w| json!({
            "id": w.id, "name": w.name, "monitor": w.monitor, "windows": w.windows,
        })).collect::<Vec<_>>(),
        "window": s.window.as_ref().map(|w| json!({ "class": w.class, "title": w.title, "workspace": w.workspace })),
        "keyboard": s.keyboard.as_ref().map(|k| json!({ "layout": k.layout, "code": k.code })),
        "sharing_screen": s.sharing,
    })
}

fn device(d: &audio::Device) -> Value {
    json!({ "name": d.name, "description": d.description, "kind": lower(d.kind), "volume": pct(d.volume), "muted": d.muted })
}

fn audio_json(s: &audio::State) -> Value {
    json!({
        "output": s.sink().map(device),
        "input": s.source().map(device),
        "outputs": s.sinks.iter().map(device).collect::<Vec<_>>(),
        "inputs": s.sources.iter().map(device).collect::<Vec<_>>(),
        "server": s.server,
    })
}

fn position(p: &media::Player) -> Duration {
    match (p.status, p.position_at) {
        (media::Status::Playing, Some(at)) => p.position + at.elapsed(),
        _ => p.position,
    }
}

fn media_json(s: &media::State) -> Value {
    json!({
        "selected": s.selected,
        "players": s.players.iter().map(|p| json!({
            "id": p.bus,
            "name": p.identity,
            "status": lower(p.status),
            "title": p.title,
            "artist": p.artist,
            "album": p.album,
            "position_s": position(p).as_secs(),
            "length_s": p.length.map(|l| l.as_secs()),
        })).collect::<Vec<_>>(),
    })
}

fn network(s: &crate::services::network::State) -> Value {
    json!({
        "connection": s.primary.as_ref().map(|p| json!({ "name": p.name, "kind": lower(p.kind), "strength": p.strength })),
        "wifi": { "enabled": s.wifi_enabled, "device": s.wifi_device },
        "wired": s.wired.as_ref().map(|w| json!({ "interface": w.interface, "connected": w.connected })),
        "networks": s.access_points.iter().take(12).map(|a| json!({
            "ssid": a.ssid, "strength": a.strength, "secured": a.secured, "active": a.active, "known": a.known,
        })).collect::<Vec<_>>(),
        "vpns": s.vpns.iter().map(|v| json!({ "name": v.name, "active": v.active })).collect::<Vec<_>>(),
    })
}

fn bluetooth(s: &crate::services::bluetooth::State) -> Value {
    json!({
        "adapter": s.adapter,
        "powered": s.powered,
        "devices": s.devices.iter().filter(|d| d.paired || d.connected).map(|d| json!({
            "name": d.name, "address": d.address, "connected": d.connected, "battery": d.battery,
        })).collect::<Vec<_>>(),
    })
}

fn power_json(s: &power::State) -> Value {
    json!({
        "battery": s.battery.as_ref().map(|b| json!({
            "percent": b.percent.round(), "state": lower(b.charge), "seconds_left": b.seconds_left,
        })),
        "profile": s.profile,
        "profiles": s.profiles,
        "peripherals": s.peripherals.iter().map(|p| json!({
            "name": p.name, "kind": lower(p.kind), "percent": p.percent.round(),
        })).collect::<Vec<_>>(),
    })
}

fn tray(s: &crate::services::tray::State) -> Value {
    json!({
        "items": s.items.iter().map(|i| json!({ "title": i.title, "status": lower(i.status) })).collect::<Vec<_>>(),
    })
}

/// `media [play-pause|play|pause|next|previous] [PLAYER]`: controls the selected player, or the
/// one whose name or id contains PLAYER.
pub fn media(args: &[String], cx: &mut App) -> Response {
    let action = args.first().map(String::as_str).unwrap_or("play-pause");
    let Some(entity) = Services::get(cx).media.clone() else {
        return Response::error("media isn't running: add the media module to the bar");
    };
    let svc = entity.read(cx);
    let players = &svc.state.players;
    let player = match args.get(1) {
        Some(want) => {
            let want = want.to_lowercase();
            players.iter().find(|p| p.identity.to_lowercase().contains(&want) || p.bus.to_lowercase().contains(&want))
        }
        None => players.iter().find(|p| svc.state.selected.as_ref() == Some(&p.bus)).or_else(|| players.first()),
    };
    let Some(player) = player else { return Response::error("nothing is playing") };
    let playing = player.status == media::Status::Playing;
    let cmd = match action {
        "play-pause" | "toggle" => Some(media::Cmd::PlayPause),
        "play" => (!playing).then_some(media::Cmd::PlayPause),
        "pause" => playing.then_some(media::Cmd::PlayPause),
        "next" => Some(media::Cmd::Next),
        "previous" | "prev" => Some(media::Cmd::Previous),
        other => {
            return Response::error(format!("media takes play-pause, play, pause, next or previous, not {other:?}"));
        }
    };
    let bus = player.bus.clone();
    let name = player.identity.clone();
    if svc.state.selected.as_ref() != Some(&bus) {
        svc.send(media::Cmd::Select(bus));
    }
    if let Some(cmd) = cmd {
        svc.send(cmd);
    }
    Response::data(json!({ "player": name, "done": action }))
}

/// How a volume argument changes the level.
#[derive(Debug, PartialEq)]
pub enum Change {
    Set(f32),
    By(f32),
    Mute(Option<bool>),
}

/// `N`, `N%`, `+N`, `-N`, `mute`, `unmute` or `toggle-mute`.
pub fn parse_change(arg: &str) -> Result<Change, String> {
    match arg {
        "mute" => return Ok(Change::Mute(Some(true))),
        "unmute" => return Ok(Change::Mute(Some(false))),
        "toggle-mute" => return Ok(Change::Mute(None)),
        _ => {}
    }
    let num = arg.trim_end_matches('%');
    let bad = || format!("volume takes N, +N, -N (percent), mute, unmute or toggle-mute, not {arg:?}");
    let value: f32 = num.trim_start_matches(['+', '-']).parse().map_err(|_| bad())?;
    if !(0.0..=150.0).contains(&value) {
        return Err(bad());
    }
    Ok(match num.chars().next() {
        Some('+') => Change::By(value / 100.),
        Some('-') => Change::By(-value / 100.),
        _ => Change::Set(value / 100.),
    })
}

/// `volume [--mic] [CHANGE]`: the output (or microphone) level, after changing it. Levels stop at
/// 100%; the sound popover's slider does too.
pub fn volume(args: &[String], cx: &mut App) -> Response {
    let mic = args.iter().any(|a| a == "--mic");
    let change = match args.iter().find(|a| *a != "--mic") {
        Some(a) => match parse_change(a) {
            Ok(c) => Some(c),
            Err(e) => return Response::error(e),
        },
        None => None,
    };
    let Some(entity) = Services::get(cx).audio.clone() else {
        return Response::error("sound isn't running: add the sound module to the bar");
    };
    let svc = entity.read(cx);
    let Some(dev) = (if mic { svc.state.source() } else { svc.state.sink() }) else {
        return Response::error(if mic { "no microphone" } else { "no output device" });
    };
    let (mut level, mut muted) = (dev.volume, dev.muted);
    match change {
        Some(Change::Set(v)) => level = v.clamp(0., 1.),
        Some(Change::By(d)) => level = (level + d).clamp(0., 1.),
        Some(Change::Mute(m)) => muted = m.unwrap_or(!muted),
        None => {}
    }
    match change {
        Some(Change::Set(_) | Change::By(_)) => {
            svc.send(if mic { audio::Cmd::SourceVolume(level) } else { audio::Cmd::SinkVolume(level) })
        }
        Some(Change::Mute(_)) => {
            svc.send(if mic { audio::Cmd::SourceMute(muted) } else { audio::Cmd::SinkMute(muted) })
        }
        None => {}
    }
    Response::data(json!({ "device": dev.description, "volume": pct(level), "muted": muted }))
}

/// `profile [NAME]`: the power profile, after switching to NAME.
pub fn profile(args: &[String], cx: &mut App) -> Response {
    let Some(entity) = Services::get(cx).power.clone() else {
        return Response::error("power isn't running: add the battery module to the bar");
    };
    let svc = entity.read(cx);
    match args.first() {
        None => Response::data(json!({ "profile": svc.state.profile, "profiles": svc.state.profiles })),
        Some(want) if svc.state.profiles.contains(want) => {
            svc.send(power::Cmd::SetProfile(want.clone()));
            Response::data(json!({ "profile": want, "profiles": svc.state.profiles }))
        }
        Some(want) => Response::error(format!("no power profile {want:?}; try {}", svc.state.profiles.join(", "))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn volume_changes_parse() {
        assert_eq!(parse_change("40"), Ok(Change::Set(0.4)));
        assert_eq!(parse_change("40%"), Ok(Change::Set(0.4)));
        assert_eq!(parse_change("+5"), Ok(Change::By(0.05)));
        assert_eq!(parse_change("-10%"), Ok(Change::By(-0.1)));
        assert_eq!(parse_change("mute"), Ok(Change::Mute(Some(true))));
        assert_eq!(parse_change("toggle-mute"), Ok(Change::Mute(None)));
        assert!(parse_change("loud").is_err());
        assert!(parse_change("900").is_err());
    }
}
