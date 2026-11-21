//! The Symmetry tool's third pick may be the origin point or an origin axis (#1075); with three
//! lines, the construction line between the other two is the symmetry line in any pick order (#1796).
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

fn centred(h: &Harness) -> bool {
    [point(h, "mid.start"), point(h, "mid.end")].iter().all(|p| (p.x - 5.0).abs() < 1e-6)
}

// The number is painted ten pixels right and ten pixels above its picked entity.
fn marked_position(number: usize) -> [f64; 2] {
    let rect = crate::scenario::rect_of(&format!("viewport:constraint_pick:{number}")).expect("numbered pick marker");
    [rect.center().x as f64 - 10.0, rect.center().y as f64 + 10.0]
}

fn distance_to_segment(p: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
    let v = [b[0] - a[0], b[1] - a[1]];
    let length_squared = v[0] * v[0] + v[1] * v[1];
    let t = (((p[0] - a[0]) * v[0] + (p[1] - a[1]) * v[1]) / length_squared).clamp(0.0, 1.0);
    (p[0] - a[0] - t * v[0]).hypot(p[1] - a[1] - t * v[1])
}

/// Pending picks stay highlighted and numbered until the constraint runs (#18474).
#[test]
fn pending_symmetry_lines_show_their_pick_order_until_applied() {
    let mut h = between_two_lines();
    h.app.start("sketch.constraint.symmetry");
    h.frames(2);
    assert!(crate::scenario::rect_of("viewport:constraint_pick:1").is_none());
    for (n, xy) in [[0.0, 10.0], [10.0, 10.0]].into_iter().enumerate() {
        let at = h.call("ui.at", json!({"sketch":xy}))["result"].clone();
        h.step(&json!({"click":at})).unwrap();
        h.frames(2);
        for picked in 1..=n + 1 {
            assert!(crate::scenario::rect_of(&format!("viewport:constraint_pick:{picked}")).is_some(), "pick {picked} stays marked");
        }
        assert!(crate::scenario::rect_of(&format!("viewport:constraint_pick:{}", n + 2)).is_none(), "only completed picks are marked");
    }
    let left = [h.at(&json!({"sketch":[0.0, 0.0]})).unwrap(), h.at(&json!({"sketch":[0.0, 20.0]})).unwrap()];
    let right = [h.at(&json!({"sketch":[10.0, 20.0]})).unwrap(), h.at(&json!({"sketch":[10.0, 0.0]})).unwrap()];
    assert!(distance_to_segment(marked_position(1), left[0], left[1]) < 12.0, "first marker belongs to the left line");
    assert!(distance_to_segment(marked_position(2), right[0], right[1]) < 12.0, "second marker belongs to the right line");
    // The third pick completes Symmetry and clears the pending overlay.
    let at = h.call("ui.at", json!({"sketch":[3.0,10.0]}))["result"].clone();
    h.step(&json!({"click":at})).unwrap();
    h.frames(2);
    assert!(h.app.tool.as_ref().is_some_and(|t| t.picks.is_empty()));
    assert!(crate::scenario::rect_of("viewport:constraint_pick:1").is_none());
    assert!(crate::scenario::rect_of("viewport:constraint_pick:2").is_none());
    assert!(centred(&h));
}

/// Point picks use the same numbering as curves, including the original pick order.
#[test]
fn pending_symmetry_points_are_marked_before_picking_an_axis() {
    let mut h = two_points_picked();
    h.frames(2);
    assert!(crate::scenario::rect_of("viewport:constraint_pick:1").is_some());
    assert!(crate::scenario::rect_of("viewport:constraint_pick:2").is_some());
    assert!(crate::scenario::rect_of("viewport:constraint_pick:3").is_none());
    for (number, xy) in [(1, [10.0, 5.0]), (2, [-12.0, -3.0])] {
        let expected = h.at(&json!({"sketch":xy})).unwrap();
        let actual = marked_position(number);
        assert!((actual[0] - expected[0]).hypot(actual[1] - expected[1]) < 12.0, "marker {number} belongs to point {xy:?}");
    }
    let at = h.call("ui.at", json!({"sketch":[0,6]}))["result"].clone();
    h.step(&json!({"click":at})).unwrap();
    h.frames(2);
    assert!(crate::scenario::rect_of("viewport:constraint_pick:1").is_none());
    assert!(crate::scenario::rect_of("viewport:constraint_pick:2").is_none());
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
