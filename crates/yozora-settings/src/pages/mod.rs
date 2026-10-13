//! Settings pages. Built-in pages for the shared theme and the bar; a generic page rendered
//! from any app's settings schema.

pub mod about;
pub mod appearance;
pub mod bar;
pub mod generic;
pub mod modules;
pub mod monitors;
pub mod motion;

use gpui::{AnyElement, Context, IntoElement, Window, div};

use crate::view::{AppPage, Page, SettingsView};

pub fn render(view: &mut SettingsView, window: &mut Window, cx: &mut Context<SettingsView>) -> AnyElement {
    match view.page.clone() {
        Page::Appearance => appearance::render(view, window, cx),
        Page::Motion => motion::render(view, window, cx),
        Page::App(id, page) => {
            let Some(app) = view.app(&id).cloned() else {
                return div().into_any_element();
            };
            match page {
                AppPage::Bar => bar::render(view, &app, cx),
                AppPage::Modules => modules::render(view, &app, cx),
                AppPage::Monitors => monitors::render(view, &app, cx),
                AppPage::Options => generic::render(view, &app, window, cx),
                AppPage::About => about::render(view, &app, cx),
            }
        }
    }
}

/// The page's heading and one line under it.
pub fn title(view: &SettingsView) -> (String, String) {
    let (t, d): (&str, String) = match &view.page {
        Page::Appearance => ("Appearance", "Theme, accent and glass for every Yozora app.".into()),
        Page::Motion => ("Motion", "How things move. Springs, not durations.".into()),
        Page::App(id, page) => {
            let name = view.app(id).map(|a| a.manifest.name.clone()).unwrap_or_default();
            return match page {
                AppPage::Bar => ("Bar".into(), "Style, size and the space it keeps.".into()),
                AppPage::Modules => ("Modules".into(), "What the bar shows, and where. Drag to reorder.".into()),
                AppPage::Monitors => ("Monitors".into(), "Which screens get a bar, and how much of one.".into()),
                AppPage::Options => (
                    format!("{name} options"),
                    view.app(id)
                        .and_then(|a| a.manifest.description.clone())
                        .unwrap_or_else(|| "Everything else this app lets you change.".into()),
                ),
                AppPage::About => {
                    (name.clone(), view.app(id).and_then(|a| a.manifest.description.clone()).unwrap_or_default())
                }
            };
        }
    };
    (t.into(), d)
}

/// Keys "Restore defaults" removes on the current page.
pub fn reset_keys(view: &SettingsView) -> Vec<String> {
    let keys: &[&str] = match &view.page {
        Page::Appearance => &["theme", "accent", "glass", "font"],
        Page::Motion => &["motion"],
        Page::App(id, page) => {
            return match page {
                AppPage::Bar => ["bar.style", "bar.height", "bar.reserve_space", "bar.blur"].map(String::from).to_vec(),
                AppPage::Modules => vec!["modules".into()],
                AppPage::Monitors => vec!["monitor".into(), "bar.monitors".into()],
                AppPage::Options => view
                    .app(id)
                    .and_then(|a| a.schema.as_ref())
                    .map(|s| s.section.iter().flat_map(|s| &s.field).map(|f| f.key.clone()).collect())
                    .unwrap_or_default(),
                AppPage::About => Vec::new(),
            };
        }
    };
    keys.iter().map(|k| k.to_string()).collect()
}

pub struct SearchEntry {
    pub page: Page,
    pub title: String,
    pub help: String,
    /// Where it lives, shown under the title.
    pub place: String,
}

/// Everything search can find: built-in settings and every schema field.
pub fn search_entries(view: &SettingsView) -> Vec<SearchEntry> {
    let mut entries = Vec::new();
    let add = |entries: &mut Vec<SearchEntry>, page: Page, place: &str, items: &[(&str, &str)]| {
        for (title, help) in items {
            entries.push(SearchEntry {
                page: page.clone(),
                title: title.to_string(),
                help: help.to_string(),
                place: place.into(),
            });
        }
    };
    add(&mut entries, Page::Appearance, "Appearance", appearance::SEARCH);
    add(&mut entries, Page::Motion, "Motion", motion::SEARCH);
    for app in &view.apps {
        let id = app.id().to_string();
        if app.is_bar() {
            let name = &app.manifest.name;
            add(&mut entries, Page::App(id.clone(), AppPage::Bar), &format!("{name} · Bar"), bar::SEARCH);
            add(&mut entries, Page::App(id.clone(), AppPage::Modules), &format!("{name} · Modules"), modules::SEARCH);
            add(
                &mut entries,
                Page::App(id.clone(), AppPage::Monitors),
                &format!("{name} · Monitors"),
                monitors::SEARCH,
            );
        }
        if let Some(schema) = &app.schema {
            for section in &schema.section {
                for field in &section.field {
                    entries.push(SearchEntry {
                        page: Page::App(id.clone(), AppPage::Options),
                        title: field.label.clone(),
                        help: format!("{} {} {}", field.help.clone().unwrap_or_default(), field.key, section.title),
                        place: format!("{} · {}", app.manifest.name, section.title),
                    });
                }
            }
        }
        entries.push(SearchEntry {
            page: Page::App(id.clone(), AppPage::About),
            title: format!("About {}", app.manifest.name),
            help: "version running start socket".into(),
            place: app.manifest.name.clone(),
        });
    }
    entries
}
