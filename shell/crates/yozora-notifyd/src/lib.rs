//! The notification daemon, without a UI. The shell draws toasts and the notification center
//! itself; this crate owns `org.freedesktop.Notifications`, the history and the control socket.
//! [`server::should_toast`] and [`server::toast_timeout`] say what a toast should do.

pub mod hypr;
pub mod server;
pub mod store;
