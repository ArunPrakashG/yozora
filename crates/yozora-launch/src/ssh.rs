//! Hosts from `~/.ssh/config`, following `Include`. Patterns (`*`, `?`, `!`) aren't hosts.

use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq)]
pub struct Host {
    pub alias: String,
    pub hostname: String,
    pub user: String,
    pub port: String,
}

impl Host {
    pub fn key(&self) -> String {
        format!("ssh:{}", self.alias)
    }

    /// `user@hostname:port`, leaving out what isn't set.
    pub fn sub(&self) -> String {
        let mut s = String::new();
        if !self.user.is_empty() {
            s.push_str(&self.user);
            s.push('@');
        }
        s.push_str(if self.hostname.is_empty() { &self.alias } else { &self.hostname });
        if !self.port.is_empty() && self.port != "22" {
            s.push(':');
            s.push_str(&self.port);
        }
        s
    }
}

pub fn hosts() -> Vec<Host> {
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else { return Vec::new() };
    let dir = home.join(".ssh");
    let mut out = Vec::new();
    read(&dir.join("config"), &dir, &mut out, 0);
    let mut seen = std::collections::HashSet::new();
    out.retain(|h: &Host| seen.insert(h.alias.clone()));
    out
}

fn read(path: &Path, ssh_dir: &Path, out: &mut Vec<Host>, depth: u8) {
    if depth > 4 {
        return;
    }
    let Ok(text) = std::fs::read_to_string(path) else { return };
    parse(&text, out, &mut |pattern, out| {
        for p in glob(pattern, ssh_dir) {
            read(&p, ssh_dir, out, depth + 1);
        }
    });
}

/// Parses one file. `include` is called with each `Include` pattern.
fn parse(text: &str, out: &mut Vec<Host>, include: &mut dyn FnMut(&str, &mut Vec<Host>)) {
    // The hosts the current `Host` block names, as indices into `out`.
    let mut block: Vec<usize> = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (key, value) = match line.split_once(|c: char| c.is_whitespace() || c == '=') {
            Some((k, v)) => (k.to_lowercase(), v.trim_start_matches(|c: char| c.is_whitespace() || c == '=').trim()),
            None => continue,
        };
        let value = value.trim_matches('"');
        match key.as_str() {
            "host" => {
                block.clear();
                for alias in value.split_whitespace() {
                    if alias.contains(['*', '?', '!']) {
                        continue;
                    }
                    block.push(out.len());
                    out.push(Host {
                        alias: alias.to_string(),
                        hostname: String::new(),
                        user: String::new(),
                        port: String::new(),
                    });
                }
            }
            "match" => block.clear(),
            "include" => {
                for pattern in value.split_whitespace() {
                    include(pattern, out);
                }
            }
            "hostname" | "user" | "port" => {
                for &i in &block {
                    let h = &mut out[i];
                    let field = match key.as_str() {
                        "hostname" => &mut h.hostname,
                        "user" => &mut h.user,
                        _ => &mut h.port,
                    };
                    // The first value wins, as in ssh.
                    if field.is_empty() {
                        *field = value.to_string();
                    }
                }
            }
            _ => {}
        }
    }
}

/// The files an `Include` pattern names: relative to `~/.ssh`, with `*` in the last part.
fn glob(pattern: &str, ssh_dir: &Path) -> Vec<PathBuf> {
    let p = crate::scripts::expand_home(pattern);
    let p = if p.is_absolute() { p } else { ssh_dir.join(p) };
    let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    if !name.contains('*') {
        return vec![p];
    }
    let (pre, post) = name.split_once('*').unwrap_or((&name, ""));
    let Some(dir) = p.parent() else { return Vec::new() };
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut out: Vec<PathBuf> = entries
        .flatten()
        .filter(|e| {
            let n = e.file_name().to_string_lossy().into_owned();
            n.starts_with(pre) && n.ends_with(post) && n.len() >= pre.len() + post.len()
        })
        .map(|e| e.path())
        .collect();
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hosts() {
        let text = "Host *\n  ServerAliveInterval 30\n\nHost box nas\n  HostName 10.0.0.2\n  User me\n  Port 2222\n\nHost=web\n  Hostname=example.com\nInclude conf.d/*\nMatch host foo\n  User nobody\n";
        let mut out = Vec::new();
        let mut included = Vec::new();
        parse(text, &mut out, &mut |p, _| included.push(p.to_string()));
        assert_eq!(out.iter().map(|h| h.alias.as_str()).collect::<Vec<_>>(), ["box", "nas", "web"]);
        assert_eq!(out[0].sub(), "me@10.0.0.2:2222");
        assert_eq!(out[1].hostname, "10.0.0.2");
        assert_eq!(out[2].sub(), "example.com");
        assert_eq!(included, ["conf.d/*"]);
    }
}
