//! The resident launcher: its data, and one window per monitor that stays mapped while closed.
//!
//! Opening a GPUI window costs about 20 ms, three frames at 144 Hz, so the windows are opened
//! once and kept. Closed, a window sits on the background layer, fully transparent, with no
//! input region and no keyboard. Opening moves it to the overlay layer, takes the keyboard and
//! draws: one commit, one frame.

use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

use gpui::{
    App, AppContext, Bounds, Context, Entity, Global, Task, WindowBackgroundAppearance, WindowBounds, WindowHandle,
    WindowKind, WindowOptions, layer_shell::*, point, px, size,
};
use yozora_config::launch::LaunchSettings;
use yozora_ui::hypr;

use crate::{
    browser::{self, Page},
    matcher::Fuzzy,
    money::{self, Rates},
    projects::{self, Project},
    scripts::{self, Script},
    search::{self, Entry, Section, Sources},
    sources::{self, Clip, Cmd, Emoji, FileHit, Win},
    ssh::{self, Host},
    usage::{self, Usage},
    view::LaunchView,
};

/// Everything the views read. One per process.
pub struct Core {
    pub settings: LaunchSettings,
    pub apps: Arc<Vec<Entry>>,
    pub usage: Usage,
    pub windows: Vec<Win>,
    pub commands: Vec<Cmd>,
    pub dnd: Option<bool>,
    pub clips: Vec<Clip>,
    pub emoji: Option<Vec<Emoji>>,
    pub recent: Vec<FileHit>,
    /// Locate results, and the query they're for.
    pub located: (String, Vec<FileHit>),
    pub fuzzy: Fuzzy,
    pub home: PathBuf,
    locating: Option<Task<()>>,
    clips_task: Option<Task<()>>,
    pub scripts: Vec<Script>,
    pub hosts: Vec<Host>,
    pub projects: Arc<Vec<Project>>,
    pub pages: Arc<Vec<Page>>,
    pub rates: Option<Rates>,
    /// When projects and browser pages were last read; they're slow, so not on every open.
    slow_at: Option<Instant>,
    /// Bumped whenever data a search reads changes, so an open view searches again.
    pub generation: u64,
}

impl Core {
    pub fn new(settings: LaunchSettings) -> Self {
        let commands = sources::commands(&settings.launch, None);
        Self {
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
            locating: None,
            clips_task: None,
            scripts: Vec::new(),
            hosts: Vec::new(),
            projects: Arc::new(Vec::new()),
            pages: Arc::new(Vec::new()),
            rates: None,
            slow_at: None,
            generation: 0,
        }
    }

    /// Runs a search over what's in memory, and starts fetching what isn't yet.
    pub fn search(&mut self, query: &str, cx: &mut Context<Self>) -> Vec<Section> {
        let started = Instant::now();
        let (mode, q) = search::parse_mode(query);
        if mode == search::Mode::Emoji && self.emoji.is_none() {
            self.emoji = Some(sources::emoji());
        }
        if mode == search::Mode::Clipboard && self.clips.is_empty() && self.clips_task.is_none() {
            self.fetch_clips(cx);
        }
        let wants_files = mode == search::Mode::Files
            || (mode == search::Mode::All && self.settings.launch.files_in_search && q.chars().count() >= 3);
        if wants_files && !q.is_empty() && self.located.0 != q {
            self.locate(q.to_string(), cx);
        }

        let launch = &self.settings.launch;
        let empty: Vec<Emoji> = Vec::new();
        let located: &[FileHit] = if self.located.0 == q { &self.located.1 } else { &[] };
        let web_name = web_name(&launch.web_search);
        let s = Sources {
            apps: &self.apps,
            windows: &self.windows,
            commands: &self.commands,
            clips: &self.clips,
            emoji: self.emoji.as_deref().unwrap_or(&empty),
            recent: &self.recent,
            located,
            usage: &self.usage,
            now: usage::now(),
            max: launch.max_results as usize,
            suggestions: launch.suggestions,
            files_in_search: launch.files_in_search,
            home: &self.home,
            web_name: &web_name,
            scripts: &self.scripts,
            hosts: &self.hosts,
            projects: &self.projects,
            pages: if launch.browser { &self.pages } else { &[] },
            rates: if launch.currency { self.rates.as_ref() } else { None },
        };
        let out = search::search(query, &mut self.fuzzy, &s);
        tracing::trace!("searched {query:?} in {:?}", started.elapsed());
        out
    }

    /// Runs locate for `q` after a short pause, so typing quickly runs it once.
    fn locate(&mut self, q: String, cx: &mut Context<Self>) {
        let home = self.home.clone();
        self.locating = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_millis(90)).await;
            let query = q.clone();
            let hits = cx.background_spawn(async move { sources::locate(&query, &home, 60) }).await;
            let _ = this.update(cx, |core, cx| {
                core.located = (q, hits);
                core.locating = None;
                core.changed(cx);
            });
        }));
    }

    fn fetch_clips(&mut self, cx: &mut Context<Self>) {
        self.clips_task = Some(cx.spawn(async move |this, cx| {
            let clips = cx.background_spawn(async { sources::clipboard() }).await;
            let _ = this.update(cx, |core, cx| {
                core.clips = clips;
                core.clips_task = None;
                core.changed(cx);
            });
        }));
    }

    /// Refreshes the things that change between opens: windows, Do Not Disturb, recent files.
    /// The clipboard is fetched again when it's asked for.
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        self.clips.clear();
        self.located = (String::new(), Vec::new());
        let dirs = self.settings.launch.script_dirs.clone();
        cx.spawn(async move |this, cx| {
            let (windows, dnd, recent, scripts, hosts) = cx
                .background_spawn(async move {
                    (sources::windows(), sources::dnd(), sources::recent_files(), scripts::scan(&dirs), ssh::hosts())
                })
                .await;
            let _ = this.update(cx, |core, cx| {
                core.windows = windows;
                core.dnd = dnd;
                core.recent = recent;
                core.scripts = scripts;
                core.hosts = hosts;
                core.commands = sources::commands(&core.settings.launch, dnd);
                core.changed(cx);
            });
        })
        .detach();
        if self.slow_at.is_none_or(|t| t.elapsed() > Duration::from_secs(600)) {
            self.load_slow(cx);
        }
    }

    /// Reads projects, browser pages and currency rates in the background. At startup, and
    /// again on opening once they're ten minutes old.
    pub fn load_slow(&mut self, cx: &mut Context<Self>) {
        self.slow_at = Some(Instant::now());
        let launch = self.settings.launch.clone();
        if launch.currency && self.rates.is_none() {
            self.rates = money::cached();
        }
        cx.spawn(async move |this, cx| {
            let (projects, pages, rates) = cx
                .background_spawn(async move {
                    let projects = projects::scan(&launch.project_roots);
                    let pages = if launch.browser { browser::pages() } else { Vec::new() };
                    let rates = if launch.currency { money::refresh() } else { None };
                    (projects, pages, rates)
                })
                .await;
            let _ = this.update(cx, |core, cx| {
                core.projects = Arc::new(projects);
                core.pages = Arc::new(pages);
                if rates.is_some() {
                    core.rates = rates;
                }
                core.changed(cx);
            });
        })
        .detach();
    }

    pub fn changed(&mut self, cx: &mut Context<Self>) {
        self.generation += 1;
        cx.notify();
    }

    /// Records a launch and saves, off the UI thread.
    pub fn record(&mut self, key: &str, cx: &mut Context<Self>) {
        let (now, hour) = usage::now();
        self.usage.record(key, now, hour);
        self.save(cx);
    }

    pub fn save(&mut self, cx: &mut Context<Self>) {
        let mut usage = self.usage.clone();
        cx.background_spawn(async move {
            usage.prune(usage::now().0);
            if let Err(e) = usage.save(&usage::state_file()) {
                tracing::warn!("saving launch history: {e:#}");
            }
        })
        .detach();
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

/// The app index: desktop entries with their icons resolved. Slow (it reads every desktop
/// file and may rasterize icons), so it runs on a background thread.
pub fn index() -> Vec<Entry> {
    let started = Instant::now();
    let apps = crate::desktop::scan(&crate::desktop::dirs());
    let entries: Vec<Entry> = apps
        .into_iter()
        .map(|app| {
            let icon = yozora_ui::icon_theme::find(&app.icon, &[]).map(|p| yozora_ui::icon_theme::fit(&p, ICON_PX));
            Entry::new(app, icon)
        })
        .collect();
    tracing::info!("indexed {} apps in {:?}", entries.len(), started.elapsed());
    entries
}

/// Icons are drawn at up to 48 px; rasterized at twice that for 2x monitors.
const ICON_PX: u32 = 96;

/// The per-monitor windows.
pub struct Launcher {
    pub core: Entity<Core>,
    windows: Vec<(String, WindowHandle<LaunchView>)>,
}

impl Global for Launcher {}

impl Launcher {
    pub fn init(core: Entity<Core>, cx: &mut App) {
        cx.set_global(Launcher { core, windows: Vec::new() });
        // Open every monitor's window now, closed, so the first open is as quick as the rest.
        if let Ok(monitors) = hypr::monitors() {
            for m in monitors {
                Self::window_for(&m, cx);
            }
        }
    }

    fn window_for(monitor: &hypr::Monitor, cx: &mut App) -> Option<WindowHandle<LaunchView>> {
        if let Some((_, h)) = cx.global::<Launcher>().windows.iter().find(|(n, _)| *n == monitor.name)
            && h.read(cx).is_ok()
        {
            return Some(*h);
        }
        cx.global_mut::<Launcher>().windows.retain(|(n, _)| *n != monitor.name);
        let display = hypr::display_of(monitor, cx);
        let core = cx.global::<Launcher>().core.clone();
        let name = monitor.name.clone();
        let started = Instant::now();
        let opened = cx.open_window(
            WindowOptions {
                titlebar: None,
                focus: false,
                // Zero: stretched to the anchored edges, which is the monitor less the bar.
                window_bounds: Some(WindowBounds::Windowed(Bounds {
                    origin: point(px(0.), px(0.)),
                    size: size(px(0.), px(0.)),
                })),
                app_id: Some("yozora-launch".into()),
                window_background: WindowBackgroundAppearance::Transparent,
                display_id: display,
                // It's never "active" in GPUI's sense while closed; without this GPUI would draw
                // the open animation at 30 fps until the keyboard arrives.
                inactive_frame_interval: None,
                kind: WindowKind::LayerShell(LayerShellOptions {
                    namespace: "yozora-launch".into(),
                    layer: Layer::Background,
                    anchor: Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT,
                    exclusive_zone: None,
                    keyboard_interactivity: KeyboardInteractivity::None,
                    ..Default::default()
                }),
                ..Default::default()
            },
            |window, cx| {
                window.set_input_region(Some(&[]));
                cx.new(|cx| LaunchView::new(core, name, window, cx))
            },
        );
        match opened {
            Ok(h) => {
                tracing::debug!("opened the window for {} in {:?}", monitor.name, started.elapsed());
                cx.global_mut::<Launcher>().windows.push((monitor.name.clone(), h));
                Some(h)
            }
            Err(e) => {
                tracing::error!("could not open a window on {}: {e:#}", monitor.name);
                None
            }
        }
    }

    fn open_one(cx: &App) -> Option<WindowHandle<LaunchView>> {
        cx.global::<Launcher>().windows.iter().map(|(_, h)| *h).find(|h| h.read(cx).is_ok_and(|v| v.is_open()))
    }

    pub fn is_open(cx: &App) -> bool {
        Self::open_one(cx).is_some()
    }

    /// Opens on the focused monitor, with `query` typed in.
    pub fn open(query: Option<String>, cx: &mut App) {
        let started = Instant::now();
        let monitors = hypr::monitors().unwrap_or_default();
        let Some(focused) = monitors.iter().find(|m| m.focused).or(monitors.first()) else {
            tracing::warn!("no monitors from Hyprland");
            return;
        };
        if let Some(open) = Self::open_one(cx) {
            let same = open.read(cx).is_ok_and(|v| v.monitor == focused.name);
            if same {
                if let Some(q) = query {
                    let _ = open.update(cx, |v, window, cx| v.set_query(&q, window, cx));
                }
                return;
            }
            let _ = open.update(cx, |v, window, cx| v.close(false, window, cx));
        }
        let Some(handle) = Self::window_for(focused, cx) else { return };
        let core = cx.global::<Launcher>().core.clone();
        core.update(cx, |c, cx| c.refresh(cx));
        let _ = handle.update(cx, |v, window, cx| v.open(query.as_deref().unwrap_or(""), window, cx));
        tracing::debug!("open took {:?} before the frame", started.elapsed());
    }

    pub fn close(cx: &mut App) {
        if let Some(open) = Self::open_one(cx) {
            let _ = open.update(cx, |v, window, cx| v.close(true, window, cx));
        }
    }

    pub fn toggle(query: Option<String>, cx: &mut App) {
        if Self::is_open(cx) && query.is_none() {
            Self::close(cx);
        } else {
            Self::open(query, cx);
        }
    }

    /// Replaces the settings, and redraws.
    pub fn set_settings(settings: LaunchSettings, cx: &mut App) {
        let core = cx.global::<Launcher>().core.clone();
        core.update(cx, |c, cx| {
            c.commands = sources::commands(&settings.launch, c.dnd);
            let slow = (&c.settings.launch.project_roots, c.settings.launch.browser, c.settings.launch.currency)
                != (&settings.launch.project_roots, settings.launch.browser, settings.launch.currency);
            c.settings = settings;
            if slow {
                c.load_slow(cx);
            }
            c.changed(cx);
        });
    }
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
