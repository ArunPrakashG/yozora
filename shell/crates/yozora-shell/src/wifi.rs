//! The Wi-Fi popover: the radio switch, the network you are on, the others in range, and VPNs.
//! It also holds the small list pieces (switch, row, section label) the Bluetooth and control
//! center screens use, since all three share one grammar.

use rune::tok::TextStyle;
use rune::{Color, El, col, el, row, text};
use yozora_services::network::{Cmd, State};
use yozora_style::{Look, Tokens};

use crate::kit::{cell, glyph_el, label, panel};
use crate::stage::Stage;

pub const W: f32 = 340.0;

/// The panel with the look's own chrome: Phosphor sits in a frame with its name cut into the top
/// edge.
pub(crate) fn shell(t: &Tokens, w: f32, tag: &str) -> El {
    let p = panel(t, w);
    if t.look != Look::Phosphor {
        return p;
    }
    let cut = el().pin(Some(18.0), Some(-7.0), None, None).px(6.0)
        .fill(Color { a: 1.0, ..t.surf })
        .child(label(t, 12.0, 600.0, tag.to_lowercase(), t.ink));
    p.pt(24.0).px(20.0).pb(20.0).child(cut)
}

/// A panel title: roman, italic serif or a shell prompt, per look.
pub(crate) fn title(t: &Tokens, s: &str) -> El {
    match t.look {
        Look::Folio => text(s.to_owned())
            .font(TextStyle { italic: true, tracking: -0.4, ..TextStyle::new(26.0, 500.0).family(t.fonts.body) })
            .color(t.ink),
        Look::Phosphor => label(t, 15.0, 600.0, format!("> {}", s.to_lowercase()), t.ink),
        Look::Prism => text(s.to_owned())
            .font(TextStyle { tracking: -0.5, ..TextStyle::new(22.0, 600.0).family(t.fonts.body) })
            .color(t.ink),
        _ => label(t, 22.0, 600.0, s, t.ink),
    }
}

/// The title row, with an optional control on the right.
pub(crate) fn head(t: &Tokens, s: &str, right: Option<El>) -> El {
    let r = row().w_full().h(48.0).center_y().pl(8.0).pr(2.0).mb(2.0).child(title(t, s).grow(1.0));
    match right {
        Some(e) => r.child(e),
        None => r,
    }
}

/// A switch, drawn only. The caller makes it pressable.
pub(crate) fn switch(t: &Tokens, on: bool) -> El {
    let square = matches!(t.look, Look::Folio | Look::Phosphor);
    let round = if square { 0.0 } else { 9.0 };
    let (track, knob) = match t.look {
        Look::Folio => (if on { el().fill(t.acc) } else { el().border(1.0, t.ink) }, el().fill(t.ink)),
        Look::Phosphor => (el().border(1.0, t.ink), el().fill(if on { t.ink } else { t.ink3 })),
        Look::Prism => (
            el().fill(if on { t.acc } else { Color { a: 0.12, ..t.ink } }),
            el().fill(t.on_ink).shadow(1.0, 3.0, Color { a: 0.2, ..t.ink }),
        ),
        _ => (el().fill(if on { t.acc } else { t.fill }), el().fill(if on { t.on_acc } else { t.ink })),
    };
    let inner = row().size(40.0, 24.0).px(3.0).center_y();
    let inner = if on { inner.justify_end() } else { inner };
    track.size(40.0, 24.0).radius(if square { 0.0 } else { 12.0 }).child(inner.child(knob.size(18.0, 18.0).radius(round)))
}

/// A switch with a hit area around it.
pub(crate) fn switch_button(t: &Tokens, key: &'static str, name: &str, on: bool, f: impl Fn(&mut Stage) + 'static) -> El {
    cell(44.0, 40.0, switch(t, on)).key(key).role("switch").label(name).hit().on_press(f)
}

/// Text and icon colours on a row: a selected Phosphor row is inverted.
pub(crate) fn ink(t: &Tokens, selected: bool) -> (Color, Color) {
    if selected && t.look == Look::Phosphor {
        (t.on_ink, Color { a: 0.7, ..t.on_ink })
    } else {
        (t.ink, t.ink2)
    }
}

/// A list row: a lead, a title with an optional second line, and an optional trail. The caller
/// makes it pressable.
pub(crate) fn item(t: &Tokens, selected: bool, lead: El, name: &str, sub: Option<&str>, trail: Option<El>) -> El {
    let (main, dim) = ink(t, selected);
    let mut body = col().grow(1.0).gap(2.0).child(label(t, 14.0, 500.0, name, main).truncate());
    if let Some(s) = sub {
        body = body.child(label(t, 12.0, 400.0, s, dim).truncate());
    }
    let mut r = row().w_full().min_h(50.0).px(14.0).gap(14.0).center_y().radius(t.radii.ctl)
        .child(lead)
        .child(body);
    if let Some(e) = trail {
        r = r.child(e);
    }
    if selected {
        r = match t.look {
            Look::Folio => r.border_sides([0.0, 0.0, 0.0, 3.0], t.acc),
            Look::Phosphor => r.fill(t.ink),
            Look::Prism => r.fill(t.fill).shadow(1.0, 4.0, Color { a: 0.12, ..t.ink }),
            _ => r.fill(t.fill),
        };
    } else {
        r = r.hover(t.raised);
    }
    r
}

/// A lead icon in a fixed cell.
pub(crate) fn lead(t: &Tokens, name: &str, color: Color) -> El {
    cell(18.0, 18.0, glyph_el(t, name, 18.0, color))
}

/// A small heading between groups, with an optional control on its right.
pub(crate) fn section(t: &Tokens, s: &str, right: Option<El>) -> El {
    let text = match t.look {
        Look::Folio => text_el(t, 10.5, 500.0, s.to_uppercase(), 1.5, true),
        Look::Phosphor => label(t, 12.0, 400.0, format!("# {}", s.to_lowercase()), t.ink2),
        _ => label(t, 12.0, 500.0, s, t.ink2),
    };
    let mut r = row().w_full().px(10.0).pt(12.0).pb(4.0).gap(8.0).center_y().child(text.grow(1.0));
    if let Some(e) = right {
        r = r.child(e);
    }
    if t.look == Look::Folio {
        return col().w_full().px(10.0).mt(6.0).child(el().w_full().h(1.0).fill(t.ink)).child(r.px(0.0));
    }
    r
}

/// Mono, tracked text: Folio's small caps lines.
pub(crate) fn text_el(t: &Tokens, size: f32, weight: f32, s: String, tracking: f32, dim: bool) -> El {
    text(s)
        .font(TextStyle { tracking, ..TextStyle::new(size, weight).family(t.fonts.fig) })
        .color(if dim { t.ink2 } else { t.ink })
}

/// A rule between groups.
pub(crate) fn sep(t: &Tokens) -> El {
    let line = match t.look {
        Look::Folio => el().w_full().h(1.0).fill(Color { a: 0.25, ..t.ink }),
        Look::Phosphor => el().w_full().h(1.0).border(1.0, t.rule).border_dash(3.0, 3.0),
        _ => el().w_full().h(1.0).fill(t.rule),
    };
    col().w_full().px(12.0).my(6.0).child(line)
}

/// Centred lines for a state with nothing to list.
pub(crate) fn empty(t: &Tokens, lines: &[&str]) -> El {
    let mut c = col().w_full().px(16.0).py(14.0).gap(4.0).items_center();
    for (i, l) in lines.iter().enumerate() {
        c = c.child(label(t, 13.0, if i == 0 { 500.0 } else { 400.0 }, *l, if i == 0 { t.ink } else { t.ink2 }));
    }
    c
}

/// Whether `command` is an executable on PATH. Remembered, since a screen asks on every draw.
pub(crate) fn on_path(command: &str) -> bool {
    static SEEN: std::sync::Mutex<Vec<(String, bool)>> = std::sync::Mutex::new(Vec::new());
    let mut seen = SEEN.lock().unwrap();
    if let Some((_, found)) = seen.iter().find(|(c, _)| c == command) {
        return *found;
    }
    let found = std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(command).is_file()));
    seen.push((command.to_owned(), found));
    found
}

/// A row at the foot of a panel that opens the first of `tools` that is installed. Nothing when
/// none is.
pub(crate) fn tool_row(t: &Tokens, key: &'static str, name: &str, tools: &[&'static str]) -> Option<El> {
    let tool = *tools.iter().find(|c| on_path(c))?;
    Some(
        item(t, false, lead(t, "gear", t.ink2), name, None, None)
            .key(key).role("button").label(name.to_owned()).hit()
            .on_press(move |s: &mut Stage| {
                yozora_services::run_shell(tool);
                s.open = None;
            }),
    )
}

/// A signal-strength icon: strong networks draw full, weak ones small and faint.
fn signal(t: &Tokens, strength: u8, selected: bool) -> El {
    let (main, _) = ink(t, selected);
    if strength >= 50 {
        lead(t, "wifi", main)
    } else {
        lead(t, "wifi2", Color { a: main.a * 0.45, ..main })
    }
}

fn send(s: &Stage, cmd: Cmd) {
    if let Some(n) = &s.sh.services.lock().unwrap().network {
        n.send(cmd);
    }
}

pub fn view(s: &Stage, t: &Tokens) -> El {
    let (state, error) = match s.sh.services.lock().unwrap().network.as_ref() {
        Some(n) => {
            let snap = n.read();
            (snap.state.clone(), snap.error.clone())
        }
        None => (State::default(), Some("no service".into())),
    };
    let mut p = shell(t, W, "wi-fi");

    if let Some(e) = error {
        return p.child(head(t, "Network", None)).child(empty(t, &["NetworkManager is not available.", &e]));
    }

    if state.wifi_device {
        let on = state.wifi_enabled;
        p = p.child(head(t, "Wi-Fi", Some(switch_button(t, "wifi-toggle", "Wi-Fi", on, move |s| send(s, Cmd::SetWifi(!on))))));
        if on {
            let (active, others): (Vec<_>, Vec<_>) = state.access_points.iter().partition(|a| a.active);
            for ap in active {
                let sub = if ap.secured && !ap.security.is_empty() { format!("Connected \u{b7} {}", ap.security) } else { "Connected".into() };
                let (main, _) = ink(t, true);
                p = p.child(
                    item(t, true, signal(t, ap.strength, true), &ap.ssid, Some(&sub), Some(glyph_el(t, "check", 16.0, main)))
                        .key("net-active").role("button").label(format!("Disconnect from {}", ap.ssid)).hit()
                        .on_press(|s: &mut Stage| send(s, Cmd::Disconnect)),
                );
            }
            {
                let rescan = cell(28.0, 28.0, glyph_el(t, "sync", 15.0, t.ink2)).key("rescan").role("button").label("Scan for networks").hit()
                    .radius(t.radii.pill).on_press(|s: &mut Stage| send(s, Cmd::Scan));
                p = p.child(section(t, "Other networks", Some(rescan)));
            }
            for (i, ap) in others.into_iter().take(8).enumerate() {
                let ssid = ap.ssid.clone();
                let sub = if state.activating.as_deref() == Some(ap.ssid.as_str()) {
                    Some("Connecting\u{2026}")
                } else if ap.known {
                    Some("Saved")
                } else if !ap.secured {
                    Some("Open network")
                } else if !ap.security.is_empty() {
                    Some(ap.security.as_str())
                } else {
                    None
                };
                let trail = ap.secured.then(|| glyph_el(t, "lock", 14.0, t.ink2));
                p = p.child(
                    item(t, false, signal(t, ap.strength, false), &ap.ssid, sub, trail)
                        .key(format!("ap{i}")).role("button").label(format!("Connect to {}", ap.ssid)).hit()
                        .on_press(move |s: &mut Stage| send(s, Cmd::Connect(ssid.clone()))),
                );
            }
            if state.access_points.is_empty() {
                p = p.child(empty(t, &["Looking for networks\u{2026}"]));
            } else if state.access_points.iter().all(|a| a.active) {
                p = p.child(empty(t, &["No other networks in range."]));
            }
        } else {
            p = p.child(empty(t, &["Wi-Fi is off.", "Wired connections stay up."]));
        }
    }

    if let Some(w) = &state.wired {
        if state.wifi_device {
            p = p.child(sep(t));
        } else {
            p = p.child(head(t, "Network", None));
        }
        let sub = if w.connected { format!("Connected \u{b7} {}", w.interface) } else { "Cable unplugged".into() };
        let c = if w.connected { t.ink } else { t.ink3 };
        p = p.child(item(t, false, lead(t, "globe", c), "Ethernet", Some(&sub), None));
    }
    if !state.wifi_device && state.wired.is_none() {
        p = p.child(head(t, "Network", None)).child(empty(t, &["No network devices."]));
    }

    if !state.vpns.is_empty() {
        p = p.child(sep(t));
        for (i, vpn) in state.vpns.iter().enumerate() {
            let (uuid, on) = (vpn.uuid.clone(), vpn.active);
            let sub = if vpn.connecting { "Connecting\u{2026}" } else if on { "Connected" } else { "VPN" };
            p = p.child(
                item(t, false, lead(t, "shield", if on { t.ink } else { t.ink2 }), &vpn.name, Some(sub), Some(switch(t, on || vpn.connecting)))
                    .key(format!("vpn{i}")).role("switch").label(format!("{} VPN", vpn.name)).hit()
                    .on_press(move |s: &mut Stage| send(s, Cmd::SetVpn(uuid.clone(), !on))),
            );
        }
    }
    if let Some(row) = tool_row(t, "net-settings", "Network settings\u{2026}", &["nm-connection-editor"]) {
        p = p.child(sep(t)).child(row);
    }
    p
}
