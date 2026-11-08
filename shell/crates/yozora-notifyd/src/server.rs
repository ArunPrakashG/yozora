//! The D-Bus side: `org.freedesktop.Notifications` for apps, and `dev.yozora.Notify1` for the
//! bar's notification center.
//!
//! zbus answers method calls on its own thread. The store sits behind a mutex that is never held
//! across an await. Nothing is drawn here: the shell draws toasts from the history this serves.
//! The daemon still decides what would be a toast ([`should_toast`], [`toast_timeout`]) so it can
//! close notifications on time, tell apps they expired and wake snoozed ones. What the person or a
//! script does arrives as [`Cmd`].

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use anyhow::Context as _;
use async_channel::{Receiver, Sender};
use async_io::Timer;
use futures_lite::{FutureExt, StreamExt};
use serde_json::json;
use yozora_config::notify::NotifyConfig;
use yozora_ipc::{Incoming, Response};
use yozora_services::icon_theme as icons;
use zbus::{
    Connection, fdo, interface,
    object_server::SignalEmitter,
    zvariant::{OwnedValue, Value},
};

use crate::store::{Notification, Store, Urgency, now, strip_markup};

pub const NAME: &str = "org.freedesktop.Notifications";
pub const FDO_PATH: &str = "/org/freedesktop/Notifications";
pub const YZ_PATH: &str = "/dev/yozora/Notify";

/// Why a notification closed, from the spec.
#[derive(Clone, Copy, Debug)]
pub enum Reason {
    Expired = 1,
    Dismissed = 2,
    Closed = 3,
}

/// What happened, from the shell, the control socket or the daemon's own timers.
pub enum Cmd {
    /// Its time on screen ran out.
    Expired(u32),
    Dismissed(u32),
    /// Closed by a script or an agent rather than by the person.
    Closed(u32),
    Action(u32, String),
    Reply(u32, String),
    Ipc(Incoming),
    /// Hyprland says the screen is being shared, or not.
    Sharing(bool),
    /// The center opened: everything on screen but critical notifications is done.
    ExpireAll,
    /// New settings from `notify.toml`.
    Config(NotifyConfig),
    /// A timer changed; look at the clock again.
    Wake,
    Quit,
}

pub struct Inner {
    pub store: Store,
    pub config: NotifyConfig,
    pub sharing: bool,
    /// Notifications that would be on screen as toasts, and when each times out, if it does.
    shown: HashMap<u32, Option<Instant>>,
}

impl Inner {
    pub fn new(store: Store, config: NotifyConfig) -> Self {
        Self { store, config, sharing: false, shown: HashMap::new() }
    }

    fn unread(&self) -> bool {
        self.store.unread()
    }

    /// Notes that `n` is on screen from `now`.
    fn show(&mut self, n: &Notification, now: Instant) {
        let until = toast_timeout(n, &self.config).map(|d| now + d);
        self.shown.insert(n.id, until);
    }

    fn remove(&mut self, id: u32) -> Option<Notification> {
        self.shown.remove(&id);
        self.store.remove(id)
    }

    /// The ids whose time is up at `now`, no longer on screen.
    fn timed_out(&mut self, now: Instant) -> Vec<u32> {
        let ids: Vec<u32> = self.shown.iter().filter(|(_, until)| until.is_some_and(|u| u <= now)).map(|(id, _)| *id).collect();
        for id in &ids {
            self.shown.remove(id);
        }
        ids
    }

    /// The ids on screen that aren't critical, no longer on screen.
    fn take_expirable(&mut self) -> Vec<u32> {
        let ids: Vec<u32> = self
            .shown
            .keys()
            .copied()
            .filter(|id| self.store.get(*id).is_some_and(|n| n.urgency != Urgency::Critical))
            .collect();
        for id in &ids {
            self.shown.remove(id);
        }
        ids
    }

    /// When the daemon next has something to do: a timeout or a snoozed notification coming back.
    /// Never more than 30 seconds away, so the clock jumping or a suspend can't leave it asleep.
    fn next_wake(&self, now: Instant) -> Instant {
        let mut at = now + Duration::from_secs(30);
        if let Some(until) = self.shown.values().flatten().min() {
            at = at.min(*until);
        }
        if let Some(snooze) = self.store.next_snooze() {
            at = at.min(now + Duration::from_secs(snooze.saturating_sub(crate::store::now()) + 1));
        }
        at
    }
}

#[derive(Clone)]
pub struct Shared {
    pub inner: Arc<Mutex<Inner>>,
    pub cmds: Sender<Cmd>,
}

impl Shared {
    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn wake(&self) {
        let _ = self.cmds.try_send(Cmd::Wake);
    }
}

struct Fdo {
    shared: Shared,
}

#[interface(name = "org.freedesktop.Notifications")]
impl Fdo {
    fn get_capabilities(&self) -> Vec<&'static str> {
        vec![
            "actions",
            "body",
            "body-markup",
            "icon-static",
            "persistence",
            "inline-reply",
            "x-canonical-private-synchronous",
        ]
    }

    #[allow(clippy::too_many_arguments)]
    async fn notify(
        &self,
        app_name: String,
        replaces_id: u32,
        app_icon: String,
        summary: String,
        body: String,
        actions: Vec<String>,
        hints: HashMap<String, OwnedValue>,
        expire_timeout: i32,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> u32 {
        let n = build(app_name, app_icon, summary, body, actions, &hints, expire_timeout);
        let (added, show) = {
            let mut inner = self.shared.lock();
            let show = should_toast(&n, inner.store.dnd, &inner.config);
            let added = inner.store.add(n, replaces_id);
            let stored = inner.store.get(added.id()).cloned();
            // An update that no longer toasts (Do Not Disturb came on) takes the old one off screen.
            inner.shown.remove(&added.id());
            if let Some(n) = stored.filter(|_| show) {
                inner.show(&n, Instant::now());
            }
            (added, show)
        };
        tracing::debug!("notification {:?}{}", added, if show { "" } else { " (no toast)" });
        if show {
            self.shared.wake();
        }
        changed(emitter.connection()).await;
        added.id()
    }

    async fn close_notification(&self, id: u32, #[zbus(signal_emitter)] emitter: SignalEmitter<'_>) {
        let removed = self.shared.lock().remove(id).is_some();
        if removed {
            let _ = Self::notification_closed(&emitter, id, Reason::Closed as u32).await;
            changed(emitter.connection()).await;
        }
    }

    fn get_server_information(&self) -> (String, String, String, String) {
        ("Yozora Notify".into(), "Yozora".into(), env!("CARGO_PKG_VERSION").into(), "1.2".into())
    }

    #[zbus(signal)]
    async fn notification_closed(emitter: &SignalEmitter<'_>, id: u32, reason: u32) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn action_invoked(emitter: &SignalEmitter<'_>, id: u32, action_key: &str) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn notification_replied(emitter: &SignalEmitter<'_>, id: u32, text: &str) -> zbus::Result<()>;
}

/// The notification center's interface, for the bar.
struct Center {
    shared: Shared,
}

#[interface(name = "dev.yozora.Notify1")]
impl Center {
    /// Everything in the center, newest first, with the same keys as mako's `ListHistory` plus
    /// `time`, `icon`, `image`, `pinned`, `snoozed-until` (0 unless snoozed), `unread` and
    /// `actions` (key, label, key, label… without the default action or inline reply).
    fn list(&self) -> Vec<HashMap<String, OwnedValue>> {
        let inner = self.shared.lock();
        inner
            .store
            .list()
            .map(|n| {
                let mut d = to_dict(n);
                if let Ok(v) = Value::from(inner.store.is_unread(n)).try_to_owned() {
                    d.insert("unread".into(), v);
                }
                d
            })
            .collect()
    }

    /// Do Not Disturb, and whether anything arrived since the center was last opened that's
    /// worth the bell's dot.
    fn get_state(&self) -> (bool, bool) {
        let inner = self.shared.lock();
        (inner.store.dnd, inner.unread())
    }

    /// Keeps a notification at the top of the center and through "Clear all", or stops.
    async fn pin(&self, id: u32, on: bool, #[zbus(signal_emitter)] emitter: SignalEmitter<'_>) {
        if self.shared.lock().store.pin(id, on) {
            let _ = Self::changed(&emitter).await;
        }
    }

    /// Hides a notification for `minutes`, then shows it again. 0 brings it back now.
    async fn snooze(&self, id: u32, minutes: u32, #[zbus(signal_emitter)] emitter: SignalEmitter<'_>) {
        let until = if minutes == 0 { 0 } else { now() + u64::from(minutes.min(7 * 24 * 60)) * 60 };
        {
            let mut inner = self.shared.lock();
            if !inner.store.snooze(id, until) {
                return;
            }
            inner.shown.remove(&id);
        }
        self.shared.wake();
        let _ = Self::changed(&emitter).await;
    }

    async fn set_do_not_disturb(&self, on: bool, #[zbus(signal_emitter)] emitter: SignalEmitter<'_>) {
        self.shared.lock().store.set_dnd(on);
        let _ = Self::changed(&emitter).await;
    }

    async fn dismiss(&self, id: u32) {
        let _ = self.shared.cmds.send(Cmd::Dismissed(id)).await;
    }

    async fn invoke_action(&self, id: u32, key: String) {
        let _ = self.shared.cmds.send(Cmd::Action(id, key)).await;
    }

    /// Sends `text` to the app as the answer to a notification that accepts inline replies.
    async fn reply(&self, id: u32, text: String) {
        if self.shared.lock().store.get(id).is_some_and(|n| n.reply_label().is_some()) {
            let _ = self.shared.cmds.send(Cmd::Reply(id, text)).await;
        }
    }

    async fn clear_all(&self, #[zbus(signal_emitter)] emitter: SignalEmitter<'_>) {
        // Pinned notifications stay, and so do their toasts.
        let ids = {
            let mut inner = self.shared.lock();
            let ids = inner.store.clear();
            for id in &ids {
                inner.shown.remove(id);
            }
            ids
        };
        let fdo = SignalEmitter::new(emitter.connection(), FDO_PATH);
        if let Ok(fdo) = fdo {
            for id in ids {
                let _ = Fdo::notification_closed(&fdo, id, Reason::Dismissed as u32).await;
            }
        }
        let _ = Self::changed(&emitter).await;
    }

    /// The center was opened. Also sends open toasts to it, except critical ones.
    async fn mark_read(&self, #[zbus(signal_emitter)] emitter: SignalEmitter<'_>) {
        self.shared.lock().store.mark_read();
        let _ = self.shared.cmds.send(Cmd::ExpireAll).await;
        let _ = Self::changed(&emitter).await;
    }

    /// Something in the center or Do Not Disturb changed. Read `List` and `GetState` again.
    #[zbus(signal)]
    async fn changed(emitter: &SignalEmitter<'_>) -> zbus::Result<()>;
}

async fn changed(conn: &Connection) {
    if let Ok(e) = SignalEmitter::new(conn, YZ_PATH) {
        let _ = Center::changed(&e).await;
    }
}

/// `n` for the bar.
fn to_dict(n: &Notification) -> HashMap<String, OwnedValue> {
    let mut d = HashMap::new();
    let mut put = |k: &str, v: Value<'_>| {
        if let Ok(v) = v.try_to_owned() {
            d.insert(k.to_string(), v);
        }
    };
    let path = |p: &Option<PathBuf>| p.as_ref().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
    put("id", n.id.into());
    put("app-name", n.app.as_str().into());
    put("app-icon", n.app_icon.as_str().into());
    put("summary", n.summary.as_str().into());
    put("body", n.body.as_str().into());
    put("desktop-entry", n.desktop_entry.as_str().into());
    put("category", n.category.as_str().into());
    put("urgency", n.urgency.byte().into());
    put("time", n.time.into());
    put("pinned", n.pinned.into());
    put("snoozed-until", n.snoozed_until.into());
    put("icon", path(&n.icon).into());
    put("image", path(&n.image).into());
    put("default-action", n.has_action("default").into());
    put("actions", n.buttons().flat_map(|(k, l)| [k.as_str(), l.as_str()]).collect::<Vec<_>>().into());
    d
}

/// Whether a new notification gets a toast. The center keeps it either way.
pub fn should_toast(n: &Notification, dnd: bool, config: &NotifyConfig) -> bool {
    match n.urgency {
        Urgency::Critical => true,
        _ if dnd => false,
        Urgency::Low => config.toasts.show_low,
        Urgency::Normal => true,
    }
}

/// How long a toast stays: the app's timeout, or the configured one (two thirds of it for low
/// urgency), or until closed for critical ones and apps that ask.
pub fn toast_timeout(n: &Notification, config: &NotifyConfig) -> Option<Duration> {
    if n.urgency == Urgency::Critical {
        return None;
    }
    let ms = match n.expire_ms {
        0 => return None,
        ms if ms > 0 => ms as u64,
        _ if n.urgency == Urgency::Low => u64::from(config.toasts.timeout_ms) * 2 / 3,
        _ => u64::from(config.toasts.timeout_ms),
    };
    Some(Duration::from_millis(ms))
}

fn hint<T: TryFrom<OwnedValue>>(hints: &HashMap<String, OwnedValue>, key: &str) -> Option<T> {
    T::try_from(hints.get(key)?.try_clone().ok()?).ok()
}

fn hint_bool(hints: &HashMap<String, OwnedValue>, key: &str) -> bool {
    hint::<bool>(hints, key).unwrap_or(false)
}

/// A notification from a `Notify` call, with its icon and image looked up.
pub fn build(
    app: String,
    app_icon: String,
    summary: String,
    body: String,
    actions: Vec<String>,
    hints: &HashMap<String, OwnedValue>,
    expire_ms: i32,
) -> Notification {
    let urgency = hint::<u8>(hints, "urgency").map(Urgency::from_byte).unwrap_or_default();
    let desktop_entry = hint::<String>(hints, "desktop-entry").unwrap_or_default();
    let progress = hint::<i32>(hints, "value")
        .or_else(|| hint::<u32>(hints, "value").map(|v| v.min(100) as i32))
        .map(|v| v.clamp(0, 100) as u8);
    let tag = hint::<String>(hints, "x-canonical-private-synchronous")
        .or_else(|| hint::<String>(hints, "x-dunst-stack-tag"))
        .filter(|t| !t.is_empty());
    let image = ["image-data", "image_data", "icon_data"]
        .iter()
        .find_map(|k| hints.get(*k))
        .and_then(image_from_pixels)
        .or_else(|| {
            ["image-path", "image_path"]
                .iter()
                .find_map(|k| hint::<String>(hints, k))
                .and_then(|p| icons::find(p.strip_prefix("file://").unwrap_or(&p), &[]))
        });
    // Wide pictures go under the body at the toast's width; the rest fill the 36 px icon.
    let preview = image
        .as_ref()
        .and_then(|p| image::image_dimensions(p).ok())
        .is_some_and(|(w, h)| w >= 256 && w as f32 >= h as f32 * 1.2);
    let image = image.map(|p| icons::fit(&p, if preview { 712 } else { 72 }));
    let actions = actions.chunks(2).filter(|c| c.len() == 2).map(|c| (c[0].clone(), c[1].clone())).collect();
    let icon = icons::find(app_icon.strip_prefix("file://").unwrap_or(&app_icon), &[])
        .or_else(|| icons::app_icon(&desktop_entry))
        .or_else(|| icons::app_icon(&app))
        // Drawn at 20 px. Symbolic icons are masks, which the renderer keeps sharp itself.
        .map(|p| if icons::is_symbolic(&p) { p } else { icons::fit(&p, 48) });
    Notification {
        id: 0,
        app,
        app_icon,
        summary: strip_markup(&summary),
        body: strip_markup(&body),
        actions,
        urgency,
        category: hint::<String>(hints, "category").unwrap_or_default(),
        desktop_entry,
        image,
        preview,
        icon,
        progress,
        resident: hint_bool(hints, "resident"),
        transient: hint_bool(hints, "transient"),
        tag,
        expire_ms,
        time: 0,
        ..Default::default()
    }
}

/// Where pixel images from apps are written, so the center can still show them later.
fn images_dir() -> PathBuf {
    std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
        .unwrap_or_else(std::env::temp_dir)
        .join("yozora")
        .join("notify")
}

/// Writes an `image-data` hint, `(iiibiiay)`, to a PNG. Large images are shrunk to 512 px.
fn image_from_pixels(v: &OwnedValue) -> Option<PathBuf> {
    let (w, h, stride, alpha, bits, channels, data) =
        <(i32, i32, i32, bool, i32, i32, Vec<u8>)>::try_from(v.try_clone().ok()?).ok()?;
    pixels_to_png(w, h, stride, alpha, bits, channels, &data, &images_dir())
}

#[allow(clippy::too_many_arguments)]
fn pixels_to_png(
    w: i32,
    h: i32,
    stride: i32,
    alpha: bool,
    bits: i32,
    channels: i32,
    data: &[u8],
    dir: &Path,
) -> Option<PathBuf> {
    use std::hash::{Hash, Hasher};
    let channels = channels as usize;
    if w <= 0 || h <= 0 || bits != 8 || channels != if alpha { 4 } else { 3 } || w > 4096 || h > 4096 {
        return None;
    }
    let (w, h, stride) = (w as usize, h as usize, stride as usize);
    if stride < w * channels || data.len() < stride * (h - 1) + w * channels {
        return None;
    }
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    (w, h, data).hash(&mut hasher);
    let out = dir.join(format!("{:016x}.png", hasher.finish()));
    if out.exists() {
        return Some(out);
    }
    let mut rgba = Vec::with_capacity(w * h * 4);
    for row in data.chunks(stride).take(h) {
        for px in row[..w * channels].chunks_exact(channels) {
            rgba.extend_from_slice(&px[..3]);
            rgba.push(if alpha { px[3] } else { 255 });
        }
    }
    let mut image = image::RgbaImage::from_raw(w as u32, h as u32, rgba)?;
    if w.max(h) > 512 {
        let scale = 512.0 / w.max(h) as f32;
        let (nw, nh) = (((w as f32) * scale).round().max(1.) as u32, ((h as f32) * scale).round().max(1.) as u32);
        image = image::imageops::resize(&image, nw, nh, image::imageops::FilterType::Lanczos3);
    }
    std::fs::create_dir_all(dir).ok()?;
    let tmp = out.with_extension("part");
    image.save_with_format(&tmp, image::ImageFormat::Png).ok()?;
    std::fs::rename(&tmp, &out).ok()?;
    Some(out)
}

/// Deletes saved images nothing in the center points to any more.
pub fn prune_images(store: &Store) {
    let dir = images_dir();
    let Ok(entries) = std::fs::read_dir(&dir) else { return };
    let used: std::collections::HashSet<PathBuf> = store.list().filter_map(|n| n.image.clone()).collect();
    for e in entries.flatten() {
        let p = e.path();
        if !used.contains(&p) {
            let _ = std::fs::remove_file(p);
        }
    }
}

/// Connects to the session bus and takes the notification name. Fails if another daemon has it
/// and won't give it up.
pub async fn connect(shared: Shared, replace: bool) -> anyhow::Result<Connection> {
    use zbus::fdo::{RequestNameFlags, RequestNameReply};
    let conn = zbus::connection::Builder::session()?
        .serve_at(FDO_PATH, Fdo { shared: shared.clone() })?
        .serve_at(YZ_PATH, Center { shared })?
        .build()
        .await
        .context("connecting to the session bus")?;
    let mut flags = RequestNameFlags::AllowReplacement | RequestNameFlags::DoNotQueue;
    if replace {
        flags |= RequestNameFlags::ReplaceExisting;
    }
    match conn.request_name_with_flags(NAME, flags).await? {
        RequestNameReply::PrimaryOwner | RequestNameReply::AlreadyOwner => Ok(conn),
        RequestNameReply::Exists | RequestNameReply::InQueue => {
            let owner = owner_name(&conn).await.unwrap_or_else(|| "another notification daemon".into());
            anyhow::bail!(
                "{owner} already provides notifications. Stop it first (for mako: `systemctl --user stop mako` or `pkill -x mako`), or pass --replace if it allows that."
            )
        }
    }
}

/// The process name of whoever owns the notification name, for error messages.
async fn owner_name(conn: &Connection) -> Option<String> {
    let dbus = fdo::DBusProxy::new(conn).await.ok()?;
    let name = zbus::names::BusName::try_from(NAME).ok()?;
    let pid = dbus.get_connection_unix_process_id(name).await.ok()?;
    let comm = std::fs::read_to_string(format!("/proc/{pid}/comm")).ok()?;
    Some(comm.trim().to_string())
}

/// Handles what the shell, the control socket and the clock ask for, until `quit` or another
/// daemon replaces this one.
pub async fn serve(conn: Connection, shared: Shared, cmds: Receiver<Cmd>) -> anyhow::Result<()> {
    enum Event {
        Cmd(Cmd),
        Timer,
        Done,
    }
    let fdo = SignalEmitter::new(&conn, FDO_PATH)?.into_owned();
    let dbus = fdo::DBusProxy::new(&conn).await?;
    let mut lost = dbus.receive_name_lost().await?;
    loop {
        let at = shared.lock().next_wake(Instant::now());
        let event = async { cmds.recv().await.map_or(Event::Done, Event::Cmd) }
            .or(async {
                while let Some(signal) = lost.next().await {
                    if signal.args().is_ok_and(|a| a.name() == NAME) {
                        tracing::info!("another notification daemon took over; quitting");
                        break;
                    }
                }
                Event::Done
            })
            .or(async {
                Timer::at(at).await;
                Event::Timer
            });
        let cmd = match event.await {
            Event::Cmd(cmd) => cmd,
            Event::Timer => {
                timer(&fdo, &shared).await;
                changed(&conn).await;
                continue;
            }
            Event::Done => return Ok(()),
        };
        match cmd {
            Cmd::Expired(id) => {
                let mut inner = shared.lock();
                inner.shown.remove(&id);
                inner.store.expired(id);
                drop(inner);
                let _ = Fdo::notification_closed(&fdo, id, Reason::Expired as u32).await;
            }
            Cmd::Dismissed(id) => {
                let removed = shared.lock().remove(id).is_some();
                if removed {
                    let _ = Fdo::notification_closed(&fdo, id, Reason::Dismissed as u32).await;
                }
            }
            Cmd::Closed(id) => {
                if shared.lock().remove(id).is_some() {
                    let _ = Fdo::notification_closed(&fdo, id, Reason::Closed as u32).await;
                }
            }
            Cmd::Action(id, key) => {
                let resident = shared.lock().store.get(id).map(|n| n.resident);
                let Some(resident) = resident else { continue };
                let _ = Fdo::action_invoked(&fdo, id, &key).await;
                if !resident {
                    shared.lock().remove(id);
                    let _ = Fdo::notification_closed(&fdo, id, Reason::Dismissed as u32).await;
                }
            }
            Cmd::Reply(id, text) => {
                let removed = shared.lock().remove(id).is_some();
                if removed {
                    let _ = Fdo::notification_replied(&fdo, id, &text).await;
                    let _ = Fdo::notification_closed(&fdo, id, Reason::Dismissed as u32).await;
                }
            }
            Cmd::Ipc(incoming) => {
                let response = ipc(&incoming.request, &shared);
                incoming.reply(response);
            }
            Cmd::Sharing(on) => {
                shared.lock().sharing = on;
                continue;
            }
            Cmd::ExpireAll => {
                let ids = shared.lock().take_expirable();
                for id in ids {
                    shared.lock().store.expired(id);
                    let _ = Fdo::notification_closed(&fdo, id, Reason::Expired as u32).await;
                }
            }
            Cmd::Config(config) => {
                let mut inner = shared.lock();
                inner.store.set_limit(config.history.limit as usize);
                inner.config = config;
                continue;
            }
            Cmd::Wake => continue,
            Cmd::Quit => return Ok(()),
        }
        changed(&conn).await;
    }
}

/// Times out what has been on screen long enough, and brings snoozed notifications back.
async fn timer(fdo: &SignalEmitter<'static>, shared: &Shared) {
    let (expired, due, dnd, config) = {
        let mut inner = shared.lock();
        let expired = inner.timed_out(Instant::now());
        for id in &expired {
            inner.store.expired(*id);
        }
        let due = inner.store.due(now());
        // Drop screen entries for notifications that history limits pushed out.
        let Inner { store, shown, .. } = &mut *inner;
        shown.retain(|id, _| store.get(*id).is_some());
        (expired, due, inner.store.dnd, inner.config.clone())
    };
    for id in expired {
        let _ = Fdo::notification_closed(fdo, id, Reason::Expired as u32).await;
    }
    for n in due {
        // Back in the center either way; on screen only if a toast would show now.
        if should_toast(&n, dnd, &config) {
            tracing::debug!("showing notification {} again", n.id);
            shared.lock().show(&n, Instant::now());
        }
    }
}

fn ipc(request: &yozora_ipc::Request, shared: &Shared) -> Response {
    let arg = request.args.first().map(String::as_str);
    match request.cmd.as_str() {
        // Settings uses this to tell whether the server is running.
        "ping" => Response::data(json!({ "version": env!("CARGO_PKG_VERSION"), "warnings": [] })),
        "dnd" => {
            let mut inner = shared.lock();
            let on = match arg {
                Some("on") => true,
                Some("off") => false,
                Some("toggle") | None => !inner.store.dnd,
                Some(other) => return Response::error(format!("dnd takes on, off or toggle, not {other:?}")),
            };
            inner.store.set_dnd(on);
            Response::data(json!({ "dnd": on }))
        }
        "status" => {
            let inner = shared.lock();
            Response::data(json!({
                "dnd": inner.store.dnd,
                "unread": inner.unread(),
                "sharing": inner.sharing,
                "count": inner.store.list().count(),
            }))
        }
        "list" => {
            let inner = shared.lock();
            Response::data(json!(inner.store.list().map(to_json).collect::<Vec<_>>()))
        }
        "get" => {
            let inner = shared.lock();
            match id(arg).map(|id| inner.store.get(id)) {
                Ok(Some(n)) => Response::data(to_json(n)),
                Ok(None) => Response::error(format!("no notification {}", arg.unwrap_or_default())),
                Err(e) => e,
            }
        }
        "dismiss" => {
            if request.args.is_empty() {
                return Response::error("dismiss needs one or more ids");
            }
            let mut closed = Vec::new();
            for a in &request.args {
                let id = match id(Some(a)) {
                    Ok(id) => id,
                    Err(e) => return e,
                };
                if shared.lock().store.get(id).is_some() {
                    let _ = shared.cmds.try_send(Cmd::Closed(id));
                    closed.push(id);
                }
            }
            Response::data(json!({ "dismissed": closed }))
        }
        "pin" => {
            let id = match id(arg) {
                Ok(id) => id,
                Err(e) => return e,
            };
            let on = match request.args.get(1).map(String::as_str) {
                None | Some("on") => true,
                Some("off") => false,
                Some(other) => return Response::error(format!("pin takes on or off, not {other:?}")),
            };
            if shared.lock().store.pin(id, on) {
                Response::data(json!({ "id": id, "pinned": on }))
            } else {
                Response::error(format!("no notification {id}"))
            }
        }
        "snooze" => {
            let id = match id(arg) {
                Ok(id) => id,
                Err(e) => return e,
            };
            let Some(minutes) = request.args.get(1).and_then(|m| m.parse::<u32>().ok()).filter(|m| *m > 0) else {
                return Response::error("snooze needs an id and a number of minutes");
            };
            let minutes = minutes.min(7 * 24 * 60);
            let until = now() + u64::from(minutes) * 60;
            let snoozed = {
                let mut inner = shared.lock();
                let snoozed = inner.store.snooze(id, until);
                inner.shown.remove(&id);
                snoozed
            };
            if snoozed {
                shared.wake();
                Response::data(json!({ "id": id, "until": until }))
            } else {
                Response::error(format!("no notification {id}"))
            }
        }
        "action" => {
            let id = match id(arg) {
                Ok(id) => id,
                Err(e) => return e,
            };
            let key = request.args.get(1).cloned().unwrap_or_else(|| "default".into());
            let known = shared.lock().store.get(id).map(|n| n.has_action(&key) && key != "inline-reply");
            match known {
                Some(true) => {
                    let _ = shared.cmds.try_send(Cmd::Action(id, key.clone()));
                    Response::data(json!({ "id": id, "action": key }))
                }
                Some(false) => Response::error(format!("notification {id} has no action {key:?}")),
                None => Response::error(format!("no notification {id}")),
            }
        }
        "reply" => {
            let id = match id(arg) {
                Ok(id) => id,
                Err(e) => return e,
            };
            let Some(text) = request.args.get(1).filter(|t| !t.is_empty()) else {
                return Response::error("reply needs an id and some text");
            };
            let known = shared.lock().store.get(id).map(|n| n.reply_label().is_some());
            match known {
                Some(true) => {
                    let _ = shared.cmds.try_send(Cmd::Reply(id, text.clone()));
                    Response::data(json!({ "id": id, "replied": true }))
                }
                Some(false) => Response::error(format!("notification {id} does not take replies")),
                None => Response::error(format!("no notification {id}")),
            }
        }
        "clear" => {
            // Through the command loop, so apps hear their notifications closed.
            let ids: Vec<u32> = shared.lock().store.list().filter(|n| !n.pinned).map(|n| n.id).collect();
            for id in &ids {
                let _ = shared.cmds.try_send(Cmd::Dismissed(*id));
            }
            Response::data(json!({ "cleared": ids }))
        }
        "quit" => {
            let _ = shared.cmds.try_send(Cmd::Quit);
            Response::ok()
        }
        other => Response::error(format!(
            "unknown command {other:?}; try dnd, status, list, get, dismiss, pin, snooze, action, reply, clear or quit"
        )),
    }
}

fn id(arg: Option<&str>) -> Result<u32, Response> {
    arg.and_then(|a| a.parse().ok()).ok_or_else(|| Response::error("needs a notification id, a number"))
}

/// `n` for the control socket.
fn to_json(n: &Notification) -> serde_json::Value {
    json!({
        "id": n.id,
        "app": n.app,
        "summary": n.summary,
        "body": n.body,
        "urgency": n.urgency,
        "time": n.time,
        "actions": n.actions.iter().filter(|(k, _)| k != "inline-reply")
            .map(|(k, l)| json!({ "key": k, "label": l })).collect::<Vec<_>>(),
        "pinned": n.pinned,
        "snoozed_until": n.snoozed_until,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pixel_images_become_pngs() {
        let dir = std::env::temp_dir().join(format!("yozora-notify-px-{}", std::process::id()));
        // 2×2 RGB with padding at the end of each row.
        let data = [255, 0, 0, 0, 255, 0, 9, 9, 0, 0, 255, 255, 255, 255, 9, 9];
        let out = pixels_to_png(2, 2, 8, false, 8, 3, &data, &dir).expect("a png");
        let png = image::open(&out).unwrap().into_rgba8();
        assert_eq!(png.get_pixel(1, 0).0, [0, 255, 0, 255]);
        assert_eq!(png.get_pixel(0, 1).0, [0, 0, 255, 255]);
        // Bad shapes are refused rather than read out of bounds.
        assert_eq!(pixels_to_png(4, 4, 16, true, 8, 4, &data, &dir), None);
        assert_eq!(pixels_to_png(2, 2, 8, true, 8, 3, &data, &dir), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn urgency_and_dnd_decide_toasts() {
        let config = NotifyConfig::default();
        let n = |u| Notification { urgency: u, ..Default::default() };
        assert!(should_toast(&n(Urgency::Normal), false, &config));
        assert!(!should_toast(&n(Urgency::Normal), true, &config));
        assert!(!should_toast(&n(Urgency::Low), false, &config));
        assert!(should_toast(&n(Urgency::Critical), true, &config));
    }

    #[test]
    fn timeouts_follow_urgency() {
        let config = NotifyConfig::default();
        let n = |urgency, expire_ms| Notification { urgency, expire_ms, ..Default::default() };
        let normal = Duration::from_millis(u64::from(config.toasts.timeout_ms));
        assert_eq!(toast_timeout(&n(Urgency::Normal, -1), &config), Some(normal));
        assert_eq!(toast_timeout(&n(Urgency::Low, -1), &config), Some(normal * 2 / 3));
        assert_eq!(toast_timeout(&n(Urgency::Normal, 2500), &config), Some(Duration::from_millis(2500)));
        assert_eq!(toast_timeout(&n(Urgency::Normal, 0), &config), None);
        assert_eq!(toast_timeout(&n(Urgency::Critical, 9000), &config), None);
    }

    #[test]
    fn screen_entries_time_out_and_expire() {
        let mut inner = Inner::new(Store::empty(10), NotifyConfig::default());
        let add = |inner: &mut Inner, urgency, expire_ms| {
            let n = Notification { urgency, expire_ms, ..Default::default() };
            let id = inner.store.add(n, 0).id();
            let n = inner.store.get(id).cloned().unwrap();
            inner.show(&n, Instant::now());
            id
        };
        let short = add(&mut inner, Urgency::Normal, 100);
        let long = add(&mut inner, Urgency::Normal, 60_000);
        let sticky = add(&mut inner, Urgency::Critical, -1);
        assert!(inner.next_wake(Instant::now()) <= Instant::now() + Duration::from_millis(100));
        assert_eq!(inner.timed_out(Instant::now()), Vec::<u32>::new());
        assert_eq!(inner.timed_out(Instant::now() + Duration::from_secs(1)), [short]);
        // Opening the center finishes everything but critical notifications.
        assert_eq!(inner.take_expirable(), [long]);
        assert!(inner.shown.contains_key(&sticky));
        assert!(inner.remove(sticky).is_some());
        assert!(inner.shown.is_empty());
    }

    #[test]
    fn hints_are_read() {
        let mut hints = HashMap::new();
        hints.insert("urgency".to_string(), OwnedValue::from(2u8));
        hints.insert("value".to_string(), OwnedValue::from(140i32));
        hints.insert("transient".to_string(), OwnedValue::from(true));
        hints.insert("x-canonical-private-synchronous".to_string(), Value::from("volume").try_to_owned().unwrap());
        let actions = ["default", "Open", "reply", "Reply", "odd"].map(String::from).to_vec();
        let n = build("App".into(), String::new(), "<b>Hi</b>".into(), String::new(), actions, &hints, -1);
        assert_eq!(n.urgency, Urgency::Critical);
        assert_eq!(n.progress, Some(100));
        assert!(n.transient);
        assert_eq!(n.tag.as_deref(), Some("volume"));
        assert_eq!(n.summary, "Hi");
        assert_eq!(n.buttons().map(|(k, _)| k.as_str()).collect::<Vec<_>>(), ["reply"]);
        assert!(n.has_action("default"));
    }
}
