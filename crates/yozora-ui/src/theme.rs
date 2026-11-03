//! Design tokens from the Yozora design language (`design/language/tokens.css`), resolved from
//! `theme.toml`. Each field and const here has a `--yz-*` custom property of the same name there.

use std::time::Duration;

use gpui::{App, BoxShadow, Global, Pixels, Rgba, SharedString, point, px, rgba};
use yozora_config::theme::{Accent, Glass, Motion, ThemeConfig, ThemeMode};

/// Badges and other tiny corners.
pub const RADIUS_XS: Pixels = px(4.);
/// Buttons, list rows, menu items.
pub const RADIUS_SM: Pixels = px(6.);
/// Cards and tiles inside a surface, inputs.
pub const RADIUS_MD: Pixels = px(8.);
/// Toasts.
pub const RADIUS_LG: Pixels = px(10.);
/// Popovers, the launcher, dialogs.
pub const RADIUS_XL: Pixels = px(12.);

/// The 4 px spacing grid, `--yz-space-1` to `--yz-space-9`.
pub const SPACE: [Pixels; 9] = [px(2.), px(4.), px(6.), px(8.), px(12.), px(16.), px(20.), px(24.), px(32.)];

pub const BAR_H: Pixels = px(36.);
pub const CONTROL_H: Pixels = px(32.);
pub const ROW_H: Pixels = px(40.);
pub const TOAST_W: Pixels = px(380.);
pub const POPOVER_W: Pixels = px(380.);

/// Motion tokens. Springs live in `spring.rs` (`SETTLE`, `POP`); these are the durations.
pub mod motion {
    use std::time::Duration;

    /// Press feedback: scale 0.97.
    pub const PRESS: Duration = Duration::from_millis(80);
    /// Hover fills and text color.
    pub const HOVER: Duration = Duration::from_millis(120);
    /// Anything leaving: fade out and travel toward the screen edge.
    pub const EXIT: Duration = Duration::from_millis(140);
    /// Arrivals, when an animation is a duration rather than a spring.
    pub const ENTER: Duration = Duration::from_millis(220);
    /// Reflow after something above has left.
    pub const MOVE: Duration = Duration::from_millis(260);
    /// Delay between list rows when a popover opens, for at most `STAGGER_MAX` rows.
    pub const STAGGER: Duration = Duration::from_millis(18);
    pub const STAGGER_MAX: usize = 6;
    /// Every move, with reduced motion: a fade, no travel.
    pub const REDUCED: Duration = Duration::from_millis(100);
    /// How far a surface travels on its way in, in px.
    pub const RISE: f32 = 8.;
    /// How far a toast travels toward the edge on its way out, in px.
    pub const SLIDE_OUT: f32 = 12.;
}

#[derive(Clone, Debug)]
pub struct Theme {
    pub dark: bool,
    pub font: SharedString,
    pub mono: SharedString,
    pub motion: Motion,

    /// The brand's midnight, the same in both themes, for the logo tile.
    pub midnight: Rgba,
    pub bg: Rgba,
    /// The bar: tint over compositor blur.
    pub glass_bar: Rgba,
    /// Toasts, popovers, the launcher: opaque.
    pub surface: Rgba,
    /// Cards, inputs and wells inside a surface.
    pub surface_2: Rgba,
    pub fill: Rgba,
    pub fill_hover: Rgba,
    pub fill_press: Rgba,
    pub line: Rgba,
    pub line_strong: Rgba,
    /// The 1 px highlight along the top inside edge of a raised surface.
    pub hi: Rgba,
    pub ink: Rgba,
    pub ink_2: Rgba,
    pub ink_3: Rgba,
    /// Text and icons on an `ink` fill, like a slider's.
    pub on_ink: Rgba,
    pub accent: Rgba,
    pub accent_ink: Rgba,
    pub accent_soft: Rgba,
    pub ok: Rgba,
    pub warn: Rgba,
    pub danger: Rgba,
    pub danger_soft: Rgba,
    shadow: [Rgba; 2],
}

impl Global for Theme {}

impl Theme {
    /// Resolves tokens. `system_dark` is the portal's color scheme, used by `auto`.
    pub fn new(config: &ThemeConfig, system_dark: bool, opaque_bar: bool) -> Self {
        let dark = match config.theme {
            ThemeMode::Midnight => true,
            ThemeMode::Daybreak => false,
            ThemeMode::Auto => system_dark,
        };
        let (accent, accent_soft) = match (dark, config.accent) {
            (true, Accent::Starlight) => (0x8BBEFFFF, 0x8BBEFF29),
            (true, Accent::Aurora) => (0x8FE3CFFF, 0x8FE3CF29),
            (true, Accent::Nebula) => (0xBBA8FFFF, 0xBBA8FF2B),
            (true, Accent::Dawn) => (0xFFC58BFF, 0xFFC58B29),
            (false, Accent::Starlight) => (0x2A64CCFF, 0x2A64CC1F),
            (false, Accent::Aurora) => (0x0B7562FF, 0x0B75621F),
            (false, Accent::Nebula) => (0x6A4FDBFF, 0x6A4FDB1F),
            (false, Accent::Dawn) => (0x9A4F0BFF, 0x9A4F0B1F),
        };
        // Bar tint alpha per glass level. Only the bar is glass: Hyprland doesn't blur popups of
        // layer surfaces, so everything the bar opens is an opaque surface.
        let bar_a = match (dark, config.glass) {
            (true, Glass::Clear) => 0x52,
            (true, Glass::Balanced) => 0xB8,
            (true, Glass::Frosted) => 0xD9,
            (false, Glass::Clear) => 0x61,
            (false, Glass::Balanced) => 0xC7,
            (false, Glass::Frosted) => 0xE0,
        };
        // Without compositor blur, translucent tint just looks dirty; go nearly opaque.
        let bar_a = if opaque_bar { bar_a.max(0xE6) } else { bar_a };
        let with_alpha = |rgb: u32, a: u32| rgba((rgb << 8) | a);

        if dark {
            Self {
                dark,
                font: config.font.clone().into(),
                mono: "Geist Mono".into(),
                motion: config.motion.clone(),
                midnight: rgba(0x0A0E17FF),
                bg: rgba(0x0A0E17FF),
                glass_bar: with_alpha(0x0A0E17, bar_a),
                surface: rgba(0x121826FF),
                surface_2: rgba(0x1A2234FF),
                fill: rgba(0xE8F1FF0D),
                fill_hover: rgba(0xE8F1FF14),
                fill_press: rgba(0xE8F1FF1F),
                line: rgba(0xA0BEFF1A),
                line_strong: rgba(0xAAC8FF2B),
                hi: rgba(0xFFFFFF0F),
                ink: rgba(0xEDF2FCFF),
                ink_2: rgba(0xA6B1C7FF),
                ink_3: rgba(0x8794ADFF),
                on_ink: rgba(0x0A0E17FF),
                accent: rgba(accent),
                accent_ink: rgba(0x0A0E17FF),
                accent_soft: rgba(accent_soft),
                ok: rgba(0x8FE3CFFF),
                warn: rgba(0xFFC58BFF),
                danger: rgba(0xFF8F8FFF),
                danger_soft: rgba(0xFF8F8F24),
                shadow: [rgba(0x00000099), rgba(0x00000059)],
            }
        } else {
            Self {
                dark,
                font: config.font.clone().into(),
                mono: "Geist Mono".into(),
                motion: config.motion.clone(),
                midnight: rgba(0x0A0E17FF),
                bg: rgba(0xEEF1F6FF),
                glass_bar: with_alpha(0xF6F8FC, bar_a),
                surface: rgba(0xFFFFFFFF),
                surface_2: rgba(0xF3F5F9FF),
                fill: rgba(0x0F16260A),
                fill_hover: rgba(0x0F162612),
                fill_press: rgba(0x0F16261C),
                line: rgba(0x0F1E461A),
                line_strong: rgba(0x0F1E4629),
                hi: rgba(0xFFFFFFE6),
                ink: rgba(0x0F1626FF),
                ink_2: rgba(0x465270FF),
                ink_3: rgba(0x5A6680FF),
                on_ink: rgba(0xFFFFFFFF),
                accent: rgba(accent),
                accent_ink: rgba(0xFFFFFFFF),
                accent_soft: rgba(accent_soft),
                ok: rgba(0x0B7562FF),
                warn: rgba(0x9A4F0BFF),
                danger: rgba(0xB83426FF),
                danger_soft: rgba(0xB834261A),
                shadow: [rgba(0x14234647), rgba(0x1423461A)],
            }
        }
    }

    /// The one shadow, for raised surfaces: toasts, popovers, the launcher, the OSD, with the `hi`
    /// line along the top inside edge. It reaches 14 px below and 4 px to the sides, so a layer
    /// surface that leaves `SHADOW_REACH` around its card never clips it. It never animates; fade
    /// the whole surface instead.
    pub fn raised_shadow(&self) -> Vec<BoxShadow> {
        vec![
            BoxShadow {
                color: self.shadow[0].into(),
                offset: point(px(0.), px(10.)),
                blur_radius: px(24.),
                spread_radius: px(-10.),
                inset: false,
            },
            BoxShadow {
                color: self.shadow[1].into(),
                offset: point(px(0.), px(1.)),
                blur_radius: px(2.),
                spread_radius: px(0.),
                inset: false,
            },
            self.inset_hi(),
        ]
    }

    /// The 1 px `hi` line along a raised surface's top inside edge.
    pub fn inset_hi(&self) -> BoxShadow {
        BoxShadow {
            color: self.hi.into(),
            offset: point(px(0.), px(1.)),
            blur_radius: px(0.),
            spread_radius: px(0.),
            inset: true,
        }
    }

    pub fn global(cx: &App) -> &Self {
        cx.global::<Self>()
    }

    /// A duration from the motion tokens, or the reduced-motion fade.
    pub fn dur(&self, d: Duration) -> Duration {
        if self.motion.reduced { motion::REDUCED } else { d }
    }
}

/// Room a layer surface leaves around a raised card so `raised_shadow` isn't clipped.
pub const SHADOW_REACH: Pixels = px(16.);

/// Picks the first installed font of `wanted`, Inter, or the system default.
pub fn resolve_font(cx: &App, wanted: &str) -> SharedString {
    first_installed(cx, &[wanted, "Inter", "Noto Sans", "Cantarell"]).unwrap_or_else(|| ".SystemUIFont".into())
}

/// Geist Mono for numbers, or the closest monospace font installed.
pub fn resolve_mono(cx: &App) -> SharedString {
    first_installed(cx, &["Geist Mono", "JetBrains Mono", "DejaVu Sans Mono", "Adwaita Mono"])
        .unwrap_or_else(|| "monospace".into())
}

fn first_installed(cx: &App, families: &[&str]) -> Option<SharedString> {
    let names = cx.text_system().all_font_names();
    families.iter().find(|f| names.iter().any(|n| n == *f)).map(|f| SharedString::from(f.to_string()))
}

impl Theme {
    /// Swaps the font tokens for installed fonts: `wanted` for text, and a monospace face.
    pub fn resolve_fonts(&mut self, cx: &App, wanted: &str) {
        self.font = resolve_font(cx, wanted);
        self.mono = resolve_mono(cx);
    }
}
