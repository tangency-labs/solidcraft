//! Components as frames (regression cases for user bug #4): picks, sketches and bodies on
//! moved components and pasted instances.

use std::f64::consts::PI;

use serde_json::json;

use super::{rel, run};
use crate::*;

/// Components are frames: world picks on a moved component (whichever is active) land on its
/// geometry, bodies moved into it keep their place, and sketches on its faces follow it.
#[test]
fn moved_component_picks_and_frames() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "Base"}));
    let c = run(&mut s, "component.create", json!({"name": "Arm", "grounded": false}));
    let (cid, occ) = (c["component"].as_u64().unwrap(), c["occurrence"].as_u64().unwrap());
    run(&mut s, "solid.box", json!({"corner": [20, 0, 0], "length": 10, "width": 4, "height": 4, "body_name": "ArmBody"}));
    run(&mut s, "component.activate", json!({"component": 0}));
    run(&mut s, "occurrence.move", json!({"occurrence": occ, "translate": [0, 50, 0]}));
    let wbox = |s: &Session, b: &str| s.world_state().unwrap().body(b).unwrap().mesh().bounds();
    let vol = |s: &mut Session, b: &str| run(s, "inspect.measure", json!({"bodies": [b]}))["total"]["volume_mm3"].as_f64().unwrap();
    // Root active: a fillet picked at the moved edge in the world.
    run(&mut s, "solid.fillet", json!({"edges": [[25, 54, 4]], "radius": 1}));
    assert!(rel(vol(&mut s, "ArmBody"), 160.0 - (1.0 - PI / 4.0) * 10.0) < 1e-4, "{}", vol(&mut s, "ArmBody"));
    // An explicit fixed plane on the modified body's top keeps the world pick;
    // unsupported modifier lineage cannot silently create a weak attachment.
    let sk = run(&mut s, "sketch.create", json!({"plane": {"face": [25, 52, 4],"fixed":true}}));
    run(&mut s, "sketch.circle.center", json!({"center": [0, 0], "radius": 1}));
    run(&mut s, "sketch.finish", json!({}));
    let si = run(&mut s, "sketch.inspect", json!({"sketch": sk["sketch"]}));
    let z: Vec<f64> = si["points"].as_array().unwrap().iter().map(|p| p["world"][2].as_f64().unwrap()).collect();
    assert!(z.iter().all(|z| (z - 4.0).abs() < 1e-6), "{si}");
    // Base moved into the moved component stays where it was.
    let before = wbox(&s, "Base");
    run(&mut s, "component.move_bodies", json!({"bodies": ["Base"], "component": "Arm"}));
    let after = wbox(&s, "Base");
    assert!(before.min.dist(after.min) < 1e-9 && before.max.dist(after.max) < 1e-9, "{before:?} {after:?}");
    // A pasted instance: a pick on it lands on the shared definition.
    run(&mut s, "occurrence.copy", json!({"component": cid, "translate": [0, 20, 0]}));
    let v0 = vol(&mut s, "ArmBody");
    let names: Vec<String> = s.world_state().unwrap().bodies.iter().map(|b| b.name.clone()).collect();
    let inst = names.iter().find(|n| n.starts_with("ArmBody (")).cloned().unwrap();
    let ib = wbox(&s, &inst);
    // Its bottom edge along x at the far side (y max, z 0).
    run(&mut s, "solid.fillet", json!({"edges": [[25, ib.max.y, 0]], "radius": 1}));
    assert!(vol(&mut s, "ArmBody") < v0 - 1.0, "the instance's pick reached the definition");
}

/// An extrude in a moved component, made from a sketch of another component (here the root), is
/// built where the sketch is shown: the sketch is seen through both placements, as a sketch on
/// that component's face already is. Before, its plane was read as if it were in the feature's
/// own component, so the join landed turned along with the moved component.
#[test]
fn extrude_in_a_moved_component_from_a_root_sketch() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [-40, -5], "p1": [0, 5]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance": 10, "body_name": "Jaw"}));
    let made = run(&mut s, "component.from_bodies", json!({"bodies": ["Jaw"]}));
    let jaw = made["components"][0]["component"].as_u64().unwrap();
    run(&mut s, "occurrence.ground", json!({"occurrence": "Jaw:1", "grounded": false}));
    // Turned half a turn about Z: the jaw now lies along +X (x 0..40, y -5..5).
    run(&mut s, "occurrence.move", json!({"occurrence": "Jaw:1", "axis": [0, 0, 1], "angle": "180 deg", "origin": [0, 0, 0]}));
    run(&mut s, "component.activate", json!({"component": 0}));
    // A root sketch: a tab on the jaw's +Y side, x 30..40, y 5..7 in the world.
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Tab"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [30, 5], "p1": [40, 7]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "component.activate", json!({"component": jaw}));
    run(&mut s, "solid.extrude", json!({"sketch": "Tab", "distance": 10, "operation": "join", "targets": ["Jaw"]}));
    let b = s.world_state().unwrap().body("Jaw").unwrap().mesh().bounds();
    let near = |a: f64, e: f64| (a - e).abs() < 1e-6;
    assert!(near(b.min.x, 0.0) && near(b.max.x, 40.0) && near(b.min.y, -5.0) && near(b.max.y, 7.0), "{b:?}");
    let v = run(&mut s, "inspect.measure", json!({"bodies": ["Jaw"]}))["total"]["volume_mm3"].as_f64().unwrap();
    assert!(rel(v, 4000.0 + 200.0) < 1e-6, "{v}");
}

/// To Object in a moved component: the picked face is a world point, kept in the feature's own
/// component like its other picks, so the extrude ends at that face (here the jaw's top, 10 mm
/// above the sketch) wherever the component is placed.
#[test]
fn extrude_to_a_face_in_a_moved_component() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [-40, -5], "p1": [0, 5]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance": 10, "body_name": "Jaw"}));
    let made = run(&mut s, "component.from_bodies", json!({"bodies": ["Jaw"]}));
    let jaw = made["components"][0]["component"].as_u64().unwrap();
    run(&mut s, "occurrence.ground", json!({"occurrence": "Jaw:1", "grounded": false}));
    run(&mut s, "occurrence.move", json!({"occurrence": "Jaw:1", "axis": [0, 0, 1], "angle": "180 deg", "origin": [0, 0, 0]}));
    // Lifted too, so the jaw's own frame and the world differ in height (x 0..40, z 5..15).
    run(&mut s, "occurrence.move", json!({"occurrence": "Jaw:1", "translate": [0, 0, 5]}));
    run(&mut s, "component.activate", json!({"component": 0}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Tab"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [30, 5], "p1": [40, 7]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "component.activate", json!({"component": jaw}));
    // Up to the jaw's top face, picked in the world at its middle: the tab rises from z 0 to 15.
    run(&mut s, "solid.extrude", json!({"sketch": "Tab", "to": [20, 0, 15], "operation": "join", "targets": ["Jaw"]}));
    let b = s.world_state().unwrap().body("Jaw").unwrap().mesh().bounds();
    let near = |a: f64, e: f64| (a - e).abs() < 1e-6;
    assert!(
        near(b.min.x, 0.0) && near(b.max.x, 40.0) && near(b.min.y, -5.0) && near(b.max.y, 7.0) && near(b.min.z, 0.0) && near(b.max.z, 15.0),
        "{b:?}"
    );
    let v = run(&mut s, "inspect.measure", json!({"bodies": ["Jaw"]}))["total"]["volume_mm3"].as_f64().unwrap();
    assert!(rel(v, 4000.0 + 10.0 * 2.0 * 15.0) < 1e-6, "{v}");
}

/// Component `Target` with a ramp (top face z = -20 + y/2 over x -10..10, y -20..20, in its own
/// frame), its occurrence moved by `moves`, then a new component `Post`, active, with a 6 x 6 mm
/// sketch `Pad` centred at the origin (#928).
fn ramp_and_pad(s: &mut Session, moves: &[serde_json::Value]) {
    run(s, "component.create", json!({"name": "Target", "grounded": false}));
    run(s, "sketch.create", json!({"plane": "YZ", "name": "Wedge"}));
    run(s, "sketch.line", json!({"points": [[-20, -30], [20, -30], [20, -10], [-20, -30]]}));
    run(s, "sketch.finish", json!({}));
    run(s, "solid.extrude", json!({"sketch": "Wedge", "distance": "20 mm", "direction": "symmetric", "body_name": "Ramp"}));
    run(s, "component.activate", json!({"component": "root"}));
    for m in moves {
        run(s, "occurrence.move", m.clone());
    }
    run(s, "component.create", json!({"name": "Post"}));
    run(s, "sketch.create", json!({"plane": "XY", "name": "Pad"}));
    run(s, "sketch.rectangle.two_point", json!({"p0": [-3, -3], "p1": [3, 3]}));
    run(s, "sketch.finish", json!({}));
}

fn leg_volume(s: &mut Session) -> f64 {
    run(s, "inspect.measure", json!({"bodies": ["Leg"]}))["total"]["volume_mm3"].as_f64().unwrap()
}

/// To Object on a slanted face of a body in another component whose occurrence is moved: the
/// extrude ends at the face where it is shown, not where the component's own frame has it
/// (#928: the pad ran 20 mm deep, as if the ramp had not moved).
#[test]
fn extrude_to_a_slanted_face_in_another_moved_component() {
    // Moved 5 mm along +y: the face is z = -20 + (y - 5)/2, 22.5 mm below the pad on average.
    let mut s = Session::default();
    ramp_and_pad(&mut s, &[json!({"occurrence": "Target:1", "space": "world", "translate": [0, 5, 0]})]);
    run(
        &mut s,
        "solid.extrude",
        json!({"sketch": "Pad", "distance": "1 mm", "direction": "negative", "to": [0, 5, -20], "operation": "new", "body_name": "Leg"}),
    );
    let v = leg_volume(&mut s);
    assert!(rel(v, 36.0 * 22.5) < 1e-6, "{v}");
    // Turned a quarter turn about Z and moved 4 mm along +x: the face is z = -18 - x/2.
    let mut s = Session::default();
    ramp_and_pad(
        &mut s,
        &[json!({"occurrence": "Target:1", "space": "world", "axis": [0, 0, 1], "angle": "90 deg", "origin": [0, 0, 0], "translate": [4, 0, 0]})],
    );
    run(
        &mut s,
        "solid.extrude",
        json!({"sketch": "Pad", "distance": "1 mm", "direction": "negative", "to": [0, 0, -18], "operation": "new", "body_name": "Leg"}),
    );
    let v = leg_volume(&mut s);
    assert!(rel(v, 36.0 * 18.0) < 1e-6, "{v}");
}

/// From Object on a slanted face in another, moved component: the extrude starts where the
/// pad's middle meets the face as shown (z -22.5), not where it is in the component's frame.
#[test]
fn extrude_from_a_slanted_face_in_another_moved_component() {
    let mut s = Session::default();
    ramp_and_pad(&mut s, &[json!({"occurrence": "Target:1", "space": "world", "translate": [0, 5, 0]})]);
    run(
        &mut s,
        "solid.extrude",
        json!({"sketch": "Pad", "from": [0, 5, -20], "distance": 4, "direction": "negative", "operation": "new", "body_name": "Leg"}),
    );
    let b = s.world_state().unwrap().body("Leg").unwrap().mesh().bounds();
    assert!((b.max.z + 22.5).abs() < 1e-6 && (b.min.z + 26.5).abs() < 1e-6, "{b:?}");
}

/// The To face in another component is kept by name, as one in the same component is: when that
/// component's occurrence moves on, the extrude follows the face.
#[test]
fn extrude_to_a_face_in_another_component_follows_it() {
    let mut s = Session::default();
    ramp_and_pad(&mut s, &[json!({"occurrence": "Target:1", "space": "world", "translate": [0, 5, 0]})]);
    run(
        &mut s,
        "solid.extrude",
        json!({"sketch": "Pad", "distance": "1 mm", "direction": "negative", "to": [0, 5, -20], "operation": "new", "body_name": "Leg"}),
    );
    let names = s.doc.features.last().map(|f| f.face_names.clone()).unwrap_or_default();
    assert!(names.first().is_some_and(|n| !n.is_empty()), "the To face is stored by name: {names:?}");
    // Another 10 mm along +y: the face is z = -20 + (y - 15)/2, 27.5 mm below the pad on average.
    run(&mut s, "component.activate", json!({"component": "root"}));
    run(&mut s, "occurrence.move", json!({"occurrence": "Target:1", "space": "world", "translate": [0, 10, 0]}));
    let v = leg_volume(&mut s);
    assert!(rel(v, 36.0 * 27.5) < 1e-6, "{v}");
}

/// Revolve To a curved face of a body in another component whose occurrence is turned: the
/// sweep is cut back to the post where it is shown (here a quarter of a turn earlier, at 45°).
#[test]
fn revolve_to_a_cylinder_in_another_moved_component() {
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name": "Pin", "grounded": false}));
    run(&mut s, "solid.cylinder", json!({"base": [0, 14, -5], "radius": 4, "height": 15, "body_name": "PostBody"}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    // From 90° round to 45° about Z: the post's axis is at (14 sin 45°, 14 cos 45°).
    run(&mut s, "occurrence.move", json!({"occurrence": "Pin:1", "space": "world", "axis": [0, 0, 1], "angle": "-45 deg", "origin": [0, 0, 0]}));
    run(&mut s, "sketch.create", json!({"name": "Profile", "plane": "XZ"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [10, 0], "p1": [15, 5]}));
    run(&mut s, "sketch.finish", json!({}));
    let (c, sn) = ((PI / 4.0).cos(), (PI / 4.0).sin());
    // The post's side nearest the profile: its axis plus 4 mm turned the same way.
    let to = [14.0 * sn + 4.0 * c, 14.0 * c - 4.0 * sn, 2.0];
    run(
        &mut s,
        "solid.revolve",
        json!({"sketch": "Profile", "axis": {"origin": [0, 0, 0], "dir": [0, 0, 1]}, "to": to, "operation": "new", "body_name": "Arc"}),
    );
    let world = s.world_state().unwrap();
    let arc = &world.body("Arc").unwrap().body;
    let post = &world.body("PostBody").unwrap().body;
    let overlap = solidcraft_kernel::boolean(arc, post, solidcraft_kernel::BoolOp::Intersect).unwrap();
    assert!(overlap.is_none_or(|b| solidcraft_kernel::measure(&b).unwrap().volume < 1e-3), "the arc runs into the post");
    // Past the picked point's angle (29°) on the inner side, where the post leaves room.
    let at = |r: f64, deg: f64| Vec3::new(r * deg.to_radians().cos(), r * deg.to_radians().sin(), -2.5);
    assert!(arc.contains(at(10.2, 35.0)), "the arc is not cut back to the post's surface");
}

/// A To Object pick is named after the face it is on, the nearest one, even when a face of a
/// body earlier in the model lies within its naming tolerance: here a big shim of another
/// component 0.02 mm under the floor's top face. Naming the shim's face would move the point
/// onto it and end the extrude 0.02 mm too deep.
#[test]
fn extrude_to_names_the_face_it_is_on() {
    for moved in [false, true] {
        let mut s = Session::default();
        run(&mut s, "component.create", json!({"name": "Other", "grounded": false}));
        let z = if moved { -40.02 + 10.0 } else { -40.02 };
        run(&mut s, "solid.box", json!({"corner": [-50, -50, z], "length": 100, "width": 100, "height": 20, "body_name": "Shim"}));
        run(&mut s, "component.activate", json!({"component": "root"}));
        if moved {
            run(&mut s, "occurrence.move", json!({"occurrence": "Other:1", "space": "world", "translate": [0, 0, -10]}));
        }
        run(&mut s, "component.create", json!({"name": "Post"}));
        run(&mut s, "solid.box", json!({"corner": [-10, -10, -30], "length": 20, "width": 20, "height": 10, "body_name": "Floor"}));
        run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Pad"}));
        run(&mut s, "sketch.rectangle.two_point", json!({"p0": [-3, -3], "p1": [3, 3]}));
        run(&mut s, "sketch.finish", json!({}));
        run(
            &mut s,
            "solid.extrude",
            json!({"sketch": "Pad", "distance": "1 mm", "direction": "negative", "to": [5, 5, -20], "operation": "new", "body_name": "Leg"}),
        );
        let b = s.world_state().unwrap().body("Leg").unwrap().mesh().bounds();
        assert!((b.min.z + 20.0).abs() < 1e-9 && b.max.z.abs() < 1e-9, "moved {moved}: {b:?}");
    }
}

/// A named To face settles which face the extrude ends at: when a part of another component is
/// later placed against the target, its face through the stored point is as near as the named
/// one, and it comes earlier in the model, but the extrude still follows the slanted face.
#[test]
fn extrude_to_a_named_face_keeps_it_when_another_part_touches_it() {
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name": "Other", "grounded": false}));
    // A plate whose top face is z = 50, out of the way for now.
    run(&mut s, "solid.box", json!({"corner": [-50, -50, 30], "length": 100, "width": 100, "height": 20, "body_name": "Plate"}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    run(&mut s, "component.create", json!({"name": "Post"}));
    // The ramp in the extrude's own component: top face z = -20 + y/2.
    run(&mut s, "sketch.create", json!({"plane": "YZ", "name": "Wedge"}));
    run(&mut s, "sketch.line", json!({"points": [[-20, -30], [20, -30], [20, -10], [-20, -30]]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch": "Wedge", "distance": "20 mm", "direction": "symmetric", "body_name": "Ramp"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Pad"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [-3, -3], "p1": [3, 3]}));
    run(&mut s, "sketch.finish", json!({}));
    run(
        &mut s,
        "solid.extrude",
        json!({"sketch": "Pad", "distance": "1 mm", "direction": "negative", "to": [0, 2, -19], "operation": "new", "body_name": "Leg"}),
    );
    assert!(rel(leg_volume(&mut s), 36.0 * 20.0) < 1e-6, "{}", leg_volume(&mut s));
    // The plate lowered so its top face runs through the To point (z = -19).
    run(&mut s, "component.activate", json!({"component": "root"}));
    run(&mut s, "occurrence.move", json!({"occurrence": "Other:1", "space": "world", "translate": [0, 0, -69]}));
    let v = leg_volume(&mut s);
    assert!(rel(v, 36.0 * 20.0) < 1e-6, "the extrude ended at the plate: {v}");
}
