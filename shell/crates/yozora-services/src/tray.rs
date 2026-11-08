//! System tray: a StatusNotifierItem host with DBusMenu menus.
//!
//! Another bar may already run the StatusNotifierWatcher; then we register with it as one more
//! host. If nobody does, we become the watcher ourselves.

use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};

use zbus::{
    Connection, MatchRule,
    object_server::SignalEmitter,
    zvariant::{OwnedValue, Value},
};

use super::{
    Link, Tray,
    dbus::{self, Props, Wake},
};

const WATCHER: &str = "org.kde.StatusNotifierWatcher";
const WATCHER_PATH: &str = "/StatusNotifierWatcher";
const ITEM: &str = "org.kde.StatusNotifierItem";
const MENU: &str = "com.canonical.dbusmenu";

/// Straight RGBA pixels.
#[derive(Clone)]
pub struct Pixmap {
    pub width: u32,
    pub height: u32,
    pub rgba: Arc<Vec<u8>>,
}

#[derive(Clone)]
pub enum Icon {
    /// One of the bar's own icons, for tray items whose icon names a common thing.
    /// The name of a glyph in the shell's icon set.
    Builtin(&'static str),
    /// A single-color SVG, drawn tinted like the bar's own icons.
    Symbolic(PathBuf),
    File(PathBuf),
    Pixmap(Pixmap),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Status {
    #[default]
    Active,
    Passive,
    NeedsAttention,
}

#[derive(Clone)]
pub struct Item {
    /// The watcher's id for it: "bus/path".
    pub key: String,
    pub title: String,
    pub status: Status,
    pub icon: Option<Icon>,
    pub has_menu: bool,
    /// Left click should open the menu instead of activating.
    pub item_is_menu: bool,
    dest: String,
    path: String,
    menu: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Toggle {
    None,
    Check(bool),
    Radio(bool),
}

#[derive(Clone, Debug, PartialEq)]
pub struct MenuItem {
    pub id: i32,
    pub label: String,
    pub enabled: bool,
    pub separator: bool,
    pub toggle: Toggle,
    pub children: Vec<MenuItem>,
}

#[derive(Clone, Default)]
pub struct State {
    pub items: Vec<Item>,
    /// The menu last asked for, keyed by item.
    pub menu: Option<(String, Vec<MenuItem>)>,
}

#[derive(Clone, Debug)]
pub enum Cmd {
    Activate(String),
    SecondaryActivate(String),
    Scroll(String, i32),
    LoadMenu(String),
    MenuClick(String, i32),
}

pub fn start(fake: bool) -> Tray {
    if fake {
        super::spawn("tray", true, run_fake)
    } else {
        super::spawn("tray", false, |link| super::supervise(link, run))
    }
}

/// Two items with menus that exercise submenus, check and radio marks, separators and a
/// disabled row. Clicking a mark flips it, a plain row does nothing.
fn run_fake(link: Link<State, Cmd>) {
    let row = |id: i32, label: &str, toggle: Toggle| MenuItem { id, label: label.into(), enabled: true, separator: false, toggle, children: Vec::new() };
    let sep = |id: i32| MenuItem { id, label: String::new(), enabled: true, separator: true, toggle: Toggle::None, children: Vec::new() };
    let item = |key: &str, title: &str, icon: &'static str| Item {
        key: key.into(),
        title: title.into(),
        status: Status::Active,
        icon: Some(Icon::Builtin(icon)),
        has_menu: true,
        item_is_menu: true,
        dest: key.into(),
        path: "/StatusNotifierItem".into(),
        menu: Some("/Menu".into()),
    };
    let mut status = row(3, "Status", Toggle::None);
    status.children = vec![row(31, "Online", Toggle::Radio(true)), row(32, "Away", Toggle::Radio(false)), row(33, "Invisible", Toggle::Radio(false))];
    let mut more = row(6, "More", Toggle::None);
    more.children = vec![row(61, "Preferences", Toggle::None), row(62, "Check for updates", Toggle::None)];
    let mut disabled = row(5, "Sync paused", Toggle::None);
    disabled.enabled = false;
    let mut menus = HashMap::from([
        ("fake.chat".to_string(), vec![row(1, "Open chat", Toggle::None), row(2, "Notifications", Toggle::Check(true)), status, sep(4), disabled, more, sep(7), row(8, "Quit", Toggle::None)]),
        ("fake.sync".to_string(), vec![row(1, "Sync now", Toggle::None), row(2, "Pause syncing", Toggle::Check(false))]),
    ]);
    let mut state = State { items: vec![item("fake.chat", "Chat", "bt"), item("fake.sync", "Sync", "wifi")], menu: None };
    link.publish(state.clone());
    while let Ok(cmd) = link.cmds.recv_blocking() {
        match cmd {
            Cmd::LoadMenu(key) => {
                if let Some(m) = menus.get(&key) {
                    state.menu = Some((key, m.clone()));
                }
            }
            Cmd::MenuClick(key, id) => {
                if let Some(m) = menus.get_mut(&key) {
                    flip(m, id);
                    state.menu = Some((key, m.clone()));
                }
            }
            _ => {}
        }
        if !link.publish(state.clone()) {
            break;
        }
    }
}

/// A click on a check row flips it; on a radio row it selects it and clears its siblings.
fn flip(items: &mut [MenuItem], id: i32) -> bool {
    if let Some(i) = items.iter().position(|m| m.id == id) {
        match items[i].toggle {
            Toggle::Check(on) => items[i].toggle = Toggle::Check(!on),
            Toggle::Radio(_) => {
                for m in items.iter_mut().filter(|m| matches!(m.toggle, Toggle::Radio(_))) {
                    m.toggle = Toggle::Radio(false);
                }
                items[i].toggle = Toggle::Radio(true);
            }
            Toggle::None => {}
        }
        return true;
    }
    items.iter_mut().any(|m| flip(&mut m.children, id))
}

/// Splits a watcher id into bus name and object path.
fn split(key: &str) -> (String, String) {
    match key.find('/') {
        Some(0) | None => (key.to_string(), "/StatusNotifierItem".into()),
        Some(i) => (key[..i].to_string(), key[i..].to_string()),
    }
}

/// The bar's own icon for a theme icon name that means the same thing, so that tray items
/// like nm-applet and fcitx match the modules beside them. Theme icons for these vary a lot:
/// hicolor's wired icon, for one, is drawn at 35% opacity.
fn builtin(name: &str) -> Option<&'static str> {
    let name = name.strip_suffix("-symbolic").unwrap_or(name);
    let icon = match name {
        "network-wireless-offline" | "network-wireless-disconnected" | "nm-signal-00" => "wifi2",
        "nm-signal-25" | "network-wireless-signal-weak" | "network-wireless-signal-none" => "wifi2",
        "nm-signal-50" | "network-wireless-signal-ok" => "wifi2",
        n if n.starts_with("nm-signal") || n.starts_with("network-wireless") => "wifi",
        "input-keyboard" | "fcitx" | "fcitx-kbd" | "input-keyboard-us" => "kbd",
        "bluetooth" | "bluetooth-active" | "blueman" | "blueman-tray" | "blueman-active" => "bt",
        _ => return None,
    };
    Some(icon)
}

/// ARGB32 pixmaps in network byte order, to straight RGBA.
fn pixmap(props: &Props) -> Option<Pixmap> {
    let list: Vec<(i32, i32, Vec<u8>)> = dbus::get(props, "IconPixmap")?;
    let (w, h, data) = list
        .into_iter()
        .filter(|(w, h, d)| *w > 0 && *h > 0 && d.len() == (*w * *h * 4) as usize)
        .min_by_key(|(w, _, _)| if *w >= 32 { *w - 32 } else { (32 - *w) * 4 })?;
    let mut rgba = Vec::with_capacity(data.len());
    for px in data.as_chunks::<4>().0 {
        let (a, r, g, b) = (px[0], px[1], px[2], px[3]);
        rgba.extend_from_slice(&[r, g, b, a]);
    }
    let mut buffer = image::RgbaImage::from_raw(w as u32, h as u32, rgba)?;
    if w > 32 || h > 32 {
        // The GPU would shrink it with a plain bilinear sample, which aliases past 2x.
        let (w, h) = ((w * 32 / w.max(h)).max(1) as u32, (h * 32 / w.max(h)).max(1) as u32);
        buffer = image::imageops::resize(&buffer, w, h, image::imageops::FilterType::Lanczos3);
    }
    Some(Pixmap { width: buffer.width(), height: buffer.height(), rgba: Arc::new(buffer.into_raw()) })
}

async fn item(conn: &Connection, key: &str) -> anyhow::Result<Item> {
    let (dest, path) = split(key);
    let p = dbus::get_all(conn, &dest, &path, ITEM).await?;
    let status = match dbus::string(&p, "Status").as_str() {
        "Passive" => Status::Passive,
        "NeedsAttention" => Status::NeedsAttention,
        _ => Status::Active,
    };
    let theme_path = dbus::string(&p, "IconThemePath");
    let extra: Vec<PathBuf> = (!theme_path.is_empty()).then(|| PathBuf::from(&theme_path)).into_iter().collect();
    let name_key = if status == Status::NeedsAttention { "AttentionIconName" } else { "IconName" };
    let name = dbus::string(&p, name_key);
    let name = if name.is_empty() { dbus::string(&p, "IconName") } else { name };
    // Drawn at 16 px; 32 covers a 2x monitor.
    let icon = builtin(&name)
        .map(Icon::Builtin)
        .or_else(|| {
            super::icons::find_symbolic(&name, &extra).map(|p| {
                if super::icons::is_symbolic(&p) { Icon::Symbolic(p) } else { Icon::File(super::icons::fit(&p, 32)) }
            })
        })
        .or_else(|| pixmap(&p).map(Icon::Pixmap));
    let title =
        dbus::get::<String>(&p, "Title").filter(|t| !t.is_empty()).or_else(|| dbus::get(&p, "Id")).unwrap_or_default();
    let menu = dbus::path(&p, "Menu");
    Ok(Item {
        key: key.to_string(),
        title,
        status,
        icon,
        has_menu: menu.is_some(),
        item_is_menu: dbus::get(&p, "ItemIsMenu").unwrap_or(false),
        dest,
        path,
        menu,
    })
}

fn menu_item(id: i32, props: &HashMap<String, OwnedValue>, children: Vec<OwnedValue>) -> Option<MenuItem> {
    if !dbus::get::<bool>(props, "visible").unwrap_or(true) {
        return None;
    }
    let label = dbus::string(props, "label");
    // Mnemonics: "_File" shows as "File"; "__" is a literal underscore.
    let label = label.replace("__", "\u{0}").replace('_', "").replace('\u{0}', "_");
    let state: i32 = dbus::get(props, "toggle-state").unwrap_or(0);
    let toggle = match dbus::string(props, "toggle-type").as_str() {
        "checkmark" => Toggle::Check(state == 1),
        "radio" => Toggle::Radio(state == 1),
        _ => Toggle::None,
    };
    Some(MenuItem {
        id,
        label,
        enabled: dbus::get(props, "enabled").unwrap_or(true),
        separator: dbus::string(props, "type") == "separator",
        toggle,
        children: children.into_iter().filter_map(parse_child).collect(),
    })
}

fn parse_child(v: OwnedValue) -> Option<MenuItem> {
    let (id, props, children) = <(i32, HashMap<String, OwnedValue>, Vec<OwnedValue>)>::try_from(v).ok()?;
    menu_item(id, &props, children)
}

async fn load_menu(conn: &Connection, item: &Item) -> anyhow::Result<Vec<MenuItem>> {
    let path = item.menu.as_deref().ok_or_else(|| anyhow::anyhow!("item has no menu"))?;
    // Some apps fill their menu lazily on AboutToShow; failures here are harmless.
    let _ = dbus::invoke(conn, &item.dest, path, MENU, "AboutToShow", &(0i32,)).await;
    type Layout = (u32, (i32, HashMap<String, OwnedValue>, Vec<OwnedValue>));
    let (_rev, (id, props, children)): Layout =
        dbus::call(conn, &item.dest, path, MENU, "GetLayout", &(0i32, -1i32, Vec::<String>::new())).await?;
    Ok(menu_item(id, &props, children).map(|root| root.children).unwrap_or_default())
}

/// Our own watcher, for when no other process provides one.
struct Watcher {
    items: Arc<Mutex<Vec<String>>>,
}

#[zbus::interface(name = "org.kde.StatusNotifierWatcher")]
impl Watcher {
    async fn register_status_notifier_item(
        &self,
        service: &str,
        #[zbus(header)] header: zbus::message::Header<'_>,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) {
        let sender = header.sender().map(|s| s.to_string()).unwrap_or_default();
        let key = if service.starts_with('/') { format!("{sender}{service}") } else { service.to_string() };
        {
            let mut items = self.items.lock().unwrap_or_else(|e| e.into_inner());
            if items.contains(&key) {
                return;
            }
            items.push(key.clone());
        }
        let _ = Self::status_notifier_item_registered(&emitter, &key).await;
    }

    async fn register_status_notifier_host(&self, _service: &str) {}

    #[zbus(property)]
    fn registered_status_notifier_items(&self) -> Vec<String> {
        self.items.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    #[zbus(property)]
    fn is_status_notifier_host_registered(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn protocol_version(&self) -> i32 {
        0
    }

    #[zbus(signal)]
    async fn status_notifier_item_registered(emitter: &SignalEmitter<'_>, service: &str) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn status_notifier_item_unregistered(emitter: &SignalEmitter<'_>, service: &str) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn status_notifier_host_registered(emitter: &SignalEmitter<'_>) -> zbus::Result<()>;
}

async fn registered(conn: &Connection, own: &Option<Arc<Mutex<Vec<String>>>>) -> Vec<String> {
    if let Some(items) = own {
        return items.lock().unwrap_or_else(|e| e.into_inner()).clone();
    }
    match dbus::get_all(conn, WATCHER, WATCHER_PATH, WATCHER).await {
        Ok(p) => dbus::get(&p, "RegisteredStatusNotifierItems").unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

async fn run(link: &Link<State, Cmd>) -> anyhow::Result<()> {
    let conn = Connection::session().await?;
    let host = format!("org.kde.StatusNotifierHost-{}", std::process::id());
    // Set up the object server before taking a name, so calls that arrive early get an
    // answer instead of being dropped.
    let _ = conn.object_server();
    conn.request_name(host.as_str()).await?;

    let mut own = None;
    if !dbus::name_has_owner(&conn, WATCHER).await {
        let items = Arc::new(Mutex::new(Vec::new()));
        conn.object_server().at(WATCHER_PATH, Watcher { items: items.clone() }).await?;
        conn.request_name(WATCHER).await?;
        tracing::info!("no StatusNotifierWatcher running; providing one");
        own = Some(items);
    } else {
        dbus::invoke(&conn, WATCHER, WATCHER_PATH, WATCHER, "RegisterStatusNotifierHost", &(host.as_str(),)).await?;
    }

    let rule = |iface: &'static str| -> anyhow::Result<MatchRule<'static>> {
        Ok(MatchRule::builder().msg_type(zbus::message::Type::Signal).interface(iface)?.build())
    };
    let owners = MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .sender("org.freedesktop.DBus")?
        .member("NameOwnerChanged")?
        .build();
    let mut stream = futures_lite::stream::or(
        futures_lite::stream::or(dbus::signals(&conn, rule(ITEM)?).await?, dbus::signals(&conn, rule(WATCHER)?).await?),
        futures_lite::stream::or(dbus::signals(&conn, rule(MENU)?).await?, dbus::signals(&conn, owners).await?),
    );

    let mut state = State::default();
    loop {
        // If the other bar providing the watcher quit, start over and become the watcher.
        if own.is_none() && !dbus::name_has_owner(&conn, WATCHER).await {
            anyhow::bail!("the StatusNotifierWatcher went away");
        }
        // Drop items whose owner went away, when we are the watcher.
        if let Some(items) = &own {
            let keys = items.lock().unwrap_or_else(|e| e.into_inner()).clone();
            let mut alive = Vec::new();
            for key in keys {
                if dbus::name_has_owner(&conn, &split(&key).0).await {
                    alive.push(key);
                }
            }
            *items.lock().unwrap_or_else(|e| e.into_inner()) = alive;
        }
        let mut items = Vec::new();
        for key in registered(&conn, &own).await {
            match item(&conn, &key).await {
                Ok(i) => items.push(i),
                Err(e) => tracing::debug!("tray item {key}: {e:#}"),
            }
        }
        state.items = items;
        if let Some((key, _)) = &state.menu
            && let Some(i) = state.items.iter().find(|i| &i.key == key)
            && let Ok(menu) = load_menu(&conn, i).await
        {
            state.menu = Some((key.clone(), menu));
        }
        if !link.publish(state.clone()) {
            return Ok(());
        }

        match dbus::next(&link.cmds, &mut stream, Duration::from_millis(80)).await? {
            Wake::Quit => return Ok(()),
            Wake::Changed => {}
            Wake::Command(cmd) => {
                let find = |key: &str| state.items.iter().find(|i| i.key == key).cloned();
                let result = match cmd {
                    Cmd::Activate(key) => match find(&key) {
                        Some(i) => dbus::invoke(&conn, &i.dest, &i.path, ITEM, "Activate", &(0i32, 0i32)).await,
                        None => Ok(()),
                    },
                    Cmd::SecondaryActivate(key) => match find(&key) {
                        Some(i) => {
                            dbus::invoke(&conn, &i.dest, &i.path, ITEM, "SecondaryActivate", &(0i32, 0i32)).await
                        }
                        None => Ok(()),
                    },
                    Cmd::Scroll(key, delta) => match find(&key) {
                        Some(i) => dbus::invoke(&conn, &i.dest, &i.path, ITEM, "Scroll", &(delta, "vertical")).await,
                        None => Ok(()),
                    },
                    Cmd::LoadMenu(key) => match find(&key) {
                        Some(i) => load_menu(&conn, &i).await.map(|menu| {
                            state.menu = Some((key, menu));
                        }),
                        None => Ok(()),
                    },
                    Cmd::MenuClick(key, id) => match find(&key) {
                        Some(Item { dest, menu: Some(path), .. }) => {
                            let at = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .map_or(0, |d| d.as_secs() as u32);
                            dbus::invoke(&conn, &dest, &path, MENU, "Event", &(id, "clicked", Value::from(0i32), at))
                                .await
                        }
                        _ => Ok(()),
                    },
                };
                if let Err(e) = result {
                    tracing::warn!("tray: {e:#}");
                }
            }
        }
    }
}

pub async fn check() -> anyhow::Result<String> {
    let conn = Connection::session().await?;
    if !dbus::name_has_owner(&conn, WATCHER).await {
        return Ok("StatusNotifier · no watcher yet; the bar will provide one".into());
    }
    let items = registered(&conn, &None).await;
    Ok(format!("StatusNotifier · {} item(s)", items.len()))
}

#[cfg(test)]
mod tests {
    #[test]
    fn splits_keys() {
        assert_eq!(
            super::split(":1.5/org/ayatana/NotificationItem/x"),
            (":1.5".into(), "/org/ayatana/NotificationItem/x".into())
        );
        assert_eq!(
            super::split("org.kde.StatusNotifierItem-3-1"),
            ("org.kde.StatusNotifierItem-3-1".into(), "/StatusNotifierItem".into())
        );
    }

    #[test]
    fn common_icon_names_use_builtin_icons() {
        assert_eq!(super::builtin("nm-signal-50"), Some("wifi2"));
        assert_eq!(super::builtin("input-keyboard-symbolic"), Some("kbd"));
        assert_eq!(super::builtin("vicinae"), None);
    }
}
