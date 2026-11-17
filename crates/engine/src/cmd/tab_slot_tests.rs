use serde_json::{Value, json};

use crate::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id}: {e}"))
}

fn volume(s: &Session, name: &str) -> f64 {
    let st = s.model.state();
    solidcraft_kernel::measure(&st.body(name).unwrap().body).unwrap().volume
}

/// Fixture F (design 16): Bottom 160 x 100 x 6, Wall 120 x 6 x 60 standing on it.
fn fixture_f() -> Session {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"corner": [0, 0, 0], "length": 160, "width": 100, "height": 6, "body_name": "Bottom", "operation": "new"}));
    run(&mut s, "solid.box", json!({"corner": [20, 30, 6], "length": 120, "width": 6, "height": 60, "body_name": "Wall", "operation": "new"}));
    s
}

fn tab_slot(s: &mut Session, extra: Value) -> Result<Value, crate::EngineError> {
    let mut p = json!({"tab_plates": ["Wall"], "slot_plates": ["Bottom"], "count": 3, "tab_width": 20, "end_distance": 10, "name": "Joints"});
    for (k, v) in extra.as_object().cloned().unwrap_or_default() {
        p[k] = v;
    }
    s.execute("solid.tab_slot", &p)
}

#[test]
fn a_tab_slot_cuts_fixture_f() {
    let mut s = fixture_f();
    let reply = tab_slot(&mut s, json!({})).unwrap();
    assert_eq!(reply["name"], "Joints", "{reply}");
    assert!((volume(&s, "Wall") - 45_360.0).abs() < 1e-3, "{reply}");
    assert!((volume(&s, "Bottom") - 93_745.68).abs() < 1e-3, "{reply}");
    assert_eq!(reply["tabs"], 3, "{reply}");
    assert_eq!(reply["slots"], 3, "{reply}");
    assert_eq!(reply["plate_joints"][0]["tab_plate"], "Wall", "{reply}");
    assert_eq!(reply["plate_joints"][0]["slot_plate"], "Bottom", "{reply}");
    assert_eq!(reply["skipped"], json!([]), "{reply}");
    assert_eq!(reply["tool_faces"].as_array().map(Vec::len), Some(2), "{reply}");
    // The feature stores the clearance written out and keys its picks.
    let f = s.doc.features.last().unwrap();
    let solidcraft_doc::FeatureKind::TabSlot { tab_plates, slot_plates, next_plate_key, clearance, .. } = &f.kind else { panic!("{:?}", f.kind) };
    assert_eq!((tab_plates[0].key, slot_plates[0].key, *next_plate_key), (Some(1), Some(2), 3));
    assert_eq!(clearance.to_string(), "0.1 mm");
}

#[test]
fn a_tab_slot_refuses_what_cannot_be_cut() {
    let mut s = fixture_f();
    let n = s.doc.features.len();
    for (extra, words) in [
        (json!({"tab_plates": []}), "Pick the plates"),
        (json!({"tab_plates": ["Wall", "Wall"]}), "Wall is picked twice as a tab plate"),
        (json!({"slot_plates": ["Bottom", "Wall"]}), "Wall is both a tab plate and a slot plate"),
        (json!({"tab_plates": ["Nothing"]}), "no body named Nothing"),
        (json!({"spacing": 30}), "not both"),
        (json!({"count": 0}), "count"),
        (json!({"count": 101}), "count"),
        (json!({"tab_width": 0}), "tab_width"),
        (json!({"clearance": -1}), "clearance"),
        (json!({"end_distance": 200000}), "end_distance"),
        (json!({"tab_plates": [[1, 2, 3]]}), "a plate is a body name"),
        (json!({"tab_plates": [{"body": "Wall", "near": "x"}]}), "near"),
    ] {
        let e = tab_slot(&mut s, extra.clone()).expect_err(&extra.to_string()).to_string();
        assert!(e.contains(words), "{extra}: {e}");
    }
    assert_eq!(s.doc.features.len(), n, "a refused command added a feature");
}

#[test]
fn a_tab_slot_with_every_pair_skipped_fails_no_plate_joint() {
    let mut s = fixture_f();
    // Far stands 4 mm above Bottom: it meets no slot plate.
    run(&mut s, "solid.box", json!({"corner": [20, 30, 10], "length": 120, "width": 6, "height": 60, "body_name": "Far", "operation": "new"}));
    let n = s.doc.features.len();
    let (wall, bottom) = (volume(&s, "Wall"), volume(&s, "Bottom"));
    let e = tab_slot(&mut s, json!({"tab_plates": ["Far"]})).expect_err("every pair skipped").to_string();
    assert!(e.contains("no_plate_joint"), "{e}");
    assert_eq!(s.doc.features.len(), n, "a failed tab and slot was added: {e}");
    assert!((volume(&s, "Wall") - wall).abs() < 1e-6 && (volume(&s, "Bottom") - bottom).abs() < 1e-6, "{e}");
}

#[test]
fn a_tab_slot_edit_is_checked_against_the_model_before_it() {
    let mut s = fixture_f();
    tab_slot(&mut s, json!({})).unwrap();
    for (set, words) in [
        (json!({"count": 0}), "count"),
        (json!({"count": 2, "spacing": 30}), "not both"),
        (json!({"tab_width": 0}), "tab_width"),
        (json!({"tab_plates": [{"key": 1, "body": "Nothing"}]}), "no body named Nothing"),
        (json!({"slot_plates": [{"key": 2, "body": "Wall"}]}), "Wall is both a tab plate and a slot plate"),
    ] {
        let e = s.execute("timeline.edit", &json!({"feature": "Joints", "set": set.clone()})).expect_err(&set.to_string()).to_string();
        assert!(e.contains(words), "{set}: {e}");
        let f = s.doc.features.last().unwrap();
        let solidcraft_doc::FeatureKind::TabSlot { count, .. } = &f.kind else { panic!("{:?}", f.kind) };
        assert_eq!(count.as_ref().map(ToString::to_string).as_deref(), Some("3"), "{set}: a refused edit changed the feature");
    }
    // A good edit recomputes: two tabs of 20 x 6 x 6 on the 120 x 6 x 60 wall.
    run(&mut s, "timeline.edit", json!({"feature": "Joints", "set": {"count": 2}}));
    assert!((volume(&s, "Wall") - 44_640.0).abs() < 1e-3, "{}", volume(&s, "Wall"));
}
