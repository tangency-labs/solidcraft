//! tangency-labs/solidcore#1672 (SolidCraft rig sweeps/4007): the outer half of a disc
//! of radius 14 centred 14 from the z axis, revolved 270 degrees about -z with
//! `fixes.revolve_analytic_profile_faces`, has an `SmTorus` face that `orient_body`
//! swaps (`SwapUV`), which rebuilds the torus from its NURB.
//!
//! - Upstream's rebuild mirrors the tube interval -90..90 to 90..450 (WR-408), so the
//!   torus fails `SmSurfOfRevolution` test 2 and `SmTorus` test 2. The fix is
//!   `fixes.torus_signed_gen_curve_interval`.
//! - The rebuilt major radius is 14 less a few ulps, so the horn torus is a spindle by
//!   rounding. Upstream's check then clears its result with no report (WR-539). The
//!   fix is `fixes.torus_check_spindle_apex`: the apexes at V = +-(90 + asin(R/r)) lie
//!   outside -90..90.
//!
//! About +z nothing is swapped and the body is valid either way.

#![allow(clippy::unwrap_used)]

use solidcore::curve::curve::Curve;
use solidcore::curve::ellipse::Ellipse;
use solidcore::curve::line::Line;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent1d::Extent1d;
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::PI;
use solidcore::sweep::api_brep::create_rotational_sweep;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::{Brep, Kernel, Vec3};

/// Pappus: the half disc away from the axis (area pi 14^2 / 2, centroid
/// 14 + 4 * 14 / (3 pi) from the axis) over three quarters of a turn.
const EXPECTED_VOLUME: f64 = 28_932.176_098_892_29;

/// The kernel SolidCraft runs: the analytic profile faces and the torus fixes.
fn fixed_kernel() -> Kernel {
    let mut k = Kernel::default();
    k.fixes.revolve_analytic_profile_faces = true;
    k.fixes.torus_signed_gen_curve_interval = true;
    k.fixes.torus_check_spindle_apex = true;
    k.fixes.torus_check_reads_v_interval = true;
    k
}

/// The half disc revolved 270 degrees about `axis` through the origin, capped.
fn revolve(k: &mut Kernel, axis: Vec3) -> Brep {
    // the arc from (14, 0, -14) through (28, 0, 0) to (14, 0, 14)
    let frame = Axis2Placement::new(
        Vec3::new(14.0, 0.0, 0.0),
        Vec3::new(0.0, 0.0, -1.0),
        Vec3::new(1.0, 0.0, 0.0),
    );
    let arc = Curve::from(
        Ellipse::create_canonical_circle(
            k,
            &frame,
            14.0,
            Some(&Extent1d::from_min_max(0.0, 180.0)),
            None,
        )
        .unwrap(),
    );
    let line = Curve::from(
        Line::create_line_segment(
            3,
            Vec3::new(14.0, 0.0, 14.0),
            Vec3::new(14.0, 0.0, -14.0),
            None,
        )
        .unwrap(),
    );
    create_rotational_sweep(
        k,
        &[Some(&arc), Some(&line)],
        Vec3::new(0.0, 0.0, 0.0),
        axis,
        270.0,
        true,
    )
    .unwrap()
}

/// `check_brep` at level 2: `AssertValid`'s result and the failed reports as
/// `(class, test)`.
fn check(k: &mut Kernel, b: &mut Brep) -> (Option<bool>, Vec<(i64, u64)>) {
    let c = check_brep(k, b, CheckLevel::Level2).unwrap();
    let failed = c
        .reports
        .reports()
        .iter()
        .filter(|r| !r.ok)
        .map(|r| (r.reporting_type, r.test_index))
        .collect();
    (c.assert_valid, failed)
}

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-8, &attrs).unwrap()
}

fn assert_valid_with_volume(k: &mut Kernel, axis: Vec3) {
    let mut b = revolve(k, axis);
    assert_eq!(check(k, &mut b), (Some(true), vec![]), "axis {axis:?}");
    let v = volume(k, &mut b);
    assert!(
        (v - EXPECTED_VOLUME).abs() <= 1e-9 * EXPECTED_VOLUME,
        "axis {axis:?}: volume {v}, expected {EXPECTED_VOLUME}"
    );
}

#[test]
fn closed_form_is_pappus() {
    let r: f64 = 14.0;
    let area = PI * r * r / 2.0;
    let centroid = r + 4.0 * r / (3.0 * PI);
    let v = 0.75 * 2.0 * PI * centroid * area;
    assert!((v - EXPECTED_VOLUME).abs() <= 1e-12 * EXPECTED_VOLUME);
}

#[test]
fn issue_1672_revolve_about_minus_z_is_valid_with_the_fixes() {
    assert_valid_with_volume(&mut fixed_kernel(), Vec3::new(0.0, 0.0, -1.0));
}

#[test]
fn issue_1672_revolve_about_plus_z_is_valid_with_the_fixes() {
    assert_valid_with_volume(&mut fixed_kernel(), Vec3::new(0.0, 0.0, 1.0));
}

#[test]
fn issue_1672_revolve_without_fixes_is_valid() {
    for axis in [Vec3::new(0.0, 0.0, -1.0), Vec3::new(0.0, 0.0, 1.0)] {
        assert_valid_with_volume(&mut Kernel::default(), axis);
    }
}

/// Upstream's torus fixes off: the rebuilt torus is the inner half of the tube
/// (WR-408), and its two parameterisation tests fail.
#[test]
fn issue_1672_upstream_torus_rebuild_fails_test_2() {
    let mut k = Kernel::default();
    k.fixes.revolve_analytic_profile_faces = true;
    let mut b = revolve(&mut k, Vec3::new(0.0, 0.0, -1.0));
    assert_eq!(
        check(&mut k, &mut b),
        (Some(false), vec![(15020, 2), (15015, 2)])
    );
}

/// The signed interval without the spindle-apex check: upstream's spindle test fails
/// the rounded horn torus with no report (WR-539).
#[test]
fn issue_1672_upstream_spindle_check_fails_without_a_report() {
    let mut k = fixed_kernel();
    k.fixes.torus_check_spindle_apex = false;
    k.fixes.torus_check_reads_v_interval = false;
    let mut b = revolve(&mut k, Vec3::new(0.0, 0.0, -1.0));
    assert_eq!(check(&mut k, &mut b), (Some(false), vec![]));
}
