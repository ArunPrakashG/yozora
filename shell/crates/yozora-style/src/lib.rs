//! Tokens, effects levels and motion for every Yozora look. One `Look` picks a full set; nothing
//! else in the shell changes between looks, so a switch re-skins colour, type, shape and motion
//! together.

mod looks;
mod motion;

use rune::{Color, Family};

pub use looks::tokens;
pub use motion::{Entrance, Motion};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Look {
    /// Night. Daybreak is its day mode.
    #[default]
    Tension,
    Daybreak,
    Folio,
    Phosphor,
    Prism,
}

impl Look {
    pub const ALL: [Look; 5] = [Look::Tension, Look::Daybreak, Look::Folio, Look::Phosphor, Look::Prism];

    pub fn name(self) -> &'static str {
        match self {
            Look::Tension => "Surface tension",
            Look::Daybreak => "Daybreak",
            Look::Folio => "Folio",
            Look::Phosphor => "Phosphor",
            Look::Prism => "Prism",
        }
    }

    /// The word the CLI and config use.
    pub fn slug(self) -> &'static str {
        match self {
            Look::Tension => "tension",
            Look::Daybreak => "daybreak",
            Look::Folio => "folio",
            Look::Phosphor => "phosphor",
            Look::Prism => "prism",
        }
    }

    pub fn from_slug(s: &str) -> Option<Look> {
        Look::ALL.into_iter().find(|l| l.slug() == s)
    }
}

/// How much the shell animates and shades. Still draws everything at rest.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Fx {
    #[default]
    Full,
    Calm,
    Still,
}

impl Fx {
    pub fn slug(self) -> &'static str {
        match self {
            Fx::Full => "full",
            Fx::Calm => "calm",
            Fx::Still => "still",
        }
    }

    pub fn from_slug(s: &str) -> Option<Fx> {
        [Fx::Full, Fx::Calm, Fx::Still].into_iter().find(|f| f.slug() == s)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Radii {
    pub panel: f32,
    pub card: f32,
    pub ctl: f32,
    pub chip: f32,
    pub pill: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fonts {
    pub display: Family,
    pub body: Family,
    /// Figures and labels. Tabular.
    pub fig: Family,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tokens {
    pub look: Look,
    pub dark: bool,
    pub bg: Color,
    pub surf: Color,
    /// A second surface a step off `surf`, and the liquid's colour in Surface tension.
    pub surf2: Color,
    pub liq: Color,
    pub raised: Color,
    pub fill: Color,
    pub ink: Color,
    pub ink2: Color,
    pub ink3: Color,
    pub on: Color,
    pub on_ink: Color,
    pub rule: Color,
    pub acc: Color,
    pub on_acc: Color,
    pub alert: Color,
    pub alert_bg: Color,
    pub radii: Radii,
    pub stroke: f32,
    pub fonts: Fonts,
    /// Space above the bar, its height, and where panels start below the top edge.
    pub bar_top: f32,
    pub bar_h: f32,
    pub panel_top: f32,
}

/// Every font file the looks use, for `AppSpec::fonts`.
pub const FONTS: &[&[u8]] = &[
    include_bytes!("../../../assets/fonts/InterVariable.ttf"),
    include_bytes!("../../../assets/fonts/InstrumentSerif-Regular.ttf"),
    include_bytes!("../../../assets/fonts/InstrumentSerif-Italic.ttf"),
    include_bytes!("../../../assets/fonts/Newsreader.ttf"),
    include_bytes!("../../../assets/fonts/Newsreader-Italic.ttf"),
    include_bytes!("../../../assets/fonts/IBMPlexMono-Regular.ttf"),
    include_bytes!("../../../assets/fonts/IBMPlexMono-Medium.ttf"),
    include_bytes!("../../../assets/fonts/JetBrainsMono-Regular.ttf"),
    include_bytes!("../../../assets/fonts/JetBrainsMono-SemiBold.ttf"),
    include_bytes!("../../../assets/fonts/VT323-Regular.ttf"),
    include_bytes!("../../../assets/fonts/Geist.ttf"),
];

mod glyphs;

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use rune::Icon;

/// The glyph `name` drawn the way `look` draws strokes, as `rune::icon` takes it. Built once
/// per (look, name) and kept for the life of the process.
pub fn glyph(look: Look, name: &str) -> &'static Icon {
    static CACHE: OnceLock<Mutex<HashMap<(Look, String), &'static Icon>>> = OnceLock::new();
    let mut cache = CACHE.get_or_init(Default::default).lock().unwrap();
    if let Some(icon) = cache.get(&(look, name.to_owned())) {
        return icon;
    }
    let body = glyphs::GLYPHS.iter().find(|(n, _)| *n == name).map(|(_, b)| *b).unwrap_or_else(|| panic!("no glyph named {name}"));
    let t = tokens(look);
    let (cap, join) = if look == Look::Phosphor { ("square", "miter") } else { ("round", "round") };
    let body = format!(r#"<g fill="none" stroke="currentColor" stroke-width="{}" stroke-linecap="{cap}" stroke-linejoin="{join}">{body}</g>"#, t.stroke);
    let icon: &'static Icon = Box::leak(Box::new(Icon { name: Box::leak(format!("{}-{name}", look.slug()).into_boxed_str()), view: 24.0, body: Box::leak(body.into_boxed_str()) }));
    cache.insert((look, name.to_owned()), icon);
    icon
}
