//! Edits config files in place, keeping comments and formatting, and writes them atomically.

use std::{
    io::Write,
    path::{Path, PathBuf},
};

use toml_edit::{DocumentMut, Item, Table, Value};

/// An open config file being edited.
pub struct ConfigFile {
    path: PathBuf,
    doc: DocumentMut,
}

impl ConfigFile {
    /// Opens `path`, or starts an empty document if it doesn't exist.
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(e) => return Err(e.into()),
        };
        let doc =
            text.parse::<DocumentMut>().map_err(|e| anyhow::anyhow!("{}: {}", path.display(), e.to_string().trim()))?;
        Ok(Self { path: path.to_path_buf(), doc })
    }

    pub fn document(&self) -> &DocumentMut {
        &self.doc
    }

    /// Reads the value at a dotted path such as `bar.height`.
    pub fn get(&self, dotted: &str) -> Option<&Item> {
        let mut item = self.doc.as_item();
        for part in split(dotted) {
            item = item.as_table_like()?.get(&part)?;
        }
        Some(item)
    }

    /// Sets the value at a dotted path, creating tables on the way.
    pub fn set(&mut self, dotted: &str, value: impl Into<Value>) {
        let parts = split(dotted);
        let (last, tables) = parts.split_last().expect("empty key");
        let mut table: &mut dyn toml_edit::TableLike = self.doc.as_table_mut();
        for part in tables {
            if !table.contains_key(part) {
                let mut new = Table::new();
                new.set_implicit(true);
                table.insert(part, Item::Table(new));
            }
            table =
                table.get_mut(part).and_then(Item::as_table_like_mut).expect("a scalar sits where a table is needed");
        }
        match table.get_mut(last).and_then(Item::as_value_mut) {
            // Keep the existing decoration (comments, alignment) on the value.
            Some(existing) => {
                let decor = existing.decor().clone();
                *existing = value.into();
                *existing.decor_mut() = decor;
            }
            None => {
                table.insert(last, Item::Value(value.into()));
            }
        }
    }

    /// Removes the value at a dotted path, falling back to the default.
    pub fn remove(&mut self, dotted: &str) {
        let parts = split(dotted);
        let (last, tables) = parts.split_last().expect("empty key");
        let mut table: &mut dyn toml_edit::TableLike = self.doc.as_table_mut();
        for part in tables {
            match table.get_mut(part).and_then(Item::as_table_like_mut) {
                Some(t) => table = t,
                None => return,
            }
        }
        table.remove(last);
    }

    /// Writes through a temp file and a rename, so watchers never see a half-written file.
    pub fn save(&self) -> anyhow::Result<()> {
        write_atomic(&self.path, self.doc.to_string().as_bytes())
    }
}

/// Splits `monitor."HDMI-A-1".enabled` into its parts, honouring quotes.
fn split(dotted: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    for c in dotted.chars() {
        match c {
            '"' => quoted = !quoted,
            '.' if !quoted => parts.push(std::mem::take(&mut current)),
            c => current.push(c),
        }
    }
    parts.push(current);
    parts
}

pub fn write_atomic(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension(format!("tmp.{}", std::process::id()));
    {
        let mut file = std::fs::File::create(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    std::fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edits_keep_comments() {
        let path = std::env::temp_dir().join(format!("yozora-edit-{}.toml", std::process::id()));
        std::fs::write(&path, "# my bar\n[bar]\nheight = 34 # tall enough\n").unwrap();
        let mut file = ConfigFile::open(&path).unwrap();
        file.set("bar.height", 38);
        file.set("monitor.\"HDMI-A-1\".enabled", false);
        file.save().unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("# my bar"));
        assert!(text.contains("height = 38 # tall enough"), "{text}");
        assert!(text.contains("[monitor.HDMI-A-1]"), "{text}");
        let file = ConfigFile::open(&path).unwrap();
        assert_eq!(file.get("monitor.\"HDMI-A-1\".enabled").and_then(|i| i.as_bool()), Some(false));
        std::fs::remove_file(&path).unwrap();
    }
}
