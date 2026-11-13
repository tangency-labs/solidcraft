//! tangency-labs/solidcore#392 (WR-76): `GlobalSurfaceIntersect` returns an open tangent
//! curve twice. Where two surfaces touch along a line that crosses their boundaries,
//! `FindBoundaryStartPoints` finds a start point at each end, `TraceCurve` traces the
//! line from the first, and from the second its first step snaps onto the first start
//! point (`TestPoint`), an exit that skips the "already traced" test: the line comes
//! back a second time.
//!
//! The case is the SSI corpus' `c2n_cylN_on_planeN` (jyviko/ssi-corpus `b04c8a6`): a
//! rational quadratic NURBS cylinder of radius 1 about the x axis at `z = 1`, lying on the
//! NURBS plane `z = 0` along `y = 0, -2 <= x <= 2`, at approximation tolerance `1e-6`.
//! With `UpstreamFixes::ssi_trace_tangent_curves_once` off (upstream) the result is two
//! tangent curves on top of each other; with it on, one.
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
use solidcore::surface::surface::p2::{SurfaceIntersection, TsectCurveType};
use solidcore::surface::surface::Surface;

/// A NURBS surface from Cartesian control points and weights, `u` rows of `nv` points,
/// as the corpus gives them (NLib control points are homogeneous).
fn nurbs(
    degree: [usize; 2],
    knots_u: &[f64],
    knots_v: &[f64],
    nv: usize,
    points: &[[f64; 3]],
    weights: &[f64],
) -> Surface {
    let net: Vec<Vec<CPoint>> = points
        .chunks(nv)
        .zip(weights.chunks(nv))
        .map(|(row, w)| {
            row.iter()
                .zip(w)
                .map(|(p, &w)| CPoint::new(p[0] * w, p[1] * w, p[2] * w, w))
                .collect()
        })
        .collect();
    let sur = NurbSurface::new(
        net,
        degree[0],
        degree[1],
        knots_u.to_vec(),
        knots_v.to_vec(),
    )
    .unwrap();
    Surface::from(BSplineSurface::from_nurb(sur, false))
}

/// The corpus' plane and cylinder, intersected as its `gsi` runner does.
fn intersect(fix: bool) -> SurfaceIntersection {
    let mut k = Kernel::default();
    k.fixes.ssi_trace_tangent_curves_once = fix;
    let plane = nurbs(
        [1, 1],
        &[0.0, 0.0, 1.0, 1.0],
        &[0.0, 0.0, 1.0, 1.0],
        2,
        &[
            [-3.0, -3.0, 0.0],
            [-3.0, 3.0, 0.0],
            [3.0, -3.0, 0.0],
            [3.0, 3.0, 0.0],
        ],
        &[1.0; 4],
    );
    let w = std::f64::consts::FRAC_1_SQRT_2;
    let cylinder = nurbs(
        [2, 1],
        &[
            0.0, 0.0, 0.0, 0.25, 0.25, 0.5, 0.5, 0.75, 0.75, 1.0, 1.0, 1.0,
        ],
        &[0.0, 0.0, 1.0, 1.0],
        2,
        &[
            [2.0, 0.0, 2.0],
            [-2.0, 0.0, 2.0],
            [2.0, 0.999_999_999_999_999_9, 2.0],
            [-2.0, 0.999_999_999_999_999_9, 2.0],
            [2.0, 1.0, 1.0],
            [-2.0, 1.0, 1.0],
            [2.0, 1.0, 1.110_223_024_625_156_5e-16],
            [-2.0, 1.0, 1.110_223_024_625_156_5e-16],
            [2.0, 1.224_646_799_147_353_2e-16, 0.0],
            [-2.0, 1.224_646_799_147_353_2e-16, 0.0],
            [2.0, -0.999_999_999_999_999_9, -2.220_446_049_250_313e-16],
            [-2.0, -0.999_999_999_999_999_9, -2.220_446_049_250_313e-16],
            [2.0, -1.0, 0.999_999_999_999_999_8],
            [-2.0, -1.0, 0.999_999_999_999_999_8],
            [2.0, -1.000_000_000_000_000_2, 1.999_999_999_999_999_8],
            [-2.0, -1.000_000_000_000_000_2, 1.999_999_999_999_999_8],
            [2.0, -2.449_293_598_294_706_4e-16, 2.0],
            [-2.0, -2.449_293_598_294_706_4e-16, 2.0],
        ],
        &[
            1.0, 1.0, w, w, 1.0, 1.0, w, w, 1.0, 1.0, w, w, 1.0, 1.0, w, w, 1.0, 1.0,
        ],
    );
    let d1 = plane.natural_uv_domain_with_nurb(&k).unwrap();
    let d2 = cylinder.natural_uv_domain_with_nurb(&k).unwrap();
    let tol = approx_tol3d(&k, 1e-6);
    plane
        .global_surface_intersect(&k, &d1, &cylinder, &d2, [true, true], Some(tol), None)
        .unwrap()
}

/// The end points of each non-degenerate curve, on the line `y = 0, z = 0`.
fn ends(r: &SurfaceIntersection) -> Vec<[[f64; 3]; 2]> {
    let k = Kernel::default();
    r.curves_3d
        .iter()
        .map(|c| {
            let ivl = c.natural_interval();
            let a = c.evaluate_point(&k, ivl.min()).unwrap();
            let b = c.evaluate_point(&k, ivl.max()).unwrap();
            [[a.x, a.y, a.z], [b.x, b.y, b.z]]
        })
        .collect()
}

#[test]
fn upstream_traces_the_tangent_line_twice() {
    let r = intersect(false);
    assert_eq!(
        r.curve_types,
        [TsectCurveType::Tangent, TsectCurveType::Tangent],
        "{:?}",
        ends(&r)
    );
    // both run the whole line, from x = -2 to x = 2 (one each way)
    for [a, b] in ends(&r) {
        assert!((a[0].abs() - 2.0).abs() < 1e-6 && (b[0].abs() - 2.0).abs() < 1e-6);
        assert!((a[0] + b[0]).abs() < 1e-6);
    }
}

#[test]
fn fixed_traces_the_tangent_line_once() {
    let r = intersect(true);
    assert_eq!(r.curve_types, [TsectCurveType::Tangent], "{:?}", ends(&r));
    let [[a, b]] = <[[[f64; 3]; 2]; 1]>::try_from(ends(&r)).unwrap();
    assert!((a[0].abs() - 2.0).abs() < 1e-6 && (b[0].abs() - 2.0).abs() < 1e-6);
    assert!((a[0] + b[0]).abs() < 1e-6);
    for p in [a, b] {
        assert!(p[1].abs() < 1e-5 && p[2].abs() < 1e-5, "{p:?}");
    }
}
