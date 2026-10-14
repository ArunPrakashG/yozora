//! `org.freedesktop.ScreenSaver`: the D-Bus way apps (browsers, players, call apps) ask for the
//! screen to stay awake. Each request is dropped if its app disappears.

use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU32, Ordering},
    },
};

use async_channel::Sender;
use futures_lite::StreamExt;

use crate::Msg;

#[derive(Default)]
pub struct Inhibitors {
    held: Mutex<HashMap<u32, (String, String)>>,
    next: AtomicU32,
}

impl Inhibitors {
    fn add(&self, sender: String, app: String, reason: String) -> u32 {
        let cookie = self.next.fetch_add(1, Ordering::Relaxed) + 1;
        self.held.lock().unwrap_or_else(|e| e.into_inner()).insert(cookie, (sender, format!("{app}: {reason}")));
        cookie
    }

    fn remove(&self, cookie: u32) -> bool {
        self.held.lock().unwrap_or_else(|e| e.into_inner()).remove(&cookie).is_some()
    }

    fn remove_sender(&self, sender: &str) -> bool {
        let mut held = self.held.lock().unwrap_or_else(|e| e.into_inner());
        let before = held.len();
        held.retain(|_, (s, _)| s != sender);
        held.len() != before
    }

    pub fn list(&self) -> Vec<String> {
        let held = self.held.lock().unwrap_or_else(|e| e.into_inner());
        let mut v: Vec<String> = held.values().map(|(_, what)| what.clone()).collect();
        v.sort();
        v
    }

    pub fn count(&self) -> usize {
        self.held.lock().unwrap_or_else(|e| e.into_inner()).len()
    }
}

struct ScreenSaver {
    inhibitors: Arc<Inhibitors>,
    tx: Sender<Msg>,
}

#[zbus::interface(name = "org.freedesktop.ScreenSaver")]
impl ScreenSaver {
    async fn inhibit(
        &self,
        application_name: String,
        reason_for_inhibit: String,
        #[zbus(header)] header: zbus::message::Header<'_>,
    ) -> u32 {
        let sender = header.sender().map(|s| s.to_string()).unwrap_or_default();
        let cookie = self.inhibitors.add(sender, application_name, reason_for_inhibit);
        let _ = self.tx.send(Msg::InhibitorsChanged).await;
        cookie
    }

    async fn un_inhibit(&self, cookie: u32) {
        if self.inhibitors.remove(cookie) {
            let _ = self.tx.send(Msg::InhibitorsChanged).await;
        }
    }

    async fn get_active(&self) -> bool {
        false
    }

    async fn simulate_user_activity(&self) {}
}

/// Serves the interface and drops requests from apps that quit. Returns when the bus ends, or
/// right away if another program (hypridle, say) already owns the name.
pub fn serve(inhibitors: Arc<Inhibitors>, tx: Sender<Msg>) -> anyhow::Result<()> {
    async_io::block_on(async {
        let make = || ScreenSaver { inhibitors: inhibitors.clone(), tx: tx.clone() };
        let conn = zbus::connection::Builder::session()?
            .serve_at("/org/freedesktop/ScreenSaver", make())?
            .serve_at("/ScreenSaver", make())?
            .name("org.freedesktop.ScreenSaver")?
            .build()
            .await
            .map_err(|e| anyhow::anyhow!("can't own org.freedesktop.ScreenSaver (is hypridle running?): {e}"))?;
        let dbus = zbus::fdo::DBusProxy::new(&conn).await?;
        let mut changes = dbus.receive_name_owner_changed().await?;
        while let Some(change) = changes.next().await {
            let args = change.args()?;
            if args.new_owner().is_none() && inhibitors.remove_sender(args.name().as_str()) {
                let _ = tx.send(Msg::InhibitorsChanged).await;
            }
        }
        anyhow::Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_go_when_the_app_does() {
        let i = Inhibitors::default();
        let a = i.add(":1.5".into(), "mpv".into(), "playing".into());
        let b = i.add(":1.9".into(), "firefox".into(), "call".into());
        assert_ne!(a, b);
        assert_eq!(i.count(), 2);
        assert!(i.remove_sender(":1.5"));
        assert!(!i.remove(a));
        assert_eq!(i.list(), vec!["firefox: call".to_string()]);
        assert!(i.remove(b) && i.count() == 0);
    }
}
