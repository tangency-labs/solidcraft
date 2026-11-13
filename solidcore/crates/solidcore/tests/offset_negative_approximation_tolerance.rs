//! The offset at the edge of what the model resolves. The nightly fuzz of
//! `offset.offset_executive_p2`: fuzz00175 (a member distance of `1e-300`, which both
//! kernels spent tens of seconds subdividing toward, HANG-53) is refused before anything
//! is built, naming the distance and the bound. fuzz00087: refused by its distance; with
//! a resolvable one, its approximation tolerance -1e-300 is refused too (WR-749: its
//! magnitude would be the temporary B-rep's tolerance, below the effective zero); and
//! fuzz00035 / fuzz00108 / fuzz00117: `SetTolerance` turns that magnitude, when zero,
//! into `SM_ZONE_TOL_3D` times the empty B-rep's box size (1).
#![allow(clippy::unwrap_used, clippy::panic)]

#[path = "support/offset_fixture.rs"]
mod offset_fixture;

use offset_fixture::Notifications;
use solidcore::error::offset::OffsetFailure;
use solidcore::error::{ErrorKind, Reason};
use solidcore::math::Point3;
use solidcore::offset::offset_executive::p1::{OffsetExecutive, OffsetOperation};
use solidcore::offset::offset_executive::p2::SolidOffsetOptions;
use solidcore::primitive::api_primitives::create_cylinder;
use solidcore::{Brep, Kernel, Result, SolidError};

/// The solid offset (extending convex edges, so through `do_extended_offset`, or
/// rounded) of `original` by `distance` with approximation tolerance `approx`.
fn offset(
    kernel: &mut Kernel,
    original: &mut Brep,
    distance: f64,
    approx: f64,
    extend: bool,
) -> Result<Brep> {
    let mut notifications = Notifications::new(original);
    let mut executive = OffsetExecutive::new(OffsetOperation::SolidOffset, distance, approx);
    executive.set_extend_convex_edges(extend);
    executive.set_merge_results(false);
    executive.do_solid_offset(
        kernel,
        Some(original),
        SolidOffsetOptions::default(),
        &mut notifications.context(),
    )
}

fn refusal(error: &SolidError) -> OffsetFailure {
    assert_eq!(error.kind(), ErrorKind::InvalidInput, "{error}");
    let SolidError::Failed(failure) = error else {
        panic!("not a refusal: {error}");
    };
    let Reason::Offset(reason) = &failure.reason else {
        panic!("not an offset refusal: {error}");
    };
    *reason
}

#[test]
fn a_member_distance_below_the_brep_tolerance_is_refused_naming_it() {
    // fuzz00175: `mate_two_curves_problemShell15_face8_outset` with a member distance
    // of 1e-300 (the C++ took 21 s, Rust 31.5 s); here on a cylinder, both paths.
    for extend in [true, false] {
        let mut kernel = Kernel::default();
        let mut original =
            create_cylinder(&mut kernel, Point3::new(1.0, -2.0, 0.5), 3.0, 5.0).unwrap();
        let tolerance = original.tolerance();
        let faces = original.face_list().len();
        let error = offset(&mut kernel, &mut original, 1e-300, tolerance, extend).unwrap_err();
        assert_eq!(
            refusal(&error),
            OffsetFailure::DistanceUnresolved {
                distance: 1e-300,
                resolution: tolerance,
            }
        );
        let message = error.to_string();
        assert!(message.contains("1e-300"), "{message}");
        assert!(message.contains(&format!("{tolerance:e}")), "{message}");
        // refused before anything is built: the input is as it was
        assert_eq!(original.face_list().len(), faces);
    }
}

#[test]
fn a_member_distance_just_above_the_brep_tolerance_builds() {
    for extend in [true, false] {
        let mut kernel = Kernel::default();
        let mut original =
            create_cylinder(&mut kernel, Point3::new(1.0, -2.0, 0.5), 3.0, 5.0).unwrap();
        let tolerance = original.tolerance();
        let distance = tolerance * (1.0 + f64::EPSILON);
        let result = offset(&mut kernel, &mut original, distance, tolerance, extend).unwrap();
        assert!(!result.face_list().is_empty(), "extend={extend}");
    }
}

#[test]
fn fuzz00087_is_refused_by_its_distance_then_by_its_approximation_tolerance() {
    let mut kernel = Kernel::default();
    let mut original = create_cylinder(&mut kernel, Point3::new(1.0, -2.0, 0.5), 3.0, 5.0).unwrap();
    // fuzz00087 as found (a B-rep tolerance of 1e300) offsets by less than the tolerance
    original.set_tolerance(1e300);
    let error = offset(&mut kernel, &mut original, 0.5, -1e-300, true).unwrap_err();
    assert!(matches!(
        refusal(&error),
        OffsetFailure::DistanceUnresolved { distance, resolution }
            if distance == 0.5 && resolution == 1e300
    ));
    // with a resolvable distance, the approximation tolerance's magnitude, 1e-300, would
    // be the temporary B-rep's tolerance: one no B-rep can hold (WR-749), refused by name
    original.set_tolerance(0.25);
    let mut notifications = Notifications::new(&mut original);
    let mut executive = OffsetExecutive::new(OffsetOperation::SolidOffset, 0.5, -1e-300);
    executive.set_inset(true);
    executive.set_extend_convex_edges(true);
    executive.set_merge_results(false);
    let error = executive
        .do_solid_offset(
            &mut kernel,
            Some(&mut original),
            SolidOffsetOptions::default(),
            &mut notifications.context(),
        )
        .unwrap_err();
    assert_eq!(
        refusal(&error),
        OffsetFailure::ToleranceUnresolved { tolerance: -1e-300 }
    );
    assert_eq!(executive.approximation_tol_3d(), -1e-300);
}

#[test]
fn zero_approximation_tolerance_gives_the_zone_tolerance() {
    for approx in [0.0, -0.0] {
        let mut kernel = Kernel::default();
        let mut original =
            create_cylinder(&mut kernel, Point3::new(1.0, -2.0, 0.5), 3.0, 5.0).unwrap();
        let mut notifications = Notifications::new(&mut original);
        let mut executive = OffsetExecutive::new(OffsetOperation::SolidOffset, 0.5, approx);
        executive.set_extend_convex_edges(true);
        let result = executive
            .do_solid_offset(
                &mut kernel,
                Some(&mut original),
                SolidOffsetOptions::default(),
                &mut notifications.context(),
            )
            .unwrap();
        assert_eq!(result.tolerance(), 1e-5, "{approx:?}");
        assert_eq!(executive.approximation_tol_3d().to_bits(), approx.to_bits());
    }
}
