use super::*;
use crate::drawing_view::{NamedView, basis_looking};

use AngleFrom::{Edge, ModelAxis};
use LinearDirection::{Aligned, Along, Horizontal, Vertical};
use Reading::{Projected, True};

/// Paper x along X, y along Z.
fn front() -> Plane {
    NamedView::Front.basis()
}

/// From the (1, -1, 1) corner.
fn iso() -> Plane {
    NamedView::IsoFrontRight.basis()
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-12 * b.abs().max(1.0)
}

fn reads(m: Result<Measured>) -> (f64, bool) {
    let m = m.unwrap();
    (m.value, m.foreshortened)
}

fn degrees(m: Result<Measured>) -> f64 {
    m.unwrap().value.to_degrees()
}

fn refused<T: std::fmt::Debug>(result: Result<T>, says: &str) {
    match result {
        Err(Invalid(message)) => assert!(message.contains(says), "{message:?} doesn't say {says:?}"),
        other => panic!("expected a refusal saying {says:?}, got {other:?}"),
    }
}

/// From (0, 0, 0) to (30, 0, 40) in the front view: 30 across, 40 up, 50 aligned, 70/√2 along
/// (1, 1), 30 along (-2, 0); none foreshortened, and the true length is 50 too.
#[test]
fn a_segment_in_the_views_plane_reads_its_lengths() {
    let (a, b) = (Vec3::ZERO, Vec3::new(30.0, 0.0, 40.0));
    let cases = [
        (Horizontal, 30.0),
        (Vertical, 40.0),
        (Aligned, 50.0),
        (Along(Vec2::new(1.0, 1.0)), 70.0 / 2f64.sqrt()),
        (Along(Vec2::new(-2.0, 0.0)), 30.0),
    ];
    for (direction, value) in cases {
        let (v, foreshortened) = reads(linear(&front(), a, b, direction, Projected));
        assert!(close(v, value) && !foreshortened, "{direction:?}: {v}");
    }
    assert_eq!(reads(linear(&front(), a, b, Aligned, True)), (50.0, false));
}

/// The same segment 20 mm deeper at one end, to (30, 20, 40): still 30, 40 and 50 as the view
/// shows it, now foreshortened; its true length √2900. Scale and place don't enter: a view moved
/// along its normal reads the same.
#[test]
fn a_segment_out_of_the_views_plane_is_foreshortened() {
    let (a, b) = (Vec3::ZERO, Vec3::new(30.0, 20.0, 40.0));
    for (direction, value) in [(Horizontal, 30.0), (Vertical, 40.0), (Aligned, 50.0)] {
        let (v, foreshortened) = reads(linear(&front(), a, b, direction, Projected));
        assert!(close(v, value) && foreshortened, "{direction:?}: {v}");
    }
    let (v, foreshortened) = reads(linear(&front(), a, b, Aligned, True));
    assert!(close(v, 2900f64.sqrt()) && foreshortened);
    let moved = Plane { origin: Vec3::new(5.0, -300.0, 7.0), ..front() };
    let (v, foreshortened) = reads(linear(&moved, a, b, Aligned, Projected));
    assert!(close(v, 50.0) && foreshortened, "{v}");
}

/// A 50 mm cube edge along X in the front-right isometric view: 50·√(2/3) as drawn, 50 true,
/// foreshortened either way.
#[test]
fn a_cube_edge_in_an_isometric_view() {
    let edge = (Vec3::ZERO, Vec3::new(50.0, 0.0, 0.0));
    let (drawn, foreshortened) = reads(linear(&iso(), edge.0, edge.1, Aligned, Projected));
    assert!(close(drawn, 50.0 * (2.0f64 / 3.0).sqrt()) && foreshortened, "{drawn}");
    let (true_length, foreshortened) = reads(linear(&iso(), edge.0, edge.1, Aligned, True));
    assert!(close(true_length, 50.0) && foreshortened);
}

/// A 30° bevel in the front view, from its low end: 30° to the horizontal, 60° to the vertical,
/// 30° to the X axis and 60° to a vertical edge, none foreshortened; from its high end it reads
/// the other side, 150° to the horizontal. The true angle to X is 30° too; to an axis leaning out
/// of the view, (1, 1, 0), it is 30° as drawn and foreshortened. The Y axis is seen end-on in
/// this view, so an angle to it is refused.
#[test]
fn a_bevel_reads_its_angles_to_the_view_and_the_model() {
    let rise = 30f64.to_radians();
    let bevel = (Vec3::ZERO, Vec3::new(rise.cos(), 0.0, rise.sin()) * 10.0);
    let upright = Edge(Vec3::ZERO, Vec3::new(0.0, 0.0, 10.0));
    for (from, value) in [(AngleFrom::Horizontal, 30.0), (AngleFrom::Vertical, 60.0), (ModelAxis(Vec3::X), 30.0), (upright, 60.0)] {
        let m = angle(&front(), bevel, from, Projected).unwrap();
        assert!(close(m.value.to_degrees(), value) && !m.foreshortened, "{from:?}: {}", m.value.to_degrees());
    }
    assert!(close(degrees(angle(&front(), (bevel.1, bevel.0), AngleFrom::Horizontal, Projected)), 150.0));
    assert!(close(degrees(angle(&front(), bevel, ModelAxis(Vec3::X), True)), 30.0));
    let leaning = angle(&front(), bevel, ModelAxis(Vec3::new(1.0, 1.0, 0.0)), Projected).unwrap();
    assert!(close(leaning.value.to_degrees(), 30.0) && leaning.foreshortened, "an axis leaning out of the view: {leaning:?}");
    refused(angle(&front(), bevel, ModelAxis(Vec3::Y), Projected), "end-on");
}

/// The X and Z axes in the front-right isometric view (both toward the viewer) are 120° apart as
/// drawn and 90° in the model, foreshortened; X and Y, one away from the viewer, are 60° as drawn.
#[test]
fn model_axes_in_an_isometric_view() {
    let x = (Vec3::ZERO, Vec3::X * 50.0);
    let drawn = angle(&iso(), x, ModelAxis(Vec3::Z), Projected).unwrap();
    assert!(close(drawn.value.to_degrees(), 120.0) && drawn.foreshortened, "{drawn:?}");
    assert!(close(degrees(angle(&iso(), x, ModelAxis(Vec3::Z), True)), 90.0));
    assert!(close(degrees(angle(&iso(), x, Edge(Vec3::ZERO, Vec3::Y), Projected)), 60.0));
}

/// What can't be measured, each refused with its reason.
#[test]
fn what_cant_be_measured_is_refused() {
    let (a, b) = (Vec3::ZERO, Vec3::new(30.0, 0.0, 40.0));
    for p in [Vec3::new(f64::NAN, 0.0, 0.0), Vec3::new(0.0, f64::INFINITY, 0.0), Vec3::new(0.0, 0.0, 1.000_000_1e12)] {
        refused(linear(&front(), a, p, Aligned, Projected), "model point");
        refused(angle(&front(), (p, b), AngleFrom::Horizontal, Projected), "model point");
    }
    linear(&front(), Vec3::new(-1e12, 0.0, 0.0), Vec3::new(1e12, 0.0, 0.0), Horizontal, Projected).unwrap();
    refused(linear(&front(), a, a, Aligned, True), "no length");
    refused(linear(&front(), a, a + Vec3::X * 1e-10, Horizontal, Projected), "no length");
    let skewed = Plane { x: Vec3::new(2.0, 0.0, 0.0), ..front() };
    refused(linear(&skewed, a, b, Horizontal, Projected), "basis");
    refused(angle(&skewed, (a, b), AngleFrom::Horizontal, Projected), "basis");
    for along in [Vec2::ZERO, Vec2::new(f64::NAN, 1.0), Vec2::new(f64::INFINITY, 0.0)] {
        refused(linear(&front(), a, b, Along(along), Projected), "direction to measure along");
    }
    refused(linear(&front(), a, Vec3::new(0.0, 0.0, 40.0), Horizontal, Projected), "would read 0");
    refused(linear(&front(), a, Vec3::new(0.0, 25.0, 0.0), Aligned, Projected), "would read 0");
    for direction in [Horizontal, Vertical, Along(Vec2::X)] {
        refused(linear(&front(), a, b, direction, True), "true length");
    }
    let edge = (a, b);
    refused(angle(&front(), edge, Edge(a, b * 2.0), Projected), "parallel");
    refused(angle(&front(), edge, Edge(b, a), Projected), "parallel");
    refused(angle(&front(), edge, Edge(b, a), True), "parallel");
    refused(angle(&front(), (a, Vec3::new(0.0, 25.0, 0.0)), AngleFrom::Horizontal, Projected), "end-on");
    refused(angle(&front(), edge, AngleFrom::Horizontal, True), "true angle");
    refused(angle(&front(), edge, AngleFrom::Vertical, True), "true angle");
    for axis in [Vec3::ZERO, Vec3::new(f64::NAN, 0.0, 1.0), Vec3::new(1e308, 1e308, 0.0)] {
        refused(angle(&front(), edge, ModelAxis(axis), Projected), "model axis");
    }
    refused(angle(&front(), edge, Edge(a, a), Projected), "no length");
    // A chain is 3 points or more, each past the one before along its line.
    let (p, q) = (Vec3::new(10.0, 0.0, 0.0), Vec3::new(20.0, 0.0, 5.0));
    refused(chain(&front(), &[], Horizontal), "no points");
    refused(chain(&front(), &[a, p], Horizontal), "at least 3 points");
    refused(chain(&front(), &[a, q, p], Horizontal), "one way");
    refused(chain(&front(), &[a, Vec3::new(0.0, 0.0, 9.0), q], Horizontal), "one way");
    refused(chain(&front(), &[a, p, p, q], Horizontal), "no length");
    refused(chain(&front(), &[a, p, Vec3::new(0.0, 30.0, 0.0)], Aligned), "one point in this view");
    refused(chain(&front(), &[a, p, q], Along(Vec2::ZERO)), "direction to measure along");
    refused(chain(&front(), &[a, Vec3::new(f64::NAN, 0.0, 0.0), q], Horizontal), "model point");
    refused(chain(&skewed, &[a, p, q], Horizontal), "basis");
}

/// A corner at 0 and hole centres at 10, 30 and 60 along the front view's horizontal read 10, 20
/// and 30, adding up to the overall 60; given the other way round, the members come last to first,
/// and a direction given against the chain's way reads the same. Aligned up a slope, each member is
/// its own part of the line, and a point behind the view's plane makes its members, and so the
/// chain, foreshortened.
#[test]
fn a_chain_reads_each_point_to_the_next_along_one_line() {
    let values = |c: Chain| c.members.iter().map(|m| m.value).collect::<Vec<_>>();
    let holes = [Vec3::ZERO, Vec3::new(10.0, 0.0, 15.0), Vec3::new(30.0, 0.0, 15.0), Vec3::new(60.0, 0.0, 15.0)];
    let across = chain(&front(), &holes, Horizontal).unwrap();
    assert_eq!(across.overall, Measured { value: 60.0, foreshortened: false });
    assert!((across.members.iter().map(|m| m.value).sum::<f64>() - 60.0).abs() <= 1e-9);
    assert_eq!(values(across), [10.0, 20.0, 30.0]);
    let mut back = holes;
    back.reverse();
    assert_eq!(values(chain(&front(), &back, Horizontal).unwrap()), [30.0, 20.0, 10.0]);
    assert_eq!(values(chain(&front(), &holes, Along(Vec2::new(-3.0, 0.0))).unwrap()), [10.0, 20.0, 30.0]);
    // Up a 3-4-5 slope, then with its third point 7 behind the view's plane.
    let slope = [Vec3::ZERO, Vec3::new(3.0, 0.0, 4.0), Vec3::new(9.0, 0.0, 12.0), Vec3::new(12.0, 0.0, 16.0)];
    let parts = |c: Chain| values(c).iter().zip([5.0, 10.0, 5.0]).all(|(v, part)| close(*v, part));
    assert!(parts(chain(&front(), &slope, Aligned).unwrap()));
    let mut behind = slope;
    behind[2].y = 7.0;
    let tilted = chain(&front(), &behind, Aligned).unwrap();
    assert_eq!(tilted.members.iter().map(|m| m.foreshortened).collect::<Vec<_>>(), [false, true, true]);
    assert!(tilted.overall.foreshortened && close(tilted.overall.value, 20.0), "{tilted:?}");
    assert!(parts(tilted), "as the view shows them");
}

/// A cylinder along X seen from the front has its silhouettes above and below its axis, and so does
/// one leaning back toward Y; in the isometric view every way is square to the axis and the view's
/// normal. Seen along its axis, a cylinder has no silhouettes.
#[test]
fn a_cylinders_silhouettes_are_square_to_its_axis_and_the_view() {
    let up = silhouette_side(&front(), Vec3::X * 5.0).unwrap();
    assert!(up.dist(Vec3::Z) < 1e-12 || up.dist(-Vec3::Z) < 1e-12, "{up:?}");
    let leaning = silhouette_side(&front(), Vec3::new(1.0, 1.0, 0.0)).unwrap();
    assert!(leaning.cross(Vec3::Z).len() < 1e-12, "{leaning:?}");
    for axis in [Vec3::X, Vec3::Y, Vec3::Z, Vec3::new(1.0, -2.0, 0.5)] {
        let side = silhouette_side(&iso(), axis).unwrap();
        assert!(close(side.len(), 1.0) && side.dot(axis).abs() < 1e-12 && side.dot(iso().normal()).abs() < 1e-12, "{axis:?}");
    }
    refused(silhouette_side(&front(), Vec3::Y), "end-on");
    refused(silhouette_side(&front(), Vec3::new(0.0, -3.0, 0.0)), "end-on");
    refused(silhouette_side(&front(), Vec3::new(1e-12, 1.0, 0.0)), "end-on");
    refused(silhouette_side(&front(), Vec3::ZERO), "axis");
    refused(silhouette_side(&front(), Vec3::new(f64::NAN, 0.0, 1.0)), "axis");
    refused(silhouette_side(&Plane { x: Vec3::new(2.0, 0.0, 0.0), ..front() }, Vec3::X), "basis");
}

fn cylinder(origin: Vec3, axis: Vec3, radius: f64) -> Cylinder {
    Cylinder { origin, axis, radius }
}

/// Two shafts along X in the front view, radius 5 at the origin and radius 3 at z = 30: near to
/// near 22, near to far 28, far to near 32, far to far 38, whichever is given first; the same up
/// the view and 22√2 along (1, 1). Behind the first, or leaning back, the second still reads 22,
/// foreshortened. In the isometric view, two posts along Z read 10√2 between their axes.
#[test]
fn two_cylinders_read_between_the_silhouettes_given() {
    use SilhouetteSide::{Far, Near};
    let (a, b) = (cylinder(Vec3::ZERO, Vec3::X, 5.0), cylinder(Vec3::new(0.0, 0.0, 30.0), Vec3::X * 2.0, 3.0));
    let read = |a: &Cylinder, a_side, b: &Cylinder, b_side, direction| between_silhouettes(&front(), a, a_side, b, b_side, direction);
    let is = |read: Result<(Measured, [Vec3; 2])>, value, foreshortened| {
        let (v, f) = reads(read.map(|(m, _)| m));
        assert!(close(v, value) && f == foreshortened, "{v} {f}, not {value} {foreshortened}");
    };
    for (a_side, b_side, value) in [(Near, Near, 22.0), (Near, Far, 28.0), (Far, Near, 32.0), (Far, Far, 38.0)] {
        is(read(&a, a_side, &b, b_side, Aligned), value, false);
        is(read(&b, b_side, &a, a_side, Vertical), value, false);
    }
    let (m, ways) = read(&a, Near, &b, Near, Along(Vec2::new(1.0, 1.0))).unwrap();
    assert!(close(m.value, 22.0 * 2f64.sqrt()) && ways[0].dist(Vec3::Z) < 1e-12 && ways[1].dist(-Vec3::Z) < 1e-12, "{m:?} {ways:?}");
    let behind = cylinder(Vec3::new(0.0, 40.0, 30.0), -Vec3::X, 3.0);
    is(read(&a, Near, &behind, Near, Aligned), 22.0, true);
    let leaning = cylinder(Vec3::new(0.0, 0.0, 30.0), Vec3::new(1.0, 1.0, 0.0), 3.0);
    is(read(&a, Near, &leaning, Near, Aligned), 22.0, true);
    let posts = (cylinder(Vec3::ZERO, Vec3::Z, 2.0), cylinder(Vec3::new(10.0, 10.0, -7.0), Vec3::Z, 1.0));
    is(between_silhouettes(&iso(), &posts.0, Near, &posts.1, Near, Aligned), 200f64.sqrt() - 3.0, false);

    refused(read(&a, Near, &cylinder(Vec3::new(30.0, 0.0, 0.0), Vec3::Z, 3.0), Near, Aligned), "aren't parallel");
    refused(read(&a, Near, &cylinder(Vec3::new(20.0, 10.0, 0.0), Vec3::X, 3.0), Near, Aligned), "one line in this view");
    refused(read(&a, Near, &cylinder(Vec3::new(0.0, 0.0, 10.0), Vec3::X, 5.0), Near, Aligned), "would read 0");
    refused(read(&a, Near, &b, Near, Horizontal), "run along the direction");
    refused(read(&a, Near, &b, Near, Along(Vec2::ZERO)), "zero or not finite");
    refused(read(&cylinder(Vec3::ZERO, Vec3::Y, 5.0), Near, &b, Near, Aligned), "end-on");
    refused(read(&a, Near, &cylinder(b.origin, Vec3::ZERO, 3.0), Near, Aligned), "axis");
    for radius in [0.0, -1.0, f64::NAN, f64::INFINITY, 2e12] {
        refused(read(&a, Near, &cylinder(b.origin, Vec3::X, radius), Near, Aligned), "a radius over 0");
    }
    refused(read(&cylinder(Vec3::new(f64::NAN, 0.0, 0.0), Vec3::X, 5.0), Near, &b, Near, Aligned), "a radius over 0");
    refused(read(&cylinder(Vec3::new(2e12, 0.0, 0.0), Vec3::X, 5.0), Near, &b, Near, Aligned), "a radius over 0");
    refused(between_silhouettes(&Plane { x: Vec3::new(2.0, 0.0, 0.0), ..front() }, &a, Near, &b, Near, Aligned), "basis");
}

fn random(state: &mut u64) -> u64 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    *state
}

/// Points, edges, bases and directions, three cases in four tidy (the named views and views
/// looking any way, points within a metre) and the rest hostile (non-finite, huge, repeated,
/// along the view's normal), 20,000 seeded cases: nothing panics; across² + up² = aligned²; a
/// projected aligned length is never more than the true one; every angle is from 0 to π, and a
/// true angle and its projection agree in a view square to both lines; a chain's members are each
/// more than 0 and add up to its overall; between two cylinders parallel on the view, far to far
/// is the most of the four, and aligned the least of the directions.
#[test]
fn measurements_seeded_never_panic_and_keep_their_rules() {
    let mut seed = 0x4896_19a0_d1e5_7c3b;
    let hostile = [0.0, -0.0, 1e-10, 1e12, 1.1e12, f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 1e300, -5.0];
    let (mut lengths, mut angles, mut chains, mut silhouettes) = (0, 0, 0, 0);
    for case in 0..20_000 {
        let tidy = !random(&mut seed).is_multiple_of(4);
        let number = |seed: &mut u64| {
            if tidy {
                (random(seed) % 2_001) as f64 / 2.0 - 500.0
            } else {
                hostile.get((random(seed) % hostile.len() as u64) as usize).copied().unwrap_or(0.0)
            }
        };
        let point = |seed: &mut u64| Vec3::new(number(seed), number(seed), number(seed));
        let basis = match random(&mut seed) % 3 {
            0 => NamedView::ALL.get((random(&mut seed) % 10) as usize).map(|v| v.basis()).unwrap_or_else(front),
            1 => basis_looking(point(&mut seed), Vec3::Z, Vec3::ZERO).unwrap_or_else(|_| iso()),
            _ => Plane { origin: point(&mut seed), x: point(&mut seed), y: point(&mut seed) },
        };
        let (a, b, c, d) = (point(&mut seed), point(&mut seed), point(&mut seed), point(&mut seed));
        let along = Vec2::new(number(&mut seed), number(&mut seed));
        let read = |direction| linear(&basis, a, b, direction, Projected).map(|m| m.value);
        if let (Ok(h), Ok(v), Ok(aligned)) = (read(Horizontal), read(Vertical), read(Aligned)) {
            lengths += 1;
            assert!((h * h + v * v - aligned * aligned).abs() <= 1e-9 * aligned * aligned, "case {case}");
            let true_length = linear(&basis, a, b, Aligned, True).map(|m| m.value);
            assert!(true_length.is_ok_and(|t| aligned <= t * (1.0 + 1e-12)), "case {case}");
        }
        let _ = linear(&basis, a, b, Along(along), Projected);
        for direction in [Horizontal, Vertical, Aligned, Along(along)] {
            if let Ok(chained) = chain(&basis, &[a, b, c, d], direction) {
                chains += 1;
                let sum: f64 = chained.members.iter().map(|m| m.value).sum();
                assert!(chained.members.iter().all(|m| m.value > EPS), "case {case}: {chained:?}");
                assert!((sum - chained.overall.value).abs() <= 1e-9 * chained.overall.value.max(1.0), "case {case}: {chained:?}");
            }
        }
        for from in [AngleFrom::Horizontal, AngleFrom::Vertical, ModelAxis(c - d), Edge(c, d)] {
            for reading in [Projected, True] {
                if let Ok(m) = angle(&basis, (a, b), from, reading) {
                    angles += 1;
                    assert!(m.value > 0.0 && m.value < std::f64::consts::PI, "case {case}: {m:?}");
                }
            }
        }
        // The second axis is the first, leaning toward or from the view, so the two are parallel there.
        let first = Cylinder { origin: a, axis: c - d, radius: number(&mut seed).abs() };
        let second = Cylinder { origin: b, axis: (c - d) + basis.normal() * number(&mut seed), radius: number(&mut seed).abs() };
        let read = |a_side, b_side, direction| between_silhouettes(&basis, &first, a_side, &second, b_side, direction).map(|(m, _)| m.value).ok();
        let far = read(SilhouetteSide::Far, SilhouetteSide::Far, Aligned);
        let sides = [SilhouetteSide::Near, SilhouetteSide::Far];
        for (a_side, b_side) in sides.iter().flat_map(|a| sides.map(|b| (*a, b))) {
            let aligned = read(a_side, b_side, Aligned);
            if let (Some(v), Some(far)) = (aligned, far) {
                silhouettes += 1;
                assert!(v.is_finite() && v > EPS && v <= far * (1.0 + 1e-9), "case {case}: {v} {far}");
            }
            for direction in [Horizontal, Vertical, Along(along)] {
                if let (Some(v), Some(slanted)) = (aligned, read(a_side, b_side, direction)) {
                    assert!(v <= slanted * (1.0 + 1e-9), "case {case}: {v} {slanted}");
                }
            }
        }
        if let (Ok(drawn), Ok(model)) = (angle(&basis, (a, b), Edge(c, d), Projected), angle(&basis, (a, b), Edge(c, d), True))
            && !drawn.foreshortened
        {
            assert!((drawn.value - model.value).abs() <= 1e-6, "case {case}: {drawn:?} {model:?}");
        }
    }
    assert!(lengths > 5_000 && angles > 20_000 && chains > 2_000 && silhouettes > 20_000, "{lengths} {angles} {chains} {silhouettes}");
}
