//! The control sockets the `yozora` command talks to. The shell answers where the separate bar,
//! launcher and settings apps used to, so the same commands drive it.

use std::sync::Arc;
use std::time::Instant;

use serde_json::json;
use yozora_config::paths;
use yozora_ipc::{Incoming, Response, Server};

use crate::settings;
use crate::shared::Shared;
use crate::stage::Pop;

/// The popover a bar module name opens, and the bar icon it grows from.
fn module(name: &str) -> Option<(Pop, &'static str)> {
    Some(match name {
        "launcher" | "menu" => (Pop::Launcher, ""),
        "media" => (Pop::Media, "media"),
        "tray" => (Pop::Tray, "tray0"),
        "network" | "wifi" => (Pop::Wifi, "wifi"),
        "sound" | "audio" => (Pop::Sound, "vol"),
        "devices" | "bluetooth" => (Pop::Bluetooth, "bt"),
        "notifications" => (Pop::Notifications, "bell"),
        "clock" | "calendar" => (Pop::Calendar, "clock"),
        "control-center" => (Pop::Control, "cc"),
        _ => return None,
    })
}

pub fn start(sh: &Arc<Shared>) {
    let started = Instant::now();
    for id in ["bar", "launch", "settings"] {
        let server = match Server::bind(&paths::socket_path(id)) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("{id} socket: {e:#}");
                continue;
            }
        };
        let (tx, rx) = async_channel::unbounded();
        let _ = std::thread::Builder::new().name(format!("ipc-{id}")).spawn(move || async_io::block_on(server.run(tx)));
        let sh = sh.clone();
        let _ = std::thread::Builder::new().name(format!("ipc-{id}-reply")).spawn(move || {
            while let Ok(incoming) = rx.recv_blocking() {
                let reply = match id {
                    "bar" => bar(&sh, &incoming, started),
                    "launch" => launch(&sh, &incoming),
                    _ => settings_socket(&sh, &incoming),
                };
                incoming.reply(reply);
            }
        });
    }
}

fn ping() -> Response {
    Response::data(json!({ "version": env!("CARGO_PKG_VERSION"), "warnings": [] }))
}

fn rss_kib() -> Option<u64> {
    std::fs::read_to_string("/proc/self/status").ok()?.lines().find_map(|l| l.strip_prefix("VmRSS:")?.trim().strip_suffix("kB")?.trim().parse().ok())
}

fn bar(sh: &Arc<Shared>, i: &Incoming, started: Instant) -> Response {
    let args = &i.request.args;
    match i.request.cmd.as_str() {
        "ping" => ping(),
        "reload" => {
            sh.set_config(crate::config::load());
            Response::data(json!({ "warnings": [] }))
        }
        "toggle" => {
            let Some((pop, key)) = args.first().and_then(|n| module(n)) else {
                return Response::error("toggle needs a module with a popover, like control-center");
            };
            let output = args.get(1).cloned();
            let target = output.clone().or_else(|| sh.outputs().first().cloned());
            let Some(name) = target else { return Response::error("no bar is open") };
            let x = sh.anchor(&name, key).unwrap_or(f32::MAX);
            if pop == Pop::Launcher {
                sh.on_stage(Some(&name), |s| s.toggle(Pop::Launcher, 0.0));
            } else {
                sh.open(&name, pop, if x == f32::MAX { 0.0 } else { x });
            }
            Response::ok()
        }
        "close" => {
            sh.close();
            Response::ok()
        }
        "stats" => Response::data(json!({
            "version": env!("CARGO_PKG_VERSION"),
            "uptime_s": started.elapsed().as_secs(),
            "rss_kib": rss_kib(),
            "bars": sh.outputs(),
        })),
        other => Response::error(format!("the shell does not answer `{other}`")),
    }
}

fn launch(sh: &Arc<Shared>, i: &Incoming) -> Response {
    let query = i.request.args.first().cloned();
    match i.request.cmd.as_str() {
        "ping" => Response::ok(),
        "open" | "toggle" => {
            let toggle = i.request.cmd == "toggle";
            let was_open = sh.shown() == Some(Pop::Launcher);
            if was_open && toggle {
                sh.close();
            } else {
                sh.on_stage(None, move |s| {
                    if s.open != Some(Pop::Launcher) {
                        s.toggle(Pop::Launcher, 0.0);
                    }
                    if let Some(q) = query {
                        s.launcher.query = q;
                        crate::launcher::search(s);
                    }
                });
            }
            Response::ok()
        }
        "close" => {
            sh.close();
            Response::ok()
        }
        "status" => Response::data(json!({ "open": sh.shown() == Some(Pop::Launcher) })),
        other => Response::error(format!("unknown command {other:?}")),
    }
}

fn settings_socket(sh: &Arc<Shared>, i: &Incoming) -> Response {
    match i.request.cmd.as_str() {
        "ping" => ping(),
        "focus" | "page" => {
            let page = match i.request.args.first() {
                Some(name) => match settings::page_named(name) {
                    Some(p) => Some(p),
                    None => return Response::error(format!("no settings page called {name:?}")),
                },
                None => None,
            };
            sh.on_stage(None, move |s| {
                if s.open != Some(Pop::Settings) {
                    s.toggle(Pop::Settings, 0.0);
                }
                if let Some(p) = page {
                    s.settings_page = p;
                }
            });
            Response::ok()
        }
        other => Response::error(format!("unknown command `{other}`; try ping or focus")),
    }
}
