//! `lock.toml`: Yozora Lock's own settings.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::{
    load::{self, Loaded, Warning},
    theme::ThemeConfig,
};

pub const FILE: &str = "lock.toml";

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct BackgroundSection {
    /// An image to blur behind the lock screen. Empty uses the wallpaper from swaybg, hyprpaper
    /// or swww. `{output}` is replaced by the monitor's connector name, like `DP-1`.
    pub image: String,
    /// Blur the image. Off shows it as it is, under the same dark veil.
    pub blur: bool,
}

impl Default for BackgroundSection {
    fn default() -> Self {
        Self { image: String::new(), blur: true }
    }
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct ClockSection {
    /// strftime format for the time.
    pub format: String,
    /// strftime format for the date under it.
    pub date: String,
}

impl Default for ClockSection {
    fn default() -> Self {
        Self { format: "%-I:%M".into(), date: "%A, %-d %B".into() }
    }
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct ShowSection {
    /// What's playing, with previous, play or pause, and next.
    pub media: bool,
    /// How many notifications arrived while locked. Their contents are never shown.
    pub notifications: bool,
    /// Suspend, restart and shut down.
    pub power: bool,
}

impl Default for ShowSection {
    fn default() -> Self {
        Self { media: true, notifications: true, power: true }
    }
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct AuthSection {
    /// Wrong passwords in a row before a wait.
    pub tries: u32,
    /// The first wait, in seconds. It doubles each time.
    pub wait_s: u32,
}

impl Default for AuthSection {
    fn default() -> Self {
        Self { tries: 5, wait_s: 30 }
    }
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct LockConfig {
    pub version: u32,
    pub background: BackgroundSection,
    pub clock: ClockSection,
    pub show: ShowSection,
    pub auth: AuthSection,
}

impl Default for LockConfig {
    fn default() -> Self {
        Self {
            version: 1,
            background: BackgroundSection::default(),
            clock: ClockSection::default(),
            show: ShowSection::default(),
            auth: AuthSection::default(),
        }
    }
}

/// Everything the lock screen reads, loaded together.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LockSettings {
    pub lock: LockConfig,
    pub theme: ThemeConfig,
}

/// Loads `theme.toml` and `lock.toml` from `dir`. The `[theme]` table in `lock.toml` overrides
/// keys from `theme.toml`.
pub fn load(dir: &Path) -> anyhow::Result<Loaded<LockSettings>> {
    let (mut theme_table, theme_text) = load::read_table(&dir.join("theme.toml"))?;
    let (mut table, text) = load::read_table(&dir.join(FILE))?;
    if let Some(toml::Value::Table(over)) = table.remove("theme") {
        crate::bar::merge(&mut theme_table, over);
    }
    let theme: Loaded<ThemeConfig> = load::deserialize(theme_table, &theme_text, "theme.toml")?;
    let lock: Loaded<LockConfig> = load::deserialize(table, &text, FILE)?;
    let mut warnings = theme.warnings;
    warnings.extend(lock.warnings);
    let mut value = LockSettings { lock: lock.value, theme: theme.value };
    validate(&mut value.lock, &mut warnings);
    Ok(Loaded { value, warnings })
}

/// Clamps values that parse but make no sense.
fn validate(c: &mut LockConfig, warnings: &mut Vec<Warning>) {
    let mut warn = |message: String| warnings.push(Warning { file: FILE.into(), line: None, message });
    if !(1..=20).contains(&c.auth.tries) {
        warn(format!("auth.tries {} is outside 1–20; using 5", c.auth.tries));
        c.auth.tries = 5;
    }
    if c.auth.wait_s > 600 {
        warn("auth.wait_s is above 600; using 600".into());
        c.auth.wait_s = 600;
    }
    if c.clock.format.trim().is_empty() {
        c.clock.format = ClockSection::default().format;
    }
    if c.version > 1 {
        warn(format!("version {} is newer than this version of Yozora Lock understands", c.version));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_and_clamping() {
        let dir = std::env::temp_dir().join(format!("yozora-lock-cfg-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let loaded = load(&dir).unwrap();
        assert_eq!(loaded.value.lock, LockConfig::default());
        assert!(loaded.warnings.is_empty());

        std::fs::write(dir.join(FILE), "[auth]\ntries = 0\n\n[show]\nmedia = false\n\n[theme]\naccent = \"nebula\"\n")
            .unwrap();
        let loaded = load(&dir).unwrap();
        assert_eq!(loaded.value.lock.auth.tries, 5);
        assert!(!loaded.value.lock.show.media);
        assert_eq!(loaded.value.theme.accent, crate::theme::Accent::Nebula);
        assert_eq!(loaded.warnings.len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
