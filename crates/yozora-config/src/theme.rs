//! `theme.toml`: appearance and motion shared by every Yozora app.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemeMode {
    #[default]
    Midnight,
    Daybreak,
    /// Follows the portal's `color-scheme`.
    Auto,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Accent {
    #[default]
    Starlight,
    Aurora,
    Nebula,
    Dawn,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Glass {
    Clear,
    #[default]
    Balanced,
    Frosted,
}

#[derive(Clone, Copy, Debug, PartialEq, Deserialize, Serialize)]
pub struct SpringParams {
    pub stiffness: f32,
    pub damping: f32,
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct Motion {
    /// Replaces every spring with a 120 ms fade.
    pub reduced: bool,
    pub popover: SpringParams,
    pub indicator: SpringParams,
}

impl Default for Motion {
    fn default() -> Self {
        Self {
            reduced: false,
            popover: SpringParams { stiffness: 380.0, damping: 28.0 },
            indicator: SpringParams { stiffness: 520.0, damping: 34.0 },
        }
    }
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct ThemeConfig {
    pub version: u32,
    pub theme: ThemeMode,
    pub accent: Accent,
    pub glass: Glass,
    /// Font family, with Inter and then the system UI font as fallbacks.
    pub font: String,
    pub motion: Motion,
}

impl Default for ThemeConfig {
    fn default() -> Self {
        Self {
            version: 1,
            theme: ThemeMode::default(),
            accent: Accent::default(),
            glass: Glass::default(),
            font: "Geist".into(),
            motion: Motion::default(),
        }
    }
}
