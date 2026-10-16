//! The exhaustive `match` in `yozora_ui::kit::display::component_name` has no wildcard arm, so a
//! new `Node` variant fails to compile there: that is the compile-time guarantee. This test checks
//! the names stay in step with `yozora_module::COMPONENTS`.

use std::collections::BTreeSet;

use yozora_module::{COMPONENTS, Node};
use yozora_ui::kit::display::component_name;

fn node(json: &str) -> Node {
    serde_json::from_str(json).expect("valid node")
}

#[test]
fn every_node_maps_to_a_component() {
    let nodes = [
        node(r#"{"type":"section","rows":[]}"#),
        node(r#"{"type":"row","title":"a"}"#),
        node(r#"{"type":"toggle","title":"a","on":true,"action":"x"}"#),
        node(r#"{"type":"slider","title":"a","value":0.5,"action":"x"}"#),
        node(r#"{"type":"progress","title":"a","value":0.5}"#),
        node(r#"{"type":"buttons","items":[]}"#),
        node(r#"{"type":"empty","title":"a"}"#),
        node(r#"{"type":"note","text":"a"}"#),
    ];
    let names: BTreeSet<&str> = nodes.iter().map(component_name).collect();
    assert_eq!(names.len(), 8);
    for n in &names {
        assert!(COMPONENTS.contains(n), "{n} not in COMPONENTS");
    }
}
