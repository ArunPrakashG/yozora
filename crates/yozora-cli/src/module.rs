//! `yozora module`: check, new and list.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use anyhow::{Context, bail};
use serde_json::json;
use yozora_module::{Manifest, Refresh, Update};

use crate::out::Out;

const EXEC_TIMEOUT: Duration = Duration::from_secs(10);

pub fn run(mut args: Vec<String>, out: &Out) -> anyhow::Result<()> {
    if args.is_empty() {
        bail!("usage: yozora module check PATH [--sample FILE] | new NAME [--dir DIR] | list");
    }
    let sub = args.remove(0);
    match sub.as_str() {
        "check" => {
            let sample = crate::take_value(&mut args, "--sample")?;
            anyhow::ensure!(args.len() == 1, "usage: yozora module check PATH [--sample FILE]");
            let m = check(Path::new(&args[0]), sample.as_deref().map(Path::new))?;
            out.done(
                format!("ok: {} {}", m.name, m.version),
                json!({ "ok": true, "name": m.name, "version": m.version }),
            );
            Ok(())
        }
        "new" => {
            let dir = crate::take_value(&mut args, "--dir")?;
            anyhow::ensure!(args.len() == 1, "usage: yozora module new NAME [--dir DIR]");
            let path = scaffold(&args[0], Path::new(dir.as_deref().unwrap_or(".")))?;
            out.done(
                format!("created {}", path.display()),
                json!({ "name": args[0], "path": path.display().to_string() }),
            );
            Ok(())
        }
        "list" => list(out),
        other => bail!("unknown module command {other:?}; use check, new or list"),
    }
}

fn manifest_path(path: &Path) -> PathBuf {
    if path.is_dir() { path.join("module.toml") } else { path.to_path_buf() }
}

/// 1-based line of byte offset `at`.
fn line_at(src: &str, at: usize) -> usize {
    src[..at.min(src.len())].bytes().filter(|b| *b == b'\n').count() + 1
}

/// Parses a manifest; errors read `FILE:LINE: message`.
fn parse_manifest(file: &Path, src: &str) -> Result<Manifest, String> {
    let name = file.display();
    if let Err(e) = toml::from_str::<Manifest>(src) {
        let line = e.span().map_or(1, |s| line_at(src, s.start));
        return Err(format!("{name}:{line}: {}", e.message()));
    }
    Manifest::parse(src).map_err(|e| {
        // Semantic problem: point at the first field named in backticks.
        let msg = e.0;
        let line = msg
            .split('`')
            .nth(1)
            .and_then(|f| f.rsplit('.').next())
            .and_then(|key| {
                src.lines().position(|l| {
                    let t = l.trim_start();
                    t.starts_with(key) && t[key.len()..].trim_start().starts_with('=')
                        || t.contains(&format!("{key} ="))
                })
            })
            .map_or(1, |i| i + 1);
        format!("{name}:{line}: {msg}")
    })
}

fn check(path: &Path, sample: Option<&Path>) -> anyhow::Result<Manifest> {
    let file = manifest_path(path);
    let src = std::fs::read_to_string(&file).with_context(|| format!("can't read {}", file.display()))?;
    let manifest = parse_manifest(&file, &src).map_err(|e| anyhow::anyhow!(e))?;

    let lines: Vec<(usize, String)> = match sample {
        Some(s) => std::fs::read_to_string(s)
            .with_context(|| format!("can't read {}", s.display()))?
            .lines()
            .enumerate()
            .map(|(i, l)| (i + 1, l.to_string()))
            .collect(),
        None => {
            let dir = file.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
            run_exec(&manifest, dir)?
        }
    };
    let mut lines: Vec<(usize, String)> = lines.into_iter().filter(|(_, l)| !l.trim().is_empty()).collect();
    if lines.is_empty() {
        bail!("sample: no output to check");
    }
    if sample.is_none() {
        // Only the line the host would act on.
        let keep = if matches!(manifest.refresh, Refresh::Push(_)) { 0 } else { lines.len() - 1 };
        lines = vec![lines.swap_remove(keep)];
    }
    let mut errs = Vec::new();
    for (n, line) in &lines {
        match Update::parse_line(line) {
            Err(e) => errs.push(format!("sample:{n}: {e}")),
            Ok(u) => {
                if let Some(Err(es)) = u.popover.as_ref().map(|p| p.validate()) {
                    errs.extend(es.into_iter().map(|e| format!("sample:{n}: {e}")));
                }
            }
        }
    }
    if !errs.is_empty() {
        bail!("{}", errs.join("\n"));
    }
    Ok(manifest)
}

/// Runs the manifest's `exec`, returning numbered stdout lines.
fn run_exec(m: &Manifest, dir: &Path) -> anyhow::Result<Vec<(usize, String)>> {
    let mut child = Command::new("sh")
        .arg("-c")
        .arg(&m.exec)
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| format!("can't run `{}`", m.exec))?;
    let stdout = child.stdout.take().context("no stdout")?;
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for l in BufReader::new(stdout).lines().map_while(Result::ok) {
            if tx.send(l).is_err() {
                break;
            }
        }
    });
    let push = matches!(m.refresh, Refresh::Push(_));
    let deadline = Instant::now() + EXEC_TIMEOUT;
    let mut lines = Vec::new();
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        match rx.recv_timeout(left) {
            Ok(l) => {
                lines.push((lines.len() + 1, l));
                if push && lines.iter().any(|(_, l)| !l.trim().is_empty()) {
                    break;
                }
            }
            Err(_) => break,
        }
    }
    let _ = child.kill();
    let _ = child.wait();
    Ok(lines)
}

fn scaffold(name: &str, dir: &Path) -> anyhow::Result<PathBuf> {
    let ok = !name.is_empty() && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    anyhow::ensure!(ok, "invalid module name {name:?}: use lowercase letters, digits and hyphens");
    let path = dir.join(name);
    anyhow::ensure!(!path.exists(), "{} already exists", path.display());
    std::fs::create_dir_all(&path).with_context(|| format!("can't create {}", path.display()))?;
    let manifest = format!(
        "schema = 1\nname = \"{name}\"\ntitle = \"{name}\"\nversion = \"0.1.0\"\nicon = \"puzzle-piece-symbolic\"\n\
         refresh = {{ interval = 30 }}\nexec = \"./run.sh\"\n\n[[actions]]\nid = \"refresh\"\nlabel = \"Refresh\"\n"
    );
    std::fs::write(path.join("module.toml"), manifest)?;
    let update = json!({
        "bar": { "icon": "puzzle-piece-symbolic", "text": name },
        "popover": {
            "title": name,
            "body": [
                { "type": "section", "heading": "Status", "rows": [
                    { "type": "row", "title": "Hello", "value": "world" },
                    { "type": "row", "title": "Module", "subtitle": "scaffolded by yozora module new" }
                ]},
                { "type": "buttons", "items": [ { "label": "Refresh", "action": "refresh", "primary": true } ] }
            ]
        }
    });
    let script = format!("#!/bin/sh\ncat <<'EOF'\n{update}\nEOF\n");
    let run = path.join("run.sh");
    std::fs::write(&run, script)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&run, std::fs::Permissions::from_mode(0o755))?;
    }
    Ok(path)
}

fn search_dirs() -> Vec<PathBuf> {
    if let Ok(p) = std::env::var("YOZORA_MODULE_PATH") {
        return p.split(':').filter(|s| !s.is_empty()).map(PathBuf::from).collect();
    }
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")));
    let mut dirs = Vec::new();
    if let Some(c) = config {
        dirs.push(c.join("yozora/modules"));
    }
    dirs.push(PathBuf::from("/usr/share/yozora/modules"));
    dirs
}

fn list(out: &Out) -> anyhow::Result<()> {
    let mut items = Vec::new();
    for dir in search_dirs() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        let mut subs: Vec<PathBuf> = rd.filter_map(|e| e.ok()).map(|e| e.path()).collect();
        subs.sort();
        for sub in subs {
            let file = sub.join("module.toml");
            let Ok(src) = std::fs::read_to_string(&file) else { continue };
            let dirname = sub.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let (name, version, state) = match parse_manifest(&file, &src) {
                Ok(m) => (m.name, m.version, "ok".to_string()),
                Err(e) => (dirname, "-".to_string(), e),
            };
            items.push(json!({
                "name": name, "version": version,
                "path": sub.display().to_string(), "state": state,
            }));
        }
    }
    let lines: Vec<String> = if items.is_empty() {
        vec!["no modules installed".into()]
    } else {
        items
            .iter()
            .map(|i| {
                format!(
                    "{} {} {} {}",
                    i["name"].as_str().unwrap_or(""),
                    i["version"].as_str().unwrap_or(""),
                    i["path"].as_str().unwrap_or(""),
                    i["state"].as_str().unwrap_or("")
                )
            })
            .collect()
    };
    out.lines(&lines, json!(items));
    Ok(())
}
