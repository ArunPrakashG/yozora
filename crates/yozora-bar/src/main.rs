//! Yozora Bar. Phase 1 spike: a layer-shell bar on every monitor and one spring popover.

mod bar;
mod frame_stats;
mod popover;
mod spring;
mod theme;

use std::{sync::OnceLock, time::Instant};

use gpui::App;
use gpui_platform::application;
use tracing_subscriber::EnvFilter;

/// When the process started, for the startup measurement.
pub static STARTED: OnceLock<Instant> = OnceLock::new();

fn main() {
    STARTED.get_or_init(Instant::now);
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_env("YOZORA_LOG").unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    application().run(|cx: &mut App| {
        let displays = cx.displays();
        if displays.is_empty() {
            tracing::error!("no displays reported; is this a Wayland session?");
            cx.quit();
            return;
        }
        for monitor in displays {
            let b = monitor.bounds();
            let label = format!(
                "display {:?} at {},{} {}x{}",
                monitor.id(),
                f32::from(b.origin.x),
                f32::from(b.origin.y),
                f32::from(b.size.width),
                f32::from(b.size.height),
            );
            tracing::info!(
                "opening bar on {label}, refresh {:?}",
                monitor.refresh_interval()
            );
            if let Err(error) = bar::Bar::open(monitor.as_ref(), label, cx) {
                tracing::error!("could not open bar: {error:#}");
            }
        }
    });
}
