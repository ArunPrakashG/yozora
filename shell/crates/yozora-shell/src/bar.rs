//! The bar: mark, workspaces, window title, now playing, tray, status icons, clock.
//! Geometry is fixed so every popover knows where its icon sits in every look.

use rune::tok::TextStyle;
use rune::{Cx, El, col, el, icon, row, spacer, text};
use yozora_style::{Fx, Look, Tokens, glyph, tokens};

pub const SIDE: f32 = 10.0;

pub struct Bar {
    pub look: Look,
    pub fx: Fx,
    pub active: usize,
    pub workspaces: usize,
    pub title: String,
    pub media: Option<(String, String)>,
    pub tray: Vec<&'static str>,
    pub status: Vec<&'static str>,
    pub clock: (String, String),
}

impl Bar {
    pub fn demo(look: Look) -> Bar {
        Bar {
            look, fx: Fx::Full, active: 1, workspaces: 5,
            title: "kitty — ~/Projects/yozora".into(),
            media: Some(("Says".into(), "Nils Frahm".into())),
            tray: vec!["sync", "cloud"],
            status: vec!["bt", "wifi", "vol", "cc", "bell"],
            clock: ("Sun 8 Nov".into(), "21:42".into()),
        }
    }

    /// Height the surface needs: the gap above the bar plus the bar. Also its exclusive zone.
    pub fn height(&self) -> f32 {
        let t = tokens(self.look);
        t.bar_top + t.bar_h
    }
}

fn label(t: &Tokens, size: f32, weight: f32, s: impl Into<String>, color: rune::Color) -> El {
    text(s.into()).font(TextStyle::new(size, weight).family(t.fonts.body)).color(color)
}

fn fig(t: &Tokens, size: f32, weight: f32, s: impl Into<String>, color: rune::Color) -> El {
    text(s.into()).font(TextStyle::new(size, weight).family(t.fonts.fig).features("'tnum' 1")).color(color)
}

fn cell(w: f32, h: f32, child: El) -> El {
    row().size(w, h).center().child(child)
}

fn workspaces(b: &Bar, t: &Tokens) -> El {
    let mut r = row().w(120.0).h(28.0).center_y();
    for i in 0..b.workspaces {
        let on = i == b.active;
        let dot = if on {
            el().size(20.0, 8.0).radius(4.0).fill(t.ink)
        } else if i < b.active + 2 && i != b.active && i < 3 {
            el().size(6.0, 6.0).radius(3.0).fill(t.ink2)
        } else {
            el().size(6.0, 6.0).radius(3.0).border(1.25, t.ink3)
        };
        r = r.child(cell(if on { 28.0 } else { 24.0 }, 28.0, dot).key(format!("ws{i}")).role("button").label(format!("Workspace {}", i + 1)).hit());
    }
    r
}

fn media(b: &Bar, t: &Tokens) -> El {
    let Some((title, artist)) = &b.media else { return el().w(176.0) };
    let bars = row().gap(2.0).h(12.0).children([5.0, 12.0, 8.0].map(|h| el().w(2.0).h(h).radius(1.0).fill(t.acc)));
    row().key("media").role("button").label("Now playing").hit().w(176.0).h(28.0).center_y().gap(8.0).px(10.0).radius(t.radii.pill)
        .child(bars)
        .child(label(t, 13.0, 600.0, title.clone(), t.ink))
        .child(label(t, 13.0, 400.0, artist.clone(), t.ink2))
}

fn status_icons(b: &Bar, t: &Tokens) -> El {
    let mut r = row().h(28.0).px(4.0);
    for name in &b.status {
        r = r.child(cell(34.0, 28.0, icon(glyph(b.look, name), 17.0).color(t.ink)).key(*name).role("button").label(*name).hit().radius(t.radii.pill));
    }
    r
}

pub fn view(b: &Bar, _: &mut Cx<Bar>) -> El {
    let t = tokens(b.look);
    let mut bar = row().key("bar").pin(Some(SIDE), Some(t.bar_top), Some(SIDE), None).h(t.bar_h).px(6.0).gap(4.0).center_y()
        .radius(t.radii.pill).fill(t.surf).border(1.0, t.rule);
    bar = bar
        .child(cell(40.0, 28.0, icon(glyph(b.look, "moon"), 20.0).color(t.ink)).key("mark").role("button").label("Yozora").hit())
        .child(workspaces(b, &t))
        .child(el().w(10.0))
        .child(label(&t, 13.0, 400.0, b.title.clone(), t.ink2))
        .child(spacer())
        .child(media(b, &t))
        .child(row().w(60.0).h(28.0).children(b.tray.iter().map(|n| cell(30.0, 28.0, icon(glyph(b.look, n), 17.0).color(t.ink)).key(*n).role("button").label(*n).hit())))
        .child(status_icons(b, &t))
        .child(row().key("clock").role("button").label("Calendar").hit().w(132.0).h(28.0).center().gap(8.0).radius(t.radii.pill).fill(t.raised)
            .child(label(&t, 13.0, 500.0, b.clock.0.clone(), t.ink2))
            .child(fig(&t, 13.0, 600.0, b.clock.1.clone(), t.ink)));
    col().w_full().h_full().child(bar)
}
