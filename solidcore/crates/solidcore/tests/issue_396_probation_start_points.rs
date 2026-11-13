//! tangency-labs/solidcore#396 part 2 (WR-79): `SmAdvSurfaceIntersector::FindInteriorCurves`
//! skips every start point whose surface normals are within 5 degrees (guess) or 2
//! degrees (solution). On a small closed loop between nearly tangent surfaces the
//! normals differ by only about the loop radius over the curvature radius, so the loop
//! never gets a start point and is missed: the NURBS paraboloid `z = x^2 + y^2` cut by
//! the plane `z = r^2` (a loop of radius `r`, ssi-corpus `c7_tiny_loop_parab_1e-4`,
//! `_1e-5`) has no output at a tolerance of 1e-6.
//!
//! With `Kernel::fixes.ssi_low_angle_start_on_probation` such a start point is traced on
//! probation and kept when it closes into a transversal loop larger than twice the
//! tolerance.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
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

/// The corpus' biquadratic paraboloid `z = x^2 + y^2` over `[-1, 1]^2`.
fn paraboloid() -> Surface {
    let pts = [
        [-1.0, -1.0, 2.0],
        [-1.0, 0.0, 0.0],
        [-1.0, 1.0, 2.0],
        [0.0, -1.0, 0.0],
        [0.0, 0.0, -2.0],
        [0.0, 1.0, 0.0],
        [1.0, -1.0, 2.0],
        [1.0, 0.0, 0.0],
        [1.0, 1.0, 2.0],
    ];
    let net: Vec<Vec<CPoint>> = pts
        .chunks(3)
        .map(|row| {
            row.iter()
                .map(|p| CPoint::new(p[0], p[1], p[2], 1.0))
                .collect()
        })
        .collect();
    let knots = vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0];
    let sur = NurbSurface::new(net, 2, 2, knots.clone(), knots).unwrap();
    Surface::from(BSplineSurface::from_nurb(sur, false))
}

/// The paraboloid against the plane `z = r^2` (a loop of radius `r`) at the corpus'
/// tolerance, 1e-6, as the ssi-corpus runner calls it.
fn parab_plane(fix: bool, r: f64) -> SurfaceIntersection {
    let mut k = Kernel::default();
    k.fixes.ssi_low_angle_start_on_probation = fix;
    let s1 = paraboloid();
    let d1 = s1.natural_uv_domain_with_nurb(&k).unwrap();
    let dom = Extent2d::from_bounds(-1.0, -1.0, 1.0, 1.0);
    let plane = Plane::new(
        Vec3::new(0.0, 0.0, r * r),
        Vec3::new(1.0, 0.0, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
        Vec2::new(1.0, 1.0),
        &dom,
    );
    let _ = plane.natural_uv_domain();
    let s2 = Surface::from(plane);
    let approx = approx_tol3d(&k, 1e-6);
    s1.global_surface_intersect(&k, &d1, &s2, &dom, [true, true], Some(approx), None)
        .unwrap()
}

/// The smallest and largest distance of the curves' points from the z axis, and the
/// largest distance from the plane.
fn radii(out: &SurfaceIntersection, r: f64) -> (f64, f64, f64) {
    let k = Kernel::default();
    let (mut lo, mut hi, mut dz) = (f64::INFINITY, 0.0_f64, 0.0_f64);
    for c in &out.curves_3d {
        let ivl = c.natural_interval();
        for i in 0..=32 {
            let t = ivl.min() + (ivl.max() - ivl.min()) * f64::from(i) / 32.0;
            let p = c.evaluate_point(&k, t).unwrap();
            let rho = (p.x * p.x + p.y * p.y).sqrt();
            lo = lo.min(rho);
            hi = hi.max(rho);
            dz = dz.max((p.z - r * r).abs());
        }
    }
    (lo, hi, dz)
}

#[test]
fn small_parab_loop_is_missed_by_default() {
    for r in [1e-4, 1e-5] {
        let out = parab_plane(false, r);
        assert!(out.curves_3d.is_empty(), "r = {r}: {:?}", out.curve_types);
    }
}

#[test]
fn small_parab_loop_is_traced_on_probation_with_the_fix() {
    for r in [1e-4, 1e-5] {
        let out = parab_plane(true, r);
        assert_eq!(out.curves_3d.len(), 1, "r = {r}: {:?}", out.curve_types);
        // a transversal loop (at a fraction of a degree, typed near tangent)
        assert!(
            matches!(
                out.curve_types[..],
                [TsectCurveType::Crossing | TsectCurveType::NearTangent]
            ),
            "r = {r}: {:?}",
            out.curve_types
        );
        let (lo, hi, dz) = radii(&out, r);
        // within the 1e-6 tolerance of the true circle
        assert!(
            (lo - r).abs() <= 1e-6 && (hi - r).abs() <= 1e-6 && dz <= 1e-6,
            "r = {r}: radius {lo}..{hi}, off the plane {dz}"
        );
    }
}

#[test]
fn transversal_cut_is_unchanged_by_the_fix() {
    // a plane well above the apex: normals far apart, no probation
    for r in [0.5, 0.8] {
        let a = parab_plane(false, r);
        let b = parab_plane(true, r);
        assert_eq!(a.curve_types, b.curve_types, "r = {r}");
        assert_eq!(a.curves_3d.len(), b.curves_3d.len(), "r = {r}");
        let (lo_a, hi_a, _) = radii(&a, r);
        let (lo_b, hi_b, _) = radii(&b, r);
        assert_eq!(lo_a.to_bits(), lo_b.to_bits(), "r = {r}");
        assert_eq!(hi_a.to_bits(), hi_b.to_bits(), "r = {r}");
    }
}
