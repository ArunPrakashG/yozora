//! What the Settings modal remembers between frames: the config files it has open, the one text
//! field being edited, and the last message. The files sit behind a lock so a page can write
//! while it is still being drawn (a field that lost focus saves as the page redraws).

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use rune::TextEdit;
use yozora_config::bar::BarConfig;

use crate::settings_doc::{Doc, string_array};

/// What a text field writes when it is committed.
#[derive(Clone, Debug, PartialEq)]
pub enum Target {
    /// Sets a string. An empty field removes the key, so the default applies again. A secret
    /// also makes the file readable by its owner only.
    Str { file: String, key: String, secret: bool },
    /// Sets a number, if the text parses and is in range. An empty field removes the key.
    Num { file: String, key: String, float: bool, min: Option<f64>, max: Option<f64> },
    /// Appends to a list of strings.
    Append { file: String, key: String },
    /// Starts a `[custom.<name>]` module.
    NewCustom { file: String },
    /// A time of day, `HH:MM`.
    Clock { file: String, key: String },
}

#[derive(Clone)]
struct Active {
    id: String,
    target: Target,
    /// The field has held focus at least once, so losing it now means the person moved on.
    seen: bool,
}

#[derive(Default)]
struct Inner {
    docs: HashMap<String, Doc>,
    reloaded: Option<Instant>,
    active: Option<Active>,
    error: Option<String>,
    /// A "restore defaults" press waiting for its second press, and when it was made.
    confirm: Option<(String, Instant)>,
    monitor: Option<String>,
}

#[derive(Default)]
pub struct SettingsUi {
    /// The text field being edited. There is only ever one.
    pub edit: TextEdit,
    inner: Mutex<Inner>,
}

fn config_path(file: &str) -> std::path::PathBuf {
    yozora_config::paths::config_dir().join(file)
}

impl Inner {
    fn doc(&mut self, file: &str) -> &mut Doc {
        self.docs.entry(file.to_owned()).or_insert_with(|| Doc::open(&config_path(file)))
    }

    fn edit(&mut self, file: &str, f: impl FnOnce(&mut Doc) -> anyhow::Result<()>) {
        self.error = f(self.doc(file)).err().map(|e| format!("{e:#}"));
    }
}

impl SettingsUi {
    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    // ---- Files -----------------------------------------------------------------------------

    pub fn value(&self, file: &str, key: &str) -> Option<toml::Value> {
        self.lock().doc(file).value(key)
    }

    pub fn is_set(&self, file: &str, key: &str) -> bool {
        self.lock().doc(file).is_set(key)
    }

    pub fn table_keys(&self, file: &str, key: &str) -> Vec<String> {
        self.lock().doc(file).table_keys(key)
    }

    pub fn bar_config(&self, file: &str) -> BarConfig {
        self.lock().doc(file).parse()
    }

    /// Why `file` cannot be edited, when it does not parse.
    pub fn file_error(&self, file: &str) -> Option<String> {
        self.lock().doc(file).error.clone()
    }

    pub fn set(&self, file: &str, key: &str, value: impl Into<toml_edit::Value>) {
        let value = value.into();
        self.lock().edit(file, |d| d.set(key, value));
    }

    pub fn remove(&self, file: &str, keys: &[&str]) {
        self.lock().edit(file, |d| d.remove(keys));
    }

    /// Several changes to one file, written once.
    pub fn edit(&self, file: &str, f: impl FnOnce(&mut yozora_config::edit::ConfigFile)) {
        self.lock().edit(file, |d| d.edit(f));
    }

    pub fn set_list(&self, file: &str, key: &str, items: &[String]) {
        self.set(file, key, string_array(items));
    }

    /// Reads the files again, since the shell or a text editor may have changed them. Quick, and
    /// only done a few times a second.
    pub fn tick(&self) {
        let mut g = self.lock();
        if g.reloaded.is_some_and(|t| t.elapsed() < Duration::from_millis(400)) {
            return;
        }
        g.reloaded = Some(Instant::now());
        for doc in g.docs.values_mut() {
            doc.reload();
        }
    }

    /// Forgets everything held from an earlier visit, so a reopened Settings starts clean.
    pub fn reset(&mut self) {
        let mut g = self.lock();
        g.docs.clear();
        g.active = None;
        g.error = None;
        g.confirm = None;
        g.reloaded = None;
    }

    pub fn error(&self) -> Option<String> {
        self.lock().error.clone()
    }

    /// Shows `message` above the page, as a failed save does.
    pub fn fail(&self, message: String) {
        self.lock().error = Some(message);
    }

    pub fn clear_error(&self) {
        self.lock().error = None;
    }

    // ---- Restore defaults ------------------------------------------------------------------

    /// Asks once, then acts on a second press within four seconds.
    pub fn restore(&self, id: &str, file: &str, keys: &[String]) {
        let mut g = self.lock();
        let armed = g.confirm.take().is_some_and(|(c, at)| c == id && at.elapsed() < Duration::from_secs(4));
        if !armed {
            g.confirm = Some((id.to_owned(), Instant::now()));
            return;
        }
        let keys: Vec<&str> = keys.iter().map(String::as_str).collect();
        g.edit(file, |d| d.remove(&keys));
    }

    pub fn confirming(&self, id: &str) -> bool {
        self.lock().confirm.as_ref().is_some_and(|(c, at)| c == id && at.elapsed() < Duration::from_secs(4))
    }

    // ---- Monitors --------------------------------------------------------------------------

    pub fn monitor(&self) -> Option<String> {
        self.lock().monitor.clone()
    }

    pub fn select_monitor(&self, name: &str) {
        self.lock().monitor = Some(name.to_owned());
    }

    // ---- The text field --------------------------------------------------------------------

    pub fn is_active(&self, id: &str) -> bool {
        self.lock().active.as_ref().is_some_and(|a| a.id == id)
    }

    /// Opens the field `id` for typing, saving whichever one was open before it.
    pub fn activate(&mut self, id: &str, target: Target, text: &str, secret: bool) {
        self.commit();
        let mut edit = if secret { TextEdit::new(text).secure() } else { TextEdit::new(text) };
        edit.focused = true;
        // Everything is selected, so typing replaces it.
        edit.anchor = 0;
        self.edit = edit;
        self.lock().active = Some(Active { id: id.to_owned(), target, seen: false });
    }

    /// Drops what was typed.
    pub fn cancel(&mut self) {
        self.lock().active = None;
        self.edit.focused = false;
    }

    /// Called as each frame is drawn: notes that the open field has focus, and saves it once it
    /// loses focus (a click elsewhere).
    pub fn watch_focus(&self) {
        let seen_then_lost = {
            let mut g = self.lock();
            match g.active.as_mut() {
                Some(a) if self.edit.focused => {
                    a.seen = true;
                    false
                }
                Some(a) => a.seen,
                None => false,
            }
        };
        if seen_then_lost {
            self.commit();
        }
    }

    /// Enter in the open field. Saves it, and for a list reopens it empty so the next item can
    /// follow. Returns whether the field stays open.
    pub fn submit(&mut self) -> bool {
        let again = self.lock().active.as_ref().filter(|a| matches!(a.target, Target::Append { .. })).map(|a| (a.id.clone(), a.target.clone()));
        self.commit();
        match again {
            Some((id, target)) if self.error().is_none() => {
                self.activate(&id, target, "", false);
                true
            }
            _ => false,
        }
    }

    /// Saves the open field, if there is one, and closes it.
    pub fn commit(&self) {
        let (active, text) = {
            let mut g = self.lock();
            (g.active.take(), self.edit.text.clone())
        };
        let Some(active) = active else { return };
        let mut g = self.lock();
        g.error = None;
        match active.target {
            Target::Str { file, key, secret } => {
                let before = g.doc(&file).value(&key).and_then(|v| v.as_str().map(String::from));
                if before.as_deref() == Some(text.as_str()) || (before.is_none() && text.is_empty()) {
                    return;
                }
                let path = config_path(&file);
                // Before the save too, so the key is never in a file others can read; saves keep
                // the mode.
                if secret && path.exists() {
                    let _ = yozora_config::edit::restrict(&path);
                }
                g.edit(&file, |d| if text.is_empty() { d.remove(&[key.as_str()]) } else { d.set(&key, text) });
                if secret && let Err(e) = yozora_config::edit::restrict(&path) {
                    g.error = Some(format!("couldn't make {file} private: {e:#}"));
                }
            }
            Target::Num { file, key, float, min, max } => {
                let text = text.trim();
                if text.is_empty() {
                    if g.doc(&file).is_set(&key) {
                        g.edit(&file, |d| d.remove(&[key.as_str()]));
                    }
                    return;
                }
                let Ok(n) = text.parse::<f64>() else {
                    g.error = Some(format!("`{text}` isn't a number"));
                    return;
                };
                if min.is_some_and(|m| n < m) || max.is_some_and(|m| n > m) {
                    g.error = Some(format!(
                        "{key} must be between {} and {}",
                        min.map_or("-\u{221e}".into(), |m| m.to_string()),
                        max.map_or("\u{221e}".into(), |m| m.to_string())
                    ));
                    return;
                }
                if float {
                    g.edit(&file, |d| d.set(&key, (n * 1000.0).round() / 1000.0));
                } else {
                    g.edit(&file, |d| d.set(&key, n.round() as i64));
                }
            }
            Target::Append { file, key } => {
                let text = text.trim().to_owned();
                if text.is_empty() {
                    return;
                }
                let mut list: Vec<String> = match g.doc(&file).value(&key) {
                    Some(toml::Value::Array(a)) => a.iter().filter_map(|v| v.as_str().map(String::from)).collect(),
                    _ => Vec::new(),
                };
                if !list.contains(&text) {
                    list.push(text);
                }
                g.edit(&file, |d| d.set(&key, string_array(&list)));
            }
            Target::NewCustom { file } => {
                let name = text.trim().to_lowercase();
                if name.is_empty() {
                    return;
                }
                if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
                    g.error = Some("a module name is letters, digits, dashes and underscores".into());
                    return;
                }
                let key = format!("custom.{name}.exec");
                if g.doc(&file).is_set(&key) {
                    g.error = Some(format!("custom/{name} already exists"));
                    return;
                }
                g.edit(&file, |d| d.set(&key, String::new()));
            }
            Target::Clock { file, key } => {
                let text = text.trim();
                let ok = text.split_once(':').is_some_and(|(h, m)| {
                    h.len() == 2 && m.len() == 2 && h.parse::<u32>().is_ok_and(|h| h < 24) && m.parse::<u32>().is_ok_and(|m| m < 60)
                });
                if !ok {
                    g.error = Some(format!("`{text}` isn't a time; write it as HH:MM"));
                    return;
                }
                if g.doc(&file).value(&key).and_then(|v| v.as_str().map(String::from)).as_deref() != Some(text) {
                    g.edit(&file, |d| d.set(&key, text.to_owned()));
                }
            }
        }
    }
}
