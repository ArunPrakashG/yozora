//! D-Bus while locked: what's playing (MPRIS), how many notifications arrived (yozora-notify),
//! and logind for the power buttons and the session's locked hint.
//!
//! A lock screen runs for minutes at a time, so this polls every two seconds rather than
//! keeping a model of every player.

use std::{collections::HashMap, path::PathBuf, time::Duration};

use futures_lite::FutureExt;
use zbus::{
    Connection,
    zvariant::{OwnedValue, Value},
};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Media {
    pub title: String,
    pub artist: String,
    /// Only local artwork; a lock screen doesn't fetch URLs.
    pub art: Option<PathBuf>,
    pub playing: bool,
    bus_name: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Snapshot {
    pub media: Option<Media>,
    /// Notifications that arrived since locking, if yozora-notify is running.
    pub notifications: Option<usize>,
}

#[derive(Clone, Copy, Debug)]
pub enum MediaCmd {
    Previous,
    PlayPause,
    Next,
}

#[derive(Clone, Copy, Debug)]
pub enum Power {
    Suspend,
    Reboot,
    ShutDown,
}

/// Runs until `updates` closes. Blocking; give it a thread.
pub fn run(updates: async_channel::Sender<Snapshot>, cmds: async_channel::Receiver<MediaCmd>) {
    let result = async_io::block_on(async {
        let conn = Connection::session().await?;
        let mut first_id = None;
        let mut last = None;
        loop {
            let snapshot = Snapshot {
                media: media(&conn).await.unwrap_or_else(|e| {
                    tracing::debug!("media: {e:#}");
                    None
                }),
                notifications: notifications(&conn, &mut first_id).await,
            };
            if last.as_ref() != Some(&snapshot) {
                if updates.send(snapshot.clone()).await.is_err() {
                    return anyhow::Ok(());
                }
                last = Some(snapshot);
            }
            let cmd = async { cmds.recv().await.ok() }
                .or(async {
                    async_io::Timer::after(Duration::from_secs(2)).await;
                    None
                })
                .await;
            if let (Some(cmd), Some(media)) = (cmd, last.as_ref().and_then(|s| s.media.as_ref())) {
                let method = match cmd {
                    MediaCmd::Previous => "Previous",
                    MediaCmd::PlayPause => "PlayPause",
                    MediaCmd::Next => "Next",
                };
                let r = conn
                    .call_method(
                        Some(media.bus_name.as_str()),
                        "/org/mpris/MediaPlayer2",
                        Some("org.mpris.MediaPlayer2.Player"),
                        method,
                        &(),
                    )
                    .await;
                if let Err(e) = r {
                    tracing::warn!("{method}: {e}");
                }
                // Let the player catch up before reading it again.
                async_io::Timer::after(Duration::from_millis(150)).await;
            }
        }
    });
    if let Err(e) = result {
        tracing::warn!("session bus: {e:#}");
    }
}

async fn property(conn: &Connection, dest: &str, name: &str) -> anyhow::Result<OwnedValue> {
    let reply = conn
        .call_method(
            Some(dest),
            "/org/mpris/MediaPlayer2",
            Some("org.freedesktop.DBus.Properties"),
            "Get",
            &("org.mpris.MediaPlayer2.Player", name),
        )
        .await?;
    Ok(reply.body().deserialize::<OwnedValue>()?)
}

/// The playing player, else the first paused one.
async fn media(conn: &Connection) -> anyhow::Result<Option<Media>> {
    let names: Vec<String> = conn
        .call_method(
            Some("org.freedesktop.DBus"),
            "/org/freedesktop/DBus",
            Some("org.freedesktop.DBus"),
            "ListNames",
            &(),
        )
        .await?
        .body()
        .deserialize()?;
    let mut paused = None;
    for name in names.into_iter().filter(|n| n.starts_with("org.mpris.MediaPlayer2.")) {
        let Ok(status) = property(conn, &name, "PlaybackStatus").await else { continue };
        let status: String = status.try_into().unwrap_or_default();
        let playing = status == "Playing";
        if !playing && status != "Paused" {
            continue;
        }
        let Ok(meta) = property(conn, &name, "Metadata").await else { continue };
        let Ok(meta) = HashMap::<String, OwnedValue>::try_from(meta) else { continue };
        let text = |key: &str| match meta.get(key).map(|v| &**v) {
            Some(Value::Str(s)) => s.to_string(),
            Some(Value::Array(a)) => a
                .iter()
                .filter_map(|v| if let Value::Str(s) = v { Some(s.to_string()) } else { None })
                .collect::<Vec<_>>()
                .join(", "),
            _ => String::new(),
        };
        let title = text("xesam:title");
        if title.is_empty() {
            continue;
        }
        let art = text("mpris:artUrl")
            .strip_prefix("file://")
            .map(|p| PathBuf::from(percent_decode(p)))
            .filter(|p| p.is_file());
        let media = Media { title, artist: text("xesam:artist"), art, playing, bus_name: name };
        if playing {
            return Ok(Some(media));
        }
        paused = paused.or(Some(media));
    }
    Ok(paused)
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Ok(b) = u8::from_str_radix(std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or(""), 16)
        {
            out.push(b);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// How many notifications yozora-notify has received since the first call. Only ids are read.
async fn notifications(conn: &Connection, first_id: &mut Option<u32>) -> Option<usize> {
    let reply = conn
        .call_method(
            Some("org.freedesktop.Notifications"),
            "/dev/yozora/Notify",
            Some("dev.yozora.Notify1"),
            "List",
            &(),
        )
        .await
        .ok()?;
    let items: Vec<HashMap<String, OwnedValue>> = reply.body().deserialize().ok()?;
    let ids: Vec<u32> = items.iter().filter_map(|n| n.get("id").and_then(|v| u32::try_from(v).ok())).collect();
    let first = *first_id.get_or_insert_with(|| ids.iter().copied().max().unwrap_or(0));
    Some(ids.iter().filter(|id| **id > first).count())
}

async fn logind(method: &str, body: &(bool,), path: &str, iface: &str) -> anyhow::Result<()> {
    let conn = Connection::system().await?;
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
    fn decodes_art_paths() {
        assert_eq!(percent_decode("/tmp/My%20Song.png"), "/tmp/My Song.png");
        assert_eq!(percent_decode("/a%2"), "/a%2");
    }
}
