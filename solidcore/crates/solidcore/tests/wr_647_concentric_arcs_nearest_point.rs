//! WR-647: which contacts of two curves within tolerance are a coincidence range.
//!
//! Two concentric 90 degree arcs, r 5 and r 5 + `gap`, at tolerance 1e-5, are `gap` apart at
//! every angle (closed form: the points at one angle lie on one ray from the centre). Within
//! the tolerance they are the same curve: `SimpleCoincidenceChecker` and
//! `GlobalCurveIntersect` return one coincidence range over the whole arcs, as upstream.
//! (The fork's 04b42b4, "case 3e", took such a range for the one point where the curves
//! come nearest; with a constant separation every point is nearest, and an edge against a
//! fitted copy of itself, whose separation is never zero, fell apart into points: the
//! coincident skins of tangency-labs/solidcore#1036.) A crossing of two distinct curves
//! seen through the tolerance is decided on the merged range in `GlobalCurveIntersect`
//! (`issue_1979_ring_lip_crossing_not_coincidence.rs`, and the crossing test below).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::curve::curve::p8::CurveIntersectOptions;
use solidcore::curve::curve::Curve;
use solidcore::geom::solution_array::{ObjectKind, Solution, SolutionObject, SolutionType};
use solidcore::math::Point3;
use solidcore::primitive::api_curves::create_arc;
use solidcore::Kernel;

const TOL: f64 = 1e-5;

/// A curve handle for the solutions of `GlobalCurveIntersect` (none are named here).
#[derive(Debug, Clone, Copy, PartialEq)]
struct H;

impl SolutionObject for H {
    fn kind(&self) -> ObjectKind {
        ObjectKind::Curve
    }
}

fn arc(r: f64) -> Curve {
    Curve::from(create_arc(Point3::new(0.0, 0.0, 0.0), r, 0.0, 90.0).unwrap())
}

/// `SimpleCoincidenceChecker` of the arc r 5 against the arc r 5 + `gap`, over their natural
/// intervals.
fn check(gap: f64) -> (Curve, Curve, Vec<Solution<u8>>) {
    let k = Kernel::default();
    let a = arc(5.0);
    let b = arc(5.0 + gap);
    let (ia, ib) = (a.natural_interval(), b.natural_interval());
    let sols = a
        .simple_coincidence_checker(&k, &ia, &b, &ib, TOL, None, None, [Some(0u8), Some(1u8)])
        .unwrap();
    (a, b, sols)
}

/// One coincidence range over the whole arcs, and the arcs are `gap` apart along it.
#[test]
fn wr_647_concentric_arcs_within_tolerance_are_a_range() {
    let k = Kernel::default();
    for gap in [1e-6, 5e-6, 1e-9] {
        let (a, b, sols) = check(gap);
        assert_eq!(sols.len(), 1, "gap {gap}: {sols:?}");
        let s = &sols[0];
        assert_eq!(s.solution_type, SolutionType::RangeOfValues, "gap {gap}");
        let (ia, ib) = (a.natural_interval(), b.natural_interval());
        assert!(
            (s.start.parameters[0] - ia.min()).abs() < 1e-9,
            "gap {gap}: {s:?}"
        );
        assert!(
            (s.end.parameters[0] - ia.max()).abs() < 1e-9,
            "gap {gap}: {s:?}"
        );
        assert!(
            (s.start.parameters[1] - ib.min()).abs() < 1e-9,
            "gap {gap}: {s:?}"
        );
        assert!(
            (s.end.parameters[1] - ib.max()).abs() < 1e-9,
            "gap {gap}: {s:?}"
        );
        // the range's ends pair the arcs' ends, `gap` apart (closed form)
        for (t, u) in [(ia.min(), ib.min()), (ia.max(), ib.max())] {
            let d = a
                .evaluate_point(&k, t)
                .unwrap()
                .distance(b.evaluate_point(&k, u).unwrap());
            assert!((d - gap).abs() <= 1e-12, "gap {gap}: ends {d} apart");
        }
    }
}

/// The merged answer of `GlobalCurveIntersect` is the same range: it ends where the arcs end,
/// so it is no crossing seen through the tolerance.
#[test]
fn wr_647_concentric_arcs_intersect_in_one_range() {
    let k = Kernel::default();
    for gap in [1e-6, 5e-6] {
        let a = arc(5.0);
        let b = arc(5.0 + gap);
        let (ia, ib) = (a.natural_interval(), b.natural_interval());
        let sols = a
            .global_curve_intersect(
                &k,
                &ia,
                &b,
                &ib,
                TOL,
                &CurveIntersectOptions::<H>::default(),
            )
            .unwrap();
        let all = sols.as_slice();
        assert_eq!(all.len(), 1, "gap {gap}: {all:?}");
        assert_eq!(
            all[0].solution_type,
            SolutionType::RangeOfValues,
            "gap {gap}: {all:?}"
        );
    }
}

/// The same arc twice: the separation vanishes, so it stays a range.
#[test]
fn wr_647_the_same_arc_twice_stays_a_range() {
    let (_, _, sols) = check(0.0);
    assert_eq!(sols.len(), 1, "{sols:?}");
    assert_eq!(sols[0].solution_type, SolutionType::RangeOfValues);
}

/// An arc against a copy of itself that stops 4.8e-6 short of each end (an edge against
/// its coincident copy in a body cut by itself): the curves lie on each other, and the answer
/// is the coincidence range over the shorter arc.
#[test]
fn an_arc_and_its_shorter_copy_coincide_over_the_copy() {
    let shortfall_deg = (4.8e-6 / 5.0_f64).to_degrees();
    let k = Kernel::default();
    let a = arc(5.0);
    let b = Curve::from(
        create_arc(
            Point3::new(0.0, 0.0, 0.0),
            5.0,
            shortfall_deg,
            90.0 - shortfall_deg,
        )
        .unwrap(),
    );
    let (ia, ib) = (a.natural_interval(), b.natural_interval());
    let sols = a
        .simple_coincidence_checker(&k, &ia, &b, &ib, TOL, None, None, [Some(0u8), Some(1u8)])
        .unwrap();
    assert_eq!(sols.len(), 1, "{sols:?}");
    let s = &sols[0];
    assert_eq!(s.solution_type, SolutionType::RangeOfValues, "{s:?}");
    // the range covers the copy, end to end
    assert!((s.start.parameters[1] - ib.min()).abs() < 1e-9, "{s:?}");
    assert!((s.end.parameters[1] - ib.max()).abs() < 1e-9, "{s:?}");
    for (t, u) in [
        (s.start.parameters[0], ib.min()),
        (s.end.parameters[0], ib.max()),
    ] {
        let gap = a
            .evaluate_point(&k, t)
            .unwrap()
            .distance(b.evaluate_point(&k, u).unwrap());
        // the range's ends pair points within the tolerance (the shortfall, 4.8e-6)
        assert!(
            gap <= 4.8e-6 + 1e-12,
            "the range's ends lie on each other: {gap}"
        );
    }
}

/// Two equal arcs (r 5, 0..180 degrees) whose centres are `e` apart along x cross once, at
/// x = e / 2, y = sqrt(25 - e^2 / 4) (closed form), at an angle of about e / 5 radians, so
/// they stay within the tolerance over a stretch about the crossing whose ends lie inside
/// both arcs. `GlobalCurveIntersect` gives that crossing as one point, not a range.
#[test]
fn wr_647_equal_arcs_a_little_apart_cross_at_one_point() {
    let k = Kernel::default();
    for e in [1e-4, 1e-3] {
        let a = Curve::from(create_arc(Point3::new(0.0, 0.0, 0.0), 5.0, 0.0, 180.0).unwrap());
        let b = Curve::from(create_arc(Point3::new(e, 0.0, 0.0), 5.0, 0.0, 180.0).unwrap());
        let (ia, ib) = (a.natural_interval(), b.natural_interval());
        let sols = a
            .global_curve_intersect(
                &k,
                &ia,
                &b,
                &ib,
                TOL,
                &CurveIntersectOptions::<H>::default(),
            )
            .unwrap();
        let all = sols.as_slice();
        assert_eq!(all.len(), 1, "e {e}: {all:?}");
        let s = &all[0];
        assert_eq!(s.solution_type, SolutionType::SingleValue, "e {e}: {s:?}");
        let want = Point3::new(e / 2.0, (25.0 - e * e / 4.0).sqrt(), 0.0);
        let p = a.evaluate_point(&k, s.start.parameters[0]).unwrap();
        let q = b.evaluate_point(&k, s.start.parameters[1]).unwrap();
        assert!(
            p.distance(want) <= TOL && q.distance(want) <= TOL,
            "e {e}: {p:?} {q:?}"
        );
    }
}
