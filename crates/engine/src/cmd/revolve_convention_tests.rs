#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use crate::Session;
use serde_json::{Value, json};
use solidcraft_doc::{Document, FeatureKind, RevolveConvention};
use solidcraft_geom::Vec3;
use std::f64::consts::{FRAC_PI_2, PI, SQRT_2};

fn run(s: &mut Session, cmd: &str, p: Value) -> Value {
    s.execute(cmd, &p).unwrap_or_else(|e| panic!("{cmd}: {e}"))
}

fn profile(s: &mut Session, opposed: bool) {
    run(s, "sketch.create", json!({"name":"Profile","plane":"XY"}));
    run(s, "sketch.rectangle.two_point", json!({"p0":[2,0],"p1":[4,10]}));
    if opposed {
        run(s, "sketch.rectangle.two_point", json!({"p0":[-4,0],"p1":[-2,10]}));
    }
    run(s, "sketch.finish", json!({}));
}

fn turn(s: &mut Session, sign: f64, settings: Value) {
    let mut p = json!({"sketch":"Profile","axis":{"origin":[0,0,0],"dir":[0,sign,0]},"angle":"90 deg","direction":"symmetric","operation":"new","name":"Turn","body_name":"Turned"});
    p.as_object_mut().unwrap().extend(settings.as_object().unwrap().clone());
    run(s, "solid.revolve", p);
}

fn check_centered(s: &Session, opposed: bool) {
    let state = s.model.state();
    let expected = 30.0 * PI * if opposed { 2.0 } else { 1.0 };
    let volume: f64 = state.bodies.iter().map(|mb| solidcraft_kernel::measure(&mb.body).unwrap().volume).sum();
    assert!((volume - expected).abs() < 0.03, "{volume} vs {expected}");
    for sign in if opposed { vec![1.0, -1.0] } else { vec![1.0] } {
        let body = &state.bodies.iter().find(|mb| mb.body.contains(Vec3::new(sign * 3.0, 5.0, 0.0))).unwrap().body;
        assert!(!body.contains(Vec3::new(0.0, 5.0, 3.0)));
        assert!(body.contains(Vec3::new(sign * 3.0 * (PI / 6.0).cos(), 5.0, 1.5)));
        assert!(body.contains(Vec3::new(sign * 3.0 * (PI / 6.0).cos(), 5.0, -1.5)));
        let bounds = body.mesh(&solidcraft_kernel::MeshTolerance::chord(1e-4)).unwrap().bounds();
        assert!((bounds.min.z + 2.0 * SQRT_2).abs() < 1e-6, "{bounds:?}");
        assert!((bounds.max.z - 2.0 * SQRT_2).abs() < 1e-6, "{bounds:?}");
        let inner_x = if sign > 0.0 { bounds.min.x } else { -bounds.max.x };
        assert!((inner_x - SQRT_2).abs() < 1e-6, "{bounds:?}");
        let centroid = solidcraft_kernel::measure(body).unwrap().centroid;
        let radial = (2.0 / 3.0) * (64.0 - 8.0) / (16.0 - 4.0);
        let want_x = radial * (PI / 4.0).sin() / (PI / 4.0);
        assert!((centroid.x - sign * want_x).abs() < 0.005, "{centroid:?}");
        assert!((centroid.y - 5.0).abs() < 1e-6);
        assert!(centroid.z.abs() < 1e-6);
    }
}

#[test]
fn new_symmetric_revolves_center_material_for_both_axis_signs_and_negative_angles() {
    for sign in [-1.0, 1.0] {
        for angle in ["90 deg", "-90 deg"] {
            let mut s = Session::default();
            profile(&mut s, false);
            turn(&mut s, sign, json!({"angle":angle}));
            assert!(matches!(s.doc.find_feature("Turn").unwrap().kind, FeatureKind::Revolve { convention: RevolveConvention::SketchNormal, .. }));
            check_centered(&s, false);
        }
    }
}

#[test]
fn opposed_profiles_use_independent_sweep_axes() {
    for sign in [-1.0, 1.0] {
        let mut s = Session::default();
        profile(&mut s, true);
        turn(&mut s, sign, json!({}));
        check_centered(&s, true);
    }
}

#[test]
fn two_sided_revolves_use_the_canonical_negative_and_positive_limits() {
    for sign in [-1.0, 1.0] {
        let mut s = Session::default();
        profile(&mut s, false);
        turn(&mut s, sign, json!({"direction":"positive","angle":"60 deg","angle2":"30 deg"}));
        let state = s.model.state();
        let body = &state.bodies[0].body;
        let bounds = body.mesh(&solidcraft_kernel::MeshTolerance::chord(1e-4)).unwrap().bounds();
        assert!((bounds.min.x - 1.0).abs() < 1e-6, "{bounds:?}");
        assert!((bounds.max.x - 4.0).abs() < 1e-4);
        assert!((bounds.min.z + 2.0).abs() < 1e-6);
        assert!((bounds.max.z - 2.0 * 3.0f64.sqrt()).abs() < 1e-6);
        assert!(body.contains(Vec3::new(3.0, 5.0, 0.0)));
        assert!(!body.contains(Vec3::new(0.0, 5.0, 3.0)));
        assert!((solidcraft_kernel::measure(body).unwrap().volume - 30.0 * PI).abs() < 0.03);
    }
}

#[test]
fn legacy_missing_convention_keeps_geometry_through_edit_redefine_undo_and_reopen() {
    let mut s = Session::default();
    profile(&mut s, false);
    turn(&mut s, -1.0, json!({"convention":"legacy_axis_start"}));
    let mut saved: Value = serde_json::from_str(&s.doc.to_json()).unwrap();
    saved["format"] = json!("solidcraft/10");
    for f in saved["features"].as_array_mut().unwrap() {
        f.as_object_mut().unwrap().remove("convention");
    }
    let mut s = Session::new(Document::from_json(&saved.to_string()).unwrap());
    let original = s.doc.find_feature("Turn").unwrap().kind.clone();
    let check = |s: &Session| {
        assert_eq!(s.doc.find_feature("Turn").unwrap().kind, original);
        let state = s.model.state();
        let body = &state.bodies[0].body;
        assert!(body.contains(Vec3::new(0.0, 5.0, 3.0)));
        assert!(!body.contains(Vec3::new(3.0, 5.0, 0.0)));
        let bounds = body.mesh(&solidcraft_kernel::MeshTolerance::chord(1e-4)).unwrap().bounds();
        assert!((bounds.min.x + 2.0 * SQRT_2).abs() < 1e-6);
        assert!((bounds.max.x - 2.0 * SQRT_2).abs() < 1e-6);
        assert!((bounds.min.z - SQRT_2).abs() < 1e-6);
    };
    check(&s);
    let angle_input = s.doc.find_feature("Turn").unwrap().param_names[0].clone();
    run(&mut s, "parameters.change", json!({"name":angle_input,"expression":"60 deg"}));
    let changed = s.model.state();
    assert!(changed.bodies[0].body.contains(Vec3::new(1.5, 5.0, 3.0 * 3.0f64.sqrt() / 2.0)));
    assert!(!changed.bodies[0].body.contains(Vec3::new(3.0, 5.0, 0.0)));
    run(&mut s, "edit.undo", json!({}));
    check(&s);
    run(&mut s, "timeline.edit", json!({"feature":"Turn","set":{"angle":"90 deg"}}));
    check(&s);
    run(
        &mut s,
        "timeline.redefine",
        json!({"feature":"Turn","command":"solid.revolve","params":{"sketch":"Profile","axis":{"origin":[0,0,0],"dir":[0,-1,0]},"angle":"90 deg","direction":"symmetric","body_name":"Turned"}}),
    );
    check(&s);
    run(&mut s, "timeline.edit", json!({"feature":"Turn","set":{"convention":"sketch_normal"}}));
    check_centered(&s, false);
    run(&mut s, "edit.undo", json!({}));
    check(&s);
    let reopened = Session::new(Document::from_json(&s.doc.to_json()).unwrap());
    check(&reopened);
    // A newly authored feature in this migrated document uses current semantics.
    turn(&mut s, -1.0, json!({"name":"NewTurn","body_name":"NewTurned"}));
    assert!(matches!(s.doc.find_feature("NewTurn").unwrap().kind, FeatureKind::Revolve { convention: RevolveConvention::SketchNormal, .. }));
}

#[test]
fn extent_handle_angles_match_new_and_legacy_material() {
    let mut s = Session::default();
    profile(&mut s, false);
    let state = s.model.state();
    let ss = &state.sketches[0];
    let regions = vec![ss.profiles[0].region.clone()];
    for convention in [RevolveConvention::SketchNormal, RevolveConvention::LegacyAxisStart] {
        let extent = solidcraft_doc::revolve_extent_with_convention(
            &state,
            &ss.plane,
            &regions,
            (Vec3::ZERO, -Vec3::Y),
            (FRAC_PI_2, None),
            true,
            None,
            convention,
        )
        .unwrap();
        assert_eq!(extent.rotation_axis, -Vec3::Y);
        assert!((extent.display_start - if convention == RevolveConvention::SketchNormal { -PI / 4.0 } else { PI / 4.0 }).abs() < 1e-12);
        assert!((extent.sweep - FRAC_PI_2).abs() < 1e-12);
    }
}

#[test]
fn symmetric_revolve_preserves_a_translated_rotated_sketch_frame() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"name":"Profile","plane":{"origin":[10,20,30],"x_dir":[0,0,1],"y_dir":[1,0,0]}}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0":[2,0],"p1":[4,10]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.revolve", json!({"sketch":"Profile","axis":{"origin":[10,20,30],"dir":[-1,0,0]},"angle":"90 deg","direction":"symmetric"}));
    let state = s.model.state();
    let body = &state.bodies[0].body;
    let bounds = body.mesh(&solidcraft_kernel::MeshTolerance::chord(1e-4)).unwrap().bounds();
    assert!(bounds.min.dist(Vec3::new(10.0, 20.0 - 2.0 * SQRT_2, 30.0 + SQRT_2)) < 1e-6, "{bounds:?}");
    assert!(bounds.max.dist(Vec3::new(20.0, 20.0 + 2.0 * SQRT_2, 34.0)) < 1e-4, "{bounds:?}");
    assert!(body.contains(Vec3::new(15.0, 20.0, 33.0)));
    assert!(!body.contains(Vec3::new(15.0, 23.0, 30.0)));
    assert!((solidcraft_kernel::measure(body).unwrap().volume - 30.0 * PI).abs() < 0.03);
}

#[test]
fn copied_revolves_keep_their_convention_and_angular_material() {
    for convention in ["sketch_normal", "legacy_axis_start"] {
        let mut s = Session::default();
        profile(&mut s, false);
        turn(&mut s, -1.0, json!({"convention":convention}));
        run(&mut s, "timeline.copy", json!({"features":["Profile","Turn"]}));
        run(&mut s, "timeline.paste", json!({"translate":[0,20,0]}));
        let state = s.model.state();
        let probe = if convention == "sketch_normal" { Vec3::new(3.0, 25.0, 0.0) } else { Vec3::new(0.0, 25.0, 3.0) };
        assert!(state.bodies.iter().any(|mb| mb.body.contains(probe)));
        let modes: Vec<_> = s
            .doc
            .features
            .iter()
            .filter_map(|f| if let FeatureKind::Revolve { convention, .. } = f.kind { Some(convention) } else { None })
            .collect();
        assert_eq!(modes.len(), 2);
        assert_eq!(modes[0], modes[1]);
        let reopened = Session::new(Document::from_json(&s.doc.to_json()).unwrap());
        assert!(reopened.model.state().bodies.iter().any(|mb| mb.body.contains(probe)));
    }
}

#[test]
fn centered_coplanar_cut_uses_the_actual_angular_interval() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"corner":[2,0,-4],"length":2,"width":10,"height":8,"body_name":"Stock"}));
    profile(&mut s, false);
    turn(&mut s, -1.0, json!({"operation":"cut","targets":["Stock"]}));
    let state = s.model.state();
    // Removed sector clipped by x>=2: 10∫[-π/4,π/4](16−4sec²θ)/2 dθ.
    // The sector crosses the whole stock at z=0, leaving two disconnected solids.
    let expected = 200.0 - 40.0 * PI;
    let volumes: Vec<_> = state.bodies.iter().map(|mb| solidcraft_kernel::measure(&mb.body).unwrap().volume).collect();
    let total: f64 = volumes.iter().sum();
    assert!((total - expected).abs() < 0.05, "{volumes:?}: total{total}, expected{expected}");
    assert_eq!(state.bodies.len(), 2);
    assert!(volumes.iter().all(|v| (*v - expected / 2.0).abs() < 0.025));
    let contains = |p| state.bodies.iter().any(|mb| mb.body.contains(p));
    assert!(!contains(Vec3::new(3.0, 5.0, 0.0)));
    assert!(contains(Vec3::new(3.0, 5.0, 3.5)));
    assert!(contains(Vec3::new(3.0, 5.0, -3.5)));
}

#[test]
fn one_sided_and_full_revolves_keep_the_existing_material() {
    for angle in ["90 deg", "360 deg"] {
        for sign in [-1.0, 1.0] {
            for convention in ["legacy_axis_start", "sketch_normal"] {
                let mut s = Session::default();
                profile(&mut s, false);
                turn(&mut s, sign, json!({"angle":angle,"direction":"positive","convention":convention}));
                let state = s.model.state();
                let body = &state.bodies[0].body;
                assert!(body.contains(Vec3::new(3.0 / SQRT_2, 5.0, 3.0 / SQRT_2)));
                assert_eq!(body.contains(Vec3::new(-3.0 / SQRT_2, 5.0, -3.0 / SQRT_2)), angle == "360 deg");
                let expected = if angle == "360 deg" { 120.0 * PI } else { 30.0 * PI };
                assert!((solidcraft_kernel::measure(body).unwrap().volume - expected).abs() < 0.08);
            }
        }
    }
}

#[test]
fn point_targets_use_each_profiles_canonical_axis_for_both_axis_signs() {
    for sign in [-1.0, 1.0] {
        for opposed in [false, true] {
            let mut s = Session::default();
            profile(&mut s, opposed);
            turn(&mut s, sign, json!({"to":[0,5,3]}));
            let state = s.model.state();
            let expected = 30.0 * PI * if opposed { 2.0 } else { 1.0 };
            let volume: f64 = state.bodies.iter().map(|mb| solidcraft_kernel::measure(&mb.body).unwrap().volume).sum();
            assert!((volume - expected).abs() < 0.05, "{volume}");
            assert!(state.bodies.iter().any(|mb| mb.body.contains(Vec3::new(3.0 / SQRT_2, 5.0, 3.0 / SQRT_2))));
            assert!(!state.bodies.iter().any(|mb| mb.body.contains(Vec3::new(3.0 / SQRT_2, 5.0, -3.0 / SQRT_2))));
            assert_eq!(state.bodies.iter().any(|mb| mb.body.contains(Vec3::new(-3.0 / SQRT_2, 5.0, 3.0 / SQRT_2))), opposed);
            assert!(!state.bodies.iter().any(|mb| mb.body.contains(Vec3::new(-3.0 / SQRT_2, 5.0, -3.0 / SQRT_2))));
        }
    }
}

#[test]
fn an_axis_target_is_an_actionable_atomic_error_for_new_convention() {
    let mut s = Session::default();
    profile(&mut s, false);
    let before = s.doc.to_json();
    let e = s.execute("solid.revolve", &json!({"sketch":"Profile","axis":"Y","to":[0,5,0]})).unwrap_err().to_string();
    assert!(e.contains("target lies on the axis"), "{e}");
    assert_eq!(s.doc.to_json(), before);
}

#[test]
fn curved_targets_keep_only_the_material_before_the_post_for_both_axis_signs() {
    for sign in [-1.0, 1.0] {
        for opposed in [false, true] {
            let mut s = Session::default();
            run(&mut s, "solid.cylinder", json!({"base":[0,14,-5],"radius":4,"height":15,"body_name":"Post"}));
            run(&mut s, "sketch.create", json!({"name":"Profile","plane":"XZ"}));
            run(&mut s, "sketch.rectangle.two_point", json!({"p0":[10,0],"p1":[15,5]}));
            if opposed {
                run(&mut s, "sketch.rectangle.two_point", json!({"p0":[-15,0],"p1":[-10,5]}));
            }
            run(&mut s, "sketch.finish", json!({}));
            run(&mut s, "solid.revolve", json!({"sketch":"Profile","axis":{"origin":[0,0,0],"dir":[0,0,sign]},"to":[4,14,2],"body_name":"Arc"}));
            let state = s.model.state();
            let post = &state.body("Post").unwrap().body;
            let arcs: Vec<_> = state.bodies.iter().filter(|mb| mb.name != "Post").collect();
            assert_eq!(arcs.len(), if opposed { 2 } else { 1 });
            for side in if opposed { vec![1.0, -1.0] } else { vec![1.0] } {
                let probe = Vec3::new(side * 12.0 * (PI / 6.0).cos(), 6.0, -2.5);
                let arc = &arcs.iter().find(|mb| mb.body.contains(probe)).unwrap().body;
                assert!(!arc.contains(Vec3::new(-side * 12.0 * (PI / 6.0).cos(), 6.0, -2.5)));
                assert!(!arc.contains(Vec3::new(side * 12.0 * (PI / 6.0).cos(), -6.0, -2.5)));
                let full = PI * (225.0 - 100.0) * 5.0;
                let angle = solidcraft_kernel::measure(arc).unwrap().volume / full * 360.0;
                assert!(angle > 60.0 && angle < 90.0, "{angle}");
                let overlap = solidcraft_kernel::boolean(arc, post, solidcraft_kernel::BoolOp::Intersect).unwrap();
                assert!(overlap.is_none_or(|b| solidcraft_kernel::measure(&b).unwrap().volume < 1e-3));
            }
        }
    }
}

/// The construction-axis points of a feature's JSON moved along x by `dx`.
fn shift_points(v: &mut Value, dx: f64) {
    match v {
        Value::Array(a) if a.len() == 3 && a.iter().all(Value::is_number) => a[0] = json!(a[0].as_f64().unwrap() + dx),
        Value::Array(a) => a.iter_mut().for_each(|x| shift_points(x, dx)),
        Value::Object(o) => o.values_mut().for_each(|x| shift_points(x, dx)),
        _ => {}
    }
}

/// A construction axis picked as the revolve axis (#2885, #4506) is saved by name, survives
/// saving and opening, and the revolve follows the axis when it is edited.
#[test]
fn construction_axis_revolve_follows_the_axis() {
    let mut s = Session::default();
    run(&mut s, "construct.axis.two_points", json!({"a":{"point":[0,0,0]},"b":{"point":[0,30,0]},"name":"Axis1"}));
    profile(&mut s, false);
    run(&mut s, "solid.revolve", json!({"sketch":"Profile","axis":"Axis1","angle":"180 deg","operation":"new","profiles":[0],"name":"Turn"}));
    let turn = s.doc.find_feature("Turn").unwrap();
    assert!(matches!(&turn.kind, FeatureKind::Revolve { axis: solidcraft_doc::AxisRef::Construction { name }, .. } if name == "Axis1"));
    let reopened = Document::from_json(&s.doc.to_json()).unwrap();
    assert_eq!(reopened.find_feature("Turn").unwrap().kind, turn.kind);
    let volume = |s: &Session| solidcraft_kernel::measure(&s.model.state().bodies[0].body).unwrap().volume;
    assert!((volume(&s) - 60.0 * PI).abs() < 0.05);
    assert!((volume(&Session::new(reopened)) - 60.0 * PI).abs() < 0.05);
    // Moving the axis 1 mm away from the profile widens the ring: radii 3 to 5 instead of 2 to 4.
    let mut axis = serde_json::to_value(s.doc.find_feature("Axis1").unwrap()).unwrap();
    let def = if axis.get("def").is_some() { axis.get_mut("def") } else { axis.pointer_mut("/kind/def") }.unwrap();
    shift_points(def, -1.0);
    let set = if axis.get("def").is_some() { json!({"def": axis["def"]}) } else { json!({"kind": {"def": axis["kind"]["def"]}}) };
    run(&mut s, "timeline.edit", json!({"feature":"Axis1","set":set}));
    assert!((volume(&s) - 80.0 * PI).abs() < 0.05, "{}", volume(&s));
}

#[test]
fn construction_axis_revolve_rejects_invalid_axes_atomically() {
    for (a, b, message) in [
        ([0, 0, 0], [0, 0, 30], "does not lie in the sketch plane"),
        ([0, 0, 1], [0, 30, 1], "does not lie in the sketch plane"),
        ([3, 0, 0], [3, 30, 0], "profile crosses the revolve axis"),
    ] {
        let mut s = Session::default();
        run(&mut s, "construct.axis.two_points", json!({"a":{"point":a},"b":{"point":b},"name":"Axis1"}));
        profile(&mut s, false);
        let before = s.doc.to_json();
        let error =
            s.execute("solid.revolve", &json!({"sketch":"Profile","axis":"Axis1","angle":"180 deg","operation":"new"})).unwrap_err().to_string();
        assert!(error.contains(message), "{error}");
        assert_eq!(s.doc.to_json(), before);
        assert!(s.model.state().bodies.is_empty());
    }
}
