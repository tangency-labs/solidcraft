//! Regression tests for tangency-labs/solidcore#1490: a revolve's torus is built inside
//! out, and its flag is lost (`SmSurfOfRevolution` Test #11 `"InsideOut GenCurve"`).
//!
//! - With `fixes.revolve_analytic_profile_faces`, `analytic_revolution` made an arc
//!   falling round its minor circle an inside-out torus (and a falling meridian an
//!   inside-out sphere). Fork tangency-labs/smlib 9853e54 builds them with
//!   `CreateCanonical(.., FALSE, FALSE, ..)`, never inside out.
//! - Upstream's `SmTorus` constructor never sets `m_bInsideOut` and its copy
//!   constructor resets it, so an inside-out torus fails Test #11 (WR-500); the opt-in
//!   `fixes.torus_keeps_inside_out` keeps the flag.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::curve::curve::{Curve, CurveKind};
use solidcore::geom::assert_array::{AssertArray, ObjectKey};
use solidcore::geom::attribute::AttributeStore;
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent2d::Extent2d;
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::nurbs::b_spline_curve::{BSplineCurve, NurbCircleParam};
use solidcore::surface::surface::{Surface, SurfaceKind};
use solidcore::surface::torus::{Torus, TorusOptions};
use solidcore::sweep::api_brep::create_rotational_sweep;
use solidcore::{Kernel, Point3, Vec3};

/// Whether `SmSurfOfRevolution` Test #11 fails on `s`.
fn test11_fails(k: &Kernel, s: &Surface, what: &str) -> bool {
    let mut list = AssertArray::new();
    let _ = s
        .assert_valid(k, Some(&mut list), 0, ObjectKey(1))
        .unwrap_or_else(|e| panic!("{what}: {e:?}"));
    list.reports().iter().any(|r| r.test_index == 11 && !r.ok)
}

/// The torus's and its generating circle's inside-out flags.
fn flags(t: &Torus) -> (bool, bool) {
    let gen = match t.base().gen_curve().unwrap().kind() {
        CurveKind::Ellipse(e) => e.inside_out(),
        _ => panic!("the generator is a circle"),
    };
    (t.inside_out(), gen)
}

#[test]
fn issue_1490_inside_out_torus_keeps_its_flag() {
    for fix in [false, true] {
        let mut k = Kernel::default();
        k.fixes.torus_keeps_inside_out = fix;
        // as the rig runs: the recognised torus keeps the tested NURB (SetFromGwNurb,
        // whose rebuild sets the flag)
        k.fixes.found_torus_sphere_keep_tested_nurb = true;
        let mut t = Torus::new(
            &k,
            Point3::new(0.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            Extent2d::from_bounds(0.0, -90.0, 270.0, 90.0),
            20.0,
            4.0,
            TorusOptions {
                swap_uv: false,
                inside_out: true,
                circle_nurb: None,
            },
        )
        .unwrap();
        t.make_nurb(&k).unwrap();
        // made, copied, recognised from its NURB and the recognised torus's copy
        let found = Torus::is_nurb_surface_torus(&k, t.base().base(), 1.0)
            .unwrap()
            .unwrap();
        let (copy, found_copy) = (t.clone(), found.clone());
        let cases = [
            ("made", t),
            ("copy", copy),
            ("recognised", found),
            ("recognised copy", found_copy),
        ];
        for (name, s) in cases {
            // upstream: the constructor leaves the surface's flag FALSE, only the
            // rebuild sets it, and copies reset it
            let kept = fix || name == "recognised";
            assert_eq!(flags(&s), (kept, true), "{name} fix {fix}");
            assert_eq!(
                test11_fails(&k, &Surface::from(s), &format!("{name} {fix}")),
                !kept,
                "{name} fix {fix}"
            );
        }
    }
}

fn line(a: Vec3, b: Vec3) -> Curve {
    Curve::from(BSplineCurve::create_line_segment(3, a, b).unwrap())
}

/// The rig's sweeps/4007 profile: the line x = 14, z -14..14 and the half circle of
/// radius 14 about (14, 0, 0) through (28, 0, 0), wound either way, turned 270 degrees
/// about Z: the horn torus R = r = 14.
fn horn_torus(k: &mut Kernel, down: bool) -> solidcore::Brep {
    let (top, bottom) = (Vec3::new(14.0, 0.0, 14.0), Vec3::new(14.0, 0.0, -14.0));
    let centre = Vec3::new(14.0, 0.0, 0.0);
    // the arc from the top round to the bottom (down) or from the bottom up
    let x = if down { Vec3::Z } else { -Vec3::Z };
    let arc = Curve::from(
        BSplineCurve::create_circle_segment(
            3,
            &Axis2Placement::new(centre, x, Vec3::X),
            14.0,
            0.0,
            180.0,
            NurbCircleParam::Quadratic,
        )
        .unwrap(),
    );
    let side = if down {
        line(bottom, top)
    } else {
        line(top, bottom)
    };
    create_rotational_sweep(
        k,
        &[Some(&side), Some(&arc)],
        Vec3::ZERO,
        Vec3::Z,
        270.0,
        true,
    )
    .unwrap()
}

#[test]
fn issue_1490_revolved_torus_is_never_inside_out() {
    for down in [false, true] {
        let mut k = Kernel::default();
        k.fixes.revolve_analytic_profile_faces = true;
        let mut b = horn_torus(&mut k, down);
        let tori: Vec<Torus> = b
            .face_list()
            .iter()
            .filter_map(
                |&f| match b.face(f).unwrap().surface.as_ref().unwrap().kind() {
                    SurfaceKind::Torus(t) => Some((**t).clone()),
                    _ => None,
                },
            )
            .collect();
        assert_eq!(tori.len(), 1, "down {down}");
        let t = &tori[0];
        // fork 9853e54: never inside out, and so the surface and generator agree
        assert_eq!(flags(t), (false, false), "down {down}");
        assert!(
            !test11_fails(&k, &Surface::from(t.clone()), "revolve"),
            "down {down}"
        );
        // Pappus: half disc pi 14^2 / 2 at 14 + 4 14 / (3 pi) from the axis, 3/2 pi
        let area = std::f64::consts::PI * 196.0 / 2.0;
        let centroid = 14.0 + 56.0 / (3.0 * std::f64::consts::PI);
        let want = area * centroid * 1.5 * std::f64::consts::PI;
        let store = AttributeStore::default();
        let attrs = PropertyAttributes {
            store: &store,
            key_of: &|_| solidcore::geom::attribute::ObjectKey(0),
        };
        let got = brep_volume(&k, &mut b, 1e-6, &attrs).unwrap();
        assert!(
            (got - want).abs() <= 1e-6 * want,
            "down {down}: {got} {want}"
        );
    }
}

#[test]
fn issue_1490_revolved_sphere_is_never_inside_out() {
    // a quarter disc of radius 3 (rim, arc, axis), wound either way, turned 270 degrees
    for down in [false, true] {
        let mut k = Kernel::default();
        k.fixes.revolve_analytic_profile_faces = true;
        let (o, rim, pole) = (
            Vec3::ZERO,
            Vec3::new(3.0, 0.0, 0.0),
            Vec3::new(0.0, 0.0, 3.0),
        );
        // the meridian from the rim up to the pole, or from the pole down
        let (x, y) = if down {
            (Vec3::Z, Vec3::X)
        } else {
            (Vec3::X, Vec3::Z)
        };
        let arc = Curve::from(
            BSplineCurve::create_circle_segment(
                3,
                &Axis2Placement::new(o, x, y),
                3.0,
                0.0,
                90.0,
                NurbCircleParam::Quadratic,
            )
            .unwrap(),
        );
        let curves = if down {
            [line(o, pole), arc, line(rim, o)]
        } else {
            [line(o, rim), arc, line(pole, o)]
        };
        let refs: Vec<Option<&Curve>> = curves.iter().map(Some).collect();
        let mut b =
            create_rotational_sweep(&mut k, &refs, Vec3::ZERO, Vec3::Z, 270.0, true).unwrap();
        let spheres: Vec<bool> = b
            .face_list()
            .iter()
            .filter_map(
                |&f| match b.face(f).unwrap().surface.as_ref().unwrap().kind() {
                    SurfaceKind::Sphere(s) => Some(s.inside_out()),
                    _ => None,
                },
            )
            .collect();
        assert_eq!(spheres, [false], "down {down}");
        let want = 2.0 / 3.0 * std::f64::consts::PI * 27.0 * 0.75;
        let store = AttributeStore::default();
        let attrs = PropertyAttributes {
            store: &store,
            key_of: &|_| solidcore::geom::attribute::ObjectKey(0),
        };
        let got = brep_volume(&k, &mut b, 1e-6, &attrs).unwrap();
        assert!(
            (got - want).abs() <= 1e-6 * want,
            "down {down}: {got} {want}"
        );
    }
}
