use serde_json::{Value, json};

use crate::{FeatureKind, PlatePick};

fn kind(v: Value) -> FeatureKind {
    serde_json::from_value(v).unwrap()
}

#[test]
fn a_tab_slot_with_every_field_saves_unchanged() {
    let v = json!({
        "type": "tab_slot",
        "tab_plates": [{"key": 1, "body": "Wall", "near": [10.0, 5.0, 3.0]}],
        "slot_plates": [{"key": 2, "body": "Bottom"}, {"key": 4, "body": "Side"}],
        "next_plate_key": 5,
        "count": "3",
        "spacing": "40",
        "tab_width": "20",
        "end_distance": "10",
        "clearance": "0.1",
        "protrusion": "1"
    });
    assert_eq!(serde_json::to_value(kind(v.clone())).unwrap(), v);
}

#[test]
fn a_tab_slot_without_its_optional_fields_gets_the_defaults() {
    let v = json!({
        "type": "tab_slot",
        "tab_plates": [{"body": "Wall"}],
        "slot_plates": [{"body": "Bottom"}],
        "clearance": "0.1"
    });
    let k = kind(v.clone());
    let FeatureKind::TabSlot { tab_plates, slot_plates, next_plate_key, count, spacing, tab_width, end_distance, protrusion, .. } = &k else {
        panic!("not a tab and slot: {k:?}");
    };
    assert!(count.is_none() && spacing.is_none() && tab_width.is_none() && end_distance.is_none() && protrusion.is_none());
    assert_eq!(*next_plate_key, 0);
    let unkeyed = |p: &PlatePick| p.key.is_none() && p.near.is_none();
    assert!(tab_plates.iter().all(unkeyed) && slot_plates.iter().all(unkeyed));
    // The next key is always written; nothing else is added.
    let mut want = v;
    want["next_plate_key"] = json!(0);
    assert_eq!(serde_json::to_value(&k).unwrap(), want);
}

#[test]
fn a_tab_slot_has_its_tags() {
    let k = kind(json!({
        "type": "tab_slot",
        "tab_plates": [{"body": "Wall"}],
        "slot_plates": [{"body": "Bottom"}],
        "clearance": "0.1"
    }));
    assert_eq!(k.kind_key(), "tab_slot");
    assert_eq!(k.type_name(), "TabSlotFeature");
    assert_eq!(k.base_name(), "TabSlot");
    assert_eq!(FeatureKind::type_name_to_kind_key("TabSlotFeature"), Some("tab_slot"));
}

/// Loads a design whose one feature is this Tab and Slot and returns the picks' keys (tab plates
/// first) and `next_plate_key` after the load.
fn keys_after_load(feature: Value) -> (Vec<Option<u32>>, u32) {
    let mut d = crate::Document::new("Keys");
    d.add_feature(kind(feature), None).unwrap();
    let back = crate::format::read(&d.to_json()).unwrap();
    let FeatureKind::TabSlot { tab_plates, slot_plates, next_plate_key, .. } = &back.features[0].kind else {
        panic!("not a tab and slot: {:?}", back.features[0].kind);
    };
    (tab_plates.iter().chain(slot_plates).map(|p| p.key).collect(), *next_plate_key)
}

#[test]
fn a_file_without_plate_keys_gets_them_on_load() {
    let (keys, next) = keys_after_load(json!({
        "type": "tab_slot",
        "tab_plates": [{"body": "Wall"}, {"body": "Back"}],
        "slot_plates": [{"body": "Bottom"}],
        "clearance": "0.1"
    }));
    assert_eq!(keys, [Some(1), Some(2), Some(3)]);
    assert_eq!(next, 4);
}

#[test]
fn loading_keeps_keys_and_never_hands_one_out_twice() {
    // A stale next key below a kept key: minting starts past the largest key.
    let (keys, next) = keys_after_load(json!({
        "type": "tab_slot",
        "tab_plates": [{"key": 7, "body": "Wall"}, {"body": "Back"}],
        "slot_plates": [{"body": "Bottom"}, {"key": 2, "body": "Side"}],
        "next_plate_key": 3,
        "clearance": "0.1"
    }));
    assert_eq!(keys, [Some(7), Some(8), Some(9), Some(2)]);
    assert_eq!(next, 10);
    // A next key past every key (a pick was removed): it is kept, and minting starts there.
    let (keys, next) = keys_after_load(json!({
        "type": "tab_slot",
        "tab_plates": [{"key": 1, "body": "Wall"}],
        "slot_plates": [{"body": "Bottom"}],
        "next_plate_key": 6,
        "clearance": "0.1"
    }));
    assert_eq!(keys, [Some(1), Some(6)]);
    assert_eq!(next, 7);
    // Every pick keyed and the next key right: nothing changes.
    let (keys, next) = keys_after_load(json!({
        "type": "tab_slot",
        "tab_plates": [{"key": 1, "body": "Wall"}],
        "slot_plates": [{"key": 2, "body": "Bottom"}],
        "next_plate_key": 3,
        "clearance": "0.1"
    }));
    assert_eq!(keys, [Some(1), Some(2)]);
    assert_eq!(next, 3);
}
