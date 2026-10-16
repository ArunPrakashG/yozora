use serde_json::json;
use yozora_module::{COMPONENTS, Popover};

fn pop(body: serde_json::Value) -> Result<Popover, serde_json::Error> {
    serde_json::from_value(json!({ "title": "T", "body": body }))
}

#[test]
fn styling_fields_rejected() {
    for f in ["color", "width", "font", "size_px"] {
        let row = json!([{ "type": "row", "title": "a", f: "x" }]);
        assert!(pop(row).is_err(), "row accepted {f}");
        let sec = json!([{ "type": "section", "rows": [], f: 1 }]);
        assert!(pop(sec).is_err(), "section accepted {f}");
    }
}

#[test]
fn good_tree_validates() {
    let p = pop(json!([
        { "type": "section", "rows": [{ "type": "row", "title": "a", "action": "open" }] },
        { "type": "slider", "title": "v", "value": 0.5, "action": "vol" }
    ]))
    .unwrap();
    assert!(p.validate().is_ok());
}

#[test]
fn too_many_rows_fails() {
    let rows: Vec<_> = (0..9).map(|_| json!({ "type": "row", "title": "a" })).collect();
    let p = pop(json!([{ "type": "section", "rows": rows }])).unwrap();
    assert!(p.validate().is_err());
}

#[test]
fn bad_values_fail() {
    let p = pop(json!([
        { "type": "progress", "title": "p", "value": 1.5 },
        { "type": "toggle", "title": "t", "on": true, "action": "" }
    ]))
    .unwrap();
    assert_eq!(p.validate().unwrap_err().len(), 2);
}

#[test]
fn components_listed() {
    assert!(COMPONENTS.contains(&"badge") && COMPONENTS.contains(&"buttons"));
}
