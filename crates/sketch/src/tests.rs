use solidcraft_geom::prelude::*;
use std::f64::consts::{FRAC_PI_2, PI};

use solidcore_record::{Plane, Seg2, Vec2, Vec3};

use crate::*;

pub(crate) fn v(x: f64, y: f64) -> Vec2 {
    Vec2::new(x, y)
}

/// Deterministic pseudo-random numbers for the solver harnesses.
pub(crate) struct Lcg(pub u64);

impl Lcg {
    pub(crate) fn next(&mut self) -> f64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
    pub(crate) fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + self.next() * (hi - lo)
    }
    /// ±1, evenly.
    pub(crate) fn sign(&mut self) -> f64 {
        if self.next() < 0.5 { 1.0 } else { -1.0 }
    }
}

thread_local! {
    static ID_SEARCHES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// A linear search for a point or curve by id on this thread.
pub(crate) fn count_id_search() {
    ID_SEARCHES.with(|c| c.set(c.get() + 1));
}

thread_local! {
    static ID_SCANS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// A search of every id in a sketch (is this id taken?) on this thread.
pub(crate) fn count_id_scan() {
    ID_SCANS.with(|c| c.set(c.get() + 1));
}

/// Run `f` with the constructive seed on or off, without the seed-on/seed-off comparison (for
/// tests that measure the difference).
pub(crate) fn seeded<T>(on: bool, f: impl FnOnce() -> T) -> T {
    use crate::construct::switch;
    switch::without_differential(|| if on { f() } else { switch::without_seed(f) })
}

/// The points that are constants for a plain solve.
pub(crate) fn given(sk: &Sketch) -> Vec<bool> {
    crate::solver::held_points(sk, &Hold::default())
}

/// The constructive seed of a plain solve.
pub(crate) fn seed_of(sk: &Sketch) -> crate::construct::Seed {
    crate::construct::seed(sk, &given(sk), None, true, &crate::solver::points_on_curves(sk), &mut crate::decompose::Budget::full())
}

/// Rough rectangle from four lines sharing corner points.
fn rect(sk: &mut Sketch, p: [Vec2; 4]) -> [usize; 4] {
    let a = sk.add_point(p[0], None).unwrap();
    let b = sk.add_point(p[1], None).unwrap();
    let c = sk.add_point(p[2], None).unwrap();
    let d = sk.add_point(p[3], None).unwrap();
    [
        sk.add_line_pts(a, b, None).unwrap(),
        sk.add_line_pts(b, c, None).unwrap(),
        sk.add_line_pts(c, d, None).unwrap(),
        sk.add_line_pts(d, a, None).unwrap(),
    ]
}

fn line_pts(sk: &Sketch, l: usize) -> (Vec2, Vec2) {
    match sk.curves[l].kind {
        CurveKind::Line { a, b } => (sk.points[a].pos, sk.points[b].pos),
        _ => panic!(),
    }
}

#[test]
fn fully_constrained_rectangle() {
    let mut sk = Sketch::new();
    let l = rect(&mut sk, [v(0.3, -0.2), v(38.0, 1.0), v(41.0, 29.0), v(-1.0, 31.0)]);
    use ConstraintKind::*;
    sk.add_constraint(Horizontal { l: l[0] }, None).unwrap();
    sk.add_constraint(Horizontal { l: l[2] }, None).unwrap();
    sk.add_constraint(Vertical { l: l[1] }, None).unwrap();
    sk.add_constraint(Vertical { l: l[3] }, None).unwrap();
    let (a, _) = match sk.curves[l[0]].kind {
        CurveKind::Line { a, b } => (a, b),
        _ => panic!(),
    };
    sk.add_constraint(Coincident { p: a, q: 0 }, None).unwrap();
    sk.add_constraint(Length { l: l[0], value: 40.0 }, Some("d1".into())).unwrap();
    sk.add_constraint(Length { l: l[1], value: 30.0 }, Some("d2".into())).unwrap();
    let rep = solve(&mut sk);
    assert!(rep.ok(), "{rep:?}");
    assert_eq!(rep.dof, 0, "{rep:?}");
    assert!(rep.point_determined.iter().all(|d| *d));
    let (p0, p1) = line_pts(&sk, l[0]);
    assert!(p0.dist(v(0.0, 0.0)) < 1e-7, "{p0:?}");
    assert!(p1.dist(v(40.0, 0.0)) < 1e-7, "{p1:?}");
    let (_, p2) = line_pts(&sk, l[1]);
    assert!(p2.dist(v(40.0, 30.0)) < 1e-7, "{p2:?}");
    // Change a dimension and re-solve.
    sk.constraints[5].kind.set_value(55.0);
    assert!(solve(&mut sk).ok());
    let (_, p1) = line_pts(&sk, l[0]);
    assert!(p1.dist(v(55.0, 0.0)) < 1e-7);
}

#[test]
fn zero_distance_matches_coincident() {
    for seed in [false, true] {
        seeded(seed, || {
            for fixed in [false, true] {
                for value in [0.0, 1e-12] {
                    let mut sk = Sketch::new();
                    let p = if fixed { 0 } else { sk.add_point(v(2.0, 3.0), None).unwrap() };
                    let q = sk.add_point(sk.point(p).unwrap(), None).unwrap();
                    let mut equivalent = sk.clone();
                    let id = sk.add_constraint(ConstraintKind::Distance { p, q, value }, None).unwrap();
                    equivalent.add_constraint(ConstraintKind::Coincident { p, q }, None).unwrap();
                    let report = solve(&mut sk);
                    let expected = solve(&mut equivalent);
                    assert!(report.ok() && expected.ok(), "{report:?}, {expected:?}");
                    assert_eq!(report.dof, if fixed { 0 } else { 2 }, "{report:?}");
                    assert_eq!(report.dof, expected.dof);
                    assert_eq!(report.point_determined, expected.point_determined);
                    assert!(!report.redundant.contains(&id), "{report:?}");
                    assert!(determined_by(&sk, 0).is_empty());
                    assert!(sk.point(q).unwrap().dist(equivalent.point(q).unwrap()) < 1e-8);
                    sk.add_constraint(ConstraintKind::Coincident { p, q }, None).unwrap();
                    assert_eq!(solve(&mut sk).redundant.len(), 1);
                    assert_eq!(determined_by(&sk, 1), vec![0]);
                }
            }
        });
    }
}

#[test]
fn zero_distance_to_line_matches_point_on_line() {
    for seed in [false, true] {
        seeded(seed, || {
            for value in [0.0, 1e-12] {
                for start in [v(2.0, 2.0), v(3.0, 1.0)] {
                    let mut sk = Sketch::new();
                    let l = sk.add_line(v(1.0, 1.0), v(4.0, 4.0), None, None, None).unwrap();
                    sk.curves[l].fixed = true;
                    let p = sk.add_point(start, None).unwrap();
                    let mut equivalent = sk.clone();
                    let id = sk.add_constraint(ConstraintKind::PointLineDistance { p, l, value }, None).unwrap();
                    equivalent.add_constraint(ConstraintKind::PointOnCurve { p, c: l }, None).unwrap();
                    let report = solve(&mut sk);
                    let expected = solve(&mut equivalent);
                    assert!(report.ok() && expected.ok(), "{report:?}, {expected:?}");
                    assert_eq!(report.dof, 1, "{report:?}");
                    assert_eq!(report.dof, expected.dof);
                    assert_eq!(report.point_determined, expected.point_determined);
                    assert!(!report.redundant.contains(&id), "{report:?}");
                    assert!(determined_by(&sk, 0).is_empty());
                    assert!(sk.point(p).unwrap().dist(equivalent.point(p).unwrap()) < 1e-8);
                    sk.add_constraint(ConstraintKind::PointOnCurve { p, c: l }, None).unwrap();
                    assert_eq!(solve(&mut sk).redundant.len(), 1);
                    assert_eq!(determined_by(&sk, 1), vec![0]);
                }
            }
        });
    }
}

#[test]
fn zero_distance_edits_preserve_dimension_rank() {
    for seed in [false, true] {
        seeded(seed, || {
            for point_line in [false, true] {
                for direction in [Vec2::X, Vec2::Y, v(1.0, -1.0) / 2.0_f64.sqrt()] {
                    let mut sk = Sketch::new();
                    let p = sk.add_point(direction * 5.0, None).unwrap();
                    let kind = if point_line {
                        let along = direction.perp() * 10.0;
                        let l = sk.add_line(along * -1.0, along, None, None, None).unwrap();
                        sk.curves[l].fixed = true;
                        ConstraintKind::PointLineDistance { p, l, value: 5.0 }
                    } else {
                        ConstraintKind::Distance { p: 0, q: p, value: 5.0 }
                    };
                    let id = sk.add_constraint(kind, None).unwrap();
                    let alignment = if direction == Vec2::X {
                        ConstraintKind::HorizontalPoints { p: 0, q: p }
                    } else if direction == Vec2::Y {
                        ConstraintKind::VerticalPoints { p: 0, q: p }
                    } else {
                        let l = sk.add_line(direction * -10.0, direction * 10.0, None, None, None).unwrap();
                        sk.curves[l].fixed = true;
                        ConstraintKind::PointOnCurve { p, c: l }
                    };
                    sk.add_constraint(alignment, None).unwrap();
                    for value in [5.0, 0.0, 5.0] {
                        let old = sk.driving_values();
                        sk.constraints[0].kind.set_value(value);
                        let report = solve_after_value_edit(&mut sk, &old);
                        assert!(report.ok(), "{point_line}, {direction:?}, {value}: {report:?}");
                        assert_eq!(report.dof, 0, "{point_line}, {direction:?}, {value}: {report:?}");
                        assert!(report.point_determined[p]);
                        assert!(!report.redundant.contains(&id), "{report:?}");
                        assert!(determined_by(&sk, 0).is_empty());
                        assert!(constraint_residual(&sk, 0).unwrap() < 1e-6);
                        assert!((sk.point(p).unwrap().len() - value).abs() < 1e-6);
                    }
                }
            }
        });
    }
}

/// A horizontal or vertical distance edited from 0 to a positive value moves the point off the
/// axis again, on the side the dimension measures towards (`q` past the origin).
#[test]
fn component_distance_edited_from_zero_opens_again() {
    for seed in [false, true] {
        seeded(seed, || {
            for vertical in [false, true] {
                let mut sk = Sketch::new();
                let q = sk.add_point(v(3.0, 4.0), None).unwrap();
                let (edited, other) = if vertical {
                    (ConstraintKind::DistanceY { p: 0, q, value: 4.0 }, ConstraintKind::DistanceX { p: 0, q, value: 3.0 })
                } else {
                    (ConstraintKind::DistanceX { p: 0, q, value: 3.0 }, ConstraintKind::DistanceY { p: 0, q, value: 4.0 })
                };
                let id = sk.add_constraint(edited, None).unwrap();
                sk.add_constraint(other, None).unwrap();
                let last = sk.constraints.len() - 2;
                for value in [0.0, 0.5, 0.0, 3.0] {
                    let old = sk.driving_values();
                    sk.constraints[last].kind.set_value(value);
                    let report = solve_after_value_edit(&mut sk, &old);
                    assert!(report.ok(), "{vertical}, {value}: {report:?}");
                    assert_eq!(report.dof, 0, "{vertical}, {value}: {report:?}");
                    assert!(!report.redundant.contains(&id), "{report:?}");
                    assert!(constraint_residual(&sk, last).unwrap() < 1e-6);
                    let at = sk.point(q).unwrap();
                    assert!(((if vertical { at.y } else { at.x }) - value).abs() < 1e-6, "{vertical}, {value}: {at:?}");
                }
            }
        });
    }
}

/// A length dimension of zero still constrains the sketch: the two ends of the line meet, and
/// the dimension holds them together instead of leaving a degree of freedom behind (#5505).
#[test]
fn zero_length_keeps_the_line_constrained() {
    for seed in [false, true] {
        seeded(seed, || {
            let mut sk = Sketch::new();
            let a = sk.add_point(v(0.0, 0.0), None).unwrap();
            let b = sk.add_point(v(10.0, 0.0), None).unwrap();
            let l = sk.add_line_pts(a, b, None).unwrap();
            sk.add_constraint(ConstraintKind::Fix { p: a }, None).unwrap();
            let id = sk.add_constraint(ConstraintKind::Length { l, value: 0.0 }, None).unwrap();
            let report = solve(&mut sk);
            assert!(report.ok(), "{report:?}");
            assert_eq!(report.dof, 0, "a zero length holds the line's ends together: {report:?}");
            assert!(!report.redundant.contains(&id), "a zero length is not redundant: {report:?}");
            assert!(sk.point(b).unwrap().dist(v(0.0, 0.0)) < 1e-6, "{:?}", sk.point(b));
        });
    }
}

#[test]
fn under_constrained_reports_dof() {
    let mut sk = Sketch::new();
    let l = sk.add_line(v(0.0, 0.0), v(10.0, 1.0), None, None, None).unwrap();
    sk.add_constraint(ConstraintKind::Horizontal { l }, None).unwrap();
    let rep = solve(&mut sk);
    assert!(rep.ok());
    assert_eq!(rep.dof, 3);
    let (a, b) = line_pts(&sk, l);
    assert!((a.y - b.y).abs() < 1e-9);
    // Minimal movement: the line moved by about half a unit each.
    assert!((a.y - 0.5).abs() < 0.05 && (b.y - 0.5).abs() < 0.05, "{a:?} {b:?}");
}

#[test]
fn conflicting_constraints_fail_and_restore() {
    let mut sk = Sketch::new();
    let l = sk.add_line(v(0.0, 0.0), v(10.0, 0.0), None, None, None).unwrap();
    sk.add_constraint(ConstraintKind::Horizontal { l }, None).unwrap();
    sk.add_constraint(ConstraintKind::Vertical { l }, None).unwrap();
    sk.add_constraint(ConstraintKind::Length { l, value: 10.0 }, None).unwrap();
    let before = sk.clone();
    let rep = solve(&mut sk);
    assert_eq!(rep.status, SolveStatus::Failed);
    assert!(!rep.failing.is_empty());
    assert_eq!(sk.points, before.points);
}

#[test]
fn circles_tangent_equal_concentric() {
    let mut sk = Sketch::new();
    let c1 = sk.add_circle(v(0.0, 0.0), 5.0, Some(0), None).unwrap();
    let c2 = sk.add_circle(v(12.0, 1.0), 3.0, None, None).unwrap();
    use ConstraintKind::*;
    sk.add_constraint(Radius { c: c1, value: 10.0 }, None).unwrap();
    sk.add_constraint(Equal { a: c1, b: c2 }, None).unwrap();
    sk.add_constraint(Tangent { a: c1, b: c2 }, None).unwrap();
    let rep = solve(&mut sk);
    assert!(rep.ok(), "{rep:?}");
    assert!((sk.radius(c2).unwrap() - 10.0).abs() < 1e-7);
    assert!((sk.center(c2).unwrap().len() - 20.0).abs() < 1e-7);
    let c3 = sk.add_circle(v(3.0, 3.0), 2.0, None, None).unwrap();
    sk.add_constraint(Concentric { a: c3, b: c2 }, None).unwrap();
    assert!(solve(&mut sk).ok());
    assert!(sk.center(c3).unwrap().dist(sk.center(c2).unwrap()) < 1e-7);
}

/// The least and greatest of `gap` over many points along a free-form curve, read from its own
/// definition (not the solver's polyline).
fn freeform_gap_range(sk: &Sketch, c: usize, gap: impl Fn(Vec2) -> f64) -> (f64, f64) {
    let p = |i: usize| sk.points[i].pos;
    let n = 20_000;
    let at: Box<dyn Fn(f64) -> Vec2> = match sk.curves[c].kind {
        CurveKind::Ellipse { c, m, r } => Box::new(move |t| crate::curves::ellipse_point(p(c), p(m), r, t * 2.0 * PI)),
        CurveKind::Conic { a, b, apex, rho } => Box::new(move |t| crate::curves::conic_point(p(a), p(apex), p(b), rho, t)),
        _ => panic!("not an ellipse or a conic"),
    };
    (0..=n).map(|i| gap(at(i as f64 / n as f64))).fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), g| (lo.min(g), hi.max(g)))
}

/// Touching means the gap reaches zero on one side only: `(least, greatest)` crossing zero is a
/// crossing.
fn assert_touches(what: &str, (lo, hi): (f64, f64)) {
    let near = if lo.abs() < hi.abs() { lo } else { hi };
    assert!(near.abs() < 1e-3 && (lo > -1e-3 || hi < 1e-3), "{what}: the gap runs from {lo} to {hi}");
}

/// A tangent to an ellipse or a conic keeps the curves touching, also once the geometry moves:
/// a crossing has a zero gap as well, and was taken for a touch (#11535).
#[test]
fn a_tangent_to_an_ellipse_or_a_conic_touches_and_never_crosses() {
    use ConstraintKind::*;
    let side = |a: Vec2, b: Vec2| move |q: Vec2| (b - a).cross(q - a) / a.dist(b);
    // A line on an ellipse (a 30, b 15), then the line's end dragged to (38, 6).
    let mut sk = Sketch::new();
    let (ec, em) = (sk.add_point(v(0.0, 0.0), None).unwrap(), sk.add_point(v(30.0, 0.0), None).unwrap());
    let e = sk.add_curve(CurveKind::Ellipse { c: ec, m: em, r: 15.0 }, None).unwrap();
    let l = sk.add_line(v(-20.0, 25.0), v(40.0, 20.0), None, None, None).unwrap();
    sk.add_constraint(Tangent { a: l, b: e }, None).unwrap();
    assert!(solve(&mut sk).ok());
    let (a, b) = line_pts(&sk, l);
    assert_touches("the line", freeform_gap_range(&sk, e, side(a, b)));
    let CurveKind::Line { b: end, .. } = sk.curves[l].kind else { panic!("a line") };
    sk.points[end].pos = v(38.0, 6.0);
    let hold = Hold { points: vec![ec, em, end], radii: vec![e] };
    assert!(solve_holding(&mut sk, &hold).ok());
    let (a, b) = line_pts(&sk, l);
    assert_touches("the line dragged", freeform_gap_range(&sk, e, side(a, b)));

    // A circle of radius 10 on the same ellipse, then the major axis's end dragged from 30 to 36.
    let mut sk = Sketch::new();
    let (ec, em) = (sk.add_point(v(0.0, 0.0), None).unwrap(), sk.add_point(v(30.0, 0.0), None).unwrap());
    let e = sk.add_curve(CurveKind::Ellipse { c: ec, m: em, r: 15.0 }, None).unwrap();
    let c = sk.add_circle(v(45.0, 12.0), 10.0, None, None).unwrap();
    sk.add_constraint(Radius { c, value: 10.0 }, None).unwrap();
    sk.add_constraint(Tangent { a: e, b: c }, None).unwrap();
    assert!(solve(&mut sk).ok());
    let round = |sk: &Sketch| {
        let (cc, r) = (sk.center(c).unwrap(), sk.radius(c).unwrap());
        move |q: Vec2| q.dist(cc) - r
    };
    assert_touches("the circle", freeform_gap_range(&sk, e, round(&sk)));
    sk.points[em].pos = v(36.0, 0.0);
    assert!(solve_holding(&mut sk, &Hold { points: vec![ec, em], radii: vec![e] }).ok());
    assert_touches("the circle on the longer ellipse", freeform_gap_range(&sk, e, round(&sk)));

    // A line on a conic arch, then the line's end dragged down to (45, 2).
    let mut sk = Sketch::new();
    let [ca, cb, apex] = [v(0.0, 0.0), v(40.0, 0.0), v(20.0, 20.0)].map(|p| sk.add_point(p, None).unwrap());
    let k = sk.add_curve(CurveKind::Conic { a: ca, b: cb, apex, rho: 0.5 }, None).unwrap();
    let l = sk.add_line(v(-10.0, 15.0), v(50.0, 12.0), None, None, None).unwrap();
    sk.add_constraint(Tangent { a: l, b: k }, None).unwrap();
    assert!(solve(&mut sk).ok());
    let (a, b) = line_pts(&sk, l);
    assert_touches("the line on the conic", freeform_gap_range(&sk, k, side(a, b)));
    let CurveKind::Line { b: end, .. } = sk.curves[l].kind else { panic!("a line") };
    sk.points[end].pos = v(45.0, 2.0);
    assert!(solve_holding(&mut sk, &Hold { points: vec![ca, cb, apex, end], radii: vec![] }).ok());
    let (a, b) = line_pts(&sk, l);
    assert_touches("the line on the conic dragged", freeform_gap_range(&sk, k, side(a, b)));

    // A line that crosses the arch from end to end (x = 20 through the apex's span): the gap is
    // monotone and ~0 at the crossing, which is not a touch.
    let mut sk = Sketch::new();
    let [ca, cb, apex] = [v(0.0, 0.0), v(40.0, 0.0), v(20.0, 20.0)].map(|p| sk.add_point(p, None).unwrap());
    let k = sk.add_curve(CurveKind::Conic { a: ca, b: cb, apex, rho: 0.5 }, None).unwrap();
    let l = sk.add_line(v(20.0, -10.0), v(20.0, 40.0), None, None, None).unwrap();
    sk.add_constraint(Tangent { a: l, b: k }, None).unwrap();
    // The solver may report it cannot do it, but it must never call the crossing a touch.
    if solve(&mut sk).ok() {
        let (a, b) = line_pts(&sk, l);
        assert_touches("the line across the conic", freeform_gap_range(&sk, k, side(a, b)));
    }
}

#[test]
fn line_tangent_to_circle_and_angles() {
    let mut sk = Sketch::new();
    let c = sk.add_circle(v(0.0, 0.0), 10.0, Some(0), None).unwrap();
    use ConstraintKind::*;
    sk.add_constraint(Radius { c, value: 10.0 }, None).unwrap();
    let l = sk.add_line(v(-20.0, 12.0), v(20.0, 11.0), None, None, None).unwrap();
    sk.add_constraint(Tangent { a: l, b: c }, None).unwrap();
    sk.add_constraint(Horizontal { l }, None).unwrap();
    assert!(solve(&mut sk).ok());
    let (a, _) = line_pts(&sk, l);
    assert!((a.y.abs() - 10.0).abs() < 1e-7, "{a:?}");
    let m = sk.add_line(v(0.0, 0.0), v(10.0, 3.0), Some(0), None, None).unwrap();
    sk.add_constraint(Angle { a: l, b: m, value: PI / 6.0, flip: false }, None).unwrap();
    sk.add_constraint(Length { l: m, value: 8.0 }, None).unwrap();
    assert!(solve(&mut sk).ok());
    let (p, q) = line_pts(&sk, m);
    let (la, lb) = line_pts(&sk, l);
    assert!(((q - p).angle() - (lb - la).angle() - PI / 6.0).abs() < 1e-7, "{la:?} {lb:?} {p:?} {q:?}");
    let n2 = sk.add_line(v(1.0, 1.0), v(2.0, 5.0), None, None, None).unwrap();
    sk.add_constraint(Perpendicular { a: m, b: n2 }, None).unwrap();
    sk.add_constraint(Parallel { a: n2, b: n2 }, None).unwrap();
    assert!(solve(&mut sk).ok());
    let (p2, q2) = line_pts(&sk, n2);
    assert!(((q2 - p2).angle() - (PI / 6.0 + FRAC_PI_2)).abs() < 1e-6 || ((q2 - p2).angle() - (PI / 6.0 - FRAC_PI_2)).abs() < 1e-6);
}

#[test]
fn midpoint_symmetry_point_on_curve_arc() {
    let mut sk = Sketch::new();
    use ConstraintKind::*;
    let l = sk.add_line(v(-5.0, 0.0), v(5.0, 0.0), None, None, None).unwrap();
    let p = sk.add_point(v(1.0, 1.0), None).unwrap();
    sk.add_constraint(Midpoint { p, l }, None).unwrap();
    let a = sk.add_arc(v(0.0, 0.0), v(4.0, 0.0), v(0.0, 4.0), [None, None, None], None).unwrap();
    sk.add_constraint(Radius { c: a, value: 6.0 }, None).unwrap();
    let q = sk.add_point(v(3.0, 3.0), None).unwrap();
    sk.add_constraint(PointOnCurve { p: q, c: a }, None).unwrap();
    let s1 = sk.add_point(v(-2.0, 1.0), None).unwrap();
    let s2 = sk.add_point(v(3.0, -1.5), None).unwrap();
    let axis = sk.add_line(v(0.0, -10.0), v(0.0, 10.0), None, None, None).unwrap();
    sk.add_constraint(Vertical { l: axis }, None).unwrap();
    sk.add_constraint(Symmetric { p: s1, q: s2, l: axis }, None).unwrap();
    let rep = solve(&mut sk);
    assert!(rep.ok(), "{rep:?}");
    let (la, lb) = line_pts(&sk, l);
    assert!(sk.points[p].pos.dist((la + lb) * 0.5) < 1e-7);
    assert!((sk.radius(a).unwrap() - 6.0).abs() < 1e-7);
    let c = sk.center(a).unwrap();
    assert!((sk.points[q].pos.dist(c) - 6.0).abs() < 1e-7);
    let (ax, _) = line_pts(&sk, axis);
    assert!((sk.points[s1].pos.x + sk.points[s2].pos.x - 2.0 * ax.x).abs() < 1e-7);
    assert!((sk.points[s1].pos.y - sk.points[s2].pos.y).abs() < 1e-7);
}

#[test]
fn profiles_rectangle_with_hole_and_split() {
    let mut sk = Sketch::new();
    rect(&mut sk, [v(0.0, 0.0), v(40.0, 0.0), v(40.0, 30.0), v(0.0, 30.0)]);
    sk.add_circle(v(20.0, 15.0), 5.0, None, None).unwrap();
    let ps = find_profiles(&sk);
    assert_eq!(ps.len(), 2, "{ps:?}");
    let big = ps.iter().find(|p| p.region.holes.len() == 1).unwrap();
    assert!((big.area - (1200.0 - PI * 25.0)).abs() < 1e-6);
    let disc = ps.iter().find(|p| p.region.holes.is_empty()).unwrap();
    assert!((disc.area - PI * 25.0).abs() < 1e-6);
    assert_eq!(big.hole_curves.len(), 1);

    // A line across the rectangle splits it into two regions (with the hole in one).
    let mut sk2 = Sketch::new();
    let l = rect(&mut sk2, [v(0.0, 0.0), v(40.0, 0.0), v(40.0, 30.0), v(0.0, 30.0)]);
    let (b0, _) = line_pts(&sk2, l[0]);
    let _ = b0;
    let pa = sk2.add_point(v(10.0, 0.0), None).unwrap();
    let pb = sk2.add_point(v(10.0, 30.0), None).unwrap();
    // Re-split bottom and top edges at x = 10 by rebuilding the sketch as 7 lines.
    let mut s3 = Sketch::new();
    let pts = [v(0.0, 0.0), v(10.0, 0.0), v(40.0, 0.0), v(40.0, 30.0), v(10.0, 30.0), v(0.0, 30.0)];
    let ids: Vec<usize> = pts.iter().map(|p| s3.add_point(*p, None).unwrap()).collect();
    for i in 0..6 {
        s3.add_line_pts(ids[i], ids[(i + 1) % 6], None).unwrap();
    }
    s3.add_line_pts(ids[1], ids[4], None).unwrap();
    let _ = (pa, pb);
    let ps = find_profiles(&s3);
    assert_eq!(ps.len(), 2);
    let mut areas: Vec<f64> = ps.iter().map(|p| p.area).collect();
    areas.sort_by(f64::total_cmp);
    assert!((areas[0] - 300.0).abs() < 1e-9 && (areas[1] - 900.0).abs() < 1e-9, "{areas:?}");
}

#[test]
fn profile_with_arcs_and_construction_ignored() {
    // Slot: two lines and two half arcs.
    let mut sk = Sketch::new();
    let p = [v(0.0, 0.0), v(20.0, 0.0), v(20.0, 10.0), v(0.0, 10.0)];
    let ids: Vec<usize> = p.iter().map(|q| sk.add_point(*q, None).unwrap()).collect();
    sk.add_line_pts(ids[0], ids[1], None).unwrap();
    sk.add_arc(v(20.0, 5.0), p[1], p[2], [None, Some(ids[1]), Some(ids[2])], None).unwrap();
    sk.add_line_pts(ids[2], ids[3], None).unwrap();
    sk.add_arc(v(0.0, 5.0), p[3], p[0], [None, Some(ids[3]), Some(ids[0])], None).unwrap();
    let c = sk.add_line(v(-5.0, -5.0), v(30.0, 20.0), None, None, None).unwrap();
    sk.curves[c].construction = true;
    let ps = find_profiles(&sk);
    assert_eq!(ps.len(), 1, "{ps:?}");
    assert!((ps[0].area - (200.0 + PI * 25.0)).abs() < 1e-6, "{}", ps[0].area);
    assert_eq!(ps[0].outer_curves.len(), 4);
}

#[test]
fn profiles_join_unconstrained_nearby_line_arc_endpoints() {
    let mut sk = Sketch::new();
    let bottom_left = sk.add_point(v(0.0, 0.0), None).unwrap();
    let bottom_right = sk.add_point(v(20.0, 0.0), None).unwrap();
    let arc_start = sk.add_point(v(20.00004, 5.0 - (25.0_f64 - 0.00004_f64.powi(2)).sqrt()), None).unwrap();
    let center = sk.add_point(v(20.0, 5.0), None).unwrap();
    let top_right = sk.add_point(v(20.0, 10.0), None).unwrap();
    let top_left = sk.add_point(v(0.0, 10.0), None).unwrap();
    let left_center = sk.add_point(v(0.0, 5.0), None).unwrap();

    sk.add_line_pts(bottom_left, bottom_right, None).unwrap();
    sk.add_arc(v(20.0, 5.0), sk.points[arc_start].pos, v(20.0, 10.0), [Some(center), Some(arc_start), Some(top_right)], None).unwrap();
    sk.add_line_pts(top_right, top_left, None).unwrap();
    sk.add_arc(v(0.0, 5.0), v(0.0, 10.0), v(0.0, 0.0), [Some(left_center), Some(top_left), Some(bottom_left)], None).unwrap();

    let profiles = find_profiles(&sk);
    assert_eq!(profiles.len(), 1, "{profiles:?}");
    assert!(profiles[0].region.outer.is_closed(1e-6), "{:?}", profiles[0].region.outer.segs);
    assert!((profiles[0].area - (200.0 + PI * 25.0)).abs() < 1e-6, "{}", profiles[0].area);
}

/// #14512: the reporter's sketch, a Ø3 circle cut off by three lines that touch it. Splitting the
/// circle at the 135-degree line, which touches it at 225 degrees, left two arcs of 0.00003 and
/// 0.00001 mm next to the corner. With them the sketch had no profile at all.
#[test]
fn profiles_ignore_near_zero_arcs_left_by_a_split_at_a_touching_line() {
    let mut sk = Sketch::new();
    let centre = v(0.0, 0.0);
    let p = |sk: &mut Sketch, x: f64, y: f64| sk.add_point(v(x, y), None).unwrap();
    let bottom_left = p(&mut sk, -0.6213203435596425, -1.5);
    let bottom_right = p(&mut sk, 0.6213203435596425, -1.5);
    let right = p(&mut sk, 1.0606601717798214, -1.0606601717798212);
    let corner = p(&mut sk, -1.0606821956789731, -1.060638147870911);
    let sliver = p(&mut sk, -1.0606633888849604, -1.0606569546649243);
    let sliver_end = p(&mut sk, -1.060656954688482, -1.0606633888614028);
    let arc = |sk: &mut Sketch, a: usize, b: usize| {
        let (pa, pb) = (sk.points[a].pos, sk.points[b].pos);
        sk.add_arc(centre, pa, pb, [None, Some(a), Some(b)], None).unwrap()
    };
    sk.add_line_pts(bottom_left, bottom_right, None).unwrap();
    sk.add_line_pts(bottom_right, right, None).unwrap();
    sk.add_line_pts(bottom_left, corner, None).unwrap();
    arc(&mut sk, right, corner);
    arc(&mut sk, corner, sliver);
    arc(&mut sk, sliver, sliver_end);

    // The 270-degree sector and the polygon from the centre through the four line ends.
    let r = 1.5;
    let polygon = 1.8639610306789276;
    let ps = find_profiles(&sk);
    assert_eq!(ps.len(), 1, "{ps:?}");
    assert!((ps[0].area - (0.75 * PI * r * r + polygon)).abs() < 1e-6, "{}", ps[0].area);
}

/// The end plate of #14256: an outline whose 25 mm arc meets a horizontal and a vertical line
/// tangentially, each joint held by two "point on curve" constraints instead of one shared point.
/// The solver leaves the arc's ends on the lines only to its 1e-9 mm residual, which along the
/// tangent puts them 5e-5 mm and 2.25e-4 mm from the lines' ends. The horizontal line is raised
/// by `lift`.
fn tangent_joint_end_plate(lift: f64) -> Sketch {
    let mut sk = Sketch::new();
    let knee = (50.0_f64.powi(2) - 11.125_f64.powi(2)).sqrt();
    let mut point = |x: f64, y: f64| sk.add_point(v(x, y), None).unwrap();
    let top_left = point(-14.0, 90.0);
    let top_right = point(14.0, 90.0);
    let shoulder = point(14.0, 48.0);
    let knee = point(knee, 11.125);
    let right_top = point(90.0, 11.125);
    let right_bottom = point(90.0, -11.125);
    let step_bottom = point(25.0, -11.125);
    let step_top = point(25.0, 0.0);
    let arc_start = point(24.999999999949498, 5.025035674136865e-5);
    let arc_end = point(0.0002252128472149822, 24.999999999324313);
    let ledge_right = point(0.0, 25.0 + lift);
    let ledge_left = point(-14.0, 25.0 + lift);
    let big_centre = point(0.0, 0.0);
    let small_centre = point(0.0, 0.0);
    sk.add_line_pts(top_left, top_right, None).unwrap();
    sk.add_line_pts(top_right, shoulder, None).unwrap();
    sk.add_arc(v(0.0, 0.0), sk.points[knee].pos, sk.points[shoulder].pos, [Some(big_centre), Some(knee), Some(shoulder)], None).unwrap();
    sk.add_line_pts(right_top, knee, None).unwrap();
    sk.add_line_pts(right_bottom, right_top, None).unwrap();
    sk.add_line_pts(step_bottom, right_bottom, None).unwrap();
    sk.add_line_pts(step_top, step_bottom, None).unwrap();
    sk.add_arc(v(0.0, 0.0), sk.points[arc_start].pos, sk.points[arc_end].pos, [Some(small_centre), Some(arc_start), Some(arc_end)], None).unwrap();
    sk.add_line_pts(ledge_left, ledge_right, None).unwrap();
    sk.add_line_pts(top_left, ledge_left, None).unwrap();
    sk
}

#[test]
fn profiles_join_line_and_arc_ends_that_lie_on_each_other_at_a_tangent_joint() {
    let profiles = find_profiles(&tangent_joint_end_plate(0.0));
    assert_eq!(profiles.len(), 1, "{profiles:?}");
    assert!(profiles[0].region.outer.is_closed(1e-9), "{:?}", profiles[0].region.outer.segs);
    assert_eq!(profiles[0].outer_curves.len(), 10, "{:?}", profiles[0].outer_curves);
    // The polygon through the corners, plus the segment the 50 mm arc adds, less the quarter
    // disc's segment the 25 mm arc takes away.
    let knee = (50.0_f64.powi(2) - 11.125_f64.powi(2)).sqrt();
    let corners = [
        (25.0, -11.125),
        (90.0, -11.125),
        (90.0, 11.125),
        (knee, 11.125),
        (14.0, 48.0),
        (14.0, 90.0),
        (-14.0, 90.0),
        (-14.0, 25.0),
        (0.0, 25.0),
        (25.0, 0.0),
    ];
    let polygon = (0..corners.len()).map(|i| {
        let ((x0, y0), (x1, y1)) = (corners[i], corners[(i + 1) % corners.len()]);
        x0 * y1 - x1 * y0
    });
    let segment = |r: f64, sweep: f64| r * r / 2.0 * (sweep - sweep.sin());
    let area = polygon.sum::<f64>() / 2.0 + segment(50.0, 48.0_f64.atan2(14.0) - 11.125_f64.atan2(knee)) - segment(25.0, FRAC_PI_2);
    assert!((profiles[0].area - area).abs() < 1e-6, "{} vs {area}", profiles[0].area);

    // Raised 1e-5 mm, the line's end is off the arc's circle and the arc's end off the line:
    // the 2.5e-4 mm gap is a real one and the outline stays open.
    let open = find_profiles(&tangent_joint_end_plate(1e-5));
    assert!(open.is_empty(), "{open:?}");
}

#[test]
fn removal_and_references() {
    let mut sk = Sketch::new();
    let l1 = sk.add_line(v(0.0, 0.0), v(1.0, 0.0), None, None, Some("l1")).unwrap();
    let e = sk.resolve_point("l1.end").unwrap();
    let l2 = sk.add_line(v(1.0, 0.0), v(1.0, 1.0), Some(e), None, Some("l2")).unwrap();
    sk.add_constraint(ConstraintKind::Horizontal { l: l1 }, None).unwrap();
    sk.add_constraint(ConstraintKind::Vertical { l: l2 }, None).unwrap();
    assert!(sk.add_line(v(0.0, 0.0), v(1.0, 0.0), None, None, Some("l1")).is_err());
    sk.remove_curves(&[l1]);
    assert_eq!(sk.curves.len(), 1);
    assert_eq!(sk.constraints.len(), 1);
    assert!(sk.resolve_point("l2.start").is_some());
    assert!(sk.resolve_point("l1.start").is_none());
    assert_eq!(sk.points.len(), 3, "origin + l2's two points");
    assert!(solve(&mut sk).ok());
    let json = serde_json::to_string(&sk).unwrap();
    let back: Sketch = serde_json::from_str(&json).unwrap();
    assert_eq!(back, sk);
}

#[test]
fn hostile_values_rejected() {
    let mut sk = Sketch::new();
    assert!(sk.add_point(v(f64::NAN, 0.0), None).is_err());
    assert!(sk.add_circle(v(0.0, 0.0), -1.0, None, None).is_err());
    assert!(sk.add_circle(v(0.0, 0.0), f64::INFINITY, None, None).is_err());
    assert!(sk.add_constraint(ConstraintKind::Horizontal { l: 99 }, None).is_err());
    let c = sk.add_circle(v(0.0, 0.0), 1.0, None, None).unwrap();
    assert!(sk.add_constraint(ConstraintKind::Horizontal { l: c }, None).is_err());
    assert!(sk.add_constraint(ConstraintKind::Radius { c, value: f64::NAN }, None).is_err());
    assert!(sk.add_constraint(ConstraintKind::Radius { c, value: -2.0 }, None).is_err());
}

#[test]
fn profiles_split_at_crossings_and_touch_points() {
    // A line across a circle: two half discs.
    let mut sk = Sketch::new();
    sk.add_circle(v(0.0, 0.0), 10.0, None, None).unwrap();
    sk.add_line(v(-15.0, 0.0), v(15.0, 0.0), None, None, None).unwrap();
    let ps = find_profiles(&sk);
    assert_eq!(ps.len(), 2, "{ps:?}");
    for p in &ps {
        assert!((p.area - PI * 50.0).abs() < 1e-6, "{}", p.area);
    }
    // Belt: two circles joined by two tangent lines whose ends sit on the circles.
    let mut b = Sketch::new();
    let c1 = b.add_circle(v(0.0, 0.0), 20.0, Some(0), None).unwrap();
    let c2 = b.add_circle(v(50.0, 0.0), 10.0, None, None).unwrap();
    let l1 = b.add_line(v(3.0, 19.0), v(47.0, 13.0), None, None, None).unwrap();
    let l2 = b.add_line(v(1.0, -22.0), v(44.0, -8.0), None, None, None).unwrap();
    use ConstraintKind::*;
    for (l, end, c) in [(l1, "start", c1), (l1, "end", c2), (l2, "start", c1), (l2, "end", c2)] {
        let p = b.resolve_point(&format!("{}.{end}", b.curves[l].id)).unwrap();
        b.add_constraint(PointOnCurve { p, c }, None).unwrap();
    }
    for (l, c) in [(l1, c1), (l1, c2), (l2, c1), (l2, c2)] {
        b.add_constraint(Tangent { a: l, b: c }, None).unwrap();
    }
    b.add_constraint(Radius { c: c1, value: 20.0 }, None).unwrap();
    b.add_constraint(Radius { c: c2, value: 10.0 }, None).unwrap();
    let r = solve(&mut b);
    assert!(r.ok(), "{r:?}");
    let ps = find_profiles(&b);
    assert_eq!(ps.len(), 3, "two discs and the band between them: {:?}", ps.iter().map(|p| p.area).collect::<Vec<_>>());
    let total: f64 = ps.iter().map(|p| p.area).sum();
    assert!(total > PI * 500.0, "{total}");
}

/// Curves made tangent by the solver touch within rounding noise, either side of exact (#588):
/// the touch still splits them. Lines running past their tangent points close the band between
/// two circles, and a square's side tangent to a circle closes the square off from the disc.
#[test]
fn solver_tangencies_split_profiles() {
    use ConstraintKind::*;
    // Two fixed circles and two free lines, longer than the band, made tangent to both.
    for (r1, r2, d) in [(15.0, 8.0, 40.0), (20.0, 10.0, 50.0), (7.3, 7.3, 21.1)] {
        let mut b = Sketch::new();
        let c1 = b.add_circle(v(0.0, 0.0), r1, None, None).unwrap();
        let c2 = b.add_circle(v(d, 0.0), r2, None, None).unwrap();
        let l1 = b.add_line(v(-r1, r1 + 2.0), v(d + r2 + 5.0, r2 - 1.0), None, None, None).unwrap();
        let l2 = b.add_line(v(-r1, -r1 - 2.0), v(d + r2 + 5.0, 1.0 - r2), None, None, None).unwrap();
        for c in [c1, c2] {
            let p = b.resolve_point(&format!("{}.center", b.curves[c].id)).unwrap();
            b.add_constraint(Fix { p }, None).unwrap();
        }
        b.add_constraint(Radius { c: c1, value: r1 }, None).unwrap();
        b.add_constraint(Radius { c: c2, value: r2 }, None).unwrap();
        for (l, c) in [(l1, c1), (l1, c2), (l2, c1), (l2, c2)] {
            b.add_constraint(Tangent { a: l, b: c }, None).unwrap();
        }
        assert!(solve(&mut b).ok());
        let ps = find_profiles(&b);
        // The band: the trapezoid between the tangent points less the circular segments the
        // circles take from it (as exact sums: the hull less the two discs).
        let alpha = ((r1 - r2) / d).asin();
        let tangent_len = (d * d - (r1 - r2).powi(2)).sqrt();
        let hull = r1 * r1 * (PI / 2.0 + alpha) + r2 * r2 * (PI / 2.0 - alpha) + (r1 + r2) * tangent_len;
        let total: f64 = ps.iter().map(|p| p.area).sum();
        assert_eq!(ps.len(), 3, "r {r1}, {r2}: {:?}", ps.iter().map(|p| p.area).collect::<Vec<_>>());
        assert!((total - hull).abs() < 1e-6 * hull, "r {r1}, {r2}: {total} vs {hull}");
    }
    // A square tangent to a circle inside it and outside it, the circle placed by the solver.
    for (inside, centre) in [(true, v(5.0, 6.3)), (false, v(5.0, 16.7))] {
        let mut s = Sketch::new();
        let sides = [v(0.0, 0.0), v(10.0, 0.0), v(10.0, 10.0), v(0.0, 10.0)];
        let mut lines = Vec::new();
        for k in 0..4 {
            lines.push(s.add_line(sides[k], sides[(k + 1) % 4], None, None, None).unwrap());
        }
        for k in 0..4 {
            let (a, b) = (
                s.resolve_point(&format!("{}.end", s.curves[lines[k]].id)).unwrap(),
                s.resolve_point(&format!("{}.start", s.curves[lines[(k + 1) % 4]].id)).unwrap(),
            );
            s.add_constraint(Coincident { p: a, q: b }, None).unwrap();
            s.add_constraint(Fix { p: a }, None).unwrap();
        }
        let c = s.add_circle(centre, 3.7, None, None).unwrap();
        s.add_constraint(Radius { c, value: 3.7 }, None).unwrap();
        s.add_constraint(Tangent { a: lines[2], b: c }, None).unwrap();
        assert!(solve(&mut s).ok());
        let ps = find_profiles(&s);
        let disc = PI * 3.7 * 3.7;
        let mut areas: Vec<f64> = ps.iter().map(|p| p.area).collect();
        areas.sort_by(f64::total_cmp);
        let want = if inside { [disc, 100.0 - disc] } else { [disc, 100.0] };
        let mut want = want.to_vec();
        want.sort_by(f64::total_cmp);
        assert_eq!(areas.len(), 2, "inside {inside}: {areas:?}");
        for (a, w) in areas.iter().zip(&want) {
            assert!((a - w).abs() < 1e-6, "inside {inside}: {areas:?} vs {want:?}");
        }
    }
}

/// Sorted curve ids.
fn id_set(sk: &Sketch, curves: &[usize]) -> Vec<String> {
    let mut ids: Vec<String> = curves.iter().map(|c| sk.curves[*c].id.clone()).collect();
    ids.sort();
    ids
}

/// A projected line of `a`–`b` in `sk`.
fn reference_line(sk: &mut Sketch, a: Vec2, b: Vec2) -> usize {
    let link = sk.add_link(LinkKind::Project, LinkSource::SketchCurve { sketch: 1, curve: "edge".into() }, &[LinkGeom::Line(a, b)]).unwrap();
    sk.link_curves(&link)[0]
}

/// #635: a side constrained onto a longer projected line lies on it only up to the solver's
/// round-off. Where they overlap they are one edge of the drawing, so the rectangle is still a
/// closed profile, and the curve that came first names that edge, as when the side lies on the
/// line exactly.
#[test]
fn a_side_on_a_projected_line_still_closes_its_rectangle() {
    // Round-off left on the side's two ends (the first pair as solved in #635), and none.
    for (y0, y1) in
        [(1.6e-12, 2.2e-12), (2.2e-12, 1.6e-12), (-1e-12, 1e-12), (1e-12, -1e-12), (-3e-11, -1e-11), (-3e-10, 3e-10), (2e-10, 4e-10), (0.0, 0.0)]
    {
        for reference_first in [true, false] {
            for (ra, rb) in [(v(40.0, 0.0), v(-10.0, 0.0)), (v(-10.0, 0.0), v(40.0, 0.0))] {
                let mut sk = Sketch::new();
                let early = reference_first.then(|| reference_line(&mut sk, ra, rb));
                let sides = rect(&mut sk, [v(5.0, y0), v(25.0, y1), v(25.0, 15.0), v(5.0, 15.0)]);
                let reference = early.unwrap_or_else(|| reference_line(&mut sk, ra, rb));
                let case = format!("ends at y {y0}, {y1}; reference {} {ra:?} to {rb:?}", if reference_first { "first" } else { "last" });
                let ps = find_profiles(&sk);
                assert_eq!(ps.len(), 1, "{case}: {:?}", ps.iter().map(|p| (p.area, &p.outer_curves)).collect::<Vec<_>>());
                assert!((ps[0].area - 300.0).abs() < 1e-6, "{case}: {}", ps[0].area);
                let mut got = ps[0].outer_curves.clone();
                got.sort();
                let want = id_set(&sk, &[if reference_first { reference } else { sides[0] }, sides[1], sides[2], sides[3]]);
                assert_eq!(got, want, "{case}");
                // Whole curves make a lasting identity; a piece of the reference cut at the corners does not.
                let whole = (!reference_first).then(|| (want, Vec::new()));
                assert_eq!(ps[0].whole_curve_boundaries(), whole, "{case}");
            }
        }
    }
}

/// A third curve meeting a side and the projected line under it would meet each a little apart:
/// a circle touching the line from inside the rectangle (it misses or crosses the side by the
/// round-off), and a line crossing both at a shallow angle (the crossings 3e-5 mm apart). It
/// meets both where it meets the first of them, so they still make one edge, whichever way the
/// side runs.
#[test]
fn a_curve_meeting_a_side_on_a_projected_line_meets_both_at_once() {
    for y0 in [3e-10, -3e-10, 0.0] {
        for reference_first in [true, false] {
            for reversed in [false, true] {
                let case = format!("side at y {y0}, reference {}, side reversed {reversed}", if reference_first { "first" } else { "last" });
                let corners = |x1: f64| {
                    let (a, b) = if reversed { (x1, 5.0) } else { (5.0, x1) };
                    [v(a, y0), v(b, y0), v(b, 15.0), v(a, 15.0)]
                };
                let mut sk = Sketch::new();
                let early = reference_first.then(|| reference_line(&mut sk, v(-10.0, 0.0), v(40.0, 0.0)));
                rect(&mut sk, corners(25.0));
                sk.add_circle(v(15.0, 5.0), 5.0, None, None).unwrap();
                early.unwrap_or_else(|| reference_line(&mut sk, v(-10.0, 0.0), v(40.0, 0.0)));
                let mut areas: Vec<f64> = find_profiles(&sk).iter().map(|p| p.area).collect();
                areas.sort_by(f64::total_cmp);
                assert_eq!(areas.len(), 2, "touching circle, {case}: {areas:?}");
                assert!((areas[0] - 25.0 * PI).abs() < 1e-6 && (areas[1] - (300.0 - 25.0 * PI)).abs() < 1e-6, "touching circle, {case}: {areas:?}");

                let mut sk = Sketch::new();
                let early = reference_first.then(|| reference_line(&mut sk, v(-10.0, 0.0), v(440.0, 0.0)));
                rect(&mut sk, corners(405.0));
                // Through (200, 0), rising 1e-5 per mm: a sliver of 205 x 0.00205 / 2 under it.
                sk.add_line(v(0.0, -0.002), v(430.0, 0.0023), None, None, None).unwrap();
                early.unwrap_or_else(|| reference_line(&mut sk, v(-10.0, 0.0), v(440.0, 0.0)));
                let mut areas: Vec<f64> = find_profiles(&sk).iter().map(|p| p.area).collect();
                areas.sort_by(f64::total_cmp);
                assert_eq!(areas.len(), 2, "shallow line, {case}: {areas:?}");
                assert!((areas[0] - 0.210125).abs() < 1e-6 && (areas[1] - (6000.0 - 0.210125)).abs() < 1e-6, "shallow line, {case}: {areas:?}");
            }
        }
    }
}

/// The same for drawn curves on drawn curves: two rectangles drawn side by side, the second's
/// side solved onto the first's; and a quarter disc whose arc is solved onto a projected circle.
#[test]
fn curves_solved_onto_each_other_keep_the_regions_on_both_sides() {
    for (x0, x1) in [(1e-12, -1e-12), (-2e-12, -1e-12), (2.2e-12, 1.6e-12), (0.0, 0.0)] {
        let mut sk = Sketch::new();
        let a = rect(&mut sk, [v(0.0, 0.0), v(20.0, 0.0), v(20.0, 15.0), v(0.0, 15.0)]);
        let b = rect(&mut sk, [v(20.0 + x0, 0.0), v(40.0, 0.0), v(40.0, 15.0), v(20.0 + x1, 15.0)]);
        let ps = find_profiles(&sk);
        let areas: Vec<f64> = ps.iter().map(|p| p.area).collect();
        assert_eq!(ps.len(), 2, "{x0}, {x1}: {areas:?}");
        assert!(areas.iter().all(|a| (a - 300.0).abs() < 1e-6), "{x0}, {x1}: {areas:?}");
        // The shared side is the first rectangle's (it was there first).
        let mut first = ps[0].outer_curves.clone();
        first.sort();
        assert_eq!(first, id_set(&sk, &a), "{x0}, {x1}");
        let mut second = ps[1].outer_curves.clone();
        second.sort();
        assert_eq!(second, id_set(&sk, &[b[0], b[1], b[2], a[1]]), "{x0}, {x1}");
    }
    let hole = |sk: &mut Sketch| {
        let link = sk
            .add_link(LinkKind::Project, LinkSource::SketchCurve { sketch: 1, curve: "hole".into() }, &[LinkGeom::Circle(v(0.0, 0.0), 10.0)])
            .unwrap();
        sk.link_curves(&link)[0]
    };
    for (dx, dr) in [(1e-12, 2e-12), (-1e-12, -1e-12), (0.0, 0.0)] {
        for circle_first in [true, false] {
            let mut sk = Sketch::new();
            let early = circle_first.then(|| hole(&mut sk));
            let c = sk.add_point(v(dx, -dx), None).unwrap();
            let (p0, p1) = (sk.add_point(v(10.0 + dr, 0.0), None).unwrap(), sk.add_point(v(0.0, 10.0 + dr), None).unwrap());
            let arc = sk.add_arc(v(dx, -dx), v(10.0 + dr, 0.0), v(0.0, 10.0 + dr), [Some(c), Some(p0), Some(p1)], None).unwrap();
            let (l0, l1) = (sk.add_line_pts(c, p0, None).unwrap(), sk.add_line_pts(p1, c, None).unwrap());
            let circle = early.unwrap_or_else(|| hole(&mut sk));
            let case = format!("{dx}, {dr}, circle {}", if circle_first { "first" } else { "last" });
            let ps = find_profiles(&sk);
            let areas: Vec<f64> = ps.iter().map(|p| p.area).collect();
            assert_eq!(ps.len(), 2, "{case}: {areas:?}");
            let quarter = ps.iter().find(|p| p.area < 100.0).unwrap();
            assert!((quarter.area - 25.0 * PI).abs() < 1e-6, "{case}: {areas:?}");
            // One piece each: round-off alone does not cut the arc where it crosses the circle.
            let mut got = quarter.outer_curves.clone();
            got.sort();
            assert_eq!(got, id_set(&sk, &[if circle_first { circle } else { arc }, l0, l1]), "{case}");
            let rest = ps.iter().find(|p| p.area > 100.0).unwrap();
            assert!((rest.area - 75.0 * PI).abs() < 1e-6, "{case}: {areas:?}");
        }
    }
}

/// What does not run along one path stays apart: a chord and an arc bulging past it by about
/// the merge tolerance still bound their thin region, and two whole circles further apart than
/// it are still cut where they cross, into a lens and two crescents. Closer than it, their
/// halves are one path: one disc.
#[test]
fn near_overlaps_that_are_not_one_path_keep_their_regions() {
    let s = 1.5e-5;
    let r = (25.0 + s * s) / (2.0 * s);
    let mut sk = Sketch::new();
    let (a, b) = (sk.add_point(v(-5.0, 0.0), None).unwrap(), sk.add_point(v(5.0, 0.0), None).unwrap());
    let chord = sk.add_line_pts(a, b, None).unwrap();
    let arc = sk.add_arc(v(0.0, s - r), v(5.0, 0.0), v(-5.0, 0.0), [None, Some(b), Some(a)], None).unwrap();
    let ps = find_profiles(&sk);
    assert_eq!(ps.len(), 1, "{:?}", ps.iter().map(|p| p.area).collect::<Vec<_>>());
    // Two thirds of chord times height, to well within the rounding of the area.
    assert!((ps[0].area - 2.0 / 3.0 * 10.0 * s).abs() < 1e-8, "{}", ps[0].area);
    let mut got = ps[0].outer_curves.clone();
    got.sort();
    assert_eq!(got, id_set(&sk, &[chord, arc]));

    // The same with a free-form curve: a flat cubic (height s) over its chord. It takes part as
    // sampled pieces, one of them on the chord, but its exact curve bounds the region.
    let s = 8e-6;
    let mut sk = Sketch::new();
    let ids: Vec<usize> = [v(-5.0, 0.0), v(-5.0 / 3.0, 4.0 * s / 3.0), v(5.0 / 3.0, 4.0 * s / 3.0), v(5.0, 0.0)]
        .iter()
        .map(|p| sk.add_point(*p, None).unwrap())
        .collect();
    sk.add_curve(CurveKind::Spline { pts: ids.clone(), control: true, degree: 3, periodic: false, nurbs: None, handles: Vec::new() }, None).unwrap();
    sk.add_line_pts(ids[3], ids[0], None).unwrap();
    let ps = find_profiles(&sk);
    assert_eq!(ps.len(), 1, "{:?}", ps.iter().map(|p| p.area).collect::<Vec<_>>());
    assert!((ps[0].area - 20.0 * s / 3.0).abs() < 1e-9, "{}", ps[0].area);

    // An arc a little off a circle, more than the merge tolerance apart at its ends (so they
    // don't cut the circle): the two still cut each other where they cross, and no profile is
    // the uncut circle.
    let mut sk = Sketch::new();
    let circle = sk.add_circle(v(0.0, 0.0), 10.0, None, None).unwrap();
    let (c, r) = (v(8e-6, 0.0), 10.0 + 6e-6);
    let (p0, p1) = (c + Vec2::from_angle(PI / 4.0) * r, c + Vec2::from_angle(-PI / 4.0) * r);
    let (a0, a1, tip) = (sk.add_point(p0, None).unwrap(), sk.add_point(p1, None).unwrap(), sk.add_point(v(30.0, 0.0), None).unwrap());
    sk.add_arc(c, p0, p1, [None, Some(a0), Some(a1)], None).unwrap();
    sk.add_line_pts(a1, tip, None).unwrap();
    sk.add_line_pts(tip, a0, None).unwrap();
    let ps = find_profiles(&sk);
    let circle_id = sk.curves[circle].id.clone();
    assert!(ps.iter().all(|p| p.outer_curves != [circle_id.clone()] && p.whole_curve_boundaries().is_none()), "{ps:?}");
    // Profiles don't overlap: together they cover the disc and the arc's region once, that is the
    // disc and the triangle to the tip less the disc's segment inside it (to within the slivers
    // between arc and circle).
    let beyond = 0.5 * (p0 - v(30.0, 0.0)).cross(p1 - v(30.0, 0.0)).abs() - 50.0 * (PI / 2.0 - 1.0);
    let covered = ps.iter().map(|p| p.area).sum::<f64>();
    assert!((covered - PI * 100.0 - beyond).abs() < 1e-2, "{:?}", ps.iter().map(|p| p.area).collect::<Vec<_>>());

    let r = 10.0_f64;
    for (d, regions) in [(1e-3, 3), (1e-4, 3), (8e-6, 1)] {
        let mut sk = Sketch::new();
        let first = sk.add_circle(v(0.0, 0.0), r, None, None).unwrap();
        sk.add_circle(v(d, 0.0), r, None, None).unwrap();
        let ps = find_profiles(&sk);
        let areas: Vec<f64> = ps.iter().map(|p| p.area).collect();
        assert_eq!(ps.len(), regions, "{d}: {areas:?}");
        let lens = 2.0 * r * r * (d / (2.0 * r)).acos() - d / 2.0 * (4.0 * r * r - d * d).sqrt();
        let union = if regions == 1 { PI * r * r } else { 2.0 * PI * r * r - lens };
        assert!((areas.iter().sum::<f64>() - union).abs() < 1e-6, "{d}: {areas:?}");
        // Cut where they cross: no whole-circle identity.
        assert!(ps.iter().all(|p| p.whole_curve_boundaries().is_none()), "{d}");
        if regions == 1 {
            assert_eq!(ps[0].outer_curves, vec![sk.curves[first].id.clone(); 2], "{d}: the first circle's halves");
        }
    }
}

/// Curves count as lying on one another only within the merge tolerance of the one carrying
/// them, not along a chain: an arc between two whole circles 1.6e-5 apart (each within 8e-6 of
/// it) does not stop the circles cutting each other where they cross.
#[test]
fn curves_lying_on_one_another_do_not_chain_past_the_tolerance() {
    let (d, r) = (16e-6, 10.0_f64);
    let mut sk = Sketch::new();
    sk.add_circle(v(0.0, 0.0), r, None, None).unwrap();
    sk.add_circle(v(d, 0.0), r, None, None).unwrap();
    let c = v(d / 2.0, 0.0);
    let (p0, p1) = (c + Vec2::from_angle(PI / 4.0) * r, c + Vec2::from_angle(3.0 * PI / 4.0) * r);
    sk.add_arc(c, p0, p1, [None, None, None], None).unwrap();
    let areas: Vec<f64> = find_profiles(&sk).iter().map(|p| p.area).collect();
    let lens = 2.0 * r * r * (d / (2.0 * r)).acos() - d / 2.0 * (4.0 * r * r - d * d).sqrt();
    // The discs' union once, not each disc whole.
    assert!((areas.iter().sum::<f64>() - (2.0 * PI * r * r - lens)).abs() < 1e-6, "{areas:?}");

    // Two dividers of a square, each carried by a short line 8e-6 off it that does not reach
    // their crossing: they still cross at one node, and the square has four quarters.
    let mut sk = Sketch::new();
    sk.add_line(v(-20.0, 0.0), v(-10.0, 0.0), None, None, None).unwrap();
    sk.add_line(v(0.0, -20.0), v(0.0, -10.0), None, None, None).unwrap();
    rect(&mut sk, [v(-15.0, -15.0), v(15.0, -15.0), v(15.0, 15.0), v(-15.0, 15.0)]);
    sk.add_line(v(-15.0, 8e-6), v(15.0, 8e-6), None, None, None).unwrap();
    sk.add_line(v(8e-6, -15.0), v(8e-6, 15.0), None, None, None).unwrap();
    let areas: Vec<f64> = find_profiles(&sk).iter().map(|p| p.area).collect();
    assert_eq!(areas.len(), 4, "{areas:?}");
    assert!((areas.iter().sum::<f64>() - 900.0).abs() < 1e-6, "{areas:?}");
}

#[test]
fn merging_adjacent_regions() {
    let a = solidcore_record::Region2 {
        outer: solidcore_record::Loop2::polygon(&[v(0.0, 0.0), v(10.0, 0.0), v(10.0, 10.0), v(0.0, 10.0)]),
        holes: vec![],
    };
    let b = solidcore_record::Region2 {
        outer: solidcore_record::Loop2::polygon(&[v(10.0, 0.0), v(20.0, 0.0), v(20.0, 10.0), v(10.0, 10.0)]),
        holes: vec![],
    };
    let c = solidcore_record::Region2 { outer: solidcore_record::Loop2::polygon(&[v(50.0, 0.0), v(60.0, 0.0), v(60.0, 10.0)]), holes: vec![] };
    let m = merge_regions(&[a, b, c]);
    assert_eq!(m.len(), 2);
    let mut areas: Vec<f64> = m.iter().map(|r| r.area()).collect();
    areas.sort_by(f64::total_cmp);
    assert!((areas[0] - 50.0).abs() < 1e-9 && (areas[1] - 200.0).abs() < 1e-9, "{areas:?}");
    // The two squares make one rectangle: the outline isn't left split where the shared edge was.
    let sides = m.iter().map(|r| r.outer.segs.len()).collect::<Vec<_>>();
    assert!(sides.contains(&4) && sides.contains(&3), "{sides:?}");
}

/// Two regions on either side of an arc whose end is a tangent touch: the sketch splits the
/// touch into two corners a few hundredths of a micron apart, so each region's copy of the arc
/// ends at its own corner (#12612, 3e-5 to 6e-5 mm apart). Ends that close are one corner of a
/// profile ([`ENDPOINT_JOIN_TOL`]), so the arc is a shared boundary and the merged region is the
/// square without it. Before, the copies were not matched (within 1e-5 mm only) and stayed in the
/// outline as a slit, out along the arc and back, which SMLib's extrusion refused (status 1005).
#[test]
fn regions_sharing_an_arc_whose_tangent_end_is_split_merge_without_a_slit() {
    let delta: f64 = 5e-6; // radians on r 10: 5e-5 mm
    let q = v(10.0 * delta.cos(), 10.0 * delta.sin());
    let quarter = solidcore_record::Region2 {
        outer: solidcore_record::Loop2 {
            segs: vec![
                Seg2::Line { a: v(0.0, 0.0), b: v(10.0, 0.0) },
                Seg2::Arc { center: v(0.0, 0.0), radius: 10.0, start: 0.0, sweep: FRAC_PI_2 },
                Seg2::Line { a: v(0.0, 10.0), b: v(0.0, 0.0) },
            ],
        },
        holes: vec![],
    };
    let rest = solidcore_record::Region2 {
        outer: solidcore_record::Loop2 {
            segs: vec![
                Seg2::Line { a: q, b: v(20.0, 0.0) },
                Seg2::Line { a: v(20.0, 0.0), b: v(20.0, 20.0) },
                Seg2::Line { a: v(20.0, 20.0), b: v(0.0, 20.0) },
                Seg2::Line { a: v(0.0, 20.0), b: v(0.0, 10.0) },
                Seg2::Arc { center: v(0.0, 0.0), radius: 10.0, start: FRAC_PI_2, sweep: -(FRAC_PI_2 - delta) },
            ],
        },
        holes: vec![],
    };
    let m = merge_regions(&[quarter, rest]);
    assert_eq!(m.len(), 1, "{m:?}");
    let r = &m[0];
    assert!(r.holes.is_empty(), "{r:?}");
    assert!((r.area() - 400.0).abs() < 1e-3, "{}", r.area());
    assert!(r.outer.segs.iter().all(|s| matches!(s, Seg2::Line { .. })), "the arc is left in the outline: {r:?}");
    assert_eq!(r.outer.segs.len(), 4, "{r:?}");
    // Every corner closes exactly: each side starts where the one before it ends.
    let n = r.outer.segs.len();
    for i in 0..n {
        let (s, t) = (r.outer.segs[i], r.outer.segs[(i + 1) % n]);
        assert_eq!(s.end(), t.start(), "corner {i}: {r:?}");
    }
}

/// A sketch holding `text` the way `sketch.text` makes it: one text link of the built-in font's
/// outlines.
fn text_sketch(text: &str, at: Vec2, height: f64) -> Sketch {
    let mut sk = Sketch::new();
    let source = LinkSource::Text { text: text.into(), at, height, angle: 0.0, font: None, style: Default::default(), exprs: Default::default() };
    sk.add_link(LinkKind::Text, source, &text_geometry(text, at, height, 0.0).unwrap()).unwrap();
    sk
}

fn sorted_areas(ps: &[Profile]) -> Vec<f64> {
    let mut a: Vec<f64> = ps.iter().map(|p| p.area).collect();
    a.sort_by(f64::total_cmp);
    a
}

fn assert_areas(got: &[f64], mut want: Vec<f64>) {
    want.sort_by(f64::total_cmp);
    assert_eq!(got.len(), want.len(), "{got:?} against {want:?}");
    for (g, w) in got.iter().zip(&want) {
        assert!((g - w).abs() < 2e-3, "{got:?} against {want:?}");
    }
}

#[test]
fn text_counters_are_holes_of_their_letters_not_profiles_of_their_own() {
    // Areas in mm² at a capital height of 10 mm, summed from the Liberation Sans contours (shoelace
    // formula, quadratic pieces flattened finely): a glyph's outer contour, then its counters.
    //   O: 83.3133 - 47.4584   A: 39.2159 - 9.0647   B: 68.9283 - 12.9336 - 16.6277
    //   8: 56.8394 - 10.6451 - 13.7347
    let (o, o_counter) = (83.3133 - 47.4584, 47.4584);
    let (a, a_counter) = (39.2159 - 9.0647, 9.0647);
    let (b, b_counters) = (68.9283 - 12.9336 - 16.6277, [12.9336, 16.6277]);
    let (eight, eight_counters) = (56.8394 - 10.6451 - 13.7347, [10.6451, 13.7347]);
    let check = |text: &str, letters: Vec<f64>, counters: Vec<f64>| {
        let sk = text_sketch(text, Vec2::ZERO, 10.0);
        let all = find_drawn_profiles(&sk);
        // Every counter is a closed region of its own, next to its letter with the hole in it...
        assert_areas(&sorted_areas(&all), letters.iter().chain(&counters).copied().collect());
        // ...and "all profiles" of text means the letters.
        assert_areas(&sorted_areas(&without_text_counters(&sk, all)), letters);
    };
    check("O", vec![o], vec![o_counter]);
    check("8", vec![eight], eight_counters.to_vec());
    check("AB", vec![a, b], [vec![a_counter], b_counters.to_vec()].concat());
    // A letter without a counter is untouched.
    check("I", vec![13.5557], vec![]);
}

#[test]
fn text_beside_other_curves_is_drawn_geometry() {
    // A 30 x 20 rectangle with an "I" (a 10 mm tall bar, 13.5557 mm² in Liberation Sans) beside it.
    let mut sk = text_sketch("I", v(40.0, 0.0), 10.0);
    for (p, q) in [((0.0, 0.0), (30.0, 0.0)), ((30.0, 0.0), (30.0, 20.0)), ((30.0, 20.0), (0.0, 20.0)), ((0.0, 20.0), (0.0, 0.0))] {
        sk.add_line(v(p.0, p.1), v(q.0, q.1), None, None, None).unwrap();
    }
    assert_areas(&sorted_areas(&find_drawn_profiles(&sk)), vec![13.5557, 600.0]);
    // Projected reference geometry still doesn't count as drawn.
    let square = |x: f64, s: f64| {
        vec![
            LinkGeom::Line(v(x, 0.0), v(x + s, 0.0)),
            LinkGeom::Line(v(x + s, 0.0), v(x + s, s)),
            LinkGeom::Line(v(x + s, s), v(x, s)),
            LinkGeom::Line(v(x, s), v(x, 0.0)),
        ]
    };
    sk.add_link(LinkKind::Project, LinkSource::Origin, &square(100.0, 5.0)).unwrap();
    assert_eq!(find_profiles(&sk).len(), 3);
    assert_areas(&sorted_areas(&find_drawn_profiles(&sk)), vec![13.5557, 600.0]);
    sk.add_link(LinkKind::Intersect, LinkSource::Origin, &square(110.0, 7.0)).unwrap();
    assert_areas(&sorted_areas(&find_drawn_profiles(&sk)), vec![13.5557, 49.0, 600.0]);
}

#[test]
fn a_letter_merged_with_its_counter_is_the_filled_letter() {
    // The ring of an O and its counter share the counter's two conics; merged, they cancel along it.
    let sk = text_sketch("O", Vec2::ZERO, 10.0);
    let regions: Vec<_> = find_profiles(&sk).into_iter().map(|p| p.region).collect();
    assert_eq!(regions.len(), 2);
    let merged = merge_regions(&regions);
    assert_eq!(merged.len(), 1, "{} regions", merged.len());
    assert!(merged[0].holes.is_empty(), "{} holes", merged[0].holes.len());
    assert!((merged[0].area() - 83.3133).abs() < 2e-3, "{}", merged[0].area());
}

#[test]
fn links_update_in_place_rebuild_and_break() {
    use crate::{LinkGeom, LinkKind, LinkSource};
    let mut sk = Sketch::new();
    let src = LinkSource::Origin;
    let sq = |s: f64| {
        vec![
            LinkGeom::Line(v(0.0, 0.0), v(s, 0.0)),
            LinkGeom::Line(v(s, 0.0), v(s, s)),
            LinkGeom::Line(v(s, s), v(0.0, s)),
            LinkGeom::Line(v(0.0, s), v(0.0, 0.0)),
        ]
    };
    let j = sk.add_link(LinkKind::Project, src, &sq(10.0)).unwrap();
    assert_eq!(sk.link_curves(&j).len(), 4);
    // Corners are shared: 4 points, all fixed.
    let pts = sk.link_points(&j);
    assert_eq!(pts.len(), 4);
    assert!(pts.iter().all(|p| sk.points[*p].fixed));
    // A free line dimensioned to a projected corner keeps its constraint across an update.
    let l = sk.add_line(v(3.0, 3.0), v(5.0, 7.0), None, None, None).unwrap();
    let (la, _) = match sk.curves[l].kind {
        CurveKind::Line { a, b } => (a, b),
        _ => panic!(),
    };
    sk.add_constraint(ConstraintKind::Coincident { p: la, q: pts[2] }, None).unwrap();
    let ids: Vec<String> = sk.link_curves(&j).iter().map(|c| sk.curves[*c].id.clone()).collect();
    assert!(!sk.update_link(&j, &sq(20.0)).unwrap(), "same shape updates in place");
    let ids2: Vec<String> = sk.link_curves(&j).iter().map(|c| sk.curves[*c].id.clone()).collect();
    assert_eq!(ids, ids2);
    assert!(solve(&mut sk).ok());
    assert!(sk.points[la].pos.dist(v(20.0, 20.0)) < 1e-9);
    assert_eq!(find_profiles(&sk).len(), 1);
    // A different shape rebuilds (the coincidence on the old corner goes).
    assert!(sk.update_link(&j, &[LinkGeom::Circle(v(1.0, 1.0), 4.0)]).unwrap());
    assert_eq!(sk.link_curves(&j).len(), 1);
    assert_eq!(sk.constraints.len(), 0);
    // Linked circles keep their radius in the solver.
    let c = sk.link_curves(&j)[0];
    let rep = solve(&mut sk);
    assert!(rep.curve_determined[c], "{rep:?}");
    // Breaking the link frees it.
    sk.break_link(&j).unwrap();
    assert!(sk.links.is_empty());
    assert!(sk.curves[c].link.is_none());
    let rep = solve(&mut sk);
    assert!(!rep.curve_determined[c]);
    // Deleting all of a link's curves removes the link.
    let j2 = sk.add_link(LinkKind::Project, LinkSource::Origin, &[LinkGeom::Line(v(0.0, 0.0), v(1.0, 0.0))]).unwrap();
    let cs = sk.link_curves(&j2);
    sk.remove_curves(&cs);
    assert!(sk.link(&j2).is_none());
    // Nothing to project is an error, not an empty link.
    assert!(sk.add_link(LinkKind::Project, LinkSource::Origin, &[LinkGeom::Line(v(0.0, 0.0), v(0.0, 0.0))]).is_err());
    // Round trip through JSON.
    let j3 = sk.add_link(LinkKind::Intersect, LinkSource::Axis { name: "X".into() }, &[LinkGeom::Point(v(2.0, 2.0))]).unwrap();
    let back: Sketch = serde_json::from_str(&serde_json::to_string(&sk).unwrap()).unwrap();
    assert_eq!(back, sk);
    assert_eq!(back.link(&j3).unwrap().kind, LinkKind::Intersect);
}

/// A reference that names no body describes itself by its kind alone: the message must not
/// end in "face of " with nothing after the "of" (#4929).
#[test]
fn a_source_without_a_body_name_names_its_kind_alone() {
    use solidcore_record::Vec3;
    let at = Vec3::ZERO;
    for (source, expected) in [
        (LinkSource::Edge { body: String::new(), at }, "edge"),
        (LinkSource::Face { body: String::new(), at }, "face"),
        (LinkSource::Vertex { body: String::new(), at }, "vertex"),
        (LinkSource::Iso { body: String::new(), at, dir: "u".into() }, "isoparametric curve"),
        (LinkSource::Spun { body: String::new(), origin: at, dir: Vec3::X }, "spun profile"),
    ] {
        assert_eq!(source.describe(), expected, "{source:?}");
    }
    // A named body keeps its name.
    assert_eq!(LinkSource::Face { body: "Body1".into(), at }.describe(), "face of Body1");
}

/// Many independent constrained rectangles solve quickly (per-component work).
#[test]
fn large_sketch_solves_fast() {
    let mut sk = Sketch::new();
    for i in 0..150 {
        let o = v(i as f64 * 50.0, 0.0);
        let l = rect(&mut sk, [o, o + v(40.0, 0.5), o + v(40.5, 30.0), o + v(-0.3, 30.2)]);
        use ConstraintKind::*;
        sk.add_constraint(Horizontal { l: l[0] }, None).unwrap();
        sk.add_constraint(Horizontal { l: l[2] }, None).unwrap();
        sk.add_constraint(Vertical { l: l[1] }, None).unwrap();
        sk.add_constraint(Vertical { l: l[3] }, None).unwrap();
        sk.add_constraint(Length { l: l[0], value: 40.0 }, None).unwrap();
        sk.add_constraint(Length { l: l[1], value: 30.0 }, None).unwrap();
    }
    let t = std::time::Instant::now();
    let rep = solve(&mut sk);
    let ms = t.elapsed().as_millis();
    assert!(rep.ok());
    assert_eq!(rep.dof, 300);
    assert!(ms < 10_000, "{ms} ms");
    eprintln!("large sketch: {ms} ms");
}

#[test]
fn long_connected_chain_solves() {
    let mut sk = Sketch::new();
    let n = 300;
    let mut prev = 0usize;
    for i in 0..n {
        let p = sk.add_point(v((i + 1) as f64 * 10.0 + 0.3, if i % 2 == 0 { 0.4 } else { -0.2 }), None).unwrap();
        let l = sk.add_line_pts(prev, p, None).unwrap();
        sk.add_constraint(ConstraintKind::Horizontal { l }, None).unwrap();
        sk.add_constraint(ConstraintKind::Length { l, value: 10.0 }, None).unwrap();
        prev = p;
    }
    let t = std::time::Instant::now();
    let rep = solve(&mut sk);
    let ms = t.elapsed().as_millis();
    eprintln!("chain: {ms} ms, iters {}", rep.iterations);
    assert!(rep.ok(), "{rep:?}");
    assert_eq!(rep.dof, 0);
    assert!(ms < 10_000, "{ms} ms");
}

/// Free-form curves stay exact in profiles: a degree-5 spline closed by a line, and a fit spline
/// cut in two by a line through it (each side gets an exact sub-curve ending on the line).
#[test]
fn exact_profiles_for_high_degree_and_cut_free_form_curves() {
    use solidcore_record::Seg2;
    let exact_loop = |p: &Profile| p.region.outer.segs.iter().all(|s| !matches!(s, Seg2::Line { .. }) || s.length() > 1.0);
    let closed = |p: &Profile| p.region.outer.joints().all(|(a, b)| a.end().dist(b.start()) < 1e-9);
    // Degree 5 over six points, closed by a line.
    let mut sk = Sketch::default();
    let ids: Vec<usize> = [v(0.0, 0.0), v(4.0, 10.0), v(10.0, 12.0), v(16.0, 12.0), v(22.0, 10.0), v(26.0, 0.0)]
        .iter()
        .map(|p| sk.add_point(*p, None).unwrap())
        .collect();
    sk.add_curve(CurveKind::Spline { pts: ids.clone(), control: true, degree: 5, periodic: false, nurbs: None, handles: Vec::new() }, None).unwrap();
    sk.add_line_pts(ids[5], ids[0], None).unwrap();
    let pr = find_profiles(&sk);
    assert_eq!(pr.len(), 1);
    assert!(pr[0].region.outer.segs.iter().any(|s| matches!(s, Seg2::Bezier { n: 5, .. })), "{:?}", pr[0].region.outer.segs);
    assert!(closed(&pr[0]));
    // A fit spline arch over a base line, cut by a vertical line at x = 10.
    let mut sk = Sketch::default();
    let ids: Vec<usize> = [v(0.0, 0.0), v(6.0, 9.0), v(14.0, 9.0), v(20.0, 0.0)].iter().map(|p| sk.add_point(*p, None).unwrap()).collect();
    sk.add_curve(CurveKind::Spline { pts: ids.clone(), control: false, degree: 3, periodic: false, nurbs: None, handles: Vec::new() }, None).unwrap();
    sk.add_line_pts(ids[3], ids[0], None).unwrap();
    let (a, b) = (sk.add_point(v(10.0, -5.0), None).unwrap(), sk.add_point(v(10.0, 20.0), None).unwrap());
    sk.add_line_pts(a, b, None).unwrap();
    let pr = find_profiles(&sk);
    assert_eq!(pr.len(), 2, "{pr:?}");
    let whole: f64 = pr.iter().map(|p| p.area).sum();
    for p in &pr {
        assert!(closed(p), "{:?}", p.region.outer.segs);
        assert!(exact_loop(p), "polyline pieces left: {:?}", p.region.outer.segs);
        // The crossing lies on the line x = 10.
        assert!(
            p.region
                .outer
                .segs
                .iter()
                .any(|s| matches!(s, Seg2::Cubic { .. }) && ((s.start().x - 10.0).abs() < 1e-9 || (s.end().x - 10.0).abs() < 1e-9))
        );
    }
    // Same total area as the uncut arch.
    sk.curves.pop();
    let uncut: f64 = find_profiles(&sk).iter().map(|p| p.area).sum();
    assert!((whole - uncut).abs() < 1e-6 * uncut, "{whole} vs {uncut}");
}

/// A face boundary saved with stations along its straight edges (#1049: booleans used to leave
/// vertices nothing else used) keeps its saved pieces when the face comes back with each
/// straight edge whole: the same region, and features that name the pieces still find them.
#[test]
fn projected_boundary_keeps_its_pieces_when_only_stations_go() {
    use crate::{LinkGeom, LinkKind, LinkSource};
    let mut sk = Sketch::new();
    let v = Vec2::new;
    let split = vec![
        LinkGeom::Line(v(0.0, 0.0), v(5.0, 0.0)),
        LinkGeom::Line(v(5.0, 0.0), v(10.0, 0.0)),
        LinkGeom::Line(v(10.0, 0.0), v(10.0, 4.0)),
        LinkGeom::Line(v(10.0, 4.0), v(10.0, 10.0)),
        LinkGeom::Line(v(10.0, 10.0), v(0.0, 10.0)),
        LinkGeom::Line(v(0.0, 10.0), v(0.0, 0.0)),
    ];
    let j = sk.add_link(LinkKind::Project, LinkSource::Origin, &split).unwrap();
    let ids: Vec<String> = sk.link_curves(&j).iter().map(|c| sk.curves[*c].id.clone()).collect();
    let whole = vec![
        LinkGeom::Line(v(0.0, 0.0), v(10.0, 0.0)),
        LinkGeom::Line(v(10.0, 0.0), v(10.0, 10.0)),
        LinkGeom::Line(v(10.0, 10.0), v(0.0, 10.0)),
        LinkGeom::Line(v(0.0, 10.0), v(0.0, 0.0)),
    ];
    assert!(!sk.update_link(&j, &whole).unwrap(), "only the stations went: the pieces stay");
    let kept: Vec<String> = sk.link_curves(&j).iter().map(|c| sk.curves[*c].id.clone()).collect();
    assert_eq!(ids, kept);
    // A boundary that really changed is rebuilt.
    let smaller = vec![
        LinkGeom::Line(v(0.0, 0.0), v(8.0, 0.0)),
        LinkGeom::Line(v(8.0, 0.0), v(8.0, 10.0)),
        LinkGeom::Line(v(8.0, 10.0), v(0.0, 10.0)),
        LinkGeom::Line(v(0.0, 10.0), v(0.0, 0.0)),
    ];
    assert!(sk.update_link(&j, &smaller).unwrap());
    assert_eq!(sk.link_curves(&j).len(), 4);
}

/// A projection that changes shape is rebuilt, and what was drawn on it stays: a line from a
/// projected corner starts at that corner again, points held on a side or a corner stay held
/// there (on a piece of a side that was split), and a line whose corner is gone is left where
/// it was, loose, instead of being deleted with the corner.
#[test]
fn rebuilt_projection_keeps_the_lines_drawn_on_it() {
    use crate::{LinkGeom, LinkKind, LinkSource};
    let mut sk = Sketch::new();
    let square = |s: f64| {
        vec![
            LinkGeom::Line(v(0.0, 0.0), v(s, 0.0)),
            LinkGeom::Line(v(s, 0.0), v(s, s)),
            LinkGeom::Line(v(s, s), v(0.0, s)),
            LinkGeom::Line(v(0.0, s), v(0.0, 0.0)),
        ]
    };
    let j = sk.add_link(LinkKind::Project, LinkSource::Origin, &square(10.0)).unwrap();
    let sides = sk.link_curves(&j);
    let corner = |sk: &Sketch, at: Vec2| sk.link_points(&j).into_iter().find(|p| sk.points[*p].pos.dist(at) < 1e-9).unwrap();
    let ends = |sk: &Sketch, id: &str| match sk.curves[sk.curve_index(id).unwrap()].kind {
        CurveKind::Line { a, b } => (a, b),
        _ => panic!("{id} is not a line"),
    };
    // A line drawn from the top right corner shares the projected point.
    let from_corner = sk.add_line(v(10.0, 10.0), v(20.0, 20.0), Some(corner(&sk, v(10.0, 10.0))), None, None).unwrap();
    // Lines held on the bottom side, on the right side and on the top left corner.
    let on_bottom = sk.add_line(v(4.0, 0.0), v(4.0, -6.0), None, None, None).unwrap();
    let on_right = sk.add_line(v(10.0, 3.0), v(16.0, 3.0), None, None, None).unwrap();
    let to_corner = sk.add_line(v(-5.0, 15.0), v(0.0, 10.0), None, None, None).unwrap();
    let ids: Vec<String> = [from_corner, on_bottom, on_right, to_corner].iter().map(|c| sk.curves[*c].id.clone()).collect();
    let (bottom_foot, _) = ends(&sk, &ids[1]);
    let (right_foot, _) = ends(&sk, &ids[2]);
    let (_, tip) = ends(&sk, &ids[3]);
    sk.add_constraint(ConstraintKind::PointOnCurve { p: bottom_foot, c: sides[0] }, None).unwrap();
    sk.add_constraint(ConstraintKind::PointOnCurve { p: right_foot, c: sides[1] }, None).unwrap();
    sk.add_constraint(ConstraintKind::Coincident { p: tip, q: corner(&sk, v(0.0, 10.0)) }, None).unwrap();
    let (corner_id, bottom_id) = (sk.points[corner(&sk, v(10.0, 10.0))].id.clone(), sk.curves[sides[0]].id.clone());
    // The projection grows and its right side comes in two pieces: another shape, rebuilt.
    let grown = vec![
        LinkGeom::Line(v(0.0, 0.0), v(15.0, 0.0)),
        LinkGeom::Line(v(15.0, 0.0), v(15.0, 7.0)),
        LinkGeom::Line(v(15.0, 7.0), v(15.0, 15.0)),
        LinkGeom::Line(v(15.0, 15.0), v(0.0, 15.0)),
        LinkGeom::Line(v(0.0, 15.0), v(0.0, 0.0)),
    ];
    assert!(sk.update_link(&j, &grown).unwrap());
    assert_eq!(sk.link_curves(&j).len(), 5);
    for id in &ids {
        assert!(sk.curve_index(id).is_some(), "{id} was deleted with the projection");
    }
    let (start, _) = ends(&sk, &ids[0]);
    assert_eq!(sk.points[start].link.as_deref(), Some(j.as_str()), "the line starts at the new projected corner");
    assert!(sk.points[start].pos.dist(v(15.0, 15.0)) < 1e-9);
    // The corner and the side that were matched keep their ids: they moved, they are not new.
    assert_eq!(sk.points[start].id, corner_id);
    assert!(sk.curve_index(&bottom_id).is_some_and(|c| sk.is_linked_curve(c)));
    assert_eq!(sk.constraints.len(), 3, "{:?}", sk.constraints);
    assert!(solve(&mut sk).ok());
    let at = |sk: &Sketch, id: &str, end: usize| {
        let (a, b) = ends(sk, id);
        sk.points[if end == 0 { a } else { b }].pos
    };
    assert!(at(&sk, &ids[1], 0).y.abs() < 1e-9, "held on the bottom side");
    assert!((at(&sk, &ids[2], 0).x - 15.0).abs() < 1e-9, "held on the right side");
    assert!(at(&sk, &ids[3], 1).dist(v(0.0, 15.0)) < 1e-9, "held on the top left corner");
    // The projection becomes a circle: no corner or side is left to hold the lines. They stay
    // where they were, and the one that shared a corner gets a point of its own there.
    let before: Vec<(Vec2, Vec2)> = ids.iter().map(|id| (at(&sk, id, 0), at(&sk, id, 1))).collect();
    assert!(sk.update_link(&j, &[LinkGeom::Circle(v(7.5, 7.5), 4.0)]).unwrap());
    for (id, (a, b)) in ids.iter().zip(&before) {
        assert!(sk.curve_index(id).is_some(), "{id} was deleted with the projection");
        assert_eq!((at(&sk, id, 0), at(&sk, id, 1)), (*a, *b));
    }
    let (start, _) = ends(&sk, &ids[0]);
    assert!(sk.points[start].link.is_none() && !sk.points[start].fixed);
    assert!(sk.constraints.is_empty(), "{:?}", sk.constraints);
    assert!(solve(&mut sk).ok());
}

#[test]
fn a_degree_nine_control_spline_stays_exact_and_closes_a_profile_exactly() {
    // A G4 blend curve is one degree-9 Bézier; a sketch keeps it as a control spline whose exact
    // piece is that Bézier, and profile measurement must keep using the curve, not its chords.
    let mut sk = Sketch::new();
    let bump =
        [v(0.0, 0.0), v(1.5, 4.0), v(3.0, 7.0), v(5.0, 9.5), v(7.5, 11.0), v(12.5, 11.0), v(15.0, 9.5), v(17.0, 7.0), v(18.5, 4.0), v(20.0, 0.0)];
    let pts: Vec<usize> = bump.iter().map(|q| sk.add_point(*q, None).unwrap()).collect();
    let s = sk
        .add_curve(CurveKind::Spline { pts: pts.clone(), control: true, degree: 9, periodic: false, nurbs: None, handles: Vec::new() }, None)
        .unwrap();
    let pieces = sk.exact_pieces(s);
    assert_eq!(pieces.len(), 1, "{pieces:?}");
    let Seg2::Bezier { n, p } = pieces[0] else { panic!("{:?}", pieces[0]) };
    assert_eq!(n, 9, "the highest degree a sketch piece can carry");
    for (i, q) in bump.iter().enumerate() {
        assert!(p[i].dist(*q) < 1e-12, "control {i}");
    }
    // Evaluation and the exact piece agree at the spline's real degree (a degree clamp would show
    // here), and a line's own exact piece still comes back as a line.
    let exact = pieces[0];
    for k in 0..=10 {
        let t = k as f64 / 10.0;
        assert!(crate::curves::spline_point(&bump, true, 9, t).dist(exact.point_at(t)) < 1e-9, "t={t}");
    }
    let l = sk.add_line(v(-5.0, 0.0), v(-2.0, 1.0), None, None, None).unwrap();
    assert!(matches!(sk.exact_pieces(l).as_slice(), [Seg2::Line { .. }]));
    // Close the loop: the region's area is the shoelace integral of the exact pieces, which a fine
    // sampling of the piece reproduces (a chord approximation would miss the bump's sagitta).
    sk.add_line(v(20.0, 0.0), v(0.0, 0.0), Some(pts[9]), Some(pts[0]), None).unwrap();
    let ps = find_profiles(&sk);
    assert_eq!(ps.len(), 1, "{ps:?}");
    let n = 20_000;
    let mut sampled = 0.0;
    for i in 0..n {
        let (t0, t1) = (i as f64 / n as f64, (i + 1) as f64 / n as f64);
        let (a, b) = (exact.point_at(t0), exact.point_at(t1));
        sampled += 0.5 * a.cross(b);
    }
    sampled -= 0.5 * v(20.0, 0.0).cross(v(0.0, 0.0));
    assert!((ps[0].area - sampled.abs()).abs() < 1e-5, "{} vs {sampled}", ps[0].area);
    assert!((ps[0].area - 100.0).abs() > 1e-3, "the region is not its chord: {}", ps[0].area);
    // A degree above the cap is still refused.
    let mut high: Vec<usize> = pts.clone();
    high.push(sk.add_point(v(21.0, -2.0), None).unwrap());
    assert!(
        sk.add_curve(CurveKind::Spline { pts: high, control: true, degree: 10, periodic: false, nurbs: None, handles: Vec::new() }, None).is_err()
    );
}

/// How far apart two curves run: the largest distance from a sample of either to the other.
fn curves_apart(a: &[Vec2], b: &[Vec2]) -> f64 {
    let one_way = |from: &[Vec2], to: &[Vec2]| from.iter().map(|q| crate::curves::polyline_signed_dist(to, *q).abs()).fold(0.0, f64::max);
    one_way(a, b).max(one_way(b, a))
}

/// A fit spline through five points, its points and curve index.
fn wave_spline(sk: &mut Sketch, periodic: bool) -> (Vec<usize>, usize) {
    let mut shape = vec![v(0.0, 0.0), v(10.0, 12.0), v(25.0, 4.0), v(38.0, 15.0), v(50.0, 0.0)];
    if periodic {
        shape.push(v(0.0, 0.0));
    }
    let mut pts: Vec<usize> = shape.iter().map(|q| sk.add_point(*q, None).unwrap()).collect();
    if periodic {
        pts.pop();
        pts.push(pts[0]);
    }
    let s =
        sk.add_curve(CurveKind::Spline { pts: pts.clone(), control: false, degree: 3, periodic, nurbs: None, handles: Vec::new() }, None).unwrap();
    (pts, s)
}

/// The distance from `q` to the exact curve `ci`.
fn off_curve(sk: &Sketch, ci: usize, q: Vec2) -> f64 {
    sk.exact_pieces(ci).iter().map(|seg| seg.point_at(seg.closest_param(q)).dist(q)).fold(f64::INFINITY, f64::min)
}

/// #6533: a fit point added to a spline lands on the curve where it was asked for, in order
/// between its neighbours. The curve still runs through every old point and the new one, and
/// elsewhere barely moves: most where interpolation through one more point bends the end span.
#[test]
fn a_fit_point_inserted_into_a_spline_keeps_the_curve_through_its_points() {
    let mut sk = Sketch::new();
    let (pts, s) = wave_spline(&mut sk, false);
    let before = sk.polyline(s);
    let old: Vec<Vec2> = pts.iter().map(|i| sk.point(*i).unwrap()).collect();
    for (at, slot, apart) in [(v(18.0, 9.0), 2, 0.05), (v(31.0, 9.0), 3, 0.05), (v(44.0, 9.0), 4, 1.0)] {
        let mut k = sk.clone();
        let p = k.insert_spline_point(s, at).unwrap();
        let new = k.point(p).unwrap();
        assert!(off_curve(&sk, s, new) < 1e-9, "the new point is on the old curve: {new:?}");
        assert!(new.dist(at) < 2.0, "near the spot asked for: {new:?}");
        let CurveKind::Spline { pts: now, control, periodic, .. } = &k.curves[s].kind else { panic!() };
        assert!(!control && !periodic);
        assert_eq!(now.len(), pts.len() + 1);
        assert_eq!(now[slot], p, "between its neighbours: {now:?}");
        assert_eq!(now.iter().filter(|i| **i != p).copied().collect::<Vec<_>>(), pts, "the old points keep their order");
        for q in old.iter().chain([&new]) {
            assert!(off_curve(&k, s, *q) < 1e-9, "the new curve runs through {q:?}");
        }
        let moved = curves_apart(&before, &k.polyline(s));
        assert!(moved < apart, "at {at:?} the curve moved {moved}");
    }
    // A spot on an existing point, a control point spline and something not a spline are refused.
    let mut k = sk.clone();
    assert!(k.insert_spline_point(s, old[2]).is_err());
    assert_eq!(k, sk, "a refused insertion changes nothing");
    let cv: Vec<usize> = old.iter().map(|q| k.add_point(*q + v(0.0, 30.0), None).unwrap()).collect();
    let c = k.add_curve(CurveKind::Spline { pts: cv, control: true, degree: 3, periodic: false, nurbs: None, handles: Vec::new() }, None).unwrap();
    assert!(k.insert_spline_point(c, v(20.0, 38.0)).is_err());
    let l = k.add_line(v(0.0, -10.0), v(50.0, -10.0), None, None, None).unwrap();
    assert!(k.insert_spline_point(l, v(20.0, -10.0)).is_err());
    assert!(k.insert_spline_point(s, v(f64::NAN, 0.0)).is_err());
}

/// #6533: a closed spline takes a point in its closing span too, before the repeated first point,
/// and stays closed.
#[test]
fn a_fit_point_inserted_into_a_closed_spline_keeps_it_closed() {
    let mut sk = Sketch::new();
    let (pts, s) = wave_spline(&mut sk, true);
    let p = sk.insert_spline_point(s, v(25.0, -8.0)).unwrap();
    let CurveKind::Spline { pts: now, periodic, .. } = &sk.curves[s].kind else { panic!() };
    assert!(periodic);
    assert_eq!(now.len(), pts.len() + 1);
    assert_eq!((now[now.len() - 2], now.first(), now.last()), (p, Some(&pts[0]), Some(&pts[0])));
    assert!(off_curve(&sk, s, sk.point(p).unwrap()) < 1e-9);
    assert!(sk.extend_spline(s, v(60.0, 0.0), false).is_err(), "a closed spline has no end");
}

/// #6533: a spline runs on to a new first or last point, unless that end is joined to something.
#[test]
fn a_spline_extends_to_a_new_end_unless_the_end_is_joined() {
    let mut sk = Sketch::new();
    let (pts, s) = wave_spline(&mut sk, false);
    let end = sk.extend_spline(s, v(60.0, 10.0), false).unwrap();
    let start = sk.extend_spline(s, v(-10.0, 5.0), true).unwrap();
    let CurveKind::Spline { pts: now, .. } = &sk.curves[s].kind else { panic!() };
    assert_eq!((now.first(), now.last(), now.len()), (Some(&start), Some(&end), pts.len() + 2));
    assert_eq!(sk.curves[s].kind.ends(), Some((start, end)));
    // A line from the end: the end is shared, so the spline does not run past it.
    sk.add_line(v(60.0, 10.0), v(70.0, 10.0), Some(end), None, None).unwrap();
    assert!(sk.extend_spline(s, v(65.0, 20.0), false).is_err());
    assert!(sk.extend_spline(s, v(-10.0, 5.0), true).is_err(), "already a point there");
}

/// Where spline `s`'s points are, in order.
fn spline_places(sk: &Sketch, s: usize) -> Vec<Vec2> {
    let CurveKind::Spline { pts, .. } = &sk.curves[s].kind else { panic!("{s} is no spline") };
    pts.iter().map(|i| sk.point(*i).unwrap()).collect()
}

/// #10783: a spline gives up one of its points. The curve runs through the others, in order, and
/// the point goes with the constraints on it; a point a line also ends at stays, for the line.
#[test]
fn a_point_removed_from_a_spline_goes_and_the_curve_keeps_the_others() {
    let mut sk = Sketch::new();
    let (pts, s) = wave_spline(&mut sk, false);
    let mut others = spline_places(&sk, s);
    let gone = others.remove(2);
    // A free point held on the one that goes: the constraint goes, the free point stays.
    let free = sk.add_point(gone, None).unwrap();
    sk.add_constraint(ConstraintKind::Coincident { p: pts[2], q: free }, None).unwrap();
    let count = sk.points.len();
    assert_eq!(sk.remove_spline_point(s, pts[2]), Ok(true));
    assert_eq!(spline_places(&sk, s), others);
    for q in &others {
        assert!(off_curve(&sk, s, *q) < 1e-9, "the curve still runs through {q:?}");
    }
    assert!(matches!(sk.curves[s].kind, CurveKind::Spline { control: false, periodic: false, .. }));
    assert_eq!(sk.points.len(), count - 1, "the point went");
    assert!(sk.constraints.is_empty(), "the constraint on it went: {:?}", sk.constraints);
    // A free end goes too: the spline starts at the next point.
    let start = sk.curves[s].kind.ends().unwrap().0;
    assert_eq!(sk.remove_spline_point(s, start), Ok(true));
    others.remove(0);
    assert_eq!(spline_places(&sk, s), others);
    // An inner point a line also ends at stays, for the line.
    let CurveKind::Spline { pts: now, .. } = sk.curves[s].kind.clone() else { panic!() };
    let line = sk.add_line(others[1], v(40.0, 30.0), Some(now[1]), None, None).unwrap();
    assert_eq!(sk.remove_spline_point(s, now[1]), Ok(false));
    assert!(sk.curves[line].kind.uses(now[1]) && !sk.curves[s].kind.uses(now[1]));
    others.remove(1);
    assert_eq!(spline_places(&sk, s), others);
    // Two points are as few as a spline keeps.
    assert_eq!(others.len(), 2);
    let before = sk.clone();
    assert!(sk.remove_spline_point(s, sk.curves[s].kind.ends().unwrap().1).is_err());
    assert_eq!(sk, before, "a refused removal changes nothing");
}

/// #10783: what a spline keeps: an end joined to a line or held by a constraint, a closed
/// spline's closing point and its last three points. A control point spline gives up a point as a
/// fit point spline does.
#[test]
fn a_spline_keeps_its_joined_ends_and_closing_point() {
    let mut sk = Sketch::new();
    let (pts, s) = wave_spline(&mut sk, false);
    let line = sk.add_line(sk.point(pts[4]).unwrap(), v(60.0, 0.0), Some(pts[4]), None, None).unwrap();
    let tied = sk.add_point(sk.point(pts[0]).unwrap(), None).unwrap();
    sk.add_constraint(ConstraintKind::Coincident { p: pts[0], q: tied }, None).unwrap();
    let before = sk.clone();
    assert!(sk.remove_spline_point(s, pts[4]).is_err(), "a line ends there");
    assert!(sk.remove_spline_point(s, pts[0]).is_err(), "a constraint holds the start");
    assert!(sk.remove_spline_point(s, tied).is_err(), "not one of the spline's points");
    assert!(sk.remove_spline_point(line, pts[4]).is_err(), "a line has no spline points");
    assert_eq!(sk, before, "a refused removal changes nothing");

    let mut sk = Sketch::new();
    let (pts, s) = wave_spline(&mut sk, true);
    assert!(sk.remove_spline_point(s, pts[0]).is_err(), "the closing point");
    assert_eq!(sk.remove_spline_point(s, pts[2]), Ok(true));
    assert_eq!(sk.remove_spline_point(s, pts[1]), Ok(true));
    let CurveKind::Spline { pts: now, periodic, .. } = sk.curves[s].kind.clone() else { panic!() };
    assert!(periodic);
    assert_eq!(now.len(), 4, "three points and the closing one: {now:?}");
    assert!(sk.remove_spline_point(s, now[1]).is_err(), "a closed spline keeps three points");
    sk.validate_periodic_splines().unwrap();

    let mut sk = Sketch::new();
    let places = [v(0.0, 0.0), v(10.0, 10.0), v(20.0, -10.0), v(30.0, 0.0)];
    let cv: Vec<usize> = places.iter().map(|q| sk.add_point(*q, None).unwrap()).collect();
    let c = sk
        .add_curve(CurveKind::Spline { pts: cv.clone(), control: true, degree: 3, periodic: false, nurbs: None, handles: Vec::new() }, None)
        .unwrap();
    assert_eq!(sk.remove_spline_point(c, cv[1]), Ok(true));
    assert!(matches!(sk.curves[c].kind, CurveKind::Spline { control: true, .. }));
    assert_eq!(spline_places(&sk, c), vec![places[0], places[2], places[3]]);
}

/// #755: a line drawn through a junction of a loop, but 2e-5 off it, still splits the loop. The
/// junction lies on the loop's two curves only, so it cuts neither of them where the line
/// crosses, and that crossing, next to the junction, was taken for one already cut there: the
/// line crossed the loop without meeting it, and the sketch had no profiles at all.
#[test]
fn profiles_line_passing_next_to_a_junction_splits_the_loop() {
    let o = v(0.0, 0.0);
    let at = |r: f64, a: f64| v(r * a.cos(), r * a.sin());
    // A slot: half circles of radius 1 round (0, 0) and (-5, 0), joined by the lines y = 1 and
    // y = -1, which meet the arcs tangentially at (0, 1) and (0, -1).
    let mut sk = Sketch::new();
    let right = sk.add_arc(o, v(0.0, -1.0), v(0.0, 1.0), [None; 3], None).unwrap();
    let left = sk.add_arc(v(-5.0, 0.0), v(-5.0, 1.0), v(-5.0, -1.0), [None; 3], None).unwrap();
    let ends = |i: usize| match sk.curves[i].kind {
        CurveKind::Arc { a, b, .. } => (a, b),
        _ => panic!("an arc"),
    };
    let ((r0, r1), (l0, l1)) = (ends(right), ends(left));
    sk.add_line_pts(r1, l0, None).unwrap();
    sk.add_line_pts(l1, r0, None).unwrap();
    // Lines from (0, 0) through the junctions to an arc of radius 2. The upper one is turned by
    // 2e-5 rad, so it passes 2e-5 from (0, 1) and crosses the line y = 1 next to it.
    let d = 2e-5;
    let (top, bottom) = (at(2.0, FRAC_PI_2 + d), v(0.0, -2.0));
    sk.add_line(o, top, None, None, None).unwrap();
    sk.add_line(o, bottom, None, None, None).unwrap();
    sk.add_arc(o, bottom, top, [None; 3], None).unwrap();
    let mut areas: Vec<f64> = find_profiles(&sk).iter().map(|p| p.area).collect();
    areas.sort_by(f64::total_cmp);
    // The right half disc, the band round it out to radius 2, and the rest of the slot.
    let want = [FRAC_PI_2, FRAC_PI_2 * (4.0 - 1.0), 10.0 + FRAC_PI_2];
    assert_eq!(areas.len(), 3, "{areas:?}");
    for (got, want) in areas.iter().zip(want) {
        assert!((got - want).abs() < 1e-3, "{areas:?} against {want}");
    }
}

/// A line ending 2e-5 from a corner, next to a crossing there: the corner, where the side of the
/// square meets the long line, stays a cut of the long line, and the square stays a profile.
#[test]
fn profiles_crossing_next_to_a_corner_keeps_the_corner() {
    let mut sk = Sketch::new();
    let (a, b) = (sk.add_point(v(1.0, 0.0), None).unwrap(), sk.add_point(v(2.0, 0.0), None).unwrap());
    let (c, d) = (sk.add_point(v(2.0, 1.0), None).unwrap(), sk.add_point(v(1.0, 1.0), None).unwrap());
    // The square's base is part of a longer line, which its sides end on.
    sk.add_line(v(0.0, 0.0), v(3.0, 0.0), None, None, None).unwrap();
    for (p, q) in [(b, c), (c, d), (d, a)] {
        sk.add_line_pts(p, q, None).unwrap();
    }
    // A line hanging across the long one, 2e-5 from the square's corner (1, 0).
    sk.add_line(v(1.0 - 2e-5, -0.5), v(1.0 - 2e-5, 0.5), None, None, None).unwrap();
    let areas: Vec<f64> = find_profiles(&sk).iter().map(|p| p.area).collect();
    assert_eq!(areas.len(), 1, "{areas:?}");
    assert!(areas.iter().all(|x| (x - 1.0).abs() < 1e-9), "{areas:?}");
}

/// A line across a rectangle, and a stray line ending on it 1e-3 from where it crosses the
/// rectangle's side: both the crossing and that end cut the long line, and the rectangle is
/// split into its two halves.
#[test]
fn profiles_crossing_next_to_a_stray_end_still_splits() {
    let mut sk = Sketch::new();
    rect(&mut sk, [v(0.0, -1.0), v(1.0, -1.0), v(1.0, 1.0), v(0.0, 1.0)]);
    sk.add_line(v(-1.0, 0.0), v(2.0, 0.0), None, None, None).unwrap();
    sk.add_line(v(0.00099998, 0.0000099), v(0.3, 0.6), None, None, None).unwrap();
    let areas: Vec<f64> = find_profiles(&sk).iter().map(|p| p.area).collect();
    assert_eq!(areas.len(), 2, "{areas:?}");
    assert!(areas.iter().all(|x| (x - 1.0).abs() < 1e-9), "{areas:?}");
}

/// A line whose own end overshoots a crossing line by 2e-5, with a rectangle standing on it 1e-3
/// further on: the crossing next to that end gives way to the rectangle's corner, which still
/// cuts the line, and the rectangle stays a profile.
#[test]
fn profiles_crossing_next_to_an_overshooting_end_keeps_the_corner_beside_it() {
    let mut sk = Sketch::new();
    sk.add_line(v(-2e-5, 0.0), v(3.0, 0.0), None, None, None).unwrap();
    sk.add_line(v(0.0, -0.5), v(0.0, 0.5), None, None, None).unwrap();
    let x = 0.00099999;
    let p = [v(x, 0.0000099), v(x, 1.0), v(2.0, 1.0), v(2.0, 0.0)].map(|q| sk.add_point(q, None).unwrap());
    for w in p.windows(2) {
        sk.add_line_pts(w[0], w[1], None).unwrap();
    }
    let areas: Vec<f64> = find_profiles(&sk).iter().map(|p| p.area).collect();
    assert_eq!(areas.len(), 1, "{areas:?}");
    assert!(areas.iter().all(|a| (a - (2.0 - x)).abs() < 1e-4), "{areas:?}");
}

#[test]
fn zero_component_dimensions_keep_independent_rank() {
    for vertical in [false, true] {
        for value in [0.0, 0.5, 1.0] {
            let mut sk = Sketch::new();
            let q = sk.add_point(if vertical { v(10.0, 3.0) } else { v(3.0, 10.0) }, None).unwrap();
            let existing =
                if vertical { ConstraintKind::DistanceX { p: 0, q, value: 10.0 } } else { ConstraintKind::DistanceY { p: 0, q, value: 10.0 } };
            sk.add_constraint(existing, None).unwrap();
            assert_eq!(solve(&mut sk).dof, 1);
            let added = if vertical { ConstraintKind::DistanceY { p: 0, q, value } } else { ConstraintKind::DistanceX { p: 0, q, value } };
            let id = sk.add_constraint(added, None).unwrap();
            let report = solve(&mut sk);
            assert!(report.ok(), "{report:?}");
            assert_eq!(report.dof, 0, "{vertical}, {value}: {report:?}");
            assert!(!report.redundant.contains(&id), "{report:?}");
            let ci = sk.constraints.iter().position(|c| c.id == id).unwrap();
            assert!(determined_by(&sk, ci).is_empty());
            assert!(constraint_residual(&sk, ci).unwrap() < 1e-6);
        }
    }
}

/// A fillet as `sketch.fillet` makes it: `l1` from (10, 10) to (35, 10), the arc round
/// (35, 15) from (35, 10) to (40, 15), `l2` from (40, 15) to (40, 30); horizontal, vertical,
/// radius 5, tangent at both ends. Returns (l1, arc, l2, l1's far point, l2's far point).
fn filleted_corner() -> (Sketch, usize, usize, usize, usize, usize) {
    let mut sk = Sketch::new();
    let l1 = sk.add_line(v(10.0, 10.0), v(35.0, 10.0), None, None, None).unwrap();
    let CurveKind::Line { a: far1, b: ta } = sk.curves[l1].kind else { panic!() };
    let arc = sk.add_arc(v(35.0, 15.0), v(35.0, 10.0), v(40.0, 15.0), [None, Some(ta), None], None).unwrap();
    let CurveKind::Arc { b: tb, .. } = sk.curves[arc].kind else { panic!() };
    let l2 = sk.add_line(v(40.0, 15.0), v(40.0, 30.0), Some(tb), None, None).unwrap();
    let CurveKind::Line { b: far2, .. } = sk.curves[l2].kind else { panic!() };
    use ConstraintKind::*;
    sk.add_constraint(Horizontal { l: l1 }, None).unwrap();
    sk.add_constraint(Vertical { l: l2 }, None).unwrap();
    sk.add_constraint(Radius { c: arc, value: 5.0 }, None).unwrap();
    sk.add_constraint(Tangent { a: l1, b: arc }, None).unwrap();
    sk.add_constraint(Tangent { a: l2, b: arc }, None).unwrap();
    assert!(solve(&mut sk).ok());
    (sk, l1, arc, l2, far1, far2)
}

/// The arc's counter-clockwise sweep from its stored `a` to `b`, degrees.
fn sweep_deg(sk: &Sketch, arc: usize) -> f64 {
    let CurveKind::Arc { c, a, b } = sk.curves[arc].kind else { panic!() };
    let (c, a, b) = (sk.points[c].pos, sk.points[a].pos, sk.points[b].pos);
    ((b - c).angle() - (a - c).angle()).rem_euclid(2.0 * PI).to_degrees()
}

/// Each line leaves the arc at its touch point along the arc's travel (a smooth path, not a
/// line cutting back through the arc).
fn lines_leave_the_arc(sk: &Sketch, arc: usize, lines: [usize; 2]) -> bool {
    let CurveKind::Arc { c, a, b } = sk.curves[arc].kind else { panic!() };
    lines.iter().all(|&l| {
        let CurveKind::Line { a: la, b: lb } = sk.curves[l].kind else { panic!() };
        let (p, far) = if la == a || la == b { (la, lb) } else { (lb, la) };
        let outward = if p == b { 1.0 } else { -1.0 };
        (sk.points[p].pos - sk.points[c].pos).perp().dot(sk.points[far].pos - sk.points[p].pos) * outward > 1e-6
    })
}

#[test]
fn a_fillet_follows_its_corner_when_the_corner_mirrors() {
    // Pin the vertical line's far end to a point below the horizontal line: the corner mirrors.
    // The fillet is the quarter arc on the other side, not a line through its arc or a
    // three-quarter arc.
    let (mut sk, l1, arc, l2, _, far2) = filleted_corner();
    let q = sk.add_point(v(40.0, -20.0), None).unwrap();
    sk.points[q].fixed = true;
    sk.add_constraint(ConstraintKind::Coincident { p: far2, q }, None).unwrap();
    let rep = solve(&mut sk);
    assert!(rep.ok(), "{rep:?}");
    assert!((sweep_deg(&sk, arc) - 90.0).abs() < 1e-6, "sweep {}", sweep_deg(&sk, arc));
    assert!(lines_leave_the_arc(&sk, arc, [l1, l2]));
    assert!(sk.center(arc).unwrap().dist(v(35.0, 5.0)) < 1e-6, "{:?}", sk.center(arc));
    let (_, q1) = line_pts(&sk, l1);
    let (p2, _) = line_pts(&sk, l2);
    assert!(q1.dist(v(35.0, 10.0)) < 1e-6 && p2.dist(v(40.0, 5.0)) < 1e-6, "{q1:?} {p2:?}");
    // Dragged back up (a move against the positions before it), it mirrors again.
    sk.constraints.retain(|c| !matches!(c.kind, ConstraintKind::Coincident { .. }));
    let reference: Vec<Vec2> = sk.points.iter().map(|p| p.pos).collect();
    sk.points[far2].pos = v(40.0, 30.0);
    let rep = solve_with(&mut sk, &Hold { points: vec![far2], radii: Vec::new() }, Some(&reference), Analysis::Full, None);
    assert!(rep.ok(), "{rep:?}");
    assert!((sweep_deg(&sk, arc) - 90.0).abs() < 1e-6, "sweep {}", sweep_deg(&sk, arc));
    assert!(lines_leave_the_arc(&sk, arc, [l1, l2]));
    assert!(sk.center(arc).unwrap().dist(v(35.0, 15.0)) < 1e-6, "{:?}", sk.center(arc));
}

#[test]
fn a_fillet_folded_through_flat_is_refused_like_a_fold() {
    // Without the horizontal and vertical, the second line turns about the corner: the arc
    // shrinks towards flat (the lines collinear) and the lines keep leaving it. Past flat the
    // only arc left is the loop the other way round; that step is refused as a fold and the
    // sketch stays where it was.
    let (mut sk, l1, arc, l2, _, far2) = filleted_corner();
    sk.constraints.retain(|c| !matches!(c.kind, ConstraintKind::Horizontal { .. } | ConstraintKind::Vertical { .. }));
    let hold = Hold { points: vec![far2], radii: Vec::new() };
    let mut refused = None;
    for step in 1..=16 {
        let t = step as f64 / 16.0 * PI;
        let reference: Vec<Vec2> = sk.points.iter().map(|p| p.pos).collect();
        // From straight up, round through the right towards straight down, 15 mm from the corner.
        sk.points[far2].pos = v(40.0, 10.0) + Vec2::from_angle(FRAC_PI_2 - t) * 15.0;
        let rep = solve_with(&mut sk, &hold, Some(&reference), Analysis::Full, None);
        if !rep.ok() {
            assert!(!rep.folded.is_empty(), "step {step}: {rep:?}");
            let stays = sk.points.iter().enumerate().filter(|(i, _)| *i != far2).all(|(i, p)| p.pos == reference[i]);
            assert!(stays, "step {step}: the sketch stays");
            refused = Some(step);
            break;
        }
        let sweep = sweep_deg(&sk, arc);
        assert!(sweep < 90.0 + 1e-6, "step {step}: sweep {sweep}");
        assert!(lines_leave_the_arc(&sk, arc, [l1, l2]), "step {step}: sweep {sweep}");
    }
    assert!(refused.is_some_and(|s| (7..=9).contains(&s)), "refused at {refused:?}");
}

/// A sketch's bounds reach a circle's and an arc's extremes, not only the points of their
/// polylines: a whole circle is one arc whose chords start at its start angle, and a circle of
/// radius 5 at (-300, 0) spans x from -305 exactly.
#[test]
fn bounds_reach_the_extremes_of_circles_and_arcs() {
    let mut sk = Sketch::new();
    sk.add_circle(v(-300.0, 0.0), 5.0, None, None).unwrap();
    let (lo, hi) = sk.bounds().unwrap();
    assert!(lo.dist(v(-305.0, -5.0)) < 1e-9 && hi.dist(v(-295.0, 5.0)) < 1e-9, "{lo:?} {hi:?}");
    // A quarter arc from (10, 0) to (0, 10) about the origin: its corners, no more.
    let mut sk = Sketch::new();
    sk.add_arc(v(0.0, 0.0), v(10.0, 0.0), v(0.0, 10.0), [None; 3], None).unwrap();
    let (lo, hi) = sk.bounds().unwrap();
    assert!(lo.dist(v(0.0, 0.0)) < 1e-9 && hi.dist(v(10.0, 10.0)) < 1e-9, "{lo:?} {hi:?}");
}

/// A control spline with its own knots and weights (a projected free-form edge): its exact pieces
/// are its knot spans, a rational quadratic's are conics, its points follow its knots, it reads
/// back from a saved design the same, and knots that don't fit it are refused.
#[test]
fn a_spline_with_its_own_knots_is_its_spans_exactly() {
    let mut sk = Sketch::new();
    // Two cubic Béziers end to end (the joint knot three times), the second twice as long in
    // parameter: (0,0) (1,2) (2,2) (3,0) then (3,0) (4,-2) (5,-2) (6,0).
    let ctrl = [v(0.0, 0.0), v(1.0, 2.0), v(2.0, 2.0), v(3.0, 0.0), v(4.0, -2.0), v(5.0, -2.0), v(6.0, 0.0)];
    let nurbs = Nurbs { knots: vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 3.0, 3.0, 3.0, 3.0], weights: Vec::new() };
    let pts: Vec<usize> = ctrl.iter().map(|q| sk.add_point(*q, None).unwrap()).collect();
    let kind = CurveKind::Spline { pts: pts.clone(), control: true, degree: 3, periodic: false, nurbs: Some(nurbs.clone()), handles: Vec::new() };
    let s = sk.add_curve(kind.clone(), None).unwrap();
    let pieces = sk.exact_pieces(s);
    assert_eq!(pieces.len(), 2, "{pieces:?}");
    let Seg2::Cubic { p0, p1, p2, p3 } = pieces[1] else { panic!("{:?}", pieces[1]) };
    for (got, want) in [(p0, ctrl[3]), (p1, ctrl[4]), (p2, ctrl[5]), (p3, ctrl[6])] {
        assert!(got.dist(want) < 1e-12, "{got:?} {want:?}");
    }
    // t runs over the knots: a third of the way is the joint, where the uniform spline over the
    // same points would not be.
    assert!(nurbs.point(&ctrl, 3, 1.0 / 3.0).unwrap().dist(ctrl[3]) < 1e-12);
    assert!(crate::curves::spline_point(&ctrl, true, 3, 1.0 / 3.0).dist(ctrl[3]) > 1e-3);
    let back = nurbs.reversed();
    let rev: Vec<Vec2> = ctrl.iter().rev().copied().collect();
    for k in 0..=12 {
        let t = k as f64 / 12.0;
        assert!(nurbs.point(&ctrl, 3, t).unwrap().dist(back.point(&rev, 3, 1.0 - t).unwrap()) < 1e-12, "t={t}");
    }
    let poly = sk.polyline(s);
    assert!(poly.first().unwrap().dist(ctrl[0]) < 1e-12 && poly.last().unwrap().dist(ctrl[6]) < 1e-12);
    // Saved and read back: the same curve.
    let json = serde_json::to_string(&kind).unwrap();
    assert_eq!(serde_json::from_str::<CurveKind>(&json).unwrap(), kind);
    // A quarter circle of radius 2 as a rational quadratic: one conic, its middle point on the circle.
    let w = std::f64::consts::FRAC_1_SQRT_2;
    let quarter = Nurbs { knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0], weights: vec![1.0, w, 1.0] };
    let arc = [v(2.0, 0.0), v(2.0, 2.0), v(0.0, 2.0)];
    let [Seg2::Conic { w: cw, .. }] = quarter.segs(&arc, 2).unwrap()[..] else { panic!("conic") };
    assert!((cw - w).abs() < 1e-12);
    assert!((quarter.point(&arc, 2, 0.5).unwrap().len() - 2.0).abs() < 1e-12);
    // Knots that don't fit: the wrong count, not clamped, decreasing, not finite, an interior knot
    // past the degree, a weight that isn't positive; and knots on a fit spline.
    let bad = [
        Nurbs { knots: vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0], weights: Vec::new() },
        Nurbs { knots: vec![0.0, 0.0, 0.0, 0.5, 1.0, 1.0, 1.0, 3.0, 3.0, 3.0, 3.0], weights: Vec::new() },
        Nurbs { knots: vec![0.0, 0.0, 0.0, 0.0, 2.0, 1.0, 1.0, 3.0, 3.0, 3.0, 3.0], weights: Vec::new() },
        Nurbs { knots: vec![0.0, 0.0, 0.0, 0.0, f64::NAN, 1.0, 1.0, 3.0, 3.0, 3.0, 3.0], weights: Vec::new() },
        Nurbs { knots: vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 3.0, 3.0, 3.0, 3.0], weights: Vec::new() },
        Nurbs { knots: nurbs.knots.clone(), weights: vec![1.0, 1.0, 0.0, 1.0, 1.0, 1.0, 1.0] },
    ];
    for (i, k) in bad.iter().enumerate() {
        assert!(k.check(ctrl.len(), 3).is_err(), "{i}");
        assert!(
            sk.add_curve(
                CurveKind::Spline { pts: pts.clone(), control: true, degree: 3, periodic: false, nurbs: Some(k.clone()), handles: Vec::new() },
                None,
            )
            .is_err(),
            "{i}"
        );
        assert!(k.point(&ctrl, 3, 0.5).is_none() && k.segs(&ctrl, 3).is_none() && k.polyline(&ctrl, 3).is_empty(), "{i}");
    }
    let fourfold = Nurbs { knots: vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 3.0, 3.0, 3.0, 3.0], weights: Vec::new() };
    assert!(fourfold.check(8, 3).is_err());
    assert!(
        sk.add_curve(CurveKind::Spline { pts, control: false, degree: 3, periodic: false, nurbs: Some(nurbs), handles: Vec::new() }, None).is_err()
    );
}

/// #8639: two rounded rectangles drawn as closed control-point splines with the same straight
/// sides and different corners (as a DXF can carry an outline twice). Where the sides lie on each
/// other the two curves' sampled pieces end a few millionths apart; every profile must still close
/// within the 1e-6 a solid needs, or the region around them can't be extruded or shown.
#[test]
fn rounded_rectangles_sharing_their_sides_leave_every_profile_closed() {
    // Control points of a rounded rectangle 17.6 by 17.6, corner `r`, sides split in thirds.
    fn rounded(sk: &mut Sketch, r: f64) {
        let (lo, hi) = (0.0, 17.6);
        let k = r * 0.448;
        let mut pts = Vec::new();
        for (cx, cy, dx, dy) in [(hi, hi, 1.0, 1.0), (hi, lo, 1.0, -1.0), (lo, lo, -1.0, -1.0), (lo, hi, -1.0, 1.0)] {
            // Into the corner along the side, round it, out along the next side (clockwise).
            let (a, b) = if dx * dy > 0.0 { ((cx - dx * r, cy), (cx, cy - dy * r)) } else { ((cx, cy - dy * r), (cx - dx * r, cy)) };
            let (ka, kb) = if dx * dy > 0.0 { ((cx - dx * k, cy), (cx, cy - dy * k)) } else { ((cx, cy - dy * k), (cx - dx * k, cy)) };
            pts.extend([v(a.0, a.1), v(ka.0, ka.1), v(kb.0, kb.1), v(b.0, b.1)]);
            // Two more points along the next side, so it is straight.
            let (nx, ny) = if dx * dy > 0.0 { (cx, cy - dy * (hi - r)) } else { (cx - dx * (hi - r), cy) };
            let t = |f: f64| v(b.0 + (nx - b.0) * f, b.1 + (ny - b.1) * f);
            pts.extend([t(1.0 / 3.0), t(2.0 / 3.0)]);
        }
        let mut ids: Vec<usize> = pts.iter().map(|p| sk.add_point(*p, None).unwrap()).collect();
        ids.push(ids[0]);
        sk.add_curve(CurveKind::Spline { pts: ids, control: true, degree: 3, periodic: false, nurbs: None, handles: Vec::new() }, None).unwrap();
    }
    let mut sk = Sketch::new();
    rounded(&mut sk, 0.55);
    rounded(&mut sk, 0.8);
    sk.add_circle(v(8.8, 8.8), 10.0, None, None).unwrap();
    let ps = find_profiles(&sk);
    assert!(!ps.is_empty());
    for p in &ps {
        for lp in std::iter::once(&p.region.outer).chain(&p.region.holes) {
            let n = lp.segs.len();
            let gap = (0..n).map(|i| lp.segs[i].end().dist(lp.segs[(i + 1) % n].start())).fold(0.0, f64::max);
            assert!(lp.is_closed(1e-6), "a loop of {:?} is open by {gap:.3e}", crate::distinct_curves(&p.outer_curves));
        }
    }
}

/// A Pierce constraint holds a point of this sketch where a curve of another sketch crosses
/// the plane of this one (#3599): the point of the XY sketch sits where the XZ sketch's line
/// crosses it, and stays there.
#[test]
fn a_pierce_constraint_holds_its_point_where_the_other_sketches_curve_crosses_this_plane() {
    let mut sk = Sketch::new();
    let p = sk.add_point(v(20.0, 20.0), None).unwrap();
    // The other sketch's line on the XZ plane: it crosses the XY plane at (5, 0, 0).
    let world = vec![Vec3::new(5.0, 0.0, -5.0), Vec3::new(5.0, 0.0, 5.0)];
    sk.add_constraint(ConstraintKind::Pierce { p, sketch: 7, curve: "l1".into(), plane: Plane::XY, world }, None).unwrap();
    let report = solve(&mut sk);
    assert!(report.ok(), "{report:?}");
    let at = sk.points[p].pos;
    assert!((at.x - 5.0).abs() < 1e-9 && at.y.abs() < 1e-9, "{at:?}");
    // The point sits in the plane: nothing off it.
    assert!(Plane::XY.height(Plane::XY.to_world(at)).abs() < 1e-9);
}

/// The point follows the curve of the other sketch: the curve moves across this plane, the
/// pierced point moves with it.
#[test]
fn a_pierced_point_follows_the_curve_of_the_other_sketch() {
    let mut sk = Sketch::new();
    let p = sk.add_point(v(20.0, 20.0), None).unwrap();
    let world = vec![Vec3::new(5.0, 0.0, -5.0), Vec3::new(5.0, 0.0, 5.0)];
    sk.add_constraint(ConstraintKind::Pierce { p, sketch: 7, curve: "l1".into(), plane: Plane::XY, world }, None).unwrap();
    solve(&mut sk);
    assert!((sk.points[p].pos.x - 5.0).abs() < 1e-9, "{:?}", sk.points[p].pos);

    // The other sketch's line moves across this plane.
    if let ConstraintKind::Pierce { world, .. } = &mut sk.constraints[0].kind {
        *world = vec![Vec3::new(7.0, 0.0, -5.0), Vec3::new(7.0, 0.0, 5.0)];
    }
    solve(&mut sk);
    assert!((sk.points[p].pos.x - 7.0).abs() < 1e-9, "{:?}", sk.points[p].pos);
}

/// A pierce needs a curve that meets this sketch's plane: a curve that never crosses it has
/// no point to pierce, and the constraint is refused.
#[test]
fn a_pierce_constraint_whose_curve_never_crosses_the_plane_is_refused() {
    let mut sk = Sketch::new();
    let p = sk.add_point(v(20.0, 20.0), None).unwrap();
    // A line on the XZ plane that stays above the XY plane: it never reaches it.
    let world = vec![Vec3::new(5.0, 0.0, 1.0), Vec3::new(5.0, 0.0, 5.0)];
    let e = sk.add_constraint(ConstraintKind::Pierce { p, sketch: 7, curve: "l1".into(), plane: Plane::XY, world }, None).unwrap_err().to_string();
    assert!(e.contains("cross"), "{e}");
}

/// A pierce renumbers with the sketch: its point is renumbered, the curve of the other
/// sketch is not (it has no index here).
#[test]
fn a_pierce_constraint_remaps_its_point() {
    let mut kind = ConstraintKind::Pierce { p: 3, sketch: 7, curve: "l1".into(), plane: Plane::XY, world: vec![] };
    assert!(kind.remap(&|i| Some(i + 1), &|_| Some(0)));
    assert!(matches!(kind, ConstraintKind::Pierce { p: 4, .. }), "{kind:?}");
    // A point with no new number leaves the constraint partly renumbered: it must go.
    let mut kind = ConstraintKind::Pierce { p: 3, sketch: 7, curve: "l1".into(), plane: Plane::XY, world: vec![] };
    assert!(!kind.remap(&|_| None, &|_| Some(0)));
}

/// A drag step that cannot succeed gives up once the iteration has stalled (#6606). Holding
/// the two right corners of a rectangle further apart than its dimensioned left side leaves the
/// left corners a least-squares valley (the perpendicular repeats what horizontal and vertical
/// already say), along which the iteration crawled for its whole allowance.
#[test]
fn a_held_solve_that_cannot_succeed_stops_once_it_stalls() {
    let mut sk = Sketch::new();
    let [bottom, right, top, left] = rect(&mut sk, [v(-6.0, -20.0), v(23.5, -20.8), v(23.5, 0.0), v(-6.0, 0.0)]);
    for kind in [
        ConstraintKind::Horizontal { l: bottom },
        ConstraintKind::Horizontal { l: top },
        ConstraintKind::Vertical { l: right },
        ConstraintKind::Vertical { l: left },
        ConstraintKind::Length { l: left, value: 20.0 },
        ConstraintKind::Perpendicular { a: top, b: left },
    ] {
        sk.add_constraint(kind, None).unwrap();
    }
    let hold = Hold { points: sk.curves[right].kind.point_ids(), radii: vec![] };
    let reference: Vec<Vec2> = sk.points.iter().map(|p| p.pos).collect();
    let solve = |analysis| {
        let mut moved = sk.clone();
        let rep = solve_with(&mut moved, &hold, Some(&reference), analysis, None);
        assert!(!rep.ok());
        assert_eq!(moved.points, sk.points, "a failed solve leaves the geometry as it was");
        rep.iterations
    };
    let stalled = solve(Analysis::StatusOnly);
    assert!(stalled < 100, "a held solve gives up once stalled: {stalled} iterations");
    // A solve whose report names the violated constraints still iterates to the end.
    assert!(solve(Analysis::Full) >= 500);
}

/// Pattern upkeep runs before every solve, each drag frame included. It once looked up every
/// copy and its arc points by a linear search over all ids, so dragging anything in a sketch
/// with a large pattern of arcs spent most of each frame there; its searches must not grow
/// with the copies. The copies still follow their seed's arc direction.
#[test]
fn pattern_upkeep_does_not_search_ids_per_copy() {
    let mut sk = Sketch::new();
    let center = sk.add_point(v(0.0, 0.0), None).unwrap();
    let arc = sk.add_arc(v(20.0, 0.0), v(23.0, 0.0), v(20.0, 3.0), [None, None, None], None).unwrap();
    let entities = vec![sk.curves[arc].id.clone()];
    let kind = SketchPatternKind::Circular {
        center: sk.points[center].id.clone(),
        count: 50,
        angle: 360.0,
        alignment: PatternAlignment::Right,
        reversed: false,
    };
    let id = sk.fresh("pat").unwrap();
    sk.patterns.push(SketchPattern { id, entities, kind: kind.clone(), count_expression: None, instances: vec![], expressions: Default::default() });
    sk.update_pattern(0, kind).unwrap();
    assert_eq!(sk.curves.len(), 50);
    sk.curves[arc].reversed = true;
    ID_SEARCHES.with(|c| c.set(0));
    sk.refresh_pattern_shapes();
    assert_eq!(ID_SEARCHES.with(std::cell::Cell::get), 0, "id searches in pattern upkeep");
    assert!(sk.curves.iter().all(|c| c.reversed));
}

/// Inserting a dense drawing searched the whole sketch for every new id, so the insert took time
/// in proportion to the square of its size (about 40 s for 60000 lines). A batch searches no
/// sketch for an id, and gives the points and curves exactly the ids, checks and refusals that
/// adding them one at a time does, including after ids a damaged counter would reuse.
#[test]
fn a_batch_adds_points_and_curves_without_searching_for_ids() {
    let mut base = Sketch::new();
    base.add_point(v(0.0, 0.0), Some("p2")).unwrap();
    base.add_point(v(1.0, 0.0), Some("l1")).unwrap();
    let mut one_by_one = base.clone();
    let mut batched = base.clone();
    let lines = 2000;
    for i in 0..lines {
        let (x, y) = (f64::from(i % 50), f64::from(i / 50));
        let a = one_by_one.add_point(v(x, y), None).unwrap();
        let b = one_by_one.add_point(v(x + 0.5, y + 0.5), None).unwrap();
        one_by_one.add_curve(CurveKind::Line { a, b }, None).unwrap();
    }
    ID_SCANS.with(|c| c.set(0));
    {
        let mut batch = batched.batch();
        for i in 0..lines {
            let (x, y) = (f64::from(i % 50), f64::from(i / 50));
            let a = batch.add_point(v(x, y), None).unwrap();
            let b = batch.add_point(v(x + 0.5, y + 0.5), None).unwrap();
            batch.add_curve(CurveKind::Line { a, b }, None).unwrap();
        }
        assert!(matches!(batch.add_point(v(0.0, 0.0), Some("p2")), Err(SketchError::Duplicate(_))), "a taken id is refused");
        assert!(matches!(batch.add_curve(CurveKind::Line { a: 0, b: usize::MAX }, None), Err(SketchError::Invalid(_))), "the same checks");
        assert_eq!(batch.sketch().points.len(), one_by_one.points.len());
    }
    assert_eq!(ID_SCANS.with(std::cell::Cell::get), 0, "id searches of the whole sketch in a batch");
    assert_eq!(batched, one_by_one, "the same ids as one at a time");
    assert_eq!(batched.fresh("p").unwrap(), one_by_one.fresh("p").unwrap(), "the counters carry on alike");
}

/// A fillet whose corner the solve leaves where it is stays that corner's arc, even when the
/// reference it is judged against (the positions before a move, here the corner mirrored) shows
/// it the other way round: ending its sweep where it started is no turn of the corner, and
/// swapping its ends would make the fillet its long complement. A corner of 135° (lines turning
/// by 45°), all its points fixed.
#[test]
fn a_fillet_the_solve_leaves_in_place_keeps_its_short_arc_against_a_mirrored_reference() {
    let mut sk = Sketch::new();
    let s = std::f64::consts::FRAC_1_SQRT_2;
    let l1 = sk.add_line(v(10.0, 10.0), v(35.0, 10.0), None, None, None).unwrap();
    let CurveKind::Line { b: ta, .. } = sk.curves[l1].kind else { panic!() };
    let end = v(35.0 + 5.0 * s, 15.0 - 5.0 * s);
    let arc = sk.add_arc(v(35.0, 15.0), v(35.0, 10.0), end, [None, Some(ta), None], None).unwrap();
    let CurveKind::Arc { b: tb, .. } = sk.curves[arc].kind else { panic!() };
    let l2 = sk.add_line(end, end + v(10.0, 10.0), Some(tb), None, None).unwrap();
    sk.add_constraint(ConstraintKind::Radius { c: arc, value: 5.0 }, None).unwrap();
    sk.add_constraint(ConstraintKind::Tangent { a: l1, b: arc }, None).unwrap();
    sk.add_constraint(ConstraintKind::Tangent { a: l2, b: arc }, None).unwrap();
    for p in &mut sk.points {
        p.fixed = true;
    }
    assert!(solve(&mut sk).ok());
    assert!((sweep_deg(&sk, arc) - 45.0).abs() < 1e-6, "sweep {}", sweep_deg(&sk, arc));
    let mirrored: Vec<Vec2> = sk.points.iter().map(|p| v(80.0 - p.pos.x, p.pos.y)).collect();
    let rep = solve_with(&mut sk, &Hold::default(), Some(&mirrored), Analysis::Full, None);
    assert!(rep.ok(), "{rep:?}");
    assert!((sweep_deg(&sk, arc) - 45.0).abs() < 1e-6, "sweep {}", sweep_deg(&sk, arc));
    assert!(lines_leave_the_arc(&sk, arc, [l1, l2]));
}

/// A fillet over three quarters of a turn (the round end of a drop shape) whose corner really
/// mirrored, its arc's ends as they were, stays the long arc its lines leave: judged against the
/// corner before the mirror its sweep jumps, but its lines already leave it, so no swap makes it
/// the short complement with both lines cutting back through it.
#[test]
fn a_mirrored_fillet_over_three_quarters_of_a_turn_stays_the_arc_its_lines_leave() {
    let mut sk = Sketch::new();
    let (fa, fb) = (-FRAC_PI_2, -FRAC_PI_2 + 300f64.to_radians());
    let (pa, pb) = (Vec2::from_angle(fa) * 5.0, Vec2::from_angle(fb) * 5.0);
    let (da, db) = (v(-fa.sin(), fa.cos()), v(-fb.sin(), fb.cos()));
    let l1 = sk.add_line(pa - da * 10.0, pa, None, None, None).unwrap();
    let CurveKind::Line { b: ta, .. } = sk.curves[l1].kind else { panic!() };
    let arc = sk.add_arc(v(0.0, 0.0), pa, pb, [None, Some(ta), None], None).unwrap();
    let CurveKind::Arc { b: tb, .. } = sk.curves[arc].kind else { panic!() };
    let l2 = sk.add_line(pb, pb + db * 10.0, Some(tb), None, None).unwrap();
    sk.add_constraint(ConstraintKind::Radius { c: arc, value: 5.0 }, None).unwrap();
    sk.add_constraint(ConstraintKind::Tangent { a: l1, b: arc }, None).unwrap();
    sk.add_constraint(ConstraintKind::Tangent { a: l2, b: arc }, None).unwrap();
    assert!(lines_leave_the_arc(&sk, arc, [l1, l2]));
    let before: Vec<Vec2> = sk.points.iter().map(|p| p.pos).collect();
    for p in &mut sk.points {
        p.pos = v(-p.pos.x, p.pos.y);
        p.fixed = true;
    }
    let rep = solve_with(&mut sk, &Hold::default(), Some(&before), Analysis::Full, None);
    assert!(rep.ok(), "{rep:?}");
    assert!((sweep_deg(&sk, arc) - 300.0).abs() < 1e-6, "sweep {}", sweep_deg(&sk, arc));
    assert!(lines_leave_the_arc(&sk, arc, [l1, l2]));
}

/// #10521: a circle drawn twice inside a ring is one hole of the ring, not two. The ring between
/// the outer circles keeps the area of the disc less that hole once.
#[test]
fn a_circle_drawn_twice_is_one_hole() {
    for twin_radius in [1.6, 1.6 + 8.9e-11, 1.6 - 8.9e-11] {
        let mut b = Sketch::new();
        let inner = b.add_circle(v(0.0, 0.0), 1.6, None, None).unwrap();
        let twin = b.add_circle(v(0.0, 0.0), twin_radius, None, None).unwrap();
        let middle = b.add_circle(v(0.0, 0.0), 3.25, None, None).unwrap();
        let outer = b.add_circle(v(0.0, 0.0), 4.0, None, None).unwrap();
        let ps = find_profiles(&b);
        let disc = |r: f64| PI * r * r;
        let of = |c: usize| ps.iter().find(|p| p.outer_curves == vec![b.curves[c].id.clone()]).unwrap_or_else(|| panic!("no profile for curve {c}"));
        assert!((of(inner).area - disc(1.6)).abs() < 1e-6);
        assert!((of(twin).area - disc(1.6)).abs() < 1e-6);
        // The ring around the twins has one hole: 33.18 - 8.04.
        let ring = of(middle);
        assert_eq!(ring.hole_curves.len(), 1, "radius {twin_radius}: {:?}", ring.hole_curves);
        assert!((ring.area - (disc(3.25) - disc(1.6))).abs() < 1e-6, "radius {twin_radius}: {}", ring.area);
        assert!((of(outer).area - (disc(4.0) - disc(3.25))).abs() < 1e-6);
    }
}

/// A short piece with no other piece beside it is a gap, not a point (#14520 review): a square
/// whose top-left ends miss by 0.00003 splits its left side where the top line ends, and the
/// 0.00003 piece left above that stays open, as the import's tighter join tolerance asked.
#[test]
fn profiles_keep_a_lone_short_piece_open() {
    let mut sk = Sketch::new();
    let p = |sk: &mut Sketch, x: f64, y: f64| sk.add_point(v(x, y), None).unwrap();
    let bottom_left = p(&mut sk, 0.0, 0.0);
    let bottom_right = p(&mut sk, 10.0, 0.0);
    let right_bottom = p(&mut sk, 10.00005, 0.0);
    let top_right = p(&mut sk, 10.0, 10.0);
    let top_left = p(&mut sk, 0.0, 10.0);
    let left_top = p(&mut sk, 0.0, 10.00003);
    let left_bottom = p(&mut sk, 0.0, 0.00004);
    sk.add_line_pts(bottom_left, bottom_right, None).unwrap();
    sk.add_line_pts(right_bottom, top_right, None).unwrap();
    sk.add_line_pts(top_right, top_left, None).unwrap();
    sk.add_line_pts(left_top, left_bottom, None).unwrap();
    let ps = find_profiles(&sk);
    assert!(ps.is_empty(), "{ps:?}");
}

/// Scaling a fit spline's points and tangent handles scales its curve alike: a handle is measured
/// against the chords beside its point, not an absolute length (#6502).
#[test]
fn scaling_a_spline_with_handles_scales_its_curve() {
    let p = [Vec2::new(0.0, 0.0), Vec2::new(8.0, 6.0), Vec2::new(17.0, 2.0), Vec2::new(25.0, 10.0)];
    let mut handles = crate::curves::fit_tangent_handles(&p, false).unwrap();
    // A turned handle, so the curve is not the natural one.
    handles[1] += Vec2::new(1.5, -2.0);
    let before = crate::curves::fit_beziers_with_handles(&p, &handles, false).unwrap();
    for f in [0.1, 3.0] {
        let scaled: Vec<Vec2> = p.iter().map(|q| *q * f).collect();
        let scaled_handles: Vec<Vec2> = handles.iter().map(|q| *q * f).collect();
        let after = crate::curves::fit_beziers_with_handles(&scaled, &scaled_handles, false).unwrap();
        for (a, b) in after.iter().zip(&before) {
            for (x, y) in a.iter().zip(b) {
                assert!(x.dist(*y * f) < 1e-9 * f.max(1.0), "scale {f}: {x:?} vs {:?}", *y * f);
            }
        }
    }
}

/// Deleting a point a spline with tangent handles needs removes the spline, its tangent lines
/// and its handle points: the sketch stays valid (#6502).
#[test]
fn deleting_a_fit_point_takes_the_splines_tangent_lines() {
    let mut sk = Sketch::new();
    let pts: Vec<usize> = [v(0.0, 0.0), v(8.0, 6.0), v(17.0, 2.0), v(25.0, 10.0)].iter().map(|p| sk.add_point(*p, None).unwrap()).collect();
    let fit = pts[1];
    let spline = sk.add_curve(CurveKind::Spline { pts, control: false, degree: 3, periodic: false, nurbs: None, handles: Vec::new() }, None).unwrap();
    sk.add_spline_handles(spline).unwrap();
    let before = sk.points.len();
    sk.remove_points(&[fit]);
    assert!(sk.curves.iter().all(|c| c.spline_handle_of.is_none()), "no tangent line is left without its spline");
    assert!(!sk.curves.iter().any(|c| matches!(c.kind, CurveKind::Spline { .. })));
    assert_eq!(sk.points.len(), before - 1 - 4, "the point and the four handle points go");
    sk.validate_spline_handles().unwrap();
}

/// #6502: a fit spline without stored handles previews them where storing them puts them; a
/// spline with handles, or a control point spline, previews none.
#[test]
fn spline_handle_preview_matches_the_stored_handles() {
    let mut sk = Sketch::new();
    let fit = [Vec2::new(-40.0, 0.0), Vec2::new(-15.0, 20.0), Vec2::new(15.0, -10.0), Vec2::new(40.0, 10.0)];
    let pts: Vec<usize> = fit.iter().map(|p| sk.add_point(*p, None).unwrap()).collect();
    let spline = sk.add_curve(CurveKind::Spline { pts, control: false, degree: 3, periodic: false, nurbs: None, handles: Vec::new() }, None).unwrap();
    let preview = sk.spline_handle_preview(spline).expect("a fit spline previews its handles");
    assert_eq!(preview.len(), fit.len());
    sk.add_spline_handles(spline).unwrap();
    let CurveKind::Spline { handles, .. } = &sk.curves[spline].kind else { panic!("a fit spline") };
    for (shown, stored) in preview.iter().zip(handles) {
        assert!(shown.dist(sk.point(*stored).unwrap()) < 1e-12, "{shown:?}");
    }
    assert!(sk.spline_handle_preview(spline).is_none(), "stored handles are not previewed");
    let control_pts: Vec<usize> = fit.iter().map(|p| sk.add_point(*p + Vec2::new(0.0, 50.0), None).unwrap()).collect();
    let control = sk
        .add_curve(CurveKind::Spline { pts: control_pts, control: true, degree: 3, periodic: false, nurbs: None, handles: Vec::new() }, None)
        .unwrap();
    assert!(sk.spline_handle_preview(control).is_none(), "a control point spline has no tangent handles");
}

/// A band of rounded caps over the top of a ring (two arcs of radius 15 tangent to the ring's
/// sides, joined by a line). On the left the cap's arc and the ring's corner arc leave the ring's
/// side along one tangent, apart only by the solver's round-off; on the right the cap's arc ends
/// a hair short of the top line's end. Both corners must still close the band.
#[test]
fn a_cap_whose_arcs_leave_a_ring_along_its_sides_closes_a_profile() {
    for lift in [1e-6, -1e-6] {
        let mut sk = Sketch::new();
        let p = |sk: &mut Sketch, x: f64, y: f64| sk.add_point(v(x, y), None).unwrap();
        // The ring: sides from y = -100 to 0, corners of radius 6, top at y = 6.
        let (bl, left, left_top) = (p(&mut sk, 0.0, -100.0), p(&mut sk, 0.0, 0.0), p(&mut sk, 6.0, 6.0));
        let (right_top, right, br) = (p(&mut sk, 94.0, 6.0), p(&mut sk, 100.0, 0.0), p(&mut sk, 100.0, -100.0));
        sk.add_line_pts(bl, left, None).unwrap();
        sk.add_arc(v(6.0, 0.0), v(6.0, 6.0), v(0.0, 0.0), [None, Some(left_top), Some(left)], None).unwrap();
        sk.add_line_pts(left_top, right_top, None).unwrap();
        sk.add_arc(v(94.0, 0.0), v(100.0, 0.0), v(94.0, 6.0), [None, Some(right), Some(right_top)], None).unwrap();
        sk.add_line_pts(right, br, None).unwrap();
        sk.add_line_pts(br, bl, None).unwrap();
        // The cap: its left arc's centre a round-off off the ring's side, its right arc ending
        // 2e-5 along the top line from the line's end.
        let (cap_left, cap_right) = (p(&mut sk, 15.0, 15.0), p(&mut sk, 85.0, 15.0));
        sk.add_arc(v(15.0, lift), v(15.0, 15.0), v(0.0, 0.0), [None, Some(cap_left), Some(left)], None).unwrap();
        sk.add_line_pts(cap_right, cap_left, None).unwrap();
        let short = v(85.0 + 2e-5, (225.0 - 4e-10_f64).sqrt());
        sk.add_arc(v(85.0, 0.0), v(100.0, 0.0), short, [None, Some(right), None], None).unwrap();
        let ps = find_profiles(&sk);
        let band = ps.iter().find(|p| p.outer_curves.contains(&sk.curves[6].id));
        let Some(band) = band else { panic!("lift {lift}: no profile through the cap: {ps:?}") };
        // The cap less the ring's rounded top: 70 x 15 + two quarter discs of 15 - (88 x 6 + two of 6).
        let want = 70.0 * 15.0 + PI * 225.0 / 2.0 - (88.0 * 6.0 + PI * 36.0 / 2.0);
        assert!((band.area - want).abs() < 1e-3, "lift {lift}: {} vs {want}", band.area);
        for p in &ps {
            assert!(p.region.outer.is_closed(1e-6), "lift {lift}: {:?}", p.outer_curves);
        }
    }
}

/// A ring with rounded top corners (radius 6) under two caps over one top line: a wide cap of
/// radius 15 and a narrow one of radius 9 resting on the ring's top line. At the top line's left
/// end both caps' arcs leave along the line, as a solved sketch leaves them: the wide arc's
/// tangent `tilt` rad down from the line, the narrow arc starting 2e-5 mm past the line's end,
/// as near the line's end as the wide arc's start, so it is neither's unique nearest end.
/// The region under the narrow cap must close, and so must the slivers between the caps.
#[test]
fn two_caps_leaving_one_line_along_its_direction_each_close_a_profile() {
    for tilt in [0.0, 1e-7, 5e-6] {
        let mut sk = Sketch::new();
        let p = |sk: &mut Sketch, x: f64, y: f64| sk.add_point(v(x, y), None).unwrap();
        // The ring: sides from y = -50 to 0, corners of radius 6, top line at y = 6.
        let (bl, left, left_top) = (p(&mut sk, 0.0, -50.0), p(&mut sk, 0.0, 0.0), p(&mut sk, 6.0, 6.0));
        let (right_top, right, br) = (p(&mut sk, 94.0, 6.0), p(&mut sk, 100.0, 0.0), p(&mut sk, 100.0, -50.0));
        sk.add_line_pts(bl, left, None).unwrap();
        sk.add_arc(v(6.0, 0.0), v(6.0, 6.0), v(0.0, 0.0), [None, Some(left_top), Some(left)], None).unwrap();
        let ring_top = sk.add_line_pts(left_top, right_top, None).unwrap();
        sk.add_arc(v(94.0, 0.0), v(100.0, 0.0), v(94.0, 6.0), [None, Some(right), Some(right_top)], None).unwrap();
        sk.add_line_pts(right, br, None).unwrap();
        sk.add_line_pts(br, bl, None).unwrap();
        // The caps' top line at y = 15. On the right it runs 1.8e-4 mm past the narrow arc's
        // end, which lies on it, and the wide arc ends 1e-5 mm short of the line's end.
        let (top_left, top_right) = (p(&mut sk, 15.0, 15.0), p(&mut sk, 85.0 + 1.8e-4, 15.0));
        sk.add_line_pts(top_right, top_left, None).unwrap();
        let wide_right_end = v(85.0 + 1.7e-4, (225.0 - 1.7e-4_f64 * 1.7e-4).sqrt());
        let wide_right = p(&mut sk, wide_right_end.x, wide_right_end.y);
        sk.add_arc(v(85.0, 0.0), v(100.0, 0.0), wide_right_end, [None, Some(right), Some(wide_right)], None).unwrap();
        let narrow_right = p(&mut sk, 85.0, 15.0);
        sk.add_arc(v(85.0, 6.0), v(94.0, 6.0), v(85.0, 15.0), [None, Some(right_top), Some(narrow_right)], None).unwrap();
        // The wide arc on the left: its centre `d` right of and below the round one, so that it
        // still passes through the line's end and the ring's side top, with its tangent at the
        // line's end `tilt` down from the line. It starts at a point of its own on the line's end
        // (held there by a coincidence), as drawn in the app.
        let d = tilt * 15.0;
        let wide_start = p(&mut sk, 15.0, 15.0);
        sk.add_arc(v(15.0 + d, -d), v(15.0, 15.0), v(0.0, 0.0), [None, Some(wide_start), Some(left)], None).unwrap();
        // The narrow arc on the left starts 2e-5 mm past the line's end, on its line.
        let (narrow_start, narrow_end) = (p(&mut sk, 15.0 - 2e-5, 15.0), p(&mut sk, 6.0 - 2e-5, 6.0));
        let narrow =
            sk.add_arc(v(15.0 - 2e-5, 6.0), v(15.0 - 2e-5, 15.0), v(6.0 - 2e-5, 6.0), [None, Some(narrow_start), Some(narrow_end)], None).unwrap();
        let ps = find_profiles(&sk);
        let (ring_top, narrow) = (&sk.curves[ring_top].id, &sk.curves[narrow].id);
        let under = ps.iter().find(|p| p.outer_curves.contains(ring_top) && p.outer_curves.contains(narrow));
        let all = || ps.iter().map(|p| (&p.outer_curves, p.area)).collect::<Vec<_>>();
        let Some(under) = under else { panic!("tilt {tilt}: no profile under the narrow cap: {:?}", all()) };
        // 70 x 9 and two quarter discs of radius 9 (the line past the narrow arc adds nothing).
        let want = 70.0 * 9.0 + PI * 81.0 / 2.0;
        assert!((under.area - want).abs() < 1e-3, "tilt {tilt}: {} vs {want}", under.area);
        // The ring, the region under the narrow cap and the two slivers between the caps.
        assert_eq!(ps.len(), 4, "tilt {tilt}: {:?}", all());
        for p in &ps {
            assert!(p.region.outer.is_closed(1e-6), "tilt {tilt}: {:?}", p.outer_curves);
        }
    }
}
