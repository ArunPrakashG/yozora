//! Which monitor toasts open on, from Hyprland.

use gpui::{App, DisplayId};
use yozora_ui::hypr::{display_of, monitors, request};

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
    let display = target.and_then(|m| display_of(m, cx)).or_else(|| cx.primary_display().map(|d| d.id()));
    (display, target.map(|m| m.name.clone()))
}

/// Calls `changed` whenever Hyprland starts or stops sharing the screen, from its event socket,
/// until `changed` returns false. Reconnects if Hyprland restarts; returns at once outside
/// Hyprland.
pub fn watch_screencast(mut changed: impl FnMut(bool) -> bool) {
    use std::io::BufRead;
    let Ok(sig) = std::env::var("HYPRLAND_INSTANCE_SIGNATURE") else { return };
    let runtime = std::env::var_os("XDG_RUNTIME_DIR").map(std::path::PathBuf::from).unwrap_or_else(|| "/tmp".into());
    let path = runtime.join("hypr").join(sig).join(".socket2.sock");
    let mut sharing = false;
    loop {
        if let Ok(stream) = std::os::unix::net::UnixStream::connect(&path) {
            for line in std::io::BufReader::new(stream).lines() {
                let Ok(line) = line else { break };
                if let Some(on) = screencast(&line)
                    && on != sharing
                {
                    sharing = on;
                    if !changed(on) {
                        return;
                    }
                }
            }
        }
        // The socket went away: sharing ended with it.
        if sharing {
            sharing = false;
            if !changed(false) {
                return;
            }
        }
        std::thread::sleep(std::time::Duration::from_secs(5));
    }
}

/// `screencast>>1,0` → started.
fn screencast(line: &str) -> Option<bool> {
    let rest = line.strip_prefix("screencast>>")?;
    Some(rest.split(',').next()? == "1")
}

#[cfg(test)]
mod tests {
    #[test]
    fn reads_screencast_events() {
        assert_eq!(super::screencast("screencast>>1,0"), Some(true));
        assert_eq!(super::screencast("screencast>>0,1"), Some(false));
        assert_eq!(super::screencast("workspace>>2"), None);
    }
}
