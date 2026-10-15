//! Just enough of Hyprland's socket for apps other than the bar: which monitor is which, and
//! one-off commands.

use std::{
    io::{Read, Write},
    os::unix::net::UnixStream,
    path::PathBuf,
    time::Duration,
};

use gpui::{App, DisplayId};

pub struct Monitor {
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub focused: bool,
    /// The size in logical pixels, as the compositor lays it out: rotated and scaled.
    pub width: f32,
    pub height: f32,
}

/// Sends `command` to Hyprland's request socket and returns the reply.
pub fn request(command: &str) -> anyhow::Result<String> {
    let runtime = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(|| "/tmp".into());
    let sig = std::env::var("HYPRLAND_INSTANCE_SIGNATURE")?;
    let mut stream = UnixStream::connect(runtime.join("hypr").join(sig).join(".socket.sock"))?;
    stream.set_read_timeout(Some(Duration::from_millis(500)))?;
    stream.write_all(command.as_bytes())?;
    let mut reply = String::new();
    stream.read_to_string(&mut reply)?;
    Ok(reply)
}

pub fn monitors() -> anyhow::Result<Vec<Monitor>> {
    let value: serde_json::Value = serde_json::from_str(&request("j/monitors")?)?;
    Ok(value
        .as_array()
        .map(|a| {
            a.iter()
                .map(|m| Monitor {
                    name: m["name"].as_str().unwrap_or_default().to_string(),
                    x: m["x"].as_i64().unwrap_or(0) as i32,
                    y: m["y"].as_i64().unwrap_or(0) as i32,
                    focused: m["focused"].as_bool().unwrap_or(false),
                    width: logical(m, "width", "height"),
                    height: logical(m, "height", "width"),
                })
                .collect()
        })
        .unwrap_or_default())
}

/// `key` of the monitor in logical pixels: the mode's sides swap on a quarter turn
/// (transform 1, 3, 5, 7), and the scale divides them.
fn logical(m: &serde_json::Value, key: &str, other: &str) -> f32 {
    let quarter = m["transform"].as_i64().unwrap_or(0) % 2 == 1;
    let side = m[if quarter { other } else { key }].as_f64().unwrap_or(0.) as f32;
    side / m["scale"].as_f64().filter(|s| *s > 0.).unwrap_or(1.) as f32
}

/// The GPUI display for a Hyprland monitor. GPUI's display bounds ignore rotation, but their
/// origin is the output's position, which is unique.
pub fn display_of(monitor: &Monitor, cx: &App) -> Option<DisplayId> {
    cx.displays()
        .into_iter()
        .find(|d| {
            let o = d.bounds().origin;
            f32::from(o.x).round() as i32 == monitor.x && f32::from(o.y).round() as i32 == monitor.y
        })
        .map(|d| d.id())
}
