//! The Now playing popover: art, what is playing, a progress track, transport, and a picker when
//! more than one player is running.

use std::sync::{Arc, Mutex, Once, OnceLock, Weak};
use std::time::{Duration, Instant};

use rune::tok::TextStyle;
use rune::{Color, El, Image, ImageCache, SurfaceFit, col, row, text};
use yozora_services::media::{Cmd, Player, Status};
use yozora_style::{Look, Tokens};

use crate::kit::{cell, fig, glyph_el, label, panel};
use crate::shared::Shared;
use crate::sound::{CLEAR, alpha, note, section, title, track};
use crate::stage::{Pop, Stage};

pub const W: f32 = 360.0;
const ART_H: f32 = 184.0;

/// One image cache for every screen that shows pictures (album art, tray icons). A finished
/// decode redraws the shell.
pub(crate) fn images(sh: &Arc<Shared>) -> &'static ImageCache {
    static CACHE: OnceLock<ImageCache> = OnceLock::new();
    CACHE.get_or_init(|| {
        let sh: Weak<Shared> = Arc::downgrade(sh);
        ImageCache::new(Arc::new(move || {
            if let Some(sh) = sh.upgrade() {
                sh.redraw();
            }
        }))
    })
}

/// Redraws once a second for as long as the popover keeps being drawn while a song plays, so the
/// position moves. It stops by itself when the popover closes or the song pauses.
fn keep_time(sh: &Arc<Shared>) {
    static LAST: Mutex<Option<(Instant, Weak<Shared>)>> = Mutex::new(None);
    static THREAD: Once = Once::new();
    *LAST.lock().unwrap() = Some((Instant::now(), Arc::downgrade(sh)));
    THREAD.call_once(|| {
        let spawned = std::thread::Builder::new().name("yz-seek".into()).spawn(|| loop {
            std::thread::sleep(Duration::from_secs(1));
            let live = LAST.lock().unwrap().as_ref().and_then(|(at, sh)| (at.elapsed() < Duration::from_millis(1500)).then(|| sh.upgrade()).flatten());
            if let Some(sh) = live {
                sh.redraw();
            }
        });
        let _ = spawned;
    });
}

fn clock(d: Duration) -> String {
    let s = d.as_secs();
    if s >= 3600 { format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60) } else { format!("{}:{:02}", s / 60, s % 60) }
}

fn art(t: &Tokens, sh: &Arc<Shared>, p: &Player) -> El {
    static HELD: Mutex<Option<Image>> = Mutex::new(None);
    let base = row().w_full().h(ART_H).center().radius(t.radii.card).fill(t.surf2);
    let base = match t.look {
        Look::Folio => base.border(1.0, t.ink),
        Look::Phosphor => base.border(1.0, t.rule),
        _ => base,
    };
    match &p.art {
        Some(path) => {
            let img = images(sh).file(path, ART_H, 1.0);
            let e = base.pixels_fit(img.pixels(), SurfaceFit::Fill);
            *HELD.lock().unwrap() = Some(img);
            e
        }
        None => base.child(glyph_el(t, "music", 28.0, t.ink3)),
    }
}

/// A pill: the output name, or one player in the picker. `on` marks the chosen one.
fn chip(t: &Tokens, key: String, name: &str, glyph: Option<&str>, on: bool, press: impl Fn(&mut Stage) + 'static) -> El {
    let boxed = matches!(t.look, Look::Folio | Look::Phosphor);
    let (bg, fg) = match (t.look, on) {
        (Look::Folio | Look::Phosphor, true) => (t.ink, t.on_ink),
        (Look::Folio | Look::Phosphor, false) => (CLEAR, t.ink),
        (Look::Prism, true) => (t.acc, t.on_acc),
        (Look::Prism, false) => (Color { r: 1.0, g: 1.0, b: 1.0, a: 0.9 }, t.ink),
        (_, true) => (t.fill, t.ink),
        (_, false) => (t.raised, t.ink),
    };
    let name_el = match t.look {
        Look::Folio => text(name.to_uppercase()).font(TextStyle::new(11.0, 500.0).family(t.fonts.fig).tracking(0.9)).color(fg),
        Look::Phosphor => label(t, 13.0, 400.0, name, fg),
        _ => label(t, 13.0, 500.0, name, fg),
    }
    .truncate();
    let mut r = row().h(34.0).max_w(220.0).px(if t.look == Look::Phosphor { 10.0 } else { 16.0 }).gap(6.0).center_y()
        .radius(t.radii.pill).fill(bg)
        .key(key).role("button").label(name.to_string()).hit().on_press(press);
    if t.look == Look::Phosphor {
        r = r.child(label(t, 13.0, 400.0, "[", fg));
    }
    if let Some(g) = glyph {
        r = r.child(cell(15.0, 15.0, glyph_el(t, g, 15.0, fg)));
    }
    r = r.child(name_el);
    if t.look == Look::Phosphor {
        r = r.child(label(t, 13.0, 400.0, "]", fg));
    }
    if boxed {
        r = r.border(1.0, t.ink);
    } else if t.look == Look::Prism && !on {
        r = r.shadow(1.0, 3.0, Color { r: 0.02, g: 0.03, b: 0.12, a: 0.12 });
    }
    r
}

/// Elapsed, the track, and the length. Nothing for a stream that has no length.
fn scrub(t: &Tokens, p: &Player) -> Option<El> {
    let length = p.length.filter(|l| !l.is_zero())?;
    let pos = p.position_now().min(length);
    let label_w = if length.as_secs() >= 3600 { 48.0 } else { 34.0 };
    let track_w = W - 36.0 - 16.0 - 2.0 * label_w - 24.0;
    Some(
        row().w_full().px(8.0).py(4.0).gap(12.0).center_y()
            .child(row().w(label_w).child(fig(t, 12.0, 500.0, clock(pos), t.ink2)))
            .child(track(t, track_w, pos.as_secs_f32() / length.as_secs_f32(), true))
            .child(row().w(label_w).justify_end().child(fig(t, 12.0, 500.0, clock(length), t.ink2))),
    )
}

fn transport(t: &Tokens, p: &Player) -> El {
    let step = |key: &'static str, name: &str, can: bool, cmd: fn() -> Cmd| {
        let b = cell(44.0, 44.0, glyph_el(t, name, 20.0, t.ink)).key(key).role("button").label(key).radius(t.radii.pill);
        if can {
            b.hit().hover(t.fill).on_press(move |s: &mut Stage| send(s, cmd()))
        } else {
            b.opacity(0.4).disabled(true)
        }
    };
    let playing = p.status == Status::Playing;
    let (bg, fg) = match t.look {
        Look::Prism => (t.acc, t.on_acc),
        Look::Tension | Look::Daybreak => (t.ink, t.surf),
        _ => (t.ink, t.on_ink),
    };
    let mut big = cell(56.0, 56.0, glyph_el(t, if playing { "pause" } else { "play" }, 22.0, fg))
        .key("play").role("button").label(if playing { "Pause" } else { "Play" }).hit()
        .radius(t.radii.pill).fill(bg).on_press(|s: &mut Stage| send(s, Cmd::PlayPause));
    if t.look == Look::Prism {
        big = big.shadow(10.0, 20.0, alpha(t.acc, 0.5));
    }
    row().w_full().justify_center().center_y().gap(24.0).pt(2.0).pb(4.0)
        .child(step("prev", "prev", p.can_previous, || Cmd::Previous))
        .child(big)
        .child(step("next", "next", p.can_next, || Cmd::Next))
}

fn send(s: &Stage, cmd: Cmd) {
    if let Some(m) = &s.sh.services.lock().unwrap().media {
        m.send(cmd);
    }
}

pub fn view(s: &Stage, t: &Tokens) -> El {
    let (media, audio) = {
        let svc = s.sh.services.lock().unwrap();
        (svc.media.clone(), svc.audio.clone())
    };
    let heading = || panel(t, W).child(title(t, "Now playing"));
    let Some(media) = media else { return heading().child(note(t, "Media is not available.")) };
    let state = media.read().state.clone();
    let Some(p) = state.current().cloned() else {
        return heading().child(note(t, "Nothing is playing. Players that support MPRIS show up here."));
    };
    if p.status == Status::Playing {
        keep_time(&s.sh);
    }

    let name = if p.title.is_empty() { p.identity.clone() } else { p.title.clone() };
    let by = [p.artist.as_str(), p.album.as_str()].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" · ");
    let sub_size = match t.look {
        Look::Folio => 15.0,
        Look::Phosphor => 13.0,
        _ => 14.0,
    };
    let words = col().grow(1.0).min_w(0.0).gap(5.0)
        .child(label(t, 18.0, 600.0, name, t.ink).truncate())
        .children((!by.is_empty()).then(|| label(t, sub_size, 400.0, by, t.ink2).wrap().max_lines(2)));
    // The output the sound goes to; pressing it opens the sound popover to change it.
    let output = audio.as_ref().and_then(|a| a.read().state.sink().map(|d| (d.description.clone(), d.kind)));
    let mut head = row().w_full().px(8.0).pt(8.0).gap(12.0).items_end().child(words);
    if let Some((name, kind)) = output {
        let glyph = match kind {
            yozora_services::audio::DeviceKind::Headphones => "phones",
            yozora_services::audio::DeviceKind::Display => "monitor",
            _ => "speaker",
        };
        head = head.child(chip(t, "output".into(), &name, Some(glyph), false, |s: &mut Stage| s.open = Some(Pop::Sound)));
    }

    let mut out = panel(t, W).child(art(t, &s.sh, &p)).child(head);
    out = out.children(scrub(t, &p)).child(transport(t, &p));
    if state.players.len() > 1 {
        let chips = row().w_full().px(4.0).flex_wrap().gaps(8.0, 8.0).children(state.players.iter().enumerate().map(|(i, pl)| {
            let bus = pl.bus.clone();
            chip(t, format!("player{i}"), &pl.identity, None, pl.bus == p.bus, move |s: &mut Stage| send(s, Cmd::Select(bus.clone())))
        }));
        out = out.child(section(t, "Players")).child(chips);
    }
    out
}
