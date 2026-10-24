#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use crate::Session;
use serde_json::{Value, json};
use solidcraft_doc::{Document, ExtrudeConvention, FeatureKind};

fn run(s: &mut Session, cmd: &str, params: Value) {
    s.execute(cmd, &params).unwrap_or_else(|e| panic!("{cmd}: {e}"));
}

fn fixture(settings: Value) -> Session {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane":"XY","name":"Square"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[10,10]}));
    run(&mut s, "sketch.finish", json!({}));
    let mut p = json!({"sketch":"Square","distance":"10 mm","direction":"symmetric","operation":"new","name":"Depth","body_name":"Block"});
    p.as_object_mut().unwrap().extend(settings.as_object().unwrap().clone());
    run(&mut s, "solid.extrude", p);
    s
}

fn check(s: &Session, lo: f64, hi: f64) {
    assert!(s.model.results.iter().all(|r| r.error.is_none()));
    let st = s.model.state();
    let body = st.body("Block").unwrap();
    let bounds = body.mesh().bounds();
    assert!((bounds.min.z - lo).abs() < 1e-7 && (bounds.max.z - hi).abs() < 1e-7, "{bounds:?}");
    let m = solidcraft_kernel::measure(&body.body).unwrap();
    assert!((m.volume - 100.0 * (hi - lo)).abs() < 1e-7, "{m:?}");
    assert!((m.centroid.z - (lo + hi) * 0.5).abs() < 1e-7);
}

#[test]
fn symmetric_extrude_total_distance_is_centered_and_follows_parameters() {
    for distance in ["10 mm", "-10 mm"] {
        let mut s = fixture(json!({"distance":distance,"from":[0,0,2],"start_offset":"3 mm"}));
        check(&s, 0.0, 10.0);
        let FeatureKind::Extrude { extent, .. } = &s.doc.find_feature("Depth").unwrap().kind else { panic!("extrude") };
        assert_eq!(extent.convention, ExtrudeConvention::TotalDistance);
        let name = s.doc.find_feature("Depth").unwrap().param_names[0].clone();
        run(&mut s, "parameters.change", json!({"name":name,"expression":"20 mm"}));
        check(&s, -5.0, 15.0);
        run(&mut s, "edit.undo", json!({}));
        check(&s, 0.0, 10.0);
        check(&Session::new(Document::from_json(&s.doc.to_json()).unwrap()), 0.0, 10.0);
    }
}

#[test]
fn symmetric_extrude_switch_from_one_side_keeps_total_thickness() {
    let mut s = fixture(json!({"direction":"positive","convention":"legacy_per_side"}));
    check(&s, 0.0, 10.0);
    run(&mut s, "timeline.edit", json!({"feature":"Depth","set":{"extent":{"direction":"symmetric"}}}));
    check(&s, -5.0, 5.0);
    run(&mut s, "edit.undo", json!({}));
    check(&s, 0.0, 10.0);
    run(
        &mut s,
        "timeline.redefine",
        json!({"feature":"Depth","command":"solid.extrude","params":{"sketch":"Square","distance":"10 mm","direction":"symmetric","body_name":"Block"}}),
    );
    check(&s, -5.0, 5.0);
}

#[test]
fn symmetric_extrude_legacy_missing_marker_preserves_geometry_and_edits() {
    let s = fixture(json!({"convention":"legacy_per_side"}));
    let mut saved: Value = serde_json::from_str(&s.doc.to_json()).unwrap();
    saved["format"] = json!("solidcraft/21");
    for f in saved["features"].as_array_mut().unwrap() {
        if f["type"] == "extrude" {
            f["extent"].as_object_mut().unwrap().remove("convention");
        }
    }
    let mut s = Session::new(Document::from_json(&saved.to_string()).unwrap());
    check(&s, -10.0, 10.0);
    run(&mut s, "timeline.edit", json!({"feature":"Depth","set":{"extent":{"distance":"12 mm"}}}));
    check(&s, -12.0, 12.0);
    run(&mut s, "edit.undo", json!({}));
    run(
        &mut s,
        "timeline.redefine",
        json!({"feature":"Depth","command":"solid.extrude","params":{"sketch":"Square","distance":"10 mm","direction":"symmetric","body_name":"Block"}}),
    );
    check(&s, -10.0, 10.0);
    check(&Session::new(Document::from_json(&s.doc.to_json()).unwrap()), -10.0, 10.0);
}

#[test]
fn symmetric_extrude_through_all_keeps_reach_and_two_sides_keep_independent_distances() {
    let mut s = fixture(json!({"distance2":"3 mm"}));
    check(&s, -3.0, 10.0);
    run(&mut s, "solid.box", json!({"length":10,"width":10,"height":10,"corner":[0,0,100],"body_name":"FarAbove"}));
    run(&mut s, "solid.box", json!({"length":10,"width":10,"height":10,"corner":[0,0,-110],"body_name":"FarBelow"}));
    run(
        &mut s,
        "solid.extrude",
        json!({"sketch":"Square","direction":"symmetric","through_all":true,"operation":"cut","targets":["FarAbove","FarBelow"]}),
    );
    assert!(s.model.state().body("FarAbove").is_none());
    assert!(s.model.state().body("FarBelow").is_none());
}

#[test]
fn symmetric_extrude_invalid_convention_is_atomic() {
    let mut s = fixture(json!({}));
    let before = s.doc.to_json();
    for convention in [json!("unknown"), json!(7), Value::Null] {
        assert!(s.execute("solid.extrude", &json!({"sketch":"Square","distance":10,"direction":"symmetric","convention":convention})).is_err());
        assert_eq!(s.doc.to_json(), before);
    }
}

#[test]
fn symmetric_extrude_thin_wall_uses_total_distance_and_to_object_remains_one_sided() {
    let s = fixture(json!({"thin":true,"thickness":"2 mm"}));
    let st = s.model.state();
    let body = st.body("Block").unwrap();
    let b = body.mesh().bounds();
    assert!((b.min.z + 5.0).abs() < 1e-7 && (b.max.z - 5.0).abs() < 1e-7);
    // A centred 2 mm wall around a 10 mm square: (12² - 8²) × 10.
    assert!((solidcraft_kernel::measure(&body.body).unwrap().volume - 800.0).abs() < 1e-7);
    check(&fixture(json!({"to":[0,0,10]})), 0.0, 10.0);
}

#[test]
fn surface_extrude_shared_extent_retains_its_existing_per_side_semantics() {
    let mut s = fixture(json!({}));
    run(&mut s, "surface.extrude", json!({"sketch":"Square","distance":"10 mm","direction":"symmetric","name":"Sheet","body_name":"Sheet"}));
    let check_sheet = |s: &Session| {
        let st = s.model.state();
        let bounds = st.body("Sheet").unwrap().mesh().bounds();
        assert!((bounds.min.z + 10.0).abs() < 1e-7 && (bounds.max.z - 10.0).abs() < 1e-7);
    };
    check_sheet(&s);
    run(
        &mut s,
        "timeline.redefine",
        json!({"feature":"Sheet","command":"surface.extrude","params":{"sketch":"Square","distance":"10 mm","direction":"symmetric","body_name":"Sheet"}}),
    );
    check_sheet(&s);
    check_sheet(&Session::new(Document::from_json(&s.doc.to_json()).unwrap()));
}

/// A second distance goes the other way from the first side (#2700): along the normal when the
/// first side goes against it, whether by `direction: negative` or by a negative distance.
#[test]
fn a_second_distance_goes_the_other_way_from_the_first_side() {
    for (settings, lo, hi) in [
        (json!({"direction":"positive","distance":"10 mm","distance2":"4 mm"}), -4.0, 10.0),
        (json!({"direction":"negative","distance":"10 mm","distance2":"4 mm"}), -10.0, 4.0),
        (json!({"direction":"positive","distance":"-10 mm","distance2":"4 mm"}), -10.0, 4.0),
    ] {
        let s = fixture(settings.clone());
        check(&s, lo, hi);
        // Saved and opened again, it keeps its sides.
        check(&Session::new(Document::from_json(&s.doc.to_json()).unwrap()), lo, hi);
    }
}

/// Through all on one side and a distance on the other, either way round (#2700). A 40 mm cube
/// round the origin, and an r5 hole sketched 10 mm above its middle: through all goes 10 mm up
/// or 30 mm down to the cube's faces, and the 5 mm goes the other way.
#[test]
fn through_all_on_either_side_with_a_distance_on_the_other() {
    let hole = std::f64::consts::PI * 25.0;
    for (direction, depth) in [("positive", 15.0), ("negative", 35.0)] {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length":40,"width":40,"height":40,"corner":[-20,-20,-20],"body_name":"Box"}));
        run(&mut s, "construct.plane.offset", json!({"base":"XY","offset":10,"name":"Above"}));
        run(&mut s, "sketch.create", json!({"plane":"Above","name":"Hole"}));
        run(&mut s, "sketch.circle.center", json!({"center":[0,0],"radius":5}));
        run(&mut s, "sketch.finish", json!({}));
        run(
            &mut s,
            "solid.extrude",
            json!({"sketch":"Hole","through_all":true,"distance2":"5 mm","direction":direction,"operation":"cut","targets":["Box"]}),
        );
        assert!(s.model.results.iter().all(|r| r.error.is_none()));
        let st = s.model.state();
        let m = solidcraft_kernel::measure(&st.body("Box").unwrap().body).unwrap();
        // The hole's depth from the volume removed (within the circle's measuring precision).
        let deep = (64000.0 - m.volume) / hole;
        assert!((deep - depth).abs() < 0.05, "{direction}: {deep} deep, want {depth}");
    }
}

/// Designs saved before #2700 ignored `direction` on a two-sided extrude; opening one keeps its
/// shape: its first distance still goes along the normal.
#[test]
fn an_older_reversed_two_sided_extrude_keeps_its_shape() {
    let s = fixture(json!({"direction":"negative","distance":"10 mm","distance2":"4 mm"}));
    let mut saved: Value = serde_json::from_str(&s.doc.to_json()).unwrap();
    saved["format"] = json!("solidcraft/27");
    saved.as_object_mut().unwrap().remove("format_pending");
    let s = Session::new(Document::from_json(&saved.to_string()).unwrap());
    check(&s, -4.0, 10.0);
    let FeatureKind::Extrude { extent, .. } = &s.doc.find_feature("Depth").unwrap().kind else { panic!("extrude") };
    assert_eq!(extent.direction, solidcraft_doc::Direction::Positive);
}

/// Extrude targets: the bodies a Join, Cut or Intersect changes.
mod targets {
    #![allow(clippy::unwrap_used, clippy::panic)]

    use crate::{Session, auto_operation, join_face_body, scope_bodies};
    use serde_json::{Value, json};

    fn run(s: &mut Session, command: &str, params: Value) -> Value {
        s.execute(command, &params).unwrap_or_else(|error| panic!("{command}: {error}"))
    }

    fn volume(s: &Session, body: &str) -> f64 {
        solidcraft_kernel::measure(&s.model.state().body(body).unwrap().body).unwrap().volume
    }

    /// A 40 x 40 x 5 plate, a copy of it where it is (a Move with `copy`), and a rim sketched on
    /// the plate's top.
    fn plate_and_copy() -> Session {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 40, "width": 40, "height": 5, "body_name": "Plate"}));
        run(&mut s, "solid.move", json!({"bodies": ["Plate"], "copy": true}));
        run(&mut s, "sketch.create", json!({"plane": "XY", "offset": 5, "name": "Rim"}));
        run(&mut s, "sketch.rectangle.two_point", json!({"p0": [2, 2], "p1": [38, 38]}));
        run(&mut s, "sketch.rectangle.two_point", json!({"p0": [4, 4], "p1": [36, 36]}));
        run(&mut s, "sketch.finish", json!({}));
        s
    }

    const RIM: f64 = (36.0 * 36.0 - 32.0 * 32.0) * 3.0;

    /// Without targets a Join takes every body of the component it meets, hidden or not: the copy
    /// is joined in. Visibility never changes what a command does; named targets keep the copy.
    #[test]
    fn a_join_without_targets_takes_every_body_it_meets_and_targets_keep_the_rest() {
        let mut s = plate_and_copy();
        run(&mut s, "browser.visibility", json!({"items": ["Plate (2)"], "visible": false}));
        let params = json!({"sketch": "Rim", "profiles": [0], "distance": 3, "operation": "join"});
        let r = run(&mut s, "solid.extrude", params.clone());
        assert_eq!(r["removed_bodies"], json!(["Plate (2)"]), "{r}");
        run(&mut s, "edit.undo", json!({}));
        let mut scoped = params;
        scoped["targets"] = json!(["Plate"]);
        let r = run(&mut s, "solid.extrude", scoped);
        assert_eq!((r["changed_bodies"].clone(), r["removed_bodies"].clone()), (json!(["Plate"]), json!([])), "{r}");
        assert!((volume(&s, "Plate") - (8000.0 + RIM)).abs() < 1e-6);
        assert!((volume(&s, "Plate (2)") - 8000.0).abs() < 1e-6);
        // Saved and reopened, the targets keep the copy apart.
        let reopened = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
        assert_eq!(reopened.model.state().bodies.len(), 2);
    }

    /// An extrude up to a reference body's face joins only its targets: the reference stays.
    #[test]
    fn an_extrude_to_a_reference_body_joins_only_its_targets() {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 40, "width": 40, "height": 5, "body_name": "Plate"}));
        run(&mut s, "solid.box", json!({"length": 40, "width": 40, "height": 5, "corner": [0, 0, 10], "operation": "new", "body_name": "Ref"}));
        run(&mut s, "sketch.create", json!({"plane": "XY", "offset": 5, "name": "Post"}));
        run(&mut s, "sketch.rectangle.two_point", json!({"p0": [10, 10], "p1": [30, 30]}));
        run(&mut s, "sketch.finish", json!({}));
        let r = run(&mut s, "solid.extrude", json!({"sketch": "Post", "to": [20, 20, 10], "operation": "join", "targets": ["Plate"]}));
        assert_eq!(r["removed_bodies"], json!([]), "{r}");
        assert!((volume(&s, "Plate") - 10_000.0).abs() < 1e-6);
        assert!((volume(&s, "Ref") - 8000.0).abs() < 1e-6);
    }

    /// `operation: auto` probes the targets it is given (else the participants), as the evaluator
    /// changes them: a profile starting inside a body that is not a target joins.
    #[test]
    fn auto_operation_probes_only_the_named_targets() {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 40, "width": 40, "height": 5, "body_name": "Plate"}));
        run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [15, 15, 5], "operation": "new", "body_name": "Block"}));
        run(&mut s, "sketch.create", json!({"plane": "XY", "offset": 5, "name": "Pad"}));
        run(&mut s, "sketch.rectangle.two_point", json!({"p0": [16, 16], "p1": [24, 24]}));
        run(&mut s, "sketch.finish", json!({}));
        let p = |extra: Value| {
            let mut p = json!({"sketch": "Pad", "distance": 3});
            p.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
            p
        };
        assert_eq!(auto_operation(&s, &p(json!({}))), Some("cut"));
        assert_eq!(auto_operation(&s, &p(json!({"targets": ["Plate"]}))), Some("join"));
        assert_eq!(auto_operation(&s, &p(json!({"targets": ["Block"]}))), Some("cut"));
        assert_eq!(auto_operation(&s, &p(json!({"targets": ["*"]}))), Some("cut"));
        // `*` is every body only on its own, as the evaluator reads it.
        assert_eq!(auto_operation(&s, &p(json!({"targets": ["*", "Plate"]}))), Some("join"));
        // Targets win over participants, as they do when the extrude is evaluated.
        assert_eq!(auto_operation(&s, &p(json!({"targets": ["Plate"], "participants": ["Block"]}))), Some("join"));
        assert_eq!(auto_operation(&s, &p(json!({"participants": ["Plate"]}))), Some("join"));
    }

    /// The body a Join defaults to: the picked face's, else the face its sketch lies on.
    #[test]
    fn join_face_body_is_the_face_or_sketch_face_body() {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 20, "width": 20, "height": 10, "body_name": "Base"}));
        run(&mut s, "sketch.create", json!({"plane": {"face": [10, 10, 10]}, "name": "OnTop"}));
        run(&mut s, "sketch.circle.center", json!({"center": [0, 0], "radius": 2}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "sketch.create", json!({"plane": "XY", "offset": 10, "name": "Free"}));
        run(&mut s, "sketch.circle.center", json!({"center": [5, 5], "radius": 2}));
        run(&mut s, "sketch.finish", json!({}));
        assert_eq!(join_face_body(&s, &json!({"sketch": "OnTop"})).as_deref(), Some("Base"));
        assert_eq!(join_face_body(&s, &json!({"sketch": "Free"})), None);
        assert_eq!(join_face_body(&s, &json!({"face": [10, 10, 10], "face_body": "Base"})).as_deref(), Some("Base"));
        assert_eq!(join_face_body(&s, &json!({"face": [10, 10, 10]})), None);
    }

    /// The bodies the active component works on: its own, else those of the components inside
    /// it placed where it is.
    #[test]
    fn scope_bodies_are_the_components_own_else_its_parts_in_place() {
        let mut s = Session::default();
        run(&mut s, "component.create", json!({"name": "Base"}));
        run(&mut s, "solid.box", json!({"length": 40, "width": 40, "height": 5, "body_name": "Plate"}));
        run(&mut s, "component.activate", json!({"component": 0}));
        assert_eq!(scope_bodies(&s), ["Plate"]);
        run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [60, 0, 0], "operation": "new", "body_name": "Own"}));
        assert_eq!(scope_bodies(&s), ["Own"]);
    }
}
