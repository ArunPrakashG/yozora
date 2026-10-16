//! What a module's bar item shows, as plain data.
//!
//! No GPUI and no `crate::` paths: `tests/module_error_state.rs` includes this file directly,
//! because yozora-bar is a binary crate and its modules can't be imported.
#![allow(dead_code)]

use yozora_module::{Tone, Update};

/// The standard error state: the same icon and label for every module that has nothing better to show.
pub const ERROR_ICON: &str = "warning";
pub const ERROR_TEXT: &str = "Error";
/// The icon before a module has reported anything.
pub const PENDING_ICON: &str = "dot";

/// A bar item, ready to draw.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemView {
    pub icon: String,
    pub text: Option<String>,
    pub badge: Option<u32>,
    pub tone: Tone,
    /// The module has failed. Its last good update may still be what is shown.
    pub errored: bool,
}

/// The bar item for a module's state.
///
/// With an error and a last good update, that update stays on screen with the tone forced to
/// warning. With an error and nothing good, the standard error state is shown alone.
pub fn item_view(last_good: Option<&Update>, error: Option<&str>) -> ItemView {
    match (last_good, error) {
        (Some(u), None) => ItemView {
            icon: u.bar.icon.clone(),
            text: u.bar.text.clone(),
            badge: u.bar.badge,
            tone: u.bar.tone,
            errored: false,
        },
        (Some(u), Some(_)) => ItemView {
            icon: u.bar.icon.clone(),
            text: u.bar.text.clone(),
            badge: u.bar.badge,
            tone: Tone::Warning,
            errored: true,
        },
        (None, Some(_)) => ItemView {
            icon: ERROR_ICON.into(),
            text: Some(ERROR_TEXT.into()),
            badge: None,
            tone: Tone::Danger,
            errored: true,
        },
        // Not reported yet: a quiet placeholder rather than nothing, so the zone doesn't jump.
        (None, None) => {
            ItemView { icon: PENDING_ICON.into(), text: None, badge: None, tone: Tone::Neutral, errored: false }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn update() -> Update {
        Update::parse_line(r#"{"bar":{"icon":"star","text":"3","badge":2,"tone":"accent"}}"#).unwrap()
    }

    #[test]
    fn module_render_item_from_update() {
        let v = item_view(Some(&update()), None);
        assert_eq!(
            v,
            ItemView {
                icon: "star".into(),
                text: Some("3".into()),
                badge: Some(2),
                tone: Tone::Accent,
                errored: false
            }
        );
    }

    #[test]
    fn module_render_item_error_without_last_good() {
        let v = item_view(None, Some("boom"));
        assert!(v.errored);
        assert_eq!((v.icon.as_str(), v.tone, v.text.as_deref()), ("warning", Tone::Danger, Some("Error")));
    }

    #[test]
    fn module_render_item_error_keeps_last_good() {
        let v = item_view(Some(&update()), Some("boom"));
        assert!(v.errored);
        assert_eq!((v.icon.as_str(), v.tone, v.text.as_deref(), v.badge), ("star", Tone::Warning, Some("3"), Some(2)));
    }
}
