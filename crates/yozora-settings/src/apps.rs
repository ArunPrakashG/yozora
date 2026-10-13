//! Installed apps, found through manifests, and whether each is running.

use std::{path::PathBuf, time::Duration};

use yozora_config::{
    manifest::{self, AppManifest},
    paths,
    schema::Schema,
};
use yozora_ipc::Request;

/// The app Settings has built-in pages for.
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
}

pub fn manifest_dirs(extra: &[PathBuf]) -> Vec<PathBuf> {
    let mut dirs = extra.to_vec();
    dirs.extend(paths::manifest_dirs());
    dirs
}

/// Scans manifests, keeping the known status of apps that are still there.
pub fn discover(extra: &[PathBuf], previous: &[AppEntry]) -> Vec<AppEntry> {
    manifest::discover_in(&manifest_dirs(extra))
        .into_iter()
        .map(|m| {
            let mut entry = AppEntry::new(m);
            if let Some(old) = previous.iter().find(|o| o.manifest == entry.manifest) {
                entry.status = old.status.clone();
            }
            entry
        })
        .collect()
}

/// Pings an app's socket. Blocking; run it off the main thread.
pub fn probe(manifest: &AppManifest) -> Status {
    let Some(socket) = manifest.socket_path() else {
        return Status::NoSocket;
    };
    if !socket.exists() {
        return Status::Stopped;
    }
    match yozora_ipc::request_timeout(&socket, &Request::new("ping", Vec::<String>::new()), Duration::from_secs(1)) {
        Ok(r) if r.ok => Status::Running {
            version: r.data["version"].as_str().map(String::from),
            warnings: r.data["warnings"]
                .as_array()
                .map(|a| a.iter().filter_map(|w| w.as_str().map(String::from)).collect())
                .unwrap_or_default(),
        },
        // It answered, so it's running, even if it doesn't know `ping`.
        Ok(_) => Status::Running { version: None, warnings: Vec::new() },
        Err(_) => Status::Stopped,
    }
}

/// Starts an app detached from Settings, so closing Settings doesn't stop it.
pub fn start(manifest: &AppManifest) -> anyhow::Result<()> {
    use std::os::unix::process::CommandExt;
    let mut command = std::process::Command::new(&manifest.binary);
    yozora_ui::gpu::child(&mut command)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .process_group(0);
    let mut child = command.spawn().map_err(|e| anyhow::anyhow!("starting {}: {e}", manifest.binary))?;
    // Reap it in the background so it doesn't linger as a zombie if it exits early.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

/// Opens a file or URL with the desktop's default handler.
pub fn open(target: &str) {
    let target = target.to_string();
    std::thread::spawn(move || {
        if let Err(e) = yozora_ui::gpu::child(&mut std::process::Command::new("xdg-open"))
            .arg(&target)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
        {
            tracing::warn!("xdg-open {target}: {e}");
        }
    });
}
