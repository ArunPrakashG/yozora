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
