//! Partial planar contact must sew a solid without displacing or inventing material.
use crate::*;
use solidcraft_geom::Vec3;
use std::f64::consts::PI;

#[test]
fn split_contact_arc_crossing_one_edge_twice_preserves_the_body() {
    let a = box_solid(Vec3::ZERO, Vec3::new(25.0, 47.5, 37.5)).unwrap();
    let b = cylinder(Vec3::new(0.0, 7.5, 0.0), -Vec3::X, 5.0, 2.5).unwrap();
    let face = a.faces(0.01).unwrap().iter().position(|f| f.plane_normal.is_some_and(|n| n.dot(-Vec3::X) > 0.99)).unwrap();
    let side = b
        .record()
        .faces
        .iter()
        .position(|f| !matches!(f.surface, solidcraft_geom::brep::Surface::Plane { .. } | solidcraft_geom::brep::Surface::PlaneThrough { .. }))
        .unwrap();
    let r = split_faces(&a, &[face], &SplitTool::Face { body: &b, face: side }).unwrap();
    assert_eq!(r.face_count(), a.face_count() + 1);
    assert!(r.validity().is_empty(), "{:?}", r.validity());
    assert!(r.history().is_some());
    let (before, after) = (measure(&a).unwrap(), measure(&r).unwrap());
    assert!((before.volume - after.volume).abs() < 1e-7);
    assert!((before.area - after.area).abs() < 1e-7);
}

#[test]
fn partial_contact_retains_an_authored_split_away_from_the_contact() {
    let a = box_solid(Vec3::ZERO, Vec3::new(25.0, 47.5, 37.5)).unwrap();
    let face = a.faces(0.01).unwrap().iter().position(|f| f.plane_normal.is_some_and(|n| n.dot(Vec3::X) > 0.99)).unwrap();
    let a = split_faces(&a, &[face], &SplitTool::Plane(solidcraft_geom::Plane { origin: Vec3::new(0.0, 0.0, 15.0), ..solidcraft_geom::Plane::XY }))
        .unwrap();
    let b = cylinder(Vec3::new(0.0, 7.5, 0.0), -Vec3::X, 5.0, 2.5).unwrap();
    let r = boolean(&a, &b, BoolOp::Union).unwrap().unwrap();
    assert_eq!(r.split_lines(), a.split_lines());
    assert!(r.regularization_protected);
    assert!(r.validity().is_empty());
    let remote_faces =
        r.faces(0.01).unwrap().iter().filter(|f| f.plane_normal.is_some_and(|n| n.dot(Vec3::X) > 0.99) && (f.centroid.x - 25.0).abs() < 1e-7).count();
    assert_eq!(remote_faces, 2);
}

#[test]
fn partial_circular_contact_preserves_material_and_history() {
    for z in [-3.0_f64, 0.0, 3.0] {
        for angle in [0.0, 0.37] {
            let a = box_solid(Vec3::ZERO, Vec3::new(25.0, 47.5, 37.5)).unwrap();
            let b = cylinder(Vec3::new(0.0, 7.5, z), -Vec3::X, 5.0, 2.5).unwrap();
            let place = |body: &Body| transform(body, Vec3::new(4.0, -8.0, 2.0), Vec3::ZERO, Vec3::new(1.0, 2.0, 3.0), angle).unwrap();
            let (a, b) = (place(&a), place(&b));
            for reversed in [false, true] {
                let (a, b) = if reversed { (&b, &a) } else { (&a, &b) };
                let r = boolean(a, b, BoolOp::Union).unwrap().unwrap();
                assert!(r.validity().is_empty(), "z={z} angle={angle}: {:?}", r.validity());
                assert_eq!(r.shell_count(), 1);
                assert!(r.history().is_some(), "missing history z={z} angle={angle}");
                let m = measure(&r).unwrap();
                let volume = 25.0 * 47.5 * 37.5 + PI * 25.0 * 2.5;
                let contact = 25.0 * (-z / 5.0).acos() + z * (25.0 - z * z).sqrt();
                let area = 2.0 * (25.0 * 47.5 + 25.0 * 37.5 + 47.5 * 37.5) + 2.0 * PI * 5.0 * (5.0 + 2.5) - 2.0 * contact;
                assert!((m.volume - volume).abs() < 0.1, "z={z} angle={angle}: volume {} expected {volume}", m.volume);
                assert!((m.area - area).abs() < 0.2, "z={z} angle={angle}: area {} expected {area}", m.area);
            }
        }
    }
}
