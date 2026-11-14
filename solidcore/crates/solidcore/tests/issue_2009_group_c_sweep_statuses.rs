//! #2009 group C: statuses the sweeps, extrusions and their topology sweep drop in SMLib
//! and the SolidCraft fork passes on (tangency-labs/smlib c8f6a31, WR-662), as the port now
//! always does (#14246 batch S5 retired `UpstreamFixes::sweep_statuses_pass_on`). Each test
//! fails one callee and checks that its status reaches the caller.

#![allow(clippy::expect_used, clippy::indexing_slicing)] // test helpers

use solidcore::curve::curve::Curve;
use solidcore::math::{Point3, Vec3};
use solidcore::primitive::api_curves;
use solidcore::primitive::api_primitives::create_box;
use solidcore::sweep::api_brep::{create_linear_sweep, non_manifold_sweep, SweepSelection};
use solidcore::Kernel;

/// fork c8f6a31 (`SmPrimitiveCreation::CreateLinearSweep`,
/// `SER(pBrep->CreatePlanarFacesWith3DCurves(..))`): a circle and a separate line bound
/// no planar region, so the caps cannot be made. Upstream sweeps the two walls without
/// caps and succeeds; the fork, and so the port, returns the cap's error.
#[test]
fn linear_sweep_passes_on_failed_caps() {
    let circle =
        Curve::from(api_curves::create_circle(Point3::new(0.0, 0.0, 0.0), 2.0).expect("circle"));
    let line = Curve::from(
        api_curves::create_line_segment(Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0))
            .expect("line"),
    );
    let mut k = Kernel::default();
    let r = create_linear_sweep(
        &mut k,
        &[Some(&circle), Some(&line)],
        Vec3::new(0.0, 0.0, 1.0),
        10.0,
        true,
    );
    assert!(r.is_err(), "{r:?}");
}

/// fork c8f6a31 (`SmTopologySweep::DoSweep`, `SER(pBrepToSweep->CalculateBoundingBox(sBBox))`):
/// a face without surface has no box. Upstream places the sweep frame at an empty box's
/// middle and goes on; the fork returns the box's error.
#[test]
fn whole_brep_sweep_passes_on_a_failed_box() {
    let mut k = Kernel::default();
    let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 1.0, 2.0, 3.0).expect("box");
    let f = b.face_list()[0];
    b.face_mut(f).expect("face").surface = None;
    let r = non_manifold_sweep(
        &mut k,
        &mut b,
        None,
        Vec3::new(0.0, 0.0, 1.0),
        5.0,
        true,
        false,
        SweepSelection::default(),
    );
    assert!(
        matches!(
            &r,
            Err(solidcore::SolidError::InvalidInput { operation, .. })
                if operation.contains("bounding_box")
        ),
        "{r:?}"
    );
}
