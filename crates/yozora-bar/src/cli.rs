//! Command-line parsing. Small enough not to need a parser crate.

use std::path::PathBuf;

use crate::services::Fakes;

pub const HELP: &str = "\
Yozora Bar — a top panel for Hyprland

USAGE
    yozora-bar [run] [--config DIR] [--fake[=LIST]]
    yozora-bar msg COMMAND [ARGS…]
    yozora-bar check [--config DIR]
    yozora-bar --version

COMMANDS
    run       Start the bar (the default).
    msg       Send a command to the running bar:
                toggle MODULE [MONITOR]   open or close a module's popover;
                                          tray:N or tray:NAME opens a tray menu
                close                     close any open popover
                reload                    re-read the configuration
                stats                     frame timing and memory
                bench [workspaces|popover] [N]
                                          animate N times and report frame timing
                quit
    check     Validate the configuration and probe system services.

OPTIONS
    --config DIR   Read configuration from DIR instead of ~/.config/yozora.
    --fake[=LIST]  Use fake backends. LIST is comma-separated from network, bluetooth,
                   power, audio, media, notifications, or all. Without a list, fakes
                   network, bluetooth and power, the hardware a desktop often lacks.

ENVIRONMENT
    YOZORA_LOG     Log filter, like `info` or `yozora_bar=debug`.
";

#[derive(Debug)]
pub enum Command {
    Run { config: Option<PathBuf>, fakes: Fakes },
    Msg { cmd: String, args: Vec<String> },
    Check { config: Option<PathBuf> },
    Version,
    Help,
}

pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Command, String> {
    let mut args = args.into_iter().peekable();
    let mut sub = None;
    let mut config = None;
    let mut fakes = Fakes::default();
    let mut rest = Vec::new();
    while let Some(arg) = args.next() {
        if sub.as_deref() == Some("msg") {
            rest.push(arg);
            continue;
        }
        match arg.as_str() {
            "-h" | "--help" => return Ok(Command::Help),
            "-V" | "--version" => return Ok(Command::Version),
            "--config" => config = Some(PathBuf::from(args.next().ok_or("--config needs a directory")?)),
            "--fake" => fakes = Fakes(Fakes::DEFAULT.iter().map(|s| s.to_string()).collect()),
            // Old spelling from the design mockups.
            "--check" => sub = Some("check".to_string()),
            a if a.starts_with("--config=") => config = Some(PathBuf::from(&a["--config=".len()..])),
            a if a.starts_with("--fake=") => {
                let mut set = std::collections::BTreeSet::new();
                for name in a["--fake=".len()..].split(',').map(str::trim).filter(|s| !s.is_empty()) {
                    if name == "all" {
                        set.extend(Fakes::ALL.iter().map(|s| s.to_string()));
                    } else if Fakes::ALL.contains(&name) {
                        set.insert(name.to_string());
                    } else {
                        return Err(format!("unknown fake backend {name:?}; expected one of {}", Fakes::ALL.join(", ")));
                    }
                }
                fakes = Fakes(set);
            }
            "run" | "msg" | "check" if sub.is_none() => sub = Some(arg),
            a if a.starts_with('-') => return Err(format!("unknown option {a}")),
            a => return Err(format!("unexpected argument {a:?}")),
        }
    }
    match sub.as_deref() {
        None | Some("run") => Ok(Command::Run { config, fakes }),
        Some("check") => Ok(Command::Check { config }),
        Some("msg") => {
            let mut rest = rest.into_iter();
            let cmd = rest.next().ok_or("msg needs a command; try `yozora-bar msg reload`")?;
            Ok(Command::Msg { cmd, args: rest.collect() })
        }
        Some(other) => Err(format!("unknown command {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(args: &[&str]) -> Result<Command, String> {
        parse(args.iter().map(|s| s.to_string()))
    }

    #[test]
    fn parses() {
        assert!(matches!(p(&[]), Ok(Command::Run { config: None, .. })));
        match p(&["--fake=media,audio"]) {
            Ok(Command::Run { fakes, .. }) => assert!(fakes.has("media") && !fakes.has("network")),
            other => panic!("{other:?}"),
        }
        match p(&["msg", "toggle", "control-center", "--x"]) {
            Ok(Command::Msg { cmd, args }) => assert_eq!((cmd.as_str(), args.len()), ("toggle", 2)),
            other => panic!("{other:?}"),
        }
        assert!(p(&["--fake=wifi"]).is_err());
        assert!(matches!(p(&["check", "--config", "/tmp"]), Ok(Command::Check { config: Some(_) })));
    }
}
