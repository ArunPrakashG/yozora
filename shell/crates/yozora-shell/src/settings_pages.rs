//! The settings pages that show state rather than edit a file: displays, motion, and about.

use rune::{El, col, row};
use yozora_config::paths;
use yozora_style::{Entrance, Fx, Look, Motion, Tokens};

use crate::kit::{fig, label};
use crate::settings_apps::{self, AppEntry, Status};
use crate::settings_ui::{button, card, group, mono, notice, setting};
use crate::stage::Stage;

const REPO: &str = "https://github.com/ArunPrakashG/yozora";

fn pair(t: &Tokens, first: bool, k: &str, v: String) -> El {
    let r = row().w_full().h(44.0).px(18.0).items_center().justify_between()
        .child(label(t, 13.0, 500.0, k, t.ink2))
        .child(fig(t, 13.0, 500.0, v, t.ink).truncate());
    if first { r } else { r.border_sides([1.0, 0.0, 0.0, 0.0], t.rule) }
}

fn rows(t: &Tokens, items: Vec<(String, String)>) -> El {
    let mut c = card(t);
    for (i, (k, v)) in items.into_iter().enumerate() {
        c = c.child(pair(t, i == 0, &k, v));
    }
    c
}

pub fn displays(s: &Stage, t: &Tokens) -> Vec<El> {
    let monitors = settings_apps::monitors().ok().unwrap_or_default();
    let mut names = s.sh.outputs();
    for m in monitors.iter() {
        if !names.contains(&m.name) {
            names.push(m.name.clone());
        }
    }
    let items = names
        .into_iter()
        .map(|name| {
            let mode = monitors.iter().find(|m| m.name == name).map(|m| format!("{} \u{d7} {} \u{b7} {:.0} Hz", m.mode.0, m.mode.1, m.refresh));
            (name, mode.unwrap_or_else(|| "bar and panels".into()))
        })
        .collect();
    vec![
        rows(t, items),
        label(t, 12.0, 400.0, "Choose which displays carry the bar on the Monitors tab of the Bar.", t.ink2).wrap(),
    ]
}

pub fn motion(s: &Stage, t: &Tokens) -> Vec<El> {
    let fx = s.sh.look().1;
    let name = match fx {
        Fx::Full => "Full",
        Fx::Calm => "Calm",
        Fx::Still => "Still",
    };
    let line = match fx {
        Fx::Full => "Every entrance, spring and ambient layer plays.",
        Fx::Calm => "Entrances play without overshoot; the ambient layer is off.",
        Fx::Still => "Panels appear at once. Nothing moves on its own.",
    };
    let items = Look::ALL
        .into_iter()
        .map(|look| {
            let m = Motion::of(look, fx);
            let how = match m.entrance {
                Entrance::Bud => "buds from the icon",
                Entrance::Unfold => "unfolds from the rule",
                Entrance::Scan => "scans in",
                Entrance::Iris => "opens like an iris",
            };
            (look.name().to_owned(), format!("{how} \u{b7} {:.2}s", m.open))
        })
        .collect();
    vec![
        notice(t, &format!("Effects are {name}"), line, false),
        rows(t, items),
        label(t, 12.0, 400.0, "Change effects on the Appearance page, or with yozora fx set.", t.ink2).wrap(),
    ]
}

pub fn about(s: &Stage, t: &Tokens) -> Vec<El> {
    let (look, fx) = s.sh.look();
    let items = vec![
        ("Version".to_owned(), env!("CARGO_PKG_VERSION").to_owned()),
        ("Style".into(), look.name().into()),
        ("Effects".into(), fx.slug().into()),
        ("Config".into(), paths::config_dir().display().to_string()),
        ("Sockets".into(), paths::runtime_dir().display().to_string()),
    ];
    vec![
        rows(t, items),
        row().gap(10.0)
            .child(button(t, "about-source".into(), Some("code"), "Source", true, |_| settings_apps::open(REPO)))
            .child(button(t, "about-docs".into(), Some("file"), "Docs", false, |_| settings_apps::open(&format!("{REPO}/tree/main/docs")))),
    ]
}

/// One app: whether it runs, where its files are, and what it warns about.
pub fn app_about(s: &Stage, t: &Tokens, app: &AppEntry) -> Vec<El> {
    let m = &app.manifest;
    let (status, version, warnings) = match &app.status {
        Status::Running { version, warnings } => ("Running", version.clone(), warnings.clone()),
        Status::Stopped => ("Not running", None, Vec::new()),
        Status::NoSocket => ("No control socket, so Settings can't tell", None, Vec::new()),
        Status::Unknown => ("Checking\u{2026}", None, Vec::new()),
    };
    let start = if app.status == Status::Stopped && app.startable() {
        let manifest = m.clone();
        button(t, "about-start".into(), Some("power"), "Start", true, move |s: &mut Stage| {
            if let Err(e) = settings_apps::start(&manifest) {
                s.settings.fail(format!("{e:#}"));
            }
        })
    } else {
        row()
    };
    let _ = s;
    let mut info = card(t)
        .child(setting(t, true, "Status", Some(status.into()), None, start))
        .child(setting(t, false, "Version", Some(version.unwrap_or_else(|| "Not reported".into())), None, row()));
    if let Some(socket) = m.socket_path() {
        info = info.child(setting(t, false, "Control socket", Some(socket.display().to_string()), None, row()));
    }
    info = info
        .child(setting(t, false, "Configuration", Some(m.config_path().display().to_string()), None, row()))
        .child(setting(t, false, "Manifest", Some(m.dir.join(format!("{}.toml", m.id)).display().to_string()), None, row()));
    let head = card(t).padding(20.0).gap(6.0)
        .child(label(t, 20.0, 600.0, m.name.clone(), t.ink))
        .child(label(t, 13.0, 400.0, m.description.clone().unwrap_or_default(), t.ink2).wrap())
        .child(mono(t, format!("{} \u{b7} {}", m.id, m.binary), t.ink3));
    let mut out = vec![head, group(t, "Running", None), info];
    if !warnings.is_empty() {
        out.push(group(t, "Configuration warnings", None));
        let mut c = col().w_full().gap(8.0);
        for w in warnings {
            c = c.child(notice(t, "Warning", w, true));
        }
        out.push(c);
    }
    out
}
