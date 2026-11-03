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

/// The z range of surface body `name`.
fn sheet_z(s: &Session, name: &str) -> (f64, f64) {
    assert!(s.model.results.iter().all(|r| r.error.is_none()));
    let st = s.model.state();
    let bounds = st.body(name).unwrap().mesh().bounds();
    (bounds.min.z, bounds.max.z)
}

fn assert_z(s: &Session, name: &str, lo: f64, hi: f64) {
    let (z0, z1) = sheet_z(s, name);
    assert!((z0 - lo).abs() < 1e-7 && (z1 - hi).abs() < 1e-7, "{name}: {z0}..{z1}, want {lo}..{hi}");
}

/// A symmetric surface extrude reads its distance as a solid one does: the total, split evenly
/// around the start plane, through a redefine and a save.
#[test]
fn surface_extrude_symmetric_distance_is_the_total_like_solid_extrude() {
    let mut s = fixture(json!({}));
    run(&mut s, "surface.extrude", json!({"sketch":"Square","distance":"10 mm","direction":"symmetric","name":"Sheet","body_name":"Sheet"}));
    assert_z(&s, "Sheet", -5.0, 5.0);
    let FeatureKind::SurfaceExtrude { extent, .. } = &s.doc.find_feature("Sheet").unwrap().kind else { panic!("surface extrude") };
    assert_eq!(extent.convention, ExtrudeConvention::TotalDistance);
    run(
        &mut s,
        "timeline.redefine",
        json!({"feature":"Sheet","command":"surface.extrude","params":{"sketch":"Square","distance":"10 mm","direction":"symmetric","body_name":"Sheet"}}),
    );
    assert_z(&s, "Sheet", -5.0, 5.0);
    assert_z(&Session::new(Document::from_json(&s.doc.to_json()).unwrap()), "Sheet", -5.0, 5.0);
    // A one-sided surface extrude made symmetric splits its distance too, as a solid one does.
    run(&mut s, "surface.extrude", json!({"sketch":"Square","distance":"10 mm","convention":"legacy_per_side","name":"Wall","body_name":"Wall"}));
    assert_z(&s, "Wall", 0.0, 10.0);
    run(&mut s, "timeline.edit", json!({"feature":"Wall","set":{"extent":{"direction":"symmetric"}}}));
    assert_z(&s, "Wall", -5.0, 5.0);
}

/// Surface extrudes saved before they matched solid ones stored `legacy_per_side` (or, older
/// still, no marker): they rebuild the distance on each side, and keep it through edits.
#[test]
fn older_symmetric_surface_extrudes_keep_the_distance_on_each_side() {
    let mut s = fixture(json!({}));
    run(
        &mut s,
        "surface.extrude",
        json!({"sketch":"Square","distance":"10 mm","direction":"symmetric","convention":"legacy_per_side","name":"Sheet","body_name":"Sheet"}),
    );
    assert_z(&s, "Sheet", -10.0, 10.0);
    let marked: Value = serde_json::from_str(&s.doc.to_json()).unwrap();
    let mut unmarked = marked.clone();
    unmarked["format"] = json!("solidcraft/21");
    unmarked.as_object_mut().unwrap().remove("format_pending");
    for f in unmarked["features"].as_array_mut().unwrap() {
        if let Some(extent) = f.get_mut("extent").and_then(Value::as_object_mut) {
            extent.remove("convention");
        }
    }
    for saved in [marked, unmarked] {
        let mut s = Session::new(Document::from_json(&saved.to_string()).unwrap());
        assert_z(&s, "Sheet", -10.0, 10.0);
        run(&mut s, "timeline.edit", json!({"feature":"Sheet","set":{"extent":{"distance":"12 mm"}}}));
        assert_z(&s, "Sheet", -12.0, 12.0);
        run(
            &mut s,
            "timeline.redefine",
            json!({"feature":"Sheet","command":"surface.extrude","params":{"sketch":"Square","distance":"10 mm","direction":"symmetric","body_name":"Sheet"}}),
        );
        assert_z(&s, "Sheet", -10.0, 10.0);
    }
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
