//! The standard error state for a module's bar item.

#[path = "../src/modules/module_view.rs"]
mod module_view;

use module_view::*;
use yozora_module::{Tone, Update};

fn update() -> Update {
    Update::parse_line(r#"{"bar":{"icon":"star","text":"3 new","badge":3,"tone":"success"}}"#).unwrap()
}

#[test]
fn error_without_last_good_is_the_standard_state() {
    let v = item_view(None, Some("exited with status 1"));
    assert!(v.errored);
    assert_eq!(v.icon, "warning");
    assert_eq!(v.tone, Tone::Danger);
    assert_eq!(v.text.as_deref(), Some("Error"));
    assert_eq!(v.badge, None);
}

#[test]
fn error_with_last_good_keeps_showing_it_as_a_warning() {
    let v = item_view(Some(&update()), Some("timed out"));
    assert!(v.errored);
    assert_eq!(v.tone, Tone::Warning);
    assert_eq!(v.icon, "star");
    assert_eq!(v.text.as_deref(), Some("3 new"));
    assert_eq!(v.badge, Some(3));
}

#[test]
fn no_error_shows_the_update_as_is() {
    let v = item_view(Some(&update()), None);
    assert!(!v.errored);
    assert_eq!((v.icon.as_str(), v.text.as_deref(), v.badge, v.tone), ("star", Some("3 new"), Some(3), Tone::Success));
}
