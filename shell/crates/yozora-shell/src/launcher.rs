//! The launcher: one line of text over a list of what it finds. Typing, arrows, Enter and Esc
//! are handled here from the stage's key events, so it works the moment it opens.

use std::sync::{Arc, Mutex};

use rune::{Color, El, Image, Key, KeyEvent, SurfaceFit, col, el, row};
use yozora_search::{Modifier, Outcome, Row, Section, Visual};
use yozora_style::{Look, Tokens};

use crate::kit::{cell, glyph_el, label};
use crate::shared::Shared;
use crate::stage::Stage;

pub const W: f32 = 620.0;
const ROW_H: f32 = 52.0;
const SHOWN: usize = 7;

#[derive(Default)]
pub struct Launcher {
    pub query: String,
    pub sel: usize,
    pub sections: Vec<Section>,
    generation: u64,
}

impl Launcher {
    pub fn rows(&self) -> Vec<&Row> {
        self.sections.iter().flat_map(|s| s.rows.iter()).collect()
    }

    fn selected(&self) -> Option<Row> {
        self.rows().get(self.sel).map(|r| (*r).clone())
    }
}

/// Fills in what the query finds. Called on every change and whenever the engine says it has more.
pub fn search(s: &mut Stage) {
    let engine = s.sh.engine();
    s.launcher.sections = engine.search(&s.launcher.query);
    s.launcher.generation = engine.generation();
    let n = s.launcher.rows().len();
    s.launcher.sel = s.launcher.sel.min(n.saturating_sub(1));
}

pub fn open(s: &mut Stage) {
    s.launcher = Launcher::default();
    search(s);
}

/// Searches again if the engine found more since the last draw.
pub fn sync(s: &mut Stage) {
    if s.sh.engine().generation() != s.launcher.generation {
        search(s);
    }
}

fn glyph_name(g: yozora_search::Glyph) -> &'static str {
    use yozora_search::Glyph::*;
    match g {
        Search => "search",
        Window => "monitor",
        Clipboard | Package | File => "disk",
        Folder => "folder",
        Smile => "sun",
        Calculator => "gauge",
        Shell | Code | Terminal => "term",
        Globe | Link => "globe",
        Star => "check",
        Clock | Reset => "sync",
        Image => "shot",
        Lock => "lock",
        Moon => "moon",
        Settings => "gear",
        Power => "power",
        Logout => "arrow",
        Music => "music",
    }
}

/// What a key does. Returns whether it was used.
pub fn on_key(s: &mut Stage, e: &KeyEvent) -> bool {
    let n = s.launcher.rows().len();
    match &e.key {
        Key::Escape => s.open = None,
        Key::Up => s.launcher.sel = s.launcher.sel.saturating_sub(1),
        Key::Down => s.launcher.sel = (s.launcher.sel + 1).min(n.saturating_sub(1)),
        Key::Tab => s.launcher.sel = if n == 0 { 0 } else { (s.launcher.sel + 1) % n },
        Key::Backspace => {
            if e.mods.cmd || e.mods.alt {
                s.launcher.query.clear();
            } else {
                s.launcher.query.pop();
            }
            s.launcher.sel = 0;
            search(s);
        }
        Key::Enter => run(s, if e.mods.shift { Modifier::Shift } else if e.mods.alt { Modifier::Alt } else { Modifier::None }),
        Key::Command('u') => {
            s.launcher.query.clear();
            search(s);
        }
        Key::Text(t) if !e.mods.cmd => {
            s.launcher.query.push_str(t);
            s.launcher.sel = 0;
            search(s);
        }
        _ => return false,
    }
    true
}

pub fn run(s: &mut Stage, modifier: Modifier) {
    let Some(row) = s.launcher.selected() else { return };
    let engine = s.sh.engine();
    let act = engine.default_act(&row, modifier);
    match engine.activate(&row, act, None) {
        Ok(Outcome::Launched) => s.open = None,
        Ok(Outcome::Updated) => search(s),
        Ok(_) => {}
        Err(e) => tracing::warn!("launcher: {e:#}"),
    }
}

/// Keeps the images of the rows on screen alive for as long as they are drawn.
fn icon(t: &Tokens, sh: &Arc<Shared>, row: &Row) -> El {
    static HELD: Mutex<Vec<Image>> = Mutex::new(Vec::new());
    let tile = |e: El| cell(36.0, 36.0, e).radius(t.radii.ctl).fill(t.raised);
    match &row.visual {
        Visual::Image(path) => {
            let img = crate::media::images(sh).file(path.as_path(), 36.0, 1.0);
            let e = el().size(36.0, 36.0).radius(t.radii.ctl).pixels_fit(img.pixels(), SurfaceFit::Fit);
            let mut held = HELD.lock().unwrap();
            if held.len() > 64 {
                held.clear();
            }
            held.push(img);
            e
        }
        Visual::Glyph(g) => tile(glyph_el(t, glyph_name(*g), 18.0, t.ink2)),
        Visual::Emoji(s) => tile(label(t, 20.0, 400.0, s.clone(), t.ink)),
        Visual::Letter(c) => tile(label(t, 16.0, 600.0, c.to_string(), t.ink2)),
    }
}

fn item(t: &Tokens, sh: &Arc<Shared>, row: &Row, index: usize, on: bool) -> El {
    let boxed = matches!(t.look, Look::Folio | Look::Phosphor);
    let mut r = row_base(t, on).key(format!("row{index}")).role("button").label(row.title.clone()).hit()
        .child(icon(t, sh, row))
        .child(
            col().grow(1.0).gap(1.0).justify_center()
                .child(label(t, 14.0, 600.0, row.title.clone(), t.ink).truncate())
                .child(if row.sub.is_empty() { el().h(0.0) } else { label(t, 12.0, 400.0, row.sub.clone(), t.ink2).truncate() }),
        )
        .on_press(move |s: &mut Stage| {
            s.launcher.sel = index;
            run(s, Modifier::None);
        });
    if let Some(b) = &row.badge {
        r = r.child(label(t, 12.0, 500.0, b.clone(), t.ink3));
    }
    if on {
        r = r.child(label(t, 12.0, 500.0, row.target.verb(), t.ink2));
    }
    let _ = boxed;
    r
}

fn row_base(t: &Tokens, on: bool) -> El {
    let r = row().w_full().h(ROW_H).px(8.0).gap(12.0).center_y().radius(t.radii.ctl);
    if on { r.fill(t.fill) } else { r }
}

pub fn view(s: &Stage, t: &Tokens) -> El {
    let l = &s.launcher;
    let mut list = col().w_full().gap(2.0).padding(8.0);
    let mut index = 0;
    let mut shown = 0;
    for sec in &l.sections {
        if shown >= SHOWN {
            break;
        }
        list = list.child(label(t, 11.0, 500.0, sec.title, t.ink3).padding(8.0));
        for r in &sec.rows {
            if shown >= SHOWN {
                break;
            }
            list = list.child(item(t, &s.sh, r, index, index == l.sel));
            index += 1;
            shown += 1;
        }
    }
    if index == 0 {
        list = list.child(label(t, 13.0, 400.0, yozora_search::empty_message(&l.query), t.ink2).padding(16.0));
    }
    let caret = el().w(2.0).h(22.0).fill(t.acc);
    let mut input = row().w_full().h(64.0).px(22.0).gap(14.0).center_y().child(glyph_el(t, "search", 18.0, t.ink2));
    input = if l.query.is_empty() {
        input.child(caret).child(label(t, 20.0, 400.0, "Search apps, files, windows, math", t.ink3).pin(Some(58.0), Some(19.0), None, None))
    } else {
        input.child(label(t, 20.0, 500.0, l.query.clone(), t.ink)).child(caret)
    };
    let shell = col().w(W).radius(t.radii.panel).fill(t.surf).border(1.0, t.rule).clip_rounded(t.radii.panel)
        .shadow(24.0, 64.0, Color { r: 0.0, g: 0.0, b: 0.0, a: 0.45 })
        .child(input)
        .child(el().w_full().h(1.0).fill(t.rule))
        .child(list);
    shell
}
