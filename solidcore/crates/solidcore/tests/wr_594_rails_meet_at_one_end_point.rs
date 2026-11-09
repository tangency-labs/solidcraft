//! WR-594 (tangency-labs/smlib@5fdefb9): a circular round ending at a degenerate
//! corner, whose cross section there is a point, kept a near pole instead of an exact
//! one. Each rail is lifted from its own surface, so at the corner's vertex the rails'
//! ends differ by the model's accuracy (6.5e-12 mm on SolidCraft's test-peg part).
//! `SmCircularCrossSectionFSG::CreateSurfaceFromCurves` makes the point section at
//! the first rail's end, but the second rail, a boundary of the skinned surface, kept
//! its own end, so the result's check refused the surface as close to singular at its
//! boundary (`SmSurface` test 5). The opt-in `fillet_rails_meet_at_one_end_point`
//! moves both rails' ends to their midpoint first, so the end collapses exactly.
//!
//! The peg's two-round sequence did not reach the changed branch in the port (the
//! second round fails or builds an invalid body before it), so this drives
//! `CircularCrossSectionFsg::create_surface_from_curves` directly with rails that
//! meet at their ends to 6.5e-12.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers
use solidcore::blend::fillet_standard_solver::p1::CircularCrossSectionFsg;
use solidcore::curve::curve::Curve;
use solidcore::geom::vector2d::Point2;
use solidcore::math::Point3;
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::Kernel;

/// The rails' ends' miss at the degenerate end.
const MISS: f64 = 6.5e-12;
const TOL: f64 = 1e-6;

/// A round of radius 2 about the x axis from x = 0 to 10 whose arc closes from a
/// quarter turn to a point: the rails start at (0, sqrt 2, +-sqrt 2) and meet at
/// (10, 2, 0), the second rail's end `MISS` off in z.
fn round(fix: bool) -> ([Point3; 4], Point3, Point3) {
    let mut k = Kernel::default();
    k.fixes.fillet_rails_meet_at_one_end_point = fix;
    let s2 = 2.0_f64.sqrt();
    let p = Point3::new;
    let line = |a: Point3, b: Point3| BSplineCurve::create_line_segment(3, a, b).unwrap();
    let mut center = Curve::from(line(p(0.0, 0.0, 0.0), p(10.0, 0.0, 0.0)));
    let mut rail1 = line(p(0.0, s2, s2), p(10.0, 2.0, 0.0));
    let mut rail2 = line(p(0.0, s2, -s2), p(10.0, 2.0, MISS));
    let srf = CircularCrossSectionFsg::default()
        .create_surface_from_curves(&k, &mut center, &mut rail1, &mut rail2, TOL, false)
        .unwrap();
    let dom = srf.natural_uv_domain();
    let (u, v) = (dom.u_interval(), dom.v_interval());
    let corners = [
        srf.evaluate_point(Point2::new(u.min(), v.min())).unwrap(),
        srf.evaluate_point(Point2::new(u.max(), v.min())).unwrap(),
        srf.evaluate_point(Point2::new(u.min(), v.max())).unwrap(),
        srf.evaluate_point(Point2::new(u.max(), v.max())).unwrap(),
    ];
    let t = rail1.natural_interval().max();
    (
        corners,
        rail1.evaluate_point(t).unwrap(),
        rail2.evaluate_point(t).unwrap(),
    )
}

/// The surface's corners at the degenerate end (x = 10).
fn end_corners(corners: &[Point3; 4]) -> Vec<Point3> {
    corners
        .iter()
        .copied()
        .filter(|c| (c.x - 10.0).abs() < 1e-6)
        .collect()
}

fn bits(p: Point3) -> [u64; 3] {
    [p.x.to_bits(), p.y.to_bits(), p.z.to_bits()]
}

#[test]
fn rails_keep_their_ends_by_default() {
    let (corners, e1, e2) = round(false);
    assert_eq!(bits(e1), bits(Point3::new(10.0, 2.0, 0.0)));
    assert!((e2.z - MISS).abs() < 1e-15, "{e2:?}");
    let ends = end_corners(&corners);
    assert_eq!(ends.len(), 2);
    assert_ne!(bits(ends[0]), bits(ends[1]), "a near pole");
}

#[test]
fn rails_end_at_one_point_with_the_fix() {
    let (corners, e1, e2) = round(true);
    let meet = Point3::new(10.0, 2.0, MISS / 2.0);
    assert_eq!(bits(e1), bits(e2), "the rails end at one point");
    assert!(e1.distance(meet) < 1e-15, "{e1:?}");
    let ends = end_corners(&corners);
    assert_eq!(ends.len(), 2);
    assert_eq!(bits(ends[0]), bits(ends[1]), "an exact pole");
    assert!(ends[0].distance(meet) < 1e-12);
}
