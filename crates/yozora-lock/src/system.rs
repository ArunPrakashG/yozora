//! What the lock screen shows about the machine: who is logged in, battery, network, the
//! keyboard layout, and where each monitor's wallpaper comes from. All cheap reads of files and
//! sockets; nothing here needs a daemon.

use std::{
    ffi::CStr,
    path::{Path, PathBuf},
};

pub struct User {
    /// The login name, which PAM checks.
    pub login: String,
    /// The full name from the password database, or the login name.
    pub name: String,
    /// `~/.face` or AccountsService's picture, if there is one.
    pub picture: Option<PathBuf>,
}

impl User {
    pub fn current() -> anyhow::Result<Self> {
        let uid = unsafe { libc::getuid() };
        let mut pwd: libc::passwd = unsafe { std::mem::zeroed() };
        let mut buf = vec![0 as libc::c_char; 16 * 1024];
        let mut result = std::ptr::null_mut();
        let rc = unsafe { libc::getpwuid_r(uid, &mut pwd, buf.as_mut_ptr(), buf.len(), &mut result) };
        if rc != 0 || result.is_null() {
            anyhow::bail!("uid {uid} isn't in the password database");
        }
        let login = unsafe { CStr::from_ptr(pwd.pw_name) }.to_string_lossy().into_owned();
        let gecos = if pwd.pw_gecos.is_null() {
            String::new()
        } else {
            unsafe { CStr::from_ptr(pwd.pw_gecos) }.to_string_lossy().into_owned()
        };
        let home = unsafe { CStr::from_ptr(pwd.pw_dir) }.to_string_lossy().into_owned();
        // GECOS is "Full Name,room,phone,…".
        let full = gecos.split(',').next().unwrap_or_default().trim();
        let name = if full.is_empty() { login.clone() } else { full.to_string() };
        let picture = [
            Path::new(&home).join(".face"),
            Path::new(&home).join(".face.icon"),
            Path::new("/var/lib/AccountsService/icons").join(&login),
        ]
        .into_iter()
        .find(|p| p.is_file());
        Ok(Self { login, name, picture })
    }

    /// The first letter of the name, for the avatar when there's no picture.
    pub fn initial(&self) -> String {
        self.name.chars().next().map(|c| c.to_uppercase().collect()).unwrap_or_else(|| "?".into())
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Status {
    /// Percent and whether it's charging, if there's a battery.
    pub battery: Option<(u8, bool)>,
    pub network: Network,
    /// A short name for the keyboard layout, like `EN`.
    pub layout: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Network {
    #[default]
    Offline,
    Wired,
    Wireless,
}

impl Status {
    pub fn read() -> Self {
        Self { battery: battery(), network: network(), layout: layout() }
    }
}

fn read(path: &Path) -> Option<String> {
    std::fs::read_to_string(path).ok().map(|s| s.trim().to_string())
}

fn battery() -> Option<(u8, bool)> {
    let entries = std::fs::read_dir("/sys/class/power_supply").ok()?;
    let mut total = 0u32;
    let mut count = 0u32;
    let mut charging = false;
    for e in entries.flatten() {
        let p = e.path();
        // Peripheral batteries (mice, headsets) have scope "Device".
        if read(&p.join("type")).as_deref() != Some("Battery") || read(&p.join("scope")).as_deref() == Some("Device") {
            continue;
        }
        let Some(capacity) = read(&p.join("capacity")).and_then(|c| c.parse::<u32>().ok()) else { continue };
        total += capacity.min(100);
        count += 1;
        charging |= matches!(read(&p.join("status")).as_deref(), Some("Charging" | "Full"));
    }
    (count > 0).then(|| ((total / count) as u8, charging))
}

fn network() -> Network {
    let Ok(entries) = std::fs::read_dir("/sys/class/net") else { return Network::Offline };
    let mut best = Network::Offline;
    for e in entries.flatten() {
        let p = e.path();
        let name = e.file_name().to_string_lossy().into_owned();
        // Loopback, bridges and virtual links don't mean the machine is online.
        if name == "lo" || !p.join("device").exists() {
            continue;
        }
        if read(&p.join("operstate")).as_deref() != Some("up") {
            continue;
        }
        if p.join("wireless").exists() || p.join("phy80211").exists() {
            if best == Network::Offline {
                best = Network::Wireless;
            }
        } else {
            best = Network::Wired;
        }
    }
    best
}

/// The main keyboard's active layout, shortened: "English (US)" becomes `EN`.
fn layout() -> Option<String> {
    let reply = yozora_ui::hypr::request("j/devices").ok()?;
    let value: serde_json::Value = serde_json::from_str(&reply).ok()?;
    let keyboards = value["keyboards"].as_array()?;
    let main = keyboards.iter().find(|k| k["main"].as_bool() == Some(true)).or(keyboards.first())?;
    short_layout(main["active_keymap"].as_str()?)
}

pub fn short_layout(keymap: &str) -> Option<String> {
    let word = keymap.split(|c: char| !c.is_alphabetic()).find(|w| !w.is_empty())?;
    Some(word.chars().take(2).flat_map(char::to_uppercase).collect())
}

/// The wallpaper on `output`: the configured image, else whatever swaybg, hyprpaper or swww
/// is showing there.
pub fn wallpaper(configured: &str, output: Option<&str>) -> Option<PathBuf> {
    if !configured.trim().is_empty() {
        let path = expand(&configured.replace("{output}", output.unwrap_or_default()));
        return path.is_file().then_some(path);
    }
    let output = output.unwrap_or_default();
    swaybg(output).or_else(|| hyprpaper(output)).or_else(|| swww(output)).filter(|p| p.is_file())
}

fn expand(path: &str) -> PathBuf {
    match path.strip_prefix("~/") {
        Some(rest) => std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default().join(rest),
        None => PathBuf::from(path),
    }
}

/// From the arguments of running swaybg processes: `-o OUTPUT -i IMAGE`, or `-i IMAGE` for all.
fn swaybg(output: &str) -> Option<PathBuf> {
    let mut any = None;
    for e in std::fs::read_dir("/proc").ok()?.flatten() {
        if !e.file_name().to_string_lossy().bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        let Ok(cmdline) = std::fs::read(e.path().join("cmdline")) else { continue };
        let args: Vec<String> = cmdline
            .split(|b| *b == 0)
            .filter(|a| !a.is_empty())
            .map(|a| String::from_utf8_lossy(a).into_owned())
            .collect();
        if !args.first().is_some_and(|a| a.rsplit('/').next() == Some("swaybg")) {
            continue;
        }
        if let Some(path) = swaybg_image(&args, output) {
            return Some(path);
        }
        any = any.or_else(|| swaybg_image(&args, "*"));
    }
    any
}

pub fn swaybg_image(args: &[String], output: &str) -> Option<PathBuf> {
    let mut current = "*".to_string();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "-o" | "--output" => current = it.next()?.clone(),
            "-i" | "--image" => {
                let image = it.next()?;
                if current == output || current == "*" {
                    return Some(PathBuf::from(image));
                }
            }
            _ => {}
        }
    }
    None
}

/// `hyprctl hyprpaper listactive` prints `DP-1 = /path/to/image`.
fn hyprpaper(output: &str) -> Option<PathBuf> {
    let reply = hyprpaper_request("listactive")?;
    reply.lines().find_map(|l| {
        let (name, path) = l.split_once(" = ")?;
        let name = name.trim();
        (name == output || name.is_empty()).then(|| PathBuf::from(path.trim()))
    })
}

fn hyprpaper_request(command: &str) -> Option<String> {
    use std::io::{Read, Write};
    let runtime = std::env::var_os("XDG_RUNTIME_DIR")?;
    let sig = std::env::var("HYPRLAND_INSTANCE_SIGNATURE").ok()?;
    let mut stream =
        std::os::unix::net::UnixStream::connect(Path::new(&runtime).join("hypr").join(sig).join(".hyprpaper.sock"))
            .ok()?;
    stream.set_read_timeout(Some(std::time::Duration::from_millis(300))).ok()?;
    stream.write_all(command.as_bytes()).ok()?;
    let mut reply = String::new();
    stream.read_to_string(&mut reply).ok()?;
    Some(reply)
}

/// `swww query` prints `DP-1: 1920x1080, scale: 1, currently displaying: image: /path`.
fn swww(output: &str) -> Option<PathBuf> {
    let out = std::process::Command::new("swww").arg("query").stderr(std::process::Stdio::null()).output().ok()?;
    String::from_utf8_lossy(&out.stdout).lines().find_map(|l| {
        let (name, rest) = l.split_once(':')?;
        let name = name.trim().trim_start_matches(": ");
        if name != output {
            return None;
        }
        Some(PathBuf::from(rest.split("image: ").nth(1)?.trim()))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layouts_shorten() {
        assert_eq!(short_layout("English (US)").as_deref(), Some("EN"));
        assert_eq!(short_layout("German").as_deref(), Some("GE"));
        assert_eq!(short_layout("").as_deref(), None);
    }

    #[test]
    fn swaybg_arguments() {
        let args: Vec<String> =
            ["swaybg", "-o", "DP-1", "-i", "/a.jpg", "-m", "fill", "-o", "HDMI-A-1", "-i", "/b.jpg"]
                .iter()
                .map(|s| s.to_string())
                .collect();
        assert_eq!(swaybg_image(&args, "DP-1"), Some("/a.jpg".into()));
        assert_eq!(swaybg_image(&args, "HDMI-A-1"), Some("/b.jpg".into()));
        assert_eq!(swaybg_image(&args, "DP-2"), None);
        let all: Vec<String> = ["swaybg", "-i", "/c.png"].iter().map(|s| s.to_string()).collect();
        assert_eq!(swaybg_image(&all, "DP-2"), Some("/c.png".into()));
    }
}
