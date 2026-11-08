//! The command line, and the control socket other Yozora apps and `yozora-lock status` talk to.

use std::path::PathBuf;

use yozora_config::paths;
use yozora_ipc::{Request, Response, Server};

pub const SOCKET_ID: &str = "lock";
/// A preview has its own socket, so it never stops a real lock from starting.
pub const PREVIEW_SOCKET_ID: &str = "lock-preview";

pub const HELP: &str = "\
yozora-lock: a lock screen for Hyprland, in Yozora's style

Usage:
  yozora-lock [--config DIR]     Lock the session now
  yozora-lock --preview          Show the lock screen without locking (Escape closes it)
  yozora-lock status             Whether the screen is locked, as JSON

Options:
  --config DIR    Read lock.toml and theme.toml from DIR instead of ~/.config/yozora
  --pam-dir DIR   Read the PAM service `yozora-lock` from DIR instead of /etc/pam.d (for testing)
  -h, --help      Show this help
  -V, --version   Show the version

Yozora Idle (yozora-idle) starts it when you're away and before sleep. To use hypridle instead,
set in ~/.config/hypr/hypridle.conf:
  lock_cmd = yozora-lock
  before_sleep_cmd = loginctl lock-session
  inhibit_sleep = 3

A second yozora-lock while locked does nothing, so `lock_cmd` needs no `pidof` check.
";

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Options {
    pub preview: bool,
    pub config: Option<PathBuf>,
    pub pam_dir: Option<PathBuf>,
}

impl Options {
    /// The directory holding `lock.toml` and `theme.toml`.
    pub fn config_dir(&self) -> PathBuf {
        self.config.clone().unwrap_or_else(paths::config_dir)
    }

    pub fn socket_id(&self) -> &'static str {
        if self.preview { PREVIEW_SOCKET_ID } else { SOCKET_ID }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    Help,
    Version,
    Status,
    Run(Options),
}

pub fn parse(mut args: impl Iterator<Item = String>) -> Result<Command, String> {
    let mut o = Options::default();
    while let Some(a) = args.next() {
        match a.as_str() {
            "-h" | "--help" | "help" => return Ok(Command::Help),
            "-V" | "--version" => return Ok(Command::Version),
            "status" => return Ok(Command::Status),
            "--preview" => o.preview = true,
            "--config" => o.config = Some(PathBuf::from(args.next().ok_or("--config needs a directory")?)),
            "--pam-dir" => o.pam_dir = Some(PathBuf::from(args.next().ok_or("--pam-dir needs a directory")?)),
            other => return Err(format!("unknown argument {other:?}")),
        }
    }
    Ok(Command::Run(o))
}

/// The output of `yozora-lock status`: whether a real lock answers on its socket.
pub fn status_json() -> serde_json::Value {
    serde_json::json!({ "locked": yozora_ipc::is_alive(&paths::socket_path(SOCKET_ID)) })
}

/// Why the control socket could not be taken.
#[derive(Debug)]
pub enum BindError {
    /// A lock (or preview) is already running; not an error for hypridle.
    AlreadyRunning,
    Failed(anyhow::Error),
}

/// Takes the control socket for this run.
pub fn bind(o: &Options) -> Result<Server, BindError> {
    let path = paths::socket_path(o.socket_id());
    Server::bind(&path).map_err(|e| {
        // Already locked: nothing to do.
        if yozora_ipc::is_alive(&path) { BindError::AlreadyRunning } else { BindError::Failed(e) }
    })
}

/// What `yozora-lock` prints when [`bind`] says another one is running.
pub fn already_running_message(o: &Options) -> &'static str {
    if o.preview { "a preview is already open" } else { "already locked" }
}

pub fn reply(request: &Request) -> Response {
    match request.cmd.as_str() {
        "ping" => Response::data(serde_json::json!({ "version": env!("CARGO_PKG_VERSION"), "warnings": [] })),
        "status" => Response::data(serde_json::json!({ "locked": true })),
        other => Response::error(format!("unknown command {other:?}; try status")),
    }
}

/// Answers the control socket on its own thread for the rest of the process.
pub fn serve(socket: Server) {
    std::thread::Builder::new()
        .name("yz-ipc".into())
        .spawn(move || {
            let (tx, rx) = async_channel::unbounded();
            async_io::block_on(futures_lite::future::zip(socket.run(tx), async move {
                while let Ok(incoming) = rx.recv().await {
                    let response = reply(&incoming.request);
                    incoming.reply(response);
                }
            }));
        })
        .expect("spawning the control socket thread");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> impl Iterator<Item = String> {
        list.iter().map(|s| s.to_string()).collect::<Vec<_>>().into_iter()
    }

    #[test]
    fn parses_commands_and_options() {
        assert_eq!(parse(args(&[])), Ok(Command::Run(Options::default())));
        assert_eq!(parse(args(&["--help"])), Ok(Command::Help));
        assert_eq!(parse(args(&["status"])), Ok(Command::Status));
        assert_eq!(parse(args(&["-V"])), Ok(Command::Version));
        let run = parse(args(&["--preview", "--config", "/c", "--pam-dir", "/p"]));
        let o = Options { preview: true, config: Some("/c".into()), pam_dir: Some("/p".into()) };
        assert_eq!(run, Ok(Command::Run(o.clone())));
        assert_eq!(o.socket_id(), PREVIEW_SOCKET_ID);
        assert!(parse(args(&["--config"])).is_err());
        assert!(parse(args(&["--nope"])).unwrap_err().contains("unknown argument"));
    }

    #[test]
    fn replies_to_the_control_socket() {
        assert!(reply(&Request::new("ping", [] as [&str; 0])).ok);
        assert_eq!(reply(&Request::new("status", [] as [&str; 0])).data["locked"], true);
        assert!(!reply(&Request::new("bogus", [] as [&str; 0])).ok);
    }
}
