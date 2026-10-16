//! Row truncation rule and tone/size mapping. Only pure functions and state are tested: GPUI
//! elements are not instantiated (no test-support).

use yozora_config::theme::ThemeConfig;
use yozora_module::{Size, Tone};
use yozora_ui::{
    Theme,
    kit::{
        controls::{row_height, tone_color},
        display::ROW_LAYOUT,
    },
};

#[test]
fn value_truncates_before_label() {
    const { assert!(ROW_LAYOUT.value_shrinks) };
    const { assert!(!ROW_LAYOUT.label_shrinks) };
    const { assert!(ROW_LAYOUT.value_max_fraction_percent <= 50) };
}

#[test]
fn compact_is_shorter() {
    assert!(row_height(Size::Compact) < row_height(Size::Regular));
}

#[test]
fn tones_are_distinct() {
    let theme = Theme::new(&ThemeConfig::default(), true, false);
    let tones = [Tone::Neutral, Tone::Accent, Tone::Success, Tone::Warning, Tone::Danger];
    for (i, a) in tones.iter().enumerate() {
        for b in &tones[i + 1..] {
            assert_ne!(tone_color(*a, &theme), tone_color(*b, &theme), "{a:?} vs {b:?}");
        }
    }
}
