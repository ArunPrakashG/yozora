//! Notification history and Do Not Disturb, from Yozora Notify or else mako, over D-Bus.
//!
//! Either daemon shows notifications itself; the bar reads the history for its notification
//! center and toggles Do Not Disturb.
//!
//! Yozora Notify keeps the history, the read marker and Do Not Disturb, and the bar only asks.
//! mako has no call to clear its history, so with mako "Clear all" and dismissing from history
//! hide entries here, remembered in `$XDG_STATE_HOME/yozora/notifications.json`. mako doesn't
//! record when a notification arrived either, so the bar notes when it first saw each.

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
const YZ_PATH: &str = "/dev/yozora/Notify";
const YZ_IFACE: &str = "dev.yozora.Notify1";
pub const DND_MODE: &str = "do-not-disturb";

/// Which daemon the history comes from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Daemon {
    #[default]
    Yozora,
    Mako,
}

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
    /// Unix seconds when it arrived, or when the bar first saw it if the daemon doesn't say.
    pub seen: Option<u64>,
    /// Clicking it opens something in the app.
    pub default_action: bool,
    /// From Jev, through Yozora Notify: it can't wait.
    pub urgent: bool,
    /// From Jev: it can wait.
    pub quiet: bool,
    /// From Jev: hidden while the screen is shared.
    pub private: bool,
    /// From Jev: message, alert, reminder, update or promotion; empty if unknown.
    pub kind: String,
    /// From Jev: it asks for something, so it offers pinning and a reminder.
    pub needs_action: bool,
    pub pinned: bool,
}

#[derive(Clone, Debug, Default)]
pub struct State {
    /// Newest first.
    pub items: Vec<Notification>,
    pub dnd: bool,
    pub unread: bool,
    pub daemon: Daemon,
}

#[derive(Clone, Debug)]
pub enum Cmd {
    SetDnd(bool),
    Dismiss(u32),
    ClearAll,
    MarkRead,
    /// Runs a notification's default action. Yozora Notify only.
    Activate(u32),
    /// Pins or unpins one. Yozora Notify only.
    Pin(u32, bool),
    /// Hides one for this many minutes, then shows it again. Yozora Notify only.
    Snooze(u32, u32),
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
            // Yozora Notify has already looked the icon up and sized it.
            let resolved = Some(dbus::string(&n, "icon")).filter(|p| !p.is_empty()).map(PathBuf::from);
            Notification {
                id: dbus::get(&n, "id").unwrap_or(0),
                icon_path: resolved.or_else(|| icon_path(&icon, &desktop_entry, &app)),
                app,
                summary: strip_markup(&dbus::string(&n, "summary")),
                body: strip_markup(&dbus::string(&n, "body")).trim().to_string(),
                icon,
                desktop_entry,
                urgency: dbus::get(&n, "urgency").unwrap_or(1),
                seen: dbus::get::<u64>(&n, "time").filter(|t| *t > 0),
                default_action: dbus::get(&n, "default-action").unwrap_or(false),
                urgent: dbus::get(&n, "urgent").unwrap_or(false),
                quiet: dbus::get(&n, "quiet").unwrap_or(false),
                private: dbus::get(&n, "private").unwrap_or(false),
                kind: dbus::string(&n, "kind"),
                needs_action: dbus::get(&n, "needs-action").unwrap_or(false),
                pinned: dbus::get(&n, "pinned").unwrap_or(false),
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
                // Drawn at 22 px; symbolic icons are drawn as masks, which GPUI keeps sharp itself.
                .map(|p| if icons::is_symbolic(&p) { p } else { icons::fit(&p, 44) })
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
        daemon: Daemon::Mako,
    })
}

async fn yozora_state(conn: &Connection) -> anyhow::Result<(bool, bool)> {
    dbus::call(conn, DEST, YZ_PATH, YZ_IFACE, "GetState", &()).await
}

async fn yozora_snapshot(conn: &Connection) -> anyhow::Result<State> {
    let list: Vec<HashMap<String, OwnedValue>> = dbus::call(conn, DEST, YZ_PATH, YZ_IFACE, "List", &()).await?;
    let (dnd, unread) = yozora_state(conn).await?;
    Ok(State { items: parse(list), dnd, unread, daemon: Daemon::Yozora })
}

async fn run(link: &Link<State, Cmd>) -> anyhow::Result<()> {
    let conn = Connection::session().await?;
    if !dbus::name_has_owner(&conn, DEST).await {
        anyhow::bail!("no notification server on the session bus; start yozora-notify or mako");
    }
    if yozora_state(&conn).await.is_ok() {
        return run_yozora(link, &conn).await;
    }
    // Fails fast when the daemon is neither.
    modes(&conn).await.map_err(|e| e.context("the notification server is neither yozora-notify nor mako"))?;
    run_mako(link, &conn).await
}

async fn run_yozora(link: &Link<State, Cmd>, conn: &Connection) -> anyhow::Result<()> {
    let mut stream = dbus::signals_from(conn, DEST).await?;
    link.publish(yozora_snapshot(conn).await?);
    loop {
        match dbus::next(&link.cmds, &mut stream, Duration::from_millis(40)).await? {
            Wake::Quit => return Ok(()),
            Wake::Changed => {}
            Wake::Command(cmd) => {
                let call = |method: &'static str| dbus::invoke(conn, DEST, YZ_PATH, YZ_IFACE, method, &());
                match cmd {
                    Cmd::SetDnd(on) => dbus::invoke(conn, DEST, YZ_PATH, YZ_IFACE, "SetDoNotDisturb", &(on,)).await?,
                    Cmd::Dismiss(id) => dbus::invoke(conn, DEST, YZ_PATH, YZ_IFACE, "Dismiss", &(id,)).await?,
                    Cmd::Activate(id) => {
                        dbus::invoke(conn, DEST, YZ_PATH, YZ_IFACE, "InvokeAction", &(id, "default")).await?
                    }
                    Cmd::ClearAll => call("ClearAll").await?,
                    Cmd::MarkRead => call("MarkRead").await?,
                    Cmd::Pin(id, on) => dbus::invoke(conn, DEST, YZ_PATH, YZ_IFACE, "Pin", &(id, on)).await?,
                    Cmd::Snooze(id, minutes) => {
                        dbus::invoke(conn, DEST, YZ_PATH, YZ_IFACE, "Snooze", &(id, minutes)).await?
                    }
                }
            }
        }
        // The daemon may have been replaced, by mako say: start over to find out.
        if yozora_state(conn).await.is_err() {
            anyhow::bail!("the notification server went away");
        }
        link.publish(yozora_snapshot(conn).await?);
    }
}

async fn run_mako(link: &Link<State, Cmd>, conn: &Connection) -> anyhow::Result<()> {
    let conn = conn.clone();
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
                // mako runs actions from its own popups only, and has no pins or snoozes.
                Cmd::Activate(_) | Cmd::Pin(..) | Cmd::Snooze(..) => {}
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
        ..Default::default()
    };
    let mut state = State {
        // As sorted by Jev: the kind, then urgent, private, asks for something, or can wait.
        items: vec![
            Notification {
                kind: "message".into(),
                urgent: true,
                private: true,
                needs_action: true,
                ..n(7, "Signal", "Sam", "Are you still coming at 5? I'm outside", 60)
            },
            Notification {
                kind: "alert".into(),
                urgent: true,
                needs_action: true,
                ..n(6, "Slack", "Deploy failed", "prod-api: rollout stuck, 3/8 pods crashlooping", 300)
            },
            Notification {
                kind: "update".into(),
                ..n(4, "Terminal", "Build finished", "yozora-bar · 0 warnings", 120)
            },
            Notification {
                kind: "update".into(),
                ..n(3, "Firefox", "Download complete", "hyprland-0.56.2.tar.gz", 720)
            },
            Notification {
                kind: "promotion".into(),
                quiet: true,
                ..n(5, "Steam", "Summer Sale", "Up to 90% off", 1800)
            },
            Notification {
                kind: "update".into(),
                quiet: true,
                ..n(2, "Software Update", "Updates available", "14 packages can be upgraded", 3600)
            },
            n(1, "Syncthing", "Folder up to date", "Projects/yozora", 3 * 3600),
        ],
        dnd: false,
        unread: true,
        daemon: Daemon::Yozora,
    };
    link.publish(state.clone());
    while let Ok(cmd) = link.cmds.recv_blocking() {
        match cmd {
            Cmd::SetDnd(on) => state.dnd = on,
            Cmd::Dismiss(id) => state.items.retain(|n| n.id != id),
            Cmd::ClearAll => state.items.clear(),
            Cmd::MarkRead => state.unread = false,
            Cmd::Activate(id) | Cmd::Snooze(id, _) => state.items.retain(|n| n.id != id),
            Cmd::Pin(id, on) => {
                if let Some(n) = state.items.iter_mut().find(|n| n.id == id) {
                    n.pinned = on;
                }
            }
        }
        if !link.publish(state.clone()) {
            break;
        }
    }
}

pub async fn check() -> anyhow::Result<String> {
    let conn = Connection::session().await?;
    if let Ok(st) = yozora_snapshot(&conn).await {
        let dnd = if st.dnd { "on" } else { "off" };
        return Ok(format!("yozora-notify · {} in history · do not disturb {dnd}", st.items.len()));
    }
    let modes = modes(&conn).await.map_err(|e| e.context("is yozora-notify or mako running?"))?;
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
