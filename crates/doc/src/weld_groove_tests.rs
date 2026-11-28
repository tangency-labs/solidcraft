//! Groove welds: each part's groove faces read as one chain from the toe on a Y groove and an I
//! groove, the same chain whatever the order of the picks, the parts only read, and every typed
//! refusal of picks that are not one chain from the toe; the joint's frame on both grooves, the
//! same whatever the order of the picks, mirrored when the parts swap (also off the world axes),
//! signed by the toe on an outer face that is not convex, along the axis of a round groove, the
//! parts only read, and a plate not straight along the joint (naming its wall), an outer face that
//! is no plane, a right outer face off the arrow side, square to the left one or turned 45° or more
//! (on and off the world axes), a round groove whose walls are no cylinders and faces past the
//! direction search's budget refused; the weld filling a Y groove and a round groove flat (their
//! volumes in closed form), a fill the groove cannot take refused with the size that builds; parts
//! of different length, a gap that steps along the joint and a plate whose ends lean (both, or one
//! with the other square) refused with the stretches that keep one section, each of which builds on
//! its own; a wall whose runs never overlap refused as never whole; a wall with a hole or a notch
//! refused as interrupted; a square end read in two pieces and a stretch asked a hair past its ends
//! still built as they should; two halves of one circle not taken for one section; a pick off the
//! chain refused naming it; the fill's notes and refusals in weld words (ISO 6520-1 no. 511 as a
//! warning on the weld face, no. 515 on the root side, full penetration). Ported from the second groove PR's first slice (solidcore `weld::groove::joint`,
//! never pushed) onto ops reads, now through the kernel interface (#18500).

use std::f64::consts::{FRAC_PI_2, TAU};

use solidcore_record::{Loop2, Plane, Region2, Seg2, Vec2, Vec3};
use solidcraft_kernel::{Body, Contour, extrude_planar_regions};

use super::*;

/// A plate: the polygon `corners` (x, y, counter-clockwise) swept 120 along z.
fn plate(corners: &[(f64, f64)]) -> Body {
    swept(&Plane::XY, corners)
}

/// The polygon `corners` in `plane`, swept 120 along its normal.
fn swept(plane: &Plane, corners: &[(f64, f64)]) -> Body {
    swept_by(plane, corners, 120.0)
}

/// The polygon `corners` in `plane`, swept `length` along its normal.
fn swept_by(plane: &Plane, corners: &[(f64, f64)], length: f64) -> Body {
    let pts: Vec<Vec2> = corners.iter().map(|&(x, y)| Vec2::new(x, y)).collect();
    swept_loop(plane, Loop2::polygon(&pts), length)
}

/// The region inside `outline` in `plane`, swept `length` along its normal by the kernel, as the
/// app makes a body: an arc swept square to its plane is a cylinder, which gives a direction.
fn swept_loop(plane: &Plane, outline: Loop2, length: f64) -> Body {
    let region = Region2 { outer: outline, holes: Vec::new() };
    extrude_planar_regions(plane, &[region], 0.0, length).unwrap()
}

/// The kernel's zone tolerance.
fn zone() -> f64 {
    solidcraft_kernel::profiles::zone_tolerance().unwrap()
}

/// The record faces of `plate` whose outward normal is `n` (a unit vector).
fn facing(b: &Body, n: [f64; 3]) -> Vec<usize> {
    (0..b.record().faces.len())
        .filter(|&f| b.face_plane_normal(f).unwrap().is_some_and(|m| m.value.iter().zip(n).all(|(a, b)| (a - b).abs() < 1e-9)))
        .collect()
}

fn unit(x: f64, y: f64) -> [f64; 3] {
    let l = x.hypot(y);
    [x / l, y / l, 0.0]
}

const UP: [f64; 3] = [0.0, 1.0, 0.0];

/// A 60° V with a root face 1 and a gap 1 on 8 mm plates: the left and right plates, and each
/// one's groove faces (bevel, then root face).
fn y_groove() -> ([Body; 2], [Vec<usize>; 2]) {
    y_groove_of(120.0)
}

/// [`y_groove`] with the right plate `length` long (the left one 120).
fn y_groove_of(length: f64) -> ([Body; 2], [Vec<usize>; 2]) {
    let t = 30.0_f64.to_radians().tan();
    let top = 0.5 + 7.0 * t;
    let left = plate(&[(-40.0, 0.0), (-40.0, -8.0), (-0.5, -8.0), (-0.5, -7.0), (-top, 0.0)]);
    let right = swept_by(&Plane::XY, &[(top, 0.0), (0.5, -7.0), (0.5, -8.0), (40.0, -8.0), (40.0, 0.0)], length);
    let lf = [facing(&left, unit(1.0, t)), facing(&left, [1.0, 0.0, 0.0])].concat();
    let rf = [facing(&right, unit(-1.0, t)), facing(&right, [-1.0, 0.0, 0.0])].concat();
    assert_eq!((lf.len(), rf.len()), (2, 2), "{lf:?} {rf:?}");
    ([left, right], [lf, rf])
}

/// The pick of `faces` on `plate`, its outer face the one facing +y.
fn pick<'a>(plate: &'a Body, faces: &'a [usize]) -> GroovePick<'a> {
    GroovePick { part: plate, faces, outer: facing(plate, UP)[0] }
}

#[test]
fn a_y_groove_gives_each_parts_chain_from_the_toe() {
    let (parts, faces) = y_groove();
    let before: Vec<_> = parts.iter().map(|b| b.record().clone()).collect();
    for ((plate, f), part) in parts.iter().zip(&faces).zip([JointPart::Left, JointPart::Right]) {
        let got = chain(&pick(plate, f), part).unwrap();
        // the bevel first (it meets the outer face), then the root face
        assert_eq!(&got.faces, f);
        // the toe: one edge, on both the bevel and the outer face
        let outer = facing(plate, UP)[0];
        assert_eq!(got.toe.len(), 1, "{got:?}");
        for face in [f[0], outer] {
            assert!(plate.rim_edges(&[face]).unwrap().contains(&got.toe[0]));
        }
    }
    // the parts are only read
    let after: Vec<_> = parts.iter().map(|b| b.record().clone()).collect();
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
    let refused = |plate: &Body, f: &[usize], part: JointPart| chain(&pick(plate, f), part).unwrap_err();
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

/// The joint's frame, the outer faces the ones facing +y.
fn joint(parts: &[Body; 2], faces: [&[usize]; 2]) -> Result<Plane> {
    joint_frame([&pick(&parts[0], faces[0]), &pick(&parts[1], faces[1])])
}

/// The farthest any point of a test's parts lies from its frame's origin, rounded up to 300 mm:
/// the L-shaped plate's far corner (200, -8, 240) is 269.9 mm from its toe's middle (-1, 0, 60);
/// the 40 by 120 plates' corners are about 75 mm from theirs.
const REACH: f64 = 300.0;

/// How far each of a frame's unit axes may be off what the test expects, as a vector length: half
/// the zone tolerance over `√3 REACH`. A point `o + a x + b y + c n` with `|(a, b, c)| <= L` moves
/// with the axes by at most `L sqrt(|Δx|² + |Δy|² + |Δn|²) <= √3 L axes()`, `zone / 2` for
/// `L <= REACH`, and with the origin by at most [`points`], the other half: so every point within
/// [`REACH`] of the origin is within the zone tolerance ([`zone`], the kernel's
/// own bound for two points being the same), whatever the axes' errors (brett19 on #22033).
fn axes() -> f64 {
    zone() / (2.0 * 3.0_f64.sqrt() * REACH)
}

/// How far the frame's origin may be off, as a vector length: half the zone tolerance (the other
/// half is the axes', [`axes`]).
fn points() -> f64 {
    zone() / 2.0
}

/// Whether `a` is within `tol` of `b`, as a vector length.
fn within(a: Vec3, b: [f64; 3], tol: f64) -> bool {
    let d = [a.x - b[0], a.y - b[1], a.z - b[2]];
    (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt() <= tol
}

/// Whether the unit axis `a` is within [`axes`] of `b`.
fn near(a: Vec3, b: [f64; 3]) -> bool {
    within(a, b, axes())
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
    assert!(within(frame.origin, [-top, 0.0, 60.0], points()), "{:?}", frame.origin);
    // the same frame again, and the parts only read
    let before: Vec<_> = parts.iter().map(|b| b.record().clone()).collect();
    assert_eq!(joint(&parts, [&faces[0], &faces[1]]).unwrap(), frame);
    let after: Vec<_> = parts.iter().map(|b| b.record().clone()).collect();
    assert_eq!(after, before);
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
    // no direction runs both walls straight; the wall named is the one that strays from the
    // first direction tried, the cross product of a wall's normal and the outer faces'
    let named = [
        GrooveRefusal::GrooveFaceNotStraight { part: JointPart::Left, face: lf[0] },
        GrooveRefusal::GrooveFaceNotStraight { part: JointPart::Right, face: rf[0] },
    ];
    assert!(named.contains(&got), "{got:?}");
}

#[test]
fn a_right_outer_face_off_the_arrow_side_is_refused() {
    let left = plate(&[(-40.0, 0.0), (-40.0, -6.0), (-1.0, -6.0), (-1.0, 0.0)]);
    let right = plate(&[(1.0, 0.0), (1.0, -6.0), (40.0, -6.0), (40.0, 0.0)]);
    let (lf, rf) = (facing(&left, [1.0, 0.0, 0.0]), facing(&right, [-1.0, 0.0, 0.0]));
    // the right plate's bottom as its outer face: it meets the wall, but faces away from the
    // left plate's top
    let down = [0.0, -1.0, 0.0];
    let got = frame_of([pick(&left, &lf), pick_up(&right, &rf, down)]).unwrap_err();
    assert_eq!(got, GrooveRefusal::OuterFaceWrongSide { part: JointPart::Right, face: facing(&right, down)[0] });
    let got = groove_weld([&pick(&left, &lf), &pick_up(&right, &rf, down)], &flat()).unwrap_err();
    assert_eq!(got, GrooveRefusal::OuterFaceWrongSide { part: JointPart::Right, face: facing(&right, down)[0] });
}

/// brett19's point 3 on #22033 review 5: a right outer face square to the left one (a corner joint,
/// not covered here) is refused whatever the rounding, on the world axes and off them (the plane of
/// [`a_joint_off_the_world_axes_takes_its_plane_and_mirrors_within_the_allowance`]). The right
/// plate stands below the joint: its top is its groove face, its side facing away from the left
/// plate its outer face.
#[test]
fn a_right_outer_face_square_to_the_left_one_is_refused_on_and_off_the_world_axes() {
    let r5 = 5.0_f64.sqrt();
    let off = Plane { origin: Vec3::new(3.0, -2.0, 7.0), x: Vec3::new(2.0 / 3.0, 1.0 / 3.0, 2.0 / 3.0), y: Vec3::new(-1.0 / r5, 2.0 / r5, 0.0) };
    for plane in [Plane::XY, off] {
        let (ex, ey) = (plane.x, plane.y);
        let world = |a: f64, b: f64| [a * ex.x + b * ey.x, a * ex.y + b * ey.y, a * ex.z + b * ey.z];
        let left = swept(&plane, &[(-40.0, 0.0), (-40.0, -6.0), (-1.0, -6.0), (-1.0, 0.0)]);
        let right = swept(&plane, &[(1.0, -40.0), (7.0, -40.0), (7.0, 0.0), (1.0, 0.0)]);
        let (lf, rf) = (facing(&left, world(1.0, 0.0)), facing(&right, world(0.0, 1.0)));
        assert_eq!((lf.len(), rf.len()), (1, 1), "{lf:?} {rf:?}");
        let side = world(1.0, 0.0);
        let got = frame_of([pick_up(&left, &lf, world(0.0, 1.0)), pick_up(&right, &rf, side)]).unwrap_err();
        assert_eq!(got, GrooveRefusal::OuterFaceWrongSide { part: JointPart::Right, face: facing(&right, side)[0] });
    }
}

/// brett19's point A on #22033 review 6: a right plate turned up from the left one about its toe, so
/// the plates meet at 180° less the turn, on the world axes and off them. At 40° it is a butt
/// joint; at 45° (two equal components, decided by the margin, not the rounding) and at 50° it is
/// refused as a corner joint, not as a face off the arrow side.
#[test]
fn a_right_outer_face_turned_45_degrees_or_more_is_refused_as_a_corner_on_and_off_the_world_axes() {
    let r5 = 5.0_f64.sqrt();
    let off = Plane { origin: Vec3::new(3.0, -2.0, 7.0), x: Vec3::new(2.0 / 3.0, 1.0 / 3.0, 2.0 / 3.0), y: Vec3::new(-1.0 / r5, 2.0 / r5, 0.0) };
    for plane in [Plane::XY, off] {
        let (ex, ey) = (plane.x, plane.y);
        let world = |a: f64, b: f64| [a * ex.x + b * ey.x, a * ex.y + b * ey.y, a * ex.z + b * ey.z];
        let left = swept(&plane, &[(-40.0, 0.0), (-40.0, -6.0), (-1.0, -6.0), (-1.0, 0.0)]);
        let lf = facing(&left, world(1.0, 0.0));
        for degrees in [40.0, 45.0, 50.0_f64] {
            let (s, c) = degrees.to_radians().sin_cos();
            // the 6 mm plate's toe at (1, 0), its top running 40 along (c, s)
            let bottom = (1.0 + 6.0 * s, -6.0 * c);
            let right = swept(&plane, &[(1.0, 0.0), bottom, (bottom.0 + 40.0 * c, bottom.1 + 40.0 * s), (1.0 + 40.0 * c, 40.0 * s)]);
            let (rf, up) = (facing(&right, world(-c, -s)), world(-s, c));
            assert_eq!((lf.len(), rf.len(), facing(&right, up).len()), (1, 1, 1), "{degrees}: {rf:?}");
            let got = frame_of([pick_up(&left, &lf, world(0.0, 1.0)), pick_up(&right, &rf, up)]);
            if degrees < 45.0 {
                assert!(got.is_ok(), "{degrees}: {got:?}");
                continue;
            }
            let got = got.unwrap_err();
            assert_eq!(got, GrooveRefusal::OuterFaceWrongSide { part: JointPart::Right, face: facing(&right, up)[0] }, "{degrees}");
            assert!(got.to_string().contains("a corner joint, not built yet"), "{got}");
        }
    }
}

/// The pick of `faces` on `plate`, its outer face the one facing `up`.
fn pick_up<'a>(plate: &'a Body, faces: &'a [usize], up: [f64; 3]) -> GroovePick<'a> {
    GroovePick { part: plate, faces, outer: facing(plate, up)[0] }
}

/// The joint's frame of two picks.
fn frame_of(picks: [GroovePick<'_>; 2]) -> Result<Plane> {
    joint_frame([&picks[0], &picks[1]])
}

/// brett19's probe on #22033: the left plate's top runs past its toe (x = -1, z 0..120) on a tab
/// out to x = 200 (z 120..240), so the top face's centroid lies at x about 66, beyond the toe. The
/// frame is still signed from the toe: x out of the left part, +x, and the joint runs +z.
#[test]
fn an_outer_face_that_is_not_convex_still_signs_the_frame_from_its_toe() {
    let above = Plane { origin: Vec3::new(0.0, 0.0, 0.0), x: Vec3::new(1.0, 0.0, 0.0), y: Vec3::new(0.0, 0.0, 1.0) };
    let left = swept_by(&above, &[(-40.0, 0.0), (-1.0, 0.0), (-1.0, 120.0), (200.0, 120.0), (200.0, 240.0), (-40.0, 240.0)], 8.0);
    let right = swept_by(&above, &[(1.0, 0.0), (40.0, 0.0), (40.0, 110.0), (1.0, 110.0)], 8.0);
    let lf: Vec<usize> = facing(&left, [1.0, 0.0, 0.0])
        .into_iter()
        .filter(|&f| {
            left.face_square_level(f, Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 0.0, 0.0)).unwrap().is_some_and(|l| (l.level + 1.0).abs() < 1e-6)
        })
        .collect();
    let rf = facing(&right, [-1.0, 0.0, 0.0]);
    assert_eq!((lf.len(), rf.len()), (1, 1), "{lf:?} {rf:?}");
    let frame = joint(&[left, right], [&lf, &rf]).unwrap();
    assert!(near(frame.x, [1.0, 0.0, 0.0]) && near(frame.y, [0.0, 1.0, 0.0]), "{frame:?}");
    let n = normal(&frame);
    assert!(near(Vec3::new(n[0], n[1], n[2]), [0.0, 0.0, 1.0]), "{frame:?}");
}

fn line(a: (f64, f64), b: (f64, f64)) -> Seg2 {
    Seg2::Line { a: Vec2::new(a.0, a.1), b: Vec2::new(b.0, b.1) }
}

fn arc(center: (f64, f64), radius: f64, start: f64, sweep: f64) -> Seg2 {
    Seg2::Arc { center: Vec2::new(center.0, center.1), radius, start, sweep }
}

/// A round groove on 8 mm plates: each plate's wall a quarter circle of radius 8 from its toe on
/// the surface (x = -9 and 9) down to the root (x = -1 and 1). The only planes among the picked
/// faces are the two outer faces, which are parallel, so the walls' axis gives the direction.
fn round_groove() -> ([Body; 2], [Vec<usize>; 2]) {
    round_groove_of(arc((-1.0, 0.0), 8.0, -FRAC_PI_2, -FRAC_PI_2), arc((1.0, 0.0), 8.0, 0.0, -FRAC_PI_2))
}

/// [`round_groove`] with its walls `left_wall` (from (-1, -8) to (-9, 0)) and `right_wall` (from
/// (9, 0) to (1, -8)).
fn round_groove_of(left_wall: Seg2, right_wall: Seg2) -> ([Body; 2], [Vec<usize>; 2]) {
    let left = swept_loop(
        &Plane::XY,
        Loop2 { segs: vec![line((-40.0, 0.0), (-40.0, -8.0)), line((-40.0, -8.0), (-1.0, -8.0)), left_wall, line((-9.0, 0.0), (-40.0, 0.0))] },
        120.0,
    );
    let right = swept_loop(
        &Plane::XY,
        Loop2 { segs: vec![line((1.0, -8.0), (40.0, -8.0)), line((40.0, -8.0), (40.0, 0.0)), line((40.0, 0.0), (9.0, 0.0)), right_wall] },
        120.0,
    );
    let wall = |b: &Body| -> Vec<usize> { (0..b.record().faces.len()).filter(|&f| b.face_plane_normal(f).unwrap().is_none()).collect() };
    let (lf, rf) = (wall(&left), wall(&right));
    assert_eq!((lf.len(), rf.len()), (1, 1), "{lf:?} {rf:?}");
    ([left, right], [lf, rf])
}

#[test]
fn a_round_groove_runs_along_its_walls_axis() {
    let (parts, faces) = round_groove();
    let frame = joint(&parts, [&faces[0], &faces[1]]).unwrap();
    assert!(near(frame.x, [1.0, 0.0, 0.0]) && near(frame.y, [0.0, 1.0, 0.0]), "{frame:?}");
    let n = normal(&frame);
    assert!(near(Vec3::new(n[0], n[1], n[2]), [0.0, 0.0, 1.0]), "{frame:?}");
    assert!(within(frame.origin, [-9.0, 0.0, 60.0], points()), "{:?}", frame.origin);
}

/// brett19 on #22033 (reviews 4 and 5): the round groove of [`round_groove`] with its quarter
/// circles drawn as cubic Béziers (control points 0.5523 r along the tangents), so its walls are
/// extrusions of a free-form curve, not cylinders, whatever the kernel's fixes: the only planes are
/// the parallel outer faces and no face gives an axis, so the joint has no direction.
#[test]
fn a_round_groove_whose_walls_are_no_cylinders_has_no_direction() {
    let k = 0.5523 * 8.0;
    let cubic = |p: [(f64, f64); 4]| Seg2::Cubic {
        p0: Vec2::new(p[0].0, p[0].1),
        p1: Vec2::new(p[1].0, p[1].1),
        p2: Vec2::new(p[2].0, p[2].1),
        p3: Vec2::new(p[3].0, p[3].1),
    };
    let (parts, faces) = round_groove_of(
        cubic([(-1.0, -8.0), (-1.0 - k, -8.0), (-9.0, -k), (-9.0, 0.0)]),
        cubic([(9.0, 0.0), (9.0, -k), (1.0 + k, -8.0), (1.0, -8.0)]),
    );
    assert_eq!(joint(&parts, [&faces[0], &faces[1]]).unwrap_err(), GrooveRefusal::DirectionUndetermined);
}

/// brett19 on #22033: faces past the ops search's budget (#22066's sixty 10 mm plates, their tops
/// spread round y, each tilted off square to y by 0.9 of what the zone allows) give no proved
/// direction, and the joint is refused as undecided, not as a face that is not straight.
#[test]
fn faces_past_the_direction_search_s_budget_are_undecided() {
    let zone = zone();
    let alpha = 0.9 * zone / (2.0 * 10.0 * 2.0_f64.sqrt());
    let plates: Vec<(Body, usize)> = (0..60)
        .map(|k| {
            let (sin, cos) = (TAU * f64::from(k) / 60.0).sin_cos();
            let l = (1.0 + alpha * alpha).sqrt();
            let n = [cos / l, alpha / l, sin / l];
            let x = [-sin, 0.0, cos];
            let y = [n[1] * x[2] - n[2] * x[1], n[2] * x[0] - n[0] * x[2], n[0] * x[1] - n[1] * x[0]];
            let plane = Plane { origin: Vec3::new(0.0, 0.0, 0.0), x: Vec3::new(x[0], x[1], x[2]), y: Vec3::new(y[0], y[1], y[2]) };
            let plate = swept_by(&plane, &[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)], 1.0);
            let top = facing(&plate, n);
            assert_eq!(top.len(), 1, "{k}: {top:?}");
            (plate, top[0])
        })
        .collect();
    let picks: Vec<GroovePick<'_>> = plates.iter().map(|(p, f)| GroovePick { part: p, faces: &[], outer: *f }).collect();
    let faces: Vec<(JointPart, &GroovePick<'_>, usize)> = picks.iter().map(|p| (JointPart::Left, p, p.outer)).collect();
    assert_eq!(direction(&faces).unwrap_err(), GrooveRefusal::DirectionUndecided);
}

#[test]
fn an_outer_face_that_is_no_plane_is_refused() {
    let (parts, faces) = round_groove();
    let [left, right] = &parts;
    // the left plate's round wall as its outer face, its top as the groove face
    let top = facing(left, UP);
    let picks = [GroovePick { part: left, faces: &top, outer: faces[0][0] }, pick(right, &faces[1])];
    assert_eq!(frame_of(picks).unwrap_err(), GrooveRefusal::OuterFaceNotPlane { part: JointPart::Left, face: faces[0][0] });
}

/// The Y groove of [`y_groove`] laid in a plane off the world axes (x along (2, 1, 2) / 3, y along
/// (-1, 2, 0) / sqrt 5, through (3, -2, 7)): the frame is that plane's, and swapping the parts
/// mirrors x and the normal, within [`axes`].
#[test]
fn a_joint_off_the_world_axes_takes_its_plane_and_mirrors_within_the_allowance() {
    let r5 = 5.0_f64.sqrt();
    let (ex, ey) = ([2.0 / 3.0, 1.0 / 3.0, 2.0 / 3.0], [-1.0 / r5, 2.0 / r5, 0.0]);
    let plane = Plane { origin: Vec3::new(3.0, -2.0, 7.0), x: Vec3::new(ex[0], ex[1], ex[2]), y: Vec3::new(ey[0], ey[1], ey[2]) };
    let world = |a: f64, b: f64| -> [f64; 3] {
        let l = a.hypot(b);
        [(a * ex[0] + b * ey[0]) / l, (a * ex[1] + b * ey[1]) / l, (a * ex[2] + b * ey[2]) / l]
    };
    let t = 30.0_f64.to_radians().tan();
    let top = 0.5 + 7.0 * t;
    let left = swept(&plane, &[(-40.0, 0.0), (-40.0, -8.0), (-0.5, -8.0), (-0.5, -7.0), (-top, 0.0)]);
    let right = swept(&plane, &[(top, 0.0), (0.5, -7.0), (0.5, -8.0), (40.0, -8.0), (40.0, 0.0)]);
    let lf = [facing(&left, world(1.0, t)), facing(&left, world(1.0, 0.0))].concat();
    let rf = [facing(&right, world(-1.0, t)), facing(&right, world(-1.0, 0.0))].concat();
    assert_eq!((lf.len(), rf.len()), (2, 2), "{lf:?} {rf:?}");
    let ez = [ex[1] * ey[2] - ex[2] * ey[1], ex[2] * ey[0] - ex[0] * ey[2], ex[0] * ey[1] - ex[1] * ey[0]];
    let frame = frame_of([pick_up(&left, &lf, ey), pick_up(&right, &rf, ey)]).unwrap();
    assert!(near(frame.x, ex) && near(frame.y, ey), "{frame:?}");
    let n = normal(&frame);
    assert!(near(Vec3::new(n[0], n[1], n[2]), ez), "{frame:?}");
    let swapped = frame_of([pick_up(&right, &rf, ey), pick_up(&left, &lf, ey)]).unwrap();
    assert!(near(swapped.x, ex.map(|v| -v)) && near(swapped.y, ey), "{swapped:?}");
    let m = normal(&swapped);
    assert!(near(Vec3::new(m[0], m[1], m[2]), ez.map(|v| -v)), "{swapped:?}");
}

// ---- the weld ----

/// A body's box (the kernel's, SMLib's tight box, so a test allows it 1e-6 mm), its low and high
/// corners.
fn bounds(b: &Body) -> ([f64; 3], [f64; 3]) {
    let bb = b.exact_bounds().unwrap();
    ([bb.min.x, bb.min.y, bb.min.z], [bb.max.x, bb.max.y, bb.max.z])
}

fn flat() -> FillOptions {
    FillOptions { face: Contour::Flat, second_face: None, depth: None }
}

fn weld(parts: &[Body; 2], faces: &[Vec<usize>; 2], options: &FillOptions) -> Result<GrooveWeld> {
    groove_weld([&pick(&parts[0], &faces[0]), &pick(&parts[1], &faces[1])], options)
}

#[test]
fn a_y_groove_is_filled_flat_along_the_whole_joint() {
    let (parts, faces) = y_groove();
    let before: Vec<_> = parts.iter().map(|b| b.record().clone()).collect();
    let got = weld(&parts, &faces, &flat()).unwrap();
    // the groove's section: the V from the surface down to the root face, 1 wide, then the gap
    // between the root faces, 1 deep: (1 + (1 + 14 tan 30°)) / 2 · 7 + 1
    let area = 8.0 + 49.0 * 30.0_f64.to_radians().tan();
    let volume = got.body.volume_at(1e-9).unwrap();
    assert!((volume - 120.0 * area).abs() <= 1e-6 * volume, "{volume} != {}", 120.0 * area);
    assert!((got.fill.channel_depth - 8.0).abs() < 1e-9, "{:?}", got.fill);
    let (lo, hi) = bounds(&got.body);
    assert!(lo[2].abs() < 1e-6 && (hi[2] - 120.0).abs() < 1e-6, "{lo:?} {hi:?}");
    assert!(lo[1] > -8.0 - 1e-6 && hi[1] < 1e-6, "{lo:?} {hi:?}");
    // the parts are only read
    let after: Vec<_> = parts.iter().map(|b| b.record().clone()).collect();
    assert_eq!(after, before);
}

#[test]
fn a_fill_the_groove_cannot_take_is_refused_with_the_size_that_builds() {
    let (parts, faces) = y_groove();
    let deep = FillOptions { face: Contour::Concave { depth: 50.0 }, ..flat() };
    let got = weld(&parts, &faces, &deep).unwrap_err();
    let GrooveRefusal::Fill { fit: Some(fit), .. } = got else {
        panic!("not a fill refusal with a fit: {got:?}");
    };
    let fits = FillOptions { face: Contour::Concave { depth: fit }, ..flat() };
    assert!(weld(&parts, &faces, &fits).is_ok());
}

/// The stretches of a [`GrooveRefusal::SectionChanges`], checked against `want` (from, to).
fn stretches_of(got: &GrooveRefusal, want: &[(f64, f64)]) {
    let GrooveRefusal::SectionChanges { stretches } = got else {
        panic!("not a section refusal: {got:?}");
    };
    assert_eq!(stretches.len(), want.len(), "{stretches:?}");
    for (s, &(from, to)) in stretches.iter().zip(want) {
        assert!((s.from - from).abs() < 1e-9 && (s.to - to).abs() < 1e-9, "{stretches:?}");
    }
}

fn weld_over(parts: &[Body; 2], faces: &[Vec<usize>; 2], from: f64, to: f64) -> Result<GrooveWeld> {
    groove_weld_over([&pick(&parts[0], &faces[0]), &pick(&parts[1], &faces[1])], &flat(), Stretch { from, to })
}

#[test]
fn parts_of_different_length_are_refused_with_the_stretch_they_share_which_builds() {
    let (parts, faces) = y_groove_of(100.0);
    let got = weld(&parts, &faces, &flat()).unwrap_err();
    stretches_of(&got, &[(0.0, 100.0)]);
    assert!(got.to_string().contains("weld 0.00 to 100.00 mm"), "{got}");
    let area = 8.0 + 49.0 * 30.0_f64.to_radians().tan();
    let volume = weld_over(&parts, &faces, 0.0, 100.0).unwrap().body.volume_at(1e-9).unwrap();
    assert!((volume - 100.0 * area).abs() <= 1e-6 * volume, "{volume} != {}", 100.0 * area);
}

/// An I groove whose left plate's wall steps back 1 at z = 60 (a gap of 1, then 2) on 8 mm
/// plates, swept down from y = 0: the plates, and each one's wall faces (the left one's two).
fn stepped_gap() -> ([Body; 2], [Vec<usize>; 2]) {
    let above = Plane { origin: Vec3::new(0.0, 0.0, 0.0), x: Vec3::new(1.0, 0.0, 0.0), y: Vec3::new(0.0, 0.0, 1.0) };
    let left = swept_by(&above, &[(-40.0, 0.0), (-0.5, 0.0), (-0.5, 60.0), (-1.5, 60.0), (-1.5, 120.0), (-40.0, 120.0)], 8.0);
    let right = swept_by(&above, &[(0.5, 0.0), (40.0, 0.0), (40.0, 120.0), (0.5, 120.0)], 8.0);
    let (lf, rf) = (facing(&left, [1.0, 0.0, 0.0]), facing(&right, [-1.0, 0.0, 0.0]));
    assert_eq!((lf.len(), rf.len()), (2, 1), "{lf:?} {rf:?}");
    ([left, right], [lf, rf])
}

#[test]
fn a_gap_that_steps_along_the_joint_is_refused_with_its_two_stretches_each_of_which_builds() {
    let (parts, faces) = stepped_gap();
    let got = weld(&parts, &faces, &flat()).unwrap_err();
    stretches_of(&got, &[(0.0, 60.0), (60.0, 120.0)]);
    assert!(got.to_string().contains("weld 0.00 to 60.00 mm, 60.00 to 120.00 mm"), "{got}");
    // each stretch filled flat: the gap times the plates' 8 times its 60
    for (from, to, gap) in [(0.0, 60.0, 1.0), (60.0, 120.0, 2.0)] {
        let built = weld_over(&parts, &faces, from, to).unwrap();
        let volume = built.body.volume_at(1e-9).unwrap();
        assert!((volume - gap * 8.0 * 60.0).abs() <= 1e-6 * volume, "{from}: {volume}");
        let (lo, hi) = bounds(&built.body);
        assert!((lo[2] - from).abs() < 1e-6 && (hi[2] - to).abs() < 1e-6, "{lo:?} {hi:?}");
    }
    // a part of one stretch builds; a span across the step does not
    assert!(weld_over(&parts, &faces, 10.0, 50.0).is_ok());
    stretches_of(&weld_over(&parts, &faces, 50.0, 70.0).unwrap_err(), &[(0.0, 60.0), (60.0, 120.0)]);
}

/// brett19's point 1 on #22033 review 5: plate ends not square to the joint. An I groove (gap 1 on
/// 8 mm plates) whose right plate is swept along x from a parallelogram, so both its ends lean:
/// its top runs z 0 to 120, its bottom z 8 tan 30° to 120 - 8 tan 30°. Its wall's ends read no line
/// across the joint and its span is where its whole section is present, so the joint is refused
/// with that stretch (not with none), and the stretch builds: the gap times 8 times its length.
#[test]
fn plate_ends_not_square_to_the_joint_give_the_stretch_where_the_groove_is_whole() {
    let lean = 8.0 * 30.0_f64.to_radians().tan();
    let left = plate(&[(-40.0, 0.0), (-40.0, -8.0), (-0.5, -8.0), (-0.5, 0.0)]);
    let along_x = Plane { origin: Vec3::new(0.5, 0.0, 0.0), x: Vec3::new(0.0, 1.0, 0.0), y: Vec3::new(0.0, 0.0, 1.0) };
    let right = swept_by(&along_x, &[(-8.0, lean), (0.0, 0.0), (0.0, 120.0), (-8.0, 120.0 - lean)], 39.5);
    let (lf, rf) = (facing(&left, [1.0, 0.0, 0.0]), facing(&right, [-1.0, 0.0, 0.0]));
    assert_eq!((lf.len(), rf.len()), (1, 1), "{lf:?} {rf:?}");
    let (parts, faces) = ([left, right], [lf, rf]);
    let got = weld(&parts, &faces, &flat()).unwrap_err();
    stretches_of(&got, &[(lean, 120.0 - lean)]);
    let built = weld_over(&parts, &faces, lean, 120.0 - lean).unwrap();
    let volume = built.body.volume_at(1e-9).unwrap();
    let want = 8.0 * (120.0 - 2.0 * lean);
    assert!((volume - want).abs() <= 1e-6 * volume, "{volume} != {want}");
}

/// brett19's point B on #22033 review 6: the plate of
/// [`plate_ends_not_square_to_the_joint_give_the_stretch_where_the_groove_is_whole`] with only its
/// near end leaning: its top runs z 0 to 120, its bottom z 8 tan 30° to 120. Its wall's square end
/// is the one line read across the joint ([`planar_edges`]), so its section comes from that end, not
/// from the line between its runs; the joint is refused with the stretch where the wall is whole,
/// which builds.
#[test]
fn a_plate_with_one_end_square_reads_its_section_from_that_end() {
    let lean = 8.0 * 30.0_f64.to_radians().tan();
    let left = plate(&[(-40.0, 0.0), (-40.0, -8.0), (-0.5, -8.0), (-0.5, 0.0)]);
    let along_x = Plane { origin: Vec3::new(0.5, 0.0, 0.0), x: Vec3::new(0.0, 1.0, 0.0), y: Vec3::new(0.0, 0.0, 1.0) };
    let right = swept_by(&along_x, &[(-8.0, lean), (0.0, 0.0), (0.0, 120.0), (-8.0, 120.0)], 39.5);
    let (lf, rf) = (facing(&left, [1.0, 0.0, 0.0]), facing(&right, [-1.0, 0.0, 0.0]));
    assert_eq!((lf.len(), rf.len()), (1, 1), "{lf:?} {rf:?}");
    let (parts, faces) = ([left, right], [lf, rf]);
    let got = weld(&parts, &faces, &flat()).unwrap_err();
    stretches_of(&got, &[(lean, 120.0)]);
    let built = weld_over(&parts, &faces, lean, 120.0).unwrap();
    let volume = built.body.volume_at(1e-9).unwrap();
    let want = 8.0 * (120.0 - lean);
    assert!((volume - want).abs() <= 1e-6 * volume, "{volume} != {want}");
}

/// brett19's point D on #22033 review 6: a plate swept from a parallelogram whose top runs z 0 to 10
/// and whose bottom runs z 12 to 22. Its wall is a plane straight along z, but its runs never
/// overlap, so it has its whole section nowhere: refused as such, not as a face not straight.
#[test]
fn a_wall_whose_runs_never_overlap_is_refused_as_never_whole() {
    let left = plate(&[(-40.0, 0.0), (-40.0, -8.0), (-0.5, -8.0), (-0.5, 0.0)]);
    let along_x = Plane { origin: Vec3::new(0.5, 0.0, 0.0), x: Vec3::new(0.0, 1.0, 0.0), y: Vec3::new(0.0, 0.0, 1.0) };
    let right = swept_by(&along_x, &[(-8.0, 12.0), (0.0, 0.0), (0.0, 10.0), (-8.0, 22.0)], 39.5);
    let (lf, rf) = (facing(&left, [1.0, 0.0, 0.0]), facing(&right, [-1.0, 0.0, 0.0]));
    assert_eq!((lf.len(), rf.len()), (1, 1), "{lf:?} {rf:?}");
    let got = weld(&[left, right], &[lf, rf.clone()], &flat()).unwrap_err();
    assert_eq!(got, GrooveRefusal::GrooveFaceNeverWhole { part: JointPart::Right, face: rf[0] });
}

/// brett19's point C on #22033 review 6: the round groove of [`round_groove`] built, so its arc walls
/// go through [`face_read`] and [`wall_of`] and its walls' tangent meeting with the plates' bottoms
/// at (∓1, -8) reads within [`ALLOWANCE`]. Filled flat: the 2 by 8 gap and two quarter circles of
/// radius 8, 16 + 32π, along the 120.
#[test]
fn a_round_groove_is_filled_flat_along_its_arc_walls() {
    let (parts, faces) = round_groove();
    let got = weld(&parts, &faces, &flat()).unwrap();
    let want = 120.0 * (16.0 + 32.0 * std::f64::consts::PI);
    let volume = got.body.volume_at(1e-9).unwrap();
    assert!((volume - want).abs() <= 1e-6 * volume, "{volume} != {want}");
    assert!((got.fill.channel_depth - 8.0).abs() < 1e-9, "{:?}", got.fill);
}

/// brett19's point 5 on #22033 review 5: a pick off the chain (the left plate's bottom with its
/// bevel, which share no edge) leaves no piece of the joint with one section; the refusal names it,
/// not a joint with no stretch.
#[test]
fn a_pick_off_the_chain_is_refused_naming_it() {
    let (parts, faces) = y_groove();
    let bottom = facing(&parts[0], [0.0, -1.0, 0.0])[0];
    let picks = [vec![faces[0][0], bottom], faces[1].clone()];
    let got = weld(&parts, &picks, &flat()).unwrap_err();
    assert_eq!(got, GrooveRefusal::PicksNotOneChain { part: JointPart::Left, face: bottom });
}

#[test]
fn a_concave_face_below_the_plate_surface_is_a_warning_in_weld_words() {
    let (parts, faces) = y_groove();
    let got = weld(&parts, &faces, &FillOptions { face: Contour::Concave { depth: 0.5 }, ..flat() }).unwrap();
    let notes = got.notes();
    let warning = notes.iter().find(|n| n.warning).unwrap_or_else(|| panic!("no warning: {notes:?}"));
    assert!(warning.text.contains("ISO 6520-1 no. 511") && warning.text.contains("ISO 5817"), "{warning:?}");
    assert!(warning.text.contains("0.50 mm below the left plate's surface"), "{warning:?}");
}

#[test]
fn a_depth_past_the_root_is_full_penetration() {
    let (parts, faces) = y_groove();
    let got = weld(&parts, &faces, &FillOptions { depth: Some(20.0), ..flat() }).unwrap();
    let notes = got.notes();
    assert!(notes.iter().any(|n| !n.warning && n.text.contains("full penetration")), "{notes:?}");
}

#[test]
fn a_fill_refusal_says_what_is_wrong_in_weld_words_and_the_size_that_builds() {
    let (parts, faces) = y_groove();
    let got = weld(&parts, &faces, &FillOptions { face: Contour::Concave { depth: 50.0 }, ..flat() }).unwrap_err();
    let text = got.to_string();
    assert!(text.starts_with("the groove cannot be filled as asked: a concave weld face 50.00 mm deep"), "{text}");
    assert!(text.ends_with("mm builds"), "{text}");
}

/// brett19 on #22033: the Y groove's frame tests cannot show which way the direction was found
/// (the plane pair's is within a few ulps of every other), so the faces `joint_frame` reads are
/// asked directly: the two plane normals furthest from parallel give it.
#[test]
fn a_y_groove_takes_its_direction_from_a_plane_pair() {
    use solidcraft_kernel::{StraightBy, straight_direction};
    let (parts, faces) = y_groove();
    let mut reads: Vec<(&Body, usize)> = Vec::new();
    for (part, f) in parts.iter().zip(&faces) {
        let outer = facing(part, UP)[0];
        reads.extend(f.iter().chain([&outer]).map(|&face| (part, face)));
    }
    let got = straight_direction(&reads).unwrap();
    assert!(matches!(got, Straight::Along { by: StraightBy::Planes(..), .. }), "{got:?}");
}

/// The I groove's left plate (gap 1 with a right wall at x = 0.5, 8 mm plates).
fn i_groove_left() -> Body {
    plate(&[(-40.0, 0.0), (-40.0, -8.0), (-0.5, -8.0), (-0.5, 0.0)])
}

/// A right plate swept 39.5 along x from x = 0.5, from the region of `outline` (y, z) less `holes`:
/// its wall is the face at x = 0.5, facing -x.
fn along_x(outline: &[(f64, f64)], holes: &[&[(f64, f64)]]) -> Body {
    let plane = Plane { origin: Vec3::new(0.5, 0.0, 0.0), x: Vec3::new(0.0, 1.0, 0.0), y: Vec3::new(0.0, 0.0, 1.0) };
    let poly = |c: &[(f64, f64)]| Loop2::polygon(&c.iter().map(|&(x, y)| Vec2::new(x, y)).collect::<Vec<_>>());
    let region = Region2 { outer: poly(outline), holes: holes.iter().map(|&h| poly(h)).collect() };
    extrude_planar_regions(&plane, &[region], 0.0, 39.5).unwrap()
}

/// The I groove with `right` as its right plate: the parts and each one's wall.
fn i_groove_with(right: Body) -> ([Body; 2], [Vec<usize>; 2]) {
    let left = i_groove_left();
    let (lf, rf) = (facing(&left, [1.0, 0.0, 0.0]), facing(&right, [-1.0, 0.0, 0.0]));
    assert_eq!((lf.len(), rf.len()), (1, 1), "{lf:?} {rf:?}");
    ([left, right], [lf, rf])
}

/// CodeRabbit on #22033: a concave root-side face below the plates' root-side surfaces is root
/// concavity (ISO 6520-1 no. 515), still a warning, but not an incompletely filled groove (511)
/// and not said to fail every quality level.
#[test]
fn a_concave_root_side_face_is_root_concavity_not_an_incompletely_filled_groove() {
    let (parts, faces) = y_groove();
    let through = FillOptions { face: Contour::Flat, second_face: Some(Contour::Concave { depth: 0.3 }), depth: Some(20.0) };
    let notes = weld(&parts, &faces, &through).unwrap().notes();
    let root = notes.iter().find(|n| n.text.contains("root-side weld face dips")).unwrap_or_else(|| panic!("no root-side note: {notes:?}"));
    assert!(root.warning && root.text.contains("root concavity (ISO 6520-1 no. 515)"), "{root:?}");
    assert!(notes.iter().all(|n| !n.text.contains("no. 511") && !n.text.contains("no quality level")), "{notes:?}");
}

/// CodeRabbit on #22033 (major): an I groove whose right plate has a square hole through it along
/// x, 50 to 70 along the joint: its wall has a second loop, so it is not whole all along its span
/// and is refused, not swept across the hole.
#[test]
fn a_wall_with_a_hole_inside_is_refused_as_interrupted() {
    let right = along_x(&[(-8.0, 0.0), (0.0, 0.0), (0.0, 120.0), (-8.0, 120.0)], &[&[(-5.0, 50.0), (-5.0, 70.0), (-3.0, 70.0), (-3.0, 50.0)]]);
    let (parts, faces) = i_groove_with(right);
    let got = weld(&parts, &faces, &flat()).unwrap_err();
    assert_eq!(got, GrooveRefusal::GrooveFaceInterrupted { part: JointPart::Right, face: faces[1][0] });
}

/// The same with a notch 2 deep cut into the right plate's top, 40 to 60 along the joint: one loop,
/// but the wall's toe run has a gap, so it is refused as interrupted.
#[test]
fn a_wall_with_a_notch_in_its_edge_is_refused_as_interrupted() {
    let right = along_x(&[(-8.0, 0.0), (0.0, 0.0), (0.0, 40.0), (-2.0, 40.0), (-2.0, 60.0), (0.0, 60.0), (0.0, 120.0), (-8.0, 120.0)], &[]);
    let (parts, faces) = i_groove_with(right);
    let got = weld(&parts, &faces, &flat()).unwrap_err();
    assert_eq!(got, GrooveRefusal::GrooveFaceInterrupted { part: JointPart::Right, face: faces[1][0] });
}

/// CodeRabbit on #22033: the right wall's square end at z = 0 is two collinear edges, so its reads
/// disagree; the wall is a plane, so its section is the line between its runs and the whole joint
/// builds: the gap 1 times 8 times 120.
#[test]
fn a_square_end_read_in_two_pieces_still_gives_the_wall_its_section() {
    let right = along_x(&[(-8.0, 0.0), (-4.0, 0.0), (0.0, 0.0), (0.0, 120.0), (-8.0, 120.0)], &[]);
    let (parts, faces) = i_groove_with(right);
    let volume = weld(&parts, &faces, &flat()).unwrap().body.volume_at(1e-9).unwrap();
    assert!((volume - 960.0).abs() <= 1e-6 * volume, "{volume} != 960");
}

/// CodeRabbit on #22033: the parts of different length, welded over their stretch asked half a zone
/// tolerance past each end: the weld is held to the stretch, 0 to 100, not past the plates' faces.
#[test]
fn a_stretch_asked_a_hair_past_its_ends_is_held_to_them() {
    let (parts, faces) = y_groove_of(100.0);
    let z = zone();
    let built = weld_over(&parts, &faces, -z / 2.0, 100.0 + z / 2.0).unwrap();
    let (lo, hi) = bounds(&built.body);
    assert!(lo[2] >= -1e-6 && hi[2] <= 100.0 + 1e-6, "{lo:?} {hi:?}");
}

/// CodeRabbit on #22033 (major): two halves of one circle have the same ends, centre and radius, but
/// are not one section; an arc and itself the other way round are.
#[test]
fn two_halves_of_one_circle_are_not_one_section() {
    let half = |start: f64| Seg2::Arc { center: Vec2::new(0.0, 0.0), radius: 8.0, start, sweep: std::f64::consts::PI };
    let (upper, lower) = (half(0.0), half(std::f64::consts::PI));
    assert!(!same(&upper, &lower, 1e-9));
    assert!(same(&upper, &upper, 1e-9) && same(&upper, &reversed(&upper), 1e-9));
}
