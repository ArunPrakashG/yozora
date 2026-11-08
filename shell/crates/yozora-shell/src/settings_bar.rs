//! The bar's own pages: its style and layout, the modules on each side, the monitors it appears
//! on, and the custom modules a person writes. All of it edits `bar.toml` in place.

use rune::{El, col, el, row};
use yozora_config::bar::{BarConfig, BarStyle, Zones};
use yozora_config::schema::{Field, FieldKind};
use yozora_style::Tokens;

use crate::kit::{glyph_el, label};
use crate::settings_apps::{self, AppEntry, Monitor};
use crate::settings_doc::string_array;
use crate::settings_form::{Input, field_row, text_field};
use crate::settings_state::Target;
use crate::settings_ui::{button, card, group, mono, notice, segmented, setting, toggle, tool};
use crate::stage::Stage;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Zone {
    Left,
    Center,
    Right,
}

impl Zone {
    const ALL: [Zone; 3] = [Zone::Left, Zone::Center, Zone::Right];

    fn key(self) -> &'static str {
        match self {
            Zone::Left => "modules.left",
            Zone::Center => "modules.center",
            Zone::Right => "modules.right",
        }
    }

    fn name(self) -> &'static str {
        match self {
            Zone::Left => "Left",
            Zone::Center => "Center",
            Zone::Right => "Right",
        }
    }

    fn initial(self) -> &'static str {
        &self.name()[..1]
    }

    fn list(self, zones: &mut Zones) -> &mut Vec<String> {
        match self {
            Zone::Left => &mut zones.left,
            Zone::Center => &mut zones.center,
            Zone::Right => &mut zones.right,
        }
    }
}

/// Every module the bar ships: key, name, what it shows, glyph, and the side it starts on.
const CATALOG: &[(&str, &str, &str, &str, Zone)] = &[
    ("launcher", "Menu", "About, settings, session", "star", Zone::Left),
    ("workspaces", "Workspaces", "Hyprland workspaces", "monitor", Zone::Left),
    ("window", "Window title", "The focused window", "window", Zone::Left),
    ("tray", "Tray", "StatusNotifier icons", "package", Zone::Right),
    ("media", "Media", "MPRIS players", "music", Zone::Right),
    ("network", "Network", "NetworkManager", "wifi", Zone::Right),
    ("sound", "Sound", "Outputs and inputs", "vol", Zone::Right),
    ("devices", "Bluetooth", "Devices and their batteries", "bt", Zone::Right),
    ("battery", "Battery", "UPower and power profiles", "battery", Zone::Right),
    ("notifications", "Notifications", "History, Do Not Disturb", "bell", Zone::Right),
    ("clock", "Clock", "Time and calendar", "clock", Zone::Right),
    ("control-center", "Control center", "Quick settings", "cc", Zone::Right),
    ("module/sudo-toggle", "Passwordless sudo", "A lock that switches sudo's password prompt", "lock", Zone::Right),
];

const MINIMAL: (&[&str], &[&str]) = (&["workspaces"], &["clock", "control-center"]);
const STANDARD: (&[&str], &[&str]) = (&["launcher", "workspaces", "window"], &["media", "network", "sound", "clock", "control-center"]);
const FULL: (&[&str], &[&str]) = (
    &["launcher", "workspaces", "window"],
    &["tray", "media", "network", "sound", "devices", "battery", "notifications", "clock", "control-center"],
);
const PRESETS: [(&[&str], &[&str]); 3] = [MINIMAL, STANDARD, FULL];
const HEIGHTS: [u32; 3] = [30, 34, 38];

struct Info {
    label: String,
    detail: String,
    glyph: &'static str,
    home: Zone,
}

fn info(name: &str) -> Info {
    if let Some(&(_, label, detail, glyph, home)) = CATALOG.iter().find(|c| c.0 == name) {
        return Info { label: label.into(), detail: detail.into(), glyph, home };
    }
    if let Some(custom) = name.strip_prefix("custom/") {
        return Info { label: custom.into(), detail: "Custom command".into(), glyph: "term", home: Zone::Right };
    }
    Info { label: name.into(), detail: "Unknown module; the bar skips it".into(), glyph: "alert", home: Zone::Right }
}

/// Puts `name` in `zone`, before `before` or at the end, taking it out of wherever it was.
fn place(zones: &mut Zones, name: &str, zone: Zone, before: Option<&str>) {
    for z in Zone::ALL {
        z.list(zones).retain(|n| n != name);
    }
    let list = zone.list(zones);
    let at = before.and_then(|b| list.iter().position(|n| n == b)).unwrap_or(list.len());
    list.insert(at, name.to_owned());
}

fn save_zones(s: &Stage, file: &str, zones: &Zones) {
    s.settings.edit(file, |f| {
        f.set(Zone::Left.key(), string_array(&zones.left));
        f.set(Zone::Center.key(), string_array(&zones.center));
        f.set(Zone::Right.key(), string_array(&zones.right));
    });
}

/// A drawn miniature of a bar style: a screen with the bar across its top.
fn bar_preview(t: &Tokens, style: BarStyle) -> El {
    let screen = el().w_full().h(96.0).radius(t.radii.card).fill(t.bg).clip_rounded(t.radii.card);
    let bar = match style {
        BarStyle::Flush => el().pin(Some(0.0), Some(0.0), Some(0.0), None).h(16.0).fill(t.surf).border_sides([0.0, 0.0, 0.0, 1.0], t.rule),
        BarStyle::Floating => el().pin(Some(14.0), Some(10.0), Some(14.0), None).h(16.0).radius(t.radii.pill.min(8.0)).fill(t.surf).border(1.0, t.rule),
    };
    let dots = row().pin(Some(26.0), Some(style_dot_top(style)), None, None).gap(6.0)
        .child(el().size(18.0, 4.0).radius(2.0).fill(t.acc))
        .child(el().size(26.0, 4.0).radius(2.0).fill(t.ink3));
    let window = el().pin(Some(34.0), Some(44.0), Some(34.0), Some(0.0)).radius(t.radii.chip.min(8.0)).fill(t.fill).border(1.0, t.rule);
    screen.child(window).child(bar).child(dots)
}

fn style_dot_top(style: BarStyle) -> f32 {
    if style == BarStyle::Flush { 6.0 } else { 16.0 }
}

fn style_card(t: &Tokens, file: &str, current: BarStyle, style: BarStyle, value: &'static str, title: &str, detail: &str) -> El {
    let on = current == style;
    let file = file.to_owned();
    let c = col().w(366.0).gap(12.0).padding(8.0).radius(t.radii.card).fill(t.raised).key(format!("bar-style-{value}")).role("button").label(title).hit()
        .child(bar_preview(t, style))
        .child(row().px(8.0).pb(4.0).gap(10.0).items_center().child(label(t, 15.0, 600.0, title, t.ink)).child(label(t, 13.0, 400.0, detail, t.ink2)))
        .on_press(move |s: &mut Stage| s.settings.set(&file, "bar.style", value));
    if on { c.border(2.0, t.acc) } else { c }
}

fn monitors_summary(config: &BarConfig) -> String {
    match settings_apps::monitors() {
        Ok(list) => {
            let on: Vec<&str> = list.iter().filter(|m| config.shows_on(&m.name)).map(|m| m.name.as_str()).collect();
            match on.len() {
                0 => "Hidden on every monitor".into(),
                n if n == list.len() => format!("On every monitor: {}", on.join(", ")),
                _ => format!("On {}", on.join(", ")),
            }
        }
        Err(_) if config.bar.monitors.is_empty() => "On every monitor".into(),
        Err(_) => format!("On {}", config.bar.monitors.join(", ")),
    }
}

/// Style: how the bar is drawn and the space it keeps.
pub fn style_page(s: &Stage, t: &Tokens, app: &AppEntry, go_monitors: impl Fn(&mut Stage) + 'static) -> Vec<El> {
    let file = app.manifest.config.clone();
    let c = s.settings.bar_config(&file);
    let bar = &c.bar;
    let cards = row().gap(16.0)
        .child(style_card(t, &file, bar.style, BarStyle::Flush, "flush", "Flush", "Edge to edge"))
        .child(style_card(t, &file, bar.style, BarStyle::Floating, "floating", "Floating", "Inset, rounded"));

    let mut labels: Vec<String> = HEIGHTS.iter().map(|h| h.to_string()).collect();
    let index = HEIGHTS.iter().position(|&h| h == bar.height).unwrap_or_else(|| {
        labels.push(bar.height.to_string());
        labels.len() - 1
    });
    let f_height = file.clone();
    let height = segmented(t, "bar-height", &labels, Some(index), 70.0 * labels.len() as f32, move |s, i| {
        if let Some(&h) = HEIGHTS.get(i) {
            s.settings.set(&f_height, "bar.height", h as i64);
        }
    });
    let switch = |key: &'static str, name: &str, field: &'static str, on: bool| {
        let file = file.clone();
        toggle(t, key.into(), name, on, move |s: &mut Stage| s.settings.set(&file, field, !on))
    };
    let layout = card(t)
        .child(setting(t, true, "Height", Some("Icons stay 16 px at every height.".into()), None, height))
        .child(setting(t, false, "Reserve space", Some("Tiled windows stop below the bar instead of under it.".into()), None, switch("bar-reserve", "Reserve space", "bar.reserve_space", bar.reserve_space)))
        .child(setting(t, false, "Blur behind the bar", Some("Asks Hyprland to blur behind the bar. Off makes the tint more opaque instead.".into()), None, switch("bar-blur", "Blur behind the bar", "bar.blur", bar.blur)))
        .child(setting(t, false, "Monitors", Some(monitors_summary(&c)), None, button(t, "bar-manage".into(), None, "Manage", false, go_monitors)));
    vec![group(t, "Style", None), cards, group(t, "Layout", None), layout]
}

// ---- Modules ---------------------------------------------------------------------------------

fn zone_chip(t: &Tokens, file: &str, zones: &Zones, name: &str, zone: Zone, here: bool) -> El {
    let (file, zones, name) = (file.to_owned(), zones.clone(), name.to_owned());
    let (bg, ink) = if here { (t.acc, t.on_acc) } else { (t.raised, t.ink2) };
    let b = row().size(28.0, 28.0).center().radius(t.radii.chip.min(10.0)).fill(bg)
        .key(format!("mod-{}-{}", name, zone.initial())).role("button").label(format!("Move to {}", zone.name()))
        .child(label(t, 11.0, 700.0, zone.initial(), ink));
    if here {
        b
    } else {
        b.hit().on_press(move |s: &mut Stage| {
            let mut z = zones.clone();
            place(&mut z, &name, zone, None);
            save_zones(s, &file, &z);
        })
    }
}

fn module_row(t: &Tokens, file: &str, zones: &Zones, name: &str, zone: Zone, i: usize, len: usize) -> El {
    let inf = info(name);
    let (f1, f2, f3) = (file.to_owned(), file.to_owned(), file.to_owned());
    let (z1, z2, z3) = (zones.clone(), zones.clone(), zones.clone());
    let (n1, n2, n3) = (name.to_owned(), name.to_owned(), name.to_owned());
    let mut chips = row().gap(4.0);
    for z in Zone::ALL {
        chips = chips.child(zone_chip(t, file, zones, name, z, z == zone));
    }
    row().w_full().h(52.0).px(14.0).gap(12.0).items_center()
        .border_sides([if i == 0 { 0.0 } else { 1.0 }, 0.0, 0.0, 0.0], t.rule)
        .child(row().size(30.0, 30.0).center().radius(t.radii.chip.min(10.0)).fill(t.raised).child(glyph_el(t, inf.glyph, 16.0, t.acc)))
        .child(
            col().grow(1.0).min_w(0.0).gap(1.0)
                .child(label(t, 14.0, 500.0, inf.label, t.ink).truncate())
                .child(mono(t, format!("{name} \u{b7} {}", inf.detail), t.ink3).truncate()),
        )
        .child(chips)
        .child(tool(t, format!("mod-{name}-up"), "up", "Move up", i > 0, move |s: &mut Stage| {
            let mut z = z1.clone();
            let list = zone.list(&mut z);
            list.swap(i, i - 1);
            save_zones(s, &f1, &z);
        }))
        .child(tool(t, format!("mod-{name}-down"), "down", "Move down", i + 1 < len, move |s: &mut Stage| {
            let mut z = z2.clone();
            zone.list(&mut z).swap(i, i + 1);
            save_zones(s, &f2, &z);
        }))
        .child(toggle(t, format!("mod-{n3}-sw"), &n1, true, move |s: &mut Stage| {
            let mut z = z3.clone();
            for zn in Zone::ALL {
                zn.list(&mut z).retain(|n| n != &n2);
            }
            save_zones(s, &f3, &z);
        }))
}

fn off_row(t: &Tokens, file: &str, zones: &Zones, name: &str, i: usize) -> El {
    let inf = info(name);
    let (file, zones, n) = (file.to_owned(), zones.clone(), name.to_owned());
    let home = inf.home;
    row().w_full().h(52.0).px(14.0).gap(12.0).items_center()
        .border_sides([if i == 0 { 0.0 } else { 1.0 }, 0.0, 0.0, 0.0], t.rule)
        .child(row().size(30.0, 30.0).center().radius(t.radii.chip.min(10.0)).fill(t.raised).child(glyph_el(t, inf.glyph, 16.0, t.ink3)))
        .child(
            col().grow(1.0).min_w(0.0).gap(1.0)
                .child(label(t, 14.0, 500.0, inf.label, t.ink2).truncate())
                .child(mono(t, format!("{name} \u{b7} {}", inf.detail), t.ink3).truncate()),
        )
        .child(label(t, 12.0, 400.0, format!("Joins {}", home.name().to_lowercase()), t.ink3))
        .child(toggle(t, format!("mod-{name}-sw"), name, false, move |s: &mut Stage| {
            let mut z = zones.clone();
            place(&mut z, &n, home, None);
            save_zones(s, &file, &z);
        }))
}

fn custom_field(key: String, label: &str, help: &str, kind: FieldKind) -> Field {
    Field {
        key,
        kind,
        label: label.into(),
        help: Some(help.into()),
        default: None,
        options: Vec::new(),
        option_labels: Vec::new(),
        min: (kind == FieldKind::Int).then_some(1.0),
        max: (kind == FieldKind::Int).then_some(86400.0),
        step: (kind == FieldKind::Int).then_some(1.0),
        secret: false,
    }
}

/// Modules: what the bar shows and where, and the custom ones.
pub fn modules_page(s: &Stage, t: &Tokens, app: &AppEntry) -> Vec<El> {
    let file = app.manifest.config.clone();
    let config = s.settings.bar_config(&file);
    let zones = config.modules.clone();
    let preset = PRESETS.iter().position(|(l, r)| {
        zones.center.is_empty() && zones.left.iter().map(String::as_str).eq(l.iter().copied()) && zones.right.iter().map(String::as_str).eq(r.iter().copied())
    });
    let f_preset = file.clone();
    let preset_ctl = row().gap(10.0).items_center()
        .child(if preset.is_none() { label(t, 12.0, 500.0, "Custom", t.ink3) } else { el() })
        .child(segmented(t, "mod-preset", &["Minimal".into(), "Standard".into(), "Full".into()], preset, 270.0, move |s, i| {
            let (l, r) = PRESETS[i];
            let z = Zones { left: l.iter().map(|x| x.to_string()).collect(), center: Vec::new(), right: r.iter().map(|x| x.to_string()).collect() };
            save_zones(s, &f_preset, &z);
        }));
    let mut out = vec![card(t).child(setting(t, true, "Preset", Some("A starting point. Change any module below to customise.".into()), None, preset_ctl))];

    let enabled: Vec<String> = zones.left.iter().chain(&zones.center).chain(&zones.right).cloned().collect();
    let customs = s.settings.table_keys(&file, "custom");
    let mut known: Vec<String> = CATALOG.iter().map(|c| c.0.to_owned()).collect();
    known.extend(customs.iter().map(|k| format!("custom/{k}")));

    for zone in Zone::ALL {
        let mut z = zones.clone();
        let on = zone.list(&mut z).clone();
        let count = if on.is_empty() { "empty".to_owned() } else { format!("{} on", on.len()) };
        out.push(
            row().w_full().justify_between().pt(6.0).child(label(t, 12.0, 600.0, zone.name().to_uppercase(), t.ink2)).child(mono(t, count, t.ink3)),
        );
        let mut c = card(t);
        if on.is_empty() {
            c = c.child(row().w_full().h(52.0).px(18.0).items_center().child(label(t, 13.0, 400.0, "Nothing here. Turn a module on below, or move one with the L, C and R buttons.", t.ink3)));
        }
        for (i, name) in on.iter().enumerate() {
            c = c.child(module_row(t, &file, &zones, name, zone, i, on.len()));
        }
        out.push(c);
    }

    let off: Vec<&String> = known.iter().filter(|n| !enabled.contains(n)).collect();
    if !off.is_empty() {
        out.push(group(t, "Available", Some("Switched off. Turning one on adds it to its usual side.")));
        let mut c = card(t);
        for (i, name) in off.iter().enumerate() {
            c = c.child(off_row(t, &file, &zones, name, i));
        }
        out.push(c);
    }

    out.push(group(t, "Custom modules", Some("Any command that prints one JSON object per line can be a module. Turn it on above to show it.")));
    for name in &customs {
        out.push(custom_card(s, t, &file, name));
    }
    let add = text_field(s, Input {
        id: format!("{file}:custom:new"),
        target: Target::NewCustom { file: file.clone() },
        current: String::new(),
        placeholder: "A name, such as load".into(),
        width: 260.0,
        secret: false,
        mono: true,
        t,
    });
    out.push(card(t).child(setting(t, true, "Add a module", Some("Press Enter to create it, then set its command.".into()), None, add)));
    out
}

fn custom_card(s: &Stage, t: &Tokens, file: &str, name: &str) -> El {
    let fields = [
        custom_field(format!("custom.{name}.exec"), "Command", "Runs through sh -c. With no interval it is read as a stream of lines.", FieldKind::String),
        custom_field(format!("custom.{name}.interval"), "Repeat every", "Seconds between runs. Leave it unset to read a stream instead.", FieldKind::Int),
        custom_field(format!("custom.{name}.on_click"), "On click", "A command to run when the module is pressed.", FieldKind::String),
    ];
    let (f, n) = (file.to_owned(), name.to_owned());
    let mut c = card(t).child(
        row().w_full().h(44.0).px(18.0).items_center()
            .child(label(t, 14.0, 600.0, format!("custom/{name}"), t.ink).grow(1.0)),
    );
    for field in &fields {
        c = c.child(field_row(s, t, file, field, false));
    }
    c.child(setting(t, false, "Remove", Some("Deletes this module and takes it off the bar.".into()), None,
        button(t, format!("custom-{name}-rm"), Some("x"), "Remove module", false, move |s: &mut Stage| {
            s.settings.commit();
            let mut zones = s.settings.bar_config(&f).modules;
            for z in Zone::ALL {
                z.list(&mut zones).retain(|m| m != &format!("custom/{n}"));
            }
            s.settings.edit(&f, |file| {
                file.remove(&format!("custom.{n}"));
                file.set(Zone::Left.key(), string_array(&zones.left));
                file.set(Zone::Center.key(), string_array(&zones.center));
                file.set(Zone::Right.key(), string_array(&zones.right));
            });
        })))
}

// ---- Monitors --------------------------------------------------------------------------------

const MINIMAL_LEFT: &[&str] = &["workspaces"];
const MINIMAL_RIGHT: &[&str] = &["clock", "control-center"];

fn monitor_key(name: &str, rest: &str) -> String {
    format!("monitor.\"{name}\".{rest}")
}

/// Monitors: which screens carry a bar and what it shows there.
pub fn monitors_page(s: &Stage, t: &Tokens, app: &AppEntry) -> Vec<El> {
    let file = app.manifest.config.clone();
    let config = s.settings.bar_config(&file);
    let list: Vec<Monitor> = match settings_apps::monitors() {
        Ok(m) if !m.is_empty() => m.to_vec(),
        other => {
            let reason = other.err().unwrap_or_else(|| "Hyprland reports no monitors".into());
            return vec![
                notice(t, "Can't read the monitor layout", reason, true),
                row().child(button(t, "mon-retry".into(), Some("sync"), "Try again", false, |_| settings_apps::invalidate())),
            ];
        }
    };
    let selected = s.settings.monitor().filter(|n| list.iter().any(|m| &m.name == n)).unwrap_or_else(|| list[0].name.clone());
    let monitor = list.iter().find(|m| m.name == selected).cloned().unwrap_or_else(|| list[0].clone());

    // The screens drawn to scale where they sit in the layout.
    let (min_x, min_y) = (list.iter().map(|m| m.x).min().unwrap_or(0), list.iter().map(|m| m.y).min().unwrap_or(0));
    let span_w = (list.iter().map(|m| m.x + m.width).max().unwrap_or(1) - min_x).max(1) as f32;
    let span_h = (list.iter().map(|m| m.y + m.height).max().unwrap_or(1) - min_y).max(1) as f32;
    let (stage_w, stage_h) = (440.0, 150.0);
    let scale = (stage_w / span_w).min(stage_h / span_h);
    let (ox, oy) = ((stage_w - span_w * scale) / 2.0, (stage_h - span_h * scale) / 2.0);
    let mut screens = el().w(stage_w).h(stage_h);
    for m in &list {
        let is_sel = m.name == selected;
        let name = m.name.clone();
        let mut screen = el().pin(Some(ox + (m.x - min_x) as f32 * scale), Some(oy + (m.y - min_y) as f32 * scale), None, None)
            .size(m.width as f32 * scale, m.height as f32 * scale).radius(t.radii.chip.min(8.0)).fill(t.bg).clip_rounded(t.radii.chip.min(8.0))
            .border(2.0, if is_sel { t.acc } else { t.rule }).key(format!("mon-{}", m.name)).role("button").label(m.name.as_str()).hit()
            .on_press(move |s: &mut Stage| s.settings.select_monitor(&name));
        if config.shows_on(&m.name) {
            screen = screen.child(el().pin(Some(0.0), Some(0.0), Some(0.0), None).h(8.0).fill(t.surf).border_sides([0.0, 0.0, 0.0, 1.0], t.rule));
        }
        screen = screen.child(el().pin(Some(8.0), None, None, Some(6.0)).child(label(t, 11.0, 600.0, m.name.as_str(), t.ink2)));
        screens = screens.child(screen);
    }
    let stage = row().w_full().h(176.0).center().radius(t.radii.card).fill(t.fill).child(screens);

    let on = config.shows_on(&monitor.name);
    let over = config.monitor.get(&monitor.name).cloned().unwrap_or_default();
    let m = &over.modules;
    let mods_index = if m.left.is_none() && m.center.is_none() && m.right.is_none() {
        Some(0)
    } else if m.left.as_deref().is_some_and(|l| l.iter().map(String::as_str).eq(MINIMAL_LEFT.iter().copied()))
        && m.right.as_deref().is_some_and(|r| r.iter().map(String::as_str).eq(MINIMAL_RIGHT.iter().copied()))
        && m.center.as_deref().is_none_or(|c| c.is_empty())
    {
        Some(1)
    } else {
        None
    };
    let new_get_bar = config.bar.monitors.is_empty();
    let showing: Vec<String> = list.iter().filter(|m| config.shows_on(&m.name)).map(|m| m.name.clone()).collect();

    let (f_show, f_mods, f_new) = (file.clone(), file.clone(), file.clone());
    let (n_show, n_mods) = (monitor.name.clone(), monitor.name.clone());
    let spec = format!(
        "{} \u{d7} {} \u{b7} {:.0} Hz{}",
        monitor.mode.0, monitor.mode.1, monitor.refresh,
        if monitor.description.is_empty() { String::new() } else { format!(" \u{b7} {}", monitor.description) }
    );
    let mods_ctl = row().gap(10.0).items_center()
        .child(if mods_index.is_none() { label(t, 12.0, 500.0, "Custom", t.ink3) } else { el() })
        .child(segmented(t, "mon-mods", &["Same as main".into(), "Minimal".into()], mods_index, 240.0, move |s, i| {
            let name = n_mods.clone();
            s.settings.edit(&f_mods, |f| {
                let key = |z: &str| monitor_key(&name, &format!("modules.{z}"));
                if i == 0 {
                    for z in ["left", "center", "right"] {
                        f.remove(&key(z));
                    }
                } else {
                    f.set(&key("left"), string_array(MINIMAL_LEFT));
                    f.set(&key("center"), string_array::<&str>(&[]));
                    f.set(&key("right"), string_array(MINIMAL_RIGHT));
                }
            });
        }));
    let rows = card(t)
        .child(setting(t, true, "Show the bar",
            Some(if on { "The bar is on this monitor.".into() } else { "Windows use the full height.".into() }), None,
            toggle(t, "mon-on".into(), "Show the bar", on, move |s: &mut Stage| s.settings.set(&f_show, &monitor_key(&n_show, "enabled"), !on))))
        .child(setting(t, false, "Modules", Some("Minimal keeps a second screen quiet: workspaces, clock, control center.".into()), None, mods_ctl))
        .child(setting(t, false, "New monitors get a bar",
            Some(if new_get_bar { "Screens you plug in show the bar right away.".into() } else { format!("Only {} show the bar.", config.bar.monitors.join(", ")) }), None,
            toggle(t, "mon-new".into(), "New monitors get a bar", new_get_bar, move |s: &mut Stage| {
                if new_get_bar {
                    s.settings.set(&f_new, "bar.monitors", string_array(&showing));
                } else {
                    s.settings.remove(&f_new, &["bar.monitors"]);
                }
            })));
    vec![stage, group(t, &format!("{} \u{b7} {spec}", monitor.name), None), rows]
}

/// The keys "Restore defaults" removes on each of the bar's own tabs.
pub fn reset_keys(tab: &str) -> Vec<String> {
    let keys: &[&str] = match tab {
        "style" => &["bar.style", "bar.height", "bar.reserve_space", "bar.blur"],
        "modules" => &["modules"],
        "monitors" => &["monitor", "bar.monitors"],
        _ => &[],
    };
    keys.iter().map(|k| k.to_string()).collect()
}
