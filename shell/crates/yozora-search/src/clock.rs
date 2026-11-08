//! Time zones: `time in tokyo`, `london time`, and `3pm ist to pst` or `15:30 in new york`.

use chrono::{DateTime, FixedOffset, Local, NaiveTime, Offset, TimeZone, Utc};
use chrono_tz::Tz;

use crate::calc::Answer;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Zone {
    Local,
    Tz(Tz),
    Fixed(FixedOffset),
}

impl Zone {
    fn offset_at(self, t: DateTime<Utc>) -> FixedOffset {
        match self {
            Zone::Local => Local.offset_from_utc_datetime(&t.naive_utc()).fix(),
            Zone::Tz(tz) => tz.offset_from_utc_datetime(&t.naive_utc()).fix(),
            Zone::Fixed(o) => o,
        }
    }

    /// `t`'s date in this zone, then the local time on it.
    fn at(self, day: DateTime<Utc>, time: NaiveTime) -> Option<DateTime<Utc>> {
        let date = day.with_timezone(&self.offset_at(day)).date_naive();
        let naive = date.and_time(time);
        let utc = match self {
            Zone::Local => Local.from_local_datetime(&naive).earliest()?.with_timezone(&Utc),
            Zone::Tz(tz) => tz.from_local_datetime(&naive).earliest()?.with_timezone(&Utc),
            Zone::Fixed(o) => o.from_local_datetime(&naive).single()?.with_timezone(&Utc),
        };
        Some(utc)
    }
}

/// Abbreviations, standard and summer time alike, and a few places with names that aren't a
/// zone's.
const ALIASES: &[(&str, &str)] = &[
    ("utc", "UTC"),
    ("gmt", "UTC"),
    ("z", "UTC"),
    ("ist", "Asia/Kolkata"),
    ("india", "Asia/Kolkata"),
    ("delhi", "Asia/Kolkata"),
    ("new delhi", "Asia/Kolkata"),
    ("mumbai", "Asia/Kolkata"),
    ("bangalore", "Asia/Kolkata"),
    ("bengaluru", "Asia/Kolkata"),
    ("chennai", "Asia/Kolkata"),
    ("hyderabad", "Asia/Kolkata"),
    ("pst", "America/Los_Angeles"),
    ("pdt", "America/Los_Angeles"),
    ("pt", "America/Los_Angeles"),
    ("sf", "America/Los_Angeles"),
    ("san francisco", "America/Los_Angeles"),
    ("seattle", "America/Los_Angeles"),
    ("california", "America/Los_Angeles"),
    ("mst", "America/Denver"),
    ("mdt", "America/Denver"),
    ("cst", "America/Chicago"),
    ("cdt", "America/Chicago"),
    ("ct", "America/Chicago"),
    ("est", "America/New_York"),
    ("edt", "America/New_York"),
    ("et", "America/New_York"),
    ("nyc", "America/New_York"),
    ("boston", "America/New_York"),
    ("washington", "America/New_York"),
    ("bst", "Europe/London"),
    ("uk", "Europe/London"),
    ("cet", "Europe/Berlin"),
    ("cest", "Europe/Berlin"),
    ("eet", "Europe/Athens"),
    ("eest", "Europe/Athens"),
    ("msk", "Europe/Moscow"),
    ("gst", "Asia/Dubai"),
    ("uae", "Asia/Dubai"),
    ("pkt", "Asia/Karachi"),
    ("sgt", "Asia/Singapore"),
    ("hkt", "Asia/Hong_Kong"),
    ("china", "Asia/Shanghai"),
    ("beijing", "Asia/Shanghai"),
    ("jst", "Asia/Tokyo"),
    ("japan", "Asia/Tokyo"),
    ("kst", "Asia/Seoul"),
    ("korea", "Asia/Seoul"),
    ("aest", "Australia/Sydney"),
    ("aedt", "Australia/Sydney"),
    ("nzst", "Pacific/Auckland"),
    ("nzdt", "Pacific/Auckland"),
    ("brt", "America/Sao_Paulo"),
    ("germany", "Europe/Berlin"),
    ("france", "Europe/Paris"),
    ("spain", "Europe/Madrid"),
    ("italy", "Europe/Rome"),
    ("netherlands", "Europe/Amsterdam"),
];

fn zone(name: &str) -> Option<(Zone, String)> {
    let n = name.trim().to_lowercase();
    if n.is_empty() {
        return None;
    }
    if matches!(n.as_str(), "local" | "here" | "me" | "my time") {
        return Some((Zone::Local, "local time".into()));
    }
    // UTC+5:30, GMT-3.
    for p in ["utc", "gmt"] {
        if let Some(rest) = n.strip_prefix(p)
            && let Some(sign) = rest.chars().next().filter(|c| *c == '+' || *c == '-')
        {
            let (h, m) = rest[1..].split_once(':').unwrap_or((&rest[1..], "0"));
            let secs = (h.trim().parse::<i32>().ok()? * 60 + m.trim().parse::<i32>().ok()?) * 60;
            let o = FixedOffset::east_opt(if sign == '-' { -secs } else { secs })?;
            return Some((Zone::Fixed(o), format!("UTC{sign}{}", rest[1..].trim())));
        }
    }
    let label = |tz: Tz| tz.name().rsplit('/').next().unwrap_or(tz.name()).replace('_', " ");
    if let Some((_, id)) = ALIASES.iter().find(|(a, _)| *a == n) {
        let tz: Tz = id.parse().ok()?;
        let shown = if n.len() <= 4 { n.to_uppercase() } else { title_case(&n) };
        return Some((Zone::Tz(tz), if tz == Tz::UTC { "UTC".into() } else { shown }));
    }
    if let Ok(tz) = name.trim().parse::<Tz>() {
        return Some((Zone::Tz(tz), label(tz)));
    }
    // A city: the last part of a zone's name, such as Los_Angeles.
    let want = n.replace(' ', "_");
    chrono_tz::TZ_VARIANTS
        .iter()
        .find(|tz| tz.name().rsplit('/').next().is_some_and(|c| c.eq_ignore_ascii_case(&want)))
        .map(|tz| (Zone::Tz(*tz), label(*tz)))
}

fn title_case(s: &str) -> String {
    s.split(' ')
        .map(|w| {
            let mut c = w.chars();
            c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// `3pm`, `3:30 pm`, `15:30`, `noon`, `midnight`.
fn time(s: &str) -> Option<NaiveTime> {
    let s = s.trim().to_lowercase().replace(' ', "");
    match s.as_str() {
        "noon" => return NaiveTime::from_hms_opt(12, 0, 0),
        "midnight" => return NaiveTime::from_hms_opt(0, 0, 0),
        _ => {}
    }
    let (digits, pm) = if let Some(d) = s.strip_suffix("pm") {
        (d, Some(true))
    } else if let Some(d) = s.strip_suffix("am") {
        (d, Some(false))
    } else {
        (s.as_str(), None)
    };
    let (h, m) = digits.split_once(':').unwrap_or((digits, "0"));
    let (mut h, m): (u32, u32) = (h.parse().ok()?, m.parse().ok()?);
    // A bare number is only a time with am or pm: `3 ist` could be anything.
    if pm.is_none() && !digits.contains(':') {
        return None;
    }
    match pm {
        Some(true) if (1..=12).contains(&h) => h = h % 12 + 12,
        Some(false) if (1..=12).contains(&h) => h %= 12,
        Some(_) => return None,
        None => {}
    }
    NaiveTime::from_hms_opt(h, m, 0)
}

fn hhmm(t: DateTime<FixedOffset>) -> String {
    t.format("%-I:%M %p").to_string()
}

/// "5 h 30 min ahead" of local time.
fn difference(offset: FixedOffset, now: DateTime<Utc>) -> String {
    let local = Zone::Local.offset_at(now).local_minus_utc();
    let d = offset.local_minus_utc() - local;
    if d == 0 {
        return "same as here".into();
    }
    let (h, m) = (d.abs() / 3600, d.abs() % 3600 / 60);
    let amount = if m == 0 { format!("{h} h") } else { format!("{h} h {m} min") };
    format!("{amount} {}", if d > 0 { "ahead" } else { "behind" })
}

pub fn answer(q: &str, now: DateTime<Utc>) -> Option<Answer> {
    let lower = q.trim().to_lowercase();
    let lower = lower.trim_end_matches('?');

    // The time somewhere now.
    let place = lower
        .strip_prefix("what time is it in ")
        .or_else(|| lower.strip_prefix("time in "))
        .or_else(|| lower.strip_prefix("now in "))
        .or_else(|| lower.strip_suffix(" time"))
        .or_else(|| lower.strip_prefix("time "));
    if let Some(place) = place
        && let Some((z, label)) = zone(place)
    {
        let t = now.with_timezone(&z.offset_at(now));
        return Some(Answer {
            value: hhmm(t),
            detail: format!("{} in {label} · {}", t.format("%a %-d %b"), difference(*t.offset(), now)),
        });
    }

    // A time from one zone in another: "3pm ist to pst", "15:30 in tokyo" (from here).
    let (left, to) = lower.split_once(" to ").or_else(|| lower.split_once(" in "))?;
    let (to, to_label) = zone(to)?;
    let words: Vec<&str> = left.split_whitespace().collect();
    // The time is the first word or two ("3:30 pm"); what's left is the zone it's in.
    let (t, from) = (1..=words.len().min(2)).rev().find_map(|n| {
        let t = time(&words[..n].join(" "))?;
        let rest = words[n..].join(" ");
        let from = if rest.is_empty() { Some((Zone::Local, "local time".to_string())) } else { zone(&rest) };
        Some((t, from?))
    })?;
    let (from, from_label) = from;
    let at = from.at(now, t)?;
    let there = at.with_timezone(&to.offset_at(at));
    let here_day = at.with_timezone(&from.offset_at(at)).date_naive();
    let day = match (there.date_naive() - here_day).num_days() {
        0 => String::new(),
        1 => " the next day".into(),
        -1 => " the day before".into(),
        n => format!(" {n:+} days"),
    };
    Some(Answer {
        value: hhmm(there),
        detail: format!("{} {from_label} → {to_label}{day} · {}", t.format("%-I:%M %p"), there.format("%a %-d %b")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    #[test]
    fn now_somewhere() {
        let now = at("2026-01-15T12:00:00Z");
        assert_eq!(answer("time in tokyo", now).unwrap().value, "9:00 PM");
        assert_eq!(answer("london time", now).unwrap().value, "12:00 PM");
        assert_eq!(answer("time in ist", now).unwrap().value, "5:30 PM");
        assert_eq!(answer("time in utc+5:30", now).unwrap().value, "5:30 PM");
        assert_eq!(answer("Time in New York?", now).unwrap().value, "7:00 AM");
        assert!(answer("time in narnia", now).is_none());
        assert!(answer("bedtime", now).is_none());
    }

    #[test]
    fn converts_times() {
        let now = at("2026-01-15T12:00:00Z");
        let a = answer("3pm ist to pst", now).unwrap();
        assert_eq!(a.value, "1:30 AM");
        assert!(a.detail.contains("IST → PST"), "{}", a.detail);
        assert_eq!(answer("15:30 utc in tokyo", now).unwrap().value, "12:30 AM");
        assert!(answer("15:30 utc in tokyo", now).unwrap().detail.contains("the next day"));
        assert_eq!(answer("9 am new york to london", now).unwrap().value, "2:00 PM");
        // Summer time: New York is UTC-4 in July.
        assert_eq!(answer("9am est to utc", at("2026-07-15T12:00:00Z")).unwrap().value, "1:00 PM");
        // Not times.
        assert!(answer("3 ist to pst", now).is_none());
        assert!(answer("5 km to mi", now).is_none());
        assert!(answer("rust in action", now).is_none());
    }
}
