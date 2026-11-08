//! The resident engine: the data a search reads, kept fresh in the background, and what
//! happens when a row is chosen. A UI owns one [`Engine`], calls [`Engine::search`] on every
//! keystroke, and searches again whenever [`Engine::generation`] moves (the notify callback
//! says when).

use std::{
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, MutexGuard,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use yozora_config::launch::LaunchSettings;

use crate::{
    browser::{self, Page},
    glyph::Glyph,
    matcher::Fuzzy,
    money::{self, Rates},
    projects::{self, Project},
    scripts::{self, Script},
    search::{self, Entry, Row, Section, Sources, Target, Visual},
    sources::{self, Clip, Cmd, Emoji, FileHit, Win},
    spawn,
    ssh::{self, Host},
    usage::{self, Usage},
};

/// What can be done to a row. [`Engine::actions`] lists the ones a row offers and
/// [`Engine::default_act`] picks the one a key does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Act {
    Primary,
    /// Run the app's n-th desktop action.
    AppAction(usize),
    Pin,
    Hide,
    CloseWindow,
    DeleteClip,
    OpenFolder,
    CopyPath,
    /// Run a shell command without a terminal.
    Background,
    OpenTerminal,
    /// Runs a script that asks for confirmation.
    Confirmed,
}

/// The modifier held with Enter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Modifier {
    None,
    Shift,
    Alt,
}

/// An entry of a row's action menu.
#[derive(Clone, Debug, PartialEq)]
pub struct ActionItem {
    pub label: String,
    /// The key that does it directly, for display.
    pub hint: &'static str,
    pub act: Act,
}

/// What [`Engine::activate`] did.
#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    /// It ran and was recorded for ranking: the launcher should close.
    Launched,
    /// Something changed (a pin, a hidden app, a deleted clip): stay open and search again.
    Updated,
    /// The script at this index wants arguments: ask for them, then activate again with the
    /// typed text.
    NeedArgs(usize),
    /// Ask before running: activate again with the item's act to confirm.
    Confirm(ActionItem),
    /// Nothing applies to this row.
    Nothing,
}

#[derive(Default)]
struct Fast {
    windows: Vec<Win>,
    dnd: Option<bool>,
    recent: Vec<FileHit>,
    scripts: Vec<Script>,
    hosts: Vec<Host>,
}

struct State {
    settings: LaunchSettings,
    apps: Arc<Vec<Entry>>,
    usage: Usage,
    windows: Vec<Win>,
    commands: Vec<Cmd>,
    dnd: Option<bool>,
    clips: Vec<Clip>,
    emoji: Option<Vec<Emoji>>,
    recent: Vec<FileHit>,
    /// Locate results, and the query they're for.
    located: (String, Vec<FileHit>),
    fuzzy: Fuzzy,
    home: PathBuf,
    /// Bumped by each locate request; only the latest one's result is kept.
    locate_seq: u64,
    clips_fetching: bool,
    /// Whether the app index has been built once.
    apps_ready: bool,
    scripts: Vec<Script>,
    hosts: Vec<Host>,
    projects: Arc<Vec<Project>>,
    pages: Arc<Vec<Page>>,
    rates: Option<Rates>,
    /// When projects and browser pages were last read; they're slow, so not on every open.
    slow_at: Option<Instant>,
}

struct Inner {
    state: Mutex<State>,
    generation: AtomicU64,
    notify: Box<dyn Fn() + Send + Sync>,
}

impl Inner {
    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn changed(&self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
        (self.notify)();
    }

    fn save(&self, state: &State) {
        let usage = state.usage.clone();
        background("save", move || {
            let mut usage = usage;
            usage.prune(usage::now().0);
            if let Err(e) = usage.save(&usage::state_file()) {
                tracing::warn!("saving launch history: {e:#}");
            }
        });
    }
}

fn background(name: &str, f: impl FnOnce() + Send + 'static) {
    if let Err(e) = std::thread::Builder::new().name(format!("yz-search-{name}")).spawn(f) {
        tracing::warn!("could not start a {name} thread: {e}");
    }
}

/// A cheap handle to the engine; clones share it.
#[derive(Clone)]
pub struct Engine {
    inner: Arc<Inner>,
}

impl Engine {
    /// `notify` is called, from any thread, whenever data a search reads has changed.
    pub fn new(settings: LaunchSettings, notify: impl Fn() + Send + Sync + 'static) -> Self {
        let commands = sources::commands(&settings.launch, None);
        let state = State {
            settings,
            apps: Arc::new(Vec::new()),
            usage: Usage::load(&usage::state_file()),
            windows: Vec::new(),
            commands,
            dnd: None,
            clips: Vec::new(),
            emoji: None,
            recent: Vec::new(),
            located: (String::new(), Vec::new()),
            fuzzy: Fuzzy::new(),
            home: std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| "/".into()),
            locate_seq: 0,
            clips_fetching: false,
            apps_ready: false,
            scripts: Vec::new(),
            hosts: Vec::new(),
            projects: Arc::new(Vec::new()),
            pages: Arc::new(Vec::new()),
            rates: None,
            slow_at: None,
        };
        Engine { inner: Arc::new(Inner { state: Mutex::new(state), generation: AtomicU64::new(0), notify: Box::new(notify) }) }
    }

    /// Builds the app index and keeps it current as desktop files change, and loads windows,
    /// scripts, hosts, projects and pages, so the first open has them.
    pub fn start(&self) {
        let weak = Arc::downgrade(&self.inner);
        background("apps", move || watch_apps(weak));
        self.refresh();
    }

    /// Bumped whenever data a search reads changes; search again when it moves.
    pub fn generation(&self) -> u64 {
        self.inner.generation.load(Ordering::SeqCst)
    }

    pub fn settings(&self) -> LaunchSettings {
        self.inner.state().settings.clone()
    }

    /// Replaces the settings; re-reads the slow sources if what they depend on changed.
    pub fn set_settings(&self, settings: LaunchSettings) {
        let slow = {
            let mut st = self.inner.state();
            st.commands = sources::commands(&settings.launch, st.dnd);
            let old = &st.settings.launch;
            let slow = (&old.project_roots, old.browser, old.currency)
                != (&settings.launch.project_roots, settings.launch.browser, settings.launch.currency);
            st.settings = settings;
            slow
        };
        if slow {
            self.load_slow();
        }
        self.inner.changed();
    }

    /// Swaps in a fresh app index.
    pub fn set_apps(&self, apps: Vec<Entry>) {
        {
            let mut st = self.inner.state();
            st.apps = Arc::new(apps);
            st.apps_ready = true;
        }
        self.inner.changed();
    }

    pub fn apps(&self) -> Arc<Vec<Entry>> {
        self.inner.state().apps.clone()
    }

    /// Runs a search over what's in memory, and starts fetching what isn't yet (the clipboard,
    /// locate results), which calls notify when it lands. Empty sections are left out.
    pub fn search(&self, query: &str) -> Vec<Section> {
        let started = Instant::now();
        let mut guard = self.inner.state();
        let st = &mut *guard;
        let (mode, q) = search::parse_mode(query);
        if mode == search::Mode::Emoji && st.emoji.is_none() {
            st.emoji = Some(sources::emoji());
        }
        if mode == search::Mode::Clipboard && st.clips.is_empty() && !st.clips_fetching {
            st.clips_fetching = true;
            let inner = self.inner.clone();
            background("clips", move || {
                let clips = sources::clipboard();
                {
                    let mut st = inner.state();
                    st.clips = clips;
                    st.clips_fetching = false;
                }
                inner.changed();
            });
        }
        let wants_files = mode == search::Mode::Files
            || (mode == search::Mode::All && st.settings.launch.files_in_search && q.chars().count() >= 3);
        if wants_files && !q.is_empty() && st.located.0 != q {
            self.locate(st, q.to_string());
        }

        let launch = &st.settings.launch;
        let empty: Vec<Emoji> = Vec::new();
        let located: &[FileHit] = if st.located.0 == q { &st.located.1 } else { &[] };
        let web_name = web_name(&launch.web_search);
        let s = Sources {
            apps: &st.apps,
            windows: &st.windows,
            commands: &st.commands,
            clips: &st.clips,
            emoji: st.emoji.as_deref().unwrap_or(&empty),
            recent: &st.recent,
            located,
            usage: &st.usage,
            now: usage::now(),
            max: launch.max_results as usize,
            suggestions: launch.suggestions,
            files_in_search: launch.files_in_search,
            home: &st.home,
            web_name: &web_name,
            scripts: &st.scripts,
            hosts: &st.hosts,
            projects: &st.projects,
            pages: if launch.browser { &st.pages } else { &[] },
            rates: if launch.currency { st.rates.as_ref() } else { None },
        };
        let mut out = search::search(query, &mut st.fuzzy, &s);
        out.retain(|s| !s.rows.is_empty());
        tracing::trace!("searched {query:?} in {:?}", started.elapsed());
        out
    }

    /// What a search for `query` is still waiting on, for a UI to say so: the app index on a cold
    /// start, the clipboard history, or the file locator. `None` once everything it reads is in.
    pub fn loading(&self, query: &str) -> Option<&'static str> {
        let st = self.inner.state();
        let (mode, q) = search::parse_mode(query);
        if !st.apps_ready && matches!(mode, search::Mode::All) && !q.is_empty() {
            return Some("Indexing apps");
        }
        if mode == search::Mode::Clipboard && st.clips_fetching {
            return Some("Reading the clipboard");
        }
        let wants_files = mode == search::Mode::Files
            || (mode == search::Mode::All && st.settings.launch.files_in_search && q.chars().count() >= 3);
        if wants_files && !q.is_empty() && st.located.0 != q {
            return Some("Searching files");
        }
        None
    }

    /// Runs locate for `q` after a short pause, so typing quickly runs it once.
    fn locate(&self, st: &mut State, q: String) {
        st.locate_seq += 1;
        let seq = st.locate_seq;
        let home = st.home.clone();
        let inner = self.inner.clone();
        background("locate", move || {
            std::thread::sleep(Duration::from_millis(90));
            if inner.state().locate_seq != seq {
                return;
            }
            let hits = sources::locate(&q, &home, 60);
            {
                let mut st = inner.state();
                if st.locate_seq != seq {
                    return;
                }
                st.located = (q, hits);
            }
            inner.changed();
        });
    }

    /// Refreshes the things that change between opens: windows, Do Not Disturb, recent files,
    /// scripts and SSH hosts, and (when they're ten minutes old) projects, pages and rates.
    /// The clipboard is fetched again when it's asked for. Returns at once.
    pub fn refresh(&self) {
        let (dirs, slow) = self.prepare_refresh();
        let inner = self.inner.clone();
        background("refresh", move || apply_fast(&inner, fetch_fast(&dirs)));
        if slow {
            self.load_slow();
        }
    }

    /// [`Engine::refresh`], waiting for everything it reads.
    pub fn refresh_blocking(&self) {
        let (dirs, slow) = self.prepare_refresh();
        apply_fast(&self.inner, fetch_fast(&dirs));
        if slow {
            self.load_slow_blocking();
        }
    }

    fn prepare_refresh(&self) -> (Vec<String>, bool) {
        let mut st = self.inner.state();
        st.clips.clear();
        st.located = (String::new(), Vec::new());
        let slow = st.slow_at.is_none_or(|t| t.elapsed() > Duration::from_secs(600));
        (st.settings.launch.script_dirs.clone(), slow)
    }

    /// Reads projects, browser pages and currency rates in the background. At startup, and
    /// again on refreshing once they're ten minutes old.
    pub fn load_slow(&self) {
        let launch = self.begin_slow();
        let inner = self.inner.clone();
        background("slow", move || finish_slow(&inner, launch));
    }

    pub fn load_slow_blocking(&self) {
        let launch = self.begin_slow();
        finish_slow(&self.inner, launch);
    }

    fn begin_slow(&self) -> yozora_config::launch::LaunchConfig {
        let mut st = self.inner.state();
        st.slow_at = Some(Instant::now());
        let launch = st.settings.launch.clone();
        if launch.currency && st.rates.is_none() {
            st.rates = money::cached();
        }
        launch
    }

    /// The icons of pinned and often used apps, so a UI can decode them before anyone asks.
    pub fn warm_icon_paths(&self) -> Vec<PathBuf> {
        let st = self.inner.state();
        let (now, hour) = usage::now();
        let mut keys: Vec<String> = st.usage.pins.clone();
        keys.extend(st.usage.top(now, hour, 24).into_iter().map(str::to_string));
        st.apps
            .iter()
            .filter(|e| keys.contains(&e.key()))
            .filter_map(|e| e.icon.as_ref().map(|p| p.as_ref().clone()))
            .collect()
    }

    /// The row a script shows while its arguments are typed.
    pub fn args_row(&self, script: usize) -> Option<Row> {
        self.inner.state().scripts.get(script).map(|sc| search::args_row(script, sc))
    }

    /// What a script asks for, as the field's placeholder: "name, count (optional)".
    pub fn args_hint(&self, script: usize) -> Option<String> {
        self.inner.state().scripts.get(script).map(|sc| {
            sc.args
                .iter()
                .map(|a| if a.optional { format!("{} (optional)", a.placeholder) } else { a.placeholder.clone() })
                .collect::<Vec<_>>()
                .join(", ")
        })
    }

    /// The entries of `row`'s action menu, the first being what Enter does.
    pub fn actions(&self, row: &Row) -> Vec<ActionItem> {
        let item = |label: &str, hint: &'static str, act| ActionItem { label: label.to_string(), hint, act };
        let mut items = Vec::new();
        match &row.target {
            Target::App { index, .. } => {
                items.push(item("Open", "↵", Act::Primary));
                if let Some(e) = self.inner.state().apps.get(*index) {
                    for (i, a) in e.app.actions.iter().enumerate() {
                        items.push(item(&a.name, if i == 0 { "Shift ↵" } else { "" }, Act::AppAction(i)));
                    }
                }
                items.push(item(if row.pinned { "Unpin" } else { "Pin to the top" }, "Ctrl P", Act::Pin));
                items.push(item("Hide from results", "", Act::Hide));
            }
            Target::Command(_) => items.push(item("Run", "↵", Act::Primary)),
            Target::Window(_) => {
                items.push(item("Focus", "↵", Act::Primary));
                items.push(item("Close the window", "", Act::CloseWindow));
            }
            Target::Clip(_) => {
                items.push(item("Copy", "↵", Act::Primary));
                items.push(item("Delete from history", "", Act::DeleteClip));
            }
            Target::File(_) => {
                items.push(item("Open", "↵", Act::Primary));
                items.push(item("Open the folder", "Alt ↵", Act::OpenFolder));
                items.push(item("Copy the path", "Shift ↵", Act::CopyPath));
            }
            Target::Shell(_) => {
                items.push(item("Run in a terminal", "↵", Act::Primary));
                items.push(item("Run without a terminal", "Shift ↵", Act::Background));
            }
            Target::Copy(_) => items.push(item("Copy", "↵", Act::Primary)),
            Target::Web(_) => items.push(item("Search", "↵", Act::Primary)),
            Target::Script(_) => {
                items.push(item("Run", "↵", Act::Primary));
                items.push(item("Open the script's folder", "", Act::OpenFolder));
            }
            Target::Ssh(_) => {
                items.push(item("Connect in a terminal", "↵", Act::Primary));
                items.push(item("Copy the ssh command", "Shift ↵", Act::CopyPath));
            }
            Target::Project(_) => {
                items.push(item("Open in the editor", "↵", Act::Primary));
                items.push(item("Open a terminal here", "Shift ↵", Act::OpenTerminal));
                items.push(item("Open the folder", "Alt ↵", Act::OpenFolder));
                items.push(item("Copy the path", "", Act::CopyPath));
            }
            Target::Url(_) => {
                items.push(item("Open", "↵", Act::Primary));
                items.push(item("Copy the link", "Shift ↵", Act::CopyPath));
            }
        }
        items
    }

    /// What Enter does to `row` with `modifier` held.
    pub fn default_act(&self, row: &Row, modifier: Modifier) -> Act {
        match (modifier, &row.target) {
            (Modifier::Shift, Target::App { index, .. }) => {
                let has = self.inner.state().apps.get(*index).is_some_and(|e| !e.app.actions.is_empty());
                if has { Act::AppAction(0) } else { Act::Primary }
            }
            (Modifier::Shift, Target::Shell(_)) => Act::Background,
            (Modifier::Shift, Target::File(_)) => Act::CopyPath,
            (Modifier::Alt, Target::File(_)) => Act::OpenFolder,
            (Modifier::Shift, Target::Project(_)) => Act::OpenTerminal,
            (Modifier::Alt, Target::Project(_)) => Act::OpenFolder,
            (Modifier::Shift, Target::Url(_) | Target::Ssh(_)) => Act::CopyPath,
            _ => Act::Primary,
        }
    }

    /// Does `act` to `row`. `typed` is the text of the argument field when the UI is asking a
    /// script for its arguments, and `None` otherwise. Slow work (focusing, copying) runs on
    /// its own thread. An error is meant to be shown to the user.
    pub fn activate(&self, row: &Row, act: Act, typed: Option<&str>) -> anyhow::Result<Outcome> {
        let (terminal, editor, web_search) = {
            let st = self.inner.state();
            let l = &st.settings.launch;
            (l.terminal.clone(), l.editor.clone(), l.web_search.clone())
        };
        let copy = |text: String| {
            background("copy", move || {
                if let Err(e) = sources::copy_text(&text) {
                    tracing::warn!("copying: {e:#}");
                }
            })
        };
        match (&act, &row.target) {
            (Act::Pin, _) => {
                if let Some(key) = &row.key {
                    let mut st = self.inner.state();
                    st.usage.toggle_pin(key);
                    self.inner.save(&st);
                    drop(st);
                    self.inner.changed();
                }
                return Ok(Outcome::Updated);
            }
            (Act::Hide, _) => {
                if let Some(key) = &row.key {
                    let mut st = self.inner.state();
                    st.usage.hide(key);
                    self.inner.save(&st);
                    drop(st);
                    self.inner.changed();
                }
                return Ok(Outcome::Updated);
            }
            (Act::Primary | Act::AppAction(_), Target::App { index, action }) => {
                let apps = self.apps();
                let entry = apps.get(*index).ok_or_else(|| anyhow::anyhow!("that app is gone"))?;
                let action = if let Act::AppAction(a) = act { Some(a) } else { *action };
                spawn::app(&entry.app, action, &terminal)?;
            }
            (Act::Primary, Target::Window(address)) => {
                let address = address.clone();
                background("focus", move || {
                    if let Err(e) = sources::focus(&address) {
                        tracing::warn!("focusing {address}: {e:#}");
                    }
                });
            }
            (Act::CloseWindow, Target::Window(address)) => {
                let address = address.clone();
                background("close", move || {
                    if let Err(e) = sources::close_window(&address) {
                        tracing::warn!("closing {address}: {e:#}");
                    }
                });
            }
            (Act::Primary, Target::Command(i)) => {
                let run = {
                    let st = self.inner.state();
                    st.commands.get(*i).ok_or_else(|| anyhow::anyhow!("no such command"))?.run.clone()
                };
                spawn::shell(&run)?;
            }
            (Act::Primary, Target::Copy(text)) => copy(text.clone()),
            (Act::Primary, Target::Clip(line)) => {
                let line = line.clone();
                background("clip", move || {
                    if let Err(e) = sources::clip_copy(&line) {
                        tracing::warn!("copying from the clipboard history: {e:#}");
                    }
                });
            }
            (Act::DeleteClip, Target::Clip(line)) => {
                sources::clip_delete(line)?;
                self.inner.state().clips.retain(|x| x.line != *line);
                self.inner.changed();
                return Ok(Outcome::Updated);
            }
            (Act::Primary, Target::File(path)) => spawn::open(&path.to_string_lossy())?,
            (Act::OpenFolder, Target::File(path)) => {
                let dir = if path.is_dir() { path.as_path() } else { path.parent().unwrap_or(path) };
                spawn::open(&dir.to_string_lossy())?;
            }
            (Act::CopyPath, Target::File(path)) => copy(path.to_string_lossy().into_owned()),
            (Act::Primary, Target::Shell(cmd)) => spawn::shell_in_terminal(cmd, &terminal)?,
            (Act::Background, Target::Shell(cmd)) => spawn::shell(cmd)?,
            (Act::Primary | Act::Confirmed, Target::Script(i)) => {
                let sc = self
                    .inner
                    .state()
                    .scripts
                    .get(*i)
                    .cloned()
                    .ok_or_else(|| anyhow::anyhow!("that script is gone"))?;
                if !sc.args.is_empty() && typed.is_none() {
                    return Ok(Outcome::NeedArgs(*i));
                }
                if sc.confirm && act != Act::Confirmed {
                    return Ok(Outcome::Confirm(ActionItem {
                        label: format!("Run {}", sc.title),
                        hint: "↵",
                        act: Act::Confirmed,
                    }));
                }
                let args = scripts::split_args(typed.unwrap_or_default(), sc.args.len());
                if let Some(a) = sc.args.iter().zip(&args).find(|(a, v)| !a.optional && v.is_empty()).map(|(a, _)| a) {
                    anyhow::bail!("{} needs a value", a.placeholder);
                }
                scripts::run(&sc, &args, &terminal)?;
            }
            (Act::OpenFolder, Target::Script(i)) => {
                let path = self.inner.state().scripts.get(*i).map(|s| s.path.clone()).unwrap_or_default();
                spawn::open(&path.parent().unwrap_or(&path).to_string_lossy())?;
            }
            (Act::Primary, Target::Ssh(alias)) => {
                spawn::run("ssh", &spawn::in_terminal(&terminal, vec!["ssh".into(), alias.clone()]), None)?;
            }
            (Act::CopyPath, Target::Ssh(alias)) => copy(format!("ssh {alias}")),
            (Act::Primary, Target::Project(path)) => projects::open(path, &editor, &terminal)?,
            (Act::OpenTerminal, Target::Project(path)) => projects::terminal_in(path, &terminal)?,
            (Act::OpenFolder, Target::Project(path)) => spawn::open(&path.to_string_lossy())?,
            (Act::CopyPath, Target::Project(path)) => copy(path.to_string_lossy().into_owned()),
            (Act::Primary, Target::Url(url)) => spawn::open(url)?,
            (Act::CopyPath, Target::Url(url)) => copy(url.clone()),
            (Act::Primary, Target::Web(q)) => spawn::open(&web_search.replace("{}", &encode(q)))?,
            _ => return Ok(Outcome::Nothing),
        }
        if let Some(key) = &row.key {
            let (now, hour) = usage::now();
            let mut st = self.inner.state();
            st.usage.record(key, now, hour);
            self.inner.save(&st);
        }
        Ok(Outcome::Launched)
    }
}

fn fetch_fast(dirs: &[String]) -> Fast {
    Fast {
        windows: sources::windows(),
        dnd: sources::dnd(),
        recent: sources::recent_files(),
        scripts: scripts::scan(dirs),
        hosts: ssh::hosts(),
    }
}

fn apply_fast(inner: &Inner, fast: Fast) {
    {
        let mut st = inner.state();
        st.windows = fast.windows;
        st.dnd = fast.dnd;
        st.recent = fast.recent;
        st.scripts = fast.scripts;
        st.hosts = fast.hosts;
        st.commands = sources::commands(&st.settings.launch, fast.dnd);
    }
    inner.changed();
}

fn finish_slow(inner: &Inner, launch: yozora_config::launch::LaunchConfig) {
    let projects = projects::scan(&launch.project_roots);
    let pages = if launch.browser { browser::pages() } else { Vec::new() };
    let rates = if launch.currency { money::refresh() } else { None };
    {
        let mut st = inner.state();
        st.projects = Arc::new(projects);
        st.pages = Arc::new(pages);
        if rates.is_some() {
            st.rates = rates;
        }
    }
    inner.changed();
}

/// Watches the applications directories, and swaps in a fresh index whenever they change.
/// Ends when the engine is gone.
fn watch_apps(engine: std::sync::Weak<Inner>) {
    use inotify::{Inotify, WatchMask};
    let publish = |engine: &std::sync::Weak<Inner>| -> bool {
        let Some(inner) = engine.upgrade() else { return false };
        let apps = index();
        {
            let mut st = inner.state();
            st.apps = Arc::new(apps);
            st.apps_ready = true;
        }
        inner.changed();
        true
    };
    if !publish(&engine) {
        return;
    }
    let Ok(mut inotify) = Inotify::init() else { return };
    let mask = WatchMask::CREATE | WatchMask::DELETE | WatchMask::MODIFY | WatchMask::MOVED_TO | WatchMask::MOVED_FROM;
    let mut watched = 0;
    for dir in crate::desktop::dirs() {
        if inotify.watches().add(&dir, mask).is_ok() {
            watched += 1;
        }
    }
    tracing::debug!("watching {watched} application directories");
    let mut buf = [0u8; 4096];
    loop {
        if inotify.read_events_blocking(&mut buf).is_err() {
            return;
        }
        // Package managers write many files in a row: wait for a quiet moment.
        std::thread::sleep(Duration::from_millis(400));
        while inotify.read_events(&mut buf).is_ok_and(|mut e| e.next().is_some()) {}
        if !publish(&engine) {
            return;
        }
    }
}

/// Icons are drawn at up to 48 px; rasterized at twice that for 2x monitors.
const ICON_PX: u32 = 96;

/// The app index: desktop entries with their icons resolved. Slow (it reads every desktop
/// file and may rasterize icons), so call it off the UI thread. [`Engine::start`] does this
/// itself and keeps it current.
pub fn index() -> Vec<Entry> {
    let started = Instant::now();
    let apps = crate::desktop::scan(&crate::desktop::dirs());
    let entries: Vec<Entry> = apps
        .into_iter()
        .map(|app| {
            let icon = yozora_services::icons::find(&app.icon, &[]).map(|p| yozora_services::icons::fit(&p, ICON_PX));
            Entry::new(app, icon)
        })
        .collect();
    tracing::info!("indexed {} apps in {:?}", entries.len(), started.elapsed());
    entries
}

/// Loads the launcher's settings from `dir`, falling back to the defaults and logging why.
pub fn load_settings(dir: &Path) -> LaunchSettings {
    match yozora_config::launch::load(dir) {
        Ok(loaded) => {
            for w in &loaded.warnings {
                tracing::warn!("config: {w}");
            }
            loaded.value
        }
        Err(e) => {
            tracing::error!("configuration: {e:#}; using defaults");
            LaunchSettings::default()
        }
    }
}

/// "DuckDuckGo" for its URL, or the host.
fn web_name(url: &str) -> String {
    let host = url.split("://").nth(1).unwrap_or(url).split('/').next().unwrap_or_default();
    let host = host.trim_start_matches("www.");
    match host {
        "duckduckgo.com" => "DuckDuckGo".into(),
        h if h.starts_with("google.") => "Google".into(),
        "search.brave.com" => "Brave Search".into(),
        "kagi.com" => "Kagi".into(),
        "bing.com" => "Bing".into(),
        "startpage.com" => "Startpage".into(),
        h => h.to_string(),
    }
}

/// The row Ctrl+Enter acts on: search the web for `q`.
pub fn web_row(q: &str) -> Row {
    Row {
        title: String::new(),
        hl: Vec::new(),
        sub: String::new(),
        visual: Visual::Glyph(Glyph::Globe),
        badge: None,
        pinned: false,
        top: false,
        target: Target::Web(q.to_string()),
        key: None,
    }
}

/// Percent-encodes a query for a URL.
pub fn encode(q: &str) -> String {
    let mut out = String::with_capacity(q.len() * 3);
    for b in q.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_engine_names() {
        assert_eq!(web_name("https://duckduckgo.com/?q={}"), "DuckDuckGo");
        assert_eq!(web_name("https://www.google.com/search?q={}"), "Google");
        assert_eq!(web_name("https://search.example.org/s?q={}"), "search.example.org");
    }
}
