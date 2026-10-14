//! PulseAudio (and pipewire-pulse) through `pactl`.
//!
//! `pactl subscribe` reports changes and `pactl -f json` reads state. Driving the client
//! binary instead of linking libpulse keeps a C dependency and its main loop out of the bar,
//! and costs a few milliseconds per change, which is nothing at the rate audio state changes.
//! Volume changes from a dragged slider are coalesced so only the latest value is applied.

use std::{
    io::{BufRead, BufReader},
    process::{Command, Stdio},
    time::Duration,
};

use anyhow::Context as _;
use futures_lite::FutureExt;
use gpui::{App, Entity};
use serde_json::Value;

use super::{Audio, Link};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DeviceKind {
    #[default]
    Speakers,
    Headphones,
    Display,
    Microphone,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Device {
    pub name: String,
    pub description: String,
    pub detail: String,
    pub kind: DeviceKind,
    /// 1.0 is 100%. Can exceed 1.0 when boosted elsewhere.
    pub volume: f32,
    pub muted: bool,
}

#[derive(Clone, Debug, Default)]
pub struct State {
    pub sinks: Vec<Device>,
    pub sources: Vec<Device>,
    pub default_sink: String,
    pub default_source: String,
    /// "PulseAudio" or "PipeWire", for the popover footer.
    pub server: String,
}

impl State {
    pub fn sink(&self) -> Option<&Device> {
        self.sinks.iter().find(|d| d.name == self.default_sink).or(self.sinks.first())
    }

    pub fn source(&self) -> Option<&Device> {
        self.sources.iter().find(|d| d.name == self.default_source).or(self.sources.first())
    }

    pub fn sink_mut(&mut self) -> Option<&mut Device> {
        let name = self.default_sink.clone();
        let i = self.sinks.iter().position(|d| d.name == name).unwrap_or(0);
        self.sinks.get_mut(i)
    }

    pub fn source_mut(&mut self) -> Option<&mut Device> {
        let name = self.default_source.clone();
        let i = self.sources.iter().position(|d| d.name == name).unwrap_or(0);
        self.sources.get_mut(i)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Cmd {
    SinkVolume(f32),
    SinkMute(bool),
    SourceVolume(f32),
    SourceMute(bool),
    DefaultSink(String),
}

pub fn start(cx: &mut App, fake: bool) -> Entity<Audio> {
    if fake {
        super::spawn(cx, "audio", true, run_fake)
    } else {
        super::spawn(cx, "audio", false, |link| super::supervise(link, run))
    }
}

fn pactl(args: &[&str]) -> anyhow::Result<String> {
    let out = Command::new("pactl").args(args).stdin(Stdio::null()).output().context("running pactl")?;
    if !out.status.success() {
        anyhow::bail!("pactl {}: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn json(args: &[&str]) -> anyhow::Result<Value> {
    let mut full = vec!["-f", "json"];
    full.extend_from_slice(args);
    serde_json::from_str(&pactl(&full)?).context("parsing pactl output")
}

fn volume(v: &Value) -> f32 {
    let Some(channels) = v["volume"].as_object() else { return 0.0 };
    let sum: f64 = channels.values().filter_map(|c| c["value"].as_f64()).sum();
    if channels.is_empty() { 0.0 } else { (sum / channels.len() as f64 / 65536.0) as f32 }
}

fn device(v: &Value, input: bool) -> Device {
    let props = &v["properties"];
    let prop = |k: &str| props[k].as_str().unwrap_or_default();
    let description = v["description"].as_str().unwrap_or_default().to_string();
    let port = v["active_port"].as_str().unwrap_or_default().to_lowercase();
    let bluetooth = prop("device.bus") == "bluetooth";
    let kind = if input {
        DeviceKind::Microphone
    } else if matches!(prop("device.form_factor"), "headphone" | "headset" | "hands-free")
        || port.contains("headphone")
        || bluetooth
    {
        DeviceKind::Headphones
    } else if port.contains("hdmi") || port.contains("displayport") || description.contains("HDMI") {
        DeviceKind::Display
    } else {
        DeviceKind::Speakers
    };
    let detail = if bluetooth {
        "Bluetooth".to_string()
    } else if kind == DeviceKind::Display {
        "Display audio".to_string()
    } else {
        prop("device.description").to_string()
    };
    Device {
        name: v["name"].as_str().unwrap_or_default().to_string(),
        description,
        detail,
        kind,
        volume: volume(v),
        muted: v["mute"].as_bool().unwrap_or(false),
    }
}

pub fn snapshot() -> anyhow::Result<State> {
    // Each pactl run takes about 7 ms; run the three at once so the OSD follows a key quickly.
    let (info, sinks, sources) = std::thread::scope(|s| {
        let sinks = s.spawn(|| json(&["list", "sinks"]));
        let sources = s.spawn(|| json(&["list", "sources"]));
        let info = json(&["info"]);
        let join = |h: std::thread::ScopedJoinHandle<'_, anyhow::Result<Value>>| {
            h.join().unwrap_or_else(|_| Err(anyhow::anyhow!("pactl thread panicked")))
        };
        (info, join(sinks), join(sources))
    });
    let (info, sinks, sources) = (info?, sinks?, sources?);
    let server = info["server_name"].as_str().unwrap_or_default();
    Ok(State {
        sinks: sinks.as_array().into_iter().flatten().map(|v| device(v, false)).collect(),
        sources: sources
            .as_array()
            .into_iter()
            .flatten()
            // Monitor sources mirror outputs; nobody wants them in a mic list.
            .filter(|v| v["monitor_of_sink"].as_str().is_none_or(|s| s.is_empty() || s == "n/a"))
            .filter(|v| !v["name"].as_str().unwrap_or_default().ends_with(".monitor"))
            .map(|v| device(v, true))
            .collect(),
        default_sink: info["default_sink_name"].as_str().unwrap_or_default().to_string(),
        default_source: info["default_source_name"].as_str().unwrap_or_default().to_string(),
        server: if server.contains("PipeWire") { "pipewire-pulse".into() } else { "PulseAudio".into() },
    })
}

fn apply(cmd: &Cmd, state: &State) -> anyhow::Result<()> {
    let pct = |v: f32| format!("{}%", (v.clamp(0.0, 1.5) * 100.0).round() as u32);
    let sink = state.sink().map(|d| d.name.clone()).unwrap_or("@DEFAULT_SINK@".into());
    let source = state.source().map(|d| d.name.clone()).unwrap_or("@DEFAULT_SOURCE@".into());
    match cmd {
        Cmd::SinkVolume(v) => pactl(&["set-sink-volume", &sink, &pct(*v)])?,
        Cmd::SinkMute(m) => pactl(&["set-sink-mute", &sink, if *m { "1" } else { "0" }])?,
        Cmd::SourceVolume(v) => pactl(&["set-source-volume", &source, &pct(*v)])?,
        Cmd::SourceMute(m) => pactl(&["set-source-mute", &source, if *m { "1" } else { "0" }])?,
        Cmd::DefaultSink(name) => pactl(&["set-default-sink", name])?,
    };
    Ok(())
}

/// Keeps only the newest of each kind of command, so a fast slider drag sends one update.
fn coalesce(first: Cmd, rx: &async_channel::Receiver<Cmd>) -> Vec<Cmd> {
    let mut cmds = vec![first];
    while let Ok(next) = rx.try_recv() {
        cmds.retain(|c| std::mem::discriminant(c) != std::mem::discriminant(&next));
        cmds.push(next);
    }
    cmds
}

enum Wake {
    Event,
    EventsClosed,
    Cmd(Option<Cmd>),
}

async fn run(link: &Link<State, Cmd>) -> anyhow::Result<()> {
    let mut child = Command::new("pactl")
        .arg("subscribe")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .context("starting `pactl subscribe`; is pactl installed?")?;
    let stdout = child.stdout.take().context("pactl stdout")?;
    let (event_tx, events) = async_channel::bounded::<()>(1);
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let Ok(line) = line else { break };
            if ["sink #", "source #", "server", "card #"].iter().any(|k| line.contains(k)) {
                let _ = event_tx.try_send(());
            }
        }
    });
    let result = async {
        let mut state = snapshot()?;
        link.publish(state.clone());
        loop {
            let wake = async {
                match events.recv().await {
                    Ok(()) => Wake::Event,
                    Err(_) => Wake::EventsClosed,
                }
            }
            .or(async { Wake::Cmd(link.cmds.recv().await.ok()) })
            .await;
            match wake {
                Wake::Event => {
                    // A volume change touches the sink and its card; let the burst finish.
                    async_io::Timer::after(Duration::from_millis(15)).await;
                    while events.try_recv().is_ok() {}
                    state = snapshot()?;
                    link.publish(state.clone());
                }
                Wake::EventsClosed => {
                    anyhow::bail!("`pactl subscribe` exited; is the sound server running?")
                }
                Wake::Cmd(None) => return Ok(()),
                Wake::Cmd(Some(cmd)) => {
                    for cmd in coalesce(cmd, &link.cmds) {
                        if let Err(e) = apply(&cmd, &state) {
                            tracing::warn!("{e:#}");
                        }
                    }
                }
            }
        }
    }
    .await;
    let _ = child.kill();
    let _ = child.wait();
    result
}

fn run_fake(link: Link<State, Cmd>) {
    let dev = |name: &str, description: &str, detail: &str, kind, volume| Device {
        name: name.into(),
        description: description.into(),
        detail: detail.into(),
        kind,
        volume,
        muted: false,
    };
    let mut state = State {
        sinks: vec![
            dev("speakers", "Speakers", "Built-in Audio Analog Stereo", DeviceKind::Speakers, 0.62),
            dev("headphones", "Headphones", "Bluetooth · 80% battery", DeviceKind::Headphones, 0.45),
            dev("hdmi", "HDMI-A-1", "Display audio", DeviceKind::Display, 1.0),
        ],
        sources: vec![dev("mic", "Microphone", "Built-in Audio", DeviceKind::Microphone, 0.38)],
        default_sink: "speakers".into(),
        default_source: "mic".into(),
        server: "fake".into(),
    };
    link.publish(state.clone());
    while let Ok(cmd) = link.cmds.recv_blocking() {
        match cmd {
            Cmd::SinkVolume(v) => state.sink_mut().into_iter().for_each(|d| d.volume = v),
            Cmd::SinkMute(m) => state.sink_mut().into_iter().for_each(|d| d.muted = m),
            Cmd::SourceVolume(v) => state.source_mut().into_iter().for_each(|d| d.volume = v),
            Cmd::SourceMute(m) => state.source_mut().into_iter().for_each(|d| d.muted = m),
            Cmd::DefaultSink(name) => state.default_sink = name,
        }
        if !link.publish(state.clone()) {
            break;
        }
    }
}

pub fn check() -> anyhow::Result<String> {
    let s = snapshot()?;
    let sink = s.sink().map(|d| d.description.clone()).unwrap_or("no output".into());
    Ok(format!("{} · {} output(s), default {sink}", s.server, s.sinks.len()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coalesces_by_kind() {
        let (tx, rx) = async_channel::unbounded();
        tx.try_send(Cmd::SinkVolume(0.2)).unwrap();
        tx.try_send(Cmd::SourceVolume(0.4)).unwrap();
        tx.try_send(Cmd::SinkVolume(0.3)).unwrap();
        let cmds = coalesce(Cmd::SinkVolume(0.1), &rx);
        assert_eq!(cmds, vec![Cmd::SourceVolume(0.4), Cmd::SinkVolume(0.3)]);
    }
}
