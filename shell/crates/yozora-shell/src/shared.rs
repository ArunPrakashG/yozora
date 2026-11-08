//! What every surface of the shell shares: the services, the look, and the handles to push
//! changes at the surfaces.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use rune::OverlayHandle;
use yozora_services::{Fakes, Kind, Services};
use yozora_config::shell::ShellConfig;
use yozora_style::{Fx, Look};

use crate::bar::Bar;
use crate::stage::{Pop, Stage};

pub struct Shared {
    pub services: Mutex<Services>,
    config: Mutex<ShellConfig>,
    stages: Mutex<Vec<(String, OverlayHandle<Stage>)>>,
    bars: Mutex<Vec<OverlayHandle<Bar>>>,
    anchors: Mutex<HashMap<String, f32>>,
    engine: OnceLock<yozora_search::Engine>,
    /// What the stages last drew as open, for the control sockets to report.
    shown: Mutex<Option<Pop>>,
}

impl Shared {
    pub fn new(config: ShellConfig, fakes: Fakes) -> Arc<Shared> {
        let mut services = Services::new(fakes);
        services.ensure(
            [Kind::Audio, Kind::Bluetooth, Kind::Hyprland, Kind::Media, Kind::Network, Kind::Notifications, Kind::Power, Kind::Tray],
            &Default::default(),
        );
        Arc::new(Shared { services: Mutex::new(services), config: Mutex::new(config), stages: Mutex::new(Vec::new()), bars: Mutex::new(Vec::new()), anchors: Mutex::new(HashMap::new()), engine: OnceLock::new(), shown: Mutex::new(None) })
    }

    /// The look and effects in force now. Surface tension turns to Daybreak by day.
    pub fn look(&self) -> (Look, Fx) {
        let c = self.config.lock().unwrap();
        let fx = Fx::from_slug(&c.fx).unwrap_or(Fx::Full);
        let look = Look::from_slug(&c.look).unwrap_or_default();
        let day = match c.time.as_str() {
            "day" => true,
            "follow" => {
                use chrono::Timelike;
                let now = chrono::Local::now();
                c.is_day(now.hour() * 60 + now.minute())
            }
            _ => false,
        };
        (if look == Look::Tension && day { Look::Daybreak } else { look }, fx)
    }

    pub fn config(&self) -> ShellConfig {
        self.config.lock().unwrap().clone()
    }

    pub fn set_config(&self, config: ShellConfig) {
        *self.config.lock().unwrap() = config;
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
            s.update(|st, _| st.sync());
        }
    }

    /// Redraws once `after` has passed, for things that end on a timer.
    pub fn redraw_after(self: &Arc<Self>, after: Duration) {
        let sh = self.clone();
        std::thread::spawn(move || {
            std::thread::sleep(after);
            sh.redraw();
        });
    }

    pub fn set_anchor(&self, output: &str, key: &str, x: f32) {
        self.anchors.lock().unwrap().insert(format!("{output}/{key}"), x);
    }

    pub fn anchor(&self, output: &str, key: &str) -> Option<f32> {
        self.anchors.lock().unwrap().get(&format!("{output}/{key}")).copied()
    }

    pub fn open(&self, output: &str, pop: Pop, anchor_x: f32) {
        if pop == Pop::Notifications && let Some(n) = &self.services.lock().unwrap().notifications {
            n.send(yozora_services::notifications::Cmd::MarkRead);
        }
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

    /// The launcher's search engine, started on first use.
    pub fn engine(self: &Arc<Self>) -> &yozora_search::Engine {
        self.engine.get_or_init(|| {
            let weak = Arc::downgrade(self);
            let engine = yozora_search::Engine::new(yozora_search::load_settings(&yozora_config::paths::config_dir()), move || {
                if let Some(sh) = weak.upgrade() {
                    sh.redraw();
                }
            });
            engine.start();
            engine
        })
    }

    pub fn note_open(&self, pop: Option<Pop>) {
        *self.shown.lock().unwrap() = pop;
    }

    pub fn shown(&self) -> Option<Pop> {
        *self.shown.lock().unwrap()
    }

    pub fn outputs(&self) -> Vec<String> {
        self.stages.lock().unwrap().iter().map(|(n, _)| n.clone()).collect()
    }

    /// Runs `f` on the stage of `output`, or the first one when there is no such output, and
    /// gives it the keys.
    pub fn on_stage(&self, output: Option<&str>, f: impl FnOnce(&mut Stage) + Send + 'static) -> bool {
        let stages = self.stages.lock().unwrap();
        let Some((_, s)) = stages.iter().find(|(n, _)| Some(n.as_str()) == output).or_else(|| stages.first()) else { return false };
        s.update(move |st, _| f(st));
        s.request_keys();
        true
    }

    pub fn close(&self) {
        for (_, s) in self.stages.lock().unwrap().iter() {
            s.update(|st, _| st.open = None);
        }
    }
}
