//! Yozora Notify: the notification server, with toasts in the bar's style and a history the
//! bar's notification center reads.

mod hypr;
mod server;
mod store;
mod toasts;

use std::{
    path::PathBuf,
    process::ExitCode,
    sync::{Arc, Mutex},
};

use gpui::{App, AppContext};
use gpui_platform::application;
use tracing_subscriber::EnvFilter;
use yozora_config::{notify::NotifySettings, paths};
use yozora_ipc::{Request, Server};
use yozora_ui::Theme;

use crate::{
    server::{Cmd, Inner, Shared, UiEvent},
    store::Store,
    toasts::Stack,
};

const SOCKET_ID: &str = "notify";

const HELP: &str = "\
yozora-notify: notifications for Hyprland, in Yozora's style

Usage:
  yozora-notify [--replace] [--config DIR]   Run the notification server
  yozora-notify dnd [on|off|toggle]          Do Not Disturb (toggle by default)
  yozora-notify status                       Do Not Disturb, unread and count, as JSON
  yozora-notify list                         The notification center, as JSON
  yozora-notify clear                        Empty the notification center
  yozora-notify quit                         Stop the running server

Options:
  --replace       Take over from another daemon that allows it, such as an older yozora-notify
  --config DIR    Read notify.toml and theme.toml from DIR instead of ~/.config/yozora
  -h, --help      Show this help
  -V, --version   Show the version

Only one notification server can run at a time. Stop mako or dunst before starting this one.
";

enum Command {
    Help,
    Version,
    Run { replace: bool, config: Option<PathBuf> },
    Msg(String, Vec<String>),
}

fn parse(mut args: impl Iterator<Item = String>) -> Result<Command, String> {
    let mut replace = false;
    let mut config = None;
    while let Some(a) = args.next() {
        match a.as_str() {
            "-h" | "--help" | "help" => return Ok(Command::Help),
            "-V" | "--version" => return Ok(Command::Version),
            "--replace" => replace = true,
            "--config" => config = Some(PathBuf::from(args.next().ok_or("--config needs a directory")?)),
            "dnd" | "status" | "list" | "clear" | "quit" => return Ok(Command::Msg(a, args.collect())),
            other => return Err(format!("unknown argument {other:?}")),
        }
    }
    Ok(Command::Run { replace, config })
}

fn main() -> ExitCode {
    match parse(std::env::args().skip(1)) {
        Ok(Command::Help) => {
            print!("{HELP}");
            ExitCode::SUCCESS
        }
        Ok(Command::Version) => {
            println!("yozora-notify {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Ok(Command::Msg(cmd, args)) => msg(cmd, args),
        Ok(Command::Run { replace, config }) => run(replace, config.unwrap_or_else(paths::config_dir)),
        Err(e) => {
            eprintln!("yozora-notify: {e}\n\nRun `yozora-notify --help` for usage.");
            ExitCode::from(2)
        }
    }
}

fn msg(cmd: String, args: Vec<String>) -> ExitCode {
    let path = paths::socket_path(SOCKET_ID);
    match yozora_ipc::request(&path, &Request::new(cmd, args)) {
        Ok(r) if r.ok => {
            if !r.data.is_null() {
                println!("{}", serde_json::to_string_pretty(&r.data).unwrap_or_default());
            }
            ExitCode::SUCCESS
        }
        Ok(r) => {
            eprintln!("yozora-notify: {}", r.error.unwrap_or_else(|| "request failed".into()));
            ExitCode::FAILURE
        }
        Err(e) => {
            eprintln!("yozora-notify: is it running? ({}: {e})", path.display());
            ExitCode::FAILURE
        }
    }
}

fn load(dir: &std::path::Path) -> NotifySettings {
    match yozora_config::notify::load(dir) {
        Ok(loaded) => {
            for w in &loaded.warnings {
                tracing::warn!("config: {w}");
            }
            loaded.value
        }
        Err(e) => {
            tracing::error!("configuration: {e:#}; using defaults");
            NotifySettings::default()
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

fn apply_theme(settings: &NotifySettings, dark: bool, cx: &mut App) {
    let mut theme = Theme::new(&settings.theme, dark, false);
    theme.font = yozora_ui::theme::resolve_font(cx, &settings.theme.font);
    if cx.reduce_motion() {
        theme.motion.reduced = true;
    }
    cx.set_global(theme);
    cx.refresh_windows();
}

fn run(replace: bool, dir: PathBuf) -> ExitCode {
    yozora_ui::gpu::prefer_vulkan();
    yozora_ui::malloc::tune();
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_env("YOZORA_LOG").unwrap_or_else(|_| EnvFilter::new("info")))
        .init();

    let settings = load(&dir);
    let socket = match Server::bind(&paths::socket_path(SOCKET_ID)) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("yozora-notify: {e:#}");
            return ExitCode::FAILURE;
        }
    };

    let store = Store::load(&store::state_file(), settings.notify.history.limit as usize);
    server::prune_images(&store);
    let (ui_tx, ui_rx) = async_channel::unbounded::<UiEvent>();
    let (cmd_tx, cmd_rx) = async_channel::unbounded::<Cmd>();
    let shared = Shared {
        inner: Arc::new(Mutex::new(Inner { store, config: settings.notify.clone() })),
        ui: ui_tx,
        cmds: cmd_tx.clone(),
    };

    // Take the bus name before opening anything, so a second daemon exits right away.
    let (conn, dark) = match async_io::block_on(async {
        let conn = server::connect(shared.clone(), replace).await?;
        anyhow::Ok((conn, system_dark().await))
    }) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("yozora-notify: {e:#}");
            return ExitCode::FAILURE;
        }
    };
    tracing::info!("serving {} on the session bus", server::NAME);

    let serve_shared = shared.clone();
    std::thread::Builder::new()
        .name("yz-dbus".into())
        .spawn(move || {
            if let Err(e) = async_io::block_on(server::serve(conn, serve_shared.clone(), cmd_rx)) {
                tracing::error!("notification server: {e:#}");
                let _ = serve_shared.ui.try_send(UiEvent::Quit);
            }
        })
        .expect("spawning the D-Bus thread");

    let ipc_cmds = cmd_tx.clone();
    std::thread::Builder::new()
        .name("yz-ipc".into())
        .spawn(move || {
            let (tx, rx) = async_channel::unbounded();
            async_io::block_on(futures_lite::future::zip(socket.run(tx), async move {
                while let Ok(incoming) = rx.recv().await {
                    if ipc_cmds.send(Cmd::Ipc(incoming)).await.is_err() {
                        break;
                    }
                }
            }));
        })
        .expect("spawning the control socket thread");

    let (cfg_tx, cfg_rx) = async_channel::unbounded::<()>();
    let watch_dir = dir.clone();
    std::thread::Builder::new()
        .name("yz-config".into())
        .spawn(move || {
            let result = async_io::block_on(async {
                let mut watcher = yozora_config::watch::DirWatcher::new(&watch_dir)?;
                loop {
                    let names = watcher.changed().await?;
                    if names.iter().any(|n| n == "notify.toml" || n == "theme.toml") {
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
        toasts::warm_up(cx);
        cx.background_spawn(async { hypr::apply_blur_rule() }).detach();
        cx.set_global(Stack::new(settings.notify.clone(), cmd_tx));

        cx.spawn(async move |cx| {
            while let Ok(event) = ui_rx.recv().await {
                cx.update(|cx| match event {
                    UiEvent::Show(n) => Stack::show(*n, cx),
                    UiEvent::Close(id) => Stack::close(id, cx),
                    UiEvent::CloseAll => Stack::close_all(cx),
                    UiEvent::ExpireAll => Stack::expire_all(cx),
                    UiEvent::Quit => cx.quit(),
                });
            }
        })
        .detach();

        cx.spawn(async move |cx| {
            while cfg_rx.recv().await.is_ok() {
                let settings = load(&dir);
                let dark = system_dark().await;
                {
                    let mut inner = shared.inner.lock().unwrap_or_else(|e| e.into_inner());
                    inner.store.set_limit(settings.notify.history.limit as usize);
                    inner.config = settings.notify.clone();
                }
                tracing::info!("configuration reloaded");
                cx.update(|cx| {
                    apply_theme(&settings, dark, cx);
                    Stack::set_config(settings.notify, cx);
                });
            }
        })
        .detach();
    });
    ExitCode::SUCCESS
}
