//! Regression test for tangency-labs/solidcore#1511: a disc whose seam vertex lies on
//! the axis of a rotational sweep. Upstream's `SmTopologySweep::SweepEdgeHigh` read the
//! base edge's curve after `MakeFaceWithCurves` had let it go (the swept vertex is a
//! point and the curve's trim merged away) and crashed; fork tangency-labs/smlib 80d307d
//! checks the curve against the face's trims first. The port holds the face's curves by
//! index (`built.curve_edges`), so a curve the face creator dropped is a missing edge,
//! never a stale read: the sweep returns an error or a sound body, and never panics.
//!
//! The same quarter horn torus through the whole-body check gates WR-505 (#1515, the
//! fork's smlib#41, `Kernel::fixes.internal_pole_row_through_knot`) at operation level
//! (#1954): upstream's `HasInternalPole` takes the corner rows on the axis for an internal
//! pole and the check fails; the fork's finds none.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::curve::curve::Curve;
use solidcore::curve::ellipse::Ellipse;
use solidcore::geom::attribute::AttributeStore;
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent1d::Extent1d;
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::nurbs::b_spline_surface::BSplineSurface;
use solidcore::sweep::api_brep::create_rotational_sweep;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::{Kernel, Point3, Vec3};
use std::f64::consts::PI;

/// The fork's capi case: the circle of radius 5 about (0, 0, 20) in the plane z = 20,
/// from (5, 0, 20), revolved a quarter turn about the line through (5, 0, 20) along Y
/// (the circle's seam vertex on the axis), capped.
fn revolve_touching_disc(k: &mut Kernel) -> solidcore::Result<solidcore::Brep> {
    revolve_disc(k, 5.0)
}

/// The same circle revolved a quarter turn about the line along Y through
/// (`axis_x`, 0, 20): 5 is the horn torus above, more than 5 a ring torus.
fn revolve_disc(k: &mut Kernel, axis_x: f64) -> solidcore::Result<solidcore::Brep> {
    let frame = Axis2Placement::new(Point3::new(0.0, 0.0, 20.0), Vec3::X, Vec3::Y);
    let c = Curve::from(
        Ellipse::create_canonical_circle(
            k,
            &frame,
            5.0,
            Some(&Extent1d::from_min_max(0.0, 360.0)),
            None,
        )
        .unwrap(),
    );
    create_rotational_sweep(
        k,
        &[Some(&c)],
        Point3::new(axis_x, 0.0, 20.0),
        Vec3::Y,
        90.0,
        true,
    )
}

#[test]
fn issue_1511_disc_touching_the_axis_errs_or_builds_a_sound_body() {
    // a quarter of the horn torus: (pi / 2) * (pi 5^2) * 5 (Pappus)
    let want = PI / 2.0 * PI * 25.0 * 5.0;
    let mut k = Kernel::default();
    match revolve_touching_disc(&mut k) {
        Err(e) => eprintln!("no body: {e}"),
        Ok(mut b) => {
            // a sound body: intact topology and the quarter horn torus's volume (to a
            // coarse integration tolerance, which keeps the test cheap)
            solidcore::topology::integrity::check(&b).unwrap();
            let got = volume(&k, &mut b);
            eprintln!("body, volume {got} (horn torus quarter {want})");
            assert!(
                (got - want).abs() <= 1e-2 * want,
                "unsound body, volume {got}, want {want}"
            );
        }
    }
}

/// The body's volume, to a coarse integration tolerance (cheap, and far tighter than the
/// difference between the tori below).
fn volume(k: &Kernel, b: &mut solidcore::Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| solidcore::geom::attribute::ObjectKey(0),
    };
    brep_volume(k, b, 1e-3, &attrs).unwrap()
}

/// WR-505 (#1515) through a revolve and the whole-body check (#1954). The quarter horn
/// torus is built the same with the flag off or on (its volume is the closed form either
/// way); only the check's answer changes. Off (pinned upstream), `HasInternalPole` tests a
/// corner row at the circle's double knots, finds the rows on the axis beside the NURB's
/// boundary pole coincident and reports B-spline surface test 2, an internal pole; on (the
/// fork), it tests the row through the knot and the body is valid. Control: the same disc
/// revolved about an axis 8 from its centre (a ring torus, no pole) is valid either way.
#[test]
fn wr_505_quarter_horn_torus_passes_the_body_check_only_with_the_fix() {
    // Pappus: the disc's area times the quarter turn of its centre
    let horn = PI / 2.0 * PI * 25.0 * 5.0;
    let ring = PI / 2.0 * PI * 25.0 * 8.0;
    for fix in [false, true] {
        let mut k = Kernel::default();
        k.fixes.internal_pole_row_through_knot = fix;

        let mut b = revolve_disc(&mut k, 5.0).unwrap();
        assert_eq!(b.face_list().len(), 3, "fix {fix}");
        let got = volume(&k, &mut b);
        eprintln!("fix {fix}: quarter horn torus, volume {got} (closed form {horn})");
        assert!(
            (got - horn).abs() <= 1e-2 * horn,
            "fix {fix}: {got}, want {horn}"
        );
        let check = check_brep(&mut k, &mut b, CheckLevel::Level2).unwrap();
        assert!(check.pointer_reports.is_empty(), "fix {fix}: {check:?}");
        let failures: Vec<_> = check
            .failures()
            .map(|r| (r.reporting_type, r.test_index))
            .collect();
        if fix {
            assert!(check.is_valid(), "{check:?}");
            assert_eq!(check.assert_valid, Some(true));
            assert!(failures.is_empty(), "{failures:?}");
        } else {
            assert!(!check.is_valid(), "{check:?}");
            assert_eq!(check.assert_valid, Some(false));
            assert_eq!(failures, [(BSplineSurface::TYPE, 2)], "{check:?}");
        }

        let mut b = revolve_disc(&mut k, 8.0).unwrap();
        assert_eq!(b.face_list().len(), 3, "fix {fix}");
        let got = volume(&k, &mut b);
        eprintln!("fix {fix}: quarter ring torus, volume {got} (closed form {ring})");
        assert!(
            (got - ring).abs() <= 1e-2 * ring,
            "fix {fix}: {got}, want {ring}"
        );
        let check = check_brep(&mut k, &mut b, CheckLevel::Level2).unwrap();
        assert!(check.is_valid(), "fix {fix}: {check:?}");
        assert_eq!(check.failure_count(), 0, "fix {fix}");
    }
}
