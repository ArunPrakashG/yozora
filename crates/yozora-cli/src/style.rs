//! `yozora style` and `yozora fx`: which look the shell wears and how much it moves. They edit
//! `shell.toml`, which the shell watches, so a change shows at once.

use serde_json::json;
use yozora_config::{
    edit::ConfigFile,
    paths,
    shell::{self, FX, LOOKS},
};

use crate::{out::Out, take_flag};

fn file() -> anyhow::Result<ConfigFile> {
    ConfigFile::open(&paths::config_dir().join(shell::FILE))
}

fn current() -> anyhow::Result<shell::ShellConfig> {
    Ok(shell::load(&paths::config_dir())?.value)
}

pub fn style(mut args: Vec<String>, out: &Out) -> anyhow::Result<()> {
    let follow = take_flag(&mut args, "--follow-sun");
    let day = take_flag(&mut args, "--day");
    let night = take_flag(&mut args, "--night");
    let sub = if args.is_empty() { "get".to_owned() } else { args.remove(0) };
    match sub.as_str() {
        "get" => {
            let c = current()?;
            out.done(format!("{} ({}), effects {}", c.look, c.time, c.fx), json!({ "look": c.look, "time": c.time, "fx": c.fx }));
        }
        "list" => out.lines(&LOOKS.iter().map(|s| s.to_string()).collect::<Vec<_>>(), json!({ "looks": LOOKS })),
        "set" => {
            anyhow::ensure!(args.len() == 1, "style set takes a name: {}", LOOKS.join(", "));
            // Daybreak is Surface tension in its day sky.
            let (look, time) = match args[0].to_lowercase().as_str() {
                "daybreak" => ("tension".to_owned(), Some("day")),
                other => (other.to_owned(), None),
            };
            anyhow::ensure!(LOOKS.contains(&look.as_str()), "no style {look:?}; pick one of {}, or daybreak", LOOKS.join(", "));
            anyhow::ensure!([follow, day, night].iter().filter(|b| **b).count() <= 1, "pick one of --follow-sun, --day, --night");
            let time = if follow { Some("follow") } else if day { Some("day") } else if night { Some("night") } else { time };
            let mut f = file()?;
            f.set("look", look.clone());
            if let Some(t) = time {
                f.set("time", t);
            }
            f.save()?;
            let c = current()?;
            out.done(format!("style {} ({})", c.look, c.time), json!({ "look": c.look, "time": c.time }));
        }
        other => anyhow::bail!("style takes get, set NAME [--follow-sun|--day|--night] or list, not {other:?}"),
    }
    Ok(())
}

pub fn fx(mut args: Vec<String>, out: &Out) -> anyhow::Result<()> {
    let sub = if args.is_empty() { "get".to_owned() } else { args.remove(0) };
    match (sub.as_str(), args.as_slice()) {
        ("get", []) => {
            let c = current()?;
            out.done(c.fx.clone(), json!({ "fx": c.fx }));
        }
        ("set", [level]) => {
            anyhow::ensure!(FX.contains(&level.as_str()), "effects are {}", FX.join(", "));
            let mut f = file()?;
            f.set("fx", level.clone());
            f.save()?;
            out.done(format!("effects {level}"), json!({ "fx": level }));
        }
        _ => anyhow::bail!("fx takes get or set {}", FX.join("|")),
    }
    Ok(())
}
