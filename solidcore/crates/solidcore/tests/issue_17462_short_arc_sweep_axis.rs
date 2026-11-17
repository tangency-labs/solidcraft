//! SolidCraft #17462: a linear sweep turns a profile curve that is an arc, swept square to its
//! plane, into a cylinder (WR-565). The arc's frame comes from fitting the curve, and for a
//! very short arc that fit is ill-conditioned: on 0.008 mm of a spline, a 0.005 degree arc of
//! radius 88 mm whose control points carry an ulp of rounding off their plane, its normal
//! leaned 5e-8 rad off the sweep. The sweep accepted it (53 mm times that is under the B-rep
//! tolerance) and built the cylinder on that normal, so the wall leaned across the sweep, its
//! far edge 2.6e-6 mm off the swept curve and its rows no translates of each other, and
//! SolidCraft could not recognise the cylinder in its own NURBS form.
//!
//! The cylinder's axis is now the sweep direction itself, its origin in the profile's plane.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test

use solidcore::curve::curve::Curve;
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::crv_basic::Curve as NurbCurve;
use solidcore::nurbs::math::CPoint;
use solidcore::surface::implicit::AnalyticSupport;
use solidcore::sweep::api_brep::create_linear_sweep;
use solidcore::topology::brep::p1;
use solidcore::{Kernel, Vec3};

/// The cubic Bézier of a 0.005 degree arc of radius 88.3 about (-8.19, 145.98) at z = -53,
/// its two inner control points an ulp or two off that plane, as a profile placed there by a
/// transform holds them.
fn short_arc() -> Curve {
    let (cx, cy, r) = (-8.185_840_780_2, 145.979_194_294, 88.295_980_686);
    let (a0, sweep) = (-1.4235_f64, 0.005_f64.to_radians());
    let on = |a: f64| (cx + r * a.cos(), cy + r * a.sin());
    let tangent = |a: f64| (-a.sin(), a.cos());
    let h = 4.0 / 3.0 * (sweep / 4.0).tan() * r;
    let (p0, p3) = (on(a0), on(a0 + sweep));
    let (t0, t3) = (tangent(a0), tangent(a0 + sweep));
    let points = [
        (p0.0, p0.1, -53.0),
        (p0.0 + h * t0.0, p0.1 + h * t0.1, -53.000_000_000_000_01),
        (p3.0 - h * t3.0, p3.1 - h * t3.1, -52.999_999_999_999_99),
        (p3.0, p3.1, -53.0),
    ]
    .into_iter()
    .map(|(x, y, z)| CPoint::from_point(Vec3::new(x, y, z)))
    .collect();
    let nurb = NurbCurve::new(points, 3, vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0]).unwrap();
    Curve::from(BSplineCurve::from_nurb(nurb, 3, false))
}

#[test]
fn a_very_short_arc_swept_square_is_a_cylinder_about_the_sweep_direction() {
    let mut k = Kernel::default();
    k.fixes.linear_sweep_arc_is_cylinder = true;
    let curve = short_arc();
    // The case needs the fit's normal off the sweep: it is what the sweep corrects.
    // (the sweep's zero, `1000 SM_EFF_ZERO (1 + the ends' largest coordinate)`)
    let (frame, ..) = curve
        .is_arc(&k, 8, 1e-9 * (1.0 + 146.0))
        .unwrap()
        .expect("an arc");
    let normal = frame.z_axis();
    assert!(
        normal.x != 0.0 || normal.y != 0.0,
        "the fitted normal is exact: {normal:?}"
    );
    assert!(normal.x.hypot(normal.y) < 1e-6, "{normal:?}");

    let body = create_linear_sweep(
        &mut k,
        &[Some(&curve)],
        Vec3::new(0.0, 0.0, 1.0),
        53.0,
        false,
    )
    .unwrap();
    let faces = p1::faces(&body, None).unwrap();
    let [face] = faces.as_slice() else {
        panic!("one face: {faces:?}");
    };
    let surface = body.face(*face).unwrap().surface.as_ref().unwrap();
    let support = AnalyticSupport::of(surface).expect("a cylinder");
    let AnalyticSupport::Cylinder { placement, radius } = support else {
        panic!("{support:?}");
    };
    let axis = placement.z_axis();
    assert!(
        axis.x == 0.0 && axis.y == 0.0 && axis.z.abs() == 1.0,
        "{axis:?}"
    );
    // The curve's ends are on the cylinder, at both ends of the sweep.
    let ends = curve.ends(&k).unwrap();
    for p in [ends.start, ends.end] {
        for p in [p, p + Vec3::new(0.0, 0.0, 53.0)] {
            let off = (p - placement.origin()).cross(axis).length() - radius;
            assert!(off.abs() < 1e-9, "{p:?} is {off:e} off the cylinder");
        }
    }
}
