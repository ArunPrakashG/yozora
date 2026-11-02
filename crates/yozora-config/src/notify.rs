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

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct NotifyConfig {
    pub version: u32,
    pub toasts: ToastSection,
    pub history: HistorySection,
}

impl Default for NotifyConfig {
    fn default() -> Self {
        Self { version: 1, toasts: ToastSection::default(), history: HistorySection::default() }
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
