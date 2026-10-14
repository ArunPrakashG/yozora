//! The Yozora skill for AI agents: written to `$XDG_DATA_HOME/yozora/skills/yozora/` and linked
//! into each harness's skills folder, so updating Yozora updates the skill.
//!
//! The `yozora-agent-skill` user service runs `agent install --auto` at login. That links only for
//! harnesses that are installed, never touches a folder it didn't make, and stops for good after
//! `agent uninstall`.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use yozora_config::paths;

use crate::{out::Out, take_flag};

pub const SKILL: &str = include_str!("../../../packaging/agent/yozora/SKILL.md");
const NAME: &str = "yozora";

/// A skills folder some harness reads.
struct Target {
    /// Who reads it.
    who: &'static str,
    /// Where skills go, under home.
    dir: &'static str,
    /// Any of these existing, under home, means a harness that reads `dir` is installed.
    markers: &'static [&'static str],
}

const TARGETS: [Target; 2] = [
    Target { who: "Claude Code", dir: ".claude/skills", markers: &[".claude"] },
    Target {
        who: "Codex, opencode and other harnesses that read ~/.agents/skills",
        dir: ".agents/skills",
        markers: &[".agents", ".codex", ".config/opencode"],
    },
];

fn source() -> PathBuf {
    paths::data_home().join("yozora/skills").join(NAME)
}

/// Left by `agent uninstall`: login stops linking.
fn off_marker() -> PathBuf {
    paths::state_dir().join("agent-skill-off")
}

/// What's at a link location.
#[derive(Debug, PartialEq)]
enum At {
    Missing,
    Ours,
    /// Something else the user put there: left alone.
    Theirs,
}

fn at(link: &Path, source: &Path) -> At {
    match std::fs::symlink_metadata(link) {
        Err(_) => At::Missing,
        Ok(m) if m.file_type().is_symlink() => {
            if std::fs::read_link(link).is_ok_and(|t| t == source) {
                At::Ours
            } else {
                At::Theirs
            }
        }
        Ok(_) => At::Theirs,
    }
}

pub fn run(mut args: Vec<String>, out: &Out) -> anyhow::Result<()> {
    let sub = if args.is_empty() { "status".to_string() } else { args.remove(0) };
    let home = paths::home_dir();
    match sub.as_str() {
        "install" => {
            let auto = take_flag(&mut args, "--auto");
            let all = take_flag(&mut args, "--all");
            install(&home, &source(), &off_marker(), auto, all, out)
        }
        "uninstall" | "remove" => uninstall(&home, &source(), &off_marker(), out),
        "status" => {
            out.value(&status(&home, &source(), &off_marker()));
            Ok(())
        }
        "skill" | "show" => {
            say_raw!("{SKILL}");
            Ok(())
        }
        other => anyhow::bail!("agent takes install, uninstall, status or skill, not {other:?}"),
    }
}

fn status(home: &Path, source: &Path, off: &Path) -> Value {
    let targets: Vec<Value> = TARGETS
        .iter()
        .map(|t| {
            let link = home.join(t.dir).join(NAME);
            json!({
                "for": t.who,
                "link": link,
                "harness_found": t.markers.iter().any(|m| home.join(m).exists()),
                "state": format!("{:?}", at(&link, source)).to_lowercase(),
            })
        })
        .collect();
    json!({
        "skill": source.join("SKILL.md"),
        "skill_current": std::fs::read_to_string(source.join("SKILL.md")).is_ok_and(|s| s == SKILL),
        "auto_install": !off.exists(),
        "targets": targets,
    })
}

fn install(home: &Path, source: &Path, off: &Path, auto: bool, all: bool, out: &Out) -> anyhow::Result<()> {
    if auto && off.exists() {
        out.done("skipped: turned off by `yozora agent uninstall`", json!({ "skipped": true }));
        return Ok(());
    }
    let _ = std::fs::remove_file(off);
    let file = source.join("SKILL.md");
    if std::fs::read_to_string(&file).ok().as_deref() != Some(SKILL) {
        yozora_config::edit::write_atomic(&file, SKILL.as_bytes())?;
    }
    let mut lines = Vec::new();
    let mut results = Vec::new();
    for t in &TARGETS {
        let dir = home.join(t.dir);
        let link = dir.join(NAME);
        let found = t.markers.iter().any(|m| home.join(m).exists());
        let state = match at(&link, source) {
            At::Ours => "linked",
            At::Theirs => "left alone: something else is there",
            At::Missing if !found && !all => "skipped: not installed",
            At::Missing => {
                std::fs::create_dir_all(&dir)?;
                std::os::unix::fs::symlink(source, &link)?;
                "linked"
            }
        };
        lines.push(format!("{}: {state} ({})", t.who, link.display()));
        results.push(json!({ "for": t.who, "link": link, "state": state }));
    }
    out.lines(&lines, json!({ "skill": file, "targets": results }));
    Ok(())
}

fn uninstall(home: &Path, source: &Path, off: &Path, out: &Out) -> anyhow::Result<()> {
    let mut removed = Vec::new();
    for t in &TARGETS {
        let link = home.join(t.dir).join(NAME);
        if at(&link, source) == At::Ours {
            std::fs::remove_file(&link)?;
            removed.push(link);
        }
    }
    let _ = std::fs::remove_dir_all(source);
    if let Some(dir) = off.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(off, "Remove this file, or run `yozora agent install`, to link the Yozora skill at login again.\n")?;
    out.done(
        format!("removed {} link(s); login won't add them again", removed.len()),
        json!({ "removed": removed, "auto_install": false }),
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_only_what_it_should_and_leaves_the_rest() {
        let home = std::env::temp_dir().join(format!("yozora-agent-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        let source = home.join(".local/share/yozora/skills/yozora");
        let off = home.join(".local/state/yozora/agent-skill-off");
        let out = Out { json: true };
        // Only Claude Code is installed, and the user has their own `yozora` under ~/.agents.
        std::fs::create_dir_all(home.join(".claude")).unwrap();
        std::fs::create_dir_all(home.join(".agents/skills/yozora")).unwrap();
        install(&home, &source, &off, true, false, &out).unwrap();
        assert_eq!(std::fs::read_to_string(home.join(".claude/skills/yozora/SKILL.md")).unwrap(), SKILL);
        assert_eq!(at(&home.join(".agents/skills/yozora"), &source), At::Theirs);
        // Twice is fine.
        install(&home, &source, &off, true, false, &out).unwrap();

        uninstall(&home, &source, &off, &out).unwrap();
        assert_eq!(at(&home.join(".claude/skills/yozora"), &source), At::Missing);
        assert!(home.join(".agents/skills/yozora").is_dir(), "theirs stays");
        // Login respects the uninstall; asking again doesn't.
        install(&home, &source, &off, true, false, &out).unwrap();
        assert_eq!(at(&home.join(".claude/skills/yozora"), &source), At::Missing);
        install(&home, &source, &off, false, false, &out).unwrap();
        assert_eq!(at(&home.join(".claude/skills/yozora"), &source), At::Ours);
        assert!(!off.exists());
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn the_skill_names_itself() {
        assert!(SKILL.starts_with("---\nname: yozora\ndescription: "));
    }
}
