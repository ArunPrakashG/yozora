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
use crate::bar_config::Conf;
use crate::stage::{Pop, Stage};

pub struct Shared {
    pub services: Mutex<Services>,
    config: Mutex<ShellConfig>,
    stages: Mutex<Vec<(String, OverlayHandle<Stage>)>>,
    bars: Mutex<Vec<OverlayHandle<Bar>>>,
    anchors: Mutex<HashMap<String, f32>>,
    engine: OnceLock<yozora_search::Engine>,
    /// What the stages last drew as open, for the control sockets to report.
    /// Notices volume, microphone, output and layout changes and puts them on the focused monitor.
    pub osd: crate::osd::Hub,
    /// `bar.toml` as last loaded.
    bar_conf: Mutex<Arc<Conf>>,
    /// Each bar's output, width and what was last asked of its surface.
    slots: Mutex<Vec<BarSlot>>,
    /// Which popover each output's stage has open, so its bar can mark the item that opened it.
    opened: Mutex<HashMap<String, Option<OpenState>>>,
}

/// What an output's stage shows: the popover, the bar button it grows from and, for a
/// `module/<name>`, which module.
#[derive(Clone, Debug, PartialEq)]
pub struct OpenState {
    pub pop: Pop,
    pub anchor: f32,
    pub module: String,
}

/// One bar surface and the size, zone and visibility it was last given.
pub struct BarSlot {
    pub output: String,
    pub width: f32,
    pub handle: OverlayHandle<Bar>,
    pub last: Option<(f32, i32, bool)>,
}

impl Shared {
    pub fn new(config: ShellConfig, fakes: Fakes) -> Arc<Shared> {
        let mut services = Services::new(fakes);
        services.ensure(
            [Kind::Audio, Kind::Bluetooth, Kind::Hyprland, Kind::Media, Kind::Network, Kind::Notifications, Kind::Power, Kind::Tray],
            &Default::default(),
        );
        Arc::new(Shared { services: Mutex::new(services), config: Mutex::new(config), stages: Mutex::new(Vec::new()), bars: Mutex::new(Vec::new()), anchors: Mutex::new(HashMap::new()), engine: OnceLock::new(), osd: Default::default(), bar_conf: Mutex::new(Arc::new(Conf::default())), slots: Mutex::new(Vec::new()), opened: Mutex::new(HashMap::new()) })
    }

    /// The look and effects in force now. Surface tension turns to Daybreak by day.
    pub fn look(&self) -> (Look, Fx) {
        crate::config::resolve(&self.config.lock().unwrap())
    }

    pub fn config(&self) -> ShellConfig {
        self.config.lock().unwrap().clone()
    }

    pub fn set_config(&self, config: ShellConfig) {
        // The file watcher reports our own writes back; there is nothing new in them.
        if *self.config.lock().unwrap() == config {
            return;
        }
        let was = self.look().0;
        *self.config.lock().unwrap() = config;
        // A new look can mean a bar of another height.
        crate::bar_config::sync_surfaces(self);
        if self.look().0 != was {
            self.redraw_instant();
        } else {
            self.redraw();
        }
    }

    pub fn attach_stage(&self, output: &str, h: OverlayHandle<Stage>) {
        self.stages.lock().unwrap().push((output.to_owned(), h));
    }

    /// The stages, by output, for surfaces that pick one.
    pub fn stage_handles(&self) -> Vec<(String, OverlayHandle<Stage>)> {
        self.stages.lock().unwrap().clone()
    }

    pub fn bar_handles(&self) -> Vec<OverlayHandle<Bar>> {
        self.bars.lock().unwrap().clone()
    }

    pub fn attach_bar(&self, h: OverlayHandle<Bar>) {
        self.bars.lock().unwrap().push(h);
    }

    /// A bar surface on `output`, which `bar_config::sync_surfaces` then sizes and shows.
    pub fn attach_bar_on(&self, output: &str, width: f32, h: OverlayHandle<Bar>) {
        self.slots.lock().unwrap().push(BarSlot { output: output.to_owned(), width, handle: h.clone(), last: None });
        self.attach_bar(h);
    }

    pub fn each_bar(&self, mut f: impl FnMut(&mut BarSlot)) {
        self.slots.lock().unwrap().iter_mut().for_each(&mut f);
    }

    pub fn bar_conf(&self) -> Arc<Conf> {
        self.bar_conf.lock().unwrap().clone()
    }

    pub fn set_bar_conf(&self, c: Arc<Conf>) {
        *self.bar_conf.lock().unwrap() = c;
    }

    /// The look's tokens, with the panel row following the bar's real height.
    pub fn tokens(&self, look: Look) -> yozora_style::Tokens {
        self.bar_conf().tokens(look)
    }

    /// Runs `f` on the stage of `output` without waking it for keys, for things like tooltips
    /// that must not take focus.
    pub fn with_stage(&self, output: &str, f: impl FnOnce(&mut Stage) + Send + 'static) {
        if let Some((_, s)) = self.stages.lock().unwrap().iter().find(|(n, _)| n == output) {
            s.update(move |st, _| f(st));
        }
    }

    /// Asks every surface to draw again.
    pub fn redraw(&self) {
        self.osd.observe(self);
        for b in self.bars.lock().unwrap().iter() {
            b.update(|_, _| {});
        }
        for (_, s) in self.stages.lock().unwrap().iter() {
            s.update(|st, _| st.sync());
        }
    }

    /// `redraw` for a new look: every surface lands it at once. Gliding the bars' and panels'
    /// layout between two looks keeps all of them drawing, one thread for all of them, for as
    /// long as the glide lasts.
    pub fn redraw_instant(&self) {
        self.osd.observe(self);
        for b in self.bars.lock().unwrap().iter() {
            b.update(|_, tx| tx.instant());
        }
        for (_, s) in self.stages.lock().unwrap().iter() {
            s.update(|st, tx| {
                tx.instant();
                st.sync();
            });
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

    /// Records what `output`'s stage draws as open, and redraws that output's bar when it changed
    /// so the item that opened it stays marked.
    pub fn note_open_on(&self, output: &str, pop: Option<Pop>, anchor: f32, module: &str) {
        let now = pop.map(|pop| OpenState { pop, anchor, module: if pop == Pop::Module { module.to_owned() } else { String::new() } });
        let closed = now.is_none();
        {
            let mut map = self.opened.lock().unwrap();
            if map.get(output) == Some(&now) {
                return;
            }
            map.insert(output.to_owned(), now);
        }
        // The stage holds the keyboard while a panel is open (`request_keys`); give it back to
        // the window it came from once nothing is.
        if closed && let Some((_, s)) = self.stages.lock().unwrap().iter().find(|(n, _)| n == output) {
            s.release_keys();
        }
        let slots = self.slots.lock().unwrap();
        if let Some(s) = slots.iter().find(|s| s.output == output) {
            s.handle.update(|_, _| {});
        }
    }

    pub fn open_on(&self, output: &str) -> Option<OpenState> {
        self.opened.lock().unwrap().get(output).cloned().flatten()
    }

    /// Opens or closes the popover of `module/<name>` on `output`.
    pub fn open_module(&self, output: &str, name: &str, anchor_x: f32) {
        for (n, s) in self.stages.lock().unwrap().iter() {
            if n == output {
                let name = name.to_owned();
                s.update(move |st, _| st.open_module(&name, anchor_x));
                s.request_keys();
            } else {
                s.update(|st, _| st.open = None);
            }
        }
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
