//! Yozora Idle: locks, turns the screens off and suspends when you're away. It also locks when
//! asked (`loginctl lock-session`) and before the computer sleeps, holding the sleep until the
//! screen is locked. It replaces hypridle.
//!
//! This only decides *when*. The lock screen is Yozora Lock.

mod hypr;
mod logind;
mod saver;
mod wl;

use std::{
    collections::HashSet,
    os::unix::process::CommandExt,
    path::PathBuf,
    process::{Command, ExitCode, Stdio},
    sync::Arc,
    time::Duration,
};

use serde_json::json;
use tracing_subscriber::EnvFilter;
use yozora_config::{idle::IdleConfig, paths};
use yozora_ipc::{Incoming, Request, Response, Server};

use crate::{logind::Logind, saver::Inhibitors};

const SOCKET_ID: &str = "idle";
/// How long to hold a sleep for the lock screen to come up. logind gives up on us at 5 s anyway.
const LOCK_WAIT: Duration = Duration::from_secs(5);

const HELP: &str = "\
yozora-idle: lock, turn the screens off and suspend when you're away

Usage:
  yozora-idle [--config DIR]   Run (normally started by the systemd user unit)
  yozora-idle status           What it's doing now, as JSON

Options:
  --config DIR   Read idle.toml from DIR instead of ~/.config/yozora
  -h, --help     Show this help
  -V, --version  Show the version

Times are set in idle.toml or Yozora Settings. It replaces hypridle: stop hypridle first, or
both will lock.
";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Stage {
    Lock,
    ScreenOff,
    Suspend,
}

pub enum Msg {
    Idled(Stage),
    Resumed(Stage),
    InhibitorsChanged,
    /// `loginctl lock-session` or similar.
    LockRequested,
    /// logind's PrepareForSleep: true as the computer is about to sleep, false after waking.
    Sleep(bool),
    Ipc(Incoming),
    Reload,
    WaylandFailed(String),
}

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let mut config = None;
    let mut status = false;
    while let Some(a) = args.next() {
        match a.as_str() {
            "-h" | "--help" | "help" => {
                print!("{HELP}");
                return ExitCode::SUCCESS;
            }
            "-V" | "--version" => {
                println!("yozora-idle {}", env!("CARGO_PKG_VERSION"));
                return ExitCode::SUCCESS;
            }
            "status" => status = true,
            "--config" => match args.next() {
                Some(d) => config = Some(PathBuf::from(d)),
                None => {
                    eprintln!("yozora-idle: --config needs a directory");
                    return ExitCode::from(2);
                }
            },
            other => {
                eprintln!("yozora-idle: unknown argument {other:?}\n\nRun `yozora-idle --help` for usage.");
                return ExitCode::from(2);
            }
        }
    }
    if status {
        return match yozora_ipc::request(&paths::socket_path(SOCKET_ID), &Request::new("status", Vec::<String>::new()))
        {
            Ok(r) if r.ok => {
                println!("{}", r.data);
                ExitCode::SUCCESS
            }
            Ok(r) => {
                eprintln!("yozora-idle: {}", r.error.unwrap_or_default());
                ExitCode::FAILURE
            }
            Err(_) => {
                eprintln!("yozora-idle: not running");
                ExitCode::FAILURE
            }
        };
    }
    match run(config.unwrap_or_else(paths::config_dir)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("yozora-idle: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn load(dir: &std::path::Path) -> IdleConfig {
    match yozora_config::idle::load(dir) {
        Ok(l) => {
            for w in &l.warnings {
                tracing::warn!("config: {w}");
            }
            l.value
        }
        Err(e) => {
            tracing::error!("configuration: {e:#}; using defaults");
            IdleConfig::default()
        }
    }
}

fn run(dir: PathBuf) -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_env("YOZORA_LOG").unwrap_or_else(|_| EnvFilter::new("info")))
        .init();
    let socket = Server::bind(&paths::socket_path(SOCKET_ID))?;
    let cfg = load(&dir);
    let (tx, rx) = async_channel::unbounded::<Msg>();

    let logind = match Logind::connect() {
        Ok(l) => {
            l.watch(tx.clone());
            Some(l)
        }
        Err(e) => {
            tracing::error!("logind: {e:#}; lock requests and locking before sleep won't work");
            None
        }
    };

    let inhibitors = Arc::new(Inhibitors::default());
    if cfg.inhibit.respect {
        let (inh, tx) = (inhibitors.clone(), tx.clone());
        std::thread::Builder::new().name("yz-screensaver".into()).spawn(move || {
            if let Err(e) = saver::serve(inh, tx) {
                tracing::warn!("{e:#}");
            }
        })?;
    }

    let stages: Vec<(Stage, u32)> = [
        (Stage::Lock, cfg.lock.after_min),
        (Stage::ScreenOff, cfg.screen.off_after_min),
        (Stage::Suspend, cfg.suspend.after_min),
    ]
    .into_iter()
    .filter(|(_, m)| *m > 0)
    .map(|(s, m)| (s, m * 60_000))
    .collect();
    if !stages.is_empty() {
        let tx = tx.clone();
        std::thread::Builder::new().name("yz-idle-notify".into()).spawn(move || {
            if let Err(e) = wl::run(stages, tx.clone()) {
                tracing::error!("idle detection: {e:#}");
                let _ = tx.send_blocking(Msg::WaylandFailed(format!("{e:#}")));
            }
        })?;
    }

    {
        let tx = tx.clone();
        std::thread::Builder::new().name("yz-ipc".into()).spawn(move || {
            let (itx, irx) = async_channel::unbounded();
            async_io::block_on(futures_lite::future::zip(socket.run(itx), async move {
                while let Ok(incoming) = irx.recv().await {
                    if tx.send(Msg::Ipc(incoming)).await.is_err() {
                        break;
                    }
                }
            }));
        })?;
    }

    {
        let (tx, dir) = (tx.clone(), dir.clone());
        std::thread::Builder::new().name("yz-config".into()).spawn(move || {
            let r = async_io::block_on(async {
                let mut w = yozora_config::watch::DirWatcher::new(&dir)?;
                loop {
                    if w.changed().await?.iter().any(|n| n == "idle.toml" || n == "*") {
                        tx.send(Msg::Reload).await?;
                    }
                }
                #[allow(unreachable_code)]
                anyhow::Ok(())
            });
            tracing::debug!("config watcher stopped: {r:?}");
        })?;
    }

    let mut s = Idle {
        cfg,
        logind,
        inhibitors,
        idle: HashSet::new(),
        acted: HashSet::new(),
        keep_awake: false,
        sleep_hold: None,
        wayland_error: None,
    };
    s.hold_sleep();
    tracing::info!(
        "idle: lock {} min, screens off {} min, suspend {} min (0 is never)",
        s.cfg.lock.after_min,
        s.cfg.screen.off_after_min,
        s.cfg.suspend.after_min
    );
    while let Ok(msg) = rx.recv_blocking() {
        match msg {
            Msg::Idled(stage) => {
                s.idle.insert(stage);
                s.act(stage);
            }
            Msg::Resumed(stage) => {
                s.idle.remove(&stage);
                if s.acted.remove(&stage) && stage == Stage::ScreenOff {
                    hypr::dpms(true);
                }
            }
            Msg::InhibitorsChanged => s.retry(),
            Msg::LockRequested => s.lock(),
            Msg::Sleep(true) => s.before_sleep(),
            Msg::Sleep(false) => s.hold_sleep(),
            Msg::Ipc(incoming) => {
                let response = s.ipc(&incoming.request);
                incoming.reply(response);
            }
            Msg::Reload => {
                tracing::info!("idle.toml changed; restarting to apply it");
                if s.acted.contains(&Stage::ScreenOff) {
                    hypr::dpms(true);
                }
                // Idle timeouts are fixed when they're created, so start over in place.
                let err = Command::new(std::env::current_exe()?).args(std::env::args().skip(1)).exec();
                anyhow::bail!("restarting: {err}");
            }
            Msg::WaylandFailed(e) => s.wayland_error = Some(e),
        }
    }
    Ok(())
}

struct Idle {
    cfg: IdleConfig,
    logind: Option<Logind>,
    inhibitors: Arc<Inhibitors>,
    /// Stages the compositor says we're idle for.
    idle: HashSet<Stage>,
    /// Stages we already acted on this idle period.
    acted: HashSet<Stage>,
    keep_awake: bool,
    /// The delay inhibitor that holds a sleep until the screen is locked.
    sleep_hold: Option<zbus::zvariant::OwnedFd>,
    wayland_error: Option<String>,
}

impl Idle {
    /// Idle time is ignored while kept awake, or while an app asks for the screen to stay on.
    fn blocked(&self) -> bool {
        self.keep_awake || (self.cfg.inhibit.respect && self.inhibitors.count() > 0)
    }

    fn act(&mut self, stage: Stage) {
        if self.blocked() || !self.acted.insert(stage) {
            return;
        }
        tracing::info!("idle: {stage:?}");
        match stage {
            Stage::Lock => self.lock(),
            Stage::ScreenOff => hypr::dpms(false),
            Stage::Suspend => match &self.logind {
                Some(l) => {
                    if let Err(e) = l.suspend() {
                        tracing::warn!("suspend: {e:#}");
                    }
                }
                None => tracing::warn!("can't suspend without logind"),
            },
        }
    }

    /// Acts on stages that came due while something was holding the screen awake.
    fn retry(&mut self) {
        for stage in self.idle.clone() {
            self.act(stage);
        }
    }

    /// Starts the lock screen. A second one while locked does nothing.
    fn lock(&self) {
        match Command::new("sh")
            .arg("-c")
            .arg(&self.cfg.lock.command)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0)
            .spawn()
        {
            // Reaping keeps a finished lock screen from lingering as a zombie.
            Ok(mut child) => {
                std::thread::spawn(move || {
                    let _ = child.wait();
                });
            }
            Err(e) => tracing::error!("locking with {:?}: {e}", self.cfg.lock.command),
        }
    }

    fn hold_sleep(&mut self) {
        if !self.cfg.suspend.lock_first || self.sleep_hold.is_some() {
            return;
        }
        if let Some(l) = &self.logind {
            match l.delay_sleep() {
                Ok(fd) => self.sleep_hold = Some(fd),
                Err(e) => tracing::warn!("holding sleep for the lock: {e:#}"),
            }
        }
    }

    /// The computer is about to sleep: lock first, and let it sleep once the screen is up.
    fn before_sleep(&mut self) {
        if !self.cfg.suspend.lock_first {
            return;
        }
        self.lock();
        if let Some(l) = &self.logind
            && !l.wait_locked(LOCK_WAIT)
        {
            tracing::warn!(
                "the screen wasn't locked after {} s; letting the computer sleep anyway",
                LOCK_WAIT.as_secs()
            );
        }
        self.sleep_hold = None;
    }

    fn ipc(&mut self, request: &Request) -> Response {
        match request.cmd.as_str() {
            "ping" => Response::data(json!({ "version": env!("CARGO_PKG_VERSION"), "warnings": [] })),
            "status" => {
                let mut stages: Vec<String> = self.idle.iter().map(|s| format!("{s:?}").to_lowercase()).collect();
                stages.sort();
                Response::data(json!({
                    "locked": self.logind.as_ref().is_some_and(Logind::locked),
                    "keep_awake": self.keep_awake,
                    "held_awake_by": self.inhibitors.list(),
                    "idle_for": stages,
                    "lock_after_min": self.cfg.lock.after_min,
                    "screen_off_after_min": self.cfg.screen.off_after_min,
                    "suspend_after_min": self.cfg.suspend.after_min,
                    "lock_before_sleep": self.cfg.suspend.lock_first,
                    "hyprland": hypr::available(),
                    "logind": self.logind.is_some(),
                    "error": self.wayland_error,
                }))
            }
            "keep-awake" => {
                match request.args.first().map(String::as_str) {
                    None | Some("status") => {}
                    Some("on") => self.keep_awake = true,
                    Some("off") => self.keep_awake = false,
                    Some("toggle") => self.keep_awake = !self.keep_awake,
                    Some(other) => {
                        return Response::error(format!("keep-awake takes on, off or toggle, not {other:?}"));
                    }
                }
                self.retry();
                Response::data(json!({ "keep_awake": self.keep_awake }))
            }
            "lock" => {
                self.lock();
                Response::ok()
            }
            other => Response::error(format!("unknown command {other:?}; try status, keep-awake or lock")),
        }
    }
}
