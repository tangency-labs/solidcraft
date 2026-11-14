//! #2009 group A: statuses SMLib drops and the SolidCraft fork passes on, behind
//! `UpstreamFixes::brep_edit_statuses_pass_on` (WR-658). No input fails these calls on
//! valid geometry, so each test injects the callee's failure and checks that the
//! status reaches the caller with the flag on and is dropped with it off.

#![allow(clippy::expect_used, clippy::indexing_slicing)] // test helpers

use solidcore::math::Point3;
use solidcore::primitive::api_primitives::create_box;
use solidcore::topology::brep::mirror::transform_matrix;
use solidcore::topology::Brep;
use solidcore::{Kernel, SolidError};

fn kernel(pass_on: bool) -> Kernel {
    let mut k = Kernel::default();
    k.fixes.brep_edit_statuses_pass_on = pass_on;
    k
}

fn unit_box(k: &mut Kernel) -> Brep {
    create_box(k, Point3::new(0.0, 0.0, 0.0), 1.0, 2.0, 3.0).expect("box")
}

/// fork 60a8a51 (`SmBrep::CalculateTightBoundingBox`, `SER(sFaceList[ii]->CalculateBoundingBox(..))`):
/// a face without surface has no box. Upstream unions the boxes it has; the fork
/// returns the face's error.
#[test]
fn tight_box_passes_on_a_failed_face_box() {
    for pass_on in [false, true] {
        let mut k = kernel(pass_on);
        let mut b = unit_box(&mut k);
        let f = b.face_list()[0];
        b.face_mut(f).expect("face").surface = None;
        let r = solidcore::query::brep::calculate_tight_bounding_box(&k, &mut b);
        if pass_on {
            assert!(
                matches!(
                    r,
                    Err(SolidError::InvalidInput {
                        operation: "query::face::p1::calculate_bounding_box",
                        ..
                    })
                ),
                "{r:?}"
            );
        } else {
            let bbox = r.expect("dropped status");
            assert_eq!(bbox.max(), Point3::new(1.0, 2.0, 3.0));
        }
    }
}

/// fork 60a8a51 (`SmBrep::Transform` of a matrix, `SER((svRow0 * svRow1).AngleBetween(svRow2, ..))`):
/// a zero first row has no angle. Upstream reads the angle as 0 and goes on with a
/// rigid transform built from the zero row; the fork returns the angle's error.
#[test]
fn matrix_transform_passes_on_a_failed_angle() {
    let m = [
        [0.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    for pass_on in [false, true] {
        let mut k = kernel(pass_on);
        let mut b = unit_box(&mut k);
        let r = transform_matrix(&mut k, &mut b, &m, true);
        if pass_on {
            assert!(
                matches!(
                    r,
                    Err(SolidError::InvalidInput {
                        operation: "geom::vector3d::Vec3::angle_between",
                        ..
                    })
                ),
                "{r:?}"
            );
        } else {
            assert!(r.is_ok(), "{r:?}");
        }
    }
}

/// fork 4803d6d (`SmApiStitch`, `SER(pBrep->StitchAndOrient())`): an edge without
/// curve fails the stitch. Upstream's API drops `StitchAndOrient`'s status and
/// succeeds; the fork returns it.
#[test]
fn api_stitch_passes_on_a_failed_stitch_and_orient() {
    for pass_on in [false, true] {
        let mut k = kernel(pass_on);
        let mut b = unit_box(&mut k);
        let e = b.edge_list()[0];
        b.edge_mut(e).expect("edge").curve = None;
        let r = solidcore::heal::stitch::p1::api_stitch(&mut k, Some(&mut b));
        if pass_on {
            assert!(
                matches!(
                    r,
                    Err(SolidError::InvalidInput {
                        operation: "heal::stitch::p2",
                        ..
                    })
                ),
                "{r:?}"
            );
        } else {
            assert!(r.is_ok(), "{r:?}");
        }
    }
}

/// fork 4803d6d (`SmApiProjectBrepOntoPlane`, `SER(pBrep->CalculateBoundingBox(BBox))`):
/// a face without surface has no box. Upstream projects from an empty box; the fork
/// returns the box's error.
#[test]
fn projection_onto_plane_passes_on_a_failed_box() {
    for pass_on in [false, true] {
        let mut k = kernel(pass_on);
        let mut b = unit_box(&mut k);
        let f = b.face_list()[0];
        b.face_mut(f).expect("face").surface = None;
        let mut out = Vec::new();
        let r = solidcore::topology::api_projection::project_brep_onto_plane(
            &mut k,
            Some(&b),
            solidcore::math::Vec3::new(0.0, 0.0, -1.0),
            solidcore::math::Vec3::new(0.0, 0.0, 1.0),
            &mut out,
        );
        let box_error = matches!(
            r,
            Err(SolidError::InvalidInput {
                operation: "query::face::p1::calculate_bounding_box",
                ..
            })
        );
        assert_eq!(box_error, pass_on, "{r:?}");
    }
}

/// fork e7ebc8d (`SmStitch::UnifyNormals`, `SER(pBrepToOrient->CalculateBoundingBox(sBrepBBox))`):
/// a face without surface has no box. Upstream builds the edge tree from an empty box
/// and unifies the faces it reaches; the fork returns the box's error.
#[test]
fn unify_normals_passes_on_a_failed_box() {
    for pass_on in [false, true] {
        let mut k = kernel(pass_on);
        let mut b = unit_box(&mut k);
        let start = b.face_list()[0];
        let f = b.face_list()[1];
        b.face_mut(f).expect("face").surface = None;
        let r =
            solidcore::heal::stitch::p1::api_unify_normals(&mut k, Some(&mut b), Some(start), 4);
        if pass_on {
            assert!(
                matches!(
                    r,
                    Err(SolidError::InvalidInput {
                        operation: "query::face::p1::calculate_bounding_box",
                        ..
                    })
                ),
                "{r:?}"
            );
        } else {
            assert!(r.is_ok(), "{r:?}");
        }
    }
}
