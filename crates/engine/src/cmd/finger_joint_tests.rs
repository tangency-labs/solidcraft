#![allow(clippy::unwrap_used)]

use serde_json::{Value, json};

use crate::Session;

fn run(s: &mut Session, id: &str, p: &Value) -> Value {
    s.execute(id, p).unwrap_or_else(|e| panic!("{id}: {e}"))
}

fn crossing_blocks() -> Session {
    let mut s = Session::default();
    run(&mut s, "solid.box", &json!({"length":100,"width":3,"height":50,"body_name":"A","operation":"new"}));
    run(&mut s, "solid.box", &json!({"corner":[48.5,-48.5,0],"length":3,"width":100,"height":50,"body_name":"B","operation":"new"}));
    s
}

#[test]
fn finger_joint_cuts_complementary_teeth_and_keeps_both_bodies() {
    let mut s = crossing_blocks();
    let before_a = solidcraft_kernel::measure(&s.model.state().body("A").unwrap().body).unwrap().volume;
    let before_b = solidcraft_kernel::measure(&s.model.state().body("B").unwrap().body).unwrap().volume;

    run(&mut s, "solid.finger_joint", &json!({"bodies":["A","B"],"count":3,"clearance":0.1,"name":"Joint"}));

    let state = s.model.state();
    let a = &state.body("A").unwrap().body;
    let b = &state.body("B").unwrap().body;
    assert_eq!(state.bodies.len(), 2);
    assert!(solidcraft_kernel::measure(a).unwrap().volume < before_a);
    assert!(solidcraft_kernel::measure(b).unwrap().volume < before_b);
    assert!(solidcraft_kernel::boolean(a, b, solidcraft_kernel::BoolOp::Intersect).unwrap().is_none());
}

#[test]
fn finger_joint_patterns_have_matching_ends_and_one_extra_segment() {
    let mut s = crossing_blocks();
    let before_a = solidcraft_kernel::measure(&s.model.state().body("A").unwrap().body).unwrap().volume;
    let before_b = solidcraft_kernel::measure(&s.model.state().body("B").unwrap().body).unwrap().volume;

    run(&mut s, "solid.finger_joint", &json!({"bodies":["A","B"],"count":3}));

    let state = s.model.state();
    let after_a = solidcraft_kernel::measure(&state.body("A").unwrap().body).unwrap().volume;
    let after_b = solidcraft_kernel::measure(&state.body("B").unwrap().body).unwrap().volume;
    let removed_a = before_a - after_a;
    let removed_b = before_b - after_b;
    let segment_volume = 3.0 * 3.0 * 50.0 / 7.0;
    let expected_a = segment_volume * 3.0;
    let expected_b = segment_volume * 4.0;
    assert!((removed_a - expected_a).abs() < expected_a * 1e-5, "{removed_a} vs {expected_a}");
    assert!((removed_b - expected_b).abs() < expected_b * 1e-5, "{removed_b} vs {expected_b}");
    assert!(
        solidcraft_kernel::boolean(&state.body("A").unwrap().body, &state.body("B").unwrap().body, solidcraft_kernel::BoolOp::Intersect)
            .unwrap()
            .is_none()
    );
}

#[test]
fn finger_joint_cuts_long_overlap_without_removing_surrounding_material() {
    use solidcore_record::Vec3;
    use solidcraft_kernel::PointClass::{Inside, Outside};

    for clearance in [0.0, 0.2] {
        let mut s = Session::default();
        run(&mut s, "solid.box", &json!({"corner":[-200,0,0],"length":450,"width":9.5,"height":254,"body_name":"A","operation":"new"}));
        run(&mut s, "solid.box", &json!({"corner":[148,-100,0],"length":14,"width":254,"height":254,"body_name":"B","operation":"new"}));
        let before = (volume(&s, "A"), volume(&s, "B"));
        let feature = run(&mut s, "solid.finger_joint", &json!({"bodies":["A","B"],"count":3,"clearance":clearance}));
        assert!(s.model.result(feature["feature"].as_u64().unwrap()).unwrap().warning.is_none());
        let width = 254.0 / 7.0;
        let expected_a = 9.5 * (14.0 + clearance) * (3.0 * width + 3.0 * clearance);
        let expected_b = 14.0 * (9.5 + clearance) * (4.0 * width + 3.0 * clearance);
        for (name, original, expected) in [("A", before.0, expected_a), ("B", before.1, expected_b)] {
            let removed = original - volume(&s, name);
            assert!((removed - expected).abs() < 0.05, "{name} clearance {clearance}: {removed} vs {expected}");
        }
        let state = s.model.state();
        let a = &state.body("A").unwrap().body;
        let b = &state.body("B").unwrap().body;
        for (height, a_expected, b_expected) in [(0.5 * width, Inside, Outside), (1.5 * width, Outside, Inside)] {
            let point = Vec3::new(155.0, 4.75, height);
            assert_eq!(a.classify_point(point).unwrap(), a_expected);
            assert_eq!(b.classify_point(point).unwrap(), b_expected);
        }
        assert_eq!(a.classify_point(Vec3::new(147.0, 4.75, 1.5 * width)).unwrap(), Inside);
        assert_eq!(b.classify_point(Vec3::new(155.0, 10.5, 0.5 * width)).unwrap(), Inside);
        no_overlap_after(&s);
    }
}

#[test]
fn finger_joint_inversion_and_edge_offset_keep_three_pairs_and_exact_geometry() {
    let mut s = crossing_blocks();
    let before_a = solidcraft_kernel::measure(&s.model.state().body("A").unwrap().body).unwrap().volume;
    let before_b = solidcraft_kernel::measure(&s.model.state().body("B").unwrap().body).unwrap().volume;

    run(&mut s, "solid.finger_joint", &json!({"bodies":["A","B"],"count":3,"inverted":true,"edge_offset":"1 mm"}));

    let state = s.model.state();
    let after_a = solidcraft_kernel::measure(&state.body("A").unwrap().body).unwrap().volume;
    let after_b = solidcraft_kernel::measure(&state.body("B").unwrap().body).unwrap().volume;
    let segment_width = 48.0 / 7.0;
    let removed_a = before_a - after_a;
    let removed_b = before_b - after_b;
    let expected_a = 9.0 * (4.0 * segment_width + 2.0);
    let expected_b = 9.0 * 3.0 * segment_width;
    assert!((removed_a - expected_a).abs() < expected_a * 1e-5, "{removed_a} vs {expected_a}");
    assert!((removed_b - expected_b).abs() < expected_b * 1e-5, "{removed_b} vs {expected_b}");
    assert!(
        solidcraft_kernel::boolean(&state.body("A").unwrap().body, &state.body("B").unwrap().body, solidcraft_kernel::BoolOp::Intersect)
            .unwrap()
            .is_none()
    );
}

#[test]
fn finger_joint_accepts_crossing_blocks_with_a_small_extra_overhang() {
    let mut s = Session::default();
    run(&mut s, "solid.box", &json!({"length":50,"width":3,"height":50,"body_name":"A","operation":"new"}));
    run(&mut s, "solid.box", &json!({"corner":[48.5,-48.5,0],"length":3,"width":100,"height":50,"body_name":"B","operation":"new"}));

    run(&mut s, "solid.finger_joint", &json!({"bodies":["A","B"]}));

    let state = s.model.state();
    assert!(
        solidcraft_kernel::boolean(&state.body("A").unwrap().body, &state.body("B").unwrap().body, solidcraft_kernel::BoolOp::Intersect)
            .unwrap()
            .is_none()
    );
}

#[test]
fn finger_joint_defaults_to_zero_clearance() {
    let mut s = crossing_blocks();
    run(&mut s, "solid.finger_joint", &json!({"bodies":["A","B"],"count":2,"name":"Joint"}));
    let state = s.model.state();
    assert!(
        solidcraft_kernel::boolean(&state.body("A").unwrap().body, &state.body("B").unwrap().body, solidcraft_kernel::BoolOp::Intersect)
            .unwrap()
            .is_none()
    );
}

#[test]
fn finger_joint_clearance_is_half_on_each_side_of_each_plate() {
    use solidcore_record::Vec3;
    use solidcraft_kernel::PointClass::{Inside, Outside};

    let mut s = crossing_blocks();
    let (before_a, before_b) = (volume(&s, "A"), volume(&s, "B"));
    run(&mut s, "solid.finger_joint", &json!({"bodies":["A","B"],"count":3,"clearance":"1 mm"}));
    let state = s.model.state();
    let a = &state.body("A").unwrap().body;
    let b = &state.body("B").unwrap().body;
    let width = 50.0 / 7.0;
    let expected_a = 12.0 * (3.0 * width + 3.0);
    let expected_b = 12.0 * (4.0 * width + 3.0);
    let removed_a = before_a - volume(&s, "A");
    let removed_b = before_b - volume(&s, "B");
    assert!((removed_a - expected_a).abs() < expected_a * 1e-5, "A: {removed_a} vs {expected_a}");
    assert!((removed_b - expected_b).abs() < expected_b * 1e-5, "B: {removed_b} vs {expected_b}");
    for (x, expected) in [(47.9, Inside), (48.1, Outside), (51.9, Outside), (52.1, Inside)] {
        assert_eq!(a.classify_point(Vec3::new(x, 1.5, 1.5 * width)).unwrap(), expected, "A transverse {x}");
    }
    for (y, expected) in [(-0.6, Inside), (-0.4, Outside), (3.4, Outside), (3.6, Inside)] {
        assert_eq!(b.classify_point(Vec3::new(50.0, y, 0.5 * width)).unwrap(), expected, "B transverse {y}");
    }
    for (delta, expected_a, expected_b) in [(-0.6, Inside, Outside), (-0.4, Outside, Outside), (0.4, Outside, Outside), (0.6, Outside, Inside)] {
        let point = Vec3::new(50.0, 1.5, width + delta);
        assert_eq!(a.classify_point(point).unwrap(), expected_a, "A longitudinal {delta}");
        assert_eq!(b.classify_point(point).unwrap(), expected_b, "B longitudinal {delta}");
    }
    no_overlap_after(&s);
}

#[test]
fn finger_joint_clearance_must_leave_a_positive_finger_width() {
    let mut valid = crossing_blocks();
    run(&mut valid, "solid.finger_joint", &json!({"bodies":["A","B"],"count":3,"clearance":"4 mm"}));
    no_overlap_after(&valid);

    let mut invalid = crossing_blocks();
    let error = invalid.execute("solid.finger_joint", &json!({"bodies":["A","B"],"count":3,"clearance":"8 mm"})).unwrap_err();
    assert!(error.to_string().contains("less than a finger width"), "{error}");
    assert_eq!(invalid.doc.features.len(), 2);
}

#[test]
fn finger_joint_uses_two_selected_bodies_when_names_are_omitted() {
    for params in [json!({}), json!({"bodies":null})] {
        let mut s = crossing_blocks();
        s.selection = vec![crate::Sel::Body { name: "A".into() }, crate::Sel::Body { name: "B".into() }];

        run(&mut s, "solid.finger_joint", &params);

        assert_eq!(s.doc.features.len(), 3);
        assert!(s.model.state().body("A").is_some());
        assert!(s.model.state().body("B").is_some());
    }
}

#[test]
fn finger_joint_rejects_an_explicitly_empty_body_list_instead_of_using_selection() {
    let mut s = crossing_blocks();
    s.selection = vec![crate::Sel::Body { name: "A".into() }, crate::Sel::Body { name: "B".into() }];

    assert_bad_finger_joint_params(&mut s, &json!({"bodies":[]}));
}

#[test]
fn finger_joint_rejects_bodies_outside_the_active_component_before_creating_a_feature() {
    let mut s = crossing_blocks();
    run(&mut s, "component.from_bodies", &json!({"bodies":["B"]}));

    assert_bad_finger_joint_params(&mut s, &json!({"bodies":["A","B"]}));
}

#[test]
fn finger_joint_null_options_keep_their_defaults() {
    let mut s = crossing_blocks();
    run(&mut s, "solid.finger_joint", &json!({"bodies":["A","B"],"clearance":null,"edge_offset":null,"inverted":null}));
    let kind = &s.doc.features.last().unwrap().kind;
    assert!(
        matches!(kind, solidcraft_doc::FeatureKind::FingerJoint { clearance, edge_offset: None, inverted: false, .. } if clearance.as_str() == "0")
    );
    no_overlap_after(&s);
}

#[test]
fn finger_joint_only_defaults_invalid_or_negative_values_when_they_are_absent_or_null() {
    for params in [
        json!({"bodies":["A","B"],"clearance":true}),
        json!({"bodies":["A","B"],"edge_offset":true}),
        json!({"bodies":["A","B"],"inverted":"false"}),
        json!({"bodies":["A","B"],"clearance":"-0.1 mm"}),
        json!({"bodies":["A","B"],"edge_offset":"-0.1 mm"}),
    ] {
        let mut s = crossing_blocks();
        assert_bad_finger_joint_params(&mut s, &params);
    }
}

fn assert_bad_finger_joint_params(s: &mut Session, params: &Value) {
    let feature_count = s.doc.features.len();
    let error = s.execute("solid.finger_joint", params).unwrap_err();
    assert!(matches!(&error, crate::EngineError::BadParams { .. }), "expected BadParams, got {error}");
    assert_eq!(s.doc.features.len(), feature_count, "invalid input must not add a feature");
}

#[test]
fn finger_joint_rejects_bad_counts_and_ambiguous_intersections() {
    let mut s = crossing_blocks();
    assert!(s.execute("solid.finger_joint", &json!({"bodies":["A","B"],"count":0})).is_err());
    assert_eq!(s.doc.features.len(), 2);

    let mut ambiguous = Session::default();
    run(&mut ambiguous, "solid.box", &json!({"length":10,"width":3,"height":3,"body_name":"A","operation":"new"}));
    run(&mut ambiguous, "solid.box", &json!({"corner":[4,-3,0],"length":3,"width":10,"height":3,"body_name":"B","operation":"new"}));
    let before = solidcraft_kernel::measure(&ambiguous.model.state().body("A").unwrap().body).unwrap().volume;
    assert!(ambiguous.execute("solid.finger_joint", &json!({"bodies":["A","B"],"count":2})).is_err());
    let after = solidcraft_kernel::measure(&ambiguous.model.state().body("A").unwrap().body).unwrap().volume;
    assert_eq!(before, after);
    assert_eq!(ambiguous.doc.features.len(), 2, "an ambiguous plate pair must not add a failed feature");

    let mut invalid_offset = crossing_blocks();
    assert!(invalid_offset.execute("solid.finger_joint", &json!({"bodies":["A","B"],"edge_offset":"25 mm"})).is_err());
    assert_eq!(invalid_offset.doc.features.len(), 2);

    let mut negative_offset = crossing_blocks();
    assert!(negative_offset.execute("solid.finger_joint", &json!({"bodies":["A","B"],"edge_offset":"-1 mm"})).is_err());
    assert_eq!(negative_offset.doc.features.len(), 2);
}

fn no_overlap_after(s: &Session) {
    let state = s.model.state();
    assert!(
        solidcraft_kernel::boolean(&state.body("A").unwrap().body, &state.body("B").unwrap().body, solidcraft_kernel::BoolOp::Intersect)
            .unwrap()
            .is_none()
    );
}

fn joined(a: Value, b: Value) -> Session {
    let mut s = Session::default();
    let mut a = a;
    a["body_name"] = json!("A");
    a["operation"] = json!("new");
    let mut b = b;
    b["body_name"] = json!("B");
    b["operation"] = json!("new");
    run(&mut s, "solid.box", &a);
    run(&mut s, "solid.box", &b);
    s
}

fn angled_crossing_blocks() -> Session {
    let mut s = Session::default();
    run(&mut s, "solid.box", &json!({"length":100,"width":3,"height":50,"body_name":"A","operation":"new"}));
    run(&mut s, "solid.box", &json!({"center":[0,0,25],"length":3,"width":100,"height":50,"body_name":"B","operation":"new"}));
    run(&mut s, "solid.move", &json!({"bodies":["B"],"axis":[0,0,1],"angle":"60 deg"}));
    run(&mut s, "solid.move", &json!({"bodies":["B"],"translate":[50,1.5,0]}));
    s
}

#[test]
fn finger_joint_accepts_plates_of_different_heights() {
    let mut s = joined(json!({"length":100,"width":3,"height":80}), json!({"corner":[48.5,-48.5,10],"length":3,"width":100,"height":50}));
    run(&mut s, "solid.finger_joint", &json!({"bodies":["A","B"],"count":3}));
    no_overlap_after(&s);
}

#[test]
fn finger_joint_accepts_a_plate_flush_with_the_end_of_the_other() {
    let mut s = joined(json!({"length":100,"width":3,"height":50}), json!({"corner":[48.5,-97,0],"length":3,"width":100,"height":50}));
    run(&mut s, "solid.finger_joint", &json!({"bodies":["A","B"],"count":3}));
    no_overlap_after(&s);
}

#[test]
fn finger_joint_accepts_a_plate_passing_through_beyond_the_edge() {
    let mut s = joined(json!({"length":100,"width":3,"height":50}), json!({"corner":[48.5,-48.5,-20],"length":3,"width":100,"height":90}));
    run(&mut s, "solid.finger_joint", &json!({"bodies":["A","B"],"count":3}));
    no_overlap_after(&s);
}

#[test]
fn finger_joint_accepts_a_plate_at_an_angle() {
    let mut s = angled_crossing_blocks();
    let (before_a, before_b) = (volume(&s, "A"), volume(&s, "B"));

    run(&mut s, "solid.finger_joint", &json!({"bodies":["A","B"],"count":3}));

    assert!(volume(&s, "A") < before_a);
    assert!(volume(&s, "B") < before_b);
    for (name, plate_normal) in [("A", [0.0, 1.0, 0.0]), ("B", [0.5, 0.8660254037844386, 0.0])] {
        let faces = s.model.state().body(name).unwrap().body.faces(1e-4).unwrap();
        assert!(faces.len() > 6, "{name} should have cut faces");
        for face in faces {
            let normal = face.plane_normal.expect("box plates and their cuts have planar faces");
            let cosine = (normal.x * plate_normal[0] + normal.y * plate_normal[1] + normal.z * plate_normal[2]).abs();
            assert!(cosine < 1e-6 || (cosine - 1.0).abs() < 1e-6, "{name} has an inclined face: {normal:?}");
        }
    }
    no_overlap_after(&s);
}

#[test]
fn finger_joint_clearance_expands_perpendicular_cuts_on_angled_plates() {
    let mut s = angled_crossing_blocks();
    let (before_a, before_b) = (volume(&s, "A"), volume(&s, "B"));
    let clearance = 0.4;
    run(&mut s, "solid.finger_joint", &json!({"bodies":["A","B"],"count":3,"clearance":clearance}));
    let projected_width = 3.0 * (1.0 + 3.0_f64.sqrt() / 2.0) / 0.5;
    let segment = 50.0 / 7.0;
    for (name, before, segment_length) in [("A", before_a, 3.0 * segment + 3.0 * clearance), ("B", before_b, 4.0 * segment + 3.0 * clearance)] {
        let expected = 3.0 * (projected_width + clearance) * segment_length;
        let removed = before - volume(&s, name);
        assert!((removed - expected).abs() < expected * 1e-5, "{name}: {removed} vs {expected}");
    }
    no_overlap_after(&s);
}

#[test]
fn finger_joint_angled_cuts_go_through_the_plate() {
    let mut s = Session::default();
    run(&mut s, "solid.box", &json!({"length":100,"width":3,"height":30,"body_name":"A","operation":"new"}));
    run(&mut s, "solid.box", &json!({"center":[0,0,0],"length":3,"width":100,"height":90,"body_name":"B","operation":"new"}));
    run(&mut s, "solid.move", &json!({"bodies":["B"],"axis":[0,0,1],"angle":"60 deg"}));
    run(&mut s, "solid.move", &json!({"bodies":["B"],"translate":[50,1.5,-25]}));

    let overlap = solidcraft_kernel::boolean(
        &s.model.state().body("A").unwrap().body,
        &s.model.state().body("B").unwrap().body,
        solidcraft_kernel::BoolOp::Intersect,
    )
    .unwrap()
    .unwrap();
    let overlap_volume = solidcraft_kernel::measure(&overlap).unwrap().volume;
    let (before_a, before_b) = (volume(&s, "A"), volume(&s, "B"));

    run(&mut s, "solid.finger_joint", &json!({"bodies":["A","B"],"count":3}));

    let removed_a = before_a - volume(&s, "A");
    let removed_b = before_b - volume(&s, "B");
    let projected_area = 9.0 * (1.0 + 3.0_f64.sqrt() / 2.0) / 0.5;
    let expected_a = projected_area * 20.0 * 3.0 / 7.0;
    let expected_b = projected_area * 20.0 * 4.0 / 7.0;
    assert!((removed_a - expected_a).abs() < expected_a * 1e-5, "{removed_a} vs {expected_a}");
    assert!((removed_b - expected_b).abs() < expected_b * 1e-5, "{removed_b} vs {expected_b}");
    assert!(removed_a + removed_b > overlap_volume, "through-cuts must extend beyond the angled overlap");
    no_overlap_after(&s);
}

#[test]
fn finger_joint_patterns_separate_overlap_regions_independently() {
    use solidcore_record::Vec3;
    use solidcraft_kernel::PointClass::{Inside, Outside};

    for (clearance, expected_a, expected_b) in [(0.0, 126.0, 189.0), (0.2, 142.08, 209.28)] {
        let mut s = crossing_blocks();
        run(&mut s, "solid.box", &json!({"corner":[48,-2,15],"length":4,"width":6,"height":15,"operation":"cut","targets":["B"]}));
        let before_a = volume(&s, "A");
        let before_b = volume(&s, "B");
        let feature = run(&mut s, "solid.finger_joint", &json!({"bodies":["A","B"],"count":2,"clearance":clearance}));
        let state = s.model.state();
        assert!(s.model.result(feature["feature"].as_u64().unwrap()).unwrap().warning.is_none());
        for (name, before, expected) in [("A", before_a, expected_a), ("B", before_b, expected_b)] {
            let removed = before - volume(&s, name);
            assert!((removed - expected).abs() < 0.005, "{name} clearance {clearance}: {removed} vs {expected}");
        }
        for (height, a, b) in [(1.5, Inside, Outside), (4.5, Outside, Inside), (36.0, Outside, Inside), (48.0, Inside, Outside)] {
            let point = Vec3::new(50.0, 1.5, height);
            assert_eq!(state.body("A").unwrap().body.classify_point(point).unwrap(), a, "A at {height}");
            assert_eq!(state.body("B").unwrap().body.classify_point(point).unwrap(), b, "B at {height}");
        }
        assert_eq!(state.body("A").unwrap().body.classify_point(Vec3::new(50.0, 1.5, 25.0)).unwrap(), Inside);
        no_overlap_after(&s);
    }
}

#[test]
fn finger_joint_handles_stepped_overlap_without_cutting_the_gap() {
    use solidcore_record::Vec3;
    use solidcraft_kernel::PointClass::{Inside, Outside};

    for (bodies, inverted, a_retains_ends) in
        [(["A", "B"], false, true), (["A", "B"], true, false), (["B", "A"], false, false), (["B", "A"], true, true)]
    {
        for clearance in [0.0, 0.2] {
            let mut s = crossing_blocks();
            run(&mut s, "solid.box", &json!({"corner":[48,-2,20],"length":4,"width":3.5,"height":10,"operation":"cut","targets":["B"]}));
            let before = (volume(&s, "A"), volume(&s, "B"));
            run(&mut s, "solid.finger_joint", &json!({"bodies":bodies,"count":2,"clearance":clearance,"inverted":inverted}));
            let state = s.model.state();
            let a = &state.body("A").unwrap().body;
            let b = &state.body("B").unwrap().body;
            if clearance == 0.0 {
                // The notch removes 1.5 mm of the overlap across 10 mm of one slot. When A
                // owns that slot, clipping therefore removes 3 mm × 1.5 mm × 10 mm less from A.
                let expected = if a_retains_ends { (180.0, 225.0) } else { (225.0, 180.0) };
                assert!((before.0 - volume(&s, "A") - expected.0).abs() < 0.005, "{bodies:?} inverted={inverted}");
                assert!((before.1 - volume(&s, "B") - expected.1).abs() < 0.005, "{bodies:?} inverted={inverted}");
            }
            let outside_footprint = if clearance == 0.0 || a_retains_ends { Inside } else { Outside };
            assert_eq!(a.classify_point(Vec3::new(50.0, 0.75, 25.0)).unwrap(), outside_footprint);
            assert_eq!(b.classify_point(Vec3::new(50.0, 0.75, 25.0)).unwrap(), Outside);
            let overlap_point = Vec3::new(50.0, 2.25, 25.0);
            assert_eq!(a.classify_point(overlap_point).unwrap(), if a_retains_ends { Inside } else { Outside });
            assert_eq!(b.classify_point(overlap_point).unwrap(), if a_retains_ends { Outside } else { Inside });
            no_overlap_after(&s);
        }
    }
}

#[test]
fn finger_joint_zero_clearance_does_not_cut_outside_a_stepped_overlap_footprint() {
    use solidcore_record::Vec3;
    use solidcraft_kernel::PointClass::{Inside, Outside};

    let mut s = crossing_blocks();
    run(&mut s, "solid.box", &json!({"corner":[48,-2,10],"length":4,"width":3.5,"height":10,"operation":"cut","targets":["B"]}));

    run(&mut s, "solid.finger_joint", &json!({"bodies":["A","B"],"count":2,"clearance":0}));

    let state = s.model.state();
    let a = &state.body("A").unwrap().body;
    assert_eq!(a.classify_point(Vec3::new(50.0, 0.75, 15.0)).unwrap(), Inside);
    assert_eq!(a.classify_point(Vec3::new(50.0, 2.25, 15.0)).unwrap(), Outside);
    assert_eq!(a.classify_point(Vec3::new(50.0, 0.75, 35.0)).unwrap(), Outside);
    no_overlap_after(&s);
}

#[test]
fn finger_joint_accepts_sloped_overlap_with_rectangular_cuts() {
    for (bodies, inverted) in [(["A", "B"], false), (["A", "B"], true), (["B", "A"], false), (["B", "A"], true)] {
        for clearance in [0.0, 0.2] {
            let mut s = Session::default();
            run(&mut s, "solid.box", &json!({"length":100,"width":3,"height":50,"body_name":"A","operation":"new"}));
            run(&mut s, "solid.box", &json!({"center":[0,0,25],"length":3,"width":100,"height":50,"body_name":"B","operation":"new"}));
            run(&mut s, "solid.move", &json!({"bodies":["B"],"axis":[0,1,0],"angle":"20 deg"}));
            run(&mut s, "solid.move", &json!({"bodies":["B"],"translate":[50,1.5,0]}));
            let before = (volume(&s, "A"), volume(&s, "B"));
            run(&mut s, "solid.finger_joint", &json!({"bodies":bodies,"count":3,"clearance":clearance,"inverted":inverted}));
            assert!(volume(&s, "A") < before.0);
            assert!(volume(&s, "B") < before.1);
            no_overlap_after(&s);
        }
    }
}

#[test]
fn finger_joint_sloped_footprint_can_be_cut_without_its_bounding_box() {
    use solidcore_record::{Loop2, Plane, Region2, Vec2, Vec3};
    use solidcraft_kernel::PointClass::{Inside, Outside};

    let plate = solidcraft_kernel::box_solid(Vec3::ZERO, Vec3::new(100.0, 3.0, 50.0)).unwrap();
    let plane = Plane { origin: Vec3::ZERO, x: Vec3::X, y: Vec3::Z };
    let outline = Loop2::polygon(&[Vec2::new(45.0, 10.0), Vec2::new(55.0, 10.0), Vec2::new(55.0, 30.0), Vec2::new(50.0, 30.0)]);
    let footprint = Region2 { outer: outline, holes: vec![] };
    let tool = solidcraft_kernel::extrude_planar_regions(&plane, &[footprint], -4.0, 1.0).unwrap();
    let cut = solidcraft_kernel::boolean(&plate, &tool, solidcraft_kernel::BoolOp::Cut).unwrap().unwrap();

    let removed = solidcraft_kernel::measure(&plate).unwrap().volume - solidcraft_kernel::measure(&cut).unwrap().volume;
    assert!((removed - 450.0).abs() < 0.005, "{removed}");
    assert_eq!(cut.classify_point(Vec3::new(46.0, 1.5, 29.0)).unwrap(), Inside);
    assert_eq!(cut.classify_point(Vec3::new(53.0, 1.5, 29.0)).unwrap(), Outside);
}

#[test]
fn finger_joint_stepped_overlap_keeps_pattern_continuous_across_a_step() {
    let mut s = crossing_blocks();
    run(&mut s, "solid.box", &json!({"corner":[48,-2,20],"length":4,"width":3.5,"height":10,"operation":"cut","targets":["B"]}));
    let before = (volume(&s, "A"), volume(&s, "B"));

    run(&mut s, "solid.finger_joint", &json!({"bodies":["A","B"],"count":3}));

    let removed_a = before.0 - volume(&s, "A");
    let removed_b = before.1 - volume(&s, "B");
    // The notch removes 1.5 mm of overlap across 50/7 mm of A's slot.
    let expected_a = 9.0 * 150.0 / 7.0 - 4.5 * 50.0 / 7.0;
    let expected_b = 9.0 * 200.0 / 7.0 - 4.5 * 20.0 / 7.0;
    assert!((removed_a - expected_a).abs() < 0.005, "{removed_a} vs {expected_a}");
    assert!((removed_b - expected_b).abs() < 0.005, "{removed_b} vs {expected_b}");
    no_overlap_after(&s);
}

#[test]
fn finger_joint_redefine_keeps_identity_and_undo_restores_the_original() {
    let mut s = crossing_blocks();
    let feature =
        run(&mut s, "solid.finger_joint", &json!({"bodies":["A","B"],"count":3,"clearance":"0.1 mm","name":"Joint"}))["feature"].as_u64().unwrap();
    run(
        &mut s,
        "timeline.redefine",
        &json!({"feature":feature,"command":"solid.finger_joint",
            "params":{"bodies":["A","B"],"count":2,"clearance":"0.2 mm","inverted":true,"edge_offset":"1 mm"}}),
    );
    let edited = s.doc.feature(feature).unwrap();
    assert_eq!(edited.name, "Joint");
    assert!(matches!(&edited.kind, solidcraft_doc::FeatureKind::FingerJoint { count: 2, inverted: true, edge_offset: Some(_), .. }));
    no_overlap_after(&s);
    run(&mut s, "edit.undo", &json!({}));
    assert!(matches!(
        &s.doc.feature(feature).unwrap().kind,
        solidcraft_doc::FeatureKind::FingerJoint { count: 3, inverted: false, edge_offset: None, .. }
    ));
}

fn three_plate_joint(short_last: bool) -> Session {
    let mut s = Session::default();
    run(&mut s, "solid.box", &json!({"length":100,"width":3,"height":50,"body_name":"A","operation":"new"}));
    for (name, x, height) in [("B", 20.0, if short_last { 50.0 } else { 7.0 }), ("C", 70.0, if short_last { 7.0 } else { 50.0 })] {
        run(&mut s, "solid.box", &json!({"corner":[x,-48.5,0],"length":3,"width":100,"height":height,"body_name":name,"operation":"new"}));
    }
    s
}

#[test]
fn finger_joint_multiple_bodies_follow_the_same_order_as_separate_pairs() {
    let mut combined = three_plate_joint(false);
    let mut sequential = three_plate_joint(false);
    run(&mut combined, "solid.finger_joint", &json!({"bodies":["A","B","C"],"count":2,"clearance":"0.1 mm"}));
    for pair in [["A", "B"], ["A", "C"]] {
        run(&mut sequential, "solid.finger_joint", &json!({"bodies":pair,"count":2,"clearance":"0.1 mm"}));
    }
    for name in ["A", "B", "C"] {
        let actual = volume(&combined, name);
        let expected = volume(&sequential, name);
        assert!((actual - expected).abs() < expected * 1e-6, "{name}: {actual} vs {expected}");
    }
    assert_eq!(combined.doc.features.len(), 4);
    assert!(combined.model.result(combined.doc.features.last().unwrap().id).unwrap().warning.is_none());
}

fn three_axis_joint() -> Session {
    let mut s = Session::default();
    run(&mut s, "solid.box", &json!({"corner":[-50,-50,-1.5],"length":100,"width":100,"height":3,"body_name":"A","operation":"new"}));
    run(&mut s, "solid.box", &json!({"corner":[-1.5,-50,-50],"length":3,"width":100,"height":100,"body_name":"B","operation":"new"}));
    run(&mut s, "solid.box", &json!({"corner":[-50,-1.5,-50],"length":100,"width":3,"height":100,"body_name":"C","operation":"new"}));
    s
}

#[test]
fn finger_joint_multiple_bodies_only_joins_each_later_body_to_the_anchor() {
    let mut combined = three_axis_joint();
    let mut sequential = three_axis_joint();
    let state = combined.model.state();
    assert!(
        solidcraft_kernel::boolean(&state.body("B").unwrap().body, &state.body("C").unwrap().body, solidcraft_kernel::BoolOp::Intersect)
            .unwrap()
            .is_some()
    );

    run(&mut combined, "solid.finger_joint", &json!({"bodies":["A","B","C"],"count":2}));
    for pair in [["A", "B"], ["A", "C"]] {
        run(&mut sequential, "solid.finger_joint", &json!({"bodies":pair,"count":2}));
    }
    for name in ["A", "B", "C"] {
        let actual = volume(&combined, name);
        let expected = volume(&sequential, name);
        assert!((actual - expected).abs() < expected * 1e-6, "{name}: {actual} vs {expected}");
    }
}

#[test]
fn finger_joint_multiple_bodies_restore_failed_pairs_and_continue() {
    for short_last in [false, true] {
        let mut s = three_plate_joint(short_last);
        let short = if short_last { "C" } else { "B" };
        let long = if short_last { "B" } else { "C" };
        let before_short = volume(&s, short);
        let before_long = volume(&s, long);
        let before_a = volume(&s, "A");
        let result = run(&mut s, "solid.finger_joint", &json!({"bodies":["A","B","C"],"count":3,"clearance":"1 mm"}));
        let feature = result["feature"].as_u64().unwrap();
        let warning = s.model.result(feature).unwrap().warning.as_deref().unwrap();
        assert!(warning.contains(short) && warning.contains("clearance"), "{warning}");
        assert!((volume(&s, short) - before_short).abs() < 1e-6);
        assert!(volume(&s, long) < before_long);
        assert!(volume(&s, "A") < before_a);
        assert!(
            solidcraft_kernel::boolean(
                &s.model.state().body("A").unwrap().body,
                &s.model.state().body(long).unwrap().body,
                solidcraft_kernel::BoolOp::Intersect,
            )
            .unwrap()
            .is_none()
        );
        run(
            &mut s,
            "timeline.redefine",
            &json!({"feature":feature,"command":"solid.finger_joint","params":{"bodies":["A","B","C"],"count":3,"clearance":"0.1 mm"}}),
        );
        assert!(s.model.result(feature).unwrap().warning.is_none());
        assert!(volume(&s, short) < before_short);
        run(&mut s, "edit.undo", &json!({}));
        assert!((volume(&s, short) - before_short).abs() < 1e-6);
        run(&mut s, "edit.undo", &json!({}));
        assert!((volume(&s, "A") - before_a).abs() < 1e-6);
        assert!((volume(&s, long) - before_long).abs() < 1e-6);
    }
}

#[test]
fn finger_joint_multiple_bodies_reject_duplicates_and_excessive_selection() {
    let mut s = crossing_blocks();
    assert!(s.execute("solid.finger_joint", &json!({"bodies":["A","B","A"]})).is_err());
    let too_many = (0..=solidcraft_doc::MAX_FINGER_JOINT_BODIES).map(|i| format!("Body{i}")).collect::<Vec<_>>();
    assert!(s.execute("solid.finger_joint", &json!({"bodies":too_many})).is_err());
    assert_eq!(s.doc.features.len(), 2);
}

#[test]
fn finger_joint_accepts_a_plate_that_already_has_a_joint() {
    let mut s = crossing_blocks();
    run(&mut s, "solid.box", &json!({"corner":[10,-48.5,0],"length":3,"width":100,"height":50,"body_name":"C","operation":"new"}));
    run(&mut s, "solid.finger_joint", &json!({"bodies":["A","C"],"count":2}));
    run(&mut s, "solid.finger_joint", &json!({"bodies":["A","B"],"count":3}));
    no_overlap_after(&s);
}

#[test]
fn finger_joint_after_angled_joint_uses_exact_plate_normals() {
    use solidcore_record::Vec3;
    use solidcraft_kernel::PointClass::Inside;

    let mut s = Session::default();
    run(&mut s, "solid.box", &json!({"corner":[-250,0,0],"length":500,"width":9.5,"height":254,"body_name":"A","operation":"new"}));
    run(&mut s, "solid.box", &json!({"center":[0,0,127],"length":14,"width":254,"height":254,"body_name":"B","operation":"new"}));
    run(&mut s, "solid.move", &json!({"bodies":["B"],"axis":[0,0,1],"angle":"35 deg"}));
    run(&mut s, "solid.move", &json!({"bodies":["B"],"translate":[150,4.75,0]}));
    run(&mut s, "solid.box", &json!({"corner":[-52,-50,0],"length":14,"width":254,"height":254,"body_name":"C","operation":"new"}));
    run(&mut s, "solid.finger_joint", &json!({"bodies":["A","B"],"count":3,"inverted":true}));
    let before_a = volume(&s, "A");
    let before_c = volume(&s, "C");
    let feature = run(&mut s, "solid.finger_joint", &json!({"bodies":["A","C"],"count":3}));
    assert!(s.model.result(feature["feature"].as_u64().unwrap()).unwrap().warning.is_none());
    let width = 254.0 / 7.0;
    for (name, removed, expected) in
        [("A", before_a - volume(&s, "A"), 9.5 * 14.0 * width * 3.0), ("C", before_c - volume(&s, "C"), 9.5 * 14.0 * width * 4.0)]
    {
        assert!((removed - expected).abs() < 0.05, "{name}: {removed} vs {expected}");
    }
    let state = s.model.state();
    let a = &state.body("A").unwrap().body;
    let b = &state.body("B").unwrap().body;
    let c = &state.body("C").unwrap().body;
    assert!(solidcraft_kernel::boolean(a, b, solidcraft_kernel::BoolOp::Intersect).unwrap().is_none());
    assert!(solidcraft_kernel::boolean(a, c, solidcraft_kernel::BoolOp::Intersect).unwrap().is_none());
    assert_eq!(a.classify_point(Vec3::new(-53.0, 4.75, 1.5 * width)).unwrap(), Inside);
    assert_eq!(c.classify_point(Vec3::new(-45.0, 10.5, 0.5 * width)).unwrap(), Inside);
}

fn volume(s: &Session, name: &str) -> f64 {
    solidcraft_kernel::measure(&s.model.state().body(name).unwrap().body).unwrap().volume
}

#[test]
fn finger_joint_leaves_both_bodies_intact_when_it_cannot_apply() {
    let cases = [
        (json!({"length":100,"width":3,"height":50}), json!({"corner":[0,20,0],"length":100,"width":3,"height":50})),
        (json!({"length":100,"width":3,"height":50}), json!({"corner":[200,-48.5,0],"length":3,"width":100,"height":50})),
    ];
    for (a, b) in cases {
        let mut s = joined(a, b);
        let (before_a, before_b) = (volume(&s, "A"), volume(&s, "B"));
        run(&mut s, "solid.finger_joint", &json!({"bodies":["A","B"],"count":3}));
        assert_eq!((volume(&s, "A"), volume(&s, "B")), (before_a, before_b));
    }
}
