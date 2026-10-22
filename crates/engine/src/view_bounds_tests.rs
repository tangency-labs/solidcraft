// Home and Fit frame what is visible now: bodies where they are placed (occurrence moves,
// joints, motion), sketches only while they are drawn, nothing hidden or consumed.
use serde_json::{Value, json};
use solidcraft_geom::{Aabb3, Vec3};

use crate::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    match s.execute(id, &p) {
        Ok(v) => v,
        Err(e) => panic!("{id} {p}: {e}"),
    }
}

/// What the viewport draws: visible bodies' meshes and the sketches it shows, from the scene.
fn drawn(s: &Session) -> Aabb3 {
    let cam = solidcraft_render::Camera { distance: 1e5, ..Default::default() };
    let scene = crate::view::scene(s, &cam).unwrap();
    let mut b = Aabb3::EMPTY;
    for m in &scene.meshes {
        b = b.union(&m.mesh.bounds());
    }
    use crate::view::colors::{AXIS_X, AXIS_Y, AXIS_Z, GRID, GRID_MAJOR};
    for l in scene.lines.iter().filter(|l| ![GRID, GRID_MAJOR, AXIS_X, AXIS_Y, AXIS_Z].contains(&l.color)) {
        for p in &l.points {
            b.add(*p);
        }
    }
    b
}

fn same(a: Aabb3, b: Aabb3) {
    assert!(!a.is_empty() && !b.is_empty(), "{a:?} {b:?}");
    assert!(a.min.dist(b.min) < 1e-6 && a.max.dist(b.max) < 1e-6, "Home frames {a:?}, the view shows {b:?}");
}

fn near(a: Aabb3, lo: [f64; 3], hi: [f64; 3]) {
    let (lo, hi) = (Vec3::new(lo[0], lo[1], lo[2]), Vec3::new(hi[0], hi[1], hi[2]));
    assert!(a.min.dist(lo) < 1e-6 && a.max.dist(hi) < 1e-6, "{a:?} != {lo:?}..{hi:?}");
}

/// A 10 x 10 x 5 block drawn as a sketch and extruded, in the active component.
fn block(s: &mut Session) {
    run(s, "sketch.create", json!({"plane": "XY"}));
    run(s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [10, 10]}));
    run(s, "sketch.finish", json!({}));
    run(s, "solid.extrude", json!({"distance": 5}));
}

#[test]
fn moved_body_leaves_its_used_sketch_out() {
    let mut s = Session::default();
    block(&mut s);
    near(crate::view::bounds(&s).unwrap(), [0., 0., 0.], [10., 10., 5.]);
    let body = s.world_state().unwrap().bodies[0].name.clone();
    run(&mut s, "solid.move", json!({"bodies": [body], "translate": [500, 0, 0]}));
    near(crate::view::bounds(&s).unwrap(), [500., 0., 0.], [510., 10., 5.]);
    same(crate::view::bounds(&s).unwrap(), drawn(&s));
}

#[test]
fn moved_component_frames_where_it_is_now() {
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name": "A"}));
    block(&mut s);
    run(&mut s, "component.activate", json!({"component": "root"}));
    let b = run(&mut s, "component.create", json!({"name": "B"}))["occurrence"].as_u64().unwrap();
    block(&mut s);
    run(&mut s, "component.activate", json!({"component": "root"}));
    run(&mut s, "occurrence.move", json!({"occurrence": b, "translate": [800, 0, 0]}));
    near(crate::view::bounds(&s).unwrap(), [0., 0., 0.], [810., 10., 5.]);
    run(&mut s, "occurrence.move", json!({"occurrence": b, "translate": [-780, 0, 0]}));
    near(crate::view::bounds(&s).unwrap(), [0., 0., 0.], [30., 10., 5.]);
    same(crate::view::bounds(&s).unwrap(), drawn(&s));
    // Pending (not captured) moves too.
    run(&mut s, "occurrence.move", json!({"occurrence": b, "translate": [0, 300, 0], "capture": false}));
    near(crate::view::bounds(&s).unwrap(), [0., 0., 0.], [30., 310., 5.]);
    same(crate::view::bounds(&s).unwrap(), drawn(&s));
}

#[test]
fn body_consumed_by_a_join_or_cut_is_not_framed() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [200, 0, 0], "operation": "new"}));
    let names: Vec<String> = s.world_state().unwrap().bodies.iter().map(|b| b.name.clone()).collect();
    run(&mut s, "solid.move", json!({"bodies": [names[1].clone()], "translate": [-195, 0, 0]}));
    run(&mut s, "solid.combine", json!({"target": names[0].clone(), "tools": [names[1].clone()], "operation": "cut"}));
    near(crate::view::bounds(&s).unwrap(), [0., 0., 0.], [5., 10., 10.]);
    same(crate::view::bounds(&s).unwrap(), drawn(&s));
}

#[test]
fn hidden_component_is_not_framed() {
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name": "A"}));
    block(&mut s);
    run(&mut s, "component.activate", json!({"component": "root"}));
    let b = run(&mut s, "component.create", json!({"name": "B"}))["occurrence"].as_u64().unwrap();
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [10, 10]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance": 5}));
    // An unused sketch in B, drawn while B is shown.
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [40, 40]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    run(&mut s, "occurrence.move", json!({"occurrence": b, "translate": [400, 0, 0]}));
    near(crate::view::bounds(&s).unwrap(), [0., 0., 0.], [440., 40., 5.]);
    same(crate::view::bounds(&s).unwrap(), drawn(&s));
    run(&mut s, "browser.visibility", json!({"visible": false, "paths": [[b]]}));
    near(crate::view::bounds(&s).unwrap(), [0., 0., 0.], [10., 10., 5.]);
    same(crate::view::bounds(&s).unwrap(), drawn(&s));
}

/// Two leaves joined by a hinge (as in the joint tests).
fn hinge(s: &mut Session) {
    for name in ["LeafA", "LeafB"] {
        run(s, "component.activate", json!({"component": "root"}));
        run(s, "component.create", json!({"name": name}));
        block_leaf(s);
    }
    run(s, "component.activate", json!({"component": "root"}));
    let a = s.doc.occurrence_of(s.doc.find_component("LeafA").unwrap()).unwrap().id;
    run(s, "occurrence.ground", json!({"occurrence": a, "grounded": true}));
    let b = s.doc.occurrence_of(s.doc.find_component("LeafB").unwrap()).unwrap().id;
    // B starts far away; the joint brings it onto A.
    run(s, "occurrence.move", json!({"occurrence": b, "translate": [600, 0, 0]}));
    run(
        s,
        "joint.create",
        json!({"type": "revolute", "a": {"occurrence": "LeafA", "circle": [1, 37, 4]}, "b": {"occurrence": "LeafB", "circle": [601, 37, 4]}, "flip": true, "name": "Hinge"}),
    );
}

fn block_leaf(s: &mut Session) {
    // The plate drawn as a sketch (used by its extrude), and the knuckle.
    run(s, "sketch.create", json!({"plane": "XY"}));
    run(s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [40, 30]}));
    run(s, "sketch.finish", json!({}));
    run(s, "solid.extrude", json!({"distance": 3}));
    run(s, "solid.cylinder", json!({"base": [0, 33, 4], "axis": [1, 0, 0], "radius": 4, "height": 40, "operation": "new"}));
}

#[test]
fn joint_and_drive_frame_the_joined_place() {
    let mut s = Session::default();
    hinge(&mut s);
    let b = crate::view::bounds(&s).unwrap();
    assert!(b.max.x < 50.0, "the joint brought B onto A: {b:?}");
    same(b, drawn(&s));
    run(&mut s, "joint.drive", json!({"joint": "Hinge", "value": 90}));
    let b = crate::view::bounds(&s).unwrap();
    assert!(b.max.x < 50.0 && b.max.z > 30.0, "{b:?}");
    same(b, drawn(&s));
    run(&mut s, "motion.study", json!({"name": "Swing", "steps": 10, "keys": [{"joint": "Hinge", "points": [[0, 0], [10, 180]]}]}));
    run(&mut s, "motion.play", json!({"study": "Swing", "step": 10}));
    let b = crate::view::bounds(&s).unwrap();
    assert!(b.max.x < 50.0 && b.max.y > 60.0, "{b:?}");
    same(b, drawn(&s));
}
