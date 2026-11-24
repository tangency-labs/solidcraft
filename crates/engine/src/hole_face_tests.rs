//! A hole placed on a face follows that face along the hole's axis, never sideways: when the face
//! moves the hole moves with it, and when the face no longer reaches the place the hole stays
//! where it was put and says so instead of sliding to the face's nearest point.

use serde_json::{Value, json};
use solidcore_record::Vec3;

use crate::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    match s.execute(id, &p) {
        Ok(v) => v,
        Err(e) => panic!("{id} {p}: {e}"),
    }
}

fn change(s: &mut Session, name: &str, expression: &str) {
    run(s, "parameters.change", json!({"name": name, "expression": expression}));
}

fn volume(s: &mut Session) -> f64 {
    run(s, "inspect.measure", json!({}))["total"]["volume_mm3"].as_f64().unwrap()
}

/// Whether the model holds material at (x, y, z).
fn solid_at(s: &Session, x: f64, y: f64, z: f64) -> bool {
    s.model.state().bodies.iter().any(|b| b.body.classify_point(Vec3::new(x, y, z)).unwrap() == solidcraft_kernel::PointClass::Inside)
}

/// What the hole feature says (its error and its warning); empty when it is silent.
fn said(s: &Session, feature: &str) -> String {
    let id = s.doc.find_feature(feature).unwrap().id;
    let r = s.model.result(id).unwrap();
    format!("{}{}", r.error.clone().unwrap_or_default(), r.warning.clone().unwrap_or_default())
}

/// A valid counterbore cannot be checked against a made-up through-all depth after its body
/// disappears. Report the missing body, and recover the hole when the deletion is undone.
#[test]
fn a_hole_without_its_body_reports_the_missing_body_and_recovers_on_undo() {
    for command in ["timeline.delete", "timeline.suppress"] {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 20, "width": 20, "height": 6, "name": "Plate"}));
        run(&mut s, "sketch.create", json!({"plane": "XY", "offset": 6, "name": "HolePts"}));
        run(&mut s, "sketch.point", json!({"point": [10, 10]}));
        run(&mut s, "sketch.finish", json!({}));
        run(
            &mut s,
            "solid.hole",
            json!({"sketch": "HolePts", "diameter": 3.4, "type": "counterbore", "cb_diameter": 6.5, "cb_depth": 3.3, "name": "Hole"}),
        );
        assert_eq!(said(&s, "Hole"), "");
        let drilled_volume = volume(&mut s);
        run(&mut s, command, json!({"feature": "Plate"}));
        assert!(s.model.state().bodies.is_empty());
        assert_eq!(said(&s, "Hole"), "there is no body to cut or intersect", "{command}");
        run(&mut s, "edit.undo", json!({}));
        assert_eq!(said(&s, "Hole"), "");
        assert_eq!(s.model.state().bodies.len(), 1);
        near(volume(&mut s), drilled_volume);
        run(&mut s, "edit.redo", json!({}));
        assert!(s.model.state().bodies.is_empty());
        assert_eq!(said(&s, "Hole"), "there is no body to cut or intersect", "{command} after redo");
    }
}

#[test]
fn a_hole_the_face_no_longer_reaches_stays_where_it_was_placed_and_says_so() {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "length", "expression": "100 mm"}));
    run(&mut s, "solid.box", json!({"length": "length", "width": 50, "height": 20, "name": "Plate"}));
    run(&mut s, "solid.hole", json!({"position": [80, 25, 20], "diameter": "6 mm", "name": "Hole"}));
    // The bore is at x = 80, 3 mm either way.
    let drilled_at_80 = |s: &Session| !solid_at(s, 80.0, 25.0, 10.0) && solid_at(s, 76.0, 25.0, 10.0) && solid_at(s, 84.0, 25.0, 10.0);
    assert!(drilled_at_80(&s));
    // Longer: the place is still on the top face, and the hole stays at x = 80.
    change(&mut s, "length", "150 mm");
    assert_eq!(said(&s, "Hole"), "");
    assert!(drilled_at_80(&s));
    // Shorter than the place: nothing is drilled at the new end, and the hole says why. A hole
    // moved to the face's nearest point would cut half a hole there (59 717 mm³ left).
    change(&mut s, "length", "60 mm");
    let left = volume(&mut s);
    assert!((left - 60_000.0).abs() < 1.0, "half a hole was cut at the plate's end: {left} mm³ of 60000");
    let msg = said(&s, "Hole");
    assert!(msg.contains("no longer on face") && msg.contains("[80, 25, 20]"), "{msg}");
    // Long again: the hole is back where it was put, with no message.
    change(&mut s, "length", "100 mm");
    assert_eq!(said(&s, "Hole"), "");
    assert!(drilled_at_80(&s));
}

#[test]
fn a_hole_placed_by_expressions_is_checked_where_they_evaluate_now_not_where_it_was_made() {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "L", "expression": "100 mm"}));
    run(&mut s, "solid.box", json!({"length": "L", "width": 40, "height": 10, "center": [0, 0, 5], "name": "Plate", "body_name": "Plate"}));
    // A window cut that moves with L: at L = 100 it spans x -5..25, at L = 180 it spans 35..65.
    run(
        &mut s,
        "solid.box",
        json!({"length": 30, "width": 20, "height": 20, "center": ["L/2 - 40", 0, 5], "operation": "cut", "targets": ["Plate"], "name": "Window"}),
    );
    run(&mut s, "solid.hole", json!({"position": ["L/2 - 10", 0, 10], "diameter": 5, "name": "Hole"}));
    assert_eq!(said(&s, "Hole"), "");
    // The hole follows to x = 80, which is on the top face. Its made-at point, x = 40, now lies
    // in the window: the hole must not say it stayed where it was placed.
    change(&mut s, "L", "180 mm");
    assert!(!solid_at(&s, 80.0, 0.0, 5.0) && solid_at(&s, 75.0, 0.0, 5.0), "the bore is at x = 80");
    assert_eq!(said(&s, "Hole"), "");
}

#[test]
fn a_slanted_hole_stays_on_its_axis_when_its_face_moves() {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "height", "expression": "20 mm"}));
    run(&mut s, "solid.box", json!({"length": 60, "width": 30, "height": "height", "name": "Block"}));
    // Entering the top face at x = 20, running on in x as it goes down: 1 across for every 2 down.
    run(&mut s, "solid.hole", json!({"position": [20, 15, 20], "direction": [1, 0, -2], "diameter": "2 mm", "name": "Hole"}));
    // The axis is the line through the place along the direction, whatever the height of the block:
    // x = 20 + (20 - z) / 2. At mid-height the bore is empty on it and solid 3 mm to either side
    // (the bore is about 2.2 mm wide there).
    let on_axis = |s: &Session, h: f64| {
        let (x, z) = (20.0 + (20.0 - h / 2.0) / 2.0, h / 2.0);
        assert!(!solid_at(s, x, 15.0, z), "block height {h}: the bore should pass through x = {x} at z = {z}");
        assert!(solid_at(s, x - 3.0, 15.0, z) && solid_at(s, x + 3.0, 15.0, z), "block height {h}: material 3 mm beside the axis");
        // (The kernel may note a boolean retry for a slanted bore; the hole's place is on its face.)
        assert!(!said(s, "Hole").contains("no longer on face"), "block height {h}: {}", said(s, "Hole"));
    };
    on_axis(&s, 20.0);
    // Taller: the top face moves up. Moved to the face's nearest point the hole would slide
    // sideways onto another axis (5 mm further along x at mid-height); along its axis it enters
    // higher up the same line.
    change(&mut s, "height", "30 mm");
    on_axis(&s, 30.0);
    // Lower: the same line enters further along.
    change(&mut s, "height", "12 mm");
    on_axis(&s, 12.0);
}

// Hole centres on a sketch drill into the material whichever way the sketch plane faces. Base
// part: a 40 x 40 x 20 box at z 0..20 (32000 mm³).

fn box_part(s: &mut Session) {
    run(s, "solid.box", json!({"length": 40, "width": 40, "height": 20, "name": "Part"}));
}

/// A sketch named `Pts` on the XY plane lifted to `z`, holding the given points (ids p0, p1, ...).
fn points_sketch(s: &mut Session, z: f64, at: &[[f64; 2]]) -> Vec<String> {
    let plane = if z == 0.0 { json!("XY") } else { json!({"origin": [0, 0, z], "x_dir": [1, 0, 0], "y_dir": [0, 1, 0]}) };
    run(s, "sketch.create", json!({"plane": plane, "name": "Pts"}));
    let ids: Vec<String> = (0..at.len()).map(|i| format!("pt{z}_{i}").replace(['-', '.'], "m")).collect();
    for (id, p) in ids.iter().zip(at) {
        run(s, "sketch.point", json!({"point": p, "id": id}));
    }
    run(s, "sketch.finish", json!({}));
    ids
}

fn drill(s: &mut Session, ids: &[String], extra: Value) {
    let mut p = json!({"sketch": "Pts", "points": ids, "diameter": 10, "name": "H"});
    if let (Some(m), Some(e)) = (p.as_object_mut(), extra.as_object()) {
        m.extend(e.clone());
    }
    run(s, "solid.hole", p);
}

fn removed(s: &mut Session) -> f64 {
    32_000.0 - volume(s)
}

fn near(a: f64, b: f64) {
    assert!((a - b).abs() < 1.0, "{a} vs {b}");
}

const CB: fn() -> Value = || json!({"type": "counterbore", "diameter": 6, "cb_diameter": 12, "cb_depth": 3, "depth": 10});

#[test]
fn sketch_under_the_part_drills_up_into_it() {
    let mut s = Session::default();
    box_part(&mut s);
    let ids = points_sketch(&mut s, 0.0, &[[20.0, 20.0]]);
    drill(&mut s, &ids, json!({"depth": 8}));
    near(removed(&mut s), 628.3);
    assert!(!solid_at(&s, 20.0, 20.0, 4.0) && solid_at(&s, 20.0, 20.0, 10.0));
}

#[test]
fn a_hole_on_a_sketch_alone_skips_arc_centres() {
    // A rounded corner's arc centre is the arc's geometry, not a hole site (#18429): only the
    // stand-alone point drills, one Ø10 x 8 hole (628.3 mm³).
    let mut s = Session::default();
    box_part(&mut s);
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Pts"}));
    run(&mut s, "sketch.arc.center_point", json!({"center": [5, 5], "start": [0, 5], "end": [5, 0]}));
    run(&mut s, "sketch.point", json!({"point": [20, 20], "id": "mine"}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.hole", json!({"sketch": "Pts", "diameter": 10, "depth": 8, "name": "H"}));
    near(removed(&mut s), 628.3);
    assert!(!solid_at(&s, 20.0, 20.0, 4.0) && solid_at(&s, 5.0, 5.0, 4.0));
}

#[test]
fn a_hole_on_a_sketch_alone_skips_a_construction_rectangles_centre() {
    // A construction rectangle's centre is reference geometry, not a hole site (#18429): only the
    // stand-alone point drills, one Ø10 x 8 hole (628.3 mm³).
    let mut s = Session::default();
    box_part(&mut s);
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Pts"}));
    run(&mut s, "sketch.rectangle.center", json!({"center": [8, 8], "corner": [12, 12], "construction": true}));
    run(&mut s, "sketch.point", json!({"point": [20, 20], "id": "mine"}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.hole", json!({"sketch": "Pts", "diameter": 10, "depth": 8, "name": "H"}));
    near(removed(&mut s), 628.3);
    assert!(!solid_at(&s, 20.0, 20.0, 4.0) && solid_at(&s, 8.0, 8.0, 4.0));
}

#[test]
fn a_hole_on_a_sketch_alone_drills_a_point_at_a_lone_construction_lines_midpoint() {
    // Only a whole construction rectangle makes its centre reference geometry: a point held at the
    // midpoint of one construction line is still a hole site, one Ø10 x 8 hole (628.3 mm³).
    let mut s = Session::default();
    box_part(&mut s);
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Pts"}));
    run(&mut s, "sketch.line", json!({"points": [[15, 20], [25, 20]], "construction": true, "ids": ["guide"]}));
    run(&mut s, "sketch.point", json!({"point": [20, 20], "id": "mine"}));
    run(&mut s, "sketch.constraint.midpoint", json!({"point": "mine", "line": "guide"}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.hole", json!({"sketch": "Pts", "diameter": 10, "depth": 8, "name": "H"}));
    near(removed(&mut s), 628.3);
    assert!(!solid_at(&s, 20.0, 20.0, 4.0));
}

#[test]
fn a_hole_on_a_sketch_alone_drills_a_rectangle_centre_with_a_regular_diagonal() {
    // A construction rectangle whose second diagonal is regular geometry is not all reference:
    // its centre drills, with the stand-alone point, two Ø10 x 8 holes (1256.6 mm³).
    let mut s = Session::default();
    box_part(&mut s);
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Pts"}));
    let r = run(&mut s, "sketch.rectangle.center", json!({"center": [8, 8], "corner": [12, 12], "construction": true}));
    // the four edges come first; the diagonals follow them, the second one last
    assert_eq!(r["curves"], json!(["l1", "l2", "l3", "l4"]), "{r}");
    run(&mut s, "sketch.construction", json!({"curves": ["l6"], "value": false}));
    run(&mut s, "sketch.point", json!({"point": [20, 20], "id": "mine"}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.hole", json!({"sketch": "Pts", "diameter": 10, "depth": 8, "name": "H"}));
    near(removed(&mut s), 1256.6);
    assert!(!solid_at(&s, 20.0, 20.0, 4.0) && !solid_at(&s, 8.0, 8.0, 4.0));
}

#[test]
fn sketch_under_the_part_counterbore_and_four_points() {
    let one = std::f64::consts::PI * 36.0 * 3.0 + std::f64::consts::PI * 9.0 * 7.0;
    let mut s = Session::default();
    box_part(&mut s);
    let ids = points_sketch(&mut s, 0.0, &[[20.0, 20.0]]);
    drill(&mut s, &ids, CB());
    near(removed(&mut s), one);
    near(one, 537.2);
    let mut s = Session::default();
    box_part(&mut s);
    let ids = points_sketch(&mut s, 0.0, &[[10.0, 10.0], [30.0, 10.0], [10.0, 30.0], [30.0, 30.0]]);
    drill(&mut s, &ids, CB());
    near(removed(&mut s), 4.0 * one);
}

#[test]
fn sketch_under_the_part_through_all() {
    let mut s = Session::default();
    box_part(&mut s);
    let ids = points_sketch(&mut s, 0.0, &[[20.0, 20.0]]);
    drill(&mut s, &ids, json!({}));
    near(removed(&mut s), 1570.8);
    assert!(!solid_at(&s, 20.0, 20.0, 19.0));
}

#[test]
fn sketch_in_the_air_below_the_part() {
    let mut s = Session::default();
    box_part(&mut s);
    let ids = points_sketch(&mut s, -10.0, &[[20.0, 20.0]]);
    drill(&mut s, &ids, json!({"depth": 8}));
    near(removed(&mut s), 628.3);
}

#[test]
fn sketch_on_top_face_height_is_unchanged() {
    let one = std::f64::consts::PI * 36.0 * 3.0 + std::f64::consts::PI * 9.0 * 7.0;
    let mut s = Session::default();
    box_part(&mut s);
    let ids = points_sketch(&mut s, 20.0, &[[20.0, 20.0]]);
    drill(&mut s, &ids, json!({"depth": 8}));
    near(removed(&mut s), 628.3);
    assert!(!solid_at(&s, 20.0, 20.0, 16.0) && solid_at(&s, 20.0, 20.0, 10.0));
    let mut s = Session::default();
    box_part(&mut s);
    let ids = points_sketch(&mut s, 20.0, &[[10.0, 10.0], [30.0, 10.0], [10.0, 30.0], [30.0, 30.0]]);
    drill(&mut s, &ids, CB());
    near(removed(&mut s), 4.0 * one);
    let mut s = Session::default();
    box_part(&mut s);
    let ids = points_sketch(&mut s, 20.0, &[[20.0, 20.0]]);
    drill(&mut s, &ids, json!({}));
    near(removed(&mut s), 1570.8);
}

#[test]
fn sketch_inside_the_body_still_drills_down() {
    let mut s = Session::default();
    box_part(&mut s);
    let ids = points_sketch(&mut s, 10.0, &[[20.0, 20.0]]);
    drill(&mut s, &ids, json!({"depth": 8}));
    near(removed(&mut s), 628.3);
    // As before: the spot moves back up to the top face and drills down from there (z 12..20).
    assert!(!solid_at(&s, 20.0, 20.0, 16.0) && solid_at(&s, 20.0, 20.0, 6.0));
}

// A center exactly on an edge or a corner of the part: half or a quarter of the hole is in the
// material, and the side is chosen the same way as for a center well inside the face.

#[test]
fn center_on_an_edge_drills_half_a_hole_from_either_side() {
    let half = std::f64::consts::PI * 25.0 * 8.0 / 2.0;
    near(half, 314.2);
    for (z, cut, kept) in [(20.0, 16.0, 6.0), (0.0, 4.0, 14.0)] {
        let mut s = Session::default();
        box_part(&mut s);
        let ids = points_sketch(&mut s, z, &[[0.0, 20.0]]);
        drill(&mut s, &ids, json!({"depth": 8}));
        near(removed(&mut s), half);
        assert!(!solid_at(&s, 2.0, 20.0, cut) && solid_at(&s, 2.0, 20.0, kept), "sketch at z {z}");
    }
}

#[test]
fn center_on_a_corner_drills_a_quarter_of_a_hole_from_either_side() {
    let quarter = std::f64::consts::PI * 25.0 * 8.0 / 4.0;
    near(quarter, 157.1);
    for (z, cut, kept) in [(20.0, 16.0, 6.0), (0.0, 4.0, 14.0)] {
        let mut s = Session::default();
        box_part(&mut s);
        let ids = points_sketch(&mut s, z, &[[0.0, 0.0]]);
        drill(&mut s, &ids, json!({"depth": 8}));
        near(removed(&mut s), quarter);
        assert!(!solid_at(&s, 2.0, 2.0, cut) && solid_at(&s, 2.0, 2.0, kept), "sketch at z {z}");
    }
}

#[test]
fn pattern_of_a_flipped_hole_and_round_trip() {
    let mut s = Session::default();
    box_part(&mut s);
    let ids = points_sketch(&mut s, 0.0, &[[8.0, 8.0]]);
    drill(&mut s, &ids, json!({"depth": 8}));
    near(removed(&mut s), 628.3);
    run(&mut s, "solid.pattern.rectangular", json!({"features": ["H"], "dir1": [1, 0, 0], "count1": 3, "spacing1": 12, "name": "Row"}));
    near(removed(&mut s), 3.0 * 628.3);
    let before = volume(&mut s);
    let doc = solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap();
    let mut again = Session::new(doc);
    near(volume(&mut again), before);
}

// Two plates of the box, joined by a wall, with an air gap between them: the sketch plane sits on
// the face of one plate and the holes go into that plate, not across the gap into the other one.

/// The 40 x 40 x 20 box with a slot cut through it from z = `lo` to `hi` (a wall stays at x < 5).
fn slotted_part(s: &mut Session, lo: f64, hi: f64) {
    run(s, "solid.box", json!({"length": 40, "width": 40, "height": 20}));
    run(s, "sketch.create", json!({"plane": "XY", "offset": format!("{lo} mm"), "name": "Slot"}));
    run(s, "sketch.rectangle.two_point", json!({"p0": [5, -1], "p1": [45, 41]}));
    run(s, "sketch.finish", json!({}));
    run(s, "solid.extrude", json!({"sketch": "Slot", "distance": format!("{} mm", hi - lo), "operation": "cut", "direction": "positive"}));
}

/// A sketch `Pts` at z = `z` with its normal pointing down (-z), holding one point over (20, 20).
fn down_facing_point(s: &mut Session, z: f64) {
    let plane = json!({"origin": [0, 0, z], "x_dir": [1, 0, 0], "y_dir": [0, -1, 0]});
    run(s, "sketch.create", json!({"plane": plane, "name": "Pts"}));
    run(s, "sketch.point", json!({"point": [20, -20], "id": "p1"}));
    run(s, "sketch.finish", json!({}));
}

fn pi() -> f64 {
    std::f64::consts::PI
}

#[test]
fn plates_sketch_normal_into_the_gap_drills_the_plate_it_touches() {
    let mut s = Session::default();
    slotted_part(&mut s, 6.0, 14.0);
    let base = volume(&mut s);
    down_facing_point(&mut s, 6.0);
    let cb = json!({"type": "counterbore", "diameter": 4, "cb_diameter": 10, "cb_depth": 2, "depth": 9});
    drill(&mut s, &["p1".to_string()], cb);
    near(base - volume(&mut s), pi() * 25.0 * 2.0 + pi() * 4.0 * 4.0);
    assert!(!solid_at(&s, 20.0, 20.0, 5.0) && !solid_at(&s, 20.0, 20.0, 1.0) && solid_at(&s, 20.0, 20.0, 16.0));
}

#[test]
fn plates_sketch_normal_away_from_the_gap_drills_the_plate_it_touches() {
    let mut s = Session::default();
    slotted_part(&mut s, 8.0, 14.0);
    let base = volume(&mut s);
    run(&mut s, "sketch.create", json!({"plane": {"origin": [0, 0, 14], "x_dir": [1, 0, 0], "y_dir": [0, 1, 0]}, "name": "Pts"}));
    run(&mut s, "sketch.point", json!({"point": [20, 20], "id": "p1"}));
    run(&mut s, "sketch.finish", json!({}));
    drill(&mut s, &["p1".to_string()], json!({"depth": 7}));
    near(base - volume(&mut s), pi() * 25.0 * 6.0);
    assert!(!solid_at(&s, 20.0, 20.0, 17.0) && solid_at(&s, 20.0, 20.0, 4.0));
}

#[test]
fn two_separate_bodies_sketch_touching_one_drills_it_not_the_far_one() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 40, "width": 40, "height": 6, "name": "Low"}));
    run(&mut s, "solid.box", json!({"length": 40, "width": 40, "height": 6, "corner": [0, 0, 14], "name": "High"}));
    let base = volume(&mut s);
    near(base, 2.0 * 40.0 * 40.0 * 6.0);
    down_facing_point(&mut s, 6.0);
    drill(&mut s, &["p1".to_string()], json!({"depth": 9}));
    near(base - volume(&mut s), pi() * 25.0 * 6.0);
    assert!(!solid_at(&s, 20.0, 20.0, 3.0) && !solid_at(&s, 20.0, 20.0, 5.9));
    assert!(solid_at(&s, 20.0, 20.0, 15.0) && solid_at(&s, 20.0, 20.0, 19.0));
}

#[test]
fn sketch_floating_in_the_gap_touching_neither_still_drills_down() {
    let mut s = Session::default();
    slotted_part(&mut s, 6.0, 14.0);
    let base = volume(&mut s);
    // Normal +z (the default direction), material ahead on both sides, touching neither.
    points_sketch(&mut s, 10.0, &[[20.0, 20.0]]);
    drill(&mut s, &["pt10_0".to_string()], json!({"depth": 4}));
    near(base - volume(&mut s), pi() * 25.0 * 4.0);
    assert!(!solid_at(&s, 20.0, 20.0, 4.0) && solid_at(&s, 20.0, 20.0, 16.0));
}

/// #2387: a drill point crossing the sloping back of a wall must leave a real hole.
#[test]
fn drilled_sketch_points_break_through_a_sloping_back_face() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": {"origin": [0, 0, 0], "x_dir": [1, 0, 0], "y_dir": [0, 0, 1]}, "name": "Wall"}));
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [15, 0], [15, 30], [7.5, 30], [0, 0]]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch": "Wall", "distance": 40, "direction": "negative"}));
    run(&mut s, "sketch.create", json!({"plane": {"origin": [15, 0, 0], "x_dir": [0, 1, 0], "y_dir": [0, 0, 1]}, "name": "Pts"}));
    run(&mut s, "sketch.line", json!({"points": [[5, 16], [10, 16]], "ids": ["l10"]}));
    run(&mut s, "sketch.point", json!({"point": [30, 16], "id": "p12"}));
    run(&mut s, "sketch.finish", json!({}));
    run(
        &mut s,
        "solid.hole",
        json!({"sketch": "Pts", "points": ["l10.end", "p12"], "depth": "10 mm", "diameter": "5 mm", "type": "drilled", "tip_angle": "118 deg", "name": "H"}),
    );
    let id = s.doc.find_feature("H").unwrap().id;
    let result = s.model.result(id).unwrap();
    assert!(result.error.is_none());
    // The cylindrical bore and cone, less the oblique conical cap outside x = z/4.
    let r = 2.5;
    let k = 59f64.to_radians().tan();
    let tip = r / k;
    let outside = 4.0 - (5.0 - tip);
    let cap = pi() * k * k * outside.powi(3) / (3.0 * (1.0 - (k / 4.0).powi(2)).powf(1.5));
    let expected = 2.0 * (pi() * r * r * (10.0 + tip / 3.0) - cap);
    let left = volume(&mut s);
    // The measured curved faces are tessellated; 0.05 mm³ is 0.013% of the removed material.
    assert!((13_500.0 - left - expected).abs() < 0.05, "removed {} vs {expected}", 13_500.0 - left);
    assert_eq!(s.model.state().bodies.len(), 1);
    for b in &s.model.state().bodies {
        assert!(b.body.validity().is_empty());
        assert_eq!(b.body.shell_count(), 1);
    }
    for y in [10.0, 30.0] {
        assert!(!solid_at(&s, 10.0, y, 16.0), "bore at y = {y}");
        assert!(!solid_at(&s, 4.2, y, 16.0), "tip breaks through at y = {y}");
        assert!(solid_at(&s, 10.0, y + 3.0, 16.0), "material beside the bore at y = {y}");
    }
    let saved = s.doc.to_json();
    run(&mut s, "edit.undo", json!({}));
    near(volume(&mut s), 13_500.0);
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(s.doc.to_json(), saved);
    near(volume(&mut s), left);
    let mut reopened = Session::new(solidcraft_doc::Document::from_json(&saved).unwrap());
    assert!(reopened.model.result(id).unwrap().error.is_none());
    near(volume(&mut reopened), left);
}

/// Back (z 0…6) under a smaller Front (z 14…20), and a sketch on Front's top face with points
/// over Front (`in`) and beyond its edges, over Back (`out1`, `out2`).
fn front_and_back(s: &mut Session) {
    run(s, "solid.box", json!({"length": 40, "width": 40, "height": 6, "body_name": "Back"}));
    run(s, "solid.box", json!({"corner": [10, 10, 14], "length": 20, "width": 20, "height": 6, "body_name": "Front"}));
    run(s, "sketch.create", json!({"plane": {"face": [20, 20, 20]}, "name": "Pts", "project_edges": false}));
    for (id, p) in [("in", [20, 20]), ("out1", [5, 5]), ("out2", [35, 35])] {
        run(s, "sketch.point", json!({"point": p, "id": id}));
    }
    run(s, "sketch.finish", json!({}));
}

/// #2882: holes at the points of a sketch on a body's face drill that body, never one behind it.
#[test]
fn holes_from_a_sketch_on_a_face_drill_that_body_only() {
    let mut s = Session::default();
    front_and_back(&mut s);
    let base = volume(&mut s);
    run(&mut s, "solid.hole", json!({"sketch": "Pts", "points": ["in", "out1", "out2"], "diameter": 5, "depth": 4, "name": "H"}));
    assert_eq!(s.doc.find_feature("H").unwrap().participants, vec!["Front".to_string()]);
    near(base - volume(&mut s), pi() * 6.25 * 4.0);
    assert!(!solid_at(&s, 20.0, 20.0, 18.0), "drilled into Front");
    for (x, y) in [(5.0, 5.0), (35.0, 35.0), (20.0, 20.0)] {
        assert!(solid_at(&s, x, y, 5.5) && solid_at(&s, x, y, 3.0), "Back is not drilled at ({x}, {y})");
    }
    let mut reopened = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    near(base - volume(&mut reopened), pi() * 6.25 * 4.0);
}

/// Through all from Front's face stops at Front; `targets` picks another body to drill instead.
#[test]
fn face_sketch_holes_through_all_stay_in_their_body_unless_targets_say_otherwise() {
    let mut s = Session::default();
    front_and_back(&mut s);
    run(&mut s, "solid.hole", json!({"sketch": "Pts", "points": ["in"], "diameter": 5, "name": "H"}));
    assert!(!solid_at(&s, 20.0, 20.0, 15.0) && solid_at(&s, 20.0, 20.0, 3.0));
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "solid.hole", json!({"sketch": "Pts", "points": ["in"], "diameter": 5, "targets": ["Back"], "name": "H"}));
    assert!(solid_at(&s, 20.0, 20.0, 15.0) && !solid_at(&s, 20.0, 20.0, 3.0));
}

/// A hole starts on a face: a position off every face (here 1 mm past the box's side) is
/// refused at once, instead of drilling where nobody picked; a point on the face still drills.
#[test]
fn a_hole_position_off_every_face_is_refused() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 40, "width": 30, "height": 20}));
    let v0 = volume(&mut s);
    let started = std::time::Instant::now();
    let hole = |x: f64| json!({"position": [x, 15, 20], "diameter": 8, "depth": 10, "type": "drilled", "tip_angle": 118});
    let e = s.execute("solid.hole", &hole(-1.0)).unwrap_err().to_string();
    assert!(e.contains("is not on a face of a body"), "{e}");
    assert!(started.elapsed().as_secs_f64() < 2.0, "refusing took {:?}", started.elapsed());
    // Above the top face, and given a direction: still not on a face.
    let e = s.execute("solid.hole", &json!({"position": [20, 15, 25], "direction": [0, 0, -1], "diameter": 8})).unwrap_err().to_string();
    assert!(e.contains("is not on a face of a body"), "{e}");
    assert_eq!(volume(&mut s), v0, "the refused holes changed nothing");
    run(&mut s, "solid.hole", hole(20.0));
    assert!(volume(&mut s) < v0, "a hole on the face drills");
    assert!(!solid_at(&s, 20.0, 15.0, 15.0));
}

#[test]
fn a_hole_may_start_over_the_mouth_of_an_existing_bore() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 40, "width": 30, "height": 20}));
    run(&mut s, "solid.hole", json!({"position": [20, 15, 20], "diameter": 8, "depth": 10, "type": "drilled", "tip_angle": 118}));
    // A counterbore centred on the bore starts in its mouth, which is in the top face's plane.
    let v1 = volume(&mut s);
    run(&mut s, "solid.hole", json!({"position": [20, 15, 20], "diameter": 12, "depth": 3, "type": "drilled", "tip_angle": 118}));
    assert!(volume(&mut s) < v1, "the wider hole cuts round the bore");
    // Past the face's edge is still off the face.
    let e = s.execute("solid.hole", &json!({"position": [-1, 15, 20], "diameter": 8})).unwrap_err().to_string();
    assert!(e.contains("is not on a face of a body"), "{e}");
}

/// A hole moved off the part cuts nothing and says so (#10701), as a pattern's copies do: by a
/// parameter of its position or a sketch point moved away. Back on the part it is silent.
#[test]
fn a_hole_moved_off_the_part_says_it_removes_no_material() {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "hx", "expression": "10 mm"}));
    run(&mut s, "solid.box", json!({"length": 40, "width": 30, "height": 10, "name": "Plate"}));
    run(&mut s, "solid.hole", json!({"position": ["hx", "10 mm", "10 mm"], "diameter": 5, "name": "H1"}));
    assert_eq!(said(&s, "H1"), "");
    change(&mut s, "hx", "100 mm");
    assert!((volume(&mut s) - 12_000.0).abs() < 1e-6, "nothing is cut");
    assert!(said(&s, "H1").contains("the hole removes no material"), "{}", said(&s, "H1"));
    change(&mut s, "hx", "30 mm");
    assert_eq!(said(&s, "H1"), "");
    // Holes at two sketch points, one of them moved off the plate.
    run(&mut s, "sketch.create", json!({"plane": "XY", "offset": 10, "name": "HP"}));
    run(&mut s, "sketch.point", json!({"point": [10, 20], "id": "a"}));
    run(&mut s, "sketch.point", json!({"point": [20, 20], "id": "b"}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.hole", json!({"sketch": "HP", "points": ["a", "b"], "diameter": 4, "name": "H2"}));
    assert_eq!(said(&s, "H2"), "");
    run(&mut s, "sketch.edit", json!({"sketch": "HP"}));
    run(&mut s, "sketch.move_point", json!({"point": "a", "to": [100, 100]}));
    run(&mut s, "sketch.finish", json!({}));
    assert!(said(&s, "H2").contains("1 of 2 holes remove no material"), "{}", said(&s, "H2"));
}

/// A hole wider than the part takes all of it away and says so (#10702): at creation, and when
/// a parameter widens it later. A hole across the part's width keeps the rest, silently.
#[test]
fn a_hole_wider_than_the_part_says_it_removes_all_of_it() {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "hd", "expression": "40 mm"}));
    run(&mut s, "solid.box", json!({"length": 40, "width": 30, "height": 10, "name": "Plate"}));
    run(&mut s, "solid.hole", json!({"position": [10, 10, 10], "diameter": "hd", "name": "H1"}));
    assert_eq!(said(&s, "H1"), "");
    assert_eq!(s.model.state().bodies.len(), 1);
    change(&mut s, "hd", "100 mm");
    assert!(s.model.state().bodies.is_empty());
    assert!(said(&s, "H1").contains("the hole removes all of Body1"), "{}", said(&s, "H1"));
    change(&mut s, "hd", "5 mm");
    assert_eq!(said(&s, "H1"), "");
}

/// A hole across the part's width that touches none of its sides keeps the rest, in one piece and
/// silently, and says so once a parameter widens it past the whole part. The same hole touching a
/// side along a line is the case above.
#[test]
fn a_hole_across_the_part_clear_of_its_sides_keeps_the_rest_silently() {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "hd", "expression": "40 mm"}));
    run(&mut s, "solid.box", json!({"length": 40, "width": 30, "height": 10, "name": "Plate"}));
    run(&mut s, "solid.hole", json!({"position": [10, 15, 10], "diameter": "hd", "name": "H1"}));
    assert_eq!(said(&s, "H1"), "");
    assert_eq!(s.model.state().bodies.len(), 1);
    // The plate less the part of the 40 mm circle inside it, 10 mm deep.
    assert!((volume(&mut s) - 3623.4382).abs() < 1e-3, "{}", volume(&mut s));
    change(&mut s, "hd", "100 mm");
    assert!(s.model.state().bodies.is_empty());
    assert!(said(&s, "H1").contains("the hole removes all of Body1"), "{}", said(&s, "H1"));
    change(&mut s, "hd", "5 mm");
    assert_eq!(said(&s, "H1"), "");
}

/// The volume of one body.
fn body_volume(s: &mut Session, name: &str) -> f64 {
    let m = run(s, "inspect.measure", json!({}));
    let b = m["bodies"].as_array().unwrap().iter().find(|b| b["name"] == name).unwrap_or_else(|| panic!("no body {name}: {m}"));
    b["volume_mm3"].as_f64().unwrap()
}

/// A Block (x 0…20) made first, then a 3 mm Plate (x −3…0) whose front face touches the Block's
/// back face round (0, 30, 20).
fn plate_behind_block(s: &mut Session) {
    run(s, "solid.box", json!({"length": 20, "width": 40, "height": 30, "corner": [0, 10, 5], "body_name": "Block", "operation": "new"}));
    run(s, "solid.box", json!({"length": 3, "width": 60, "height": 40, "corner": [-3, 0, 0], "body_name": "Plate", "operation": "new"}));
}

/// The Plate with a 3.4 mm hole through it at (y 30, z 20), and the Block untouched.
fn plate_drilled_through(s: &mut Session) {
    near(body_volume(s, "Plate"), 7200.0 - pi() * 1.7 * 1.7 * 3.0);
    near(body_volume(s, "Block"), 24_000.0);
    assert!(!solid_at(s, -2.5, 30.0, 20.0), "the hole comes out of the Plate's back");
    assert!(solid_at(s, 0.5, 30.0, 20.0), "the Block is not drilled");
}

/// #12066: a hole on a face that another part touches there drills the part it names. Both faces
/// pass through the point, and the Block, made first, used to win: the hole drilled away from
/// the Plate and cut only a shallow pocket into it, without a word.
#[test]
fn a_hole_on_a_face_another_part_touches_drills_the_part_it_names() {
    let mut s = Session::default();
    plate_behind_block(&mut s);
    run(&mut s, "solid.hole", json!({"position": [0, 30, 20], "targets": ["Plate"], "diameter": 3.4, "name": "H"}));
    assert!(hole_direction(&s, "H").x < -0.999, "drills along -X: {:?}", hole_direction(&s, "H"));
    assert_eq!(said(&s, "H"), "");
    plate_drilled_through(&mut s);
}

/// #12066: a design saved with that hole's direction pointing into the Block (as older builds
/// saved it) drills the Plate when it is opened: the direction is mirrored into the Plate's face,
/// as format 18 does for one taken from a thin plate's far side.
#[test]
fn a_saved_hole_turned_towards_a_touching_part_drills_its_own_part_when_opened() {
    let mut s = Session::default();
    plate_behind_block(&mut s);
    run(&mut s, "solid.hole", json!({"position": [0, 30, 20], "targets": ["Plate"], "diameter": 3.4, "name": "H"}));
    let mut saved: Value = serde_json::from_str(&s.doc.to_json()).unwrap();
    let hole = saved["features"].as_array_mut().unwrap().iter_mut().find(|f| f["name"] == "H").unwrap();
    hole["direction"] = json!([1.0, 0.0, 0.0]);
    let mut opened = Session::new(solidcraft_doc::Document::from_json(&saved.to_string()).unwrap());
    assert!((hole_direction(&opened, "H").x - 1.0).abs() < 1e-9, "the design keeps the direction it was saved with");
    assert_eq!(said(&opened, "H"), "");
    plate_drilled_through(&mut opened);
}

fn hole_direction(s: &Session, name: &str) -> Vec3 {
    match &s.doc.find_feature(name).unwrap().kind {
        solidcraft_doc::FeatureKind::Hole { direction, .. } => direction.0,
        other => panic!("{name} is not a hole: {}", other.type_name()),
    }
}

/// A hole placed on a Cover's top to drill the Base under it (`targets` leaves the Cover out)
/// still goes by the Cover's face, the only face there: it drills down through the Base, made
/// now or opened from a design that saved the direction up (as format 18 repairs).
#[test]
fn a_hole_placed_on_another_parts_face_drills_its_own_part_behind_it() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 40, "width": 40, "height": 10, "body_name": "Base"}));
    run(&mut s, "solid.box", json!({"corner": [10, 10, 10], "length": 20, "width": 20, "height": 4, "body_name": "Cover", "operation": "new"}));
    run(&mut s, "solid.hole", json!({"position": [20, 20, 14], "targets": ["Base"], "diameter": 5, "name": "H"}));
    let drilled_base_only = |s: &mut Session| {
        assert_eq!(said(s, "H"), "");
        near(body_volume(s, "Base"), 16_000.0 - pi() * 6.25 * 10.0);
        near(body_volume(s, "Cover"), 1600.0);
    };
    assert!(hole_direction(&s, "H").z < -0.999, "{:?}", hole_direction(&s, "H"));
    drilled_base_only(&mut s);
    let mut saved: Value = serde_json::from_str(&s.doc.to_json()).unwrap();
    let hole = saved["features"].as_array_mut().unwrap().iter_mut().find(|f| f["name"] == "H").unwrap();
    hole["direction"] = json!([0.0, 0.0, 1.0]);
    let mut opened = Session::new(solidcraft_doc::Document::from_json(&saved.to_string()).unwrap());
    assert!(hole_direction(&opened, "H").z > 0.999);
    drilled_base_only(&mut opened);
}

/// #11538: a sketch on an offset plane inside the part is where the hole starts, not the part's
/// top face above it. A 5 mm blind hole from z 12 removes z 7..12.
#[test]
fn hole_from_a_sketch_on_an_offset_plane_inside_the_part_starts_at_the_plane() {
    let mut s = Session::default();
    box_part(&mut s);
    run(&mut s, "sketch.create", json!({"plane": "XY", "offset": "12 mm", "name": "Pts"}));
    run(&mut s, "sketch.point", json!({"point": [20, 20], "id": "p1"}));
    run(&mut s, "sketch.finish", json!({}));
    let base = volume(&mut s);
    drill(&mut s, &["p1".to_string()], json!({"depth": 5}));
    // The tool runs a hair past its start, so the walls are checked, not the volume.
    assert!(solid_at(&s, 20.0, 20.0, 6.5) && !solid_at(&s, 20.0, 20.0, 7.5) && !solid_at(&s, 20.0, 20.0, 11.5));
    assert!(solid_at(&s, 20.0, 20.0, 14.5) && solid_at(&s, 20.0, 20.0, 18.0) && base - volume(&mut s) < pi() * 25.0 * 6.0);
    // Nothing above the plane goes: the tool's reach above a face is not used inside the part.
    assert!(solid_at(&s, 20.0, 20.0, 12.05) && solid_at(&s, 20.0, 20.0, 12.3), "the material just above the plane stays");
}

/// #11538: a sketch plane just under the top face (z 19.95 in a 20 mm part) is still a start
/// inside the part: the tool's reach above a face, wider than the 0.05 mm roof, does not open it.
#[test]
fn hole_from_a_sketch_plane_just_under_the_top_face_keeps_the_roof() {
    let mut s = Session::default();
    box_part(&mut s);
    run(&mut s, "sketch.create", json!({"plane": "XY", "offset": "19.95 mm", "name": "Pts"}));
    run(&mut s, "sketch.point", json!({"point": [20, 20], "id": "p1"}));
    run(&mut s, "sketch.finish", json!({}));
    drill(&mut s, &["p1".to_string()], json!({"depth": 5}));
    assert!(!solid_at(&s, 20.0, 20.0, 17.0) && !solid_at(&s, 20.0, 20.0, 19.9), "the hole is drilled below the plane");
    assert!(solid_at(&s, 20.0, 20.0, 19.98), "the roof above the plane stays");
}

/// A sketch on a standard plane through the middle of a part keeps drilling from the part's
/// surface, as before #11538 limited the new rule to derived planes.
#[test]
fn hole_from_a_sketch_on_a_standard_plane_inside_the_part_still_starts_at_the_surface() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 40, "width": 40, "height": 20, "corner": [-20, -20, -10], "name": "Part"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Pts"}));
    run(&mut s, "sketch.point", json!({"point": [0, 0], "id": "p1"}));
    run(&mut s, "sketch.finish", json!({}));
    drill(&mut s, &["p1".to_string()], json!({"depth": 5}));
    // Starts at the top (z 10) going down, not at z 0.
    assert!(!solid_at(&s, 0.0, 0.0, 7.5) && solid_at(&s, 0.0, 0.0, 3.0) && solid_at(&s, 0.0, 0.0, -3.0));
}

/// The copies of a hole from a sketch on an offset plane start at the plane too (#11538).
#[test]
fn patterned_copy_of_an_offset_plane_hole_starts_at_the_plane_too() {
    let mut s = Session::default();
    box_part(&mut s);
    run(&mut s, "sketch.create", json!({"plane": "XY", "offset": "12 mm", "name": "Pts"}));
    run(&mut s, "sketch.point", json!({"point": [10, 20], "id": "p1"}));
    run(&mut s, "sketch.finish", json!({}));
    drill(&mut s, &["p1".to_string()], json!({"depth": 5}));
    run(&mut s, "solid.pattern.rectangular", json!({"features": ["H"], "dir1": [1, 0, 0], "count1": 2, "spacing1": 15, "name": "Row"}));
    for x in [10.0, 25.0] {
        assert!(solid_at(&s, x, 20.0, 6.5) && !solid_at(&s, x, 20.0, 7.5) && !solid_at(&s, x, 20.0, 11.5) && solid_at(&s, x, 20.0, 14.5), "x {x}");
    }
}

/// A design saved before #11538 keeps its hole from a sketch on an offset plane inside the part
/// where it was: starting at the top face (z 20) and 5 mm deep, also after an edit.
#[test]
fn a_saved_offset_plane_hole_keeps_starting_at_the_surface() {
    let mut s = Session::default();
    box_part(&mut s);
    run(&mut s, "sketch.create", json!({"plane": "XY", "offset": "12 mm", "name": "Pts"}));
    run(&mut s, "sketch.point", json!({"point": [20, 20], "id": "p1"}));
    run(&mut s, "sketch.finish", json!({}));
    drill(&mut s, &["p1".to_string()], json!({"depth": 5}));
    // As the release before saved it: the last folded format, none of today's pending changes.
    let mut v: Value = serde_json::from_str(&s.doc.to_json()).unwrap();
    v["format"] = json!("solidcraft/50");
    v.as_object_mut().unwrap().remove("format_pending");
    let mut opened = Session::new(solidcraft_doc::Document::from_json(&v.to_string()).unwrap());
    let from_surface = |s: &Session| {
        !solid_at(s, 20.0, 20.0, 19.0) && !solid_at(s, 20.0, 20.0, 15.5) && solid_at(s, 20.0, 20.0, 14.0) && solid_at(s, 20.0, 20.0, 11.5)
    };
    assert!(from_surface(&opened));
    run(&mut opened, "timeline.edit", json!({"feature": "H", "set": {"diameter": 8}}));
    assert!(from_surface(&opened), "an edited saved hole still starts at the surface");
}

/// #10697: a countersink's cone stops at the hole's depth. A 3.1 mm deep countersunk hole in a
/// 3.2 mm plate leaves its 0.1 mm floor (the cone used to run on to 90 % of the hole radius and
/// open the bottom); a hole shallower than its countersink is cut to the hole's depth only.
#[test]
fn a_countersink_stops_at_the_depth_of_its_hole() {
    let plate = |s: &mut Session| run(s, "solid.box", json!({"length": 40, "width": 40, "height": 3.2, "center": [0, 0, 1.6], "body_name": "Block"}));
    let hole = |depth: f64| json!({"position": [0, 0, 3.2], "diameter": 6, "depth": depth, "type": "countersink", "cs_diameter": 12, "cs_angle": 90, "name": "CS"});
    let mut s = Session::default();
    plate(&mut s);
    run(&mut s, "solid.hole", hole(3.1));
    assert!(solid_at(&s, 0.0, 0.0, 0.05), "the 0.1 mm floor is still there");
    assert!(!solid_at(&s, 0.0, 0.0, 0.2), "the hole is open above its floor");
    // Depth 2: the cone is cut at 2 mm, a frustum from radius 6 to 4: pi h / 3 (R^2 + R r + r^2).
    let mut s = Session::default();
    plate(&mut s);
    run(&mut s, "solid.hole", hole(2.0));
    let frustum = std::f64::consts::PI * 2.0 / 3.0 * (36.0 + 24.0 + 16.0);
    let expected = 40.0 * 40.0 * 3.2 - frustum;
    let v = volume(&mut s);
    assert!((v - expected).abs() < 1e-3 * expected, "{v} vs {expected}");
}

/// #17873: a hole drilled To Object on the far side of a 3 mm wall comes out through that side
/// whole. The sides lean 1e-5 rad apart (as a sketch can leave them), so the hole's floor, square
/// to its axis on the near side, used to cross the far side and the cut left half of it across
/// the exit. A drilled hole to that side comes out as a through hole too: its bore runs on past
/// the exit, and neither it nor a drill point reaches the plate standing 0.5 mm beyond.
#[test]
fn a_hole_to_the_far_side_of_a_wall_opens_the_whole_exit() {
    let (near_side, far_side) = (3.1e-6, 1.31e-5);
    let wall = |s: &mut Session| {
        run(s, "sketch.create", json!({"plane": "XY", "name": "Wall"}));
        let corners = [[0.0, 5.5], [40.0, 5.5 + 40.0 * near_side], [40.0, 8.5 + 40.0 * far_side], [0.0, 8.5]];
        run(s, "sketch.line", json!({"points": corners, "closed": true}));
        run(s, "sketch.finish", json!({}));
        run(s, "solid.extrude", json!({"sketch": "Wall", "distance": 30, "body_name": "Body1"}));
    };
    let hole = |kind: Value| {
        let mut h = json!({"position": [20, 5.5 + 20.0 * near_side, 15], "to": [20, 8.5 + 20.0 * far_side, 15], "diameter": 8, "name": "Hole2"});
        h.as_object_mut().unwrap().extend(kind.as_object().unwrap().clone());
        h
    };
    let mut s = Session::default();
    wall(&mut s);
    run(&mut s, "solid.hole", hole(json!({"type": "countersink", "cs_diameter": 10, "cs_angle": 90})));
    assert_eq!(said(&s, "Hole2"), "");
    let faces = run(&mut s, "model.faces", json!({"body": "Body1"}))["faces"].as_array().cloned().unwrap();
    let floor: Vec<_> = faces.iter().filter(|f| f["name"].as_str().is_some_and(|n| n.ends_with(":bore:base"))).collect();
    assert!(floor.is_empty(), "no floor is left across the exit: {floor:?}");
    // Areas are measured on the faces' tessellation: the whole Ø8 exit takes about 49 mm² of the
    // far side's 1200, half of it about 24.5.
    let side: f64 = faces.iter().filter(|f| f["plane_normal"][1].as_f64().is_some_and(|n| n > 0.99)).map(|f| f["area"].as_f64().unwrap()).sum();
    assert!(side < 1200.0 - 45.0, "the far side lost the whole exit: {side} mm² left");

    let mut s = Session::default();
    wall(&mut s);
    run(&mut s, "solid.box", json!({"length": 40, "width": 3, "height": 30, "center": [20, 10.5, 15], "body_name": "Beyond"}));
    run(&mut s, "solid.hole", hole(json!({"type": "drilled", "tip_angle": 118})));
    assert_eq!(said(&s, "Hole2"), "");
    let faces = run(&mut s, "model.faces", json!({"body": "Body1"}))["faces"].as_array().cloned().unwrap();
    assert!(!faces.iter().any(|f| f["name"].as_str().is_some_and(|n| n.ends_with(":bore:base"))), "a through bore: {faces:?}");
    near(body_volume(&mut s, "Body1"), 3600.0 - 48.0 * std::f64::consts::PI);
    near(body_volume(&mut s, "Beyond"), 3600.0);
}

#[test]
fn a_construction_point_is_not_a_hole_site_and_the_flag_is_saved() {
    // A point marked construction (#18448) only helps place other geometry: a Hole from the whole
    // sketch skips it. Turned back to a normal point it drills, and the flag is saved.
    let mut s = Session::default();
    box_part(&mut s);
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Pts"}));
    run(&mut s, "sketch.point", json!({"point": [8, 8], "id": "guide"}));
    run(&mut s, "sketch.point", json!({"point": [20, 20], "id": "mine"}));
    let marked = run(&mut s, "sketch.construction", json!({"points": ["guide"]}));
    assert_eq!((marked["changed"].as_u64(), marked["construction"].as_bool()), (Some(1), Some(true)), "{marked}");
    run(&mut s, "sketch.finish", json!({}));
    let saved = s.doc.to_json();
    let reopened = solidcraft_doc::Document::from_json(&saved).unwrap();
    let sketch_id = s.model.state().sketches.first().map(|k| k.feature).expect("the sketch");
    let flags = |d: &solidcraft_doc::Document| d.sketch(sketch_id).unwrap().points.iter().map(|p| (p.id.clone(), p.construction)).collect::<Vec<_>>();
    assert!(flags(&reopened).contains(&("guide".into(), true)) && flags(&reopened).contains(&("mine".into(), false)), "{:?}", flags(&reopened));
    run(&mut s, "solid.hole", json!({"sketch": "Pts", "diameter": 10, "depth": 8, "name": "H"}));
    near(removed(&mut s), 628.3);
    assert!(!solid_at(&s, 20.0, 20.0, 4.0) && solid_at(&s, 8.0, 8.0, 4.0));
    // Back to a normal point: a hole made now drills it too (the one already made keeps its sites).
    run(&mut s, "sketch.edit", json!({"sketch": "Pts"}));
    let back = run(&mut s, "sketch.construction", json!({"points": ["guide"], "value": false}));
    assert_eq!(back["construction"].as_bool(), Some(false), "{back}");
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.hole", json!({"sketch": "Pts", "diameter": 4, "depth": 8, "name": "H2"}));
    assert!(!solid_at(&s, 8.0, 8.0, 4.0), "the point drills again");
    // The origin cannot be made construction, and a point that is not there is refused.
    run(&mut s, "sketch.edit", json!({"sketch": "Pts"}));
    assert_eq!(run(&mut s, "sketch.construction", json!({"points": ["origin"]}))["changed"], 0);
    assert!(s.execute("sketch.construction", &json!({"points": ["nope"]})).is_err());
    // The origin in the selection does not decide the switch: guide is normal again, and a
    // repeat makes it construction again.
    let flip = run(&mut s, "sketch.construction", json!({"points": ["origin", "guide"]}));
    assert_eq!(flip["construction"].as_bool(), Some(true), "{flip}");
    let flip = run(&mut s, "sketch.construction", json!({"points": ["origin", "guide"]}));
    assert_eq!(flip["construction"].as_bool(), Some(false), "{flip}");
}

/// A counterbore drilled up from a sketch below the part into a face that slopes at 40° across the
/// axis opens the whole slanted face (#20045). The face rises 11 tan 40° = 9.2 mm over the
/// counterbore's radius above the point where the axis enters it; the tools ran on only about 1 mm
/// past that point and left a wedge over the low side, half closing the bore. Both tools now start
/// clear of the face, so the cut is the two plain cylinders' from the face: the slanted face is
/// linear, so over each tool's disc it averages to its height on the axis.
#[test]
fn a_counterbore_into_a_slanted_face_opens_the_whole_face() {
    let mut s = Session::default();
    let rise = 60.0 * 40f64.to_radians().tan();
    run(&mut s, "sketch.create", json!({"plane": "XZ"}));
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [60, -rise], [60, -80], [0, -80]], "closed": true}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance": 40}));
    let block = volume(&mut s);
    near(block, (60.0 * 80.0 - 60.0 * rise / 2.0) * 40.0);
    let ids = points_sketch(&mut s, 0.0, &[[30.0, 20.0]]);
    drill(&mut s, &ids, json!({"type": "counterbore", "diameter": 10.5, "cb_diameter": 22, "cb_depth": 20}));
    assert_eq!(said(&s, "H"), "");
    // The axis enters the slanted face at z = rise / 2; the counterbore's floor is 20 above that.
    let entry = rise / 2.0;
    let floor = entry + 20.0;
    let expected = std::f64::consts::PI * (11.0f64.powi(2) * 20.0 + 5.25f64.powi(2) * (80.0 - floor));
    let cut = block - volume(&mut s);
    assert!((cut - expected).abs() < 0.05, "removed {cut} mm³, the two cylinders from the face {expected} mm³");
    // Air on the low side of the counterbore and of the bore, just above the slanted face there.
    for x in [20.5, 26.0] {
        let face = rise * x / 60.0;
        assert!(!solid_at(&s, x, 20.0, face + 0.5), "material over the slanted face at x = {x}");
    }
}

/// #20245: a hole drilled from a sketch point on a plane turned about a ring's axis cuts through.
#[test]
fn a_hole_drilled_from_a_sketch_on_a_turned_plane_cuts_through() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Ring sketch"}));
    run(&mut s, "sketch.circle.center", json!({"center": [0, 0], "radius": 150}));
    run(&mut s, "sketch.circle.center", json!({"center": [0, 0], "radius": 146}));
    run(&mut s, "sketch.finish", json!({}));
    run(
        &mut s,
        "solid.extrude",
        json!({"sketch": "Ring sketch", "profiles": [{"point": [148, 0]}], "distance": "20 mm", "operation": "new", "body_name": "Ring"}),
    );
    run(&mut s, "construct.plane.at_angle", json!({"base": "YZ", "axis": "Z", "angle": "60 deg", "name": "Turned plane"}));
    run(&mut s, "construct.plane.offset", json!({"base": "Turned plane", "offset": "150 mm", "name": "Tangent plane"}));
    run(&mut s, "sketch.create", json!({"plane": "Tangent plane", "name": "Hole sketch"}));
    run(&mut s, "sketch.point", json!({"point": [0, 10]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.hole", json!({"sketch": "Hole sketch", "diameter": "6 mm", "depth": "7 mm", "targets": ["Ring"], "name": "Hole"}));
    let vol = volume(&mut s);
    assert!((vol - 74279.81).abs() < 0.1, "volume {vol} should match ring minus hole without skin");
    let faces = run(&mut s, "model.faces", json!({"body": "Ring"}))["faces"].as_array().cloned().unwrap();
    assert_eq!(faces.len(), 5, "expected 5 faces, got {}", faces.len());
}

/// #20424: a hole drilled from a tube's axis out through its wall starts on the bore itself, not
/// on the bore's display mesh, whose facets lie inside it by their chord: drilled there, the tool
/// left the material between the facets and the bore. Two Ø4 through holes opposite each other,
/// one drilled, the other its mirror or drilled again from the other side: each removes the
/// cylinder's piece of the wall, the integral over its disc of the wall's length along its axis
/// (the drill point is in the air past the tube), and the tube keeps 4 + 2 faces.
#[test]
fn holes_drilled_out_from_a_tubes_axis_start_on_its_bore() {
    let (ro, ri, r, len) = (30.0_f64, 28.5_f64, 2.0_f64, 900.0_f64);
    // Over the disc w² + z² <= r², the wall's length along the axis at w; w = r sin t.
    let n = 20_000;
    let step = std::f64::consts::PI / n as f64;
    let one: f64 = (0..n)
        .map(|i| {
            let t = -std::f64::consts::FRAC_PI_2 + (i as f64 + 0.5) * step;
            let w = r * t.sin();
            2.0 * r * t.cos() * ((ro * ro - w * w).sqrt() - (ri * ri - w * w).sqrt()) * r * t.cos() * step
        })
        .sum();
    let expected = std::f64::consts::PI * (ro * ro - ri * ri) * len - 2.0 * one;
    let s2 = std::f64::consts::FRAC_1_SQRT_2;
    let sketch = |s: &mut Session, x: f64, name: &str| {
        run(s, "sketch.create", json!({"plane": {"origin": [0, 0, 0], "x_dir": [x, x, 0], "y_dir": [0, 0, 1]}, "name": name}));
        run(s, "sketch.point", json!({"point": [0, 0]}));
        run(s, "sketch.finish", json!({}));
    };
    for mirrored in [true, false] {
        let mut s = Session::default();
        run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Rings"}));
        run(&mut s, "sketch.circle.center", json!({"center": [0, 0], "radius": ri}));
        run(&mut s, "sketch.circle.center", json!({"center": [0, 0], "radius": ro}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "solid.extrude", json!({"sketch": "Rings", "distance": 450, "distance2": 450, "body_name": "Tube"}));
        sketch(&mut s, s2, "Out");
        run(&mut s, "solid.hole", json!({"sketch": "Out", "diameter": "4 mm", "type": "drilled", "tip_angle": "118 deg", "name": "H1"}));
        if mirrored {
            run(&mut s, "solid.mirror", json!({"features": ["H1"], "plane": {"origin": [0, 0, 0], "normal": [-s2, s2, 0]}}));
        } else {
            sketch(&mut s, -s2, "Back");
            run(&mut s, "solid.hole", json!({"sketch": "Back", "diameter": "4 mm", "type": "drilled", "tip_angle": "118 deg", "name": "H2"}));
        }
        let vol = volume(&mut s);
        assert!((vol - expected).abs() < 1e-3, "mirrored {mirrored}: volume {vol}, expected {expected}");
        let faces = run(&mut s, "model.faces", json!({"body": "Tube"}))["faces"].as_array().map_or(0, Vec::len);
        assert_eq!(faces, 6, "mirrored {mirrored}: the tube's 4 faces and one bore per hole");
    }
}
