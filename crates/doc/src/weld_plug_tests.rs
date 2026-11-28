//! Plug and slot welds: round, slot, square-cornered L, countersunk and counterbored holes filled
//! flush, partly, convex and concave against their volumes by hand, each weld checked valid; the
//! parts left as they were; the gap named; the refusals the parts can bring about (`NotClosed` and
//! `FloorUnmeasured` guard ops' answers and no valid pair of parts reaches them).

use std::f64::consts::{FRAC_PI_2, PI, TAU};

use solidcore_record::brep::Brep;
use solidcraft_kernel::{SmlibCheck, box_solid, validity};

use super::*;

/// The top part's thickness.
const T: f64 = 8.0;
const UP: [f64; 3] = [0.0, 0.0, 1.0];

/// What a body is: its record.
fn record(b: &Body) -> Brep {
    b.record().clone()
}

/// A box from corner `a` to corner `b`.
fn cuboid(a: [f64; 3], b: [f64; 3]) -> Body {
    box_solid(a.into(), b.into()).unwrap()
}

/// `a` with `b` cut out of it.
fn cut(a: Body, b: Body) -> Body {
    solidcraft_kernel::boolean(&a, &b, BoolOp::Cut).unwrap().unwrap()
}

/// The part below: 60 x 60 x 5 under z = 0, `w` wide in x from -30.
fn lower(w: f64) -> Body {
    cuboid([-30.0, -30.0, -5.0], [w - 30.0, 30.0, 0.0])
}

/// The top part: 60 x 60 x `T` from z = `z`, the tools cut out of it.
fn upper(z: f64, tools: Vec<Body>) -> Body {
    tools.into_iter().fold(cuboid([-30.0, -30.0, z], [30.0, 30.0, z + T]), cut)
}

/// Turned about z: radius `r0` at `z0` to `r1` at `z1`.
fn turned(r0: f64, z0: f64, r1: f64, z1: f64) -> Body {
    let l = |a: [f64; 2], b: [f64; 2]| Curve::Line { start: [a[0], 0.0, a[1]], end: [b[0], 0.0, b[1]] };
    profiles::revolve_curves(
        &[l([0.0, z0], [r0, z0]), l([r0, z0], [r1, z1]), l([r1, z1], [0.0, z1]), l([0.0, z1], [0.0, z0])],
        [0.0; 3],
        UP,
        TAU,
        true,
        None,
    )
    .unwrap()
}

fn prism(outline: &[Curve], h: f64) -> Body {
    profiles::extrude_curves(outline, UP, h, true).unwrap()
}

/// A stadium slot `l` long along x and `w` wide, on z = -1, through the top part.
fn stadium(l: f64, w: f64) -> Body {
    let (a, r) = (l / 2.0 - w / 2.0, w / 2.0);
    let arc = |cx: f64, start: f64| Curve::Arc { center: [cx, 0.0, -1.0], axis: UP, reference: [1.0, 0.0, 0.0], radius: r, start, end: start + PI };
    let line = |x0: f64, y: f64, x1: f64| Curve::Line { start: [x0, y, -1.0], end: [x1, y, -1.0] };
    prism(&[arc(a, -FRAC_PI_2), line(a, r, -a), arc(-a, FRAC_PI_2), line(-a, -r, a)], T + 2.0)
}

fn round_hole() -> Body {
    turned(6.0, -1.0, 6.0, T + 1.0)
}

/// The face of `top` a line from `at` along `dir` meets first.
fn wall(top: &Body, at: [f64; 3], dir: [f64; 3]) -> usize {
    top.face_hits_by_ray(at, dir).unwrap()[0].face
}

fn weld(top: &Body, below: &Body, at: [f64; 3], dir: [f64; 3], style: PlugStyle) -> Result<Plug> {
    plug_weld(top, below, wall(top, at, dir), &style)
}

/// The weld's volume, once the kernel's full check passes on it (brett19 on #22646: every weld).
fn volume(p: &Plug) -> f64 {
    let v = validity(&p.body);
    assert!(v.record.is_ok() && matches!(v.smlib, Some(SmlibCheck::Valid)), "the weld fails the kernel's check: {v:?}");
    p.body.volume_at(1e-9).unwrap()
}

fn close(got: f64, want: f64) {
    assert!((got - want).abs() <= 1e-6 * want.abs().max(1.0), "got {got}, want {want}");
}

const IN: [f64; 3] = [0.0, 0.0, 4.0];
const X: [f64; 3] = [1.0, 0.0, 0.0];
const ROUND: f64 = PI * 36.0;

#[test]
fn a_round_plug_fills_its_hole_flush_and_leaves_both_parts_as_they_were() {
    let (top, below) = (upper(0.0, vec![round_hole()]), lower(60.0));
    let before = (record(&top), record(&below));
    let p = weld(&top, &below, IN, X, PlugStyle::default()).unwrap();
    close(volume(&p), ROUND * T);
    assert_eq!(p.gap, None);
    assert_eq!((record(&top), record(&below)), before);
}

#[test]
fn a_slot_fills_flush_and_partly() {
    let area = 28.0 * 12.0 + ROUND;
    let (top, below) = (upper(0.0, vec![stadium(40.0, 12.0)]), lower(60.0));
    close(volume(&weld(&top, &below, IN, X, PlugStyle::default()).unwrap()), area * T);
    let half = PlugStyle { fill: PlugFill::Partial { depth: 4.0 }, ..PlugStyle::default() };
    close(volume(&weld(&top, &below, IN, X, half).unwrap()), area * 4.0);
}

#[test]
fn a_countersunk_and_a_counterbored_hole_are_filled_by_their_walls() {
    let frustum = |h: f64, r0: f64, r1: f64| PI * h / 3.0 * (r0 * r0 + r0 * r1 + r1 * r1);
    let below = lower(60.0);
    // radius 5.5 at the bottom face, 9.5 at the top
    let sunk = upper(0.0, vec![turned(5.0, -1.0, 10.0, T + 1.0)]);
    close(volume(&weld(&sunk, &below, IN, X, PlugStyle::default()).unwrap()), frustum(T, 5.5, 9.5));
    let half = PlugStyle { fill: PlugFill::Partial { depth: 4.0 }, ..PlugStyle::default() };
    close(volume(&weld(&sunk, &below, IN, X, half).unwrap()), frustum(4.0, 5.5, 7.5));
    // r 6 through, r 10 from z 5 up
    let bored = upper(0.0, vec![round_hole(), turned(10.0, 5.0, 10.0, T + 1.0)]);
    close(volume(&weld(&bored, &below, [0.0, 0.0, 2.0], X, PlugStyle::default()).unwrap()), PI * (36.0 * 5.0 + 100.0 * 3.0));
}

#[test]
fn convex_and_concave_faces_add_and_take_the_dome() {
    let (top, below) = (upper(0.0, vec![round_hole()]), lower(60.0));
    let h = 1.5;
    let cap = PI * h * (3.0 * 36.0 + h * h) / 6.0;
    // no shape given: spherical on a round hole (Q126), the same as asking for it
    for shape in [None, Some(DomeShape::Spherical)] {
        let sphere = PlugStyle { face: PlugFace::Convex { height: h, shape }, ..PlugStyle::default() };
        close(volume(&weld(&top, &below, IN, X, sphere).unwrap()), ROUND * T + cap);
    }
    let dish = PlugStyle { face: PlugFace::Concave { depth: h, shape: None }, ..PlugStyle::default() };
    close(volume(&weld(&top, &below, IN, X, dish).unwrap()), ROUND * T - cap);
    let ellipse = PlugStyle { face: PlugFace::Concave { depth: h, shape: Some(DomeShape::Elliptical) }, ..PlugStyle::default() };
    close(volume(&weld(&top, &below, IN, X, ellipse).unwrap()), ROUND * T - 2.0 / 3.0 * ROUND * h);
    // any star-shaped outline: (2/3) A h on the slot
    let area = 28.0 * 12.0 + ROUND;
    let slot = upper(0.0, vec![stadium(40.0, 12.0)]);
    // no shape given: elliptical on any outline that is not a circle (Q126)
    for shape in [None, Some(DomeShape::Elliptical)] {
        let ellipse = PlugStyle { face: PlugFace::Convex { height: h, shape }, ..PlugStyle::default() };
        close(volume(&weld(&slot, &below, IN, X, ellipse).unwrap()), area * T + 2.0 / 3.0 * area * h);
    }
}

#[test]
fn an_l_shaped_slot_fills_flush_but_takes_no_dome() {
    let l = |a: [f64; 2], b: [f64; 2]| Curve::Line { start: [a[0], a[1], -1.0], end: [b[0], b[1], -1.0] };
    let pts = [[0.0, 0.0], [20.0, 0.0], [20.0, 6.0], [6.0, 6.0], [6.0, 20.0], [0.0, 20.0]];
    let outline: Vec<Curve> = (0..6).map(|i| l(pts[i], pts[(i + 1) % 6])).collect();
    let (top, below) = (upper(0.0, vec![prism(&outline, T + 2.0)]), lower(60.0));
    let at = [3.0, 3.0, 4.0];
    close(volume(&weld(&top, &below, at, X, PlugStyle::default()).unwrap()), (20.0 * 6.0 + 6.0 * 14.0) * T);
    let convex = PlugStyle { face: PlugFace::Convex { height: PROPOSED_CROWN, shape: None }, ..PlugStyle::default() };
    assert!(matches!(weld(&top, &below, at, X, convex), Err(PlugRefusal::Dome(DomeRefusal::NotStar))));
}

#[test]
fn a_gap_below_is_named_and_left_open() {
    let (top, below) = (upper(0.5, vec![turned(6.0, -1.0, 6.0, T + 2.0)]), lower(60.0));
    let p = weld(&top, &below, IN, X, PlugStyle::default()).unwrap();
    close(volume(&p), ROUND * T);
    close(p.gap.unwrap(), 0.5);
}

#[test]
fn a_floor_that_does_not_reach_under_the_whole_hole_is_refused() {
    let top = upper(0.0, vec![stadium(40.0, 12.0)]);
    // the part below ends at x = 10, the slot runs to x = 20
    assert!(matches!(weld(&top, &lower(40.0), IN, X, PlugStyle::default()), Err(PlugRefusal::FloorShort { .. })));
    // a hole of radius 3 in the part below, under a plug of radius 6
    let holed = cut(lower(60.0), turned(3.0, -6.0, 3.0, 1.0));
    let plug = upper(0.0, vec![round_hole()]);
    assert!(matches!(weld(&plug, &holed, IN, X, PlugStyle::default()), Err(PlugRefusal::FloorShort { .. })));
}

#[test]
fn a_slot_open_at_the_part_s_edge_and_depths_past_the_top_are_refused() {
    let open = upper(0.0, vec![cuboid([10.0, -5.0, -1.0], [40.0, 5.0, T + 1.0])]);
    let below = lower(60.0);
    assert!(matches!(weld(&open, &below, [20.0, 0.0, 4.0], [0.0, 1.0, 0.0], PlugStyle::default()), Err(PlugRefusal::NotAHole { .. })));
    let top = upper(0.0, vec![round_hole()]);
    let deep = PlugStyle { fill: PlugFill::Partial { depth: T }, ..PlugStyle::default() };
    assert_eq!(weld(&top, &below, IN, X, deep).unwrap_err(), PlugRefusal::TooDeep { max: T });
    let dish = PlugStyle { face: PlugFace::Concave { depth: T, shape: None }, ..PlugStyle::default() };
    assert!(matches!(weld(&top, &below, IN, X, dish), Err(PlugRefusal::TooDeep { .. })));
    let bad = PlugStyle { fill: PlugFill::Partial { depth: f64::NAN }, ..PlugStyle::default() };
    assert!(matches!(weld(&top, &below, IN, X, bad), Err(PlugRefusal::Invalid(_))));
}

/// Turned about z through the profile points `(r, z)`, in order, closed back to the first.
fn turned_profile(points: &[[f64; 2]]) -> Body {
    let n = points.len();
    let line = |a: [f64; 2], b: [f64; 2]| Curve::Line { start: [a[0], 0.0, a[1]], end: [b[0], 0.0, b[1]] };
    let profile: Vec<Curve> = (0..n).map(|i| line(points[i], points[(i + 1) % n])).collect();
    profiles::revolve_curves(&profile, [0.0; 3], UP, TAU, true, None).unwrap()
}

#[test]
fn a_part_with_chamfered_outer_edges_still_has_its_top() {
    // a round plate, its top edge chamfered: the top face touches the chamfer, not the side
    let disc = turned_profile(&[[0.0, 0.0], [30.0, 0.0], [30.0, 6.0], [28.0, 8.0], [0.0, 8.0]]);
    let top = cut(disc, round_hole());
    close(volume(&weld(&top, &lower(60.0), IN, X, PlugStyle::default()).unwrap()), ROUND * T);
}

#[test]
fn a_partial_fill_is_cut_where_the_hole_widens_past_the_part_s_bottom() {
    // the part widens from radius 10 to 30, its hole from 2 to 18: at depth 2 the cut must reach
    // past radius 10 (CodeRabbit's case on #22646)
    let frustum = |h: f64, r0: f64, r1: f64| PI * h / 3.0 * (r0 * r0 + r0 * r1 + r1 * r1);
    let part = turned_profile(&[[0.0, 0.0], [10.0, 0.0], [30.0, T], [0.0, T]]);
    let top = cut(part, turned(1.0, -0.5, 19.0, T + 0.5));
    let below = lower(60.0);
    let at = [0.0, 0.0, 1.0];
    close(volume(&weld(&top, &below, at, X, PlugStyle::default()).unwrap()), frustum(T, 2.0, 18.0));
    let two = PlugStyle { fill: PlugFill::Partial { depth: 2.0 }, ..PlugStyle::default() };
    close(volume(&weld(&top, &below, at, X, two).unwrap()), frustum(2.0, 2.0, 6.0));
}

#[test]
fn a_counterbore_from_below_is_filled_from_the_faying_face() {
    // r 6 through, r 10 from the bottom up to z 3: the counterbore's ceiling faces down with a hole
    // in it, but the weld stands on the plate's bottom face (brett19 on #22646)
    let top = upper(0.0, vec![round_hole(), turned(10.0, -1.0, 10.0, 3.0)]);
    let below = lower(60.0);
    let full = PI * (100.0 * 3.0 + 36.0 * 5.0);
    // picked on the through hole, on the counterbore's wall and on its ceiling (a step)
    for (at, dir) in [([0.0, 0.0, 6.0], X), ([0.0, 0.0, 1.5], X), ([8.0, 0.0, 1.5], UP)] {
        let p = weld(&top, &below, at, dir, PlugStyle::default()).unwrap();
        close(volume(&p), full);
        assert_eq!(p.gap, None);
    }
    let two = PlugStyle { fill: PlugFill::Partial { depth: 2.0 }, ..PlugStyle::default() };
    close(volume(&weld(&top, &below, IN, X, two).unwrap()), PI * 100.0 * 2.0);
    let five = PlugStyle { fill: PlugFill::Partial { depth: 5.0 }, ..PlugStyle::default() };
    close(volume(&weld(&top, &below, IN, X, five).unwrap()), PI * (100.0 * 3.0 + 36.0 * 2.0));
    let h = 1.5;
    let convex = PlugStyle { face: PlugFace::Convex { height: h, shape: None }, ..PlugStyle::default() };
    close(volume(&weld(&top, &below, IN, X, convex).unwrap()), full + PI * h * (3.0 * 36.0 + h * h) / 6.0);
}

#[test]
fn the_part_s_top_face_picked_names_no_hole() {
    let (top, below) = (upper(0.0, vec![round_hole()]), lower(60.0));
    let picked = weld(&top, &below, [20.0, 20.0, T + 1.0], [0.0, 0.0, -1.0], PlugStyle::default());
    assert!(matches!(picked, Err(PlugRefusal::NotAHole { .. })), "{picked:?}");
}

#[test]
fn a_blind_hole_from_below_has_no_top() {
    let (top, below) = (upper(0.0, vec![turned(6.0, -1.0, 6.0, 4.0)]), lower(60.0));
    assert_eq!(weld(&top, &below, [0.0, 0.0, 2.0], X, PlugStyle::default()).unwrap_err(), PlugRefusal::NoTopFace);
}

#[test]
fn a_floor_that_is_not_flat_is_refused() {
    // the part below is a shallow cone, its apex under the hole's middle
    let cone = turned_profile(&[[0.0, -5.0], [30.0, -5.0], [30.0, -1.0], [0.0, 0.0]]);
    let top = upper(0.0, vec![round_hole()]);
    assert!(matches!(weld(&top, &cone, IN, X, PlugStyle::default()), Err(PlugRefusal::FloorNotFlat { .. })));
}

#[test]
fn parts_that_overlap_under_the_hole_are_refused() {
    // the part below reaches 0.5 into the top part: the line down starts inside it and meets its
    // bottom face, which faces down (brett19 on #22646: once read as a 4.5 gap)
    let raised = cuboid([-30.0, -30.0, -4.5], [30.0, 30.0, 0.5]);
    let top = upper(0.0, vec![round_hole()]);
    assert!(matches!(weld(&top, &raised, IN, X, PlugStyle::default()), Err(PlugRefusal::PartsOverlap { .. })));
}
