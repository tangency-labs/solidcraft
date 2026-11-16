//! WR-75 (fork tangency-labs/smlib ef4ab25, e9fde8f, d8c61db, 1693c9d): with
//! `Kernel::fixes.ssi_tangent_curve_tracer` the surface/surface intersection traces the
//! curves along which the surfaces touch, and reports the singular points where crossing
//! branches meet. Two ssi-corpus cases (jyviko/ssi-corpus), the surfaces rebuilt here
//! from their definitions:
//!
//! - `c2n_quartic_circle`: the quartic `z = (x^2 + y^2 - 1)^2` over `[-2, 2]^2` (a
//!   degree 4 Bezier patch, its control net computed below) against the plane `z = 0`.
//!   They touch along the unit circle; upstream returns nothing.
//! - `c4_steinmetz`: two perpendicular unit cylinders (axes z and x, height 4) given, as
//!   the corpus gives them, as rational B-spline surfaces (an analytic `Cone` pair takes
//!   the cylinder/cylinder path, which never reaches the tracer); their two ellipses
//!   cross at `(0, +-1, 0)`, which upstream does not report.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::cast_precision_loss
)] // test helpers

use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::tol::approx_tol3d;
use solidcore::geom::vector2d::Vec2;
use solidcore::geom::vector3d::Vec3;
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_surface::BSplineSurface;
use solidcore::nurbs::math::CPoint;
use solidcore::nurbs::srf_basic::Surface as NurbSurface;
use solidcore::surface::plane::Plane;
use solidcore::surface::surface::p2::{SurfaceIntersection, TsectCurveType};
use solidcore::surface::surface::Surface;

/// `C(n, k)`.
fn choose(n: usize, k: usize) -> f64 {
    (0..k).fold(1.0, |acc, i| acc * (n - i) as f64 / (i + 1) as f64)
}

/// The degree 4 Bernstein coefficients of `x(t)^a`, `x = -2 + 4 t`, for `a` in 0..=4.
fn bernstein_powers() -> Vec<[f64; 5]> {
    let mut power = vec![1.0];
    let mut out = Vec::new();
    for _ in 0..=4 {
        let mut p = [0.0; 5];
        p[..power.len()].copy_from_slice(&power);
        let mut b = [0.0; 5];
        for (i, bi) in b.iter_mut().enumerate() {
            *bi = (0..=i).map(|k| choose(i, k) / choose(4, k) * p[k]).sum();
        }
        out.push(b);
        // multiply by x(t) = -2 + 4 t
        let mut next = vec![0.0; power.len() + 1];
        for (i, c) in power.iter().enumerate() {
            next[i] += -2.0 * c;
            next[i + 1] += 4.0 * c;
        }
        power = next;
    }
    out
}

/// `z = (x^2 + y^2 - 1)^2` over `[-2, 2]^2` as a degree (4, 4) Bezier patch.
fn quartic() -> Surface {
    let b = bernstein_powers();
    // (x^2 + y^2 - 1)^2 = x^4 + y^4 + 2 x^2 y^2 - 2 x^2 - 2 y^2 + 1
    let terms = [
        (4, 0, 1.0),
        (0, 4, 1.0),
        (2, 2, 2.0),
        (2, 0, -2.0),
        (0, 2, -2.0),
        (0, 0, 1.0),
    ];
    let net: Vec<Vec<CPoint>> = (0..5)
        .map(|i| {
            (0..5)
                .map(|j| {
                    let z: f64 = terms.iter().map(|&(a, c, m)| m * b[a][i] * b[c][j]).sum();
                    CPoint::new(-2.0 + i as f64, -2.0 + j as f64, z, 1.0)
                })
                .collect()
        })
        .collect();
    let knots = vec![0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0];
    let nurb = NurbSurface::new(net, 4, 4, knots.clone(), knots).unwrap();
    Surface::from(BSplineSurface::from_nurb(nurb, false))
}

fn plane() -> Surface {
    let dom = Extent2d::from_bounds(-3.0, -3.0, 3.0, 3.0);
    let p = Plane::new(Vec3::ZERO, Vec3::X, Vec3::Y, Vec2::new(1.0, 1.0), &dom);
    let _ = p.natural_uv_domain();
    Surface::from(p)
}

/// The unit cylinder from `origin` along `x` cross `y`, length 4, as a degree (2, 1)
/// rational B-spline (the nine-point circle in the `x`, `y` plane, swept along the axis).
fn cylinder(origin: Vec3, x: Vec3, y: Vec3) -> Surface {
    let axis = x.cross(y);
    let w = std::f64::consts::FRAC_1_SQRT_2;
    let circle = [
        (1.0, 0.0, 1.0),
        (1.0, 1.0, w),
        (0.0, 1.0, 1.0),
        (-1.0, 1.0, w),
        (-1.0, 0.0, 1.0),
        (-1.0, -1.0, w),
        (0.0, -1.0, 1.0),
        (1.0, -1.0, w),
        (1.0, 0.0, 1.0),
    ];
    let net: Vec<Vec<CPoint>> = circle
        .iter()
        .map(|&(a, b, wt)| {
            [0.0, 4.0]
                .iter()
                .map(|&h| {
                    let p = origin + x * a + y * b + axis * h;
                    CPoint::new(p.x * wt, p.y * wt, p.z * wt, wt)
                })
                .collect()
        })
        .collect();
    let ku = vec![
        0.0, 0.0, 0.0, 0.25, 0.25, 0.5, 0.5, 0.75, 0.75, 1.0, 1.0, 1.0,
    ];
    let kv = vec![0.0, 0.0, 1.0, 1.0];
    let nurb = NurbSurface::new(net, 2, 1, ku, kv).unwrap();
    Surface::from(BSplineSurface::from_nurb(nurb, false))
}

/// `GlobalSurfaceIntersect` at the corpus tolerance (1e-6).
fn intersect(fix: bool, s1: &Surface, s2: &Surface) -> SurfaceIntersection {
    let mut k = Kernel::default();
    k.fixes.ssi_tangent_curve_tracer = fix;
    let d1 = s1.natural_uv_domain_with_nurb(&k).unwrap();
    let d2 = s2.natural_uv_domain_with_nurb(&k).unwrap();
    let tol = approx_tol3d(&k, 1e-6);
    s1.global_surface_intersect(&k, &d1, s2, &d2, [true, true], Some(tol), None)
        .unwrap()
}

/// How many of 36 ten-degree sectors of the unit circle in `z = 0` a curve of the result
/// covers (sampled at 400 points, within `1e-4`).
fn circle_sectors(x: &SurfaceIntersection) -> usize {
    let k = Kernel::default();
    let mut best = 0;
    for c in &x.curves_3d {
        let ivl = c.natural_interval();
        let mut hit = [false; 36];
        for j in 0..=400 {
            let p = c
                .evaluate_point(&k, ivl.evaluate(f64::from(j) / 400.0))
                .unwrap();
            let r = (p.x * p.x + p.y * p.y).sqrt();
            if (r - 1.0).abs() < 1e-4 && p.z.abs() < 1e-4 {
                let a = p.y.atan2(p.x).to_degrees().rem_euclid(360.0);
                hit[((a / 10.0) as usize).min(35)] = true;
            }
        }
        best = best.max(hit.iter().filter(|&&h| h).count());
    }
    best
}

#[test]
fn wr75_quartic_touching_a_plane_along_a_circle() {
    let (q, p) = (quartic(), plane());
    let upstream = intersect(false, &q, &p);
    assert!(circle_sectors(&upstream) < 18, "{:?}", upstream.curve_types);

    let fixed = intersect(true, &q, &p);
    assert_eq!(circle_sectors(&fixed), 36, "{:?}", fixed.curve_types);
    assert!(
        fixed.curve_types.contains(&TsectCurveType::Tangent),
        "{:?}",
        fixed.curve_types
    );
}

/// Whether the result has a touching point within `1e-3` of `at`.
fn touches_at(x: &SurfaceIntersection, at: Vec3) -> bool {
    let k = Kernel::default();
    x.curves_3d.iter().zip(&x.curve_types).any(|(c, t)| {
        *t == TsectCurveType::Touching
            && c.evaluate_point(&k, c.natural_interval().min())
                .unwrap()
                .distance(at)
                < 1e-3
    })
}

#[test]
fn wr75_steinmetz_crossing_points_are_reported() {
    let a = cylinder(Vec3::new(0.0, 0.0, -2.0), Vec3::X, Vec3::Y);
    let b = cylinder(Vec3::new(-2.0, 0.0, 0.0), Vec3::Y, Vec3::Z);
    let nodes = [Vec3::new(0.0, 1.0, 0.0), Vec3::new(0.0, -1.0, 0.0)];

    let upstream = intersect(false, &a, &b);
    assert!(
        !nodes.iter().all(|&n| touches_at(&upstream, n)),
        "{:?}",
        upstream.curve_types
    );

    let fixed = intersect(true, &a, &b);
    for n in nodes {
        assert!(touches_at(&fixed, n), "{n:?}: {:?}", fixed.curve_types);
    }
}
