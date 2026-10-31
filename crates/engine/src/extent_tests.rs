//! To Object extents on curved targets, and shells outside/both on curved bodies.

use std::f64::consts::PI;

use serde_json::{Value, json};

use crate::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    match s.execute(id, &p) {
        Ok(v) => v,
        Err(e) => panic!("{id} {p}: {e}"),
    }
}

fn body(s: &mut Session, b: &str) -> Value {
    run(s, "inspect.measure", json!({ "bodies": [b] }))["bodies"][0].clone()
}

fn stepped_block() -> Session {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "step_height", "expression": "10 mm"}));
    run(&mut s, "solid.box", json!({"corner": [-10, -10, 0], "length": 20, "width": 20, "height": 10, "body_name": "Block"}));
    run(&mut s, "sketch.create", json!({"plane": {"origin": [0, 0, 10], "x_dir": [1, 0, 0], "y_dir": [0, 1, 0]}}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [-5, -5], "p1": [5, 5]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance": "step_height", "operation": "join"}));
    s
}

/// A column beside a step ends at the step's top, including when the To point lies on an
/// edge or corner shared with a vertical wall (#3729).
#[test]
fn extrude_to_a_planar_face_boundary_ends_at_its_plane() {
    for to in [[5, 0, 20], [5, 5, 20], [0, 0, 20], [1000, 1000, 20]] {
        let mut s = stepped_block();
        run(&mut s, "sketch.create", json!({"plane": {"origin": [0, 0, 10], "x_dir": [1, 0, 0], "y_dir": [0, 1, 0]}}));
        run(&mut s, "sketch.circle.center", json!({"center": [8, 0], "radius": 1.5}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "solid.extrude", json!({"to": to, "operation": "join"}));
        let b = body(&mut s, "Block");
        let top = b["bbox"]["max"][2].as_f64().unwrap();
        let volume = b["volume_mm3"].as_f64().unwrap();
        assert!((top - 20.0).abs() < 1e-6, "To {to:?}: z max {top}");
        assert!((volume - (5000.0 + 10.0 * PI * 1.5 * 1.5)).abs() < 0.1, "To {to:?}: volume {volume}");
    }
}

/// The face chosen at an edge must also be the stored reference: raising the step must raise
/// the end of the extrusion, including after a save/reload.
#[test]
fn extrude_to_a_planar_face_boundary_follows_it_after_reload() {
    for to in [[5, 0, 20], [5, 5, 20]] {
        let mut s = stepped_block();
        run(&mut s, "sketch.create", json!({"plane": {"origin": [0, 0, 30], "x_dir": [1, 0, 0], "y_dir": [0, 1, 0]}}));
        run(&mut s, "sketch.circle.center", json!({"center": [8, 0], "radius": 1.5}));
        run(&mut s, "sketch.finish", json!({}));
        // A negative reach with an offset past the target, away from the start.
        run(&mut s, "solid.extrude", json!({"to": to, "to_offset": 2, "operation": "new", "body_name": "Post"}));
        let b = body(&mut s, "Post");
        assert!((b["bbox"]["min"][2].as_f64().unwrap() - 18.0).abs() < 1e-6, "{b}");
        let doc = solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap();
        let mut cold = Session::new(doc);
        for current in [&mut s, &mut cold] {
            run(current, "parameters.change", json!({"name": "step_height", "expression": "15 mm"}));
            let b = body(current, "Post");
            let volume = b["volume_mm3"].as_f64().unwrap();
            assert!((b["bbox"]["min"][2].as_f64().unwrap() - 23.0).abs() < 1e-6, "{b}");
            assert!((b["bbox"]["max"][2].as_f64().unwrap() - 30.0).abs() < 1e-6, "{b}");
            assert!((volume - 7.0 * PI * 1.5 * 1.5).abs() < 0.1, "{b}");
            assert!(current.model.results.iter().all(|r| r.error.is_none() && r.warning.is_none()), "{:?}", current.model.results);
        }
    }
}

/// A post from the side of one block, To Object a face of a second block that runs along the
/// extrusion (its top or its side, the block only partly in front of the profile): the post ends
/// where that face begins, at the block's near side, and the To offset moves the end. It used to
/// run on past the whole block, trimmed only where the block was, and the offset did nothing.
#[test]
fn extrude_to_a_face_along_the_direction_ends_where_the_face_begins() {
    for (to, offset, end) in [([35, 18, 7], 0.0, 25.0), ([35, 18, 7], 2.0, 27.0), ([38, 8, 0], 0.0, 25.0), ([38, 8, 0], -1.5, 23.5)] {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 20, "width": 20, "height": 20, "body_name": "A"}));
        run(&mut s, "solid.box", json!({"corner": [25, 8, -13], "length": 20, "width": 20, "height": 20, "operation": "new", "body_name": "B"}));
        run(&mut s, "sketch.create", json!({"plane": {"face": [20, 10, 10]}}));
        run(&mut s, "sketch.rectangle.two_point", json!({"p0": [2, 2], "p1": [18, 18]}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "solid.extrude", json!({"to": to, "to_offset": offset, "operation": "new", "body_name": "Post"}));
        let b = body(&mut s, "Post");
        let reach = b["bbox"]["max"][0].as_f64().unwrap();
        assert!((reach - end).abs() < 1e-6, "To {to:?} offset {offset}: x max {reach}");
        assert!((b["volume_mm3"].as_f64().unwrap() - 256.0 * (end - 20.0)).abs() < 1e-3, "To {to:?} offset {offset}: {b}");
    }
}

/// A column To Object the side wall of the step it stands beside, a wall that starts at the
/// profile's plane: it goes the wall's height, to the step's top.
#[test]
fn extrude_to_a_wall_rising_from_the_profile_goes_its_height() {
    let mut s = stepped_block();
    run(&mut s, "sketch.create", json!({"plane": {"origin": [0, 0, 10], "x_dir": [1, 0, 0], "y_dir": [0, 1, 0]}}));
    run(&mut s, "sketch.circle.center", json!({"center": [8, 0], "radius": 1.5}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"to": [5, 0, 14], "operation": "new", "body_name": "Post"}));
    let b = body(&mut s, "Post");
    assert!((b["bbox"]["max"][2].as_f64().unwrap() - 20.0).abs() < 1e-6, "{b}");
}

/// A square post extruded up to a horizontal cylinder (axis along x, r 10, centre z 30): its
/// top takes the cylinder's shape, so the post is shortest in the middle.
#[test]
fn extrude_to_a_cylinder_follows_it() {
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"base": [-20, 0, 30], "axis": [1, 0, 0], "radius": 10, "height": 40, "body_name": "Bar"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "P"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [-4, -4], "p1": [4, 4]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch": "P", "to": [0, 0, 20], "body_name": "Post"}));
    let b = body(&mut s, "Post");
    // Highest at the post's sides (y = ±4): z = 30 − √(100 − 16); the volume follows the arc.
    let top = b["bbox"]["max"][2].as_f64().unwrap_or(0.0);
    assert!((top - (30.0 - (100.0f64 - 16.0).sqrt())).abs() < 1e-3, "{b}");
    // ∫ over y of (30 − √(100 − y²)) × 8 (x width), y from −4 to 4.
    let n = 2000;
    let want: f64 = (0..n).map(|i| -4.0 + 8.0 * (i as f64 + 0.5) / n as f64).map(|y| (30.0 - (100.0 - y * y).sqrt()) * 8.0 * 8.0 / n as f64).sum();
    assert!((b["volume_mm3"].as_f64().unwrap_or(0.0) - want).abs() < want * 2e-3, "{} vs {want}", b["volume_mm3"]);
}

/// A round post extruded up to a sphere (r 15 at z 40): it stops on the sphere's underside.
#[test]
fn extrude_to_a_sphere_follows_it() {
    let mut s = Session::default();
    run(&mut s, "solid.sphere", json!({"center": [0, 0, 40], "radius": 15, "body_name": "Ball"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "P"}));
    run(&mut s, "sketch.circle.center", json!({"center": [0, 0], "radius": 5}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch": "P", "to": [0, 0, 25], "body_name": "Post"}));
    let b = body(&mut s, "Post");
    let top = b["bbox"]["max"][2].as_f64().unwrap_or(0.0);
    assert!((top - (40.0 - (225.0f64 - 25.0).sqrt())).abs() < 1e-2, "{b}");
    // Volume: cylinder to z 25 plus the ring between the sphere cap and the plane z = 25.
    let cap = |r: f64| 40.0 - (225.0 - r * r).sqrt();
    let n = 2000;
    let want: f64 = (0..n).map(|i| 5.0 * (i as f64 + 0.5) / n as f64).map(|r| cap(r) * 2.0 * PI * r * 5.0 / n as f64).sum();
    assert!((b["volume_mm3"].as_f64().unwrap_or(0.0) - want).abs() < want * 2e-3, "{} vs {want}", b["volume_mm3"]);
}

/// Shell Outside and Both on a cylinder (open top): exact offsets of the curved wall.
#[test]
fn shell_outside_and_both_on_a_cylinder() {
    let shell = |dir: &str| {
        let mut s = Session::default();
        run(&mut s, "solid.cylinder", json!({"radius": 10, "height": 20, "body_name": "C"}));
        run(&mut s, "solid.shell", json!({"faces": [[0, 0, 20]], "thickness": 2, "direction": dir}));
        body(&mut s, "C")["volume_mm3"].as_f64().unwrap_or(0.0)
    };
    let out = PI * (144.0 * 22.0 - 100.0 * 20.0);
    assert!((shell("outside") - out).abs() < out * 1e-3, "{} vs {out}", shell("outside"));
    let both = PI * (121.0 * 21.0 - 81.0 * 19.0);
    assert!((shell("both") - both).abs() < both * 1e-3, "{} vs {both}", shell("both"));
}

/// A ring section revolved To Object up to a post standing in its path: it ends on the post's
/// round side (touching, not overlapping), between first contact and the post's centre.
#[test]
fn revolve_to_a_cylinder_follows_it() {
    for side in [1.0, -1.0] {
        let mut s = Session::default();
        run(&mut s, "solid.cylinder", json!({"base": [0, 14.0 * side, -5], "radius": 4, "height": 15, "body_name": "Post"}));
        run(&mut s, "sketch.create", json!({"plane": "XZ", "name": "S"}));
        run(&mut s, "sketch.line", json!({"points": [[10, 0], [15, 0], [15, 5], [10, 5]], "closed": true}));
        run(&mut s, "sketch.finish", json!({}));
        let r = s.execute("solid.revolve", &json!({"sketch": "S", "axis": "y", "to": [4, 14.0 * side, 2], "body_name": "Arc"}));
        if r.is_err() {
            continue; // the post on the side the revolve turns away from is past the turn
        }
        let st = s.model.state();
        let (arc, post) = (st.body("Arc").unwrap(), st.body("Post").unwrap());
        let overlap = solidcraft_kernel::boolean(&arc.body, &post.body, solidcraft_kernel::BoolOp::Intersect)
            .ok()
            .flatten()
            .map(|b| solidcraft_kernel::measure(&b).map(|m| m.volume).unwrap_or(0.0))
            .unwrap_or(0.0);
        assert!(overlap < 1e-3, "no overlap: {overlap}");
        let v = solidcraft_kernel::measure(&arc.body).unwrap().volume;
        let full = PI * (225.0 - 100.0) * 5.0;
        // The post is at a quarter turn one way (three quarters the other): the ring stops
        // short of the post's centre line, within its half-width of it.
        let turned = v / full * 360.0;
        let to_centre = if turned < 180.0 { 90.0 } else { 270.0 };
        assert!(turned < to_centre && turned > to_centre - 30.0, "{turned}°");
        return;
    }
    panic!("neither post was reached");
}

/// To Object picks the face that is at the picked point, not the first body with any face
/// near it. Here the pick is on a post (r 2.75) only 2 mm above a plate: the ridge must end
/// on the post. Before, the plate (within 5 % of its size of the pick) was taken as the
/// target, nothing trimmed the tool, and the ridge ran past the post.
#[test]
fn extrude_to_a_cylinder_beside_another_body() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"corner": [-98, -5, 0], "length": 101, "width": 10, "height": 3, "body_name": "Plate"}));
    run(&mut s, "solid.cylinder", json!({"base": [0, 0, 0], "axis": [0, 0, 1], "radius": 2.75, "height": 10, "body_name": "Post"}));
    // A ridge above the plate, y -1.5..0, z 4..7, from the plate's far end towards the post.
    run(&mut s, "sketch.create", json!({"plane": {"origin": [-98, 0, 0], "x_dir": [0, 1, 0], "y_dir": [0, 0, 1]}, "name": "End"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [-1.5, 4], "p1": [0, 7]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch": "End", "to": [-2.75, 0, 5], "operation": "new", "body_name": "Ridge"}));
    let b = body(&mut s, "Ridge");
    // Its end follows the post: farthest at the ridge's edge y = -1.5, x = -√(2.75² − 1.5²).
    let end = b["bbox"]["max"][0].as_f64().unwrap_or(f64::MAX);
    assert!((end + (2.75f64 * 2.75 - 1.5 * 1.5).sqrt()).abs() < 1e-3, "{b}");
    assert!((b["bbox"]["min"][0].as_f64().unwrap_or(0.0) + 98.0).abs() < 1e-6, "{b}");
}
