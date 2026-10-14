//! Config files, app manifests and settings schemas shared by Yozora apps.
//!
//! Files live in `~/.config/yozora/`. `theme.toml` is shared by every app; each app has its own
//! `<id>.toml`. Precedence is built-in defaults, then `theme.toml`, then the app's own `[theme]`
//! table. No file is required: a missing file means defaults.

pub mod bar;
pub mod edit;
pub mod idle;
pub mod launch;
pub mod load;
pub mod lock;
pub mod manifest;
pub mod notify;
pub mod paths;
pub mod schema;
pub mod theme;
pub mod watch;

pub use load::{Loaded, Warning};
