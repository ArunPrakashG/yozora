//! The bar's live view of the desktop, and its controls.

use serde_json::json;

use crate::{ask, out::Out};

pub fn state(args: Vec<String>, out: &Out) -> anyhow::Result<()> {
    out.value(&ask("bar", "state", &args)?);
    Ok(())
}

/// `media`, `volume` and `profile`: the bar answers with the state after the change.
pub fn simple(cmd: &str, args: Vec<String>, out: &Out) -> anyhow::Result<()> {
    out.value(&ask("bar", cmd, &args)?);
    Ok(())
}

pub fn bar(mut args: Vec<String>, out: &Out) -> anyhow::Result<()> {
    anyhow::ensure!(!args.is_empty(), "bar takes toggle MODULE [MONITOR], close or reload");
    let cmd = args.remove(0);
    match cmd.as_str() {
        "toggle" | "open" => {
            anyhow::ensure!(!args.is_empty(), "bar toggle needs a module, like notifications");
            ask("bar", "toggle", &args)?;
            out.done(format!("toggled {}", args[0]), json!({ "toggled": args[0] }));
        }
        "close" => {
            ask("bar", "close", &[])?;
            out.done("closed", json!({ "closed": true }));
        }
        "reload" => out.value(&ask("bar", "reload", &[])?),
        other => anyhow::bail!("bar takes toggle, close or reload, not {other:?}"),
    }
    Ok(())
}
