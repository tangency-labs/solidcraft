//! #617: manual Horizontal/Vertical involving a circle's centre and a square's edge. Selecting
//! the centre and the edge (or picking both with the tool, or the context menu) must align the
//! centre with the edge; only the auto-constrain inference used to do it, while the manual path
//! silently did nothing.

use serde_json::json;
use solidcraft_engine::Sel::{SketchCurve, SketchPoint};
use solidcraft_engine::geom::Vec2;

use crate::context_menu::{self, Target};
use crate::scenario::Harness;

/// A fixed 100x100 square (`l1` bottom, `l2` right edge) with a radius-10 circle `c1` at (40, 60).
fn square_with_circle() -> Harness {
    let mut h = Harness::new();
    h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [100, 100]})).unwrap();
    h.app.run("sketch.constraint.fix", json!({"entities": ["l1", "l2", "l3", "l4"]})).unwrap();
    h.app.run("sketch.circle.center", json!({"center": [40, 60], "radius": 10, "id": "c1"})).unwrap();
    h.app.cam.distance = 320.0;
    h.settle();
    h
}

fn point(h: &Harness, id: &str) -> Vec2 {
    let sk = h.app.session.doc.sketch(h.app.session.active_sketch.unwrap()).unwrap();
    sk.point(sk.resolve_point(id).unwrap()).unwrap()
}

#[test]
fn selecting_the_centre_and_an_edge_preselects_the_alignment() {
    let h = square_with_circle();
    let state = h.app.session.world_state();
    let sk = &state.sketch(h.app.session.active_sketch.unwrap()).unwrap().sketch;
    let edge = || SketchCurve { id: "l2".into(), sketch: None };
    let centre = || SketchPoint { id: "c1.center".into() };
    for sel in [vec![centre(), edge()], vec![edge(), centre()]] {
        let params = crate::tools::constraint_preselection("sketch.constraint.horizontal_vertical", &sel, Some(sk)).unwrap();
        // The right edge is vertical: the alignment shares x with the line's midpoint.
        assert_eq!(params["mode"], "vertical", "{sel:?}");
        assert_eq!(params["points"][0], "c1.center", "{sel:?}");
        assert_eq!(params["points"][1]["midpoint"], "l2", "{sel:?}");
    }
    // The bottom edge is horizontal: the mode follows the line's own axis.
    let sel = vec![centre(), SketchCurve { id: "l1".into(), sketch: None }];
    let params = crate::tools::constraint_preselection("sketch.constraint.horizontal_vertical", &sel, Some(sk)).unwrap();
    assert_eq!(params["mode"], "horizontal");
    // A circle (not a line) beside the centre is not an alignment.
    let sel = vec![centre(), SketchCurve { id: "c1".into(), sketch: None }];
    assert!(crate::tools::constraint_preselection("sketch.constraint.horizontal_vertical", &sel, Some(sk)).is_none());
}

#[test]
fn the_hv_tool_aligns_a_picked_centre_with_a_picked_edge() {
    let mut h = square_with_circle();
    h.app.start("sketch.constraint.horizontal_vertical");
    // The circle's centre, then the square's right edge.
    for xy in [[40, 60], [100, 50]] {
        let at = h.call("ui.at", json!({"sketch": xy}))["result"].clone();
        h.step(&json!({"click": at})).unwrap();
    }
    assert!((point(&h, "c1.center").x - 100.0).abs() < 1e-6, "centre: {:?}", point(&h, "c1.center"));
}

#[test]
fn the_context_menu_aligns_the_centre_with_the_edge() {
    let mut h = square_with_circle();
    h.app.session.selection = vec![SketchPoint { id: "c1.center".into() }, SketchCurve { id: "l2".into(), sketch: None }];
    let items = context_menu::items(&h.app, &Target::Viewport);
    let item = items.iter().find(|i| i.label == "Vertical Constraint").expect("the menu offers Vertical Constraint");
    context_menu::run_item(&mut h.app, item, egui::Pos2::ZERO);
    assert!((point(&h, "c1.center").x - 100.0).abs() < 1e-6, "centre: {:?}", point(&h, "c1.center"));
}
