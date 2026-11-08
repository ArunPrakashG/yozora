//! Yozora Notify: the notification server. The shell draws the toasts and the notification
//! center from the history this serves.

use std::{
    path::PathBuf,
    process::ExitCode,
    sync::{Arc, Mutex},
};

use tracing_subscriber::EnvFilter;
use yozora_config::{notify::NotifySettings, paths};
use yozora_ipc::{Request, Server};
use yozora_notifyd::{
    hypr,
    server::{self, Cmd, Inner, Shared},
    store::{self, Store},
};

const SOCKET_ID: &str = "notify";

const HELP: &str = "\
yozora-notify: notifications for Hyprland

Usage:
  yozora-notify [--replace] [--config DIR]   Run the notification server
  yozora-notify dnd [on|off|toggle]          Do Not Disturb (toggle by default)
  yozora-notify status                       Do Not Disturb, unread and count, as JSON
  yozora-notify list                         The notification center, as JSON
  yozora-notify clear                        Empty the notification center
  yozora-notify quit                         Stop the running server

Options:
  --replace       Take over from another daemon that allows it, such as an older yozora-notify
  --config DIR    Read notify.toml from DIR instead of ~/.config/yozora
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

fn run(replace: bool, dir: PathBuf) -> ExitCode {
    let filter = EnvFilter::try_from_env("YOZORA_LOG").unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt().with_env_filter(filter).init();

    let settings = load(&dir);
    let socket = match Server::bind(&paths::socket_path(SOCKET_ID)) {
        Ok(s) => s,
        // Started twice, say by systemd while a manual instance runs: not a failure to restart.
        Err(e) if yozora_ipc::already_running(&e) => {
            eprintln!("yozora-notify: {e:#}");
            return ExitCode::SUCCESS;
        }
        Err(e) => {
            eprintln!("yozora-notify: {e:#}");
            return ExitCode::FAILURE;
        }
    };

    let store = Store::load(&store::state_file(), settings.notify.history.limit as usize);
    server::prune_images(&store);
    let (cmd_tx, cmd_rx) = async_channel::unbounded::<Cmd>();
    let shared = Shared { inner: Arc::new(Mutex::new(Inner::new(store, settings.notify.clone()))), cmds: cmd_tx.clone() };

    // Take the bus name before opening anything, so a second daemon exits right away.
    let conn = match async_io::block_on(server::connect(shared.clone(), replace)) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("yozora-notify: {e:#}");
            return ExitCode::FAILURE;
        }
    };
    tracing::info!("serving {} on the session bus", server::NAME);

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

    let share_cmds = cmd_tx.clone();
    std::thread::Builder::new()
        .name("yz-screencast".into())
        .spawn(move || hypr::watch_screencast(|on| share_cmds.send_blocking(Cmd::Sharing(on)).is_ok()))
        .expect("spawning the screen sharing watcher");

    let watch_dir = dir.clone();
    std::thread::Builder::new()
        .name("yz-config".into())
        .spawn(move || {
            let result = async_io::block_on(async {
                let mut watcher = yozora_config::watch::DirWatcher::new(&watch_dir)?;
                loop {
                    let names = watcher.changed().await?;
                    if names.iter().any(|n| n == "notify.toml") {
                        let settings = load(&watch_dir);
                        tracing::info!("configuration reloaded");
                        cmd_tx.send(Cmd::Config(settings.notify)).await?;
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

    let result = async_io::block_on(server::serve(conn, shared, cmd_rx));
    // Let the reply to `quit` reach its client before the process goes.
    std::thread::sleep(std::time::Duration::from_millis(100));
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            tracing::error!("notification server: {e:#}");
            ExitCode::FAILURE
        }
    }
}
