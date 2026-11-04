//! Measure on a body's curved faces: the area comes from the exact surface, not from the
//! display mesh's triangles, which run inside a curve (#9086).
#![allow(clippy::unwrap_used)]

use std::f64::consts::TAU;

use serde_json::{Value, json};
use solidcore_record::Vec3;

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

/// A planar face's area is unchanged: it is exact on the mesh already.
///
/// The face picked is the box's side at x = 40 (30 × 10), which the mesh measures exactly.
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
