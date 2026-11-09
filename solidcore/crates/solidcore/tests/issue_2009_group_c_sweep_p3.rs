//! #2009 group C (fork c8f6a31): the curve sweep, revolve and offset-ruled-surface
//! statuses SMLib drops and the SolidCraft fork passes on, behind
//! `UpstreamFixes::sweep_statuses_pass_on` (WR-662). No valid input fails these calls,
//! so each test injects the callee's failure (a curve without NURB data, a path
//! without tangent) and compares the flag on with the flag off.

#![allow(clippy::expect_used, clippy::indexing_slicing)] // test helpers

use solidcore::curve::curve::Curve;
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent1d::Extent1d;
use solidcore::math::Vec3;
use solidcore::nurbs::b_spline_curve::p1::BSplineCurve;
use solidcore::primitive::primitive_creation::p3::{
    prepare_curve_sweep, ruled_surface_from_offset, OffsetRuleOptions,
};
use solidcore::primitive::primitive_creation::SweepOptions;
use solidcore::{Kernel, SolidError};

fn kernel(pass_on: bool) -> Kernel {
    let mut k = Kernel::default();
    k.fixes.sweep_statuses_pass_on = pass_on;
    k
}

fn operation(r: &Result<impl std::fmt::Debug, SolidError>) -> String {
    format!("{r:?}")
}

fn line(a: Vec3, b: Vec3) -> BSplineCurve {
    BSplineCurve::create_line_segment(3, a, b).expect("line")
}

/// fork c8f6a31 (`CreateCurveSweep`, `SER(sNurbs[ii]->SubdivideAtDiscontinuity(..))`):
/// a profile without NURB data cannot be split. Upstream keeps it whole and goes on;
/// the fork returns the split's error.
#[test]
fn curve_sweep_passes_on_a_failed_profile_split() {
    let path = line(Vec3::ZERO, Vec3::Z);
    let profiles = [Some(Curve::from(BSplineCurve::new(3)))];
    let run = |pass_on| {
        prepare_curve_sweep(
            &kernel(pass_on),
            &profiles,
            &path,
            None,
            None,
            SweepOptions {
                // no reference-point evaluation: the split is the first failure
                profile_at_path_start: true,
                ..SweepOptions::default()
            },
            true,
        )
        .map(|_| ())
    };
    let (off, on) = (run(false), run(true));
    eprintln!("off {} / on {}", operation(&off), operation(&on));
    // SubdivideAtDiscontinuity fails building the missing NURB.
    assert!(operation(&on).contains("make_nurb"), "{on:?}");
    assert!(off.is_ok(), "{off:?}");
}

/// fork c8f6a31 (`CreateCurveSweep`, `SER(sNurbs[0]->EvaluatePoint(.., sProfileRefPt))`):
/// a profile without NURB data has no start point. Upstream keeps the origin as the
/// reference point; the fork returns the evaluation's error.
#[test]
fn curve_sweep_passes_on_a_failed_profile_reference_point() {
    let path = line(Vec3::ZERO, Vec3::Z);
    let profiles = [Some(Curve::from(BSplineCurve::new(3)))];
    let run = |pass_on| {
        prepare_curve_sweep(
            &kernel(pass_on),
            &profiles,
            &path,
            None,
            None,
            SweepOptions::default(),
            false,
        )
        .map(|_| ())
    };
    let (off, on) = (run(false), run(true));
    eprintln!("off {} / on {}", operation(&off), operation(&on));
    // EvaluatePoint fails building the missing NURB (before the profile's transform,
    // which would fail in `BSplineCurve::transform`).
    assert!(operation(&on).contains("make_nurb"), "{on:?}");
    assert!(off.is_ok(), "{off:?}");
}

/// fork c8f6a31 (`CreateCurveSweep`, `SER(sPathPtAndDir[1].MakeUnitOrthoVectors(..))`):
/// a path without start tangent (a point curve) has no frame. Upstream turns the path
/// by a frame with an unset y axis; the fork returns the frame's error.
#[test]
fn curve_sweep_passes_on_a_failed_path_frame() {
    let path = BSplineCurve::create_point_curve(Vec3::ZERO).expect("point curve");
    let profiles = [Some(Curve::from(line(Vec3::ZERO, Vec3::X)))];
    let run = |pass_on| {
        prepare_curve_sweep(
            &kernel(pass_on),
            &profiles,
            &path,
            None,
            None,
            SweepOptions::default(),
            false,
        )
        .map(|_| ())
    };
    let (off, on) = (run(false), run(true));
    eprintln!("off {} / on {}", operation(&off), operation(&on));
    // MakeUnitOrthoVectors fails unitizing the zero tangent.
    assert!(operation(&on).contains("Vec3::unitize"), "{on:?}");
    assert!(off.is_ok(), "{off:?}");
}

/// fork c8f6a31 (`sm_CreateRuledSurfaceFromOffsetCurve`,
/// `SER(pBSC->DegreeReduction(dBrepTol, TRUE))`): an offset curve without NURB data
/// cannot be reduced. Upstream goes on (and fails later without an offset map); the
/// fork returns the reduction's error.
#[test]
fn ruled_surface_passes_on_a_failed_degree_reduction() {
    let run = |pass_on| {
        let mut offset = BSplineCurve::new(3);
        let mut generators = [line(Vec3::ZERO, Vec3::X)];
        let mut unmarried = [true];
        let mut missing = false;
        ruled_surface_from_offset(
            &kernel(pass_on),
            &mut offset,
            None,
            &mut generators,
            [Extent1d::from_min_max(0.0, 1.0); 2],
            false,
            &mut unmarried,
            &mut missing,
            OffsetRuleOptions {
                tolerance: 1e-5,
                same_point_tolerance: 1e-10,
                offset: 1.0,
                movement: Axis2Placement::IDENTITY,
                surface_intersection: false,
            },
        )
        .map(|_| ())
    };
    let (off, on) = (run(false), run(true));
    eprintln!("off {} / on {}", operation(&off), operation(&on));
    assert!(operation(&on).contains("degree_reduction"), "{on:?}");
    assert!(
        operation(&off).contains("ruled_surface_from_offset"),
        "{off:?}"
    );
}

/// fork c8f6a31 (`CreateRotationalSweep`, `SER(((SmBSplineCurve*)r3DCurves[i])->SubdivideAtDiscontinuity(..))`):
/// a profile without NURB data cannot be split. Upstream keeps it whole and goes on to
/// reject it as degenerate; the fork returns the split's error.
#[test]
fn rotational_sweep_passes_on_a_failed_profile_split() {
    let curve = Curve::from(BSplineCurve::new(3));
    let run = |pass_on| {
        let mut k = kernel(pass_on);
        solidcore::sweep::api_brep::create_rotational_sweep(
            &mut k,
            &[Some(&curve)],
            Vec3::ZERO,
            Vec3::Z,
            360.0,
            false,
        )
        .map(|_| ())
    };
    let (off, on) = (run(false), run(true));
    eprintln!("off {} / on {}", operation(&off), operation(&on));
    assert!(operation(&on).contains("make_nurb"), "{on:?}");
    assert!(
        operation(&off).contains("degenerate revolved curve"),
        "{off:?}"
    );
}
