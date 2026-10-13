//! Yozora Settings: one window for every installed Yozora app.
//!
//! Apps are found through their manifests. Settings never talks to an app's internals: it edits
//! the app's TOML file and the app hot-reloads it.

mod app;
mod apps;
mod doc;
mod hypr;
mod pages;
mod view;
mod widgets;

use std::{path::PathBuf, process::ExitCode};

use gpui::App;
use gpui_platform::application;
use tracing_subscriber::EnvFilter;
use yozora_config::paths;
use yozora_ipc::{Request, Server};

pub const SOCKET_ID: &str = "settings";

const HELP: &str = "\
Yozora Settings

Usage: yozora-settings [options]

Options:
  --page <page>     Open a page: appearance, motion, bar, modules, monitors, options, about,
                    or <app>:<page> for another app (for example launch:about)
  --config <dir>    Edit files in <dir> instead of ~/.config/yozora
  --apps <dir>      Also look for app manifests in <dir> (for development)
  -h, --help        Show this help
  -V, --version     Show the version

A second launch focuses the open window and switches to --page.
";

#[derive(Default)]
pub struct Options {
    pub page: Option<String>,
    pub config_dir: PathBuf,
    pub extra_apps: Vec<PathBuf>,
}

fn parse(mut args: impl Iterator<Item = String>) -> Result<Option<Options>, String> {
    let mut opts = Options { config_dir: paths::config_dir(), ..Default::default() };
    while let Some(arg) = args.next() {
        let (flag, inline) = match arg.split_once('=') {
            Some((f, v)) if f.starts_with("--") => (f.to_string(), Some(v.to_string())),
            _ => (arg.clone(), None),
        };
        let mut value = |name: &str| {
            inline.clone().or_else(|| args.next()).ok_or_else(|| format!("{name} needs a value"))
        };
        match flag.as_str() {
            "-h" | "--help" => {
                print!("{HELP}");
                return Ok(None);
            }
            "-V" | "--version" => {
                println!("yozora-settings {}", env!("CARGO_PKG_VERSION"));
                return Ok(None);
            }
            "--page" => opts.page = Some(value("--page")?),
            "--config" => opts.config_dir = PathBuf::from(value("--config")?),
            "--apps" => opts.extra_apps.push(PathBuf::from(value("--apps")?)),
            other => return Err(format!("unknown option `{other}`")),
        }
    }
    Ok(Some(opts))
}

fn main() -> ExitCode {
    let opts = match parse(std::env::args().skip(1)) {
        Ok(Some(o)) => o,
        Ok(None) => return ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("yozora-settings: {e}\n\nRun `yozora-settings --help` for usage.");
            return ExitCode::from(2);
        }
    };
    // Before any thread starts: it changes the environment.
    yozora_ui::gpu::prefer_vulkan();
    yozora_ui::malloc::tune();
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_env("YOZORA_LOG").unwrap_or_else(|_| EnvFilter::new("info")))
        .init();

    // One window: a second launch hands its page to the first and exits.
    let socket = paths::socket_path(SOCKET_ID);
    if yozora_ipc::is_alive(&socket) {
        let args: Vec<String> = opts.page.iter().cloned().collect();
        match yozora_ipc::request(&socket, &Request::new("focus", args)) {
            Ok(r) if r.ok => {
                hypr::focus_settings_window();
                return ExitCode::SUCCESS;
            }
            Ok(r) => eprintln!("yozora-settings: {}", r.error.unwrap_or_default()),
            Err(e) => eprintln!("yozora-settings: the open window didn't answer: {e:#}"),
        }
        return ExitCode::FAILURE;
    }
    let server = match Server::bind(&socket) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("yozora-settings: {e:#}");
            return ExitCode::FAILURE;
        }
    };
    let (tx, rx) = async_channel::unbounded();
    std::thread::Builder::new()
        .name("yz-ipc".into())
        .spawn(move || async_io::block_on(server.run(tx)))
        .expect("spawning the control socket thread");

    application().with_assets(yozora_ui::Assets).run(move |cx: &mut App| {
        app::start(cx, opts, rx);
    });
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(s: &str) -> impl Iterator<Item = String> {
        s.split_whitespace().map(String::from).collect::<Vec<_>>().into_iter()
    }

    #[test]
    fn parses_options() {
        let o = parse(args("--page about --config /tmp/x --apps=/tmp/apps")).unwrap().unwrap();
        assert_eq!(o.page.as_deref(), Some("about"));
        assert_eq!(o.config_dir, PathBuf::from("/tmp/x"));
        assert_eq!(o.extra_apps, vec![PathBuf::from("/tmp/apps")]);
        assert!(parse(args("--bogus")).is_err());
        assert!(parse(args("--page")).is_err());
    }
}
