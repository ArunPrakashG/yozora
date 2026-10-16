//! System integrations. Each service runs on its own thread and publishes whole snapshots to a
//! GPUI entity, so views read plain data and never block on I/O.
//!
//! Services start on first use. Every service that talks to hardware has a fake backend for
//! machines without that hardware, chosen with `--fake`.

pub mod audio;
pub mod bluetooth;
pub mod brightness;
pub mod custom;
pub mod dbus;
pub mod hyprland;
pub mod media;
pub mod modules;
pub mod network;
pub mod notifications;
pub mod power;
pub mod tray;

use std::{collections::BTreeSet, time::Duration};

use async_channel::{Receiver, Sender};
use gpui::{App, AppContext, Entity, Global};
/// Icon theme lookup, shared with the other Yozora apps.
pub use yozora_ui::icon_theme as icons;

/// A service's latest snapshot plus the channel for commands to its backend.
pub struct Service<S, C> {
    pub state: S,
    /// Set while the backend can't reach its system service. The last good state is kept.
    pub error: Option<String>,
    /// False until the first snapshot arrives.
    pub ready: bool,
    pub fake: bool,
    tx: Sender<C>,
}

impl<S, C> Service<S, C> {
    pub fn send(&self, cmd: C) {
        if self.tx.try_send(cmd).is_err() {
            tracing::debug!("service command dropped: backend is busy or gone");
        }
    }

    /// Whether there is anything to show: a snapshot and no error.
    pub fn up(&self) -> bool {
        self.ready && self.error.is_none()
    }
}

pub enum Update<S> {
    State(S),
    Error(String),
}

/// The backend's end of a service.
pub struct Link<S, C> {
    pub cmds: Receiver<C>,
    out: Sender<Update<S>>,
}

impl<S, C> Link<S, C> {
    /// Publishes a snapshot. Returns false once the app has gone away.
    pub fn publish(&self, state: S) -> bool {
        self.out.send_blocking(Update::State(state)).is_ok()
    }

    pub fn fail(&self, error: impl Into<String>) -> bool {
        self.out.send_blocking(Update::Error(error.into())).is_ok()
    }

    pub fn closed(&self) -> bool {
        self.out.is_closed()
    }
}

/// Starts `backend` on a named thread and returns the entity it feeds.
pub fn spawn<S, C>(
    cx: &mut App,
    name: &'static str,
    fake: bool,
    backend: impl FnOnce(Link<S, C>) + Send + 'static,
) -> Entity<Service<S, C>>
where
    S: Default + Send + 'static,
    C: Send + 'static,
{
    let (tx, cmds) = async_channel::bounded(64);
    let (out, updates) = async_channel::unbounded();
    let entity = cx.new(|_| Service { state: S::default(), error: None, ready: false, fake, tx });
    std::thread::Builder::new()
        .name(format!("yz-{name}"))
        .spawn(move || backend(Link { cmds, out }))
        .expect("spawning a service thread");

    let weak = entity.downgrade();
    cx.spawn(async move |cx| {
        while let Ok(update) = updates.recv().await {
            let Some(entity) = weak.upgrade() else { break };
            entity.update(cx, |service, cx| {
                match update {
                    Update::State(state) => {
                        service.state = state;
                        service.ready = true;
                        service.error = None;
                    }
                    Update::Error(error) => {
                        if service.error.as_ref() != Some(&error) {
                            tracing::warn!("{name}: {error}");
                        }
                        service.error = Some(error);
                    }
                }
                cx.notify();
            });
        }
    })
    .detach();
    entity
}

/// Runs an async backend, restarting it with backoff when it fails, until the app goes away.
pub fn supervise<S, C>(link: Link<S, C>, mut run: impl AsyncFnMut(&Link<S, C>) -> anyhow::Result<()>) {
    let mut delay = Duration::from_secs(1);
    loop {
        match async_io::block_on(run(&link)) {
            Ok(()) => return,
            Err(error) => {
                if !link.fail(format!("{error:#}")) {
                    return;
                }
            }
        }
        // Drop commands that arrive while the system service is down.
        let deadline = std::time::Instant::now() + delay;
        while std::time::Instant::now() < deadline {
            if link.closed() {
                return;
            }
            std::thread::sleep(Duration::from_millis(250));
            while link.cmds.try_recv().is_ok() {}
        }
        delay = (delay * 2).min(Duration::from_secs(30));
    }
}

/// Things a module can depend on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    Audio,
    Bluetooth,
    Custom,
    Hyprland,
    Media,
    Network,
    Notifications,
    Power,
    Tray,
}

/// Which services use fake backends.
#[derive(Clone, Debug, Default)]
pub struct Fakes(pub BTreeSet<String>);

impl Fakes {
    /// `--fake` with no list fakes the hardware the brief calls out: Wi-Fi, Bluetooth, battery.
    pub const DEFAULT: &[&str] = &["network", "bluetooth", "power"];
    pub const ALL: &[&str] = &["network", "bluetooth", "power", "audio", "media", "notifications"];

    pub fn has(&self, name: &str) -> bool {
        self.0.contains(name)
    }
}

pub type Audio = Service<audio::State, audio::Cmd>;
pub type Bluetooth = Service<bluetooth::State, bluetooth::Cmd>;
pub type Custom = Service<custom::State, custom::Cmd>;
pub type Hyprland = Service<hyprland::State, hyprland::Cmd>;
pub type Media = Service<media::State, media::Cmd>;
pub type Network = Service<network::State, network::Cmd>;
pub type Notifications = Service<notifications::State, notifications::Cmd>;
pub type Power = Service<power::State, power::Cmd>;
pub type Tray = Service<tray::State, tray::Cmd>;

#[derive(Default)]
pub struct Services {
    pub fakes: Fakes,
    pub audio: Option<Entity<Audio>>,
    pub bluetooth: Option<Entity<Bluetooth>>,
    pub custom: Option<Entity<Custom>>,
    pub hyprland: Option<Entity<Hyprland>>,
    pub media: Option<Entity<Media>>,
    pub network: Option<Entity<Network>>,
    pub notifications: Option<Entity<Notifications>>,
    pub power: Option<Entity<Power>>,
    pub tray: Option<Entity<Tray>>,
}

impl Global for Services {}

impl Services {
    pub fn get(cx: &App) -> &Self {
        cx.global::<Self>()
    }

    /// Starts any service in `kinds` that isn't running yet. Running services are kept: they
    /// are cheap when idle, and restarting them would drop state like the notification history.
    pub fn ensure(
        kinds: impl IntoIterator<Item = Kind>,
        customs: &std::collections::BTreeMap<String, yozora_config::bar::CustomConfig>,
        cx: &mut App,
    ) {
        let fakes = cx.global::<Self>().fakes.clone();
        for kind in kinds {
            let s = cx.global::<Self>();
            let missing = match kind {
                Kind::Audio => s.audio.is_none(),
                Kind::Bluetooth => s.bluetooth.is_none(),
                Kind::Custom => s.custom.is_none(),
                Kind::Hyprland => s.hyprland.is_none(),
                Kind::Media => s.media.is_none(),
                Kind::Network => s.network.is_none(),
                Kind::Notifications => s.notifications.is_none(),
                Kind::Power => s.power.is_none(),
                Kind::Tray => s.tray.is_none(),
            };
            if kind == Kind::Custom
                && let Some(custom) = cx.global::<Self>().custom.clone()
            {
                custom.read(cx).send(custom::Cmd::Configure(customs.clone()));
                continue;
            }
            if !missing {
                continue;
            }
            tracing::debug!(?kind, "starting service");
            match kind {
                Kind::Audio => {
                    let e = audio::start(cx, fakes.has("audio"));
                    cx.global_mut::<Self>().audio = Some(e);
                }
                Kind::Bluetooth => {
                    let e = bluetooth::start(cx, fakes.has("bluetooth"));
                    cx.global_mut::<Self>().bluetooth = Some(e);
                }
                Kind::Custom => {
                    let e = custom::start(cx, customs.clone());
                    cx.global_mut::<Self>().custom = Some(e);
                }
                Kind::Hyprland => {
                    let e = hyprland::start(cx);
                    cx.global_mut::<Self>().hyprland = Some(e);
                }
                Kind::Media => {
                    let e = media::start(cx, fakes.has("media"));
                    cx.global_mut::<Self>().media = Some(e);
                }
                Kind::Network => {
                    let e = network::start(cx, fakes.has("network"));
                    cx.global_mut::<Self>().network = Some(e);
                }
                Kind::Notifications => {
                    let e = notifications::start(cx, fakes.has("notifications"));
                    cx.global_mut::<Self>().notifications = Some(e);
                }
                Kind::Power => {
                    let e = power::start(cx, fakes.has("power"));
                    cx.global_mut::<Self>().power = Some(e);
                }
                Kind::Tray => {
                    let e = tray::start(cx);
                    cx.global_mut::<Self>().tray = Some(e);
                }
            }
        }
    }
}

/// Runs a shell command detached, for user-configured actions like `lock` or `on_click`.
pub fn run_shell(command: &str) {
    if command.trim().is_empty() {
        return;
    }
    tracing::info!("running: {command}");
    let spawned = yozora_ui::gpu::child(&mut std::process::Command::new("sh"))
        .arg("-c")
        .arg(command)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .spawn();
    match spawned {
        // Reap it on a thread so it doesn't linger as a zombie.
        Ok(mut child) => {
            std::thread::spawn(move || {
                let _ = child.wait();
            });
        }
        Err(e) => tracing::warn!("could not run {command:?}: {e}"),
    }
}
