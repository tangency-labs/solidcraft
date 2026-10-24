//! The Symmetry tool's third pick may be the origin point or an origin axis (#1075); with three
//! lines, the construction line between the other two is the symmetry line in any pick order (#1796).

use serde_json::json;
use solidcraft_engine::geom::Vec2;

use crate::scenario::Harness;

/// An XY sketch with points p (10, 5) and q (-12, -3), the Symmetry tool started and both picked.
fn two_points_picked() -> Harness {
    let mut h = Harness::new();
    h.app.ui.show_origin = true;
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
    let sel = |ids: [&str; 3]| ids.map(|id| SketchCurve { id: id.into() });
    for ids in [["mid", "left", "right"], ["right", "mid", "left"]] {
        let params = crate::tools::constraint_preselection("sketch.constraint.symmetry", &sel(ids), Some(sk)).unwrap();
        assert_eq!(params["line"], "mid", "{ids:?}");
    }
    // Without a construction line among them, the last one stays the symmetry line.
    let params = crate::tools::constraint_preselection("sketch.constraint.symmetry", &sel(["left", "right", "mid"]), None).unwrap();
    assert_eq!(params["line"], "mid");
}
