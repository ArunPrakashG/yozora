//! The Tray popover: an app's right-click menu. Submenus are shown flat, their entries indented
//! under the parent's name.

use std::sync::Arc;

use rune::{El, ScrollSpec, col, el, row};
use yozora_services::tray::{Cmd, Icon, Item, MenuItem, Status, Toggle};
use yozora_style::{Look, Tokens};

use crate::kit::{cell, glyph_el, label, panel};
use crate::media::images;
use crate::shared::Shared;
use crate::sound::{alpha, note};
use crate::stage::Stage;

pub const W: f32 = 260.0;
const ROW_H: f32 = 42.0;
/// Menus taller than this scroll.
const MAX_H: f32 = 520.0;

/// Menu labels mark mnemonics with underscores: "_File" and "Save __as" become "File" and "Save _as".
fn plain(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '_' {
            if chars.peek() == Some(&'_') {
                out.push('_');
                chars.next();
            }
        } else {
            out.push(c);
        }
    }
    out
}

pub(crate) fn app_icon(t: &Tokens, sh: &Arc<Shared>, item: &Item) -> El {
    let img = |image: rune::Image| el().size(18.0, 18.0).radius(t.radii.chip).pixels_fit(image.pixels(), rune::SurfaceFit::Fit);
    match &item.icon {
        Some(Icon::Builtin(name)) => glyph_el(t, name, 18.0, t.ink),
        Some(Icon::File(path)) => img(images(sh).file(path, 18.0, 2.0)),
        Some(Icon::Pixmap(p)) => img(images(sh).rgba(p.width, p.height, p.rgba.as_ref().clone())),
        Some(Icon::Symbolic(_)) | None => glyph_el(t, "sync", 18.0, t.ink),
    }
}

fn header(t: &Tokens, sh: &Arc<Shared>, item: &Item) -> El {
    let words = col().grow(1.0).min_w(0.0).gap(2.0)
        .child(label(t, 15.0, 600.0, item.title.clone(), t.ink).truncate())
        .children((item.status == Status::NeedsAttention).then(|| label(t, 12.0, 400.0, "Needs attention", t.ink2)));
    row().w_full().min_h(48.0).px(14.0).gap(14.0).center_y().child(cell(18.0, 18.0, app_icon(t, sh, item))).child(words)
}

enum Entry {
    Rule,
    Row(El),
}

fn flatten(t: &Tokens, key: &str, items: &[MenuItem], depth: usize, out: &mut Vec<Entry>) {
    let marks = items.iter().any(|i| i.toggle != Toggle::None);
    let indent = 14.0 + 14.0 * depth as f32;
    for item in items {
        if item.separator {
            out.push(Entry::Rule);
            continue;
        }
        let name = plain(&item.label);
        let ink = if item.enabled { t.ink } else { t.ink3 };
        if !item.children.is_empty() {
            out.push(Entry::Row(row().w_full().h(34.0).px(14.0).pl(indent).center_y().child(label(t, 12.0, 500.0, name, if item.enabled { t.ink2 } else { t.ink3 }).truncate())));
            if item.enabled {
                flatten(t, key, &item.children, depth + 1, out);
            }
            continue;
        }
        let mut r = row().w_full().h(ROW_H).px(14.0).pl(indent).gap(10.0).center_y().radius(t.radii.ctl);
        if marks {
            let on = matches!(item.toggle, Toggle::Check(true) | Toggle::Radio(true));
            r = r.child(cell(16.0, 16.0, if on { glyph_el(t, "check", 15.0, t.acc) } else { el() }));
        }
        r = r.child(label(t, 14.0, 500.0, name.clone(), ink).grow(1.0).truncate()).key(format!("item{}", item.id)).role("button").label(name);
        if item.enabled {
            let (key, id) = (key.to_string(), item.id);
            r = r.hit().hover(t.fill).on_press(move |s: &mut Stage| {
                if let Some(tray) = &s.sh.services.lock().unwrap().tray {
                    tray.send(Cmd::MenuClick(key.clone(), id));
                }
                s.open = None;
            });
        } else {
            r = r.disabled(true);
        }
        out.push(Entry::Row(r));
    }
}

fn rule(t: &Tokens) -> El {
    let line = match t.look {
        Look::Phosphor => el().w_full().h(1.0).border(1.0, t.rule).border_dash(3.0, 3.0),
        Look::Folio => el().w_full().h(1.0).fill(alpha(t.ink, 0.25)),
        _ => el().w_full().h(1.0).fill(t.rule),
    };
    row().w_full().px(12.0).py(6.0).child(line)
}

pub fn view(s: &Stage, t: &Tokens) -> El {
    let tray = s.sh.services.lock().unwrap().tray.clone();
    let state = tray.as_ref().map(|x| x.read().state.clone()).unwrap_or_default();
    let p = panel(t, W);
    let Some((key, menu)) = state.menu.as_ref() else {
        return p.child(note(t, if state.items.is_empty() { "No tray items." } else { "Loading menu…" }));
    };
    let mut entries = Vec::new();
    flatten(t, key, menu, 0, &mut entries);
    // A rule only divides two rows.
    let mut rows: Vec<El> = Vec::new();
    let mut pending = false;
    for e in entries {
        match e {
            Entry::Rule => pending = !rows.is_empty(),
            Entry::Row(r) => {
                if std::mem::take(&mut pending) {
                    rows.push(rule(t));
                }
                rows.push(r);
            }
        }
    }
    let mut p = p;
    if let Some(item) = state.items.iter().find(|i| &i.key == key) {
        p = p.child(header(t, &s.sh, item));
        if !rows.is_empty() {
            p = p.child(rule(t));
        }
    }
    if rows.is_empty() {
        return p.child(note(t, "This app has no menu."));
    }
    // Each row and the gap after it.
    let tall = rows.len() as f32 * (ROW_H + 10.0) > MAX_H;
    let list = col().w_full().gap(10.0).children(rows);
    p.child(if tall { list.h(MAX_H).scroll(ScrollSpec { stick_bottom: false }) } else { list })
}
