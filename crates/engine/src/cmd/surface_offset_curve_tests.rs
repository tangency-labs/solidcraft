//! `surface.offset_curve` (#22414): curves at a distance along a face, checked against closed-form
//! values. The same three designs are the command-line examples in the pull request.

use serde_json::{Value, json};
use solidcore_record::Vec3;
use solidcraft_geom::brep::eval::CurveEval;
use solidcraft_geom::prelude::*;

use crate::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    match s.execute(id, &p) {
        Ok(v) => v,
        Err(e) => panic!("{id} {p}: {e}"),
    }
}

/// The exact pieces of the wire the reply names: their points, `n` steps each.
fn wire_points(s: &Session, reply: &Value, n: usize) -> Vec<Vec<Vec3>> {
    let st = s.model.state();
    let sk = &st.sketch(s.active_sketch.unwrap()).unwrap().sketch;
    let wires = reply["wires"].as_array().unwrap();
    assert!(!wires.is_empty(), "{reply}");
    let mut out = Vec::new();
    for id in wires {
        let w = &sk.wires[sk.wire_index(id.as_str().unwrap()).unwrap()];
        assert!(!w.exact.is_empty(), "the wire keeps its exact curve");
        for piece in &w.exact {
            let e = CurveEval::new(&piece.curve).unwrap();
            let [a, b] = piece.interval;
            out.push((0..=n).map(|k| e.point(a + (b - a) * k as f64 / n as f64).unwrap()).collect());
        }
    }
    out
}

fn length(pts: &[Vec3]) -> f64 {
    pts.windows(2).map(|w| w[0].dist(w[1])).sum()
}

/// The scratch sheet: an arc 40 wide rising 8 (radius 29, centre (20, −21)), extruded 30 up z.
fn scratch_sheet(s: &mut Session) {
    run(s, "sketch.create", json!({"plane": "XY", "name": "Arc"}));
    run(s, "sketch.arc.three_point", json!({"start": [0, 0], "end": [40, 0], "through": [20, 8]}));
    run(s, "sketch.finish", json!({}));
    run(s, "surface.extrude", json!({"sketch": "Arc", "curves": ["a1"], "distance": 30, "body_name": "Sheet"}));
}

#[test]
fn offset_curve_on_the_scratch_sheet_is_the_unrolled_offset() {
    let mut s = Session::default();
    scratch_sheet(&mut s);
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Marks"}));
    let r = run(&mut s, "surface.offset_curve", json!({"face": [20, 8, 15], "from": {"edge": [0, 0, 15]}, "distance": 10}));
    // 10 mm along the arc from (0, 0): its angle about the centre (20, −21) turns by 10/29.
    let phi = 21f64.atan2(-20.0) - 10.0 / 29.0;
    let (x, y) = (20.0 + 29.0 * phi.cos(), -21.0 + 29.0 * phi.sin());
    assert!((x - 8.276).abs() < 1e-3, "{x}");
    assert_eq!(r["clipped"], false, "{r}");
    assert!(r["deviation"].as_f64().unwrap() < 1e-6, "{r}");
    let pieces = wire_points(&s, &r, 8);
    assert_eq!(pieces.len(), 1);
    for p in &pieces[0] {
        assert!((p.x - x).abs() < 1e-6 && (p.y - y).abs() < 1e-6 && p.z > -1e-6 && p.z < 30.0 + 1e-6, "{p:?}");
    }
    assert!((length(&pieces[0]) - 30.0).abs() < 1e-6);
    // The arc edge at the bottom, 12 up the sheet: the same arc at z = 12.
    let r = run(&mut s, "surface.offset_curve", json!({"face": [20, 8, 15], "from": {"edge": [20, 8, 0]}, "distance": 12, "tolerance": 1e-5}));
    assert!(r["deviation"].as_f64().unwrap() <= 1e-5, "{r}");
    let pieces = wire_points(&s, &r, 400);
    let sweep = std::f64::consts::PI - 2.0 * 21f64.atan2(20.0);
    assert!((length(&pieces[0]) - 29.0 * sweep).abs() < 1e-3, "{}", length(&pieces[0]));
    assert!(pieces[0].iter().all(|p| (p.z - 12.0).abs() < 1e-6 && ((p.x - 20.0).hypot(p.y + 21.0) - 29.0).abs() < 1e-4));
}

#[test]
fn offset_curve_on_a_plate_from_an_edge_and_from_a_sketch_line() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Rect"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [40, 30]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "surface.patch", json!({"sketch": "Rect", "body_name": "Plate"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Mark"}));
    let line = run(&mut s, "sketch.line", json!({"points": [[20, 0], [20, 30]]}))["curves"][0].as_str().unwrap().to_string();
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "sketch.create", json!({"plane": "XZ", "name": "Offsets"}));
    // The bottom edge (y = 0), 7 in: y = 7 across the plate, 40 long.
    let r = run(&mut s, "surface.offset_curve", json!({"face": [20, 15, 0], "body": "Plate", "from": {"edge": [20, 0, 0]}, "distance": 7}));
    let p = &wire_points(&s, &r, 4)[0];
    assert!(p.iter().all(|q| (q.y - 7.0).abs() < 1e-9 && q.z.abs() < 1e-9), "{p:?}");
    assert!((length(p) - 40.0).abs() < 1e-9);
    // The line x = 20 of another sketch, 5 toward the face point's side (x = 30): x = 25, 30 long.
    let r = run(&mut s, "surface.offset_curve", json!({"face": [30, 15, 0], "from": {"sketch": "Mark", "curve": line}, "distance": 5}));
    let p = &wire_points(&s, &r, 4)[0];
    assert!(p.iter().all(|q| (q.x - 25.0).abs() < 1e-9), "{p:?}");
    assert!((length(p) - 30.0).abs() < 1e-9);
    // And to the other side.
    let r = run(&mut s, "surface.offset_curve", json!({"face": [10, 15, 0], "from": {"sketch": "Mark", "curve": line}, "distance": 5}));
    assert!(wire_points(&s, &r, 4)[0].iter().all(|q| (q.x - 15.0).abs() < 1e-9));
}

#[test]
fn offset_curve_up_a_cylinder_is_its_circle() {
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"base": [0, 0, 0], "radius": 6, "height": 20}));
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    let rim = [6.0 * 2f64.cos(), 6.0 * 2f64.sin()];
    let r = run(
        &mut s,
        "surface.offset_curve",
        json!({"face": [rim[0], rim[1], 10], "from": {"edge": [rim[0], rim[1], 0]}, "distance": 5, "tolerance": 1e-4}),
    );
    assert!(r["deviation"].as_f64().unwrap() <= 1e-4, "{r}");
    let total: f64 = wire_points(&s, &r, 800).iter().map(|p| length(p)).sum();
    // 2π · 6 = 37.699.
    assert!((total - std::f64::consts::TAU * 6.0).abs() < 1e-3, "{total}");
    for p in wire_points(&s, &r, 16).iter().flatten() {
        assert!((p.z - 5.0).abs() < 1e-6 && (p.x.hypot(p.y) - 6.0).abs() < 1e-4, "{p:?}");
    }
}

/// The plate of `offset_curve_on_a_plate_from_an_edge_and_from_a_sketch_line`, a sketch `Mark`
/// with the line x = 20 across it (its id returned) and an XZ sketch `Offsets` being edited.
fn plate_with_a_mark(s: &mut Session) -> String {
    run(s, "sketch.create", json!({"plane": "XY", "name": "Rect"}));
    run(s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [40, 30]}));
    run(s, "sketch.finish", json!({}));
    run(s, "surface.patch", json!({"sketch": "Rect", "body_name": "Plate"}));
    run(s, "sketch.create", json!({"plane": "XY", "name": "Mark"}));
    let line = run(s, "sketch.line", json!({"points": [[20, 0], [20, 30]]}))["curves"][0].as_str().unwrap().to_string();
    run(s, "sketch.finish", json!({}));
    run(s, "sketch.create", json!({"plane": "XZ", "name": "Offsets"}));
    line
}

/// The offset-curve link of the sketch being edited.
fn the_link(s: &Session, link: &str) -> solidcraft_sketch::LinkSource {
    let st = s.model.state();
    st.sketch(s.active_sketch.unwrap()).unwrap().sketch.link(link).unwrap().source.clone()
}

/// Flip: the face point at x = 30 picks the x > 20 side, flipped the curve goes to x = 15; the
/// link keeps `flip`, and an edge refuses it (its offset always goes into the face).
#[test]
fn offset_curve_flip_goes_to_the_other_side_of_the_face_point() {
    let mut s = Session::default();
    let line = plate_with_a_mark(&mut s);
    let r = run(&mut s, "surface.offset_curve", json!({"face": [30, 15, 0], "from": {"sketch": "Mark", "curve": line}, "distance": 5, "flip": true}));
    let p = &wire_points(&s, &r, 4)[0];
    assert!(p.iter().all(|q| (q.x - 15.0).abs() < 1e-9 && q.z.abs() < 1e-9), "{p:?}");
    assert!((length(p) - 30.0).abs() < 1e-9);
    assert!(matches!(the_link(&s, r["link"].as_str().unwrap()), solidcraft_sketch::LinkSource::FaceOffset { flip: true, .. }));
    // Not flipped (false is the default): x = 25, and the saved link has no `flip` key.
    let r =
        run(&mut s, "surface.offset_curve", json!({"face": [30, 15, 0], "from": {"sketch": "Mark", "curve": line}, "distance": 5, "flip": false}));
    assert!(wire_points(&s, &r, 4)[0].iter().all(|q| (q.x - 25.0).abs() < 1e-9));
    let saved = serde_json::to_value(the_link(&s, r["link"].as_str().unwrap())).unwrap();
    assert!(saved.get("flip").is_none(), "{saved}");
    let e = s
        .execute("surface.offset_curve", &json!({"face": [20, 15, 0], "from": {"edge": [20, 0, 0]}, "distance": 7, "flip": true}))
        .unwrap_err()
        .to_string();
    assert!(e.contains("always goes into the face"), "{e}");
    let e = s
        .execute("surface.offset_curve", &json!({"face": [20, 15, 0], "from": {"edge": [20, 0, 0]}, "distance": 7, "flip": 1}))
        .unwrap_err()
        .to_string();
    assert!(e.contains("flip"), "{e}");
}

/// `link`: an offset curve changed in place keeps its link and 3D curve ids, in one undo step.
#[test]
fn offset_curve_link_changes_an_offset_curve_in_place() {
    let mut s = Session::default();
    let line = plate_with_a_mark(&mut s);
    let made = run(&mut s, "surface.offset_curve", json!({"face": [20, 15, 0], "body": "Plate", "from": {"edge": [20, 0, 0]}, "distance": 7}));
    let (link, wire) = (made["link"].as_str().unwrap().to_string(), made["wires"][0].as_str().unwrap().to_string());
    // The same link, now 12 from the edge: y = 12, still 40 long.
    let r = run(&mut s, "surface.offset_curve", json!({"face": [20, 15, 0], "from": {"edge": [20, 0, 0]}, "distance": 12, "link": link}));
    assert_eq!((r["link"].as_str(), r["wires"][0].as_str()), (Some(link.as_str()), Some(wire.as_str())), "{r}");
    let p = &wire_points(&s, &r, 4)[0];
    assert!(p.iter().all(|q| (q.y - 12.0).abs() < 1e-9 && q.z.abs() < 1e-9), "{p:?}");
    assert!((length(p) - 40.0).abs() < 1e-9);
    let st = s.model.state();
    let sk = &st.sketch(s.active_sketch.unwrap()).unwrap().sketch;
    assert_eq!((sk.links.len(), sk.wires.len()), (1, 1));
    drop(st);
    // Now from the Mark line, 5 toward x = 10, flipped: x = 25, 30 long; then one undo is y = 12.
    let r = run(
        &mut s,
        "surface.offset_curve",
        json!({"face": [10, 15, 0], "from": {"sketch": "Mark", "curve": line}, "distance": 5, "flip": true, "tolerance": 0.001, "link": link}),
    );
    assert!(wire_points(&s, &r, 4)[0].iter().all(|q| (q.x - 25.0).abs() < 1e-9));
    let stored = |s: &Session| match the_link(s, &link) {
        solidcraft_sketch::LinkSource::FaceOffset { flip, distance, tolerance, .. } => (flip, distance, tolerance),
        other => panic!("{other:?}"),
    };
    assert_eq!(stored(&s), (true, 5.0, Some(0.001)));
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(stored(&s), (false, 12.0, None));
    // A link that is not an offset curve, and one that is not there, are refused and change nothing.
    let other = run(&mut s, "sketch.isoparametric_curve", json!({"face": [20, 15, 0]}))["link"].as_str().unwrap().to_string();
    let before = s.doc.to_json();
    for bad in [other.as_str(), "nope"] {
        let e = s
            .execute("surface.offset_curve", &json!({"face": [20, 15, 0], "from": {"edge": [20, 0, 0]}, "distance": 3, "link": bad}))
            .unwrap_err()
            .to_string();
        assert!(e.contains(bad), "{e}");
    }
    assert_eq!(s.doc.to_json(), before);
}

/// The pull request's examples 2 and 3: an offset curve changed in place on the scratch sheet
/// (10 then 5 mm from the straight edge; 50 is refused and keeps it) and up a cylinder (5 then
/// 8 mm from the rim), with the refusals of a missing link and a `flip` that is not a boolean.
#[test]
fn offset_curve_edits_on_the_scratch_sheet_and_the_cylinder() {
    let mut s = Session::default();
    scratch_sheet(&mut s);
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Marks"}));
    let call = |d: f64| json!({"face": [20, 8, 15], "from": {"edge": [0, 0, 15]}, "distance": d});
    let made = run(&mut s, "surface.offset_curve", call(10.0));
    let link = made["link"].as_str().unwrap().to_string();
    let mut edit = call(5.0);
    edit["link"] = json!(link);
    let r = run(&mut s, "surface.offset_curve", edit);
    // 5 mm along the arc from (0, 0): the angle about (20, −21) turns by 5/29.
    let phi = 21f64.atan2(-20.0) - 5.0 / 29.0;
    let (x, y) = (20.0 + 29.0 * phi.cos(), -21.0 + 29.0 * phi.sin());
    assert!((x - 3.8993).abs() < 1e-3 && (y - 3.1201).abs() < 1e-3, "{x} {y}");
    let p = &wire_points(&s, &r, 8)[0];
    assert!(p.iter().all(|q| (q.x - x).abs() < 1e-6 && (q.y - y).abs() < 1e-6), "{p:?}");
    assert!((length(p) - 30.0).abs() < 1e-6);
    assert_eq!(r["wires"], made["wires"]);
    let mut wide = call(50.0);
    wide["link"] = json!(link);
    let before = s.doc.to_json();
    assert!(s.execute("surface.offset_curve", &wide).unwrap_err().to_string().contains("runs off the face everywhere"));
    assert_eq!(s.doc.to_json(), before, "a refused edit keeps the curve as it was");

    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"base": [0, 0, 0], "radius": 6, "height": 20}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Rings"}));
    let rim = [6.0 * 2f64.cos(), 6.0 * 2f64.sin()];
    let ring = |d: f64| json!({"face": [rim[0], rim[1], 10], "from": {"edge": [rim[0], rim[1], 0]}, "distance": d, "tolerance": 1e-4});
    let made = run(&mut s, "surface.offset_curve", ring(5.0));
    let mut edit = ring(8.0);
    edit["link"] = made["link"].clone();
    let r = run(&mut s, "surface.offset_curve", edit);
    assert!(r["deviation"].as_f64().unwrap() <= 1e-4, "{r}");
    let total: f64 = wire_points(&s, &r, 800).iter().map(|p| length(p)).sum();
    assert!((total - std::f64::consts::TAU * 6.0).abs() < 1e-3, "{total}");
    for p in wire_points(&s, &r, 16).iter().flatten() {
        assert!((p.z - 8.0).abs() < 1e-6 && (p.x.hypot(p.y) - 6.0).abs() < 1e-4, "{p:?}");
    }
    let mut missing = ring(3.0);
    missing["link"] = json!("nope");
    assert!(s.execute("surface.offset_curve", &missing).unwrap_err().to_string().contains("no link `nope`"));
    let mut not_bool = ring(3.0);
    not_bool["flip"] = json!("yes");
    assert!(s.execute("surface.offset_curve", &not_bool).unwrap_err().to_string().contains("flip"));
}

#[test]
fn offset_curve_refusals() {
    let mut s = Session::default();
    scratch_sheet(&mut s);
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Off"}));
    let off = run(&mut s, "sketch.line", json!({"points": [[10, -10], [30, -10]]}))["curves"][0].as_str().unwrap().to_string();
    run(&mut s, "sketch.finish", json!({}));
    let call = json!({"face": [20, 8, 15], "from": {"edge": [0, 0, 15]}, "distance": 10});
    // No sketch being edited.
    assert!(s.execute("surface.offset_curve", &call).is_err());
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Marks"}));
    let err = |s: &mut Session, p: Value| s.execute("surface.offset_curve", &p).unwrap_err().to_string();
    // The sheet is 44.1 mm across: 50 runs off it everywhere.
    let e = err(&mut s, json!({"face": [20, 8, 15], "from": {"edge": [0, 0, 15]}, "distance": 50}));
    assert!(e.contains("runs off the face everywhere"), "{e}");
    let e = err(&mut s, json!({"face": [20, 8, 15], "from": {"edge": [0, 0, 15]}, "distance": 0}));
    assert!(e.contains("more than 0"), "{e}");
    let e = err(&mut s, json!({"face": [20, 8, 15], "from": {"body": "Sheet"}, "distance": 5}));
    assert!(e.contains("`from` must be"), "{e}");
    let e = err(&mut s, json!({"face": [20, 8, 15], "from": {"edge": [0, 0, 15]}, "distance": 5, "tolerance": -1}));
    assert!(e.contains("tolerance"), "{e}");
    // A line of a sketch (made before this one) that does not lie on the sheet.
    let e = err(&mut s, json!({"face": [20, 8, 15], "from": {"sketch": "Off", "curve": off}, "distance": 5}));
    assert!(e.contains("not on the face"), "{e}");
    // Nothing was added by the refusals.
    let st = s.model.state();
    assert!(st.sketch(s.active_sketch.unwrap()).unwrap().sketch.wires.is_empty());
}
