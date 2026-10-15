//! Yozora Lock: the lock screen, through ext-session-lock-v1. If this process dies while
//! locked, Hyprland keeps the session locked; it never unlocks by accident.
//!
//! Yozora Idle starts it (when you're away, on `loginctl lock-session`, before sleep). hypridle
//! works too: `lock_cmd = yozora-lock`.

mod auth;
mod background;
mod pam;
mod services;
mod system;
mod view;

use std::{path::PathBuf, process::ExitCode, time::Duration};

use gpui::{
    App, AppContext, Bounds, DisplayId, Entity, WindowBackgroundAppearance, WindowBounds, WindowKind, WindowOptions,
    layer_shell::*, point, px, size,
};
use gpui_platform::application;
use tracing_subscriber::EnvFilter;
use yozora_config::{lock::LockSettings, paths};
use yozora_ipc::{Request, Response, Server};
use yozora_ui::Theme;

use crate::{
    system::{Status, User},
    view::{Lock, LockView},
};

const SOCKET_ID: &str = "lock";
/// A preview has its own socket, so it never stops a real lock from starting.
const PREVIEW_SOCKET_ID: &str = "lock-preview";

const HELP: &str = "\
yozora-lock: a lock screen for Hyprland, in Yozora's style

Usage:
  yozora-lock [--config DIR]     Lock the session now
  yozora-lock --preview          Show the lock screen without locking (Escape closes it)
  yozora-lock status             Whether the screen is locked, as JSON

Options:
  --config DIR    Read lock.toml and theme.toml from DIR instead of ~/.config/yozora
  --pam-dir DIR   Read the PAM service `yozora-lock` from DIR instead of /etc/pam.d (for testing)
  -h, --help      Show this help
  -V, --version   Show the version

Yozora Idle (yozora-idle) starts it when you're away and before sleep. To use hypridle instead,
set in ~/.config/hypr/hypridle.conf:
  lock_cmd = yozora-lock
  before_sleep_cmd = loginctl lock-session
  inhibit_sleep = 3

A second yozora-lock while locked does nothing, so `lock_cmd` needs no `pidof` check.
";

struct Options {
    preview: bool,
    config: Option<PathBuf>,
    pam_dir: Option<PathBuf>,
}

enum Command {
    Help,
    Version,
    Status,
    Run(Options),
}

fn parse(mut args: impl Iterator<Item = String>) -> Result<Command, String> {
    let mut o = Options { preview: false, config: None, pam_dir: None };
    while let Some(a) = args.next() {
        match a.as_str() {
            "-h" | "--help" | "help" => return Ok(Command::Help),
            "-V" | "--version" => return Ok(Command::Version),
            "status" => return Ok(Command::Status),
            "--preview" => o.preview = true,
            "--config" => o.config = Some(PathBuf::from(args.next().ok_or("--config needs a directory")?)),
            "--pam-dir" => o.pam_dir = Some(PathBuf::from(args.next().ok_or("--pam-dir needs a directory")?)),
            other => return Err(format!("unknown argument {other:?}")),
        }
    }
    Ok(Command::Run(o))
}

fn main() -> ExitCode {
    match parse(std::env::args().skip(1)) {
        Ok(Command::Help) => {
            print!("{HELP}");
            ExitCode::SUCCESS
        }
        Ok(Command::Version) => {
            println!("yozora-lock {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Ok(Command::Status) => {
            let locked = yozora_ipc::is_alive(&paths::socket_path(SOCKET_ID));
            println!("{}", serde_json::json!({ "locked": locked }));
            ExitCode::SUCCESS
        }
        Ok(Command::Run(o)) => run(o),
        Err(e) => {
            eprintln!("yozora-lock: {e}\n\nRun `yozora-lock --help` for usage.");
            ExitCode::from(2)
        }
    }
}

fn load(dir: &std::path::Path) -> LockSettings {
    match yozora_config::lock::load(dir) {
        Ok(loaded) => {
            for w in &loaded.warnings {
                tracing::warn!("config: {w}");
            }
            loaded.value
        }
        Err(e) => {
            tracing::error!("configuration: {e:#}; using defaults");
            LockSettings::default()
        }
    }
}

/// The portal's color scheme: true unless it prefers light.
fn system_dark() -> bool {
    async_io::block_on(async {
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
    })
    .unwrap_or(true)
}

fn run(o: Options) -> ExitCode {
    yozora_ui::gpu::prefer_vulkan();
    yozora_ui::malloc::tune();
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_env("YOZORA_LOG").unwrap_or_else(|_| EnvFilter::new("info")))
        .init();

    if !o.preview {
        // Unwinding would drop the lock windows, and dropping the last one unlocks the session.
        // Aborting leaves the compositor holding the lock, which is the point.
        let default = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            default(info);
            tracing::error!("panic while locked: {info}; aborting so the session stays locked");
            std::process::abort();
        }));
    }

    let socket_path = paths::socket_path(if o.preview { PREVIEW_SOCKET_ID } else { SOCKET_ID });
    let socket = match Server::bind(&socket_path) {
        Ok(s) => s,
        Err(e) => {
            // Already locked: nothing to do, and not an error for hypridle.
            if yozora_ipc::is_alive(&socket_path) {
                eprintln!("yozora-lock: {}", if o.preview { "a preview is already open" } else { "already locked" });
                return ExitCode::SUCCESS;
            }
            eprintln!("yozora-lock: {e:#}");
            return ExitCode::FAILURE;
        }
    };

    // Without a user to check the password against, locking would be forever.
    let user = match User::current() {
        Ok(u) => u,
        Err(e) => {
            eprintln!("yozora-lock: not locking: {e:#}");
            return ExitCode::FAILURE;
        }
    };
    let service = match &o.pam_dir {
        Some(dir) => pam::Service { name: "yozora-lock".into(), confdir: Some(dir.clone()) },
        None => pam::Service::system(),
    };
    tracing::info!("checking passwords with the PAM service {:?}", service.name);

    let dir = o.config.clone().unwrap_or_else(paths::config_dir);
    let settings = load(&dir);
    let dark = system_dark();

    let (snap_tx, snap_rx) = async_channel::unbounded();
    let (media_tx, media_rx) = async_channel::unbounded();
    std::thread::Builder::new()
        .name("yz-services".into())
        .spawn(move || services::run(snap_tx, media_rx))
        .expect("spawning the services thread");

    std::thread::Builder::new()
        .name("yz-ipc".into())
        .spawn(move || {
            let (tx, rx) = async_channel::unbounded();
            async_io::block_on(futures_lite::future::zip(socket.run(tx), async move {
                while let Ok(incoming) = rx.recv().await {
                    let response = reply(&incoming.request);
                    incoming.reply(response);
                }
            }));
        })
        .expect("spawning the control socket thread");

    let preview = o.preview;
    let failed = std::rc::Rc::new(std::cell::Cell::new(false));
    let failed_in_app = failed.clone();
    application().with_assets(yozora_ui::Assets).with_quit_mode(gpui::QuitMode::Explicit).run(move |cx: &mut App| {
        let mut theme = Theme::new(&settings.theme, dark, false);
        theme.font = yozora_ui::theme::resolve_font(cx, &settings.theme.font);
        if cx.reduce_motion() {
            theme.motion.reduced = true;
        }
        cx.set_global(theme);

        let lock = cx.new(|_| Lock::new(settings.lock.clone(), user, service, media_tx, preview));

        cx.spawn({
            let lock = lock.clone();
            async move |cx| {
                while let Ok(snapshot) = snap_rx.recv().await {
                    lock.update(cx, |l, cx| {
                        l.snapshot = snapshot;
                        cx.notify();
                    });
                }
            }
        })
        .detach();

        cx.spawn({
            let lock = lock.clone();
            async move |cx| {
                loop {
                    let status = cx.background_spawn(async { Status::read() }).await;
                    lock.update(cx, |l, cx| {
                        if l.status != status {
                            l.status = status;
                            cx.notify();
                        }
                    });
                    cx.background_executor().timer(Duration::from_secs(5)).await;
                }
            }
        })
        .detach();

        let failed = failed_in_app.clone();
        cx.spawn({
            let lock = lock.clone();
            async move |cx| {
                // Displays show up once the compositor has sent its outputs.
                let mut waited = Duration::ZERO;
                while cx.update(|cx| cx.displays().is_empty()) && waited < Duration::from_secs(3) {
                    cx.background_executor().timer(Duration::from_millis(20)).await;
                    waited += Duration::from_millis(20);
                }
                let opened = cx.update(|cx| open_windows(&lock, cx));
                if opened == 0 {
                    tracing::error!("could not open a lock surface on any monitor");
                    failed.set(true);
                    cx.update(|cx| cx.quit());
                    return;
                }
                if !preview {
                    cx.background_spawn(async { services::set_locked_hint(true) }).detach();
                }
                // Ticks for the clock and waits. Also notices when the compositor ends the lock
                // (another locker, or it refused), which closes every lock window.
                loop {
                    cx.background_executor().timer(Duration::from_millis(500)).await;
                    let gone = cx.update(|cx| {
                        lock.update(cx, |l, cx| l.tick(cx));
                        let l = lock.read(cx);
                        l.leaving.is_none() && cx.windows().is_empty()
                    });
                    if gone {
                        tracing::error!("the compositor ended the lock; is another locker running?");
                        failed.set(true);
                        cx.update(|cx| cx.quit());
                        return;
                    }
                }
            }
        })
        .detach();

        cx.observe_displays({
            let lock = lock.clone();
            move |_, cx| {
                if lock.read(cx).leaving.is_none() {
                    open_windows(&lock, cx);
                }
            }
        })
        .detach();
    });
    if failed.get() { ExitCode::FAILURE } else { ExitCode::SUCCESS }
}

fn reply(request: &Request) -> Response {
    match request.cmd.as_str() {
        "ping" => Response::data(serde_json::json!({ "version": env!("CARGO_PKG_VERSION"), "warnings": [] })),
        "status" => Response::data(serde_json::json!({ "locked": true })),
        other => Response::error(format!("unknown command {other:?}; try status")),
    }
}

/// A display with its Hyprland name, whether it has focus, and its real size.
type Named = (DisplayId, Option<String>, bool, Option<(f32, f32)>);

/// Opens a lock window on every display that doesn't have one. Returns how many there are.
fn open_windows(lock: &Entity<Lock>, cx: &mut App) -> usize {
    let monitors = yozora_ui::hypr::monitors().unwrap_or_default();
    let named: Vec<Named> = cx
        .displays()
        .iter()
        .map(|d| {
            let m = monitors.iter().find(|m| yozora_ui::hypr::display_of(m, cx) == Some(d.id()));
            (d.id(), m.map(|m| m.name.clone()), m.is_some_and(|m| m.focused), m.map(|m| (m.width, m.height)))
        })
        .collect();
    let preview = lock.read(cx).preview;
    for (display, output, focused, logical) in named {
        if lock.read(cx).windows.iter().any(|(d, _)| *d == display) {
            continue;
        }
        // GPUI's display size ignores a rotated monitor (a vertical one would get a landscape
        // window), so ask Hyprland for the real one.
        let bounds = match logical.filter(|(w, h)| *w > 0. && *h > 0.) {
            Some((w, h)) => Bounds { origin: point(px(0.), px(0.)), size: size(px(w), px(h)) },
            None => cx
                .find_display(display)
                .map(|d| d.bounds())
                .unwrap_or(Bounds { origin: point(px(0.), px(0.)), size: size(px(1920.), px(1080.)) }),
        };
        let namespace = if preview { "yozora-lock-preview".to_string() } else { SESSION_LOCK_NAMESPACE.to_string() };
        let view_lock = lock.clone();
        let view_output = output.clone();
        let opened = cx.open_window(
            WindowOptions {
                titlebar: None,
                focus: true,
                window_bounds: Some(WindowBounds::Windowed(Bounds {
                    origin: point(px(0.), px(0.)),
                    size: bounds.size,
                })),
                app_id: Some("yozora-lock".into()),
                window_background: WindowBackgroundAppearance::Opaque,
                display_id: Some(display),
                kind: WindowKind::LayerShell(LayerShellOptions {
                    namespace,
                    layer: Layer::Overlay,
                    anchor: Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT,
                    exclusive_zone: Some(px(-1.)),
                    keyboard_interactivity: KeyboardInteractivity::Exclusive,
                    ..Default::default()
                }),
                ..Default::default()
            },
            move |window, cx| cx.new(|cx| LockView::new(view_lock, display, view_output, window, cx)),
        );
        match opened {
            Ok(handle) => {
                tracing::info!("locked {}", output.as_deref().unwrap_or("a monitor"));
                lock.update(cx, |l, _| {
                    l.windows.push((display, handle.into()));
                    if focused || l.main.is_none() {
                        l.main = Some(display);
                    }
                });
            }
            Err(e) => tracing::error!("could not lock {}: {e:#}", output.as_deref().unwrap_or("a monitor")),
        }
    }
    lock.read(cx).windows.len()
}
