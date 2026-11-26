#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use crate::Session;
use serde_json::{Value, json};
use solidcraft_doc::{Document, ExtrudeConvention, FeatureKind};
use solidcraft_geom::MeshGeom;

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
        // A sign only turns its own side over: Distance 2 stays where Direction puts it, so a
        // negative Distance 1 leaves the span between the two ends.
        (json!({"direction":"positive","distance":"-10 mm","distance2":"4 mm"}), -10.0, -4.0),
        (json!({"direction":"negative","distance":"-10 mm","distance2":"4 mm"}), 4.0, 10.0),
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

/// The second side takes the extent types of the first (#18691): Through All, or To Object with
/// its own offset. A 40 mm cube round the origin, and an r5 hole sketched 10 mm above its
/// middle: the first side goes up and the second down, so the removed depth is how far each
/// reaches added together.
#[test]
fn the_second_side_takes_through_all_and_to_object() {
    let hole = std::f64::consts::PI * 25.0;
    for (settings, depth) in [
        // Both sides through all: 10 mm up to the top face and 30 mm down to the bottom one.
        (json!({"through_all":true,"through_all2":true}), 40.0),
        // A distance up and through all down.
        (json!({"distance":"5 mm","through_all2":true}), 35.0),
        // To the bottom face, picked at a point on it; the offset carries the end 5 mm short.
        (json!({"distance":"5 mm","to2":[0,0,-20]}), 35.0),
        (json!({"distance":"5 mm","to2":[0,0,-20],"to_offset2":"-5 mm"}), 30.0),
        // Through all up, to an object down.
        (json!({"through_all":true,"to2":[0,0,-20]}), 40.0),
        // To an object up and through all down.
        (json!({"to":[0,0,20],"through_all2":true}), 40.0),
        // To an object on both sides.
        (json!({"to":[0,0,20],"to2":[0,0,-20]}), 40.0),
    ] {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length":40,"width":40,"height":40,"corner":[-20,-20,-20],"body_name":"Box"}));
        run(&mut s, "construct.plane.offset", json!({"base":"XY","offset":10,"name":"Above"}));
        run(&mut s, "sketch.create", json!({"plane":"Above","name":"Hole"}));
        run(&mut s, "sketch.circle.center", json!({"center":[0,0],"radius":5}));
        run(&mut s, "sketch.finish", json!({}));
        let mut p = json!({"sketch":"Hole","operation":"cut","targets":["Box"],"name":"Bore"});
        p.as_object_mut().unwrap().extend(settings.as_object().unwrap().clone());
        run(&mut s, "solid.extrude", p);
        let removed = |s: &Session| {
            assert!(s.model.results.iter().all(|r| r.error.is_none()), "{settings}: {:?}", s.model.results);
            let st = s.model.state();
            (64000.0 - solidcraft_kernel::measure(&st.body("Box").unwrap().body).unwrap().volume) / hole
        };
        let deep = removed(&s);
        assert!((deep - depth).abs() < 0.05, "{settings}: {deep} deep, want {depth}");
        // Saved and opened again, it keeps both extents.
        let reopened = Session::new(Document::from_json(&s.doc.to_json()).unwrap());
        let deep = removed(&reopened);
        assert!((deep - depth).abs() < 0.05, "{settings} reopened: {deep} deep, want {depth}");
    }
}

/// A Join whose two To Object sides end on different bodies joins both of them, and the second
/// side is for Two Sides: a symmetric extrude refuses it (#18691).
#[test]
fn a_join_to_objects_on_both_sides_joins_both_bodies() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length":20,"width":20,"height":5,"corner":[-10,-10,5],"body_name":"Up"}));
    run(&mut s, "solid.box", json!({"length":20,"width":20,"height":5,"corner":[-10,-10,-10],"body_name":"Down"}));
    run(&mut s, "sketch.create", json!({"plane":"XY","name":"Pin"}));
    run(&mut s, "sketch.circle.center", json!({"center":[0,0],"radius":2}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch":"Pin","to":[0,0,5],"to2":[0,0,-5],"operation":"join","name":"Pin"}));
    assert!(s.model.results.iter().all(|r| r.error.is_none()), "{:?}", s.model.results);
    let st = s.model.state();
    assert_eq!(st.bodies.len(), 1, "the pin, Up and Down are one body: {:?}", st.bodies.iter().map(|b| b.name.clone()).collect::<Vec<_>>());
    let volume = solidcraft_kernel::measure(&st.bodies[0].body).unwrap().volume;
    let want = 2.0 * 20.0 * 20.0 * 5.0 + std::f64::consts::PI * 4.0 * 10.0;
    assert!((volume - want).abs() < 0.5, "{volume} mm3, want {want}");
    for second in [json!({"through_all2":true}), json!({"to2":[0,0,-5]})] {
        let mut p = json!({"sketch":"Pin","distance":"3 mm","direction":"symmetric","operation":"new","name":"Sym"});
        p.as_object_mut().unwrap().extend(second.as_object().unwrap().clone());
        assert!(s.execute("solid.extrude", &p).is_err(), "{second}: a symmetric extrude has no second extent");
    }
}

/// The second side's To Object ends on a level, so it takes a plane parallel to the sketch, a vertex
/// or a point; a curved or slanted face is refused, not flattened (#18691). A pin 10 mm above a
/// plate's middle goes down: the flank of a horizontal cylinder and the slope of a wedge are
/// refused with the reason, the plate's top face is not.
#[test]
fn the_second_side_refuses_a_to_object_that_is_curved_or_slanted() {
    let cases: Vec<(&str, Box<dyn Fn(&mut Session)>, serde_json::Value)> = vec![
        (
            "a cylinder's flank",
            Box::new(|s: &mut Session| {
                run(s, "sketch.create", json!({"plane":"YZ","name":"Round"}));
                run(s, "sketch.circle.center", json!({"center":[0,-30],"radius":10}));
                run(s, "sketch.finish", json!({}));
                run(s, "solid.extrude", json!({"sketch":"Round","distance":40,"direction":"symmetric","name":"Target","body_name":"Target"}));
            }),
            json!([0, 0, -20]),
        ),
        (
            "a slanted plane",
            Box::new(|s: &mut Session| {
                // An XZ sketch's y runs along -Z: the wedge is at Z -40 up to a slope from -30 to -20.
                run(s, "sketch.create", json!({"plane":"XZ","name":"Wedge"}));
                run(s, "sketch.line", json!({"points": [[-20,40],[20,40],[20,20],[-20,30]], "closed": true}));
                run(s, "sketch.finish", json!({}));
                run(s, "solid.extrude", json!({"sketch":"Wedge","distance":30,"direction":"symmetric","name":"Target","body_name":"Target"}));
            }),
            json!([0, 0, -25]),
        ),
    ];
    for (what, build, pick) in cases {
        let mut s = Session::default();
        build(&mut s);
        assert!(s.model.results.iter().all(|r| r.error.is_none()), "{what} target: {:?}", s.model.results);
        run(&mut s, "construct.plane.offset", json!({"base":"XY","offset":10,"name":"Above"}));
        run(&mut s, "sketch.create", json!({"plane":"Above","name":"Pin"}));
        run(&mut s, "sketch.circle.center", json!({"center":[0,0],"radius":2}));
        run(&mut s, "sketch.finish", json!({}));
        let refused = s.execute("solid.extrude", &json!({"sketch":"Pin","distance":"5 mm","to2":pick,"operation":"new","name":"Pin"}));
        let mut errors: Vec<String> = s.model.results.iter().filter_map(|r| r.error.clone()).collect();
        errors.extend(refused.err().map(|e| e.to_string()));
        assert!(
            errors.iter().any(|e| e.contains("not supported yet") && e.contains("second side")),
            "{what}: want a typed refusal naming the second side, got {errors:?}"
        );
    }
}

/// Through all keeps the side a saved negative distance points to (#10415): switching an inward
/// cut to Through All must not turn it to the plane's positive side. A 40 mm cube round the
/// origin, and an r5 hole sketched 10 mm above its middle: positive reaches 10 mm up to the
/// cube's top face, negative reaches 30 mm down to its bottom face.
#[test]
fn through_all_follows_the_sign_of_the_saved_distance() {
    let hole = std::f64::consts::PI * 25.0;
    for (distance, depth) in [("5 mm", 10.0), ("-5 mm", 30.0)] {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length":40,"width":40,"height":40,"corner":[-20,-20,-20],"body_name":"Box"}));
        run(&mut s, "construct.plane.offset", json!({"base":"XY","offset":10,"name":"Above"}));
        run(&mut s, "sketch.create", json!({"plane":"Above","name":"Hole"}));
        run(&mut s, "sketch.circle.center", json!({"center":[0,0],"radius":5}));
        run(&mut s, "sketch.finish", json!({}));
        run(
            &mut s,
            "solid.extrude",
            json!({"sketch":"Hole","through_all":true,"distance":distance,"direction":"positive","operation":"cut","targets":["Box"]}),
        );
        assert!(s.model.results.iter().all(|r| r.error.is_none()));
        let st = s.model.state();
        let m = solidcraft_kernel::measure(&st.body("Box").unwrap().body).unwrap();
        let deep = (64000.0 - m.volume) / hole;
        assert!((deep - depth).abs() < 0.05, "{distance}: {deep} deep, want {depth}");
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

/// Designs saved before entered distances kept their sides sent Distance 2 to the other side of
/// a negative first distance. Opening one keeps that, for an expression too and after its
/// parameter changes, and leaves the distance and its named input as they were.
#[test]
fn an_older_two_sided_extrude_with_a_negative_distance_keeps_its_sides() {
    let mut base = fixture(json!({"direction":"positive","distance":"10 mm","distance2":"10 mm"}));
    run(&mut base, "parameters.add", json!({"name":"width","expression":"20 mm"}));
    let mut saved: Value = serde_json::from_str(&base.doc.to_json()).unwrap();
    // Format 50 predates the v51 second-side migration. A moving current-1 tag
    // can already include that migration after unrelated format changes.
    saved["format"] = json!("solidcraft/50");
    saved.as_object_mut().unwrap().remove("format_pending");
    for (distance, lo, hi) in [("-width", -20.0, 10.0), ("-0 mm", -10.0, 0.0), ("-10 mm", -10.0, 10.0)] {
        let mut file = saved.clone();
        for f in file["features"].as_array_mut().unwrap() {
            if f["type"] == "extrude" {
                f["extent"]["distance"] = json!(distance);
            }
        }
        let mut opened = Session::new(Document::from_json(&file.to_string()).unwrap());
        check(&opened, lo, hi);
        let FeatureKind::Extrude { extent, .. } = &opened.doc.find_feature("Depth").unwrap().kind else { panic!("extrude") };
        assert_eq!(extent.distance, distance);
        if distance == "-width" {
            run(&mut opened, "parameters.change", json!({"name":"width","expression":"15 mm"}));
            check(&opened, -15.0, 10.0);
        }
        check(&Session::new(Document::from_json(&opened.doc.to_json()).unwrap()), if distance == "-width" { -15.0 } else { lo }, hi);
    }
}

#[test]
fn through_all_reach_uses_exact_body_bounds() {
    let s = fixture(json!({"direction": "positive", "distance": "10 mm"}));
    let st = s.model.state();
    let reach = super::through_all_reach(&st, &json!({"through_all": true}), solidcore_record::Vec3::ZERO);
    assert!(reach.is_some());
    let reach = reach.unwrap();
    assert!(reach > 10.0);
}

#[test]
fn symmetric_extrude_with_symmetric_offset_leaves_clearance_gap_and_follows_parameters() {
    let mut s = fixture(json!({
        "distance": "10 mm",
        "direction": "symmetric",
        "start_offset": "3 mm",
        "symmetric_offset": true,
    }));
    assert!(s.model.results.iter().all(|r| r.error.is_none()));
    let st = s.model.state();
    assert_eq!(st.bodies.len(), 2);
    let b0 = &st.bodies[0];
    let b1 = &st.bodies[1];
    let bounds0 = b0.mesh().bounds();
    let bounds1 = b1.mesh().bounds();
    let (neg, pos) = if bounds0.min.z < bounds1.min.z { (bounds0, bounds1) } else { (bounds1, bounds0) };
    assert!((neg.min.z - (-8.0)).abs() < 1e-7 && (neg.max.z - (-3.0)).abs() < 1e-7, "{neg:?}");
    assert!((pos.min.z - 3.0).abs() < 1e-7 && (pos.max.z - 8.0).abs() < 1e-7, "{pos:?}");
    let m0 = solidcraft_kernel::measure(&b0.body).unwrap();
    let m1 = solidcraft_kernel::measure(&b1.body).unwrap();
    assert!((m0.volume - 500.0).abs() < 1e-7 && (m1.volume - 500.0).abs() < 1e-7);

    // Changing the distance expression stretches both wings outward, leaving the clearance gap intact.
    let name = s.doc.find_feature("Depth").unwrap().param_names[0].clone();
    run(&mut s, "parameters.change", json!({"name": name, "expression": "20 mm"}));
    let st = s.model.state();
    let bounds0 = st.bodies[0].mesh().bounds();
    let bounds1 = st.bodies[1].mesh().bounds();
    let (neg, pos) = if bounds0.min.z < bounds1.min.z { (bounds0, bounds1) } else { (bounds1, bounds0) };
    assert!((neg.min.z - (-13.0)).abs() < 1e-7 && (neg.max.z - (-3.0)).abs() < 1e-7, "{neg:?}");
    assert!((pos.min.z - 3.0).abs() < 1e-7 && (pos.max.z - 13.0).abs() < 1e-7, "{pos:?}");

    // Undo returns to original bounds.
    run(&mut s, "edit.undo", json!({}));
    let st = s.model.state();
    let bounds0 = st.bodies[0].mesh().bounds();
    let bounds1 = st.bodies[1].mesh().bounds();
    let (neg, pos) = if bounds0.min.z < bounds1.min.z { (bounds0, bounds1) } else { (bounds1, bounds0) };
    assert!((neg.min.z - (-8.0)).abs() < 1e-7 && (neg.max.z - (-3.0)).abs() < 1e-7);
    assert!((pos.min.z - 3.0).abs() < 1e-7 && (pos.max.z - 8.0).abs() < 1e-7);

    // Sketch attached to a wing cap keeps its attachment across parameter edits.
    let on_cap = s.execute("sketch.create", &json!({"plane": {"face": [5, 5, 8]}, "name": "OnPosCap", "project_edges": false})).unwrap()["sketch"]
        .as_u64()
        .unwrap();
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "parameters.change", json!({"name": name, "expression": "20 mm"}));
    assert!((s.model.state().sketch(on_cap).unwrap().plane.origin.z - 13.0).abs() < 1e-7);
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "edit.undo", json!({}));

    // Serialization roundtrip preserves symmetric_offset and geometry.
    let reloaded = Session::new(Document::from_json(&s.doc.to_json()).unwrap());
    assert!(reloaded.model.results.iter().all(|r| r.error.is_none()));
    let st = reloaded.model.state();
    assert_eq!(st.bodies.len(), 2);
    let FeatureKind::Extrude { extent, .. } = &reloaded.doc.find_feature("Depth").unwrap().kind else { panic!("extrude") };
    assert!(extent.symmetric_offset);

    // Extrude Cut with symmetric offset leaves the central web untouched.
    let mut sc = Session::default();
    run(&mut sc, "solid.box", json!({"length": 40, "width": 40, "height": 40, "corner": [-20, -20, -20], "body_name": "Target"}));
    run(&mut sc, "sketch.create", json!({"plane": "XY", "name": "CutSquare"}));
    run(&mut sc, "sketch.rectangle.two_point", json!({"p0": [-5, -5], "p1": [5, 5]}));
    run(&mut sc, "sketch.finish", json!({}));
    run(
        &mut sc,
        "solid.extrude",
        json!({
            "sketch": "CutSquare",
            "distance": "10 mm",
            "direction": "symmetric",
            "start_offset": "3 mm",
            "symmetric_offset": true,
            "operation": "cut",
            "targets": ["Target"],
        }),
    );
    assert!(sc.model.results.iter().all(|r| r.error.is_none()));
    let st_c = sc.model.state();
    let tb = st_c.body("Target").unwrap();
    let m_cut = solidcraft_kernel::measure(&tb.body).unwrap();
    // 64000 - 2 * (10 * 10 * 5) = 63000
    assert!((m_cut.volume - 63000.0).abs() < 1e-5, "volume {}", m_cut.volume);
}

#[test]
fn symmetric_offset_with_second_distance_is_rejected() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Square"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [10, 10]}));
    run(&mut s, "sketch.finish", json!({}));
    let err = s
        .execute(
            "solid.extrude",
            &json!({
                "sketch": "Square",
                "distance": "10 mm",
                "distance2": "4 mm",
                "direction": "symmetric",
                "start_offset": "3 mm",
                "symmetric_offset": true,
            }),
        )
        .unwrap_err()
        .to_string();
    assert!(err.contains("symmetric offset with a second distance"), "unexpected error: {err}");

    let err_from = s
        .execute(
            "solid.extrude",
            &json!({
                "sketch": "Square",
                "distance": "10 mm",
                "direction": "symmetric",
                "from": [0, 0, 3],
                "symmetric_offset": true,
            }),
        )
        .unwrap_err()
        .to_string();
    assert!(err_from.contains("symmetric offset with From Object"), "unexpected error: {err_from}");
}
