//! Regression tests for tangency-labs/solidcore#1508: `SmSurfOfExtrusion::AssertValid`
//! test 8 ("origin is a point on the generating curve") point-solves the origin with an
//! unscaled `SM_EFF_ZERO`, so an origin that the solve resolves a few 1e-12 off the
//! curve is reported missing at coordinates of about 10.
//!
//! The pinned C++ keeps that unscaled solve (and so does the port by default). With
//! `Kernel::fixes.extrusion_origin_scaled_zero` test 8 instead uses the issue's
//! coordinate-aware zero `SM_EFF_ZERO * (1.0 + origin.max_dimension())`; that is its own
//! factor, not the factor 100 of the neighbouring point tests (WR-511, #1508;
//! tangency-labs/smlib#31).
//!
//! These tests go through the production `Surface::assert_valid`, not the arithmetic of
//! the tolerance: the default path must still report test 8, the opt-in path must not,
//! and an origin genuinely off the generator must fail either way. The full override is
//! already upstream; the off/on comparison isolates the #1508 opt-in branch. The
//! issue-scale cases intentionally add a 5e-12 out-of-plane residual to model the
//! solve's reported residual; it is not an exact
//! reproduction of the original valid-text-outline/C++ trigger.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::curve::curve::Curve;
use solidcore::geom::assert_array::{AssertArray, ObjectKey};
use solidcore::geom::extent2d::Extent2d;
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::crv_basic::Curve as NurbCurve;
use solidcore::nurbs::math::CPoint;
use solidcore::surface::surf_of_extrusion::SurfOfExtrusion;
use solidcore::surface::surface::assert_valid::LEVEL_GIVEN;
use solidcore::surface::surface::{Surface, SurfaceKind};
use solidcore::{Kernel, Vec3};

/// A planar quadratic Bezier `(0, 0) - (1, 1) - (2, 0)` translated by `offset` in x and
/// y (the text-outline shape of the issue).
fn quadratic_bezier(offset: f64) -> Curve {
    let pw = vec![
        CPoint::from_point(Vec3::new(offset, offset, 0.0)),
        CPoint::from_point(Vec3::new(offset + 1.0, offset + 1.0, 0.0)),
        CPoint::from_point(Vec3::new(offset + 2.0, offset, 0.0)),
    ];
    let nurb = NurbCurve::new(pw, 2, vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0]).unwrap();
    Curve::from(BSplineCurve::from_nurb(nurb, 3, false))
}

/// The extrusion of [`quadratic_bezier`] at `offset` along z, with its origin at the
/// curve point 45.678% along its interval plus `origin_delta` (the solve's residual).
fn extrusion(k: &Kernel, offset: f64, origin_delta: Vec3) -> Surface {
    let gen = quadratic_bezier(offset);
    let origin = gen.evaluate_point(k, 0.45678).unwrap() + origin_delta;
    Surface::from(
        SurfOfExtrusion::new(
            k,
            gen,
            origin,
            Vec3::new(0.0, 0.0, 10.0),
            Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0),
            false,
        )
        .unwrap(),
    )
}

/// `Surface::assert_valid` at `level`: whether every test passed and the failed test
/// numbers.
fn check(k: &Kernel, s: &Surface, level: u8) -> (bool, Vec<u64>) {
    let mut list = AssertArray::new();
    let ok = s
        .assert_valid(k, Some(&mut list), level, ObjectKey(1))
        .unwrap();
    let failed = list
        .reports()
        .iter()
        .filter(|r| !r.ok)
        .map(|r| r.test_index)
        .collect();
    (ok, failed)
}

fn set_fix(k: &mut Kernel, on: bool) {
    k.fixes.extrusion_origin_scaled_zero = on;
}

#[test]
fn ordinary_origin_is_valid_with_and_without_the_fix() {
    let mut k = Kernel::default();
    let s = extrusion(&k, 0.0, Vec3::ZERO);
    for on in [false, true] {
        set_fix(&mut k, on);
        assert_eq!(check(&k, &s, 0), (true, vec![]), "fix {on}");
    }
}

#[test]
fn issue_scale_origin_at_the_solver_residual_needs_the_scaled_zero() {
    // The issue reports the C++ solve resolving an origin evaluated on this quadratic to
    // a few 1e-12 at coordinates of about 10 (#1508). This intentionally adds a 5e-12
    // out-of-plane residual to model that; it is not the exact original trigger, but the
    // unscaled SM_EFF_ZERO rejects it and the scaled zero accepts it.
    let mut k = Kernel::default();
    let s = extrusion(&k, 10.0, Vec3::new(0.0, 0.0, 5.0e-12));

    set_fix(&mut k, false);
    let default = check(&k, &s, 0);
    assert_eq!(default, (false, vec![8]), "default");

    set_fix(&mut k, true);
    let fixed = check(&k, &s, 0);
    assert_eq!(fixed, (true, vec![]), "fix");
    eprintln!("issue_1508 origin residual: default {default:?}, fix {fixed:?}");
}

#[test]
fn translated_quadratic_origin_is_valid_with_the_fix() {
    // An origin evaluated exactly on the curve, far from the origin of coordinates:
    // the solve's absolute residual can exceed the unscaled zero depending on the
    // platform's rounding. The default may therefore pass, but it must not report
    // any other test; the scaled zero must accept it.
    let mut k = Kernel::default();
    let s = extrusion(&k, 1.0e8, Vec3::ZERO);

    set_fix(&mut k, false);
    let (_, default_failed) = check(&k, &s, 0);
    assert!(
        default_failed.iter().all(|&test| test == 8),
        "default reports only test 8: {default_failed:?}"
    );

    set_fix(&mut k, true);
    let fixed = check(&k, &s, 0);
    assert_eq!(fixed, (true, vec![]), "fix");
    eprintln!("issue_1508 translated origin: default {default_failed:?}, fix {fixed:?}");
}

#[test]
fn an_origin_off_the_generator_still_fails_with_the_fix() {
    let mut k = Kernel::default();
    let s = extrusion(&k, 10.0, Vec3::new(0.0, 0.0, 1.0e-6));
    for on in [false, true] {
        set_fix(&mut k, on);
        assert_eq!(check(&k, &s, 0), (false, vec![8]), "fix {on}");
    }
}

#[test]
fn a_made_nurb_extrusion_passes_at_every_level() {
    // The complete override: with a NURB the base checks, tests 5/6 (STEP <-> NURB) and
    // test 25 (generator parameterization) run too.
    let mut k = Kernel::default();
    for offset in [0.0, 10.0] {
        let ext = SurfOfExtrusion::create_canonical(&k, quadratic_bezier(offset), Vec3::Z).unwrap();
        let s = Surface::from(ext);
        for on in [false, true] {
            set_fix(&mut k, on);
            for level in [0, 1, 2, LEVEL_GIVEN] {
                assert_eq!(
                    check(&k, &s, level),
                    (true, vec![]),
                    "offset {offset} fix {on} level {level}"
                );
            }
        }
    }
}

#[test]
fn made_nurb_issue_scale_residual_needs_the_scaled_zero() {
    // Exercise the same synthetic residual with a real NURB, as a swept extrusion
    // has, so its base, STEP/NURB and generator-parameterization checks also run.
    let mut k = Kernel::default();
    let mut s = extrusion(&k, 10.0, Vec3::new(0.0, 0.0, 5.0e-12));
    let SurfaceKind::SurfOfExtrusion(ext) = s.kind_mut() else {
        panic!("expected an extrusion");
    };
    ext.make_nurb(&k).unwrap();
    assert!(ext.base().gw_nurb().is_some());

    for level in [0, 1, 2, LEVEL_GIVEN] {
        set_fix(&mut k, false);
        let default = check(&k, &s, level);
        assert_eq!(default, (false, vec![8]), "default level {level}");

        set_fix(&mut k, true);
        let fixed = check(&k, &s, level);
        assert_eq!(fixed, (true, vec![]), "fix level {level}");
        eprintln!(
            "issue_1508 made-NURB residual level {level}: default {default:?}, fix {fixed:?}"
        );
    }
}
