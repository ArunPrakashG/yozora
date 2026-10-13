//! A config file open for editing: the comment-preserving document, its text for the preview,
//! and which lines the last save changed.

use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use yozora_config::edit::ConfigFile;

pub struct Doc {
    pub path: PathBuf,
    file: Option<ConfigFile>,
    /// Why the file can't be edited, when it doesn't parse.
    pub error: Option<String>,
    pub text: String,
    /// Lines (0-based) the last save changed, for the preview's highlight.
    pub changed: Vec<usize>,
    /// Saves made from this window, so each save restarts the highlight.
    pub saves: u32,
}

impl Doc {
    pub fn open(path: &Path) -> Self {
        let mut doc = Self {
            path: path.to_path_buf(),
            file: None,
            error: None,
            text: String::new(),
            changed: Vec::new(),
            saves: 0,
        };
        doc.reload();
        doc
    }

    pub fn file_name(&self) -> String {
        self.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
    }

    /// Re-reads the file. Returns whether its text changed.
    pub fn reload(&mut self) -> bool {
        let text = match std::fs::read_to_string(&self.path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(e) => {
                self.error = Some(format!("{e}"));
                return true;
            }
        };
        if text == self.text && (self.file.is_some() || self.error.is_some()) {
            return false;
        }
        self.text = text;
        match ConfigFile::open(&self.path) {
            Ok(file) => {
                self.file = Some(file);
                self.error = None;
            }
            Err(e) => {
                self.file = None;
                self.error = Some(format!("{e:#}"));
            }
        }
        self.changed.clear();
        true
    }

    pub fn is_editable(&self) -> bool {
        self.file.is_some()
    }

    /// The file deserialized into `T`, or `T`'s defaults when it's missing or broken.
    pub fn parse<T: DeserializeOwned + Default>(&self) -> T {
        toml::from_str(&self.text).unwrap_or_default()
    }

    /// The value at a dotted key, if the file sets it.
    pub fn value(&self, key: &str) -> Option<toml::Value> {
        let item = self.file.as_ref()?.get(key)?;
        let value = item.as_value()?;
        let table: toml::Table = toml::from_str(&format!("v = {value}")).ok()?;
        table.get("v").cloned()
    }

    pub fn is_set(&self, key: &str) -> bool {
        self.file.as_ref().is_some_and(|f| f.get(key).is_some())
    }

    /// Applies `edit` and saves. The app watching the file reloads it.
    pub fn edit(&mut self, edit: impl FnOnce(&mut ConfigFile)) -> anyhow::Result<()> {
        let Some(file) = self.file.as_mut() else {
            anyhow::bail!("{} has an error; fix it before editing here", self.file_name());
        };
        edit(file);
        let new = file.document().to_string();
        if new == self.text {
            return Ok(());
        }
        file.save()?;
        self.changed = changed_lines(&self.text, &new);
        self.text = new;
        self.saves += 1;
        Ok(())
    }

    pub fn set(&mut self, key: &str, value: impl Into<toml_edit::Value>) -> anyhow::Result<()> {
        let value = value.into();
        self.edit(|f| f.set(key, value))
    }

    pub fn remove(&mut self, keys: &[&str]) -> anyhow::Result<()> {
        self.edit(|f| {
            for key in keys {
                f.remove(key);
            }
        })
    }
}

/// Lines of `new` that differ from `old`, by trimming the common start and end.
pub fn changed_lines(old: &str, new: &str) -> Vec<usize> {
    let old: Vec<&str> = old.lines().collect();
    let new: Vec<&str> = new.lines().collect();
    let prefix = old.iter().zip(&new).take_while(|(a, b)| a == b).count();
    let max_suffix = old.len().min(new.len()) - prefix;
    let suffix = old
        .iter()
        .rev()
        .zip(new.iter().rev())
        .take(max_suffix)
        .take_while(|(a, b)| a == b)
        .count();
    (prefix..new.len() - suffix).collect()
}

/// `toml::Value` to `toml_edit::Value`, for writing values a schema describes.
pub fn to_edit_value(value: &toml::Value) -> Option<toml_edit::Value> {
    let text = format!("v = {value}");
    let doc: toml_edit::DocumentMut = text.parse().ok()?;
    doc.get("v")?.as_value().cloned().map(|mut v| {
        v.decor_mut().clear();
        v
    })
}

pub fn string_array<S: AsRef<str>>(items: &[S]) -> toml_edit::Value {
    let mut array = toml_edit::Array::new();
    for item in items {
        array.push(item.as_ref());
    }
    toml_edit::Value::Array(array)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn changed_lines_finds_the_edit() {
        assert_eq!(changed_lines("a\nb\nc\n", "a\nB\nc\n"), vec![1]);
        assert_eq!(changed_lines("a\nc\n", "a\nb\nc\n"), vec![1]);
        assert_eq!(changed_lines("", "[bar]\nheight = 30\n"), vec![0, 1]);
        assert!(changed_lines("a\nb\n", "a\n").is_empty());
    }

    #[test]
    fn edits_round_trip() {
        let path = std::env::temp_dir().join(format!("yozora-doc-{}.toml", std::process::id()));
        std::fs::write(&path, "# keep me\n[bar]\nheight = 34\n").unwrap();
        let mut doc = Doc::open(&path);
        doc.set("bar.height", 38).unwrap();
        doc.set("modules.left", string_array(&["workspaces", "window"])).unwrap();
        assert_eq!(doc.value("bar.height"), Some(toml::Value::Integer(38)));
        assert!(doc.text.contains("# keep me"));
        assert_eq!(doc.saves, 2);
        let bar: yozora_config::bar::BarConfig = doc.parse();
        assert_eq!(bar.modules.left, vec!["workspaces".to_string(), "window".to_string()]);
        doc.remove(&["bar.height"]).unwrap();
        assert!(!doc.is_set("bar.height"));
        assert_eq!(
            to_edit_value(&toml::Value::String("x".into())).map(|v| v.to_string()),
            Some("\"x\"".into())
        );
        std::fs::remove_file(&path).unwrap();
    }
}
