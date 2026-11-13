//! Regression test for WR-342 (tangency-labs/solidcore#974, the fork's commit "an inset
//! face that does not fit its offset surface's domain is clipped right side out";
//! SolidCraft rig shell/4567): a vase whose profile meets the axis at a slant, shelled
//! 2 mm inward and open at the top.
//!
//! The vase is a full turn of the profile (0, 0) - (15, 5) - (15, 30) - (0, 30) in the
//! XY plane about the y axis: a conical bottom whose generator starts on the axis, a
//! cylindrical wall and a flat top. The bottom's inward offset generator crosses the
//! axis, so the offset surface is cut there (WR-343) and its domain no longer holds the
//! face's. `BuildOffsetOfFaceuse` then
//! clips the face to the offset domain with a 2D boolean of planar faces made from the
//! face's UV loops. Taken from the inset's opposite face use, upstream's loops run
//! clockwise, the planar face is inside out, the intersection is empty and the bottom
//! gets no offset: the offset is open, so the default shell refuses it and retains
//! the input. With
//! `offset_clip_right_side_out` the bottom is offset and the shell is closed, its
//! volume the vase's less the cavity bounded by the exact offsets (a cylinder r 13 and
//! the bottom cone moved 2 along its normal):
//! `pi R^2 (h/3 + H - h) - pi (R - 2)^2 ((H - y1) + (y1 - y0) / 3)`, with
//! `y0 = 2 / cos a` and `y1 = (h / R)(R - 2) + y0`, `tan a = h / R`.
//! The offsets are B-spline approximations, so the volume is held to 1e-5 relative.
//! The vase is revolved both ways round the axis (the rig's was about -y).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

#[path = "support/offset_fixture.rs"]
mod offset_fixture;

use offset_fixture::{planar_faces_at, shell, volume, Notifications};
use solidcore::curve::curve::Curve;
use solidcore::error::offset::{OffsetFailure, Unsoundness};
use solidcore::error::Reason;
use solidcore::math::{Point3, Vec3, PI};
use solidcore::offset::offset_executive::p1::OffsetExecutive;
use solidcore::offset::offset_executive::p2::{OffsetOptions, ShellOptions};
use solidcore::primitive::api_curves::create_line_segment;
use solidcore::query::api_queries::brep_is_manifold_solid;
use solidcore::sweep::api_brep::create_rotational_sweep;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::{Brep, Kernel, SolidError};

/// Wall radius, height of the conical bottom, height of the vase.
const R: f64 = 15.0;
const HB: f64 = 5.0;
const H: f64 = 30.0;

/// The vase revolved about +y (`up`) or -y.
fn vase(k: &mut Kernel, up: bool) -> Brep {
    let p = |x: f64, y: f64| Point3::new(x, y, 0.0);
    let profile = [
        (p(0.0, 0.0), p(R, HB)),
        (p(R, HB), p(R, H)),
        (p(R, H), p(0.0, H)),
        (p(0.0, H), p(0.0, 0.0)),
    ];
    let curves: Vec<Curve> = profile
        .iter()
        .map(|&(a, b)| Curve::from(create_line_segment(a, b).unwrap()))
        .collect();
    let curves: Vec<Option<&Curve>> = curves.iter().map(Some).collect();
    let (origin, axis) = if up {
        (p(0.0, 0.0), Vec3::new(0.0, 1.0, 0.0))
    } else {
        (p(0.0, H), Vec3::new(0.0, -1.0, 0.0))
    };
    create_rotational_sweep(k, &curves, origin, axis, 360.0, true).unwrap()
}

/// The vase shelled 2 inward, open at its top, with WR-342's `fixed`.
fn shelled(up: bool, fixed: bool) -> (Kernel, Brep) {
    let mut k = Kernel::default();
    k.fixes.offset_clip_right_side_out = fixed;
    let part = vase(&mut k, up);
    let open = planar_faces_at(&k, &part, 1, H);
    assert_eq!(open.len(), 1, "the top");
    let b = shell(&mut k, part, -2.0, &open).unwrap();
    (k, b)
}

fn closed_form() -> f64 {
    let a = (HB / R).atan();
    let y0 = 2.0 / a.cos();
    let y1 = HB / R * (R - 2.0) + y0;
    let vase = PI * R * R * (HB / 3.0 + H - HB);
    let cavity = PI * (R - 2.0).powi(2) * ((H - y1) + (y1 - y0) / 3.0);
    vase - cavity
}

#[test]
fn wr_342_vase_shells_closed_with_the_fix() {
    for up in [true, false] {
        let (k, mut b) = shelled(up, true);
        assert!(brep_is_manifold_solid(&b).unwrap(), "up {up}");
        let v = volume(&k, &mut b).unwrap();
        let exact = closed_form();
        assert!(
            ((v - exact) / exact).abs() < 1e-5,
            "up {up}: {v} against {exact}"
        );
    }
}

/// Without WR-342 the bottom gets no offset. The default shell refuses the open
/// result and retains the vase, for either rotational sweep direction.
#[test]
fn wr_342_default_refuses_the_open_shell_and_retains_the_vase() {
    for up in [true, false] {
        let mut k = Kernel::default();
        let mut part = vase(&mut k, up);
        let open = planar_faces_at(&k, &part, 1, H);
        assert_eq!(open.len(), 1);
        let mut notifications = Notifications::new(&mut part);
        let key = part.key();
        let tolerance = part.tolerance();
        let ids = (
            part.face_list().to_vec(),
            part.edge_list().to_vec(),
            part.vertex_list().to_vec(),
        );
        let points: Vec<_> = part
            .vertices()
            .iter()
            .map(|(id, v)| (id, v.point, v.tolerance))
            .collect();
        let outcome = OffsetExecutive::shell_brep(
            &mut k,
            part,
            ShellOptions {
                offset: OffsetOptions {
                    distance: -2.0,
                    extend_convex_edges: true,
                    merge_results: true,
                },
                create_offset_solid: false,
                merge_sheet_side_walls: false,
            },
            &open,
            &mut notifications.context(),
        );
        let error = outcome.result.as_ref().unwrap_err();
        let SolidError::Failed(failure) = error else {
            panic!("up {up}: not a typed refusal: {error}");
        };
        assert_eq!(
            failure.operation, "offset::offset_executive::p2::shell_brep",
            "up {up}: {error:?}"
        );
        assert_eq!(
            failure.reason,
            Reason::Offset(OffsetFailure::Unsound {
                what: Unsoundness::NotManifoldSolid
            }),
            "up {up}: {error:?}"
        );
        assert!(!outcome.input_ownership_transferred());
        let mut retained = outcome.retained_input.unwrap();
        assert_eq!(retained.key(), key);
        assert_eq!(retained.tolerance(), tolerance);
        assert_eq!(
            (
                retained.face_list().to_vec(),
                retained.edge_list().to_vec(),
                retained.vertex_list().to_vec()
            ),
            ids
        );
        assert_eq!(
            retained
                .vertices()
                .iter()
                .map(|(id, v)| (id, v.point, v.tolerance))
                .collect::<Vec<_>>(),
            points
        );
        // Check the retained caches as returned, without refreshing them first.
        assert!(
            solidcore::topology::integrity::check(&retained).is_ok(),
            "up {up}"
        );
        assert!(brep_is_manifold_solid(&retained).unwrap(), "up {up}");
        for level in [CheckLevel::Level0, CheckLevel::Level1, CheckLevel::Level2] {
            let checked = check_brep(&mut k, &mut retained, level).unwrap();
            assert!(
                checked.is_valid(),
                "up {up}, {level:?}: {:?}",
                checked.lines()
            );
        }
        let v = volume(&k, &mut retained).unwrap();
        let exact = PI * R * R * (HB / 3.0 + H - HB);
        // The original cone and cylinder, not an open or partially hollowed body.
        assert!((v - exact).abs() <= 1e-9 * exact, "up={up}: {v}");
    }
}
