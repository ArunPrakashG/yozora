//! The lock screen. One `ext-session-lock` surface per display; the password goes to PAM through
//! `yozora-lockd`, and the session unlocks only when PAM says yes.
//!   yozora-lock            lock the session now
//!   yozora-lock --preview  draw the screen on an overlay, Escape closes it
//!   yozora-lock status     whether the session is locked

use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;

use rune::tok::TextStyle;
use rune::{
    App, AppSpec, Color, Cx, DisplayTarget, El, ImageCache, Key, KeyEvent, KeyPolicy, OverlayHandle, OverlayInput, OverlayOptions, OverlayPlacement, OverlaySpec,
    RunOptions, SurfaceFit, Transition, Tx, WaylandLayer, WindowSpec, col, el, row, text,
};
use yozora_lockd::{Engine, Phase, Tone, cli};
use yozora_shell::kit::{glyph_el, label};
use yozora_style::{FONTS, Look, Tokens, tokens};

struct Lock {
    engine: Arc<Mutex<Engine>>,
    look: Look,
    output: String,
    size: (f32, f32),
    wallpaper: Arc<Mutex<Option<PathBuf>>>,
    leaving: bool,
    preview: bool,
}

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
    row().gap(8.0).center_y().children((0..shown).map(|_| el().size(7.0, 7.0).radius(radius).fill(t.ink)))
}

fn form(s: &Lock, t: &Tokens) -> El {
    let engine = s.engine.lock().unwrap();
    let now = Instant::now();
    let (tone, hint) = match engine.auth.waiting_for(now) {
        Some(d) => (Tone::Bad, format!("Too many tries · {}", yozora_lockd::countdown(d))),
        None => engine.auth.hint(false),
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
        .child(glyph_el(t, "arrow", 16.0, if boxed { t.on_ink } else { t.on_acc }));
    let field = row().grow(1.0).center_y().child(if n == 0 {
        label(t, 14.0, 400.0, if busy { "Checking…" } else { "Password" }, t.ink3)
    } else {
        dots(t, n)
    });
    pill = pill.child(field).child(go);

    let avatar = row().size(64.0, 64.0).center().radius(32.0).fill(t.raised).child(text(initial).font(TextStyle::new(32.0, 400.0).family(t.fonts.display)).color(t.ink));
    let who = label(t, 16.0, 600.0, name, t.ink);
    let align_left = t.look == Look::Phosphor;
    let mut f = col().gap(16.0).items_center().child(avatar).child(who).child(pill).child(label(t, 13.0, 400.0, hint, hint_color));
    if align_left {
        f = f.items_start();
    }
    f
}

fn view(s: &Lock, _: &mut Cx<Lock>) -> El {
    let t = tokens(s.look);
    let (time, date) = s.engine.lock().unwrap().clock();
    let (w, h) = s.size;
    let k = (h / 900.0).clamp(0.6, 1.3);
    let size = clock_size(s.look) * k;
    let top = el_top(s.look) * k;
    let mut root = el().w_full().h_full().child(backdrop(s, &t));
    if !s.leaving {
        let clock = col().pin(Some(0.0), Some(top), Some(0.0), None).items_center().gap(6.0)
            .child(label(&t, 18.0, 500.0, date, t.ink2))
            .child(text(time).font(TextStyle::new(size, 400.0).family(t.fonts.display).features("'tnum' 1")).color(t.ink));
        let left = if s.look == Look::Phosphor { Some(w * 436.0 / 1440.0) } else { Some(0.0) };
        let right = if s.look == Look::Phosphor { None } else { Some(0.0) };
        let form = col().pin(left, None, right, Some(120.0 * k)).items_center().justify_end().child(form(s, &t));
        root = root.child(clock.transition(Transition::opacity())).child(form.transition(Transition::opacity()));
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
        for d in displays.iter().filter(|d| only.is_none() || d.name == only) {
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
                Lock { engine: engine.clone(), look, output: name, size, wallpaper, leaving: false, preview },
            );
            handle.show();
            handle.request_keys();
            handles().lock().unwrap().push(handle);
        }
        engine.lock().unwrap().mark_locked();
        let tick = engine.clone();
        std::thread::Builder::new().name("lock-tick".into()).spawn(move || loop {
            std::thread::sleep(yozora_lockd::TICK);
            let update = tick.lock().unwrap().poll(Instant::now());
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
        }).ok();
        vec![WindowSpec::from_app("host", app)]
    });
}
