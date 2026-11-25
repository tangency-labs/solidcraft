//! #2393, nightly fuzz00167 of `offset.offset_executive_p2`: the extended offset of a
//! sphere (centre (0.5, 1, -1), radius 2.5, B-rep tolerance 1e-6) outward by 0.4 with the
//! member approximation tolerance -1e-300.
//!
//! Upstream sets that tolerance's magnitude, 1e-300, as the offset B-rep's tolerance
//! (WR-749). On this sphere the face build then fails, `BuildOffsetOfFaceuse` ignores the
//! failure and the extension loop's `ReplaceSurface` dereferences the null owner face of
//! the half-built face: SIGSEGV (UB-291). The port used to build the sphere, but at
//! tolerance 1e-300 the body fails its own validity check (`ValidatePointers` test 0,
//! "Tolerance less than effective zero", and edge/vertex gaps of 7.7e-15 above it). It
//! now refuses a nonzero approximation tolerance at or below `SM_EFF_ZERO`, naming it.
//!
//! The same offset with the fixture's ordinary tolerance (1e-6, `extended_sphere_outset`)
//! is graded by the closed form: the offset of a sphere of radius `r` by `d` is the
//! concentric sphere of radius `r + d`, so a manifold solid of volume `4/3 pi (r + d)^3`
//! whose tolerance a B-rep can hold.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

#[path = "support/offset_fixture.rs"]
mod offset_fixture;

use std::f64::consts::PI;

use solidcore::error::offset::OffsetFailure;
use solidcore::error::{ErrorKind, Reason};
use solidcore::math::{Point3, EFF_ZERO};
use solidcore::offset::offset_executive::p1::{OffsetExecutive, OffsetOperation};
use solidcore::primitive::api_primitives::create_sphere;
use solidcore::query::api_queries::brep_is_manifold_solid;
use solidcore::{Brep, Kernel, SolidError};

const CENTRE: [f64; 3] = [0.5, 1.0, -1.0];
const RADIUS: f64 = 2.5;
const DISTANCE: f64 = 0.4;

/// `SmOffsetExecutive::DoExtendedOffset` of the sphere as the difftest case runs it:
/// solid offset, extended convex edges, offset solid created, no merge.
fn extended_offset(k: &mut Kernel, approx_tol: f64) -> solidcore::Result<Brep> {
    let [x, y, z] = CENTRE;
    let mut sphere = create_sphere(k, Point3::new(x, y, z), RADIUS).unwrap();
    sphere.set_tolerance(1e-6);
    let mut notifications = offset_fixture::Notifications::new(&mut sphere);
    let mut executive = OffsetExecutive::new(OffsetOperation::SolidOffset, DISTANCE, approx_tol);
    executive.set_extend_convex_edges(true);
    executive.set_create_offset_solid(true);
    executive.do_extended_offset(k, Some(&mut sphere), &mut notifications.context())
}

/// fuzz00167's tolerance and its neighbours at and below the effective zero are refused
/// by name, before any geometry is built.
#[test]
fn an_effectively_zero_approx_tol_is_refused_naming_it() {
    for approx_tol in [-1e-300, 1e-300, -EFF_ZERO, EFF_ZERO, 1e-14] {
        let mut k = Kernel::default();
        let error = extended_offset(&mut k, approx_tol).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::InvalidInput, "{error}");
        let SolidError::Failed(failure) = &error else {
            panic!("{approx_tol:e}: not a refusal: {error}");
        };
        let Reason::Offset(reason) = &failure.reason else {
            panic!("{approx_tol:e}: not an offset refusal: {error}");
        };
        assert_eq!(
            *reason,
            OffsetFailure::ToleranceUnresolved {
                tolerance: approx_tol
            }
        );
        let message = error.to_string();
        assert!(message.contains(&format!("{approx_tol:e}")), "{message}");
    }
}

/// The ordinary tolerance builds the concentric sphere of radius 2.9.
#[test]
fn the_fixture_tolerance_offsets_a_sphere_to_the_concentric_sphere() {
    let mut k = Kernel::default();
    let mut body = extended_offset(&mut k, 1e-6).expect("the extended offset builds");
    body.refresh_cached_topology();
    assert!(brep_is_manifold_solid(&body).unwrap());
    let volume = offset_fixture::volume(&k, &mut body).unwrap();
    let want = 4.0 / 3.0 * PI * (RADIUS + DISTANCE).powi(3);
    assert!(
        (volume - want).abs() <= 1e-6 * want,
        "volume {volume} vs closed form {want}"
    );
    assert!(body.tolerance() > EFF_ZERO);
}
