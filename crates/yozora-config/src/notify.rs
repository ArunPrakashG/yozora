//! `notify.toml`: Yozora Notify's own settings.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::{
    load::{self, Loaded, Warning},
    theme::ThemeConfig,
};

pub const FILE: &str = "notify.toml";

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct ToastSection {
    /// How long a normal notification stays when the app doesn't say, in milliseconds.
    pub timeout_ms: u32,
    /// The most toasts on screen at once. The rest wait in the center.
    pub max_visible: u32,
    /// `focused` for the monitor with keyboard focus, or a connector name like `DP-1`.
    pub monitor: String,
    /// Show low-urgency notifications as toasts too. Off sends them straight to the center.
    pub show_low: bool,
}

impl Default for ToastSection {
    fn default() -> Self {
        Self { timeout_ms: 5000, max_visible: 4, monitor: "focused".into(), show_low: false }
    }
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct HistorySection {
    /// How many notifications the center keeps.
    pub limit: u32,
}

impl Default for HistorySection {
    fn default() -> Self {
        Self { limit: 200 }
    }
}

/// Sorting by Jev (TypeSafe AI): how much each notification needs attention. Off unless turned
/// on with a key; when on, the app name, title and (with `send_body`) the body leave the machine.
#[derive(Clone, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct JevSection {
    pub enabled: bool,
    pub api_key: String,
    pub url: String,
    /// Send the body too, not just the app and title.
    pub send_body: bool,
    /// Apps never sent, by name or desktop entry, ignoring case.
    pub skip_apps: Vec<String>,
    /// Below these odds a toast leaves early and doesn't count as unread.
    pub quiet_below: f64,
    /// At or above these odds a notification is shown during Do Not Disturb.
    pub breakthrough: f64,
}

impl Default for JevSection {
    fn default() -> Self {
        Self {
            enabled: false,
            api_key: String::new(),
            url: "https://api.typesafe.ai/v1/systemone".into(),
            send_body: true,
            skip_apps: Vec::new(),
            quiet_below: 0.25,
            breakthrough: 0.85,
        }
    }
}

impl JevSection {
    /// On, with a key.
    pub fn active(&self) -> bool {
        self.enabled && !self.api_key.trim().is_empty()
    }
}

// The key never goes to logs.
impl std::fmt::Debug for JevSection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JevSection")
            .field("enabled", &self.enabled)
            .field("api_key", &if self.api_key.is_empty() { "" } else { "(set)" })
            .field("url", &self.url)
            .field("send_body", &self.send_body)
            .field("skip_apps", &self.skip_apps)
            .field("quiet_below", &self.quiet_below)
            .field("breakthrough", &self.breakthrough)
            .finish()
    }
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct NotifyConfig {
    pub version: u32,
    pub toasts: ToastSection,
    pub history: HistorySection,
    pub jev: JevSection,
}

impl Default for NotifyConfig {
    fn default() -> Self {
        Self {
            version: 1,
            toasts: ToastSection::default(),
            history: HistorySection::default(),
            jev: JevSection::default(),
        }
    }
}

/// Everything the daemon reads, loaded together.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NotifySettings {
    pub notify: NotifyConfig,
    pub theme: ThemeConfig,
}

/// Loads `theme.toml` and `notify.toml` from `dir`. The `[theme]` table in `notify.toml`
/// overrides keys from `theme.toml`.
pub fn load(dir: &Path) -> anyhow::Result<Loaded<NotifySettings>> {
    let (mut theme_table, theme_text) = load::read_table(&dir.join("theme.toml"))?;
    let (mut table, text) = load::read_table(&dir.join(FILE))?;
    if let Some(toml::Value::Table(over)) = table.remove("theme") {
        crate::bar::merge(&mut theme_table, over);
    }
    let theme: Loaded<ThemeConfig> = load::deserialize(theme_table, &theme_text, "theme.toml")?;
    let notify: Loaded<NotifyConfig> = load::deserialize(table, &text, FILE)?;
    let mut warnings = theme.warnings;
    warnings.extend(notify.warnings);
    let mut value = NotifySettings { notify: notify.value, theme: theme.value };
    validate(&mut value.notify, &mut warnings);
    Ok(Loaded { value, warnings })
}

/// Clamps values that parse but make no sense.
fn validate(c: &mut NotifyConfig, warnings: &mut Vec<Warning>) {
    let mut warn = |message: String| warnings.push(Warning { file: FILE.into(), line: None, message });
    if !(1000..=120_000).contains(&c.toasts.timeout_ms) {
        warn(format!("toasts.timeout_ms {} is outside 1000–120000; using 5000", c.toasts.timeout_ms));
        c.toasts.timeout_ms = 5000;
    }
    if !(1..=8).contains(&c.toasts.max_visible) {
        warn(format!("toasts.max_visible {} is outside 1–8; using 4", c.toasts.max_visible));
        c.toasts.max_visible = 4;
    }
    if c.history.limit > 2000 {
        warn("history.limit is above 2000; using 2000".into());
        c.history.limit = 2000;
    }
    for (name, v, default) in
        [("jev.quiet_below", &mut c.jev.quiet_below, 0.25), ("jev.breakthrough", &mut c.jev.breakthrough, 0.85)]
    {
        if !(0.0..=1.0).contains(v) {
            warn(format!("{name} {v} is outside 0–1; using {default}"));
            *v = default;
        }
    }
    if c.jev.enabled && c.jev.api_key.trim().is_empty() {
        warn("jev.enabled is on but jev.api_key is empty; Jev stays off".into());
    }
    if c.version > 1 {
        warn(format!("version {} is newer than this version of Yozora Notify understands", c.version));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_without_files_and_overrides_with() {
        let dir = std::env::temp_dir().join(format!("yozora-notify-cfg-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let loaded = load(&dir).unwrap();
        assert_eq!(loaded.value.notify, NotifyConfig::default());
        assert!(loaded.warnings.is_empty());

        std::fs::write(dir.join("theme.toml"), "accent = \"aurora\"\n").unwrap();
        std::fs::write(dir.join(FILE), "[toasts]\ntimeout_ms = 8000\nmax_visible = 99\n\n[theme]\naccent = \"dawn\"\n")
            .unwrap();
        let loaded = load(&dir).unwrap();
        assert_eq!(loaded.value.notify.toasts.timeout_ms, 8000);
        assert_eq!(loaded.value.notify.toasts.max_visible, 4);
        assert_eq!(loaded.value.theme.accent, crate::theme::Accent::Dawn);
        assert_eq!(loaded.warnings.len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
