//! The lock screen. One `ext-session-lock` surface per display; the password goes to PAM through
//! `yozora-lockd`, and the session unlocks only when PAM says yes.
//!   yozora-lock            lock the session now
//!   yozora-lock --preview  draw the screen on an overlay, Escape closes it
//!   yozora-lock status     whether the session is locked

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;

use rune::tok::TextStyle;
use rune::{
    App, AppSpec, Color, Cx, DisplayTarget, El, ImageCache, Key, KeyEvent, KeyPolicy, OverlayHandle, OverlayInput, OverlayOptions, OverlayPlacement, OverlaySpec,
    RunOptions, Spring, SpringPair, SurfaceFit, Transition, Tx, WaylandLayer, WindowSpec, col, el, row, text,
};
use yozora_lockd::glance::MediaCmd;
use yozora_lockd::power::{Power, Press};
use yozora_lockd::system::Network;
use yozora_lockd::{Engine, Phase, Tone, cli};
use yozora_shell::kit::{cell, fig, glyph_el, label};
use yozora_style::{FONTS, Look, Tokens, tokens};

struct Lock {
    engine: Arc<Mutex<Engine>>,
    look: Look,
    output: String,
    /// Which of the displays this is, in the order they were locked.
    index: usize,
    size: (f32, f32),
    wallpaper: Arc<Mutex<Option<PathBuf>>>,
    leaving: bool,
    /// Set for an instant after a wrong password: the pill is thrown aside and springs back.
    kick: bool,
    preview: bool,
}

/// The display that carries the form and the extras: the one with the keyboard, which starts as
/// the focused monitor. Every other display shows the clock and the wallpaper only.
static MAIN: AtomicUsize = AtomicUsize::new(0);

type Handles = Mutex<Vec<OverlayHandle<Lock>>>;

fn handles() -> &'static Handles {
    static H: OnceLock<Handles> = OnceLock::new();
    H.get_or_init(Default::default)
}

fn redraw() {
    for h in handles().lock().unwrap().iter() {
        h.update(|_, _| {});
    }
}

fn images() -> &'static ImageCache {
    static C: OnceLock<ImageCache> = OnceLock::new();
    C.get_or_init(|| ImageCache::new(Arc::new(redraw)))
}

fn host(_: &(), _: &mut Cx<()>) -> El {
    col()
}

fn on_key(s: &mut Lock, e: &KeyEvent, _: &mut Tx) -> bool {
    if s.leaving {
        return true;
    }
    // Typing on a display makes it the one with the form.
    if MAIN.swap(s.index, Ordering::Relaxed) != s.index {
        redraw();
    }
    let mut engine = s.engine.lock().unwrap();
    match &e.key {
        Key::Escape if s.preview => std::process::exit(0),
        Key::Escape => {
            engine.auth.clear();
        }
        Key::Backspace => {
            engine.auth.backspace();
        }
        Key::Enter => {
            engine.submit();
        }
        Key::Command('u') => {
            engine.auth.clear();
        }
        Key::Text(t) if !e.mods.cmd => {
            engine.auth.push(t);
        }
        _ => return false,
    }
    drop(engine);
    redraw();
    true
}

/// How big the clock is for each look, out of a 900 point tall screen.
/// The form's height (avatar, name, field and hint) and its distance from the bottom edge.
const FORM_H: f32 = 200.0;
const FORM_BOTTOM: f32 = 100.0;

fn clock_size(look: Look) -> f32 {
    match look {
        Look::Tension | Look::Daybreak => 240.0,
        Look::Folio => 260.0,
        Look::Phosphor => 300.0,
        Look::Prism => 230.0,
    }
}

fn backdrop(s: &Lock, t: &Tokens) -> El {
    let base = el().pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).fill(t.bg);
    let path = s.wallpaper.lock().unwrap().clone();
    match (s.look, path) {
        (Look::Folio | Look::Phosphor, _) | (_, None) => base,
        (look, Some(path)) => {
            let img = images().file(&path, s.size.1, 1.0);
            let tint = if look == Look::Prism { Color { r: 1.0, g: 1.0, b: 1.0, a: 0.35 } } else { Color { r: 0.02, g: 0.02, b: 0.04, a: 0.28 } };
            base.pixels_fit(img.pixels(), SurfaceFit::Fill).child(el().pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).fill(tint))
        }
    }
}

fn dots(t: &Tokens, n: usize) -> El {
    let shown = n.min(14);
    let radius = if t.look == Look::Phosphor { 0.0 } else { 4.0 };
    let pop = Transition::scale(0.4).allow_small_scale().combined(Transition::opacity())
        .springs(SpringPair::new(Spring::new(0.28, 0.35), Spring::new(0.12, 0.0)));
    row().gap(8.0).center_y().children((0..shown).map(|i| el().key(format!("dot{i}")).size(7.0, 7.0).radius(radius).fill(t.ink).transition(pop)))
}

fn form(s: &Lock, t: &Tokens) -> El {
    let engine = s.engine.lock().unwrap();
    let now = Instant::now();
    let (tone, hint) = match engine.auth.waiting_for(now) {
        Some(d) => (Tone::Bad, format!("Too many tries · {}", yozora_lockd::countdown(d))),
        None => engine.auth.hint(caps_lock()),
    };
    let hint_color = match tone {
        Tone::Plain => t.ink3,
        Tone::Warn => t.acc,
        Tone::Bad => t.alert,
    };
    let initial = engine.user.initial().to_string();
    let name = engine.user.login.clone();
    let n = engine.auth.dots();
    let busy = engine.auth.phase == Phase::Checking;
    drop(engine);

    let boxed = matches!(t.look, Look::Folio | Look::Phosphor);
    let mut pill = row().w(300.0).h(44.0).pl(20.0).pr(6.0).gap(8.0).center_y().radius(t.radii.pill).fill(t.fill);
    pill = if boxed { pill.border(1.0, t.ink) } else { pill.border(1.0, t.rule) };
    let go = row().size(32.0, 32.0).center().radius(t.radii.pill).fill(if boxed { t.ink } else { t.acc })
        .child(glyph_el(t, "arrow", 16.0, if boxed { t.on_ink } else { t.on_acc }))
        .key("go").role("button").label("Unlock").hit()
        .on_press(|s: &mut Lock| {
            if s.engine.lock().unwrap().submit() {
                redraw();
            }
        });
    let field = row().grow(1.0).center_y().child(if n == 0 {
        label(t, 14.0, 400.0, if busy { "Checking…" } else { "Password" }, t.ink3)
    } else {
        dots(t, n)
    });
    pill = pill.child(field).child(go)
        .offset(if s.kick { 18.0 } else { 0.0 }, 0.0)
        .offset_motion(Spring::new(0.55, 0.72));

    let avatar = row().size(64.0, 64.0).center().radius(32.0).fill(t.raised).child(text(initial).font(TextStyle::new(32.0, 400.0).family(t.fonts.display)).color(t.ink));
    let who = label(t, 16.0, 600.0, name, t.ink);
    let align_left = t.look == Look::Phosphor;
    let hint = if tone == Tone::Warn {
        row().gap(6.0).center_y().child(glyph_el(t, "caps", 15.0, hint_color)).child(label(t, 13.0, 400.0, hint, hint_color))
    } else {
        row().center_y().child(label(t, 13.0, 400.0, hint, hint_color))
    };
    let mut f = col().gap(16.0).items_center().child(avatar).child(who).child(pill).child(hint);
    if align_left {
        f = f.items_start();
    }
    f
}

/// Whether Caps Lock is on. Rune does not report lock keys, so this reads the keyboard's LED.
fn caps_lock() -> bool {
    let Ok(dir) = std::fs::read_dir("/sys/class/leds") else { return false };
    dir.flatten().any(|e| {
        e.file_name().to_string_lossy().ends_with("::capslock")
            && std::fs::read_to_string(e.path().join("brightness")).is_ok_and(|b| b.trim() != "0")
    })
}

/// A floating capsule the extras sit in, drawn from the look's own surface tokens.
fn capsule(t: &Tokens) -> El {
    let boxed = matches!(t.look, Look::Folio | Look::Phosphor);
    let c = row().center_y().radius(t.radii.pill).fill(t.raised);
    if boxed { c.border(1.0, t.ink) } else { c.border(1.0, t.rule) }
}

/// Top right: the keyboard layout, the network and the battery.
fn status(engine: &Engine, t: &Tokens) -> El {
    let st = engine.status();
    let mut pill = capsule(t).pin(None, Some(28.0), Some(48.0), None).h(34.0).px(12.0).gap(12.0);
    if let Some(layout) = st.layout {
        pill = pill.child(
            row().h(18.0).px(5.0).center().radius(t.radii.chip).border(1.0, t.ink2)
                .child(fig(t, 10.5, 700.0, layout, t.ink2)),
        );
    }
    let net = match st.network {
        Network::Wireless => "wifi",
        Network::Wired => "ethernet",
        Network::Offline => "wifioff",
    };
    pill = pill.child(glyph_el(t, net, 16.0, t.ink2));
    if let Some((percent, charging)) = st.battery {
        pill = pill.child(
            row().gap(5.0).center_y()
                .child(glyph_el(t, "battery", 16.0, if charging { t.acc } else { t.ink2 }))
                .child(fig(t, 12.5, 500.0, format!("{percent}%"), t.ink2)),
        );
    }
    pill
}

/// Bottom left: what is playing, with previous, play or pause, and next.
fn media(engine: &Engine, t: &Tokens, m: yozora_lockd::glance::Media) -> El {
    let mut art = row().size(48.0, 48.0).center().radius(t.radii.chip).fill(t.fill);
    match &m.art {
        Some(path) => art = art.pixels_fit(images().file(path, 48.0, 2.0).pixels(), SurfaceFit::Fill),
        None => art = art.child(glyph_el(t, "music", 20.0, t.ink2)),
    }
    let _ = engine;
    let button = |key: &'static str, name: &str, glyph: &str, cmd: MediaCmd| {
        cell(32.0, 32.0, glyph_el(t, glyph, 16.0, t.ink)).radius(t.radii.ctl)
            .key(key).role("button").label(name.to_owned()).hit().hover(t.fill)
            .on_press(move |s: &mut Lock| s.engine.lock().unwrap().media(cmd))
    };
    let words = col().grow(1.0).min_w(0.0).gap(2.0)
        .child(label(t, 13.0, 600.0, m.title, t.ink).truncate())
        .child(label(t, 12.0, 400.0, m.artist, t.ink2).truncate());
    let card = row().pin(Some(48.0), None, None, Some(44.0)).w(340.0).padding(10.0).gap(12.0).center_y().radius(t.radii.card).fill(t.raised)
        .child(art).child(words)
        .child(button("prev", "Previous", "prev", MediaCmd::Previous))
        .child(button("play", if m.playing { "Pause" } else { "Play" }, if m.playing { "pause" } else { "play" }, MediaCmd::PlayPause))
        .child(button("next", "Next", "next", MediaCmd::Next));
    if matches!(t.look, Look::Folio | Look::Phosphor) { card.border(1.0, t.ink) } else { card.border(1.0, t.rule) }
}

/// Bottom centre: how many notifications arrived while locked. Their contents stay hidden.
fn notes(t: &Tokens, n: usize) -> El {
    let what = if n == 1 { "1 notification".to_owned() } else { format!("{n} notifications") };
    col().pin(Some(0.0), None, Some(0.0), Some(52.0)).items_center().child(
        capsule(t).h(36.0).pl(12.0).pr(16.0).gap(9.0)
            .child(glyph_el(t, "bell", 16.0, t.ink))
            .child(label(t, 12.5, 600.0, what, t.ink))
            .child(label(t, 12.5, 400.0, "\u{b7} contents hidden until you unlock", t.ink2)),
    )
}

/// Bottom right: suspend, restart and shut down. The last two ask twice.
fn power(engine: &Engine, t: &Tokens) -> El {
    let entries = [("suspend", "moon", "Suspend", Power::Suspend), ("restart", "restart", "Restart", Power::Reboot), ("shutdown", "power", "Shut down", Power::ShutDown)];
    let mut r = row().pin(None, None, Some(48.0), Some(44.0)).gap(10.0);
    for (key, glyph, name, action) in entries {
        let confirming = engine.confirm.is_confirming(action);
        let (fill, fg) = if confirming { (t.alert, t.on_ink) } else { (t.raised, t.ink) };
        let disc = cell(48.0, 48.0, glyph_el(t, glyph, 18.0, fg)).radius(t.radii.pill).fill(fill);
        let disc = if matches!(t.look, Look::Folio | Look::Phosphor) { disc.border(1.0, t.ink) } else { disc.border(1.0, t.rule) };
        r = r.child(
            col().w(88.0).gap(7.0).items_center()
                .child(disc)
                .child(label(t, 11.5, 400.0, if confirming { "Again to confirm" } else { name }, if confirming { t.alert } else { t.ink2 }))
                .key(key).role("button").label(name).hit()
                .on_press(move |s: &mut Lock| {
                    let _: Press = s.engine.lock().unwrap().power(action);
                    redraw();
                }),
        );
    }
    r
}

fn view(s: &Lock, _: &mut Cx<Lock>) -> El {
    let t = tokens(s.look);
    let (time, date) = s.engine.lock().unwrap().clock();
    let (w, h) = s.size;
    // The clock takes what the form leaves: the form is a fixed height at the foot of the screen,
    // so the clock scales to the room between, and never runs into it.
    let room = h - FORM_H - FORM_BOTTOM - 40.0 - 30.0;
    let k = (room / (el_top(s.look) + 1.35 * clock_size(s.look))).clamp(0.4, 1.25);
    let size = clock_size(s.look) * k;
    let top = el_top(s.look) * k;
    let mut root = el().w_full().h_full().child(backdrop(s, &t));
    if !s.leaving {
        let clock = col().pin(Some(0.0), Some(top), Some(0.0), None).items_center().gap(6.0)
            .child(label(&t, 18.0, 500.0, date, t.ink2))
            .child(text(time).font(TextStyle::new(size, 400.0).family(t.fonts.display).features("'tnum' 1")).color(t.ink));
        let left = if s.look == Look::Phosphor { Some(w * 436.0 / 1440.0) } else { Some(0.0) };
        let right = if s.look == Look::Phosphor { None } else { Some(0.0) };
        root = root.child(clock.transition(Transition::opacity()));
        if MAIN.load(Ordering::Relaxed) == s.index {
            let form = col().pin(left, None, right, Some(FORM_BOTTOM)).items_center().justify_end().child(form(s, &t));
            root = root.child(form.transition(Transition::opacity()));
            let engine = s.engine.lock().unwrap();
            let show = &engine.config.show;
            let (show_media, show_notes, show_power) = (show.media, show.notifications, show.power);
            let glance = engine.glance();
            let mut extras = vec![status(&engine, &t)];
            if show_media && let Some(m) = glance.media {
                extras.push(media(&engine, &t, m));
            }
            if show_notes && let Some(n) = glance.notifications.filter(|n| *n > 0) {
                extras.push(notes(&t, n));
            }
            if show_power {
                extras.push(power(&engine, &t));
            }
            drop(engine);
            for e in extras {
                root = root.child(e.transition(Transition::opacity()));
            }
        }
    }
    root
}

fn el_top(look: Look) -> f32 {
    match look {
        Look::Folio => 120.0,
        Look::Phosphor => 130.0,
        _ => 150.0,
    }
}

fn main() {
    let options = match cli::parse(std::env::args().skip(1)) {
        Ok(cli::Command::Run(o)) => o,
        Ok(cli::Command::Help) => return println!("{}", cli::HELP),
        Ok(cli::Command::Version) => return println!("yozora-lock {}", env!("CARGO_PKG_VERSION")),
        Ok(cli::Command::Status) => return println!("{}", cli::status_json()),
        Err(e) => {
            eprintln!("yozora-lock: {e}");
            std::process::exit(2);
        }
    };
    let socket = match cli::bind(&options) {
        Ok(s) => s,
        Err(cli::BindError::AlreadyRunning) => return println!("{}", cli::already_running_message(&options)),
        Err(cli::BindError::Failed(e)) => {
            eprintln!("yozora-lock: {e:#}");
            std::process::exit(1);
        }
    };
    if !options.preview {
        yozora_lockd::abort_on_panic();
    }
    let config = yozora_lockd::load_settings(&options.config_dir()).lock;
    let engine = match Engine::start(config, &options, redraw) {
        Ok(e) => Arc::new(Mutex::new(e)),
        Err(e) => {
            eprintln!("yozora-lock: {e:#}");
            std::process::exit(1);
        }
    };
    cli::serve(socket);
    let look = yozora_shell::config::resolve(&yozora_shell::config::load()).0;
    let preview = options.preview;

    rune::run_windows_with(RunOptions { accessory: true }, move |displays| {
        let mut app = App::new(AppSpec { fonts: FONTS, ..AppSpec::new("Yozora lock", (10.0, 10.0), || (), host) }).windowless();
        let only = std::env::var("YZ_OUT").ok().filter(|_| preview);
        let focused = yozora_lockd::system::monitors().into_iter().find(|m| m.focused).map(|m| m.name);
        let shown: Vec<_> = displays.iter().filter(|d| only.is_none() || d.name == only).collect();
        if let Some(i) = shown.iter().position(|d| d.name.is_some() && d.name == focused) {
            MAIN.store(i, Ordering::Relaxed);
        }
        for (index, d) in shown.into_iter().enumerate() {
            let name = d.name.clone().unwrap_or_default();
            let size = (d.size.0 as f32 / d.scale as f32, d.size.1 as f32 / d.scale as f32);
            let wallpaper = Arc::new(Mutex::new(None));
            {
                let (engine, slot, out) = (engine.clone(), wallpaper.clone(), name.clone());
                let config = engine.lock().unwrap().config.clone();
                std::thread::spawn(move || {
                    *slot.lock().unwrap() = yozora_lockd::background_for(&config, Some(&out));
                    redraw();
                });
            }
            let handle = app.overlay(
                OverlaySpec::new("yozora-lock", size, view)
                    .options(OverlayOptions {
                        placement: Some(OverlayPlacement::FullScreen),
                        wayland_layer: if preview { WaylandLayer::Overlay } else { WaylandLayer::SessionLock },
                        input: OverlayInput::All,
                        keys: KeyPolicy::OnDemand,
                        display: d.id.clone().map_or(DisplayTarget::Primary, DisplayTarget::Id),
                        ..Default::default()
                    })
                    .on_key(on_key),
                Lock { engine: engine.clone(), look, output: name, index, size, wallpaper, leaving: false, kick: false, preview },
            );
            handle.show();
            handle.request_keys();
            handles().lock().unwrap().push(handle);
        }
        engine.lock().unwrap().mark_locked();
        let tick = engine.clone();
        std::thread::Builder::new().name("lock-tick".into()).spawn(move || {
            let mut seen = 0;
            loop {
            std::thread::sleep(yozora_lockd::TICK);
            let (update, shakes) = {
                let mut e = tick.lock().unwrap();
                let update = e.poll(Instant::now());
                (update, e.auth.shakes)
            };
            if shakes != seen {
                for h in handles().lock().unwrap().iter() {
                    h.update(|s, _| s.kick = true);
                }
                std::thread::spawn(|| {
                    std::thread::sleep(std::time::Duration::from_millis(90));
                    for h in handles().lock().unwrap().iter() {
                        h.update(|s, _| s.kick = false);
                    }
                });
            }
            seen = shakes;
            if update.changed {
                redraw();
            }
            if update.unlocked {
                for h in handles().lock().unwrap().iter() {
                    h.update(|s, _| s.leaving = true);
                }
                std::thread::sleep(yozora_lockd::LEAVE);
                tick.lock().unwrap().mark_unlocked();
                if !preview {
                    rune::session_unlock();
                }
                std::process::exit(0);
            }
            }
        }).ok();
        vec![WindowSpec::from_app("host", app)]
    });
}
