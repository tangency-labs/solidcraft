//! WR-507 / WR-508 (#1520, #1521) through the 4x4 `SmBrep::Transform`
//! ([`transform_matrix`]), the overload SolidCraft calls: the fork's own regression
//! tests (`TransformKeepsEdgeTolerance`, `ScaledBodyWithoutVertices` in
//! tangency-labs/smlib#49 and #50, `capi/tests/flat_roundtrip.cpp`) on the port. Each
//! test turns on one flag and fails with it off.

use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::Point3;
use solidcore::primitive::api_primitives as api;
use solidcore::topology::brep::mirror::transform_matrix;
use solidcore::topology::brep::p1;
use solidcore::topology::{edge, vertex};
use solidcore::Kernel;
use std::f64::consts::PI;

/// WR-507 (tangency-labs/smlib#49): a 10 x 20 x 30 box at (1, 2, 3) whose first edge and
/// its two vertices are given 1e-3, far above the box's own tolerance. As given, mirrored
/// in x = 30 and turned a quarter about z (each a unit scale through the 4x4) the edge
/// keeps 1e-3, and scaled by 2 it has 2e-3. Upstream sets every tolerance to the body's.
#[test]
fn wr507_transform_matrix_keeps_or_scales_an_edge_tolerance() {
    let wide = 1e-3;
    let moves: [(&str, [[f64; 4]; 4], f64); 4] = [
        (
            "as given",
            [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
            1.0,
        ),
        (
            "mirrored",
            [
                [-1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [60.0, 0.0, 0.0, 1.0],
            ],
            1.0,
        ),
        (
            "turned",
            [
                [0.0, 1.0, 0.0, 0.0],
                [-1.0, 0.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [5.0, 6.0, 7.0, 1.0],
            ],
            1.0,
        ),
        (
            "scaled",
            [
                [2.0, 0.0, 0.0, 0.0],
                [0.0, 2.0, 0.0, 0.0],
                [0.0, 0.0, 2.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
            2.0,
        ),
    ];
    for (name, m, scale) in moves {
        let mut k = Kernel::default();
        k.fixes.transform_scales_each_tolerance = true;
        let mut b = api::create_box(&mut k, Point3::new(1.0, 2.0, 3.0), 10.0, 20.0, 30.0).unwrap();
        assert!(
            b.tolerance() < wide * 1e-1,
            "{name}: box tolerance {:e}",
            b.tolerance()
        );
        let e = p1::edges(&b, None).unwrap()[0];
        edge::set_tolerance(&mut b, e, wide, false, false).unwrap();
        for v in [edge::start_vertex(&b, e), edge::end_vertex(&b, e)] {
            let v = v.unwrap().unwrap();
            vertex::set_tolerance(&mut b, v, wide, false, false).unwrap();
        }
        transform_matrix(&mut k, &mut b, &m, true).unwrap();
        let widest = p1::edges(&b, None)
            .unwrap()
            .into_iter()
            .map(|e| edge::tolerance(&b, e).unwrap())
            .fold(0.0_f64, f64::max);
        let want = wide * scale;
        assert!(
            (widest - want).abs() <= 1e-15 * scale,
            "{name}: widest edge tolerance {widest:e}, want {want:e}"
        );
    }
}

/// WR-508 (tangency-labs/smlib#50): a lone torus (R 6, r 2 about (10, 5, 3)), which has
/// no vertex spanning a size, scaled by 2 and by 0.5 through the 4x4. Upstream sizes it
/// from its vertex box and gives it a zero tolerance; with the fix its tolerance is at
/// least that of the same torus made at the scaled size, and a ball of radius 1.5 (scaled
/// alike) cuts out of its tube to the analytic volume. Without the fix the cut returns a
/// body whose volume is the whole torus' (3790.07 for a scale of 2, 59.220 for 0.5; the
/// fork's C API failed it with 1001). The volume is integrated to 1e-3, not the fork's
/// 1e-8, to keep the unoptimised test short; it is within 4e-5 of the analytic one, and
/// the uncut torus is 3% off.
#[test]
fn wr508_transform_matrix_sizes_a_lone_torus_from_its_box() {
    let c = Point3::new(10.0, 5.0, 3.0);
    for s in [2.0, 0.5] {
        let mut k = Kernel::default();
        k.fixes.transform_model_size_from_body_box = true;
        let m = [
            [s, 0.0, 0.0, 0.0],
            [0.0, s, 0.0, 0.0],
            [0.0, 0.0, s, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ];
        let ck = Point3::new(c.x * s, c.y * s, c.z * s);
        let mut torus = api::create_torus(&mut k, c, 6.0, 2.0).unwrap();
        transform_matrix(&mut k, &mut torus, &m, true).unwrap();
        let mut made_k = Kernel::default();
        let made = api::create_torus(&mut made_k, ck, 6.0 * s, 2.0 * s).unwrap();
        let tol = torus.tolerance();
        assert!(
            tol > 0.0 && tol >= made.tolerance(),
            "scale {s}: tolerance {tol:e}, made at that size {:e}",
            made.tolerance()
        );

        let ball =
            api::create_sphere(&mut k, Point3::new(ck.x + 6.0 * s, ck.y, ck.z), 1.5 * s).unwrap();
        let mut cut = solidcore::boolean::api_brep::boolean_difference(&mut k, torus, ball)
            .unwrap_or_else(|e| panic!("scale {s}: ball cut out of the scaled torus: {e}"));
        let store = AttributeStore::new();
        let key_of = |_| ObjectKey(0);
        let attrs = PropertyAttributes {
            store: &store,
            key_of: &key_of,
        };
        let v = brep_volume(&k, &mut cut, 1e-3, &attrs).unwrap();
        let want = s * s * s * (2.0 * PI * PI * 6.0 * 4.0 - 4.0 / 3.0 * PI * 1.5 * 1.5 * 1.5);
        assert!(
            (v - want).abs() <= 1e-4 * want,
            "scale {s}: volume {v}, want {want}"
        );
    }
}
