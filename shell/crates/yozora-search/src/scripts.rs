//! Script commands in Raycast's format, which vicinae also reads: a script with `@raycast.`
//! (or `@vicinae.`) lines in a comment near the top.
//!
//! ```sh
//! #!/bin/bash
//! # @raycast.schemaVersion 1
//! # @raycast.title Update System
//! # @raycast.mode silent
//! # @raycast.argument1 { "type": "text", "placeholder": "Package", "optional": true }
//! ```
//!
//! `silent` and `compact` run without a window and show the last line of output as a
//! notification; `fullOutput` runs in the terminal.

use std::path::{Path, PathBuf};

use crate::spawn;

/// How much of a script is read for its header.
const HEAD: usize = 8 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Output {
    /// No window; the last line of output as a notification.
    Silent,
    /// The output in a terminal.
    Terminal,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Arg {
    pub placeholder: String,
    pub optional: bool,
    pub percent_encoded: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Script {
    pub path: PathBuf,
    pub title: String,
    pub description: String,
    pub package: String,
    /// An emoji or a short symbol, or an image next to the script.
    pub icon: Option<Icon>,
    pub output: Output,
    pub args: Vec<Arg>,
    pub dir: Option<PathBuf>,
    pub confirm: bool,
    /// The interpreter from `@vicinae.exec`, if given.
    pub exec: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Icon {
    Text(String),
    Image(PathBuf),
}

impl Script {
    pub fn key(&self) -> String {
        format!("script:{}", self.path.display())
    }

    /// What's shown under the title.
    pub fn sub(&self) -> String {
        match (self.description.is_empty(), self.package.is_empty()) {
            (false, false) => format!("{} · {}", self.package, self.description),
            (false, true) => self.description.clone(),
            (true, false) => self.package.clone(),
            (true, true) => "Script command".into(),
        }
    }
}

/// Reads one script's header, or `None` if it isn't a script command.
pub fn parse(path: &Path, head: &str) -> Option<Script> {
    let mut s = Script {
        path: path.to_path_buf(),
        title: String::new(),
        description: String::new(),
        package: String::new(),
        icon: None,
        output: Output::Silent,
        args: Vec::new(),
        dir: None,
        confirm: false,
        exec: Vec::new(),
    };
    let mut args: Vec<(u8, Arg)> = Vec::new();
    for line in head.lines().take(80) {
        let line = line.trim_start().trim_start_matches(['#', '/', '-', ';', '\'', '*', ' ']).trim_start();
        let Some(rest) = line.strip_prefix("@raycast.").or_else(|| line.strip_prefix("@vicinae.")) else { continue };
        let (key, value) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
        let value = value.trim();
        match key {
            "title" => s.title = value.to_string(),
            "description" => s.description = value.to_string(),
            "packageName" => s.package = value.to_string(),
            "mode" => s.output = if value == "fullOutput" { Output::Terminal } else { Output::Silent },
            "needsConfirmation" => s.confirm = value == "true",
            "currentDirectoryPath" => s.dir = Some(expand_home(value)),
            "icon" if !value.is_empty() => s.icon = Some(icon(path, value)),
            "exec" => s.exec = serde_json::from_str(value).unwrap_or_default(),
            k if k.starts_with("argument") => {
                let Ok(n) = k["argument".len()..].parse::<u8>() else { continue };
                let v: serde_json::Value = serde_json::from_str(value).unwrap_or_default();
                let placeholder = v["placeholder"].as_str().unwrap_or("Argument").to_string();
                args.push((
                    n,
                    Arg {
                        placeholder,
                        optional: v["optional"].as_bool().unwrap_or(false),
                        percent_encoded: v["percentEncoded"].as_bool().unwrap_or(false),
                    },
                ));
            }
            _ => {}
        }
    }
    if s.title.is_empty() {
        return None;
    }
    args.sort_by_key(|(n, _)| *n);
    s.args = args.into_iter().map(|(_, a)| a).take(3).collect();
    Some(s)
}

fn icon(script: &Path, value: &str) -> Icon {
    let lower = value.to_lowercase();
    if [".png", ".svg", ".jpg", ".jpeg"].iter().any(|e| lower.ends_with(e)) && !lower.starts_with("http") {
        let p = expand_home(value);
        let p = if p.is_absolute() { p } else { script.parent().unwrap_or(Path::new("/")).join(p) };
        if p.is_file() {
            return Icon::Image(p);
        }
    }
    Icon::Text(value.chars().take(2).collect())
}

pub fn expand_home(p: &str) -> PathBuf {
    match p.strip_prefix('~') {
        Some(rest) if rest.is_empty() || rest.starts_with('/') => {
            let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| "/".into());
            home.join(rest.trim_start_matches('/'))
        }
        _ => PathBuf::from(p),
    }
}

/// Every script command in `dirs`, by title. A script that's in two folders (vicinae's folder
/// is often a link) shows once.
pub fn scan(dirs: &[String]) -> Vec<Script> {
    use std::io::Read;
    let mut out: Vec<Script> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(expand_home(dir)) else { continue };
        for e in entries.flatten() {
            let path = e.path();
            if e.file_name().to_string_lossy().starts_with('.') || !path.is_file() {
                continue;
            }
            let real = std::fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
            if !seen.insert(real) {
                continue;
            }
            let Ok(mut f) = std::fs::File::open(&path) else { continue };
            let mut buf = vec![0; HEAD];
            let n = f.read(&mut buf).unwrap_or(0);
            buf.truncate(n);
            if let Some(s) = parse(&path, &String::from_utf8_lossy(&buf)) {
                out.push(s);
            }
        }
    }
    out.sort_by_cached_key(|s| s.title.to_lowercase());
    out
}

/// Splits what was typed into the script's arguments: words, with quotes keeping spaces, and
/// the last argument taking the rest.
pub fn split_args(text: &str, n: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text.trim();
    while out.len() + 1 < n && !rest.is_empty() {
        let (word, after) = if let Some(r) = rest.strip_prefix('"') {
            match r.find('"') {
                Some(end) => (&r[..end], &r[end + 1..]),
                None => (r, ""),
            }
        } else {
            rest.split_once(char::is_whitespace).unwrap_or((rest, ""))
        };
        out.push(word.to_string());
        rest = after.trim_start();
    }
    if !rest.is_empty() || out.len() < n {
        let last = rest.strip_prefix('"').and_then(|r| r.strip_suffix('"')).unwrap_or(rest);
        out.push(last.to_string());
    }
    out.truncate(n);
    while out.len() < n {
        out.push(String::new());
    }
    out
}

fn encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// The command line for a script and its arguments.
fn argv(s: &Script, args: &[String]) -> Vec<String> {
    let mut out = s.exec.clone();
    if out.is_empty() && !is_executable(&s.path) {
        out.push("sh".into());
    }
    out.push(s.path.to_string_lossy().into_owned());
    for (a, value) in s.args.iter().zip(args) {
        out.push(if a.percent_encoded { encode(value) } else { value.clone() });
    }
    out
}

fn is_executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(p).is_ok_and(|m| m.permissions().mode() & 0o111 != 0)
}

/// Shows the last line of output as a notification, or the exit code if it failed silently.
const NOTIFY: &str = r#"title=$1; shift
out=$("$@" 2>&1); rc=$?
last=$(printf '%s\n' "$out" | sed '/^[[:space:]]*$/d' | tail -n 1)
if [ -n "$last" ]; then notify-send -a "Yozora Launch" "$title" "$last"
elif [ "$rc" -ne 0 ]; then notify-send -a "Yozora Launch" -u critical "$title" "Failed with exit code $rc"
fi
exit $rc"#;

const HOLD: &str = r#""$@"; rc=$?; printf '\n[exited with %d] Press Enter to close ' "$rc"; read -r _"#;

pub fn run(s: &Script, args: &[String], terminal: &str) -> anyhow::Result<()> {
    let argv = argv(s, args);
    let dir = s.dir.clone().or_else(|| s.path.parent().map(Path::to_path_buf));
    let id = format!("script-{}", s.path.file_stem().map(|n| n.to_string_lossy()).unwrap_or_default());
    let full: Vec<String> = match s.output {
        Output::Silent => {
            let mut v = vec!["sh".into(), "-c".into(), NOTIFY.into(), "sh".into(), s.title.clone()];
            v.extend(argv);
            v
        }
        Output::Terminal => {
            let mut v = vec!["sh".into(), "-c".into(), HOLD.into(), "sh".into()];
            v.extend(argv);
            spawn::in_terminal(terminal, v)
        }
    };
    spawn::run(&id, &full, dir.as_deref())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_headers() {
        let head = "#!/bin/bash\n\n# @raycast.schemaVersion 1\n# @raycast.title Update System\n# @raycast.mode fullOutput\n# @raycast.packageName System\n# @raycast.icon 🔄\n# @raycast.argument1 { \"type\": \"text\", \"placeholder\": \"Package\", \"optional\": true }\n# @raycast.argument2 { \"type\": \"text\", \"placeholder\": \"Query\", \"percentEncoded\": true }\necho hi\n";
        let s = parse(Path::new("/s/update.sh"), head).unwrap();
        assert_eq!(s.title, "Update System");
        assert_eq!(s.output, Output::Terminal);
        assert_eq!(s.icon, Some(Icon::Text("🔄".into())));
        assert_eq!(s.args.len(), 2);
        assert!(s.args[0].optional && !s.args[1].optional);
        assert_eq!(s.sub(), "System");

        let v = "// @vicinae.title Hello\n// @vicinae.exec [\"/usr/bin/node\"]\n";
        let s = parse(Path::new("/s/hello.js"), v).unwrap();
        assert_eq!(s.exec, vec!["/usr/bin/node"]);
        assert_eq!(s.output, Output::Silent);
        assert_eq!(argv(&s, &[]), vec!["/usr/bin/node", "/s/hello.js"]);

        assert!(parse(Path::new("/s/x"), "#!/bin/sh\necho no header\n").is_none());
    }

    #[test]
    fn splits_arguments() {
        assert_eq!(split_args("vim", 1), vec!["vim"]);
        assert_eq!(split_args("hello big world", 2), vec!["hello", "big world"]);
        assert_eq!(split_args("\"two words\" rest", 2), vec!["two words", "rest"]);
        assert_eq!(split_args("", 2), vec!["", ""]);
        assert_eq!(split_args("one", 3), vec!["one", "", ""]);
        assert_eq!(encode("a b&c"), "a%20b%26c");
    }
}
