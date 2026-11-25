//! Groove welds: each part's groove faces read as one chain from the toe on a Y groove and an I
//! groove, the same chain whatever the order of the picks, the parts only read, and every typed
//! refusal of picks that are not one chain from the toe; the joint's frame on both grooves, the
//! same whatever the order of the picks, mirrored when the parts swap (also off the world axes),
//! signed by the toe on an outer face that is not convex, along the axis of a round groove, the
//! parts only read, and a plate not straight along the joint or an outer face that is no plane
//! refused. Ported from the second groove PR's first slice (solidcore
//! `weld::groove::joint`, never pushed) onto ops reads (#18500).

use std::f64::consts::FRAC_PI_2;

use solidcore_record::{Loop2, Plane, Region2, Seg2, Vec2, Vec3};
use solidcraft_kernel::services::prism::extrude_planar_regions;
use solidcraft_kernel::services::{ExactBody as Body, Order, session, sweep};

use super::*;

/// A plate: the polygon `corners` (x, y, counter-clockwise) swept 120 along z, and its order.
fn plate(corners: &[(f64, f64)]) -> (Body, Order) {
    swept(&Plane::XY, corners)
}

/// The polygon `corners` in `plane`, swept 120 along its normal, and its order.
fn swept(plane: &Plane, corners: &[(f64, f64)]) -> (Body, Order) {
    swept_by(plane, corners, 120.0)
}

/// The polygon `corners` in `plane`, swept `length` along its normal, and its order.
fn swept_by(plane: &Plane, corners: &[(f64, f64)], length: f64) -> (Body, Order) {
    let pts: Vec<Vec2> = corners.iter().map(|&(x, y)| Vec2::new(x, y)).collect();
    swept_loop(plane, Loop2::polygon(&pts), length)
}

/// The region inside `outline` in `plane`, swept `length` along its normal, and its order. The
/// kernel runs with the app's fixes, as it does once the app has made a body: with them an arc
/// swept square to its plane is a cylinder (`linear_sweep_arc_is_cylinder`), without them an
/// extrusion, which gives no direction.
fn swept_loop(plane: &Plane, outline: Loop2, length: f64) -> (Body, Order) {
    session::set_fixes(solidcraft_kernel::smlib_trace::fixes()).unwrap();
    let region = Region2 { outer: outline, holes: Vec::new() };
    let body = extrude_planar_regions(plane, &[region], 0.0, length).unwrap();
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

/// The farthest any point of a test's parts lies from its frame's origin, rounded up: 300 mm (the
/// L-shaped plate's far corner is about 270 mm from its toe; the 40 by 120 plates' about 75 mm).
const REACH: f64 = 300.0;

/// How far each of a frame's unit axes may be off what the test expects, as a vector length: half
/// the zone tolerance over `√3 REACH`. A point `o + a x + b y + c n` with `|(a, b, c)| <= L` moves
/// with the axes by at most `L sqrt(|Δx|² + |Δy|² + |Δn|²) <= √3 L axes()`, `zone / 2` for
/// `L <= REACH`, and with the origin by at most [`points`], the other half: so every point within
/// [`REACH`] of the origin is within the zone tolerance ([`sweep::zone_tolerance`], the kernel's
/// own bound for two points being the same), whatever the axes' errors (brett19 on #22033).
fn axes() -> f64 {
    sweep::zone_tolerance().unwrap() / (2.0 * 3.0_f64.sqrt() * REACH)
}

/// How far the frame's origin may be off, as a vector length: half the zone tolerance (the other
/// half is the axes', [`axes`]).
fn points() -> f64 {
    sweep::zone_tolerance().unwrap() / 2.0
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
    let before: Vec<_> = parts.iter().map(|(b, _)| b.to_record().unwrap().0).collect();
    assert_eq!(joint(&parts, [&faces[0], &faces[1]]).unwrap(), frame);
    let after: Vec<_> = parts.iter().map(|(b, _)| b.to_record().unwrap().0).collect();
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
    assert!(matches!(got, GrooveRefusal::GrooveFaceNotStraight { .. }), "{got:?}");
}

/// The pick of `faces` on `plate`, its outer face the one facing `up`.
fn pick_up<'a>(plate: &'a (Body, Order), faces: &'a [usize], up: [f64; 3]) -> GroovePick<'a> {
    GroovePick { part: &plate.0, order: &plate.1, faces, outer: facing(plate, up)[0] }
}

/// The joint's frame of two picks.
fn frame_of(picks: [GroovePick<'_>; 2]) -> Result<Plane> {
    let chains = [chain(&picks[0], JointPart::Left)?, chain(&picks[1], JointPart::Right)?];
    joint_frame([&picks[0], &picks[1]], [&chains[0], &chains[1]])
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
        .filter(|&f| (left.0.face_centroid(&left.1, f, 1e-8).unwrap().centroid[0] + 1.0).abs() < 1e-6)
        .collect();
    let rf = facing(&right, [-1.0, 0.0, 0.0]);
    assert_eq!((lf.len(), rf.len()), (1, 1), "{lf:?} {rf:?}");
    let top = facing(&left, UP)[0];
    assert!(left.0.face_centroid(&left.1, top, 1e-8).unwrap().centroid[0] > 60.0);
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
fn round_groove() -> ([(Body, Order); 2], [Vec<usize>; 2]) {
    let left = swept_loop(
        &Plane::XY,
        Loop2 {
            segs: vec![
                line((-40.0, 0.0), (-40.0, -8.0)),
                line((-40.0, -8.0), (-1.0, -8.0)),
                arc((-1.0, 0.0), 8.0, -FRAC_PI_2, -FRAC_PI_2),
                line((-9.0, 0.0), (-40.0, 0.0)),
            ],
        },
        120.0,
    );
    let right = swept_loop(
        &Plane::XY,
        Loop2 {
            segs: vec![
                line((1.0, -8.0), (40.0, -8.0)),
                line((40.0, -8.0), (40.0, 0.0)),
                line((40.0, 0.0), (9.0, 0.0)),
                arc((1.0, 0.0), 8.0, 0.0, -FRAC_PI_2),
            ],
        },
        120.0,
    );
    let wall = |(b, o): &(Body, Order)| -> Vec<usize> { (0..o.faces.len()).filter(|&f| b.face_plane_normal(o, f).unwrap().is_none()).collect() };
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

#[test]
fn an_outer_face_that_is_no_plane_is_refused() {
    let (parts, faces) = round_groove();
    let [left, right] = &parts;
    // the left plate's round wall as its outer face, its top as the groove face
    let top = facing(left, UP);
    let picks = [GroovePick { part: &left.0, order: &left.1, faces: &top, outer: faces[0][0] }, pick(right, &faces[1])];
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

/// brett19 on #22033: the Y groove's frame tests cannot show which way the direction was found
/// (the plane pair's is within a few ulps of every other), so the faces `joint_frame` reads are
/// asked directly: the two plane normals furthest from parallel give it.
#[test]
fn a_y_groove_takes_its_direction_from_a_plane_pair() {
    use solidcraft_kernel::services::measure::{StraightBy, straight_direction};
    let (parts, faces) = y_groove();
    let mut reads: Vec<(&Body, &Order, usize)> = Vec::new();
    for (part, f) in parts.iter().zip(&faces) {
        let outer = facing(part, UP)[0];
        reads.extend(f.iter().chain([&outer]).map(|&face| (&part.0, &part.1, face)));
    }
    let got = straight_direction(&reads).unwrap();
    assert!(matches!(got, Straight::Along { by: StraightBy::Planes(..), .. }), "{got:?}");
}
