//! A face thinner than the tolerance still meshes: the lens of a sphere's patch that pokes out
//! of a box's side has two edges that, sampled coarsely, run along one line in its parameters.
//! The shell's mesh had a hole there.
//!
//! So does a face pinched to a point, where its boundary touches itself within the tolerance (a
//! sphere touching a box's edge leaves one on the box, #1648).

use std::collections::HashMap;

use solidcraft_geom::Vec3;
use truck_meshalgo::prelude::*;
use truck_modeling as mt;

use crate::body::p3;
use crate::{BoolOp, build::box_solid, build::sphere, ops::boolean};

/// Triangle sides in the mesh with no triangle on the other side of them, with the points
/// within a thousandth of a millimetre taken as one.
fn open_sides(mesh: &solidcraft_geom::Mesh) -> usize {
    let key = |i: u32| {
        let p = mesh.positions[i as usize];
        [p.x, p.y, p.z].map(|c| (c * 1e3).round() as i64)
    };
    let mut sides: HashMap<([i64; 3], [i64; 3]), u32> = HashMap::new();
    for t in &mesh.triangles {
        for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
            *sides.entry((key(a), key(b))).or_default() += 1;
        }
    }
    sides.keys().filter(|(a, b)| !sides.contains_key(&(*b, *a))).count()
}

/// The sphere's equator lies in the box's top and its circle on the box's side x = 22.5 dips to
/// z = 24.7, while the edge of the sphere's bottom patch bulges 0.15 mm past that side: a
/// face of about 0.35 mm² between the two. At 1 % of the body it had no triangle.
#[test]
fn a_lens_thinner_than_the_tolerance_is_meshed() {
    let a = sphere(Vec3::new(20.0, 10.0, 27.5), 3.75).unwrap();
    let b = box_solid(Vec3::new(15.0, 0.0, 22.5), Vec3::new(22.5, 20.0, 27.5)).unwrap();
    let union = boolean(&a, &b, BoolOp::Union).unwrap().unwrap();
    assert!(union.validity().is_empty(), "{:?}", union.validity());
    let size = union.size();
    for tol in [size * 1e-2, size * 2.5e-3, size * 1e-3] {
        let mesh = union.mesh(&crate::MeshTolerance::chord(tol)).unwrap();
        let faces: std::collections::HashSet<u32> = mesh.tri_face.iter().copied().collect();
        assert_eq!(faces.len(), union.face_count(), "faces with triangles at {tol}");
        assert_eq!(open_sides(&mesh), 0, "open sides at {tol}");
    }
}

/// Twice the signed area of a closed polyline in the XY plane.
fn area2(points: &[mt::Point3]) -> f64 {
    points.iter().zip(points.iter().cycle().skip(1)).map(|(p, q)| p.x * q.y - q.x * p.y).sum()
}

/// A face in the XY plane between y = 0 and an arch from (5, h) to (0, h) whose middle dips
/// 2.8e-7 below y = 0, as the intersection curve does in the union of #1648. Its loop starts
/// along the bottom, so the bottom goes into the triangulation before the arch.
#[test]
fn a_face_touching_its_own_edge_meshes() {
    let (h, dip) = (0.3175, 2.8e-7);
    let v = |x: f64, y: f64| mt::builder::vertex(p3(Vec3::new(x, y, 0.0)));
    let (lb, rb, rt, lt) = (v(0.0, 0.0), v(5.0, 0.0), v(5.0, h), v(0.0, h));
    let control = vec![p3(Vec3::new(5.0, h, 0.0)), p3(Vec3::new(2.5, -h - 2.0 * dip, 0.0)), p3(Vec3::new(0.0, h, 0.0))];
    let arch = mt::BSplineCurve::new(mt::KnotVec::bezier_knot(2), control);
    let arch = mt::Edge::new(&rt, &lt, mt::Curve::BSplineCurve(arch));
    let wire: mt::Wire = vec![mt::builder::line(&lb, &rb), mt::builder::line(&rb, &rt), arch, mt::builder::line(&lt, &lb)].into();
    let plane = mt::Plane::new(p3(Vec3::ZERO), p3(Vec3::X), p3(Vec3::Y));
    let face = mt::Face::new(vec![wire], mt::Surface::Plane(plane));
    let shell: mt::Shell = vec![face].into();
    for tol in [0.3, 0.08, 0.02, 0.005] {
        let meshed = shell.robust_triangulation_within(Deflection { display: true, ..Deflection::chord(tol) });
        let face = meshed.face_iter().next().unwrap();
        let boundary: Vec<mt::Point3> = face.boundaries()[0].edge_iter().flat_map(|e| e.oriented_curve().0.into_iter().skip(1)).collect();
        let mesh = face.surface().unwrap();
        let area: f64 = mesh
            .faces()
            .triangle_iter()
            .map(|t| {
                let [a, b, c] = [0, 1, 2].map(|i| mesh.positions()[t[i].pos]);
                (b - a).cross(c - a).magnitude() / 2.0
            })
            .sum();
        // The triangles cover the face's boundary polygon, the parts on both sides of the pinch.
        let polygon = area2(&boundary).abs() / 2.0;
        assert!((area - polygon).abs() < 1e-5, "at {tol}: triangles cover {area}, the boundary {polygon}");
    }
}

/// The union of #1648: the sphere touches the box's bottom edge at (2.5, 7.5, 17.5).
#[test]
fn sphere_touching_a_box_edge_unites_soundly() {
    let a = sphere(Vec3::new(2.5, 15.0, 27.5), 12.5).unwrap();
    let b = box_solid(Vec3::new(0.0, 0.0, 17.5), Vec3::new(5.0, 7.5, 22.5)).unwrap();
    let union = boolean(&a, &b, BoolOp::Union).unwrap().unwrap();
    assert!(union.validity().is_empty(), "{:?}", union.validity());
    let size = union.size();
    for tol in [size * 1e-2, size * 2.5e-3, size * 6e-4] {
        let faces: std::collections::HashSet<u32> = union.mesh(&crate::MeshTolerance::chord(tol)).unwrap().tri_face.into_iter().collect();
        assert_eq!(faces.len(), union.face_count(), "at {tol}");
    }
}
