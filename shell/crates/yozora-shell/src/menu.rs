//! The Yozora menu the bar's mark opens: settings, reload and the session actions, with a
//! confirmation for the ones that end the session. Also the small state the bar keeps on the
//! stage: the tooltip, the menu's question and a module popover's pending action.

use std::sync::Arc;

use rune::{Color, El, col, row};
use yozora_services::run_shell;
use yozora_style::{Look, Tokens};

use crate::kit::{cell, glyph_el, label};
use crate::module_pop::Pending;
use crate::shared::Shared;
use crate::stage::{Pop, Stage};
use crate::tooltip::Tip;
use crate::wifi::{head, sep, shell, text_el};

pub const W: f32 = 290.0;

/// What the bar keeps on the stage between frames.
#[derive(Default)]
pub struct BarUi {
    pub tip: Option<Tip>,
    /// The session action waiting for a yes.
    pub confirm: Option<Session>,
    /// The `module/<name>` whose popover is open.
    pub module: String,
    /// A module action that asked to be confirmed.
    pub pending: Option<Pending>,
    /// A module slider being dragged: its node number and value.
    pub slide: Option<(usize, f32)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Session {
    Logout,
    Reboot,
    Poweroff,
}

impl Session {
    fn question(self) -> &'static str {
        match self {
            Session::Logout => "Log out now?",
            Session::Reboot => "Restart now?",
            Session::Poweroff => "Shut down now?",
        }
    }

    fn verb(self) -> &'static str {
        match self {
            Session::Logout => "Log out",
            Session::Reboot => "Restart",
            Session::Poweroff => "Shut down",
        }
    }

    fn command(self, sh: &Shared) -> String {
        let s = sh.bar_conf().bar.session.clone();
        match self {
            Session::Logout => s.logout,
            Session::Reboot => s.reboot,
            Session::Poweroff => s.poweroff,
        }
    }
}

const DEFAULT_LOCK: &str = "loginctl lock-session";

/// The command that locks the screen: the configured one when it was changed, otherwise the lock
/// screen this shell ships.
pub fn lock_command(sh: &Shared) -> String {
    let configured = sh.bar_conf().bar.session.lock.clone();
    if configured != DEFAULT_LOCK {
        return configured;
    }
    let beside = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.join("yozora-lock"))).filter(|p| p.is_file());
    let on_path = std::env::var_os("PATH").and_then(|paths| std::env::split_paths(&paths).map(|d| d.join("yozora-lock")).find(|p| p.is_file()));
    match beside.or(on_path) {
        Some(p) => format!("'{}'", p.display().to_string().replace('\'', r"'\''")),
        None => "yozora lock".into(),
    }
}

/// A row of the menu: a name, and the shortcut that does the same where there is one.
fn entry(t: &Tokens, key: &'static str, name: &str, hint: Option<&str>, danger: bool, f: impl Fn(&mut Stage) + 'static) -> El {
    let color = if danger { t.alert } else { t.ink };
    let mut r = row().w_full().h(36.0).px(12.0).gap(8.0).center_y().radius(t.radii.ctl)
        .child(label(t, 14.0, 500.0, name, color).grow(1.0));
    if let Some(h) = hint {
        r = r.child(match t.look {
            Look::Folio | Look::Phosphor => text_el(t, 11.0, 400.0, h.to_owned(), 0.4, true),
            _ => label(t, 12.0, 400.0, h, t.ink3),
        });
    }
    r.key(key).role("button").label(name.to_owned()).hit().hover(t.raised).on_press(f)
}

/// A pill-shaped button for a question: `filled` is the one that does the thing.
pub(crate) fn pill(t: &Tokens, key: impl Into<String>, name: &str, filled: bool, danger: bool, f: impl Fn(&mut Stage) + 'static) -> El {
    let accent = if danger { t.alert } else { t.ink };
    let (bg, fg) = match (t.look, filled) {
        (Look::Folio | Look::Phosphor, true) => (Some(accent), if danger { Color { r: 1.0, g: 1.0, b: 1.0, a: 1.0 } } else { t.on_ink }),
        (Look::Folio | Look::Phosphor, false) => (None, t.ink),
        (_, true) => (Some(accent), if danger { Color { r: 1.0, g: 1.0, b: 1.0, a: 1.0 } } else { t.on_ink }),
        (_, false) => (Some(t.raised), t.ink),
    };
    let words = match t.look {
        Look::Folio => text_el(t, 11.0, 500.0, name.to_uppercase(), 0.9, false).color(fg),
        _ => label(t, 13.0, 500.0, name, fg),
    };
    let mut b = row().h(36.0).px(16.0).grow(1.0).center().radius(t.radii.pill).child(words);
    b = match bg {
        Some(c) => b.fill(c),
        None => b.border(1.0, t.ink),
    };
    if matches!(t.look, Look::Folio | Look::Phosphor) && filled {
        b = b.border(1.0, accent);
    }
    b.key(key).role("button").label(name.to_owned()).hit().on_press(f)
}

fn open_settings(s: &mut Stage, page: &str) {
    s.bar.confirm = None;
    if s.open != Some(Pop::Settings) {
        s.toggle(Pop::Settings, 0.0);
    }
    if let Some(p) = crate::settings::page_named(page) {
        s.settings_page = p;
    }
}

fn reload(s: &mut Stage) {
    let sh: Arc<Shared> = s.sh.clone();
    s.open = None;
    // Applying starts and stops services, which can wait on the system; keep it off the UI thread.
    let _ = std::thread::Builder::new().name("bar-reload".into()).spawn(move || {
        sh.set_config(crate::config::load());
        crate::bar_config::apply(&sh);
    });
}

fn run(s: &mut Stage, command: String) {
    run_shell(&command);
    s.open = None;
}

fn confirm(s: &Stage, t: &Tokens, what: Session) -> El {
    let _ = s;
    let words = col().w_full().px(8.0).pb(6.0)
        .child(label(t, 13.0, 400.0, "Unsaved work in open apps may be lost.", t.ink2));
    let buttons = row().w_full().gap(10.0).pt(6.0)
        .child(pill(t, "menu-cancel", "Cancel", false, false, |s| s.bar.confirm = None))
        .child(pill(t, "menu-confirm", what.verb(), true, true, move |s| {
            let command = what.command(&s.sh);
            run(s, command);
        }));
    shell(t, W, "yozora").child(head(t, what.question(), None)).child(words).child(buttons)
}

pub fn view(s: &Stage, t: &Tokens) -> El {
    if let Some(what) = s.bar.confirm {
        return confirm(s, t, what);
    }
    let mark = cell(36.0, 36.0, glyph_el(t, "moon", 20.0, if matches!(t.look, Look::Folio | Look::Phosphor) { t.on_ink } else { t.acc }))
        .radius(t.radii.ctl);
    let mark = if matches!(t.look, Look::Folio | Look::Phosphor) { mark.fill(t.ink) } else { mark.fill(t.fill) };
    let who = col().gap(1.0)
        .child(label(t, 15.0, 600.0, "Yozora", t.ink))
        .child(label(t, 12.0, 400.0, format!("Bar {}", env!("CARGO_PKG_VERSION")), t.ink2));
    let top = row().w_full().h(52.0).px(8.0).gap(12.0).center_y().child(mark).child(who);

    shell(t, W, "yozora")
        .child(top)
        .child(sep(t))
        .child(entry(t, "menu-about", "About Yozora", None, false, |s| open_settings(s, "about")))
        .child(entry(t, "menu-bar", "Bar settings\u{2026}", Some("Super ,"), false, |s| open_settings(s, "bar")))
        .child(entry(t, "menu-reload", "Reload bar", Some("Super \u{21e7} R"), false, reload))
        .child(sep(t))
        .child(entry(t, "menu-lock", "Lock screen", Some("Super L"), false, |s| {
            let command = lock_command(&s.sh);
            run(s, command);
        }))
        .child(entry(t, "menu-logout", "Log out\u{2026}", None, false, |s| s.bar.confirm = Some(Session::Logout)))
        .child(sep(t))
        .child(entry(t, "menu-reboot", "Restart\u{2026}", None, false, |s| s.bar.confirm = Some(Session::Reboot)))
        .child(entry(t, "menu-poweroff", "Shut down\u{2026}", None, true, |s| s.bar.confirm = Some(Session::Poweroff)))
}
