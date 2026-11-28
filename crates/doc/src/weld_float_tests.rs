//! Floating welds: a fillet along a sketch line on the path's left and above the sketch, each flip
//! and an angle between the legs other than 90°, flat, convex and concave faces and a groove
//! between two sketched walls, all against their closed-form volumes; along an arc turning either
//! way (Pappus), and an arc tighter than the weld refused; a sketched rectangle with the weld
//! inside (corners united) and outside (corners wrapped round, closed at the start too); a 3D
//! sketch's corner turning up (united) and down (wrapped), each with its notice; a U-turn split
//! into two welds; a free curve, pieces that do not join and an up direction along the path
//! refused; a concave face too deep refused with the depth that builds, which builds; the weld
//! moved and turned with its path (T216); and a spot built from a sketch point and normal, the same
//! spot wherever it is placed.

use std::f64::consts::{FRAC_PI_2, PI};

use solidcore_record::{Plane, Seg2, Vec2, Vec3};
use solidcraft_kernel::{Contour, PathSeg, SmlibCheck, transform, validity};

use super::*;
use crate::weld_spot::{SpotLook, SpotWeld, spot_weld};

const UP: [f64; 3] = [0.0, 0.0, 1.0];

/// A right triangle of legs 6 and 6: 18 mm².
const LEG: f64 = 6.0;
const AREA: f64 = 18.0;

fn flat() -> FillOptions {
    FillOptions { face: Contour::Flat, second_face: None, depth: None }
}

fn fillet() -> FloatSection {
    FloatSection::Fillet { legs: [LEG, LEG], angle: FRAC_PI_2, flip_side: false, flip_up: false }
}

fn v(x: f64, y: f64, z: f64) -> Vec3 {
    Vec3::new(x, y, z)
}

fn line(a: Vec3, b: Vec3) -> PathSeg {
    PathSeg::Line { a, b }
}

/// The lines through `points` in order.
fn lines(points: &[Vec3]) -> Vec<PathSeg> {
    points.windows(2).map(|w| line(w[0], w[1])).collect()
}

fn volume(b: &Body) -> f64 {
    b.volume_at(1e-9).unwrap()
}

/// The kernel's check: the record's, and the kernel's own at its strictest level.
fn check(b: &Body, what: &str) {
    let v = validity(b);
    assert!(v.record.is_ok() && matches!(v.smlib, Some(SmlibCheck::Valid)), "{what}: {v:?}");
}

fn close(got: f64, want: f64, what: &str) {
    assert!((got - want).abs() <= 1e-6 * want.abs(), "{what}: volume {got} != {want}");
}

/// The one weld of a path that does not double back, checked.
fn one(path: &[PathSeg], section: &FloatSection, options: &FillOptions) -> (Body, Vec<FloatNote>) {
    let got = float_weld(path, UP, section, options).unwrap();
    assert_eq!(got.welds.len(), 1, "one weld");
    let body = got.welds.into_iter().next().unwrap();
    check(&body, "the weld");
    (body, got.notes)
}

/// A circle's segment over chord `c` at sagitta `h`.
fn segment(c: f64, h: f64) -> f64 {
    let r = (c * c / 4.0 + h * h) / (2.0 * h);
    let theta = 4.0 * (2.0 * h / c).atan();
    r * r / 2.0 * (theta - theta.sin())
}

#[test]
fn a_fillet_along_a_sketch_line_sits_left_of_it_and_above_the_sketch() {
    let (body, notes) = one(&lines(&[v(0.0, 0.0, 0.0), v(100.0, 0.0, 0.0)]), &fillet(), &flat());
    close(volume(&body), AREA * 100.0, "flat fillet");
    assert!(notes.is_empty());
    let bb = body.exact_bounds().unwrap();
    for (got, want) in [(bb.min.y, 0.0), (bb.max.y, LEG), (bb.min.z, 0.0), (bb.max.z, LEG)] {
        assert!((got - want).abs() < 1e-6, "bounds {bb:?}");
    }
}

#[test]
fn each_flip_puts_the_legs_on_the_other_side() {
    let path = lines(&[v(0.0, 0.0, 0.0), v(100.0, 0.0, 0.0)]);
    for (flip_side, flip_up) in [(true, false), (false, true), (true, true)] {
        let section = FloatSection::Fillet { legs: [LEG, LEG], angle: FRAC_PI_2, flip_side, flip_up };
        let (body, _) = one(&path, &section, &flat());
        close(volume(&body), AREA * 100.0, "flipped fillet");
        let bb = body.exact_bounds().unwrap();
        let (y, z) = (if flip_side { -LEG } else { LEG }, if flip_up { -LEG } else { LEG });
        assert!((bb.min.y - y.min(0.0)).abs() < 1e-6 && (bb.max.y - y.max(0.0)).abs() < 1e-6, "{flip_side} {flip_up}: {bb:?}");
        assert!((bb.min.z - z.min(0.0)).abs() < 1e-6 && (bb.max.z - z.max(0.0)).abs() < 1e-6, "{flip_side} {flip_up}: {bb:?}");
    }
}

#[test]
fn legs_at_120_degrees_and_unequal_legs_have_their_triangle_s_area() {
    let path = lines(&[v(0.0, 0.0, 0.0), v(100.0, 0.0, 0.0)]);
    let angle = 120f64.to_radians();
    let section = FloatSection::Fillet { legs: [4.0, 8.0], angle, flip_side: false, flip_up: false };
    let (body, _) = one(&path, &section, &flat());
    close(volume(&body), 0.5 * 4.0 * 8.0 * angle.sin() * 100.0, "120 degrees");
}

#[test]
fn convex_and_concave_faces_add_and_take_their_segment() {
    let path = lines(&[v(0.0, 0.0, 0.0), v(100.0, 0.0, 0.0)]);
    let c = LEG * 2f64.sqrt();
    let convex = FillOptions { face: Contour::Convex { height: 1.0 }, ..flat() };
    close(volume(&one(&path, &fillet(), &convex).0), (AREA + segment(c, 1.0)) * 100.0, "convex 1.0");
    let concave = FillOptions { face: Contour::Concave { depth: 1.0 }, ..flat() };
    close(volume(&one(&path, &fillet(), &concave).0), (AREA - segment(c, 1.0)) * 100.0, "concave 1.0");
}

#[test]
fn a_concave_face_too_deep_is_refused_with_the_depth_that_builds() {
    let path = lines(&[v(0.0, 0.0, 0.0), v(100.0, 0.0, 0.0)]);
    let deep = FillOptions { face: Contour::Concave { depth: 2.0 }, ..flat() };
    let got = float_weld(&path, UP, &fillet(), &deep).unwrap_err();
    let FloatRefusal::Fill { fit: Some(fit), .. } = got else { panic!("{got:?}") };
    // the arc tangent to the legs at the toes: (c / 2) tan(45° / 2)
    let touch = LEG * 2f64.sqrt() / 2.0 * (PI / 8.0).tan();
    assert!(fit < touch && fit > touch - 0.01, "fit {fit} against {touch}");
    let fits = FillOptions { face: Contour::Concave { depth: fit }, ..flat() };
    one(&path, &fillet(), &fits);
}

#[test]
fn a_groove_between_two_sketched_walls_fills_them() {
    // a 6 mm V opening 6 at the top, 1 at the root, open at the root: closed flat there
    let walls = [
        vec![Seg2::Line { a: Vec2::new(-3.0, 0.0), b: Vec2::new(-0.5, -6.0) }],
        vec![Seg2::Line { a: Vec2::new(3.0, 0.0), b: Vec2::new(0.5, -6.0) }],
    ];
    let section = FloatSection::Groove { walls, depth_normal: Some(Vec2::new(0.0, 1.0)) };
    let (body, _) = one(&lines(&[v(0.0, 0.0, 0.0), v(100.0, 0.0, 0.0)]), &section, &flat());
    close(volume(&body), (6.0 + 1.0) / 2.0 * 6.0 * 100.0, "groove");
}

#[test]
fn along_an_arc_the_weld_is_its_section_turned_either_way() {
    // a quarter circle of radius 50 about the origin from (50, 0, 0): turning left about +Z the
    // leg lies toward the centre, the section's centroid 2 nearer it; turning right, 2 further
    for (axis, distance) in [(v(0.0, 0.0, 1.0), 48.0), (v(0.0, 0.0, -1.0), 52.0)] {
        let path = [PathSeg::Arc { a: v(50.0, 0.0, 0.0), center: Vec3::ZERO, axis, angle: FRAC_PI_2 }];
        let (body, _) = one(&path, &fillet(), &flat());
        close(volume(&body), AREA * distance * FRAC_PI_2, "arc");
    }
}

#[test]
fn an_arc_tighter_than_the_weld_is_refused() {
    let path = [PathSeg::Arc { a: v(5.0, 0.0, 0.0), center: Vec3::ZERO, axis: v(0.0, 0.0, 1.0), angle: FRAC_PI_2 }];
    let got = float_weld(&path, UP, &fillet(), &flat()).unwrap_err();
    let FloatRefusal::ArcTighterThanSection { piece: 0, radius, reach } = got else { panic!("{got:?}") };
    assert!((radius - 5.0).abs() < 1e-9 && (reach - LEG).abs() < 1e-9, "{radius} {reach}");
}

#[test]
fn a_sketched_rectangle_unites_its_inside_corners_and_wraps_its_outside_ones() {
    // counter-clockwise, 40 x 25, closed: the weld's default side (left) is inside
    let path = lines(&[v(0.0, 0.0, 0.0), v(40.0, 0.0, 0.0), v(40.0, 25.0, 0.0), v(0.0, 25.0, 0.0), v(0.0, 0.0, 0.0)]);
    let prisms = AREA * 130.0;
    // inside: at each corner the two pieces overlap by the integral of (6 - max(x, y)) over a 6 x 6 square, 72
    let (inside, notes) = one(&path, &fillet(), &flat());
    close(volume(&inside), prisms - 4.0 * 72.0, "inside");
    assert!(notes.is_empty(), "sketch corners have no notice: {notes:?}");
    // outside: each corner adds a quarter cone of radius and height 6, 18 pi
    let out = FloatSection::Fillet { legs: [LEG, LEG], angle: FRAC_PI_2, flip_side: true, flip_up: false };
    let (outside, notes) = one(&path, &out, &flat());
    close(volume(&outside), prisms + 4.0 * 18.0 * PI, "outside");
    assert!(notes.is_empty(), "{notes:?}");
}

#[test]
fn a_3d_corner_out_of_the_section_s_plane_is_carried_round_with_a_notice() {
    let start = [v(0.0, 0.0, 0.0), v(30.0, 0.0, 0.0)];
    // up: the legs lie inside the bend; the pieces overlap by 72, as at a sketch's inside corner
    let (body, notes) = one(&lines(&[start[0], start[1], v(30.0, 0.0, 20.0)]), &fillet(), &flat());
    close(volume(&body), AREA * 50.0 - 72.0, "turning up");
    assert_eq!(notes, vec![FloatNote::CornerOutOfPlane { joint: 1, angle: FRAC_PI_2 }]);
    // down: outside the bend; the section turned about the corner fills it, a quarter of
    // the triangle turned about its leg: 18 pi
    let (body, notes) = one(&lines(&[start[0], start[1], v(30.0, 0.0, -20.0)]), &fillet(), &flat());
    close(volume(&body), AREA * 50.0 + 18.0 * PI, "turning down");
    assert!(matches!(notes.as_slice(), [FloatNote::CornerOutOfPlane { joint: 1, .. }]), "{notes:?}");
}

#[test]
fn a_path_that_doubles_back_is_split_into_two_welds() {
    let path = lines(&[v(0.0, 0.0, 0.0), v(30.0, 0.0, 0.0), v(10.0, 0.0, 0.0)]);
    let got = float_weld(&path, UP, &fillet(), &flat()).unwrap();
    assert_eq!(got.notes, vec![FloatNote::UTurn { joint: 1 }]);
    let volumes: Vec<f64> = got.welds.iter().map(volume).collect();
    assert_eq!(volumes.len(), 2);
    close(volumes[0], AREA * 30.0, "before the U-turn");
    close(volumes[1], AREA * 20.0, "after it");
}

#[test]
fn free_curves_pieces_apart_and_an_up_along_the_path_are_refused() {
    let cubic = Seg2::Cubic { p0: Vec2::new(0.0, 0.0), p1: Vec2::new(10.0, 10.0), p2: Vec2::new(20.0, -10.0), p3: Vec2::new(30.0, 0.0) };
    let curve = [PathSeg::Curve { plane: Plane::XY, seg: cubic }];
    assert_eq!(float_weld(&curve, UP, &fillet(), &flat()).unwrap_err(), FloatRefusal::FreeCurve { piece: 0 });
    let apart = [line(v(0.0, 0.0, 0.0), v(10.0, 0.0, 0.0)), line(v(11.0, 0.0, 0.0), v(20.0, 0.0, 0.0))];
    assert_eq!(float_weld(&apart, UP, &fillet(), &flat()).unwrap_err(), FloatRefusal::NotJoined { joint: 1 });
    let along = lines(&[v(0.0, 0.0, 0.0), v(0.0, 0.0, 10.0)]);
    assert!(matches!(float_weld(&along, UP, &fillet(), &flat()), Err(FloatRefusal::Invalid(_))));
    assert!(matches!(float_weld(&[], UP, &fillet(), &flat()), Err(FloatRefusal::Invalid(_))));
}

/// `p` turned 37° about the line through the origin along (1, 2, 3), then moved by (10, -20, 5).
fn moved(p: Vec3) -> Vec3 {
    turned(p) + v(10.0, -20.0, 5.0)
}

fn turned(p: Vec3) -> Vec3 {
    let turn = affine::rotation([0.0; 3], [1.0, 2.0, 3.0], 37f64.to_radians()).unwrap();
    affine::apply_point(&turn, p.into()).into()
}

#[test]
fn a_weld_moved_and_turned_with_its_path_is_the_same_weld_moved() {
    // T216: a line, a quarter arc (radius 30, the leg toward its centre) and a line
    let path = [
        line(v(0.0, 0.0, 0.0), v(40.0, 0.0, 0.0)),
        PathSeg::Arc { a: v(40.0, 0.0, 0.0), center: v(40.0, 30.0, 0.0), axis: v(0.0, 0.0, 1.0), angle: FRAC_PI_2 },
        line(v(70.0, 30.0, 0.0), v(70.0, 60.0, 0.0)),
    ];
    let (here, _) = one(&path, &fillet(), &flat());
    close(volume(&here), AREA * (40.0 + 28.0 * FRAC_PI_2 + 30.0), "the chain");
    let there_path: Vec<PathSeg> = path
        .iter()
        .map(|p| match *p {
            PathSeg::Line { a, b } => line(moved(a), moved(b)),
            PathSeg::Arc { a, center, axis, angle } => PathSeg::Arc { a: moved(a), center: moved(center), axis: turned(axis), angle },
            _ => panic!("lines and arcs only"),
        })
        .collect();
    let there = float_weld(&there_path, turned(Vec3::from(UP)).into(), &fillet(), &flat()).unwrap();
    assert_eq!(there.welds.len(), 1);
    check(&there.welds[0], "moved weld");
    close(volume(&there.welds[0]), volume(&here), "moved weld");
    // the weld built here and then moved lies where the weld built there does
    let carried = transform(&here, v(10.0, -20.0, 5.0), Vec3::ZERO, v(1.0, 2.0, 3.0), 37f64.to_radians()).unwrap();
    let (a, b) = (carried.exact_bounds().unwrap(), there.welds[0].exact_bounds().unwrap());
    for (p, q) in [(a.min, b.min), (a.max, b.max)] {
        assert!((p.x - q.x).abs() < 1e-6 && (p.y - q.y).abs() < 1e-6 && (p.z - q.z).abs() < 1e-6, "{a:?} against {b:?}");
    }
}

#[test]
fn a_spot_from_a_sketch_point_and_normal_is_the_same_spot_wherever_it_is() {
    let style = SpotLook::Marks.style();
    let total = |spot: &SpotWeld| -> f64 { spot_weld(spot).unwrap().iter().map(|p| volume(&p.body)).sum() };
    let here = SpotWeld { at: [0.0; 3], normal: UP, thicknesses: vec![1.5, 1.0], diameter: 5.0, style };
    // a sketch point on a tilted sketch plane, no part anywhere
    let there = SpotWeld { at: [3.0, -4.0, 7.0], normal: [1.0, 2.0, 2.0], ..here.clone() };
    close(total(&there), total(&here), "spot");
}
