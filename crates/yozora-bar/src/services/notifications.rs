//! Notification history and Do Not Disturb, from mako over D-Bus.
//!
//! mako keeps showing notifications itself; the bar only reads its history and toggles its
//! `do-not-disturb` mode. mako has no call to clear its history, so "Clear all" and dismissing
//! from history hide entries here, remembered in `$XDG_STATE_HOME/yozora/notifications.json`.
//! mako doesn't record when a notification arrived, so the bar notes when it first saw each.

use std::{
    collections::{BTreeMap, HashMap},
    path::PathBuf,
    sync::Mutex,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use gpui::{App, Entity};
use serde::{Deserialize, Serialize};
use zbus::{Connection, zvariant::OwnedValue};

use super::{
    Link, Notifications,
    dbus::{self, Wake},
    icons,
};

const DEST: &str = "org.freedesktop.Notifications";
const PATH: &str = "/fr/emersion/Mako";
const IFACE: &str = "fr.emersion.Mako";
pub const DND_MODE: &str = "do-not-disturb";

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Notification {
    pub id: u32,
    pub app: String,
    pub summary: String,
    pub body: String,
    pub icon: String,
    pub desktop_entry: String,
    /// The icon file to draw, looked up and shrunk here so the UI thread doesn't.
    pub icon_path: Option<PathBuf>,
    pub urgency: u8,
    /// Unix seconds when the bar first saw it, if it was around to see it.
    pub seen: Option<u64>,
}

#[derive(Clone, Debug, Default)]
pub struct State {
    /// Newest first.
    pub items: Vec<Notification>,
    pub dnd: bool,
    pub unread: bool,
}

#[derive(Clone, Debug)]
pub enum Cmd {
    SetDnd(bool),
    Dismiss(u32),
    ClearAll,
    MarkRead,
}

pub fn start(cx: &mut App, fake: bool) -> Entity<Notifications> {
    if fake {
        super::spawn(cx, "notifications", true, run_fake)
    } else {
        super::spawn(cx, "notifications", false, |link| super::supervise(link, run))
    }
}

#[derive(Default, Serialize, Deserialize)]
struct Saved {
    /// Everything up to this id was cleared.
    cleared_through: u32,
    /// Individually dismissed ids above `cleared_through`.
    hidden: Vec<u32>,
    last_read: u32,
    seen: BTreeMap<u32, u64>,
}

fn state_file() -> PathBuf {
    let base = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))
        .unwrap_or_else(std::env::temp_dir);
    base.join("yozora").join("notifications.json")
}

impl Saved {
    fn load() -> Self {
        std::fs::read(state_file()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    fn save(&self) {
        let path = state_file();
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(bytes) = serde_json::to_vec(self) {
            let _ = yozora_config::edit::write_atomic(&path, &bytes);
        }
    }

    fn visible(&self, id: u32) -> bool {
        id > self.cleared_through && !self.hidden.contains(&id)
    }
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// Drops markup; mako passes Pango markup through in bodies.
fn strip_markup(s: &str) -> String {
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
    out.replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'")
}

fn parse(list: Vec<HashMap<String, OwnedValue>>) -> Vec<Notification> {
    list.into_iter()
        .map(|n| {
            let icon = dbus::string(&n, "app-icon");
            let desktop_entry = dbus::string(&n, "desktop-entry");
            let app = dbus::string(&n, "app-name");
            Notification {
                id: dbus::get(&n, "id").unwrap_or(0),
                icon_path: icon_path(&icon, &desktop_entry, &app),
                app,
                summary: strip_markup(&dbus::string(&n, "summary")),
                body: strip_markup(&dbus::string(&n, "body")).trim().to_string(),
                icon,
                desktop_entry,
                urgency: dbus::get(&n, "urgency").unwrap_or(1),
                seen: None,
            }
        })
        .collect()
}

/// The best icon file for a notification, looked up once per name.
fn icon_path(icon: &str, desktop_entry: &str, app: &str) -> Option<PathBuf> {
    static CACHE: Mutex<Option<HashMap<String, Option<PathBuf>>>> = Mutex::new(None);
    let key = format!("{icon}\0{desktop_entry}\0{app}");
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    cache
        .get_or_insert_with(HashMap::new)
        .entry(key)
        .or_insert_with(|| {
            icons::find(icon.strip_prefix("file://").unwrap_or(icon), &[])
                .or_else(|| icons::app_icon(desktop_entry))
                .or_else(|| icons::app_icon(app))
                // Drawn at 22 px.
                .map(|p| icons::fit(&p, 48))
        })
        .clone()
}

async fn modes(conn: &Connection) -> anyhow::Result<Vec<String>> {
    let props = dbus::get_all(conn, DEST, PATH, IFACE).await?;
    Ok(dbus::get(&props, "Modes").unwrap_or_default())
}

async fn snapshot(conn: &Connection, saved: &mut Saved, first: bool) -> anyhow::Result<State> {
    let shown: Vec<HashMap<String, OwnedValue>> = dbus::call(conn, DEST, PATH, IFACE, "ListNotifications", &()).await?;
    let history: Vec<HashMap<String, OwnedValue>> = dbus::call(conn, DEST, PATH, IFACE, "ListHistory", &()).await?;
    let mut items = parse(shown);
    for n in parse(history) {
        if !items.iter().any(|i| i.id == n.id) {
            items.push(n);
        }
    }
    // mako restarts its ids from 1; forget state from a previous mako.
    let max = items.iter().map(|n| n.id).max().unwrap_or(0);
    if max < saved.last_read.max(saved.cleared_through) {
        *saved = Saved::default();
    }
    // Entries already there when the bar starts have no known arrival time: record 0.
    let t = if first { 0 } else { now() };
    let mut changed = false;
    for n in &mut items {
        let seen = *saved.seen.entry(n.id).or_insert_with(|| {
            changed = true;
            t
        });
        n.seen = (seen > 0).then_some(seen);
    }
    saved.seen.retain(|id, _| items.iter().any(|n| n.id == *id));
    if changed {
        saved.save();
    }
    items.retain(|n| saved.visible(n.id));
    items.sort_by_key(|n| std::cmp::Reverse(n.id));
    Ok(State {
        unread: items.iter().any(|n| n.id > saved.last_read),
        items,
        dnd: modes(conn).await?.iter().any(|m| m == DND_MODE),
    })
}

async fn run(link: &Link<State, Cmd>) -> anyhow::Result<()> {
    let conn = Connection::session().await?;
    if !dbus::name_has_owner(&conn, DEST).await {
        anyhow::bail!("no notification daemon on the session bus; is mako running?");
    }
    // Fails fast when the daemon isn't mako.
    modes(&conn).await.map_err(|e| e.context("the notification daemon is not mako"))?;
    let mut stream = dbus::signals_from(&conn, DEST).await?;
    let mut saved = Saved::load();
    let first = snapshot(&conn, &mut saved, true).await?;
    link.publish(first);
    loop {
        match dbus::next(&link.cmds, &mut stream, Duration::from_millis(40)).await? {
            Wake::Quit => return Ok(()),
            Wake::Changed => {}
            Wake::Command(cmd) => match cmd {
                Cmd::SetDnd(on) => {
                    let mut m = modes(&conn).await?;
                    m.retain(|x| x != DND_MODE);
                    if on {
                        m.push(DND_MODE.into());
                    }
                    dbus::invoke(&conn, DEST, PATH, IFACE, "SetModes", &(m,)).await?;
                }
                Cmd::Dismiss(id) => {
                    saved.hidden.push(id);
                    saved.save();
                    let mut opts: HashMap<&str, zbus::zvariant::Value> = HashMap::new();
                    opts.insert("id", id.into());
                    let _ = dbus::invoke(&conn, DEST, PATH, IFACE, "DismissNotifications", &(opts,)).await;
                }
                Cmd::ClearAll => {
                    let st = snapshot(&conn, &mut saved, false).await?;
                    saved.cleared_through = st.items.iter().map(|n| n.id).max().unwrap_or(saved.cleared_through);
                    saved.hidden.clear();
                    saved.save();
                    let mut opts: HashMap<&str, zbus::zvariant::Value> = HashMap::new();
                    opts.insert("all", true.into());
                    let _ = dbus::invoke(&conn, DEST, PATH, IFACE, "DismissNotifications", &(opts,)).await;
                }
                Cmd::MarkRead => {
                    let st = snapshot(&conn, &mut saved, false).await?;
                    saved.last_read =
                        st.items.iter().map(|n| n.id).max().unwrap_or(saved.last_read).max(saved.last_read);
                    saved.save();
                }
            },
        }
        link.publish(snapshot(&conn, &mut saved, false).await?);
    }
}

fn run_fake(link: Link<State, Cmd>) {
    let t = now();
    let n = |id, app: &str, summary: &str, body: &str, ago: u64| Notification {
        id,
        app: app.into(),
        summary: summary.into(),
        body: body.into(),
        icon: String::new(),
        desktop_entry: String::new(),
        icon_path: None,
        urgency: 1,
        seen: Some(t - ago),
    };
    let mut state = State {
        items: vec![
            n(4, "Terminal", "Build finished", "yozora-bar · release profile · 0 warnings", 120),
            n(3, "Firefox", "Download complete", "hyprland-0.56.2.tar.gz", 720),
            n(2, "System", "Updates available", "14 packages can be upgraded", 3600),
            n(1, "Syncthing", "Folder up to date", "Projects/yozora", 3 * 3600),
        ],
        dnd: false,
        unread: true,
    };
    link.publish(state.clone());
    while let Ok(cmd) = link.cmds.recv_blocking() {
        match cmd {
            Cmd::SetDnd(on) => state.dnd = on,
            Cmd::Dismiss(id) => state.items.retain(|n| n.id != id),
            Cmd::ClearAll => state.items.clear(),
            Cmd::MarkRead => state.unread = false,
        }
        if !link.publish(state.clone()) {
            break;
        }
    }
}

pub async fn check() -> anyhow::Result<String> {
    let conn = Connection::session().await?;
    let modes = modes(&conn).await.map_err(|e| e.context("is mako running?"))?;
    let history: Vec<HashMap<String, OwnedValue>> = dbus::call(&conn, DEST, PATH, IFACE, "ListHistory", &()).await?;
    Ok(format!("mako · {} in history · modes: {}", history.len(), modes.join(", ")))
}

#[cfg(test)]
mod tests {
    #[test]
    fn strips_markup() {
        assert_eq!(super::strip_markup("<b>Done</b> &amp; dusted"), "Done & dusted");
    }
}
