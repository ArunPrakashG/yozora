//! The modules a bar can hold, by the names `bar.toml` gives them, and which popover and
//! services each one brings. Mirrors the old bar's module list name for name.

use std::collections::BTreeSet;

use yozora_config::bar::Zones;
use yozora_services::Kind;

use crate::stage::Pop;

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Module {
    Launcher,
    Workspaces,
    Window,
    Media,
    Tray,
    Network,
    Sound,
    Devices,
    Battery,
    Notifications,
    Clock,
    ControlCenter,
    Custom(String),
    /// A `module/<name>` module run by the module runtime.
    Dynamic(String),
}

impl Module {
    pub fn parse(name: &str) -> Option<Module> {
        Some(match name {
            "launcher" | "menu" => Module::Launcher,
            "workspaces" => Module::Workspaces,
            "window" => Module::Window,
            "media" => Module::Media,
            "tray" => Module::Tray,
            "network" => Module::Network,
            "sound" | "audio" => Module::Sound,
            "devices" | "bluetooth" => Module::Devices,
            "battery" => Module::Battery,
            "notifications" => Module::Notifications,
            "clock" => Module::Clock,
            "control-center" => Module::ControlCenter,
            other => match other.strip_prefix("module/") {
                Some(n) if !n.is_empty() => Module::Dynamic(n.to_owned()),
                Some(_) => return None,
                None => Module::Custom(other.strip_prefix("custom/")?.to_owned()),
            },
        })
    }

    /// The services it reads. The control center reads most of them.
    pub fn services(&self) -> &'static [Kind] {
        match self {
            Module::Launcher | Module::Clock | Module::Dynamic(_) => &[],
            Module::Workspaces | Module::Window => &[Kind::Hyprland],
            Module::Media => &[Kind::Media],
            Module::Tray => &[Kind::Tray],
            Module::Network => &[Kind::Network],
            Module::Sound => &[Kind::Audio],
            Module::Devices => &[Kind::Bluetooth, Kind::Power],
            Module::Battery => &[Kind::Power],
            Module::Notifications => &[Kind::Notifications],
            Module::ControlCenter => &[Kind::Network, Kind::Bluetooth, Kind::Notifications, Kind::Power, Kind::Audio, Kind::Media],
            Module::Custom(_) => &[Kind::Custom],
        }
    }

    /// A status item sits in the shared cluster with its neighbours, the way the mockup groups
    /// the icons on the right.
    pub fn is_status(&self) -> bool {
        matches!(self, Module::Network | Module::Sound | Module::Devices | Module::Battery | Module::Notifications | Module::ControlCenter)
    }

    /// The popover it opens and the key its bar button records an anchor under. `None` for
    /// modules that open nothing, and for the tray, whose items each open their own menu.
    pub fn popover(&self) -> Option<(Pop, String)> {
        Some(match self {
            Module::Launcher => (Pop::Menu, "menu".into()),
            Module::Media => (Pop::Media, "media".into()),
            Module::Network => (Pop::Wifi, "wifi".into()),
            Module::Sound => (Pop::Sound, "vol".into()),
            Module::Devices => (Pop::Bluetooth, "bt".into()),
            Module::Battery => (Pop::Battery, "battery".into()),
            Module::Notifications => (Pop::Notifications, "bell".into()),
            Module::Clock => (Pop::Calendar, "clock".into()),
            Module::ControlCenter => (Pop::Control, "cc".into()),
            Module::Dynamic(n) => (Pop::Module, format!("mod:{n}")),
            Module::Workspaces | Module::Window | Module::Tray | Module::Custom(_) => return None,
        })
    }
}

/// The popover that `yozora bar toggle <name>` opens, and the anchor it grows from. The tray has
/// no single button, so it takes the first item's.
pub fn toggle_target(name: &str) -> Option<(Pop, String, Option<String>)> {
    let module = Module::parse(name)?;
    match &module {
        Module::Tray => Some((Pop::Tray, "tray0".into(), None)),
        Module::Dynamic(n) => Some((Pop::Module, format!("mod:{n}"), Some(n.clone()))),
        m => m.popover().map(|(p, k)| (p, k, None)),
    }
}

/// Module zones for one bar. Unknown names are dropped.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Layout {
    pub left: Vec<Module>,
    pub center: Vec<Module>,
    pub right: Vec<Module>,
}

impl Layout {
    pub fn parse(zones: &Zones) -> Layout {
        let parse = |names: &[String]| names.iter().filter_map(|n| Module::parse(n)).collect();
        Layout { left: parse(&zones.left), center: parse(&zones.center), right: parse(&zones.right) }
    }

    pub fn all(&self) -> impl Iterator<Item = &Module> {
        self.left.iter().chain(&self.center).chain(&self.right)
    }

    pub fn dynamic_names(&self) -> BTreeSet<String> {
        self.all().filter_map(|m| if let Module::Dynamic(n) = m { Some(n.clone()) } else { None }).collect()
    }

    pub fn services(&self) -> BTreeSet<Kind> {
        self.all().flat_map(|m| m.services().iter().copied()).collect()
    }
}

/// Names in `zones` that aren't modules, for config warnings.
pub fn unknown(zones: &Zones) -> Vec<String> {
    zones.left.iter().chain(&zones.center).chain(&zones.right).filter(|n| Module::parse(n).is_none()).cloned().collect()
}
