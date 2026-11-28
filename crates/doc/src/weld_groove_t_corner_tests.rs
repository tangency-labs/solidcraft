//! T and corner joints: a beveled corner, outside and flush, and a beveled T on a through plate
//! longer than the stem, each filled flat to its groove's closed-form volume whichever part is
//! passed first, the parts only read, and an outer face facing into the corner and a stem
//! standing off its through plate refused; a 135° corner, and a stem on a chamfered through
//! plate's edge, filled; a fillet over the T's groove (Q23) as one weld, its legs past the groove
//! or short of it, flat and convex, and a leg off its face and a fillet with no groove under it
//! refused.

use super::*;

/// The pick of `faces` on `plate`, its outer face the one facing `outer`.
fn picked<'a>(plate: &'a Body, faces: &'a [usize], outer: [f64; 3]) -> GroovePick<'a> {
    GroovePick { part: plate, faces, outer: facing(plate, outer)[0] }
}

/// The volume of the weld filling the groove between `picks` flat.
fn volume(picks: [&GroovePick<'_>; 2]) -> f64 {
    groove_weld(picks, &flat()).unwrap().body.volume_at(1e-9).unwrap()
}

const RIGHT: [f64; 3] = [1.0, 0.0, 0.0];

#[test]
fn a_beveled_corner_joint_is_filled_to_its_groove() {
    // outside corner: a lying plate 8 thick, its square end at x = 0, on an upright plate 8
    // thick whose top is beveled 45° from its outer face x = 8 down to the lying plate's
    // corner; the groove is the triangle (0, 0), (8, 0), (0, -8), outer faces at 90°
    let lying = plate(&[(-40.0, 0.0), (-40.0, -8.0), (0.0, -8.0), (0.0, 0.0)]);
    let upright = plate(&[(0.0, -48.0), (8.0, -48.0), (8.0, 0.0), (0.0, -8.0)]);
    let (lf, uf) = (facing(&lying, RIGHT), facing(&upright, unit(-1.0, 1.0)));
    let (l, u) = (picked(&lying, &lf, UP), picked(&upright, &uf, RIGHT));
    // flush corner: a lying plate beveled 45° (root face 2) against an upright plate's side,
    // tops flush; the upright's side runs on 42 past the groove's root, where the weld ends:
    // the triangle (-6, 0), (0, 0), (0, -6)
    let beveled = plate(&[(-40.0, 0.0), (-40.0, -8.0), (0.0, -8.0), (0.0, -6.0), (-6.0, 0.0)]);
    let side = plate(&[(0.0, -48.0), (8.0, -48.0), (8.0, 0.0), (0.0, 0.0)]);
    let (bf, sf) = (facing(&beveled, unit(1.0, 1.0)), facing(&side, [-1.0, 0.0, 0.0]));
    let (b, s) = (picked(&beveled, &bf, UP), picked(&side, &sf, UP));
    for (name, picks, area) in
        [("outside", [&l, &u], 32.0), ("outside swapped", [&u, &l], 32.0), ("flush", [&b, &s], 18.0), ("flush swapped", [&s, &b], 18.0)]
    {
        let got = volume(picks);
        assert!((got - 120.0 * area).abs() <= 1e-6 * got, "{name}: {got} != {}", 120.0 * area);
    }
    // the upright's inside as its outer face: it meets the bevel at the groove's root, but faces
    // into the joint
    let inside = picked(&upright, &uf, [-1.0, 0.0, 0.0]);
    let got = groove_weld([&l, &inside], &flat()).unwrap_err();
    assert_eq!(got, GrooveRefusal::OuterFaceWrongSide { part: JointPart::Right, face: inside.outer });
}

/// A beveled T: a through plate 8 thick and 200 long, from z = -40, under a stem 10 thick and
/// 120 long whose left side is beveled 45° from 6 up down to the plate, its root face 4 on the
/// plate; `lift` raises the stem off the plate. The through plate, the stem, and the stem's
/// bevel.
fn t_joint(lift: f64) -> (Body, Body, Vec<usize>) {
    let from = Plane { origin: Vec3::new(0.0, 0.0, -40.0), ..Plane::XY };
    let through = swept_by(&from, &[(-40.0, -8.0), (40.0, -8.0), (40.0, 0.0), (-40.0, 0.0)], 200.0);
    let stem = plate(&[(6.0, lift), (10.0, lift), (10.0, 60.0), (0.0, 60.0), (0.0, 6.0 + lift)]);
    let bevel = facing(&stem, unit(-1.0, -1.0));
    assert_eq!(bevel.len(), 1);
    (through, stem, bevel)
}

#[test]
fn a_beveled_t_joint_is_filled_flush_with_the_stem_along_the_stem_only() {
    let (through, stem, bevel) = t_joint(0.0);
    let before: Vec<_> = [&through, &stem].iter().map(|b| b.record().clone()).collect();
    let top = facing(&through, UP);
    let (t, s) = (picked(&through, &top, UP), picked(&stem, &bevel, [-1.0, 0.0, 0.0]));
    // the groove from the stem's toe (0, 6) flush with its side down to the plate at (0, 0), and
    // along the plate to the root (6, 0)
    for picks in [[&s, &t], [&t, &s]] {
        let got = groove_weld(picks, &flat()).unwrap();
        let volume = got.body.volume_at(1e-9).unwrap();
        assert!((volume - 18.0 * 120.0).abs() <= 1e-6 * volume, "{volume}");
        let (lo, hi) = bounds(&got.body);
        assert!(lo[2].abs() < 1e-6 && (hi[2] - 120.0).abs() < 1e-6, "{lo:?} {hi:?}");
    }
    // the parts are only read
    let after: Vec<_> = [&through, &stem].iter().map(|b| b.record().clone()).collect();
    assert_eq!(after, before);
}

/// brett19's point 2 on #22504: a corner of 135° (an octagonal frame's), the second outer face
/// at 45° to both of the frame's axes. A lying plate 8 thick, its square end at x = 0, and a plate
/// 4√2 thick running down at 45° from it, its square end from (4, -4) down to the lying plate's
/// corner (0, -8): the triangle (0, 0), (4, -4), (0, -8), 16 mm² over 120, whichever part is
/// first.
#[test]
fn a_135_degree_corner_is_filled_whichever_part_is_first() {
    let lying = plate(&[(-40.0, 0.0), (-40.0, -8.0), (0.0, -8.0), (0.0, 0.0)]);
    let sloped = plate(&[(0.0, -8.0), (28.0, -36.0), (32.0, -32.0), (4.0, -4.0)]);
    let (lf, sf) = (facing(&lying, RIGHT), facing(&sloped, unit(-1.0, 1.0)));
    assert_eq!((lf.len(), sf.len()), (1, 1), "{lf:?} {sf:?}");
    let (l, s) = (picked(&lying, &lf, UP), picked(&sloped, &sf, unit(1.0, 1.0)));
    for picks in [[&l, &s], [&s, &l]] {
        let got = volume(picks);
        assert!((got - 16.0 * 120.0).abs() <= 1e-6 * got, "{got}");
    }
}

/// brett19's point 3 on #22504: the beveled T of [`t_joint`] on a through plate whose top ends at
/// x = 0 in a 45° chamfer, right under the stem's side: the ray from the stem's toe meets the top
/// and the chamfer at one point, (0, 0). The top counts whatever the record's face order.
#[test]
fn a_stem_flush_with_a_chamfered_edge_of_its_through_plate_is_filled() {
    let (_, stem, bevel) = t_joint(0.0);
    let from = Plane { origin: Vec3::new(0.0, 0.0, -40.0), ..Plane::XY };
    let through = swept_by(&from, &[(-8.0, -8.0), (40.0, -8.0), (40.0, 0.0), (0.0, 0.0)], 200.0);
    let top = facing(&through, UP);
    let (t, s) = (picked(&through, &top, UP), picked(&stem, &bevel, [-1.0, 0.0, 0.0]));
    for picks in [[&s, &t], [&t, &s]] {
        let got = volume(picks);
        assert!((got - 18.0 * 120.0).abs() <= 1e-6 * got, "{got}");
    }
}

#[test]
fn a_stem_standing_off_its_through_plate_is_refused() {
    let (through, stem, bevel) = t_joint(1.0);
    let top = facing(&through, UP);
    let (t, s) = (picked(&through, &top, UP), picked(&stem, &bevel, [-1.0, 0.0, 0.0]));
    assert_eq!(groove_weld([&t, &s], &flat()).unwrap_err(), GrooveRefusal::StemOffThroughPart { part: JointPart::Right });
}

// ---- a fillet over a T joint's groove (Q23) ----

/// The beveled T of [`t_joint`] standing on its plate: the through plate's and the stem's picks.
fn t_picks<'a>(through: &'a Body, stem: &'a Body, top: &'a [usize], bevel: &'a [usize]) -> [GroovePick<'a>; 2] {
    [picked(through, top, UP), picked(stem, bevel, [-1.0, 0.0, 0.0])]
}

fn legs(stem: f64, through: f64) -> TFillet {
    TFillet { stem, through }
}

#[test]
fn a_fillet_over_a_t_joints_groove_is_one_weld_with_legs_past_the_groove_or_short_of_it() {
    let (through, stem, bevel) = t_joint(0.0);
    let before: Vec<_> = [&through, &stem].iter().map(|b| b.record().clone()).collect();
    let top = facing(&through, UP);
    let [t, s] = t_picks(&through, &stem, &top, &bevel);
    // the groove's 18 under the fillet's z1 z2 / 2, legs from C at the origin: up the stem's side
    // past the groove's 6, or short of it (the groove flush with the stem's side from its toe
    // down to the fillet's, Q101)
    for ((z1, z2), area) in [((8.0, 8.0), 50.0), ((4.0, 4.0), 26.0), ((4.0, 10.0), 38.0)] {
        for picks in [[&s, &t], [&t, &s]] {
            let got = groove_fillet_weld(picks, &flat(), legs(z1, z2)).unwrap();
            let volume = got.body.volume_at(1e-9).unwrap();
            assert!((volume - 120.0 * area).abs() <= 1e-6 * volume, "{z1} {z2}: {volume} != {}", 120.0 * area);
            // from the fillet's toe on the plate (x = -z2) to the root (x = 6), from the plate up
            // to the higher of the fillet's toe and the stem's toe, along the stem only
            let (lo, hi) = bounds(&got.body);
            let want = ([-z2, 0.0, 0.0], [6.0, f64::max(z1, 6.0), 120.0]);
            assert!(lo.iter().zip(want.0).chain(hi.iter().zip(want.1)).all(|(&a, b)| (a - b).abs() < 1e-6), "{z1} {z2}: {lo:?} {hi:?}");
        }
    }
    // the parts are only read
    let after: Vec<_> = [&through, &stem].iter().map(|b| b.record().clone()).collect();
    assert_eq!(after, before);
}

#[test]
fn a_convex_fillet_over_a_t_joints_groove_adds_its_crest() {
    let (through, stem, bevel) = t_joint(0.0);
    let top = facing(&through, UP);
    let [t, s] = t_picks(&through, &stem, &top, &bevel);
    // the crest 1 high over the chord 8√2 between the toes: a circular segment of radius r
    let (c, h) = (128.0_f64.sqrt(), 1.0);
    let r = (c * c / 4.0 + h * h) / (2.0 * h);
    let crest = r * r * (1.0 - h / r).acos() - (r - h) * (h * (2.0 * r - h)).sqrt();
    let convex = FillOptions { face: Contour::Convex { height: h }, ..flat() };
    for picks in [[&s, &t], [&t, &s]] {
        let volume = groove_fillet_weld(picks, &convex, legs(8.0, 8.0)).unwrap().body.volume_at(1e-9).unwrap();
        let want = 120.0 * (50.0 + crest);
        assert!((volume - want).abs() <= 1e-6 * volume, "{volume} != {want}");
    }
}

#[test]
fn a_fillet_leg_off_its_face_or_a_fillet_off_a_t_joint_is_refused() {
    let (through, stem, bevel) = t_joint(0.0);
    let top = facing(&through, UP);
    let [t, s] = t_picks(&through, &stem, &top, &bevel);
    // the plate runs 40 from C, the stem's side 60
    let off = |picks, z1: f64, z2: f64| groove_fillet_weld(picks, &flat(), legs(z1, z2)).unwrap_err();
    assert_eq!(off([&s, &t], 4.0, 50.0), GrooveRefusal::LegPastFace { part: JointPart::Right });
    assert_eq!(off([&t, &s], 4.0, 50.0), GrooveRefusal::LegPastFace { part: JointPart::Left });
    assert_eq!(off([&s, &t], 70.0, 4.0), GrooveRefusal::LegPastFace { part: JointPart::Left });
    // a depth, a leg of no length, and a butt joint
    let deep = FillOptions { depth: Some(2.0), ..flat() };
    assert!(matches!(groove_fillet_weld([&s, &t], &deep, legs(4.0, 4.0)), Err(GrooveRefusal::Invalid(_))));
    assert!(matches!(off([&s, &t], 0.0, 4.0), GrooveRefusal::Invalid(_)));
    let (parts, faces) = y_groove();
    let butt = groove_fillet_weld([&pick(&parts[0], &faces[0]), &pick(&parts[1], &faces[1])], &flat(), legs(4.0, 4.0));
    assert!(matches!(butt, Err(GrooveRefusal::Invalid(_))), "{butt:?}");
}

/// brett19's point 4 on #22504: a square-ended stem standing on the plate, its end picked as its
/// groove face: the stem's toe is on the plate, so no groove lies under the fillet, refused before
/// any leg is placed.
#[test]
fn a_fillet_over_a_square_ended_stem_is_refused() {
    let from = Plane { origin: Vec3::new(0.0, 0.0, -40.0), ..Plane::XY };
    let through = swept_by(&from, &[(-40.0, -8.0), (40.0, -8.0), (40.0, 0.0), (-40.0, 0.0)], 200.0);
    let stem = plate(&[(0.0, 0.0), (10.0, 0.0), (10.0, 60.0), (0.0, 60.0)]);
    let (top, end) = (facing(&through, UP), facing(&stem, [0.0, -1.0, 0.0]));
    let [t, s] = t_picks(&through, &stem, &top, &end);
    for picks in [[&s, &t], [&t, &s]] {
        assert_eq!(groove_fillet_weld(picks, &flat(), legs(4.0, 4.0)).unwrap_err(), GrooveRefusal::NoGrooveUnderFillet);
    }
}

// ---- the through part's face under the weld (CodeRabbit on #22504) ----

/// A through plate 8 thick from y = -8 to 0, its top's outline `outline` (x, z, counter-clockwise
/// seen from below) less `holes` (each clockwise), swept up to the top: the plate of [`t_joint`]
/// when the outline is its rectangle.
fn through_plate(outline: &[(f64, f64)], holes: &[&[(f64, f64)]]) -> Body {
    // x along x, y along -z: its normal is +y, so the plate runs from y = -8 up to 0; the plane's
    // y is -z, so each loop is reversed to keep its turn
    let plane = Plane { origin: Vec3::new(0.0, -8.0, 0.0), x: Vec3::new(1.0, 0.0, 0.0), y: Vec3::new(0.0, 0.0, -1.0) };
    let poly = |c: &[(f64, f64)]| Loop2::polygon(&c.iter().rev().map(|&(x, z)| Vec2::new(x, -z)).collect::<Vec<_>>());
    let region = Region2 { outer: poly(outline), holes: holes.iter().map(|&h| poly(h)).collect() };
    extrude_planar_regions(&plane, &[region], 0.0, 8.0).unwrap()
}

/// The rectangle of [`t_joint`]'s through plate, x from -40 to 40, z from -40 to 160.
const THROUGH: [(f64, f64); 4] = [(-40.0, -40.0), (40.0, -40.0), (40.0, 160.0), (-40.0, 160.0)];

/// A square hole 2 by 4 through the plate, x from `x` to `x + 2`, z from `z` to `z + 4`.
fn hole(x: f64, z: f64) -> [(f64, f64); 4] {
    [(x, z), (x, z + 4.0), (x + 2.0, z + 4.0), (x + 2.0, z)]
}

/// The beveled T of [`t_joint`] on a through plate with holes: one under the weld but past the
/// stem's end (z = -20), one beside the stem away from the weld (x = -30): both filled as without
/// them; one under the weld beside the stem (x from 2 to 4, z = 50), and one past the stem's end
/// that reaches 0.04 or 0.005 under it (z from -3.96 or -3.995, CodeRabbit on #22504): refused as interrupted,
/// naming the through part whichever part is first.
#[test]
fn a_hole_in_the_through_plate_matters_only_under_the_weld() {
    let (_, stem, bevel) = t_joint(0.0);
    for (holes, builds) in
        [(hole(2.0, -20.0), true), (hole(-30.0, 50.0), true), (hole(2.0, 50.0), false), (hole(2.0, -3.96), false), (hole(2.0, -3.995), false)]
    {
        let through = through_plate(&THROUGH, &[&holes]);
        let top = facing(&through, UP);
        assert_eq!(top.len(), 1, "{holes:?}");
        let (t, s) = (picked(&through, &top, UP), picked(&stem, &bevel, [-1.0, 0.0, 0.0]));
        for (picks, part) in [([&s, &t], JointPart::Right), ([&t, &s], JointPart::Left)] {
            let got = groove_weld(picks, &flat());
            if builds {
                let volume = got.unwrap().body.volume_at(1e-9).unwrap();
                assert!((volume - 18.0 * 120.0).abs() <= 1e-6 * volume, "{holes:?}: {volume}");
            } else {
                assert_eq!(got.unwrap_err(), GrooveRefusal::GrooveFaceInterrupted { part, face: top[0] }, "{holes:?}");
            }
        }
    }
}

/// The T of [`t_joint`] on a through plate whose top narrows: its edge on the fillet's side runs
/// from x = -40 at z = -40 to x = 2 at z = 160, so it is at -19 at the stem's middle and -6.4 at
/// its end. A through leg of 8 is on the face in the middle but runs off it near the end: refused;
/// a leg of 4 stays on it all along and builds (the groove's 18 under 4 · 4 / 2).
#[test]
fn a_fillet_leg_that_runs_off_a_narrowing_through_face_is_refused() {
    let (_, stem, bevel) = t_joint(0.0);
    let through = through_plate(&[(-40.0, -40.0), (40.0, -40.0), (40.0, 160.0), (2.0, 160.0)], &[]);
    let top = facing(&through, UP);
    let [t, s] = t_picks(&through, &stem, &top, &bevel);
    for (picks, part) in [([&s, &t], JointPart::Right), ([&t, &s], JointPart::Left)] {
        assert_eq!(groove_fillet_weld(picks, &flat(), legs(4.0, 8.0)).unwrap_err(), GrooveRefusal::LegPastFace { part });
        let volume = groove_fillet_weld(picks, &flat(), legs(4.0, 4.0)).unwrap().body.volume_at(1e-9).unwrap();
        assert!((volume - 120.0 * 26.0).abs() <= 1e-6 * volume, "{volume}");
    }
}

/// CodeRabbit on #22504: the T of [`t_joint`] on a through plate whose edge on the fillet's side
/// reaches x = -8 only 0.04 short of the stem's end (from x = -40 at z = -40 to x = 0.01 at
/// z = 160): a through leg of 8 is on the face but for its last 0.04, by less than a skin of volume,
/// and is refused at the end.
#[test]
fn a_fillet_leg_that_leaves_the_through_face_only_at_its_end_is_refused() {
    let (_, stem, bevel) = t_joint(0.0);
    let through = through_plate(&[(-40.0, -40.0), (40.0, -40.0), (40.0, 160.0), (0.01, 160.0)], &[]);
    let top = facing(&through, UP);
    let [t, s] = t_picks(&through, &stem, &top, &bevel);
    for (picks, part) in [([&s, &t], JointPart::Right), ([&t, &s], JointPart::Left)] {
        assert_eq!(groove_fillet_weld(picks, &flat(), legs(4.0, 8.0)).unwrap_err(), GrooveRefusal::LegPastFace { part });
    }
}
