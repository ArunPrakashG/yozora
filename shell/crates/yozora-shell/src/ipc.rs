//! The control sockets the `yozora` command talks to. The shell answers where the separate bar,
//! launcher and settings apps used to, so the same commands drive it:
//!
//! * `bar`: ping, reload, toggle, close, stats, bench, osd, state, media, volume, profile, quit
//! * `launch`: ping, open, toggle, close, status, quit
//! * `settings`: ping, focus (also `page`)
//!
//! `yozora-shell msg COMMAND [ARGS]` and `yozora-shell osd ...` send to the `bar` socket, the way
//! `yozora-bar msg` did.

use std::process::ExitCode;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::json;
use yozora_config::paths;
use yozora_ipc::{Incoming, Request, Response, Server};

use crate::shared::Shared;
use crate::stage::Pop;
use crate::{desktop, osd, settings};

const SOCKETS: [&str; 3] = ["bar", "launch", "settings"];

/// The popover a bar module name opens, the key of the bar icon it grows from, and for a
/// `module/<name>` the module. Falls back to the control center's icon when the bar has none.
fn module(name: &str) -> Option<(Pop, Vec<String>, Option<String>)> {
    let name = match name {
        "wifi" => "network",
        "calendar" => "clock",
        other => other,
    };
    let (pop, key, module) = crate::bar_modules::toggle_target(name)?;
    Some((pop, vec![key, "cc".to_owned()], module))
}

/// `tray:N` or `tray:NAME`: the menu of the Nth tray item, or the first whose title contains NAME.
/// Asks the tray for the menu and answers with the bar icon the popover grows from.
fn tray_item(sh: &Shared, name: &str) -> Option<String> {
    let which = name.strip_prefix("tray:")?;
    let svc = sh.services.lock().unwrap();
    let tray = svc.tray.as_ref()?;
    let state = tray.read();
    let index = match which.parse::<usize>() {
        Ok(n) => n,
        Err(_) => state.state.items.iter().position(|i| i.title.to_lowercase().contains(&which.to_lowercase()))?,
    };
    let item = state.state.items.get(index)?;
    tray.send(yozora_services::tray::Cmd::LoadMenu(item.key.clone()));
    Some(format!("tray{index}"))
}

pub fn start(sh: &Arc<Shared>) {
    let started = Instant::now();
    for id in SOCKETS {
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
                match id {
                    "bar" => bar(&sh, incoming, started),
                    "launch" => {
                        let reply = launch(&sh, &incoming);
                        incoming.reply(reply);
                    }
                    _ => {
                        let reply = settings_socket(&sh, &incoming);
                        incoming.reply(reply);
                    }
                }
            }
        });
    }
}

/// The warnings the configuration files raise, as the bar's `ping` lists them.
fn warnings() -> Vec<String> {
    let dir = paths::config_dir();
    let mut out = Vec::new();
    match yozora_config::shell::load(&dir) {
        Ok(l) => out.extend(l.warnings.iter().map(|w| w.to_string())),
        Err(e) => out.push(format!("{e:#}")),
    }
    match yozora_config::bar::load(&dir) {
        Ok(l) => out.extend(l.warnings.iter().map(|w| w.to_string())),
        Err(e) => out.push(format!("{e:#}")),
    }
    out
}

fn ping() -> Response {
    Response::data(json!({ "version": env!("CARGO_PKG_VERSION"), "warnings": warnings() }))
}

fn rss_kib() -> Option<u64> {
    std::fs::read_to_string("/proc/self/status").ok()?.lines().find_map(|l| l.strip_prefix("VmRSS:")?.trim().strip_suffix("kB")?.trim().parse().ok())
}

/// The output a command means: the one named, else the focused one, else the first.
fn output_for(sh: &Shared, named: Option<&String>) -> Option<String> {
    let outputs = sh.outputs();
    named
        .filter(|n| outputs.contains(n))
        .cloned()
        .or_else(|| desktop::focused_output(sh).filter(|n| outputs.contains(n)))
        .or_else(|| outputs.first().cloned())
}

fn surface(s: &rune::SurfaceStats) -> serde_json::Value {
    json!({
        "name": s.name,
        "pacing": s.pacing.to_string(),
        "redraws_per_s": s.per_second.redraws,
        "presents_per_s": s.per_second.presents,
        "presents": s.totals.presents,
        "redraws": s.totals.redraws,
        "skipped": s.totals.skipped,
        "failed_acquires": s.totals.failed(),
    })
}

fn bar(sh: &Arc<Shared>, i: Incoming, started: Instant) {
    let args = i.request.args.clone();
    let reply = match i.request.cmd.as_str() {
        "ping" => ping(),
        "reload" => {
            sh.set_config(crate::config::load());
            crate::bar_config::apply(sh);
            Response::data(json!({ "warnings": warnings() }))
        }
        "toggle" => {
            let Some(name) = args.first() else {
                return i.reply(Response::error("toggle needs a module with a popover, like control-center"));
            };
            let tray = tray_item(sh, name);
            let Some((pop, keys, dynamic)) = tray.map(|k| (Pop::Tray, vec![k], None)).or_else(|| module(name)) else {
                return i.reply(Response::error("toggle needs a module with a popover, like control-center"));
            };
            let Some(output) = output_for(sh, args.get(1)) else { return i.reply(Response::error("no bar is open")) };
            // The first of the module's icons that is on this bar; the popover grows from it.
            let x = keys.iter().find_map(|k| sh.anchor(&output, k)).unwrap_or(0.0);
            match dynamic {
                Some(name) => sh.open_module(&output, &name, x),
                None => sh.open(&output, pop, x),
            }
            Response::ok()
        }
        "close" => {
            sh.close();
            Response::ok()
        }
        "stats" => {
            let stages = sh.stage_handles();
            let bars = sh.bar_handles();
            let frames: Vec<_> = stages
                .iter()
                .enumerate()
                .map(|(n, (name, stage))| json!({ "monitor": name, "bar": bars.get(n).map(|b| surface(&b.surface_stats())), "stage": surface(&stage.surface_stats()) }))
                .collect();
            Response::data(json!({
                "version": env!("CARGO_PKG_VERSION"),
                "uptime_s": started.elapsed().as_secs(),
                "rss_kib": rss_kib(),
                "bars": sh.outputs(),
                "frames": frames,
            }))
        }
        "bench" => {
            // It animates for a while: answer from its own thread so the socket stays free.
            let sh = sh.clone();
            let _ = std::thread::Builder::new().name("ipc-bench".into()).spawn(move || {
                let what = args.first().map(String::as_str).unwrap_or("popover");
                let count = args.get(1).and_then(|n| n.parse().ok()).unwrap_or(10u32).clamp(1, 200);
                i.reply(bench(&sh, what, count, args.get(2)));
            });
            return;
        }
        "osd" => osd_command(sh, &args),
        "state" => desktop::state(sh, &args),
        "media" => desktop::media(sh, &args),
        "volume" => desktop::volume(sh, &args),
        "profile" => desktop::profile(sh, &args),
        "quit" => {
            i.reply(Response::ok());
            std::thread::sleep(Duration::from_millis(100));
            for id in SOCKETS {
                let _ = std::fs::remove_file(paths::socket_path(id));
            }
            std::process::exit(0);
        }
        other => Response::error(format!("unknown command {other:?}")),
    };
    i.reply(reply);
}

/// `osd brightness [+N%|-N%|N%]` changes the brightness and shows it; `osd volume`, `mic` and
/// `layout` show the current value.
fn osd_command(sh: &Arc<Shared>, args: &[String]) -> Response {
    match args.first().map(String::as_str) {
        Some("brightness") => match sh.osd.brightness(sh, args.get(1).map(String::as_str)) {
            Ok(level) => Response::data(json!({ "brightness": (level * 100.0).round() })),
            Err(e) => Response::error(e),
        },
        Some(what) => match sh.osd.current(sh, what) {
            Ok(shown) => {
                sh.osd.show(sh, shown, false);
                Response::ok()
            }
            Err(e) => Response::error(e),
        },
        None => Response::error("osd needs brightness, volume, mic or layout"),
    }
}

/// `bench [popover|osd] [N] [MONITOR]`: opens and closes the control center (or flashes the
/// display) N times on a monitor's stage and reports how many frames the surface drew.
///
/// Rune counts presents and redraws per surface and keeps a per-second rate for as long as the
/// surface is busy, but it does not expose a timestamp per frame. So this reports the frames
/// drawn over the run and the highest rate seen while animating, which is the frame rate the
/// surface held, not a frame-time histogram. The separate bar's `workspaces` benchmark has no
/// counterpart: the Rune bar has no workspace indicator animation to sweep.
fn bench(sh: &Arc<Shared>, what: &str, count: u32, monitor: Option<&String>) -> Response {
    if !matches!(what, "popover" | "osd") {
        return Response::error(format!("unknown benchmark {what:?}; try popover or osd"));
    }
    let Some(output) = output_for(sh, monitor) else { return Response::error("no bar is open") };
    let Some((_, stage)) = sh.stage_handles().into_iter().find(|(n, _)| *n == output) else { return Response::error("no bar is open") };
    let before = stage.surface_stats();
    let started = Instant::now();
    let mut peak = 0u64;
    let mut sample = |ms: u64| {
        let until = Instant::now() + Duration::from_millis(ms);
        while Instant::now() < until {
            peak = peak.max(stage.surface_stats().per_second.presents);
            std::thread::sleep(Duration::from_millis(40));
        }
    };
    sh.close();
    sample(700);
    for n in 0..count {
        if what == "popover" {
            sh.open(&output, Pop::Control, sh.anchor(&output, "cc").unwrap_or(0.0));
            sample(900);
            sh.close();
            sample(700);
        } else {
            let level = ((n * 7) % 100) as f32 / 100.0;
            sh.osd.show(sh, osd::Shown::Volume { level, muted: false }, false);
            sample(450);
        }
    }
    let after = stage.surface_stats();
    Response::data(json!({
        "benchmark": what,
        "runs": count,
        "monitor": output,
        "seconds": (started.elapsed().as_secs_f32() * 10.0).round() / 10.0,
        "frames_presented": after.totals.presents - before.totals.presents,
        "frames_requested": after.totals.redraws - before.totals.redraws,
        "frames_skipped": after.totals.skipped - before.totals.skipped,
        "failed_acquires": after.totals.failed() - before.totals.failed(),
        "peak_presents_per_s": peak,
        "pacing": after.pacing.to_string(),
        "note": "Rune reports frame counts and a per-second rate, not a time per frame",
    }))
}

fn launch(sh: &Arc<Shared>, i: &Incoming) -> Response {
    let query = i.request.args.first().cloned();
    match i.request.cmd.as_str() {
        "ping" => Response::ok(),
        "open" | "toggle" => {
            let toggle = i.request.cmd == "toggle";
            // Decided on the stage itself, so back to back requests see each other.
            let output = output_for(sh, None);
            sh.on_stage(output.as_deref(), move |s| {
                if toggle || s.open != Some(Pop::Launcher) {
                    s.toggle(Pop::Launcher, 0.0);
                }
                if let (Some(q), true) = (query, s.open == Some(Pop::Launcher)) {
                    crate::launcher::set_query(s, &q);
                }
            });
            Response::ok()
        }
        "close" | "quit" => {
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
            let output = output_for(sh, None);
            sh.on_stage(output.as_deref(), move |s| {
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

const HELP: &str = "\
Yozora shell: the bar, panels, launcher and Settings in one process

USAGE
    yozora-shell
    yozora-shell msg COMMAND [ARGS]
    yozora-shell osd brightness [+N%|-N%|N%]
    yozora-shell --version

COMMANDS
    msg       Send a command to the running shell:
                toggle MODULE [MONITOR]   open or close a module's popover;
                                          tray:N or tray:NAME opens a tray menu
                close                     close any open popover
                reload                    re-read the configuration
                stats                     frame counts and memory
                bench [popover|osd] [N] [MONITOR]
                                          animate N times and report frames drawn
                osd brightness|volume|mic|layout
                                          show the OSD (same as `yozora-shell osd`)
                state [SECTION...]        the desktop's state as JSON
                media, volume, profile    the same controls `yozora` has
                quit
    osd       Change the brightness and show it, or show the volume, mic or keyboard
              layout. Bind brightness keys to `yozora-shell osd brightness +5%`.

ENVIRONMENT
    YOZORA_RUNTIME_DIR   Where the control sockets live (default $XDG_RUNTIME_DIR/yozora).
    YOZORA_CONFIG_DIR    Where the configuration lives (default ~/.config/yozora).
";

/// The command-line side: `msg`, `osd`, `--version` and `--help`. Returns `None` for anything
/// else, which starts the shell.
pub fn cli(args: &[String]) -> Option<ExitCode> {
    let first = args.first()?.as_str();
    let (cmd, rest): (String, Vec<String>) = match first {
        "-h" | "--help" | "help" => {
            print!("{HELP}");
            return Some(ExitCode::SUCCESS);
        }
        "-V" | "--version" => {
            println!("yozora-shell {}", env!("CARGO_PKG_VERSION"));
            return Some(ExitCode::SUCCESS);
        }
        "msg" => {
            let Some(cmd) = args.get(1) else {
                eprintln!("yozora-shell: msg needs a command; try `yozora-shell msg reload`");
                return Some(ExitCode::from(2));
            };
            (cmd.clone(), args[2..].to_vec())
        }
        "osd" => {
            if args.len() < 2 {
                eprintln!("yozora-shell: osd needs brightness, volume, mic or layout");
                return Some(ExitCode::from(2));
            }
            ("osd".into(), args[1..].to_vec())
        }
        _ => return None,
    };
    let path = paths::socket_path("bar");
    Some(match yozora_ipc::request(&path, &Request::new(cmd, rest)) {
        Ok(r) if r.ok => {
            if !r.data.is_null() {
                println!("{}", serde_json::to_string_pretty(&r.data).unwrap_or_default());
            }
            ExitCode::SUCCESS
        }
        Ok(r) => {
            eprintln!("yozora-shell: {}", r.error.unwrap_or_else(|| "request failed".into()));
            ExitCode::FAILURE
        }
        Err(e) => {
            eprintln!("yozora-shell: is the shell running? ({}: {e})", path.display());
            ExitCode::FAILURE
        }
    })
}
