//! Screen brightness through `/sys/class/backlight` and logind.
//!
//! The kernel doesn't announce backlight changes, so nothing is watched: brightness keys call
//! `yozora-bar osd brightness +5%`, which changes it here and shows the OSD with the result.
//! Writing goes through logind's `SetBrightness`, which needs no udev rule or group.

use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::Context as _;

const SYSFS: &str = "/sys/class/backlight";

/// A backlight device.
#[derive(Clone, Debug, PartialEq)]
pub struct Backlight {
    pub name: String,
    pub path: PathBuf,
}

/// What a brightness argument asks for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Change {
    /// A fraction of the maximum, 0.0 to 1.0.
    Set(f32),
    /// Up or down by a fraction of the maximum.
    By(f32),
    /// Leave it, just show it.
    Show,
}

/// Parses `+5%`, `-10%`, `40%`, `40` or nothing.
pub fn parse(arg: Option<&str>) -> Result<Change, String> {
    let Some(arg) = arg.map(str::trim).filter(|a| !a.is_empty()) else { return Ok(Change::Show) };
    let bad = || format!("brightness takes +N%, -N% or N%, not {arg:?}");
    let digits = arg.trim_end_matches('%');
    let (sign, digits) = match digits.as_bytes().first() {
        Some(b'+') => (1.0, &digits[1..]),
        Some(b'-') => (-1.0, &digits[1..]),
        _ => (0.0, digits),
    };
    let n: f32 = digits.parse().map_err(|_| bad())?;
    if !(0.0..=100.0).contains(&n) {
        return Err(bad());
    }
    Ok(if sign == 0.0 { Change::Set(n / 100.0) } else { Change::By(sign * n / 100.0) })
}

/// The device to use: a raw (vendor) backlight before firmware and platform ones, as the
/// kernel documentation advises.
pub fn find() -> anyhow::Result<Backlight> {
    find_in(Path::new(SYSFS))
}

fn find_in(dir: &Path) -> anyhow::Result<Backlight> {
    let mut found: Vec<(u8, Backlight)> = fs::read_dir(dir)
        .with_context(|| format!("reading {}", dir.display()))?
        .flatten()
        .map(|e| {
            let path = e.path();
            let rank = match fs::read_to_string(path.join("type")).unwrap_or_default().trim() {
                "raw" => 0,
                "platform" => 1,
                _ => 2,
            };
            (rank, Backlight { name: e.file_name().to_string_lossy().into_owned(), path })
        })
        .collect();
    found.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.name.cmp(&b.1.name)));
    found.into_iter().next().map(|(_, b)| b).context("this computer has no backlight")
}

fn read_u32(path: &Path) -> anyhow::Result<u32> {
    let text = fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    text.trim().parse().with_context(|| format!("parsing {}", path.display()))
}

/// The current brightness, 0.0 to 1.0.
pub fn get(b: &Backlight) -> anyhow::Result<f32> {
    let max = read_u32(&b.path.join("max_brightness"))?.max(1);
    Ok(read_u32(&b.path.join("brightness"))? as f32 / max as f32)
}

/// Where `change` takes `current`. Never all the way to black from a step down, so the screen
/// can't be turned off by accident.
pub fn target(current: f32, change: Change) -> f32 {
    match change {
        Change::Set(v) => v.clamp(0.0, 1.0),
        Change::By(d) if d < 0.0 => (current + d).max(0.01).min(current),
        Change::By(d) => (current + d).clamp(0.0, 1.0),
        Change::Show => current,
    }
}

/// Applies `change` and returns the new brightness.
pub fn apply(change: Change) -> anyhow::Result<f32> {
    let b = find()?;
    let current = get(&b)?;
    let want = target(current, change);
    if change == Change::Show || (want - current).abs() < f32::EPSILON {
        return Ok(current);
    }
    let max = read_u32(&b.path.join("max_brightness"))?;
    let raw = (want * max as f32).round() as u32;
    async_io::block_on(async {
        let conn = zbus::Connection::system().await.context("connecting to the system bus")?;
        conn.call_method(
            Some("org.freedesktop.login1"),
            "/org/freedesktop/login1/session/auto",
            Some("org.freedesktop.login1.Session"),
            "SetBrightness",
            &("backlight", b.name.as_str(), raw),
        )
        .await
        .context("logind SetBrightness")?;
        anyhow::Ok(())
    })?;
    Ok(raw as f32 / max.max(1) as f32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_arguments() {
        assert_eq!(parse(None), Ok(Change::Show));
        assert_eq!(parse(Some("+5%")), Ok(Change::By(0.05)));
        assert_eq!(parse(Some("-10%")), Ok(Change::By(-0.1)));
        assert_eq!(parse(Some("40%")), Ok(Change::Set(0.4)));
        assert_eq!(parse(Some("40")), Ok(Change::Set(0.4)));
        assert!(parse(Some("loud")).is_err());
        assert!(parse(Some("140%")).is_err());
    }

    #[test]
    fn steps_down_stop_short_of_black() {
        assert_eq!(target(0.03, Change::By(-0.05)), 0.01);
        assert_eq!(target(0.005, Change::By(-0.05)), 0.005);
        assert_eq!(target(0.98, Change::By(0.05)), 1.0);
        assert_eq!(target(0.5, Change::Set(0.0)), 0.0);
    }

    #[test]
    fn prefers_raw_backlights() {
        let dir = std::env::temp_dir().join(format!("yozora-backlight-{}", std::process::id()));
        for (name, kind) in [("acpi_video0", "firmware"), ("intel_backlight", "raw")] {
            fs::create_dir_all(dir.join(name)).unwrap();
            fs::write(dir.join(name).join("type"), kind).unwrap();
            fs::write(dir.join(name).join("max_brightness"), "1000\n").unwrap();
            fs::write(dir.join(name).join("brightness"), "250\n").unwrap();
        }
        let b = find_in(&dir).unwrap();
        assert_eq!(b.name, "intel_backlight");
        assert_eq!(get(&b).unwrap(), 0.25);
        fs::remove_dir_all(&dir).unwrap();
    }
}
