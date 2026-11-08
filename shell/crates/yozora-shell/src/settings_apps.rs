//! The apps Settings can configure, found through their manifests, whether each is running, and
//! the monitors Hyprland reports. Scanning and probing happen off the UI thread; the page reads
//! the last result.

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use anyhow::Context as _;
use yozora_config::manifest::{self, AppManifest};
use yozora_config::schema::Schema;
use yozora_ipc::Request;

use crate::shared::Shared;

/// The app with its own bar pages.
pub const BAR: &str = "bar";

#[derive(Clone, Debug, PartialEq)]
pub enum Status {
    /// Not checked yet.
    Unknown,
    /// The app has no control socket, so Settings can't tell.
    NoSocket,
    Running { version: Option<String>, warnings: Vec<String> },
    Stopped,
}

#[derive(Clone, Debug)]
pub struct AppEntry {
    pub manifest: AppManifest,
    pub schema: Option<Schema>,
    pub schema_error: Option<String>,
    pub status: Status,
}

impl AppEntry {
    fn new(manifest: AppManifest) -> Self {
        let (schema, schema_error) = match manifest.schema_path() {
            Some(path) => match Schema::load(&path) {
                Ok(s) => (Some(s), None),
                Err(e) => (None, Some(format!("{e:#}"))),
            },
            None => (None, None),
        };
        let status = if manifest.socket.is_some() { Status::Unknown } else { Status::NoSocket };
        Self { manifest, schema, schema_error, status }
    }

    pub fn id(&self) -> &str {
        &self.manifest.id
    }

    pub fn is_bar(&self) -> bool {
        self.manifest.id == BAR
    }

    pub fn has_options(&self) -> bool {
        self.schema.as_ref().is_some_and(|s| !s.section.is_empty()) || self.schema_error.is_some()
    }

    /// The name in the sidebar: the app's own, without the product prefix.
    pub fn short_name(&self) -> String {
        self.manifest.name.strip_prefix("Yozora ").unwrap_or(&self.manifest.name).to_owned()
    }

    /// Whether Settings may start it. The shell stands in for the bar, launcher and notification
    /// daemon, and starting the lock screen would lock the session.
    pub fn startable(&self) -> bool {
        !matches!(self.id(), "bar" | "launch" | "notify" | "lock")
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Monitor {
    pub name: String,
    pub description: String,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub mode: (i32, i32),
    pub refresh: f64,
}

struct Cache {
    apps: Arc<Vec<AppEntry>>,
    monitors: Result<Arc<Vec<Monitor>>, String>,
    scanned: bool,
    at: Option<Instant>,
    busy: bool,
}

fn cache() -> &'static Mutex<Cache> {
    static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
    CACHE.get_or_init(|| {
        Mutex::new(Cache { apps: Arc::new(Vec::new()), monitors: Err("not read yet".into()), scanned: false, at: None, busy: false })
    })
}

fn scan() -> Vec<AppEntry> {
    manifest::discover().into_iter().map(AppEntry::new).collect()
}

/// The installed apps as last seen. The first call reads the manifests itself, so a page has
/// something to show at once.
pub fn apps() -> Arc<Vec<AppEntry>> {
    let mut c = cache().lock().unwrap();
    if !c.scanned {
        c.apps = Arc::new(scan());
        c.scanned = true;
    }
    c.apps.clone()
}

pub fn monitors() -> Result<Arc<Vec<Monitor>>, String> {
    if std::env::var_os("YZ_FAKE_MONITORS").is_some() {
        return read_monitors().map(Arc::new).map_err(|e| e.to_string());
    }
    cache().lock().unwrap().monitors.clone()
}

/// Starts a fresh scan and probe when the last one is old, and redraws when it lands.
pub fn refresh(sh: &Arc<Shared>) {
    {
        let mut c = cache().lock().unwrap();
        if c.busy || c.at.is_some_and(|t| t.elapsed() < Duration::from_secs(4)) {
            return;
        }
        c.busy = true;
    }
    let sh = sh.clone();
    let _ = std::thread::Builder::new().name("settings-probe".into()).spawn(move || {
        let mut apps = scan();
        for app in &mut apps {
            app.status = probe(&app.manifest);
        }
        let monitors = read_monitors().map(Arc::new).map_err(|e| format!("{e:#}"));
        {
            let mut c = cache().lock().unwrap();
            c.apps = Arc::new(apps);
            c.monitors = monitors;
            c.scanned = true;
            c.at = Some(Instant::now());
            c.busy = false;
        }
        sh.redraw();
    });
}

/// Pings an app's socket. Blocking.
fn probe(manifest: &AppManifest) -> Status {
    let Some(socket) = manifest.socket_path() else { return Status::NoSocket };
    if !socket.exists() {
        return Status::Stopped;
    }
    match yozora_ipc::request_timeout(&socket, &Request::new("ping", Vec::<String>::new()), Duration::from_secs(1)) {
        Ok(r) if r.ok => Status::Running {
            version: r.data["version"].as_str().map(String::from),
            warnings: r.data["warnings"].as_array().map(|a| a.iter().filter_map(|w| w.as_str().map(String::from)).collect()).unwrap_or_default(),
        },
        // It answered, so it's running, even if it doesn't know `ping`.
        Ok(_) => Status::Running { version: None, warnings: Vec::new() },
        Err(_) => Status::Stopped,
    }
}

/// Starts an app detached from the shell, so quitting the shell doesn't stop it.
pub fn start(manifest: &AppManifest) -> anyhow::Result<()> {
    use std::os::unix::process::CommandExt;
    let mut command = std::process::Command::new(&manifest.binary);
    command.stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).process_group(0);
    let mut child = command.spawn().map_err(|e| anyhow::anyhow!("starting {}: {e}", manifest.binary))?;
    // Reap it in the background so it doesn't linger as a zombie if it exits early.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    // Look again soon, since it takes a moment to bind its socket.
    cache().lock().unwrap().at = None;
    Ok(())
}

fn hypr_request(command: &str) -> anyhow::Result<String> {
    let sig = std::env::var("HYPRLAND_INSTANCE_SIGNATURE").context("Hyprland isn't running")?;
    let runtime = std::env::var("XDG_RUNTIME_DIR").context("XDG_RUNTIME_DIR is not set")?;
    let path = PathBuf::from(runtime).join("hypr").join(sig).join(".socket.sock");
    let mut stream = UnixStream::connect(&path).with_context(|| format!("connecting to {}", path.display()))?;
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    stream.write_all(command.as_bytes())?;
    let mut reply = String::new();
    stream.read_to_string(&mut reply)?;
    Ok(reply)
}

fn read_monitors() -> anyhow::Result<Vec<Monitor>> {
    // Two sample screens, for drawing the page without a compositor.
    if std::env::var_os("YZ_FAKE_MONITORS").is_some() {
        let screen = |name: &str, description: &str, x: i32, w: i32, h: i32, refresh: f64| Monitor {
            name: name.into(), description: description.into(), x, y: 0, width: w, height: h, mode: (w, h), refresh,
        };
        return Ok(vec![screen("DP-1", "Studio Display", 0, 2560, 1440, 144.0), screen("HDMI-A-1", "Side Panel", 2560, 1920, 1080, 75.0)]);
    }
    let reply = hypr_request("j/monitors")?;
    let value: serde_json::Value = serde_json::from_str(&reply).context("parsing j/monitors")?;
    let int = |v: &serde_json::Value, k: &str| v[k].as_i64().unwrap_or(0) as i32;
    Ok(value
        .as_array()
        .map(|a| {
            a.iter()
                .map(|m| {
                    let (w, h) = (int(m, "width"), int(m, "height"));
                    let scale = m["scale"].as_f64().filter(|s| *s > 0.0).unwrap_or(1.0);
                    let rotated = m["transform"].as_i64().unwrap_or(0) % 2 == 1;
                    let (lw, lh) = if rotated { (h, w) } else { (w, h) };
                    Monitor {
                        name: m["name"].as_str().unwrap_or_default().to_string(),
                        description: m["description"].as_str().unwrap_or_default().to_string(),
                        x: int(m, "x"),
                        y: int(m, "y"),
                        width: (lw as f64 / scale).round() as i32,
                        height: (lh as f64 / scale).round() as i32,
                        mode: (w, h),
                        refresh: m["refreshRate"].as_f64().unwrap_or(0.0),
                    }
                })
                .collect()
        })
        .unwrap_or_default())
}

/// Looks again at the apps and monitors the next time a page asks.
pub fn invalidate() {
    cache().lock().unwrap().at = None;
}

/// Opens a file or address with the desktop's default handler.
pub fn open(target: &str) {
    let target = target.to_owned();
    std::thread::spawn(move || {
        let status = std::process::Command::new("xdg-open")
            .arg(&target)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
        if let Err(e) = status {
            tracing::warn!("xdg-open {target}: {e}");
        }
    });
}
