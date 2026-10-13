//! Hyprland over its two sockets: events from `.socket2.sock`, queries and dispatches on
//! `.socket.sock`.
//!
//! Hyprland 0.56 configures with Lua, and its `dispatch` takes Lua expressions. Older releases
//! take the classic `dispatch workspace 3` form. We try Lua first and fall back for good if
//! Hyprland rejects it.

use std::{
    io::{Read, Write},
    os::unix::net::UnixStream,
    path::PathBuf,
    time::Duration,
};

use anyhow::Context as _;
use async_io::{Async, Timer};
use futures_lite::{AsyncBufReadExt, FutureExt, StreamExt, io::BufReader};
use gpui::{App, Entity};
use serde_json::Value;

use super::{Hyprland, Link};

#[derive(Clone, Debug, Default)]
pub struct State {
    pub monitors: Vec<Monitor>,
    pub workspaces: Vec<Workspace>,
    pub window: Option<Window>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Monitor {
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub focused: bool,
    pub active_workspace: i32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Workspace {
    pub id: i32,
    pub name: String,
    pub monitor: String,
    pub windows: u32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Window {
    pub title: String,
    pub class: String,
    pub workspace: i32,
}

impl State {
    pub fn monitor(&self, name: &str) -> Option<&Monitor> {
        self.monitors.iter().find(|m| m.name == name)
    }

    pub fn occupied(&self, id: i32) -> bool {
        self.workspaces.iter().any(|w| w.id == id && w.windows > 0)
    }

    /// The window to show on `monitor`'s bar: the focused window if it is on that monitor's
    /// active workspace.
    pub fn window_on(&self, monitor: &str) -> Option<&Window> {
        let m = self.monitor(monitor)?;
        self.window.as_ref().filter(|w| w.workspace == m.active_workspace)
    }
}

pub enum Cmd {
    Workspace(i32),
}

pub fn start(cx: &mut App) -> Entity<Hyprland> {
    super::spawn(cx, "hyprland", false, |link| super::supervise(link, run))
}

pub fn socket_dir() -> anyhow::Result<PathBuf> {
    let sig = std::env::var("HYPRLAND_INSTANCE_SIGNATURE")
        .context("HYPRLAND_INSTANCE_SIGNATURE is not set; is Hyprland running?")?;
    let runtime = std::env::var("XDG_RUNTIME_DIR").context("XDG_RUNTIME_DIR is not set")?;
    Ok(PathBuf::from(runtime).join("hypr").join(sig))
}

/// One request on the command socket.
pub fn request(command: &str) -> anyhow::Result<String> {
    let path = socket_dir()?.join(".socket.sock");
    let mut stream =
        UnixStream::connect(&path).with_context(|| format!("connecting to {}", path.display()))?;
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    stream.write_all(command.as_bytes())?;
    let mut reply = String::new();
    stream.read_to_string(&mut reply)?;
    Ok(reply)
}

fn query(what: &str) -> anyhow::Result<Value> {
    let reply = request(&format!("j/{what}"))?;
    serde_json::from_str(&reply).with_context(|| format!("parsing j/{what}"))
}

pub fn snapshot() -> anyhow::Result<State> {
    let int = |v: &Value, k: &str| v[k].as_i64().unwrap_or(0) as i32;
    let monitors = query("monitors")?
        .as_array()
        .map(|a| {
            a.iter()
                .map(|m| Monitor {
                    name: m["name"].as_str().unwrap_or_default().to_string(),
                    x: int(m, "x"),
                    y: int(m, "y"),
                    width: int(m, "width"),
                    height: int(m, "height"),
                    focused: m["focused"].as_bool().unwrap_or(false),
                    active_workspace: int(&m["activeWorkspace"], "id"),
                })
                .collect()
        })
        .unwrap_or_default();
    let mut workspaces: Vec<Workspace> = query("workspaces")?
        .as_array()
        .map(|a| {
            a.iter()
                .map(|w| Workspace {
                    id: int(w, "id"),
                    name: w["name"].as_str().unwrap_or_default().to_string(),
                    monitor: w["monitor"].as_str().unwrap_or_default().to_string(),
                    windows: w["windows"].as_u64().unwrap_or(0) as u32,
                })
                .collect()
        })
        .unwrap_or_default();
    workspaces.sort_by_key(|w| w.id);
    let active = query("activewindow")?;
    let window = active["address"].as_str().map(|_| Window {
        title: active["title"].as_str().unwrap_or_default().to_string(),
        class: active["class"].as_str().unwrap_or_default().to_string(),
        workspace: int(&active["workspace"], "id"),
    });
    Ok(State { monitors, workspaces, window })
}

/// Events that change anything we show.
fn relevant(line: &str) -> bool {
    const EVENTS: &[&str] = &[
        "workspace",
        "createworkspace",
        "destroyworkspace",
        "moveworkspace",
        "renameworkspace",
        "focusedmon",
        "activewindow",
        "windowtitle",
        "openwindow",
        "closewindow",
        "movewindow",
        "monitoradded",
        "monitorremoved",
        "activespecial",
        "configreloaded",
    ];
    let event = line.split(">>").next().unwrap_or_default();
    let event = event.strip_suffix("v2").unwrap_or(event);
    EVENTS.contains(&event)
}

enum Wake {
    Line(Option<std::io::Result<String>>),
    Cmd(Option<Cmd>),
}

async fn run(link: &Link<State, Cmd>) -> anyhow::Result<()> {
    let path = socket_dir()?.join(".socket2.sock");
    let events = Async::<UnixStream>::connect(&path)
        .await
        .with_context(|| format!("connecting to {}", path.display()))?;
    let mut lines = BufReader::new(events).lines();
    let mut lua = true;
    let mut last = None;
    publish(link, &mut last)?;

    loop {
        let wake = async { Wake::Line(lines.next().await) }
            .or(async { Wake::Cmd(link.cmds.recv().await.ok()) })
            .await;
        match wake {
            Wake::Line(None) => anyhow::bail!("Hyprland closed the event socket"),
            Wake::Line(Some(line)) => {
                if !relevant(&line?) {
                    continue;
                }
                // Events come in bursts (a workspace switch sends five or six). Take what
                // arrives in the next 2 ms, then query once.
                let burst = async {
                    while let Some(Ok(_)) = lines.next().await {}
                };
                burst.or(async { Timer::after(Duration::from_millis(2)).await; }).await;
                publish(link, &mut last)?;
            }
            Wake::Cmd(None) => return Ok(()),
            Wake::Cmd(Some(Cmd::Workspace(id))) => dispatch_workspace(id, &mut lua),
        }
    }
}

fn publish(link: &Link<State, Cmd>, last: &mut Option<(Vec<Monitor>, Vec<Workspace>, Option<Window>)>) -> anyhow::Result<()> {
    let state = snapshot()?;
    let key = (state.monitors.clone(), state.workspaces.clone(), state.window.clone());
    if last.as_ref() != Some(&key) {
        *last = Some(key);
        link.publish(state);
    }
    Ok(())
}

fn dispatch_workspace(id: i32, lua: &mut bool) {
    if *lua {
        match request(&format!("dispatch hl.dsp.focus({{ workspace = {id} }})")) {
            Ok(reply) if reply.trim() == "ok" => return,
            Ok(reply) => {
                tracing::info!("Hyprland rejected Lua dispatch ({}); using classic dispatch", reply.trim());
                *lua = false;
            }
            Err(e) => {
                tracing::warn!("dispatch failed: {e:#}");
                return;
            }
        }
    }
    match request(&format!("dispatch workspace {id}")) {
        Ok(reply) if reply.trim() == "ok" => {}
        Ok(reply) => tracing::warn!("dispatch workspace {id}: {}", reply.trim()),
        Err(e) => tracing::warn!("dispatch failed: {e:#}"),
    }
}

/// One line for `yozora-bar check`.
pub fn check() -> anyhow::Result<String> {
    let s = snapshot()?;
    let names: Vec<_> = s.monitors.iter().map(|m| m.name.as_str()).collect();
    Ok(format!("socket2 · {} monitor(s): {}", names.len(), names.join(", ")))
}

#[cfg(test)]
mod tests {
    #[test]
    fn filters_events() {
        assert!(super::relevant("workspacev2>>3,3"));
        assert!(super::relevant("activewindow>>kitty,~"));
        assert!(!super::relevant("activelayout>>kbd,English"));
    }
}
