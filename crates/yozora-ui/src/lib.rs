//! Theme tokens, motion and controls shared by Yozora apps.

pub mod controls;
pub mod fonts;
pub mod frame_stats;
pub mod gpu;
pub mod hypr;
pub mod icon_theme;
pub mod icons;
pub mod input;
pub mod malloc;
pub mod spring;
pub mod theme;

pub use icons::{Assets, Icon, IconElement, icon, icon_sized};
pub use theme::Theme;

/// Closes a window once its current frame is done. Removed while it draws, GPUI logs "window not
/// found" when it then asks the gone window to schedule the next frame.
pub fn close_after_frame(window: &gpui::Window, cx: &mut gpui::App) {
    let handle = window.window_handle();
    cx.spawn(async move |cx| {
        let _ = handle.update(cx, |_, window, _| window.remove_window());
    })
    .detach();
}
