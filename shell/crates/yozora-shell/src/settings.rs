//! Settings: a page list on the left, the page on the right. Appearance is where the look, its
//! time of day and the effects level are chosen. After the pages every install has come the apps
//! Settings finds through their manifests, one entry each; an app's tabs hold its own pages and
//! the options its schema describes.

use std::sync::Arc;

use rune::{Color, El, ScrollSpec, Spring, SpringPair, Transition, Tx, col, el, row};
use yozora_style::{Fx, Look, Motion, Tokens, tokens};

use crate::kit::{glyph_el, label};
use crate::settings_apps::{self, AppEntry, Status};
use crate::settings_pages as pages;
use crate::settings_ui::{self as ui, button, group, segmented, segmented_tx, status_pill};
use crate::stage::Stage;
use crate::{settings_bar as bar, settings_form as form};

pub const W: f32 = 1080.0;
pub const H: f32 = 660.0;
const SIDEBAR: f32 = 247.0;

/// The pages every install has, before the apps.
const FIXED: [(&str, &str); 4] = [("Appearance", "sun"), ("Displays", "monitor"), ("Motion", "gauge"), ("About Yozora", "globe")];

/// A page of an app. The page number carries the tab above its low byte: `page | tab << 8`, with
/// 0 meaning the app's first tab.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tab {
    Style,
    Modules,
    Monitors,
    Options,
    About,
}

impl Tab {
    const ALL: [Tab; 5] = [Tab::Style, Tab::Modules, Tab::Monitors, Tab::Options, Tab::About];

    fn name(self) -> &'static str {
        match self {
            Tab::Style => "Style",
            Tab::Modules => "Modules",
            Tab::Monitors => "Monitors",
            Tab::Options => "Options",
            Tab::About => "About",
        }
    }

    fn slug(self) -> &'static str {
        match self {
            Tab::Style => "style",
            Tab::Modules => "modules",
            Tab::Monitors => "monitors",
            Tab::Options => "options",
            Tab::About => "about",
        }
    }

    fn code(self) -> usize {
        Tab::ALL.iter().position(|t| *t == self).unwrap_or(0) + 1
    }

    fn of(code: usize) -> Option<Tab> {
        code.checked_sub(1).and_then(|i| Tab::ALL.get(i)).copied()
    }
}

fn tabs(app: &AppEntry) -> Vec<Tab> {
    let mut tabs = Vec::new();
    if app.is_bar() {
        tabs.extend([Tab::Style, Tab::Modules, Tab::Monitors]);
    }
    if app.has_options() {
        tabs.push(Tab::Options);
    }
    tabs.push(Tab::About);
    tabs
}

fn glyph_for(id: &str) -> &'static str {
    match id {
        "bar" => "window",
        "launch" => "search",
        "notify" => "bell",
        "lock" => "lock",
        "idle" => "moon",
        _ => "package",
    }
}

/// The page a name on the command line stands for: `appearance`, `displays`, `motion`, `about`,
/// an app (`bar`, `notifications`, `launcher`, `lock`, `idle`, or any installed app's id), a tab
/// of the bar (`modules`, `monitors`, `options`), or `<app>:<tab>`.
pub fn page_named(name: &str) -> Option<usize> {
    let n = name.trim().to_lowercase();
    let (head, tab) = match n.split_once(':') {
        Some((a, t)) => (a, Some(t)),
        None => (n.as_str(), None),
    };
    let alias = |keys: &[&str]| keys.contains(&head);
    if tab.is_none() {
        let fixed = match () {
            _ if alias(&["appearance", "style", "look", "theme"]) => Some(0),
            _ if alias(&["display", "displays"]) => Some(1),
            _ if alias(&["motion", "effects"]) => Some(2),
            _ if alias(&["about"]) => Some(3),
            _ => None,
        };
        if fixed.is_some() {
            return fixed;
        }
    }
    let apps = settings_apps::apps();
    // The bar's own tabs answer to their bare names, as they did in the old Settings.
    let (head, tab) = match (head, tab) {
        ("modules" | "monitors" | "options" | "more", None) => ("bar", Some(head)),
        other => other,
    };
    let id = match head {
        "notifications" | "notify" => "notify",
        "launcher" | "launch" => "launch",
        other => other,
    };
    let at = apps.iter().position(|a| a.id() == id || a.short_name().to_lowercase() == id)?;
    let code = match tab {
        None => 0,
        Some(t) => {
            let tab = Tab::ALL.iter().copied().find(|x| x.slug() == t || (t == "more" && *x == Tab::Options))?;
            if !tabs(&apps[at]).contains(&tab) {
                return None;
            }
            tab.code()
        }
    };
    Some((FIXED.len() + at) | (code << 8))
}

/// Which page and tab a page number means, given the apps there are.
fn locate(page: usize, apps: &[AppEntry]) -> (usize, Option<Tab>) {
    let index = (page & 0xff).min(FIXED.len() + apps.len().max(1) - 1);
    let tab = Tab::of(page >> 8).filter(|t| index >= FIXED.len() && apps.get(index - FIXED.len()).is_some_and(|a| tabs(a).contains(t)));
    (index, tab)
}

fn sidebar(s: &Stage, t: &Tokens, page: usize, apps: &[AppEntry]) -> El {
    let item = |i: usize, name: &str, glyph: &str, dot: Option<Color>| {
        let on = i == page;
        let mut r = row().w_full().h(42.0).px(14.0).gap(14.0).items_center().radius(t.radii.ctl).fill(if on { t.fill } else { Color::TRANSPARENT })
            .key(format!("page{i}")).role("button").label(name).hit()
            .child(glyph_el(t, glyph, 17.0, if on { t.ink } else { t.ink2 }))
            .child(label(t, 14.0, if on { 600.0 } else { 500.0 }, name, if on { t.ink } else { t.ink2 }).grow(1.0).truncate());
        if let Some(c) = dot {
            r = r.child(el().size(7.0, 7.0).radius(3.5).fill(c));
        }
        let r = if on { ui::on_mark(t, r) } else { r.hover(t.fill) };
        r.on_press(move |s: &mut Stage| {
            s.settings.commit();
            s.settings.clear_error();
            s.settings_page = i;
        })
    };
    let mut list = col().w_full().gap(4.0).px(12.0).pb(12.0);
    for (i, (name, glyph)) in FIXED.iter().enumerate() {
        list = list.child(item(i, name, glyph, None));
    }
    if !apps.is_empty() {
        list = list.child(label(t, 12.0, 600.0, "APPS", t.ink3).px(14.0).pt(14.0).pb(4.0));
    }
    for (i, app) in apps.iter().enumerate() {
        let dot = matches!(app.status, Status::Running { .. }).then_some(t.acc);
        list = list.child(item(FIXED.len() + i, &app.short_name(), glyph_for(app.id()), dot));
    }
    let _ = s;
    col().w(SIDEBAR).h_full().border_sides([0.0, 0.0, 1.0, 0.0], t.rule)
        .child(label(t, 13.0, 600.0, "Settings", t.ink2).padding(24.0).pb(12.0))
        .child(col().w_full().grow(1.0).min_h(0.0).scroll(ScrollSpec { stick_bottom: false }).child(list))
}

/// A miniature of a look: its sky, its bar and one open panel, in its own colours.
fn preview(look: Look) -> El {
    let t = tokens(look);
    let line = |w: f32, c: Color| el().w(w).h(5.0).radius(t.radii.pill.min(3.0)).fill(c);
    el().w_full().h(140.0).radius(t.radii.card).fill(t.bg).clip_rounded(t.radii.card)
        .child(el().pin(Some(12.0), Some(8.0), Some(12.0), None).h(14.0).radius(t.radii.pill.min(7.0)).fill(t.surf).border(1.0, t.rule))
        .child(
            col().pin(None, Some(28.0), Some(26.0), None).w(140.0).h(100.0).padding(14.0).gap(8.0).radius(t.radii.card.min(14.0)).fill(t.surf).border(1.0, t.rule)
                .child(line(90.0, t.ink2)).child(line(60.0, t.ink3)).child(line(110.0, t.ink2)).child(line(50.0, t.ink3)),
        )
}

fn style_card(t: &Tokens, look: Look, on: bool, tag: &str) -> El {
    let slug = look.slug();
    let card = col().w(366.0).gap(14.0).padding(8.0).radius(t.radii.card).fill(t.raised).key(format!("look-{slug}")).role("button").label(look.name()).hit()
        .child(preview(look))
        .child(
            row().px(8.0).pb(4.0).gap(10.0).items_center()
                .child(label(t, 15.0, 600.0, look.name(), t.ink))
                .child(label(t, 13.0, 400.0, tag, t.ink2)),
        )
        .on_press_tx(move |s: &mut Stage, tx: &mut Tx| {
            // A new look lands whole: fonts, radii and sizes gliding between two looks cost every
            // surface a frame for as long as they glide.
            tx.instant();
            // Daybreak is Surface tension in its day sky.
            if look == Look::Daybreak {
                crate::config::set_many(&s.sh, &[("time", "day"), ("look", "tension")]);
            } else {
                crate::config::set(&s.sh, "look", slug);
            }
        });
    if on { card.border(2.0, t.acc) } else { card }
}

fn choice(t: &Tokens, key: &'static str, options: &[(&'static str, &'static str)], current: &str, width: f32) -> El {
    let names: Vec<String> = options.iter().map(|(_, n)| n.to_string()).collect();
    let values: Vec<&'static str> = options.iter().map(|(v, _)| *v).collect();
    let at = values.iter().position(|v| *v == current);
    segmented_tx(t, key, &names, at, width, move |s, tx, i| {
        // The time of day swaps the look; the effects level only changes how it moves.
        if key == "time" {
            tx.instant();
        }
        crate::config::set(&s.sh, key, values[i]);
    })
}

fn appearance(s: &Stage, t: &Tokens) -> Vec<El> {
    let c = s.sh.config();
    let tension_on = |l: Look| match l {
        Look::Tension => c.look == "tension" && c.time != "day",
        Look::Daybreak => c.look == "tension" && c.time == "day",
        other => c.look == other.slug(),
    };
    let cards = col().gap(16.0)
        .child(row().gap(16.0)
            .child(style_card(t, Look::Tension, tension_on(Look::Tension), "Liquid. Quiet. Alive."))
            .child(style_card(t, Look::Folio, tension_on(Look::Folio), "Printed. Plain. Considered.")))
        .child(row().gap(16.0)
            .child(style_card(t, Look::Phosphor, tension_on(Look::Phosphor), "Lit. Terse. Mechanical."))
            .child(style_card(t, Look::Prism, tension_on(Look::Prism), "Light. Clear. Precise.")));
    let time = col().gap(10.0).child(label(t, 13.0, 500.0, "Time of day", t.ink2))
        .child(choice(t, "time", &[("night", "Night"), ("day", "Daybreak"), ("follow", "Follow the sun")], &c.time, 400.0));
    let fx = col().gap(10.0).child(label(t, 13.0, 500.0, "Effects", t.ink2))
        .child(choice(t, "fx", &[("full", "Full"), ("calm", "Calm"), ("still", "Still")], &c.fx, 300.0));
    let mut out = vec![group(t, "Style", None), cards, row().gap(32.0).child(time).child(fx)];
    if c.time == "follow" {
        out.push(group(t, "Daybreak hours", Some("Surface tension wears its day sky between these times.")));
        let clock = |id: &'static str, name: &str, key: &'static str, now: &str| {
            let field = form::text_field(s, form::Input {
                id: format!("shell.toml:{key}"),
                target: crate::settings_state::Target::Clock { file: "shell.toml".into(), key: key.into() },
                current: now.to_owned(),
                placeholder: "HH:MM".into(),
                width: 120.0,
                secret: false,
                mono: true,
                t,
            });
            row().gap(14.0).items_center().key(id).child(label(t, 13.0, 500.0, name, t.ink2)).child(field)
        };
        out.push(row().gap(32.0)
            .child(clock("day-from", "Day starts", "day_from", &c.day_from))
            .child(clock("night-from", "Night starts", "night_from", &c.night_from)));
    }
    out
}

/// A page's title and the line under it.
fn title(page: usize, tab: Option<Tab>, apps: &[AppEntry]) -> (String, String) {
    if page < FIXED.len() {
        return match page {
            0 => ("Appearance".into(), "The look, its time of day and how much moves. Changes apply as you press them.".into()),
            1 => ("Displays".into(), "The bar follows each connected display.".into()),
            2 => ("Motion".into(), "How the shell moves, look by look.".into()),
            _ => ("About Yozora".into(), "A Hyprland desktop shell, drawn on Rune.".into()),
        };
    }
    let Some(app) = apps.get(page - FIXED.len()) else { return (String::new(), String::new()) };
    let line = match tab.unwrap_or_else(|| tabs(app)[0]) {
        Tab::Style => "Style, size and the space it keeps.".to_owned(),
        Tab::Modules => "What the bar shows, and where.".into(),
        Tab::Monitors => "Which screens get a bar, and how much of one.".into(),
        Tab::Options | Tab::About => app.manifest.description.clone().unwrap_or_else(|| "Everything this app lets you change.".into()),
    };
    (app.short_name(), line)
}

/// The capsule and button at the top right of an app's page: whether it runs, and a way to start it.
fn app_status(t: &Tokens, app: &AppEntry) -> El {
    match &app.status {
        Status::Running { version, .. } => status_pill(
            t,
            Some(t.acc),
            match version {
                Some(v) => format!("Running {v} \u{b7} changes apply live"),
                None => "Running \u{b7} changes apply live".into(),
            },
        ),
        Status::Stopped => {
            let r = row().gap(8.0).items_center().child(status_pill(t, Some(t.ink3), "Not running"));
            if app.startable() {
                let m = app.manifest.clone();
                r.child(button(t, "app-start".into(), Some("power"), "Start", true, move |s: &mut Stage| {
                    if let Err(e) = settings_apps::start(&m) {
                        s.settings.fail(format!("{e:#}"));
                    }
                }))
            } else {
                r
            }
        }
        Status::NoSocket => status_pill(t, None, "Applies when the app reloads"),
        Status::Unknown => status_pill(t, Some(t.ink3), "Checking\u{2026}"),
    }
}

fn rise(t: &Tokens, fx: Fx, body: El) -> El {
    let m = Motion::of(t.look, fx);
    if m.row_stagger == 0.0 {
        return body;
    }
    let up = if t.look == Look::Phosphor { 0.0 } else { 8.0 };
    // Rows that leave do so at once, so the next page never waits under the last.
    body.map_children(|i, c| {
        c.transition(Transition::offset(0.0, up).combined(Transition::opacity()).springs(SpringPair::new(Spring::new(m.open * 0.6, 0.0), Spring::new(0.02, 0.0))))
            .enter_delay(m.row_stagger * (i as f32 + 1.0).min(8.0))
    })
}

pub fn view(s: &Stage, t: &Tokens) -> El {
    s.settings.tick();
    s.settings.watch_focus();
    settings_apps::refresh(&s.sh);
    let apps: Arc<Vec<AppEntry>> = settings_apps::apps();
    let (index, tab) = locate(s.settings_page, &apps);
    let (_, fx) = s.sh.look();
    let app = index.checked_sub(FIXED.len()).and_then(|i| apps.get(i));
    let tab = app.map(|a| tab.unwrap_or_else(|| tabs(a)[0]));
    let (name, line) = title(index, tab, &apps);

    let mut items: Vec<El> = Vec::new();
    if let Some(a) = app && let Some(e) = s.settings.file_error(&a.manifest.config) {
        items.push(ui::notice(t, &format!("{} has an error", a.manifest.config), format!("Fix it by hand before editing here. {e}"), true));
    }
    match (app, tab) {
        (None, _) => items.extend(match index {
            0 => appearance(s, t),
            1 => pages::displays(s, t),
            2 => pages::motion(s, t),
            _ => pages::about(s, t),
        }),
        (Some(a), Some(Tab::Style)) => {
            let go = move |s: &mut Stage| s.settings_page = index | (Tab::Monitors.code() << 8);
            items.extend(bar::style_page(s, t, a, go));
        }
        (Some(a), Some(Tab::Modules)) => items.extend(bar::modules_page(s, t, a)),
        (Some(a), Some(Tab::Monitors)) => items.extend(bar::monitors_page(s, t, a)),
        (Some(a), Some(Tab::Options)) => items.extend(form::schema_page(s, t, a)),
        (Some(a), _) => items.extend(pages::app_about(s, t, a)),
    }

    if let (Some(a), Some(tab)) = (app, tab) && tab != Tab::About {
        items.push(form::footnote(t, "file", format!("{}  \u{b7}  saved changes apply at once", a.manifest.config_path().display())));
    }

    // The head: title, and for an app its status.
    let mut head = row().w_full().px(28.0).pt(24.0).gap(20.0).justify_between().items_start()
        .child(col().grow(1.0).min_w(0.0).child(ui::heading(t, &name, &line)));
    if let Some(a) = app {
        head = head.child(app_status(t, a));
    }
    head = head.child(ui::tool(t, "settings-close".into(), "x", "Close Settings", true, |s: &mut Stage| {
        s.settings.commit();
        s.open = None;
    }));

    // The tabs, and "Restore defaults" for the one in view.
    let mut strip = None;
    if let (Some(a), Some(tab)) = (app, tab) {
        let all = tabs(a);
        let mut bar_row = row().w_full().px(28.0).pt(16.0).gap(16.0).items_center().justify_between();
        if all.len() > 1 {
            let names: Vec<String> = all.iter().map(|t| t.name().to_owned()).collect();
            let at = all.iter().position(|x| *x == tab);
            let all2 = all.clone();
            bar_row = bar_row.child(segmented(t, "tab", &names, at, 96.0 * all.len() as f32, move |s, i| {
                s.settings.commit();
                s.settings.clear_error();
                s.settings_page = index | (all2[i].code() << 8);
            }));
        } else {
            bar_row = bar_row.child(el());
        }
        let keys = match tab {
            Tab::Options => a.schema.as_ref().map(form::schema_keys).unwrap_or_default(),
            Tab::About => Vec::new(),
            other => bar::reset_keys(other.slug()),
        };
        if !keys.is_empty() {
            let id = format!("{}:{}", a.id(), tab.slug());
            let sure = s.settings.confirming(&id);
            let file = a.manifest.config.clone();
            bar_row = bar_row.child(button(t, "restore".into(), Some("restart"), if sure { "Press again to confirm" } else { "Restore defaults" }, sure, move |s: &mut Stage| {
                s.settings.commit();
                s.settings.restore(&id, &file, &keys);
            }));
        }
        strip = Some(bar_row);
    }

    let page_key = format!("pg{index}.{}", tab.map_or(0, |t| t.code()));
    let mut body = col().w_full().gap(14.0).px(28.0).pt(18.0).pb(28.0);
    for (i, e) in items.into_iter().enumerate() {
        body = body.child(e.key(format!("{page_key}.{i}")));
    }
    let body = rise(t, fx, body);
    let mut right = col().grow(1.0).min_w(0.0).h_full().child(head);
    if let Some(strip) = strip {
        right = right.child(strip);
    }
    // A failed save stays in view whatever the page is scrolled to.
    if let Some(e) = s.settings.error() {
        right = right.child(col().w_full().px(28.0).pt(14.0).child(ui::notice(t, "That didn't save", e, true).key("save-error")));
    }
    right = right.child(col().key("page-scroll").w_full().grow(1.0).min_h(0.0).scroll(ScrollSpec { stick_bottom: false }).child(body.key(page_key)));

    row().w(W).h(H).radius(t.radii.panel).fill(t.surf).border(1.0, t.rule).clip_rounded(t.radii.panel)
        .shadow(24.0, 64.0, Color { r: 0.0, g: 0.0, b: 0.0, a: 0.45 })
        .child(sidebar(s, t, index, &apps))
        .child(right)
}
