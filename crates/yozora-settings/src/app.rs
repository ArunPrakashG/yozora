//! Startup, the window, and the background work that keeps it current: config and manifest
//! watchers, app status, the desktop's color scheme and the control socket.

use std::{path::PathBuf, time::Duration};

use gpui::{
    App, AppContext, Bounds, Global, KeyBinding, WeakEntity, WindowBackgroundAppearance,
    WindowBounds, WindowDecorations, WindowOptions, actions, px, size,
};
use serde_json::json;
use yozora_config::{theme::ThemeConfig, watch::DirWatcher};
use yozora_ipc::{Incoming, Response};
use yozora_ui::Theme;

use crate::{Options, apps, view::SettingsView};

actions!(yozora_settings, [FocusSearch, CloseWindow]);

/// The desktop's light or dark preference, for `theme = "auto"`.
#[derive(Clone, Copy)]
pub struct SystemDark(pub bool);

impl Global for SystemDark {}

pub fn start(cx: &mut App, opts: Options, ipc: async_channel::Receiver<Incoming>) {
    yozora_ui::input::bind_keys(cx);
    cx.bind_keys([
        KeyBinding::new("ctrl-f", FocusSearch, None),
        KeyBinding::new("ctrl-w", CloseWindow, None),
        KeyBinding::new("ctrl-q", CloseWindow, None),
    ]);
    cx.set_global(SystemDark(true));
    let theme_config: ThemeConfig = std::fs::read_to_string(opts.config_dir.join("theme.toml"))
        .ok()
        .and_then(|t| toml::from_str(&t).ok())
        .unwrap_or_default();
    apply_theme(&theme_config, cx);

    let config_dir = opts.config_dir.clone();
    let extra_apps = opts.extra_apps.clone();
    let bounds = Bounds::centered(None, size(px(1180.), px(800.)), cx);
    let window = cx
        .open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                app_id: Some("yozora-settings".into()),
                titlebar: Some(gpui::TitlebarOptions { title: Some("Yozora Settings".into()), ..Default::default() }),
                window_background: WindowBackgroundAppearance::Transparent,
                window_decorations: Some(WindowDecorations::Server),
                window_min_size: Some(size(px(860.), px(560.))),
                inactive_frame_interval: None,
                ..Default::default()
            },
            |window, cx| cx.new(|cx| SettingsView::new(opts, window, cx)),
        )
        .expect("opening the settings window");
    let view = window.update(cx, |_, _, cx| cx.entity().downgrade()).expect("reading the view");

    // Settings only lives while its window is open.
    cx.on_window_closed(|cx, _| {
        if cx.windows().is_empty() {
            cx.quit();
        }
    })
    .detach();

    watch_dir(config_dir, view.clone(), cx, |view, names, cx| view.files_changed(names, cx));
    for dir in apps::manifest_dirs(&extra_apps) {
        watch_dir(dir, view.clone(), cx, |view, _, cx| view.rescan(cx));
    }
    poll_status(view.clone(), cx);
    watch_color_scheme(view.clone(), cx);

    cx.spawn(async move |cx| {
        while let Ok(incoming) = ipc.recv().await {
            cx.update(|cx| handle_ipc(incoming, &view, cx));
        }
    })
    .detach();
}

/// Resolves the theme for Settings' own window from `theme.toml`.
pub fn apply_theme(config: &ThemeConfig, cx: &mut App) {
    let dark = cx.global::<SystemDark>().0;
    // Settings is a normal window; its glass doesn't depend on the bar's blur rule.
    let mut theme = Theme::new(config, dark, false);
    theme.font = yozora_ui::theme::resolve_font(cx, &config.font);
    if cx.reduce_motion() {
        theme.motion.reduced = true;
    }
    cx.set_global(theme);
    cx.refresh_windows();
}

fn handle_ipc(incoming: Incoming, view: &WeakEntity<SettingsView>, cx: &mut App) {
    let args = incoming.request.args.clone();
    let response = match incoming.request.cmd.as_str() {
        "ping" => Response::data(json!({ "version": env!("CARGO_PKG_VERSION"), "warnings": [] })),
        "focus" | "page" => {
            let result = view.update(cx, |view, cx| {
                if let Some(page) = args.first() {
                    view.open_page(page, cx)?;
                }
                Ok::<_, String>(())
            });
            if let Some(window) = cx.windows().first() {
                let _ = window.update(cx, |_, window, _| window.activate_window());
            }
            match result {
                Ok(Ok(())) => Response::ok(),
                Ok(Err(e)) => Response::error(e),
                Err(_) => Response::error("the window is closing"),
            }
        }
        other => Response::error(format!("unknown command `{other}`; try ping or focus")),
    };
    incoming.reply(response);
}

/// Runs `on_change` on the view whenever files in `dir` change.
fn watch_dir(
    dir: PathBuf,
    view: WeakEntity<SettingsView>,
    cx: &mut App,
    on_change: impl Fn(&mut SettingsView, Vec<String>, &mut gpui::Context<SettingsView>) + 'static,
) {
    let (tx, rx) = async_channel::unbounded::<Vec<String>>();
    let name = dir.display().to_string();
    std::thread::Builder::new()
        .name("yz-watch".into())
        .spawn(move || {
            let mut watcher = match DirWatcher::new(&dir) {
                Ok(w) => w,
                Err(e) => {
                    tracing::debug!("not watching {name}: {e:#}");
                    return;
                }
            };
            while let Ok(names) = async_io::block_on(watcher.changed()) {
                let names = names.iter().map(|n| n.to_string_lossy().into_owned()).collect();
                if tx.send_blocking(names).is_err() {
                    return;
                }
            }
        })
        .expect("spawning a watcher");
    cx.spawn(async move |cx| {
        while let Ok(mut names) = rx.recv().await {
            // Editors write in several steps.
            cx.background_executor().timer(Duration::from_millis(80)).await;
            while let Ok(more) = rx.try_recv() {
                names.extend(more);
            }
            if view.update(cx, |view, cx| on_change(view, names, cx)).is_err() {
                return;
            }
        }
    })
    .detach();
}

/// Pings every app's socket every two seconds, and rescans manifests as a fallback for
/// directories that couldn't be watched (system ones that don't exist yet).
fn poll_status(view: WeakEntity<SettingsView>, cx: &mut App) {
    cx.spawn(async move |cx| {
        let mut tick = 0u32;
        loop {
            if tick % 3 == 0 && view.update(cx, |view, cx| view.rescan(cx)).is_err() {
                return;
            }
            let Ok(manifests) = view.read_with(cx, |view, _| view.manifests()) else {
                return;
            };
            let statuses = cx
                .background_spawn(async move {
                    manifests.into_iter().map(|m| (m.id.clone(), apps::probe(&m))).collect::<Vec<_>>()
                })
                .await;
            if view.update(cx, |view, cx| view.set_statuses(statuses, cx)).is_err() {
                return;
            }
            tick += 1;
            cx.background_executor().timer(Duration::from_secs(2)).await;
        }
    })
    .detach();
}

/// Follows xdg-desktop-portal's `color-scheme`.
fn watch_color_scheme(view: WeakEntity<SettingsView>, cx: &mut App) {
    let (tx, rx) = async_channel::unbounded::<bool>();
    std::thread::Builder::new()
        .name("yz-appearance".into())
        .spawn(move || {
            let result: anyhow::Result<()> = async_io::block_on(async {
                use futures_lite::StreamExt;
                let conn = zbus::Connection::session().await?;
                let proxy = zbus::Proxy::new(
                    &conn,
                    "org.freedesktop.portal.Desktop",
                    "/org/freedesktop/portal/desktop",
                    "org.freedesktop.portal.Settings",
                )
                .await?;
                let read = async || -> anyhow::Result<bool> {
                    let v: zbus::zvariant::OwnedValue =
                        proxy.call("ReadOne", &("org.freedesktop.appearance", "color-scheme")).await?;
                    // 1 is "prefer dark", 2 "prefer light", 0 "no preference".
                    Ok(u32::try_from(v).unwrap_or(0) != 2)
                };
                tx.send(read().await.unwrap_or(true)).await?;
                let mut changes = proxy.receive_signal("SettingChanged").await?;
                while changes.next().await.is_some() {
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
            let changed = cx.update(|cx| {
                let changed = cx.global::<SystemDark>().0 != dark;
                cx.set_global(SystemDark(dark));
                changed
            });
            if changed && view.update(cx, |view, cx| view.reapply_theme(cx)).is_err() {
                return;
            }
        }
    })
    .detach();
}
