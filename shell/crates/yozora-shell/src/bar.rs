//! The bar: mark, workspaces, window title, now playing, tray, status icons, clock. Geometry is
//! fixed so every popover knows where its icon sits in every look.

use std::collections::HashMap;
use std::sync::Arc;

use chrono::Local;
use rune::{Cx, El, col, el, icon, row, spacer};
use yozora_services::{hyprland, media::Status};
use yozora_style::{Tokens, glyph, tokens};

use crate::kit::{cell, fig, label};
use crate::shared::Shared;
use crate::stage::Pop;

pub const SIDE: f32 = 10.0;
const CLOCK_W: f32 = 132.0;

pub struct Bar {
    pub sh: Arc<Shared>,
    pub output: String,
    pub screen_w: f32,
    /// Where each button sits (its centre, in points), so a popover grows from the right icon.
    pub anchors: HashMap<String, f32>,
}

impl Bar {
    fn anchor(&self, key: &str) -> f32 {
        self.anchors.get(key).copied().unwrap_or(self.screen_w - 100.0)
    }

    pub fn open(&self, pop: Pop, key: &str) {
        self.sh.open(&self.output, pop, self.anchor(key));
    }

    /// Height the surface needs: the gap above the bar plus the bar. Also its exclusive zone.
    pub fn height(sh: &Shared) -> f32 {
        let t = tokens(sh.look().0);
        t.bar_top + t.bar_h
    }
}

fn workspaces(hypr: &hyprland::State, output: &str, t: &Tokens) -> El {
    let active = hypr.monitor(output).map_or(1, |m| m.active_workspace);
    let top = hypr.workspaces.iter().map(|w| w.id).max().unwrap_or(0).max(5).min(10);
    let mut r = row().h(28.0).center_y();
    for id in 1..=top {
        let on = id == active;
        let dot = if on {
            el().size(20.0, 8.0).radius(4.0).fill(t.ink)
        } else if hypr.occupied(id) {
            el().size(6.0, 6.0).radius(3.0).fill(t.ink2)
        } else {
            el().size(6.0, 6.0).radius(3.0).border(1.25, t.ink3)
        };
        r = r.child(
            cell(if on { 28.0 } else { 24.0 }, 28.0, dot).key(format!("ws{id}")).role("button").label(format!("Workspace {id}")).hit()
                .on_press(move |b: &mut Bar| {
                    if let Some(h) = &b.sh.services.lock().unwrap().hyprland {
                        h.send(hyprland::Cmd::Workspace(id));
                    }
                }),
        );
    }
    r
}

fn media(sh: &Shared, t: &Tokens) -> El {
    let svc = sh.services.lock().unwrap();
    let Some(m) = svc.media.as_ref() else { return el().w(0.0) };
    let snap = m.read();
    let Some(p) = snap.state.current() else { return el().w(0.0) };
    let bar_h = if p.status == Status::Playing { [5.0, 12.0, 8.0] } else { [4.0, 4.0, 4.0] };
    let bars = row().gap(2.0).h(12.0).center_y().children(bar_h.map(|h| el().w(2.0).h(h).radius(1.0).fill(t.acc)));
    let (title, artist) = (p.title.clone(), p.artist.clone());
    button(row().h(28.0).center_y().gap(8.0).px(10.0).radius(t.radii.pill), "media", Pop::Media)
        .child(bars)
        .child(label(t, 13.0, 600.0, title, t.ink))
        .child(label(t, 13.0, 400.0, artist, t.ink2))
}

/// Makes `e` a bar button that records its centre and opens `pop` when pressed.
fn button(e: El, key: impl Into<String>, pop: Pop) -> El {
    let key: String = key.into();
    let (k1, k2) = (key.clone(), key.clone());
    e.key(key.clone()).role("button").label(key).hit()
        .on_layout_tx(move |b: &mut Bar, [x, _, w, _], tx| {
            b.anchors.insert(k1.clone(), x + w / 2.0);
            tx.live_only();
        })
        .on_press(move |b: &mut Bar| b.open(pop, &k2))
}

pub fn view(b: &Bar, _: &mut Cx<Bar>) -> El {
    let (look, _fx) = b.sh.look();
    let t = tokens(look);
    let hypr_state = b.sh.services.lock().unwrap().hyprland.as_ref().map(|h| h.read().state.clone());
    let hypr = hypr_state.unwrap_or_default();
    let title = hypr.window_on(&b.output).map(|w| w.title.clone()).unwrap_or_default();

    let tray_count = b.sh.services.lock().unwrap().tray.as_ref().map_or(0, |t| t.read().state.items.len()).min(4);
    let tray = row().h(28.0).children((0..tray_count).map(|i| {
        button(cell(30.0, 28.0, icon(glyph(look, "sync"), 17.0).color(t.ink)), format!("tray{i}"), Pop::Tray)
    }));

    let bluetooth_present = b.sh.services.lock().unwrap().bluetooth.as_ref().is_some_and(|s| s.read().state.adapter.is_some());
    let mut status = row().h(28.0).px(4.0);
    for (name, pop) in [("bt", Pop::Bluetooth), ("wifi", Pop::Wifi), ("vol", Pop::Sound), ("cc", Pop::Control), ("bell", Pop::Control)] {
        if name == "bt" && !bluetooth_present {
            continue;
        }
        status = status.child(button(cell(34.0, 28.0, icon(glyph(look, name), 17.0).color(t.ink)).radius(t.radii.pill), name, pop));
    }

    let now = Local::now();
    let clock = button(row().w(CLOCK_W).h(28.0).center().gap(8.0).radius(t.radii.pill).fill(t.raised), "clock", Pop::Calendar)
        .child(label(&t, 13.0, 500.0, now.format("%a %-d %b").to_string(), t.ink2))
        .child(fig(&t, 13.0, 600.0, now.format("%H:%M").to_string(), t.ink));

    let bar = row().key("bar").pin(Some(SIDE), Some(t.bar_top), Some(SIDE), None).h(t.bar_h).px(6.0).gap(4.0).center_y()
        .radius(t.radii.pill).fill(t.surf).border(1.0, t.rule)
        .child(cell(40.0, 28.0, icon(glyph(look, "moon"), 20.0).color(t.ink)).key("mark").role("button").label("Yozora").hit())
        .child(workspaces(&hypr, &b.output, &t))
        .child(el().w(10.0))
        .child(label(&t, 13.0, 400.0, title, t.ink2))
        .child(spacer())
        .child(media(&b.sh, &t))
        .child(tray)
        .child(status)
        .child(clock);
    col().w_full().h_full().child(bar)
}
