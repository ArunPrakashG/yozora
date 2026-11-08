//! Logind: the power buttons and the session's locked hint.

use std::time::{Duration, Instant};

/// A second press on restart or shut down within this time confirms it.
pub const CONFIRM: Duration = Duration::from_secs(4);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Power {
    Suspend,
    Reboot,
    ShutDown,
}

/// Restart and shut down need a second press; this remembers the first.
#[derive(Debug, Default)]
pub struct Confirm(Option<(Power, Instant)>);

/// What a press on a power button did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Press {
    /// The first press of restart or shut down: ask again to confirm.
    Confirming,
    /// The action is going ahead.
    Go,
}

impl Confirm {
    pub fn press(&mut self, action: Power, now: Instant) -> Press {
        let confirmed = self.0.is_some_and(|(a, at)| a == action && now.duration_since(at) <= CONFIRM);
        if action != Power::Suspend && !confirmed {
            self.0 = Some((action, now));
            return Press::Confirming;
        }
        self.0 = None;
        Press::Go
    }

    pub fn is_confirming(&self, action: Power) -> bool {
        self.0.is_some_and(|(a, _)| a == action)
    }

    /// Forgets a press that's too old. Returns whether anything changed.
    pub fn expire(&mut self, now: Instant) -> bool {
        if self.0.is_some_and(|(_, at)| now.duration_since(at) > CONFIRM) {
            self.0 = None;
            return true;
        }
        false
    }
}

async fn logind(method: &str, body: &(bool,), path: &str, iface: &str) -> anyhow::Result<()> {
    let conn = zbus::Connection::system().await?;
    conn.call_method(Some("org.freedesktop.login1"), path, Some(iface), method, body).await?;
    Ok(())
}

/// Suspends, restarts or shuts down through logind. Blocking.
pub fn power(action: Power) -> anyhow::Result<()> {
    let method = match action {
        Power::Suspend => "Suspend",
        Power::Reboot => "Reboot",
        Power::ShutDown => "PowerOff",
    };
    // `interactive = false`: no polkit prompt, which nobody could answer from here.
    async_io::block_on(logind(method, &(false,), "/org/freedesktop/login1", "org.freedesktop.login1.Manager"))
}

/// Tells logind the session is locked, for `loginctl` and anything watching `LockedHint`.
/// Blocking.
pub fn set_locked_hint(locked: bool) {
    let r = async_io::block_on(logind(
        "SetLockedHint",
        &(locked,),
        "/org/freedesktop/login1/session/auto",
        "org.freedesktop.login1.Session",
    ));
    if let Err(e) = r {
        tracing::debug!("SetLockedHint({locked}): {e:#}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restart_needs_a_second_press() {
        let t = Instant::now();
        let mut c = Confirm::default();
        assert_eq!(c.press(Power::Suspend, t), Press::Go);
        assert_eq!(c.press(Power::Reboot, t), Press::Confirming);
        assert!(c.is_confirming(Power::Reboot));
        assert!(!c.is_confirming(Power::ShutDown));
        // A different button starts over.
        assert_eq!(c.press(Power::ShutDown, t), Press::Confirming);
        assert_eq!(c.press(Power::ShutDown, t + Duration::from_secs(1)), Press::Go);
        assert!(!c.is_confirming(Power::ShutDown));
    }

    #[test]
    fn a_stale_press_does_not_confirm() {
        let t = Instant::now();
        let mut c = Confirm::default();
        c.press(Power::Reboot, t);
        let late = t + CONFIRM + Duration::from_secs(1);
        assert!(c.expire(late));
        assert_eq!(c.press(Power::Reboot, late), Press::Confirming);
        assert_eq!(c.press(Power::Reboot, late + CONFIRM + Duration::from_secs(1)), Press::Confirming);
    }
}
