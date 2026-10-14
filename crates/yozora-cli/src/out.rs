//! Output: compact JSON with `--json`, otherwise readable text.

use serde_json::Value;

pub struct Out {
    pub json: bool,
}

impl Out {
    /// Prints `value`: as JSON, or as indented `key: value` lines.
    pub fn value(&self, value: &Value) {
        if self.json {
            say!("{value}");
        } else {
            let mut s = String::new();
            text(value, 0, &mut s);
            say_raw!("{s}");
        }
    }

    /// Prints `data` as JSON, or `message` as text.
    pub fn done(&self, message: impl AsRef<str>, data: Value) {
        if self.json {
            say!("{data}");
        } else {
            say!("{}", message.as_ref());
        }
    }

    /// Prints `data` as JSON, or `lines` as text.
    pub fn lines(&self, lines: &[String], data: Value) {
        if self.json {
            say!("{data}");
        } else {
            for l in lines {
                say!("{l}");
            }
        }
    }

    pub fn error(&self, message: &str) {
        if self.json {
            say!("{}", serde_json::json!({ "error": message }));
        } else {
            eprintln!("yozora: {message}");
        }
    }
}

/// A scalar as a person would read it.
pub fn scalar(v: &Value) -> String {
    match v {
        Value::Null => "-".into(),
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

fn text(v: &Value, indent: usize, s: &mut String) {
    let pad = "  ".repeat(indent);
    match v {
        Value::Object(map) => {
            for (k, v) in map {
                match v {
                    Value::Object(m) if !m.is_empty() => {
                        s.push_str(&format!("{pad}{k}:\n"));
                        text(v, indent + 1, s);
                    }
                    Value::Array(a) if !a.is_empty() && !a.iter().all(is_scalar) => {
                        s.push_str(&format!("{pad}{k}:\n"));
                        text(v, indent + 1, s);
                    }
                    Value::Array(a) => {
                        let items: Vec<String> = a.iter().map(scalar).collect();
                        s.push_str(&format!(
                            "{pad}{k}: {}\n",
                            if items.is_empty() { "-".into() } else { items.join(", ") }
                        ));
                    }
                    _ => s.push_str(&format!("{pad}{k}: {}\n", scalar(v))),
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                if is_scalar(item) {
                    s.push_str(&format!("{pad}- {}\n", scalar(item)));
                } else {
                    // The first key on the dash line, the rest under it.
                    let mut inner = String::new();
                    text(item, indent + 1, &mut inner);
                    let inner = inner.trim_start_matches(' ');
                    s.push_str(&format!("{pad}- {inner}"));
                }
            }
        }
        other => s.push_str(&format!("{pad}{}\n", scalar(other))),
    }
}

fn is_scalar(v: &Value) -> bool {
    !matches!(v, Value::Object(_) | Value::Array(_))
}

/// "3 min ago" for a Unix time.
pub fn ago(unix: u64) -> String {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let s = now.saturating_sub(unix);
    match s {
        0..60 => "just now".into(),
        60..3600 => format!("{} min ago", s / 60),
        3600..86400 => format!("{} h ago", s / 3600),
        _ => format!("{} d ago", s / 86400),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_reads_like_yaml() {
        let v = serde_json::json!({
            "dnd": false,
            "apps": [{ "id": "bar", "running": true }, { "id": "lock", "running": false }],
            "tags": ["a", "b"],
            "audio": { "output": { "volume": 40 } },
        });
        let mut s = String::new();
        text(&v, 0, &mut s);
        assert_eq!(
            s,
            "dnd: false\napps:\n  - id: bar\n    running: true\n  - id: lock\n    running: false\ntags: a, b\naudio:\n  output:\n    volume: 40\n"
        );
    }
}
