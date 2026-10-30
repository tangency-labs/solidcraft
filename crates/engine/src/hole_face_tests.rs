//! A hole placed on a face follows that face along the hole's axis, never sideways: when the face
//! moves the hole moves with it, and when the face no longer reaches the place the hole stays
//! where it was put and says so instead of sliding to the face's nearest point.

use serde_json::{Value, json};
use solidcraft_geom::Vec3;

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
