//! The Settings window: sidebar, page header, the page, a live preview of the file being edited,
//! and a footer that says where changes went.

use std::{
    collections::HashMap,
    path::PathBuf,
    time::{Duration, Instant},
};

use gpui::{
    Animation, AnimationExt, AnyElement, AppContext, Context, Entity, FocusHandle, Focusable, FontWeight,
    InteractiveElement, IntoElement, ParentElement, Render, Rgba, SharedString, StatefulInteractiveElement, Styled,
    Subscription, Task, Window, div, ease_out_quint, prelude::FluentBuilder, px, rgba,
};
use yozora_config::{manifest::AppManifest, theme::ThemeConfig};
use yozora_ui::{
    Icon, Theme,
    controls::indicator_spring,
    icon_sized,
    input::{InputEvent, TextInput},
};

use crate::{
    Options,
    app::{CloseWindow, FocusSearch},
    apps::{self, AppEntry, Status},
    doc::Doc,
    hypr::{self, Monitor},
    pages, widgets,
};

pub const THEME_FILE: &str = "theme.toml";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Page {
    Appearance,
    Motion,
    App(String, AppPage),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppPage {
    Bar,
    Modules,
    Monitors,
    Options,
    About,
}

impl AppPage {
    pub fn name(self) -> &'static str {
        match self {
            AppPage::Bar => "Bar",
            AppPage::Modules => "Modules",
            AppPage::Monitors => "Monitors",
            AppPage::Options => "Options",
            AppPage::About => "About",
        }
    }

    fn icon(self) -> Icon {
        match self {
            AppPage::Bar => Icon::Bar,
            AppPage::Modules => Icon::Modules,
            AppPage::Monitors => Icon::Display,
            AppPage::Options => Icon::Settings,
            AppPage::About => Icon::Info,
        }
    }

    fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "bar" => AppPage::Bar,
            "modules" => AppPage::Modules,
            "monitors" => AppPage::Monitors,
            "options" | "more" => AppPage::Options,
            "about" => AppPage::About,
            _ => return None,
        })
    }
}

/// What a text field writes when it's committed.
#[derive(Clone, Debug, PartialEq)]
pub enum InputTarget {
    /// Sets a string value. An empty field removes the key, falling back to the default.
    /// `secret` values also make the file readable by its owner only.
    String { file: String, key: String, secret: bool },
    /// Appends to a list of strings, then clears the field.
    Append { file: String, key: String },
    /// Sets a number, if the text parses and is in range.
    Number { file: String, key: String, float: bool, min: Option<f64>, max: Option<f64> },
}

pub struct SettingsView {
    pub config_dir: PathBuf,
    extra_apps: Vec<PathBuf>,
    pub apps: Vec<AppEntry>,
    pub page: Page,
    /// Bumped on every page change, to replay the page's entrance.
    page_serial: u32,
    docs: HashMap<String, Doc>,
    inputs: HashMap<String, (Entity<TextInput>, InputTarget)>,
    search: Entity<TextInput>,
    query: String,
    pub monitors: Result<Vec<Monitor>, String>,
    pub selected_monitor: Option<String>,
    /// The last failed edit, shown in the footer.
    pub error: Option<String>,
    /// When the motion preview was last replayed.
    pub replay: Option<Instant>,
    confirm_reset: Option<Task<()>>,
    focus: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl SettingsView {
    pub fn new(opts: Options, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let search = cx.new(|cx| TextInput::new("Search settings", window, cx));
        let subscription = cx.subscribe(&search, |this: &mut Self, input, event, cx| {
            if matches!(event, InputEvent::Changed) {
                this.query = input.read(cx).text().trim().to_string();
                cx.notify();
            }
            if matches!(event, InputEvent::Cancel) {
                input.update(cx, |i, cx| i.set_text("", cx));
                this.query.clear();
                cx.notify();
            }
        });
        let apps = apps::discover(&opts.extra_apps, &[]);
        let mut view = Self {
            config_dir: opts.config_dir,
            extra_apps: opts.extra_apps,
            apps,
            page: Page::Appearance,
            page_serial: 0,
            docs: HashMap::new(),
            inputs: HashMap::new(),
            search,
            query: String::new(),
            monitors: Err("Loading monitors…".into()),
            selected_monitor: None,
            error: None,
            replay: None,
            confirm_reset: None,
            focus: cx.focus_handle(),
            _subscriptions: vec![subscription],
        };
        if view.apps.iter().any(|a| a.is_bar()) {
            view.page = Page::App(apps::BAR.into(), AppPage::Bar);
        }
        if let Some(page) = opts.page.as_deref()
            && let Err(e) = view.open_page(page, cx)
        {
            tracing::warn!("{e}");
        }
        view.refresh_monitors(cx);
        window.focus(&view.focus, cx);
        view
    }

    // ---- State --------------------------------------------------------------------------

    pub fn app(&self, id: &str) -> Option<&AppEntry> {
        self.apps.iter().find(|a| a.id() == id)
    }

    pub fn manifests(&self) -> Vec<AppManifest> {
        self.apps.iter().map(|a| a.manifest.clone()).collect()
    }

    pub fn set_statuses(&mut self, statuses: Vec<(String, Status)>, cx: &mut Context<Self>) {
        let mut changed = false;
        for (id, status) in statuses {
            if let Some(app) = self.apps.iter_mut().find(|a| a.manifest.id == id)
                && app.status != status
            {
                app.status = status;
                changed = true;
            }
        }
        if changed {
            cx.notify();
        }
    }

    /// Re-reads manifests: installed apps appear and removed ones disappear without a restart.
    pub fn rescan(&mut self, cx: &mut Context<Self>) {
        let apps = apps::discover(&self.extra_apps, &self.apps);
        let same = apps.len() == self.apps.len()
            && apps.iter().zip(&self.apps).all(|(a, b)| a.manifest == b.manifest && a.schema == b.schema);
        if same {
            return;
        }
        tracing::info!("apps: {}", apps.iter().map(|a| a.id()).collect::<Vec<_>>().join(", "));
        self.apps = apps;
        if let Page::App(id, page) = &self.page {
            let still_valid = self.app(id).is_some_and(|a| self.app_pages(a).contains(page));
            if !still_valid {
                self.page = Page::Appearance;
                self.page_serial += 1;
            }
        }
        cx.notify();
    }

    /// Files in the config directory changed: re-read the ones open here.
    pub fn files_changed(&mut self, names: Vec<String>, cx: &mut Context<Self>) {
        let all = names.iter().any(|n| n == "*");
        let mut theme_changed = false;
        for (name, doc) in self.docs.iter_mut() {
            if (all || names.contains(name)) && doc.reload() {
                theme_changed |= name == THEME_FILE;
            }
        }
        if theme_changed {
            self.reapply_theme(cx);
        }
        cx.notify();
    }

    pub fn reapply_theme(&mut self, cx: &mut Context<Self>) {
        let config: ThemeConfig = self.doc(THEME_FILE).parse();
        crate::app::apply_theme(&config, cx);
    }

    pub fn refresh_monitors(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async |this, cx| {
            let result = cx.background_spawn(async { hypr::monitors() }).await;
            this.update(cx, |this, cx| {
                this.monitors = result.map_err(|e| format!("{e:#}"));
                if let Ok(monitors) = &this.monitors
                    && this.selected_monitor.as_ref().is_none_or(|s| !monitors.iter().any(|m| &m.name == s))
                {
                    this.selected_monitor = monitors.first().map(|m| m.name.clone());
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// The doc for a file in the config directory, opened on first use.
    pub fn doc(&mut self, file: &str) -> &mut Doc {
        let path = self.config_dir.join(file);
        self.docs.entry(file.to_string()).or_insert_with(|| Doc::open(&path))
    }

    /// Edits a file and saves it. Failures show in the footer.
    pub fn edit(&mut self, file: &str, cx: &mut Context<Self>, edit: impl FnOnce(&mut Doc) -> anyhow::Result<()>) {
        let result = edit(self.doc(file));
        self.error = result.err().map(|e| format!("{e:#}"));
        if file == THEME_FILE {
            self.reapply_theme(cx);
        }
        cx.notify();
    }

    /// The file the current page edits.
    pub fn page_file(&self) -> Option<String> {
        match &self.page {
            Page::Appearance | Page::Motion => Some(THEME_FILE.into()),
            Page::App(id, _) => self.app(id).map(|a| a.manifest.config.clone()),
        }
    }

    pub fn app_pages(&self, app: &AppEntry) -> Vec<AppPage> {
        let mut pages = Vec::new();
        if app.is_bar() {
            pages.extend([AppPage::Bar, AppPage::Modules, AppPage::Monitors]);
        }
        if app.has_options() {
            pages.push(AppPage::Options);
        }
        pages.push(AppPage::About);
        pages
    }

    /// Opens a page by name: `appearance`, `bar`, `about` (the bar's), or `<app>:<page>`.
    pub fn open_page(&mut self, name: &str, cx: &mut Context<Self>) -> Result<(), String> {
        let page = match name {
            "appearance" | "theme" => Page::Appearance,
            "motion" => Page::Motion,
            other => {
                let (app, page) = other.split_once(':').unwrap_or((apps::BAR, other));
                let page = AppPage::parse(page).ok_or_else(|| format!("unknown page `{name}`"))?;
                let entry = self.app(app).ok_or_else(|| format!("no installed app with id `{app}`"))?;
                if !self.app_pages(entry).contains(&page) {
                    return Err(format!("{} has no {} page", entry.manifest.name, page.name()));
                }
                Page::App(app.to_string(), page)
            }
        };
        self.go(page, cx);
        Ok(())
    }

    pub fn go(&mut self, page: Page, cx: &mut Context<Self>) {
        if self.page != page {
            self.page = page;
            self.page_serial += 1;
            self.error = None;
            self.confirm_reset = None;
            if matches!(self.page, Page::App(_, AppPage::Monitors)) {
                self.refresh_monitors(cx);
            }
        }
        cx.notify();
    }

    /// A text field bound to `target`, kept across frames by `id`.
    #[allow(clippy::too_many_arguments)]
    pub fn input(
        &mut self,
        id: &str,
        placeholder: &str,
        mono: bool,
        target: InputTarget,
        current: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<TextInput> {
        if let Some((input, stored)) = self.inputs.get_mut(id) {
            *stored = target;
            let input = input.clone();
            if !input.read(cx).is_focused(window) && !matches!(self.inputs[id].1, InputTarget::Append { .. }) {
                input.update(cx, |i, cx| i.set_text(current.to_string(), cx));
            }
            return input;
        }
        let placeholder = placeholder.to_string();
        let input = cx.new(|cx| {
            let input = TextInput::new(placeholder, window, cx);
            if mono { input.mono() } else { input }
        });
        input.update(cx, |i, cx| i.set_text(current.to_string(), cx));
        let key = id.to_string();
        let subscription = cx.subscribe_in(&input, window, move |this, input, event, window, cx| match event {
            InputEvent::Submit | InputEvent::Blur => {
                let text = input.read(cx).text().to_string();
                this.commit_input(&key, text, cx);
                if matches!(event, InputEvent::Submit) && !matches!(this.inputs[&key].1, InputTarget::Append { .. }) {
                    window.focus(&this.focus, cx);
                }
            }
            InputEvent::Cancel => {
                // Discard the edit; the next render shows the saved value again.
                window.focus(&this.focus, cx);
                if matches!(this.inputs[&key].1, InputTarget::Append { .. }) {
                    input.update(cx, |i, cx| i.set_text("", cx));
                }
                cx.notify();
            }
            InputEvent::Changed => {}
        });
        self._subscriptions.push(subscription);
        self.inputs.insert(id.to_string(), (input.clone(), target));
        input
    }

    fn commit_input(&mut self, id: &str, text: String, cx: &mut Context<Self>) {
        let Some((input, target)) = self.inputs.get(id).cloned() else {
            return;
        };
        match target {
            InputTarget::String { file, key, secret } => {
                let unchanged = self.doc(&file).value(&key).and_then(|v| v.as_str().map(String::from));
                if unchanged.as_deref() == Some(text.as_str()) || (unchanged.is_none() && text.is_empty()) {
                    return;
                }
                let path = self.doc(&file).path.clone();
                // Before the save too, so the key is never in a file others can read; saves keep
                // the mode.
                if secret && path.exists() {
                    let _ = yozora_config::edit::restrict(&path);
                }
                self.edit(&file, cx, |doc| if text.is_empty() { doc.remove(&[&key]) } else { doc.set(&key, text) });
                if secret && let Err(e) = yozora_config::edit::restrict(&path) {
                    self.error = Some(format!("couldn't make {file} private: {e:#}"));
                }
            }
            InputTarget::Append { file, key } => {
                let text = text.trim().to_string();
                if text.is_empty() {
                    return;
                }
                let mut list: Vec<String> = match self.doc(&file).value(&key) {
                    Some(toml::Value::Array(a)) => a.iter().filter_map(|v| v.as_str().map(String::from)).collect(),
                    _ => Vec::new(),
                };
                if !list.contains(&text) {
                    list.push(text);
                }
                input.update(cx, |i, cx| i.set_text("", cx));
                self.edit(&file, cx, |doc| doc.set(&key, crate::doc::string_array(&list)));
            }
            InputTarget::Number { file, key, float, min, max } => {
                let text = text.trim();
                if text.is_empty() {
                    self.edit(&file, cx, |doc| doc.remove(&[&key]));
                    return;
                }
                let Ok(n) = text.parse::<f64>() else {
                    self.error = Some(format!("`{text}` isn't a number"));
                    cx.notify();
                    return;
                };
                if min.is_some_and(|m| n < m) || max.is_some_and(|m| n > m) {
                    self.error = Some(format!(
                        "{key} must be between {} and {}",
                        min.map_or("-∞".into(), |m| m.to_string()),
                        max.map_or("∞".into(), |m| m.to_string())
                    ));
                    cx.notify();
                    return;
                }
                if float {
                    self.edit(&file, cx, |doc| doc.set(&key, n));
                } else {
                    self.edit(&file, cx, |doc| doc.set(&key, n.round() as i64));
                }
            }
        }
    }

    /// Restore defaults asks once, then acts on a second click within four seconds.
    fn restore_defaults(&mut self, cx: &mut Context<Self>) {
        if self.confirm_reset.take().is_none() {
            self.confirm_reset = Some(cx.spawn(async |this, cx| {
                cx.background_executor().timer(Duration::from_secs(4)).await;
                this.update(cx, |this, cx| {
                    this.confirm_reset = None;
                    cx.notify();
                })
                .ok();
            }));
            cx.notify();
            return;
        }
        let Some(file) = self.page_file() else { return };
        let keys = pages::reset_keys(self);
        let keys: Vec<&str> = keys.iter().map(String::as_str).collect();
        self.edit(&file, cx, |doc| doc.remove(&keys));
    }

    // ---- Rendering ------------------------------------------------------------------------

    fn render_sidebar(&mut self, theme: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let side_bg = if theme.dark { rgba(0xFFFFFF06) } else { rgba(0x0F162605) };
        let file = self.page_file().unwrap_or_default();
        div()
            .relative()
            .w(px(248.))
            .flex_none()
            .h_full()
            .flex()
            .flex_col()
            .px(px(14.))
            .pt(px(20.))
            .pb(px(18.))
            .bg(side_bg)
            .border_r_1()
            .border_color(theme.hair)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(11.))
                    .h(px(40.))
                    .px(px(6.))
                    .child(
                        div()
                            .size(px(32.))
                            .flex_none()
                            .rounded(px(9.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .bg(theme.midnight)
                            .text_color(theme.accent)
                            .border_1()
                            .border_color(theme.hair_2)
                            .shadow(theme.accent_glow())
                            .child(icon_sized(Icon::Logo, 17.)),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .child(div().text_size(px(14.)).font_weight(FontWeight::SEMIBOLD).child("Settings"))
                            .child(
                                div()
                                    .text_size(px(11.5))
                                    .text_color(theme.ink_3)
                                    .child(format!("Yozora {}", env!("CARGO_PKG_VERSION"))),
                            ),
                    ),
            )
            .child(div().relative().mt(px(16.)).child(self.search.clone()).when(self.query.is_empty(), |el| {
                el.child(
                    div()
                        .absolute()
                        .right(px(8.))
                        .top(px(7.))
                        .px(px(6.))
                        .py(px(1.))
                        .rounded(px(5.))
                        .border_1()
                        .border_color(theme.hair_2)
                        .text_size(px(11.))
                        .text_color(theme.ink_3)
                        .child("Ctrl F"),
                )
            }))
            .child(div().id("nav").flex_1().min_h_0().mt(px(14.)).overflow_y_scroll().child(if self.query.is_empty() {
                self.render_nav(theme, cx).into_any_element()
            } else {
                self.render_search_results(theme, cx).into_any_element()
            }))
            .when(!file.is_empty(), |el| {
                el.child(
                    div()
                        .mt(px(12.))
                        .p(px(12.))
                        .rounded(px(12.))
                        .bg(theme.fill)
                        .border_1()
                        .border_color(theme.hair)
                        .font_family(theme.mono.clone())
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(8.))
                                .text_size(px(12.))
                                .font_weight(FontWeight::MEDIUM)
                                .child(div().text_color(theme.accent).child(icon_sized(Icon::File, 15.)))
                                .child(file),
                        )
                        .child(
                            div()
                                .mt(px(4.))
                                .text_size(px(11.))
                                .text_color(theme.ink_3)
                                .truncate()
                                .child(tilde(&self.config_dir) + "/"),
                        ),
                )
            })
    }

    fn render_nav(&mut self, theme: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        const ITEM: f32 = 34.;
        const GAP: f32 = 4.;
        const HEADER: f32 = 30.;
        let mut y = 0.;
        let mut selected_y = None;
        let mut children: Vec<AnyElement> = Vec::new();

        let mut item =
            |page: Page, glyph: Icon, label: String, trailing: Option<AnyElement>, y: &mut f32, this: &Self| {
                let on = this.page == page;
                if on {
                    selected_y = Some(*y);
                }
                *y += ITEM + GAP;
                let id = SharedString::from(format!("nav-{label}-{page:?}"));
                div()
                    .id(id)
                    .relative()
                    .flex()
                    .items_center()
                    .gap(px(11.))
                    .h(px(ITEM))
                    .mb(px(GAP))
                    .px(px(10.))
                    .rounded(px(9.))
                    .text_size(px(13.5))
                    .cursor_pointer()
                    .text_color(if on { theme.ink } else { theme.ink_2 })
                    .when(on, |el| el.font_weight(FontWeight::MEDIUM))
                    .when(!on, |el| el.hover(|s| s.bg(theme.fill)))
                    .child(div().text_color(if on { theme.accent } else { theme.ink_2 }).child(icon_sized(glyph, 17.)))
                    .child(div().flex_1().truncate().child(label))
                    .children(trailing)
                    .on_click(cx.listener(move |this, _, _, cx| this.go(page.clone(), cx)))
                    .into_any_element()
            };
        let header = |text: String, trailing: Option<AnyElement>, y: &mut f32| {
            *y += HEADER;
            div()
                .h(px(HEADER))
                .flex()
                .items_end()
                .justify_between()
                .pb(px(8.))
                .px(px(10.))
                .text_size(px(11.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme.ink_3)
                .child(text.to_uppercase())
                .children(trailing)
                .into_any_element()
        };

        children.push(header("Yozora".into(), None, &mut y));
        children.push(item(Page::Appearance, Icon::Appearance, "Appearance".into(), None, &mut y, self));
        children.push(item(Page::Motion, Icon::Motion, "Motion".into(), None, &mut y, self));
        for app in &self.apps {
            y += 10.;
            let dot = status_dot(&app.status, theme);
            children.push(div().h(px(10.)).into_any_element());
            children.push(header(app.manifest.name.clone(), dot, &mut y));
            for page in self.app_pages(app) {
                children.push(item(
                    Page::App(app.id().to_string(), page),
                    page.icon(),
                    page.name().into(),
                    None,
                    &mut y,
                    self,
                ));
            }
        }
        if self.apps.is_empty() {
            children.push(div().h(px(10.)).into_any_element());
            children.push(header("Apps".into(), None, &mut y));
            children.push(
                div()
                    .px(px(10.))
                    .text_size(px(12.))
                    .text_color(theme.ink_3)
                    .child("No Yozora apps are installed yet. They appear here as soon as they are.")
                    .into_any_element(),
            );
        }

        div()
            .relative()
            .flex()
            .flex_col()
            .when_some(selected_y, |el, sy| {
                el.child(
                    div()
                        .absolute()
                        .left_0()
                        .right_0()
                        .h(px(ITEM))
                        .rounded(px(9.))
                        .bg(theme.accent_soft)
                        .border_1()
                        .border_color(with_alpha(theme.accent, 0x42))
                        .with_spring("nav-hi", indicator_spring(theme, px(sy)), |el, y| el.top(y)),
                )
            })
            .children(children)
    }

    fn render_search_results(&mut self, theme: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let query = self.query.to_lowercase();
        let results: Vec<pages::SearchEntry> = pages::search_entries(self)
            .into_iter()
            .filter(|e| e.title.to_lowercase().contains(&query) || e.help.to_lowercase().contains(&query))
            .take(14)
            .collect();
        div()
            .flex()
            .flex_col()
            .gap(px(2.))
            .when(results.is_empty(), |el| {
                el.child(div().px(px(10.)).py(px(8.)).text_size(px(12.5)).text_color(theme.ink_3).child("No matches"))
            })
            .children(results.into_iter().enumerate().map(|(i, entry)| {
                let page = entry.page.clone();
                let search = self.search.clone();
                div()
                    .id(("result", i))
                    .flex()
                    .flex_col()
                    .gap(px(1.))
                    .px(px(10.))
                    .py(px(7.))
                    .rounded(px(9.))
                    .cursor_pointer()
                    .hover(|s| s.bg(theme.fill))
                    .child(div().text_size(px(13.)).child(entry.title))
                    .child(div().text_size(px(11.5)).text_color(theme.ink_3).child(entry.place))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        search.update(cx, |s, cx| s.set_text("", cx));
                        this.query.clear();
                        window.focus(&this.focus, cx);
                        this.go(page.clone(), cx);
                    }))
            }))
    }

    fn render_header(&mut self, theme: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let (title, description) = pages::title(self);
        let pill = |dot: Option<Rgba>, text: String| {
            div()
                .flex()
                .flex_none()
                .items_center()
                .gap(px(8.))
                .h(px(28.))
                .px(px(12.))
                .rounded(px(14.))
                .text_size(px(12.))
                .text_color(theme.ink_2)
                .bg(theme.fill)
                .border_1()
                .border_color(theme.hair)
                .when_some(dot, |el, color| el.child(div().size(px(6.)).rounded_full().bg(color)))
                .child(text)
        };
        let status: AnyElement = match &self.page {
            Page::Appearance | Page::Motion => {
                pill(Some(theme.accent), "Shared by every Yozora app".into()).into_any_element()
            }
            Page::App(id, _) => match self.app(id) {
                Some(app) => match &app.status {
                    Status::Running { version, .. } => pill(
                        Some(theme.accent),
                        match version {
                            Some(v) => format!("Running {v} · changes apply live"),
                            None => "Running · changes apply live".into(),
                        },
                    )
                    .into_any_element(),
                    Status::Stopped => {
                        let manifest = app.manifest.clone();
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(pill(Some(theme.ink_3), "Not running".into()))
                            .child(widgets::button("start", Some(Icon::Power), "Start", true, theme).on_click(
                                cx.listener(move |this, _, _, cx| {
                                    this.error = apps::start(&manifest).err().map(|e| format!("{e:#}"));
                                    cx.notify();
                                }),
                            ))
                            .into_any_element()
                    }
                    Status::NoSocket => pill(None, "Applies when the app reloads".into()).into_any_element(),
                    Status::Unknown => pill(Some(theme.ink_3), "Checking…".into()).into_any_element(),
                },
                None => div().into_any_element(),
            },
        };
        div()
            .flex()
            .justify_between()
            .items_start()
            .gap(px(20.))
            .px(px(40.))
            .pt(px(30.))
            .pb(px(22.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .min_w_0()
                    .child(div().text_size(px(26.)).font_weight(FontWeight::SEMIBOLD).child(title))
                    .child(div().mt(px(6.)).text_size(px(14.)).text_color(theme.ink_2).child(description)),
            )
            .child(status)
    }

    fn render_toml(&mut self, theme: &Theme) -> Option<impl IntoElement + use<>> {
        let file = self.page_file()?;
        let secrets: Vec<String> = self
            .apps
            .iter()
            .filter(|a| a.manifest.config == file)
            .filter_map(|a| a.schema.as_ref())
            .flat_map(|s| &s.section)
            .flat_map(|s| &s.field)
            .filter(|f| f.secret)
            .map(|f| f.key.clone())
            .collect();
        let doc = self.doc(&file);
        let code_bg = if theme.dark { rgba(0x00000038) } else { rgba(0x0F16260A) };
        let label = match (&doc.error, doc.saves) {
            (Some(_), _) => "has an error".to_string(),
            (None, 0) => "live".to_string(),
            (None, 1) => "1 save".to_string(),
            (None, n) => format!("{n} saves"),
        };
        let colors = SyntaxColors::new(theme);
        let changed = doc.changed.clone();
        let saves = doc.saves;
        let lines: Vec<String> = if doc.text.trim().is_empty() {
            vec!["# Nothing set yet: every value is".into(), "# the default. Changes land here.".into()]
        } else {
            crate::doc::masked_lines(&doc.text, &secrets)
        };
        let file_key = file.clone();
        let hl = with_alpha(theme.accent, 0x4D);
        Some(
            div()
                .w(px(300.))
                .flex_none()
                .max_h_full()
                .flex()
                .flex_col()
                .rounded(px(14.))
                .bg(code_bg)
                .border_1()
                .border_color(theme.hair)
                .overflow_hidden()
                .font_family(theme.mono.clone())
                .child(
                    div()
                        .flex()
                        .flex_none()
                        .items_center()
                        .justify_between()
                        .h(px(38.))
                        .px(px(14.))
                        .border_b_1()
                        .border_color(theme.hair)
                        .text_size(px(11.5))
                        .text_color(theme.ink_2)
                        .child(div().text_color(theme.ink).font_weight(FontWeight::MEDIUM).child(file))
                        .child(label),
                )
                .child(
                    div()
                        .id("toml-body")
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scroll()
                        .pt(px(10.))
                        .pb(px(12.))
                        .text_size(px(11.5))
                        .line_height(px(17.))
                        .children(lines.into_iter().enumerate().map(move |(i, line)| {
                            let row =
                                div().px(px(16.)).min_h(px(17.)).whitespace_nowrap().children(colors.spans(&line));
                            if changed.contains(&i) {
                                row.with_animation(
                                    SharedString::from(format!("chg-{file_key}-{saves}-{i}")),
                                    Animation::new(Duration::from_millis(1400)).with_easing(ease_out_quint()),
                                    move |el, t| {
                                        let mut c = hl;
                                        c.a *= 1.0 - t;
                                        el.bg(c)
                                    },
                                )
                                .into_any_element()
                            } else {
                                row.into_any_element()
                            }
                        })),
                ),
        )
    }

    fn render_footer(&mut self, theme: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let file = self.page_file();
        let confirming = self.confirm_reset.is_some();
        let (state, path) = match &file {
            Some(f) => {
                let path = tilde(&self.config_dir.join(f));
                let doc = self.doc(f);
                (doc.error.clone().map(Err).unwrap_or(Ok(doc.saves)), path)
            }
            None => (Ok(0), String::new()),
        };
        let left: AnyElement = if let Some(error) = &self.error {
            div()
                .flex()
                .items_center()
                .gap(px(9.))
                .flex_1()
                .min_w_0()
                .text_size(px(12.5))
                .text_color(theme.danger)
                .child(icon_sized(Icon::Warning, 15.))
                .child(div().truncate().child(error.clone()))
                .into_any_element()
        } else {
            match state {
                Err(e) => div()
                    .flex()
                    .items_center()
                    .gap(px(9.))
                    .flex_1()
                    .min_w_0()
                    .text_size(px(12.5))
                    .text_color(theme.danger)
                    .child(icon_sized(Icon::Warning, 15.))
                    .child(div().truncate().child(format!("Fix this to edit here: {e}")))
                    .into_any_element(),
                Ok(saves) => {
                    let badge = div()
                        .size(px(20.))
                        .flex_none()
                        .rounded_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .bg(theme.accent_soft)
                        .text_color(theme.accent)
                        .child(icon_sized(if saves > 0 { Icon::Check } else { Icon::File }, 12.));
                    let badge = if saves > 0 && !theme.motion.reduced {
                        badge
                            .with_animation(
                                ("saved", saves as usize),
                                Animation::new(Duration::from_millis(600)).with_easing(ease_out_quint()),
                                |el, t| el.opacity(0.4 + 0.6 * t),
                            )
                            .into_any_element()
                    } else {
                        badge.into_any_element()
                    };
                    div()
                        .flex()
                        .flex_1()
                        .min_w_0()
                        .items_center()
                        .gap(px(9.))
                        .text_size(px(12.5))
                        .text_color(theme.ink_2)
                        .child(badge)
                        .child(div().flex_none().child(if saves > 0 { "Saved to" } else { "Editing" }))
                        .child(
                            div()
                                .min_w_0()
                                .truncate()
                                .font_family(theme.mono.clone())
                                .text_size(px(11.5))
                                .text_color(theme.ink)
                                .child(path),
                        )
                        .into_any_element()
                }
            }
        };
        div()
            .flex()
            .flex_none()
            .items_center()
            .justify_between()
            .gap(px(16.))
            .h(px(56.))
            .px(px(40.))
            .border_t_1()
            .border_color(theme.hair)
            .child(left)
            .when_some(file, |el, file| {
                let open_path = self.config_dir.join(&file);
                el.child(
                    div()
                        .flex()
                        .flex_none()
                        .gap(px(8.))
                        .when(!pages::reset_keys(self).is_empty(), |el| {
                            el.child(
                                widgets::button(
                                    "reset",
                                    Some(Icon::Reset),
                                    if confirming { "Click again to restore" } else { "Restore defaults" },
                                    confirming,
                                    theme,
                                )
                                .on_click(cx.listener(|this, _, _, cx| this.restore_defaults(cx))),
                            )
                        })
                        .child(
                            widgets::button("open-file", Some(Icon::External), format!("Open {file}"), false, theme)
                                .on_click(move |_, _, _| {
                                    if !open_path.exists() {
                                        let _ = yozora_config::edit::write_atomic(&open_path, b"");
                                    }
                                    apps::open(&open_path.to_string_lossy());
                                }),
                        ),
                )
            })
    }
}

impl Render for SettingsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::global(cx).clone();
        let win_bg = if theme.dark { rgba(0x0B0F18F2) } else { rgba(0xF5F7FBF5) };
        let wide = window.viewport_size().width >= px(1060.);
        let page = pages::render(self, window, cx);
        let animated = if theme.motion.reduced {
            div()
                .child(page)
                .with_animation(
                    ("page", self.page_serial as usize),
                    Animation::new(Duration::from_millis(120)),
                    |el, t| el.opacity(t),
                )
                .into_any_element()
        } else {
            div()
                .child(page)
                .with_animation(
                    ("page", self.page_serial as usize),
                    Animation::new(Duration::from_millis(440)).with_easing(ease_out_quint()),
                    |el, t| el.opacity(t).mt(px(10. * (1. - t))),
                )
                .into_any_element()
        };
        let toml = if wide { self.render_toml(&theme).map(|t| t.into_any_element()) } else { None };
        div()
            .id("settings")
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, _: &FocusSearch, window, cx| {
                let focus = this.search.read(cx).focus_handle(cx);
                window.focus(&focus, cx);
            }))
            .on_action(|_: &CloseWindow, window, _| window.remove_window())
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    // Clicking outside a text field commits it. Fields stop their own clicks.
                    if this.inputs.values().any(|(i, _)| i.read(cx).is_focused(window)) {
                        window.focus(&this.focus, cx);
                    }
                }),
            )
            .size_full()
            .flex()
            .bg(win_bg)
            .text_color(theme.ink)
            .font_family(theme.font.clone())
            .child(self.render_sidebar(&theme, cx))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .flex()
                    .flex_col()
                    .child(self.render_header(&theme, cx))
                    .child(
                        div()
                            .flex_1()
                            .min_h_0()
                            .flex()
                            .gap(px(28.))
                            .px(px(40.))
                            .pb(px(20.))
                            .child(div().id("page").flex_1().min_w_0().h_full().overflow_y_scroll().child(animated))
                            .children(toml),
                    )
                    .child(self.render_footer(&theme, cx)),
            )
    }
}

fn status_dot(status: &Status, theme: &Theme) -> Option<AnyElement> {
    let color = match status {
        Status::Running { .. } => theme.accent,
        Status::Stopped => theme.ink_3,
        Status::NoSocket | Status::Unknown => return None,
    };
    Some(div().size(px(6.)).mb(px(3.)).rounded_full().bg(color).into_any_element())
}

pub fn with_alpha(color: Rgba, alpha: u8) -> Rgba {
    Rgba { a: alpha as f32 / 255.0, ..color }
}

/// `/home/me/.config/yozora` as `~/.config/yozora`.
pub fn tilde(path: &std::path::Path) -> String {
    let s = path.display().to_string();
    match std::env::var("HOME") {
        Ok(home) if !home.is_empty() && s.starts_with(&home) => format!("~{}", &s[home.len()..]),
        _ => s,
    }
}

/// TOML coloring for the preview, from the design's code tokens.
#[derive(Clone, Copy)]
struct SyntaxColors {
    header: Rgba,
    string: Rgba,
    number: Rgba,
    boolean: Rgba,
    punct: Rgba,
    key: Rgba,
}

impl SyntaxColors {
    fn new(theme: &Theme) -> Self {
        if theme.dark {
            Self {
                header: rgba(0x8BBEFFFF),
                string: rgba(0xA9E3C8FF),
                number: rgba(0xFFC58BFF),
                boolean: rgba(0xC4B1FFFF),
                punct: rgba(0x7A869FFF),
                key: theme.ink,
            }
        } else {
            Self {
                header: rgba(0x2E6CD9FF),
                string: rgba(0x0D7F69FF),
                number: rgba(0xAD5A0DFF),
                boolean: rgba(0x6A4FDBFF),
                punct: rgba(0x5F6A84FF),
                key: theme.ink,
            }
        }
    }

    fn spans(&self, line: &str) -> Vec<AnyElement> {
        let span =
            |text: &str, color: Rgba| div().flex_none().text_color(color).child(text.to_string()).into_any_element();
        let row = |spans: Vec<AnyElement>| vec![div().flex().children(spans).into_any_element()];
        let trimmed = line.trim_start();
        if trimmed.starts_with('#') {
            return row(vec![span(line, self.punct)]);
        }
        if trimmed.starts_with('[') {
            return row(vec![span(line, self.header)]);
        }
        let Some((key, value)) = line.split_once('=') else {
            return row(vec![span(line, self.key)]);
        };
        let v = value.trim();
        let color = if v.starts_with('"') || v.starts_with('\'') || (v.starts_with('[') && v.contains('"')) {
            self.string
        } else if v == "true" || v == "false" {
            self.boolean
        } else {
            self.number
        };
        row(vec![span(key, self.key), span("=", self.punct), span(value, color)])
    }
}
