//! The notifications themselves: what the server received, what the center keeps, and Do Not
//! Disturb. Plain data with no D-Bus or UI, so it can be tested on its own.
//!
//! History is saved to `$XDG_STATE_HOME/yozora/notify.json` after every change, so the center
//! survives restarts. Ids keep counting up across restarts, so an id never means two things.

use std::{
    collections::BTreeMap,
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
    /// Whether someone else shouldn't see it, by Jev's answer. Unknown until it answers.
    pub private: Option<bool>,
    /// What it is, by Jev's answer: one of [`KINDS`], or empty.
    pub kind: String,
    /// It asks them to do something, by Jev's answer.
    pub needs_action: bool,
    /// Kept at the top of the center, and through "Clear all".
    pub pinned: bool,
    /// Unix seconds when a snoozed notification comes back; 0 if it isn't snoozed.
    pub snoozed_until: u64,
    /// Already shown again once because it was urgent and unread.
    pub reminded: bool,
}

/// What Jev can say a notification is.
pub const KINDS: [&str; 5] = ["message", "alert", "reminder", "update", "promotion"];

/// Jev's answers about one notification.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Answer {
    pub attention: f64,
    pub private: Option<bool>,
    pub kind: Option<String>,
    pub needs_action: Option<bool>,
}

/// How someone handles an app's notifications, sent to Jev so it learns what matters to them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Habit {
    /// Clicked, replied to or acted on.
    pub opened: u32,
    /// Closed without opening.
    pub dismissed: u32,
}

/// Apps whose habits are remembered; past this, the least used are forgotten.
const MAX_HABITS: usize = 200;

fn habit_key(n: &Notification) -> String {
    if n.app.trim().is_empty() { n.desktop_entry.to_lowercase() } else { n.app.trim().to_lowercase() }
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
#[serde(default)]
struct Saved {
    next_id: u32,
    dnd: bool,
    last_read: u32,
    items: Vec<Notification>,
    habits: BTreeMap<String, Habit>,
}

pub struct Store {
    /// Newest first.
    items: Vec<Notification>,
    next_id: u32,
    pub dnd: bool,
    last_read: u32,
    limit: usize,
    path: Option<PathBuf>,
    habits: BTreeMap<String, Habit>,
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
        Self { items: Vec::new(), next_id: 1, dnd: false, last_read: 0, limit, path: None, habits: BTreeMap::new() }
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
            habits: saved.habits,
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
        let saved = Saved {
            next_id: self.next_id,
            dnd: self.dnd,
            last_read: self.last_read,
            items: self.kept(),
            habits: self.habits.clone(),
        };
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

    /// Whether anything arrived since the center was last opened that's worth a dot on the bell:
    /// not quiet, not snoozed, and with `at_least`, not answered by Jev below those odds.
    pub fn unread(&self, at_least: Option<f64>) -> bool {
        self.list().any(|n| {
            n.id > self.last_read
                && !n.quiet
                && n.snoozed_until == 0
                && at_least.is_none_or(|t| n.attention.is_none_or(|a| a >= t))
        })
    }

    /// Records Jev's answers on `id`, if it still has the content they're for.
    pub fn set_answer(
        &mut self,
        id: u32,
        answer: &Answer,
        quiet: bool,
        is_for: impl Fn(&Notification) -> bool,
    ) -> Option<Notification> {
        let n = self.items.iter_mut().find(|n| n.id == id && is_for(n))?;
        n.attention = Some(answer.attention);
        n.quiet = quiet;
        n.private = answer.private.or(n.private);
        if let Some(kind) = &answer.kind {
            n.kind = kind.clone();
        }
        n.needs_action = answer.needs_action.unwrap_or(n.needs_action);
        let n = n.clone();
        self.save();
        Some(n)
    }

    /// Notes that nothing will say whether `id` is private, so it's treated as not.
    pub fn not_private(&mut self, id: u32) {
        if let Some(n) = self.items.iter_mut().find(|n| n.id == id) {
            n.private = Some(false);
        }
    }

    pub fn pin(&mut self, id: u32, on: bool) -> bool {
        let Some(n) = self.items.iter_mut().find(|n| n.id == id) else { return false };
        n.pinned = on;
        self.save();
        true
    }

    /// Hides `id` from the center until `until`, when it is shown again.
    pub fn snooze(&mut self, id: u32, until: u64) -> bool {
        let Some(n) = self.items.iter_mut().find(|n| n.id == id) else { return false };
        n.snoozed_until = until;
        self.save();
        true
    }

    /// Notifications to show again at `now`: snoozed ones whose time has come, and, after
    /// `remind_after` seconds, unread ones Jev gave at least `urgent` odds, once each.
    pub fn due(&mut self, now: u64, remind_after: Option<u64>, urgent: f64) -> Vec<Notification> {
        let last_read = self.last_read;
        let mut due = Vec::new();
        for n in self.items.iter_mut().filter(|n| !n.transient) {
            if n.snoozed_until != 0 && n.snoozed_until <= now {
                n.snoozed_until = 0;
                due.push(n.clone());
            } else if let Some(after) = remind_after
                && n.snoozed_until == 0
                && !n.reminded
                && n.id > last_read
                && n.attention.is_some_and(|a| a >= urgent)
                && n.time + after <= now
            {
                n.reminded = true;
                due.push(n.clone());
            }
        }
        if !due.is_empty() {
            self.save();
        }
        due
    }

    /// How `n`'s app has been handled, once there's enough to go on.
    pub fn habit(&self, n: &Notification) -> Option<Habit> {
        self.habits.get(&habit_key(n)).copied().filter(|h| h.opened + h.dismissed >= 3)
    }

    /// Counts `id` as opened, or as dismissed without opening.
    pub fn note(&mut self, id: u32, opened: bool) {
        let Some(key) = self.get(id).map(habit_key).filter(|k| !k.is_empty()) else { return };
        if !self.habits.contains_key(&key) && self.habits.len() >= MAX_HABITS {
            let least = self.habits.iter().min_by_key(|(_, h)| h.opened + h.dismissed).map(|(k, _)| k.clone());
            if let Some(least) = least {
                self.habits.remove(&least);
            }
        }
        let h = self.habits.entry(key).or_default();
        if opened {
            h.opened = h.opened.saturating_add(1);
        } else {
            h.dismissed = h.dismissed.saturating_add(1);
        }
        self.save();
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

    /// Empties the center, except pinned notifications.
    pub fn clear(&mut self) {
        self.items.retain(|n| n.pinned);
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
        assert!(s.unread(None));
        // An answer for other content (it was replaced since) changes nothing.
        let low = Answer { attention: 0.1, ..Default::default() };
        assert!(s.set_answer(id, &low, true, |n| n.summary == "old").is_none());
        assert!(s.unread(None));
        let got = s.set_answer(id, &low, true, |n| n.summary == "sale").unwrap();
        assert_eq!((got.attention, got.quiet), (Some(0.1), true));
        assert!(!s.unread(None));
        let sam = s.add(n("Sam: are you coming?"), 0).id();
        assert!(s.unread(None));
        // With a bar for the dot, only what Jev hasn't answered yet or rates high counts.
        assert!(s.unread(Some(0.85)));
        s.set_answer(sam, &Answer { attention: 0.5, ..low.clone() }, false, |_| true);
        assert!(s.unread(None) && !s.unread(Some(0.85)));
        s.set_answer(sam, &Answer { attention: 0.9, ..low }, false, |_| true);
        assert!(s.unread(Some(0.85)));
    }

    #[test]
    fn reminders_snoozes_and_pins() {
        let mut s = Store::empty(10);
        let urgent = s.add(Notification { time: 100, ..n("Deploy failed") }, 0).id();
        let plain = s.add(Notification { time: 100, ..n("Newsletter") }, 0).id();
        s.set_answer(urgent, &Answer { attention: 0.9, ..Default::default() }, false, |_| true);
        s.set_answer(plain, &Answer { attention: 0.4, ..Default::default() }, false, |_| true);
        assert!(s.due(500, Some(600), 0.85).is_empty(), "not yet");
        let due: Vec<u32> = s.due(700, Some(600), 0.85).iter().map(|n| n.id).collect();
        assert_eq!(due, [urgent]);
        assert!(s.due(9999, Some(600), 0.85).is_empty(), "only once");

        assert!(s.snooze(plain, 800));
        assert!(s.due(799, None, 0.85).is_empty());
        assert_eq!(s.due(800, None, 0.85).len(), 1);
        assert_eq!(s.get(plain).unwrap().snoozed_until, 0);

        s.pin(plain, true);
        s.clear();
        let left: Vec<u32> = s.list().map(|n| n.id).collect();
        assert_eq!(left, [plain], "pinned ones survive Clear all");
    }

    #[test]
    fn habits_need_a_few_to_count() {
        let mut s = Store::empty(10);
        let note = |s: &mut Store, opened| {
            let id = s.add(Notification { app: "Shop".into(), ..n("Sale") }, 0).id();
            s.note(id, opened);
        };
        note(&mut s, false);
        note(&mut s, false);
        let probe = Notification { app: "shop ".into(), ..n("x") };
        assert_eq!(s.habit(&probe), None);
        note(&mut s, true);
        assert_eq!(s.habit(&probe), Some(Habit { opened: 1, dismissed: 2 }));
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
        assert!(s.unread(None));
        s.mark_read();
        assert!(!s.unread(None));

        let id = s.add(Notification { transient: true, ..n("t") }, 0).id();
        // Transient ones are never in the center, so they don't count as unread either.
        assert!(!s.unread(None));
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
