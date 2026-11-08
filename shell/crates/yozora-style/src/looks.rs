use rune::{Color, Family};

use crate::{Fonts, Look, Radii, Tokens};

/// sRGB `0xRRGGBB` with straight alpha, as the mockups' CSS writes colours.
fn c(rgb: u32, a: f32) -> Color {
    let d = |v: u32| {
        let s = (v & 0xff) as f32 / 255.0;
        if s <= 0.04045 { s / 12.92 } else { ((s + 0.055) / 1.055).powf(2.4) }
    };
    Color { r: d(rgb >> 16), g: d(rgb >> 8), b: d(rgb), a }
}

const INTER: Family = Family::Named("Inter Variable");
const SERIF: Family = Family::Named("Instrument Serif");
const NEWSREADER: Family = Family::Named("Newsreader");
const PLEX: Family = Family::Named("IBM Plex Mono");
const JETBRAINS: Family = Family::Named("JetBrains Mono");
const VT: Family = Family::Named("VT323");
const GEIST: Family = Family::Named("Geist");

pub fn tokens(look: Look) -> Tokens {
    match look {
        Look::Tension => Tokens {
            look, dark: true,
            bg: c(0x05060B, 1.0), surf: c(0x11121D, 1.0), surf2: c(0x1C1D28, 1.0), liq: c(0x11121D, 1.0), raised: c(0xFFFFFF, 0.06), fill: c(0xFFFFFF, 0.13),
            ink: c(0xEEF0FA, 1.0), ink2: c(0xEEF0FA, 0.64), ink3: c(0xEEF0FA, 0.38),
            on: c(0xFFB86B, 1.0), on_ink: c(0x1B1206, 1.0), rule: c(0xFFFFFF, 0.09),
            acc: c(0xFFB86B, 1.0), on_acc: c(0x1B1206, 1.0), alert: c(0xFF7A7A, 1.0), alert_bg: c(0x1A1420, 1.0),
            radii: Radii { panel: 24.0, card: 16.0, ctl: 12.0, chip: 8.0, pill: 999.0 },
            stroke: 1.75, fonts: Fonts { display: SERIF, body: INTER, fig: INTER },
            bar_top: 8.0, bar_h: 36.0, panel_top: 66.0,
        },
        Look::Daybreak => Tokens {
            look, dark: false,
            bg: c(0xE8E5F0, 1.0), surf: c(0xFBFAFE, 1.0), surf2: c(0xF0EEF5, 1.0), liq: c(0xFBFAFE, 1.0), raised: c(0x14131C, 0.045), fill: c(0x14131C, 0.09),
            ink: c(0x14131C, 1.0), ink2: c(0x14131C, 0.66), ink3: c(0x14131C, 0.36),
            on: c(0xA35407, 1.0), on_ink: c(0xFFF8EE, 1.0), rule: c(0x14131C, 0.09),
            acc: c(0xA35407, 1.0), on_acc: c(0xFFF8EE, 1.0), alert: c(0xC0362C, 1.0), alert_bg: c(0xFBEDEC, 1.0),
            radii: Radii { panel: 24.0, card: 16.0, ctl: 12.0, chip: 8.0, pill: 999.0 },
            stroke: 1.75, fonts: Fonts { display: SERIF, body: INTER, fig: INTER },
            bar_top: 8.0, bar_h: 36.0, panel_top: 66.0,
        },
        Look::Folio => Tokens {
            look, dark: false,
            bg: c(0xE9E3D6, 1.0), surf: c(0xF8F4EB, 1.0), surf2: c(0xF8F4EB, 1.0), liq: c(0xF8F4EB, 1.0), raised: c(0x000000, 0.0), fill: c(0x1B1915, 0.07),
            ink: c(0x1B1915, 1.0), ink2: c(0x1B1915, 0.68), ink3: c(0x1B1915, 0.34),
            on: c(0x1B1915, 1.0), on_ink: c(0xF8F4EB, 1.0), rule: c(0x1B1915, 0.18),
            acc: c(0xB8321A, 1.0), on_acc: c(0xFFF8F0, 1.0), alert: c(0x1B1915, 1.0), alert_bg: c(0x1B1915, 1.0),
            radii: Radii { panel: 2.0, card: 0.0, ctl: 2.0, chip: 2.0, pill: 2.0 },
            stroke: 1.35, fonts: Fonts { display: NEWSREADER, body: NEWSREADER, fig: PLEX },
            bar_top: 0.0, bar_h: 40.0, panel_top: 41.0,
        },
        Look::Phosphor => Tokens {
            look, dark: true,
            bg: c(0x020503, 1.0), surf: c(0x020905, 0.96), surf2: c(0x020905, 0.96), liq: c(0x020905, 0.96), raised: c(0x8CFFB4, 0.05), fill: c(0x8CFFB4, 0.12),
            ink: c(0x8CFFB4, 1.0), ink2: c(0x8CFFB4, 0.64), ink3: c(0x8CFFB4, 0.3),
            on: c(0x8CFFB4, 1.0), on_ink: c(0x021006, 1.0), rule: c(0x8CFFB4, 0.24),
            acc: c(0x8CFFB4, 1.0), on_acc: c(0x021006, 1.0), alert: c(0xFFB000, 1.0), alert_bg: c(0xFFB000, 0.08),
            radii: Radii { panel: 0.0, card: 0.0, ctl: 0.0, chip: 0.0, pill: 0.0 },
            stroke: 2.0, fonts: Fonts { display: VT, body: JETBRAINS, fig: JETBRAINS },
            bar_top: 0.0, bar_h: 28.0, panel_top: 38.0,
        },
        Look::Prism => Tokens {
            look, dark: false,
            bg: c(0xECEEF6, 1.0), surf: c(0xFFFFFF, 0.62), surf2: c(0xFFFFFF, 0.62), liq: c(0xFFFFFF, 0.62), raised: c(0xFFFFFF, 0.64), fill: c(0xFFFFFF, 0.92),
            ink: c(0x11131F, 1.0), ink2: c(0x11131F, 0.62), ink3: c(0x11131F, 0.3),
            on: c(0x2F5BFF, 1.0), on_ink: c(0xFFFFFF, 1.0), rule: c(0x11131F, 0.09),
            acc: c(0x2F5BFF, 1.0), on_acc: c(0xFFFFFF, 1.0), alert: c(0xC62828, 1.0), alert_bg: c(0xFFECEC, 0.8),
            radii: Radii { panel: 28.0, card: 18.0, ctl: 14.0, chip: 10.0, pill: 999.0 },
            stroke: 1.6, fonts: Fonts { display: GEIST, body: GEIST, fig: GEIST },
            bar_top: 8.0, bar_h: 40.0, panel_top: 60.0,
        },
    }
}
