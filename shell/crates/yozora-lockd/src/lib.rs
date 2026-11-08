//! The lock screen's engine, without a UI. A UI draws the screen and feeds keys to [`Engine`];
//! everything else lives here: the password field's state ([`auth`]), PAM ([`pam`]), the user,
//! battery, network and keyboard layout ([`system`]), what's playing and the notification count
//! ([`glance`]), logind ([`power`]), the blurred wallpaper ([`background`]) and the command line
//! and control socket ([`cli`]).
//!
//! The UI owns the windows, the fade when leaving and the clock's pixels. It calls
//! [`Engine::poll`] on a timer (every half second) and whenever the wake-up it passed to
//! [`Engine::start`] fires, then redraws from [`Engine::auth`], [`Engine::status`] and
//! [`Engine::glance`].

pub mod auth;
pub mod background;
pub mod cli;
pub mod glance;
pub mod pam;
pub mod power;
pub mod system;

use std::{
    path::Path,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use zeroize::Zeroizing;

pub use yozora_config::lock::{LockConfig, LockSettings};

pub use crate::{
    auth::{Auth, Phase, Tone, countdown},
    cli::{Command, Options},
    glance::{Glance, Media, MediaCmd},
    pam::{Failure, Service},
    power::{Confirm, Power, Press},
    system::{Monitor, Network, Status, User},
};

/// How long the content fades before the session unlocks.
pub const LEAVE: Duration = Duration::from_millis(160);
/// How often the UI should call [`Engine::poll`].
pub const TICK: Duration = Duration::from_millis(500);

type Wake = Arc<dyn Fn() + Send + Sync>;

/// A password check running on a worker thread.
pub struct Check(async_channel::Receiver<Result<(), Failure>>);

impl Check {
    /// The result, once it's in.
    pub fn try_result(&self) -> Option<Result<(), Failure>> {
        self.0.try_recv().ok()
    }

    /// Waits for the result.
    pub async fn result(self) -> Result<(), Failure> {
        self.0.recv().await.unwrap_or_else(|_| {
            Err(Failure { messages: Vec::new(), max_tries: false, error: "the password check stopped".into() })
        })
    }
}

/// Checks `password` for `login` through PAM on its own thread. `wake` is called when the
/// result is ready. The password is wiped once PAM is done with it.
pub fn check_password(
    service: Service,
    login: String,
    password: Zeroizing<String>,
    wake: impl FnOnce() + Send + 'static,
) -> Check {
    let (tx, rx) = async_channel::bounded(1);
    std::thread::Builder::new()
        .name("yz-pam".into())
        .spawn(move || {
            let _ = tx.send_blocking(pam::authenticate(&service, &login, &password));
            wake();
        })
        .expect("spawning the password check thread");
    Check(rx)
}

/// What a [`Engine::poll`] found.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Update {
    /// Something the screen shows changed: redraw.
    pub changed: bool,
    /// PAM accepted the password. Fade out, then call [`Engine::mark_unlocked`] and close the
    /// lock surfaces. Reported once.
    pub unlocked: bool,
}

/// The state behind every lock window.
pub struct Engine {
    pub auth: Auth,
    pub user: User,
    pub config: LockConfig,
    pub service: Service,
    /// A preview instead of a real lock: no logind calls, and power buttons only log.
    pub preview: bool,
    pub confirm: Confirm,
    wake: Wake,
    status: Arc<Mutex<Status>>,
    sources: glance::Sources,
    check: Option<Check>,
}

impl Engine {
    /// Looks up who is locking and starts the status poll, the media and notification services.
    /// `wake` is called from worker threads whenever there is something new to show or a
    /// password check finished. It fails when the user isn't in the password database, because
    /// locking would then be forever.
    pub fn start(config: LockConfig, o: &Options, wake: impl Fn() + Send + Sync + 'static) -> anyhow::Result<Self> {
        let user = User::current()?;
        let service = match &o.pam_dir {
            Some(dir) => Service { name: "yozora-lock".into(), confdir: Some(dir.clone()) },
            None => Service::system(),
        };
        tracing::info!("checking passwords with the PAM service {:?}", service.name);

        let wake: Wake = Arc::new(wake);
        let waker = wake.clone();
        yozora_services::set_waker(move || waker());
        let sources = glance::Sources::start(config.show.media, config.show.notifications);

        let status = Arc::new(Mutex::new(Status::read()));
        let (shared, wake_poll) = (Arc::downgrade(&status), wake.clone());
        std::thread::Builder::new()
            .name("yz-status".into())
            .spawn(move || {
                // Ends when the engine is dropped.
                while let Some(status) = shared.upgrade() {
                    std::thread::sleep(Duration::from_secs(5));
                    let now = Status::read();
                    let mut current = status.lock().unwrap_or_else(|e| e.into_inner());
                    if *current != now {
                        *current = now;
                        drop(current);
                        wake_poll();
                    }
                }
            })
            .expect("spawning the status thread");

        Ok(Self {
            auth: Auth::new(config.auth.tries, config.auth.wait_s),
            user,
            config,
            service,
            preview: o.preview,
            confirm: Confirm::default(),
            wake,
            status,
            sources,
            check: None,
        })
    }

    /// Battery, network and keyboard layout, refreshed every five seconds.
    pub fn status(&self) -> Status {
        self.status.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// What's playing and the notification count. Empty for what `[show]` turns off.
    pub fn glance(&self) -> Glance {
        self.sources.glance()
    }

    pub fn media(&self, cmd: MediaCmd) {
        self.sources.command(cmd);
    }

    /// Sends the typed password to PAM. Returns whether a check started; the answer comes
    /// through [`Engine::poll`].
    pub fn submit(&mut self) -> bool {
        let Some(password) = self.auth.submit() else { return false };
        let wake = self.wake.clone();
        self.check = Some(check_password(self.service.clone(), self.user.login.clone(), password, move || wake()));
        true
    }

    /// Takes in a finished password check, a wait that's over and an old power confirmation.
    pub fn poll(&mut self, now: Instant) -> Update {
        let mut update = Update { changed: self.auth.tick(now) || self.auth.phase == Phase::Waiting, unlocked: false };
        if self.confirm.expire(now) {
            update.changed = true;
        }
        if let Some(result) = self.check.as_ref().and_then(Check::try_result) {
            self.check = None;
            match result {
                Ok(()) => {
                    tracing::info!("password accepted");
                    self.auth.succeeded();
                    update.unlocked = true;
                }
                Err(failure) => {
                    tracing::warn!("password rejected: {} {:?}", failure.error, failure.messages);
                    self.auth.failed(failure, now);
                }
            }
            update.changed = true;
        }
        update
    }

    /// Presses a power button. Restart and shut down ask for a second press first. A preview
    /// only logs.
    pub fn power(&mut self, action: Power) -> Press {
        let press = self.confirm.press(action, Instant::now());
        if press == Press::Go {
            if self.preview {
                tracing::info!("preview: would {action:?}");
            } else {
                std::thread::spawn(move || {
                    if let Err(e) = power::power(action) {
                        tracing::error!("{action:?}: {e:#}");
                    }
                });
            }
        }
        press
    }

    /// Tells logind the session is locked, once the lock surfaces are up. Does nothing in a
    /// preview.
    pub fn mark_locked(&self) {
        if !self.preview {
            std::thread::spawn(|| power::set_locked_hint(true));
        }
    }

    /// Clears the locked hint when the session unlocks. Blocking, so it's done before the
    /// process goes away. Does nothing in a preview.
    pub fn mark_unlocked(&self) {
        if !self.preview {
            power::set_locked_hint(false);
        }
    }

    /// The prepared wallpaper for `output` under the current settings, or none if there isn't
    /// one. Decodes and blurs on first use, so call it off the UI thread.
    pub fn background(&self, output: Option<&str>) -> Option<std::path::PathBuf> {
        background_for(&self.config, output)
    }

    /// The clock's time and date, formatted by `[clock]`.
    pub fn clock(&self) -> (String, String) {
        clock(&self.config)
    }
}

/// The wallpaper for `output` under `config`, blurred and cached.
pub fn background_for(config: &LockConfig, output: Option<&str>) -> Option<std::path::PathBuf> {
    let source = system::wallpaper(&config.background.image, output)?;
    background::prepare(&source, config.background.blur).map_err(|e| tracing::warn!("background: {e:#}")).ok()
}

/// The time and date text for now.
pub fn clock(config: &LockConfig) -> (String, String) {
    let now = chrono::Local::now();
    (now.format(&config.clock.format).to_string(), now.format(&config.clock.date).to_string())
}

/// Loads `lock.toml` and `theme.toml` from `dir`, warning about mistakes and falling back to
/// the defaults when a file is unreadable.
pub fn load_settings(dir: &Path) -> LockSettings {
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

/// The portal's color scheme: true unless it prefers light. Blocking.
pub fn system_dark() -> bool {
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

/// For a real lock: abort on a panic. Unwinding would drop the lock windows, and dropping the
/// last one unlocks the session; aborting leaves the compositor holding the lock.
pub fn abort_on_panic() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        default(info);
        tracing::error!("panic while locked: {info}; aborting so the session stays locked");
        std::process::abort();
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn failure() -> Failure {
        Failure { messages: Vec::new(), max_tries: false, error: "Authentication failure".into() }
    }

    #[test]
    fn a_check_reports_through_its_result() {
        let permit = std::env::temp_dir().join(format!("yozora-lockd-engine-{}", std::process::id()));
        std::fs::create_dir_all(&permit).unwrap();
        std::fs::write(permit.join("allow"), "auth required pam_permit.so\naccount required pam_permit.so\n").unwrap();
        std::fs::write(permit.join("deny"), "auth required pam_deny.so\n").unwrap();
        let service = |name: &str| Service { name: name.into(), confdir: Some(permit.clone()) };
        let pw = || Zeroizing::new("hunter2".to_string());

        let ok = check_password(service("allow"), "nobody".into(), pw(), || {});
        assert_eq!(async_io::block_on(ok.result()), Ok(()));
        let no = check_password(service("deny"), "nobody".into(), pw(), || {});
        assert!(async_io::block_on(no.result()).is_err());
        let _ = std::fs::remove_dir_all(&permit);
    }

    #[test]
    fn the_field_locks_out_through_the_engine_types() {
        let mut auth = Auth::new(1, 30);
        auth.push("x");
        auth.submit();
        auth.failed(failure(), Instant::now());
        assert_eq!(auth.phase, Phase::Waiting);
    }

    #[test]
    fn clock_uses_the_configured_formats() {
        let mut config = LockConfig::default();
        config.clock.format = "%H".into();
        config.clock.date = "x".into();
        let (time, date) = clock(&config);
        assert_eq!(time.len(), 2);
        assert_eq!(date, "x");
    }
}
