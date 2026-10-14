//! Yozora Launch: apps, windows, files, the clipboard, math, commands and the web, in one
//! field. It stays resident, so opening it is drawing one frame.

mod app;
mod browser;
mod calc;
mod clock;
mod desktop;
mod matcher;
mod money;
mod projects;
mod scripts;
mod search;
mod sources;
mod spawn;
mod ssh;
mod usage;
mod view;

use std::{path::PathBuf, process::ExitCode, time::Duration};

use gpui::{App, AppContext};
use gpui_platform::application;
use tracing_subscriber::EnvFilter;
use yozora_config::{launch::LaunchSettings, paths};
use yozora_ipc::{Incoming, Request, Response, Server};
use yozora_ui::Theme;

use crate::app::{Core, Launcher};

const SOCKET_ID: &str = "launch";

const HELP: &str = "\
yozora-launch: an app launcher for Hyprland, in Yozora's style

Usage:
  yozora-launch [--config DIR]       Run the launcher in the background
  yozora-launch toggle [QUERY]       Open the launcher, or close it if it's open
  yozora-launch open [QUERY]         Open it, with QUERY typed in
  yozora-launch close                Close it
  yozora-launch status               Print whether it's open, as JSON
  yozora-launch quit                 Stop the running launcher

Bind `yozora-launch toggle` to a key. If the launcher isn't running yet, toggle and open start
it. Start a query with @win, @clip, @files, @emoji, = (math), $ (shell) or ? (web).

Options:
  --config DIR    Read launch.toml and theme.toml from DIR instead of ~/.config/yozora
  -h, --help      Show this help
  -V, --version   Show the version
";

enum Command {
    Help,
    Version,
    Run { config: Option<PathBuf> },
    Msg { cmd: String, args: Vec<String>, config: Option<PathBuf> },
}

fn parse(mut args: impl Iterator<Item = String>) -> Result<Command, String> {
    let mut config = None;
    while let Some(a) = args.next() {
        match a.as_str() {
            "-h" | "--help" | "help" => return Ok(Command::Help),
            "-V" | "--version" => return Ok(Command::Version),
            "--config" => config = Some(PathBuf::from(args.next().ok_or("--config needs a directory")?)),
            "toggle" | "open" | "close" | "status" | "quit" => {
                let rest: Vec<String> = args.collect();
                let query = rest.join(" ");
                let args = if query.is_empty() { Vec::new() } else { vec![query] };
                return Ok(Command::Msg { cmd: a, args, config });
            }
            other => return Err(format!("unknown argument {other:?}")),
        }
    }
    Ok(Command::Run { config })
}

fn main() -> ExitCode {
    match parse(std::env::args().skip(1)) {
        Ok(Command::Help) => {
            print!("{HELP}");
            ExitCode::SUCCESS
        }
        Ok(Command::Version) => {
            println!("yozora-launch {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Ok(Command::Msg { cmd, args, config }) => msg(cmd, args, config),
        Ok(Command::Run { config }) => run(config.unwrap_or_else(paths::config_dir), None),
        Err(e) => {
            eprintln!("yozora-launch: {e}\n\nRun `yozora-launch --help` for usage.");
            ExitCode::from(2)
        }
    }
}

/// The fast path: one request over the socket, no GPU, no logging setup.
fn msg(cmd: String, args: Vec<String>, config: Option<PathBuf>) -> ExitCode {
    let path = paths::socket_path(SOCKET_ID);
    let request = Request::new(cmd.clone(), args.clone());
    match yozora_ipc::request_timeout(&path, &request, Duration::from_secs(5)) {
        Ok(r) if r.ok => {
            if !r.data.is_null() {
                println!("{}", r.data);
            }
            ExitCode::SUCCESS
        }
        Ok(r) => {
            eprintln!("yozora-launch: {}", r.error.unwrap_or_else(|| "request failed".into()));
            ExitCode::FAILURE
        }
        // Not running: become the launcher, opened.
        Err(_) if cmd == "toggle" || cmd == "open" => {
            run(config.unwrap_or_else(paths::config_dir), Some(args.into_iter().next().unwrap_or_default()))
        }
        Err(_) if cmd == "close" || cmd == "quit" => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("yozora-launch: is it running? ({}: {e})", path.display());
            ExitCode::FAILURE
        }
    }
}

fn load(dir: &std::path::Path) -> LaunchSettings {
    match yozora_config::launch::load(dir) {
        Ok(loaded) => {
            for w in &loaded.warnings {
                tracing::warn!("config: {w}");
            }
            loaded.value
        }
        Err(e) => {
            tracing::error!("configuration: {e:#}; using defaults");
            LaunchSettings::default()
        }
    }
}

/// The portal's color scheme: true unless it prefers light.
async fn system_dark() -> bool {
    let read = async {
        let conn = zbus::Connection::session().await?;
        let reply = conn
            .call_method(
                Some("org.freedesktop.portal.Desktop"),
                "/org/freedesktop/portal/desktop",
                Some("org.freedesktop.portal.Settings"),
                "ReadOne",
                &("org.freedesktop.appearance", "color-scheme"),
            )
            .await?;
        let v: zbus::zvariant::OwnedValue = reply.body().deserialize()?;
        anyhow::Ok(u32::try_from(v).unwrap_or(0) != 2)
    };
    read.await.unwrap_or(true)
}

fn apply_theme(settings: &LaunchSettings, dark: bool, cx: &mut App) {
    let mut theme = Theme::new(&settings.theme, dark, false);
    theme.font = yozora_ui::theme::resolve_font(cx, &settings.theme.font);
    if cx.reduce_motion() {
        theme.motion.reduced = true;
    }
    cx.set_global(theme);
    cx.refresh_windows();
}

/// Asks Hyprland to blur behind the panel: the rule the bar and yozora-notify add. Rules added
/// at runtime last until Hyprland reloads its config, so the packaged config has it too.
fn apply_blur_rule() {
    let lua = r#"eval hl.layer_rule({ match = { namespace = "^yozora-" }, blur = true, blur_popups = true, ignore_alpha = 0.2, no_anim = true })"#;
    match yozora_ui::hypr::request(lua) {
        Ok(r) if r.trim() == "ok" => tracing::debug!("applied blur layer rules"),
        Ok(_) => {
            // Pre-Lua Hyprland.
            for rule in ["blur,^(yozora-.*)$", "ignorealpha 0.2,^(yozora-.*)$", "noanim,^(yozora-.*)$"] {
                let _ = yozora_ui::hypr::request(&format!("keyword layerrule {rule}"));
            }
        }
        Err(e) => tracing::debug!("could not apply layer rules: {e:#}"),
    }
}

fn handle(incoming: Incoming, cx: &mut App) {
    let r = &incoming.request;
    let query = r.args.first().cloned();
    let response = match r.cmd.as_str() {
        "ping" => Response::ok(),
        "toggle" => {
            Launcher::toggle(query, cx);
            Response::ok()
        }
        "open" => {
            Launcher::open(query, cx);
            Response::ok()
        }
        "close" => {
            Launcher::close(cx);
            Response::ok()
        }
        "status" => Response::data(serde_json::json!({ "open": Launcher::is_open(cx) })),
        "quit" => {
            incoming.reply(Response::ok());
            cx.quit();
            return;
        }
        other => Response::error(format!("unknown command {other:?}")),
    };
    incoming.reply(response);
}

/// Watches the applications directories, and sends a fresh index whenever they change.
fn watch_apps(tx: async_channel::Sender<Vec<search::Entry>>) {
    use inotify::{Inotify, WatchMask};
    let _ = tx.send_blocking(app::index());
    let Ok(mut inotify) = Inotify::init() else { return };
    let mask = WatchMask::CREATE | WatchMask::DELETE | WatchMask::MODIFY | WatchMask::MOVED_TO | WatchMask::MOVED_FROM;
    let mut watched = 0;
    for dir in desktop::dirs() {
        if inotify.watches().add(&dir, mask).is_ok() {
            watched += 1;
        }
    }
    tracing::debug!("watching {watched} application directories");
    let mut buf = [0u8; 4096];
    loop {
        if inotify.read_events_blocking(&mut buf).is_err() {
            return;
        }
        // Package managers write many files in a row: wait for a quiet moment.
        std::thread::sleep(Duration::from_millis(400));
        while inotify.read_events(&mut buf).is_ok_and(|mut e| e.next().is_some()) {}
        if tx.send_blocking(app::index()).is_err() {
            return;
        }
    }
}

fn run(dir: PathBuf, open: Option<String>) -> ExitCode {
    yozora_ui::gpu::prefer_vulkan();
    yozora_ui::malloc::tune();
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_env("YOZORA_LOG").unwrap_or_else(|_| EnvFilter::new("info")))
        .init();

    let socket = match Server::bind(&paths::socket_path(SOCKET_ID)) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("yozora-launch: {e:#}");
            return ExitCode::FAILURE;
        }
    };
    let settings = load(&dir);
    let dark = async_io::block_on(system_dark());

    let (ipc_tx, ipc_rx) = async_channel::unbounded::<Incoming>();
    std::thread::Builder::new()
        .name("yz-ipc".into())
        .spawn(move || async_io::block_on(socket.run(ipc_tx)))
        .expect("spawning the control socket thread");

    let (apps_tx, apps_rx) = async_channel::unbounded();
    std::thread::Builder::new()
        .name("yz-apps".into())
        .spawn(move || watch_apps(apps_tx))
        .expect("spawning the app index thread");

    let (cfg_tx, cfg_rx) = async_channel::unbounded::<()>();
    let watch_dir = dir.clone();
    std::thread::Builder::new()
        .name("yz-config".into())
        .spawn(move || {
            let result = async_io::block_on(async {
                let mut watcher = yozora_config::watch::DirWatcher::new(&watch_dir)?;
                loop {
                    let names = watcher.changed().await?;
                    if names.iter().any(|n| n == yozora_config::launch::FILE || n == "theme.toml") {
                        cfg_tx.send(()).await?;
                    }
                }
                #[allow(unreachable_code)]
                anyhow::Ok(())
            });
            if let Err(e) = result {
                tracing::debug!("config watcher stopped: {e:#}");
            }
        })
        .expect("spawning the config watcher");

    application().with_assets(yozora_ui::Assets).with_quit_mode(gpui::QuitMode::Explicit).run(move |cx: &mut App| {
        yozora_ui::input::bind_keys(cx);
        apply_theme(&settings, dark, cx);
        cx.background_spawn(async { apply_blur_rule() }).detach();
        let core = cx.new(|_| Core::new(settings));
        Launcher::init(core.clone(), cx);
        // Windows, scripts, hosts, projects and pages, so the first open has them.
        core.update(cx, |c, cx| c.refresh(cx));

        let index_core = core.clone();
        cx.spawn(async move |cx| {
            let mut first = true;
            while let Ok(apps) = apps_rx.recv().await {
                cx.update(|cx| {
                    index_core.update(cx, |c, cx| {
                        c.apps = std::sync::Arc::new(apps);
                        c.changed(cx);
                    });
                });
                if first {
                    first = false;
                    // Icons for what the empty launcher shows, decoded before anyone asks.
                    cx.update(|cx| warm_icons(&index_core, cx));
                }
            }
        })
        .detach();

        cx.spawn(async move |cx| {
            while let Ok(incoming) = ipc_rx.recv().await {
                cx.update(|cx| handle(incoming, cx));
            }
        })
        .detach();

        let cfg_core = core.clone();
        cx.spawn(async move |cx| {
            while cfg_rx.recv().await.is_ok() {
                let settings = load(&dir);
                let dark = system_dark().await;
                tracing::info!("configuration reloaded");
                cx.update(|cx| {
                    apply_theme(&settings, dark, cx);
                    Launcher::set_settings(settings, cx);
                });
            }
            drop(cfg_core);
        })
        .detach();

        if let Some(q) = open {
            Launcher::open(if q.is_empty() { None } else { Some(q) }, cx);
        }
    });
    ExitCode::SUCCESS
}

/// Starts decoding the icons of pinned and often used apps into GPUI's image cache.
fn warm_icons(core: &gpui::Entity<Core>, cx: &mut App) {
    let c = core.read(cx);
    let (now, hour) = usage::now();
    let mut keys: Vec<String> = c.usage.pins.clone();
    keys.extend(c.usage.top(now, hour, 24).into_iter().map(str::to_string));
    let paths: Vec<PathBuf> = c
        .apps
        .iter()
        .filter(|e| keys.contains(&e.key()))
        .filter_map(|e| e.icon.as_ref().map(|p| p.as_ref().clone()))
        .collect();
    for p in paths {
        let _ = cx.fetch_asset::<gpui::ImgResourceLoader>(&gpui::Resource::Path(p.into()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli() {
        let p = |s: &str| parse(s.split_whitespace().map(str::to_string));
        assert!(matches!(p(""), Ok(Command::Run { config: None })));
        assert!(
            matches!(p("toggle"), Ok(Command::Msg { ref cmd, ref args, .. }) if cmd == "toggle" && args.is_empty())
        );
        assert!(
            matches!(p("open @win fire"), Ok(Command::Msg { ref cmd, ref args, .. }) if cmd == "open" && args == &["@win fire"])
        );
        assert!(matches!(p("--config /x toggle"), Ok(Command::Msg { config: Some(_), .. })));
        assert!(p("bogus").is_err());
    }
}
