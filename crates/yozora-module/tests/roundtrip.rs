use serde_json::{Value, json};
use yozora_module::{HostMessage, Update};

fn check(v: Value) {
    let line = v.to_string();
    let u = Update::parse_line(&line).unwrap();
    let out = serde_json::to_string(&u).unwrap();
    assert_eq!(Update::parse_line(&out).unwrap(), u);
    assert_eq!(serde_json::from_str::<Value>(&out).unwrap(), v);
}

#[test]
fn claude_sessions() {
    check(json!({
        "bar": { "icon": "sparkles", "text": "2", "badge": 1, "tone": "accent" },
        "popover": {
            "title": "Claude sessions",
            "caption": "2 running",
            "body": [
                { "type": "section", "heading": "Active", "rows": [
                    { "type": "row", "icon": "terminal", "title": "yozora",
                      "subtitle": "editing", "value": "12m",
                      "badge": { "text": "busy", "tone": "warning" },
                      "size": "compact", "action": "focus" }
                ]},
                { "type": "buttons", "items": [
                    { "label": "Stop all", "action": "stop", "destructive": true }
                ]}
            ],
            "footer_note": "updated just now"
        }
    }));
}

#[test]
fn updates_list() {
    check(json!({
        "bar": { "icon": "download", "badge": 3 },
        "popover": {
            "title": "Updates",
            "body": [
                { "type": "progress", "title": "Downloading", "subtitle": "linux", "value": 0.5 },
                { "type": "toggle", "title": "Auto", "on": true, "action": "auto" },
                { "type": "empty", "title": "Nothing", "message": "all clear" },
                { "type": "note", "text": "hint" }
            ]
        }
    }));
}

#[test]
fn host_messages() {
    let m: HostMessage = serde_json::from_str(r#"{"event":"action","id":"a","value":0.5}"#).unwrap();
    assert!(matches!(m, HostMessage::Action { .. }));
    let r: HostMessage = serde_json::from_str(r#"{"event":"refresh"}"#).unwrap();
    assert_eq!(r, HostMessage::Refresh);
}
