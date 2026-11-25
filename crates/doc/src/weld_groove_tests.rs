//! Groove welds: each part's groove faces read as one chain from the toe on a Y groove and an I
//! groove, the same chain whatever the order of the picks, the parts only read, and every typed
//! refusal of picks that are not one chain from the toe; the joint's frame on both grooves, the
//! same whatever the order of the picks, mirrored when the parts swap, and a plate not straight
//! along the joint refused. Ported from the second groove PR's first slice (solidcore
//! `weld::groove::joint`, never pushed) onto ops reads (#18500).

use solidcore_record::{Loop2, Plane, Region2, Vec2, Vec3};
use solidcraft_kernel::services::prism::extrude_planar_regions;
use solidcraft_kernel::services::{ExactBody as Body, Order};

use super::*;

/// A plate: the polygon `corners` (x, y, counter-clockwise) swept 120 along z, and its order.
fn plate(corners: &[(f64, f64)]) -> (Body, Order) {
    swept(&Plane::XY, corners)
}

/// The polygon `corners` in `plane`, swept 120 along its normal, and its order.
fn swept(plane: &Plane, corners: &[(f64, f64)]) -> (Body, Order) {
    let pts: Vec<Vec2> = corners.iter().map(|&(x, y)| Vec2::new(x, y)).collect();
    let region = Region2 { outer: Loop2::polygon(&pts), holes: Vec::new() };
    let body = extrude_planar_regions(plane, &[region], 0.0, 120.0).unwrap();
    let (_, order) = body.to_record().unwrap();
    (body, order)
}

/// The record faces of `plate` whose outward normal is `n` (a unit vector).
fn facing((b, o): &(Body, Order), n: [f64; 3]) -> Vec<usize> {
    (0..o.faces.len())
        .filter(|&f| b.face_plane_normal(o, f).unwrap().is_some_and(|m| m.value.iter().zip(n).all(|(a, b)| (a - b).abs() < 1e-9)))
        .collect()
}

fn unit(x: f64, y: f64) -> [f64; 3] {
    let l = x.hypot(y);
    [x / l, y / l, 0.0]
}

const UP: [f64; 3] = [0.0, 1.0, 0.0];

/// A 60° V with a root face 1 and a gap 1 on 8 mm plates: the left and right plates, and each
/// one's groove faces (bevel, then root face).
fn y_groove() -> ([(Body, Order); 2], [Vec<usize>; 2]) {
    let t = 30.0_f64.to_radians().tan();
    let top = 0.5 + 7.0 * t;
    let left = plate(&[(-40.0, 0.0), (-40.0, -8.0), (-0.5, -8.0), (-0.5, -7.0), (-top, 0.0)]);
    let right = plate(&[(top, 0.0), (0.5, -7.0), (0.5, -8.0), (40.0, -8.0), (40.0, 0.0)]);
    let lf = [facing(&left, unit(1.0, t)), facing(&left, [1.0, 0.0, 0.0])].concat();
    let rf = [facing(&right, unit(-1.0, t)), facing(&right, [-1.0, 0.0, 0.0])].concat();
    assert_eq!((lf.len(), rf.len()), (2, 2), "{lf:?} {rf:?}");
    ([left, right], [lf, rf])
}

/// The pick of `faces` on `plate`, its outer face the one facing +y.
fn pick<'a>(plate: &'a (Body, Order), faces: &'a [usize]) -> GroovePick<'a> {
    GroovePick { part: &plate.0, order: &plate.1, faces, outer: facing(plate, UP)[0] }
}

#[test]
fn a_y_groove_gives_each_parts_chain_from_the_toe() {
    let (parts, faces) = y_groove();
    let before: Vec<_> = parts.iter().map(|(b, _)| b.to_record().unwrap().0).collect();
    for ((plate, f), part) in parts.iter().zip(&faces).zip([JointPart::Left, JointPart::Right]) {
        let got = chain(&pick(plate, f), part).unwrap();
        // the bevel first (it meets the outer face), then the root face
        assert_eq!(&got.faces, f);
        // the toe: one edge, on both the bevel and the outer face
        let outer = facing(plate, UP)[0];
        assert_eq!(got.toe.len(), 1, "{got:?}");
        for face in [f[0], outer] {
            assert!(plate.0.rim_edges(&plate.1, &[face]).unwrap().contains(&got.toe[0]));
        }
    }
    // the parts are only read
    let after: Vec<_> = parts.iter().map(|(b, _)| b.to_record().unwrap().0).collect();
    assert_eq!(after, before);
}

#[test]
fn the_order_of_the_picks_changes_nothing() {
    let (parts, faces) = y_groove();
    for (plate, f) in parts.iter().zip(&faces) {
        let reversed: Vec<usize> = f.iter().rev().copied().collect();
        let twice = [f.as_slice(), f.as_slice()].concat();
        let want = chain(&pick(plate, f), JointPart::Left).unwrap();
        assert_eq!(chain(&pick(plate, &reversed), JointPart::Left).unwrap(), want);
        assert_eq!(chain(&pick(plate, &twice), JointPart::Left).unwrap(), want);
    }
}

#[test]
fn an_i_groove_is_a_chain_of_its_square_wall() {
    let left = plate(&[(-40.0, 0.0), (-40.0, -6.0), (-1.0, -6.0), (-1.0, 0.0)]);
    let wall = facing(&left, [1.0, 0.0, 0.0]);
    let got = chain(&pick(&left, &wall), JointPart::Left).unwrap();
    assert_eq!(got.faces, wall);
    assert_eq!(got.toe.len(), 1);
}

#[test]
fn picks_that_are_not_one_chain_from_the_toe_are_refused() {
    let (parts, faces) = y_groove();
    let [left, right] = &parts;
    let refused = |plate: &(Body, Order), f: &[usize], part: JointPart| chain(&pick(plate, f), part).unwrap_err();
    // the root face alone: it does not meet the outer face
    let outer = facing(left, UP)[0];
    assert_eq!(refused(left, &[faces[0][1]], JointPart::Left), GrooveRefusal::ArrowFaceNotAtToe { part: JointPart::Left, face: outer });
    // the bevel and the bottom face, which share no edge
    let bottom = facing(left, [0.0, -1.0, 0.0])[0];
    assert_eq!(refused(left, &[faces[0][0], bottom], JointPart::Left), GrooveRefusal::PicksNotOneChain { part: JointPart::Left, face: bottom });
    // the bevel, the root face, the bottom and the end cap at z = 0: the cap meets all three
    let cap = facing(left, [0.0, 0.0, -1.0])[0];
    let got = refused(left, &[faces[0][0], faces[0][1], bottom, cap], JointPart::Left);
    assert!(matches!(got, GrooveRefusal::PicksNotOneChain { part: JointPart::Left, .. }), "{got:?}");
    // the outer face among the groove faces: a T joint
    let up = facing(right, UP)[0];
    assert_eq!(refused(right, &[faces[1][0], up], JointPart::Right), GrooveRefusal::TJoint { part: JointPart::Right });
    // nothing picked, or a face the part does not have
    assert!(matches!(refused(left, &[], JointPart::Left), GrooveRefusal::Invalid(_)));
    assert!(matches!(refused(left, &[999], JointPart::Left), GrooveRefusal::Invalid(_)));
}

// ---- the joint's frame ----

/// Each part's chain and the joint's frame, the outer faces the ones facing +y.
fn joint(parts: &[(Body, Order); 2], faces: [&[usize]; 2]) -> Result<Plane> {
    let picks = [pick(&parts[0], faces[0]), pick(&parts[1], faces[1])];
    let chains = [chain(&picks[0], JointPart::Left)?, chain(&picks[1], JointPart::Right)?];
    joint_frame([&picks[0], &picks[1]], [&chains[0], &chains[1]])
}

fn near(a: Vec3, b: [f64; 3]) -> bool {
    (a.x - b[0]).abs() < 1e-12 && (a.y - b[1]).abs() < 1e-12 && (a.z - b[2]).abs() < 1e-12
}

/// The plane's normal, x × y: the joint's direction.
fn normal(p: &Plane) -> [f64; 3] {
    let (x, y) = (p.x, p.y);
    [x.y * y.z - x.z * y.y, x.z * y.x - x.x * y.z, x.x * y.y - x.y * y.x]
}

#[test]
fn a_y_groove_runs_along_z_its_frame_towards_the_toe() {
    let (parts, faces) = y_groove();
    let frame = joint(&parts, [&faces[0], &faces[1]]).unwrap();
    // x out of the left part towards its toe, y its outer face's normal, so the joint runs +z
    assert!(near(frame.x, [1.0, 0.0, 0.0]) && near(frame.y, [0.0, 1.0, 0.0]), "{frame:?}");
    assert!(near(Vec3::new(normal(&frame)[0], normal(&frame)[1], normal(&frame)[2]), [0.0, 0.0, 1.0]));
    // the origin: the middle of the left toe line
    let top = 0.5 + 7.0 * 30.0_f64.to_radians().tan();
    let o = frame.origin;
    assert!((o.x + top).abs() < 1e-9 && o.y.abs() < 1e-9 && (o.z - 60.0).abs() < 1e-9, "{o:?}");
}

#[test]
fn the_order_of_the_picks_changes_nothing_and_swapped_parts_mirror_the_frame() {
    let (parts, faces) = y_groove();
    let frame = joint(&parts, [&faces[0], &faces[1]]).unwrap();
    let reversed: [Vec<usize>; 2] = [faces[0].iter().rev().copied().collect(), faces[1].iter().rev().copied().collect()];
    assert_eq!(joint(&parts, [&reversed[0], &reversed[1]]).unwrap(), frame);
    let [l, r] = parts;
    let swapped = joint(&[r, l], [&faces[1], &faces[0]]).unwrap();
    let (x, n) = (frame.x, normal(&frame));
    assert!(near(swapped.x, [-x.x, -x.y, -x.z]), "{swapped:?}");
    assert!(near(swapped.y, [frame.y.x, frame.y.y, frame.y.z]), "{swapped:?}");
    let m = normal(&swapped);
    assert!(near(Vec3::new(m[0], m[1], m[2]), [-n[0], -n[1], -n[2]]), "{swapped:?}");
}

#[test]
fn an_i_groove_takes_its_direction_from_a_wall_and_its_outer_face() {
    let left = plate(&[(-40.0, 0.0), (-40.0, -6.0), (-1.0, -6.0), (-1.0, 0.0)]);
    let right = plate(&[(1.0, 0.0), (1.0, -6.0), (40.0, -6.0), (40.0, 0.0)]);
    let (lf, rf) = (facing(&left, [1.0, 0.0, 0.0]), facing(&right, [-1.0, 0.0, 0.0]));
    let frame = joint(&[left, right], [&lf, &rf]).unwrap();
    let n = normal(&frame);
    assert!(near(Vec3::new(n[0], n[1], n[2]), [0.0, 0.0, 1.0]), "{frame:?}");
}

#[test]
fn a_plate_not_straight_along_the_joint_is_refused() {
    // an I groove whose left plate is swept along a direction 0.01 rad off the right one's
    let (c, s) = (0.01_f64.cos(), 0.01_f64.sin());
    let tilted = Plane { origin: Vec3::new(0.0, 0.0, 0.0), x: Vec3::new(c, 0.0, s), y: Vec3::new(0.0, 1.0, 0.0) };
    let left = swept(&tilted, &[(-40.0, 0.0), (-40.0, -6.0), (-1.0, -6.0), (-1.0, 0.0)]);
    let right = plate(&[(1.0, 0.0), (1.0, -6.0), (40.0, -6.0), (40.0, 0.0)]);
    let (lf, rf) = (facing(&left, [c, 0.0, s]), facing(&right, [-1.0, 0.0, 0.0]));
    assert_eq!((lf.len(), rf.len()), (1, 1));
    let got = joint(&[left, right], [&lf, &rf]).unwrap_err();
    assert!(matches!(got, GrooveRefusal::GrooveFaceNotStraight { .. }), "{got:?}");
}
