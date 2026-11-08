//! tangency-labs/solidcore#394 (WR-74): `GlobalSurfaceIntersect` misses isolated
//! tangential contact points inside both surfaces. `FindInteriorCurves` drops every
//! leaf pair whose normals are (nearly) parallel or whose local intersection finds
//! nothing, which is exactly where two surfaces only touch.
//!
//! The cases are the ssi-corpus' `c3n_parab_apex`, `c3n_four_points` and
//! `y23_t6_ex4_tangential_point` (jyviko/ssi-corpus `b04c8a6`, surfaces copied here),
//! intersected as `difftest --ssi-corpus` does (natural domains, edges on, approximation
//! tolerance `1e-6`). By default the port returns nothing, as the pinned C++ does; with
//! `Kernel::fixes.ssi_tangent_contact_points` each true contact comes back as one
//! degenerate `TOUCHING` curve.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::tol::approx_tol3d;
use solidcore::geom::vector2d::Vec2;
use solidcore::geom::vector3d::Vec3;
use solidcore::nurbs::b_spline_surface::BSplineSurface;
use solidcore::nurbs::math::CPoint;
use solidcore::nurbs::srf_basic::Surface as NurbSurface;
use solidcore::surface::plane::Plane;
use solidcore::surface::surface::p2::{SurfaceIntersection, TsectCurveType};
use solidcore::surface::surface::Surface;
use solidcore::Kernel;

/// A non-rational Bezier patch of degree `deg` (both directions) from its `(deg+1)^2`
/// control points, row by row in u.
fn bezier(deg: usize, points: &[[f64; 3]]) -> Surface {
    let n = deg + 1;
    assert_eq!(points.len(), n * n);
    let knots: Vec<f64> = (0..2 * n).map(|i| if i < n { 0.0 } else { 1.0 }).collect();
    let net: Vec<Vec<CPoint>> = points
        .chunks(n)
        .map(|row| {
            row.iter()
                .map(|p| CPoint::new(p[0], p[1], p[2], 1.0))
                .collect()
        })
        .collect();
    let sur = NurbSurface::new(net, deg, deg, knots.clone(), knots).unwrap();
    Surface::from(BSplineSurface::from_nurb(sur, false))
}

/// The plane z = 0 over [-3, 3]^2.
fn plane_z0() -> Surface {
    let dom = Extent2d::from_bounds(-3.0, -3.0, 3.0, 3.0);
    let plane = Plane::new(
        Vec3::new(0.0, 0.0, 0.0),
        Vec3::new(1.0, 0.0, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
        Vec2::new(1.0, 1.0),
        &dom,
    );
    // as the corpus runner: the natural domain query makes the plane's NURB
    let _ = plane.natural_uv_domain();
    Surface::from(plane)
}

fn intersect(fix: bool, a: &Surface, b: &Surface) -> SurfaceIntersection {
    let mut k = Kernel::default();
    k.fixes.ssi_tangent_contact_points = fix;
    let da = a.natural_uv_domain_with_nurb(&k).unwrap();
    let db = b.natural_uv_domain_with_nurb(&k).unwrap();
    let tol = approx_tol3d(&k, 1e-6);
    a.global_surface_intersect(&k, &da, b, &db, [true, true], Some(tol), None)
        .unwrap()
}

/// The touching points of `x` (its degenerate `TOUCHING` curves), checked to lie on
/// both surfaces at their UV points.
fn touching_points(x: &SurfaceIntersection, a: &Surface, b: &Surface) -> Vec<Vec3> {
    let k = Kernel::default();
    let mut points = Vec::new();
    for (i, curve) in x.curves_3d.iter().enumerate() {
        assert_eq!(x.curve_types[i], TsectCurveType::Touching, "curve {i}");
        assert!(curve.is_degenerate(&k, 1e-12, None).unwrap());
        let p = curve.evaluate_point(&k, 0.0).unwrap();
        for (uv_curve, srf) in [(&x.surface1_uv_curves[i], a), (&x.surface2_uv_curves[i], b)] {
            let uv = uv_curve.as_ref().unwrap().evaluate_point(&k, 0.0).unwrap();
            let q = srf
                .evaluate_point(&k, solidcore::geom::vector2d::Point2::new(uv.x, uv.y))
                .unwrap();
            assert!(p.distance(q) < 1e-9, "curve {i}: {p:?} vs {q:?}");
        }
        assert!(
            x.deviations[i] < 1e-12,
            "curve {i}: gap {}",
            x.deviations[i]
        );
        points.push(p);
    }
    points
}

/// Every expected point is matched by exactly one output point within `tol`.
fn assert_points(got: &[Vec3], expected: &[[f64; 3]], tol: f64) {
    assert_eq!(got.len(), expected.len(), "{got:?}");
    for e in expected {
        let e = Vec3::new(e[0], e[1], e[2]);
        let n = got.iter().filter(|p| p.distance(e) < tol).count();
        assert_eq!(n, 1, "{e:?} in {got:?}");
    }
}

fn parab_apex() -> (Surface, Surface) {
    let paraboloid = bezier(
        2,
        &[
            [-2.0, -2.0, 8.0],
            [-2.0, 0.0, 0.0],
            [-2.0, 2.0, 8.0],
            [0.0, -2.0, 0.0],
            [0.0, 0.0, -8.0],
            [0.0, 2.0, 0.0],
            [2.0, -2.0, 8.0],
            [2.0, 0.0, 0.0],
            [2.0, 2.0, 8.0],
        ],
    );
    (paraboloid, plane_z0())
}

fn four_points() -> (Surface, Surface) {
    let quartic = bezier(
        4,
        &[
            [-2.0, -2.0, 18.0],
            [-2.0, -1.0, -6.0],
            [-2.0, 0.0, 28.666666666666664],
            [-2.0, 1.0, -6.0],
            [-2.0, 2.0, 18.0],
            [-1.0, -2.0, -6.0],
            [-1.0, -1.0, -30.0],
            [-1.0, 0.0, 4.666666666666664],
            [-1.0, 1.0, -30.0],
            [-1.0, 2.0, -6.0],
            [0.0, -2.0, 28.666666666666664],
            [0.0, -1.0, 4.666666666666664],
            [0.0, 0.0, 39.33333333333333],
            [0.0, 1.0, 4.666666666666664],
            [0.0, 2.0, 28.666666666666664],
            [1.0, -2.0, -6.0],
            [1.0, -1.0, -30.0],
            [1.0, 0.0, 4.666666666666657],
            [1.0, 1.0, -30.0],
            [1.0, 2.0, -6.0],
            [2.0, -2.0, 18.0],
            [2.0, -1.0, -6.0],
            [2.0, 0.0, 28.66666666666663],
            [2.0, 1.0, -6.0],
            [2.0, 2.0, 18.0],
        ],
    );
    (quartic, plane_z0())
}

fn y23_t6_ex4() -> (Surface, Surface) {
    let sheet = bezier(
        2,
        &[
            [0.585068, -4.333621, 0.85481695],
            [0.509593, -2.311593, 0.5250965],
            [0.669593, -0.459544, 0.30328915],
            [2.443775, -4.072479, 1.4661931],
            [2.683173, -2.230938, 1.2737512500000001],
            [2.606752, -0.802878, 1.0327949],
            [4.352898999999999, -4.311293, 2.1702085999999996],
            [3.775453, -2.642078, 1.7177202500000002],
            [4.357476, -0.5659029999999998, 1.6100020500000003],
        ],
    );
    let curved = bezier(
        2,
        &[
            [1.552345, -3.378542, 1.3577967955163],
            [1.413697, -2.416558, 1.0168665955163],
            [1.564005, -1.463028, 1.0783390955163001],
            [2.315583, -4.142466, 1.1463361955163],
            [2.681395, -2.343542, 0.8564259955163002],
            [2.312151, -0.687855, 0.6307317455163002],
            [3.405102, -3.332677, 2.3130169955163],
            [3.0195729999999994, -2.454211, 1.8982061455163002],
            [3.414793, -1.522873, 2.0487266455163002],
        ],
    );
    (sheet, curved)
}

#[test]
fn issue_394_default_misses_the_paraboloid_apex() {
    let (a, b) = parab_apex();
    assert!(intersect(false, &a, &b).curves_3d.is_empty());
}

#[test]
fn issue_394_fix_finds_the_paraboloid_apex() {
    let (a, b) = parab_apex();
    let x = intersect(true, &a, &b);
    assert_points(&touching_points(&x, &a, &b), &[[0.0, 0.0, 0.0]], 1e-9);
}

#[test]
fn issue_394_default_misses_the_four_points() {
    let (a, b) = four_points();
    assert!(intersect(false, &a, &b).curves_3d.is_empty());
}

#[test]
fn issue_394_fix_finds_the_four_points() {
    let (a, b) = four_points();
    let x = intersect(true, &a, &b);
    assert_points(
        &touching_points(&x, &a, &b),
        &[
            [1.0, 1.0, 0.0],
            [1.0, -1.0, 0.0],
            [-1.0, 1.0, 0.0],
            [-1.0, -1.0, 0.0],
        ],
        1e-9,
    );
}

#[test]
fn issue_394_default_misses_the_y23_tangential_point() {
    let (a, b) = y23_t6_ex4();
    assert!(intersect(false, &a, &b).curves_3d.is_empty());
}

#[test]
fn issue_394_fix_finds_the_y23_tangential_point() {
    let (a, b) = y23_t6_ex4();
    let x = intersect(true, &a, &b);
    // the corpus' truth, to its own data's precision (the control points have six to
    // eight digits)
    assert_points(
        &touching_points(&x, &a, &b),
        &[[2.2399990803239955, -2.4200008322270063, 1.1469998029474493]],
        1e-5,
    );
}
