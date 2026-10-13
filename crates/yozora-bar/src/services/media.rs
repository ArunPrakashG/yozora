//! Media players over MPRIS on the session bus.

use std::{
    collections::HashMap,
    hash::{Hash, Hasher},
    path::PathBuf,
    time::{Duration, Instant},
};

use gpui::{App, Entity};
use zbus::{Connection, MatchRule};

use super::{
    Link, Media,
    dbus::{self, Props, Wake},
};

const PREFIX: &str = "org.mpris.MediaPlayer2.";
const PATH: &str = "/org/mpris/MediaPlayer2";
const PLAYER: &str = "org.mpris.MediaPlayer2.Player";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Status {
    Playing,
    Paused,
    #[default]
    Stopped,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Player {
    pub bus: String,
    pub identity: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    /// A local file, downloaded to the cache first when the player gives a URL.
    pub art: Option<PathBuf>,
    pub status: Status,
    pub length: Option<Duration>,
    pub position: Duration,
    /// When `position` was read, to extrapolate while playing.
    pub position_at: Option<Instant>,
    pub can_next: bool,
    pub can_previous: bool,
}

impl Player {
    pub fn position_now(&self) -> Duration {
        match (self.status, self.position_at) {
            (Status::Playing, Some(at)) => {
                let p = self.position + at.elapsed();
                self.length.map_or(p, |l| p.min(l))
            }
            _ => self.position,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct State {
    pub players: Vec<Player>,
    pub selected: Option<String>,
}

impl State {
    /// The chosen player, else the first one playing, else any.
    pub fn current(&self) -> Option<&Player> {
        let chosen = self.selected.as_ref().and_then(|b| self.players.iter().find(|p| &p.bus == b));
        chosen.or_else(|| self.players.iter().find(|p| p.status == Status::Playing)).or_else(|| self.players.first())
    }
}

#[derive(Clone, Debug)]
pub enum Cmd {
    PlayPause,
    Next,
    Previous,
    Select(String),
}

pub fn start(cx: &mut App, fake: bool) -> Entity<Media> {
    if fake {
        super::spawn(cx, "media", true, run_fake)
    } else {
        super::spawn(cx, "media", false, |link| super::supervise(link, run))
    }
}

fn art_cache() -> PathBuf {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
        .unwrap_or_else(std::env::temp_dir);
    base.join("yozora").join("art")
}

/// Turns an `mpris:artUrl` into a local file, fetching remote art with curl.
fn art(url: &str, fetched: &mut HashMap<String, Option<PathBuf>>) -> Option<PathBuf> {
    if url.is_empty() {
        return None;
    }
    if let Some(path) = url.strip_prefix("file://") {
        let path = percent_decode(path);
        return std::path::Path::new(&path).exists().then(|| PathBuf::from(path));
    }
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return None;
    }
    if let Some(done) = fetched.get(url) {
        return done.clone();
    }
    let mut h = std::collections::hash_map::DefaultHasher::new();
    url.hash(&mut h);
    let dir = art_cache();
    let file = dir.join(format!("{:016x}", h.finish()));
    let result = if file.exists() {
        Some(file)
    } else {
        let _ = std::fs::create_dir_all(&dir);
        let tmp = file.with_extension("part");
        let ok = std::process::Command::new("curl")
            .args(["-sfL", "--max-time", "5", "--max-filesize", "8000000", "-o"])
            .arg(&tmp)
            .arg(url)
            .status()
            .is_ok_and(|s| s.success());
        (ok && std::fs::rename(&tmp, &file).is_ok()).then_some(file)
    };
    fetched.insert(url.to_string(), result.clone());
    result
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Ok(b) = u8::from_str_radix(&s[i + 1..i + 3], 16)
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

fn duration_us(v: Option<i64>) -> Option<Duration> {
    v.filter(|v| *v > 0).map(|v| Duration::from_micros(v as u64))
}

async fn player(
    conn: &Connection,
    bus: &str,
    fetched: &mut HashMap<String, Option<PathBuf>>,
) -> anyhow::Result<Player> {
    let root = dbus::get_all(conn, bus, PATH, "org.mpris.MediaPlayer2").await.unwrap_or_default();
    let p = dbus::get_all(conn, bus, PATH, PLAYER).await?;
    let meta: Props = dbus::get(&p, "Metadata").unwrap_or_default();
    let artist = dbus::get::<Vec<String>>(&meta, "xesam:artist")
        .map(|a| a.join(", "))
        .or_else(|| dbus::get::<String>(&meta, "xesam:artist"))
        .unwrap_or_default();
    let length = duration_us(
        dbus::get::<i64>(&meta, "mpris:length").or_else(|| dbus::get::<u64>(&meta, "mpris:length").map(|v| v as i64)),
    );
    let position = duration_us(dbus::get::<i64>(&p, "Position")).unwrap_or_default();
    let identity = dbus::get::<String>(&root, "Identity")
        .unwrap_or_else(|| bus.trim_start_matches(PREFIX).split('.').next().unwrap_or(bus).to_string());
    Ok(Player {
        bus: bus.to_string(),
        identity,
        title: dbus::string(&meta, "xesam:title"),
        artist,
        album: dbus::string(&meta, "xesam:album"),
        // Drawn at up to 88 px.
        art: art(&dbus::string(&meta, "mpris:artUrl"), fetched).map(|p| super::icons::fit(&p, 176)),
        status: match dbus::string(&p, "PlaybackStatus").as_str() {
            "Playing" => Status::Playing,
            "Paused" => Status::Paused,
            _ => Status::Stopped,
        },
        length,
        position,
        position_at: Some(Instant::now()),
        can_next: dbus::get(&p, "CanGoNext").unwrap_or(false),
        can_previous: dbus::get(&p, "CanGoPrevious").unwrap_or(false),
    })
}

async fn snapshot(
    conn: &Connection,
    selected: Option<String>,
    fetched: &mut HashMap<String, Option<PathBuf>>,
) -> anyhow::Result<State> {
    let names: Vec<String> =
        dbus::call(conn, "org.freedesktop.DBus", "/org/freedesktop/DBus", "org.freedesktop.DBus", "ListNames", &())
            .await?;
    let mut players = Vec::new();
    for name in names.iter().filter(|n| n.starts_with(PREFIX)) {
        // playerctld mirrors whichever player is active; listing it would show doubles.
        if name == "org.mpris.MediaPlayer2.playerctld" {
            continue;
        }
        if let Ok(p) = player(conn, name, fetched).await {
            players.push(p);
        }
    }
    players.sort_by(|a, b| a.identity.cmp(&b.identity));
    let selected = selected.filter(|s| players.iter().any(|p| &p.bus == s));
    Ok(State { players, selected })
}

async fn run(link: &Link<State, Cmd>) -> anyhow::Result<()> {
    let conn = Connection::session().await?;
    let props = MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .interface("org.freedesktop.DBus.Properties")?
        .path(PATH)?
        .build();
    let seeked =
        MatchRule::builder().msg_type(zbus::message::Type::Signal).interface(PLAYER)?.member("Seeked")?.build();
    let owners = MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .sender("org.freedesktop.DBus")?
        .member("NameOwnerChanged")?
        .arg0ns(PREFIX.trim_end_matches('.'))?
        .build();
    let mut stream = futures_lite::stream::or(
        dbus::signals(&conn, props).await?,
        futures_lite::stream::or(dbus::signals(&conn, seeked).await?, dbus::signals(&conn, owners).await?),
    );
    let mut fetched = HashMap::new();
    let mut state = snapshot(&conn, None, &mut fetched).await?;
    link.publish(state.clone());
    loop {
        match dbus::next(&link.cmds, &mut stream, Duration::from_millis(60)).await? {
            Wake::Quit => return Ok(()),
            Wake::Changed => {}
            Wake::Command(Cmd::Select(bus)) => state.selected = Some(bus),
            Wake::Command(cmd) => {
                if let Some(p) = state.current() {
                    let method = match cmd {
                        Cmd::PlayPause => "PlayPause",
                        Cmd::Next => "Next",
                        Cmd::Previous => "Previous",
                        Cmd::Select(_) => unreachable!(),
                    };
                    if let Err(e) = dbus::invoke(&conn, &p.bus, PATH, PLAYER, method, &()).await {
                        tracing::warn!("media: {e:#}");
                    }
                }
            }
        }
        state = snapshot(&conn, state.selected.clone(), &mut fetched).await?;
        link.publish(state.clone());
    }
}

fn run_fake(link: Link<State, Cmd>) {
    let tracks =
        [("Night Transit", "Lumen Drift", "Low Orbit", 252), ("Afterglow", "Ember & Static", "Long Exposure", 218)];
    let mut track = 0;
    let make = |track: usize, status, position: Duration| {
        let (title, artist, album, secs) = tracks[track];
        Player {
            bus: "org.mpris.MediaPlayer2.fake".into(),
            identity: "Spotify".into(),
            title: title.into(),
            artist: artist.into(),
            album: album.into(),
            art: None,
            status,
            length: Some(Duration::from_secs(secs)),
            position,
            position_at: Some(Instant::now()),
            can_next: true,
            can_previous: true,
        }
    };
    let mpv = Player {
        bus: "org.mpris.MediaPlayer2.mpv".into(),
        identity: "mpv".into(),
        title: "orbit-timelapse.mkv".into(),
        status: Status::Paused,
        ..Default::default()
    };
    let mut current = make(track, Status::Playing, Duration::from_secs(74));
    let mut selected = None;
    let publish = |current: &Player, selected: &Option<String>| {
        link.publish(State { players: vec![current.clone(), mpv.clone()], selected: selected.clone() })
    };
    publish(&current, &selected);
    while let Ok(cmd) = link.cmds.recv_blocking() {
        let position = current.position_now();
        match cmd {
            Cmd::PlayPause => {
                let status = if current.status == Status::Playing { Status::Paused } else { Status::Playing };
                current = make(track, status, position);
            }
            Cmd::Next | Cmd::Previous => {
                track = 1 - track;
                current = make(track, current.status, Duration::ZERO);
            }
            Cmd::Select(bus) => selected = Some(bus),
        }
        if !publish(&current, &selected) {
            break;
        }
    }
}

pub async fn check() -> anyhow::Result<String> {
    let conn = Connection::session().await?;
    let s = snapshot(&conn, None, &mut HashMap::new()).await?;
    let names: Vec<_> = s.players.iter().map(|p| p.identity.as_str()).collect();
    Ok(format!("MPRIS · {} player(s){}{}", names.len(), if names.is_empty() { "" } else { ": " }, names.join(", ")))
}

#[cfg(test)]
mod tests {
    #[test]
    fn decodes_file_urls() {
        assert_eq!(super::percent_decode("/a%20b/c.png"), "/a b/c.png");
        assert_eq!(super::percent_decode("/50%"), "/50%");
    }
}
