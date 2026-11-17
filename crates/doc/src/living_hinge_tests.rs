use serde_json::{Value, json};

use crate::FeatureKind;
use solidcore_record::living_hinge::HingeClip;

fn kind(v: Value) -> FeatureKind {
    serde_json::from_value(v).unwrap()
}

#[test]
fn a_living_hinge_with_every_field_saves_unchanged() {
    let v = json!({
        "type": "living_hinge",
        "sketch": 3,
        "profiles": {"type": "indices", "indices": [0, 1]},
        "slot_length": "20",
        "bridge": "3",
        "row_spacing": "2",
        "slot_width": "0.3",
        "angle": "90",
        "anchor": [10.0, 5.0, 0.0],
        "clip": "shorten",
        "margin": "1",
        "targets": ["Plate"]
    });
    assert_eq!(serde_json::to_value(kind(v.clone())).unwrap(), v);
}

#[test]
fn a_living_hinge_without_its_optional_fields_gets_the_defaults() {
    let v = json!({
        "type": "living_hinge",
        "sketch": 3,
        "profiles": {"type": "indices", "indices": [0]},
        "slot_length": "20",
        "bridge": "3",
        "row_spacing": "2"
    });
    let k = kind(v.clone());
    let FeatureKind::LivingHinge { slot_width, angle, anchor, clip, margin, targets, .. } = &k else {
        panic!("not a living hinge: {k:?}");
    };
    assert!(slot_width.is_none() && angle.is_none() && anchor.is_none() && margin.is_none() && targets.is_empty());
    assert_eq!(*clip, HingeClip::RunOut);
    // The clip is always written; nothing else is added.
    let mut want = v;
    want["clip"] = json!("run_out");
    assert_eq!(serde_json::to_value(&k).unwrap(), want);
}

#[test]
fn a_living_hinge_has_its_tags() {
    let k = kind(json!({
        "type": "living_hinge",
        "sketch": 3,
        "profiles": {"type": "indices", "indices": [0]},
        "slot_length": "20",
        "bridge": "3",
        "row_spacing": "2"
    }));
    assert_eq!(k.kind_key(), "living_hinge");
    assert_eq!(k.type_name(), "LivingHingeFeature");
    assert_eq!(k.base_name(), "LivingHinge");
    assert_eq!(FeatureKind::type_name_to_kind_key("LivingHingeFeature"), Some("living_hinge"));
}
