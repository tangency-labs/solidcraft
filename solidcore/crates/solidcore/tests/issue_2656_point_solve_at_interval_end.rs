//! #2656: the closest point of a curve's interval to a point whose foot lies just past the
//! interval's end, where the interval ends a rounding off a knot, is the end itself. A round's
//! guess on its other rail is such a point (the rail's end at a corner): with no solution there,
//! the whole corner failed (status 1001). The rail is a chain of straight cubic Bezier spans
//! (triple interior knots): the span an edge lies on, 26 mm long, and a span 4.8e-4 mm long on
//! each side of it, that corners left. The edge's interval is the span's knots, each a few 1e-14
//! outward.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers
use solidcore::curve::curve::p1::GlobalPointSolveOptions;
use solidcore::curve::curve::Curve;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::global_solver::{SolutionRequested, SolverOperation};
use solidcore::geom::vector3d::Vec3;
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::crv_basic::Curve as NurbCurve;
use solidcore::nurbs::math::{CPoint, NOW};
use solidcore::surface::surface::p4::NoObject;

const Z: f64 = 95.5;
/// The straight span's ends.
const A: [f64; 2] = [-126.37084495630205, -206.4726549577224];
const B: [f64; 2] = [-100.62831282965271, -202.4273999065569];
/// The tiny spans' far ends: past B and before A, on the same line.
const TAIL: [f64; 2] = [-100.62783340847137, -202.42732457050084];
const HEAD: [f64; 2] = [A[0] - (TAIL[0] - B[0]), A[1] - (TAIL[1] - B[1])];
/// The knots between the spans: the straight span is `KNOTS[1]..KNOTS[2]`.
const KNOTS: [f64; 4] = [
    0.5750590991300976,
    0.5750594477558282,
    0.5937789233610331,
    0.593779271986747,
];

fn rail() -> Curve {
    let lerp =
        |p: [f64; 2], q: [f64; 2], t: f64| [p[0] + (q[0] - p[0]) * t, p[1] + (q[1] - p[1]) * t];
    let mut pts = Vec::new();
    for (p, q) in [(HEAD, A), (A, B), (B, TAIL)] {
        let from = usize::from(!pts.is_empty());
        pts.extend((from..4).map(|i| lerp(p, q, f64::from(i as u8) / 3.0)));
    }
    let pw = pts
        .iter()
        .map(|p| CPoint::new(p[0], p[1], Z, NOW))
        .collect();
    let mut u = vec![KNOTS[0]; 4];
    for k in &KNOTS[1..3] {
        u.extend([*k; 3]);
    }
    u.extend([KNOTS[3]; 4]);
    Curve::from(BSplineCurve::from_nurb(
        NurbCurve::new(pw, 3, u).unwrap(),
        3,
        false,
    ))
}

/// The solutions' parameters for the closest point to `p` of the interval `lo..hi`.
fn closest(p: Vec3, lo: f64, hi: f64) -> Vec<f64> {
    let k = Kernel::default();
    let sols = rail()
        .global_point_solve::<NoObject>(
            &k,
            &Extent1d::from_min_max(lo, hi),
            SolverOperation::Minimize,
            p,
            8e-5,
            SolutionRequested::Single,
            &GlobalPointSolveOptions::default(),
        )
        .unwrap();
    sols.as_slice()
        .iter()
        .map(|s| s.start.param(0).unwrap())
        .collect()
}

/// The point 1 mm from the line of the span, `s` along it from A: 1 mm to its side, 1 mm below.
fn beside(s: f64) -> Vec3 {
    let (dx, dy) = (B[0] - A[0], B[1] - A[1]);
    let len = dx.hypot(dy);
    Vec3::new(
        A[0] + (dx * s - dy) / len,
        A[1] + (dy * s + dx) / len,
        Z - 1.0,
    )
}

fn len() -> f64 {
    (B[0] - A[0]).hypot(B[1] - A[1])
}

#[test]
fn a_foot_past_an_end_a_rounding_beyond_its_knot_solves_to_the_end() {
    for past in [1e-9, 1e-6, 1e-4] {
        let got = closest(beside(len() + past), KNOTS[1] + 2e-14, KNOTS[2] + 2e-14);
        assert_eq!(
            got.len(),
            1,
            "no solution for a foot {past} past the interval's end"
        );
        assert!((got[0] - KNOTS[2]).abs() < 1e-9, "{got:?}");
    }
}

#[test]
fn a_foot_before_a_start_a_rounding_before_its_knot_solves_to_the_start() {
    for before in [1e-9, 1e-6, 1e-4] {
        let got = closest(beside(-before), KNOTS[1] - 2e-14, KNOTS[2] - 2e-14);
        assert_eq!(
            got.len(),
            1,
            "no solution for a foot {before} before the interval's start"
        );
        assert!((got[0] - KNOTS[1]).abs() < 1e-9, "{got:?}");
    }
}

#[test]
fn the_same_feet_solve_to_the_end_when_the_interval_ends_on_the_knot() {
    let got = closest(beside(len() + 1e-4), KNOTS[1], KNOTS[2]);
    assert_eq!(got.len(), 1);
    assert!((got[0] - KNOTS[2]).abs() < 1e-12, "{got:?}");
}

#[test]
fn a_foot_inside_the_interval_is_unchanged() {
    let got = closest(beside(len() / 2.0), KNOTS[1] + 2e-14, KNOTS[2] + 2e-14);
    assert_eq!(got.len(), 1);
    assert!(
        (got[0] - (KNOTS[1] + KNOTS[2]) / 2.0).abs() < 1e-9,
        "{got:?}"
    );
}

#[test]
fn a_foot_in_the_next_span_beyond_the_interval_solves_there_when_the_interval_reaches_it() {
    let got = closest(beside(len() + 1e-4), KNOTS[1], KNOTS[3]);
    assert_eq!(got.len(), 1);
    assert!(got[0] > KNOTS[2] && got[0] < KNOTS[3], "{got:?}");
}
