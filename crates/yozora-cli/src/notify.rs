//! Notifications, through Yozora Notify's control socket. Sending one goes over D-Bus, so it works
//! with any notification server.

use std::collections::HashMap;

use serde_json::{Value, json};

use crate::{ask, out, out::Out, take_flag, take_value};

pub fn run(mut args: Vec<String>, out: &Out) -> anyhow::Result<()> {
    let sub = if args.first().is_some_and(|a| !a.starts_with("--")) { args.remove(0) } else { "list".into() };
    match sub.as_str() {
        "list" | "ls" => list(args, out),
        "show" | "get" => show(args, out),
        "dismiss" | "close" => {
            anyhow::ensure!(!args.is_empty(), "dismiss needs one or more ids");
            let data = ask("notify", "dismiss", &args)?;
            let n = data["dismissed"].as_array().map_or(0, Vec::len);
            out.done(format!("dismissed {n}"), data);
            Ok(())
        }
        "clear" => {
            ask("notify", "clear", &[])?;
            out.done("cleared all but pinned notifications", json!({ "cleared": true }));
            Ok(())
        }
        "pin" | "unpin" => {
            let id = one_id(&args)?;
            let on = if sub == "pin" { "on" } else { "off" };
            let data = ask("notify", "pin", &[id.clone(), on.into()])?;
            out.done(format!("{sub}ned {id}"), data);
            Ok(())
        }
        "snooze" => {
            anyhow::ensure!(args.len() == 2, "snooze takes an id and a number of minutes");
            let data = ask("notify", "snooze", &args)?;
            out.done(format!("snoozed {} for {} min", args[0], args[1]), data);
            Ok(())
        }
        "action" | "open" => {
            anyhow::ensure!(!args.is_empty() && args.len() <= 2, "action takes an id and an optional action key");
            let data = ask("notify", "action", &args)?;
            out.done(format!("invoked {} on {}", data["action"].as_str().unwrap_or("default"), args[0]), data);
            Ok(())
        }
        "send" => send(args, out),
        "status" => {
            out.value(&ask("notify", "status", &[])?);
            Ok(())
        }
        other => anyhow::bail!(
            "notifications takes list, show, dismiss, clear, pin, unpin, snooze, action, send or status, not {other:?}"
        ),
    }
}

fn one_id(args: &[String]) -> anyhow::Result<String> {
    anyhow::ensure!(args.len() == 1 && args[0].parse::<u32>().is_ok(), "needs one notification id");
    Ok(args[0].clone())
}

/// Hides what Jev called private, unless asked.
fn redact(mut n: Value, show_private: bool) -> Value {
    if !show_private && n["private"] == true {
        n["summary"] = json!("(private)");
        n["body"] = Value::Null;
        n["redacted"] = json!(true);
    }
    n
}

fn list(mut args: Vec<String>, out: &Out) -> anyhow::Result<()> {
    let unread = take_flag(&mut args, "--unread");
    let urgent = take_flag(&mut args, "--urgent");
    let show_private = take_flag(&mut args, "--show-private");
    let app = take_value(&mut args, "--app")?.map(|a| a.to_lowercase());
    let limit = take_value(&mut args, "--limit")?.map(|l| l.parse::<usize>()).transpose()?;
    anyhow::ensure!(args.is_empty(), "unexpected {:?}", args[0]);
    let all = ask("notify", "list", &[])?;
    let status = ask("notify", "status", &[]).unwrap_or(Value::Null);
    let items: Vec<Value> = all
        .as_array()
        .into_iter()
        .flatten()
        .filter(|n| !unread || (n["quiet"] != true && n["snoozed_until"].as_u64().unwrap_or(0) == 0))
        .filter(|n| !urgent || n["urgent"] == true || n["urgency"] == "critical")
        .filter(|n| app.as_ref().is_none_or(|a| n["app"].as_str().unwrap_or("").to_lowercase().contains(a)))
        .take(limit.unwrap_or(usize::MAX))
        .cloned()
        .map(|n| redact(n, show_private))
        .collect();
    if out.json {
        out.value(&json!(items));
        return Ok(());
    }
    if items.is_empty() {
        say!("No notifications.");
    }
    for n in &items {
        say!("{}", line(n));
    }
    if status["dnd"] == true {
        say!("Do Not Disturb is on.");
    }
    Ok(())
}

/// `#12  Signal: Sam  "Are you still coming?"  3 min ago  [urgent, pinned]`
fn line(n: &Value) -> String {
    let mut tags = Vec::new();
    if n["urgency"] == "critical" {
        tags.push("critical".to_string());
    }
    if n["urgent"] == true {
        tags.push("urgent".into());
    }
    if n["quiet"] == true {
        tags.push("can wait".into());
    }
    if n["pinned"] == true {
        tags.push("pinned".into());
    }
    if n["snoozed_until"].as_u64().unwrap_or(0) > 0 {
        tags.push("snoozed".into());
    }
    if n["redacted"] == true {
        tags.push("private".into());
    }
    if let Some(k) = n["kind"].as_str().filter(|k| !k.is_empty()) {
        tags.push(k.into());
    }
    let body = n["body"].as_str().unwrap_or("").replace('\n', " ");
    let body: String =
        if body.chars().count() > 80 { format!("{}…", body.chars().take(79).collect::<String>()) } else { body };
    format!(
        "#{:<4} {}: {}{}  {}{}",
        n["id"],
        out::scalar(&n["app"]),
        out::scalar(&n["summary"]),
        if body.is_empty() { String::new() } else { format!("  \"{body}\"") },
        out::ago(n["time"].as_u64().unwrap_or(0)),
        if tags.is_empty() { String::new() } else { format!("  [{}]", tags.join(", ")) },
    )
}

fn show(mut args: Vec<String>, out: &Out) -> anyhow::Result<()> {
    let show_private = take_flag(&mut args, "--show-private");
    let id = one_id(&args)?;
    out.value(&redact(ask("notify", "get", &[id])?, show_private));
    Ok(())
}

pub fn dnd(args: Vec<String>, out: &Out) -> anyhow::Result<()> {
    let data = match args.first() {
        None => {
            let s = ask("notify", "status", &[])?;
            json!({ "dnd": s["dnd"] })
        }
        Some(_) => ask("notify", "dnd", &args)?,
    };
    out.done(format!("Do Not Disturb is {}", if data["dnd"] == true { "on" } else { "off" }), data);
    Ok(())
}

fn send(mut args: Vec<String>, out: &Out) -> anyhow::Result<()> {
    let app = take_value(&mut args, "--app")?.unwrap_or_else(|| "yozora".into());
    let icon = take_value(&mut args, "--icon")?.unwrap_or_default();
    let urgency: u8 = match take_value(&mut args, "--urgency")?.as_deref() {
        None | Some("normal") => 1,
        Some("low") => 0,
        Some("critical") => 2,
        Some(other) => anyhow::bail!("--urgency takes low, normal or critical, not {other:?}"),
    };
    anyhow::ensure!(!args.is_empty() && args.len() <= 2, "send takes a summary and an optional body");
    let summary = args[0].clone();
    let body = args.get(1).cloned().unwrap_or_default();
    let id = async_io::block_on(async {
        let conn = zbus::Connection::session().await?;
        let mut hints: HashMap<&str, zbus::zvariant::Value<'_>> = HashMap::new();
        hints.insert("urgency", urgency.into());
        let reply = conn
            .call_method(
                Some("org.freedesktop.Notifications"),
                "/org/freedesktop/Notifications",
                Some("org.freedesktop.Notifications"),
                "Notify",
                &(app.as_str(), 0u32, icon.as_str(), summary.as_str(), body.as_str(), Vec::<&str>::new(), hints, -1i32),
            )
            .await?;
        anyhow::Ok(reply.body().deserialize::<u32>()?)
    })
    .map_err(|e| anyhow::anyhow!("no notification server answered: {e:#}"))?;
    out.done(format!("sent notification {id}"), json!({ "id": id }));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_ones_are_redacted_unless_asked() {
        let n = json!({ "id": 3, "app": "Messages", "summary": "Code", "body": "482913", "private": true });
        let r = redact(n.clone(), false);
        assert_eq!(r["summary"], "(private)");
        assert_eq!(r["body"], Value::Null);
        assert_eq!(redact(n.clone(), true), n);
        let open = json!({ "id": 4, "summary": "Hi", "body": "there", "private": null });
        assert_eq!(redact(open.clone(), false), open);
    }
}
