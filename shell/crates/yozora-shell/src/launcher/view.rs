//! The launcher's drawing: the field, the sections and rows (or the tiles of the empty view),
//! the Tab menu, and the footer. Row positions are worked out here from fixed heights, so the
//! selection highlight can be one element that glides from row to row on a spring.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rune::tok::TextStyle;
use rune::{Color, El, Image, ScrollSpec, Spring, SpringPair, SurfaceFit, Transition, Tx, col, el, row, text};
use yozora_search::{Mode, Row, Section, Target, Visual};
use yozora_style::{Fx, Look, Motion, Tokens};

use super::{Actions, Launcher, W};
use crate::kit::{cell, glyph_el, label, card};
use crate::shared::Shared;
use crate::sound::alpha;
use crate::stage::Stage;

/// The panel's inside: its width less the padding.
const INNER: f32 = W - 36.0;
const ROW_H: f32 = 52.0;
/// The best match gets a taller row and a larger icon.
const TOP_H: f32 = 62.0;
const HEAD_H: f32 = 30.0;
const TILE_H: f32 = 96.0;
const TILE_GAP: f32 = 6.0;
const TILES: usize = 6;
const ACT_HEAD_H: f32 = 48.0;
const ACT_H: f32 = 42.0;
const CHIPS_H: f32 = 110.0;
const MSG_H: f32 = 56.0;
/// The results scroll beyond this.
const BODY_MAX: f32 = 440.0;
const BAR_H: f32 = 60.0;
const FOOT_H: f32 = 32.0;
const PLACEHOLDER: &str = "Search apps, windows, files and more";

/// When the last request to open came in, until the first frame after it is drawn.
static REQUEST: Mutex<Option<Instant>> = Mutex::new(None);

/// Notes that a request to open came in now, so the first frame can say how long it took.
pub fn mark_request() {
    *REQUEST.lock().unwrap() = Some(Instant::now());
}

/// Whether the launcher is showing the grid of pinned apps.
fn tiles_in(l: &Launcher, sec: &Section) -> bool {
    l.idle() && sec.title == "Pinned"
}

/// One element key per row, stable while the row stays in the results.
pub fn keys(sections: &[Section]) -> Vec<String> {
    let mut seen: HashMap<u64, usize> = HashMap::new();
    let mut out = Vec::new();
    for r in sections.iter().flat_map(|s| s.rows.iter()) {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        format!("{:?}", r.target).hash(&mut h);
        r.title.hash(&mut h);
        let id = h.finish();
        let n = seen.entry(id).or_insert(0);
        out.push(if *n == 0 { format!("lr{id:x}") } else { format!("lr{id:x}-{n}") });
        *n += 1;
    }
    out
}

/// Holds the decoded icons for as long as the launcher lives, so a row that leaves the results
/// and comes back does not draw empty while its image decodes again.
pub fn hold_image(sh: &Arc<Shared>, path: &Path) -> Image {
    static HELD: Mutex<Option<HashMap<PathBuf, Image>>> = Mutex::new(None);
    let mut held = HELD.lock().unwrap();
    let map = held.get_or_insert_with(HashMap::new);
    if map.len() > 400 {
        map.clear();
    }
    map.entry(path.to_path_buf()).or_insert_with(|| crate::media::images(sh).file(path, 48.0, 2.0)).clone()
}

fn row_h(r: &Row) -> f32 {
    if r.top { TOP_H } else { ROW_H }
}

fn tile_w() -> f32 {
    (INNER - 8.0 - TILE_GAP * (TILES as f32 - 1.0)) / TILES as f32
}

/// Where each row sits in the scrolled content, `[x, y, w, h]`, and how tall the content is.
struct Geo {
    rects: Vec<[f32; 4]>,
    total: f32,
}

fn geometry(l: &Launcher, message: bool) -> Geo {
    let mut rects = Vec::new();
    let mut y = 0.0;
    for sec in &l.sections {
        y += HEAD_H;
        if tiles_in(l, sec) {
            for k in 0..sec.rows.len() {
                rects.push([4.0 + k as f32 * (tile_w() + TILE_GAP), y, tile_w(), TILE_H]);
            }
            y += TILE_H;
        } else {
            for r in &sec.rows {
                rects.push([0.0, y, INNER, row_h(r)]);
                y += row_h(r);
            }
        }
    }
    if l.idle() {
        y += CHIPS_H;
    } else if message {
        y += MSG_H;
    }
    Geo { rects, total: y }
}

/// What a row is, shown where the verb is while it isn't selected.
fn kind(t: &Target) -> &'static str {
    match t {
        Target::App { .. } => "Application",
        Target::Window(_) => "Window",
        Target::Command(_) => "Command",
        Target::Copy(_) => "Result",
        Target::Clip(_) => "Clipboard",
        Target::File(_) => "File",
        Target::Shell(_) => "Shell",
        Target::Web(_) => "Web",
        Target::Script(_) => "Script",
        Target::Ssh(_) => "SSH",
        Target::Project(_) => "Project",
        Target::Url(_) => "Page",
    }
}

fn glyph_name(g: yozora_search::Glyph) -> &'static str {
    use yozora_search::Glyph::*;
    match g {
        Search => "search",
        Window => "window",
        Clipboard => "clipboard",
        Folder => "folder",
        Smile => "smile",
        Calculator => "calc",
        Shell | Terminal => "term",
        Globe => "globe",
        Code => "code",
        Star => "star",
        Clock => "clock",
        Reset => "sync",
        Image => "image",
        Link => "link",
        Lock => "lock",
        Moon => "moon",
        Settings => "gear",
        Power => "power",
        Logout => "logout",
        Music => "music",
        Package => "package",
        File => "file",
    }
}

fn plain(t: &Tokens) -> bool {
    matches!(t.look, Look::Folio | Look::Phosphor)
}

/// Text colours on a selected row: Phosphor fills the row with ink, so the text turns to match.
struct Ink {
    main: Color,
    soft: Color,
    faint: Color,
}

fn ink(t: &Tokens, on: bool) -> Ink {
    if on && t.look == Look::Phosphor {
        Ink { main: t.on_ink, soft: alpha(t.on_ink, 0.7), faint: alpha(t.on_ink, 0.55) }
    } else {
        Ink { main: t.ink, soft: t.ink2, faint: t.ink3 }
    }
}

/// The soft white of a selected row in Prism.
fn lift() -> Color {
    Color { r: 1.0, g: 1.0, b: 1.0, a: 0.9 }
}

/// The selection: one element laid under the rows, moved to the chosen one.
fn highlight(t: &Tokens, key: &str, r: [f32; 4], radius: f32) -> El {
    let e = el().key(key).pin(Some(r[0]), Some(r[1]), None, None).size(r[2], r[3]);
    match t.look {
        Look::Folio => e.border_sides([0.0, 0.0, 0.0, 3.0], t.acc),
        Look::Phosphor => e.fill(t.ink),
        Look::Prism => e.radius(radius).fill(lift()).shadow(1.0, 4.0, Color { r: 0.16, g: 0.2, b: 0.47, a: 0.14 }),
        _ => e.radius(radius).fill(t.fill),
    }
}

/// A key cap: "↵", "Tab", "Ctrl P".
fn kbd(t: &Tokens, s: &str, on: bool) -> El {
    let c = ink(t, on);
    let e = row().h(22.0).min_w(22.0).px(6.0).center().child(text(s.to_string()).font(TextStyle::new(12.0, 500.0).family(t.fonts.fig)).color(c.main));
    match t.look {
        Look::Folio => e.border(1.0, t.ink),
        Look::Phosphor => e.border(1.0, c.main),
        Look::Prism => e.radius(6.0).fill(alpha(t.ink, 0.07)),
        _ => e.radius(6.0).fill(t.fill),
    }
}

/// What a row with no icon of its own shows: a glyph, letter or emoji in a tile.
fn visual(t: &Tokens, sh: &Arc<Shared>, v: &Visual, size: f32, on: bool) -> El {
    let c = ink(t, on);
    let tile = |inner: El| {
        let e = cell(size, size, inner);
        match t.look {
            Look::Folio => e.border(1.0, t.ink),
            Look::Phosphor => e.border(1.0, c.main),
            Look::Prism => e.radius(size * 0.275).fill(Color { r: 1.0, g: 1.0, b: 1.0, a: 1.0 }).shadow(4.0, 10.0, Color { r: 0.16, g: 0.2, b: 0.47, a: 0.2 }),
            _ => e.radius(size * 0.275).fill(t.raised),
        }
    };
    match v {
        Visual::Image(path) => {
            let img = hold_image(sh, path.as_path());
            let r = if plain(t) { 0.0 } else { size * 0.22 };
            el().size(size, size).radius(r).pixels_fit(img.pixels(), SurfaceFit::Fit)
        }
        Visual::Glyph(g) => tile(glyph_el(t, glyph_name(*g), size * 0.5, c.main)),
        Visual::Emoji(s) => cell(size, size, label(t, size * 0.6, 400.0, s.clone(), t.ink)),
        Visual::Letter(ch) => {
            let mut style = TextStyle::new(size * 0.55, 400.0).family(t.fonts.display);
            if t.look == Look::Folio {
                style = style.italic();
            }
            tile(text(ch.to_string()).font(style).color(c.main))
        }
    }
}

/// A title with the letters the query matched drawn in the accent.
fn title_text(t: &Tokens, r: &Row, size: f32, weight: f32, on: bool) -> El {
    let c = ink(t, on);
    let mut e = label(t, size, weight, r.title.clone(), c.main).truncate();
    if !r.hl.is_empty() {
        let mut spans = Vec::new();
        for (ci, (bi, ch)) in r.title.char_indices().enumerate() {
            if r.hl.contains(&ci) {
                let s = rune::Span::new(bi, bi + ch.len_utf8()).weight(700.0);
                // Phosphor's selected row is filled with the accent's own colour.
                spans.push(if on && t.look == Look::Phosphor { s.underline(c.main) } else { s.color(t.acc) });
            }
        }
        e = e.spans(spans);
    }
    e
}

/// How rows arrive: a short rise and fade, one after another. None when effects are off.
struct Arrival {
    enter: Transition,
    stagger: f32,
    fresh: bool,
}

fn arrival(t: &Tokens, fx: Fx, l: &Launcher) -> Arrival {
    let m = Motion::of(t.look, fx);
    if m.open == 0.0 {
        return Arrival { enter: Transition::none(), stagger: 0.0, fresh: false };
    }
    let rise = if t.look == Look::Phosphor { 0.0 } else { 6.0 };
    let enter = Transition::offset(0.0, rise)
        .combined(Transition::opacity())
        .springs(SpringPair::new(Spring::new(m.open * 0.5, 0.0), Spring::new(0.14, 0.0)));
    // Rows that arrive with the panel come one after another; later ones, from typing, don't wait.
    let fresh = l.opened.is_some_and(|o| o.elapsed() < Duration::from_millis(450));
    Arrival { enter, stagger: m.row_stagger, fresh }
}

fn arrive(a: &Arrival, e: El, i: usize) -> El {
    let e = e.transition(a.enter.clone());
    if a.fresh && a.stagger > 0.0 { e.enter_delay(a.stagger * (i.min(8) as f32 + 1.0)) } else { e }
}

fn hot(e: El, live: bool) -> El {
    if live { e.hit() } else { e }
}

fn result_row(t: &Tokens, sh: &Arc<Shared>, r: &Row, i: usize, key: &str, on: bool, live: bool) -> El {
    let c = ink(t, on);
    let (size, title, weight) = if r.top { (44.0, 15.5, 600.0) } else { (40.0, 14.0, 500.0) };
    let mut meta = row().gap(8.0).center_y();
    if r.pinned {
        meta = meta.child(glyph_el(t, "star", 14.0, if on && t.look == Look::Phosphor { c.main } else { t.acc }));
    }
    if let Some(b) = &r.badge {
        let chip = row().h(20.0).min_w(22.0).px(6.0).center().child(label(t, 11.5, 600.0, b.clone(), c.soft));
        meta = meta.child(if plain(t) { chip.border(1.0, c.faint) } else { chip.radius(6.0).fill(alpha(t.ink, 0.08)) });
    }
    meta = if on {
        meta.child(label(t, 12.0, 500.0, r.target.verb(), c.soft)).child(kbd(t, "↵", on))
    } else {
        meta.child(label(t, 13.0, 400.0, kind(&r.target), c.faint))
    };
    let mut text_col = col().grow(1.0).min_w(0.0).gap(2.0).justify_center().child(title_text(t, r, title, weight, on));
    if !r.sub.is_empty() {
        text_col = text_col.child(label(t, 12.0, 400.0, r.sub.clone(), c.soft).truncate());
    }
    hot(row().key(key.to_string()).w_full().h(row_h(r)).px(14.0).gap(14.0).center_y().role("button").label(r.title.clone()), live)
        .child(visual(t, sh, &r.visual, size, on))
        .child(text_col)
        .child(meta)
        .on_pointer_move_local(move |s: &mut Stage, _, tx: &mut Tx| {
            if s.launcher.actions.is_none() && s.launcher.sel != i {
                super::point_at(s, i);
            } else {
                tx.live_only();
            }
        })
        .on_press(move |s: &mut Stage| super::choose(s, i))
}

fn tile(t: &Tokens, sh: &Arc<Shared>, r: &Row, i: usize, key: &str, on: bool, live: bool) -> El {
    let c = ink(t, on);
    hot(col().key(key.to_string()).w(tile_w()).h(TILE_H).items_center().gap(8.0).pt(12.0).px(4.0).role("button").label(r.title.clone()), live)
        .child(visual(t, sh, &r.visual, 48.0, on))
        .child(label(t, 12.0, 500.0, r.title.clone(), if on { c.main } else { c.soft }).truncate())
        .on_pointer_move_local(move |s: &mut Stage, _, tx: &mut Tx| {
            if s.launcher.actions.is_none() && s.launcher.sel != i {
                super::point_at(s, i);
            } else {
                tx.live_only();
            }
        })
        .on_press(move |s: &mut Stage| super::choose(s, i))
}

fn heading(t: &Tokens, title: &str) -> El {
    row().w_full().h(HEAD_H).px(14.0).pb(6.0).items_end().child(label(t, 11.0, 600.0, title.to_uppercase(), t.ink3))
}

fn prefix_chip(t: &Tokens, prefix: &'static str, mode: Mode, live: bool) -> El {
    let e = hot(row().key(format!("lp{prefix}")).grow(1.0).min_w(0.0).h(28.0).px(10.0).gap(7.0).center_y().role("button").label(mode.label()), live)
        .child(text(prefix.to_string()).font(TextStyle::new(12.0, 500.0).family(t.fonts.fig)).color(t.acc))
        .child(label(t, 12.5, 400.0, mode.label(), t.ink2).truncate())
        .on_press(move |s: &mut Stage| super::pick_prefix(s, prefix));
    match t.look {
        Look::Folio | Look::Phosphor => e.border(1.0, t.rule).hover(t.fill),
        _ => e.radius(t.radii.chip.min(10.0)).fill(t.raised).hover(t.fill),
    }
}

/// The prefixes as chips, four, three and three across, for the empty view.
fn prefix_chips(t: &Tokens, live: bool) -> El {
    let mut chips = col().w_full().h(CHIPS_H).px(4.0).pt(8.0).gap(6.0);
    let mut it = yozora_search::PREFIXES.iter();
    for n in [4, 3, 3] {
        let mut r = row().w_full().h(28.0).gap(6.0);
        for k in 0..4 {
            match (k < n).then(|| it.next()).flatten() {
                Some((p, m)) => r = r.child(prefix_chip(t, p, *m, live)),
                None => r = r.child(el().grow(1.0)),
            }
        }
        chips = chips.child(r);
    }
    chips
}

fn field(s: &Stage, t: &Tokens) -> El {
    let l = &s.launcher;
    let (size, weight, italic) = match t.look {
        Look::Folio => (24.0, 400.0, true),
        Look::Phosphor => (16.0, 400.0, false),
        Look::Prism => (22.0, 300.0, false),
        _ => (20.0, 400.0, false),
    };
    let mut style = TextStyle::new(size, weight).family(t.fonts.body);
    if italic {
        style = style.italic();
    }
    let part = |s: &str| text(s.to_string()).font(style.clone()).color(t.ink);
    let caret = if t.look == Look::Phosphor { el().size(9.0, 20.0).fill(t.ink) } else { el().size(2.0, size * 1.15).radius(1.0).fill(t.acc) };
    let mut f = row().grow(1.0).min_w(0.0).h(32.0).center_y();
    if l.text.is_empty() {
        let hint = match l.args_for {
            Some(i) => s.sh.engine().args_hint(i).unwrap_or_default(),
            None => PLACEHOLDER.to_string(),
        };
        let ph = text(hint).font(style.clone()).color(t.ink3).truncate();
        return f.child(caret).child(row().grow(1.0).min_w(0.0).h_full().center_y().child(ph));
    }
    let (a, b) = l.selection();
    // A long line shows its tail, where the caret usually is.
    let head = &l.text[..a];
    let n = head.chars().count();
    let head = if n > 56 { format!("…{}", head.chars().skip(n - 55).collect::<String>()) } else { head.to_string() };
    let sel = &l.text[a..b];
    if !head.is_empty() {
        f = f.child(part(&head));
    }
    if l.caret == a {
        f = f.child(caret.clone());
    }
    if !sel.is_empty() {
        f = f.child(part(sel).fill(alpha(t.acc, 0.3)));
    }
    if l.caret == b && a != b {
        f = f.child(caret);
    }
    if b < l.text.len() {
        f = f.child(part(&l.text[b..]));
    }
    f
}

fn search_bar(s: &Stage, t: &Tokens) -> El {
    let l = &s.launcher;
    let query = l.query();
    let (mode, _) = yozora_search::parse_mode(&query);
    let lead = if mode == Mode::All || l.args_for.is_some() {
        glyph_el(t, if l.args_for.is_some() { "term" } else { "search" }, 20.0, t.ink2)
    } else {
        let chip = row().h(28.0).px(10.0).gap(7.0).center_y()
            .child(glyph_el(t, glyph_name(mode.icon()), 14.0, t.acc))
            .child(label(t, 12.5, 600.0, mode.label(), t.acc));
        if plain(t) { chip.border(1.0, t.acc) } else { chip.radius(t.radii.chip.min(10.0)).fill(alpha(t.acc, 0.14)) }
    };
    let n = l.count();
    let mut bar = row().w_full().h(BAR_H).px(14.0).gap(14.0).center_y().child(lead).child(field(s, t));
    if !l.idle() && n > 0 && l.actions.is_none() {
        bar = bar.child(label(t, 12.0, 400.0, if n == 1 { "1 result".to_string() } else { format!("{n} results") }, t.ink3));
    }
    let esc = row().h(22.0).px(7.0).center().border(1.0, t.rule).child(text("esc").font(TextStyle::new(11.0, 500.0).family(t.fonts.fig)).color(t.ink3));
    let rule = el().pin(Some(0.0), None, Some(0.0), Some(0.0)).h(1.0);
    let rule = match t.look {
        Look::Folio => rule.fill(t.ink),
        Look::Phosphor => rule.border(1.0, t.rule).border_dash(4.0, 3.0),
        _ => rule.fill(t.rule),
    };
    bar.child(if plain(t) { esc } else { esc.radius(6.0) }).child(rule)
}

/// The Tab menu: the choices for one result.
fn actions(t: &Tokens, sh: &Arc<Shared>, a: &Actions, live: bool) -> (El, Geo) {
    let rects: Vec<[f32; 4]> = (0..a.items.len()).map(|k| [0.0, ACT_HEAD_H + k as f32 * ACT_H, INNER, ACT_H]).collect();
    let total = ACT_HEAD_H + a.items.len() as f32 * ACT_H;
    let mut c = col().w_full().h(total);
    if let Some(r) = rects.get(a.sel) {
        c = c.child(highlight(t, "lhl-a", *r, t.radii.ctl));
    }
    c = c.child(
        row().w_full().h(ACT_HEAD_H).px(14.0).gap(12.0).center_y()
            .child(visual(t, sh, &a.row.visual, 28.0, false))
            .child(label(t, 13.0, 600.0, a.row.title.clone(), t.ink).truncate().grow(1.0))
            .child(label(t, 12.0, 400.0, "Tab to go back", t.ink3)),
    );
    for (k, item) in a.items.iter().enumerate() {
        let on = k == a.sel;
        let cl = ink(t, on);
        let mut r = hot(row().key(format!("la{k}")).w_full().h(ACT_H).px(14.0).gap(12.0).center_y().role("button").label(item.label.clone()), live)
            .child(label(t, 13.5, 500.0, item.label.clone(), cl.main).grow(1.0).truncate());
        if !item.hint.is_empty() {
            r = r.child(text(item.hint.to_string()).font(TextStyle::new(11.0, 500.0).family(t.fonts.fig)).color(cl.faint));
        }
        c = c.child(
            r.on_pointer_move_local(move |s: &mut Stage, _, tx: &mut Tx| {
                if s.launcher.actions.as_ref().is_some_and(|a| a.sel != k) {
                    super::point_at_action(s, k);
                } else {
                    tx.live_only();
                }
            })
            .on_press(move |s: &mut Stage| super::choose_action(s, k)),
        );
    }
    (c, Geo { rects, total })
}

fn footer(s: &Stage, t: &Tokens) -> El {
    let l = &s.launcher;
    let current = l.selected();
    let left = if let Some(e) = &l.error {
        e.clone()
    } else if let Some(a) = &l.actions {
        format!("Actions for {}", a.row.title)
    } else if let Some(msg) = l.loading.filter(|_| l.count() > 0) {
        msg.to_string()
    } else {
        match current {
            Some(r) => match &r.target {
                Target::Web(_) => "Search the web".to_string(),
                Target::Copy(_) => "Copy the result".to_string(),
                other => format!("{} {}", other.verb(), r.title.trim_start_matches("= ")),
            },
            None if l.idle() => "Yozora Launch".to_string(),
            None => "No matches".to_string(),
        }
    };
    let hints: Vec<(&str, String)> = if l.actions.is_some() {
        vec![("↑↓", "Choose".into()), ("↵", "Run".into()), ("Tab", "Back".into())]
    } else if l.idle() {
        vec![("↑↓", "Move".into()), ("↵", "Open".into()), ("Ctrl P", "Pin".into())]
    } else {
        vec![("↵", current.map_or("Open", |r| r.target.verb()).to_string()), ("Tab", "Actions".into()), ("Ctrl ↵", "Web".into())]
    };
    let mut h = row().gap(16.0).center_y();
    for (k, v) in hints {
        h = h.child(row().gap(6.0).center_y().child(kbd(t, k, false)).child(label(t, 12.0, 400.0, v, t.ink2)));
    }
    let color = if l.error.is_some() { t.alert } else { t.ink2 };
    row().w_full().h(FOOT_H).px(14.0).gap(12.0).center_y().child(label(t, 12.0, 400.0, left, color).truncate().grow(1.0).min_w(0.0)).child(h)
}

/// The launcher. `live` is false for the nearly invisible frame drawn at startup to warm things
/// up, which must not take clicks.
fn build(s: &Stage, t: &Tokens, live: bool) -> El {
    let l = &s.launcher;
    let (_, fx) = s.sh.look();
    if live && let Some(at) = REQUEST.lock().unwrap().take() {
        tracing::info!("launcher: first frame built {:?} after the request", at.elapsed());
    }

    let message = if l.sections.is_empty() && l.args_for.is_none() {
        match l.loading {
            Some(m) => format!("{m}…"),
            None => yozora_search::empty_message(&l.query()),
        }
    } else {
        String::new()
    };
    let (content, geo) = if let Some(a) = &l.actions {
        actions(t, &s.sh, a, live)
    } else {
        let geo = geometry(l, !message.is_empty());
        let arrival = arrival(t, fx, l);
        let mut c = col().w_full().h(geo.total);
        if let Some(r) = geo.rects.get(l.sel) {
            let radius = if l.idle() && r[3] == TILE_H { t.radii.card } else { t.radii.ctl };
            c = c.child(highlight(t, "lhl", *r, radius));
        }
        let mut i = 0;
        for sec in &l.sections {
            c = c.child(heading(t, sec.title));
            if tiles_in(l, sec) {
                let mut grid = row().w_full().h(TILE_H).px(4.0).gap(TILE_GAP);
                for r in &sec.rows {
                    let key = l.keys.get(i).cloned().unwrap_or_default();
                    grid = grid.child(arrive(&arrival, tile(t, &s.sh, r, i, &key, i == l.sel, live), i));
                    i += 1;
                }
                c = c.child(grid);
            } else {
                for r in &sec.rows {
                    let key = l.keys.get(i).cloned().unwrap_or_default();
                    c = c.child(arrive(&arrival, result_row(t, &s.sh, r, i, &key, i == l.sel, live), i));
                    i += 1;
                }
            }
        }
        if l.idle() {
            c = c.child(prefix_chips(t, live));
        } else if !message.is_empty() {
            c = c.child(col().w_full().h(MSG_H).px(14.0).pt(20.0).child(label(t, 13.0, 400.0, message, t.ink2)));
        }
        (c, geo)
    };

    let body_h = geo.total.min(BODY_MAX);
    let body = col().key("lscroll").w_full().h(body_h).scroll(ScrollSpec { stick_bottom: false }).scroll_margin(HEAD_H).child(content);
    card(t, W).child(search_bar(s, t)).child(body).child(footer(s, t))
}

pub fn view(s: &Stage, t: &Tokens) -> El {
    build(s, t, true)
}

/// The launcher with nothing in it that takes input, for the startup warm-up frame.
pub fn warm_view(s: &Stage, t: &Tokens) -> El {
    build(s, t, false)
}
