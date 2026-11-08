//! What every surface of the shell shares: the services, the look, and the handles to push
//! changes at the surfaces.

use std::sync::{Arc, Mutex};

use rune::OverlayHandle;
use yozora_services::{Fakes, Kind, Services};
use yozora_style::{Fx, Look};

use crate::bar::Bar;
use crate::stage::{Pop, Stage};

pub struct Shared {
    pub services: Mutex<Services>,
    look: Mutex<(Look, Fx)>,
    stages: Mutex<Vec<(String, OverlayHandle<Stage>)>>,
    bars: Mutex<Vec<OverlayHandle<Bar>>>,
}

impl Shared {
    pub fn new(look: Look, fx: Fx, fakes: Fakes) -> Arc<Shared> {
        let mut services = Services::new(fakes);
        services.ensure(
            [Kind::Audio, Kind::Bluetooth, Kind::Hyprland, Kind::Media, Kind::Network, Kind::Notifications, Kind::Power, Kind::Tray],
            &Default::default(),
        );
        Arc::new(Shared { services: Mutex::new(services), look: Mutex::new((look, fx)), stages: Mutex::new(Vec::new()), bars: Mutex::new(Vec::new()) })
    }

    pub fn look(&self) -> (Look, Fx) {
        *self.look.lock().unwrap()
    }

    pub fn set_look(&self, look: Look, fx: Fx) {
        *self.look.lock().unwrap() = (look, fx);
        self.redraw();
    }

    pub fn attach_stage(&self, output: &str, h: OverlayHandle<Stage>) {
        self.stages.lock().unwrap().push((output.to_owned(), h));
    }

    pub fn attach_bar(&self, h: OverlayHandle<Bar>) {
        self.bars.lock().unwrap().push(h);
    }

    /// Asks every surface to draw again.
    pub fn redraw(&self) {
        for b in self.bars.lock().unwrap().iter() {
            b.update(|_, _| {});
        }
        for (_, s) in self.stages.lock().unwrap().iter() {
            s.update(|_, _| {});
        }
    }

    pub fn open(&self, output: &str, pop: Pop, anchor_x: f32) {
        for (name, s) in self.stages.lock().unwrap().iter() {
            if name == output {
                s.update(move |st, tx| {
                    st.toggle(pop, anchor_x);
                    let _ = tx;
                });
                s.request_keys();
            } else {
                s.update(|st, _| st.open = None);
            }
        }
    }

    pub fn close(&self) {
        for (_, s) in self.stages.lock().unwrap().iter() {
            s.update(|st, _| st.open = None);
        }
    }
}
