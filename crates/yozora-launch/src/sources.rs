//! Everything other than apps that the launcher lists: open windows, built-in commands, the
//! clipboard history, emoji and files.

use std::{
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use yozora_config::launch::LaunchConfig;
use yozora_ui::{Icon, hypr};

/// An open Hyprland window.
#[derive(Clone, Debug, PartialEq)]
pub struct Win {
    pub address: String,
    pub class: String,
    pub title: String,
    pub workspace: String,
    /// 0 is the focused window, 1 the one before it, and so on.
    pub recency: i64,
}

/// Hyprland's windows, most recently focused first, without the focused one: switching to the
/// window you're in is never what you want.
pub fn windows() -> Vec<Win> {
    let Ok(reply) = hypr::request("j/clients") else { return Vec::new() };
    let mut out = parse_clients(&reply);
    out.retain(|w| w.recency != 0);
    out
}

fn parse_clients(json: &str) -> Vec<Win> {
    let Ok(serde_json::Value::Array(all)) = serde_json::from_str::<serde_json::Value>(json) else {
        return Vec::new();
    };
    let mut out: Vec<Win> = all
        .iter()
        .filter(|c| c["mapped"].as_bool().unwrap_or(true) && !c["hidden"].as_bool().unwrap_or(false))
        .filter_map(|c| {
            Some(Win {
                address: c["address"].as_str()?.to_string(),
                class: c["class"].as_str().unwrap_or_default().to_string(),
                title: c["title"].as_str().unwrap_or_default().to_string(),
                workspace: {
                    let name = c["workspace"]["name"].as_str().unwrap_or_default();
                    name.strip_prefix("special:").unwrap_or(name).to_string()
                },
                recency: c["focusHistoryID"].as_i64().unwrap_or(i64::MAX),
            })
        })
        .collect();
    out.sort_by_key(|w| w.recency);
    out
}

/// Focuses a window, switching to its workspace.
/// Focuses a window and waits until Hyprland says it's active. Called as the launcher lets
/// go of the keyboard, and Hyprland won't move focus to a window until it has.
pub fn focus(address: &str) -> anyhow::Result<()> {
    for _ in 0..15 {
        dispatch(
            &format!("hl.dsp.focus({{ window = \"address:{address}\" }})"),
            &format!("focuswindow address:{address}"),
        )?;
        if active_window()?.as_deref() == Some(address) {
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    anyhow::bail!("{address} didn't take focus")
}

fn active_window() -> anyhow::Result<Option<String>> {
    let active: serde_json::Value = serde_json::from_str(&hypr::request("j/activewindow")?)?;
    Ok(active["address"].as_str().map(str::to_string))
}

/// Closes a window. Hyprland's close dispatcher acts on the focused window (and ignores
/// arguments it doesn't know), so this focuses the window, checks that it really is the focused
/// one now, and only then closes.
pub fn close_window(address: &str) -> anyhow::Result<()> {
    focus(address).map_err(|e| e.context("not closing anything"))?;
    // Checked again right before closing, since the close acts on whatever has focus.
    if active_window()?.as_deref() != Some(address) {
        anyhow::bail!("focus moved off {address}; not closing anything");
    }
    dispatch("hl.dsp.window.close()", "killactive")
}

/// Runs a dispatcher, in Lua on Hyprland 0.55 and later and the classic syntax before that.
fn dispatch(lua: &str, classic: &str) -> anyhow::Result<()> {
    let reply = hypr::request(&format!("dispatch {lua}"))?;
    if reply.trim() == "ok" {
        return Ok(());
    }
    let reply = hypr::request(&format!("dispatch {classic}"))?;
    if reply.trim() == "ok" { Ok(()) } else { anyhow::bail!("Hyprland: {}", reply.trim()) }
}

/// A built-in command.
#[derive(Clone, Debug, PartialEq)]
pub struct Cmd {
    pub id: &'static str,
    pub name: &'static str,
    pub sub: String,
    pub icon: Icon,
    pub keywords: &'static str,
    /// Run through `sh -c`.
    pub run: String,
}

pub fn commands(config: &LaunchConfig, dnd: Option<bool>) -> Vec<Cmd> {
    let c = &config.commands;
    let dnd_sub = match dnd {
        Some(true) => "Notifications are silenced".to_string(),
        Some(false) => "Notifications are on".to_string(),
        None => "Yozora Notify".to_string(),
    };
    vec![
        Cmd {
            id: "lock",
            name: "Lock Screen",
            sub: "Yozora Lock".into(),
            icon: Icon::Lock,
            keywords: "lock screen away",
            run: c.lock.clone(),
        },
        Cmd {
            id: "dnd",
            name: "Do Not Disturb",
            sub: dnd_sub,
            icon: Icon::Moon,
            keywords: "dnd notifications focus quiet silence",
            run: "yozora-notify dnd toggle".into(),
        },
        Cmd {
            id: "settings",
            name: "Yozora Settings",
            sub: "Bar, notifications, lock, theme".into(),
            icon: Icon::Settings,
            keywords: "settings preferences config yozora",
            run: "yozora-settings".into(),
        },
        Cmd {
            id: "suspend",
            name: "Suspend",
            sub: "System".into(),
            icon: Icon::Power,
            keywords: "sleep suspend",
            run: c.suspend.clone(),
        },
        Cmd {
            id: "reboot",
            name: "Restart",
            sub: "System".into(),
            icon: Icon::Reset,
            keywords: "reboot restart",
            run: c.reboot.clone(),
        },
        Cmd {
            id: "poweroff",
            name: "Shut Down",
            sub: "System".into(),
            icon: Icon::Power,
            keywords: "shutdown power off poweroff halt",
            run: c.poweroff.clone(),
        },
        Cmd {
            id: "logout",
            name: "Log Out",
            sub: "End the Hyprland session".into(),
            icon: Icon::Logout,
            keywords: "logout exit sign out quit session",
            run: c.logout.clone(),
        },
    ]
}

/// Whether Do Not Disturb is on, from yozora-notify, if it runs.
pub fn dnd() -> Option<bool> {
    let path = yozora_config::paths::socket_path("notify");
    let r = yozora_ipc::request_timeout(
        &path,
        &yozora_ipc::Request::new("status", [] as [&str; 0]),
        std::time::Duration::from_millis(200),
    )
    .ok()?;
    r.data["dnd"].as_bool()
}

/// A clipboard history entry from cliphist.
#[derive(Clone, Debug, PartialEq)]
pub struct Clip {
    /// The whole `cliphist list` line, which is what `decode` and `delete` take.
    pub line: String,
    pub preview: String,
    pub image: bool,
}

pub fn clipboard() -> Vec<Clip> {
    let Ok(out) = Command::new("cliphist").arg("list").stderr(Stdio::null()).output() else { return Vec::new() };
    parse_cliphist(&String::from_utf8_lossy(&out.stdout))
}

fn parse_cliphist(text: &str) -> Vec<Clip> {
    text.lines()
        .filter_map(|line| {
            let (_, preview) = line.split_once('\t')?;
            let image = preview.starts_with("[[ binary data");
            let preview = if image {
                // `[[ binary data 52 KiB png 1920x1080 ]]`
                let inner = preview.trim_start_matches("[[ binary data ").trim_end_matches(" ]]");
                format!("Image · {inner}")
            } else {
                preview.split_whitespace().collect::<Vec<_>>().join(" ")
            };
            Some(Clip { line: line.to_string(), preview, image })
        })
        .take(500)
        .collect()
}

/// Puts a history entry back on the clipboard.
pub fn clip_copy(line: &str) -> anyhow::Result<()> {
    let mut decode = Command::new("cliphist").arg("decode").stdin(Stdio::piped()).stdout(Stdio::piped()).spawn()?;
    {
        use std::io::Write;
        let mut stdin = decode.stdin.take().ok_or_else(|| anyhow::anyhow!("no stdin"))?;
        stdin.write_all(line.as_bytes())?;
    }
    let stdout = decode.stdout.take().ok_or_else(|| anyhow::anyhow!("no stdout"))?;
    let mut copy = yozora_ui::gpu::child(&mut Command::new("wl-copy")).stdin(stdout).spawn()?;
    decode.wait()?;
    // wl-copy forks to serve the clipboard and its parent exits at once.
    copy.wait()?;
    Ok(())
}

pub fn clip_delete(line: &str) -> anyhow::Result<()> {
    use std::io::Write;
    let mut del = Command::new("cliphist").arg("delete").stdin(Stdio::piped()).spawn()?;
    del.stdin.take().ok_or_else(|| anyhow::anyhow!("no stdin"))?.write_all(line.as_bytes())?;
    del.wait()?;
    Ok(())
}

/// Copies `text` to the clipboard.
pub fn copy_text(text: &str) -> anyhow::Result<()> {
    use std::io::Write;
    let mut copy = yozora_ui::gpu::child(&mut Command::new("wl-copy")).stdin(Stdio::piped()).spawn()?;
    copy.stdin.take().ok_or_else(|| anyhow::anyhow!("no stdin"))?.write_all(text.as_bytes())?;
    copy.wait()?;
    Ok(())
}

/// An emoji with what it's searched by.
#[derive(Clone, Debug)]
pub struct Emoji {
    pub glyph: &'static str,
    pub name: &'static str,
    /// Name and shortcodes, joined, for matching.
    pub search: String,
}

pub fn emoji() -> Vec<Emoji> {
    emojis::iter()
        .filter(|e| e.skin_tone().is_none_or(|t| t == emojis::SkinTone::Default))
        .map(|e| {
            let mut search = e.name().to_string();
            for s in e.shortcodes() {
                search.push(' ');
                search.push_str(&s.replace('_', " "));
            }
            Emoji { glyph: e.as_str(), name: e.name(), search }
        })
        .collect()
}

/// A file from the recently used list or from locate.
#[derive(Clone, Debug, PartialEq)]
pub struct FileHit {
    pub path: PathBuf,
    pub dir: bool,
}

/// `~/.local/share/recently-used.xbel`, newest first, keeping only files that still exist.
pub fn recent_files() -> Vec<FileHit> {
    let data = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")));
    let Some(path) = data.map(|d| d.join("recently-used.xbel")) else { return Vec::new() };
    let Ok(text) = std::fs::read_to_string(path) else { return Vec::new() };
    parse_xbel(&text)
        .into_iter()
        .filter_map(|p| {
            let meta = std::fs::metadata(&p).ok()?;
            Some(FileHit { dir: meta.is_dir(), path: p })
        })
        .take(200)
        .collect()
}

fn parse_xbel(text: &str) -> Vec<PathBuf> {
    let mut items: Vec<(String, PathBuf)> = Vec::new();
    for chunk in text.split("<bookmark ").skip(1) {
        let attr = |name: &str| -> Option<&str> {
            let start = chunk.find(&format!("{name}=\""))? + name.len() + 2;
            let end = chunk[start..].find('"')? + start;
            Some(&chunk[start..end])
        };
        let Some(href) = attr("href").and_then(|h| h.strip_prefix("file://")) else { continue };
        let when = attr("modified").or_else(|| attr("visited")).unwrap_or_default().to_string();
        items.push((when, PathBuf::from(percent_decode(&xml_unescape(href)))));
    }
    // ISO 8601 times sort as strings.
    items.sort_by(|a, b| b.0.cmp(&a.0));
    let mut seen = std::collections::HashSet::new();
    items.into_iter().map(|(_, p)| p).filter(|p| seen.insert(p.clone())).collect()
}

fn xml_unescape(s: &str) -> String {
    s.replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'")
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Ok(b) = u8::from_str_radix(std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("zz"), 16)
        {
            out.push(b);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Files whose name contains every word of `query`, from plocate's database, in the home
/// directory and not hidden. Runs a process, so call it off the UI thread.
pub fn locate(query: &str, home: &Path, limit: usize) -> Vec<FileHit> {
    let words: Vec<&str> = query.split_whitespace().collect();
    let Some(first) = words.iter().max_by_key(|w| w.len()) else { return Vec::new() };
    let out = Command::new("plocate").args(["-i", "-b", "-l", "400", "--"]).arg(first).stderr(Stdio::null()).output();
    let Ok(out) = out else { return Vec::new() };
    let text = String::from_utf8_lossy(&out.stdout);
    let lower: Vec<String> = words.iter().map(|w| w.to_lowercase()).collect();
    text.lines()
        .map(PathBuf::from)
        .filter(|p| p.starts_with(home))
        .filter(|p| {
            // Skip anything under a dot directory, and build trees nobody opens by name.
            !p.strip_prefix(home).unwrap_or(p).components().any(|c| {
                let c = c.as_os_str().to_string_lossy();
                c.starts_with('.') || c == "node_modules" || c == "target" || c == "__pycache__"
            })
        })
        .filter(|p| {
            let name = p.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
            lower.iter().all(|w| name.contains(w.as_str()))
        })
        .take(limit)
        .filter_map(|p| {
            let meta = std::fs::metadata(&p).ok()?;
            Some(FileHit { dir: meta.is_dir(), path: p })
        })
        .collect()
}

/// `~/Projects/x` for a path under the home directory.
pub fn tilde(path: &Path, home: &Path) -> String {
    match path.strip_prefix(home) {
        Ok(rest) if rest.as_os_str().is_empty() => "~".into(),
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => path.display().to_string(),
    }
}

/// A glyph for a file, by its extension.
pub fn file_icon(hit: &FileHit) -> Icon {
    if hit.dir {
        return Icon::Folder;
    }
    let ext = hit.path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    match ext.as_str() {
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "avif" | "bmp" | "heic" => Icon::Image,
        "mp3" | "flac" | "ogg" | "opus" | "wav" | "m4a" => Icon::Music,
        "rs" | "c" | "h" | "cpp" | "py" | "js" | "ts" | "tsx" | "go" | "java" | "lua" | "sh" | "toml" | "json"
        | "yaml" | "yml" | "html" | "css" | "conf" => Icon::Code,
        "zip" | "tar" | "gz" | "xz" | "zst" | "7z" | "rar" => Icon::Package,
        _ => Icon::File,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clients() {
        let json = r#"[
            {"address":"0x1","class":"firefox","title":"Docs","workspace":{"id":2,"name":"2"},"focusHistoryID":1,"mapped":true,"hidden":false},
            {"address":"0x2","class":"foot","title":"~","workspace":{"id":-98,"name":"special:scratch"},"focusHistoryID":2,"mapped":true,"hidden":false},
            {"address":"0x3","class":"x","title":"gone","workspace":{"id":1,"name":"1"},"focusHistoryID":3,"mapped":false,"hidden":false},
            {"address":"0x4","class":"code","title":"view.rs","workspace":{"id":1,"name":"1"},"focusHistoryID":0,"mapped":true,"hidden":false}
        ]"#;
        let w = parse_clients(json);
        let addrs: Vec<_> = w.iter().map(|w| w.address.as_str()).collect();
        assert_eq!(addrs, vec!["0x4", "0x1", "0x2"]);
        assert_eq!(w[2].workspace, "scratch");
    }

    #[test]
    fn cliphist_lines() {
        let c = parse_cliphist("12\tcargo   build\n11\t[[ binary data 52 KiB png 1920x1080 ]]\nbad line\n");
        assert_eq!(c.len(), 2);
        assert_eq!(c[0].preview, "cargo build");
        assert_eq!(c[0].line, "12\tcargo   build");
        assert!(c[1].image);
        assert_eq!(c[1].preview, "Image · 52 KiB png 1920x1080");
    }

    #[test]
    fn xbel() {
        let text = r#"<xbel>
<bookmark href="file:///home/u/old.txt" added="2026-01-01T00:00:00Z" modified="2026-01-01T00:00:00Z" visited="x"/>
<bookmark href="file:///home/u/My%20Notes%20&amp;%20Ideas.md" added="a" modified="2026-03-01T00:00:00Z" visited="x"/>
<bookmark href="https://example.com" modified="2026-04-01T00:00:00Z"/>
</xbel>"#;
        assert_eq!(
            parse_xbel(text),
            vec![PathBuf::from("/home/u/My Notes & Ideas.md"), PathBuf::from("/home/u/old.txt")]
        );
    }

    #[test]
    fn emoji_search_text() {
        let all = emoji();
        assert!(all.len() > 1000);
        let fire = all.iter().find(|e| e.glyph == "🔥").unwrap();
        assert!(fire.search.contains("fire"));
    }
}
