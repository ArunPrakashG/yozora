//! Settings, read and written the way Yozora Settings does: through each app's schema, keeping the
//! file's comments, and never printing a secret. Apps watch their files, so changes apply live.

use std::io::BufRead;

use serde_json::{Value, json};
use yozora_config::{
    edit::{self, ConfigFile},
    manifest::{self, AppManifest},
    paths,
    schema::{Field, FieldKind, Schema, display_value},
};

use crate::out::Out;

pub fn run(mut args: Vec<String>, out: &Out) -> anyhow::Result<()> {
    anyhow::ensure!(!args.is_empty(), "config takes list, get, set, reset or path");
    let sub = args.remove(0);
    match (sub.as_str(), args.as_slice()) {
        ("list" | "ls", []) => list(None, out),
        ("list" | "ls", [app]) => list(Some(app), out),
        ("get", [app, key]) => get(app, key, out),
        ("set", [app, key, value]) => set(app, key, value, out),
        ("reset" | "unset", [app, key]) => reset(app, key, out),
        ("path", [app]) => {
            let path = if app == "theme" { paths::config_dir().join("theme.toml") } else { find(app)?.config_path() };
            out.done(path.display().to_string(), json!({ "path": path }));
            Ok(())
        }
        ("list" | "ls" | "get" | "set" | "reset" | "unset" | "path", _) => {
            anyhow::bail!("usage: config list [APP] | get APP KEY | set APP KEY VALUE | reset APP KEY | path APP")
        }
        (other, _) => anyhow::bail!("config takes list, get, set, reset or path, not {other:?}"),
    }
}

fn find(app: &str) -> anyhow::Result<AppManifest> {
    let all = manifest::discover();
    let want = app.to_lowercase();
    let ids: Vec<String> = all.iter().map(|m| m.id.clone()).collect();
    anyhow::ensure!(
        !ids.is_empty(),
        "no Yozora apps are installed: no manifests in {}",
        paths::manifest_dirs().iter().map(|d| d.display().to_string()).collect::<Vec<_>>().join(", ")
    );
    all.into_iter()
        .find(|m| m.id == want || m.name.to_lowercase() == want || m.binary == want)
        .ok_or_else(|| anyhow::anyhow!("no Yozora app {app:?}; installed: {}", ids.join(", ")))
}

fn schema(m: &AppManifest) -> anyhow::Result<Schema> {
    match m.schema_path() {
        Some(p) => Schema::load(&p),
        None => Ok(Schema::default()),
    }
}

fn field<'a>(schema: &'a Schema, key: &str) -> Option<&'a Field> {
    schema.section.iter().flat_map(|s| &s.field).find(|f| f.key == key)
}

/// Splits `monitor."HDMI-A-1".enabled` into its parts.
fn split(dotted: &str) -> Vec<String> {
    let mut parts = vec![String::new()];
    let mut quoted = false;
    for c in dotted.chars() {
        match c {
            '"' => quoted = !quoted,
            '.' if !quoted => parts.push(String::new()),
            c => parts.last_mut().expect("never empty").push(c),
        }
    }
    parts
}

/// The app's file as parsed TOML; empty if it doesn't exist.
fn read(m: &AppManifest) -> anyhow::Result<toml::Table> {
    let path = m.config_path();
    match std::fs::read_to_string(&path) {
        Ok(text) => toml::from_str(&text).map_err(|e| anyhow::anyhow!("{}: {}", path.display(), e.to_string().trim())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(toml::Table::new()),
        Err(e) => Err(e.into()),
    }
}

fn lookup<'a>(table: &'a toml::Table, key: &str) -> Option<&'a toml::Value> {
    let parts = split(key);
    let (last, path) = parts.split_last()?;
    let mut t = table;
    for p in path {
        t = t.get(p)?.as_table()?;
    }
    t.get(last)
}

fn to_json(v: &toml::Value) -> Value {
    serde_json::to_value(v).unwrap_or(Value::Null)
}

/// What a secret shows instead of its value.
fn masked(v: Option<&toml::Value>) -> Value {
    match v {
        Some(toml::Value::String(s)) if !s.is_empty() => json!("(set)"),
        _ => Value::Null,
    }
}

fn field_json(f: &Field, file: &toml::Table) -> Value {
    let current = lookup(file, &f.key);
    let value =
        if f.secret { masked(current) } else { current.or(f.default.as_ref()).map(to_json).unwrap_or(Value::Null) };
    let mut o = json!({
        "key": f.key,
        "label": f.label,
        "kind": format!("{:?}", f.kind).to_lowercase(),
        "value": value,
        "set": current.is_some(),
    });
    let m = o.as_object_mut().expect("an object");
    if !f.secret
        && let Some(d) = &f.default
    {
        m.insert("default".into(), to_json(d));
    }
    if !f.options.is_empty() {
        m.insert("options".into(), json!(f.options.iter().map(to_json).collect::<Vec<_>>()));
    }
    if let Some(v) = f.min {
        m.insert("min".into(), json!(v));
    }
    if let Some(v) = f.max {
        m.insert("max".into(), json!(v));
    }
    if f.secret {
        m.insert("secret".into(), json!(true));
    }
    if let Some(h) = &f.help {
        m.insert("help".into(), json!(h));
    }
    o
}

fn list(app: Option<&String>, out: &Out) -> anyhow::Result<()> {
    let apps = match app {
        Some(a) => vec![find(a)?],
        None => manifest::discover(),
    };
    let mut data = Vec::new();
    let mut lines = Vec::new();
    for m in &apps {
        let schema = schema(m)?;
        let file = read(m)?;
        lines.push(format!("{} ({}, {})", m.name, m.id, m.config_path().display()));
        let mut sections = Vec::new();
        for s in &schema.section {
            lines.push(format!("  {}", s.title));
            let fields: Vec<Value> = s.field.iter().map(|f| field_json(f, &file)).collect();
            for f in &fields {
                lines.push(format!(
                    "    {} = {}{}",
                    f["key"].as_str().unwrap_or_default(),
                    crate::out::scalar(&f["value"]),
                    if f["set"] == true { "" } else { "  (default)" }
                ));
            }
            sections.push(json!({ "title": s.title, "fields": fields }));
        }
        data.push(json!({ "app": m.id, "name": m.name, "file": m.config_path(), "sections": sections }));
    }
    out.lines(&lines, json!(data));
    Ok(())
}

fn get(app: &str, key: &str, out: &Out) -> anyhow::Result<()> {
    let m = find(app)?;
    let schema = schema(&m)?;
    let file = read(&m)?;
    let data = match field(&schema, key) {
        Some(f) => field_json(f, &file),
        None => {
            let v = lookup(&file, key).ok_or_else(|| {
                anyhow::anyhow!("{key:?} isn't set in {} and isn't in its schema", m.config_path().display())
            })?;
            json!({ "key": key, "value": to_json(v), "set": true })
        }
    };
    out.done(crate::out::scalar(&data["value"]), data);
    Ok(())
}

/// Reads VALUE `-` from stdin, so keys stay out of the process list and shell history.
fn value_arg(value: &str) -> anyhow::Result<String> {
    if value != "-" {
        return Ok(value.to_string());
    }
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line)?;
    Ok(line.trim_end_matches(['\n', '\r']).to_string())
}

/// Checks `text` against the field and turns it into the TOML value to write.
pub fn parse(f: &Field, text: &str) -> anyhow::Result<toml::Value> {
    let range = |v: f64| -> anyhow::Result<()> {
        if let Some(min) = f.min {
            anyhow::ensure!(v >= min, "{} is at least {min}", f.key);
        }
        if let Some(max) = f.max {
            anyhow::ensure!(v <= max, "{} is at most {max}", f.key);
        }
        Ok(())
    };
    Ok(match f.kind {
        FieldKind::Bool => toml::Value::Boolean(match text.to_lowercase().as_str() {
            "true" | "on" | "yes" | "1" => true,
            "false" | "off" | "no" | "0" => false,
            _ => anyhow::bail!("{} is true or false", f.key),
        }),
        FieldKind::Int => {
            let v: i64 = text.parse().map_err(|_| anyhow::anyhow!("{} is a whole number", f.key))?;
            range(v as f64)?;
            toml::Value::Integer(v)
        }
        FieldKind::Float => {
            let v: f64 = text.parse().map_err(|_| anyhow::anyhow!("{} is a number", f.key))?;
            range(v)?;
            toml::Value::Float(v)
        }
        FieldKind::Enum => {
            let want = text.to_lowercase();
            let i = (0..f.options.len())
                .find(|&i| {
                    display_value(&f.options[i]).to_lowercase() == want || f.option_label(i).to_lowercase() == want
                })
                .ok_or_else(|| {
                    let all: Vec<String> = f.options.iter().map(display_value).collect();
                    anyhow::anyhow!("{} is one of {}", f.key, all.join(", "))
                })?;
            f.options[i].clone()
        }
        FieldKind::String => toml::Value::String(text.to_string()),
        FieldKind::List => {
            if text.trim_start().starts_with('[') {
                let t: toml::Table = toml::from_str(&format!("v = {text}"))
                    .map_err(|e| anyhow::anyhow!("{} as a TOML array: {}", f.key, e.to_string().trim()))?;
                t["v"].clone()
            } else {
                toml::Value::Array(
                    text.split(',')
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(|s| toml::Value::String(s.into()))
                        .collect(),
                )
            }
        }
    })
}

fn to_edit(v: &toml::Value) -> anyhow::Result<toml_edit::Value> {
    let doc: toml_edit::DocumentMut = format!("v = {v}").parse()?;
    let mut value = doc.get("v").and_then(|i| i.as_value()).cloned().ok_or_else(|| anyhow::anyhow!("bad value"))?;
    value.decor_mut().clear();
    Ok(value)
}

fn set(app: &str, key: &str, value: &str, out: &Out) -> anyhow::Result<()> {
    let m = find(app)?;
    let schema = schema(&m)?;
    let path = m.config_path();
    let f = field(&schema, key).ok_or_else(|| {
        anyhow::anyhow!("{key:?} isn't a setting {} describes; edit {} by hand", m.name, path.display())
    })?;
    let text = value_arg(value)?;
    let v = parse(f, &text)?;
    let mut file = ConfigFile::open(&path)?;
    file.set(key, to_edit(&v)?);
    file.save()?;
    if f.secret {
        edit::restrict(&path)?;
    }
    let shown = if f.secret { masked(Some(&v)) } else { to_json(&v) };
    out.done(
        format!("{}: {key} = {}", m.id, crate::out::scalar(&shown)),
        json!({ "app": m.id, "key": key, "value": shown, "file": path }),
    );
    Ok(())
}

fn reset(app: &str, key: &str, out: &Out) -> anyhow::Result<()> {
    let m = find(app)?;
    let path = m.config_path();
    let mut file = ConfigFile::open(&path)?;
    if file.get(key).is_none() {
        out.done(format!("{key} was already the default"), json!({ "app": m.id, "key": key, "reset": false }));
        return Ok(());
    }
    file.remove(key);
    file.save()?;
    out.done(format!("{}: {key} back to the default", m.id), json!({ "app": m.id, "key": key, "reset": true }));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(kind: FieldKind) -> Field {
        Field {
            key: "a.b".into(),
            kind,
            label: "A".into(),
            help: None,
            default: None,
            options: Vec::new(),
            option_labels: Vec::new(),
            min: Some(1.),
            max: Some(10.),
            step: None,
            secret: false,
        }
    }

    #[test]
    fn values_are_checked_against_the_schema() {
        assert_eq!(parse(&f(FieldKind::Bool), "on").unwrap(), toml::Value::Boolean(true));
        assert!(parse(&f(FieldKind::Bool), "maybe").is_err());
        assert_eq!(parse(&f(FieldKind::Int), "4").unwrap(), toml::Value::Integer(4));
        assert!(parse(&f(FieldKind::Int), "40").is_err());
        assert!(parse(&f(FieldKind::Float), "0.5").is_err());
        let mut e = f(FieldKind::Enum);
        e.options = vec![toml::Value::Integer(30), toml::Value::Integer(34)];
        e.option_labels = vec!["Compact".into(), "Roomy".into()];
        assert_eq!(parse(&e, "roomy").unwrap(), toml::Value::Integer(34));
        assert_eq!(parse(&e, "30").unwrap(), toml::Value::Integer(30));
        assert!(parse(&e, "38").is_err());
        let list = parse(&f(FieldKind::List), "clock, media").unwrap();
        assert_eq!(list, toml::Value::Array(vec!["clock".into(), "media".into()]));
        assert_eq!(parse(&f(FieldKind::List), "[\"x\"]").unwrap(), toml::Value::Array(vec!["x".into()]));
    }

    #[test]
    fn dotted_keys_honour_quotes() {
        assert_eq!(split("monitor.\"HDMI-A-1\".enabled"), ["monitor", "HDMI-A-1", "enabled"]);
        let t: toml::Table = toml::from_str("[monitor.\"DP-1\"]\nenabled = true").unwrap();
        assert_eq!(lookup(&t, "monitor.\"DP-1\".enabled"), Some(&toml::Value::Boolean(true)));
    }

    #[test]
    fn secrets_never_show() {
        assert_eq!(masked(Some(&toml::Value::String("sk-123".into()))), json!("(set)"));
        assert_eq!(masked(Some(&toml::Value::String(String::new()))), Value::Null);
        let mut s = f(FieldKind::String);
        s.secret = true;
        let file: toml::Table = toml::from_str("[a]\nb = \"sk-123\"").unwrap();
        let shown = field_json(&s, &file).to_string();
        assert!(!shown.contains("sk-123"), "{shown}");
    }
}
