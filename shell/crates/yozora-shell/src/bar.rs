//! The bar: the modules `bar.toml` places in its three zones, in the look's own geometry. Every
//! item records where it sits, so popovers grow from the right icon, and names itself in a
//! tooltip.

use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;
use std::sync::Arc;

use chrono::format::{Item, StrftimeItems};
use chrono::{DateTime, Local};
use rune::{Color, Cx, El, WheelEvent, col, el, row};
use yozora_config::bar::BarConfig;
use yozora_services::{audio, hyprland, media::Status, network, power, run_shell};
use yozora_style::{Look, Tokens};

use crate::bar_config::{Conf, Geo};
use crate::bar_modules::{Layout, Module};
use crate::battery;
use crate::kit::{cell, fig, glyph_el, label};
use crate::module_pop::{glyph_name, item_view, tone_color};
use crate::shared::{OpenState, Shared};
use crate::stage::Pop;
use crate::tooltip;

pub const SIDE: f32 = 10.0;
const CLOCK_W: f32 = 132.0;
const DEFAULT_CLOCK: &str = "%a %-d %b  %-I:%M %p";
/// How far the wheel travels, in points, for one step.
const WHEEL_STEP: f32 = 20.0;
const TRAY_MAX: usize = 8;

pub struct Bar {
    pub sh: Arc<Shared>,
    pub output: String,
    pub screen_w: f32,
    /// Where each button sits (its centre, in points), so a popover grows from the right icon.
    pub anchors: HashMap<String, f32>,
    /// Wheel travel not yet spent on a step.
    pub wheel: f32,
}

impl Bar {
    fn anchor(&self, key: &str) -> f32 {
        self.anchors.get(key).copied().unwrap_or(self.screen_w - 100.0)
    }

    pub fn open(&self, pop: Pop, key: &str) {
        self.sh.open(&self.output, pop, self.anchor(key));
    }

    pub fn open_module(&self, name: &str, key: &str) {
        self.sh.open_module(&self.output, name, self.anchor(key));
    }

    /// Height the surface needs: the gap above the bar plus the bar. Also its exclusive zone.
    pub fn height(sh: &Shared) -> f32 {
        sh.bar_conf().geo(&yozora_style::tokens(sh.look().0)).surface_h()
    }

    /// Turns wheel travel into whole steps (positive is up), dropping what was left over when
    /// the direction turns.
    fn steps(&mut self, dy: f32) -> i32 {
        if self.wheel != 0.0 && self.wheel.signum() != dy.signum() {
            self.wheel = 0.0;
        }
        self.wheel += dy;
        let n = (self.wheel / WHEEL_STEP).trunc();
        self.wheel -= n * WHEEL_STEP;
        n as i32
    }
}

// ---- what the services say ---------------------------------------------------------------

struct NetView {
    glyph: &'static str,
    dim: bool,
    tip: String,
}

struct SoundView {
    glyph: &'static str,
    dim: bool,
    tip: String,
}

struct DevView {
    glyph: &'static str,
    dim: bool,
    low: Option<u8>,
    tip: String,
}

struct NotifView {
    dnd: bool,
    dim: bool,
    count: usize,
    unread: bool,
    tip: String,
}

struct MediaView {
    title: String,
    artist: String,
    playing: bool,
}

/// A copy of what the bar draws, taken under one lock so the services aren't held while building.
struct Snap {
    hypr: hyprland::State,
    tray: Vec<yozora_services::tray::Item>,
    net: NetView,
    sound: SoundView,
    dev: Option<DevView>,
    battery: Option<power::Battery>,
    notif: NotifView,
    media: Option<MediaView>,
    custom: BTreeMap<String, yozora_services::custom::Output>,
    modules: BTreeMap<String, yozora_services::modules::ModuleState>,
}

fn snap(sh: &Shared, layout: &Layout) -> Snap {
    let svc = sh.services.lock().unwrap();

    let hypr = svc.hyprland.as_ref().map(|h| h.read().state.clone()).unwrap_or_default();
    let tray = svc.tray.as_ref().map(|t| t.read().state.items.iter().take(TRAY_MAX).cloned().collect()).unwrap_or_default();

    let net = match svc.network.as_ref() {
        Some(n) => {
            let up = n.up();
            let g = n.read();
            let s = &g.state;
            let (glyph, tip) = match &s.primary {
                Some(p) => match p.kind {
                    network::Kind::Wifi => ("wifi", format!("Wi-Fi\n{} \u{b7} {}%", p.name, p.strength)),
                    network::Kind::Ethernet => ("ethernet", format!("Ethernet\n{}", p.name)),
                    network::Kind::Vpn => ("shield", format!("VPN\n{}", p.name)),
                    network::Kind::Other => ("globe", format!("Network\n{}", p.name)),
                },
                None if s.wifi_device && !s.wifi_enabled => ("wifioff", "Wi-Fi is off".to_owned()),
                None => ("netoff", "Not connected".to_owned()),
            };
            NetView { glyph, dim: !up || s.primary.is_none(), tip }
        }
        None => NetView { glyph: "netoff", dim: true, tip: "Network is not available".into() },
    };

    let sound = match svc.audio.as_ref() {
        Some(a) => {
            let up = a.up();
            let g = a.read();
            match g.state.sink() {
                Some(d) => {
                    let pct = (d.volume * 100.0).round() as i32;
                    let glyph = if d.muted || d.volume <= 0.0 {
                        "vol0"
                    } else if d.volume < 0.5 {
                        "vol1"
                    } else {
                        "vol"
                    };
                    let tip = if d.muted { format!("Muted\n{}", d.description) } else { format!("Volume {pct}%\n{}", d.description) };
                    SoundView { glyph, dim: d.muted || !up, tip }
                }
                None => SoundView { glyph: "vol0", dim: true, tip: "No sound output".into() },
            }
        }
        None => SoundView { glyph: "vol0", dim: true, tip: "Sound is not available".into() },
    };

    let power_state = svc.power.as_ref().map(|p| p.read().state.clone());
    let dev = svc.bluetooth.as_ref().and_then(|b| {
        let g = b.read();
        let s = &g.state;
        let peripherals = power_state.as_ref().map(|p| p.peripherals.as_slice()).unwrap_or_default();
        if s.adapter.is_none() && peripherals.is_empty() {
            return None;
        }
        let mut low: Option<u8> = None;
        for p in peripherals {
            let v = p.percent.round().clamp(0.0, 100.0) as u8;
            low = Some(low.map_or(v, |l| l.min(v)));
        }
        for d in s.connected() {
            if let Some(v) = d.battery {
                low = Some(low.map_or(v, |l| l.min(v)));
            }
        }
        let names: Vec<&str> = s.connected().map(|d| d.name.as_str()).collect();
        let tip = if s.adapter.is_some() && !s.powered {
            "Bluetooth is off".to_owned()
        } else if names.is_empty() {
            "Bluetooth\nNo devices connected".to_owned()
        } else {
            format!("Bluetooth\n{}", names.join(", "))
        };
        Some(DevView { glyph: if s.powered { "bt" } else { "btoff" }, dim: !s.powered, low, tip })
    });

    let battery = power_state.and_then(|p| p.battery);

    let notif = match svc.notifications.as_ref() {
        Some(n) => {
            let up = n.up();
            let g = n.read();
            let count = g.state.items.iter().filter(|i| i.unread && i.snoozed_until == 0).count();
            let tip = if g.state.dnd {
                "Do Not Disturb".to_owned()
            } else if count > 0 {
                format!("Notifications\n{count} unread")
            } else {
                "Notifications\nNo new notifications".to_owned()
            };
            NotifView { dnd: g.state.dnd, dim: !up, count, unread: g.state.unread, tip }
        }
        None => NotifView { dnd: false, dim: true, count: 0, unread: false, tip: "Notifications are not available".into() },
    };

    let media = svc.media.as_ref().and_then(|m| {
        let g = m.read();
        let p = g.state.current()?;
        (!p.title.is_empty()).then(|| MediaView { title: p.title.clone(), artist: p.artist.clone(), playing: p.status == Status::Playing })
    });

    let custom = svc.custom.as_ref().map(|c| c.read().state.outputs.clone()).unwrap_or_default();
    let modules = match svc.modules.as_ref() {
        Some(m) => layout.dynamic_names().into_iter().filter_map(|n| m.states().get(&n).cloned().map(|s| (n, s))).collect(),
        None => BTreeMap::new(),
    };

    Snap { hypr, tray, net, sound, dev, battery, notif, media, custom, modules }
}

// ---- paint -------------------------------------------------------------------------------

struct Ctx<'a> {
    b: &'a Bar,
    t: Tokens,
    geo: Geo,
    conf: Arc<Conf>,
    snap: Snap,
    open: Option<OpenState>,
}

/// An item's text colours: the main one and the quieter one.
#[derive(Clone, Copy)]
struct Pal {
    main: Color,
    sub: Color,
}

fn inverted(t: &Tokens) -> bool {
    matches!(t.look, Look::Folio | Look::Phosphor)
}

fn faded(c: Color, by: f32) -> Color {
    Color { a: c.a * by, ..c }
}

impl Ctx<'_> {
    fn pal(&self, open: bool) -> Pal {
        if open && inverted(&self.t) {
            Pal { main: self.t.on_ink, sub: faded(self.t.on_ink, 0.72) }
        } else {
            Pal { main: self.t.ink, sub: self.t.ink2 }
        }
    }

    fn is_open(&self, pop: Pop) -> bool {
        self.open.as_ref().is_some_and(|o| o.pop == pop)
    }

    fn is_module_open(&self, name: &str) -> bool {
        self.open.as_ref().is_some_and(|o| o.pop == Pop::Module && o.module == name)
    }

    /// The fill an item wears while its popover is open, or the hover it wears otherwise.
    fn paint(&self, e: El, open: bool) -> El {
        let t = &self.t;
        if !open {
            return e.hover(t.fill);
        }
        match t.look {
            Look::Folio | Look::Phosphor => e.fill(t.ink),
            Look::Prism if !t.dark => e
                .fill(Color { r: 1.0, g: 1.0, b: 1.0, a: 0.95 })
                .shadow(2.0, 8.0, Color { r: 40.0 / 255.0, g: 50.0 / 255.0, b: 120.0 / 255.0, a: 0.16 }),
            _ => e.fill(t.fill),
        }
    }

    fn glyph(&self, name: &str, size: f32, p: Pal, dim: bool, alert: bool) -> El {
        let c = if alert { self.t.alert } else { p.main };
        glyph_el(&self.t, name, size, if dim { faded(c, 0.45) } else { c })
    }
}

/// Makes `e` a bar button: it records its centre under `key`, and names itself in a tooltip.
fn button(c: &Ctx, e: El, key: &str, name: &str, tip: Option<String>, act: Option<Rc<dyn Fn(&mut Bar)>>) -> El {
    let k1 = key.to_owned();
    let mut e = e.key(key.to_owned()).role("button").label(name.to_owned()).hit().on_layout_tx(move |b: &mut Bar, [x, _, w, _], tx| {
        b.anchors.insert(k1.clone(), x + w / 2.0);
        b.sh.set_anchor(&b.output, &k1, x + w / 2.0);
        tx.live_only();
    });
    if let Some(act) = act {
        e = e.on_press(move |b: &mut Bar| act(b));
    }
    match tip.filter(|t| !t.trim().is_empty()) {
        Some(text) => tipped(e, key, text),
        None => {
            let _ = c;
            e
        }
    }
}

/// Shows `text` under the item once the pointer has rested on it, and takes it down on leaving.
fn tipped(e: El, key: &str, text: String) -> El {
    let key = key.to_owned();
    e.hover_delays(tooltip::DELAY, std::time::Duration::ZERO).on_hover_change(move |b: &mut Bar, inside, tx| {
        let (k, text) = (key.clone(), text.clone());
        let x = b.anchors.get(&key).copied().unwrap_or(b.screen_w / 2.0);
        b.sh.with_stage(&b.output.clone(), move |s| {
            if inside {
                if s.open.is_none() {
                    tooltip::set(s, &k, text, x);
                }
            } else {
                tooltip::clear(s, &k);
            }
        });
        tx.live_only();
    })
}

fn pop_act(pop: Pop, key: &str) -> Option<Rc<dyn Fn(&mut Bar)>> {
    let key = key.to_owned();
    Some(Rc::new(move |b: &mut Bar| b.open(pop, &key)))
}

// ---- modules -----------------------------------------------------------------------------

fn launcher(c: &Ctx) -> El {
    let open = c.is_open(Pop::Menu);
    let p = c.pal(open);
    let e = c.paint(cell(40.0, c.geo.item_h, c.glyph("moon", 20.0, p, false, false)).radius(c.t.radii.pill), open);
    button(c, e, "menu", "Yozora", Some("Yozora".into()), pop_act(Pop::Menu, "menu"))
}

/// Workspace ids shown on `output`: the configured ones, plus any occupied or active beyond them.
fn slots(h: &hyprland::State, output: &str, conf: &BarConfig) -> Vec<i32> {
    let cfg = &conf.workspaces;
    let mut ids: Vec<i32> = if cfg.per_monitor {
        h.workspaces.iter().filter(|w| w.monitor == output && w.id > 0).map(|w| w.id).collect()
    } else {
        (1..=cfg.count.max(1) as i32).collect()
    };
    for w in &h.workspaces {
        let mine = !cfg.per_monitor || w.monitor == output;
        if w.id > 0 && w.windows > 0 && mine && !ids.contains(&w.id) {
            ids.push(w.id);
        }
    }
    if let Some(m) = h.monitor(output)
        && m.active_workspace > 0
        && !ids.contains(&m.active_workspace)
    {
        ids.push(m.active_workspace);
    }
    ids.sort_unstable();
    ids.dedup();
    if ids.is_empty() {
        ids.push(1);
    }
    ids
}

fn goto(b: &Bar, id: i32) {
    if let Some(h) = &b.sh.services.lock().unwrap().hyprland {
        h.send(hyprland::Cmd::Workspace(id));
    }
}

fn workspaces(c: &Ctx) -> El {
    let t = &c.t;
    let h = &c.snap.hypr;
    let ids = slots(h, &c.b.output, &c.conf.bar);
    let active = h.monitor(&c.b.output).map_or(0, |m| m.active_workspace);
    let mut r = row().h(c.geo.item_h).center_y().key("workspaces").hit().on_wheel(|b: &mut Bar, e: WheelEvent, _| {
        let n = b.steps(e.dy);
        if n == 0 {
            return true;
        }
        let (ids, active) = {
            let svc = b.sh.services.lock().unwrap();
            let Some(h) = svc.hyprland.as_ref() else { return true };
            let g = h.read();
            (slots(&g.state, &b.output, &b.sh.bar_conf().bar), g.state.monitor(&b.output).map_or(0, |m| m.active_workspace))
        };
        let Some(i) = ids.iter().position(|id| *id == active) else { return true };
        // Wheel up goes to the previous workspace, down to the next.
        let next = if n > 0 { i.checked_sub(1) } else { Some(i + 1) };
        if let Some(&id) = next.and_then(|n| ids.get(n)) {
            goto(b, id);
        }
        true
    });
    for id in ids {
        let on = id == active;
        let dot = if on {
            el().size(20.0, 8.0).radius(4.0).fill(t.ink)
        } else if h.occupied(id) {
            el().size(6.0, 6.0).radius(3.0).fill(t.ink2)
        } else {
            el().size(6.0, 6.0).radius(3.0).border(1.25, t.ink3)
        };
        let key = format!("ws{id}");
        let e = cell(if on { 28.0 } else { 24.0 }, c.geo.item_h, dot);
        r = r.child(button(c, e, &key, &format!("Workspace {id}"), Some(format!("Workspace {id}")), Some(Rc::new(move |b: &mut Bar| goto(b, id)))));
    }
    r
}

fn ellipsize(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_owned();
    }
    let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
    out.push('\u{2026}');
    out
}

/// Splits "bar.toml - yozora - Code" into the document and the app, when the title has one.
fn split_title(title: &str, class: &str) -> (String, Option<String>) {
    if title.is_empty() {
        return (class.to_string(), None);
    }
    for sep in [" \u{2014} ", " - ", " \u{2013} "] {
        if let Some((head, tail)) = title.split_once(sep) {
            return (head.to_string(), Some(tail.to_string()));
        }
    }
    // Reverse-DNS classes like "com.anthropic.Claude" read better as their last part.
    let class = class.rsplit('.').next().unwrap_or(class);
    (title.to_string(), (!class.is_empty() && !title.eq_ignore_ascii_case(class)).then(|| class.to_string()))
}

fn window(c: &Ctx) -> Option<El> {
    let h = &c.snap.hypr;
    let max = c.conf.bar.window.max_length;
    let (text, tip) = match h.window_on(&c.b.output) {
        Some(w) => {
            let (title, app) = split_title(&w.title, &w.class);
            let title = ellipsize(&title, max);
            let text = match &app {
                Some(a) => format!("{a} \u{b7} {title}"),
                None => title,
            };
            let full = if w.class.is_empty() || w.class == w.title { w.title.clone() } else { format!("{}\n{}", w.title, w.class) };
            (text, full)
        }
        None => {
            let n = h.monitor(&c.b.output)?.active_workspace;
            (format!("Workspace {n}"), String::new())
        }
    };
    let e = label(&c.t, 13.0, 400.0, text, c.t.ink2).truncate().shrink();
    let e = row().h(c.geo.item_h).px(8.0).center_y().min_w(0.0).shrink().child(e);
    let e = e.key("window").role("text");
    Some(if tip.is_empty() { e } else { tipped(e.hit(), "window", tip) })
}

fn media(c: &Ctx) -> Option<El> {
    let m = c.snap.media.as_ref()?;
    let open = c.is_open(Pop::Media);
    let p = c.pal(open);
    let t = &c.t;
    let max = c.conf.bar.media.max_length;
    let bar_h = if m.playing { [5.0, 12.0, 8.0] } else { [4.0, 4.0, 4.0] };
    let eq = if inverted(t) && open { t.on_ink } else { t.acc };
    let eq = if m.playing { eq } else { faded(eq, 0.55) };
    let bars = row().gap(2.0).h(12.0).center_y().children(bar_h.map(|h| el().w(2.0).h(h).radius(1.0).fill(eq)));
    let tip = if m.artist.is_empty() { m.title.clone() } else { format!("{}\n{}", m.title, m.artist) };
    let mut e = row().h(c.geo.item_h).center_y().gap(8.0).px(10.0).max_w(340.0).radius(t.radii.pill).child(bars).child(
        label(t, 13.0, 600.0, ellipsize(&m.title, max), if m.playing { p.main } else { p.sub }).truncate().shrink(),
    );
    if !m.artist.is_empty() {
        e = e.child(label(t, 13.0, 400.0, ellipsize(&m.artist, max), p.sub).truncate().shrink());
    }
    Some(button(c, c.paint(e, open), "media", "Now playing", Some(tip), pop_act(Pop::Media, "media")))
}

fn tray(c: &Ctx) -> El {
    let t = &c.t;
    let mut r = row().h(c.geo.item_h).center_y();
    for (i, item) in c.snap.tray.iter().enumerate() {
        let key = item.key.clone();
        let menu = item.has_menu || item.item_is_menu;
        let k = format!("tray{i}");
        let anchor = c.b.anchors.get(&k).copied();
        let open = c.open.as_ref().is_some_and(|o| o.pop == Pop::Tray && anchor.is_some_and(|a| (a - o.anchor).abs() < 1.0));
        let e = c.paint(cell(30.0, c.geo.item_h, crate::tray::app_icon(t, &c.b.sh, item)).radius(t.radii.pill), open);
        let kk = k.clone();
        let act: Rc<dyn Fn(&mut Bar)> = Rc::new(move |b: &mut Bar| {
            let send = |cmd: yozora_services::tray::Cmd| {
                if let Some(t) = &b.sh.services.lock().unwrap().tray {
                    t.send(cmd);
                }
            };
            if menu {
                send(yozora_services::tray::Cmd::LoadMenu(key.clone()));
                b.open(Pop::Tray, &kk);
            } else {
                send(yozora_services::tray::Cmd::Activate(key.clone()));
            }
        });
        r = r.child(button(c, e, &k, &item.title, Some(item.title.clone()), Some(act)));
    }
    r
}

/// A status item: its glyph, then optional text. `min_w` keeps lone icons the approved 34 wide.
fn status_item(c: &Ctx, open: bool) -> El {
    c.paint(row().min_w(34.0).h(c.geo.item_h).px(7.0).gap(5.0).center().radius(c.t.radii.pill), open)
}

fn small(c: &Ctx, p: Pal, text: String, alert: bool) -> El {
    fig(&c.t, 12.5, 600.0, text, if alert { c.t.alert } else { p.main })
}

fn network(c: &Ctx) -> El {
    let open = c.is_open(Pop::Wifi);
    let p = c.pal(open);
    let e = status_item(c, open).child(c.glyph(c.snap.net.glyph, 17.0, p, c.snap.net.dim, false));
    button(c, e, "wifi", "Network", Some(c.snap.net.tip.clone()), pop_act(Pop::Wifi, "wifi"))
}

fn sound(c: &Ctx) -> El {
    let open = c.is_open(Pop::Sound);
    let p = c.pal(open);
    let s = &c.snap.sound;
    let e = status_item(c, open).child(c.glyph(s.glyph, 17.0, p, s.dim, false)).on_wheel(|b: &mut Bar, e: WheelEvent, _| {
        let n = b.steps(e.dy).clamp(-3, 3);
        if n != 0
            && let Some(a) = &b.sh.services.lock().unwrap().audio
        {
            let v = a.read().state.sink().map_or(0.0, |d| d.volume);
            let next = (v + 0.05 * n as f32).clamp(0.0, v.max(1.0));
            a.send(audio::Cmd::SinkVolume(next));
        }
        true
    });
    button(c, e, "vol", "Sound", Some(s.tip.clone()), pop_act(Pop::Sound, "vol"))
}

fn devices(c: &Ctx) -> Option<El> {
    let d = c.snap.dev.as_ref()?;
    let open = c.is_open(Pop::Bluetooth);
    let p = c.pal(open);
    let mut e = status_item(c, open).child(c.glyph(d.glyph, 17.0, p, d.dim, false));
    if let Some(v) = d.low {
        e = e.child(small(c, p, format!("{v}%"), v <= 15));
    }
    Some(button(c, e, "bt", "Devices", Some(d.tip.clone()), pop_act(Pop::Bluetooth, "bt")))
}

fn battery_item(c: &Ctx) -> Option<El> {
    let b = c.snap.battery.as_ref()?;
    let open = c.is_open(Pop::Battery);
    let p = c.pal(open);
    let low = battery::is_low(b);
    let glyph = if matches!(b.charge, power::Charge::Charging) { "battcharge" } else { "battery" };
    let pct = b.percent.round() as i32;
    let e = status_item(c, open).child(c.glyph(glyph, 17.0, p, false, low)).child(small(c, p, format!("{pct}%"), low));
    let tip = format!("Battery {pct}%\n{}", battery::summary(b));
    Some(button(c, e, "battery", "Battery", Some(tip), pop_act(Pop::Battery, "battery")))
}

fn notifications(c: &Ctx) -> El {
    let open = c.is_open(Pop::Notifications);
    let p = c.pal(open);
    let n = &c.snap.notif;
    let mut e = status_item(c, open).child(c.glyph(if n.dnd { "moon" } else { "bell" }, 17.0, p, n.dim, false));
    if n.count > 0 && !n.dnd {
        e = e.child(small(c, p, n.count.to_string(), false));
    }
    if n.unread && !n.dnd {
        let ring = if open && inverted(&c.t) { c.t.ink } else { c.t.surf };
        e = e.child(el().pin(Some(21.0), Some((c.geo.item_h - 17.0) / 2.0 - 4.0), None, None).size(7.0, 7.0).radius(c.t.radii.pill).fill(c.t.acc).border(1.5, ring));
    }
    button(c, e, "bell", "Notifications", Some(n.tip.clone()), pop_act(Pop::Notifications, "bell"))
}

fn control_center(c: &Ctx) -> El {
    let open = c.is_open(Pop::Control);
    let p = c.pal(open);
    let e = status_item(c, open).child(c.glyph("cc", 17.0, p, false, false));
    button(c, e, "cc", "Control center", Some("Control center".into()), pop_act(Pop::Control, "cc"))
}

fn format_time(now: &DateTime<Local>, fmt: &str) -> String {
    if StrftimeItems::new(fmt).any(|i| matches!(i, Item::Error)) { now.format(DEFAULT_CLOCK).to_string() } else { now.format(fmt).to_string() }
}

/// Whether the clock shows seconds, which decides how often it redraws.
pub fn clock_ticks_seconds(fmt: &str) -> bool {
    ["%S", "%T", "%r", "%X", "%+", "%s"].iter().any(|f| fmt.contains(f))
}

fn clock(c: &Ctx) -> El {
    let t = &c.t;
    let open = c.is_open(Pop::Calendar);
    let p = c.pal(open);
    let now = Local::now();
    let text = format_time(&now, &c.conf.bar.clock.format);
    let groups: Vec<&str> = text.split("  ").collect();
    let mut e = row().min_w(CLOCK_W).h(c.geo.item_h).px(12.0).center().gap(8.0).radius(t.radii.pill).fill(t.raised);
    for (i, g) in groups.iter().enumerate() {
        e = e.child(if i + 1 == groups.len() { fig(t, 13.0, 600.0, *g, p.main) } else { label(t, 13.0, 500.0, *g, p.sub) });
    }
    let tip = now.format("%A, %-d %B %Y").to_string();
    button(c, c.paint(e, open), "clock", "Clock", Some(tip), pop_act(Pop::Calendar, "clock"))
}

fn custom(c: &Ctx, name: &str) -> Option<El> {
    let out = c.snap.custom.get(name)?;
    if out.text.is_empty() && out.error.is_none() {
        return None;
    }
    let t = &c.t;
    let key = format!("custom/{name}");
    let p = c.pal(false);
    let mut e = row().h(c.geo.item_h).px(10.0).gap(6.0).center().radius(t.radii.pill);
    if out.text.is_empty() {
        e = e.child(c.glyph("alert", 15.0, p, false, true));
    } else {
        e = e.child(label(t, 12.5, 400.0, out.text.clone(), p.main).truncate().shrink().max_w(260.0));
    }
    let tip = if out.tooltip.is_empty() { out.error.clone().unwrap_or_default() } else { out.tooltip.clone() };
    let on_click = c.conf.bar.custom.get(name).and_then(|cc| cc.on_click.clone()).unwrap_or_default();
    let act: Option<Rc<dyn Fn(&mut Bar)>> = (!on_click.trim().is_empty()).then(|| {
        let k = key.clone();
        Rc::new(move |b: &mut Bar| {
            run_shell(&on_click);
            let (out, k) = (b.output.clone(), k.clone());
            b.sh.with_stage(&out, move |s| tooltip::clear(s, &k));
        }) as Rc<dyn Fn(&mut Bar)>
    });
    Some(button(c, c.paint(e, false), &key, name, Some(tip), act))
}

fn dynamic(c: &Ctx, name: &str) -> El {
    let t = &c.t;
    let key = format!("mod:{name}");
    let open = c.is_module_open(name);
    let p = c.pal(open);
    let state = c.snap.modules.get(name).cloned().unwrap_or_default();
    let v = item_view(state.last_good.as_ref(), state.error.as_deref());
    let tone = tone_color(t, v.tone, p.main);
    let mut e = c.paint(row().min_w(34.0).h(c.geo.item_h).px(8.0).gap(6.0).center().radius(t.radii.pill), open);
    e = e.child(glyph_el(t, glyph_name(&v.icon), 17.0, tone));
    if let Some(text) = &v.text {
        e = e.child(label(t, 12.5, 500.0, text.clone(), if v.tone == yozora_module::Tone::Neutral { p.main } else { tone }));
    }
    if let Some(n) = v.badge {
        let fill = if v.tone == yozora_module::Tone::Neutral { t.acc } else { tone };
        e = e.child(row().min_w(16.0).h(16.0).px(4.0).center().radius(8.0).fill(fill).child(fig(t, 10.5, 700.0, n.to_string(), t.on_acc)));
    }
    let body = state.last_good.as_ref().and_then(|u| u.popover.as_ref());
    let mut tip = body.map_or_else(|| name.to_owned(), |b| b.title.clone());
    if let Some(cap) = body.and_then(|b| b.caption.clone()) {
        tip = format!("{tip}\n{cap}");
    }
    if let Some(err) = &state.error {
        tip = format!("{tip}\n{err}");
    }
    let n = name.to_owned();
    let k = key.clone();
    let act: Rc<dyn Fn(&mut Bar)> = Rc::new(move |b: &mut Bar| b.open_module(&n, &k));
    button(c, e, &key, name, Some(tip), Some(act))
}

fn module(c: &Ctx, m: &Module) -> Option<El> {
    match m {
        Module::Launcher => Some(launcher(c)),
        Module::Workspaces => Some(workspaces(c)),
        Module::Window => window(c),
        Module::Media => media(c),
        Module::Tray => Some(tray(c)),
        Module::Network => Some(network(c)),
        Module::Sound => Some(sound(c)),
        Module::Devices => devices(c),
        Module::Battery => battery_item(c),
        Module::Notifications => Some(notifications(c)),
        Module::Clock => Some(clock(c)),
        Module::ControlCenter => Some(control_center(c)),
        Module::Custom(n) => custom(c, n),
        Module::Dynamic(n) => Some(dynamic(c, n)),
    }
}

/// One zone: its modules in order, with neighbouring status icons gathered into one cluster.
fn zone(c: &Ctx, mods: &[Module]) -> El {
    let mut r = row().h(c.geo.h).center_y().gap(4.0);
    let mut cluster: Vec<El> = Vec::new();
    let flush = |r: El, cluster: &mut Vec<El>| -> El {
        if cluster.is_empty() {
            return r;
        }
        r.child(row().h(c.geo.item_h).px(4.0).center_y().children(std::mem::take(cluster)))
    };
    for m in mods {
        let Some(e) = module(c, m) else { continue };
        if m.is_status() {
            cluster.push(e);
        } else {
            r = flush(r, &mut cluster).child(e);
        }
    }
    flush(r, &mut cluster)
}

pub fn view(b: &Bar, _: &mut Cx<Bar>) -> El {
    let (look, _fx) = b.sh.look();
    let conf = b.sh.bar_conf();
    let t = conf.tokens(look);
    let geo = conf.geo(&t);
    let layout = Layout::parse(&conf.bar.zones_for(&b.output));
    let c = Ctx { b, t, geo, conf: conf.clone(), snap: snap(&b.sh, &layout), open: b.sh.open_on(&b.output) };
    let t = &c.t;

    let left = zone(&c, &layout.left);
    let right = zone(&c, &layout.right);
    let mut body = row().key("bar-zones").w_full().h(geo.h).center_y().gap(4.0);
    if layout.center.is_empty() {
        body = body.child(left.grow(1.0).min_w(0.0)).child(right);
    } else {
        let centre = zone(&c, &layout.center);
        body = body
            .child(left.w(0.0).grow(1.0).min_w(0.0))
            .child(centre)
            .child(right.w(0.0).grow(1.0).min_w(0.0).justify_end());
    }

    let bar = if geo.flush {
        let edge = if t.look == Look::Folio { t.ink } else { t.rule };
        row().key("bar").pin(Some(0.0), Some(geo.top), Some(0.0), None).h(geo.h).px(SIDE + 6.0).center_y().fill(t.surf)
            .border_sides([0.0, 0.0, 1.0, 0.0], edge)
            .child(body)
    } else {
        row().key("bar").pin(Some(SIDE), Some(geo.top), Some(SIDE), None).h(geo.h).px(6.0).center_y()
            .radius(geo.radius).fill(t.surf).border(1.0, t.rule)
            .child(body)
    };
    col().w_full().h_full().child(bar)
}
