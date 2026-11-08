//! Bookmarks and history from Chromium-based browsers and Firefox-based ones. Read from their
//! files without taking their locks; nothing leaves the machine.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    time::Instant,
};

use crate::projects::query;

/// History entries read per profile, most recent first.
const HISTORY: usize = 2000;
const MAX: usize = 8000;

#[derive(Clone, Debug, PartialEq)]
pub struct Page {
    pub url: String,
    pub title: String,
    pub visits: u32,
    pub bookmark: bool,
    pub browser: &'static str,
    /// The host and path as words, for matching.
    pub words: String,
}

/// Chromium-based browsers, by their folder under `~/.config`.
const CHROMIUM: &[(&str, &str)] = &[
    ("google-chrome", "Chrome"),
    ("google-chrome-beta", "Chrome Beta"),
    ("chromium", "Chromium"),
    ("BraveSoftware/Brave-Browser", "Brave"),
    ("vivaldi", "Vivaldi"),
    ("microsoft-edge", "Edge"),
    ("thorium", "Thorium"),
];

/// Firefox-based browsers, by the folder holding their profiles.
const FIREFOX: &[(&str, &str)] =
    &[(".mozilla/firefox", "Firefox"), (".zen", "Zen"), (".librewolf", "LibreWolf"), (".floorp", "Floorp")];

pub fn pages() -> Vec<Page> {
    let started = Instant::now();
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else { return Vec::new() };
    let mut all: Vec<Page> = Vec::new();
    for (dir, name) in CHROMIUM {
        for profile in profiles(&home.join(".config").join(dir), |p| p.join("Preferences").is_file()) {
            chromium(&profile, name, &mut all);
        }
    }
    for (dir, name) in FIREFOX {
        for profile in profiles(&home.join(dir), |p| p.join("places.sqlite").is_file()) {
            firefox(&profile, name, &mut all);
        }
    }
    let out = merge(all);
    tracing::debug!("read {} bookmarks and pages in {:?}", out.len(), started.elapsed());
    out
}

fn profiles(dir: &Path, is_profile: impl Fn(&Path) -> bool) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut out: Vec<PathBuf> = entries.flatten().map(|e| e.path()).filter(|p| is_profile(p)).collect();
    out.sort();
    out
}

fn chromium(profile: &Path, browser: &'static str, out: &mut Vec<Page>) {
    if let Ok(text) = std::fs::read_to_string(profile.join("Bookmarks")) {
        let v: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
        if let Some(roots) = v["roots"].as_object() {
            for root in roots.values() {
                chromium_bookmarks(root, browser, out);
            }
        }
    }
    let sql = format!(
        "SELECT url, title, visit_count FROM urls WHERE hidden = 0 ORDER BY last_visit_time DESC LIMIT {HISTORY}"
    );
    let rows = query(&profile.join("History"), &sql, |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?))
    });
    for (url, title, visits) in rows {
        push(out, url, title, visits, false, browser);
    }
}

fn chromium_bookmarks(node: &serde_json::Value, browser: &'static str, out: &mut Vec<Page>) {
    if node["type"] == "url" {
        let url = node["url"].as_str().unwrap_or_default().to_string();
        let title = node["name"].as_str().unwrap_or_default().to_string();
        push(out, url, title, 0, true, browser);
    }
    for child in node["children"].as_array().into_iter().flatten() {
        chromium_bookmarks(child, browser, out);
    }
}

fn firefox(profile: &Path, browser: &'static str, out: &mut Vec<Page>) {
    let db = profile.join("places.sqlite");
    let marks = query(
        &db,
        "SELECT IFNULL(b.title, ''), p.url FROM moz_bookmarks b JOIN moz_places p ON p.id = b.fk WHERE b.type = 1",
        |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
    );
    for (title, url) in marks {
        push(out, url, title, 0, true, browser);
    }
    let sql = format!(
        "SELECT url, IFNULL(title, ''), visit_count FROM moz_places WHERE hidden = 0 AND visit_count > 0 ORDER BY last_visit_date DESC LIMIT {HISTORY}"
    );
    let rows = query(&db, &sql, |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?)));
    for (url, title, visits) in rows {
        push(out, url, title, visits, false, browser);
    }
}

fn push(out: &mut Vec<Page>, url: String, title: String, visits: i64, bookmark: bool, browser: &'static str) {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return;
    }
    let bare = url.split_once("://").map(|(_, r)| r).unwrap_or(&url);
    let bare = bare.strip_prefix("www.").unwrap_or(bare);
    let title = title.trim();
    let title = if title.is_empty() { bare.trim_end_matches('/').to_string() } else { title.to_string() };
    let words: String = bare
        .split(['?', '#'])
        .next()
        .unwrap_or_default()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect();
    out.push(Page { url, title, visits: visits.clamp(0, u32::MAX as i64) as u32, bookmark, browser, words });
}

/// One entry per URL: visits added up, a bookmark if any browser has it as one.
fn merge(all: Vec<Page>) -> Vec<Page> {
    let mut by_url: HashMap<String, usize> = HashMap::new();
    let mut out: Vec<Page> = Vec::new();
    for p in all {
        match by_url.get(&p.url) {
            Some(&i) => {
                let e = &mut out[i];
                e.visits = e.visits.saturating_add(p.visits);
                if p.bookmark && !e.bookmark {
                    e.bookmark = true;
                    e.title = p.title;
                }
            }
            None => {
                by_url.insert(p.url.clone(), out.len());
                out.push(p);
            }
        }
    }
    out.truncate(MAX);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merges_and_cleans() {
        let mut all = Vec::new();
        push(&mut all, "https://www.example.com/docs?q=1".into(), "".into(), 3, false, "Chrome");
        push(&mut all, "https://www.example.com/docs?q=1".into(), "Example docs".into(), 0, true, "Firefox");
        push(&mut all, "chrome://settings".into(), "Settings".into(), 9, false, "Chrome");
        let out = merge(all);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].title, "Example docs");
        assert!(out[0].bookmark);
        assert_eq!(out[0].visits, 3);
        assert_eq!(out[0].words.split_whitespace().collect::<Vec<_>>(), ["example", "com", "docs"]);

        let mut tree = Vec::new();
        let v = serde_json::json!({"children": [{"type": "url", "name": "A", "url": "https://a.org/"},
            {"type": "folder", "children": [{"type": "url", "name": "B", "url": "https://b.org/"}]}]});
        chromium_bookmarks(&v, "Chrome", &mut tree);
        assert_eq!(tree.iter().map(|p| p.title.as_str()).collect::<Vec<_>>(), ["A", "B"]);
    }
}
