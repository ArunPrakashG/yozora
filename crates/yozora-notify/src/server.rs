//! The D-Bus side: `org.freedesktop.Notifications` for apps, and `dev.yozora.Notify1` for the
//! bar's notification center.
//!
//! zbus answers method calls on its own thread. The store sits behind a mutex that is never held
//! across an await. Toasts live on the GPUI thread: the server tells them what to show with
//! [`UiEvent`], and they report back what the user did with [`Cmd`].

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use anyhow::Context as _;
use async_channel::{Receiver, Sender};
use futures_lite::{FutureExt, StreamExt};
use serde_json::json;
use yozora_config::notify::{JevSection, NotifyConfig};
use yozora_ipc::{Incoming, Response};
use yozora_ui::icon_theme as icons;
use zbus::{
    Connection, fdo, interface,
    object_server::SignalEmitter,
    zvariant::{OwnedValue, Value},
};

use crate::{
    jev,
    store::{Added, Notification, Store, Urgency, strip_markup},
};

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

/// What toasts should do.
#[derive(Clone, Debug)]
pub enum UiEvent {
    /// Show a toast, or update the one with this id in place.
    Show(Box<Notification>),
    Close(u32),
    CloseAll,
    /// The center opened: toasts that can time out go to it now.
    ExpireAll,
    /// It can wait: the toast leaves soon.
    Shorten(u32),
    Quit,
}

/// What the user did, from toasts, the bar or the control socket.
pub enum Cmd {
    Expired(u32),
    Dismissed(u32),
    Action(u32, String),
    Reply(u32, String),
    Ipc(Incoming),
    /// Jev's odds for a notification, and the fingerprint of the content they're for.
    Verdict(u32, u64, f64),
}

pub struct Inner {
    pub store: Store,
    pub config: NotifyConfig,
}

#[derive(Clone)]
pub struct Shared {
    pub inner: Arc<Mutex<Inner>>,
    pub ui: Sender<UiEvent>,
    pub cmds: Sender<Cmd>,
}

impl Shared {
    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn ui(&self, event: UiEvent) {
        let _ = self.ui.try_send(event);
    }

    /// Asks Jev about notification `id` if it's on and the notification may be sent.
    fn ask_jev(&self, id: u32) {
        let (n, cfg) = {
            let inner = self.lock();
            let Some(n) = inner.store.get(id).filter(|n| jev::eligible(n, &inner.config.jev)).cloned() else {
                return;
            };
            (n, inner.config.jev.clone())
        };
        let cmds = self.cmds.clone();
        jev::ask(&n, &cfg, move |print, odds| {
            let _ = cmds.try_send(Cmd::Verdict(id, print, odds));
        });
    }
}

/// What Jev's answer does to the toasts.
#[derive(Debug, PartialEq)]
pub enum Effect {
    Nothing,
    /// It can wait: its toast leaves early.
    Shorten,
    /// Do Not Disturb hid it, but it can't wait.
    Show,
}

/// Whether the notification can wait, and what that does.
pub fn verdict(odds: f64, dnd: bool, cfg: &JevSection) -> (bool, Effect) {
    let quiet = odds < cfg.quiet_below;
    let effect = match (dnd, quiet) {
        (true, _) if odds >= cfg.breakthrough => Effect::Show,
        (false, true) => Effect::Shorten,
        _ => Effect::Nothing,
    };
    (quiet, effect)
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
            (added, show.then_some(stored).flatten())
        };
        tracing::debug!("notification {:?}{}", added, if show.is_some() { "" } else { " (no toast)" });
        self.shared.ask_jev(added.id());
        if let Some(n) = show {
            self.shared.ui(UiEvent::Show(Box::new(n)));
        } else if let Added::Replaced(id) = added {
            // An update that no longer toasts (Do Not Disturb came on) closes the old toast.
            self.shared.ui(UiEvent::Close(id));
        }
        changed(emitter.connection()).await;
        added.id()
    }

    async fn close_notification(&self, id: u32, #[zbus(signal_emitter)] emitter: SignalEmitter<'_>) {
        let removed = self.shared.lock().store.remove(id).is_some();
        if removed {
            self.shared.ui(UiEvent::Close(id));
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
    /// `time`, `icon` and `image`.
    fn list(&self) -> Vec<HashMap<String, OwnedValue>> {
        self.shared.lock().store.list().map(to_dict).collect()
    }

    /// Do Not Disturb, and whether anything arrived since the center was last opened.
    fn get_state(&self) -> (bool, bool) {
        let inner = self.shared.lock();
        (inner.store.dnd, inner.store.unread())
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

    async fn clear_all(&self, #[zbus(signal_emitter)] emitter: SignalEmitter<'_>) {
        let ids: Vec<u32> = {
            let mut inner = self.shared.lock();
            let ids = inner.store.list().map(|n| n.id).collect();
            inner.store.clear();
            ids
        };
        self.shared.ui(UiEvent::CloseAll);
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
        self.shared.ui(UiEvent::ExpireAll);
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
    put("attention", n.attention.unwrap_or(-1.0).into());
    put("quiet", n.quiet.into());
    put("icon", path(&n.icon).into());
    put("image", path(&n.image).into());
    put("default-action", n.has_action("default").into());
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
        // Drawn at 20 px. Symbolic icons are masks, which GPUI keeps sharp itself.
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
        attention: None,
        quiet: false,
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

/// Handles what toasts, the bar and the control socket ask for, until the app quits. Also quits
/// the app if another daemon replaces this one.
pub async fn serve(conn: Connection, shared: Shared, cmds: Receiver<Cmd>) -> anyhow::Result<()> {
    let fdo = SignalEmitter::new(&conn, FDO_PATH)?.into_owned();
    let dbus = fdo::DBusProxy::new(&conn).await?;
    let mut lost = dbus.receive_name_lost().await?;
    loop {
        let next = async { cmds.recv().await.ok() }.or(async {
            while let Some(signal) = lost.next().await {
                if signal.args().is_ok_and(|a| a.name() == NAME) {
                    tracing::info!("another notification daemon took over; quitting");
                    return None;
                }
            }
            None
        });
        let Some(cmd) = next.await else {
            shared.ui(UiEvent::Quit);
            return Ok(());
        };
        match cmd {
            Cmd::Expired(id) => {
                shared.lock().store.expired(id);
                let _ = Fdo::notification_closed(&fdo, id, Reason::Expired as u32).await;
            }
            Cmd::Dismissed(id) => {
                if shared.lock().store.remove(id).is_some() {
                    shared.ui(UiEvent::Close(id));
                    let _ = Fdo::notification_closed(&fdo, id, Reason::Dismissed as u32).await;
                }
            }
            Cmd::Action(id, key) => {
                let resident = shared.lock().store.get(id).map(|n| n.resident);
                let Some(resident) = resident else { continue };
                let _ = Fdo::action_invoked(&fdo, id, &key).await;
                if !resident {
                    shared.lock().store.remove(id);
                    shared.ui(UiEvent::Close(id));
                    let _ = Fdo::notification_closed(&fdo, id, Reason::Dismissed as u32).await;
                }
            }
            Cmd::Reply(id, text) => {
                if shared.lock().store.remove(id).is_some() {
                    let _ = Fdo::notification_replied(&fdo, id, &text).await;
                    shared.ui(UiEvent::Close(id));
                    let _ = Fdo::notification_closed(&fdo, id, Reason::Dismissed as u32).await;
                }
            }
            Cmd::Ipc(incoming) => {
                let response = ipc(&incoming.request, &shared);
                incoming.reply(response);
            }
            Cmd::Verdict(id, print, odds) => {
                let (n, effect) = {
                    let mut inner = shared.lock();
                    let send_body = inner.config.jev.send_body;
                    let (quiet, effect) = verdict(odds, inner.store.dnd, &inner.config.jev);
                    let n = inner.store.set_attention(id, odds, quiet, |n| jev::fingerprint(n, send_body) == print);
                    (n, effect)
                };
                let Some(n) = n else { continue };
                tracing::debug!("jev: notification {id} needs attention at odds {odds:.2}: {effect:?}");
                match effect {
                    Effect::Show if n.urgency != Urgency::Critical => shared.ui(UiEvent::Show(Box::new(n))),
                    Effect::Shorten => shared.ui(UiEvent::Shorten(id)),
                    _ => {}
                }
            }
        }
        changed(&conn).await;
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
                "unread": inner.store.unread(),
                "count": inner.store.list().count(),
            }))
        }
        "list" => {
            let inner = shared.lock();
            let items: Vec<_> = inner
                .store
                .list()
                .map(|n| {
                    json!({
                        "id": n.id,
                        "app": n.app,
                        "summary": n.summary,
                        "body": n.body,
                        "urgency": n.urgency,
                        "time": n.time,
                        "attention": n.attention,
                        "quiet": n.quiet,
                    })
                })
                .collect();
            Response::data(json!(items))
        }
        "clear" => {
            shared.lock().store.clear();
            shared.ui(UiEvent::CloseAll);
            Response::ok()
        }
        "quit" => {
            shared.ui(UiEvent::Quit);
            Response::ok()
        }
        other => Response::error(format!("unknown command {other:?}; try dnd, status, list, clear or quit")),
    }
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
    fn jev_answers_decide() {
        let cfg = JevSection::default();
        assert_eq!(verdict(0.1, false, &cfg), (true, Effect::Shorten));
        assert_eq!(verdict(0.5, false, &cfg), (false, Effect::Nothing));
        assert_eq!(verdict(0.95, false, &cfg), (false, Effect::Nothing));
        // Do Not Disturb: only what can't wait shows.
        assert_eq!(verdict(0.95, true, &cfg), (false, Effect::Show));
        assert_eq!(verdict(0.5, true, &cfg), (false, Effect::Nothing));
        assert_eq!(verdict(0.1, true, &cfg), (true, Effect::Nothing));
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
