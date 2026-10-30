//! WR-80 (the fork's tangency-labs/smlib@0dbe681): `GlobalSurfaceIntersect` fails with
//! `SM_ERR_NULL_POINTER` when `TraceCurve` returns an empty curve (its start point
//! already on a found curve) and the curve is traced back the other way: the empty list
//! is reversed. With `UpstreamFixes::ssi_no_reverse_of_empty_trace` the intersection
//! completes.
//!
//! The case is the SSI corpus' `c2n_quartic_contact_parabola` (jyviko/ssi-corpus
//! `b04c8a6`, Apache-2.0): two NURBS sheets, degree (2, 4), in 4th-order contact along a
//! parabola (`z = 0.5w^3 - 0.5w + 0.2u^2 + w^4` and the same without `w^4`), at
//! approximation tolerance `1e-6`.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers
use solidcore::geom::tol::approx_tol3d;
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_surface::BSplineSurface;
use solidcore::nurbs::math::CPoint;
use solidcore::nurbs::srf_basic::Surface as NurbSurface;
use solidcore::surface::surface::p2::SurfaceIntersection;
use solidcore::surface::surface::Surface;

/// A non-rational NURBS sheet of degree (2, 4) from the corpus' 3 x 5 control points.
fn sheet(points: &[[f64; 3]]) -> Surface {
    let net: Vec<Vec<CPoint>> = points
        .chunks(5)
        .map(|row| {
            row.iter()
                .map(|p| CPoint::new(p[0], p[1], p[2], 1.0))
                .collect()
        })
        .collect();
    let sur = NurbSurface::new(
        net,
        2,
        4,
        vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        vec![0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0],
    )
    .unwrap();
    Surface::from(BSplineSurface::from_nurb(sur, false))
}

fn intersect(fix: bool) -> solidcore::Result<SurfaceIntersection> {
    let mut k = Kernel::default();
    k.fixes.ssi_no_reverse_of_empty_trace = fix;
    let with_quartic = sheet(&[
        [-1.0, -1.0, 1.2],
        [-1.0, -0.5, -0.30000000000000004],
        [-1.0, 0.0, 1.2],
        [-1.0, 0.5, -1.2999999999999998],
        [-1.0, 1.0, 1.1999999999999993],
        [0.0, -1.0, 0.7999999999999999],
        [0.0, -0.5, -0.7000000000000001],
        [0.0, 0.0, 0.7999999999999999],
        [0.0, 0.5, -1.6999999999999997],
        [0.0, 1.0, 0.7999999999999993],
        [1.0, -1.0, 1.2],
        [1.0, -0.5, -0.30000000000000004],
        [1.0, 0.0, 1.2],
        [1.0, 0.5, -1.2999999999999996],
        [1.0, 1.0, 1.1999999999999993],
    ]);
    let without = sheet(&[
        [-1.0, -1.0, 0.2],
        [-1.0, -0.5, 0.7],
        [-1.0, 0.0, 0.19999999999999996],
        [-1.0, 0.5, -0.30000000000000004],
        [-1.0, 1.0, 0.20000000000000018],
        [0.0, -1.0, -0.2],
        [0.0, -0.5, 0.29999999999999993],
        [0.0, 0.0, -0.20000000000000007],
        [0.0, 0.5, -0.7000000000000001],
        [0.0, 1.0, -0.19999999999999984],
        [1.0, -1.0, 0.19999999999999996],
        [1.0, -0.5, 0.7],
        [1.0, 0.0, 0.19999999999999996],
        [1.0, 0.5, -0.30000000000000004],
        [1.0, 1.0, 0.20000000000000018],
    ]);
    let d1 = with_quartic.natural_uv_domain_with_nurb(&k).unwrap();
    let d2 = without.natural_uv_domain_with_nurb(&k).unwrap();
    let tol = approx_tol3d(&k, 1e-6);
    with_quartic.global_surface_intersect(&k, &d1, &without, &d2, [true, true], Some(tol), None)
}

#[test]
fn wr_80_quartic_contact_completes_with_the_fix() {
    let r = intersect(true).unwrap();
    assert!(!r.curves_3d.is_empty(), "no curve found");
}

/// Without the opt-in the intersection fails as the pinned C++ does (WR-80).
#[test]
fn wr_80_quartic_contact_fails_upstream() {
    match intersect(false) {
        Err(solidcore::SolidError::Upstream { code, .. }) => {
            assert_eq!(code, solidcore::error::SM_ERR_NULL_POINTER);
        }
        other => panic!("{:?}", other.map(|r| r.curves_3d.len())),
    }
}
