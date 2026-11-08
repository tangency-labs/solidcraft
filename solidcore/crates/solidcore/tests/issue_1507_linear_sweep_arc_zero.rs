//! tangency-labs/solidcore#1507 (WR-576): `SmPrimitiveCreation::CreateLinearSweep` tests a
//! profile curve for an arc to the B-rep's tolerance, so a curve that is only near a
//! circle is swept as the `SmCylinder` of its fitted circle, a body that is not the sweep
//! of the curve. The opt-in `Kernel::fixes.linear_sweep_arc_to_cylinder_zero` (SolidCraft
//! change, tangency-labs/smlib fd59111) tests it to the zero an `SmCylinder`'s own check
//! holds its generating curve to, `SM_EFF_ZERO * 1000 * (1 + size)`: only a curve that is
//! a circle becomes a cylinder, any other is swept as itself (`SmSurfOfExtrusion`).
//!
//! Without #1495's `linear_sweep_arc_is_cylinder` only a whole circle becomes a cylinder,
//! so here the near circle is a closed curve: the exact rational circle stretched by
//! 5e-7 along x, an ellipse about 2e-7 off its fitted circle (closer than the B-rep
//! tolerance, 1e-5, farther than the cylinder's zero, about 2e-9 at this size). The
//! issue's text-outline letters are swept exactly either way.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::curve::curve::Curve;
use solidcore::curve::ellipse::Ellipse;
use solidcore::curve::line::Line;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::crv_basic::Curve as NurbCurve;
use solidcore::nurbs::math::CPoint;
use solidcore::surface::surface::SurfaceKind;
use solidcore::sweep::api_brep::create_linear_sweep;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::{Brep, Kernel, Vec3};

use std::f64::consts::{FRAC_1_SQRT_2, PI};

/// The surface kinds of the B-rep's faces, in face order.
fn kinds(b: &Brep) -> Vec<&'static str> {
    b.face_list()
        .iter()
        .map(
            |&f| match b.face(f).unwrap().surface.as_ref().unwrap().kind() {
                SurfaceKind::Cone(c) if c.is_cylinder_class() => "cylinder",
                SurfaceKind::Plane(_) => "plane",
                SurfaceKind::SurfOfExtrusion(_) => "extrusion",
                _ => "other",
            },
        )
        .collect()
}

/// `curves` (in z = -1) swept 1 along +z and capped, with the fix `on`: the body, its
/// volume's error relative to `area` (the exact volume) and the number of failed checks
/// of SMLib's whole-body check (`smlib_brep_check`, level 2).
fn sweep(curves: &[Curve], on: bool, area: f64) -> (Brep, f64, usize) {
    let mut k = Kernel::default();
    k.fixes.linear_sweep_arc_to_cylinder_zero = on;
    let refs: Vec<Option<&Curve>> = curves.iter().map(Some).collect();
    let mut b = create_linear_sweep(&mut k, &refs, Vec3::Z, 1.0, true).unwrap();
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    let volume = brep_volume(&k, &mut b, 1e-10, &attrs).unwrap();
    let failures = check_brep(&mut k, &mut b, CheckLevel::Level2)
        .unwrap()
        .failure_count();
    (b, (volume - area).abs() / area, failures)
}

/// The rational quadratic circle of radius `r` about `c` in its z plane (9 control
/// points), stretched by `1 + stretch` along x: an exact ellipse, C1 everywhere.
fn nurbs_circle(c: Vec3, r: f64, stretch: f64) -> Curve {
    let h = FRAC_1_SQRT_2;
    let pw = [
        (r, 0.0, 1.0),
        (r, r, h),
        (0.0, r, 1.0),
        (-r, r, h),
        (-r, 0.0, 1.0),
        (-r, -r, h),
        (0.0, -r, 1.0),
        (r, -r, h),
        (r, 0.0, 1.0),
    ]
    .iter()
    .map(|&(x, y, w)| CPoint::new(w * (c.x + x * (1.0 + stretch)), w * (c.y + y), w * c.z, w))
    .collect();
    let knots = vec![
        0.0, 0.0, 0.0, 0.25, 0.25, 0.5, 0.5, 0.75, 0.75, 1.0, 1.0, 1.0,
    ];
    Curve::from(BSplineCurve::from_nurb(
        NurbCurve::new(pw, 2, knots).unwrap(),
        3,
        false,
    ))
}

#[test]
fn issue_1507_a_curve_near_a_circle_is_swept_as_itself() {
    let (r, stretch) = (0.4, 5.0e-7);
    let near = [nurbs_circle(Vec3::new(0.5, 0.5, -1.0), r, stretch)];
    let area = PI * r * r * (1.0 + stretch);

    // the fix off (upstream): the cylinder of the fitted circle, not the sweep of the
    // curve; its volume is off by about 5e-8 of itself
    let (b, error, failures) = sweep(&near, false, area);
    assert_eq!(kinds(&b), ["cylinder", "plane", "plane"]);
    assert!(error > 1e-8, "fix off: volume error {error:e}");
    assert_eq!(failures, 0, "fix off");

    // the fix on: the curve's own extrusion, of the curve's exact volume
    let (b, error, failures) = sweep(&near, true, area);
    assert_eq!(kinds(&b), ["extrusion", "plane", "plane"]);
    assert!(error < 1e-12, "fix on: volume error {error:e}");
    assert_eq!(failures, 0, "fix on");
}

#[test]
fn issue_1507_a_circle_is_still_a_cylinder() {
    // only a curve that is a circle becomes a cylinder: an exact rational circle and an
    // analytic circle, with the fix on and off
    let (c, r) = (Vec3::new(0.5, 0.5, -1.0), 0.4);
    let k = Kernel::default();
    let frame = Axis2Placement::new(c, Vec3::X, Vec3::Y);
    let circles = [
        ("nurbs", nurbs_circle(c, r, 0.0)),
        (
            "analytic",
            Curve::from(Ellipse::create_canonical_circle(&k, &frame, r, None, None).unwrap()),
        ),
    ];
    for (name, circle) in circles {
        for on in [false, true] {
            let (b, error, failures) = sweep(std::slice::from_ref(&circle), on, PI * r * r);
            assert_eq!(kinds(&b), ["cylinder", "plane", "plane"], "{name} fix {on}");
            assert!(error < 1e-12, "{name} fix {on}: volume error {error:e}");
            assert_eq!(failures, 0, "{name} fix {on}");
        }
    }
}

/// A piece of a text outline from its start to its end: a quadratic Bezier with its
/// middle control point, or a line (`None`).
type Piece = ((f64, f64), Option<(f64, f64)>, (f64, f64));

/// The outline's curves in z = -1.
fn outline(pieces: &[Piece]) -> Vec<Curve> {
    let p = |(x, y): (f64, f64)| Vec3::new(x, y, -1.0);
    pieces
        .iter()
        .map(|&(a, control, c)| match control {
            None => Curve::from(Line::create_line_segment(3, p(a), p(c), None).unwrap()),
            Some(b) => {
                let pw = vec![
                    CPoint::from_point(p(a)),
                    CPoint::from_point(p(b)),
                    CPoint::from_point(p(c)),
                ];
                let nurb = NurbCurve::new(pw, 2, vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0]).unwrap();
                Curve::from(BSplineCurve::from_nurb(nurb, 3, false))
            }
        })
        .collect()
}

/// The area the outline's pieces bound (counterclockwise outer contours, clockwise
/// holes), by Green's theorem over the exact Beziers: the sum of the integrals of x dy,
/// by Simpson's rule, exact for their cubic integrands (a line is the Bezier whose
/// middle control point is its midpoint).
fn area(pieces: &[Piece]) -> f64 {
    let mut sum = 0.0;
    for &((x0, y0), control, (x2, y2)) in pieces {
        let (x1, y1) = control.unwrap_or(((x0 + x2) / 2.0, (y0 + y2) / 2.0));
        let x_dy = |t: f64| {
            let x = (1.0 - t) * (1.0 - t) * x0 + 2.0 * t * (1.0 - t) * x1 + t * t * x2;
            x * 2.0 * ((1.0 - t) * (y1 - y0) + t * (y2 - y1))
        };
        sum += (x_dy(0.0) + 4.0 * x_dy(0.5) + x_dy(1.0)) / 6.0;
    }
    sum
}

/// The four quadratic quarters about `(cx, cy)` of radius `r`, counterclockwise, or
/// clockwise for a hole.
fn bowl(cx: f64, cy: f64, r: f64, hole: bool) -> Vec<Piece> {
    let mut pieces = vec![
        ((cx + r, cy), Some((cx + r, cy + r)), (cx, cy + r)),
        ((cx, cy + r), Some((cx - r, cy + r)), (cx - r, cy)),
        ((cx - r, cy), Some((cx - r, cy - r)), (cx, cy - r)),
        ((cx, cy - r), Some((cx + r, cy - r)), (cx + r, cy)),
    ];
    if hole {
        pieces.reverse();
        for piece in &mut pieces {
            *piece = (piece.2, piece.1, piece.0);
        }
    }
    pieces
}

#[test]
fn issue_1507_a_text_outline_is_swept_exactly() {
    // two letters of a text outline (lines and quadratic Beziers) in z = -1, each swept
    // 1 along +z: a "D" whose stem bulges by 0.003 (a Bezier near an arc, as the issue's)
    // and an "o" with its counter
    let d: Vec<Piece> = vec![
        ((0.1, 0.0), None, (0.4, 0.0)),
        ((0.4, 0.0), Some((0.9, 0.0)), (0.9, 0.5)),
        ((0.9, 0.5), Some((0.9, 1.0)), (0.4, 1.0)),
        ((0.4, 1.0), None, (0.1, 1.0)),
        ((0.1, 1.0), Some((0.094, 0.5)), (0.1, 0.0)),
    ];
    let mut o = bowl(1.5, 0.5, 0.45, false);
    o.extend(bowl(1.5, 0.5, 0.25, true));
    for (name, letter) in [("D", d), ("o", o)] {
        let curves = outline(&letter);
        for on in [false, true] {
            let (b, error, failures) = sweep(&curves, on, area(&letter));
            assert!(
                !kinds(&b).contains(&"cylinder"),
                "{name} fix {on}: {:?}",
                kinds(&b)
            );
            assert!(error < 1e-12, "{name} fix {on}: volume error {error:e}");
            assert_eq!(failures, 0, "{name} fix {on}");
        }
    }
}
