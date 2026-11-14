//! Measure on a body's faces: the area comes from the exact surface and edges, not from the
//! display mesh's triangles, which run inside a curve (#9086, #12372).
#![allow(clippy::unwrap_used)]

use std::f64::consts::TAU;

use serde_json::{Value, json};
use solidcore_record::Vec3;
use solidcraft_geom::Vec3Geom;

use crate::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    match s.execute(id, &p) {
        Ok(v) => v,
        Err(e) => panic!("{id} {p}: {e}"),
    }
}

fn close(v: &Value, want: f64, tol: f64) -> bool {
    v.as_f64().is_some_and(|x| (x - want).abs() <= tol)
}

/// The first face whose analytic surface satisfies `want`, as a pick.
fn face_of(s: &Session, want: impl Fn(solidcraft_kernel::FaceSurface) -> bool) -> Value {
    let st = s.model.state();
    let b = st.bodies.first().unwrap();
    let m = b.mesh();
    let n = m.tri_face.iter().map(|f| *f as usize + 1).max().unwrap();
    let f = (0..n).find(|&f| solidcraft_kernel::face_surface(&b.body, f).is_some_and(&want)).expect("face");
    let p = m.triangles.iter().zip(&m.tri_face).find(|(_, tf)| **tf as usize == f).and_then(|(t, _)| m.tri(t)).unwrap()[0];
    json!({"type": "face", "body": b.name, "index": f, "point": p})
}

fn measure(s: &mut Session, items: Vec<Value>) -> Value {
    run(s, "inspect.measure", json!({ "items": items }))
}

/// A cylinder's side face is as big as its exact lateral area, 2πrh, in the same reply that
/// gives its exact radius and diameter: the area came from the display mesh's triangles, which
/// run inside the curve (#9086, the report's own cylinder).
#[test]
fn a_cylinders_side_face_is_as_big_as_its_exact_lateral_area() {
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"radius": 5, "height": 20, "base": [40, 10, 0], "operation": "new"}));
    let side = face_of(&s, |f| matches!(f, solidcraft_kernel::FaceSurface::Cylinder { radius, .. } if (radius - 5.0).abs() < 1e-6));
    let r = measure(&mut s, vec![side]);
    let item = &r["items"][0];
    assert!(close(&item["diameter"], 10.0, 1e-9), "the same reply gives the exact diameter: {r}");
    assert!(close(&item["area_mm2"], TAU * 5.0 * 20.0, 1e-6), "the side face's area is its exact 2πrh: {r}");
}

/// A hole's wall is as big as its exact lateral area, 2πrh: the area came from the display
/// mesh's triangles, which run inside the curve (#9086).
#[test]
fn a_holes_wall_is_as_big_as_its_exact_lateral_area() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 60, "width": 40, "height": 10}));
    run(&mut s, "solid.cylinder", json!({"radius": 3, "height": 12, "base": [15, 20, -1], "operation": "cut"}));
    let wall = face_of(&s, |f| matches!(f, solidcraft_kernel::FaceSurface::Cylinder { radius, .. } if (radius - 3.0).abs() < 1e-6));
    let r = measure(&mut s, vec![wall]);
    let item = &r["items"][0];
    assert!(close(&item["diameter"], 6.0, 1e-9), "the same reply gives the exact diameter: {r}");
    assert!(close(&item["area_mm2"], TAU * 3.0 * 10.0, 1e-6), "the hole's wall is its exact 2πrh: {r}");
}

/// A sphere's area is its exact 4πr², not the area of the display mesh's triangles (#9086).
#[test]
fn a_sphere_is_as_big_as_its_exact_surface() {
    let mut s = Session::default();
    run(&mut s, "solid.sphere", json!({"radius": 10, "base": [0, 0, 0], "operation": "new"}));
    let ball = face_of(&s, |f| matches!(f, solidcraft_kernel::FaceSurface::Sphere { radius, .. } if (radius - 10.0).abs() < 1e-6));
    let r = measure(&mut s, vec![ball]);
    let item = &r["items"][0];
    assert!(close(&item["diameter"], 20.0, 1e-9), "the same reply gives the exact diameter: {r}");
    assert!(close(&item["area_mm2"], 4.0 * std::f64::consts::PI * 100.0, 1e-4), "the sphere's area is its exact 4πr²: {r}");
}

/// A planar face with straight edges keeps its area.
///
/// The face picked is the box's side at x = 40 (30 × 10).
#[test]
fn a_flat_face_keeps_its_area() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 40, "width": 30, "height": 10}));
    let st = s.model.state();
    let b = st.bodies.first().unwrap();
    let m = b.mesh();
    let side = m.triangles.iter().zip(&m.tri_face).find(|(_, tf)| **tf as usize == 2).and_then(|(t, _)| m.tri(t)).unwrap()[0];
    let pick = json!({"type": "face", "body": b.name, "index": 2, "point": side});
    let r = measure(&mut s, vec![pick]);
    assert!(close(&r["items"][0]["area_mm2"], 300.0, 1e-6), "{r}");
}

/// The flat face at height `z` whose plane is across Z, as a pick.
fn flat_face_at(s: &Session, z: f64) -> Value {
    face_of(
        s,
        |f| matches!(f, solidcraft_kernel::FaceSurface::Plane { normal, point } if normal.vec().z.abs() > 1.0 - 1e-9 && (point.z - z).abs() < 1e-6),
    )
}

/// A flat face with round holes is the plate less its exact holes: the display mesh's
/// polygon for each hole runs inside the circle, so the mesh's holes were too small and the
/// face read 2293.875 mm² (#12372, the report's own plate).
#[test]
fn a_flat_face_with_round_holes_is_the_plate_less_its_exact_holes() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 60, "width": 40, "height": 10}));
    run(&mut s, "solid.cylinder", json!({"radius": 3, "height": 12, "base": [15, 20, -1], "operation": "cut"}));
    run(&mut s, "solid.cylinder", json!({"radius": 5, "height": 12, "base": [40, 20, -1], "operation": "cut"}));
    for z in [0.0, 10.0] {
        let face = flat_face_at(&s, z);
        let r = measure(&mut s, vec![face]);
        let want = 60.0 * 40.0 - std::f64::consts::PI * (9.0 + 25.0);
        assert!(close(&r["items"][0]["area_mm2"], want, 1e-6), "the face at z = {z} is 60 × 40 − π(3² + 5²) = {want}: {r}");
    }
}

/// A flat face whose outer edge is a circle is the exact disc, πr² (#12372).
#[test]
fn a_cylinders_end_is_its_exact_disc() {
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"radius": 5, "height": 20, "base": [40, 10, 0], "operation": "new"}));
    let face = flat_face_at(&s, 20.0);
    let r = measure(&mut s, vec![face]);
    assert!(close(&r["items"][0]["area_mm2"], std::f64::consts::PI * 25.0, 1e-9), "the end is the disc π·5²: {r}");
}

/// A flat face whose outer loop mixes straight edges and an arc: a quarter of a round bite
/// cut from a plate's corner leaves 60 × 40 − π·8²/4 (#12372).
#[test]
fn a_flat_face_with_an_arc_in_its_outline_is_exact() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 60, "width": 40, "height": 10}));
    run(&mut s, "solid.cylinder", json!({"radius": 8, "height": 12, "base": [0, 0, -1], "operation": "cut"}));
    let face = flat_face_at(&s, 10.0);
    let r = measure(&mut s, vec![face]);
    let want = 60.0 * 40.0 - std::f64::consts::PI * 64.0 / 4.0;
    assert!(close(&r["items"][0]["area_mm2"], want, 1e-6), "the plate less a quarter of the bite is {want}: {r}");
}

/// A flat face with an elliptic hole is the plate less the ellipse, πab: an edge that is neither
/// a line nor a circle is summed along its curve (#12372). The kernel keeps the ellipse as a
/// cubic spline fitted to it, which encloses 2·10⁻³ mm² more than πab here; the display mesh's
/// polygon read 2024.696 mm², 1.7 mm² off. Both faces read the same, though their loops run
/// the hole's edges opposite ways round.
#[test]
fn a_flat_face_with_an_elliptic_hole_is_the_plate_less_the_exact_ellipse() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 60, "width": 40, "height": 10}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Cut"}));
    run(&mut s, "sketch.ellipse", json!({"center": [30, 20], "major": [45, 20], "minor_radius": 8}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch": "Cut", "through_all": true, "operation": "cut", "direction": "symmetric"}));
    let want = 60.0 * 40.0 - std::f64::consts::PI * 15.0 * 8.0;
    let mut areas = Vec::new();
    for z in [0.0, 10.0] {
        let face = flat_face_at(&s, z);
        let r = measure(&mut s, vec![face]);
        assert!(close(&r["items"][0]["area_mm2"], want, 1e-2), "the face at z = {z} is 60 × 40 − π·15·8 = {want}: {r}");
        areas.push(r["items"][0]["area_mm2"].as_f64().unwrap());
    }
    assert!((areas[0] - areas[1]).abs() < 1e-9, "both faces read the same: {areas:?}");
}

/// The body's own volume is its exact πr²h, not the display mesh's (#9086).
///
/// The volume of a body is still measured on a tessellation of it: the exact volume of a
/// body is measured on its surfaces, which is a follow-up to the faces' own areas.
#[test]
#[ignore = "the body's volume still comes from a tessellation: the exact volume of a body is a follow-up (#9086)"]
fn a_cylinders_volume_is_its_exact_value() {
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"radius": 5, "height": 20, "base": [40, 10, 0], "operation": "new"}));
    let name = s.model.state().bodies.first().unwrap().name.clone();
    let r = measure(&mut s, vec![json!({"type": "body", "name": name})]);
    assert!(close(&r["items"][0]["volume_mm3"], std::f64::consts::PI * 25.0 * 20.0, 1e-3), "{r}");
}

/// The body's bounding box is its exact one: the tessellation's runs inside a curve (#9086).
///
/// The box of a body is still measured on a tessellation of it: the exact box of a body is
/// measured on its surfaces, which is a follow-up to the faces' own areas.
#[test]
#[ignore = "the body's box still comes from a tessellation: the exact box of a body is a follow-up (#9086)"]
fn a_cylinders_bounding_box_is_its_exact_one() {
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"radius": 5, "height": 20, "base": [40, 10, 0], "operation": "new"}));
    let r = run(&mut s, "inspect.measure", json!({}));
    let bb = &r["bodies"][0]["bbox"];
    assert!(close(&bb["min"][0], 35.0, 1e-6) && close(&bb["max"][0], 45.0, 1e-6), "the box's X runs from 35 to 45: {r}");
    assert!(close(&bb["min"][1], 5.0, 1e-6) && close(&bb["max"][1], 15.0, 1e-6), "the box's Y runs from 5 to 15: {r}");
}

/// The report's own cylinder, measured as the report measured it (#9086).
#[test]
fn the_reports_cylinder_measures_exactly() {
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"radius": 5, "height": 20, "base": [40, 10, 0], "operation": "new"}));
    let side = face_of(&s, |f| matches!(f, solidcraft_kernel::FaceSurface::Cylinder { radius, .. } if (radius - 5.0).abs() < 1e-6));
    let r = measure(&mut s, vec![side]);
    let area = r["items"][0]["area_mm2"].as_f64().unwrap();
    let want = TAU * 5.0 * 20.0;
    assert!((area - want).abs() / want < 1e-6, "the side face's area {area} against its exact {want}: {r}");
    let _ = Vec3::new(40.0, 10.0, 20.0);
}

/// A bore through the cylinder's wall cuts a rim that is not a circle: the band is no longer
/// the uncut 2πrh, so the exact shortcut gives way to the mesh's measure.
#[test]
fn a_bore_through_the_wall_is_not_measured_as_the_uncut_band() {
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"radius": 5, "height": 20, "base": [0, 0, 0], "operation": "new"}));
    run(&mut s, "solid.cylinder", json!({"radius": 1, "height": 14, "base": [-7, 0, 10], "axis": [1, 0, 0], "operation": "cut"}));
    let side = face_of(&s, |f| matches!(f, solidcraft_kernel::FaceSurface::Cylinder { radius, .. } if (radius - 5.0).abs() < 1e-6));
    let r = measure(&mut s, vec![side]);
    let area = r["items"][0]["area_mm2"].as_f64().unwrap();
    assert!(area < TAU * 5.0 * 20.0 - 1.0, "the bored wall's area {area} is less than the uncut {}: {r}", TAU * 5.0 * 20.0);
}

/// A flat face with no hole: its perimeter is the length of its four edges, with no outer and
/// inner split (#12048).
#[test]
fn a_box_faces_perimeter_is_the_length_of_its_edges() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 40, "width": 30, "height": 20}));
    let top = flat_face_at(&s, 20.0);
    let r = measure(&mut s, vec![top]);
    let item = &r["items"][0];
    assert!(close(&item["perimeter_mm"], 140.0, 1e-9), "2 x (40 + 30): {r}");
    assert!(item.get("outer_perimeter_mm").is_none() && item.get("inner_perimeter_mm").is_none(), "a face with no hole has one loop: {r}");
}

/// A flat face with two holes: the perimeter adds the outer loop and both circles, each circle at
/// its exact 2πr and not at the length of the polyline drawn for it (#12048).
#[test]
fn a_faces_perimeter_splits_into_its_outer_loop_and_its_holes() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 60, "width": 40, "height": 10}));
    run(&mut s, "solid.cylinder", json!({"radius": 3, "height": 12, "base": [15, 20, -1], "operation": "cut"}));
    run(&mut s, "solid.cylinder", json!({"radius": 5, "height": 12, "base": [40, 20, -1], "operation": "cut"}));
    let top = flat_face_at(&s, 10.0);
    let r = measure(&mut s, vec![top]);
    let item = &r["items"][0];
    let holes = TAU * 3.0 + TAU * 5.0;
    assert!(close(&item["outer_perimeter_mm"], 200.0, 1e-9), "2 x (60 + 40): {r}");
    assert!(close(&item["inner_perimeter_mm"], holes, 1e-9), "2π x (3 + 5): {r}");
    assert!(close(&item["perimeter_mm"], 200.0 + holes, 1e-9), "{r}");
}

/// The outer loop is the one around the holes, even when the holes together are the longer: three
/// long narrow slots in a small plate add up to more than the plate's outline (#12048).
#[test]
fn the_outer_loop_is_the_one_around_the_holes_not_the_longest() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 30, "width": 30, "height": 5}));
    for x in [5, 14, 23] {
        run(&mut s, "solid.box", json!({"corner": [x, 2, -1], "length": 2, "width": 26, "height": 7, "operation": "cut"}));
    }
    let top = flat_face_at(&s, 5.0);
    let r = measure(&mut s, vec![top]);
    let item = &r["items"][0];
    assert!(close(&item["outer_perimeter_mm"], 120.0, 1e-9), "the plate's outline: {r}");
    assert!(close(&item["inner_perimeter_mm"], 3.0 * 56.0, 1e-9), "three slots of 2 x (2 + 26): {r}");
}

/// A cylinder's side: both rims at their exact 2πr and not the seam that closes the surface, and
/// no outer or inner rim. A cap: its one rim. A whole sphere has no edge around it (#12048).
#[test]
fn a_curved_faces_perimeter_counts_its_rims_and_not_its_seam() {
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"radius": 5, "height": 20, "base": [40, 10, 0], "operation": "new"}));
    let side = face_of(&s, |f| matches!(f, solidcraft_kernel::FaceSurface::Cylinder { .. }));
    let r = measure(&mut s, vec![side]);
    let item = &r["items"][0];
    assert!(close(&item["perimeter_mm"], 2.0 * TAU * 5.0, 1e-9), "two rims and no seam: {r}");
    assert!(item.get("outer_perimeter_mm").is_none(), "a cylinder's rims are not outer and inner: {r}");
    let cap = flat_face_at(&s, 20.0);
    let r = measure(&mut s, vec![cap]);
    assert!(close(&r["items"][0]["perimeter_mm"], TAU * 5.0, 1e-9), "the cap's rim: {r}");

    let mut s = Session::default();
    run(&mut s, "solid.sphere", json!({"radius": 5, "operation": "new"}));
    let ball = face_of(&s, |f| matches!(f, solidcraft_kernel::FaceSurface::Sphere { .. }));
    let r = measure(&mut s, vec![ball]);
    assert!(r["items"][0].get("perimeter_mm").is_none(), "{r}");
}

/// Intersections and a positive gap are measured on curved surfaces, with both witnesses.
#[test]
fn exact_curved_measurement_has_exact_closest_points() {
    for sphere in [false, true] {
        for (start, end, want) in [([0., 10., 0.], [0., 10., 50.], 0.), ([0., 10., 17.], [0., 10., 18.], 0.), ([-5., 25., 0.], [5., 25., 0.], 5.)] {
            let mut s = Session::default();
            if sphere {
                run(&mut s, "solid.sphere", json!({"radius":20, "operation":"new"}));
            } else {
                run(&mut s, "solid.cylinder", json!({"radius":20, "height":40, "base":[-20,0,0], "axis":[1,0,0], "operation":"new"}));
            }
            let face = face_of(&s, |f| {
                if sphere {
                    matches!(f, solidcraft_kernel::FaceSurface::Sphere { .. })
                } else {
                    matches!(f, solidcraft_kernel::FaceSurface::Cylinder { .. })
                }
            });
            run(&mut s, "sketch.create", json!({"plane":"XY"}));
            run(&mut s, "sketch.options", json!({"sketch_3d":true}));
            let lines = run(&mut s, "sketch.line3d", json!({"points":[start,end]}));
            let sketch = run(&mut s, "sketch.finish", json!({}))["sketch"].as_u64().unwrap();
            let line = json!({"type":"sketch_curve", "sketch":sketch, "id":lines["created"]["wires"][0]});
            for items in [vec![face.clone(), line.clone()], vec![line, face]] {
                let r = measure(&mut s, items);
                println!("sphere={sphere}, segment={start:?}..{end:?}, expected={want}, min={}, from={}, to={}", r["min"], r["from"], r["to"]);
                assert!(close(&r["min"], want, 1e-6), "sphere={sphere}, want={want}: {r}");
                assert!(close(&r["distance_mm"], want, 1e-6), "{r}");
                let point = |name: &str| Vec3::new(r[name][0].as_f64().unwrap(), r[name][1].as_f64().unwrap(), r[name][2].as_f64().unwrap());
                let (p, q) = (point("from"), point("to"));
                assert!((p.dist(q) - want).abs() < 1e-6, "{r}");
                if want == 0. {
                    assert!(p.dist(Vec3::new(0., 10., 300_f64.sqrt())) < 1e-6, "{r}");
                }
            }
        }
    }
}

#[test]
fn exact_curved_measurement_of_a_straight_body_edge() {
    for sphere in [false, true] {
        let mut s = Session::default();
        if sphere {
            run(&mut s, "solid.sphere", json!({"radius":20, "operation":"new"}));
        } else {
            run(&mut s, "solid.cylinder", json!({"radius":20, "height":40, "base":[-20,0,0], "axis":[1,0,0], "operation":"new"}));
        }
        let face = face_of(&s, |f| {
            if sphere {
                matches!(f, solidcraft_kernel::FaceSurface::Sphere { .. })
            } else {
                matches!(f, solidcraft_kernel::FaceSurface::Cylinder { .. })
            }
        });
        run(&mut s, "solid.box", json!({"length":1, "width":1, "height":50, "corner":[0,10,0], "body_name":"Probe", "operation":"new"}));
        let r = measure(&mut s, vec![face, json!({"type":"edge", "body":"Probe", "point":[0,10,25]})]);
        println!("sphere={sphere}, body edge: min={}, from={}, to={}", r["min"], r["from"], r["to"]);
        assert!(close(&r["min"], 0., 1e-6), "sphere={sphere}: {r}");
        for name in ["from", "to"] {
            assert!(close(&r[name][0], 0., 1e-6) && close(&r[name][1], 10., 1e-6) && close(&r[name][2], 300_f64.sqrt(), 1e-6), "{r}");
        }
    }
}
