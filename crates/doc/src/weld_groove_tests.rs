//! Groove welds: each part's groove faces read as one chain from the toe on a Y groove and an I
//! groove, the same chain whatever the order of the picks, the parts only read, and every typed
//! refusal of picks that are not one chain from the toe. Ported from the second groove PR's
//! first slice (solidcore `weld::groove::joint`, never pushed) onto ops reads (#18500).

use solidcore_ops::prism::extrude_planar_regions;
use solidcore_ops::{Body, Order};
use solidcore_record::{Loop2, Plane, Region2, Vec2};

use super::*;

/// A plate: the polygon `corners` (x, y, counter-clockwise) swept 120 along z, and its order.
fn plate(corners: &[(f64, f64)]) -> (Body, Order) {
    let pts: Vec<Vec2> = corners.iter().map(|&(x, y)| Vec2::new(x, y)).collect();
    let region = Region2 { outer: Loop2::polygon(&pts), holes: Vec::new() };
    let body = extrude_planar_regions(&Plane::XY, &[region], 0.0, 120.0).unwrap();
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
