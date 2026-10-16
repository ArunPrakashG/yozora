//! The running bar: configuration, theme, one bar window per monitor, hot reload and the
//! control socket.

use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

use gpui::{App, AppContext, DisplayId, Global, WindowHandle};
use serde_json::json;
use yozora_config::{
    Warning,
    bar::{BarSettings, BarStyle},
    theme::ThemeMode,
};
use yozora_ipc::{Incoming, Response};
use yozora_ui::Theme;

use crate::{
    bar::Bar,
    modules::{Layout, Pop},
    services::{self, Kind, Services, hyprland},
};

/// The loaded configuration.
pub struct Config {
    pub settings: Arc<BarSettings>,
    pub warnings: Vec<Warning>,
    pub dir: PathBuf,
}

impl Global for Config {}

impl Config {
    pub fn get(cx: &App) -> &Self {
        cx.global::<Self>()
    }
}

struct BarWindow {
    monitor: String,
    display: DisplayId,
    handle: WindowHandle<Bar>,
}

/// Bar windows and the facts they were opened with.
pub struct Shell {
    bars: Vec<BarWindow>,
    system_dark: bool,
    /// The control center's dark mode tile, until the next restart or config reload.
    dark_override: Option<bool>,
    pub started: Instant,
    /// Whether a bar window has ever opened, so a failure to open the first one can fall back
    /// to GL.
    opened_any: bool,
}

impl Global for Shell {}

pub fn load(dir: &std::path::Path) -> anyhow::Result<yozora_config::Loaded<BarSettings>> {
    let mut loaded = yozora_config::bar::load(dir)?;
    for name in crate::modules::unknown(&loaded.value.bar.modules) {
        loaded.warnings.push(Warning {
            file: yozora_config::bar::FILE.into(),
            line: None,
            message: format!("unknown module {name:?}; known modules: {}", crate::modules::Module::NAMES.join(", ")),
        });
    }
    for (connector, o) in &loaded.value.bar.monitor {
        let zones = loaded.value.bar.zones_for(connector);
        for name in crate::modules::unknown(&zones) {
            loaded.warnings.push(Warning {
                file: yozora_config::bar::FILE.into(),
                line: None,
                message: format!("unknown module {name:?} in [monitor.{connector}]"),
            });
        }
        let _ = o;
    }
    Ok(loaded)
}

pub fn start(cx: &mut App, dir: PathBuf, loaded: yozora_config::Loaded<BarSettings>, fakes: services::Fakes) {
    for w in &loaded.warnings {
        tracing::warn!("config: {w}");
    }
    cx.set_global(Config { settings: Arc::new(loaded.value), warnings: loaded.warnings, dir: dir.clone() });
    cx.set_global(Services { fakes, ..Default::default() });
    cx.set_global(Shell {
        bars: Vec::new(),
        system_dark: true,
        dark_override: None,
        started: Instant::now(),
        opened_any: false,
    });
    apply_theme(cx);
    ensure_services(cx);
    apply_layer_rules(cx);

    // Bars need Hyprland's monitor names; open them as soon as the first snapshot arrives,
    // and reconcile whenever monitors change.
    if let Some(hypr) = Services::get(cx).hyprland.clone() {
        cx.observe(&hypr, |_, cx| reconcile(cx)).detach();
    }
    cx.observe_displays(|event, cx| {
        tracing::info!("display event: {event:?}");
        reconcile(cx);
    })
    .detach();
    // Without Hyprland, still open bars after a moment, named by display.
    cx.spawn(async |cx| {
        cx.background_executor().timer(Duration::from_millis(1500)).await;
        cx.update(reconcile);
    })
    .detach();

    watch_config(cx, dir);
    watch_color_scheme(cx);
}

fn ensure_services(cx: &mut App) {
    let settings = Config::get(cx).settings.clone();
    let mut kinds = Layout::parse(&settings.bar.modules).services();
    for connector in settings.bar.monitor.keys() {
        kinds.extend(Layout::parse(&settings.bar.zones_for(connector)).services());
    }
    let mut names = Layout::parse(&settings.bar.modules).dynamic_names();
    for connector in settings.bar.monitor.keys() {
        names.extend(Layout::parse(&settings.bar.zones_for(connector)).dynamic_names());
    }
    Services::ensure_modules(&names, cx);
    // Monitors and workspaces decide which bars exist, so Hyprland always runs.
    kinds.insert(Kind::Hyprland);
    if settings.bar.osd.enabled {
        kinds.insert(Kind::Audio);
    }
    Services::ensure(kinds, &settings.bar.custom, cx);
    crate::osd::observe(cx);
    // Bars may need to observe services that just started.
    for bar in Shell::bars(cx) {
        let _ = bar.update(cx, |bar, _, cx| bar.observe_services(cx));
    }
}

/// Tells Hyprland to blur our surfaces. Rules added at runtime last until Hyprland reloads
/// its config, which is why we add them on every start rather than asking the user to.
fn apply_layer_rules(cx: &mut App) {
    if !Config::get(cx).settings.bar.bar.blur {
        return;
    }
    cx.background_spawn(async {
        let lua = r#"eval hl.layer_rule({ match = { namespace = "^yozora-" }, blur = true, blur_popups = true, ignore_alpha = 0.2, no_anim = true })"#;
        match hyprland::request(lua) {
            Ok(r) if r.trim() == "ok" => tracing::debug!("applied blur layer rules"),
            Ok(_) => {
                // Pre-Lua Hyprland.
                for rule in ["blur,^(yozora-.*)$", "blurpopups,^(yozora-.*)$", "ignorealpha 0.2,^(yozora-.*)$", "noanim,^(yozora-.*)$"] {
                    let _ = hyprland::request(&format!("keyword layerrule {rule}"));
                }
            }
            Err(e) => tracing::debug!("could not apply layer rules: {e:#}"),
        }
    })
    .detach();
}

pub fn apply_theme(cx: &mut App) {
    let settings = Config::get(cx).settings.clone();
    let shell = cx.global::<Shell>();
    let mut config = settings.theme.clone();
    if let Some(dark) = shell.dark_override {
        config.theme = if dark { ThemeMode::Midnight } else { ThemeMode::Daybreak };
    }
    let mut theme = Theme::new(&config, shell.system_dark, !settings.bar.bar.blur);
    theme.font = yozora_ui::theme::resolve_font(cx, &settings.theme.font);
    if cx.reduce_motion() {
        theme.motion.reduced = true;
    }
    cx.set_global(theme);
    cx.refresh_windows();
}

/// Forces dark or light for this session, or follows the configuration again with `None`.
pub fn set_dark_override(dark: Option<bool>, cx: &mut App) {
    cx.global_mut::<Shell>().dark_override = dark;
    apply_theme(cx);
}

impl Shell {
    pub fn bars(cx: &App) -> Vec<WindowHandle<Bar>> {
        cx.global::<Shell>().bars.iter().map(|b| b.handle).collect()
    }
}

/// The Hyprland monitor at a display's position. GPUI's display bounds don't account for
/// rotation, but their origin is the output's position in the layout, which is unique.
fn monitor_name(display: &dyn gpui::PlatformDisplay, hypr: Option<&hyprland::State>, index: usize) -> String {
    let origin = display.bounds().origin;
    let (x, y) = (f32::from(origin.x).round() as i32, f32::from(origin.y).round() as i32);
    hypr.and_then(|h| h.monitors.iter().find(|m| m.x == x && m.y == y))
        .map(|m| m.name.clone())
        .unwrap_or_else(|| format!("display-{index}"))
}

/// The focused monitor's name and display, for surfaces that follow focus like the OSD.
pub fn focused_display(cx: &App) -> Option<(String, DisplayId)> {
    let hypr = Services::get(cx).hyprland.as_ref().filter(|h| h.read(cx).ready).map(|h| h.read(cx).state.clone());
    let focused = hypr.as_ref().and_then(|h| h.monitors.iter().find(|m| m.focused)).map(|m| m.name.clone());
    let displays = cx.displays();
    let named: Vec<_> =
        displays.iter().enumerate().map(|(i, d)| (monitor_name(d.as_ref(), hypr.as_ref(), i), d.id())).collect();
    named.iter().find(|(n, _)| Some(n) == focused.as_ref()).or(named.first()).cloned()
}

/// Opens and closes bar windows to match the connected monitors and the configuration.
pub fn reconcile(cx: &mut App) {
    let hypr = Services::get(cx).hyprland.clone();
    let hypr_state = hypr.as_ref().filter(|h| h.read(cx).ready).map(|h| h.read(cx).state.clone());
    let elapsed = cx.global::<Shell>().started.elapsed();
    if hypr_state.is_none() && elapsed < Duration::from_millis(1400) {
        return;
    }
    let settings = Config::get(cx).settings.clone();
    let mut wanted: Vec<(String, DisplayId)> = Vec::new();
    for (i, display) in cx.displays().iter().enumerate() {
        let name = monitor_name(display.as_ref(), hypr_state.as_ref(), i);
        if settings.bar.shows_on(&name) {
            wanted.push((name, display.id()));
        }
    }

    let bars = std::mem::take(&mut cx.global_mut::<Shell>().bars);
    let (keep, close): (Vec<_>, Vec<_>) =
        bars.into_iter().partition(|b| wanted.iter().any(|(n, d)| *n == b.monitor && *d == b.display));
    for bar in close {
        tracing::info!("closing bar on {}", bar.monitor);
        let _ = bar.handle.update(cx, |_, window, _| window.remove_window());
    }
    let mut bars = keep;
    for (monitor, display) in wanted {
        if bars.iter().any(|b| b.monitor == monitor && b.display == display) {
            continue;
        }
        match Bar::open(display, monitor.clone(), cx) {
            Ok(handle) => {
                tracing::info!("opened bar on {monitor}");
                bars.push(BarWindow { monitor, display, handle });
            }
            Err(e) if yozora_ui::gpu::gl_hidden() && !cx.global::<Shell>().opened_any => {
                // Most likely Vulkan doesn't work here; GL may.
                tracing::error!("could not open bar on {monitor}: {e:#}; restarting with GL");
                cx.global_mut::<Shell>().bars = bars;
                let e = yozora_ui::gpu::restart_with_gl();
                tracing::error!("restarting: {e}");
                return;
            }
            Err(e) => tracing::error!("could not open bar on {monitor}: {e:#}"),
        }
    }
    if !bars.is_empty() {
        cx.global_mut::<Shell>().opened_any = true;
    }
    cx.global_mut::<Shell>().bars = bars;
}

/// Replaces every bar with one built from the new settings. The new bars open before the old
/// ones close, so the screen never goes without a bar.
fn reopen_bars(cx: &mut App) {
    let old = std::mem::take(&mut cx.global_mut::<Shell>().bars);
    reconcile(cx);
    for bar in old {
        let _ = bar.handle.update(cx, |_, window, _| window.remove_window());
    }
}

pub fn reload(cx: &mut App) -> anyhow::Result<Vec<Warning>> {
    let dir = Config::get(cx).dir.clone();
    let loaded = load(&dir)?;
    for w in &loaded.warnings {
        tracing::warn!("config: {w}");
    }
    let old = Config::get(cx).settings.clone();
    let new = Arc::new(loaded.value);
    let geometry_changed =
        old.bar.bar != new.bar.bar || old.bar.modules != new.bar.modules || old.bar.monitor != new.bar.monitor;
    let warnings = loaded.warnings.clone();
    cx.set_global(Config { settings: new.clone(), warnings: loaded.warnings, dir });
    if old.theme != new.theme {
        cx.global_mut::<Shell>().dark_override = None;
    }
    apply_theme(cx);
    ensure_services(cx);
    if old.bar.bar.blur != new.bar.bar.blur {
        apply_layer_rules(cx);
    }
    if geometry_changed {
        reopen_bars(cx);
    } else {
        cx.refresh_windows();
    }
    tracing::info!("configuration reloaded");
    Ok(warnings)
}

/// Reloads when files in the config directory change.
fn watch_config(cx: &mut App, dir: PathBuf) {
    let (tx, rx) = async_channel::unbounded::<()>();
    std::thread::Builder::new()
        .name("yz-config".into())
        .spawn(move || {
            let mut watcher = match yozora_config::watch::DirWatcher::new(&dir) {
                Ok(w) => w,
                Err(e) => {
                    tracing::warn!("not watching {}: {e:#}", dir.display());
                    return;
                }
            };
            loop {
                match async_io::block_on(watcher.changed()) {
                    Ok(names) => {
                        let relevant = names.iter().any(|n| {
                            let n = n.to_string_lossy();
                            n == "*" || n.ends_with(".toml")
                        });
                        if relevant && tx.send_blocking(()).is_err() {
                            return;
                        }
                    }
                    Err(e) => {
                        tracing::warn!("config watcher: {e:#}");
                        return;
                    }
                }
            }
        })
        .expect("spawning the config watcher");
    cx.spawn(async move |cx| {
        while rx.recv().await.is_ok() {
            // Editors write in several steps; wait for the dust to settle.
            cx.background_executor().timer(Duration::from_millis(120)).await;
            while rx.try_recv().is_ok() {}
            cx.update(|cx| {
                if let Err(e) = reload(cx) {
                    tracing::error!("keeping the previous configuration: {e:#}");
                }
            });
        }
    })
    .detach();
}

/// Follows the desktop's light or dark preference through xdg-desktop-portal, for
/// `theme = "auto"`.
fn watch_color_scheme(cx: &mut App) {
    let (tx, rx) = async_channel::unbounded::<bool>();
    std::thread::Builder::new()
        .name("yz-appearance".into())
        .spawn(move || {
            let result: anyhow::Result<()> = async_io::block_on(async {
                use futures_lite::StreamExt;
                let conn = zbus::Connection::session().await?;
                let dest = "org.freedesktop.portal.Desktop";
                let path = "/org/freedesktop/portal/desktop";
                let iface = "org.freedesktop.portal.Settings";
                let read = async || -> anyhow::Result<bool> {
                    let v: zbus::zvariant::OwnedValue = services::dbus::call(
                        &conn,
                        dest,
                        path,
                        iface,
                        "ReadOne",
                        &("org.freedesktop.appearance", "color-scheme"),
                    )
                    .await?;
                    // 1 is "prefer dark", 2 "prefer light", 0 "no preference".
                    Ok(u32::try_from(v).unwrap_or(0) != 2)
                };
                tx.send(read().await.unwrap_or(true)).await?;
                let rule = zbus::MatchRule::builder()
                    .msg_type(zbus::message::Type::Signal)
                    .interface(iface)?
                    .member("SettingChanged")?
                    .build();
                let mut stream = services::dbus::signals(&conn, rule).await?;
                while let Some(Ok(_)) = stream.next().await {
                    if let Ok(dark) = read().await {
                        tx.send(dark).await?;
                    }
                }
                Ok(())
            });
            if let Err(e) = result {
                tracing::debug!("color scheme: {e:#}");
            }
        })
        .expect("spawning the appearance watcher");
    cx.spawn(async move |cx| {
        while let Ok(dark) = rx.recv().await {
            cx.update(|cx| {
                if cx.global::<Shell>().system_dark != dark {
                    cx.global_mut::<Shell>().system_dark = dark;
                    apply_theme(cx);
                }
            });
        }
    })
    .detach();
}

/// The bar on `monitor`, or on the focused monitor.
fn bar_for(monitor: Option<&str>, cx: &App) -> Option<WindowHandle<Bar>> {
    let shell = cx.global::<Shell>();
    let focused = Services::get(cx)
        .hyprland
        .as_ref()
        .and_then(|h| h.read(cx).state.monitors.iter().find(|m| m.focused).map(|m| m.name.clone()));
    let want = monitor.map(str::to_string).or(focused);
    shell.bars.iter().find(|b| Some(&b.monitor) == want.as_ref()).or(shell.bars.first()).map(|b| b.handle)
}

fn rss_kib() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    status.lines().find_map(|l| l.strip_prefix("VmRSS:")?.trim().strip_suffix("kB")?.trim().parse().ok())
}

pub fn handle_ipc(incoming: Incoming, cx: &mut App) {
    let args = incoming.request.args.clone();
    let response = match incoming.request.cmd.as_str() {
        "ping" => Response::data(json!({
            "version": env!("CARGO_PKG_VERSION"),
            "warnings": Config::get(cx).warnings.iter().map(|w| w.to_string()).collect::<Vec<_>>(),
        })),
        "reload" => match reload(cx) {
            Ok(w) => Response::data(json!({ "warnings": w.iter().map(|w| w.to_string()).collect::<Vec<_>>() })),
            Err(e) => Response::error(format!("{e:#}")),
        },
        "toggle" => {
            let Some(pop) = args.first().and_then(|n| Pop::parse(n).or_else(|| tray_pop(n, cx))) else {
                incoming.reply(Response::error("toggle needs a module with a popover, like control-center"));
                return;
            };
            match bar_for(args.get(1).map(String::as_str), cx) {
                Some(bar) => {
                    let _ = bar.update(cx, |bar, window, cx| bar.toggle(pop, false, window, cx));
                    Response::ok()
                }
                None => Response::error("no bar is open"),
            }
        }
        "close" => {
            for bar in Shell::bars(cx) {
                let _ = bar.update(cx, |bar, _, cx| bar.close_popover(cx));
            }
            Response::ok()
        }
        "stats" => {
            let shell = cx.global::<Shell>();
            let bars: Vec<_> = shell.bars.iter().map(|b| b.monitor.clone()).collect();
            let stats: Vec<_> =
                Shell::bars(cx).into_iter().filter_map(|h| h.read(cx).ok().map(|b| b.stats_json())).collect();
            Response::data(json!({
                "version": env!("CARGO_PKG_VERSION"),
                "uptime_s": cx.global::<Shell>().started.elapsed().as_secs(),
                "rss_kib": rss_kib(),
                "heap_used_kib": yozora_ui::malloc::heap().map(|h| h.0 / 1024),
                "heap_free_kib": yozora_ui::malloc::heap().map(|h| h.1 / 1024),
                "bars": bars,
                "frames": stats,
            }))
        }
        "bench" => {
            let what = args.first().map(String::as_str).unwrap_or("workspaces").to_string();
            let count = args.get(1).and_then(|n| n.parse().ok()).unwrap_or(20u32).clamp(1, 500);
            match bar_for(args.get(2).map(String::as_str), cx) {
                Some(bar) => {
                    let _ = bar.update(cx, |bar, window, cx| bar.bench(&what, count, incoming, window, cx));
                    return;
                }
                None => Response::error("no bar is open"),
            }
        }
        "osd" => {
            osd_command(args, incoming, cx);
            return;
        }
        "state" => {
            crate::desktop::state(args, incoming, cx);
            return;
        }
        "media" => crate::desktop::media(&args, cx),
        "volume" => crate::desktop::volume(&args, cx),
        "profile" => crate::desktop::profile(&args, cx),
        "quit" => {
            incoming.reply(Response::ok());
            cx.quit();
            return;
        }
        other => Response::error(format!("unknown command {other:?}")),
    };
    incoming.reply(response);
}

/// `osd brightness [+N%|-N%|N%]` changes the brightness and shows it; `osd volume`, `mic` and
/// `layout` show the current value.
fn osd_command(args: Vec<String>, incoming: Incoming, cx: &mut App) {
    use crate::{osd, services::brightness};
    match args.first().map(String::as_str) {
        Some("brightness") => {
            let change = match brightness::parse(args.get(1).map(String::as_str)) {
                Ok(c) => c,
                Err(e) => return incoming.reply(Response::error(e)),
            };
            cx.spawn(async move |cx| {
                // D-Bus and sysfs: off the main thread.
                let result = cx.background_executor().spawn(async move { brightness::apply(change) }).await;
                cx.update(|cx| match result {
                    Ok(level) => {
                        osd::show(osd::Shown::Brightness(level), cx);
                        incoming.reply(Response::data(json!({ "brightness": (level * 100.).round() })));
                    }
                    Err(e) => incoming.reply(Response::error(format!("{e:#}"))),
                });
            })
            .detach();
        }
        Some(what) => match osd::current(what, cx) {
            Ok(shown) => {
                osd::show(shown, cx);
                incoming.reply(Response::ok());
            }
            Err(e) => incoming.reply(Response::error(e)),
        },
        None => incoming.reply(Response::error("osd needs brightness, volume, mic or layout")),
    }
}

/// `tray:N` or `tray:NAME`: the menu of the Nth tray item, or the first whose title contains NAME.
fn tray_pop(name: &str, cx: &App) -> Option<Pop> {
    let which = name.strip_prefix("tray:")?;
    let tray = Services::get(cx).tray.as_ref()?.read(cx);
    let items = &tray.state.items;
    let item = match which.parse::<usize>() {
        Ok(n) => items.get(n),
        Err(_) => items.iter().find(|i| i.title.to_lowercase().contains(&which.to_lowercase())),
    }?;
    Some(Pop::Tray(item.key.clone()))
}

pub fn style(cx: &App) -> BarStyle {
    Config::get(cx).settings.bar.bar.style
}
