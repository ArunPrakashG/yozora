//! Turns a query into sections of results. Everything here runs on the UI thread on every
//! keystroke, so it only reads data that's already in memory: slow sources (files from
//! locate, the clipboard) are fetched elsewhere and handed in.

use std::{path::PathBuf, sync::Arc};

use gpui::SharedString;
use yozora_ui::Icon;

use crate::{
    browser::Page,
    calc, clock,
    desktop::App,
    matcher::Fuzzy,
    money::{self, Rates},
    projects::Project,
    scripts::{self, Script},
    sources::{self, Clip, Cmd, Emoji, FileHit, Win},
    ssh::Host,
    usage::Usage,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    All,
    Windows,
    Clipboard,
    Files,
    Emoji,
    Calc,
    Shell,
    Web,
    Projects,
    Ssh,
    Browser,
}

impl Mode {
    pub fn label(self) -> &'static str {
        match self {
            Mode::All => "",
            Mode::Windows => "Windows",
            Mode::Clipboard => "Clipboard",
            Mode::Files => "Files",
            Mode::Emoji => "Emoji",
            Mode::Calc => "Math",
            Mode::Shell => "Shell",
            Mode::Web => "Web",
            Mode::Projects => "Projects",
            Mode::Ssh => "SSH",
            Mode::Browser => "Browser",
        }
    }

    pub fn icon(self) -> Icon {
        match self {
            Mode::All => Icon::Search,
            Mode::Windows => Icon::Window,
            Mode::Clipboard => Icon::Clipboard,
            Mode::Files => Icon::Folder,
            Mode::Emoji => Icon::Smile,
            Mode::Calc => Icon::Calculator,
            Mode::Shell => Icon::Shell,
            Mode::Web => Icon::Globe,
            Mode::Projects => Icon::Code,
            Mode::Ssh => Icon::Terminal,
            Mode::Browser => Icon::Star,
        }
    }
}

/// The prefixes, in the order the empty state shows them: (typed, mode).
pub const PREFIXES: &[(&str, Mode)] = &[
    ("@win", Mode::Windows),
    ("@clip", Mode::Clipboard),
    ("@files", Mode::Files),
    ("@code", Mode::Projects),
    ("@ssh", Mode::Ssh),
    ("@browser", Mode::Browser),
    ("@emoji", Mode::Emoji),
    ("=", Mode::Calc),
    ("$", Mode::Shell),
    ("?", Mode::Web),
];

/// Short forms that also work.
const ALIASES: &[(&str, Mode)] = &[
    ("@w", Mode::Windows),
    ("@c", Mode::Clipboard),
    ("@f", Mode::Files),
    ("@e", Mode::Emoji),
    (":", Mode::Emoji),
    ("@p", Mode::Projects),
    ("@s", Mode::Ssh),
    ("@b", Mode::Browser),
];

/// Splits a mode prefix off the query.
pub fn parse_mode(query: &str) -> (Mode, &str) {
    let t = query.trim_start();
    for (p, mode) in PREFIXES.iter().chain(ALIASES) {
        if let Some(rest) = t.strip_prefix(p) {
            // `@win` needs a space or nothing after it, so `@wine` isn't windows; symbols don't.
            let symbol = !p.starts_with('@');
            if symbol || rest.is_empty() || rest.starts_with(' ') {
                return (*mode, rest.trim());
            }
        }
    }
    (Mode::All, t.trim_end())
}

/// A mode prefix that's finished being typed, so it can become a chip: the prefix as
/// [`PREFIXES`] spells it, and what follows. `@win ` needs its space; `=` and `$` don't.
pub fn committed_prefix(text: &str) -> Option<(&'static str, &str)> {
    let t = text.trim_start();
    PREFIXES.iter().chain(ALIASES).find_map(|(p, mode)| {
        let rest = t.strip_prefix(p)?;
        if !p.starts_with('@') || rest.starts_with(' ') {
            let canonical = PREFIXES.iter().find(|(_, m)| m == mode).map(|(p, _)| *p)?;
            Some((canonical, rest.trim_start()))
        } else {
            None
        }
    })
}

#[derive(Clone, Debug, PartialEq)]
pub enum Visual {
    /// An app or file icon, already sized.
    Image(Arc<PathBuf>),
    Glyph(Icon),
    Emoji(SharedString),
    /// The first letter of an app with no icon.
    Letter(char),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Target {
    App { index: usize, action: Option<usize> },
    Window(String),
    Command(usize),
    Copy(String),
    Clip(String),
    File(PathBuf),
    Shell(String),
    Web(String),
    Script(usize),
    Ssh(String),
    Project(PathBuf),
    Url(String),
}

impl Target {
    /// What Enter does, for the hint on the selected row and in the footer.
    pub fn verb(&self) -> &'static str {
        match self {
            Target::App { .. } | Target::File(_) => "Open",
            Target::Window(_) => "Focus",
            Target::Command(_) | Target::Shell(_) => "Run",
            Target::Copy(_) | Target::Clip(_) => "Copy",
            Target::Web(_) => "Search",
            Target::Script(_) => "Run",
            Target::Ssh(_) => "Connect",
            Target::Project(_) | Target::Url(_) => "Open",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub title: String,
    /// Char indices of `title` to highlight.
    pub hl: Vec<usize>,
    pub sub: String,
    pub visual: Visual,
    /// A workspace, for windows.
    pub badge: Option<String>,
    pub pinned: bool,
    pub top: bool,
    pub target: Target,
    /// The usage key, for things that are ranked by use and can be pinned.
    pub key: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Section {
    pub title: &'static str,
    pub rows: Vec<Row>,
}

/// An app in the index, with what's precomputed for matching.
#[derive(Clone, Debug)]
pub struct Entry {
    pub app: App,
    pub icon: Option<Arc<PathBuf>>,
    /// Generic name, keywords and the program's name, joined.
    pub extra: String,
}

impl Entry {
    pub fn new(app: App, icon: Option<PathBuf>) -> Self {
        let program = app
            .exec
            .first()
            .and_then(|p| std::path::Path::new(p).file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let mut extra = app.generic.clone();
        for k in &app.keywords {
            extra.push(' ');
            extra.push_str(k);
        }
        // `env`, `flatpak` and the like say nothing about the app.
        if !matches!(program.as_str(), "env" | "flatpak" | "sh" | "bash" | "gtk-launch" | "snap") {
            extra.push(' ');
            extra.push_str(&program);
        }
        Self { app, icon: icon.map(Arc::new), extra }
    }

    pub fn key(&self) -> String {
        app_key(&self.app.id)
    }

    pub fn visual(&self) -> Visual {
        match &self.icon {
            Some(p) => Visual::Image(p.clone()),
            None => Visual::Letter(self.app.name.chars().next().unwrap_or('?').to_ascii_uppercase()),
        }
    }

    fn sub(&self) -> String {
        if !self.app.generic.is_empty() && self.app.generic != self.app.name {
            self.app.generic.clone()
        } else {
            self.app.comment.clone()
        }
    }
}

pub fn app_key(id: &str) -> String {
    format!("app:{id}")
}

pub fn cmd_key(id: &str) -> String {
    format!("cmd:{id}")
}

/// Everything a search reads.
pub struct Sources<'a> {
    pub apps: &'a [Entry],
    pub windows: &'a [Win],
    pub commands: &'a [Cmd],
    pub clips: &'a [Clip],
    pub emoji: &'a [Emoji],
    /// Recent files, matched here, and locate results for the current query.
    pub recent: &'a [FileHit],
    pub located: &'a [FileHit],
    pub usage: &'a Usage,
    pub now: (i64, u8),
    pub max: usize,
    pub suggestions: bool,
    pub files_in_search: bool,
    pub home: &'a std::path::Path,
    pub web_name: &'a str,
    pub scripts: &'a [Script],
    pub hosts: &'a [Host],
    pub projects: &'a [Project],
    pub pages: &'a [Page],
    pub rates: Option<&'a Rates>,
}

/// Window icons come from the app with the same class.
fn window_visual(w: &Win, apps: &[Entry]) -> Visual {
    let class = w.class.to_lowercase();
    apps.iter()
        .find(|e| {
            e.app.wm_class.eq_ignore_ascii_case(&w.class)
                || e.app.id.trim_end_matches(".desktop").to_lowercase() == class
                || e.app.icon.to_lowercase() == class
        })
        .map(Entry::visual)
        .unwrap_or(Visual::Glyph(Icon::Window))
}

fn window_row(w: &Win, f: &mut Fuzzy, apps: &[Entry]) -> Row {
    let title = if w.title.is_empty() { w.class.clone() } else { w.title.clone() };
    Row {
        hl: f.indices(&title),
        title,
        sub: pretty_class(&w.class, apps),
        visual: window_visual(w, apps),
        badge: Some(w.workspace.clone()),
        pinned: false,
        top: false,
        target: Target::Window(w.address.clone()),
        key: None,
    }
}

fn pretty_class(class: &str, apps: &[Entry]) -> String {
    apps.iter()
        .find(|e| {
            e.app.wm_class.eq_ignore_ascii_case(class)
                || e.app.id.trim_end_matches(".desktop").eq_ignore_ascii_case(class)
        })
        .map(|e| e.app.name.clone())
        .unwrap_or_else(|| class.rsplit('.').next().unwrap_or(class).to_string())
}

fn app_row(i: usize, e: &Entry, f: &mut Fuzzy, s: &Sources) -> Row {
    let key = e.key();
    Row {
        hl: f.indices(&e.app.name),
        title: e.app.name.clone(),
        sub: e.sub(),
        visual: e.visual(),
        badge: None,
        pinned: s.usage.is_pinned(&key),
        top: false,
        target: Target::App { index: i, action: None },
        key: Some(key),
    }
}

fn cmd_row(i: usize, c: &Cmd, f: &mut Fuzzy) -> Row {
    Row {
        hl: f.indices(c.name),
        title: c.name.to_string(),
        sub: c.sub.clone(),
        visual: Visual::Glyph(c.icon),
        badge: None,
        pinned: false,
        top: false,
        target: Target::Command(i),
        key: Some(cmd_key(c.id)),
    }
}

fn file_row(h: &FileHit, f: &mut Fuzzy, home: &std::path::Path) -> Row {
    let name = h.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let dir = h.path.parent().map(|p| sources::tilde(p, home)).unwrap_or_default();
    Row {
        hl: f.indices(&name),
        title: name,
        sub: dir,
        visual: Visual::Glyph(sources::file_icon(h)),
        badge: None,
        pinned: false,
        top: false,
        target: Target::File(h.path.clone()),
        key: None,
    }
}

fn script_row(i: usize, sc: &Script, f: &mut Fuzzy, s: &Sources) -> Row {
    let key = sc.key();
    Row {
        hl: f.indices(&sc.title),
        title: sc.title.clone(),
        sub: sc.sub(),
        visual: match &sc.icon {
            Some(scripts::Icon::Image(p)) => Visual::Image(Arc::new(p.clone())),
            Some(scripts::Icon::Text(t)) => Visual::Emoji(t.clone().into()),
            None => Visual::Glyph(Icon::Shell),
        },
        badge: None,
        pinned: s.usage.is_pinned(&key),
        top: false,
        target: Target::Script(i),
        key: Some(key),
    }
}

/// The one row shown while a script's arguments are typed: Enter runs it.
pub fn args_row(i: usize, sc: &Script) -> Row {
    let wants: Vec<String> = sc
        .args
        .iter()
        .map(|a| if a.optional { format!("[{}]", a.placeholder) } else { a.placeholder.clone() })
        .collect();
    Row {
        hl: Vec::new(),
        title: sc.title.clone(),
        sub: format!("Type {} · Esc to go back", wants.join(" ")),
        visual: match &sc.icon {
            Some(scripts::Icon::Image(p)) => Visual::Image(Arc::new(p.clone())),
            Some(scripts::Icon::Text(t)) => Visual::Emoji(t.clone().into()),
            None => Visual::Glyph(Icon::Shell),
        },
        badge: None,
        pinned: false,
        top: false,
        target: Target::Script(i),
        key: Some(sc.key()),
    }
}

fn host_row(h: &Host, f: &mut Fuzzy) -> Row {
    Row {
        hl: f.indices(&h.alias),
        title: h.alias.clone(),
        sub: h.sub(),
        visual: Visual::Glyph(Icon::Terminal),
        badge: None,
        pinned: false,
        top: false,
        target: Target::Ssh(h.alias.clone()),
        key: Some(h.key()),
    }
}

fn project_row(p: &Project, f: &mut Fuzzy, s: &Sources) -> Row {
    let key = p.key();
    let dir = sources::tilde(&p.path, s.home);
    Row {
        hl: f.indices(&p.name),
        title: p.name.clone(),
        sub: match p.recent {
            Some(app) => format!("{dir} · recent in {app}"),
            None => dir,
        },
        visual: Visual::Glyph(Icon::Code),
        badge: None,
        pinned: s.usage.is_pinned(&key),
        top: false,
        target: Target::Project(p.path.clone()),
        key: Some(key),
    }
}

fn page_row(p: &Page, f: &mut Fuzzy) -> Row {
    let title: String = p.title.chars().take(120).collect();
    let url = p.url.split_once("://").map(|(_, r)| r).unwrap_or(&p.url);
    Row {
        hl: f.indices(&title),
        title,
        sub: format!("{} · {}", url.chars().take(90).collect::<String>(), p.browser),
        visual: Visual::Glyph(if p.bookmark { Icon::Star } else { Icon::Clock }),
        badge: None,
        pinned: false,
        top: false,
        target: Target::Url(p.url.clone()),
        key: None,
    }
}

fn score_script(sc: &Script, f: &mut Fuzzy) -> Option<u32> {
    f.score(&sc.title).max(f.score_words(&sc.sub()).map(|x| x * 3 / 5))
}

fn score_host(h: &Host, f: &mut Fuzzy) -> Option<u32> {
    f.score(&h.alias).max(f.score_words(&h.hostname).map(|x| x * 4 / 5))
}

fn score_project(p: &Project, f: &mut Fuzzy) -> Option<u32> {
    let path = p.path.to_string_lossy();
    f.score(&p.name).max(f.score_words(&path).map(|x| x * 3 / 5))
}

fn score_page(p: &Page, f: &mut Fuzzy) -> Option<u32> {
    f.score_words(&p.title).max(f.score_words(&p.words).map(|x| x * 4 / 5))
}

/// Recent projects and bookmarks, and what's visited often, rank higher.
fn project_bonus(p: &Project) -> u32 {
    if p.recent.is_some() { 30 } else { 0 }
}

fn page_bonus(p: &Page) -> u32 {
    (if p.bookmark { 40 } else { 0 }) + (8.0 * (1.0 + f64::from(p.visits)).ln()) as u32
}

/// Ranks `items` by text score plus `bonus`, keeping those `keep` accepts.
fn rank<'a, T>(
    items: &'a [T],
    f: &mut Fuzzy,
    q: &str,
    score: impl Fn(&T, &mut Fuzzy) -> Option<u32>,
    bonus: impl Fn(&T) -> u32,
    keep: impl Fn(u32) -> bool,
) -> Vec<(u32, usize, &'a T)> {
    let mut out: Vec<(u32, usize, &T)> = items
        .iter()
        .enumerate()
        .filter_map(|(i, x)| {
            let sc = score(x, f)?;
            keep(sc).then(|| (sc + bonus(x), i, x))
        })
        .collect();
    if !q.is_empty() {
        out.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    }
    out
}

/// A calculation, a conversion, a currency or a time, with its section's title.
fn answer(q: &str, s: &Sources, forced: bool) -> Option<(&'static str, Row)> {
    if let Some(a) = calc::answer(q, forced) {
        let title = if a.detail.contains('→') { "Convert" } else { "Math" };
        return Some((title, calc_row(a)));
    }
    if let Some(a) = s.rates.and_then(|r| money::answer(q, r)) {
        let copy = money::plain(&a.value);
        let mut row = calc_row(a);
        row.target = Target::Copy(copy);
        return Some(("Convert", row));
    }
    let now = chrono::DateTime::from_timestamp(s.now.0, 0)?;
    let a = clock::answer(q, now)?;
    let mut row = calc_row(a);
    row.title = row.title.trim_start_matches("= ").to_string();
    row.visual = Visual::Glyph(Icon::Clock);
    Some(("Time", row))
}

fn web_row(q: &str, web_name: &str) -> Row {
    Row {
        title: format!("Search the web for “{q}”"),
        hl: Vec::new(),
        sub: format!("{web_name} · Ctrl ↵ from any result"),
        visual: Visual::Glyph(Icon::Globe),
        badge: None,
        pinned: false,
        top: false,
        target: Target::Web(q.to_string()),
        key: None,
    }
}

/// How much use and pins add to a text match. A strong match still beats a weak one that's
/// used a lot, but between similar matches the one you use wins.
fn boost(usage: f64, pinned: bool) -> u32 {
    (25.0 * (1.0 + usage).ln()) as u32 + if pinned { 60 } else { 0 }
}

/// Scores an app: by name, or a little less by its keywords and generic name.
fn score_app(e: &Entry, f: &mut Fuzzy) -> Option<u32> {
    let by_name = f.score(&e.app.name);
    let by_extra = if e.extra.is_empty() { None } else { f.score_words(&e.extra).map(|s| s * 3 / 5) };
    by_name.max(by_extra)
}

fn usage_boost(s: &Sources, key: &str) -> u32 {
    let (now, hour) = s.now;
    boost(s.usage.score(key, now, hour), s.usage.is_pinned(key))
}

/// Whether a match is good enough to show in a section that isn't the one being searched:
/// windows, commands and files only join app results when they clearly match.
fn strong(score: u32, needle: &str) -> bool {
    score >= 24 * needle.chars().count().min(8) as u32
}

pub fn search(query: &str, f: &mut Fuzzy, s: &Sources) -> Vec<Section> {
    let (mode, q) = parse_mode(query);
    f.set(q);
    let max = s.max;
    match mode {
        Mode::All if q.is_empty() => empty(f, s),
        Mode::All => everything(q, f, s),
        Mode::Windows => {
            let mut scored: Vec<(u32, &Win)> = s
                .windows
                .iter()
                .filter_map(|w| {
                    let sc = f.score(&w.title).max(f.score(&w.class).map(|x| x * 4 / 5))?;
                    Some((sc, w))
                })
                .collect();
            if !q.is_empty() {
                scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.recency.cmp(&b.1.recency)));
            }
            let rows = scored.into_iter().take(max * 2).map(|(_, w)| window_row(w, f, s.apps)).collect();
            vec![Section { title: "Open windows", rows }]
        }
        Mode::Clipboard => {
            let mut scored: Vec<(u32, usize, &Clip)> =
                s.clips.iter().enumerate().filter_map(|(i, c)| Some((f.score(&c.preview)?, i, c))).collect();
            if !q.is_empty() {
                scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
            }
            let rows = scored
                .into_iter()
                .take(max * 2)
                .map(|(_, _, c)| {
                    let title: String = c.preview.chars().take(160).collect();
                    Row {
                        hl: f.indices(&title),
                        title,
                        sub: if c.image { "Image".into() } else { "Text".into() },
                        visual: Visual::Glyph(if c.image {
                            Icon::Image
                        } else if c.preview.starts_with("http://") || c.preview.starts_with("https://") {
                            Icon::Link
                        } else {
                            Icon::Clipboard
                        }),
                        badge: None,
                        pinned: false,
                        top: false,
                        target: Target::Clip(c.line.clone()),
                        key: None,
                    }
                })
                .collect();
            vec![Section { title: "Clipboard", rows }]
        }
        Mode::Emoji => {
            let mut scored: Vec<(u32, usize, &Emoji)> =
                s.emoji.iter().enumerate().filter_map(|(i, e)| Some((f.score_words(&e.search)?, i, e))).collect();
            if !q.is_empty() {
                scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
            }
            let rows = scored
                .into_iter()
                .take(max * 2)
                .map(|(_, _, e)| Row {
                    hl: f.indices(e.name),
                    title: e.name.to_string(),
                    sub: "Copy to the clipboard".into(),
                    visual: Visual::Emoji(SharedString::new_static(e.glyph)),
                    badge: None,
                    pinned: false,
                    top: false,
                    target: Target::Copy(e.glyph.to_string()),
                    key: None,
                })
                .collect();
            vec![Section { title: "Emoji", rows }]
        }
        Mode::Files => {
            let rows = files(q, f, s, max * 2, false);
            vec![Section { title: if q.is_empty() { "Recent files" } else { "Files" }, rows }]
        }
        Mode::Calc => match answer(q, s, true) {
            Some((title, row)) => vec![Section { title, rows: vec![row] }],
            None => Vec::new(),
        },
        Mode::Projects => {
            let rows = rank(s.projects, f, q, score_project, |p| project_bonus(p) + usage_boost(s, &p.key()), |_| true)
                .into_iter()
                .take(max * 2)
                .map(|(_, _, p)| project_row(p, f, s))
                .collect();
            vec![Section { title: "Projects", rows }]
        }
        Mode::Ssh => {
            let rows = rank(s.hosts, f, q, score_host, |h| usage_boost(s, &h.key()), |_| true)
                .into_iter()
                .take(max * 2)
                .map(|(_, _, h)| host_row(h, f))
                .collect();
            vec![Section { title: "SSH hosts", rows }]
        }
        Mode::Browser => {
            let rows = rank(s.pages, f, q, score_page, page_bonus, |_| true)
                .into_iter()
                .take(max * 2)
                .map(|(_, _, p)| page_row(p, f))
                .collect();
            vec![Section { title: if q.is_empty() { "Recent pages" } else { "Bookmarks and history" }, rows }]
        }
        Mode::Shell if q.is_empty() => Vec::new(),
        Mode::Shell => vec![Section {
            title: "Shell",
            rows: vec![Row {
                title: q.to_string(),
                hl: Vec::new(),
                sub: "Run in a terminal · Shift ↵ runs it without one".into(),
                visual: Visual::Glyph(Icon::Shell),
                badge: None,
                pinned: false,
                top: true,
                target: Target::Shell(q.to_string()),
                key: None,
            }],
        }],
        Mode::Web if q.is_empty() => Vec::new(),
        Mode::Web => vec![Section { title: "Web", rows: vec![web_row(q, s.web_name)] }],
    }
}

fn calc_row(a: calc::Answer) -> Row {
    Row {
        title: format!("= {}", a.value),
        hl: Vec::new(),
        sub: format!("{} · Enter copies the result", a.detail),
        visual: Visual::Glyph(Icon::Calculator),
        badge: None,
        pinned: false,
        top: true,
        target: Target::Copy(a.value),
        key: None,
    }
}

/// `strict` keeps to names where each typed word starts a word, for files mixed in with apps.
fn files(q: &str, f: &mut Fuzzy, s: &Sources, limit: usize, strict: bool) -> Vec<Row> {
    let mut seen = std::collections::HashSet::new();
    let mut scored: Vec<(u32, usize, &FileHit)> = Vec::new();
    for (i, h) in s.recent.iter().chain(s.located).enumerate() {
        if !seen.insert(&h.path) {
            continue;
        }
        let name = h.path.file_name().map(|n| n.to_string_lossy()).unwrap_or_default();
        // Recent files rank above located ones with the same score.
        let recent = i < s.recent.len();
        let sc = if strict { f.score_words(&name) } else { f.score(&name) };
        if let Some(sc) = sc {
            scored.push((sc + if recent { 40 } else { 0 }, i, h));
        }
    }
    if !q.is_empty() {
        scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    }
    scored.into_iter().take(limit).map(|(_, _, h)| file_row(h, f, s.home)).collect()
}

/// Nothing typed: pinned apps, then what's usually opened now.
fn empty(f: &mut Fuzzy, s: &Sources) -> Vec<Section> {
    let mut out = Vec::new();
    let index_of = |key: &str| s.apps.iter().position(|e| e.key() == key);
    let pinned: Vec<Row> =
        s.usage.pins.iter().filter_map(|k| index_of(k)).take(6).map(|i| app_row(i, &s.apps[i], f, s)).collect();
    if !pinned.is_empty() {
        out.push(Section { title: "Pinned", rows: pinned });
    }
    if s.suggestions {
        let (now, hour) = s.now;
        let rows: Vec<Row> = s
            .usage
            .top(now, hour, 24)
            .into_iter()
            .filter(|k| !s.usage.is_pinned(k) && !s.usage.is_hidden(k))
            .filter_map(|k| {
                if let Some(i) = index_of(k) {
                    let mut r = app_row(i, &s.apps[i], f, s);
                    r.sub = why(s.usage, k, now, hour);
                    return Some(r);
                }
                let mut r = if let Some(id) = k.strip_prefix("cmd:") {
                    let i = s.commands.iter().position(|c| c.id == id)?;
                    cmd_row(i, &s.commands[i], f)
                } else if k.starts_with("script:") {
                    let i = s.scripts.iter().position(|x| x.key() == k)?;
                    script_row(i, &s.scripts[i], f, s)
                } else if k.starts_with("ssh:") {
                    host_row(s.hosts.iter().find(|h| h.key() == k)?, f)
                } else if k.starts_with("proj:") {
                    project_row(s.projects.iter().find(|p| p.key() == k)?, f, s)
                } else {
                    return None;
                };
                r.sub = why(s.usage, k, now, hour);
                Some(r)
            })
            .take(4)
            .collect();
        if !rows.is_empty() {
            out.push(Section { title: "Suggested for this hour", rows });
        }
    }
    out
}

/// Why an app is suggested.
fn why(usage: &Usage, key: &str, now: i64, hour: u8) -> String {
    let Some(e) = usage.items.get(key) else { return String::new() };
    let ago = now - e.last;
    let near =
        e.hours.iter().filter(|h| matches!((i16::from(**h) - i16::from(hour)).rem_euclid(24), 0 | 1 | 23)).count();
    if e.hours.len() >= 5 && near * 2 >= e.hours.len() {
        return format!("Usually opened around {hour}:00");
    }
    match ago {
        a if a < 3600 => format!("Opened {} min ago", (a / 60).max(1)),
        a if a < 86_400 => format!("Opened {} h ago", a / 3600),
        _ => format!("Opened {} times", e.count),
    }
}

fn everything(q: &str, f: &mut Fuzzy, s: &Sources) -> Vec<Section> {
    let (now, hour) = s.now;
    let mut out = Vec::new();
    if let Some((title, row)) = answer(q, s, false) {
        out.push(Section { title, rows: vec![row] });
    }

    let mut apps: Vec<(u32, usize)> = s
        .apps
        .iter()
        .enumerate()
        .filter_map(|(i, e)| {
            let key = e.key();
            if s.usage.is_hidden(&key) {
                return None;
            }
            let text = score_app(e, f)?;
            Some((text + boost(s.usage.score(&key, now, hour), s.usage.is_pinned(&key)), i))
        })
        .collect();
    apps.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| s.apps[a.1].app.name.len().cmp(&s.apps[b.1].app.name.len())));

    // Commands and script commands, together.
    let mut cmds: Vec<(u32, Row)> = s
        .commands
        .iter()
        .enumerate()
        .filter_map(|(i, c)| {
            let sc = f.score(c.name).max(f.score_words(c.keywords).map(|x| x * 3 / 5))?;
            strong(sc, q).then(|| (sc + boost(s.usage.score(&cmd_key(c.id), now, hour), false), i))
        })
        .collect::<Vec<_>>()
        .into_iter()
        .map(|(sc, i)| (sc, cmd_row(i, &s.commands[i], f)))
        .collect();
    for (sc, i, script) in rank(s.scripts, f, q, score_script, |x| usage_boost(s, &x.key()), |sc| strong(sc, q)) {
        cmds.push((sc, script_row(i, script, f, s)));
    }
    cmds.sort_by_key(|c| std::cmp::Reverse(c.0));

    // The top hit is the best app, unless a command clearly wins.
    let top_is_cmd = matches!((apps.first(), cmds.first()), (Some(a), Some(c)) if c.0 > a.0)
        || (apps.is_empty() && !cmds.is_empty());
    let top = if top_is_cmd { Some(cmds[0].1.clone()) } else { apps.first().map(|a| app_row(a.1, &s.apps[a.1], f, s)) };
    if let Some(mut t) = top {
        t.top = out.is_empty();
        out.push(Section { title: "Top hit", rows: vec![t] });
    }
    let skip = usize::from(!top_is_cmd);
    let more: Vec<Row> = apps
        .iter()
        .skip(skip)
        .take(s.max.saturating_sub(2).max(1))
        .map(|(_, i)| app_row(*i, &s.apps[*i], f, s))
        .collect();
    if !more.is_empty() {
        out.push(Section { title: "Apps", rows: more });
    }

    let mut wins: Vec<(u32, &Win)> = s
        .windows
        .iter()
        .filter_map(|w| {
            let sc = f.score(&w.title).max(f.score(&w.class))?;
            strong(sc, q).then_some((sc, w))
        })
        .collect();
    wins.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.recency.cmp(&b.1.recency)));
    if !wins.is_empty() {
        out.push(Section {
            title: "Open windows",
            rows: wins.into_iter().take(2).map(|(_, w)| window_row(w, f, s.apps)).collect(),
        });
    }

    let cmd_rows: Vec<Row> = cmds.into_iter().skip(usize::from(top_is_cmd)).take(2).map(|(_, r)| r).collect();
    if !cmd_rows.is_empty() {
        out.push(Section { title: "Commands", rows: cmd_rows });
    }

    // Projects and hosts, the section with the better match first.
    let projects =
        rank(s.projects, f, q, score_project, |p| project_bonus(p) + usage_boost(s, &p.key()), |sc| strong(sc, q));
    let hosts = rank(s.hosts, f, q, score_host, |h| usage_boost(s, &h.key()), |sc| strong(sc, q));
    let hosts_first = hosts.first().map_or(0, |x| x.0) > projects.first().map_or(0, |x| x.0);
    let mut pair = [
        Section {
            title: "Projects",
            rows: projects.into_iter().take(2).map(|(_, _, p)| project_row(p, f, s)).collect(),
        },
        Section { title: "SSH hosts", rows: hosts.into_iter().take(2).map(|(_, _, h)| host_row(h, f)).collect() },
    ];
    if hosts_first {
        pair.reverse();
    }
    out.extend(pair.into_iter().filter(|sec| !sec.rows.is_empty()));

    if s.files_in_search && q.chars().count() >= 3 {
        let rows: Vec<Row> = files(q, f, s, 2, true);
        if !rows.is_empty() {
            out.push(Section { title: "Files", rows });
        }
    }

    if q.chars().count() >= 3 {
        let rows: Vec<Row> = rank(s.pages, f, q, score_page, page_bonus, |sc| strong(sc, q))
            .into_iter()
            .take(2)
            .map(|(_, _, p)| page_row(p, f))
            .collect();
        if !rows.is_empty() {
            out.push(Section { title: "Browser", rows });
        }
    }

    out.push(Section { title: "Web", rows: vec![web_row(q, s.web_name)] });
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::desktop::App;

    fn app(id: &str, name: &str, generic: &str, keywords: &[&str]) -> Entry {
        Entry::new(
            App {
                id: format!("{id}.desktop"),
                path: format!("/a/{id}.desktop").into(),
                name: name.into(),
                generic: generic.into(),
                comment: String::new(),
                keywords: keywords.iter().map(|s| s.to_string()).collect(),
                exec: vec![id.into()],
                icon: id.into(),
                terminal: false,
                working_dir: None,
                wm_class: String::new(),
                actions: vec![],
            },
            None,
        )
    }

    fn apps() -> Vec<Entry> {
        vec![
            app("firefox", "Firefox", "Web Browser", &["internet", "www"]),
            app("code", "Visual Studio Code", "Text Editor", &["vscode", "ide"]),
            app("ghostty", "Ghostty", "Terminal", &["shell", "console"]),
            app("nautilus", "Files", "File Manager", &["folder", "explorer"]),
            app("gimp", "GNU Image Manipulation Program", "Image Editor", &["photo"]),
        ]
    }

    fn run(q: &str, apps: &[Entry], usage: &Usage, windows: &[Win]) -> Vec<Section> {
        run_with(q, apps, usage, windows, &Extra::default())
    }

    #[derive(Default)]
    struct Extra {
        scripts: Vec<Script>,
        hosts: Vec<Host>,
        projects: Vec<Project>,
        pages: Vec<Page>,
        rates: Option<Rates>,
    }

    fn run_with(q: &str, apps: &[Entry], usage: &Usage, windows: &[Win], x: &Extra) -> Vec<Section> {
        let cmds = sources::commands(&Default::default(), None);
        let home = PathBuf::from("/home/u");
        let s = Sources {
            apps,
            windows,
            commands: &cmds,
            clips: &[],
            emoji: &[],
            recent: &[],
            located: &[],
            usage,
            now: (1_000_000, 10),
            max: 6,
            suggestions: true,
            files_in_search: true,
            home: &home,
            web_name: "DuckDuckGo",
            scripts: &x.scripts,
            hosts: &x.hosts,
            projects: &x.projects,
            pages: &x.pages,
            rates: x.rates.as_ref(),
        };
        search(q, &mut Fuzzy::new(), &s)
    }

    fn extra() -> Extra {
        let script = scripts::parse(
            std::path::Path::new("/s/update.sh"),
            "# @raycast.title Update System\n# @raycast.mode silent\n",
        )
        .unwrap();
        let host =
            |a: &str, h: &str| Host { alias: a.into(), hostname: h.into(), user: String::new(), port: String::new() };
        let project =
            |p: &str, recent| Project { path: PathBuf::from(p), name: p.rsplit('/').next().unwrap().into(), recent };
        let page = |url: &str, title: &str, visits, bookmark| Page {
            url: url.into(),
            title: title.into(),
            visits,
            bookmark,
            browser: "Chrome",
            words: url.split("://").nth(1).unwrap().replace(['.', '/'], " "),
        };
        Extra {
            scripts: vec![script],
            hosts: vec![host("homelab", "10.0.0.2"), host("vps", "example.net")],
            projects: vec![project("/home/u/src/yozora", Some("Zed")), project("/home/u/src/website", None)],
            pages: vec![
                page("https://github.com/notifications", "Notifications", 40, false),
                page("https://docs.rs/gpui", "gpui - Rust", 2, true),
            ],
            rates: money::Rates::parse(
                "<Cube time='2026-10-13'><Cube currency='USD' rate='1.1'/><Cube currency='INR' rate='92.4'/>",
            ),
        }
    }

    #[test]
    fn new_sources_join_results() {
        let apps = apps();
        let x = extra();
        let u = Usage::default();
        let r = run_with("update sys", &apps, &u, &[], &x);
        assert_eq!(r[0].title, "Top hit");
        assert_eq!(r[0].rows[0].title, "Update System");
        assert!(matches!(r[0].rows[0].target, Target::Script(0)));

        let r = run_with("yozora", &apps, &u, &[], &x);
        assert!(titles(&r).contains(&("Projects", vec!["yozora".to_string()])), "{:?}", titles(&r));
        let r = run_with("homelab", &apps, &u, &[], &x);
        assert!(titles(&r).contains(&("SSH hosts", vec!["homelab".to_string()])));
        // A host named for the query comes before a project whose folder only contains it.
        let mut x2 = extra();
        x2.hosts.push(Host {
            alias: "web-prod".into(),
            hostname: String::new(),
            user: String::new(),
            port: String::new(),
        });
        x2.projects.push(Project {
            path: PathBuf::from("/home/u/src/website/blog"),
            name: "blog".into(),
            recent: None,
        });
        x2.projects.retain(|p| p.name != "website");
        let r = run_with("web", &apps, &u, &[], &x2);
        let first = r.iter().find(|sec| sec.title == "SSH hosts" || sec.title == "Projects").unwrap();
        assert_eq!(first.title, "SSH hosts", "{:?}", titles(&r));
        let r = run_with("notifications", &apps, &u, &[], &x);
        assert!(titles(&r).contains(&("Browser", vec!["Notifications".to_string()])), "{:?}", titles(&r));

        // Each prefix lists its own, with no query too.
        let r = run_with("@ssh ", &apps, &u, &[], &x);
        assert_eq!(titles(&r), vec![("SSH hosts", vec!["homelab".to_string(), "vps".to_string()])]);
        let r = run_with("@code web", &apps, &u, &[], &x);
        assert_eq!(r[0].rows[0].title, "website");
        let r = run_with("@b rust", &apps, &u, &[], &x);
        assert_eq!(r[0].rows[0].title, "gpui - Rust");
    }

    #[test]
    fn money_and_time_answers() {
        let apps = apps();
        let x = extra();
        let r = run_with("100 usd to inr", &apps, &Usage::default(), &[], &x);
        assert_eq!(r[0].title, "Convert");
        assert_eq!(r[0].rows[0].title, "= 8,400.00 INR");
        assert_eq!(r[0].rows[0].target, Target::Copy("8400.00".into()));
        // 1_000_000 s after the epoch is 13:46 UTC.
        let r = run_with("time in utc", &apps, &Usage::default(), &[], &x);
        assert_eq!(r[0].title, "Time");
        assert_eq!(r[0].rows[0].title, "13:46");
    }

    fn titles(sections: &[Section]) -> Vec<(&'static str, Vec<String>)> {
        sections.iter().map(|s| (s.title, s.rows.iter().map(|r| r.title.clone()).collect())).collect()
    }

    #[test]
    fn modes() {
        assert_eq!(parse_mode("@win fire"), (Mode::Windows, "fire"));
        assert_eq!(parse_mode("@w"), (Mode::Windows, ""));
        assert_eq!(parse_mode("@wine"), (Mode::All, "@wine"));
        assert_eq!(parse_mode("=2+2"), (Mode::Calc, "2+2"));
        assert_eq!(parse_mode("$ ls -la"), (Mode::Shell, "ls -la"));
        assert_eq!(parse_mode("?rust gpui"), (Mode::Web, "rust gpui"));
        assert_eq!(parse_mode(":fire"), (Mode::Emoji, "fire"));
        assert_eq!(parse_mode("  firefox "), (Mode::All, "firefox"));

        assert_eq!(committed_prefix("@w fire"), Some(("@win", "fire")));
        assert_eq!(committed_prefix("@win"), None);
        assert_eq!(committed_prefix("@wine"), None);
        assert_eq!(committed_prefix("=2+2"), Some(("=", "2+2")));
        assert_eq!(committed_prefix(":smile"), Some(("@emoji", "smile")));
        assert_eq!(committed_prefix("firefox"), None);
    }

    #[test]
    fn top_hit_then_apps_then_web() {
        let apps = apps();
        let r = run("vsc", &apps, &Usage::default(), &[]);
        assert_eq!(r[0].title, "Top hit");
        assert_eq!(r[0].rows[0].title, "Visual Studio Code");
        assert!(r[0].rows[0].top);
        assert_eq!(r.last().unwrap().title, "Web");

        // Keywords and generic names find apps too.
        let r = run("browser", &apps, &Usage::default(), &[]);
        assert_eq!(r[0].rows[0].title, "Firefox");
        let r = run("photo", &apps, &Usage::default(), &[]);
        assert_eq!(r[0].rows[0].title, "GNU Image Manipulation Program");

        // But not by letters scattered through them: "fire" isn't in "File sharing".
        let mut apps = apps;
        apps.retain(|e| e.app.name != "Firefox");
        apps.push(app("xposed", "xposed", "File sharing", &["share", "link"]));
        let r = run("fire", &apps, &Usage::default(), &[]);
        assert_eq!(titles(&r), vec![("Web", vec!["Search the web for \u{201c}fire\u{201d}".to_string()])]);
    }

    #[test]
    fn use_breaks_ties() {
        let apps = apps();
        let mut usage = Usage::default();
        // Equal matches: the shorter name first.
        let first = run("f", &apps, &usage, &[]);
        for _ in 0..20 {
            usage.record("app:firefox.desktop", 1_000_000, 10);
        }
        let after = run("f", &apps, &usage, &[]);
        assert_eq!(first[0].rows[0].title, "Files");
        assert_eq!(after[0].rows[0].title, "Firefox");
        // A much better match still beats use.
        assert_eq!(run("files", &apps, &usage, &[])[0].rows[0].title, "Files");
        usage.hide("app:firefox.desktop");
        assert_eq!(run("f", &apps, &usage, &[])[0].rows[0].title, "Files");
    }

    #[test]
    fn commands_and_math() {
        let apps = apps();
        let r = run("lock", &apps, &Usage::default(), &[]);
        assert_eq!(r[0].rows[0].title, "Lock Screen");
        let r = run("12*(3+4)", &apps, &Usage::default(), &[]);
        assert_eq!(titles(&r)[0], ("Math", vec!["= 84".to_string()]));
        let r = run("10 kg to lb", &apps, &Usage::default(), &[]);
        assert_eq!(r[0].title, "Convert");
    }

    #[test]
    fn empty_state() {
        let apps = apps();
        let mut usage = Usage::default();
        assert!(run("", &apps, &usage, &[]).is_empty());
        usage.toggle_pin("app:ghostty.desktop");
        for _ in 0..3 {
            usage.record("app:firefox.desktop", 999_000, 10);
        }
        usage.record("app:ghostty.desktop", 999_000, 10);
        let r = titles(&run("", &apps, &usage, &[]));
        assert_eq!(r[0], ("Pinned", vec!["Ghostty".to_string()]));
        assert_eq!(r[1], ("Suggested for this hour", vec!["Firefox".to_string()]));
    }

    #[test]
    fn windows_join_when_they_match() {
        let apps = apps();
        let wins = vec![Win {
            address: "0x1".into(),
            class: "firefox".into(),
            title: "GPUI docs — Mozilla Firefox".into(),
            workspace: "2".into(),
            recency: 1,
        }];
        let r = run("gpui", &apps, &Usage::default(), &wins);
        let w = r.iter().find(|s| s.title == "Open windows").unwrap();
        assert_eq!(w.rows[0].badge.as_deref(), Some("2"));
        assert_eq!(w.rows[0].sub, "Firefox");
        let r = run("@win", &apps, &Usage::default(), &wins);
        assert_eq!(r[0].rows.len(), 1);
    }
}
