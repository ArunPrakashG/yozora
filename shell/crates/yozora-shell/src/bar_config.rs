//! `bar.toml` as the shell uses it: which modules go where, how the bar sits on the screen, and
//! what to start for it. Applied at start and again whenever the file changes.

use std::collections::BTreeSet;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use yozora_config::bar::{self, BarConfig, BarStyle};
use yozora_config::paths;
use yozora_services::{Kind, hyprland};
use yozora_style::{Look, Tokens, tokens};

use crate::bar_modules::{Layout, unknown};
use crate::shared::Shared;

/// A loaded `bar.toml`, plus what the file said outright. A style or height left out means the
/// look's own: Surface tension floats, Folio and Phosphor are flush, and each has its own height.
#[derive(Clone, Debug, Default)]
pub struct Conf {
    pub bar: BarConfig,
    pub style_set: bool,
    pub height_set: bool,
}

pub fn load() -> Conf {
    let dir = paths::config_dir();
    let bar = match bar::load(&dir) {
        Ok(l) => {
            for w in &l.warnings {
                tracing::warn!("{w}");
            }
            l.value.bar
        }
        Err(e) => {
            tracing::warn!("{e:#}");
            BarConfig::default()
        }
    };
    let section = yozora_config::load::read_table(&dir.join(bar::FILE)).ok().and_then(|(t, _)| t.get("bar").and_then(|b| b.as_table().cloned()));
    let said = |key: &str| section.as_ref().is_some_and(|s| s.contains_key(key));
    let conf = Conf { bar, style_set: said("style"), height_set: said("height") };
    for name in unknown(&conf.bar.modules) {
        tracing::warn!("bar.toml: no module called {name:?}");
    }
    conf
}

/// Where the bar sits and how big it is, for one look.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Geo {
    pub flush: bool,
    /// Gap between the screen edge and the bar on each side.
    pub side: f32,
    /// Gap above the bar.
    pub top: f32,
    /// The bar itself.
    pub h: f32,
    pub radius: f32,
    /// How tall the items in it are: the bar, but never taller than the approved 28.
    pub item_h: f32,
}

impl Geo {
    /// What the surface needs: the gap above the bar plus the bar.
    pub fn surface_h(&self) -> f32 {
        self.top + self.h
    }
}

impl Conf {
    pub fn geo(&self, t: &Tokens) -> Geo {
        let native_flush = t.bar_top == 0.0;
        let flush = if self.style_set { self.bar.bar.style == BarStyle::Flush } else { native_flush };
        let h = if self.height_set { (self.bar.bar.height as f32).clamp(24.0, 64.0) } else { t.bar_h };
        let top = if flush { 0.0 } else if t.bar_top > 0.0 { t.bar_top } else { 8.0 };
        Geo { flush, side: if flush { 0.0 } else { crate::bar::SIDE }, top, h, radius: if flush { 0.0 } else { t.radii.pill }, item_h: h.min(28.0) }
    }

    /// The tokens with the panel row moved to follow a bar that isn't the look's own height, and
    /// translucent surfaces for the glass looks while blur is on.
    pub fn tokens(&self, look: Look) -> Tokens {
        let mut t = tokens(look);
        // Glass looks let the compositor's blur show through; the rest stay solid.
        if self.bar.bar.blur && matches!(look, Look::Tension | Look::Daybreak | Look::Prism) {
            t.surf.a = GLASS_ALPHA;
        }
        let g = self.geo(&t);
        t.panel_top += g.surface_h() - (t.bar_top + t.bar_h);
        t
    }

    pub fn exclusive_zone(&self, g: &Geo) -> i32 {
        if self.bar.bar.reserve_space { g.surface_h().ceil() as i32 } else { 0 }
    }
}

/// How much of the compositor's blur shows through a glass surface.
const GLASS_ALPHA: f32 = 0.72;

/// Everything `bar.toml` means, put to work: services for the modules placed, blur rules, and the
/// size, place and visibility of each bar.
pub fn apply(sh: &Arc<Shared>) {
    let conf = Arc::new(load());
    sh.set_bar_conf(conf.clone());

    // Every zone any monitor might use, so a bar that appears later finds its services running.
    let mut layouts = vec![Layout::parse(&conf.bar.modules)];
    layouts.extend(conf.bar.monitor.keys().map(|name| Layout::parse(&conf.bar.zones_for(name))));
    let kinds: BTreeSet<Kind> = layouts.iter().flat_map(|l| l.services()).chain([Kind::Hyprland]).collect();
    let dynamic: BTreeSet<String> = layouts.iter().flat_map(|l| l.dynamic_names()).collect();
    {
        let mut svc = sh.services.lock().unwrap();
        svc.ensure(kinds, &conf.bar.custom);
        svc.ensure_modules(&dynamic);
    }
    blur(conf.bar.bar.blur);
    sync_surfaces(sh);
    sh.redraw();
}

/// Tells Hyprland to blur our surfaces. Hyprland matches the namespace regex against the whole
/// name, and only pixels more opaque than `ignore_alpha` are blurred, so the dim scrim is not.
/// Rules added at runtime last until Hyprland reloads its config, so they go in on every start
/// and whenever the setting turns on.
fn blur(on: bool) {
    static LAST: Mutex<Option<bool>> = Mutex::new(None);
    let mut last = LAST.lock().unwrap();
    if *last == Some(on) {
        return;
    }
    *last = Some(on);
    if !on {
        return;
    }
    let _ = std::thread::Builder::new().name("blur-rules".into()).spawn(|| {
        let lua = r#"eval hl.layer_rule({ match = { namespace = "^(yozora-.*)$" }, blur = true, blur_popups = true, ignore_alpha = 0.5, no_anim = true })"#;
        match hyprland::request(lua) {
            Ok(r) if r.trim() == "ok" => tracing::debug!("applied blur layer rules"),
            Ok(_) => {
                // Hyprland from before Lua config.
                for rule in ["blur,^(yozora-.*)$", "blurpopups,^(yozora-.*)$", "ignorealpha 0.5,^(yozora-.*)$", "noanim,^(yozora-.*)$"] {
                    let _ = hyprland::request(&format!("keyword layerrule {rule}"));
                }
            }
            Err(e) => tracing::debug!("could not apply layer rules: {e:#}"),
        }
    });
}

/// Sizes, reserves and shows or hides each bar to match the config and the look in force.
pub fn sync_surfaces(sh: &Shared) {
    let conf = sh.bar_conf();
    let (look, _) = sh.look();
    let geo = conf.geo(&tokens(look));
    sh.each_bar(|slot| {
        let want = (geo.surface_h(), conf.exclusive_zone(&geo), conf.bar.shows_on(&slot.output));
        if slot.last == Some(want) {
            return;
        }
        let first = slot.last.is_none();
        let before = slot.last;
        slot.last = Some(want);
        if first || before.map(|b| (b.0, b.1)) != Some((want.0, want.1)) {
            slot.handle.resize((slot.width, want.0));
            slot.handle.set_exclusive_zone(want.1);
        }
        if first || before.map(|b| b.2) != Some(want.2) {
            if want.2 { slot.handle.show() } else { slot.handle.hide() }
        }
    });
}

/// Applies edits to `bar.toml` as they happen.
pub fn watch(sh: Arc<Shared>) {
    let _ = std::thread::Builder::new().name("bar-config".into()).spawn(move || {
        let Ok(mut watcher) = yozora_config::watch::DirWatcher::new(&paths::config_dir()) else { return };
        loop {
            match futures_lite::future::block_on(watcher.changed()) {
                Ok(files) if files.iter().any(|f| f == bar::FILE) => {
                    // Editors write in bursts.
                    std::thread::sleep(Duration::from_millis(120));
                    apply(&sh);
                }
                Ok(_) => {}
                Err(_) => return,
            }
        }
    });
}
