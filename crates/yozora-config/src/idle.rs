//! `idle.toml`: what Yozora Idle does when nobody is at the keyboard. A time of 0 means never.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::load::{self, Loaded, Warning};

pub const FILE: &str = "idle.toml";

/// The longest wait that makes sense, in minutes: a day.
const MAX_MIN: u32 = 1440;

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct LockSection {
    /// Lock after this many minutes idle.
    pub after_min: u32,
    /// What locks the screen. It runs through `sh -c`, and a second lock while locked does nothing.
    pub command: String,
}

impl Default for LockSection {
    fn default() -> Self {
        Self { after_min: 10, command: "yozora-lock".into() }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct ScreenSection {
    /// Turn the monitors off after this many minutes idle. Any input turns them back on.
    pub off_after_min: u32,
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct SuspendSection {
    /// Suspend after this many minutes idle.
    pub after_min: u32,
    /// Lock the screen before any suspend, including one you start yourself, and hold the
    /// suspend until it is locked.
    pub lock_first: bool,
}

impl Default for SuspendSection {
    fn default() -> Self {
        Self { after_min: 0, lock_first: true }
    }
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct InhibitSection {
    /// Don't count as idle while an app asks for the screen to stay awake, like a video player
    /// or a call.
    pub respect: bool,
}

impl Default for InhibitSection {
    fn default() -> Self {
        Self { respect: true }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct IdleConfig {
    pub version: u32,
    pub lock: LockSection,
    pub screen: ScreenSection,
    pub suspend: SuspendSection,
    pub inhibit: InhibitSection,
}

pub fn load(dir: &Path) -> anyhow::Result<Loaded<IdleConfig>> {
    let (table, text) = load::read_table(&dir.join(FILE))?;
    let mut loaded: Loaded<IdleConfig> = load::deserialize(table, &text, FILE)?;
    if loaded.value.version == 0 {
        loaded.value.version = 1;
    }
    validate(&mut loaded.value, &mut loaded.warnings);
    Ok(loaded)
}

fn validate(c: &mut IdleConfig, warnings: &mut Vec<Warning>) {
    let mut warn = |message: String| warnings.push(Warning { file: FILE.into(), line: None, message });
    for (key, v) in [
        ("lock.after_min", &mut c.lock.after_min),
        ("screen.off_after_min", &mut c.screen.off_after_min),
        ("suspend.after_min", &mut c.suspend.after_min),
    ] {
        if *v > MAX_MIN {
            warn(format!("{key} {v} is above {MAX_MIN}; using {MAX_MIN}"));
            *v = MAX_MIN;
        }
    }
    if c.lock.command.trim().is_empty() {
        warn("lock.command is empty; using yozora-lock".into());
        c.lock.command = LockSection::default().command;
    }
    if c.version > 1 {
        warn(format!("version {} is newer than this version of Yozora Idle understands", c.version));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Loaded<IdleConfig> {
        let table = text.parse::<toml::Table>().unwrap();
        let mut l: Loaded<IdleConfig> = load::deserialize(table, text, FILE).unwrap();
        validate(&mut l.value, &mut l.warnings);
        l
    }

    #[test]
    fn defaults_lock_after_ten_minutes_and_nothing_else() {
        let c = parse("").value;
        assert_eq!((c.lock.after_min, c.screen.off_after_min, c.suspend.after_min), (10, 0, 0));
        assert!(c.suspend.lock_first && c.inhibit.respect);
    }

    #[test]
    fn silly_values_are_clamped_with_a_warning() {
        let l = parse("[lock]\nafter_min = 99999\ncommand = \" \"\n");
        assert_eq!(l.value.lock.after_min, 1440);
        assert_eq!(l.value.lock.command, "yozora-lock");
        assert_eq!(l.warnings.len(), 2);
    }
}
