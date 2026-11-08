//! A config file open for editing: the comment-preserving document, its text, and writes that
//! keep every comment and the file's layout.

use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use yozora_config::edit::ConfigFile;

pub struct Doc {
    pub path: PathBuf,
    file: Option<ConfigFile>,
    /// Why the file can't be edited, when it doesn't parse.
    pub error: Option<String>,
    text: String,
}

impl Doc {
    pub fn open(path: &Path) -> Self {
        let mut doc = Self { path: path.to_path_buf(), file: None, error: None, text: String::new() };
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
        true
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

    /// The names under a table, such as the `custom` modules.
    pub fn table_keys(&self, key: &str) -> Vec<String> {
        self.file
            .as_ref()
            .and_then(|f| f.get(key))
            .and_then(|i| i.as_table_like())
            .map(|t| t.iter().map(|(k, _)| k.to_owned()).collect())
            .unwrap_or_default()
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
        self.text = new;
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

/// `toml::Value` to `toml_edit::Value`, for writing values a schema describes.
pub fn to_edit_value(value: &toml::Value) -> Option<toml_edit::Value> {
    let doc: toml_edit::DocumentMut = format!("v = {value}").parse().ok()?;
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
