//! The full-screen transparent surface popovers, toasts and the volume display live on. It is
//! click-through except where something is drawn, so it costs nothing while the screen is idle.

use std::sync::Arc;

use chrono::Datelike;
use rune::{Cx, El, el};
use yozora_style::tokens;

use crate::kit::enter;
use crate::shared::Shared;
use crate::{bar, bluetooth, calendar, control, media, sound, tray, wifi};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pop {
    Calendar,
    Media,
    Wifi,
    Bluetooth,
    Sound,
    Control,
    Tray,
}

pub struct Stage {
    pub sh: Arc<Shared>,
    pub open: Option<Pop>,
    /// The bar icon the open panel grows from, in screen points.
    pub anchor_x: f32,
    pub screen_w: f32,
    pub month: (i32, u32),
}

impl Stage {
    pub fn new(sh: Arc<Shared>, screen_w: f32) -> Stage {
        let today = chrono::Local::now().date_naive();
        Stage { sh, open: None, anchor_x: 0.0, screen_w, month: (today.year(), today.month()) }
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
    if let Some(pop) = s.open {
        let (w, body) = match pop {
            Pop::Calendar => (calendar::W, calendar::view(s, &t)),
            Pop::Media => (media::W, media::view(s, &t)),
            Pop::Wifi => (wifi::W, wifi::view(s, &t)),
            Pop::Bluetooth => (bluetooth::W, bluetooth::view(s, &t)),
            Pop::Sound => (sound::W, sound::view(s, &t)),
            Pop::Control => (control::W, control::view(s, &t)),
            Pop::Tray => (tray::W, tray::view(s, &t)),
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
    root
}
