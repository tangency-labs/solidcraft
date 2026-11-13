//! A sheet turned over (`Body::turn_over`, its surface's u and v swapped) meshes facing the other
//! way: at each point both meshes have, the normal is negated, and each triangle is wound
//! counter-clockwise about its normal before and after.

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
    let (body, order) = Body::from_record(&before, true).unwrap();
    let after = body.turn_over(&order).unwrap().record;

    // the turned surface (u and v swapped) is laid out in its own order: compare at the points
    // both meshes have (its corners and the samples its edges share), and overall
    let (m0, m1) = (mesh(&before), mesh(&after));
    assert!(!m0.triangles.is_empty() && !m1.triangles.is_empty());
    let mut shared = 0;
    for (p1, n1) in m1.positions.iter().zip(&m1.normals) {
        let Some(i) = m0
            .positions
            .iter()
            .position(|p0| (p0.x - p1.x).abs() + (p0.y - p1.y).abs() + (p0.z - p1.z).abs() < 1e-12)
        else {
            continue;
        };
        let n0 = m0.normals[i];
        assert!(
            (n0.x + n1.x).abs() + (n0.y + n1.y).abs() + (n0.z + n1.z).abs() < 1e-9,
            "at {p1:?}: {n0:?} then {n1:?}"
        );
        shared += 1;
    }
    assert!(shared >= 4, "the corners at least: {shared}");
    let sum = |m: &Mesh| {
        m.normals.iter().fold(Vec3::new(0.0, 0.0, 0.0), |a, n| {
            Vec3::new(a.x + n.x, a.y + n.y, a.z + n.z)
        })
    };
    assert!(
        dot(sum(&m0), sum(&m1)) < 0.0,
        "facing the other way overall"
    );
    // each triangle wound counter-clockwise about its own corner's normal, before and after
    for m in [&m0, &m1] {
        for &t in &m.triangles {
            assert!(dot(wound(m, t), m.normals[t[0] as usize]) > 0.0);
        }
    }
}
