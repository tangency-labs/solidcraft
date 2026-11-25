//! Constraint tool picks: the Symmetry tool's third pick may be the origin point or an origin axis
//! (#1075); with three lines, the construction line between the other two is the symmetry line in
//! any pick order (#1796); a model edge or another sketch's curve is projected into the sketch first.
//! A constraint tool's pick may be a body's edge, a mirrored copy's too (#6348), and Collinear's or
//! Tangent's an origin axis (#15461).

use serde_json::json;
use solidcore_record::Vec2;
use solidcraft_engine::geom::prelude::*;

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
    let state = crate::open_meshing::session_world(&h.app.session);
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
    h.app.cam.target = solidcore_record::Vec3::new(-60.0, 25.0, 20.0);
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

/// #15461: with the Collinear tool running, a line and then the X axis are clicked: the line lands
/// on the axis, as it does with the sketch's own `x` given to the command.
#[test]
fn collinear_with_a_clicked_origin_axis() {
    let mut h = Harness::new();
    h.app.set_origin_shown(true);
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    h.app.run("sketch.line", json!({"points":[[5,8],[25,11]],"infer":false,"connect":false})).unwrap();
    h.app.cam.distance = 120.0;
    h.settle();
    h.app.start("sketch.constraint.collinear");
    // The line's middle, then the X axis (drawn from the origin along +x) away from the origin
    // point and the line.
    for xy in [[15.0, 9.5], [6.0, 0.0]] {
        let at = h.call("ui.at", json!({ "sketch": xy }))["result"].clone();
        h.step(&json!({ "click": at })).unwrap();
    }
    let (a, b) = (point(&h, "l1.start"), point(&h, "l1.end"));
    assert!(a.y.abs() < 1e-6 && b.y.abs() < 1e-6, "on the X axis: {a:?} {b:?} ({:?})", h.app.status);
}

/// A 40 x 30 x 20 box and a sketch above it, 30 up, with a point q. The Coincident tool picking q
/// and then the box's top edge at x = 40 at its middle ties q to that middle (#17920); picked
/// elsewhere along the edge, q only goes onto the edge.
#[test]
fn coincident_with_a_body_edge_at_its_middle_takes_the_middle() {
    for (at_middle, y) in [(true, 15.0), (false, 5.0)] {
        let mut h = Harness::new();
        h.app.run("solid.box", json!({"length":40,"width":30,"height":20})).unwrap();
        h.app.run("sketch.create", json!({"plane":"XY","offset":"30 mm"})).unwrap();
        h.app.run("sketch.point", json!({"point":[20,45],"id":"q"})).unwrap();
        h.app.cam.target = solidcore_record::Vec3::new(20.0, 20.0, 20.0);
        h.app.cam.distance = 200.0;
        h.settle();
        h.app.start("sketch.constraint.coincident");
        let q = h.call("ui.at", json!({"sketch":[20,45]}))["result"].clone();
        h.step(&json!({ "click": q })).unwrap();
        let edge = h.call("ui.at", json!({"world":[40.0, y, 20.0]}))["result"].clone();
        h.step(&json!({ "click": edge })).unwrap();
        let q = point(&h, "q");
        assert!((q.x - 40.0).abs() < 1e-6, "middle {at_middle}: q at {q:?} is on the edge ({:?})", h.app.status);
        assert_eq!((q.y - 15.0).abs() < 1e-6, at_middle, "middle {at_middle}: q at {q:?}");
    }
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
fn another_sketchs_curve_uses_legacy_picks_or_the_enhanced_projection_workflow() {
    for enhanced in [false, true] {
        let mut h = Harness::new();
        h.app.ui.project_link_tools = enhanced;
        h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
        h.app.run("sketch.circle.center", json!({"center":[0,0],"radius":10})).unwrap();
        h.app.finish_sketch();
        h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
        // Both sketches contain c1. Enhanced picking projects the foreign curve; the default
        // retains main's original ID routing and hit order.
        h.app.run("sketch.circle.center", json!({"center":[35,-20],"radius":3})).unwrap();
        let receiver_line = h.app.run("sketch.line", json!({"points":[[20,20],[30,25]]})).unwrap()["curves"][0].as_str().unwrap().to_string();
        h.app.cam.distance = 120.0;
        h.settle();
        h.app.start(if enhanced { "sketch.constraint.tangent" } else { "sketch.constraint.symmetry" });
        let line = h.call("ui.at", json!({"sketch":[25,22.5]}))["result"].clone();
        h.step(&json!({ "click": line })).unwrap();
        let before = h.app.session.doc.to_json();
        let circle = h.call("ui.at", json!({"sketch":[-10,0]}))["result"].clone();
        h.step(&json!({ "click": circle })).unwrap();
        let sk = h.app.session.doc.sketch(h.app.session.active_sketch.unwrap()).unwrap();
        if !enhanced {
            assert_eq!(h.app.session.doc.to_json(), before, "the default pick does not create a projection");
            assert_eq!(h.app.tool.as_ref().unwrap().picks, [receiver_line, "c1".to_string()], "main's entity IDs remain in hit order");
            continue;
        }
        assert_eq!(sk.links.len(), 1, "{:?}", h.app.status);
        assert!(sk.constraints.iter().any(|c| matches!(c.kind, solidcraft_engine::sketch::ConstraintKind::Tangent { .. })), "{:?}", h.app.status);
    }
}

#[test]
fn fix_keeps_the_generic_constraint_hint_with_projection_ui_off_or_on() {
    for enhanced in [false, true] {
        let mut h = Harness::new();
        h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
        h.app.ui.project_link_tools = enhanced;
        h.app.start("sketch.constraint.fix");
        assert_eq!(h.app.status.as_ref().unwrap().0, h.app.tr("sketch.hint.constraint.generic"));
    }
}
