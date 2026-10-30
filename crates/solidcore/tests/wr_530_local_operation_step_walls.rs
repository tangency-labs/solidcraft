//! Regression test for the WR-530 addendum (the fork's tangency-labs/smlib 5d428ae and
//! 90aabff): a local operation that moves one of two tangent faces along its normal
//! leaves the two parallel and apart. WR-530's opt-in refuses that; with
//! `local_operation_step_walls` the step is built (a ruled wall between the two edges,
//! the gaps at its ends closed).
//!
//! The cases are the fork's, rebuilt here: a block whose top is imprinted with a split,
//! one piece moved up or down (the step adds or takes off the piece's footprint times
//! the move), and WR-530's boss wall moved out 1 off its base round (the 8 of wall above
//! the round grows from r10 to r11: plus pi * (121 - 100) * 8).
//!
//! Each moved face gets its own surface offset (`CreateOffsetSurface`), as the fork's
//! `smlib_offset_faces` moves it: the local operation lifts the face's edges onto the new
//! surface by their UV, so the new surface must keep the face's parameterization.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

#[path = "support/offset_fixture.rs"]
mod offset_fixture;

use std::f64::consts::PI;

use offset_fixture::{planar_faces_at, report, volume, Notifications};
use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::boolean::api_brep::{boolean_union, boolean_with_options};
use solidcore::boolean::merge::MergeOptions;
use solidcore::error::{SolidError, SM_ERR};
use solidcore::geom::a_object::BooleanOperation;
use solidcore::math::{Point3, Vec3};
use solidcore::nurbs::knots::Side;
use solidcore::primitive::api_primitives::{create_box, create_cylinder};
use solidcore::surface::surface::Surface;
use solidcore::topology::brep::local_operation::local_operation;
use solidcore::topology::FaceId;
use solidcore::{Brep, Kernel};

fn kernel(step_walls: bool) -> Kernel {
    let mut k = Kernel::default();
    k.fixes.local_operation_refuses_loose_tangent_edges = true;
    k.fixes.local_operation_step_walls = step_walls;
    k
}

/// A `dx x dy x dz` block whose top is imprinted along x = `split` (the capi's
/// `smlib_imprint` of a box over the part x > `split`, z > dz / 2).
fn split_block(k: &mut Kernel, dx: f64, dy: f64, dz: f64, split: f64) -> Brep {
    let block = create_box(k, Point3::new(0.0, 0.0, 0.0), dx, dy, dz).unwrap();
    let cutter = create_box(k, Point3::new(split, -1.0, dz / 2.0), dx, dy + 2.0, dz).unwrap();
    let mut b = boolean_with_options(
        k,
        block,
        cutter,
        BooleanOperation::Imprint,
        MergeOptions::default(),
    )
    .unwrap()
    .result;
    b.refresh_cached_topology();
    assert_eq!(
        planar_faces_at(k, &b, 2, dz).len(),
        2,
        "the top is split in two"
    );
    b
}

/// The piece of the top at height `dz` whose middle lies on the given side of x = `split`.
fn top_piece(k: &Kernel, b: &Brep, dz: f64, split: f64, above: bool) -> FaceId {
    let pieces: Vec<_> = planar_faces_at(k, b, 2, dz)
        .into_iter()
        .filter(|&f| {
            let face = b.face(f).unwrap();
            let s = face.surface.as_ref().unwrap();
            let p = s.evaluate_point(k, face.uv_domain.mid()).unwrap();
            (p.x > split) == above
        })
        .collect();
    assert_eq!(pieces.len(), 1, "one piece on that side");
    pieces[0]
}

/// The surface of `f` offset by `d` towards `towards` (`CreateOffsetSurface`, as the
/// fork's `smlib_offset_faces` moves a face). The offset keeps the face's own
/// parameterization: the local operation lifts the face's edges onto the new surface by
/// their UV, so a surface built afresh (another origin, another radius in an arc-length
/// parameter) carries them elsewhere.
fn moved_surface(k: &Kernel, b: &Brep, f: FaceId, towards: Vec3, d: f64) -> Surface {
    let face = b.face(f).unwrap();
    let s = face.surface.as_ref().unwrap();
    let n = s
        .evaluate_normal(k, face.uv_domain.mid(), Side::Left, Side::Left)
        .unwrap();
    let signed = if n.dot(towards) > 0.0 { d } else { -d };
    s.create_offset_surface(k, signed, 1e-6).unwrap().unwrap()
}

/// The split block's `piece` moved `dz` along z (`SmBrep::LocalOperation`).
fn move_piece(k: &mut Kernel, b: &mut Brep, piece: FaceId, dz: f64) -> solidcore::Result<()> {
    let surface = moved_surface(k, b, piece, Vec3::new(0.0, 0.0, 1.0), dz);
    let mut notifications = Notifications::new(b);
    local_operation(k, b, &[piece], vec![surface], &mut notifications.context())
}

fn refused(result: solidcore::Result<()>) {
    match result {
        Err(SolidError::Upstream { code, .. }) => assert_eq!(code, SM_ERR),
        other => panic!("WR-530 refuses the step without the opt-in: {other:?}"),
    }
}

/// The fork's shell-12 dump: a 40 x 30 x 10 block, its top split at x = 20, the piece
/// x < 20 up 2: 8 faces, 13200.
#[test]
fn wr_530_step_up_on_a_split_block_top_is_built() {
    let mut k = kernel(true);
    let mut b = split_block(&mut k, 40.0, 30.0, 10.0, 20.0);
    let piece = top_piece(&k, &b, 10.0, 20.0, false);
    move_piece(&mut k, &mut b, piece, 2.0).unwrap();
    b.refresh_cached_topology();
    assert!(report(&k, "wr_530 step up 2", Ok(b), Some(13200.0), 1e-6));
}

/// `ops_kernel`'s split block: 60 x 40 x 20, the top split at x = 30, one piece up 1
/// (adds 30 x 40 x 1) and down 1 (takes it off), here the piece x < 30 (the fork's own
/// piece, x > 30: `wr_530_step_on_the_imprinted_piece_is_built`).
#[test]
fn wr_530_step_up_and_down_on_a_split_block_top_is_built() {
    for (to, want) in [(21.0, 49200.0), (19.0, 46800.0)] {
        let mut k = kernel(true);
        let mut b = split_block(&mut k, 60.0, 40.0, 20.0, 30.0);
        let piece = top_piece(&k, &b, 20.0, 30.0, false);
        move_piece(&mut k, &mut b, piece, to - 20.0).unwrap();
        b.refresh_cached_topology();
        assert!(report(
            &k,
            &format!("wr_530 step to {to}"),
            Ok(b),
            Some(want),
            1e-6
        ));
    }
}

/// The fork's own case: the piece x > 30, under the imprinting box (its side neighbours are
/// the pieces the imprint split off the side faces), up 1.
#[test]
fn wr_530_step_on_the_imprinted_piece_is_built() {
    let mut k = kernel(true);
    let mut b = split_block(&mut k, 60.0, 40.0, 20.0, 30.0);
    let piece = top_piece(&k, &b, 20.0, 30.0, true);
    move_piece(&mut k, &mut b, piece, 1.0).unwrap();
    b.refresh_cached_topology();
    assert!(report(
        &k,
        "wr_530 imprinted piece up 1",
        Ok(b),
        Some(49200.0),
        1e-6
    ));
}

#[test]
fn wr_530_split_block_step_is_refused_without_the_opt_in() {
    let mut k = kernel(false);
    let mut b = split_block(&mut k, 60.0, 40.0, 20.0, 30.0);
    let piece = top_piece(&k, &b, 20.0, 30.0, true);
    refused(move_piece(&mut k, &mut b, piece, 1.0));
}

const AXIS: (f64, f64) = (30.0, 20.0);

fn axis_distance(p: Point3) -> f64 {
    (p.x - AXIS.0).hypot(p.y - AXIS.1)
}

/// WR-530's part: a 60 x 40 x 10 plate, an r10 boss, an r2 round at the boss's base.
fn boss_part(k: &mut Kernel) -> Brep {
    let plate = create_box(k, Point3::new(0.0, 0.0, 0.0), 60.0, 40.0, 10.0).unwrap();
    let boss = create_cylinder(k, Point3::new(AXIS.0, AXIS.1, 5.0), 10.0, 15.0).unwrap();
    let mut b = boolean_union(k, plate, boss).unwrap();
    let base: Vec<_> = b
        .edges()
        .iter()
        .filter(|(_, e)| {
            let c = e.curve.as_ref().unwrap();
            let p = c.evaluate_point(k, e.interval.mid()).unwrap();
            (p.z - 10.0).abs() < 1e-9 && (axis_distance(p) - 10.0).abs() < 1e-9
        })
        .map(|(id, _)| id)
        .collect();
    assert!(!base.is_empty(), "the boss's base edge");
    fillet_edges(k, &mut b, &base, 2.0, 1, 1, 1.0).unwrap();
    b.refresh_cached_topology();
    b
}

fn move_boss_wall(k: &mut Kernel, b: &mut Brep) -> solidcore::Result<()> {
    let faces: Vec<_> = b
        .face_list()
        .iter()
        .copied()
        .filter(|&f| {
            let face = b.face(f).unwrap();
            let s = face.surface.as_ref().unwrap();
            let p = s.evaluate_point(k, face.uv_domain.mid()).unwrap();
            p.z > 12.5 && (axis_distance(p) - 10.0).abs() < 1e-6
        })
        .collect();
    assert!(!faces.is_empty(), "the boss's wall");
    // each wall face's cylinder, offset 1 outwards (r10 to r11)
    let surfaces = faces
        .iter()
        .map(|&f| {
            let face = b.face(f).unwrap();
            let s = face.surface.as_ref().unwrap();
            let p = s.evaluate_point(k, face.uv_domain.mid()).unwrap();
            let out = Vec3::new(p.x - AXIS.0, p.y - AXIS.1, 0.0);
            moved_surface(k, b, f, out, 1.0)
        })
        .collect();
    let mut notifications = Notifications::new(b);
    local_operation(k, b, &faces, surfaces, &mut notifications.context())
}

/// The boss wall moved off its base round keeps the round and gains a ledge on top of
/// it (the step wall round the circle, built without folding through its own axis).
#[test]
fn wr_530_boss_wall_moved_off_its_round_builds_the_step() {
    let mut k = kernel(true);
    let mut b = boss_part(&mut k);
    let before = volume(&k, &mut b).unwrap();
    move_boss_wall(&mut k, &mut b).unwrap();
    b.refresh_cached_topology();
    let want = before + PI * 21.0 * 8.0;
    assert!(report(
        &k,
        "wr_530 boss wall out 1",
        Ok(b),
        Some(want),
        1e-6
    ));
}

#[test]
fn wr_530_boss_wall_step_is_refused_without_the_opt_in() {
    let mut k = kernel(false);
    let mut b = boss_part(&mut k);
    refused(move_boss_wall(&mut k, &mut b));
}
