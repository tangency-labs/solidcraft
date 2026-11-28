//! Edge welds: the cap along a line and along a line and an arc against their closed-form volumes,
//! the flush mark, a corner wrapped round; and the weld on plates standing side by side: the cap
//! read from the stack, a step and a gap within and beyond the tolerance, each refusal, the plates
//! unchanged.

use std::f64::consts::FRAC_PI_2;

use solidcore_record::Vec3;
use solidcraft_kernel::{SmlibCheck, box_solid, validity};

use super::*;
use crate::weld_seam::Turn;
use crate::weld_stake::WELD_TOLERANCE;

fn volume(b: &Body) -> f64 {
    b.volume_at(1e-9).unwrap()
}

fn check(b: &Body, what: &str) {
    let v = validity(b);
    assert!(v.record.is_ok() && matches!(v.smlib, Some(SmlibCheck::Valid)), "{what}: {v:?}");
}

fn close(a: f64, b: f64, rel: f64, what: &str) {
    assert!((a - b).abs() <= rel * b.abs(), "{what}: {a} against {b}");
}

/// A circular segment `w` wide and `h` high: its area.
fn segment(w: f64, h: f64) -> f64 {
    let c = w / 2.0;
    let r = (c * c + h * h) / (2.0 * h);
    let th = 2.0 * (c / r).asin();
    r * r / 2.0 * (th - th.sin())
}

fn plate(x0: f64, x1: f64, top: f64) -> Body {
    box_solid(Vec3::new(x0, 0.0, top - 15.0), Vec3::new(x1, 30.0, top)).unwrap()
}

/// The record face a ray straight down at `(x, y)` meets first: a plate's end.
fn end(b: &Body, x: f64, y: f64) -> usize {
    b.face_hits_by_ray([x, y, 100.0], [0.0, 0.0, -1.0]).unwrap()[0].face
}

fn straight(across: [f64; 2], face: EdgeFace) -> EdgeWeld {
    EdgeWeld { start: [3.0, 0.0, 0.0], path: vec![SeamPiece::Line { to: [3.0, 30.0, 0.0] }], normal: [0.0, 0.0, 1.0], across, face }
}

#[test]
fn a_convex_cap_along_a_line_is_a_circular_segment_extruded() {
    for (w, h) in [(6.0, 1.5), (2.0, 0.5), (6.0, 3.0), (20.0, 1.0)] {
        let e = edge_weld(&straight([-w / 2.0, w / 2.0], EdgeFace::Convex { height: h })).unwrap();
        check(&e.body, "cap");
        close(volume(&e.body), segment(w, h) * 30.0, 1e-9, "cap volume");
        assert!(e.notices.is_empty());
    }
}

#[test]
fn a_flush_edge_weld_is_a_mark_flush_mark_thick() {
    let e = edge_weld(&straight([-3.0, 3.0], EdgeFace::Flush)).unwrap();
    check(&e.body, "flush");
    close(volume(&e.body), 6.0 * FLUSH_MARK * 30.0, 1e-9, "flush volume");
}

#[test]
fn a_cap_along_a_line_then_an_arc_follows_the_path() {
    // the arc turns left about +z round (-10, 30): radius 10, a quarter turn; the cap's centroid
    // lies on the path, so the arc's part is the segment turned at radius 10 (Pappus)
    let weld = EdgeWeld {
        start: [0.0, 0.0, 0.0],
        path: vec![SeamPiece::Line { to: [0.0, 30.0, 0.0] }, SeamPiece::Arc { center: [-10.0, 30.0, 0.0], to: [-10.0, 40.0, 0.0], turn: Turn::Left }],
        normal: [0.0, 0.0, 1.0],
        across: [-3.0, 3.0],
        face: EdgeFace::Convex { height: 1.5 },
    };
    let e = edge_weld(&weld).unwrap();
    check(&e.body, "line + arc");
    close(volume(&e.body), segment(6.0, 1.5) * (30.0 + 10.0 * FRAC_PI_2), 1e-9, "line + arc volume");
    assert!(e.notices.is_empty(), "{:?}", e.notices);
}

#[test]
fn a_cap_wraps_round_a_corner_with_a_notice() {
    // up +y, then turning right along +x: the cap turned about the corner on its outer side
    let weld = EdgeWeld {
        start: [0.0, 0.0, 0.0],
        path: vec![SeamPiece::Line { to: [0.0, 30.0, 0.0] }, SeamPiece::Line { to: [30.0, 30.0, 0.0] }],
        normal: [0.0, 0.0, 1.0],
        across: [-3.0, 3.0],
        face: EdgeFace::Convex { height: 1.5 },
    };
    let e = edge_weld(&weld).unwrap();
    check(&e.body, "corner");
    assert!(matches!(e.notices[..], [EdgeNotice::Path(SeamNotice::Corner { joint: 1, .. })]), "{:?}", e.notices);
    // two straight caps share at most their overlap at the corner, a w x w x h box; the wrap adds
    // at most a quarter of the segment turned at w / 2
    let two = 2.0 * segment(6.0, 1.5) * 30.0;
    let v = volume(&e.body);
    assert!(v > two - 6.0 * 6.0 * 1.5 && v < two + segment(6.0, 1.5) * 3.0 * FRAC_PI_2, "{v} against {two}");
}

#[test]
fn an_arc_tighter_than_the_cap_is_refused() {
    let weld = EdgeWeld {
        start: [0.0, 0.0, 0.0],
        path: vec![SeamPiece::Arc { center: [-2.0, 0.0, 0.0], to: [-2.0, 2.0, 0.0], turn: Turn::Left }],
        normal: [0.0, 0.0, 1.0],
        across: [-3.0, 3.0],
        face: EdgeFace::Convex { height: 1.0 },
    };
    assert!(matches!(edge_weld(&weld), Err(EdgeRefusal::ArcTighterThanCap { piece: 0 })));
}

#[test]
fn bad_input_is_refused() {
    assert!(matches!(edge_weld(&straight([1.0, 1.0], EdgeFace::Flush)), Err(EdgeRefusal::Invalid(_))));
    assert!(matches!(edge_weld(&straight([-1.0, 1.0], EdgeFace::Convex { height: 0.0 })), Err(EdgeRefusal::Invalid(_))));
    let mut w = straight([-1.0, 1.0], EdgeFace::Flush);
    w.normal = [0.0; 3];
    assert!(matches!(edge_weld(&w), Err(EdgeRefusal::Invalid(_))));
}

/// The weld along the seam between plates at x 0..3 and 3..6 (ends at z 0 and `second`), and the
/// plates, compared before and after.
fn on_two(second: f64, gap: f64, face: Option<EdgeFace>) -> Result<Edge> {
    let (a, b) = (plate(0.0, 3.0, 0.0), plate(3.0 + gap, 6.0 + gap, second));
    let (ra, rb) = (a.record().clone(), b.record().clone());
    let plates = [EdgePlate { body: &a, end: end(&a, 1.5, 15.0) }, EdgePlate { body: &b, end: end(&b, 4.5 + gap, 15.0) }];
    let got = edge_weld_on(&plates, [3.0, 0.0, 0.0], &[SeamPiece::Line { to: [3.0, 30.0, 0.0] }], face, WELD_TOLERANCE);
    assert_eq!(a.record(), &ra, "plate 1 changed");
    assert_eq!(b.record(), &rb, "plate 2 changed");
    got
}

#[test]
fn the_weld_on_two_plates_spans_the_stack_at_a_quarter_of_its_width() {
    let e = on_two(0.0, 0.0, None).unwrap();
    check(&e.body, "on two plates");
    close(volume(&e.body), segment(6.0, 1.5) * 30.0, 1e-9, "cap over the stack");
    assert!(e.notices.is_empty(), "{:?}", e.notices);
    let bb = e.body.exact_bounds().unwrap();
    assert!(bb.min.x.abs() < 1e-6 && (bb.max.x - 6.0).abs() < 1e-6, "toes at x 0 and 6: {bb:?}");
}

#[test]
fn three_plates_take_one_cap() {
    let (a, b, c) = (plate(0.0, 1.0, 0.0), plate(1.0, 2.0, 0.0), plate(2.0, 3.0, 0.0));
    let plates = [
        EdgePlate { body: &a, end: end(&a, 0.5, 15.0) },
        EdgePlate { body: &b, end: end(&b, 1.5, 15.0) },
        EdgePlate { body: &c, end: end(&c, 2.5, 15.0) },
    ];
    let e = edge_weld_on(&plates, [0.0, 0.0, 0.0], &[SeamPiece::Line { to: [0.0, 30.0, 0.0] }], None, WELD_TOLERANCE).unwrap();
    close(volume(&e.body), segment(3.0, 0.75) * 30.0, 1e-9, "three plates");
}

#[test]
fn ends_apart_within_the_tolerance_are_flush() {
    let e = on_two(-0.05, 0.0, None).unwrap();
    assert!(e.notices.is_empty(), "{:?}", e.notices);
}

#[test]
fn a_step_less_than_the_cap_is_filled_with_a_notice() {
    let e = on_two(-0.6, 0.0, None).unwrap();
    check(&e.body, "step");
    assert!(matches!(e.notices[..], [EdgeNotice::Step { by }] if (by - 0.6).abs() < 1e-9), "{:?}", e.notices);
    // the cap stands on the lower end and never runs into the higher plate
    let higher = plate(0.0, 3.0, 0.0);
    let into = solidcraft_kernel::boolean(&e.body, &higher, BoolOp::Intersect).unwrap();
    assert!(into.is_none_or(|b| volume(&b) < 1e-9), "the cap runs into the higher plate");
}

#[test]
fn a_step_past_the_cap_is_refused() {
    assert!(matches!(on_two(-3.0, 0.0, None), Err(EdgeRefusal::NotAlongside { .. } | EdgeRefusal::EndsApart { .. })));
    assert!(matches!(on_two(-1.0, 0.0, Some(EdgeFace::Flush)), Err(EdgeRefusal::EndsApart { .. })));
}

#[test]
fn a_gap_is_bridged_with_a_notice_and_refused_past_the_cap() {
    let e = on_two(0.0, 0.5, None).unwrap();
    check(&e.body, "gap");
    assert!(matches!(e.notices[..], [EdgeNotice::Gap { by }] if (by - 0.5).abs() < 1e-9), "{:?}", e.notices);
    assert!(matches!(on_two(0.0, 0.5, Some(EdgeFace::Flush)), Err(EdgeRefusal::GapTooWide { .. })));
    assert!(matches!(on_two(0.0, 2.0, Some(EdgeFace::Convex { height: 1.0 })), Err(EdgeRefusal::GapTooWide { .. })));
}

#[test]
fn one_plate_is_refused() {
    let a = plate(0.0, 3.0, 0.0);
    let plates = [EdgePlate { body: &a, end: end(&a, 1.5, 15.0) }];
    assert!(matches!(
        edge_weld_on(&plates, [0.0, 0.0, 0.0], &[SeamPiece::Line { to: [0.0, 30.0, 0.0] }], None, WELD_TOLERANCE),
        Err(EdgeRefusal::OnePlate)
    ));
}
