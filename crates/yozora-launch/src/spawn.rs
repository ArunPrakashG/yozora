//! Starting things: apps, commands in a terminal, files and URLs.
//!
//! Each runs in its own transient systemd scope when systemd is there, so stopping or
//! restarting the launcher's service never takes the apps it started with it, and `setsid -f`
//! detaches it so the launcher never has to reap it.

use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::OnceLock,
};

use crate::desktop::{self, App};

pub fn in_path(program: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(program).is_file()))
}

fn has_systemd_run() -> bool {
    static HAS: OnceLock<bool> = OnceLock::new();
    *HAS.get_or_init(|| in_path("systemd-run") && std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_some())
}

/// A scope name like `app-yozora-firefox-1a2b3c.scope`, which systemd lists under the app.
fn scope_name(id: &str) -> String {
    let clean: String = id
        .trim_end_matches(".desktop")
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .take(64)
        .collect();
    let nanos =
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(0);
    format!("app-yozora-{clean}-{:x}{nanos:08x}.scope", std::process::id())
}

/// Runs `args`, detached, from `dir` or the home directory.
pub fn run(id: &str, args: &[String], dir: Option<&Path>) -> anyhow::Result<()> {
    let Some((program, rest)) = args.split_first() else { anyhow::bail!("nothing to run") };
    let mut cmd = Command::new("setsid");
    cmd.arg("-f");
    if has_systemd_run() {
        cmd.args(["systemd-run", "--user", "--scope", "--quiet", "--collect", "--slice=app.slice"])
            .arg(format!("--unit={}", scope_name(id)))
            .arg("--");
    }
    cmd.arg(program).args(rest);
    let home = std::env::var_os("HOME").map(PathBuf::from);
    if let Some(d) = dir.filter(|d| d.is_dir()).map(Path::to_path_buf).or(home) {
        cmd.current_dir(d);
    }
    yozora_ui::gpu::child(&mut cmd);
    cmd.stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    let mut child = cmd.spawn()?;
    // `setsid -f` forks and its parent exits right away; reap that parent off this thread.
    std::thread::Builder::new().name("yz-reap".into()).spawn(move || child.wait())?;
    Ok(())
}

/// Starts an app, or one of its actions.
pub fn app(app: &App, action: Option<usize>, terminal: &str) -> anyhow::Result<()> {
    let exec = match action {
        Some(i) => &app.actions.get(i).ok_or_else(|| anyhow::anyhow!("no such action"))?.exec,
        None => &app.exec,
    };
    let mut args = desktop::expand(exec, app);
    if app.terminal {
        args = in_terminal(terminal, args);
    }
    run(&app.id, &args, app.working_dir.as_deref())
}

/// The terminal to use: the configured one, `$TERMINAL`, or the first one installed.
pub fn terminal(configured: &str) -> Option<String> {
    if !configured.trim().is_empty() {
        return Some(configured.trim().to_string());
    }
    if let Ok(t) = std::env::var("TERMINAL")
        && !t.trim().is_empty()
    {
        return Some(t);
    }
    ["ghostty", "kitty", "foot", "alacritty", "wezterm", "konsole", "gnome-terminal", "xfce4-terminal", "xterm"]
        .into_iter()
        .find(|t| in_path(t))
        .map(str::to_string)
}

/// `args` wrapped to run in `terminal`.
pub fn in_terminal(terminal: &str, args: Vec<String>) -> Vec<String> {
    let Some(term) = self::terminal(terminal) else { return args };
    let mut out: Vec<String> = desktop::split_exec(&term);
    let name = out.first().and_then(|p| Path::new(p).file_name()).map(|n| n.to_string_lossy().into_owned());
    match name.as_deref() {
        Some("gnome-terminal" | "kgx" | "ptyxis") => out.push("--".into()),
        Some("wezterm") => out.extend(["start".into(), "--".into()]),
        Some("kitty") => {}
        _ => out.push("-e".into()),
    }
    out.extend(args);
    out
}

/// Runs a shell command in the terminal, leaving a shell open after it so the output stays.
pub fn shell_in_terminal(command: &str, terminal: &str) -> anyhow::Result<()> {
    let shell = std::env::var("SHELL").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| "/bin/sh".into());
    let script = format!("{command}; exec {shell}");
    let args = in_terminal(terminal, vec![shell.clone(), "-c".into(), script]);
    run("shell", &args, None)
}

/// Runs a shell command with no window.
pub fn shell(command: &str) -> anyhow::Result<()> {
    run("command", &["sh".into(), "-c".into(), command.into()], None)
}

/// Opens a file, folder or URL in its default app.
pub fn open(target: &str) -> anyhow::Result<()> {
    run("open", &["xdg-open".into(), target.into()], None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_wrapping() {
        let a = vec!["htop".to_string()];
        assert_eq!(in_terminal("ghostty", a.clone()), vec!["ghostty", "-e", "htop"]);
        assert_eq!(in_terminal("kitty --single-instance", a.clone()), vec!["kitty", "--single-instance", "htop"]);
        assert_eq!(in_terminal("gnome-terminal", a.clone()), vec!["gnome-terminal", "--", "htop"]);
        assert_eq!(in_terminal("/usr/bin/wezterm", a), vec!["/usr/bin/wezterm", "start", "--", "htop"]);
    }

    #[test]
    fn scope_names_are_valid() {
        let s = scope_name("org.gnome.Nautilus.desktop");
        assert!(s.starts_with("app-yozora-org_gnome_Nautilus-"));
        assert!(s.ends_with(".scope"));
        assert!(s.chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c)));
    }
}
