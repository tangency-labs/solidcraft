//! Regression test for WR-578 (the fork's tangency-labs/smlib 7ff0a94 and 34318f4): a
//! solid offset leaves a face loop's vertex behind where two consecutive edges' offsets
//! run parallel and never meet, and the merge refuses the shell (1001). The gap is
//! closed by the straight edge between the two ends (the default path since #2981).
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
//! past the end of its edge. The fork refused it as `SM_ERR_OFFSET_EDGE_VANISHES` (1043).
//! Now that offset faces whose loops cross are always rebuilt (WR-345), it shells into the
//! closed form's solid. Open at the notch's floor it shells too.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers
use solidcore::math::det;

#[path = "support/offset_fixture.rs"]
mod offset_fixture;

#[path = "support/volume_leak.rs"]
mod volume_leak;
use offset_fixture::{planar_faces_at, report, shell};
use solidcore::boolean::api_brep::{boolean_difference, boolean_union};
use solidcore::curve::curve::Curve;
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::mass::integrator::properties::brep_area;
use solidcore::math::{Point3, Vec3};
use solidcore::primitive::api_curves::{create_arc, create_line_segment};
use solidcore::primitive::api_primitives::{create_box, create_cylinder};
use solidcore::query::api_queries::brep_bounding_box;
use solidcore::sweep::api_brep::create_linear_sweep;
use solidcore::topology::brep::p5::transform;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::proto_topology::p1::api_transform::{api_rotate, TransformObject};
use solidcore::{Brep, Kernel};

const SHELL_1494: f64 = 8864.598272772;

/// The box joined with the cylinder, its seam `seam_deg` from +y towards +z.
fn box_and_cylinder(k: &mut Kernel, seam_deg: f64) -> Brep {
    let body = create_box(k, Point3::new(0.0, 0.0, 0.0), 47.5, 45.0, 30.0).unwrap();
    let mut cylinder = create_cylinder(k, Point3::new(0.0, 0.0, 0.0), 7.5, 30.0).unwrap();
    let (s, c) = det::sin_cos(seam_deg.to_radians());
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

fn shell_1494(seam_deg: f64) -> (Kernel, solidcore::Result<Brep>) {
    shell_1494_with(Kernel::default(), seam_deg)
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
        let (k, result) = shell_1494(seam);
        assert!(report(
            &k,
            &format!("wr_578 shell/1494 seam {seam}"),
            result,
            Some(SHELL_1494),
            1e-8
        ));
    }
}

/// Seam 200 on the default path: each face's surface extended by its offset's reach (not
/// five offsets), the shell is sound as at seams 30 and 55 (manifold, integrity clean), its
/// volume 1.5e-7 off the fork's closed form as theirs are (WR-578's divergence).
#[test]
fn wr_578_seam_200_shells_sound() {
    let (k, result) = shell_1494(200.0);
    assert!(report(
        &k,
        "wr_578 shell/1494 seam 200",
        result,
        Some(SHELL_1494),
        1e-6
    ));
}

/// Seams 125 and 200 degrees off the front face: the fork shells these too; the port
/// returns a body that is not a manifold solid (WR-578).
#[test]
#[ignore = "open: the port returns a non-manifold body for these seams (WR-578)"]
fn wr_578_box_and_cylinder_shell_other_seams() {
    for seam in [125.0, 200.0] {
        let (k, result) = shell_1494(seam);
        assert!(report(
            &k,
            &format!("wr_578 shell/1494 seam {seam}"),
            result,
            Some(SHELL_1494),
            1e-8
        ));
    }
}

/// Seam 125, whose seam rim edge took the other arc of its circle (a trim point at the
/// circle's seam has two parameters) and left no closed solid: each edge now takes the arc
/// its base edge runs along, and the shell is sound as at seam 200.
#[test]
fn wr_578_seam_125_shells_sound() {
    let (k, result) = shell_1494(125.0);
    assert!(report(
        &k,
        "wr_578 shell/1494 seam 125",
        result,
        Some(SHELL_1494),
        1e-6
    ));
}

/// Loop-gap closure is now the default, so the formerly refused shell is sound.
#[test]
fn wr_578_box_and_cylinder_shell_on_the_default_path() {
    let (k, result) = shell_1494(30.0);
    assert!(report(
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

/// Open at the far side under the notch: the floor under the notch (2.5 thick) is thinner
/// than the two walls' 2 x 1.5, so the cavity never reaches under it and the strip's top
/// edge vanishes. The fork refused that (1043); with offset faces whose loops cross
/// rebuilt (WR-345) the shell is built, `check_brep` Level 2 clean and a manifold solid.
///
/// Closed form: the notched block 30 x 35 x 7.5 - 22.5 x 5 x 5 = 7312.5 less the cavity,
/// the block shrunk 1.5 except at the open side y = 35 (x 1.5..28.5, y 1.5..35,
/// z 1.5..6: 27 x 33.5 x 4.5) less the notch grown 1.5 into it (x < 24, y > 28.5,
/// z > 1: 22.5 x 6.5 x 4.5), so 7312.5 - (4070.25 - 658.125) = 3900.375.
///
/// The shape is pinned beyond its volume too: 16 faces (the block's 9, its y = 35 side
/// now holed, and the cavity's 7: z 1.5 and 6, x 1.5, 28.5 and 24, y 1.5 and 28.5), the
/// block's box, and the area. The notched block's surface is the box's 3075 (the notch
/// moves its top, far and x = 0 pieces to the floor and walls it cuts), less the opening
/// 4.5 x 4.5 (x 24..28.5, z 1.5..6). The cavity's is twice its L section (27 x 33.5 -
/// 22.5 x 6.5 = 758.25) plus its sides (perimeter 121, 4.5 high) less the same opening:
/// 3075 - 20.25 + 2 x 758.25 + 121 x 4.5 - 20.25 = 5095.5.
#[test]
fn wr_578_shell_thicker_than_a_step_shells_where_its_edge_vanishes() {
    let mut k = Kernel::default();
    let b = notched(&mut k);
    let strip = planar_faces_at(&k, &b, 1, 35.0);
    assert_eq!(strip.len(), 1, "the far side under the notch");
    let mut result = shell(&mut k, b, -1.5, &strip).unwrap();
    result.use_arena_naming().unwrap();
    let lines = check_brep(&mut k, &mut result, CheckLevel::Level2)
        .unwrap()
        .lines();
    assert!(lines.is_empty(), "{lines:?}");
    assert_eq!(result.face_list().len(), 16);
    let bb = brep_bounding_box(&k, &result, true).unwrap();
    for (i, hi) in [30.0, 35.0, 7.5].into_iter().enumerate() {
        assert!(bb.min_at(i).abs() <= 1e-9, "box min {i}: {}", bb.min_at(i));
        assert!(
            (bb.max_at(i) - hi).abs() <= 1e-9 * hi,
            "box max {i}: {}",
            bb.max_at(i)
        );
    }
    let area = brep_area(&k, &mut result, 1e-9).unwrap();
    let want_area = 3075.0 - 20.25 + 2.0 * 758.25 + 121.0 * 4.5 - 20.25;
    assert!(
        (area - want_area).abs() <= 1e-9 * want_area,
        "area {area}, closed form {want_area}"
    );
    let want = 7312.5 - (27.0 * 33.5 * 4.5 - 22.5 * 6.5 * 4.5);
    assert!(report(
        &k,
        "wr_578 notched open under the notch",
        Ok(result),
        Some(want),
        1e-9
    ));
}

#[test]
fn wr_578_notched_box_open_at_the_floor_shells() {
    let mut k = Kernel::default();
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
/// reads (UV trims, a snapped trim, gap caches: WR-583), and closure then refuses it;
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

fn shell_1528() -> (Kernel, solidcore::Result<Brep>) {
    let mut k = Kernel::default();
    let b = plate_1528(&mut k);
    let open = planar_faces_at(&k, &b, 0, 42.5);
    assert_eq!(open.len(), 1, "the far end");
    let result = shell(&mut k, b, -2.0, &open);
    (k, result)
}

#[test]
fn wr_578_issue_1528_plate_shells_on_a_clean_operand() {
    let (k, fixed) = shell_1528();
    let mut fixed = fixed.unwrap();
    assert_eq!(fixed.face_list().len(), 16, "the default shell's faces");
    let (old, new) = (10400.212852557255, 10400.212432293585);
    volume_leak::assert_moved_by_the_old_leak(&k, &mut fixed, old, new, 1e-6);
    assert_eq!(fixed.edge_list().len(), 36, "the default shell's edges");
    assert!(report(
        &k,
        "wr_578 #1528 plate default",
        Ok(fixed),
        // since #2890 (precise properties integrate from the trims' base): +4.8e-4 from the
        // closed form 10400.212373846494 (issue_1950's), within the volume's accuracy
        // (1e-6); it was 10400.21299138017, +6.2e-4. Since `brep_volume` closes tolerant
        // edges' gaps (#16735), +5.6e-9 from it (MEASURED); it was 10400.212852557255
        Some(new),
        1e-9
    ));
}
