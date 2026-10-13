//! Parsing with defaults and warnings for keys we don't know.

use std::{fmt, path::Path};

use serde::de::DeserializeOwned;

/// A problem that doesn't stop the config from loading.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Warning {
    pub file: String,
    pub line: Option<usize>,
    pub message: String,
}

impl fmt::Display for Warning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.line {
            Some(line) => write!(f, "{}:{line}: {}", self.file, self.message),
            None => write!(f, "{}: {}", self.file, self.message),
        }
    }
}

/// A parsed value plus the warnings found on the way.
#[derive(Clone, Debug)]
pub struct Loaded<T> {
    pub value: T,
    pub warnings: Vec<Warning>,
}

/// Reads a TOML file into a table. A missing file is an empty table.
pub fn read_table(path: &Path) -> anyhow::Result<(toml::Table, String)> {
    match std::fs::read_to_string(path) {
        Ok(text) => {
            let table = text
                .parse::<toml::Table>()
                .map_err(|e| anyhow::anyhow!("{}: {}", path.display(), e.to_string().trim()))?;
            Ok((table, text))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok((toml::Table::new(), String::new())),
        Err(e) => Err(anyhow::anyhow!("{}: {e}", path.display())),
    }
}

/// Deserializes `table`, collecting unknown keys as warnings. `source` is the original text,
/// used to find line numbers; `file` names it in messages.
pub fn deserialize<T: DeserializeOwned>(table: toml::Table, source: &str, file: &str) -> anyhow::Result<Loaded<T>> {
    let mut unknown = Vec::new();
    let value: T = serde_ignored::deserialize(toml::Value::Table(table), |path| {
        unknown.push(path.to_string());
    })
    .map_err(|e| anyhow::anyhow!("{file}: {}", e.to_string().trim()))?;
    let warnings = unknown
        .into_iter()
        .map(|key| Warning {
            file: file.to_string(),
            line: line_of(source, &key),
            message: format!("unknown key `{key}`"),
        })
        .collect();
    Ok(Loaded { value, warnings })
}

/// Finds the line that defines a dotted key, as reported by `serde_ignored`.
fn line_of(source: &str, dotted: &str) -> Option<usize> {
    let doc = source.parse::<toml_edit::ImDocument<String>>().ok()?;
    let mut item = doc.as_item();
    let mut span = None;
    for part in dotted.split('.') {
        let table = item.as_table_like()?;
        let (key, value) = table.get_key_value(part)?;
        span = key.span().or_else(|| value.span()).or(span);
        item = value;
    }
    let offset = span?.start;
    Some(source[..offset].matches('\n').count() + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(serde::Deserialize, Default)]
    #[serde(default)]
    struct Sample {
        a: u32,
        inner: Inner,
    }
    #[derive(serde::Deserialize, Default)]
    #[serde(default)]
    struct Inner {
        b: bool,
    }

    #[test]
    fn unknown_keys_become_warnings_with_lines() {
        let text = "a = 1\n\n[inner]\nb = true\ntypo = 3\n";
        let loaded: Loaded<Sample> = deserialize(text.parse().unwrap(), text, "bar.toml").unwrap();
        assert_eq!(loaded.value.a, 1);
        assert!(loaded.value.inner.b);
        assert_eq!(loaded.warnings.len(), 1);
        assert_eq!(loaded.warnings[0].to_string(), "bar.toml:5: unknown key `inner.typo`");
    }

    #[test]
    fn type_errors_fail() {
        let text = "a = \"x\"";
        assert!(deserialize::<Sample>(text.parse().unwrap(), text, "bar.toml").is_err());
    }
}
