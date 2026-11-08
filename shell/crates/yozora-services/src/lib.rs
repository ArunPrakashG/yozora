//! System integrations. Each service runs on its own thread and publishes whole snapshots, so a
//! view reads plain data and never blocks on I/O. The shell registers one wake-up with
//! [`set_waker`]; every published snapshot calls it.
//!
//! Services start on first use. Every service that talks to hardware has a fake backend for
//! machines without that hardware, chosen with `--fake`.

pub mod audio;
pub mod bluetooth;
pub mod brightness;
pub mod custom;
pub mod dbus;
pub mod hyprland;
pub mod icon_theme;
pub mod media;
pub mod modules;
pub mod network;
pub mod notifications;
pub mod power;
pub mod tray;

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, Mutex, MutexGuard, OnceLock, Weak},
    time::Duration,
};

use async_channel::{Receiver, Sender};
/// Icon theme lookup.
pub use icon_theme as icons;

type Waker = Box<dyn Fn() + Send + Sync>;
static WAKER: OnceLock<Waker> = OnceLock::new();

/// Called from a service thread whenever a snapshot changes, so the UI can redraw.
pub fn set_waker(f: impl Fn() + Send + Sync + 'static) {
    let _ = WAKER.set(Box::new(f));
}

fn wake() {
    if let Some(f) = WAKER.get() {
        f();
    }
}

/// A service's latest snapshot.
pub struct Snapshot<S> {
    pub state: S,
    /// Set while the backend can't reach its system service. The last good state is kept.
    pub error: Option<String>,
    /// False until the first snapshot arrives.
    pub ready: bool,
}

/// A handle to a running service: read its snapshot, send it commands. Cheap to clone.
pub struct Service<S, C> {
    inner: Arc<Mutex<Snapshot<S>>>,
    pub fake: bool,
    tx: Sender<C>,
}

impl<S, C> Clone for Service<S, C> {
    fn clone(&self) -> Self {
        Service { inner: self.inner.clone(), fake: self.fake, tx: self.tx.clone() }
    }
}

impl<S, C> Service<S, C> {
    pub fn read(&self) -> MutexGuard<'_, Snapshot<S>> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn send(&self, cmd: C) {
        if self.tx.try_send(cmd).is_err() {
            tracing::debug!("service command dropped: backend is busy or gone");
        }
    }

    /// Whether there is anything to show: a snapshot and no error.
    pub fn up(&self) -> bool {
        let s = self.read();
        s.ready && s.error.is_none()
    }
}

/// The backend's end of a service.
pub struct Link<S, C> {
    pub cmds: Receiver<C>,
    inner: Weak<Mutex<Snapshot<S>>>,
}

impl<S, C> Link<S, C> {
    /// Publishes a snapshot. Returns false once nothing holds the service any more.
    pub fn publish(&self, state: S) -> bool {
        let Some(inner) = self.inner.upgrade() else { return false };
        {
            let mut s = inner.lock().unwrap_or_else(|e| e.into_inner());
            s.state = state;
            s.ready = true;
            s.error = None;
        }
        wake();
        true
    }

    pub fn fail(&self, error: impl Into<String>) -> bool {
        let Some(inner) = self.inner.upgrade() else { return false };
        let error = error.into();
        {
            let mut s = inner.lock().unwrap_or_else(|e| e.into_inner());
            if s.error.as_ref() != Some(&error) {
                tracing::warn!("service: {error}");
            }
            s.error = Some(error);
        }
        wake();
        true
    }

    pub fn closed(&self) -> bool {
        self.inner.strong_count() == 0
    }
}

/// Starts `backend` on a named thread and returns the handle it feeds.
pub fn spawn<S, C>(name: &'static str, fake: bool, backend: impl FnOnce(Link<S, C>) + Send + 'static) -> Service<S, C>
where
    S: Default + Send + 'static,
    C: Send + 'static,
{
    let (tx, cmds) = async_channel::bounded(64);
    let inner = Arc::new(Mutex::new(Snapshot { state: S::default(), error: None, ready: false }));
    let link = Link { cmds, inner: Arc::downgrade(&inner) };
    std::thread::Builder::new().name(format!("yz-{name}")).spawn(move || backend(link)).expect("spawning a service thread");
    Service { inner, fake, tx }
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

/// Things a view can depend on.
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
    pub const ALL: &[&str] = &["network", "bluetooth", "power", "audio", "media", "notifications", "tray"];

    pub fn has(&self, name: &str) -> bool {
        self.0.contains(name)
    }
}

/// Runs the `module/<name>` modules and keeps what each last reported.
pub struct ModuleHost {
    inner: Arc<Mutex<ModuleInner>>,
    events: Sender<(String, modules::Event)>,
}

#[derive(Default)]
struct ModuleInner {
    states: BTreeMap<String, modules::ModuleState>,
    handles: BTreeMap<String, modules::Handle>,
}

impl ModuleHost {
    pub fn new() -> ModuleHost {
        let (events, rx) = async_channel::unbounded::<(String, modules::Event)>();
        let inner = Arc::new(Mutex::new(ModuleInner::default()));
        let weak = Arc::downgrade(&inner);
        std::thread::Builder::new()
            .name("yz-modules".into())
            .spawn(move || {
                while let Ok((name, event)) = rx.recv_blocking() {
                    let Some(inner) = weak.upgrade() else { break };
                    {
                        let mut host = inner.lock().unwrap_or_else(|e| e.into_inner());
                        // A stopped module's last events can still arrive; they are stale.
                        if host.handles.contains_key(&name) {
                            host.states.entry(name).or_default().apply(&event);
                        }
                    }
                    wake();
                }
            })
            .expect("spawning the module thread");
        ModuleHost { inner, events }
    }

    pub fn states(&self) -> BTreeMap<String, modules::ModuleState> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner()).states.clone()
    }

    pub fn send(&self, name: &str, f: impl FnOnce(&modules::Handle)) {
        if let Some(h) = self.inner.lock().unwrap_or_else(|e| e.into_inner()).handles.get(name) {
            f(h);
        }
    }

    /// Stops what is no longer wanted and starts what is new. A name that isn't installed gets
    /// a "module not found" state and is looked for again on the next reload.
    pub fn configure(&self, wanted: &BTreeSet<String>) {
        let mut host = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        host.handles.retain(|name, h| {
            let keep = wanted.contains(name);
            if !keep {
                h.stop();
            }
            keep
        });
        host.states.retain(|name, _| wanted.contains(name));
        let fresh: Vec<&String> = wanted.iter().filter(|n| !host.handles.contains_key(*n)).collect();
        if fresh.is_empty() {
            return;
        }
        let (mut found, bad) = modules::discover(&modules::default_dirs());
        for (path, why) in bad {
            tracing::warn!("module {}: {why}", path.display());
        }
        for name in fresh {
            match found.iter().position(|d| &d.manifest.name == name) {
                Some(i) => {
                    let def = found.swap_remove(i);
                    host.states.insert(name.clone(), modules::ModuleState::default());
                    host.handles.insert(name.clone(), modules::spawn(def, self.events.clone()));
                }
                None => {
                    tracing::warn!("module/{name}: not installed");
                    let state = modules::ModuleState { last_good: None, error: Some("module not found".into()) };
                    host.states.insert(name.clone(), state);
                }
            }
        }
    }
}

impl Default for ModuleHost {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for ModuleHost {
    fn drop(&mut self) {
        for h in self.inner.lock().unwrap_or_else(|e| e.into_inner()).handles.values() {
            h.stop();
        }
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
    pub audio: Option<Audio>,
    pub bluetooth: Option<Bluetooth>,
    pub custom: Option<Custom>,
    pub hyprland: Option<Hyprland>,
    pub media: Option<Media>,
    pub modules: Option<ModuleHost>,
    pub network: Option<Network>,
    pub notifications: Option<Notifications>,
    pub power: Option<Power>,
    pub tray: Option<Tray>,
}

impl Services {
    pub fn new(fakes: Fakes) -> Services {
        Services { fakes, ..Default::default() }
    }

    /// Starts the module host if any `module/<name>` is placed, and tells it which ones.
    pub fn ensure_modules(&mut self, wanted: &BTreeSet<String>) {
        if self.modules.is_none() && wanted.is_empty() {
            return;
        }
        self.modules.get_or_insert_with(ModuleHost::new).configure(wanted);
    }

    /// Starts any service in `kinds` that isn't running yet. Running services are kept: they
    /// are cheap when idle, and restarting them would drop state like the notification history.
    pub fn ensure(&mut self, kinds: impl IntoIterator<Item = Kind>, customs: &BTreeMap<String, yozora_config::bar::CustomConfig>) {
        for kind in kinds {
            if kind == Kind::Custom
                && let Some(custom) = &self.custom
            {
                custom.send(custom::Cmd::Configure(customs.clone()));
                continue;
            }
            let fake = |n: &str| self.fakes.has(n);
            match kind {
                Kind::Audio if self.audio.is_none() => self.audio = Some(audio::start(fake("audio"))),
                Kind::Bluetooth if self.bluetooth.is_none() => self.bluetooth = Some(bluetooth::start(fake("bluetooth"))),
                Kind::Custom if self.custom.is_none() => self.custom = Some(custom::start(customs.clone())),
                Kind::Hyprland if self.hyprland.is_none() => self.hyprland = Some(hyprland::start()),
                Kind::Media if self.media.is_none() => self.media = Some(media::start(fake("media"))),
                Kind::Network if self.network.is_none() => self.network = Some(network::start(fake("network"))),
                Kind::Notifications if self.notifications.is_none() => self.notifications = Some(notifications::start(fake("notifications"))),
                Kind::Power if self.power.is_none() => self.power = Some(power::start(fake("power"))),
                Kind::Tray if self.tray.is_none() => self.tray = Some(tray::start(fake("tray"))),
                _ => {}
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
    let spawned = std::process::Command::new("sh")
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
