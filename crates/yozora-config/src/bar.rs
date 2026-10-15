//! `bar.toml`: Yozora Bar's own settings.

use std::{collections::BTreeMap, path::Path};

use serde::{Deserialize, Serialize};

use crate::{
    load::{self, Loaded, Warning},
    theme::ThemeConfig,
};

pub const FILE: &str = "bar.toml";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum BarStyle {
    /// Edge to edge, flush with the top of the screen.
    #[default]
    Flush,
    /// Inset with rounded corners.
    Floating,
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct BarSection {
    pub height: u32,
    pub style: BarStyle,
    /// Reserve screen space so windows don't go under the bar.
    pub reserve_space: bool,
    /// Connector names to show the bar on. Empty means every monitor.
    pub monitors: Vec<String>,
    /// Ask Hyprland to blur behind the bar. Off makes the tint more opaque instead.
    pub blur: bool,
}

impl Default for BarSection {
    fn default() -> Self {
        Self { height: 34, style: BarStyle::Flush, reserve_space: true, monitors: Vec::new(), blur: true }
    }
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct Zones {
    pub left: Vec<String>,
    pub center: Vec<String>,
    pub right: Vec<String>,
}

impl Default for Zones {
    /// The default bar is minimal. Everything else is opt-in.
    fn default() -> Self {
        Self {
            left: vec!["workspaces".into()],
            center: Vec::new(),
            right: vec!["clock".into(), "control-center".into()],
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct ZonesOverride {
    pub left: Option<Vec<String>>,
    pub center: Option<Vec<String>>,
    pub right: Option<Vec<String>>,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct MonitorOverride {
    pub enabled: Option<bool>,
    pub modules: ZonesOverride,
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct ClockConfig {
    /// strftime format for the bar item.
    pub format: String,
    /// Show ISO week numbers in the calendar.
    pub week_numbers: bool,
}

impl Default for ClockConfig {
    fn default() -> Self {
        Self { format: "%a %-d %b  %-I:%M %p".into(), week_numbers: true }
    }
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct WorkspacesConfig {
    /// Workspaces 1 to `count` are always shown, even when empty.
    pub count: u32,
    /// Only show workspaces that live on this bar's monitor.
    pub per_monitor: bool,
}

impl Default for WorkspacesConfig {
    fn default() -> Self {
        Self { count: 5, per_monitor: false }
    }
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct WindowConfig {
    /// Longest title shown, in characters.
    pub max_length: usize,
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self { max_length: 60 }
    }
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct MediaConfig {
    pub max_length: usize,
}

impl Default for MediaConfig {
    fn default() -> Self {
        Self { max_length: 36 }
    }
}

/// Commands behind the menu and the control center's footer. Run through `sh -c`.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct SessionConfig {
    pub lock: String,
    pub logout: String,
    pub reboot: String,
    pub poweroff: String,
    pub settings: String,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            lock: "loginctl lock-session".into(),
            logout: "uwsm stop || loginctl terminate-session \"$XDG_SESSION_ID\"".into(),
            reboot: "systemctl reboot".into(),
            poweroff: "systemctl poweroff".into(),
            settings: "yozora-settings".into(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum OsdPosition {
    /// Above the bottom edge of the screen.
    #[default]
    Bottom,
    /// Under the bar.
    Top,
}

/// The on-screen display: a capsule that shows volume, brightness and keyboard layout changes.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct OsdConfig {
    pub enabled: bool,
    pub position: OsdPosition,
    /// How long it stays after the last change, in milliseconds.
    pub timeout_ms: u32,
    /// Show keyboard layout switches.
    pub keyboard_layout: bool,
}

impl Default for OsdConfig {
    fn default() -> Self {
        Self { enabled: true, position: OsdPosition::Bottom, timeout_ms: 1400, keyboard_layout: true }
    }
}

/// A `custom/<name>` module: a long-running command that prints one JSON object per line.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct CustomConfig {
    pub exec: String,
    /// Re-run `exec` every this many seconds instead of reading a stream.
    pub interval: Option<u64>,
    pub on_click: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct BarConfig {
    pub version: u32,
    pub bar: BarSection,
    pub modules: Zones,
    pub monitor: BTreeMap<String, MonitorOverride>,
    pub clock: ClockConfig,
    pub workspaces: WorkspacesConfig,
    pub window: WindowConfig,
    pub media: MediaConfig,
    pub session: SessionConfig,
    pub osd: OsdConfig,
    pub custom: BTreeMap<String, CustomConfig>,
}

impl BarConfig {
    /// Whether the bar shows on `connector`.
    pub fn shows_on(&self, connector: &str) -> bool {
        if let Some(enabled) = self.monitor.get(connector).and_then(|m| m.enabled) {
            return enabled;
        }
        self.bar.monitors.is_empty() || self.bar.monitors.iter().any(|m| m == connector)
    }

    /// The module zones for `connector`, after its overrides.
    pub fn zones_for(&self, connector: &str) -> Zones {
        let mut zones = self.modules.clone();
        if let Some(over) = self.monitor.get(connector) {
            let m = &over.modules;
            if let Some(v) = &m.left {
                zones.left = v.clone();
            }
            if let Some(v) = &m.center {
                zones.center = v.clone();
            }
            if let Some(v) = &m.right {
                zones.right = v.clone();
            }
        }
        zones
    }
}

/// Everything the bar reads, loaded together.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BarSettings {
    pub bar: BarConfig,
    pub theme: ThemeConfig,
}

/// Loads `theme.toml` and `bar.toml` from `dir`. The bar's `[theme]` table overrides keys from
/// `theme.toml`.
pub fn load(dir: &Path) -> anyhow::Result<Loaded<BarSettings>> {
    let (mut theme_table, theme_text) = load::read_table(&dir.join("theme.toml"))?;
    let (mut bar_table, bar_text) = load::read_table(&dir.join(FILE))?;

    let mut warnings: Vec<Warning> = Vec::new();
    if let Some(toml::Value::Table(over)) = bar_table.remove("theme") {
        merge(&mut theme_table, over);
    }
    let theme: Loaded<ThemeConfig> = load::deserialize(theme_table, &theme_text, "theme.toml")?;
    let bar: Loaded<BarConfig> = load::deserialize(bar_table, &bar_text, FILE)?;
    warnings.extend(theme.warnings);
    warnings.extend(bar.warnings);

    let mut value = BarSettings { bar: bar.value, theme: theme.value };
    validate(&mut value.bar, &mut warnings);
    Ok(Loaded { value, warnings })
}

pub(crate) fn merge(base: &mut toml::Table, over: toml::Table) {
    for (key, value) in over {
        match (base.get_mut(&key), value) {
            (Some(toml::Value::Table(b)), toml::Value::Table(o)) => merge(b, o),
            (_, value) => {
                base.insert(key, value);
            }
        }
    }
}

/// Clamps values that parse but make no sense.
fn validate(bar: &mut BarConfig, warnings: &mut Vec<Warning>) {
    let mut warn = |message: String| {
        warnings.push(Warning { file: FILE.into(), line: None, message });
    };
    if !(24..=64).contains(&bar.bar.height) {
        warn(format!("bar.height {} is outside 24–64; using 34", bar.bar.height));
        bar.bar.height = 34;
    }
    if bar.workspaces.count > 20 {
        warn("workspaces.count is above 20; using 20".into());
        bar.workspaces.count = 20;
    }
    if !(500..=10_000).contains(&bar.osd.timeout_ms) {
        warn(format!("osd.timeout_ms {} is outside 500–10000; using 1400", bar.osd.timeout_ms));
        bar.osd.timeout_ms = 1400;
    }
    if bar.version > 1 {
        warn(format!("version {} is newer than this bar understands", bar.version));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir_with(files: &[(&str, &str)]) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "yozora-test-{}-{}",
            std::process::id(),
            files.iter().map(|f| f.1.len()).sum::<usize>()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        for (name, text) in files {
            std::fs::write(dir.join(name), text).unwrap();
        }
        dir
    }

    #[test]
    fn missing_files_mean_defaults() {
        let loaded = load(Path::new("/nonexistent/yozora")).unwrap();
        assert_eq!(loaded.value, BarSettings::default());
        assert!(loaded.warnings.is_empty());
    }

    #[test]
    fn bar_theme_overrides_shared_theme() {
        let dir = dir_with(&[
            ("theme.toml", "accent = \"aurora\"\nglass = \"clear\"\n[motion]\nreduced = true\n"),
            ("bar.toml", "[theme]\naccent = \"dawn\"\n\n[modules]\nright = [\"clock\"]\n"),
        ]);
        let loaded = load(&dir).unwrap().value;
        assert_eq!(loaded.theme.accent, crate::theme::Accent::Dawn);
        assert_eq!(loaded.theme.glass, crate::theme::Glass::Clear);
        assert!(loaded.theme.motion.reduced);
        assert_eq!(loaded.bar.modules.right, vec!["clock".to_string()]);
        assert_eq!(loaded.bar.modules.left, vec!["workspaces".to_string()]);
    }

    #[test]
    fn monitor_overrides() {
        let text = "[bar]\nmonitors = [\"DP-1\", \"HDMI-A-1\"]\n[monitor.\"HDMI-A-1\"]\nmodules.right = [\"clock\"]\n[monitor.\"DP-2\"]\nenabled = true\n";
        let bar: BarConfig = load::deserialize(text.parse().unwrap(), text, FILE).unwrap().value;
        assert!(bar.shows_on("DP-1"));
        assert!(bar.shows_on("DP-2"));
        assert!(!bar.shows_on("eDP-1"));
        assert_eq!(bar.zones_for("HDMI-A-1").right, vec!["clock".to_string()]);
        assert_eq!(bar.zones_for("DP-1").right, Zones::default().right);
    }

    #[test]
    fn out_of_range_height_is_clamped() {
        let dir = dir_with(&[("bar.toml", "[bar]\nheight = 300\n")]);
        let loaded = load(&dir).unwrap();
        assert_eq!(loaded.value.bar.bar.height, 34);
        assert_eq!(loaded.warnings.len(), 1);
    }

    #[test]
    fn osd_section() {
        let text = "[osd]\nposition = \"top\"\ntimeout_ms = 2000\n";
        let bar: BarConfig = load::deserialize(text.parse().unwrap(), text, FILE).unwrap().value;
        assert_eq!(bar.osd.position, OsdPosition::Top);
        assert_eq!(bar.osd.timeout_ms, 2000);
        assert!(bar.osd.enabled && bar.osd.keyboard_layout);
        let dir = dir_with(&[("bar.toml", "[osd]\ntimeout_ms = 20\n")]);
        assert_eq!(load(&dir).unwrap().value.bar.osd.timeout_ms, 1400);
    }
}
