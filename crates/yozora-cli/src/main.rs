//! `yozora`: one command for scripts and AI agents to read and control the Yozora desktop.
//!
//! It talks to each app's control socket in `$XDG_RUNTIME_DIR/yozora/`, edits config files the
//! way Settings does, and links the bundled agent skill into Claude Code, Codex and opencode.
//! Every command prints JSON with `--json`.

/// `println!` that doesn't panic when the reader has gone, as with `yozora notifications | head -1`.
macro_rules! say {
    ($($t:tt)*) => {{
        use std::io::Write as _;
        let _ = writeln!(std::io::stdout().lock(), $($t)*);
    }};
}

/// `print!`, the same way.
macro_rules! say_raw {
    ($($t:tt)*) => {{
        use std::io::Write as _;
        let _ = write!(std::io::stdout().lock(), $($t)*);
    }};
}

mod agent;
mod apps;
mod config;
mod desktop;
mod notify;
mod out;

use std::process::ExitCode;

use serde_json::Value;

pub const HELP: &str = "\
yozora: read and control the Yozora desktop, for scripts and AI agents.

Usage: yozora [--json] <command> [args]

Desktop
  status                          What's running, with versions, warnings and notification counts
  apps                            Installed Yozora apps, their config files and whether they run
  desktop [SECTION...]            Live state from the bar: hyprland (monitors, workspaces, focused
                                  window), audio, media, network, bluetooth, power, tray
  media [play-pause|play|pause|next|previous] [PLAYER]
  volume [N|+N|-N|mute|unmute|toggle-mute] [--mic]
  profile [NAME]                  The power profile, or switch it
  bar toggle MODULE [MONITOR]     Open or close a bar popover (notifications, control-center,
                                  sound, network, devices, media, calendar, battery)
  bar close | bar reload
  launcher [open [QUERY]|close|toggle]
  settings [PAGE]                 Open Yozora Settings, on PAGE (appearance, motion, bar, notify, ...)
  restart [APP...]                Restart Yozora's services (bar, notify, launch, idle); all if none named
  lock                            Lock the screen now
  idle                            What Yozora Idle is counting down to, and what holds the screen on
  keep-awake [on|off|toggle]      Stop locking, screen-off and suspend while on

Notifications (Yozora Notify)
  notifications [list] [--unread] [--urgent] [--app NAME] [--limit N] [--show-private]
  notifications show ID [--show-private]
  notifications dismiss ID...
  notifications clear             Clears everything but pinned notifications
  notifications pin ID | unpin ID
  notifications snooze ID MINUTES
  notifications action ID [KEY]   Invoke an action; KEY defaults to `default` (open)
  notifications send SUMMARY [BODY] [--app NAME] [--urgency low|normal|critical] [--icon NAME]
  dnd [on|off|toggle]             Do Not Disturb

Settings files
  config list [APP]               Every setting an app's schema describes, with its value
  config get APP KEY
  config set APP KEY VALUE        VALUE `-` reads it from stdin (use that for keys)
  config reset APP KEY            Back to the default
  config path APP

Agent skill
  agent install [--all]           Link the Yozora skill into Claude Code (~/.claude/skills) and
                                  ~/.agents/skills (Codex, opencode). --all creates both
  agent uninstall                 Remove the links, and stop installing them at login
  agent status
  agent skill                     Print the skill

Notifications marked private by Jev are redacted unless --show-private is given.
";

fn main() -> ExitCode {
    let mut json = false;
    let mut args: Vec<String> = Vec::new();
    for a in std::env::args().skip(1) {
        match a.as_str() {
            "--json" => json = true,
            _ => args.push(a),
        }
    }
    let out = out::Out { json };
    if args.is_empty() || matches!(args[0].as_str(), "-h" | "--help" | "help") {
        say_raw!("{HELP}");
        return ExitCode::SUCCESS;
    }
    if matches!(args[0].as_str(), "-V" | "--version" | "version") {
        say!("yozora {}", env!("CARGO_PKG_VERSION"));
        return ExitCode::SUCCESS;
    }
    let cmd = args.remove(0);
    match run(&cmd, args, &out) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            out.error(&format!("{e:#}"));
            ExitCode::FAILURE
        }
    }
}

fn run(cmd: &str, args: Vec<String>, out: &out::Out) -> anyhow::Result<()> {
    match cmd {
        "status" => apps::status(out),
        "apps" => apps::list(out),
        "desktop" | "state" => desktop::state(args, out),
        "media" => desktop::simple("media", args, out),
        "volume" => desktop::simple("volume", args, out),
        "profile" => desktop::simple("profile", args, out),
        "bar" => desktop::bar(args, out),
        "launcher" | "launch" => apps::launcher(args, out),
        "settings" => apps::settings(args, out),
        "lock" => apps::lock(out),
        "idle" => apps::idle(out),
        "keep-awake" | "awake" => apps::keep_awake(args, out),
        "restart" => apps::restart(args, out),
        "notifications" | "notification" | "notify" | "n" => notify::run(args, out),
        "dnd" => notify::dnd(args, out),
        "config" => config::run(args, out),
        "agent" => agent::run(args, out),
        other => anyhow::bail!("unknown command {other:?}; see yozora --help"),
    }
}

/// Takes `--flag VALUE` out of `args`.
pub fn take_value(args: &mut Vec<String>, flag: &str) -> anyhow::Result<Option<String>> {
    let Some(i) = args.iter().position(|a| a == flag) else { return Ok(None) };
    anyhow::ensure!(i + 1 < args.len(), "{flag} needs a value");
    let value = args.remove(i + 1);
    args.remove(i);
    Ok(Some(value))
}

/// Takes `--flag` out of `args`.
pub fn take_flag(args: &mut Vec<String>, flag: &str) -> bool {
    let before = args.len();
    args.retain(|a| a != flag);
    args.len() != before
}

/// Sends a request to app `id`'s control socket and returns its data.
pub fn ask(id: &str, cmd: &str, args: &[String]) -> anyhow::Result<Value> {
    let path = yozora_config::paths::socket_path(id);
    let request = yozora_ipc::Request::new(cmd, args.iter().cloned());
    let response = yozora_ipc::request_timeout(&path, &request, std::time::Duration::from_secs(10))
        .map_err(|_| anyhow::anyhow!("{} isn't running (no answer on {})", apps::name(id), path.display()))?;
    match response.error {
        Some(e) if !response.ok => anyhow::bail!("{}: {e}", apps::name(id)),
        _ => Ok(response.data),
    }
}
