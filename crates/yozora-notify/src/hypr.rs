//! Which monitor toasts open on, from Hyprland.

use std::{
    io::{Read, Write},
    os::unix::net::UnixStream,
    path::PathBuf,
    time::Duration,
};

use gpui::{App, DisplayId};

struct Monitor {
    name: String,
    x: i32,
    y: i32,
    focused: bool,
}

fn request(command: &str) -> anyhow::Result<String> {
    let runtime = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(|| "/tmp".into());
    let sig = std::env::var("HYPRLAND_INSTANCE_SIGNATURE")?;
    let mut stream = UnixStream::connect(runtime.join("hypr").join(sig).join(".socket.sock"))?;
    stream.set_read_timeout(Some(Duration::from_millis(500)))?;
    stream.write_all(command.as_bytes())?;
    let mut reply = String::new();
    stream.read_to_string(&mut reply)?;
    Ok(reply)
}

/// Asks Hyprland to blur behind toasts, the same rule the bar adds. Rules added at runtime last
/// until Hyprland reloads its config, so this runs on every start.
pub fn apply_blur_rule() {
    let lua = r#"eval hl.layer_rule({ match = { namespace = "^yozora-" }, blur = true, blur_popups = true, ignore_alpha = 0.2, no_anim = true })"#;
    match request(lua) {
        Ok(r) if r.trim() == "ok" => tracing::debug!("applied blur layer rules"),
        Ok(_) => {
            // Pre-Lua Hyprland.
            for rule in ["blur,^(yozora-.*)$", "ignorealpha 0.2,^(yozora-.*)$", "noanim,^(yozora-.*)$"] {
                let _ = request(&format!("keyword layerrule {rule}"));
            }
        }
        Err(e) => tracing::debug!("could not apply layer rules: {e:#}"),
    }
}

fn monitors() -> anyhow::Result<Vec<Monitor>> {
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
                })
                .collect()
        })
        .unwrap_or_default())
}

/// The display for `wanted`: `focused`, or a connector name. Falls back to the focused monitor
/// and then to whatever GPUI calls primary. Also returns the monitor's name, if known.
pub fn display_for(wanted: &str, cx: &App) -> (Option<DisplayId>, Option<String>) {
    let monitors = monitors().unwrap_or_else(|e| {
        tracing::debug!("no Hyprland monitors: {e:#}");
        Vec::new()
    });
    let target = monitors
        .iter()
        .find(|m| wanted != "focused" && m.name == wanted)
        .or_else(|| monitors.iter().find(|m| m.focused));
    // GPUI's display bounds ignore rotation, but their origin is the output's position, which
    // is unique.
    let display = target.and_then(|m| {
        cx.displays().into_iter().find(|d| {
            let o = d.bounds().origin;
            f32::from(o.x).round() as i32 == m.x && f32::from(o.y).round() as i32 == m.y
        })
    });
    let display = display.or_else(|| cx.primary_display());
    (display.map(|d| d.id()), target.map(|m| m.name.clone()))
}
