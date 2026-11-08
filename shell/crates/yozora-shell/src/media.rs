//! The Now playing popover: art, what is playing, a progress track, transport, and a picker when
//! more than one player is running.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::{Duration, Instant};

use rune::tok::TextStyle;
use rune::{Color, Drag, El, Image, ImageCache, Phase, SurfaceFit, Tx, col, row, text};
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

/// While a song plays and this screen is drawn, asks for one more draw a second later so the
/// position keeps moving. At most one short timer runs; a closed popover or a pause lets it lapse.
pub(crate) fn keep_time(sh: &Arc<Shared>) {
    static PENDING: AtomicBool = AtomicBool::new(false);
    if PENDING.swap(true, Ordering::AcqRel) {
        return;
    }
    let sh = sh.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(1));
        PENDING.store(false, Ordering::Release);
        sh.redraw();
    });
}

/// Where the pointer has the position while seeking, 0 to 1, and when it let go. The track keeps
/// showing it for a moment after, until the player reports the new position.
static SCRUB: Mutex<Option<(f32, Option<Instant>)>> = Mutex::new(None);

fn scrubbed() -> Option<f32> {
    let mut held = SCRUB.lock().unwrap();
    match *held {
        Some((f, None)) => Some(f),
        Some((f, Some(at))) if at.elapsed() < Duration::from_millis(700) => Some(f),
        Some(_) => {
            *held = None;
            None
        }
        None => None,
    }
}

fn clock(d: Duration) -> String {
    let s = d.as_secs();
    if s >= 3600 { format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60) } else { format!("{}:{:02}", s / 60, s % 60) }
}

/// Draws `p`'s cover into `base` (a box of height `px`), or a note glyph when it has none.
pub(crate) fn cover(t: &Tokens, sh: &Arc<Shared>, p: &Player, base: El, px: f32) -> El {
    static HELD: Mutex<Vec<(u32, Image)>> = Mutex::new(Vec::new());
    match &p.art {
        Some(path) => {
            let img = images(sh).file(path, px, if px > 100.0 { 1.0 } else { 2.0 });
            let e = base.pixels_fit(img.pixels(), SurfaceFit::Fill);
            // The cache drops a picture nobody holds, so the latest one per size stays held.
            let mut held = HELD.lock().unwrap();
            held.retain(|(k, _)| *k != px as u32);
            held.push((px as u32, img));
            e
        }
        None => base.child(glyph_el(t, "music", (px * 0.4).clamp(16.0, 28.0), t.ink3)),
    }
}

fn art(t: &Tokens, sh: &Arc<Shared>, p: &Player) -> El {
    let base = row().w_full().h(ART_H).center().radius(t.radii.card).fill(t.surf2);
    let base = match t.look {
        Look::Folio => base.border(1.0, t.ink),
        Look::Phosphor => base.border(1.0, t.rule),
        _ => base,
    };
    cover(t, sh, p, base, ART_H)
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

/// Elapsed, the track, and the length. Nothing for a stream that has no length. A player that
/// can seek gets a track you click or drag, and the arrow keys move it by five seconds.
fn scrub(t: &Tokens, p: &Player) -> Option<El> {
    let length = p.length.filter(|l| !l.is_zero())?;
    let pos = p.position_now().min(length);
    let frac = scrubbed().unwrap_or(pos.as_secs_f32() / length.as_secs_f32());
    let shown = length.mul_f32(frac);
    let label_w = if length.as_secs() >= 3600 { 48.0 } else { 34.0 };
    let track_w = W - 36.0 - 16.0 - 2.0 * label_w - 24.0;
    let mut bar = row().w(track_w).h(if t.look == Look::Prism { 14.0 } else { 20.0 }).center_y().child(track(t, track_w, frac, true));
    if p.can_seek {
        bar = bar.key("seek").role("slider").label("Seek").hit()
            .on_drag(move |s: &mut Stage, d: &mut Drag, _: &mut Tx| {
                if d.w <= 0.0 {
                    return;
                }
                let f = (d.x / d.w).clamp(0.0, 1.0);
                match d.phase {
                    Phase::Up if d.cancelled() => *SCRUB.lock().unwrap() = None,
                    Phase::Up => {
                        *SCRUB.lock().unwrap() = Some((f, Some(Instant::now())));
                        send(s, Cmd::SeekTo(length.mul_f32(f)));
                    }
                    _ => *SCRUB.lock().unwrap() = Some((f, None)),
                }
            })
            .on_step(move |s: &mut Stage, by, _: &mut Tx| {
                let to = (pos.as_secs_f32() + by as f32 * 5.0).clamp(0.0, length.as_secs_f32());
                send(s, Cmd::SeekTo(Duration::from_secs_f32(to)));
            });
    }
    Some(
        row().w_full().px(8.0).py(4.0).gap(12.0).center_y()
            .child(row().w(label_w).child(fig(t, 12.0, 500.0, clock(shown), t.ink2)))
            .child(bar)
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
            chip(t, format!("player{i}"), &pl.identity, Some(if pl.status == Status::Playing { "play" } else { "music" }), pl.bus == p.bus, move |s: &mut Stage| send(s, Cmd::Select(bus.clone())))
        }));
        out = out.child(section(t, "Players")).child(chips);
    }
    out
}
