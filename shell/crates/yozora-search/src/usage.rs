//! What was launched and when, for ranking: frecency with a 7-day half-life, a boost for apps
//! usually opened at this hour, and pins.
//!
//! Saved to `$XDG_STATE_HOME/yozora/launch.json` after every launch, on a background thread.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

/// Launch times kept per item, for the hour-of-day boost.
const HISTORY: usize = 64;
const HALF_LIFE_DAYS: f64 = 7.0;
/// The boost needs this many launches before it means anything.
const HOUR_MIN: usize = 5;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub count: u32,
    /// Unix seconds.
    pub last: i64,
    /// Local hour of day (0–23) of recent launches, oldest first.
    #[serde(default)]
    pub hours: Vec<u8>,
}

impl Entry {
    /// Higher is more relevant. `now` in Unix seconds, `hour` the local hour.
    pub fn score(&self, now: i64, hour: u8) -> f64 {
        let days = (now - self.last).max(0) as f64 / 86_400.0;
        let recency = (-days / HALF_LIFE_DAYS * std::f64::consts::LN_2).exp();
        f64::from(self.count) * recency * self.hour_boost(hour)
    }

    /// 1.0 is neutral. An app opened mostly around this hour gets up to about 1.6 times.
    fn hour_boost(&self, hour: u8) -> f64 {
        if self.hours.len() < HOUR_MIN {
            return 1.0;
        }
        // Count this hour and its neighbours, so 9:58 and 10:02 help each other.
        let near = |h: u8| {
            let d = (i16::from(h) - i16::from(hour)).rem_euclid(24);
            d == 0 || d == 1 || d == 23
        };
        let share = self.hours.iter().filter(|h| near(**h)).count() as f64 / self.hours.len() as f64;
        // Three hours out of 24 is 0.125 when launches are spread evenly.
        1.0 + ((share - 0.125) * 0.7).clamp(-0.2, 0.6)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Usage {
    #[serde(default)]
    pub items: HashMap<String, Entry>,
    /// Pinned keys, in the order they show.
    #[serde(default)]
    pub pins: Vec<String>,
    /// Apps left out of results.
    #[serde(default)]
    pub hidden: Vec<String>,
}

impl Usage {
    pub fn load(path: &Path) -> Self {
        match std::fs::read(path) {
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|e| {
                tracing::warn!("{}: {e}; starting over", path.display());
                Self::default()
            }),
            Err(_) => Self::default(),
        }
    }

    /// Writes atomically, so a crash never leaves half a file.
    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.part");
        std::fs::write(&tmp, serde_json::to_vec(self)?)?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }

    pub fn record(&mut self, key: &str, now: i64, hour: u8) {
        let e = self.items.entry(key.to_string()).or_default();
        e.count = e.count.saturating_add(1);
        e.last = now;
        e.hours.push(hour);
        if e.hours.len() > HISTORY {
            e.hours.remove(0);
        }
    }

    pub fn score(&self, key: &str, now: i64, hour: u8) -> f64 {
        self.items.get(key).map_or(0.0, |e| e.score(now, hour))
    }

    pub fn is_pinned(&self, key: &str) -> bool {
        self.pins.iter().any(|p| p == key)
    }

    /// Pins or unpins `key`; returns whether it's pinned now.
    pub fn toggle_pin(&mut self, key: &str) -> bool {
        if let Some(i) = self.pins.iter().position(|p| p == key) {
            self.pins.remove(i);
            false
        } else {
            self.pins.push(key.to_string());
            true
        }
    }

    pub fn is_hidden(&self, key: &str) -> bool {
        self.hidden.iter().any(|h| h == key)
    }

    /// Leaves `key` out of results, and unpins it.
    pub fn hide(&mut self, key: &str) {
        if !self.is_hidden(key) {
            self.hidden.push(key.to_string());
        }
        self.pins.retain(|p| p != key);
    }

    /// Keys by score, best first, leaving out those scoring nothing.
    pub fn top(&self, now: i64, hour: u8, n: usize) -> Vec<&str> {
        let mut all: Vec<(&str, f64)> =
            self.items.iter().map(|(k, e)| (k.as_str(), e.score(now, hour))).filter(|(_, s)| *s > 0.01).collect();
        all.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(b.0)));
        all.into_iter().take(n).map(|(k, _)| k).collect()
    }

    /// Drops entries nobody has launched for a long time, so the file doesn't grow forever.
    pub fn prune(&mut self, now: i64) {
        self.items.retain(|_, e| e.score(now, 12) > 0.001 || now - e.last < 180 * 86_400);
    }
}

pub fn state_file() -> PathBuf {
    std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))
        .unwrap_or_else(|| "/tmp".into())
        .join("yozora/launch.json")
}

/// Unix seconds and the local hour.
pub fn now() -> (i64, u8) {
    use chrono::Timelike;
    let t = chrono::Local::now();
    (t.timestamp(), t.hour() as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: i64 = 86_400;

    #[test]
    fn recency_halves_weekly() {
        let e = Entry { count: 8, last: 0, hours: vec![] };
        assert!((e.score(0, 0) - 8.0).abs() < 1e-9);
        assert!((e.score(7 * DAY, 0) - 4.0).abs() < 1e-9);
        assert!((e.score(14 * DAY, 0) - 2.0).abs() < 1e-9);
    }

    #[test]
    fn usual_hours_win() {
        let morning = Entry { count: 10, last: 0, hours: vec![9; 10] };
        let spread = Entry { count: 10, last: 0, hours: (0..10).map(|h| h * 2).collect() };
        assert!(morning.score(0, 9) > spread.score(0, 9));
        assert!(morning.score(0, 10) > morning.score(0, 20));
        // Too little history: no boost either way.
        let few = Entry { count: 3, last: 0, hours: vec![9, 9, 9] };
        assert_eq!(few.score(0, 9), few.score(0, 20));
    }

    #[test]
    fn records_pins_and_ranks() {
        let mut u = Usage::default();
        for _ in 0..3 {
            u.record("firefox.desktop", 100, 9);
        }
        u.record("foot.desktop", 100, 9);
        assert_eq!(u.top(100, 9, 5), vec!["firefox.desktop", "foot.desktop"]);
        assert!(u.toggle_pin("foot.desktop"));
        assert!(u.is_pinned("foot.desktop"));
        assert!(!u.toggle_pin("foot.desktop"));
        assert!(u.pins.is_empty());

        let path = std::env::temp_dir().join(format!("yozora-launch-usage-{}/launch.json", std::process::id()));
        u.save(&path).unwrap();
        assert_eq!(Usage::load(&path), u);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }
}
