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
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

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

/// A notification that is on screen as a toast. The daemon keeps the clock; this is what it said
/// at `at`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Toast {
    /// Among the newest ones drawn now. The rest wait for room.
    pub visible: bool,
    /// The clock is ticking, rather than held or waiting.
    pub running: bool,
    /// Time left when `at` was read; none for toasts that stay until closed.
    pub left: Option<Duration>,
    pub total: Option<Duration>,
    pub at: Instant,
}

impl Toast {
    /// Time left now.
    pub fn remaining(&self, now: Instant) -> Option<Duration> {
        let left = self.left?;
        Some(if self.running { left.saturating_sub(now.saturating_duration_since(self.at)) } else { left })
    }

    /// How much of the time is left, 1 down to 0, if it runs out.
    pub fn fraction(&self, now: Instant) -> Option<f32> {
        let (left, total) = (self.remaining(now)?, self.total?);
        Some((left.as_secs_f32() / total.as_secs_f32().max(0.001)).clamp(0.0, 1.0))
    }
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
    pub pinned: bool,
    /// Buttons, as key and label, without the default action. Yozora Notify only.
    pub actions: Vec<(String, String)>,
    /// Unix seconds when a snoozed notification comes back, or 0.
    pub snoozed_until: u64,
    /// Arrived since the center was last opened.
    pub unread: bool,
    /// A picture the app attached, sized for drawing. Yozora Notify only.
    pub image: Option<PathBuf>,
    /// The picture is wide: drawn under the body rather than as the avatar.
    pub preview: bool,
    /// The app's progress, 0 to 100.
    pub progress: Option<u8>,
    /// The label for the reply field, if the app takes inline replies.
    pub reply: Option<String>,
    /// On screen as a toast right now.
    pub toast: Option<Toast>,
}

#[derive(Clone, Debug, Default)]
pub struct State {
    /// Newest first.
    pub items: Vec<Notification>,
    pub dnd: bool,
    pub unread: bool,
    pub daemon: Daemon,
    /// How many toasts show at once, and the monitor they open on ("focused" or a connector).
    pub toast_max: u32,
    pub toast_monitor: String,
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
    /// Hides one for this many minutes, then shows it again; 0 brings it back now. Yozora
    /// Notify only.
    Snooze(u32, u32),
    /// Runs one of a notification's buttons. Yozora Notify only.
    Action(u32, String),
    /// Answers a notification that takes inline replies. Yozora Notify only.
    Reply(u32, String),
    /// Takes a toast off the screen and leaves it in the center. Yozora Notify only.
    Expire(u32),
    /// Stops the timers of these toasts, and runs the rest: the pointer is on them or a reply is
    /// being typed. Yozora Notify only.
    Hold(Vec<u32>),
}

pub fn start(fake: bool) -> Notifications {
    if fake {
        super::spawn("notifications", true, run_fake)
    } else {
        super::spawn("notifications", false, |link| super::supervise(link, run))
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
                pinned: dbus::get(&n, "pinned").unwrap_or(false),
                actions: dbus::get::<Vec<String>>(&n, "actions")
                    .unwrap_or_default()
                    .chunks(2)
                    .filter(|c| c.len() == 2)
                    .map(|c| (c[0].clone(), c[1].clone()))
                    .collect(),
                snoozed_until: dbus::get(&n, "snoozed-until").unwrap_or(0),
                unread: dbus::get(&n, "unread").unwrap_or(false),
                image: Some(dbus::string(&n, "image")).filter(|p| !p.is_empty()).map(PathBuf::from),
                preview: dbus::get(&n, "preview").unwrap_or(false),
                progress: dbus::get::<i32>(&n, "progress").filter(|p| *p >= 0).map(|p| p.min(100) as u8),
                reply: Some(dbus::string(&n, "reply-label")).filter(|l| !l.is_empty()),
                toast: dbus::get::<bool>(&n, "toast").unwrap_or(false).then(|| {
                    let ms = |k: &str| dbus::get::<i64>(&n, k).filter(|m| *m >= 0).map(|m| Duration::from_millis(m as u64));
                    Toast {
                        visible: dbus::get(&n, "toast-visible").unwrap_or(false),
                        running: dbus::get(&n, "toast-running").unwrap_or(false),
                        left: ms("toast-left-ms"),
                        total: ms("toast-total-ms"),
                        at: Instant::now(),
                    }
                }),
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
    for n in &mut items {
        n.unread = n.id > saved.last_read;
    }
    Ok(State {
        unread: items.iter().any(|n| n.id > saved.last_read),
        items,
        dnd: modes(conn).await?.iter().any(|m| m == DND_MODE),
        daemon: Daemon::Mako,
        toast_max: 3,
        toast_monitor: "focused".into(),
    })
}

async fn yozora_state(conn: &Connection) -> anyhow::Result<(bool, bool)> {
    dbus::call(conn, DEST, YZ_PATH, YZ_IFACE, "GetState", &()).await
}

async fn yozora_snapshot(conn: &Connection) -> anyhow::Result<State> {
    let list: Vec<HashMap<String, OwnedValue>> = dbus::call(conn, DEST, YZ_PATH, YZ_IFACE, "List", &()).await?;
    let (dnd, unread) = yozora_state(conn).await?;
    // An older daemon has no settings call: show three, on the focused monitor.
    let (toast_max, toast_monitor, _): (u32, String, bool) =
        dbus::call(conn, DEST, YZ_PATH, YZ_IFACE, "GetConfig", &()).await.unwrap_or((3, "focused".into(), false));
    Ok(State { items: parse(list), dnd, unread, daemon: Daemon::Yozora, toast_max, toast_monitor })
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
                    Cmd::Action(id, key) => {
                        dbus::invoke(conn, DEST, YZ_PATH, YZ_IFACE, "InvokeAction", &(id, key.as_str())).await?
                    }
                    Cmd::Reply(id, text) => {
                        dbus::invoke(conn, DEST, YZ_PATH, YZ_IFACE, "Reply", &(id, text.as_str())).await?
                    }
                    Cmd::Expire(id) => dbus::invoke(conn, DEST, YZ_PATH, YZ_IFACE, "ExpireToast", &(id,)).await?,
                    Cmd::Hold(ids) => dbus::invoke(conn, DEST, YZ_PATH, YZ_IFACE, "HoldToasts", &(ids,)).await?,
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
                Cmd::Activate(_) | Cmd::Pin(..) | Cmd::Snooze(..) | Cmd::Action(..) | Cmd::Reply(..) | Cmd::Expire(_) | Cmd::Hold(_) => {}
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
        unread: ago < 600,
        ..Default::default()
    };
    let mut state = State {
        items: vec![
            Notification {
                pinned: true,
                ..n(9, "Calendar", "Ship the notifications rebuild", "Due Friday 18:00", 7200)
            },
            n(8, "Signal", "Priya Raman", "Are we still on for the review at 4? I moved the room to Kestrel.", 20),
            n(7, "Signal", "Sam", "Are you still coming at 5? I'm outside", 60),
            n(6, "Signal", "Priya Raman", "Sent a photo", 400),
            Notification {
                actions: vec![("join".into(), "Join".into()), ("later".into(), "Snooze 5 min".into())],
                ..n(5, "Calendar", "Design review in 10 minutes", "Kestrel · 16:00 to 16:30", 180)
            },
            n(4, "GitHub", "CI passed on #28", "Rebuild notifications on the new tokens · 4 checks in 3 m 12 s", 70),
            n(3, "Firefox", "Download complete", "hyprland-0.56.2.tar.gz · 14.8 MB", 120),
            Notification { snoozed_until: t + 1800, ..n(2, "Calendar", "Standup notes", "", 3600) },
        ],
        dnd: false,
        unread: true,
        daemon: Daemon::Yozora,
        toast_max: 3,
        toast_monitor: "focused".into(),
    };
    fake_toasts(&mut state, t);
    link.publish(state.clone());
    while let Ok(cmd) = link.cmds.recv_blocking() {
        match cmd {
            Cmd::SetDnd(on) => state.dnd = on,
            Cmd::Dismiss(id) => state.items.retain(|n| n.id != id),
            Cmd::ClearAll => state.items.clear(),
            Cmd::MarkRead => {
                state.unread = false;
                state.items.iter_mut().for_each(|n| n.unread = false);
            }
            Cmd::Activate(id) | Cmd::Action(id, _) => state.items.retain(|n| n.id != id),
            Cmd::Snooze(id, minutes) => {
                if let Some(n) = state.items.iter_mut().find(|n| n.id == id) {
                    n.snoozed_until = if minutes == 0 { 0 } else { now() + u64::from(minutes) * 60 };
                }
            }
            Cmd::Pin(id, on) => {
                if let Some(n) = state.items.iter_mut().find(|n| n.id == id) {
                    n.pinned = on;
                }
            }
            Cmd::Reply(id, _) => state.items.retain(|n| n.id != id),
            Cmd::Expire(id) => {
                if let Some(n) = state.items.iter_mut().find(|n| n.id == id) {
                    n.toast = None;
                }
            }
            Cmd::Hold(_) => {}
        }
        if !link.publish(state.clone()) {
            break;
        }
    }
}

/// With `YZ_TOASTS` set to a list of ids, adds a few richer notifications to the demo list and
/// shows the listed ones as toasts that stand still, a little over half way through their time.
fn fake_toasts(state: &mut State, t: u64) {
    let Some(ids) = std::env::var("YZ_TOASTS").ok().filter(|v| !v.is_empty()) else { return };
    let (avatar, preview) = fake_images();
    let n = |id, app: &str, summary: &str, body: &str, ago: u64| Notification {
        id,
        app: app.into(),
        summary: summary.into(),
        body: body.into(),
        urgency: 1,
        seen: Some(t - ago),
        unread: true,
        ..Default::default()
    };
    state.items.splice(
        0..0,
        [
            Notification {
                image: avatar,
                reply: Some("Reply".into()),
                default_action: true,
                ..n(13, "Signal", "Mira Okafor", "Did the build finish? I can merge once it is green.", 5)
            },
            Notification { image: preview, preview: true, ..n(12, "Screenshots", "Screenshot saved", "Pictures/Screenshots/shot-14-02.png", 8) },
            Notification { urgency: 2, ..n(11, "System", "Backup failed", "The disk is full. 2 folders were not copied.", 12) },
            Notification { progress: Some(64), ..n(10, "Downloads", "hyprland-0.56.2.tar.gz", "14.8 MB of 23.1 MB", 15) },
        ],
    );
    let wanted: Vec<u32> = ids.split(',').filter_map(|i| i.trim().parse().ok()).collect();
    for item in &mut state.items {
        if wanted.contains(&item.id) {
            let total = Duration::from_millis(if item.urgency == 2 { 0 } else { 6000 });
            item.toast = Some(Toast {
                visible: wanted.iter().position(|w| *w == item.id).is_some_and(|i| i < 3),
                running: false,
                left: (item.urgency != 2).then(|| total * 6 / 10),
                total: (item.urgency != 2).then_some(total),
                at: Instant::now(),
            });
        }
    }
    // Most recently listed first, like a stack of arrivals.
    state.items.sort_by_key(|n| std::cmp::Reverse(n.id));
}

/// A square avatar and a wide picture for the demo list, written once to the temp directory.
fn fake_images() -> (Option<PathBuf>, Option<PathBuf>) {
    let dir = std::env::temp_dir().join("yozora-fake-notify");
    let _ = std::fs::create_dir_all(&dir);
    let make = |name: &str, w: u32, h: u32, f: &dyn Fn(u32, u32) -> [u8; 3]| {
        let path = dir.join(name);
        if !path.exists() {
            let img = image::RgbImage::from_fn(w, h, |x, y| image::Rgb(f(x, y)));
            img.save(&path).ok()?;
        }
        Some(path)
    };
    let avatar = make("avatar.png", 72, 72, &|x, y| [(80 + x * 2) as u8, (120 + y) as u8, 200]);
    let preview = make("preview.png", 712, 300, &|x, y| {
        let band = ((x / 40 + y / 40) % 2) as u8 * 18;
        [(40 + x / 6) as u8 + band, (70 + y / 4) as u8 + band, 150 + band]
    });
    (avatar, preview)
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
