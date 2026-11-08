//! `shell.toml`: which look the Yozora shell wears, and how it moves.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::load::{self, Loaded, Warning};

pub const FILE: &str = "shell.toml";

/// The looks, by the name the CLI and the file use.
pub const LOOKS: &[&str] = &["tension", "folio", "phosphor", "prism"];
/// How much the shell animates.
pub const FX: &[&str] = &["full", "calm", "still"];
/// Surface tension's two skies: always night, always day, or by the clock.
pub const TIMES: &[&str] = &["night", "day", "follow"];

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct ShellConfig {
    pub version: u32,
    pub look: String,
    /// Only Surface tension has a day sky (Daybreak); other looks ignore it.
    pub time: String,
    pub fx: String,
    /// With `time = "follow"`, day starts here and night starts at `night_from` (HH:MM).
    pub day_from: String,
    pub night_from: String,
}

impl Default for ShellConfig {
    fn default() -> Self {
        Self { version: 1, look: "tension".into(), time: "night".into(), fx: "full".into(), day_from: "07:00".into(), night_from: "19:00".into() }
    }
}

pub fn load(dir: &Path) -> anyhow::Result<Loaded<ShellConfig>> {
    let (table, text) = load::read_table(&dir.join(FILE))?;
    let mut loaded: Loaded<ShellConfig> = load::deserialize(table, &text, FILE)?;
    validate(&mut loaded.value, &mut loaded.warnings);
    Ok(loaded)
}

fn minutes(s: &str) -> Option<u32> {
    let (h, m) = s.split_once(':')?;
    let (h, m) = (h.parse::<u32>().ok()?, m.parse::<u32>().ok()?);
    (h < 24 && m < 60).then_some(h * 60 + m)
}

impl ShellConfig {
    /// Whether the day sky is up at `minute_of_day` under `time = "follow"`.
    pub fn is_day(&self, minute_of_day: u32) -> bool {
        let (day, night) = (minutes(&self.day_from).unwrap_or(420), minutes(&self.night_from).unwrap_or(1140));
        if day <= night { (day..night).contains(&minute_of_day) } else { minute_of_day >= day || minute_of_day < night }
    }
}

fn validate(c: &mut ShellConfig, warnings: &mut Vec<Warning>) {
    let mut warn = |message: String| warnings.push(Warning { file: FILE.into(), line: None, message });
    let defaults = ShellConfig::default();
    if !LOOKS.contains(&c.look.as_str()) {
        warn(format!("look {:?} is not one of {}; using {}", c.look, LOOKS.join(", "), defaults.look));
        c.look = defaults.look.clone();
    }
    if !TIMES.contains(&c.time.as_str()) {
        warn(format!("time {:?} is not one of {}; using {}", c.time, TIMES.join(", "), defaults.time));
        c.time = defaults.time.clone();
    }
    if !FX.contains(&c.fx.as_str()) {
        warn(format!("fx {:?} is not one of {}; using {}", c.fx, FX.join(", "), defaults.fx));
        c.fx = defaults.fx.clone();
    }
    for (key, v, d) in [("day_from", &mut c.day_from, &defaults.day_from), ("night_from", &mut c.night_from, &defaults.night_from)] {
        if minutes(v).is_none() {
            warn(format!("{key} {v:?} is not HH:MM; using {d}"));
            *v = d.clone();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn follows_the_clock_across_midnight() {
        let c = ShellConfig { day_from: "07:00".into(), night_from: "19:00".into(), ..Default::default() };
        assert!(c.is_day(8 * 60));
        assert!(!c.is_day(20 * 60));
        assert!(!c.is_day(3 * 60));
    }
}
