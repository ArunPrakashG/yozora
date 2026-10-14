//! Yozora Bar: a top panel for Hyprland.

mod app;
mod bar;
mod cli;
mod desktop;
mod modules;
mod osd;
mod popover;
mod services;
mod tooltip;

use std::{io::Write, path::PathBuf, process::ExitCode, sync::OnceLock, time::Instant};

use gpui::App;
use gpui_platform::application;
use tracing_subscriber::EnvFilter;
use yozora_config::paths;
use yozora_ipc::{Request, Server};

use crate::cli::Command;

/// When the process started, for the startup measurement.
pub static STARTED: OnceLock<Instant> = OnceLock::new();

const SOCKET_ID: &str = "bar";

fn main() -> ExitCode {
    STARTED.get_or_init(Instant::now);
    let command = match cli::parse(std::env::args().skip(1)) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("yozora-bar: {e}\n\nRun `yozora-bar --help` for usage.");
            return ExitCode::from(2);
        }
    };
    match command {
        Command::Help => {
            print!("{}", cli::HELP);
            ExitCode::SUCCESS
        }
        Command::Version => {
            println!("yozora-bar {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Command::Msg { cmd, args } => msg(cmd, args),
        Command::Check { config } => check(config),
        Command::Run { config, fakes } => run(config, fakes),
    }
}

fn config_dir(config: Option<PathBuf>) -> PathBuf {
    config.unwrap_or_else(paths::config_dir)
}

fn msg(cmd: String, args: Vec<String>) -> ExitCode {
    let path = paths::socket_path(SOCKET_ID);
    match yozora_ipc::request(&path, &Request::new(cmd, args)) {
        Ok(r) if r.ok => {
            if !r.data.is_null() {
                println!("{}", serde_json::to_string_pretty(&r.data).unwrap_or_default());
            }
            ExitCode::SUCCESS
        }
        Ok(r) => {
            eprintln!("yozora-bar: {}", r.error.unwrap_or_else(|| "request failed".into()));
            ExitCode::FAILURE
        }
        Err(e) => {
            eprintln!("yozora-bar: is the bar running? ({}: {e})", path.display());
            ExitCode::FAILURE
        }
    }
}

/// Validates the configuration and probes every system service, without opening windows.
fn check(config: Option<PathBuf>) -> ExitCode {
    let dir = config_dir(config);
    let mut problems = 0;
    println!("Configuration: {}", dir.display());
    match app::load(&dir) {
        Ok(loaded) => {
            if loaded.warnings.is_empty() {
                println!("  ok");
            }
            for w in &loaded.warnings {
                problems += 1;
                println!("  warning: {w}");
            }
            let clock = &loaded.value.bar.clock.format;
            if !modules::calendar::valid_format(clock) {
                problems += 1;
                println!("  warning: clock.format {clock:?} is not a valid strftime format");
            }
        }
        Err(e) => {
            problems += 1;
            println!("  error: {e:#}");
        }
    }

    println!("\nServices:");
    let line = |name: &str, r: anyhow::Result<String>, problems: &mut u32| match r {
        Ok(s) => println!("  {name:<14} {s}"),
        Err(e) => {
            *problems += 1;
            println!("  {name:<14} unavailable: {e:#}");
        }
    };
    use services::*;
    line("Hyprland", hyprland::check(), &mut problems);
    line("Sound", audio::check(), &mut problems);
    async_io::block_on(async {
        line("Network", network::check().await, &mut problems);
        line("Power", power::check().await, &mut problems);
        line("Bluetooth", bluetooth::check().await, &mut problems);
        line("Media", media::check().await, &mut problems);
        let notif = notifications::check().await;
        // Only mako needs a mode set up; yozora-notify does Do Not Disturb itself.
        let missing_dnd = notif.as_ref().is_ok_and(|s| s.starts_with("mako") && !s.contains(notifications::DND_MODE));
        line("Notifications", notif, &mut problems);
        if missing_dnd {
            println!(
                "  {:<14} note: mako has no `{}` mode, so Do Not Disturb has no effect.\n  {:<14} Add this to ~/.config/mako/config:\n  {:<14}   [mode={}]\n  {:<14}   invisible=1",
                "",
                notifications::DND_MODE,
                "",
                "",
                notifications::DND_MODE,
                ""
            );
        }
        line("Tray", tray::check().await, &mut problems);
    });

    let socket = paths::socket_path(SOCKET_ID);
    println!(
        "\nControl socket: {} ({})",
        socket.display(),
        if yozora_ipc::is_alive(&socket) { "bar is running" } else { "bar is not running" }
    );
    if problems == 0 {
        println!("\nEverything looks good.");
        ExitCode::SUCCESS
    } else {
        println!("\n{problems} issue(s). Modules whose service is unavailable stay hidden or show why.");
        ExitCode::FAILURE
    }
}

/// `$XDG_STATE_HOME/yozora`, for crash logs.
fn state_dir() -> PathBuf {
    std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))
        .unwrap_or_else(std::env::temp_dir)
        .join("yozora")
}

/// Writes panics to a crash log as well as stderr, so a bar that vanished leaves a trace.
fn install_panic_hook() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let dir = state_dir();
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("bar-crash.log");
        let backtrace = std::backtrace::Backtrace::force_capture();
        let thread = std::thread::current();
        let report = format!(
            "yozora-bar {} crashed at {}\nthread {:?}: {info}\n\n{backtrace}\n",
            env!("CARGO_PKG_VERSION"),
            chrono::Local::now().to_rfc3339(),
            thread.name().unwrap_or("unnamed"),
        );
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
            let _ = f.write_all(report.as_bytes());
            eprintln!("yozora-bar: crash report written to {}", path.display());
        }
        default(info);
    }));
}

fn run(config: Option<PathBuf>, fakes: services::Fakes) -> ExitCode {
    // Before any thread starts: it changes the environment.
    yozora_ui::gpu::prefer_vulkan();
    yozora_ui::malloc::tune();
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_env("YOZORA_LOG").unwrap_or_else(|_| EnvFilter::new("info")))
        .init();
    install_panic_hook();

    let dir = config_dir(config);
    let loaded = match app::load(&dir) {
        Ok(l) => l,
        Err(e) => {
            // A broken file shouldn't leave the user without a bar: start with defaults.
            tracing::error!("configuration: {e:#}; starting with defaults");
            yozora_config::Loaded {
                value: Default::default(),
                warnings: vec![yozora_config::Warning {
                    file: yozora_config::bar::FILE.into(),
                    line: None,
                    message: format!("{e:#}"),
                }],
            }
        }
    };

    // Bind before starting GPUI so a second instance exits right away.
    let server = match Server::bind(&paths::socket_path(SOCKET_ID)) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("yozora-bar: {e:#}");
            return ExitCode::FAILURE;
        }
    };
    let (tx, rx) = async_channel::unbounded();
    std::thread::Builder::new()
        .name("yz-ipc".into())
        .spawn(move || async_io::block_on(server.run(tx)))
        .expect("spawning the control socket thread");

    if !fakes.0.is_empty() {
        tracing::info!("fake backends: {}", fakes.0.iter().cloned().collect::<Vec<_>>().join(", "));
    }
    // A bar with no monitors to show on keeps running and waits for one, so closing the last
    // window must not quit.
    application().with_assets(yozora_ui::Assets).with_quit_mode(gpui::QuitMode::Explicit).run(move |cx: &mut App| {
        app::start(cx, dir, loaded, fakes);
        cx.spawn(async move |cx| {
            while let Ok(incoming) = rx.recv().await {
                cx.update(|cx| app::handle_ipc(incoming, cx));
            }
        })
        .detach();
    });
    ExitCode::SUCCESS
}
