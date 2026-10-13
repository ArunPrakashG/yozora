//! Design tokens from the approved design, resolved from `theme.toml`.

use gpui::{App, BoxShadow, Global, Hsla, Pixels, Rgba, SharedString, point, px, rgba};
use yozora_config::theme::{Accent, Glass, Motion, ThemeConfig, ThemeMode};

pub const RADIUS_ITEM: Pixels = px(8.);
pub const RADIUS_ROW: Pixels = px(10.);
pub const RADIUS_TILE: Pixels = px(12.);
pub const RADIUS_POPOVER: Pixels = px(16.);

#[derive(Clone, Debug)]
pub struct Theme {
    pub dark: bool,
    pub font: SharedString,
    pub mono: SharedString,
    pub motion: Motion,

    pub midnight: Rgba,
    pub ink: Rgba,
    pub ink_2: Rgba,
    pub ink_3: Rgba,
    pub on_ink: Rgba,
    pub accent: Rgba,
    pub accent_ink: Rgba,
    pub accent_soft: Rgba,
    pub glass_bar: Rgba,
    pub glass_pop: Rgba,
    pub hair: Rgba,
    pub hair_2: Rgba,
    /// The inner top highlight on glass.
    pub hi: Rgba,
    pub fill: Rgba,
    pub fill_2: Rgba,
    pub fill_3: Rgba,
    pub slider_track: Rgba,
    pub slider_fill: Rgba,
    pub slider_icon: Rgba,
    pub switch_off: Rgba,
    pub danger: Rgba,
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
            (false, Accent::Starlight) => (0x2E6CD9FF, 0x2E6CD924),
            (false, Accent::Aurora) => (0x0D7F69FF, 0x0D7F6924),
            (false, Accent::Nebula) => (0x6A4FDBFF, 0x6A4FDB24),
            (false, Accent::Dawn) => (0xAD5A0DFF, 0xAD5A0D24),
        };
        // Bar and popover tint alpha per glass level. The bar's are the design's. Popovers sit
        // over windows rather than the wallpaper, and Hyprland's blur is often lighter than the
        // design's 40 px, so theirs are higher: at the design's values, text behind a popover
        // stayed readable through it.
        let (bar_a, pop_a) = match (dark, config.glass) {
            (true, Glass::Clear) => (0x52, 0xA6),
            (true, Glass::Balanced) => (0x8C, 0xD4),
            (true, Glass::Frosted) => (0xC7, 0xEB),
            (false, Glass::Clear) => (0x61, 0xB3),
            (false, Glass::Balanced) => (0x99, 0xDB),
            (false, Glass::Frosted) => (0xD6, 0xF0),
        };
        // Without compositor blur, translucent tint just looks dirty; go nearly opaque.
        let (bar_a, pop_a) = if opaque_bar {
            (bar_a.max(0xE6), pop_a.max(0xF0))
        } else {
            // Hyprland 0.56 doesn't blur popups of layer surfaces, even with `blur_popups` and
            // `decoration:blur:popups`, so popovers can't rely on blur behind them.
            (bar_a, pop_a.max(0xEB))
        };
        let with_alpha = |rgb: u32, a: u32| rgba((rgb << 8) | a);

        if dark {
            Self {
                dark,
                font: config.font.clone().into(),
                mono: "Geist Mono".into(),
                motion: config.motion.clone(),
                midnight: rgba(0x0A0E17FF),
                ink: rgba(0xE8F1FFFF),
                ink_2: rgba(0x98A5BEFF),
                ink_3: rgba(0x7A869FFF),
                on_ink: rgba(0x0A0E17FF),
                accent: rgba(accent),
                accent_ink: rgba(0x0A0E17FF),
                accent_soft: rgba(accent_soft),
                glass_bar: with_alpha(0x0A0E17, bar_a),
                glass_pop: with_alpha(0x0E131F, pop_a),
                hair: rgba(0x96B9FF1A),
                hair_2: rgba(0xAAC8FF2B),
                hi: rgba(0xFFFFFF12),
                fill: rgba(0xE8F1FF0E),
                fill_2: rgba(0xE8F1FF17),
                fill_3: rgba(0xE8F1FF24),
                slider_track: rgba(0xE8F1FF14),
                slider_fill: rgba(0xE8F1FFFF),
                slider_icon: rgba(0x0A0E17FF),
                switch_off: rgba(0x7F8CAA52),
                danger: rgba(0xFF8B8BFF),
                shadow: [rgba(0x000000B8), rgba(0x00000047)],
            }
        } else {
            Self {
                dark,
                font: config.font.clone().into(),
                mono: "Geist Mono".into(),
                motion: config.motion.clone(),
                midnight: rgba(0x0A0E17FF),
                ink: rgba(0x0F1626FF),
                ink_2: rgba(0x4A5672FF),
                ink_3: rgba(0x5F6A84FF),
                on_ink: rgba(0xFFFFFFFF),
                accent: rgba(accent),
                accent_ink: rgba(0xFFFFFFFF),
                accent_soft: rgba(accent_soft),
                glass_bar: with_alpha(0xF6F8FC, bar_a),
                glass_pop: with_alpha(0xF8FAFD, pop_a),
                hair: rgba(0x14285A1A),
                hair_2: rgba(0x14285A24),
                hi: rgba(0xFFFFFFBF),
                fill: rgba(0x0F16260B),
                fill_2: rgba(0x0F162613),
                fill_3: rgba(0x0F16261C),
                slider_track: rgba(0x0F162617),
                slider_fill: rgba(0xFFFFFFFF),
                slider_icon: rgba(0x0F1626FF),
                switch_off: rgba(0x7F8CAA52),
                danger: rgba(0xC0392BFF),
                shadow: [rgba(0x1E326452), rgba(0x1E32641A)],
            }
        }
    }

    /// The popover drop shadow: a wide soft one and a tight contact one.
    pub fn popover_shadow(&self) -> Vec<BoxShadow> {
        vec![
            BoxShadow {
                color: self.shadow[0].into(),
                offset: point(px(0.), px(24.)),
                blur_radius: px(50.),
                spread_radius: px(-18.),
                inset: false,
            },
            BoxShadow {
                color: self.shadow[1].into(),
                offset: point(px(0.), px(2.)),
                blur_radius: px(8.),
                spread_radius: px(0.),
                inset: false,
            },
        ]
    }

    /// Glow under the active workspace pill and lit tiles.
    pub fn accent_glow(&self) -> Vec<BoxShadow> {
        let mut glow: Hsla = self.accent.into();
        glow.a = 0.55;
        vec![BoxShadow {
            color: glow,
            offset: point(px(0.), px(4.)),
            blur_radius: px(14.),
            spread_radius: px(-5.),
            inset: false,
        }]
    }

    pub fn global(cx: &App) -> &Self {
        cx.global::<Self>()
    }
}

/// Picks the first installed font of `wanted`, Inter, or the system default.
pub fn resolve_font(cx: &App, wanted: &str) -> SharedString {
    let names = cx.text_system().all_font_names();
    [wanted, "Inter", "Noto Sans", "Cantarell"]
        .into_iter()
        .find(|f| names.iter().any(|n| n == f))
        .map(|f| SharedString::from(f.to_string()))
        .unwrap_or_else(|| ".SystemUIFont".into())
}
