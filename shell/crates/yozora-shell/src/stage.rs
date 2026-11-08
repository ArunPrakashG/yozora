//! The full-screen transparent surface popovers, toasts and the volume display live on. It is
//! click-through except where something is drawn, so it costs nothing while the screen is idle.

use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::Datelike;
use rune::{Cx, El, col, el};

use crate::kit::{self, enter};
use crate::shared::Shared;
use crate::{bar, battery, bluetooth, calendar, control, launcher, media, menu, module_pop, notifications, settings, sound, tooltip, tray, wifi};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pop {
    Calendar,
    Media,
    Wifi,
    Bluetooth,
    Sound,
    Control,
    Tray,
    Notifications,
    Settings,
    Launcher,
    /// The Yozora menu the bar's mark opens.
    Menu,
    Battery,
    /// A `module/<name>` popover; `Stage.bar.module` says which.
    Module,
}

pub struct Stage {
    pub sh: Arc<Shared>,
    /// When the stage came up, for the warm-up frame.
    born: Instant,
    pub open: Option<Pop>,
    /// The bar icon the open panel grows from, in screen points.
    pub anchor_x: f32,
    pub screen_w: f32,
    pub output: String,
    pub month: (i32, u32),
    pub settings_page: usize,
    /// What the Settings modal keeps between frames.
    pub settings: crate::settings_state::SettingsUi,
    pub launcher: crate::launcher::Launcher,
    /// What the notification center and its toasts keep between frames.
    pub notes: notifications::Ui,
    /// The on-screen display on this monitor, while it shows.
    pub osd: Option<crate::osd::Slot>,
    /// When a redraw is already set to retire the display.
    osd_wake: Option<Instant>,
    /// What the bar keeps here: its tooltip, the menu's question, a module's pending action.
    pub bar: menu::BarUi,
}

impl Stage {
    pub fn new(sh: Arc<Shared>, output: &str, screen_w: f32) -> Stage {
        let today = chrono::Local::now().date_naive();
        // One frame of the busiest panel, drawn nearly invisible, so the first real opening doesn't
        // pay for the pipelines, glyphs and icons it needs.
        sh.redraw_after(Duration::from_millis(1600));
        launcher::warm(&sh);
        Stage { sh, born: Instant::now(), open: None, anchor_x: 0.0, screen_w, output: output.to_owned(), month: (today.year(), today.month()), settings_page: 0, settings: Default::default(), launcher: Default::default(), notes: Default::default(), osd: None, osd_wake: None, bar: Default::default() }
    }

    pub fn toggle(&mut self, pop: Pop, anchor_x: f32) {
        self.bar.tip = None;
        self.bar.confirm = None;
        self.bar.pending = None;
        self.bar.slide = None;
        if self.open == Some(pop) {
            self.open = None;
        } else {
            self.open = Some(pop);
            if pop == Pop::Launcher {
                launcher::open(self);
            }
            self.anchor_x = anchor_x;
            let today = chrono::Local::now().date_naive();
            self.month = (today.year(), today.month());
        }
    }

    /// Opens the popover of `module/<name>`, or closes it when it is already the one showing.
    pub fn open_module(&mut self, name: &str, anchor_x: f32) {
        if self.open == Some(Pop::Module) && self.bar.module == name {
            self.toggle(Pop::Module, anchor_x);
        } else {
            self.open = None;
            self.bar.module = name.to_owned();
            self.toggle(Pop::Module, anchor_x);
        }
    }

    /// Brings toasts and the volume display up to date with the services. Runs on every redraw.
    pub fn sync(&mut self) {
        if self.open == Some(Pop::Launcher) {
            launcher::sync(self);
        }
        let now = Instant::now();
        notifications::sync(self);
        if self.osd.as_ref().is_some_and(|o| o.until <= now) {
            self.osd = None;
        }
        self.arm_osd();
    }

    /// Puts the on-screen display up on this monitor, or keeps it up with a new value.
    pub fn show_osd(&mut self, slot: crate::osd::Slot) {
        self.osd = Some(slot);
        self.arm_osd();
    }

    /// Takes the display down, for when focus has moved to another monitor. True if one was up.
    pub fn hide_osd(&mut self) -> bool {
        self.osd.take().is_some()
    }

    /// Makes sure a redraw comes when the display is due to go, since nothing else would.
    fn arm_osd(&mut self) {
        let Some(until) = self.osd.as_ref().map(|o| o.until) else { return };
        if self.osd_wake.is_none_or(|w| w <= Instant::now()) {
            self.osd_wake = Some(until);
            self.sh.redraw_after(until.saturating_duration_since(Instant::now()) + Duration::from_millis(30));
        }
    }

    pub fn step_month(&mut self, by: i32) {
        let (y, m) = self.month;
        let n = y * 12 + (m as i32 - 1) + by;
        self.month = (n.div_euclid(12), (n.rem_euclid(12) + 1) as u32);
    }
}

pub fn on_key(s: &mut Stage, e: &rune::KeyEvent, tx: &mut rune::Tx) -> bool {
    if notifications::on_key(s, e) {
        return true;
    }
    if s.open == Some(Pop::Launcher) {
        return launcher::on_key(s, e, tx);
    }
    if s.open.is_some() && e.key == rune::Key::Escape {
        s.open = None;
        return true;
    }
    false
}

/// The other looks' typefaces at the sizes the shell sets them, drawn invisibly while the stage
/// warms up, so the first change of look finds their glyphs ready instead of loading them in the
/// frame that shows them.
fn warm_fonts(current: yozora_style::Look) -> El {
    const TEXT: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZ abcdefghijklmnopqrstuvwxyz 0123456789 .,:;!?-+/%()";
    let mut out = col().pin(Some(0.0), Some(0.0), None, None).opacity(0.004);
    for look in yozora_style::Look::ALL {
        if look == current {
            continue;
        }
        let t = yozora_style::tokens(look);
        for (size, weight) in [(11.0, 600.0), (13.0, 400.0), (13.0, 600.0), (15.0, 600.0), (20.0, 400.0)] {
            out = out.child(kit::label(&t, size, weight, TEXT, t.ink)).child(kit::fig(&t, size, weight, TEXT, t.ink)).child(kit::display(&t, size + 6.0, TEXT, t.ink));
        }
    }
    out
}

pub fn view(s: &Stage, _: &mut Cx<Stage>) -> El {
    let (look, fx) = s.sh.look();
    let t = s.sh.tokens(look);
    s.sh.note_open_on(&s.output, s.open, s.anchor_x, &s.bar.module);
    s.sh.osd.note_popover(&s.output, s.open);
    let mut root = el().w_full().h_full();
    // Toasts sit under any panel that opens over them.
    if let Some(stack) = notifications::toasts(s, &t, fx) {
        root = root.child(stack);
    }
    if s.open == Some(Pop::Settings) {
        // Settings is a modal in the middle of the screen, not a popover on the bar.
        root = root
            .child(el().key("scrim").pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).hit().fill(rune::Color { r: 0.0, g: 0.0, b: 0.0, a: 0.35 }).on_press(|s: &mut Stage| {
                s.settings.commit();
                s.open = None;
            }))
            .child(
                col().pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).center().child(
                    settings::view(s, &t).key("settings").hit().on_press(|_: &mut Stage| {}).transition(enter(&t, fx, settings::W / 2.0, settings::W)),
                ),
            );
    } else if s.open == Some(Pop::Launcher) {
        root = root
            .child(el().key("scrim").pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).hit().fill(rune::Color { r: 0.0, g: 0.0, b: 0.0, a: 0.25 }).on_press(launcher::dismiss))
            .child(
                col().pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).items_center().pt(150.0).child(
                    launcher::view(s, &t).key("launcher").hit().on_press(|_: &mut Stage| {}).transition(enter(&t, fx, launcher::W / 2.0, launcher::W)),
                ),
            );
    } else if let Some(pop) = s.open {
        let (w, body) = match pop {
            Pop::Settings | Pop::Launcher => unreachable!(),
            Pop::Menu => (menu::W, menu::view(s, &t)),
            Pop::Battery => (battery::W, battery::view(s, &t)),
            Pop::Module => (module_pop::W, module_pop::view(s, &t)),
            Pop::Calendar => (calendar::W, calendar::view(s, &t)),
            Pop::Media => (media::W, media::view(s, &t)),
            Pop::Wifi => (wifi::W, wifi::view(s, &t)),
            Pop::Bluetooth => (bluetooth::W, bluetooth::view(s, &t)),
            Pop::Sound => (sound::W, sound::view(s, &t)),
            Pop::Control => (control::W, control::view(s, &t)),
            Pop::Tray => (tray::W, tray::view(s, &t)),
            Pop::Notifications => (notifications::W, notifications::view(s, &t)),
        };
        let left = (s.anchor_x - w / 2.0).clamp(bar::SIDE, s.screen_w - w - bar::SIDE);
        // A hit-catching scrim closes the panel on a click anywhere else.
        root = root
            .child(el().key("scrim").pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).hit().on_press(|s: &mut Stage| s.open = None))
            .child(
                kit::stagger(&t, fx, body).key("panel").hit().on_press(|_: &mut Stage| {}).pin(Some(left), Some(t.panel_top), None, None)
                    .transition(enter(&t, fx, s.anchor_x - left, w)),
            );
    }
    if s.born.elapsed() < Duration::from_millis(1500) {
        root = root.child(control::view(s, &t).pin(Some(0.0), Some(t.panel_top), None, None).opacity(0.004));
        if s.open != Some(Pop::Launcher) {
            root = root.child(launcher::warm_view(s, &t).pin(Some(0.0), Some(t.panel_top), None, None).opacity(0.004));
        }
        root = root.child(warm_fonts(look));
    }
    if s.open.is_none() {
        let geo = s.sh.bar_conf().geo(&t);
        if let Some(tip) = tooltip::view(s, &t, fx, geo.surface_h() + 6.0) {
            root = root.child(tip);
        }
    }
    if let Some(slot) = &s.osd {
        // Above everything else, as the separate display was: it never takes a click.
        let x = s.sh.anchor(&s.output, "vol").unwrap_or(s.screen_w - 120.0);
        root = root.child(crate::osd::view(&t, fx, slot, x, s.screen_w));
    }
    root
}
