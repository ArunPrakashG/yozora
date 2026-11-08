//! The bar: mark, workspaces, window title, now playing, tray, status icons, clock. Geometry is
//! fixed so every popover knows where its icon sits in every look.

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
}

impl Bar {
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
    row().key("media").role("button").label("Now playing").hit().h(28.0).center_y().gap(8.0).px(10.0).radius(t.radii.pill)
        .child(bars)
        .child(label(t, 13.0, 600.0, title, t.ink))
        .child(label(t, 13.0, 400.0, artist, t.ink2))
}

pub fn view(b: &Bar, _: &mut Cx<Bar>) -> El {
    let (look, _fx) = b.sh.look();
    let t = tokens(look);
    let hypr_state = b.sh.services.lock().unwrap().hyprland.as_ref().map(|h| h.read().state.clone());
    let hypr = hypr_state.unwrap_or_default();
    let title = hypr.window_on(&b.output).map(|w| w.title.clone()).unwrap_or_default();

    let tray_count = b.sh.services.lock().unwrap().tray.as_ref().map_or(0, |t| t.read().state.items.len()).min(4);
    let tray = row().h(28.0).children((0..tray_count).map(|i| {
        cell(30.0, 28.0, icon(glyph(look, "sync"), 17.0).color(t.ink)).key(format!("tray{i}")).role("button").label("Tray item").hit()
    }));

    let mut status = row().h(28.0).px(4.0);
    for name in ["wifi", "vol", "cc", "bell"] {
        status = status.child(cell(34.0, 28.0, icon(glyph(look, name), 17.0).color(t.ink)).key(name).role("button").label(name).hit().radius(t.radii.pill));
    }

    let now = Local::now();
    let clock_x = b.screen_w - SIDE - 6.0 - CLOCK_W / 2.0;
    let clock = row().key("clock").role("button").label("Calendar").hit().w(CLOCK_W).h(28.0).center().gap(8.0).radius(t.radii.pill).fill(t.raised)
        .on_press(move |b: &mut Bar| b.sh.open(&b.output, Pop::Calendar, clock_x))
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
