//! Midnight theme tokens from the approved design. Moves to `yozora-ui` when Settings starts.

use gpui::{Hsla, Pixels, Rgba, px, rgba};

pub const BAR_HEIGHT: Pixels = px(34.);
pub const POPOVER_RADIUS: Pixels = px(16.);
pub const ITEM_RADIUS: Pixels = px(8.);

pub fn ink() -> Rgba {
    rgba(0xE8F1FFFF)
}
pub fn ink_2() -> Rgba {
    rgba(0x98A5BEFF)
}
pub fn ink_3() -> Rgba {
    rgba(0x7A869FFF)
}
pub fn accent() -> Rgba {
    rgba(0x8BBEFFFF)
}
pub fn accent_ink() -> Rgba {
    rgba(0x0A0E17FF)
}
pub fn accent_soft() -> Rgba {
    rgba(0x8BBEFF29)
}
/// Bar tint at 55%. Blur comes from a Hyprland layer rule, never from us.
pub fn glass_bar() -> Rgba {
    rgba(0x0A0E178C)
}
/// Popover tint at 70%.
pub fn glass_pop() -> Rgba {
    rgba(0x0E131FB3)
}
pub fn hairline() -> Rgba {
    rgba(0xAAC8FF2B)
}
pub fn fill_2() -> Rgba {
    rgba(0xE8F1FF17)
}
pub fn fill_3() -> Rgba {
    rgba(0xE8F1FF24)
}
pub fn shadow() -> Hsla {
    rgba(0x000000B8).into()
}
