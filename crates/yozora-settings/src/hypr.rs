//! The little Settings needs from Hyprland: the monitor layout, and focusing its own window.

use std::{
    io::{Read, Write},
    os::unix::net::UnixStream,
    path::PathBuf,
    time::Duration,
};

use anyhow::Context as _;

#[derive(Clone, Debug, PartialEq)]
pub struct Monitor {
    pub name: String,
    pub description: String,
    pub x: i32,
    pub y: i32,
    /// Logical size, after scale and rotation.
    pub width: i32,
    pub height: i32,
    /// Pixel mode, before rotation.
    pub mode: (i32, i32),
    pub refresh: f64,
    pub focused: bool,
}

fn request(command: &str) -> anyhow::Result<String> {
    let sig = std::env::var("HYPRLAND_INSTANCE_SIGNATURE").context("Hyprland isn't running")?;
    let runtime = std::env::var("XDG_RUNTIME_DIR").context("XDG_RUNTIME_DIR is not set")?;
    let path = PathBuf::from(runtime).join("hypr").join(sig).join(".socket.sock");
    let mut stream = UnixStream::connect(&path).with_context(|| format!("connecting to {}", path.display()))?;
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    stream.write_all(command.as_bytes())?;
    let mut reply = String::new();
    stream.read_to_string(&mut reply)?;
    Ok(reply)
}

pub fn monitors() -> anyhow::Result<Vec<Monitor>> {
    let reply = request("j/monitors")?;
    let value: serde_json::Value = serde_json::from_str(&reply).context("parsing j/monitors")?;
    let int = |v: &serde_json::Value, k: &str| v[k].as_i64().unwrap_or(0) as i32;
    Ok(value
        .as_array()
        .map(|a| {
            a.iter()
                .map(|m| {
                    let (w, h) = (int(m, "width"), int(m, "height"));
                    let scale = m["scale"].as_f64().filter(|s| *s > 0.0).unwrap_or(1.0);
                    let rotated = m["transform"].as_i64().unwrap_or(0) % 2 == 1;
                    let (lw, lh) = if rotated { (h, w) } else { (w, h) };
                    Monitor {
                        name: m["name"].as_str().unwrap_or_default().to_string(),
                        description: m["description"].as_str().unwrap_or_default().to_string(),
                        x: int(m, "x"),
                        y: int(m, "y"),
                        width: (lw as f64 / scale).round() as i32,
                        height: (lh as f64 / scale).round() as i32,
                        mode: (w, h),
                        refresh: m["refreshRate"].as_f64().unwrap_or(0.0),
                        focused: m["focused"].as_bool().unwrap_or(false),
                    }
                })
                .collect()
        })
        .unwrap_or_default())
}

/// Asks Hyprland to focus our window, for a second launch. Wayland apps can't raise themselves
/// without an activation token, and the launcher that started the second instance has none.
pub fn focus_settings_window() {
    let lua = r#"eval hl.dispatch(hl.dsp.focus({ window = "class:^(yozora-settings)$" }))"#;
    match request(lua) {
        Ok(r) if r.trim() == "ok" || r.contains("hl.") => {}
        _ => {
            // Hyprland before the Lua config.
            let _ = request("dispatch focuswindow class:^(yozora-settings)$");
        }
    }
}
