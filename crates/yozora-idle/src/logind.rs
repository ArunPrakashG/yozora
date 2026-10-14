//! logind: lock requests, the warning before sleep, holding sleep until the screen is locked,
//! and suspending.

use std::time::{Duration, Instant};

use async_channel::Sender;
use futures_lite::StreamExt;
use zbus::{Connection, Proxy, zvariant::OwnedFd};

use crate::Msg;

const DEST: &str = "org.freedesktop.login1";
const MANAGER_PATH: &str = "/org/freedesktop/login1";
const MANAGER: &str = "org.freedesktop.login1.Manager";
const SESSION: &str = "org.freedesktop.login1.Session";

#[derive(Clone)]
pub struct Logind {
    conn: Connection,
    session_path: String,
}

impl Logind {
    pub fn connect() -> anyhow::Result<Self> {
        async_io::block_on(async {
            let conn = Connection::system().await?;
            // "auto" resolves to the caller's session for method calls, but signals come from the
            // real object, so look that up.
            let auto = Proxy::new(&conn, DEST, "/org/freedesktop/login1/session/auto", SESSION).await?;
            let id: String = auto.get_property("Id").await?;
            let manager = Proxy::new(&conn, DEST, MANAGER_PATH, MANAGER).await?;
            let path: zbus::zvariant::OwnedObjectPath = manager.call("GetSession", &(id,)).await?;
            anyhow::Ok(Self { conn, session_path: path.as_str().to_string() })
        })
    }

    /// Feeds `Lock` requests and `PrepareForSleep` into `tx`, on their own threads.
    pub fn watch(&self, tx: Sender<Msg>) {
        let (me, sender) = (self.clone(), tx.clone());
        std::thread::Builder::new()
            .name("yz-lock-signal".into())
            .spawn(move || {
                let r = async_io::block_on(async {
                    let p = Proxy::new(&me.conn, DEST, me.session_path.clone(), SESSION).await?;
                    let mut s = p.receive_signal("Lock").await?;
                    while s.next().await.is_some() {
                        sender.send(Msg::LockRequested).await?;
                    }
                    anyhow::Ok(())
                });
                tracing::warn!("lock requests stopped: {r:?}");
            })
            .expect("spawning the lock signal thread");
        let me = self.clone();
        std::thread::Builder::new()
            .name("yz-sleep-signal".into())
            .spawn(move || {
                let r = async_io::block_on(async {
                    let p = Proxy::new(&me.conn, DEST, MANAGER_PATH, MANAGER).await?;
                    let mut s = p.receive_signal("PrepareForSleep").await?;
                    while let Some(m) = s.next().await {
                        let going: bool = m.body().deserialize()?;
                        tx.send(Msg::Sleep(going)).await?;
                    }
                    anyhow::Ok(())
                });
                tracing::warn!("sleep signal stopped: {r:?}");
            })
            .expect("spawning the sleep signal thread");
    }

    /// A delay inhibitor: logind holds off suspending until it is dropped (or 5 s pass).
    pub fn delay_sleep(&self) -> anyhow::Result<OwnedFd> {
        async_io::block_on(async {
            let p = Proxy::new(&self.conn, DEST, MANAGER_PATH, MANAGER).await?;
            let fd: OwnedFd = p
                .call("Inhibit", &("sleep", "Yozora Idle", "Locks the screen before the computer sleeps", "delay"))
                .await?;
            anyhow::Ok(fd)
        })
    }

    pub fn suspend(&self) -> anyhow::Result<()> {
        async_io::block_on(async {
            let p = Proxy::new(&self.conn, DEST, MANAGER_PATH, MANAGER).await?;
            // `false`: no polkit prompt, which nobody could answer.
            p.call_method("Suspend", &(false,)).await?;
            anyhow::Ok(())
        })
    }

    /// Whether the lock screen has told logind it is up. `yozora-lock` sets it once its windows
    /// are open.
    pub fn locked(&self) -> bool {
        async_io::block_on(async {
            let p = Proxy::new(&self.conn, DEST, self.session_path.clone(), SESSION).await?;
            anyhow::Ok(p.get_property::<bool>("LockedHint").await?)
        })
        .unwrap_or(false)
    }

    /// Waits until the screen is locked, up to `limit`. Returns whether it is.
    pub fn wait_locked(&self, limit: Duration) -> bool {
        let start = Instant::now();
        loop {
            if self.locked() {
                return true;
            }
            if start.elapsed() >= limit {
                return false;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}
