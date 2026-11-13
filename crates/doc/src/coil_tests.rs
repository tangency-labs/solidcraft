use serde_json::{Value, json};

use crate::FeatureKind;

fn kind(v: Value) -> FeatureKind {
    serde_json::from_value(v).unwrap()
}

#[test]
fn an_old_coil_saves_unchanged() {
    let old = json!({
        "type": "coil",
        "base": [0.0, 0.0, 0.0],
        "axis": [0.0, 0.0, 1.0],
        "diameter": "40",
        "pitch": "8",
        "turns": "4",
        "section_size": "3",
        "section": "circular",
        "operation": "new_body"
    });
    assert_eq!(serde_json::to_value(kind(old.clone())).unwrap(), old);
}

#[test]
fn a_coil_from_a_sketch_saves_its_sketch_and_profiles() {
    let v = json!({
        "type": "coil",
        "base": [0.0, 0.0, 0.0],
        "axis": [0.0, 0.0, 1.0],
        "sketch": 7,
        "profiles": {"type": "indices", "indices": [0]},
        "pitch": "8",
        "turns": "4",
        "section": "circular",
        "operation": "new_body"
    });
    assert_eq!(serde_json::to_value(kind(v.clone())).unwrap(), v);
}

#[test]
fn all_profiles_are_omitted() {
    let v = json!({
        "type": "coil",
        "base": [0.0, 0.0, 0.0],
        "axis": [0.0, 0.0, 1.0],
        "sketch": 7,
        "pitch": "8",
        "turns": "4",
        "section": "circular",
        "operation": "new_body"
    });
    let saved = serde_json::to_value(kind(v.clone())).unwrap();
    assert_eq!(saved, v);
    assert!(saved.get("profiles").is_none());
}
