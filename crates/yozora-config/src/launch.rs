//! `launch.toml`: Yozora Launch's own settings.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::{
    load::{self, Loaded, Warning},
    theme::ThemeConfig,
};

pub const FILE: &str = "launch.toml";

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct LaunchConfig {
    pub version: u32,
    /// Runs terminal apps and `$` commands. Empty picks `$TERMINAL`, then the first installed of
    /// a few common terminals.
    pub terminal: String,
    /// The web search URL, with `{}` where the query goes.
    pub web_search: String,
    /// The most results in a section.
    pub max_results: u32,
    /// Show apps suggested for the time of day when nothing is typed.
    pub suggestions: bool,
    /// Search files as well as apps once three letters are typed.
    pub files_in_search: bool,
    /// Opens projects. Empty picks the first installed of Zed, VS Code and a few others, then
    /// `$VISUAL` or `$EDITOR` in the terminal.
    pub editor: String,
    /// Folders searched for git repositories, four levels deep. `~` is the home folder.
    pub project_roots: Vec<String>,
    /// Folders of script commands, in Raycast's format (`# @raycast.title …`, or `@vicinae.`).
    pub script_dirs: Vec<String>,
    /// Search the browsers' bookmarks and history.
    pub browser: bool,
    /// Convert currencies, with the European Central Bank's daily rates, fetched twice a day.
    pub currency: bool,
    /// Commands behind the built-in actions. Run through `sh -c`.
    pub commands: Commands,
}

impl Default for LaunchConfig {
    fn default() -> Self {
        Self {
            version: 1,
            terminal: String::new(),
            web_search: "https://duckduckgo.com/?q={}".into(),
            max_results: 6,
            suggestions: true,
            files_in_search: true,
            editor: String::new(),
            project_roots: vec!["~".into()],
            script_dirs: vec!["~/.config/yozora/scripts".into(), "~/.local/share/vicinae/scripts".into()],
            browser: true,
            currency: true,
            commands: Commands::default(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct Commands {
    pub lock: String,
    pub logout: String,
    pub suspend: String,
    pub reboot: String,
    pub poweroff: String,
}

impl Default for Commands {
    fn default() -> Self {
        Self {
            lock: "loginctl lock-session".into(),
            logout: "uwsm stop || loginctl terminate-session \"$XDG_SESSION_ID\"".into(),
            suspend: "systemctl suspend".into(),
            reboot: "systemctl reboot".into(),
            poweroff: "systemctl poweroff".into(),
        }
    }
}

/// Everything the launcher reads, loaded together.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LaunchSettings {
    pub launch: LaunchConfig,
    pub theme: ThemeConfig,
}

/// Loads `theme.toml` and `launch.toml` from `dir`. The `[theme]` table in `launch.toml`
/// overrides keys from `theme.toml`.
pub fn load(dir: &Path) -> anyhow::Result<Loaded<LaunchSettings>> {
    let (mut theme_table, theme_text) = load::read_table(&dir.join("theme.toml"))?;
    let (mut table, text) = load::read_table(&dir.join(FILE))?;
    if let Some(toml::Value::Table(over)) = table.remove("theme") {
        crate::bar::merge(&mut theme_table, over);
    }
    let theme: Loaded<ThemeConfig> = load::deserialize(theme_table, &theme_text, "theme.toml")?;
    let launch: Loaded<LaunchConfig> = load::deserialize(table, &text, FILE)?;
    let mut warnings = theme.warnings;
    warnings.extend(launch.warnings);
    let mut value = LaunchSettings { launch: launch.value, theme: theme.value };
    validate(&mut value.launch, &mut warnings);
    Ok(Loaded { value, warnings })
}

/// Clamps values that parse but make no sense.
fn validate(c: &mut LaunchConfig, warnings: &mut Vec<Warning>) {
    let mut warn = |message: String| warnings.push(Warning { file: FILE.into(), line: None, message });
    if !(1..=20).contains(&c.max_results) {
        warn(format!("max_results {} is outside 1–20; using 6", c.max_results));
        c.max_results = 6;
    }
    if !c.web_search.contains("{}") {
        warn(format!("web_search {:?} has no {{}} for the query; using DuckDuckGo", c.web_search));
        c.web_search = LaunchConfig::default().web_search;
    }
    if c.version > 1 {
        warn(format!("version {} is newer than this version of Yozora Launch understands", c.version));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_and_validation() {
        let dir = std::env::temp_dir().join(format!("yozora-launch-cfg-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let loaded = load(&dir).unwrap();
        assert_eq!(loaded.value.launch, LaunchConfig::default());
        assert!(loaded.warnings.is_empty());

        std::fs::write(
            dir.join(FILE),
            "terminal = \"ghostty\"\nweb_search = \"https://example.com\"\nmax_results = 50\n[commands]\nlock = \"hyprlock\"\n",
        )
        .unwrap();
        let loaded = load(&dir).unwrap();
        let c = &loaded.value.launch;
        assert_eq!(c.terminal, "ghostty");
        assert_eq!(c.max_results, 6);
        assert_eq!(c.web_search, LaunchConfig::default().web_search);
        assert_eq!(c.commands.lock, "hyprlock");
        assert_eq!(c.commands.reboot, "systemctl reboot");
        assert_eq!(loaded.warnings.len(), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
