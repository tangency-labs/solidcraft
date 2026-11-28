//! Tack welds: the spans of each placement (the first tack at the start, count + length from end
//! to end, one tack in the middle, a short joint, tacks that overlap, input refused); the domes
//! on a T joint, a thin stem, a butt joint and a tube standing on a plate against their volumes,
//! the parts unchanged; picked points moved onto the joint or refused; free-form and broken paths
//! refused; and the tack's weld symbol.

use std::f64::consts::PI;

use solidcore_record::Vec3;
use solidcraft_kernel::{box_solid, cylinder};

use super::*;
use crate::weld::WeldSettings;

const L: f64 = 20.0;

fn bx(a: [f64; 3], b: [f64; 3]) -> Body {
    box_solid(Vec3::from(a), Vec3::from(b)).unwrap()
}

fn vol(b: &Body) -> f64 {
    b.volume_at(1e-9).unwrap()
}

/// The face of `b` that holds `p`.
fn face_at(b: &Body, p: [f64; 3]) -> usize {
    (0..b.face_count()).find(|&f| b.face_normal_at(f, p.into()).unwrap().is_some()).unwrap()
}

fn line(start: [f64; 3], end: [f64; 3]) -> Curve {
    Curve::Line { start, end }
}

/// A quarter of the tack's ellipsoid: `length` long, radius L / 2.25 across.
fn quarter(length: f64) -> f64 {
    let r = L / LENGTH_OVER_RADIUS;
    PI * (length / 2.0) * r * r / 3.0
}

fn near(got: f64, want: f64, what: &str) {
    assert!((got - want).abs() <= 1e-6 * want.abs().max(1.0), "{what}: {got} != {want}");
}

fn same(before: &[(f64, usize)], parts: &[&Body]) {
    for (&(v, f), p) in before.iter().zip(parts) {
        assert!((vol(p) - v).abs() <= 1e-12 * v && p.face_count() == f, "a part changed");
    }
}

#[test]
fn length_and_pitch_start_at_the_joint_s_start() {
    let (spans, notices) = tack_spans(200.0, &TackPlacement::Pitch { length: L, pitch: 50.0 }).unwrap();
    assert_eq!(spans, vec![[0.0, 20.0], [50.0, 70.0], [100.0, 120.0], [150.0, 170.0]]);
    assert!(notices.is_empty());
    // the last tack ends right at the joint's end
    let (spans, _) = tack_spans(170.0, &TackPlacement::Pitch { length: L, pitch: 50.0 }).unwrap();
    assert_eq!(spans.last(), Some(&[150.0, 170.0]));
}

#[test]
fn count_and_length_run_from_end_to_end_and_one_tack_sits_in_the_middle() {
    let (spans, notices) = tack_spans(200.0, &TackPlacement::Count { count: 4, length: L }).unwrap();
    assert_eq!(spans, vec![[0.0, 20.0], [60.0, 80.0], [120.0, 140.0], [180.0, 200.0]]);
    assert!(notices.is_empty());
    let (spans, _) = tack_spans(200.0, &TackPlacement::Count { count: 1, length: L }).unwrap();
    assert_eq!(spans, vec![[90.0, 110.0]]);
}

#[test]
fn a_joint_shorter_than_a_tack_gets_one_tack_cut_to_it() {
    let (spans, notices) = tack_spans(15.0, &TackPlacement::Pitch { length: L, pitch: 50.0 }).unwrap();
    assert_eq!(spans, vec![[0.0, 15.0]]);
    assert_eq!(notices, vec![TackNotice::Cut { tack: 0, length: 15.0 }]);
}

#[test]
fn tacks_closer_than_their_length_overlap_with_a_notice() {
    let (spans, notices) = tack_spans(50.0, &TackPlacement::Pitch { length: L, pitch: 10.0 }).unwrap();
    assert_eq!(spans.len(), 4);
    assert_eq!(notices, (1..4).map(|tack| TackNotice::Overlap { tack }).collect::<Vec<_>>());
}

#[test]
fn spans_out_of_range_are_refused() {
    for (length, placement) in [
        (200.0, TackPlacement::Pitch { length: L, pitch: 0.0 }),
        (200.0, TackPlacement::Pitch { length: f64::NAN, pitch: 50.0 }),
        (200.0, TackPlacement::Pitch { length: L, pitch: 1e-9 }),
        (200.0, TackPlacement::Count { count: 0, length: L }),
        (-1.0, TackPlacement::Count { count: 2, length: L }),
    ] {
        assert!(matches!(tack_spans(length, &placement), Err(TackRefusal::Invalid(_))), "{placement:?}");
    }
}

/// A plate 200 long and a stem `thick` thick standing on it; the joint at the stem's -y face.
fn tee(thick: f64) -> (Body, Body, Vec<Curve>) {
    let plate = bx([-100.0, -30.0, -8.0], [100.0, 30.0, 0.0]);
    let stem = bx([-100.0, -thick / 2.0, 0.0], [100.0, thick / 2.0, 40.0]);
    (plate, stem, vec![line([-100.0, -thick / 2.0, 0.0], [100.0, -thick / 2.0, 0.0])])
}

#[test]
fn tacks_on_a_t_joint_are_quarter_domes_and_leave_the_parts_alone() {
    let (plate, stem, path) = tee(8.0);
    let before = [(vol(&plate), plate.face_count()), (vol(&stem), stem.face_count())];
    let joint = TackJoint { path, faces: [(&plate, face_at(&plate, [0.0, -20.0, 0.0])), (&stem, face_at(&stem, [0.0, -4.0, 20.0]))] };
    let tacks = tack_weld(&joint, &TackPlacement::Pitch { length: L, pitch: 50.0 }).unwrap();
    assert_eq!(tacks.bodies.len(), 4);
    near(tacks.length, 80.0, "length");
    for b in &tacks.bodies {
        near(vol(b), quarter(L), "a tack");
    }
    assert!(tacks.notices.is_empty());
    same(&before, &[&plate, &stem]);
}

#[test]
fn a_thin_stem_does_not_let_the_tack_through_to_its_far_side() {
    let (plate, stem, path) = tee(2.0);
    let joint = TackJoint { path, faces: [(&plate, face_at(&plate, [0.0, -20.0, 0.0])), (&stem, face_at(&stem, [0.0, -1.0, 20.0]))] };
    let tacks = tack_weld(&joint, &TackPlacement::Count { count: 1, length: L }).unwrap();
    near(vol(&tacks.bodies[0]), quarter(L), "the tack by a 2 mm stem");
    assert_eq!(tacks.bodies[0].lumps().unwrap().len(), 1);
}

#[test]
fn a_tack_on_a_butt_joint_is_half_a_dome() {
    let a = bx([-50.0, -30.0, -8.0], [0.0, 30.0, 0.0]);
    let b = bx([0.0, -30.0, -8.0], [50.0, 30.0, 0.0]);
    let joint = TackJoint {
        path: vec![line([0.0, -30.0, 0.0], [0.0, 30.0, 0.0])],
        faces: [(&a, face_at(&a, [-10.0, 0.0, 0.0])), (&b, face_at(&b, [10.0, 0.0, 0.0]))],
    };
    let tacks = tack_weld(&joint, &TackPlacement::Count { count: 1, length: L }).unwrap();
    near(vol(&tacks.bodies[0]), 2.0 * quarter(L), "the tack across the seam");
}

#[test]
fn a_point_off_the_joint_is_moved_onto_it_and_one_too_far_is_refused() {
    let (plate, stem, path) = tee(8.0);
    let faces = [(&plate, face_at(&plate, [0.0, -20.0, 0.0])), (&stem, face_at(&stem, [0.0, -4.0, 20.0]))];
    let joint = TackJoint { path, faces };
    let tacks = tack_weld(&joint, &TackPlacement::Points { length: L, points: vec![[0.0, -6.0, 0.0], [95.0, -4.0, 0.0]] }).unwrap();
    assert_eq!(tacks.bodies.len(), 2);
    assert!(matches!(tacks.notices[0], TackNotice::Moved { tack: 0, distance } if (distance - 2.0).abs() < 1e-9), "{:?}", tacks.notices);
    assert!(matches!(tacks.notices[1], TackNotice::Cut { tack: 1, length } if (length - 15.0).abs() < 1e-9), "{:?}", tacks.notices);
    near(vol(&tacks.bodies[0]), quarter(L), "the moved tack");
    near(vol(&tacks.bodies[1]), quarter(15.0), "the cut tack");
    let far = tack_weld(&joint, &TackPlacement::Points { length: L, points: vec![[0.0, -30.0, 0.0]] });
    assert!(matches!(far, Err(TackRefusal::PointOffJoint { point: 0, distance }) if (distance - 26.0).abs() < 1e-9), "{far:?}");
}

#[test]
fn tacks_follow_a_tube_standing_on_a_plate() {
    let plate = bx([-40.0, -40.0, -8.0], [40.0, 40.0, 0.0]);
    let rod = cylinder(Vec3::from([0.0, 0.0, 0.0]), Vec3::from([0.0, 0.0, 1.0]), 10.0, 30.0).unwrap();
    let before = [(vol(&plate), plate.face_count()), (vol(&rod), rod.face_count())];
    let arc = Curve::Arc { center: [0.0; 3], axis: [0.0, 0.0, 1.0], reference: [1.0, 0.0, 0.0], radius: 10.0, start: 0.0, end: PI };
    let joint = TackJoint { path: vec![arc], faces: [(&plate, face_at(&plate, [30.0, 30.0, 0.0])), (&rod, face_at(&rod, [0.0, 10.0, 15.0]))] };
    let tacks = tack_weld(&joint, &TackPlacement::Count { count: 3, length: 10.0 }).unwrap();
    assert_eq!(tacks.bodies.len(), 3);
    // no closed form: the tube curving away leaves a little more room than a flat wall
    let flat = PI * 5.0 * (10.0 / LENGTH_OVER_RADIUS).powi(2) / 3.0;
    for b in &tacks.bodies {
        let v = vol(b);
        assert!(v > 0.5 * flat && v < 1.5 * flat, "a tack by the tube: {v} against {flat}");
    }
    same(&before, &[&plate, &rod]);
}

#[test]
fn free_form_and_broken_paths_are_refused() {
    let (plate, stem, _) = tee(8.0);
    let faces = [(&plate, 0), (&stem, 0)];
    let spline = Curve::Nurbs { degree: 1, points: vec![[0.0, -4.0, 0.0, 1.0], [10.0, -4.0, 0.0, 1.0]], knots: vec![0.0, 0.0, 1.0, 1.0] };
    let placement = TackPlacement::Count { count: 1, length: L };
    let got = tack_weld(&TackJoint { path: vec![line([-10.0, -4.0, 0.0], [0.0, -4.0, 0.0]), spline], faces }, &placement);
    assert!(matches!(got, Err(TackRefusal::NotLineOrArc { piece: 1 })), "{got:?}");
    let broken = vec![line([-50.0, -4.0, 0.0], [0.0, -4.0, 0.0]), line([1.0, -4.0, 0.0], [50.0, -4.0, 0.0])];
    let got = tack_weld(&TackJoint { path: broken, faces }, &placement);
    assert_eq!(got.err(), Some(TackRefusal::Invalid("joint pieces that do not meet end to end")));
}

#[test]
fn a_tack_s_symbol_is_the_basic_symbol_with_tack_after_the_process() {
    let mut settings = WeldSettings { weld_type: "tack".into(), process: Some("135".into()), ..Default::default() };
    let symbol = settings.symbol().unwrap();
    assert_eq!((symbol.arrow_side, symbol.other_side, symbol.tail.as_deref()), (None, None, Some("135/tack")));
    settings.process = None;
    assert_eq!(settings.symbol().unwrap().tail.as_deref(), Some("tack"));
}
