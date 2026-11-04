//! Logging for the apps that run as services, which also exits them on a lost Wayland connection.

use tracing_log::NormalizeEvent;
use tracing_subscriber::{EnvFilter, Layer, filter::LevelFilter, layer::SubscriberExt, util::SubscriberInitExt};

/// Logs to stderr at `$YOZORA_LOG` (default `info`), and exits with status 1 when the Wayland
/// connection dies so systemd restarts `app`.
pub fn init(app: &'static str) {
    let filter = EnvFilter::try_from_env("YOZORA_LOG").unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer().with_filter(filter))
        .with(ConnectionWatch(app).with_filter(LevelFilter::ERROR))
        .init();
}

/// Exits when the Wayland connection dies.
///
/// A protocol error or broken socket is fatal to the connection, and wayland-backend logs it once
/// when it happens. GPUI does not always stop its event loop then: after a bad `wp_viewport` request
/// it kept spinning with no surfaces (#32). That log line is the one signal every path shares.
struct ConnectionWatch(&'static str);

impl<S: tracing::Subscriber> Layer<S> for ConnectionWatch {
    fn on_event(&self, event: &tracing::Event<'_>, _: tracing_subscriber::layer::Context<'_, S>) {
        let normalized = event.normalized_metadata();
        let target = normalized.as_ref().unwrap_or_else(|| event.metadata()).target();
        // Only the client's own errors: libwayland's log, under `wayland_backend`, also carries warnings.
        if target.starts_with("wayland_backend::") && target.ends_with("::client_impl") {
            eprintln!("{}: lost the Wayland connection; exiting so the service restarts", self.0);
            std::process::exit(1);
        }
    }
}
