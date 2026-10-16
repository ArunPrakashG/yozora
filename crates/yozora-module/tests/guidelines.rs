use std::collections::HashMap;

use serde_json::json;
use yozora_module::{COMPONENTS, MAX_ROWS_PER_SECTION, MAX_TITLE_CHARS, MIN_INTERVAL_SECS, Manifest, Popover};

const DOC_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/modules.md");

fn doc() -> String {
    std::fs::read_to_string(DOC_PATH).expect("read docs/modules.md")
}

#[test]
fn guidelines_cover_components() {
    let doc = doc();
    for name in COMPONENTS {
        let heading = format!("### `{name}`");
        assert!(doc.contains(&heading), "docs/modules.md has no heading {heading}");
    }
}

/// Parse the `| Limit | Value |` table into name -> value.
fn documented_limits(doc: &str) -> HashMap<String, u64> {
    let mut in_table = false;
    let mut out = HashMap::new();
    for line in doc.lines() {
        let t = line.trim();
        if !in_table {
            in_table = t == "| Limit | Value |";
            continue;
        }
        if !t.starts_with('|') {
            break;
        }
        if t.starts_with("|---") || t.starts_with("| ---") {
            continue;
        }
        let cells: Vec<&str> = t.trim_matches('|').split('|').map(str::trim).collect();
        assert_eq!(cells.len(), 2, "limits row must have two cells: {t}");
        let value: u64 = cells[1].parse().unwrap_or_else(|_| panic!("bad limit value in {t}"));
        out.insert(cells[0].to_string(), value);
    }
    assert!(in_table, "docs/modules.md has no `| Limit | Value |` table");
    out
}

fn popover_with_rows(n: usize) -> Result<Popover, serde_json::Error> {
    let rows: Vec<_> = (0..n).map(|_| json!({ "type": "row", "title": "a" })).collect();
    serde_json::from_value(json!({
        "title": "T",
        "body": [{ "type": "section", "rows": rows }]
    }))
}

fn popover_with_title(title: &str) -> Result<Popover, serde_json::Error> {
    serde_json::from_value(json!({ "title": title, "body": [] }))
}

fn manifest_with_interval(n: u64) -> String {
    format!(
        r#"
schema = 1
name = "claude-sessions"
title = "Claude"
version = "0.1.0"
icon = "sparkles"
refresh = {{ interval = {n} }}
exec = "claude-sessions"
"#
    )
}

#[test]
fn limits_match_docs() {
    let limits = documented_limits(&doc());
    let get = |k: &str| *limits.get(k).unwrap_or_else(|| panic!("doc has no limit `{k}`"));

    assert_eq!(get("Rows per section"), MAX_ROWS_PER_SECTION as u64);
    assert_eq!(get("Title length (characters)"), MAX_TITLE_CHARS as u64);
    assert_eq!(get("Minimum refresh interval (seconds)"), MIN_INTERVAL_SECS);

    // Rows per section: one over the documented maximum fails, the maximum passes.
    let over = popover_with_rows(MAX_ROWS_PER_SECTION + 1).unwrap();
    assert!(over.validate().is_err(), "a section over the row limit validated");
    let at = popover_with_rows(MAX_ROWS_PER_SECTION).unwrap();
    assert!(at.validate().is_ok(), "a section at the row limit failed");

    // Title length: one over the documented maximum fails, the maximum passes.
    let long = popover_with_title(&"a".repeat(MAX_TITLE_CHARS + 1)).unwrap();
    assert!(long.validate().is_err(), "a title over the limit validated");
    let exact = popover_with_title(&"a".repeat(MAX_TITLE_CHARS)).unwrap();
    assert!(exact.validate().is_ok(), "a title at the limit failed");

    // Refresh floor: one below the documented minimum fails, the minimum passes.
    let below = MIN_INTERVAL_SECS - 1;
    assert!(Manifest::parse(&manifest_with_interval(below)).is_err(), "interval {below} was accepted");
    assert!(
        Manifest::parse(&manifest_with_interval(MIN_INTERVAL_SECS)).is_ok(),
        "interval {MIN_INTERVAL_SECS} was rejected"
    );
}
