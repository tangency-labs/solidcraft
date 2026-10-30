//! A Single hole at a sketch point follows the point when the sketch changes (#2110), drilled
//! into the face it was placed on, and keeps doing so after the design is saved and reopened.

use std::f64::consts::PI;

use serde_json::{Value, json};
use solidcraft_geom::Vec3;

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
