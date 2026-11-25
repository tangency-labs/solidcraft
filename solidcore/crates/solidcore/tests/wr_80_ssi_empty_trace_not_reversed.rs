//! WR-80 (the fork's tangency-labs/smlib@0dbe681): `GlobalSurfaceIntersect` failed with
//! `SM_ERR_NULL_POINTER` when `TraceCurve` returned an empty curve (its start point
//! already on a found curve) and the curve was traced back the other way: the empty list
//! was reversed. solidcore traces back only a trace that produced points, so the
//! intersection completes; this test fails on the old path.
//!
//! The case is the SSI corpus' `c2n_quartic_contact_parabola` (jyviko/ssi-corpus
//! `b04c8a6`, Apache-2.0): two NURBS sheets, degree (2, 4), in 4th-order contact along a
//! parabola (`z = 0.5w^3 - 0.5w + 0.2u^2 + w^4` and the same without `w^4`), at
//! approximation tolerance `1e-6`. Both sheets are graphs over `(x, y) = (u, w)`, so they
//! meet where `y^4 = 0`: on the parabola `y = 0`, `z = 0.2 x^2`.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers
use solidcore::curve::curve::Curve;
use solidcore::geom::tol::approx_tol3d;
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_surface::BSplineSurface;
use solidcore::nurbs::math::CPoint;
use solidcore::nurbs::srf_basic::Surface as NurbSurface;
use solidcore::surface::surface::p2::SurfaceIntersection;
use solidcore::surface::surface::Surface;
use solidcore::Point3;

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

fn intersect(k: &Kernel) -> solidcore::Result<SurfaceIntersection> {
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
    let d1 = with_quartic.natural_uv_domain_with_nurb(k).unwrap();
    let d2 = without.natural_uv_domain_with_nurb(k).unwrap();
    let tol = approx_tol3d(k, TOL);
    with_quartic.global_surface_intersect(k, &d1, &without, &d2, [true, true], Some(tol), None)
}

/// The approximation tolerance the intersection is asked for.
const TOL: f64 = 1e-6;

/// Samples per curve, ends included.
const SAMPLES: usize = 65;

/// The curve at `SAMPLES` evenly spaced parameters, ends included.
fn samples(k: &Kernel, curve: &Curve) -> Vec<Point3> {
    let interval = curve.natural_interval();
    (0..SAMPLES)
        .map(|i| {
            let t = interval.min()
                + (interval.max() - interval.min()) * i as f64 / (SAMPLES - 1) as f64;
            curve.evaluate_point(k, t).unwrap()
        })
        .collect()
}

/// The intersection completes on the default kernel, and what it finds lies on both
/// sheets and covers the contact parabola `y = 0`, `z = 0.2 x^2`, from `x = -1` to `1`.
#[test]
fn wr_80_quartic_contact_completes() {
    let k = Kernel::default();
    let r = match intersect(&k) {
        Ok(r) => r,
        Err(e) => panic!("the intersection fails: {e:?}"),
    };
    assert!(!r.curves_3d.is_empty(), "no curve found");
    // Every sampled point lies within the tolerance `TOL` of both sheets. Both are
    // graphs `z = s(x, y)`, so a point's distance to one is, to first order, its height
    // above it over `sqrt(1 + |grad s|^2)`; the neglected curvature term is of the order
    // of the distance squared. The sheets are `y^4` apart vertically, so this also keeps
    // the points within about `(2 TOL)^(1/4)` of the contact line `y = 0`.
    let without_quartic = |x: f64, y: f64| {
        (
            0.5 * y * y * y - 0.5 * y + 0.2 * x * x,
            [0.4 * x, 1.5 * y * y - 0.5],
        )
    };
    let with_quartic = |x: f64, y: f64| {
        let (z, [zx, zy]) = without_quartic(x, y);
        (z + y.powi(4), [zx, zy + 4.0 * y * y * y])
    };
    let distance = |p: &Point3, (z, [zx, zy]): (f64, [f64; 2])| {
        (p.z - z).abs() / (1.0 + zx * zx + zy * zy).sqrt()
    };
    let curves: Vec<Vec<Point3>> = r.curves_3d.iter().map(|c| samples(&k, c)).collect();
    for p in curves.iter().flatten() {
        let d_without = distance(p, without_quartic(p.x, p.y));
        let d_with = distance(p, with_quartic(p.x, p.y));
        assert!(
            d_without <= TOL && d_with <= TOL,
            "{p:?} is {d_without:e} from the sheet without the quartic term and \
             {d_with:e} from the one with it"
        );
    }
    // The curves cover the whole parabola, x from -1 to 1, without a gap. A curve is
    // connected, so its projection on x is an interval: the union of the curves'
    // sampled x-extents must be one interval reaching both ends. Curves that meet share
    // an end point, so two neighbours' extents overlap or touch.
    let mut extents: Vec<(f64, f64)> = curves
        .iter()
        .map(|c| {
            let lo = c.iter().map(|p| p.x).fold(f64::INFINITY, f64::min);
            let hi = c.iter().map(|p| p.x).fold(f64::NEG_INFINITY, f64::max);
            (lo, hi)
        })
        .collect();
    extents.sort_by(|a, b| a.0.total_cmp(&b.0));
    let (first_lo, mut reach) = extents[0];
    assert!(
        first_lo < -1.0 + 1e-6,
        "no curve reaches x = -1 (min {first_lo})"
    );
    for &(lo, hi) in &extents[1..] {
        assert!(
            lo <= reach + 1e-6,
            "the curves leave x in ({reach}, {lo}) uncovered: {extents:?}"
        );
        reach = reach.max(hi);
    }
    assert!(reach > 1.0 - 1e-6, "no curve reaches x = 1 (max {reach})");
}
