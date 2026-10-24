use serde_json::json;
use solidcraft_engine::Sel;
use solidcraft_ui_egui::scenario::Harness;

/// V shows or hides a finished sketch whichever way it was picked: its browser row, or in the
/// view one of its profiles, curves or points. A profile or curve picked in the view did nothing.
/// What was picked in the view leaves the selection as the sketch hides (kept, it stayed drawn
/// highlighted), so V again shows it from its row.
#[test]
fn v_hides_a_sketch_picked_by_its_profile_or_curve() {
    for case in ["row", "profile", "curve"] {
        let mut h = Harness::new();
        let id = h.app.run("sketch.create", json!({"plane":"XY"})).unwrap()["sketch"].as_u64().unwrap();
        h.app.run("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[20,10]})).unwrap();
        h.app.run("sketch.finish", json!({})).unwrap();
        h.call("ui.view", json!({"view":"top"}));
        h.call("ui.view", json!({"view":"fit"}));
        h.frames(3);
        match case {
            "row" => h.app.session.selection = vec![Sel::Feature { id }],
            "profile" => h.step(&json!({"click":{"world":[10,5,0]}})).unwrap(),
            _ => h.step(&json!({"click":{"world":[10,0,0]}})).unwrap(),
        }
        assert!(!h.app.session.selection.is_empty(), "{case}: something is picked");
        h.step(&json!({"key":"V"})).unwrap();
        assert!(h.app.session.visibility.hidden_sketches.contains(&id), "{case}: V hides the sketch ({:?})", h.app.session.selection);
        if case == "row" {
            assert_eq!(h.app.session.selection, vec![Sel::Feature { id }], "the row stays picked");
        } else {
            assert!(h.app.session.selection.is_empty(), "{case}: the hidden sketch's pick is dropped ({:?})", h.app.session.selection);
            h.app.session.selection = vec![Sel::Feature { id }];
        }
        h.step(&json!({"key":"V"})).unwrap();
        assert!(!h.app.session.visibility.hidden_sketches.contains(&id), "{case}: V again shows it");
    }
}
