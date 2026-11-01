//! Constraint tool picks: the Symmetry tool's third pick may be the origin point or an origin axis
//! (#1075); with three lines, the construction line between the other two is the symmetry line in
//! any pick order (#1796); a model edge or another sketch's curve is projected into the sketch first.
//! A constraint tool's pick may be a body's edge, a mirrored copy's too (#6348).

use serde_json::json;
use solidcraft_engine::geom::Vec2;

use crate::scenario::Harness;

/// An XY sketch with points p (10, 5) and q (-12, -3), the Symmetry tool started and both picked.
fn two_points_picked() -> Harness {
    let mut h = Harness::new();
    h.app.set_origin_shown(true);
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    h.app.run("sketch.point", json!({"point":[10,5],"id":"p"})).unwrap();
    h.app.run("sketch.point", json!({"point":[-12,-3],"id":"q"})).unwrap();
    h.app.cam.distance = 120.0;
    h.settle();
    h.app.start("sketch.constraint.symmetry");
    for xy in [[10, 5], [-12, -3]] {
        let at = h.call("ui.at", json!({ "sketch": xy }))["result"].clone();
        h.step(&json!({ "click": at })).unwrap();
    }
    h
}

fn point(h: &Harness, id: &str) -> Vec2 {
    let sk = h.app.session.doc.sketch(h.app.session.active_sketch.unwrap()).unwrap();
    sk.point(sk.resolve_point(id).unwrap()).unwrap()
}

#[test]
fn symmetry_about_the_origin_point() {
    let mut h = two_points_picked();
    let at = h.call("ui.at", json!({"sketch":[0,0]}))["result"].clone();
    h.step(&json!({ "click": at })).unwrap();
    assert!((point(&h, "p") + point(&h, "q")).len() < 1e-6, "{:?} {:?}", point(&h, "p"), point(&h, "q"));
}

#[test]
fn symmetry_about_an_origin_axis() {
    let mut h = two_points_picked();
    // On the Y axis, away from the origin point and from p and q.
    let at = h.call("ui.at", json!({"sketch":[0,6]}))["result"].clone();
    h.step(&json!({ "click": at })).unwrap();
    let (p, q) = (point(&h, "p"), point(&h, "q"));
    assert!((p.x + q.x).abs() < 1e-6 && (p.y - q.y).abs() < 1e-6, "mirrored in the Y axis: {p:?} {q:?}");
}

/// Two fixed lines and a construction line between them, off centre (#1796).
fn between_two_lines() -> Harness {
    let mut h = Harness::new();
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    h.app.run("sketch.line", json!({"points":[[0,0],[0,20]],"ids":["left"]})).unwrap();
    h.app.run("sketch.line", json!({"points":[[10,20],[10,0]],"ids":["right"]})).unwrap();
    h.app.run("sketch.line", json!({"points":[[3,2],[3,18]],"ids":["mid"],"construction":true})).unwrap();
    h.app.run("sketch.constraint.fix", json!({"entities":["left","right"]})).unwrap();
    h.app.cam.distance = 80.0;
    h.settle();
    h
}

/// A 40 × 30 × 10 box (corner at the origin) and a sketch on its top face's plane holding
/// a line from (10, 10) to (20, 12), the camera looking down on it.
fn box_and_line() -> Harness {
    let mut h = Harness::new();
    h.app.run("solid.box", json!({"length":40,"width":30,"height":10})).unwrap();
    h.app.run("sketch.create", json!({"plane":"XY","offset":10})).unwrap();
    h.app.run("sketch.line", json!({"points":[[10,10],[20,12]]})).unwrap();
    h.app.cam.distance = 120.0;
    h.settle();
    h
}

fn centred(h: &Harness) -> bool {
    [point(h, "mid.start"), point(h, "mid.end")].iter().all(|p| (p.x - 5.0).abs() < 1e-6)
}

#[test]
fn the_construction_line_is_the_symmetry_line_whatever_the_pick_order() {
    for order in [["mid", "left", "right"], ["left", "mid", "right"], ["left", "right", "mid"]] {
        let mut h = between_two_lines();
        h.app.start("sketch.constraint.symmetry");
        for id in order {
            let xy = match id {
                "left" => [0, 10],
                "right" => [10, 10],
                _ => [3, 10],
            };
            let at = h.call("ui.at", json!({ "sketch": xy }))["result"].clone();
            h.step(&json!({ "click": at })).unwrap();
        }
        assert!(centred(&h), "{order:?}: mid at {:?}", point(&h, "mid.start"));
    }
}

#[test]
fn three_selected_lines_take_the_construction_line_as_the_symmetry_line() {
    use solidcraft_engine::Sel::SketchCurve;
    let h = between_two_lines();
    let state = h.app.session.world_state();
    let sk = &state.sketch(h.app.session.active_sketch.unwrap()).unwrap().sketch;
    let sel = |ids: [&str; 3]| ids.map(|id| SketchCurve { id: id.into(), sketch: None });
    for ids in [["mid", "left", "right"], ["right", "mid", "left"]] {
        let params = crate::tools::constraint_preselection("sketch.constraint.symmetry", &sel(ids), Some(sk)).unwrap();
        assert_eq!(params["line"], "mid", "{ids:?}");
    }
    // Without a construction line among them, the last one stays the symmetry line.
    let params = crate::tools::constraint_preselection("sketch.constraint.symmetry", &sel(["left", "right", "mid"]), None).unwrap();
    assert_eq!(params["line"], "mid");
}

/// A plate from x = 20 to 120 (y 0..50, 20 thick) mirrored about YZ, a sketch on its top with the
/// plate's top face projected, and a line at x = -100 over the copy, whose outer top edge is at
/// x = -120 and not in the sketch. Collinear: the line, then that edge. The click on the edge
/// projects it, and the line goes onto it, in one undo step (#6348).
#[test]
fn collinear_with_an_edge_of_a_mirrored_body_projects_the_edge() {
    let mut h = Harness::new();
    h.app.run("sketch.create", json!({"plane":"XY","name":"Base"})).unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0":[20,0],"p1":[120,50]})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    h.app.run("solid.extrude", json!({"sketch":"Base","distance":"20 mm"})).unwrap();
    h.app.run("solid.mirror", json!({"bodies":["Body1"],"plane":"YZ"})).unwrap();
    h.app.run("sketch.create", json!({"plane":{"face":[70,25,20]},"name":"Top"})).unwrap();
    h.app.run("sketch.line", json!({"points":[[-100,10],[-100,40]],"ids":["side"]})).unwrap();
    h.app.cam.target = solidcraft_engine::geom::Vec3::new(-60.0, 25.0, 20.0);
    h.app.cam.distance = 300.0;
    h.settle();
    let sketch = h.app.session.active_sketch.unwrap();
    let links = |h: &Harness| h.app.session.doc.sketch(sketch).unwrap().links.len();
    assert_eq!(links(&h), 1, "the plate's top face only");
    h.app.start("sketch.constraint.collinear");
    let line = h.call("ui.at", json!({"sketch":[-100,25]}))["result"].clone();
    h.step(&json!({ "click": line })).unwrap();
    let edge = h.call("ui.at", json!({"world":[-120,25,20]}))["result"].clone();
    h.step(&json!({ "click": edge })).unwrap();
    assert_eq!(links(&h), 2, "the copy's edge is projected");
    let (a, b) = (point(&h, "side.start"), point(&h, "side.end"));
    assert!((a.x + 120.0).abs() < 1e-6 && (b.x + 120.0).abs() < 1e-6, "on the copy's edge: {a:?} {b:?}");
    h.app.run("edit.undo", json!({})).unwrap();
    assert_eq!(links(&h), 1, "undone with the constraint");
    assert!((point(&h, "side.start").x + 100.0).abs() < 1e-6);
}

fn click_world(h: &mut Harness, at: [f64; 3]) {
    let at = h.call("ui.at", json!({ "world": at }))["result"].clone();
    h.step(&json!({ "click": at })).unwrap();
}

#[test]
fn coincident_onto_a_model_edge_projects_the_edge_first() {
    let mut h = box_and_line();
    h.app.start("sketch.constraint.coincident");
    let end = h.call("ui.at", json!({"sketch":[20,12]}))["result"].clone();
    h.step(&json!({ "click": end })).unwrap();
    // The box's top edge along y = 0, away from its corners.
    click_world(&mut h, [30.0, 0.0, 10.0]);
    let sk = h.app.session.doc.sketch(h.app.session.active_sketch.unwrap()).unwrap();
    assert_eq!(sk.links.len(), 1, "the edge was projected: {:?}", h.app.status);
    assert!(
        sk.constraints.iter().any(|c| matches!(c.kind, solidcraft_engine::sketch::ConstraintKind::PointOnCurve { .. })),
        "the end is on the projected edge: {:?}",
        h.app.status
    );
    let line = sk.curves.iter().find(|c| c.link.is_none()).unwrap().id.clone();
    let end = point(&h, &format!("{line}.end"));
    assert!(end.y.abs() < 1e-6, "{end:?}");
}

#[test]
fn another_sketchs_curve_is_projected_not_named_in_this_sketch() {
    let mut h = Harness::new();
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    h.app.run("sketch.circle.center", json!({"center":[0,0],"radius":10})).unwrap();
    h.app.finish_sketch();
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    h.app.run("sketch.line", json!({"points":[[20,20],[30,25]]})).unwrap();
    h.app.cam.distance = 120.0;
    h.settle();
    h.app.start("sketch.constraint.tangent");
    let line = h.call("ui.at", json!({"sketch":[25,22.5]}))["result"].clone();
    h.step(&json!({ "click": line })).unwrap();
    let circle = h.call("ui.at", json!({"sketch":[-10,0]}))["result"].clone();
    h.step(&json!({ "click": circle })).unwrap();
    let sk = h.app.session.doc.sketch(h.app.session.active_sketch.unwrap()).unwrap();
    assert_eq!(sk.links.len(), 1, "{:?}", h.app.status);
    assert!(sk.constraints.iter().any(|c| matches!(c.kind, solidcraft_engine::sketch::ConstraintKind::Tangent { .. })), "{:?}", h.app.status);
}
