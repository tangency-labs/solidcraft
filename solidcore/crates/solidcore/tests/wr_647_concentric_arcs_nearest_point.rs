//! WR-647 addendum (the fork's tangency-labs/smlib@04b42b4, case 3e of
//! `SmCurve::SimpleCoincidenceChecker`): two curves within tolerance whose separation keeps
//! one side without vanishing do not coincide, they come nearest at one point. Two concentric
//! 90 degree arcs, r 5 and r 5 + `gap`, checked against each other at tolerance 1e-5: upstream
//! (and the port by default) returns one coincidence range over the whole arcs. With
//! `Kernel::fixes.simple_coincidence_crossing_is_point` it is one point, the arcs `gap`
//! apart there. The same arc twice (separation at numeric zero) stays a range either way.
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

fn kernel(fix: bool) -> Kernel {
    let mut k = Kernel::default();
    k.fixes.simple_coincidence_crossing_is_point = fix;
    k
}

fn arc(r: f64) -> Curve {
    Curve::from(create_arc(Point3::new(0.0, 0.0, 0.0), r, 0.0, 90.0).unwrap())
}

/// `SimpleCoincidenceChecker` of the arc r 5 against the arc r 5 + `gap`, over their natural
/// intervals.
fn check(fix: bool, gap: f64) -> (Curve, Curve, Vec<Solution<u8>>) {
    let k = kernel(fix);
    let a = arc(5.0);
    let b = arc(5.0 + gap);
    let (ia, ib) = (a.natural_interval(), b.natural_interval());
    let sols = a
        .simple_coincidence_checker(&k, &ia, &b, &ib, TOL, None, None, [Some(0u8), Some(1u8)])
        .unwrap();
    (a, b, sols)
}

#[test]
fn wr_647_crossing_is_point_is_off_by_default() {
    assert!(!Kernel::default().fixes.simple_coincidence_crossing_is_point);
}

/// Upstream: one coincidence range over the whole arcs.
#[test]
fn wr_647_concentric_arcs_within_tolerance_are_a_range_by_default() {
    for gap in [1e-6, 5e-6, 1e-9] {
        let (a, b, sols) = check(false, gap);
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
    }
}

/// With the fix (04b42b4's case 3e): one point, where the arcs are `gap` apart.
#[test]
fn wr_647_concentric_arcs_within_tolerance_are_their_nearest_point() {
    let k = Kernel::default();
    for gap in [1e-6, 5e-6, 1e-9] {
        let (a, b, sols) = check(true, gap);
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

/// The same arc twice: the separation vanishes, so it stays a range in both settings.
#[test]
fn wr_647_the_same_arc_twice_stays_a_range() {
    for fix in [false, true] {
        let (_, _, sols) = check(fix, 0.0);
        assert_eq!(sols.len(), 1, "fix {fix}: {sols:?}");
        assert_eq!(
            sols[0].solution_type,
            SolutionType::RangeOfValues,
            "fix {fix}"
        );
    }
}
