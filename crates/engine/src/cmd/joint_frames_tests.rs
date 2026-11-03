//! A joint origin on a face or a hole keeps its turn about z as the geometry tilts.

use serde_json::{Value, json};
use solidcore_record::Vec3;
use solidcraft_geom::prelude::*;

use super::tests::{occ, run};
use crate::Session;

/// A 40 x 40 x 2 plate in component Panel, turned about Y by the `tilt` parameter (a Move feature),
/// and a 10 x 4 x 4 bar in component Bar, long along its X.
fn tilted(tilt: f64) -> Session {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "tilt", "expression": format!("{tilt} deg")}));
    run(&mut s, "component.create", json!({"name": "Panel"}));
    run(&mut s, "solid.box", json!({"length": 40, "width": 40, "height": 2, "corner": [-20, -20, 0]}));
    let plate = s.model.state().bodies.last().map(|b| b.name.clone()).unwrap_or_default();
    run(&mut s, "solid.move", json!({"bodies": [plate], "axis": [0, 1, 0], "angle": "tilt"}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    run(&mut s, "component.create", json!({"name": "Bar"}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 4, "height": 4, "corner": [100, 0, 0]}));
    s
}

/// A point on the plate's top face (the largest face facing up).
fn top(s: &mut Session) -> Value {
    let body = s.model.state().bodies.first().map(|b| b.name.clone()).unwrap_or_default();
    let faces = run(s, "model.faces", json!({"body": body}));
    let best = faces["faces"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|f| f["plane_normal"][2].as_f64().is_some_and(|z| z > 0.3))
        .max_by(|a, b| a["area"].as_f64().unwrap_or(0.0).total_cmp(&b["area"].as_f64().unwrap_or(0.0)))
        .cloned()
        .unwrap_or_default();
    best["point"].clone()
}

/// How far the bar's long axis is turned from Y, the plate's axis of tilt (degrees, 0…90).
fn bar_from_tilt_axis(s: &Session) -> f64 {
    let m = solidcraft_doc::joints::occurrence_world(&s.doc, occ(s, "Bar"));
    let x = solidcraft_doc::apply_vector(&m, Vec3::X);
    x.dot(Vec3::Y).abs().min(1.0).acos().to_degrees()
}

#[test]
fn a_part_on_a_tilting_face_keeps_its_turn_where_the_face_normal_crosses_0_9_along_x() {
    // The plate starts at 60°: its normal is 0.87 along X. Past 64.2° it is over 0.9, where a
    // frame worked out from the normal alone picks its x another way and the bar turned a
    // right angle on the face.
    let mut s = tilted(60.0);
    let a = top(&mut s);
    run(
        &mut s,
        "joint.create",
        json!({"type": "rigid", "a": {"occurrence": "Panel", "face": a}, "b": {"occurrence": "Bar", "face": [105, 2, 0]}, "name": "Seat"}),
    );
    let start = bar_from_tilt_axis(&s);
    for tilt in [62.0, 64.0, 66.0, 70.0, 80.0, 50.0, 30.0] {
        run(&mut s, "parameters.change", json!({"name": "tilt", "expression": format!("{tilt} deg")}));
        let now = bar_from_tilt_axis(&s);
        assert!((now - start).abs() < 1e-6, "at {tilt}°: the bar is {now:.3}° from the tilt axis, {start:.3}° at first");
        let r = run(&mut s, "joint.solve", json!({}));
        assert_eq!(r["errors"], json!([]), "{r}");
    }
    // The origin keeps that x in the design.
    let snap = s.doc.assembly.joints.first().map(|j| j.a.snap.clone());
    assert!(snap.as_ref().and_then(solidcraft_doc::joints::Snap::frame_x).is_some(), "{snap:?}");
}

#[test]
fn an_origin_saved_without_a_kept_x_is_placed_as_before() {
    let mut s = tilted(30.0);
    let a = top(&mut s);
    run(
        &mut s,
        "joint.create",
        json!({"type": "rigid", "a": {"occurrence": "Panel", "face": a}, "b": {"occurrence": "Bar", "face": [105, 2, 0]}, "name": "Seat"}),
    );
    let kept = bar_from_tilt_axis(&s);
    // The same joint as an older design has it: no x kept on either side. It is placed as
    // a frame from the normal alone places it, which here is where the kept x placed it too.
    let mut doc = (*s.doc).clone();
    for j in &mut doc.assembly.joints {
        for o in [&mut j.a, &mut j.b] {
            if let solidcraft_doc::joints::Snap::FaceCenter { x, .. } | solidcraft_doc::joints::Snap::CircleCenter { x, .. } = &mut o.snap {
                *x = None;
            }
        }
    }
    let text = serde_json::to_string(&doc.assembly).unwrap_or_default();
    assert!(!text.contains("\"x\""), "{text}");
    *s.doc_mut() = doc;
    run(&mut s, "joint.solve", json!({}));
    assert!((bar_from_tilt_axis(&s) - kept).abs() < 1e-6);
}
