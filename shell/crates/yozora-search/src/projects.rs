//! Projects: git repositories under the configured folders, and the folders Zed and VS Code
//! opened recently, which come first.

use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    time::Instant,
};

use crate::{scripts::expand_home, spawn};

/// How deep under each root to look for repositories.
const DEPTH: usize = 4;
/// A cap on folders read per scan, so a huge home folder can't stall it.
const MAX_DIRS: usize = 40_000;

/// Folders that never hold projects worth listing, or are too big to walk.
const SKIP: &[&str] =
    &["node_modules", "target", "vendor", "venv", "__pycache__", "build", "dist", "snap", "go", "Library", "Steam"];

#[derive(Clone, Debug, PartialEq)]
pub struct Project {
    pub path: PathBuf,
    pub name: String,
    /// Where it was opened recently, such as "Zed".
    pub recent: Option<&'static str>,
}

impl Project {
    pub fn key(&self) -> String {
        format!("proj:{}", self.path.display())
    }
}

/// Recent projects first, then repositories under `roots`, without repeats.
pub fn scan(roots: &[String]) -> Vec<Project> {
    let started = Instant::now();
    let mut out = recents();
    let mut seen: HashSet<PathBuf> = out.iter().map(|p| p.path.clone()).collect();
    let mut budget = MAX_DIRS;
    for root in roots {
        let mut found = Vec::new();
        walk(&expand_home(root), 0, &mut budget, &mut found);
        for path in found {
            if seen.insert(path.clone()) {
                out.push(project(path, None));
            }
        }
    }
    tracing::debug!("found {} projects in {:?}", out.len(), started.elapsed());
    out
}

fn project(path: PathBuf, recent: Option<&'static str>) -> Project {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.display().to_string());
    Project { path, name, recent }
}

fn walk(dir: &Path, depth: usize, budget: &mut usize, out: &mut Vec<PathBuf>) {
    if *budget == 0 {
        return;
    }
    *budget -= 1;
    if dir.join(".git").exists() {
        out.push(dir.to_path_buf());
        return;
    }
    if depth >= DEPTH {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut dirs: Vec<PathBuf> = entries
        .flatten()
        .filter(|e| {
            let name = e.file_name();
            let name = name.to_string_lossy();
            // Symlinks are skipped too, so a loop can't run away.
            !name.starts_with('.') && !SKIP.contains(&name.as_ref()) && e.file_type().is_ok_and(|t| t.is_dir())
        })
        .map(|e| e.path())
        .collect();
    dirs.sort();
    for d in dirs {
        walk(&d, depth + 1, budget, out);
    }
}

/// Folders Zed and VS Code opened, most recent first.
fn recents() -> Vec<Project> {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    let mut out: Vec<(String, PathBuf, &'static str)> = Vec::new();
    for channel in ["0-stable", "0-preview"] {
        let db = home.join(".local/share/zed/db").join(channel).join("db.sqlite");
        let rows = query(
            &db,
            "SELECT paths, timestamp FROM workspaces WHERE remote_connection_id IS NULL AND paths != '' ORDER BY timestamp DESC LIMIT 50",
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
        );
        for (paths, when) in rows {
            for p in paths.lines().filter(|p| !p.is_empty()) {
                out.push((when.clone(), PathBuf::from(p), "Zed"));
            }
        }
    }
    for (app, name) in [("Code", "VS Code"), ("VSCodium", "VSCodium"), ("Cursor", "Cursor")] {
        let db = home.join(".config").join(app).join("User/globalStorage/state.vscdb");
        let rows = query(&db, "SELECT value FROM ItemTable WHERE key = 'history.recentlyOpenedPathsList'", |r| {
            r.get::<_, String>(0)
        });
        for json in rows {
            let v: serde_json::Value = serde_json::from_str(&json).unwrap_or_default();
            for e in v["entries"].as_array().into_iter().flatten().take(50) {
                if let Some(path) = e["folderUri"].as_str().and_then(|u| u.strip_prefix("file://")) {
                    // No times here; these follow Zed's.
                    out.push((String::new(), PathBuf::from(crate::sources::percent_decode(path)), name));
                }
            }
        }
    }
    // Stable: the empty times keep VS Code's own order after Zed's.
    out.sort_by(|a, b| b.0.cmp(&a.0));
    let mut seen = HashSet::new();
    out.into_iter()
        .filter(|(_, p, _)| p.is_dir() && seen.insert(p.clone()))
        .map(|(_, p, from)| project(p, Some(from)))
        .collect()
}

/// Runs `sql` on a database another app may hold open, without taking its lock or writing.
pub fn query<T>(db: &Path, sql: &str, row: impl FnMut(&rusqlite::Row) -> rusqlite::Result<T>) -> Vec<T> {
    if !db.is_file() {
        return Vec::new();
    }
    let run = || -> rusqlite::Result<Vec<T>> {
        let path = db.display().to_string().replace('%', "%25").replace('?', "%3f").replace('#', "%23");
        let uri = format!("file:{path}?immutable=1");
        let conn = rusqlite::Connection::open_with_flags(
            uri,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_URI,
        )?;
        let mut stmt = conn.prepare(sql)?;
        stmt.query_map([], row)?.collect()
    };
    run().unwrap_or_else(|e| {
        tracing::debug!("{}: {e}", db.display());
        Vec::new()
    })
}

const GUI_EDITORS: &[&str] = &["zeditor", "zed", "code", "codium", "cursor", "subl", "kate", "gnome-text-editor"];
const TERMINAL_EDITORS: &[&str] = &["nvim", "vim", "vi", "hx", "helix", "nano", "micro", "kak", "emacs"];

/// Opens a project in the editor: the configured one, the first GUI editor installed, or
/// `$VISUAL` or `$EDITOR` in the terminal.
pub fn open(path: &Path, editor: &str, terminal: &str) -> anyhow::Result<()> {
    let chosen = if editor.trim().is_empty() {
        GUI_EDITORS
            .iter()
            .find(|e| spawn::in_path(e))
            .map(|e| e.to_string())
            .or_else(|| std::env::var("VISUAL").ok().filter(|v| !v.is_empty()))
            .or_else(|| std::env::var("EDITOR").ok().filter(|v| !v.is_empty()))
            .unwrap_or_else(|| "xdg-open".into())
    } else {
        editor.trim().to_string()
    };
    let mut args = crate::desktop::split_exec(&chosen);
    let program = args.first().and_then(|p| Path::new(p).file_name()).map(|n| n.to_string_lossy().into_owned());
    let in_terminal = program.as_deref().is_some_and(|p| TERMINAL_EDITORS.contains(&p));
    args.push(if in_terminal { ".".into() } else { path.to_string_lossy().into_owned() });
    if in_terminal {
        args = spawn::in_terminal(terminal, args);
    }
    spawn::run(program.as_deref().unwrap_or("editor"), &args, Some(path))
}

/// Opens the terminal in `path`.
pub fn terminal_in(path: &Path, terminal: &str) -> anyhow::Result<()> {
    let term = spawn::terminal(terminal).ok_or_else(|| anyhow::anyhow!("no terminal found"))?;
    spawn::run("terminal", &crate::desktop::split_exec(&term), Some(path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_repositories() {
        let root = std::env::temp_dir().join(format!("yozora-launch-proj-{}", std::process::id()));
        for d in ["a/.git", "b/c/.git", "b/c/nested/.git", "node_modules/x/.git", ".hidden/y/.git", "d/e/f/g/h/.git"] {
            std::fs::create_dir_all(root.join(d)).unwrap();
        }
        let mut out = Vec::new();
        walk(&root, 0, &mut 1000, &mut out);
        let names: Vec<String> = out.iter().map(|p| p.strip_prefix(&root).unwrap().display().to_string()).collect();
        // Not inside a repository, not in skipped folders, and four levels at most.
        assert_eq!(names, ["a", "b/c"]);
        let _ = std::fs::remove_dir_all(&root);
    }
}
