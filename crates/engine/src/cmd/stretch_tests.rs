//! `solid.stretch` (#20512): a 40 x 30 x 20 mm box, its sizes derived by hand.
#![allow(clippy::unwrap_used, clippy::panic)]

use crate::Session;
use serde_json::{Value, json};
use solidcore_record::Vec3;
use solidcraft_doc::Document;
use solidcraft_geom::prelude::*;

fn run(s: &mut Session, command: &str, params: Value) -> Value {
    s.execute(command, &params).unwrap_or_else(|error| panic!("{command}: {error}"))
}

fn block() -> Session {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 40, "width": 30, "height": 20, "body_name": "Block"}));
    s
}

fn size(s: &Session) -> (Vec3, Vec3, f64) {
    assert!(s.model.results.iter().all(|r| r.error.is_none()), "{:?}", s.model.results);
    let m = solidcraft_kernel::measure(&s.model.state().body("Block").unwrap().body).unwrap();
    (m.bbox.min, m.bbox.max, m.volume)
}

#[test]
fn a_stretch_along_x_makes_the_box_longer_and_the_left_side_stays_put() {
    let mut s = block();
    let v = run(&mut s, "solid.stretch", json!({"body": "Block", "direction": "+x", "distance": "15 mm"}));
    assert_eq!(v["changed_bodies"], json!(["Block"]));
    let (min, max, volume) = size(&s);
    assert!(min.dist(Vec3::ZERO) < 1e-9 && max.dist(Vec3::new(55.0, 30.0, 20.0)) < 1e-6, "{min:?} {max:?}");
    assert!((volume - 55.0 * 30.0 * 20.0).abs() < 1e-6, "{volume}");
    // Undo, redo, and the saved design, all give the same body.
    let saved = s.doc.to_json();
    run(&mut s, "edit.undo", json!({}));
    assert!((size(&s).1.x - 40.0).abs() < 1e-9);
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(s.doc.to_json(), saved);
    let reopened = Session::new(Document::from_json(&saved).unwrap());
    assert!((size(&reopened).1.x - 55.0).abs() < 1e-6);
}

#[test]
fn a_stretch_toward_minus_z_keeps_the_top_and_a_shrink_takes_a_slice_out() {
    let mut s = block();
    run(&mut s, "solid.stretch", json!({"body": "Block", "direction": "-z", "at": 5, "distance": 10}));
    let (min, max, _) = size(&s);
    assert!((min.z + 10.0).abs() < 1e-6 && (max.z - 20.0).abs() < 1e-6, "{min:?} {max:?}");
    run(&mut s, "solid.stretch", json!({"body": "Block", "direction": "+y", "distance": -12}));
    let (min, max, volume) = size(&s);
    assert!((min.y - 0.0).abs() < 1e-6 && (max.y - 18.0).abs() < 1e-6, "{min:?} {max:?}");
    assert!((volume - 40.0 * 18.0 * 30.0).abs() < 1e-6, "{volume}");
}

#[test]
fn the_stretch_follows_its_parameter() {
    let mut s = block();
    run(&mut s, "parameters.add", json!({"name": "extra", "expression": "10 mm"}));
    run(&mut s, "solid.stretch", json!({"body": "Block", "direction": "+x", "distance": "extra"}));
    assert!((size(&s).1.x - 50.0).abs() < 1e-6);
    run(&mut s, "parameters.change", json!({"name": "extra", "expression": "25 mm"}));
    assert!((size(&s).1.x - 65.0).abs() < 1e-6);
    run(&mut s, "parameters.change", json!({"name": "extra", "expression": "-5 mm"}));
    assert!((size(&s).1.x - 35.0).abs() < 1e-6);
}

#[test]
fn bad_input_is_refused_and_leaves_the_design_as_it_was() {
    let mut s = block();
    let before = s.doc.to_json();
    for p in [
        json!({"body": "Block", "direction": "sideways", "distance": 5}),
        json!({"body": "Block", "direction": "+x"}),
        json!({"body": "Nothing", "direction": "+x", "distance": 5}),
        json!({"body": "Block", "direction": "+x", "distance": "nonsense"}),
        json!({"direction": "+x", "distance": 5}),
    ] {
        assert!(s.execute("solid.stretch", &p).is_err(), "{p}");
        assert_eq!(s.doc.to_json(), before, "{p}");
    }
    // A shrink of more than lies in front of the cut is the feature's error, with why.
    let err = s.execute("solid.stretch", &json!({"body": "Block", "direction": "+x", "at": 30, "distance": -15})).unwrap_err().to_string();
    assert!(err.contains("takes out everything in front of the cut plane"), "{err}");
    assert_eq!(s.doc.to_json(), before);
}

#[test]
fn a_cut_measured_from_a_picked_face_follows_that_face() {
    let mut s = block();
    // A 10 mm boss on top (z 20…30) over x 0…10: the plain stretch along +X is past x = 10.
    run(&mut s, "solid.box", json!({"corner": [0, 0, 20], "length": 10, "width": 30, "height": 10, "body_name": "Boss"}));
    run(&mut s, "solid.combine", json!({"target": "Block", "tools": ["Boss"], "operation": "join"}));
    // Cut 5 mm past the boss's side face at x = 10: x = 15, clear of the boss.
    run(&mut s, "solid.stretch", json!({"body": "Block", "direction": "+x", "from": [10, 15, 25], "at": 5, "distance": 10}));
    let (_, max, volume) = size(&s);
    assert!((max.x - 50.0).abs() < 1e-6, "{max:?}");
    assert!((volume - (50.0 * 30.0 * 20.0 + 10.0 * 30.0 * 10.0)).abs() < 1e-6, "{volume}");
    // The same cut measured from the side that stays put (x = 0) gives the same body.
    let mut other = block();
    run(&mut other, "solid.box", json!({"corner": [0, 0, 20], "length": 10, "width": 30, "height": 10, "body_name": "Boss"}));
    run(&mut other, "solid.combine", json!({"target": "Block", "tools": ["Boss"], "operation": "join"}));
    run(&mut other, "solid.stretch", json!({"body": "Block", "direction": "+x", "at": 15, "distance": 10}));
    assert!((size(&other).2 - volume).abs() < 1e-6);
}

#[test]
fn one_step_stretches_along_two_axes_and_refuses_two_along_one() {
    let mut s = block();
    run(
        &mut s,
        "solid.stretch",
        json!({"body": "Block", "stretches": [{"direction": "+x", "distance": 15}, {"direction": "-y", "at": 10, "distance": 5}]}),
    );
    let (min, max, volume) = size(&s);
    assert!(min.dist(Vec3::new(0.0, -5.0, 0.0)) < 1e-6 && max.dist(Vec3::new(55.0, 30.0, 20.0)) < 1e-6, "{min:?} {max:?}");
    assert!((volume - 55.0 * 35.0 * 20.0).abs() < 1e-6, "{volume}");
    assert_eq!(s.doc.features.iter().filter(|f| matches!(f.kind, solidcraft_doc::FeatureKind::Stretch { .. })).count(), 1);
    let before = s.doc.to_json();
    for p in [
        json!({"body": "Block", "stretches": [{"direction": "+x", "distance": 5}, {"direction": "-x", "distance": 5}]}),
        json!({"body": "Block", "stretches": []}),
        json!({"body": "Block", "stretches": "x"}),
    ] {
        assert!(s.execute("solid.stretch", &p).is_err(), "{p}");
        assert_eq!(s.doc.to_json(), before, "{p}");
    }
}

/// A 100 x 60 rectangle on XY with its left side fixed, its bottom dimensioned 100 and its
/// right side 60, extruded 10 into `Plate`.
fn plate() -> Session {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Plate"}));
    let sides = run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [100, 60]}))["sides"].clone();
    run(&mut s, "sketch.dimension", json!({"entities": [sides["bottom"]], "value": 100}));
    run(&mut s, "sketch.dimension", json!({"entities": [sides["right"]], "value": 60}));
    run(&mut s, "sketch.constraint.fix", json!({"entity": sides["left"], "fixed": true}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch": "Plate", "distance": 10, "body_name": "Plate"}));
    s
}

fn plate_box(s: &Session) -> (Vec3, Vec3) {
    assert!(s.model.results.iter().all(|r| r.error.is_none()), "{:?}", s.model.results);
    let m = solidcraft_kernel::measure(&s.model.state().body("Plate").unwrap().body).unwrap();
    (m.bbox.min, m.bbox.max)
}

fn stretch_steps(s: &Session) -> usize {
    s.doc.features.iter().filter(|f| matches!(f.kind, solidcraft_doc::FeatureKind::Stretch { .. })).count()
}

#[test]
fn edit_sketch_grows_the_dimension_across_the_cut_and_the_extrude_depth() {
    let mut s = plate();
    // Along X, in the sketch's plane: the 100 mm bottom spans the cut at x = 50 and becomes 115.
    let v = run(&mut s, "solid.stretch", json!({"body": "Plate", "direction": "+x", "at": 50, "distance": 15, "edit_sketch": true}));
    assert_eq!(v["made_by"], json!("sketch"), "{v}");
    assert_eq!(stretch_steps(&s), 0, "no Stretch step: the sketch changed");
    let (min, max) = plate_box(&s);
    assert!(min.dist(Vec3::ZERO) < 1e-6 && max.dist(Vec3::new(115.0, 60.0, 10.0)) < 1e-6, "{min:?} {max:?}");
    // Along Z, the Extrude's direction: its depth grows from 10 to 15.
    let v = run(&mut s, "solid.stretch", json!({"body": "Plate", "direction": "+z", "at": 5, "distance": 5, "edit_sketch": true}));
    assert_eq!(v["made_by"], json!("sketch"), "{v}");
    assert!((plate_box(&s).1.z - 15.0).abs() < 1e-6);
    assert_eq!(stretch_steps(&s), 0);
    // Undo takes each change back whole.
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "edit.undo", json!({}));
    assert!(plate_box(&s).1.dist(Vec3::new(100.0, 60.0, 10.0)) < 1e-6);
}

#[test]
fn edit_sketch_falls_back_to_a_stretch_step_and_says_why() {
    // A box has no sketch: a Stretch step, and why the sketch was not changed.
    let mut s = block();
    let v = run(&mut s, "solid.stretch", json!({"body": "Block", "direction": "+x", "distance": 15, "edit_sketch": true}));
    assert_eq!(v["made_by"], json!("step"), "{v}");
    assert!(v["sketch_not_edited"].as_str().is_some_and(|w| w.contains("Extrude")), "{v}");
    assert!((size(&s).1.x - 55.0).abs() < 1e-6);
    // A plate shrunk from its fixed side: the sketch's side would move, so a Stretch step too.
    let mut s = plate();
    let v = run(&mut s, "solid.stretch", json!({"body": "Plate", "direction": "-z", "at": 5, "distance": 2, "edit_sketch": true}));
    assert_eq!(v["made_by"], json!("step"), "{v}");
    assert_eq!(stretch_steps(&s), 1);
}

#[test]
fn edit_sketch_moves_a_hole_drawn_beyond_the_cut_in_another_sketch() {
    let mut s = plate();
    // A 10 mm hole at (80, 30), drawn in its own sketch and cut through the plate.
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Hole"}));
    run(&mut s, "sketch.circle.center", json!({"center": [80, 30], "radius": 5, "id": "bore"}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch": "Hole", "distance": 10, "operation": "cut", "targets": ["Plate"]}));
    let before = solidcraft_kernel::measure(&s.model.state().body("Plate").unwrap().body).unwrap().volume;
    let v = run(&mut s, "solid.stretch", json!({"body": "Plate", "direction": "+x", "at": 50, "distance": 15, "edit_sketch": true}));
    assert_eq!(v["made_by"], json!("sketch"), "{v}");
    assert_eq!(stretch_steps(&s), 0);
    let (_, max) = plate_box(&s);
    assert!((max.x - 115.0).abs() < 1e-6, "{max:?}");
    let after = solidcraft_kernel::measure(&s.model.state().body("Plate").unwrap().body).unwrap().volume;
    assert!((after - (before + 15.0 * 60.0 * 10.0)).abs() < 1e-6, "{after} vs {before}");
    // The hole's sketch moved its circle with the part: its centre is at x = 95 now.
    let st = s.model.state();
    let hole = st.sketches.iter().find(|k| k.name == "Hole").unwrap();
    let centre = hole.sketch.points.iter().map(|p| p.pos.x).fold(f64::NEG_INFINITY, f64::max);
    assert!((centre - 95.0).abs() < 1e-6, "{centre}");
}
