//! The notifications themselves: what the server received, what the center keeps, and Do Not
//! Disturb. Plain data with no D-Bus or UI, so it can be tested on its own.
//!
//! History is saved to `$XDG_STATE_HOME/yozora/notify.json` after every change, so the center
//! survives restarts. Ids keep counting up across restarts, so an id never means two things.

use std::{
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Urgency {
    Low,
    #[default]
    Normal,
    Critical,
}

impl Urgency {
    pub fn from_byte(b: u8) -> Self {
        match b {
            0 => Self::Low,
            2 => Self::Critical,
            _ => Self::Normal,
        }
    }

    pub fn byte(self) -> u8 {
        match self {
            Self::Low => 0,
            Self::Normal => 1,
            Self::Critical => 2,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Notification {
    pub id: u32,
    pub app: String,
    /// The `app_icon` the app sent: a name, a path or a `file://` URI.
    pub app_icon: String,
    pub summary: String,
    /// Plain text, with markup removed.
    pub body: String,
    /// Action keys and labels, in the order the app sent them.
    pub actions: Vec<(String, String)>,
    pub urgency: Urgency,
    pub category: String,
    pub desktop_entry: String,
    /// An image the app attached, sized for drawing. Pixel data is written to a file first.
    pub image: Option<PathBuf>,
    /// The image is a wide picture, like a screenshot, shown under the body instead of as the
    /// icon.
    pub preview: bool,
    /// The icon to draw, looked up from the app's icon, desktop entry or name.
    pub icon: Option<PathBuf>,
    /// The `value` hint, 0–100, for progress.
    pub progress: Option<u8>,
    /// Stays after an action is invoked.
    pub resident: bool,
    /// Never kept in the center.
    pub transient: bool,
    /// Replaces an earlier notification with the same tag, for volume and brightness popups.
    pub tag: Option<String>,
    /// The app's timeout in milliseconds: -1 for the default, 0 for never.
    pub expire_ms: i32,
    /// Unix seconds when it arrived.
    pub time: u64,
    /// Jev's odds that it needs attention now, once it has answered.
    pub attention: Option<f64>,
    /// It can wait, by Jev's answer: its toast left early and it isn't counted as unread.
    pub quiet: bool,
}

impl Notification {
    /// Actions shown as buttons: everything but the default action and inline reply.
    pub fn buttons(&self) -> impl Iterator<Item = &(String, String)> {
        self.actions.iter().filter(|(k, _)| k != "default" && k != "inline-reply")
    }

    pub fn has_action(&self, key: &str) -> bool {
        self.actions.iter().any(|(k, _)| k == key)
    }

    /// The label for the inline reply field, if the app accepts replies.
    pub fn reply_label(&self) -> Option<&str> {
        self.actions.iter().find(|(k, _)| k == "inline-reply").map(|(_, l)| l.as_str())
    }
}

pub fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// What happened to the center when a notification arrived.
#[derive(Debug, PartialEq)]
pub enum Added {
    New(u32),
    /// It replaced an earlier one, by id or by tag, which keeps its id.
    Replaced(u32),
}

impl Added {
    pub fn id(&self) -> u32 {
        match self {
            Self::New(id) | Self::Replaced(id) => *id,
        }
    }
}

#[derive(Default, Serialize, Deserialize)]
struct Saved {
    next_id: u32,
    dnd: bool,
    last_read: u32,
    items: Vec<Notification>,
}

pub struct Store {
    /// Newest first.
    items: Vec<Notification>,
    next_id: u32,
    pub dnd: bool,
    last_read: u32,
    limit: usize,
    path: Option<PathBuf>,
}

pub fn state_file() -> PathBuf {
    std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))
        .unwrap_or_else(std::env::temp_dir)
        .join("yozora")
        .join("notify.json")
}

impl Store {
    /// A store that isn't saved anywhere.
    #[cfg(test)]
    pub fn empty(limit: usize) -> Self {
        Self { items: Vec::new(), next_id: 1, dnd: false, last_read: 0, limit, path: None }
    }

    /// Loads history from `path`, starting empty if it's missing or unreadable.
    pub fn load(path: &Path, limit: usize) -> Self {
        let saved: Saved = std::fs::read(path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        let max = saved.items.iter().map(|n| n.id).max().unwrap_or(0);
        let mut store = Self {
            next_id: saved.next_id.max(max + 1).max(1),
            items: saved.items,
            dnd: saved.dnd,
            last_read: saved.last_read,
            limit,
            path: Some(path.to_path_buf()),
        };
        store.items.retain(|n| !n.transient);
        store.trim();
        store
    }

    fn save(&self) {
        let Some(path) = &self.path else { return };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let saved = Saved { next_id: self.next_id, dnd: self.dnd, last_read: self.last_read, items: self.kept() };
        match serde_json::to_vec(&saved) {
            Ok(bytes) => {
                if let Err(e) = yozora_config::edit::write_atomic(path, &bytes) {
                    tracing::warn!("saving notification history: {e:#}");
                }
            }
            Err(e) => tracing::warn!("saving notification history: {e}"),
        }
    }

    fn kept(&self) -> Vec<Notification> {
        self.items.iter().filter(|n| !n.transient).cloned().collect()
    }

    fn trim(&mut self) {
        if self.items.len() > self.limit {
            self.items.truncate(self.limit);
        }
    }

    pub fn set_limit(&mut self, limit: usize) {
        self.limit = limit;
        self.trim();
    }

    /// Everything in the center, newest first. Transient notifications are left out.
    pub fn list(&self) -> impl Iterator<Item = &Notification> {
        self.items.iter().filter(|n| !n.transient)
    }

    pub fn get(&self, id: u32) -> Option<&Notification> {
        self.items.iter().find(|n| n.id == id)
    }

    pub fn unread(&self) -> bool {
        self.list().any(|n| n.id > self.last_read && !n.quiet)
    }

    /// Records Jev's answer on `id`, if it still has the content the answer is for.
    pub fn set_attention(
        &mut self,
        id: u32,
        odds: f64,
        quiet: bool,
        is_for: impl Fn(&Notification) -> bool,
    ) -> Option<Notification> {
        let n = self.items.iter_mut().find(|n| n.id == id && is_for(n))?;
        n.attention = Some(odds);
        n.quiet = quiet;
        let n = n.clone();
        self.save();
        Some(n)
    }

    /// Adds `n`, replacing the notification with id `replaces` or with the same tag.
    pub fn add(&mut self, mut n: Notification, replaces: u32) -> Added {
        let old = self
            .items
            .iter()
            .position(|o| replaces != 0 && o.id == replaces)
            .or_else(|| n.tag.as_ref().and_then(|t| self.items.iter().position(|o| o.tag.as_ref() == Some(t))));
        let added = match old {
            Some(i) => {
                let old = self.items.remove(i);
                n.id = old.id;
                Added::Replaced(old.id)
            }
            None => {
                n.id = self.next_id;
                self.next_id = self.next_id.wrapping_add(1).max(1);
                Added::New(n.id)
            }
        };
        if n.time == 0 {
            n.time = now();
        }
        self.items.insert(0, n);
        self.trim();
        self.save();
        added
    }

    /// Removes a notification. Returns it if it was there.
    pub fn remove(&mut self, id: u32) -> Option<Notification> {
        let i = self.items.iter().position(|n| n.id == id)?;
        let n = self.items.remove(i);
        self.save();
        Some(n)
    }

    /// Called when a toast times out: transient notifications go, the rest stay in the center.
    pub fn expired(&mut self, id: u32) {
        if self.get(id).is_some_and(|n| n.transient) {
            self.remove(id);
        }
    }

    pub fn clear(&mut self) {
        self.items.clear();
        self.save();
    }

    pub fn mark_read(&mut self) {
        self.last_read = self.next_id.wrapping_sub(1);
        self.save();
    }

    pub fn set_dnd(&mut self, on: bool) {
        self.dnd = on;
        self.save();
    }
}

/// Plain text from a body with the spec's small markup: tags dropped, entities decoded,
/// `<br>` as a line break.
pub fn strip_markup(s: &str) -> String {
    let s = s.replace("<br>", "\n").replace("<br/>", "\n").replace("<br />", "\n");
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&#39;", "'")
        .replace("&amp;", "&")
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(summary: &str) -> Notification {
        Notification { summary: summary.into(), ..Default::default() }
    }

    #[test]
    fn ids_count_up_and_replace_keeps_the_id() {
        let mut s = Store::empty(10);
        assert_eq!(s.add(n("a"), 0), Added::New(1));
        assert_eq!(s.add(n("b"), 0), Added::New(2));
        assert_eq!(s.add(n("b2"), 2), Added::Replaced(2));
        // An id that doesn't exist (any more) gets a new one.
        assert_eq!(s.add(n("c"), 99), Added::New(3));
        let summaries: Vec<_> = s.list().map(|n| n.summary.as_str()).collect();
        assert_eq!(summaries, ["c", "b2", "a"]);
    }

    #[test]
    fn quiet_ones_are_not_unread() {
        let mut s = Store::empty(10);
        s.mark_read();
        let id = s.add(n("sale"), 0).id();
        assert!(s.unread());
        // An answer for other content (it was replaced since) changes nothing.
        assert!(s.set_attention(id, 0.1, true, |n| n.summary == "old").is_none());
        assert!(s.unread());
        let got = s.set_attention(id, 0.1, true, |n| n.summary == "sale").unwrap();
        assert_eq!((got.attention, got.quiet), (Some(0.1), true));
        assert!(!s.unread());
        s.add(n("Sam: are you coming?"), 0);
        assert!(s.unread());
    }

    #[test]
    fn tags_replace_each_other() {
        let mut s = Store::empty(10);
        let vol = |v| Notification { tag: Some("volume".into()), progress: Some(v), ..n("Volume") };
        let first = s.add(vol(10), 0).id();
        assert_eq!(s.add(vol(20), 0), Added::Replaced(first));
        assert_eq!(s.list().count(), 1);
        assert_eq!(s.get(first).unwrap().progress, Some(20));
    }

    #[test]
    fn limit_unread_and_transient() {
        let mut s = Store::empty(2);
        s.add(n("a"), 0);
        s.add(n("b"), 0);
        s.add(n("c"), 0);
        assert_eq!(s.list().count(), 2);
        assert!(s.unread());
        s.mark_read();
        assert!(!s.unread());

        let id = s.add(Notification { transient: true, ..n("t") }, 0).id();
        // Transient ones are never in the center, so they don't count as unread either.
        assert!(!s.unread());
        assert!(s.get(id).is_some());
        s.expired(id);
        assert!(s.get(id).is_none());
    }

    #[test]
    fn saves_and_loads() {
        let dir = std::env::temp_dir().join(format!("yozora-notify-store-{}", std::process::id()));
        let path = dir.join("notify.json");
        let _ = std::fs::remove_dir_all(&dir);
        let mut s = Store::load(&path, 10);
        s.add(n("kept"), 0);
        s.add(Notification { transient: true, ..n("gone") }, 0);
        s.set_dnd(true);
        let s = Store::load(&path, 10);
        assert!(s.dnd);
        let summaries: Vec<_> = s.list().map(|n| n.summary.as_str()).collect();
        assert_eq!(summaries, ["kept"]);
        // Ids continue after a restart.
        let mut s = s;
        assert_eq!(s.add(n("next"), 0), Added::New(3));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn strips_markup() {
        assert_eq!(strip_markup("<b>Done</b> &amp; dusted<br>next"), "Done & dusted\nnext");
        assert_eq!(strip_markup("a &lt;b&gt; &amp;lt;"), "a <b> &lt;");
    }
}
