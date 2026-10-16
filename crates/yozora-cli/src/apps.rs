//! Which apps run, and starting the launcher, Settings and the lock screen.

use std::{os::unix::process::CommandExt, process::Stdio, time::Duration};

use serde_json::{Value, json};
use yozora_config::{manifest, paths};
use yozora_ipc::Request;

use crate::{ask, out::Out};

/// Every app the CLI knows by id, with Settings, which has no manifest.
const KNOWN: [(&str, &str); 6] = [
    ("bar", "Yozora Bar"),
    ("notify", "Yozora Notify"),
    ("launch", "Yozora Launch"),
    ("lock", "Yozora Lock"),
    ("idle", "Yozora Idle"),
    ("settings", "Yozora Settings"),
];

/// The apps that run as systemd user services, in the order they're restarted.
const SERVICES: [&str; 4] = ["bar", "notify", "launch", "idle"];

pub fn name(id: &str) -> String {
    KNOWN.iter().find(|(k, _)| *k == id).map_or_else(|| format!("Yozora {id}"), |(_, n)| n.to_string())
}

/// The app's `ping` answer, if it runs.
fn ping(id: &str) -> Option<Value> {
    let r = yozora_ipc::request_timeout(
        &paths::socket_path(id),
        &Request::new("ping", Vec::<String>::new()),
        Duration::from_secs(1),
    )
    .ok()?;
    Some(if r.ok { r.data } else { Value::Null })
}

fn app_json(id: &str) -> Value {
    let p = ping(id);
    let warnings = p.as_ref().and_then(|p| p["warnings"].as_array().cloned()).unwrap_or_default();
    json!({
        "id": id,
        "name": name(id),
        "running": p.is_some(),
        "version": p.as_ref().and_then(|p| p["version"].as_str().map(String::from)),
        "warnings": warnings,
    })
}

pub fn status(out: &Out) -> anyhow::Result<()> {
    let apps: Vec<Value> = KNOWN.iter().map(|(id, _)| app_json(id)).collect();
    let notifications = ask("notify", "status", &[]).ok();
    let launcher_open = ask("launch", "status", &[]).ok().and_then(|v| v["open"].as_bool());
    let locked = apps.iter().any(|a| a["id"] == "lock" && a["running"] == true);
    let data = json!({
        "version": env!("CARGO_PKG_VERSION"),
        "hyprland": std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some(),
        "locked": locked,
        "apps": apps,
        "notifications": notifications,
        "launcher_open": launcher_open,
    });
    if out.json {
        out.value(&data);
        return Ok(());
    }
    let mut lines = vec![format!("Yozora {}{}", env!("CARGO_PKG_VERSION"), if locked { ", locked" } else { "" })];
    for a in &apps {
        let state = if a["running"] == true {
            format!("running {}", a["version"].as_str().unwrap_or_default())
        } else {
            "not running".into()
        };
        lines.push(format!("  {:<16} {state}", a["name"].as_str().unwrap_or_default()));
        for w in a["warnings"].as_array().into_iter().flatten() {
            lines.push(format!("    warning: {}", crate::out::scalar(w)));
        }
    }
    if let Some(n) = &data["notifications"].as_object() {
        lines.push(format!(
            "Notifications: {} in the center{}{}{}",
            n["count"],
            if n["unread"] == true { ", unread" } else { "" },
            if n["dnd"] == true { ", Do Not Disturb on" } else { "" },
            if n["sharing"] == true { ", screen shared" } else { "" },
        ));
    }
    out.lines(&lines, data);
    Ok(())
}

pub fn list(out: &Out) -> anyhow::Result<()> {
    let apps: Vec<Value> = manifest::discover()
        .into_iter()
        .map(|m| {
            let mut a = app_json(&m.id);
            let o = a.as_object_mut().expect("an object");
            o.insert("name".into(), json!(m.name));
            o.insert("description".into(), json!(m.description));
            o.insert("binary".into(), json!(m.binary));
            o.insert("config".into(), json!(m.config_path()));
            o.insert("has_settings".into(), json!(m.settings.is_some()));
            a
        })
        .collect();
    out.value(&json!(apps));
    Ok(())
}

/// Starts `binary` detached, so it outlives this command.
fn spawn(binary: &str, args: &[String]) -> anyhow::Result<()> {
    std::process::Command::new(binary)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .map_err(|e| anyhow::anyhow!("starting {binary}: {e}"))?;
    Ok(())
}

pub fn launcher(mut args: Vec<String>, out: &Out) -> anyhow::Result<()> {
    let cmd = if args.is_empty() { "toggle".to_string() } else { args.remove(0) };
    anyhow::ensure!(
        matches!(cmd.as_str(), "open" | "close" | "toggle" | "status"),
        "launcher takes open [QUERY], close, toggle or status"
    );
    match ask("launch", &cmd, &args) {
        Ok(data) if cmd == "status" => out.value(&data),
        Ok(_) => out.done(format!("launcher: {cmd}"), json!({ "launcher": cmd })),
        // Not running: it starts opened.
        Err(_) if cmd == "open" || cmd == "toggle" => {
            let mut a = vec!["open".to_string()];
            a.extend(args);
            spawn("yozora-launch", &a)?;
            out.done("launcher: open", json!({ "launcher": "open", "started": true }));
        }
        Err(_) if cmd == "status" => out.value(&json!({ "open": false, "running": false })),
        Err(_) => out.done("the launcher isn't running", json!({ "launcher": cmd, "running": false })),
    }
    Ok(())
}

pub fn settings(args: Vec<String>, out: &Out) -> anyhow::Result<()> {
    let mut a = Vec::new();
    if let Some(page) = args.first() {
        a = vec!["--page".to_string(), page.clone()];
    }
    if ping("settings").is_some() {
        ask("settings", "focus", &args)?;
    } else {
        spawn("yozora-settings", &a)?;
    }
    out.done("opened Yozora Settings", json!({ "opened": "settings", "page": args.first() }));
    Ok(())
}

pub fn lock(out: &Out) -> anyhow::Result<()> {
    if ping("lock").is_some() {
        out.done("already locked", json!({ "locked": true }));
        return Ok(());
    }
    spawn("yozora-lock", &[])?;
    out.done("locking", json!({ "locked": true }));
    Ok(())
}

/// Yozora Idle's state: what it's counting down to, and what holds the screen awake.
pub fn idle(out: &Out) -> anyhow::Result<()> {
    out.value(&ask("idle", "status", &[])?);
    Ok(())
}

/// Stops locking, turning the screens off and suspending, or lets them happen again.
pub fn keep_awake(args: Vec<String>, out: &Out) -> anyhow::Result<()> {
    let data = ask("idle", "keep-awake", &args)?;
    out.done(format!("keep awake is {}", if data["keep_awake"] == true { "on" } else { "off" }), data);
    Ok(())
}

/// The service apps to restart: the ones named, or all of them.
fn restart_targets(args: &[String]) -> anyhow::Result<Vec<&'static str>> {
    if args.is_empty() || args.iter().any(|a| a == "all") {
        return Ok(SERVICES.to_vec());
    }
    args.iter()
        .map(|a| {
            let id = a.strip_prefix("yozora-").unwrap_or(a);
            SERVICES.iter().copied().find(|s| *s == id).ok_or_else(|| {
                anyhow::anyhow!(
                    "{a:?} isn't a service app; use one of {} (lock and settings aren't services)",
                    SERVICES.join(", ")
                )
            })
        })
        .collect()
}

/// Restarts Yozora's systemd user services, all of them or the ones named.
pub fn restart(args: Vec<String>, out: &Out) -> anyhow::Result<()> {
    let targets = restart_targets(&args)?;
    let units: Vec<String> = targets.iter().map(|id| format!("yozora-{id}.service")).collect();
    let status = std::process::Command::new("systemctl").args(["--user", "restart"]).args(&units).status()?;
    anyhow::ensure!(status.success(), "systemctl --user restart failed ({status})");
    // Give the apps a moment to come up, then report what's running.
    std::thread::sleep(Duration::from_millis(1500));
    let results: Vec<Value> = targets
        .iter()
        .zip(&units)
        .map(|(id, unit)| {
            let active = std::process::Command::new("systemctl")
                .args(["--user", "is-active", "--quiet", unit])
                .status()
                .is_ok_and(|s| s.success());
            json!({ "id": id, "unit": unit, "active": active })
        })
        .collect();
    let lines: Vec<String> = results
        .iter()
        .map(|r| {
            format!(
                "{:<8} {}",
                r["id"].as_str().unwrap_or(""),
                if r["active"] == true { "running" } else { "not running" }
            )
        })
        .collect();
    out.lines(&lines, json!({ "restarted": results }));
    anyhow::ensure!(
        results.iter().all(|r| r["active"] == true),
        "some services did not come back; see journalctl --user -u yozora-<app>"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::restart_targets;

    fn args(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn restart_defaults_to_every_service() {
        assert_eq!(restart_targets(&[]).unwrap(), ["bar", "notify", "launch", "idle"]);
        assert_eq!(restart_targets(&args(&["all"])).unwrap().len(), 4);
    }

    #[test]
    fn restart_takes_names_with_or_without_prefix() {
        assert_eq!(restart_targets(&args(&["bar", "yozora-idle"])).unwrap(), ["bar", "idle"]);
    }

    #[test]
    fn restart_rejects_non_services() {
        assert!(restart_targets(&args(&["lock"])).is_err());
        assert!(restart_targets(&args(&["nope"])).is_err());
    }
}
