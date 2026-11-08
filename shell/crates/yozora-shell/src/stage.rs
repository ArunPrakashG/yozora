//! The full-screen transparent surface popovers, toasts and the volume display live on. It is
//! click-through except where something is drawn, so it costs nothing while the screen is idle.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::Datelike;
use rune::{Cx, El, col, el};
use yozora_style::tokens;

use crate::kit::enter;
use crate::shared::Shared;
use crate::{bar, bluetooth, calendar, control, media, notifications, settings, sound, tray, wifi};

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
}

pub struct Stage {
    pub sh: Arc<Shared>,
    pub open: Option<Pop>,
    /// The bar icon the open panel grows from, in screen points.
    pub anchor_x: f32,
    pub screen_w: f32,
    pub output: String,
    pub month: (i32, u32),
    pub settings_page: usize,
    /// Notifications on screen as toasts, and when each goes.
    pub toasts: Vec<(u32, Instant)>,
    known: HashSet<u32>,
    primed: bool,
    /// The volume display: level, muted, and when it goes.
    pub osd: Option<(f32, bool, Instant)>,
    last_volume: Option<(f32, bool)>,
}

impl Stage {
    pub fn new(sh: Arc<Shared>, output: &str, screen_w: f32) -> Stage {
        let today = chrono::Local::now().date_naive();
        Stage { sh, open: None, anchor_x: 0.0, screen_w, output: output.to_owned(), month: (today.year(), today.month()), settings_page: 0, toasts: Vec::new(), known: HashSet::new(), primed: false, osd: None, last_volume: None }
    }

    pub fn toggle(&mut self, pop: Pop, anchor_x: f32) {
        if self.open == Some(pop) {
            self.open = None;
        } else {
            self.open = Some(pop);
            self.anchor_x = anchor_x;
            let today = chrono::Local::now().date_naive();
            self.month = (today.year(), today.month());
        }
    }

    /// Brings toasts and the volume display up to date with the services. Runs on every redraw.
    pub fn sync(&mut self) {
        let now = Instant::now();
        self.toasts.retain(|(_, until)| *until > now);
        if self.osd.is_some_and(|(_, _, until)| until <= now) {
            self.osd = None;
        }
        let svc = self.sh.services.lock().unwrap();
        if let Some(n) = &svc.notifications {
            let g = n.read();
            for item in g.state.items.iter().filter(|i| i.unread) {
                if self.known.insert(item.id) && self.primed && !g.state.dnd && item.snoozed_until == 0 {
                    // Critical ones stay until dealt with; the rest go after a few seconds.
                    let life = if item.urgency >= 2 { Duration::from_secs(60) } else { Duration::from_secs(6) };
                    self.toasts.push((item.id, now + life));
                    self.sh.redraw_after(life + Duration::from_millis(50));
                }
            }
            self.primed = self.primed || g.ready;
            let live: HashSet<u32> = g.state.items.iter().map(|i| i.id).collect();
            self.toasts.retain(|(id, _)| live.contains(id));
        }
        if let Some(a) = &svc.audio {
            let g = a.read();
            let sink = g.state.sinks.iter().find(|d| d.name == g.state.default_sink);
            if let Some(d) = sink {
                let now_v = (d.volume, d.muted);
                if self.last_volume.is_some_and(|v| v != now_v) && self.open.is_none() {
                    self.osd = Some((d.volume, d.muted, now + Duration::from_millis(1600)));
                    self.sh.redraw_after(Duration::from_millis(1650));
                }
                self.last_volume = Some(now_v);
            }
        }
    }

    pub fn step_month(&mut self, by: i32) {
        let (y, m) = self.month;
        let n = y * 12 + (m as i32 - 1) + by;
        self.month = (n.div_euclid(12), (n.rem_euclid(12) + 1) as u32);
    }
}

pub fn on_key(s: &mut Stage, e: &rune::KeyEvent, _: &mut rune::Tx) -> bool {
    if s.open.is_some() && e.key == rune::Key::Escape {
        s.open = None;
        return true;
    }
    false
}

pub fn view(s: &Stage, _: &mut Cx<Stage>) -> El {
    let (look, fx) = s.sh.look();
    let t = tokens(look);
    let mut root = el().w_full().h_full();
    if s.open == Some(Pop::Settings) {
        // Settings is a modal in the middle of the screen, not a popover on the bar.
        root = root
            .child(el().key("scrim").pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).hit().fill(rune::Color { r: 0.0, g: 0.0, b: 0.0, a: 0.35 }).on_press(|s: &mut Stage| s.open = None))
            .child(
                col().pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).center().child(
                    settings::view(s, &t).key("settings").hit().transition(enter(&t, fx, settings::W / 2.0, settings::W)),
                ),
            );
    } else if let Some(pop) = s.open {
        let (w, body) = match pop {
            Pop::Settings => unreachable!(),
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
                body.key("panel").hit().pin(Some(left), Some(t.panel_top), None, None)
                    .transition(enter(&t, fx, s.anchor_x - left, w)),
            );
    }
    if s.open.is_none() {
        let toasts: Vec<_> = {
            let svc = s.sh.services.lock().unwrap();
            let items = svc.notifications.as_ref().map(|n| n.read().state.items.clone()).unwrap_or_default();
            s.toasts.iter().filter_map(|(id, _)| items.iter().find(|i| i.id == *id).cloned()).collect()
        };
        let mut stack = col().key("toasts").pin(None, Some(t.panel_top), Some(bar::SIDE), None).gap(10.0);
        for n in &toasts {
            stack = stack.child(notifications::toast(&t, n).transition(enter(&t, fx, notifications::W / 2.0, notifications::W)));
        }
        root = root.child(stack);
        if let Some((level, muted, _)) = s.osd {
            let x = s.sh.anchor(&s.output, "vol").unwrap_or(s.screen_w - 120.0);
            let left = (x - 120.0).clamp(bar::SIDE, s.screen_w - 240.0 - bar::SIDE);
            root = root.child(notifications::osd(&t, level, muted).key("osd").pin(Some(left), Some(t.panel_top), None, None).transition(enter(&t, fx, 120.0, 240.0)));
        }
    }
    root
}
