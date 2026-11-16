//! WR-647 addendum (the fork's tangency-labs/smlib@04b42b4, case 3e of
//! `SmCurve::SimpleCoincidenceChecker`): two curves within tolerance whose separation keeps
//! one side without vanishing do not coincide, they come nearest at one point. Two concentric
//! 90 degree arcs, r 5 and r 5 + `gap`, checked against each other at tolerance 1e-5: upstream
//! returns one coincidence range over the whole arcs; the port returns one point, the arcs
//! `gap` apart there. The same arc twice (separation at numeric zero) stays a range.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::curve::curve::Curve;
use solidcore::geom::solution_array::{Solution, SolutionType};
use solidcore::math::Point3;
use solidcore::primitive::api_curves::create_arc;
use solidcore::Kernel;

const TOL: f64 = 1e-5;

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

/// 04b42b4's case 3e: one point, where the arcs are `gap` apart.
#[test]
fn wr_647_concentric_arcs_within_tolerance_are_their_nearest_point() {
    let k = Kernel::default();
    for gap in [1e-6, 5e-6, 1e-9] {
        let (a, b, sols) = check(gap);
        eprintln!("WR647 3e gap {gap}: {sols:?}");
        assert_eq!(sols.len(), 1, "gap {gap}: {sols:?}");
        let s = &sols[0];
        assert_eq!(s.solution_type, SolutionType::SingleValue, "gap {gap}");
        let p = a.evaluate_point(&k, s.start.parameters[0]).unwrap();
        let q = b.evaluate_point(&k, s.start.parameters[1]).unwrap();
        assert!(
            (p.distance(q) - gap).abs() <= 1e-12,
            "gap {gap}: the points are {} apart",
            p.distance(q)
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
/// its coincident copy in a body cut by itself): the curves lie on each other, yet the end
/// pairs' separations run along the curves, forward at one end and back at the other.
/// An earlier version of the WR-647 fix read that as a crossing: one point (where
/// the curves cannot cross, they lie on each other), or a failure where no crossing is
/// found. The side is taken across the curve, nothing changes sides, and the answer is the
/// coincidence range over the shorter arc.
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

/// Two 90 degree arcs of radius 5 whose centres are `d` = 5e-6 apart along the arcs'
/// bisector: the separation is `d cos(theta - 45 deg)` to first order, `d` in the middle and
/// `d / sqrt 2` = 3.54e-6 at the ends, on one side throughout and never zero. The curves come
/// nearest at the ends, not where the separation is stationary in the middle (its maximum).
/// The answer is one point no farther apart than the nearer end, here an end itself (within
/// second-order term, `(d / sqrt 2)^2 / (2 r)` = 1.25e-12, bounded by 1e-11).
#[test]
fn wr_647_separation_that_bulges_comes_nearest_at_an_end() {
    let k = Kernel::default();
    let d = 5e-6;
    let shift = d / 2f64.sqrt();
    let a = arc(5.0);
    let b = Curve::from(create_arc(Point3::new(-shift, -shift, 0.0), 5.0, 0.0, 90.0).unwrap());
    let (ia, ib) = (a.natural_interval(), b.natural_interval());
    let sols = a
        .simple_coincidence_checker(&k, &ia, &b, &ib, TOL, None, None, [Some(0u8), Some(1u8)])
        .unwrap();
    assert_eq!(sols.len(), 1, "{sols:?}");
    let s = &sols[0];
    assert_eq!(s.solution_type, SolutionType::SingleValue, "{s:?}");
    let p = a.evaluate_point(&k, s.start.parameters[0]).unwrap();
    let q = b.evaluate_point(&k, s.start.parameters[1]).unwrap();
    // the separation at the arcs' ends, from the closed form to first order in d / r
    let at_ends = shift;
    assert!(
        p.distance(q) <= at_ends + 1e-11,
        "the point is {} apart, the ends {at_ends}",
        p.distance(q)
    );
}
