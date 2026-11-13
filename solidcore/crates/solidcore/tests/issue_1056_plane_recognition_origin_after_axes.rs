//! #1056 (WR-652, the fork's tangency-labs/smlib@0286682): `Plane::is_nurb_surface_plane`
//! (`SmPlane::IsNurbSurfacePlane`) placed a recognised plane's origin, its point at
//! (u, v) = (0, 0), along the B-spline's own Y axis, and only then turned that axis to
//! be perpendicular to X. With a domain far from v = 0 the turn is multiplied by the
//! distance to the origin.
//!
//! The case is SolidCraft rig sweeps/4305's swept groove wall: a planar bilinear patch
//! over u 0..1, v 4..5, 40 mm along v, whose v direction leans `TURN = 3.2116e-8` rad
//! off perpendicular to u. Closed form (to first order in `TURN`, the second-order terms
//! are below 1e-14 mm):
//! - flag off (upstream): the corner at v = 4 is `4 * 40 * TURN = 5.1386e-6` mm off the
//!   wall and the corner at v = 5 is `200 * TURN = 6.4232e-6` mm off. Those are the
//!   vertex/face gaps the SolidCraft check refused against its 5e-6 limit.
//! - flag on (fixed): the corner at v = 4 is on the wall and the corner at v = 5 is off
//!   by the wall's own span times the turn, `40 * TURN = 1.2846e-6` mm. This is the
//!   analytic limit, since an orthogonal plane cannot follow the lean exactly.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers
use solidcore::geom::extent2d::Extent2d;
use solidcore::math::{Point2, Point3};
use solidcore::nurbs::b_spline_surface::BSplineSurface;
use solidcore::surface::plane::Plane;
use solidcore::Kernel;

/// The profile's turn where the path's rounded arcs meet its lines (rad).
const TURN: f64 = 3.2116e-8;
/// The wall's length along v (mm).
const SPAN: f64 = 40.0;
/// Rounding slack for coordinates of size ~200 mm (a few hundred ulps, far below the gaps).
const SLACK: f64 = 1.0e-12;

/// The sweeps/4305 wall: 1 mm along u, `SPAN` along v leaning by `TURN`, on u 0..1, v 4..5.
fn groove_wall() -> BSplineSurface {
    let lean = SPAN * TURN.tan();
    let mut wall = BSplineSurface::create_bilinear_surface(
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(1.0, 0.0, 0.0),
        Point3::new(lean, SPAN, 0.0),
        Point3::new(1.0 + lean, SPAN, 0.0),
    )
    .unwrap();
    wall.reparameterize(&Extent2d::from_bounds(0.0, 4.0, 1.0, 5.0))
        .unwrap();
    wall
}

/// The recognised plane's distance from the wall at the corners (0, 4) and (0, 5).
fn corner_gaps(k: &Kernel) -> (f64, f64) {
    let wall = groove_wall();
    let plane = Plane::is_nurb_surface_plane(k, &wall, 1.0)
        .unwrap()
        .expect("the wall is a plane");
    let gap = |uv: Point2| {
        plane
            .evaluate_point(uv)
            .distance(wall.evaluate_point(uv).unwrap())
    };
    (gap(Point2::new(0.0, 4.0)), gap(Point2::new(0.0, 5.0)))
}

#[test]
fn flag_off_places_the_origin_along_the_unturned_axis() {
    let k = Kernel::default();
    assert!(!k.fixes.plane_recognition_origin_after_axes);
    let (low, high) = corner_gaps(&k);
    assert!(
        (low - 4.0 * SPAN * TURN).abs() < SLACK,
        "v = 4 corner gap {low:e}, expected {:e}",
        4.0 * SPAN * TURN
    );
    assert!(
        (high - 5.0 * SPAN * TURN).abs() < SLACK,
        "v = 5 corner gap {high:e}, expected {:e}",
        5.0 * SPAN * TURN
    );
    // Over SolidCraft's 5e-6 vertex gap limit, as the refused cut reported.
    assert!(low > 5.0e-6 && high > 5.0e-6);
}

#[test]
fn flag_on_places_the_origin_along_the_plane_axes() {
    let mut k = Kernel::default();
    k.fixes.plane_recognition_origin_after_axes = true;
    let (low, high) = corner_gaps(&k);
    assert!(low < SLACK, "v = 4 corner gap {low:e}, expected 0");
    assert!(
        (high - SPAN * TURN).abs() < SLACK,
        "v = 5 corner gap {high:e}, expected {:e}",
        SPAN * TURN
    );
}
