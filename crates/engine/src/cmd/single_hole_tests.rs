//! A Single hole at a sketch point follows the point when the sketch changes (#2110), drilled
//! into the face it was placed on, and keeps doing so after the design is saved and reopened.

use solidcraft_geom::prelude::*;
use std::f64::consts::PI;

use serde_json::{Value, json};
use solidcore_record::Vec3;

use crate::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    match s.execute(id, &p) {
        Ok(v) => v,
        Err(e) => panic!("{id} {p}: {e}"),
    }
}

fn volume(s: &mut Session) -> f64 {
    run(s, "inspect.measure", json!({}))["total"]["volume_mm3"].as_f64().unwrap_or(f64::NAN)
}

fn solid_at(s: &Session, p: Vec3) -> bool {
    s.model.state().bodies.iter().any(|b| b.body.classify_point(p).unwrap() == solidcraft_kernel::PointClass::Inside)
}

/// The circle `c1`'s centre: its point id and where it is in the world.
fn centre(s: &Session) -> (String, Vec3) {
    let st = s.model.state();
    let ss = st.sketches.first().unwrap();
    ss.sketch
        .curves
        .iter()
        .find_map(|c| match c.kind {
            solidcraft_sketch::CurveKind::Circle { c, .. } => Some((ss.sketch.points[c].id.clone(), ss.plane.to_world(ss.sketch.points[c].pos))),
            _ => None,
        })
        .unwrap()
}

/// The bore (5 mm across, 10 mm deep, down from the top face) is about `at`: empty on its axis
/// and up to 2.4 mm from it, solid 2.6 mm from it in every direction.
fn bore_at(s: &Session, at: Vec3) {
    for k in 0..8 {
        let a = PI / 4.0 * f64::from(k);
        let p = |r: f64| at + Vec3::new(a.cos(), a.sin(), 0.0) * r - Vec3::new(0.0, 0.0, 5.0);
        assert!(!solid_at(s, p(0.0)) && !solid_at(s, p(2.4)), "the bore about {at:?} is empty {a} rad from +x");
        assert!(solid_at(s, p(2.6)), "the wall about {at:?} is solid {a} rad from +x");
    }
}

#[test]
fn a_single_hole_follows_its_sketch_point() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 40, "width": 30, "height": 20, "corner": [0, 0, 0]}));
    run(&mut s, "sketch.create", json!({"plane": {"face": [20, 15, 20]}, "name": "S"}));
    run(&mut s, "sketch.circle.center", json!({"center": [12, 9], "radius": 4, "id": "c1"}));
    run(&mut s, "sketch.finish", json!({}));
    let (id, before) = centre(&s);
    assert!((before.z - 20.0).abs() < 1e-9, "the centre is on the top face: {before:?}");
    let block = volume(&mut s);
    run(&mut s, "solid.hole", json!({"sketch": "S", "points": [id], "single": true, "diameter": 5, "depth": 10, "name": "H"}));
    let bore = PI * 2.5 * 2.5 * 10.0;
    assert!((block - volume(&mut s) - bore).abs() < 1e-3 * bore, "one 5 x 10 bore: {} of {bore}", block - volume(&mut s));
    bore_at(&s, before);

    // Move the centre 8 mm along the sketch's first axis: the hole goes with it.
    run(&mut s, "sketch.edit", json!({"sketch": "S"}));
    run(&mut s, "sketch.move_point", json!({"point": "c1.center", "to": [20, 9]}));
    run(&mut s, "sketch.finish", json!({}));
    let (_, after) = centre(&s);
    assert!((after.dist(before) - 8.0).abs() < 1e-6, "the centre moved 8 mm: {before:?} -> {after:?}");
    bore_at(&s, after);
    assert!(solid_at(&s, before - Vec3::new(0.0, 0.0, 5.0)), "the old spot is filled again");

    // Saved and reopened, the hole is still a Single one at the point.
    let mut t = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    t.refresh();
    bore_at(&t, after);
    let f = t.doc.find_feature("H").unwrap();
    assert!(matches!(&f.kind, solidcraft_doc::FeatureKind::Hole { points: Some(sp), .. } if sp.single && sp.ids == [id.clone()]), "{:?}", f.kind);
    assert!((block - volume(&mut t) - bore).abs() < 1e-3 * bore);
}

#[test]
fn a_single_hole_takes_one_point_and_no_flip() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 40, "width": 30, "height": 20, "corner": [0, 0, 0]}));
    run(&mut s, "sketch.create", json!({"plane": {"face": [20, 15, 20]}, "name": "S"}));
    run(&mut s, "sketch.circle.center", json!({"center": [12, 9], "radius": 4, "id": "c1"}));
    run(&mut s, "sketch.circle.center", json!({"center": [28, 9], "radius": 4, "id": "c2"}));
    run(&mut s, "sketch.finish", json!({}));
    let (id, _) = centre(&s);
    run(&mut s, "solid.hole", json!({"sketch": "S", "points": [id.clone()], "single": true, "diameter": 5}));
    run(&mut s, "edit.undo", json!({}));
    for p in
        [json!({"sketch": "S", "single": true, "diameter": 5}), json!({"sketch": "S", "points": [id], "single": true, "flip": true, "diameter": 5})]
    {
        assert!(s.execute("solid.hole", &p).is_err(), "{p}");
    }
}

/// A lid (rounded corners) with a countersunk hole 1.5 mm from its edge, and a second body `far` away from it.
/// Returns the lid's volume (the total less the far body's) after the hole and the bodies the hole changed.
fn lid_with_far_body(far_size: [f64; 3]) -> (f64, Value) {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "offset": "7.1 mm"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [-40.2, -9.85], "p1": [40.2, 13.05]}));
    run(&mut s, "sketch.fillet", json!({"points": ["l1.start", "l2.start", "l3.start", "l4.start"], "radius": "2 mm"}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance": "2 mm", "operation": "new", "body_name": "lid"}));
    run(
        &mut s,
        "solid.box",
        json!({"length": far_size[0], "width": far_size[1], "height": far_size[2], "corner": [158, -60, -5.9], "body_name": "far"}),
    );
    let r = run(
        &mut s,
        "solid.hole",
        json!({"position": [-38.7, -8.35, 9.1], "diameter": "2.4 mm", "type": "countersink", "cs_diameter": "4.2 mm", "cs_angle": "90 deg"}),
    );
    let lid = volume(&mut s) - far_size.iter().product::<f64>();
    (lid, r["changed_bodies"].clone())
}

/// #2904: the tool of a hole depends on the part it drills, not on how big some other body of the
/// model is: a large body far away leaves the countersink, the volume removed and the bodies
/// changed as they are with a 1 mm cube there.
#[test]
fn a_far_body_does_not_change_a_countersunk_hole() {
    let (small, small_changed) = lid_with_far_body([1.0, 1.0, 1.0]);
    let (large, large_changed) = lid_with_far_body([178.0, 160.0, 12.5]);
    assert!((small - large).abs() < 1e-3, "lid volume {small} with a 1 mm far body, {large} with a large one");
    assert_eq!(small_changed, large_changed, "bodies changed");
    assert_eq!(large_changed, json!(["lid"]), "only the lid is drilled");
}

fn body_volume(s: &mut Session, name: &str) -> f64 {
    let m = run(s, "inspect.measure", json!({}));
    let b = m["bodies"].as_array().and_then(|bs| bs.iter().find(|b| b["name"] == name)).unwrap_or_else(|| panic!("no body {name}: {m}"));
    b["volume_mm3"].as_f64().unwrap_or(f64::NAN)
}

/// #17358: a hole whose spot is over an opening in its own body starts there, on the face's plane;
/// it doesn't slide along its axis onto a body of another component under the opening. A 4 mm
/// Plate (component Lid, z 12…16) with a Ø5 bore through it, 2 mm above a Block in the root. A
/// countersunk Ø3.1 hole (Ø7, 90°) at the bore's centre cuts the countersink's ring into the
/// Plate (from r 3.5 at z 16 to r 2.5 at z 15) and leaves the Block alone: as a Single hole at a
/// point of a sketch on the Plate's top, and placed by position.
#[test]
fn a_hole_over_an_opening_in_its_body_cuts_that_body_not_one_under_it() {
    let ring = PI / 3.0 * (3.5 * 3.5 + 3.5 * 2.5 + 2.5 * 2.5) - PI * 2.5 * 2.5;
    let countersink = json!({"diameter": 3.1, "depth": 4, "type": "countersink", "cs_diameter": 7, "cs_angle": 90, "name": "H"});
    for single in [true, false] {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 40, "width": 40, "height": 10, "corner": [0, 0, 0], "body_name": "Block"}));
        run(&mut s, "component.create", json!({"name": "Lid"}));
        run(&mut s, "solid.box", json!({"length": 40, "width": 40, "height": 4, "corner": [0, 0, 12], "operation": "new", "body_name": "Plate"}));
        run(&mut s, "solid.hole", json!({"position": [20, 20, 16], "diameter": 5, "targets": ["Plate"], "name": "Bore"}));
        let plate = 6400.0 - PI * 2.5 * 2.5 * 4.0;
        assert!((body_volume(&mut s, "Plate") - plate).abs() < 1e-3, "the bore goes through the Plate");
        let mut hole = countersink.clone();
        if single {
            run(&mut s, "sketch.create", json!({"plane": {"face": [5, 5, 16]}, "name": "S", "project_edges": false}));
            run(&mut s, "sketch.point", json!({"point": [20, 20], "id": "m"}));
            run(&mut s, "sketch.finish", json!({}));
            let st = s.model.state();
            let ss = st.sketches.iter().find(|ss| ss.sketch.points.iter().any(|p| p.id == "m")).unwrap();
            let m = ss.sketch.points.iter().find(|p| p.id == "m").map(|p| ss.plane.to_world(p.pos)).unwrap();
            assert!(m.dist(Vec3::new(20.0, 20.0, 16.0)) < 1e-9, "the point is the bore's centre on the top: {m:?}");
            hole["sketch"] = json!("S");
            hole["points"] = json!(["m"]);
            hole["single"] = json!(true);
        } else {
            hole["position"] = json!([20, 20, 16]);
        }
        run(&mut s, "solid.hole", hole);
        let warning = s.model.result(s.doc.find_feature("H").unwrap().id).unwrap().warning.clone();
        assert_eq!(warning, None, "single {single}");
        let cut = plate - body_volume(&mut s, "Plate");
        assert!((cut - ring).abs() < 1e-3, "single {single}: the Plate loses the countersink's ring, {cut} of {ring} mm³");
        // The ring is cut at the Plate's top (r 3 is open 0.1 mm under it), not at its bottom.
        assert!(!solid_at(&s, Vec3::new(23.0, 20.0, 15.9)) && solid_at(&s, Vec3::new(23.0, 20.0, 12.1)), "single {single}: the ring is at the top");
        assert!((body_volume(&mut s, "Block") - 16_000.0).abs() < 1e-6, "single {single}: the Block is not drilled");
    }
}

/// #17358: a Single hole of the root, at a point of a sketch on the top of the Lid's Plate (a face
/// of a body it doesn't cut), still starts on that face: 8 mm down from z 16 reaches 2 mm into
/// the Block (top at z 10) under the 2 mm gap, not 8 mm from the Block's top.
#[test]
fn a_single_hole_on_another_components_face_starts_on_that_face() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 40, "width": 40, "height": 10, "corner": [0, 0, 0], "body_name": "Block"}));
    run(&mut s, "component.create", json!({"name": "Lid"}));
    run(&mut s, "solid.box", json!({"length": 40, "width": 40, "height": 4, "corner": [0, 0, 12], "operation": "new", "body_name": "Plate"}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    run(&mut s, "sketch.create", json!({"plane": {"face": [5, 5, 16]}, "name": "S", "project_edges": false}));
    run(&mut s, "sketch.point", json!({"point": [10, 10], "id": "m"}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.hole", json!({"sketch": "S", "points": ["m"], "single": true, "diameter": 3, "depth": 8, "name": "H"}));
    let cut = 16_000.0 - body_volume(&mut s, "Block");
    let bore = PI * 1.5 * 1.5 * 2.0;
    assert!((cut - bore).abs() < 1e-3, "the Block loses a 2 mm deep bore: {cut} of {bore} mm³");
    assert!((body_volume(&mut s, "Plate") - 6400.0).abs() < 1e-6, "the Plate is not drilled");
}

/// #17358: a hole of the root that drills only the Block, placed at points exactly on the round
/// side of the Lid's Rod (r 5, axis along x at y 20, z 17, 2 mm over the Block), starts on the
/// Rod: between its mesh's vertices the point lies off the mesh, and the Edit Hole dialog seeds
/// its Face pick where `hole_start` puts the stored point, which must not be the Block's top.
#[test]
fn a_hole_at_a_point_on_a_round_face_of_a_body_it_does_not_cut_starts_on_that_face() {
    for deg in [3.37f64, 7.5, 10.37] {
        let p = Vec3::new(20.0, 20.0 + 5.0 * deg.to_radians().sin(), 17.0 + 5.0 * deg.to_radians().cos());
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 40, "width": 40, "height": 10, "corner": [0, 0, 0], "body_name": "Block"}));
        run(&mut s, "component.create", json!({"name": "Lid"}));
        run(&mut s, "solid.cylinder", json!({"radius": 5, "height": 40, "base": [0, 20, 17], "axis": "X", "operation": "new", "body_name": "Rod"}));
        run(&mut s, "component.activate", json!({"component": "root"}));
        let hole = json!({"position": [p.x, p.y, p.z], "direction": [0, 0, -1], "diameter": 1, "depth": 1.5, "participants": ["Block"], "name": "H"});
        run(&mut s, "solid.hole", hole);
        let f = s.doc.find_feature("H").unwrap().clone();
        let start = solidcraft_doc::hole_start(&s.model.state(), &f, p, Vec3::new(0.0, 0.0, -1.0));
        assert!(start.dist(p) < 0.05, "{deg}°: starts on the Rod at {p:?}, not at {start:?}");
        assert!((body_volume(&mut s, "Block") - 16_000.0).abs() < 1e-6, "{deg}°: the Block is not drilled");
    }
}

/// #5041: a countersunk Ø6.4 hole (Ø13.7, 90°) 0.15 mm from a wall standing on the plate cuts
/// nothing above the entry face: the countersink's rim (y 29.85) stays clear of the wall (y 30).
/// 10 faces: bottom, the two L-shaped ends, front and back, the plate top, the wall's front and
/// top, the cone and the bore.
#[test]
fn a_countersink_beside_a_wall_stays_below_the_face() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 40, "width": 40, "height": 8, "corner": [0, 0, 0]}));
    run(&mut s, "solid.box", json!({"length": 40, "width": 10, "height": 30, "corner": [0, 30, 8], "operation": "join"}));
    run(
        &mut s,
        "solid.hole",
        json!({"position": [20, 23, 8], "direction": [0, 0, -1], "diameter": 6.4, "type": "countersink", "cs_diameter": 13.7, "cs_angle": 90}),
    );
    let st = s.model.state();
    let [body] = st.bodies.as_slice() else { panic!("one body, got {}", st.bodies.len()) };
    assert_eq!(body.body.face_count(), 10, "the wall is not notched");
    // In the wall's foot, 0.4 mm above the face: still solid.
    assert!(solid_at(&s, Vec3::new(20.0, 30.2, 8.4)), "the wall's foot is cut");
    let (rr, r, h) = (6.85_f64, 3.2_f64, 3.65_f64);
    let removed = PI * h / 3.0 * (rr * rr + rr * r + r * r) + PI * r * r * (8.0 - h);
    let want = 24800.0 - removed;
    let got = volume(&mut s);
    assert!((got - want).abs() < 1e-3, "volume {got}, want {want}");
}
