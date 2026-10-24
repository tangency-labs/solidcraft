//! The polylines that close an open boundary loop along a surface's domain are bounded (#1985).
//!
//! A loop laid out across a huge parameter range (#1985: a cylinder's angles millions of turns
//! apart) made its closing polyline millions of points long, and meshing them never finished:
//! the STEP export that reads its own file back hung. Such a face is now left unmeshed, which
//! `Body::validity` reports, instead. The division of the closing polyline is bounded too: the
//! work stops at the bound, however many turns the loop spans (#1374).

use truck_meshalgo::tessellation::boundary_test_hooks::boundary_points;
use truck_modeling::{self as mt, ParametricSurface, Point2, Rad, builder};

/// Turns of the runaway loop: enough for its closing polyline to pass the bound however the
/// division samples a curve that winds round so often. Before the division was bounded, 200,000
/// turns took 31 s (`ci` profile) and over a gigabyte.
const TURNS: f64 = 2e5;

/// Half a cylinder of radius 1 about Z: its angle is periodic.
fn cylinder() -> mt::Surface {
    let v = builder::vertex(mt::Point3::new(1.0, 0.0, 0.0));
    let e = builder::tsweep(&v, mt::Vector3::unit_z());
    let shell = builder::rsweep(&e, mt::Point3::new(0.0, 0.0, 0.0), mt::Vector3::unit_z(), Rad(std::f64::consts::PI));
    shell.face_iter().next().unwrap().surface()
}

/// A loop across `turns` periods of the surface's angle: open, so it is closed along the domain.
fn open_loop(period: f64, turns: f64) -> Vec<Point2> {
    vec![Point2::new(0.2, 0.0), Point2::new(0.8, 0.0), Point2::new(0.8, turns * period), Point2::new(0.2, turns * period)]
}

#[test]
fn a_loop_once_round_closes_along_the_domain() {
    let s = cylinder();
    let period = s.v_period().expect("the angle is periodic");
    let n = boundary_points(&s, open_loop(period, 1.0), 1e-3).expect("a sound open loop is closed and meshed");
    assert!(n < 1000, "{n} points for a loop once round");
}

#[test]
fn a_loop_across_many_turns_leaves_the_face_unmeshed() {
    let s = cylinder();
    let period = s.v_period().expect("the angle is periodic");
    let start = std::time::Instant::now();
    assert_eq!(boundary_points(&s, open_loop(period, TURNS), 1e-3), None);
    // About 20 ms measured; the unbounded division took 31 s. The margin is for a busy machine.
    let took = start.elapsed();
    assert!(took < std::time::Duration::from_secs(2), "the bound did not limit the work: {took:?}");
}
