//! The settings pages other than Appearance. The file-backed ones show the file as it is on disk,
//! since the shell reloads it on every save; Displays, Motion and About show live state.

use rune::{El, col, row};
use yozora_config::paths;
use yozora_style::{Entrance, Fx, Look, Motion, Tokens};

use crate::kit::{fig, label};
use crate::stage::Stage;

fn heading(t: &Tokens, page: &str, line: &str) -> El {
    col().gap(6.0).child(label(t, 22.0, 600.0, page, t.ink)).child(label(t, 13.0, 400.0, line, t.ink2))
}

fn card(t: &Tokens) -> El {
    col().w_full().padding(16.0).gap(8.0).radius(t.radii.card).fill(t.fill)
}

fn pair(t: &Tokens, k: &str, v: String) -> El {
    row().w_full().h(28.0).items_center().justify_between()
        .child(label(t, 13.0, 500.0, k, t.ink2))
        .child(fig(t, 13.0, 500.0, v, t.ink))
}

/// A config file shown as plain text: comments and all, so what is on screen is what is on disk.
fn file(t: &Tokens, page: &str, line: &str, name: &str) -> El {
    let path = paths::config_dir().join(name);
    let text = std::fs::read_to_string(&path).ok().filter(|s| !s.trim().is_empty());
    let mut body = card(t);
    match text {
        Some(text) => {
            for l in text.lines().take(18) {
                let shown = if l.is_empty() {
                    " ".to_owned()
                } else if l.chars().count() > 72 {
                    format!("{}\u{2026}", l.chars().take(71).collect::<String>())
                } else {
                    l.to_owned()
                };
                body = body.child(fig(t, 12.0, 400.0, shown, t.ink));
            }
            if text.lines().count() > 18 {
                body = body.child(label(t, 12.0, 400.0, "\u{2026}", t.ink2));
            }
        }
        None => body = body.child(label(t, 13.0, 400.0, format!("No {name} yet, so the defaults apply."), t.ink2)),
    }
    col().gap(18.0).child(heading(t, page, line)).child(body)
        .child(label(t, 12.0, 400.0, format!("{}  \u{b7}  saved changes apply at once", path.display()), t.ink2))
}

fn displays(s: &Stage, t: &Tokens) -> El {
    let mut body = card(t);
    for name in s.sh.outputs() {
        body = body.child(pair(t, &name, "bar and panels".into()));
    }
    col().gap(18.0).child(heading(t, "Displays", "The bar follows each connected display."))
        .child(body)
        .child(label(t, 12.0, 400.0, "Choose which displays carry the bar with monitors in bar.toml.", t.ink2))
}

fn motion(s: &Stage, t: &Tokens) -> El {
    let fx = s.sh.look().1;
    let name = match fx { Fx::Full => "Full", Fx::Calm => "Calm", Fx::Still => "Still" };
    let line = match fx {
        Fx::Full => "Every entrance, spring and ambient layer plays.",
        Fx::Calm => "Entrances play without overshoot; the ambient layer is off.",
        Fx::Still => "Panels appear at once. Nothing moves on its own.",
    };
    let mut body = card(t);
    for look in Look::ALL {
        let m = Motion::of(look, fx);
        let how = match m.entrance {
            Entrance::Bud => "buds from the icon",
            Entrance::Unfold => "unfolds from the rule",
            Entrance::Scan => "scans in",
            Entrance::Iris => "opens like an iris",
        };
        body = body.child(pair(t, look.name(), format!("{how} \u{b7} {:.2}s", m.open)));
    }
    col().gap(18.0).child(heading(t, "Motion", &format!("Effects are {name}. {line}"))).child(body)
        .child(label(t, 12.0, 400.0, "Change effects on the Appearance page, or with yozora fx set.", t.ink2))
}

fn about(s: &Stage, t: &Tokens) -> El {
    let (look, fx) = s.sh.look();
    let mut body = card(t)
        .child(pair(t, "Version", env!("CARGO_PKG_VERSION").into()))
        .child(pair(t, "Style", look.name().into()))
        .child(pair(t, "Effects", fx.slug().into()))
        .child(pair(t, "Config", paths::config_dir().display().to_string()));
    body = body.child(pair(t, "Sockets", paths::runtime_dir().display().to_string()));
    col().gap(18.0).child(heading(t, "About Yozora", "A Hyprland desktop shell, drawn on Rune.")).child(body)
}

pub fn view(s: &Stage, t: &Tokens, page: &str) -> El {
    match page {
        "Bar" => file(t, page, "Height, edges and the modules on each side.", "bar.toml"),
        "Notifications" => file(t, page, "How long toasts stay and how many show.", "notify.toml"),
        "Launcher" => file(t, page, "What the launcher searches and opens.", "launch.toml"),
        "Lock and idle" => file(t, page, "The lock screen and when the session locks.", "lock.toml"),
        "Displays" => displays(s, t),
        "Motion" => motion(s, t),
        _ => about(s, t),
    }
}
