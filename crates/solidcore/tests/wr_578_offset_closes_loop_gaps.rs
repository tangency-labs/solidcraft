//! Regression test for WR-578 (the fork's tangency-labs/smlib 7ff0a94 and 34318f4): a
//! solid offset leaves a face loop's vertex behind where two consecutive edges' offsets
//! run parallel and never meet, and the merge refuses the shell (1001). With
//! `offset_closes_loop_gaps` the gap is closed by the straight edge between the two ends.
//!
//! SolidCraft rig case shell/1494, rebuilt: a 47.5 x 45 x 30 box joined with an r7.5
//! cylinder along x from its side x = 47.5 to 77.5, the axis in the front face's plane
//! y = 0 at z = 10, shelled 1 inward open at the bottom. The fork's closed form is
//! 8864.598272772 (the box's inset, the cylinder's inset from x 48.5 to 76.5, and between
//! the side's two offsets the circular segment of r 6.5 beyond the chord y = 1, 2 long).
//! The cylinder's seam is turned 30 degrees off the front face (the fork's case refuses a
//! seam through one of the loop's two vertices).
//!
//! 34318f4: a 30 x 35 x 7.5 box notched 22.5 x 5 down to z = 2.5 at its far corner, shelled
//! 1.5 inward open at the far side under the notch: the wall moves the strip's top corner
//! past the end of its edge, `SM_ERR_OFFSET_EDGE_VANISHES` (1043). Open at the notch's
//! floor it shells.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

#[path = "support/offset_fixture.rs"]
mod offset_fixture;

use offset_fixture::{planar_faces_at, report, shell};
use solidcore::boolean::api_brep::{boolean_difference, boolean_union};
use solidcore::curve::curve::Curve;
use solidcore::error::{SolidError, SM_ERR};
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::math::{Point3, Vec3};
use solidcore::primitive::api_curves::{create_arc, create_line_segment};
use solidcore::primitive::api_primitives::{create_box, create_cylinder};
use solidcore::sweep::api_brep::create_linear_sweep;
use solidcore::topology::brep::p5::transform;
use solidcore::topology::proto_topology::p1::api_transform::{api_rotate, TransformObject};
use solidcore::{Brep, Kernel};

const SHELL_1494: f64 = 8864.598272772;

fn kernel(fixed: bool) -> Kernel {
    let mut k = Kernel::default();
    k.fixes.offset_closes_loop_gaps = fixed;
    k
}

/// The box joined with the cylinder, its seam `seam_deg` from +y towards +z.
fn box_and_cylinder(k: &mut Kernel, seam_deg: f64) -> Brep {
    let body = create_box(k, Point3::new(0.0, 0.0, 0.0), 47.5, 45.0, 30.0).unwrap();
    let mut cylinder = create_cylinder(k, Point3::new(0.0, 0.0, 0.0), 7.5, 30.0).unwrap();
    let (s, c) = seam_deg.to_radians().sin_cos();
    // local z (the axis) along +x: x = (0, c, s), y = (0, -s, c), z = x cross y = +x
    let placement = Axis2Placement::new(
        Point3::new(47.5, 0.0, 10.0),
        Vec3::new(0.0, c, s),
        Vec3::new(0.0, -s, c),
    );
    transform(k, &mut cylinder, &placement, None, false).unwrap();
    let mut b = boolean_union(k, body, cylinder).unwrap();
    b.refresh_cached_topology();
    b
}

fn shell_1494(fixed: bool, seam_deg: f64) -> (Kernel, solidcore::Result<Brep>) {
    shell_1494_with(kernel(fixed), seam_deg)
}

fn shell_1494_with(mut k: Kernel, seam_deg: f64) -> (Kernel, solidcore::Result<Brep>) {
    let b = box_and_cylinder(&mut k, seam_deg);
    let open = planar_faces_at(&k, &b, 2, 0.0);
    assert_eq!(open.len(), 1, "the box's bottom");
    let result = shell(&mut k, b, -1.0, &open);
    (k, result)
}

/// Seams 30 and 55 degrees off the front face, to the fork's 1e-8. The port shells both
/// sound but 2.7e-7 and 7.7e-7 off the fork's closed form (the fork reports 7e-11): a
/// divergence to trace (WR-578).
#[test]
#[ignore = "open: volume 2.7e-7 / 7.7e-7 off the fork's (WR-578)"]
fn wr_578_box_and_cylinder_shell_closes_the_loop_gaps() {
    for seam in [30.0, 55.0] {
        let (k, result) = shell_1494(true, seam);
        assert!(report(
            &k,
            &format!("wr_578 shell/1494 seam {seam}"),
            result,
            Some(SHELL_1494),
            1e-8
        ));
    }
}

/// Seams 125 and 200 degrees off the front face: the fork shells these too; the port
/// returns a body that is not a manifold solid (WR-578).
#[test]
#[ignore = "open: the port returns a non-manifold body for these seams (WR-578)"]
fn wr_578_box_and_cylinder_shell_other_seams() {
    for seam in [125.0, 200.0] {
        let (k, result) = shell_1494(true, seam);
        assert!(report(
            &k,
            &format!("wr_578 shell/1494 seam {seam}"),
            result,
            Some(SHELL_1494),
            1e-8
        ));
    }
}

/// Seams 125 and 200 with the opt-in alone: the offset is no closed solid, and the shell
/// refuses it (`SM_ERR`) instead of handing it back as upstream's `ShellBrep` does
/// (WR-454), so the flag never turns upstream's refusal into a broken body.
#[test]
fn wr_578_other_seams_are_refused() {
    for seam in [125.0, 200.0] {
        match shell_1494_with(kernel(true), seam).1 {
            Err(SolidError::Upstream { code, .. }) => assert_eq!(code, SM_ERR, "seam {seam}"),
            other => panic!("seam {seam}: {other:?}"),
        }
    }
}

/// Without the opt-in the shell is not that sound body (upstream refuses it, 1001).
#[test]
fn wr_578_box_and_cylinder_shell_without_the_opt_in() {
    let (k, result) = shell_1494(false, 30.0);
    assert!(!report(
        &k,
        "wr_578 shell/1494 default",
        result,
        Some(SHELL_1494),
        1e-8
    ));
}

fn notched(k: &mut Kernel) -> Brep {
    let body = create_box(k, Point3::new(0.0, 0.0, 0.0), 30.0, 35.0, 7.5).unwrap();
    let notch = create_box(k, Point3::new(0.0, 30.0, 2.5), 22.5, 20.0, 22.5).unwrap();
    let mut b = boolean_difference(k, body, notch).unwrap();
    b.refresh_cached_topology();
    b
}

#[test]
fn wr_578_shell_thicker_than_a_step_is_edge_vanishes() {
    let mut k = kernel(true);
    let b = notched(&mut k);
    let strip = planar_faces_at(&k, &b, 1, 35.0);
    assert_eq!(strip.len(), 1, "the far side under the notch");
    match shell(&mut k, b, -1.5, &strip) {
        Err(SolidError::Upstream { code, .. }) => assert_eq!(code, 1043),
        other => panic!("SM_ERR_OFFSET_EDGE_VANISHES expected: {other:?}"),
    }
}

#[test]
fn wr_578_notched_box_open_at_the_floor_shells() {
    let mut k = kernel(true);
    let b = notched(&mut k);
    let floor = planar_faces_at(&k, &b, 2, 2.5);
    assert_eq!(floor.len(), 1, "the notch's floor");
    let result = shell(&mut k, b, -1.5, &floor);
    assert!(report(
        &k,
        "wr_578 notched open at floor",
        result,
        None,
        0.0
    ));
}

/// tangency-labs/solidcore#1528's plate: a 27.5 thick plate on three straight sides and
/// a half circle r 10 (x < 0), a blind hole r 5 along x through the round side at
/// (y, z) = (12.5, 12.5) from x = -10 to 7.5, shelled 2 inward, open at x = 42.5, on a
/// clean operand. A prior mass-properties query on the operand changes what the shell
/// reads (UV trims, a snapped trim, gap caches: WR-583), and the opt-in then refuses it;
/// nothing here measures the operand.
fn plate_1528(k: &mut Kernel) -> Brep {
    // the profile swept 27.5 along z (one curved side face), as the #1528 fixture builds it
    let p = |x: f64, y: f64| Vec3::new(x, y, 0.0);
    let profile: Vec<Curve> = vec![
        Curve::from(create_line_segment(p(0.0, 0.0), p(42.5, 0.0)).unwrap()),
        Curve::from(create_line_segment(p(42.5, 0.0), p(42.5, 20.0)).unwrap()),
        Curve::from(create_line_segment(p(42.5, 20.0), p(0.0, 20.0)).unwrap()),
        Curve::from(create_arc(p(0.0, 10.0), 10.0, 90.0, 270.0).unwrap()),
    ];
    let refs: Vec<Option<&Curve>> = profile.iter().map(Some).collect();
    let plate = create_linear_sweep(k, &refs, Vec3::new(0.0, 0.0, 1.0), 27.5, true).unwrap();
    // a cylinder r 5 along z from (-10, 12.5, 12.5), turned onto +x about its base
    let mut drill = create_cylinder(k, Point3::new(-10.0, 12.5, 12.5), 5.0, 17.5).unwrap();
    api_rotate(
        k,
        Some(TransformObject::Brep(&mut drill)),
        Point3::new(-10.0, 12.5, 12.5),
        Vec3::new(0.0, 1.0, 0.0),
        90.0,
    )
    .unwrap();
    let mut b = boolean_difference(k, plate, drill).unwrap();
    b.refresh_cached_topology();
    b
}

fn shell_1528(fixed: bool) -> (Kernel, solidcore::Result<Brep>) {
    let mut k = kernel(fixed);
    let b = plate_1528(&mut k);
    let open = planar_faces_at(&k, &b, 0, 42.5);
    assert_eq!(open.len(), 1, "the far end");
    let result = shell(&mut k, b, -2.0, &open);
    (k, result)
}

#[test]
fn wr_578_issue_1528_plate_shells_on_a_clean_operand() {
    let (k0, default) = shell_1528(false);
    let default = default.unwrap();
    assert_eq!(default.face_list().len(), 16, "the default shell's faces");
    assert_eq!(default.edge_list().len(), 37, "the default shell's edges");
    assert!(report(
        &k0,
        "wr_578 #1528 plate default",
        Ok(default),
        None,
        0.0
    ));
    let (k, fixed) = shell_1528(true);
    let fixed = fixed.unwrap();
    assert_eq!(fixed.face_list().len(), 16, "the opt-in shell's faces");
    assert_eq!(fixed.edge_list().len(), 36, "the opt-in shell's edges");
    assert!(report(
        &k,
        "wr_578 #1528 plate opt-in",
        Ok(fixed),
        Some(10400.21299138017),
        1e-9
    ));
}
