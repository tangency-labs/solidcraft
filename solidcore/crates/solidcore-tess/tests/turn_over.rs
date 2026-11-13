//! A sheet turned over (`Body::turn_over`) meshes as the same points with each normal negated and
//! each triangle wound the other way: its record reads each face backward in its shell, which the
//! mesher follows.

#![allow(
    clippy::panic,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::float_arithmetic
)] // test helpers

use solidcore_ops::Body;
use solidcore_ops::surface::ruled;
use solidcore_record::Vec3;
use solidcore_record::brep::{Brep, Curve};
use solidcore_tess::{Mesh, MeshTolerance, plan};

/// A bumped sheet: the ruled sheet between two cubics.
fn sheet() -> Brep {
    let row = |y: f64, z: f64| Curve::Spline {
        knots: vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
        points: vec![[0.0, y, 0.0], [3.0, y, z], [6.0, y, -z], [9.0, y, 0.0]],
    };
    ruled(&row(0.0, 2.0), &row(8.0, 1.0), false).unwrap()
}

/// `r` meshed at display tolerance for a 12 mm body.
fn mesh(r: &Brep) -> Mesh {
    let (p, _) = plan(r, &MeshTolerance::display_of(12.0), 12.0).unwrap();
    let (m, refused) = p.evaluate();
    assert!(refused.is_empty());
    m
}

/// The triangle's normal by its winding: `(b - a) x (c - a)`.
fn wound(m: &Mesh, t: [u32; 3]) -> Vec3 {
    let p = |i: u32| m.positions[i as usize];
    let (a, b, c) = (p(t[0]), p(t[1]), p(t[2]));
    let (u, v) = (
        Vec3::new(b.x - a.x, b.y - a.y, b.z - a.z),
        Vec3::new(c.x - a.x, c.y - a.y, c.z - a.z),
    );
    Vec3::new(
        u.y * v.z - u.z * v.y,
        u.z * v.x - u.x * v.z,
        u.x * v.y - u.y * v.x,
    )
}

fn dot(a: Vec3, b: Vec3) -> f64 {
    a.x * b.x + a.y * b.y + a.z * b.z
}

#[test]
fn a_turned_sheet_meshes_with_its_normals_negated() {
    let before = sheet();
    let (body, _) = Body::from_record(&before, true).unwrap();
    body.turn_over().unwrap();
    let (after, _) = body.to_record().unwrap();

    let (m0, m1) = (mesh(&before), mesh(&after));
    assert_eq!(m1.positions, m0.positions, "the same points");
    assert_eq!(m1.normals.len(), m0.normals.len());
    for (n1, n0) in m1.normals.iter().zip(&m0.normals) {
        assert_eq!(
            (n1.x, n1.y, n1.z),
            (-n0.x, -n0.y, -n0.z),
            "each normal negated"
        );
    }
    assert_eq!(m1.triangles.len(), m0.triangles.len());
    assert!(!m0.triangles.is_empty());
    for (&t1, &t0) in m1.triangles.iter().zip(&m0.triangles) {
        assert_eq!(
            t1,
            [t0[0], t0[2], t0[1]],
            "each triangle wound the other way"
        );
        // wound counter-clockwise about its own corner's normal, before and after
        assert!(dot(wound(&m0, t0), m0.normals[t0[0] as usize]) > 0.0);
        assert!(dot(wound(&m1, t1), m1.normals[t1[0] as usize]) > 0.0);
    }
}
