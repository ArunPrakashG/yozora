//! Bar modules: what each one draws in the bar and in its popover.
//!
//! A module's bar item is drawn by the `Bar` view, and its popover content by the `Popover`
//! view. Both read service entities directly; nothing here owns state beyond the popover's
//! small UI state in [`PopUi`].

pub mod calendar;
pub mod control_center;
pub mod custom;
pub mod devices;
pub mod launcher;
pub mod media;
pub mod network;
pub mod notifications;
pub mod sound;
pub mod tray;
pub mod workspaces;

use std::collections::BTreeSet;

use gpui::Pixels;
use yozora_ui::controls::SliderState;

use crate::services::Kind;

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
}

impl Module {
    pub const NAMES: &[&str] = &[
        "launcher",
        "workspaces",
        "window",
        "media",
        "tray",
        "network",
        "sound",
        "devices",
        "battery",
        "notifications",
        "clock",
        "control-center",
        "custom/<name>",
    ];

    pub fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "launcher" | "menu" => Self::Launcher,
            "workspaces" => Self::Workspaces,
            "window" => Self::Window,
            "media" => Self::Media,
            "tray" => Self::Tray,
            "network" => Self::Network,
            "sound" | "audio" => Self::Sound,
            "devices" | "bluetooth" => Self::Devices,
            "battery" => Self::Battery,
            "notifications" => Self::Notifications,
            "clock" => Self::Clock,
            "control-center" => Self::ControlCenter,
            other => Self::Custom(other.strip_prefix("custom/")?.to_string()),
        })
    }

    pub fn name(&self) -> String {
        match self {
            Self::Launcher => "launcher".into(),
            Self::Workspaces => "workspaces".into(),
            Self::Window => "window".into(),
            Self::Media => "media".into(),
            Self::Tray => "tray".into(),
            Self::Network => "network".into(),
            Self::Sound => "sound".into(),
            Self::Devices => "devices".into(),
            Self::Battery => "battery".into(),
            Self::Notifications => "notifications".into(),
            Self::Clock => "clock".into(),
            Self::ControlCenter => "control-center".into(),
            Self::Custom(n) => format!("custom/{n}"),
        }
    }

    pub fn services(&self) -> &'static [Kind] {
        match self {
            Self::Launcher | Self::Clock => &[],
            Self::Workspaces | Self::Window => &[Kind::Hyprland],
            Self::Media => &[Kind::Media],
            Self::Tray => &[Kind::Tray],
            Self::Network => &[Kind::Network],
            Self::Sound => &[Kind::Audio],
            Self::Devices => &[Kind::Bluetooth, Kind::Power],
            Self::Battery => &[Kind::Power],
            Self::Notifications => &[Kind::Notifications],
            Self::ControlCenter => {
                &[Kind::Network, Kind::Bluetooth, Kind::Notifications, Kind::Power, Kind::Audio, Kind::Media]
            }
            Self::Custom(_) => &[Kind::Custom],
        }
    }

    /// The popover this module opens when clicked, if any.
    pub fn popover(&self) -> Option<Pop> {
        Some(match self {
            Self::Launcher => Pop::Menu,
            Self::Media => Pop::Media,
            Self::Network => Pop::Network,
            Self::Sound => Pop::Sound,
            Self::Devices => Pop::Devices,
            Self::Battery => Pop::Battery,
            Self::Notifications => Pop::Notifications,
            Self::Clock => Pop::Calendar,
            Self::ControlCenter => Pop::ControlCenter,
            Self::Workspaces | Self::Window | Self::Tray | Self::Custom(_) => return None,
        })
    }
}

/// Parsed module zones for one bar. Unknown names are dropped with a warning at load time.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Layout {
    pub left: Vec<Module>,
    pub center: Vec<Module>,
    pub right: Vec<Module>,
}

impl Layout {
    pub fn parse(zones: &yozora_config::bar::Zones) -> Self {
        let parse = |names: &[String]| names.iter().filter_map(|n| Module::parse(n)).collect();
        Self { left: parse(&zones.left), center: parse(&zones.center), right: parse(&zones.right) }
    }

    pub fn all(&self) -> impl Iterator<Item = &Module> {
        self.left.iter().chain(&self.center).chain(&self.right)
    }

    pub fn services(&self) -> BTreeSet<Kind> {
        self.all().flat_map(|m| m.services().iter().copied()).collect()
    }

    /// Which zone a module is in: true when it's on the left, so its popover opens rightward.
    pub fn is_left(&self, module: &Module) -> bool {
        self.left.contains(module)
    }
}

/// Names in `zones` that aren't modules, for config warnings.
pub fn unknown(zones: &yozora_config::bar::Zones) -> Vec<String> {
    zones.left.iter().chain(&zones.center).chain(&zones.right).filter(|n| Module::parse(n).is_none()).cloned().collect()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Pop {
    Menu,
    Media,
    Network,
    Sound,
    Devices,
    Battery,
    Notifications,
    Calendar,
    ControlCenter,
    /// A tray item's menu, by its key.
    Tray(String),
}

impl Pop {
    pub fn width(&self) -> Pixels {
        gpui::px(match self {
            Pop::Menu => 252.,
            Pop::Media | Pop::Sound => 320.,
            Pop::Network | Pop::Devices => 304.,
            Pop::Battery => 296.,
            Pop::Notifications => 360.,
            Pop::Calendar => 308.,
            Pop::ControlCenter => 344.,
            Pop::Tray(_) => 260.,
        })
    }

    pub fn parse(name: &str) -> Option<Self> {
        Module::parse(name).and_then(|m| m.popover())
    }

    /// The module that owns this popover, for highlighting its bar item.
    pub fn module(&self) -> Module {
        match self {
            Pop::Menu => Module::Launcher,
            Pop::Media => Module::Media,
            Pop::Network => Module::Network,
            Pop::Sound => Module::Sound,
            Pop::Devices => Module::Devices,
            Pop::Battery => Module::Battery,
            Pop::Notifications => Module::Notifications,
            Pop::Calendar => Module::Clock,
            Pop::ControlCenter => Module::ControlCenter,
            Pop::Tray(_) => Module::Tray,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionAction {
    Lock,
    Logout,
    Reboot,
    Poweroff,
}

impl SessionAction {
    pub fn verb(self) -> &'static str {
        match self {
            Self::Lock => "Lock",
            Self::Logout => "Log out",
            Self::Reboot => "Restart",
            Self::Poweroff => "Shut down",
        }
    }

    pub fn command(self, session: &yozora_config::bar::SessionConfig) -> String {
        match self {
            Self::Lock => session.lock.clone(),
            Self::Logout => session.logout.clone(),
            Self::Reboot => session.reboot.clone(),
            Self::Poweroff => session.poweroff.clone(),
        }
    }
}

/// Per-popover UI state that doesn't belong in any service.
#[derive(Default)]
pub struct PopUi {
    /// Months from the current one, for the calendar.
    pub month_offset: i32,
    /// A session action waiting for confirmation.
    pub confirm: Option<SessionAction>,
    /// The control center's footer power button shows a choice of actions.
    pub power_choices: bool,
    pub hovered: Option<usize>,
    pub volume: SliderState,
    pub mic: SliderState,
    /// Submenu path into a tray menu.
    pub submenu: Vec<i32>,
}

/// "3m", "2h", "4d": how long ago, from Unix seconds.
pub fn ago(seen: u64) -> String {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let s = now.saturating_sub(seen);
    match s {
        0..60 => "now".into(),
        60..3600 => format!("{}m", s / 60),
        3600..86400 => format!("{}h", s / 3600),
        _ => format!("{}d", s / 86400),
    }
}

/// Shortens `s` to `max` characters with an ellipsis.
pub fn ellipsize(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_names() {
        assert_eq!(Module::parse("custom/updates"), Some(Module::Custom("updates".into())));
        assert_eq!(Module::parse("control-center").and_then(|m| m.popover()), Some(Pop::ControlCenter));
        assert_eq!(Module::parse("nope"), None);
        assert_eq!(ellipsize("abcdef", 4), "abc…");
    }
}
