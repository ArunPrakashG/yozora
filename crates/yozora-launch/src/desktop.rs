//! Desktop entries: the `.desktop` files that describe installed apps.
//!
//! Only what the launcher needs from the Desktop Entry Specification: names in the user's
//! language, keywords, `Exec` with its field codes, the visibility keys and actions.

use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

/// One launchable app.
#[derive(Clone, Debug, PartialEq)]
pub struct App {
    /// The desktop file ID, like `org.gnome.Nautilus.desktop`.
    pub id: String,
    pub path: PathBuf,
    pub name: String,
    pub generic: String,
    pub comment: String,
    pub keywords: Vec<String>,
    pub exec: Vec<String>,
    pub icon: String,
    pub terminal: bool,
    pub working_dir: Option<PathBuf>,
    pub wm_class: String,
    pub actions: Vec<Action>,
}

/// A desktop action, like Firefox's "New Private Window".
#[derive(Clone, Debug, PartialEq)]
pub struct Action {
    pub name: String,
    pub exec: Vec<String>,
}

/// The locale names to try for `Name[...]`, most specific first: `de_DE@euro`, `de_DE`, `de`.
pub fn locales() -> Vec<String> {
    let raw = ["LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .find_map(|k| std::env::var(k).ok().filter(|v| !v.is_empty()))
        .unwrap_or_default();
    locale_variants(&raw)
}

fn locale_variants(raw: &str) -> Vec<String> {
    let no_enc = |s: &str| -> String {
        match (s.find('.'), s.find('@')) {
            (Some(d), Some(a)) => format!("{}{}", &s[..d], &s[a..]),
            (Some(d), None) => s[..d].to_string(),
            _ => s.to_string(),
        }
    };
    let full = no_enc(raw);
    if full.is_empty() || full == "C" || full == "POSIX" {
        return Vec::new();
    }
    let mut out = vec![full.clone()];
    let without_mod = full.split('@').next().unwrap_or_default().to_string();
    if without_mod != full {
        out.push(without_mod.clone());
    }
    if let Some(lang) = without_mod.split('_').next()
        && lang != without_mod
    {
        out.push(lang.to_string());
    }
    out
}

/// Undoes the escapes allowed in string values.
fn unescape(v: &str) -> String {
    let mut out = String::with_capacity(v.len());
    let mut chars = v.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('s') => out.push(' '),
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some(';') => out.push(';'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

/// Splits an `Exec` value into arguments, following the spec's quoting rules.
pub fn split_exec(exec: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut cur = String::new();
    let mut in_arg = false;
    let mut quoted = false;
    let mut chars = exec.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                quoted = !quoted;
                in_arg = true;
            }
            '\\' if quoted => {
                if let Some(n) = chars.next() {
                    cur.push(n);
                }
            }
            ' ' | '\t' if !quoted => {
                if in_arg {
                    args.push(std::mem::take(&mut cur));
                    in_arg = false;
                }
            }
            _ => {
                cur.push(c);
                in_arg = true;
            }
        }
    }
    if in_arg {
        args.push(cur);
    }
    args
}

/// Replaces field codes. The launcher never passes files or URLs, so those codes go away.
pub fn expand(args: &[String], app: &App) -> Vec<String> {
    let mut out = Vec::with_capacity(args.len());
    for a in args {
        match a.as_str() {
            "%f" | "%F" | "%u" | "%U" | "%d" | "%D" | "%n" | "%N" | "%v" | "%m" => {}
            "%i" if !app.icon.is_empty() => {
                out.push("--icon".into());
                out.push(app.icon.clone());
            }
            "%i" => {}
            _ => {
                let mut s = String::with_capacity(a.len());
                let mut chars = a.chars();
                while let Some(c) = chars.next() {
                    if c != '%' {
                        s.push(c);
                        continue;
                    }
                    match chars.next() {
                        Some('%') => s.push('%'),
                        Some('c') => s.push_str(&app.name),
                        Some('k') => s.push_str(&app.path.to_string_lossy()),
                        // Unknown or file codes inside a word: dropped.
                        _ => {}
                    }
                }
                if !s.is_empty() {
                    out.push(s);
                }
            }
        }
    }
    out
}

fn in_path(program: &str) -> bool {
    if program.contains('/') {
        return Path::new(program).exists();
    }
    std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(program).is_file()))
}

/// Parses a desktop file. `None` for anything that shouldn't be listed: not an application,
/// hidden, not for this desktop, or with a missing `TryExec`.
pub fn parse(id: &str, path: &Path, text: &str, locales: &[String], desktops: &[String]) -> Option<App> {
    let mut group = "";
    let mut main = Fields::default();
    let mut actions: Vec<(String, Fields)> = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(g) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            group = g;
            if let Some(a) = g.strip_prefix("Desktop Action ") {
                actions.push((a.to_string(), Fields::default()));
            }
            continue;
        }
        let Some((key, value)) = line.split_once('=') else { continue };
        let (key, value) = (key.trim(), value.trim());
        if group == "Desktop Entry" {
            main.set(key, value, locales);
        } else if group.starts_with("Desktop Action ")
            && let Some((_, f)) = actions.last_mut()
        {
            f.set(key, value, locales);
        }
    }

    let get = |k: &str| main.get(k);
    if get("Type") != Some("Application") || get("NoDisplay") == Some("true") || get("Hidden") == Some("true") {
        return None;
    }
    let list = |k: &str| -> Vec<String> {
        get(k)
            .map(|v| v.split(';').map(str::trim).filter(|s| !s.is_empty()).map(unescape).collect())
            .unwrap_or_default()
    };
    let only = list("OnlyShowIn");
    if !only.is_empty() && !only.iter().any(|d| desktops.contains(d)) {
        return None;
    }
    if list("NotShowIn").iter().any(|d| desktops.contains(d)) {
        return None;
    }
    if let Some(t) = get("TryExec")
        && !in_path(&unescape(t))
    {
        return None;
    }
    let exec = split_exec(&unescape(get("Exec")?));
    if exec.is_empty() {
        return None;
    }
    let wanted: HashSet<String> = list("Actions").into_iter().collect();
    Some(App {
        id: id.to_string(),
        path: path.to_path_buf(),
        name: unescape(get("Name")?),
        generic: get("GenericName").map(unescape).unwrap_or_default(),
        comment: get("Comment").map(unescape).unwrap_or_default(),
        keywords: list("Keywords"),
        exec,
        icon: get("Icon").map(unescape).unwrap_or_default(),
        terminal: get("Terminal") == Some("true"),
        working_dir: get("Path").filter(|p| !p.is_empty()).map(|p| PathBuf::from(unescape(p))),
        wm_class: get("StartupWMClass").map(unescape).unwrap_or_default(),
        actions: actions
            .into_iter()
            .filter(|(a, _)| wanted.contains(a))
            .filter_map(|(_, f)| {
                Some(Action { name: unescape(f.get("Name")?), exec: split_exec(&unescape(f.get("Exec")?)) })
            })
            .filter(|a| !a.exec.is_empty())
            .collect(),
    })
}

/// A group's keys, with the best localized value kept for each.
#[derive(Default)]
struct Fields {
    /// (key, value, rank): rank 0 is the most specific locale; unlocalized ranks last.
    values: Vec<(String, String, usize)>,
}

impl Fields {
    fn set(&mut self, key: &str, value: &str, locales: &[String]) {
        let (base, rank) = match key.split_once('[') {
            Some((base, rest)) => {
                let locale = rest.trim_end_matches(']');
                match locales.iter().position(|l| l == locale) {
                    Some(r) => (base, r),
                    None => return,
                }
            }
            None => (key, usize::MAX),
        };
        match self.values.iter_mut().find(|(k, _, _)| k == base) {
            Some(slot) if rank <= slot.2 => *slot = (base.to_string(), value.to_string(), rank),
            Some(_) => {}
            None => self.values.push((base.to_string(), value.to_string(), rank)),
        }
    }

    fn get(&self, key: &str) -> Option<&str> {
        self.values.iter().find(|(k, _, _)| k == key).map(|(_, v, _)| v.as_str())
    }
}

/// `applications` directories, most important first: the user's, then the system's.
pub fn dirs() -> Vec<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let data_home = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| home.map(|h| h.join(".local/share")));
    let data = std::env::var("XDG_DATA_DIRS")
        .ok()
        .filter(|d| !d.is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".into());
    let mut dirs: Vec<PathBuf> = data_home.into_iter().collect();
    dirs.extend(data.split(':').filter(|d| !d.is_empty()).map(PathBuf::from));
    let mut seen = HashSet::new();
    dirs.into_iter().map(|d| d.join("applications")).filter(|d| seen.insert(d.clone())).collect()
}

/// The desktop names this session matches `OnlyShowIn` against.
pub fn current_desktops() -> Vec<String> {
    std::env::var("XDG_CURRENT_DESKTOP")
        .unwrap_or_else(|_| "Hyprland".into())
        .split(':')
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

/// Every listed app. Earlier directories hide later ones with the same desktop file ID.
pub fn scan(dirs: &[PathBuf]) -> Vec<App> {
    let locales = locales();
    let desktops = current_desktops();
    let mut seen = HashSet::new();
    let mut apps = Vec::new();
    for dir in dirs {
        let mut stack = vec![dir.clone()];
        while let Some(d) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&d) else { continue };
            for entry in entries.flatten() {
                let path = entry.path();
                if entry.file_type().is_ok_and(|t| t.is_dir()) {
                    stack.push(path);
                    continue;
                }
                if path.extension().is_none_or(|e| e != "desktop") {
                    continue;
                }
                let Ok(rel) = path.strip_prefix(dir) else { continue };
                let id = rel.to_string_lossy().replace('/', "-");
                // Seen first wins, even when that entry is hidden: that's how users hide apps.
                if !seen.insert(id.clone()) {
                    continue;
                }
                let Ok(text) = std::fs::read_to_string(&path) else { continue };
                if let Some(app) = parse(&id, &path, &text, &locales, &desktops) {
                    apps.push(app);
                }
            }
        }
    }
    apps.sort_by_cached_key(|a| a.name.to_lowercase());
    apps
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIREFOX: &str = r#"[Desktop Entry]
Version=1.0
Name=Firefox
Name[de]=Firefox-Browser
GenericName=Web Browser
Comment=Browse the World Wide Web
Keywords=Internet;WWW;Browser;Web;Explorer;
Exec=/usr/lib/firefox/firefox %u
Icon=firefox
Terminal=false
Type=Application
Actions=new-window;new-private-window;

[Desktop Action new-window]
Name=New Window
Exec=/usr/lib/firefox/firefox --new-window %u

[Desktop Action new-private-window]
Name=New Private Window
Name[de]=Neues privates Fenster
Exec=/usr/lib/firefox/firefox --private-window %u

[Desktop Action unlisted]
Name=Not in Actions
Exec=true
"#;

    fn p(text: &str, locales: &[&str]) -> Option<App> {
        let locales: Vec<String> = locales.iter().map(|s| s.to_string()).collect();
        parse("firefox.desktop", Path::new("/x/firefox.desktop"), text, &locales, &["Hyprland".into()])
    }

    #[test]
    fn parses_an_entry_with_actions() {
        let app = p(FIREFOX, &[]).unwrap();
        assert_eq!(app.name, "Firefox");
        assert_eq!(app.generic, "Web Browser");
        assert_eq!(app.keywords, vec!["Internet", "WWW", "Browser", "Web", "Explorer"]);
        assert_eq!(app.exec, vec!["/usr/lib/firefox/firefox", "%u"]);
        assert_eq!(app.actions.len(), 2);
        assert_eq!(app.actions[1].name, "New Private Window");
    }

    #[test]
    fn picks_the_users_language() {
        let app = p(FIREFOX, &["de_DE", "de"]).unwrap();
        assert_eq!(app.name, "Firefox-Browser");
        assert_eq!(app.actions[1].name, "Neues privates Fenster");
        assert_eq!(locale_variants("de_DE.UTF-8@euro"), vec!["de_DE@euro", "de_DE", "de"]);
        assert!(locale_variants("C.UTF-8").is_empty());
    }

    #[test]
    fn skips_what_should_not_be_listed() {
        assert!(p("[Desktop Entry]\nType=Application\nName=X\nExec=x\nNoDisplay=true\n", &[]).is_none());
        assert!(p("[Desktop Entry]\nType=Link\nName=X\nURL=https://x\n", &[]).is_none());
        assert!(p("[Desktop Entry]\nType=Application\nName=X\nExec=x\nOnlyShowIn=GNOME;KDE;\n", &[]).is_none());
        assert!(p("[Desktop Entry]\nType=Application\nName=X\nExec=x\nNotShowIn=Hyprland;\n", &[]).is_none());
        assert!(p("[Desktop Entry]\nType=Application\nName=X\nExec=x\nTryExec=/nonexistent/yz\n", &[]).is_none());
        assert!(p("[Desktop Entry]\nType=Application\nName=X\nExec=x\nOnlyShowIn=Hyprland;\n", &[]).is_some());
    }

    #[test]
    fn splits_and_expands_exec() {
        assert_eq!(split_exec(r#"sh -c "echo \"hi there\"" %F"#), vec!["sh", "-c", "echo \"hi there\"", "%F"]);
        assert_eq!(split_exec(r#"app "" x"#), vec!["app", "", "x"]);
        let app = App {
            id: "a.desktop".into(),
            path: "/a.desktop".into(),
            name: "A".into(),
            generic: String::new(),
            comment: String::new(),
            keywords: vec![],
            exec: vec![],
            icon: "a-icon".into(),
            terminal: false,
            working_dir: None,
            wm_class: String::new(),
            actions: vec![],
        };
        let args: Vec<String> = ["a", "%U", "--title=%c", "%i", "100%%"].iter().map(|s| s.to_string()).collect();
        assert_eq!(expand(&args, &app), vec!["a", "--title=A", "--icon", "a-icon", "100%"]);
    }

    #[test]
    fn earlier_directories_win() {
        let root = std::env::temp_dir().join(format!("yozora-launch-apps-{}", std::process::id()));
        let (user, system) = (root.join("user"), root.join("system"));
        std::fs::create_dir_all(user.join("sub")).unwrap();
        std::fs::create_dir_all(&system).unwrap();
        let entry = |name: &str| format!("[Desktop Entry]\nType=Application\nName={name}\nExec=x\n");
        std::fs::write(user.join("a.desktop"), entry("User A")).unwrap();
        std::fs::write(system.join("a.desktop"), entry("System A")).unwrap();
        std::fs::write(user.join("hidden.desktop"), "[Desktop Entry]\nType=Application\nName=H\nExec=x\nHidden=true\n")
            .unwrap();
        std::fs::write(system.join("hidden.desktop"), entry("Hidden")).unwrap();
        std::fs::write(user.join("sub/b.desktop"), entry("B")).unwrap();
        let apps = scan(&[user, system]);
        let names: Vec<_> = apps.iter().map(|a| (a.id.as_str(), a.name.as_str())).collect();
        assert_eq!(names, vec![("sub-b.desktop", "B"), ("a.desktop", "User A")]);
        let _ = std::fs::remove_dir_all(&root);
    }
}
