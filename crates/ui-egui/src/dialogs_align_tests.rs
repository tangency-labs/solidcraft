use serde_json::json;
use solidcore_record::Vec3;
use solidcraft_engine::Sel;
use solidcraft_engine::geom::prelude::*;

use crate::dialogs::{Kind, apply_commands, edge_sel};
use crate::scenario::Harness;
use crate::viewport::Hit;

/// A 60 x 40 x 10 plate with a hole of radius 4 through it at (45, 20), and a peg of the hole's
/// size, 6 tall, standing on XY at (100, 20).
fn plate_and_peg() -> Harness {
    let mut h = Harness::new();
    h.app.run("solid.box", json!({"length": 60, "width": 40, "height": 10, "body_name": "Plate"})).unwrap();
    h.app.run("solid.cylinder", json!({"radius": 4, "height": 12, "base": [45, 20, -1], "operation": "cut"})).unwrap();
    h.app.run("solid.cylinder", json!({"radius": 4, "height": 6, "base": [100, 20, 0], "body_name": "Peg"})).unwrap();
    h.app.cam.distance = 200.0;
    h.settle();
    h.app.start("solid.align");
    h.frames(2);
    h
}

/// Clicks the edge at `at` into input `input`, as the viewport hands it to the dialog.
fn pick_edge(h: &mut Harness, input: usize, at: Vec3) {
    let Some(Sel::Edge { body, index, point }) = edge_sel(&h.app.session, at) else { panic!("no edge at {at:?}") };
    let d = h.app.dialog.as_mut().unwrap();
    d.active = input;
    let sel = d.candidate(&h.app.session, &Hit::Edge { body, index, mid: point, point }).expect("the input takes the edge");
    d.pick(&h.app.session, sel);
}

fn set_flip(h: &mut Harness, on: bool) {
    if let Some(Kind::Align { flip, .. }) = h.app.dialog.as_mut().map(|d| &mut d.kind) {
        *flip = on;
    }
}

/// Applies the dialog's command and gives `body`'s bounds after it.
fn apply_and_bound(h: &mut Harness, body: &str) -> (serde_json::Value, [Vec3; 2]) {
    let (cmd, p) = apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap().remove(0);
    assert_eq!(cmd, "solid.align");
    h.app.run(&cmd, p.clone()).unwrap();
    let st = crate::open_meshing::session_world(&h.app.session);
    let b = st.body(body).unwrap().mesh().bounds();
    (p, [b.min, b.max])
}

fn assert_on_hole_axis([min, max]: [Vec3; 2], z: [f64; 2], why: &str) {
    let (cx, cy) = ((min.x + max.x) / 2.0, (min.y + max.y) / 2.0);
    assert!((cx - 45.0).abs() < 0.05 && (cy - 20.0).abs() < 0.05, "{why}: not on the hole's axis: {min:?} {max:?}");
    assert!((min.z - z[0]).abs() < 1e-6 && (max.z - z[1]).abs() < 1e-6, "{why}: z {z:?}: {min:?} {max:?}");
}

/// A round edge picked for From and To is its circle (a frame with an axis), not its centre
/// point: the peg lands concentric on the hole's rim, and Flip turns it over into the hole.
#[test]
fn round_edges_align_as_circles_and_flip_turns_the_body_over() {
    for (flip, z) in [(false, [10.0, 16.0]), (true, [4.0, 10.0])] {
        let mut h = plate_and_peg();
        h.app.dialog.as_mut().unwrap().inputs[0].items = vec![Sel::Body { name: "Peg".into() }];
        set_flip(&mut h, flip);
        pick_edge(&mut h, 1, Vec3::new(104.0, 20.0, 0.0));
        pick_edge(&mut h, 2, Vec3::new(49.0, 20.0, 10.0));
        let (p, b) = apply_and_bound(&mut h, "Peg");
        assert!(p["from_frame"]["circle"].is_array() && p["to_frame"]["circle"].is_array(), "{p}");
        assert!(p.get("from").is_none() && p.get("to").is_none(), "{p}");
        assert_on_hole_axis(b, z, &format!("flip {flip}"));
    }
}
