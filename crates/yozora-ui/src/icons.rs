//! Line icons from the design, on a 24-unit grid with a 1.8 stroke.
//!
//! GPUI draws an SVG as a mask tinted by the element's text color, so every icon is drawn in
//! black and dimmed parts use opacity.

use std::borrow::Cow;

use gpui::{
    App, AssetSource, Bounds, Element, ElementId, GlobalElementId, InspectorElementId, IntoElement, LayoutId, Pixels,
    SharedString, StyleRefinement, Styled, Svg, Window, px, svg,
};

/// Icons, by file name under `icons/`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Icon {
    Logo,
    Wifi,
    Wifi1,
    Wifi2,
    WifiOff,
    Ethernet,
    NetworkOff,
    Vpn,
    VolumeHigh,
    VolumeLow,
    VolumeMute,
    Mic,
    MicOff,
    Speaker,
    Headphones,
    Display,
    Bluetooth,
    BluetoothOff,
    Battery,
    BatteryCharging,
    Bell,
    Moon,
    Appearance,
    Check,
    ChevronLeft,
    ChevronRight,
    Dot,
    Play,
    Pause,
    Previous,
    Next,
    Close,
    Settings,
    Lock,
    Power,
    Keyboard,
    Mouse,
    Gamepad,
    Phone,
    Terminal,
    Globe,
    Package,
    Sync,
    Cloud,
    ControlCenter,
    Music,
    Sun,
    Bolt,
    Leaf,
    Gauge,
    Calendar,
    App,
    Warning,
    Bar,
    Motion,
    Modules,
    Info,
    Search,
    File,
    Grip,
    Plus,
    Reset,
    External,
    Code,
    ArrowRight,
    CapsLock,
    Spinner,
    Clock,
    Alert,
    Window,
}

const ALL: &[Icon] = &[
    Icon::Logo,
    Icon::Wifi,
    Icon::Wifi1,
    Icon::Wifi2,
    Icon::WifiOff,
    Icon::Ethernet,
    Icon::NetworkOff,
    Icon::Vpn,
    Icon::VolumeHigh,
    Icon::VolumeLow,
    Icon::VolumeMute,
    Icon::Mic,
    Icon::MicOff,
    Icon::Speaker,
    Icon::Headphones,
    Icon::Display,
    Icon::Bluetooth,
    Icon::BluetoothOff,
    Icon::Battery,
    Icon::BatteryCharging,
    Icon::Bell,
    Icon::Moon,
    Icon::Appearance,
    Icon::Check,
    Icon::ChevronLeft,
    Icon::ChevronRight,
    Icon::Dot,
    Icon::Play,
    Icon::Pause,
    Icon::Previous,
    Icon::Next,
    Icon::Close,
    Icon::Settings,
    Icon::Lock,
    Icon::Power,
    Icon::Keyboard,
    Icon::Mouse,
    Icon::Gamepad,
    Icon::Phone,
    Icon::Terminal,
    Icon::Globe,
    Icon::Package,
    Icon::Sync,
    Icon::Cloud,
    Icon::ControlCenter,
    Icon::Music,
    Icon::Sun,
    Icon::Bolt,
    Icon::Leaf,
    Icon::Gauge,
    Icon::Calendar,
    Icon::App,
    Icon::Warning,
    Icon::Bar,
    Icon::Motion,
    Icon::Modules,
    Icon::Info,
    Icon::Search,
    Icon::File,
    Icon::Grip,
    Icon::Plus,
    Icon::Reset,
    Icon::External,
    Icon::Code,
    Icon::ArrowRight,
    Icon::CapsLock,
    Icon::Spinner,
    Icon::Clock,
    Icon::Alert,
    Icon::Window,
];

const ARC3: &str = r#"<path d="M2.5 9.2a14 14 0 0 1 19 0"/>"#;
const ARC2: &str = r#"<path d="M5.6 12.6a9.5 9.5 0 0 1 12.8 0"/>"#;
const ARC1: &str = r#"<path d="M8.7 16a5 5 0 0 1 6.6 0"/><circle class="f" cx="12" cy="19.3" r="1.2"/>"#;
const SPEAKER: &str = r#"<path d="M4 9.5h3.2L12 5.5v13l-4.8-4H4z"/>"#;

impl Icon {
    pub fn name(self) -> &'static str {
        match self {
            Icon::Logo => "logo",
            Icon::Wifi => "wifi",
            Icon::Wifi1 => "wifi-1",
            Icon::Wifi2 => "wifi-2",
            Icon::WifiOff => "wifi-off",
            Icon::Ethernet => "ethernet",
            Icon::NetworkOff => "network-off",
            Icon::Vpn => "vpn",
            Icon::VolumeHigh => "volume-high",
            Icon::VolumeLow => "volume-low",
            Icon::VolumeMute => "volume-mute",
            Icon::Mic => "mic",
            Icon::MicOff => "mic-off",
            Icon::Speaker => "speaker",
            Icon::Headphones => "headphones",
            Icon::Display => "display",
            Icon::Bluetooth => "bluetooth",
            Icon::BluetoothOff => "bluetooth-off",
            Icon::Battery => "battery",
            Icon::BatteryCharging => "battery-charging",
            Icon::Bell => "bell",
            Icon::Moon => "moon",
            Icon::Appearance => "appearance",
            Icon::Check => "check",
            Icon::ChevronLeft => "chevron-left",
            Icon::ChevronRight => "chevron-right",
            Icon::Dot => "dot",
            Icon::Play => "play",
            Icon::Pause => "pause",
            Icon::Previous => "previous",
            Icon::Next => "next",
            Icon::Close => "close",
            Icon::Settings => "settings",
            Icon::Lock => "lock",
            Icon::Power => "power",
            Icon::Keyboard => "keyboard",
            Icon::Mouse => "mouse",
            Icon::Gamepad => "gamepad",
            Icon::Phone => "phone",
            Icon::Terminal => "terminal",
            Icon::Globe => "globe",
            Icon::Package => "package",
            Icon::Sync => "sync",
            Icon::Cloud => "cloud",
            Icon::ControlCenter => "control-center",
            Icon::Music => "music",
            Icon::Sun => "sun",
            Icon::Bolt => "bolt",
            Icon::Leaf => "leaf",
            Icon::Gauge => "gauge",
            Icon::Calendar => "calendar",
            Icon::App => "app",
            Icon::Warning => "warning",
            Icon::Bar => "bar",
            Icon::Motion => "motion",
            Icon::Modules => "modules",
            Icon::Info => "info",
            Icon::Search => "search",
            Icon::File => "file",
            Icon::Grip => "grip",
            Icon::Plus => "plus",
            Icon::Reset => "reset",
            Icon::External => "external",
            Icon::Code => "code",
            Icon::ArrowRight => "arrow-right",
            Icon::CapsLock => "caps-lock",
            Icon::Spinner => "spinner",
            Icon::Clock => "clock",
            Icon::Alert => "alert",
            Icon::Window => "window",
        }
    }

    pub fn path(self) -> SharedString {
        format!("icons/{}.svg", self.name()).into()
    }

    /// The SVG body. `class="f"` marks filled shapes.
    fn body(self) -> Cow<'static, str> {
        let s: &'static str = match self {
            Icon::Logo => {
                return Cow::Borrowed(
                    r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><path fill="#000" d="M6.43 2.6A6 6 0 1 0 13.4 9.57A5 5 0 0 1 6.43 2.6Z"/><path fill="#000" d="M13.2 0.9l.55 1.55 1.55.55-1.55.55-.55 1.55-.55-1.55-1.55-.55 1.55-.55z"/></svg>"##,
                );
            }
            Icon::Wifi => return format!("{ARC3}{ARC2}{ARC1}").into(),
            Icon::Wifi2 => {
                return format!(r#"<g opacity=".25">{ARC3}</g>{ARC2}{ARC1}"#).into();
            }
            Icon::Wifi1 => {
                return format!(r#"<g opacity=".25">{ARC3}{ARC2}</g>{ARC1}"#).into();
            }
            Icon::WifiOff => {
                return format!(r#"<g opacity=".28">{ARC3}{ARC2}</g>{ARC1}<path d="M4 3.5l16 17"/>"#).into();
            }
            Icon::Ethernet => {
                r#"<path d="M5.5 5h13a1.5 1.5 0 0 1 1.5 1.5v8a1.5 1.5 0 0 1-1.5 1.5H16v2.5a1 1 0 0 1-1 1H9a1 1 0 0 1-1-1V16H5.5A1.5 1.5 0 0 1 4 14.5v-8A1.5 1.5 0 0 1 5.5 5z"/><path d="M8.5 8.5v2.5M12 8.5v2.5M15.5 8.5v2.5"/>"#
            }
            Icon::NetworkOff => {
                r#"<path d="M4 9.5a12 12 0 0 1 3-2M10.5 6.2a12 12 0 0 1 9.5 3.3M7.3 13a7 7 0 0 1 3-1.6M15.2 11.7a7 7 0 0 1 1.5 1.3"/><circle class="f" cx="12" cy="18.5" r="1.3"/><path d="M4 3.5l16 17"/>"#
            }
            Icon::Vpn => r#"<path d="M12 3.2l7 2.9v5.6c0 4.4-2.9 7.6-7 9.3-4.1-1.7-7-4.9-7-9.3V6.1z"/>"#,
            Icon::VolumeHigh => {
                return format!(
                    r#"{SPEAKER}<path d="M15.6 9.2a4 4 0 0 1 0 5.6"/><path d="M18.4 6.6a7.6 7.6 0 0 1 0 10.8"/>"#
                )
                .into();
            }
            Icon::VolumeLow => {
                return format!(r#"{SPEAKER}<path d="M15.6 9.2a4 4 0 0 1 0 5.6"/>"#).into();
            }
            Icon::VolumeMute => {
                return format!(r#"{SPEAKER}<path d="M16 10l4 4M20 10l-4 4"/>"#).into();
            }
            Icon::Mic => {
                r#"<rect x="9" y="3" width="6" height="11" rx="3"/><path d="M5.5 11.5a6.5 6.5 0 0 0 13 0M12 18v3"/>"#
            }
            Icon::MicOff => {
                r#"<rect x="9" y="3" width="6" height="11" rx="3"/><path d="M5.5 11.5a6.5 6.5 0 0 0 13 0M12 18v3M4 3.5l16 17"/>"#
            }
            Icon::Speaker => {
                r#"<rect x="6" y="3" width="12" height="18" rx="2.8"/><circle cx="12" cy="14.2" r="3.2"/><circle class="f" cx="12" cy="7.4" r="1.1"/>"#
            }
            Icon::Headphones => {
                r#"<path d="M4.5 15.5V12a7.5 7.5 0 0 1 15 0v3.5"/><rect x="3.5" y="14" width="4.5" height="6.5" rx="1.6"/><rect x="16" y="14" width="4.5" height="6.5" rx="1.6"/>"#
            }
            Icon::Display => r#"<rect x="3" y="4.5" width="18" height="12" rx="2.2"/><path d="M9 20h6M12 16.5V20"/>"#,
            Icon::Bluetooth => r#"<path d="M7 7.5l10 9-5 4.5V3l5 4.5-10 9"/>"#,
            Icon::BluetoothOff => r#"<g opacity=".45"><path d="M7 7.5l10 9-5 4.5V3l5 4.5-10 9"/></g>"#,
            Icon::Battery => r#"<rect x="2.5" y="7" width="17" height="10" rx="2.6"/><path d="M21.6 10.5v3"/>"#,
            Icon::BatteryCharging => {
                r#"<rect x="2.5" y="7" width="17" height="10" rx="2.6"/><path d="M21.6 10.5v3"/><path class="f" d="M11.8 8.4l-3.6 4.4h3l-.9 2.8 3.6-4.4h-3z"/>"#
            }
            Icon::Bell => {
                r#"<path d="M6.5 16.5V11a5.5 5.5 0 0 1 11 0v5.5l1.5 2h-14z"/><path d="M10 20.5a2 2 0 0 0 4 0"/>"#
            }
            Icon::Moon => r#"<path d="M19.5 14.6A8 8 0 0 1 9.4 4.5a8 8 0 1 0 10.1 10.1z"/>"#,
            Icon::Appearance => r#"<circle cx="12" cy="12" r="8"/><path class="f" d="M12 4a8 8 0 0 1 0 16z"/>"#,
            Icon::Check => r#"<path d="M5.5 12.5l4.2 4.2L18.5 8"/>"#,
            Icon::ChevronLeft => r#"<path d="M14.5 6.5l-5.5 5.5 5.5 5.5"/>"#,
            Icon::ChevronRight => r#"<path d="M9.5 6.5l5.5 5.5-5.5 5.5"/>"#,
            Icon::Dot => r#"<circle class="f" cx="12" cy="12" r="3"/>"#,
            Icon::Play => {
                r#"<path class="f" d="M8 5.8v12.4a.6.6 0 0 0 .9.5l9.8-6.2a.6.6 0 0 0 0-1L8.9 5.3a.6.6 0 0 0-.9.5z"/>"#
            }
            Icon::Pause => {
                r#"<rect class="f" x="6.5" y="5" width="4" height="14" rx="1.2"/><rect class="f" x="13.5" y="5" width="4" height="14" rx="1.2"/>"#
            }
            Icon::Previous => {
                r#"<path class="f" d="M20.5 6.8v10.4l-7.5-5.2z"/><path class="f" d="M12 6.8v10.4L4.5 12z"/>"#
            }
            Icon::Next => r#"<path class="f" d="M3.5 6.8v10.4l7.5-5.2z"/><path class="f" d="M12 6.8v10.4l7.5-5.2z"/>"#,
            Icon::Close => r#"<path d="M7.5 7.5l9 9M16.5 7.5l-9 9"/>"#,
            Icon::Settings => {
                r#"<path d="M4 7.5h9M18.5 7.5H20M4 16.5h1.5M11 16.5h9"/><circle cx="15.8" cy="7.5" r="2.3"/><circle cx="8.2" cy="16.5" r="2.3"/>"#
            }
            Icon::Lock => {
                r#"<rect x="5" y="10.5" width="14" height="10" rx="2.6"/><path d="M8.5 10.5V7.8a3.5 3.5 0 0 1 7 0v2.7"/>"#
            }
            Icon::Power => r#"<path d="M12 3.5v7.5"/><path d="M6.6 6.8a7.8 7.8 0 1 0 10.8 0"/>"#,
            Icon::Keyboard => {
                r#"<rect x="2.5" y="6.5" width="19" height="11" rx="2.6"/><path d="M6.5 10.2h.01M10 10.2h.01M13.5 10.2h.01M17 10.2h.01M7.5 14h9"/>"#
            }
            Icon::Mouse => r#"<rect x="7" y="3" width="10" height="18" rx="5"/><path d="M12 7v3"/>"#,
            Icon::Gamepad => {
                r#"<path d="M7.5 8h9a4.5 4.5 0 0 1 4.4 5.5l-.7 3a2.3 2.3 0 0 1-3.9 1L14.6 16H9.4l-1.7 1.5a2.3 2.3 0 0 1-3.9-1l-.7-3A4.5 4.5 0 0 1 7.5 8z"/><path d="M8 11v3M6.5 12.5h3M15.5 12h.01M17.5 13.5h.01"/>"#
            }
            Icon::Phone => r#"<rect x="7" y="2.5" width="10" height="19" rx="2.6"/><path d="M11 18.5h2"/>"#,
            Icon::Terminal => {
                r#"<rect x="3" y="4.5" width="18" height="15" rx="2.6"/><path d="M7 9.5l3 2.5-3 2.5M12.5 15h4.5"/>"#
            }
            Icon::Globe => {
                r#"<circle cx="12" cy="12" r="8.5"/><path d="M3.5 12h17"/><path d="M12 3.5c2.4 2.5 3.6 5.4 3.6 8.5s-1.2 6-3.6 8.5c-2.4-2.5-3.6-5.4-3.6-8.5s1.2-6 3.6-8.5z"/>"#
            }
            Icon::Package => r#"<path d="M12 3l8 4.5v9L12 21l-8-4.5v-9z"/><path d="M4 7.5l8 4.5 8-4.5M12 12v9"/>"#,
            Icon::Sync => {
                r#"<path d="M19.5 12a7.5 7.5 0 0 1-13.1 5"/><path d="M4.5 12a7.5 7.5 0 0 1 13.1-5"/><path d="M17.6 3.5V7h-3.5M6.4 20.5V17h3.5"/>"#
            }
            Icon::Cloud => {
                r#"<path d="M7.5 18.5h9.2a3.9 3.9 0 0 0 .5-7.8 5.6 5.6 0 0 0-10.8 1.1A3.4 3.4 0 0 0 7.5 18.5z"/>"#
            }
            Icon::ControlCenter => {
                r#"<rect x="3" y="4.5" width="18" height="6.5" rx="3.25"/><circle class="f" cx="7.6" cy="7.75" r="1.7"/><rect x="3" y="13" width="18" height="6.5" rx="3.25"/><circle class="f" cx="16.4" cy="16.25" r="1.7"/>"#
            }
            Icon::Music => {
                r#"<path d="M9 17.5V5.5l10-2v12"/><circle cx="6.5" cy="17.5" r="2.5"/><circle cx="16.5" cy="15.5" r="2.5"/>"#
            }
            Icon::Sun => {
                r#"<circle cx="12" cy="12" r="4"/><path d="M12 2.5v2M12 19.5v2M2.5 12h2M19.5 12h2M5.3 5.3l1.4 1.4M17.3 17.3l1.4 1.4M5.3 18.7l1.4-1.4M17.3 6.7l1.4-1.4"/>"#
            }
            Icon::Bolt => r#"<path d="M13 3L5.5 13.5h6L10.5 21 18.5 10.5h-6z"/>"#,
            Icon::Leaf => r#"<path d="M5 19c0-8 5-13.5 14-14 .5 9-5 14-13 14z"/><path d="M5 19c3-4 6-6.5 9.5-8"/>"#,
            Icon::Gauge => {
                r#"<path d="M4.2 17a8.5 8.5 0 1 1 15.6 0"/><path d="M12 13.5l4-4.5"/><circle class="f" cx="12" cy="13.5" r="1.5"/>"#
            }
            Icon::Calendar => {
                r#"<rect x="3.5" y="5" width="17" height="15.5" rx="2.6"/><path d="M3.5 10h17M8 3v4M16 3v4"/>"#
            }
            Icon::App => r#"<rect x="4" y="4" width="16" height="16" rx="4"/>"#,
            Icon::Warning => r#"<path d="M12 4l9 15.5H3z"/><path d="M12 10v4M12 17h.01"/>"#,
            Icon::Bar => r#"<rect x="3" y="4" width="18" height="16" rx="3"/><path d="M3 9h18"/>"#,
            Icon::Motion => r#"<path d="M3 16c2.5 0 3-9 6-9s3 11 6 11 3.5-6 6-6"/>"#,
            Icon::Modules => {
                r#"<rect x="4" y="4" width="6.5" height="6.5" rx="1.8"/><rect x="13.5" y="4" width="6.5" height="6.5" rx="1.8"/><rect x="4" y="13.5" width="6.5" height="6.5" rx="1.8"/><rect x="13.5" y="13.5" width="6.5" height="6.5" rx="1.8"/>"#
            }
            Icon::Info => {
                r#"<circle cx="12" cy="12" r="8.5"/><path d="M12 11v5"/><circle class="f" cx="12" cy="8" r="1.1"/>"#
            }
            Icon::Search => r#"<circle cx="11" cy="11" r="6.5"/><path d="M20 20l-4.2-4.2"/>"#,
            Icon::File => r#"<path d="M7 3.5h7l4.5 4.5v12.5H7z"/><path d="M14 3.5V8h4.5"/>"#,
            Icon::Grip => {
                r#"<circle class="f" cx="9.5" cy="6" r="1.5"/><circle class="f" cx="14.5" cy="6" r="1.5"/><circle class="f" cx="9.5" cy="12" r="1.5"/><circle class="f" cx="14.5" cy="12" r="1.5"/><circle class="f" cx="9.5" cy="18" r="1.5"/><circle class="f" cx="14.5" cy="18" r="1.5"/>"#
            }
            Icon::Plus => r#"<path d="M12 5v14M5 12h14"/>"#,
            Icon::Reset => r#"<path d="M4.5 12a7.5 7.5 0 1 0 2.2-5.3"/><path d="M4.5 4.5v4h4"/>"#,
            Icon::External => {
                r#"<path d="M13.5 4.5H19.5V10.5M19.5 4.5L11 13M17 14v4.5a1.5 1.5 0 0 1-1.5 1.5h-10A1.5 1.5 0 0 1 4 18.5v-10A1.5 1.5 0 0 1 5.5 7H10"/>"#
            }
            Icon::Code => r#"<path d="M8.5 7.5L4 12l4.5 4.5M15.5 7.5L20 12l-4.5 4.5"/>"#,
            Icon::ArrowRight => r#"<path d="M5 12h14"/><path d="M13 6l6 6-6 6"/>"#,
            Icon::CapsLock => r#"<path d="M12 4.5l7 7.5h-3.8v5H8.8v-5H5z"/><path d="M8.8 20h6.4"/>"#,
            // Three quarters of a circle; turned by the caller.
            Icon::Spinner => r#"<path d="M12 3.5a8.5 8.5 0 1 1-8.5 8.5"/>"#,
            Icon::Clock => r#"<path d="M12 7.5V12l3 2"/><circle cx="12" cy="12" r="8.5"/>"#,
            Icon::Alert => {
                r#"<circle cx="12" cy="12" r="8.5"/><path d="M12 7.5v5"/><circle class="f" cx="12" cy="16.2" r="1.1"/>"#
            }
            Icon::Window => {
                r#"<rect x="3.5" y="4.5" width="17" height="15" rx="2.5"/><path d="M3.5 8.5h17M6.5 6.5h.01M9 6.5h.01"/>"#
            }
        };
        Cow::Borrowed(s)
    }

    /// The complete SVG document.
    pub fn svg_source(self) -> String {
        let body = self.body();
        if body.starts_with("<svg") {
            return body.into_owned();
        }
        // Filled shapes: give them a fill and no stroke.
        let body = body.replace(r#"class="f""#, r##"fill="#000" stroke="none""##);
        format!(
            r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#000" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">{body}</svg>"##
        )
    }

    /// A battery icon with the level drawn in.
    pub fn battery_svg(percent: f32, charging: bool) -> String {
        let width = (11.5 * (percent / 100.0).clamp(0.08, 1.0) * 10.0).round() / 10.0;
        let bolt = if charging {
            r##"<path fill="#000" stroke="none" opacity=".9" d="M11.8 8.4l-3.6 4.4h3l-.9 2.8 3.6-4.4h-3z"/>"##
        } else {
            ""
        };
        let fill = if charging {
            String::new()
        } else {
            format!(r##"<rect fill="#000" stroke="none" x="4.6" y="9.1" width="{width}" height="5.8" rx="1.2"/>"##)
        };
        format!(
            r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#000" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><rect x="2.5" y="7" width="17" height="10" rx="2.6"/><path d="M21.6 10.5v3"/>{fill}{bolt}</svg>"##
        )
    }
}

/// An icon element at 16 px. Its color is the inherited text color unless set with
/// `text_color`; GPUI's own `svg` paints nothing without an explicit color.
pub fn icon(icon: Icon) -> IconElement {
    IconElement(svg().path(icon.path()).size(px(16.)).flex_none())
}

pub fn icon_sized(icon: Icon, size: f32) -> IconElement {
    IconElement(svg().path(icon.path()).size(px(size)).flex_none())
}

/// An `svg` that falls back to the surrounding text color.
pub struct IconElement(Svg);

impl IconElement {
    /// Any SVG asset path, like a battery level icon.
    pub fn from_path(path: impl Into<SharedString>, size: f32) -> Self {
        IconElement(svg().path(path).size(px(size)).flex_none())
    }

    /// Turned clockwise by `turns` (1.0 is a full turn), for spinners.
    pub fn rotate(self, turns: f32) -> Self {
        IconElement(
            self.0.with_transformation(gpui::Transformation::rotate(gpui::radians(turns * std::f32::consts::TAU))),
        )
    }

    fn inherit_color(&mut self, window: &Window) {
        if self.0.style().text.color.is_none() {
            self.0.style().text.color = Some(window.text_style().color);
        }
    }
}

impl IntoElement for IconElement {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Styled for IconElement {
    fn style(&mut self) -> &mut StyleRefinement {
        self.0.style()
    }
}

impl Element for IconElement {
    type RequestLayoutState = <Svg as Element>::RequestLayoutState;
    type PrepaintState = <Svg as Element>::PrepaintState;

    fn id(&self) -> Option<ElementId> {
        self.0.id()
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        self.0.source_location()
    }

    fn request_layout(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        self.inherit_color(window);
        self.0.request_layout(id, inspector_id, window, cx)
    }

    fn prepaint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        state: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        self.0.prepaint(id, inspector_id, bounds, state, window, cx)
    }

    fn paint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        state: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        // Paint sees the final text style, which may differ from layout's.
        if self.0.style().text.color.is_none() {
            self.inherit_color(window);
        }
        self.0.paint(id, inspector_id, bounds, state, prepaint, window, cx)
    }
}

/// Serves `icons/*.svg` from memory, and falls through to the filesystem for absolute paths.
pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> anyhow::Result<Option<Cow<'static, [u8]>>> {
        if let Some(name) = path.strip_prefix("icons/").and_then(|p| p.strip_suffix(".svg")) {
            if let Some(name) = name.strip_prefix("battery-level-") {
                // battery-level-<percent>[-charging]
                let charging = name.ends_with("-charging");
                let percent = name.trim_end_matches("-charging").parse().unwrap_or(50.0);
                return Ok(Some(Cow::Owned(Icon::battery_svg(percent, charging).into_bytes())));
            }
            return Ok(ALL.iter().find(|i| i.name() == name).map(|i| Cow::Owned(i.svg_source().into_bytes())));
        }
        if path.starts_with('/') {
            return Ok(std::fs::read(path).ok().map(Cow::Owned));
        }
        Ok(None)
    }

    fn list(&self, path: &str) -> anyhow::Result<Vec<SharedString>> {
        if path.trim_end_matches('/') == "icons" {
            return Ok(ALL.iter().map(|i| i.path()).collect());
        }
        Ok(Vec::new())
    }
}

/// The asset path for a battery icon, rounded so few variants get rasterized.
pub fn battery_path(percent: f32, charging: bool) -> SharedString {
    let step = ((percent / 10.0).round() * 10.0).clamp(0.0, 100.0) as u32;
    if charging {
        format!("icons/battery-level-{step}-charging.svg").into()
    } else {
        format!("icons/battery-level-{step}.svg").into()
    }
}

/// A built-in icon for an app with no icon of its own, like a notification from a script,
/// guessed from its name or desktop entry.
pub fn guess_app_icon(app: &str, desktop_entry: &str) -> Icon {
    let name = format!("{app} {desktop_entry}").to_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| name.contains(w));
    if has(&["terminal", "kitty", "alacritty", "foot", "wezterm", "ghostty", "konsole"]) {
        Icon::Terminal
    } else if has(&["firefox", "chrom", "browser", "brave", "zen", "vivaldi", "epiphany"]) {
        Icon::Globe
    } else if has(&["update", "pacman", "paru", "yay", "package", "pamac", "flatpak"]) {
        Icon::Package
    } else if has(&["sync", "nextcloud", "dropbox", "drive", "cloud"]) {
        Icon::Sync
    } else if has(&["spotify", "music", "player", "mpv", "vlc"]) {
        Icon::Music
    } else if has(&["bluetooth", "blueman"]) {
        Icon::Bluetooth
    } else if has(&["network", "nm-applet"]) {
        Icon::Wifi
    } else if has(&["battery", "power", "upower"]) {
        Icon::Battery
    } else {
        Icon::Bell
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_icon_is_valid_svg() {
        for icon in ALL {
            let source = icon.svg_source();
            assert!(source.starts_with("<svg") && source.ends_with("</svg>"), "{icon:?}");
            assert!(!source.contains("class="), "{icon:?} kept a class");
            assert_eq!(source.matches('<').count(), source.matches('>').count(), "{icon:?}");
        }
        assert!(Assets.load("icons/battery-level-40.svg").unwrap().is_some());
        assert!(Assets.load("icons/nope.svg").unwrap().is_none());
    }
}

#[cfg(test)]
mod guess_tests {
    use super::*;

    #[test]
    fn guesses_app_icons() {
        assert_eq!(guess_app_icon("Terminal", ""), Icon::Terminal);
        assert_eq!(guess_app_icon("", "org.mozilla.firefox"), Icon::Globe);
        assert_eq!(guess_app_icon("Syncthing", ""), Icon::Sync);
        assert_eq!(guess_app_icon("notify-send", ""), Icon::Bell);
    }
}
