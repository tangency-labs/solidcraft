//! Issue #1570: `SmBrep::Transform(const double[4][4], bScaleTol)` (and the
//! `SmMatrix::SVDecompose` it relies on) applies an arbitrary affine map `p' = p·A + t`
//! (row-vector convention) to a B-rep: shear, non-uniform scale and reflection included.
//!
//! The issue's cases on the unit cube `[0,1]^3`: every vertex maps to `v·A + t`, the
//! volume becomes `|det A|` and the shell stays outward (positive volume) under a
//! reflection.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::matrix::Matrix;
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, Vec3};
use solidcore::primitive::api_primitives::create_box;
use solidcore::topology::brep::mirror::transform_matrix;
use solidcore::topology::brep::p1;
use solidcore::{Brep, Kernel};

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-6, &attrs).unwrap()
}

fn apply(m: &[[f64; 4]; 4], p: Point3) -> Point3 {
    Vec3::new(
        p.x * m[0][0] + p.y * m[1][0] + p.z * m[2][0] + m[3][0],
        p.x * m[0][1] + p.y * m[1][1] + p.z * m[2][1] + m[3][1],
        p.x * m[0][2] + p.y * m[1][2] + p.z * m[2][2] + m[3][2],
    )
}

/// Transforms the unit cube by `m` and checks every vertex and the volume.
fn check(m: [[f64; 4]; 4], expected_volume: f64) {
    let mut k = Kernel::default();
    let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 1.0, 1.0, 1.0).unwrap();
    let vertices = p1::vertices(&b, None).unwrap();
    let before: Vec<Point3> = vertices
        .iter()
        .map(|&v| b.vertex(v).unwrap().point)
        .collect();
    transform_matrix(&mut k, &mut b, &m, true).unwrap();
    for (&v, &p) in vertices.iter().zip(&before) {
        let got = b.vertex(v).unwrap().point;
        let want = apply(&m, p);
        assert!(
            (got - want).length() < 1e-9,
            "vertex {p:?}: got {got:?}, want {want:?}"
        );
    }
    let vol = volume(&k, &mut b);
    assert!(
        (vol - expected_volume).abs() < 1e-6,
        "volume {vol}, want {expected_volume}"
    );
}

#[test]
fn issue_1570_shear_keeps_the_volume() {
    let m = [
        [1.0, 0.5, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [2.0, 3.0, 4.0, 1.0],
    ];
    check(m, 1.0);
    // the issue's spot check: (1,1,1) -> (3, 4.5, 5)
    assert_eq!(
        apply(&m, Point3::new(1.0, 1.0, 1.0)),
        Point3::new(3.0, 4.5, 5.0)
    );
}

#[test]
fn issue_1570_non_uniform_scale() {
    check(
        [
            [2.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [1.0, -2.0, 0.5, 1.0],
        ],
        2.0,
    );
}

#[test]
fn issue_1570_reflection_keeps_the_shell_outward() {
    check(
        [
            [-1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
        1.0,
    );
}

#[test]
fn issue_1570_sv_decompose_reconstructs_the_matrix() {
    let a = [1.0, 0.5, 0.0, 0.0, 1.0, 0.0, 0.3, -0.2, 2.0];
    let mut u = Matrix::from_row_major(3, 3, a.to_vec()).unwrap();
    let (w, v) = u.sv_decompose().unwrap();
    for r in 0..3 {
        for c in 0..3 {
            let mut x = 0.0;
            for (i, wi) in w.iter().enumerate() {
                x += u.at(r, i).unwrap() * wi * v.at(c, i).unwrap();
            }
            assert!((x - a[r * 3 + c]).abs() < 1e-12, "A[{r}][{c}] = {x}");
        }
    }
    assert!(w.iter().all(|&s| s >= 0.0));
}
