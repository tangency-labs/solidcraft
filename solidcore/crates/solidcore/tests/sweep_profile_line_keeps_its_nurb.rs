//! A sweep splits its profile curves at C1 discontinuities through
//! `((SmBSplineCurve *)cr3DCurves[ii])->SubdivideAtDiscontinuity(..)` (`SmPrimitiveCreation.cpp`'s
//! seven-argument `CreateLinearSweep`, :1906, and `CreateRotationalSweep`, :2899). Upstream's
//! `SmLine` and `SmEllipse` are `SmBSplineCurve`s that hold their NURB, so the split of a line
//! returns its one copy. The port builds a line's and a conic's NURB lazily. The split took
//! a copy of the curve's bare B-spline base, which had none, and `make_nurb` refused it (1001).
//! The fork passes that status on (tangency-labs/smlib c8f6a31, WR-662), so with
//! `Kernel::fixes.sweep_statuses_pass_on`, SolidCraft's preset, these sweeps failed. Without
//! the flag the status was dropped and the curve stayed whole.
//!
//! Each sweep below builds the same body with the flag off and on.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test

use solidcore::curve::curve::Curve;
use solidcore::curve::ellipse::Ellipse;
use solidcore::curve::line::Line;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::Point3;
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::crv_basic::Curve as NurbCurve;
use solidcore::nurbs::math::CPoint;
use solidcore::sweep::api_brep::{create_linear_sweep, create_rotational_sweep};
use solidcore::{Brep, Kernel, Vec3};

fn kernel(pass_on: bool) -> Kernel {
    let mut k = Kernel::default();
    k.fixes.sweep_statuses_pass_on = pass_on;
    k
}

fn line(a: Vec3, b: Vec3) -> Curve {
    Curve::from(Line::from_points(a, b, 3))
}

fn volume(k: &Kernel, body: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attributes = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, body, 1e-9, &attributes).expect("a volume")
}

/// tangency-labs/solidcore#2540's profile: a cubic B-spline span 7.1e-7 mm off its chord and
/// two lines, extruded 44 along +x to the prism of volume 98.130681517195 64 mm³ (the closed
/// form in `issue_2540_near_straight_spline_extrusion.rs`).
const P0: [f64; 3] = [-22.0, -2.808157, -25.446366];
const P1: [f64; 3] = [-22.0, -2.828967043, -25.743002617];
const P2: [f64; 3] = [-22.0, -2.849776091, -26.039639304];
const P3: [f64; 3] = [-22.0, -2.870585, -26.336276];
const P4: [f64; 3] = [-22.0, 2.148371345929137, -26.24121568504867];
const PRISM_VOLUME: f64 = 98.130_681_517_195_64;

fn point(p: [f64; 3]) -> Vec3 {
    Vec3::new(p[0], p[1], p[2])
}

#[test]
fn a_profile_of_lines_and_a_spline_extrudes_with_its_statuses_passed_on() {
    let points = [P0, P1, P2, P3]
        .into_iter()
        .map(|p| CPoint::from_point(point(p)))
        .collect();
    let nurb = NurbCurve::new(points, 3, vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0]).unwrap();
    let span = Curve::from(BSplineCurve::from_nurb(nurb, 3, false));
    let (back, front) = (line(point(P3), point(P4)), line(point(P4), point(P0)));
    for pass_on in [false, true] {
        let mut k = kernel(pass_on);
        let mut body = create_linear_sweep(
            &mut k,
            &[Some(&span), Some(&back), Some(&front)],
            Vec3::X,
            44.0,
            true,
        )
        .unwrap_or_else(|e| panic!("pass_on {pass_on}: {e:?}"));
        assert_eq!(body.face_list().len(), 5, "pass_on {pass_on}");
        let v = volume(&k, &mut body);
        assert!(
            (v - PRISM_VOLUME).abs() <= 1e-12 * PRISM_VOLUME,
            "pass_on {pass_on}: volume {v}"
        );
    }
}

#[test]
fn two_lines_sweep_to_an_open_sheet_with_their_statuses_passed_on() {
    let lines = [
        line(Point3::ZERO, Point3::new(1.0, 0.0, 0.0)),
        line(Point3::new(1.0, 0.0, 0.0), Point3::new(1.0, 1.0, 0.0)),
    ];
    for pass_on in [false, true] {
        let mut k = kernel(pass_on);
        let body = create_linear_sweep(
            &mut k,
            &[Some(&lines[0]), Some(&lines[1])],
            Vec3::Z,
            1.0,
            false,
        )
        .unwrap_or_else(|e| panic!("pass_on {pass_on}: {e:?}"));
        assert_eq!(body.face_list().len(), 2, "pass_on {pass_on}");
    }
}

/// A conic (the analytic circle `Ellipse::circle_from_3_points` makes) as a capped profile,
/// and a line revolved a full turn (`CreateRotationalSweep`'s split): each builds with the
/// flag off and on.
#[test]
fn a_circle_extrudes_and_a_line_revolves_with_their_statuses_passed_on() {
    for pass_on in [false, true] {
        let mut k = kernel(pass_on);
        let circle = Curve::from(
            Ellipse::circle_from_3_points(
                &k,
                Point3::new(2.0, 0.0, 0.0),
                Point3::new(0.0, 2.0, 0.0),
                Point3::new(-2.0, 0.0, 0.0),
                3,
                true,
            )
            .unwrap(),
        );
        let mut body = create_linear_sweep(&mut k, &[Some(&circle)], Vec3::Z, 3.0, true)
            .unwrap_or_else(|e| panic!("pass_on {pass_on}, circle: {e:?}"));
        assert_eq!(body.face_list().len(), 3, "pass_on {pass_on}, circle");
        let v = volume(&k, &mut body);
        let want = std::f64::consts::PI * 4.0 * 3.0;
        assert!(
            (v - want).abs() <= 1e-9 * want,
            "pass_on {pass_on}: circle volume {v}"
        );

        let side = line(Point3::new(1.0, 0.0, 0.0), Point3::new(1.0, 0.0, 2.0));
        let body =
            create_rotational_sweep(&mut k, &[Some(&side)], Vec3::ZERO, Vec3::Z, 360.0, false)
                .unwrap_or_else(|e| panic!("pass_on {pass_on}, revolve: {e:?}"));
        assert!(!body.face_list().is_empty(), "pass_on {pass_on}, revolve");
    }
}
