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
//! or touch seen through the tolerance is decided on the merged range in
//! `GlobalCurveIntersect` (`issue_1979_ring_lip_crossing_not_coincidence.rs`, and the
//! crossing and touch tests below). The flag that kept 04b42b4's case 3e,
//! `simple_coincidence_crossing_is_point`, is retired (WR-647 part D): a face/face contact
//! at an edge's end vertex is made one point by the tangency witness, from the surfaces.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::curve::curve::p8::CurveIntersectOptions;
use solidcore::curve::curve::Curve;
use solidcore::curve::ellipse::{Ellipse, EllipseOptions};
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::solution_array::{ObjectKind, Solution, SolutionObject, SolutionType};
use solidcore::math::{Point3, Vec3};
use solidcore::nurbs::b_spline_curve::p1::{
    BSplineCurve, BSplineCurveForm, CanonicalData, KnotType,
};
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

fn kernel() -> Kernel {
    Kernel::default()
}

fn arc(r: f64) -> Curve {
    Curve::from(create_arc(Point3::new(0.0, 0.0, 0.0), r, 0.0, 90.0).unwrap())
}

/// `SimpleCoincidenceChecker` of the arc r 5 against the arc r 5 + `gap`, over their natural
/// intervals.
fn check(gap: f64) -> (Curve, Curve, Vec<Solution<u8>>) {
    let k = kernel();
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

/// The cubic Bezier from (-1, 0) to (1, 0) with inner control points (-1/3, 0.2) and
/// (1/3, -0.2), rotated by `angle` about the origin and moved by `shift` (y, at the curve's
/// start; minus at its end, linear in between): a B-spline, so `GlobalCurveIntersect`
/// takes its general solver, not the analytic one.
fn cubic(angle: f64, shift: f64) -> Curve {
    let (c, s) = (angle.cos(), angle.sin());
    let raw = [
        (-1.0, 0.0),
        (-1.0 / 3.0, 0.2),
        (1.0 / 3.0, -0.2),
        (1.0, 0.0),
    ];
    let control: Vec<Point3> = raw
        .iter()
        .enumerate()
        .map(|(i, &(x, y))| {
            // a Bezier reproduces a linear function from equally spaced control values
            let dy = shift * (1.0 - 2.0 * i as f64 / 3.0);
            Point3::new(c * x - s * y, s * x + c * y + dy, 0.0)
        })
        .collect();
    Curve::from(
        BSplineCurve::create_canonical(&CanonicalData {
            dim: 3,
            degree: 3,
            control_points: &control,
            form: BSplineCurveForm::default(),
            knot_multiplicities: &[4, 4],
            knots: &[0.0, 1.0],
            knot_type: KnotType::default(),
            weights: None,
            trim: None,
        })
        .unwrap(),
    )
}

fn intersect(a: &Curve, b: &Curve) -> Vec<Solution<H>> {
    intersect_at(a, b, TOL)
}

fn intersect_at(a: &Curve, b: &Curve, tol: f64) -> Vec<Solution<H>> {
    let k = Kernel::default();
    let (ia, ib) = (a.natural_interval(), b.natural_interval());
    a.global_curve_intersect(&k, &ia, b, &ib, tol, &CurveIntersectOptions::<H>::default())
        .unwrap()
        .as_slice()
        .to_vec()
}

/// The cubic and its copy rotated by `angle` about the origin, the curve's midpoint
/// (t = 1/2: x = 0 and y = 0 by the control points' odd symmetry), cross there and nowhere
/// else (the curve meets each circle about the origin at two opposite points, which the
/// small rotation does not bring together). Their separation is about `angle` times the
/// distance from the origin, so they stay within the tolerance for about TOL / `angle`
/// either side of it, inside both curves (which end at distance 1). The answer is that one
/// crossing point, at the origin; upstream returns the stretch as a coincidence range.
#[test]
fn wr_647_cubics_crossing_at_a_small_angle_meet_at_one_point() {
    let k = Kernel::default();
    for angle in [1e-4, 2e-5] {
        let (a, b) = (cubic(0.0, 0.0), cubic(angle, 0.0));
        let all = intersect(&a, &b);
        assert_eq!(all.len(), 1, "angle {angle}: {all:?}");
        let s = &all[0];
        assert_eq!(
            s.solution_type,
            SolutionType::SingleValue,
            "angle {angle}: {s:?}"
        );
        let p = a.evaluate_point(&k, s.start.parameters[0]).unwrap();
        let q = b.evaluate_point(&k, s.start.parameters[1]).unwrap();
        let origin = Point3::new(0.0, 0.0, 0.0);
        assert!(
            p.distance(origin) <= TOL && q.distance(origin) <= TOL,
            "angle {angle}: {p:?} {q:?}"
        );
    }
}

/// The cubic and a copy moved by 4e-6 up at its start and down at its end (a fitted copy's
/// tolerance-scale error that changes sides, closed form: the copy is the cubic plus the
/// linear shift). The curves lie on each other end to end: one coincidence range, though
/// the separation changes sides inside it, because the range ends where the curves stop.
#[test]
fn wr_647_a_copy_changing_sides_within_tolerance_stays_a_range() {
    let (a, b) = (cubic(0.0, 0.0), cubic(0.0, 4e-6));
    let all = intersect(&a, &b);
    assert_eq!(all.len(), 1, "{all:?}");
    assert_eq!(all[0].solution_type, SolutionType::RangeOfValues, "{all:?}");
}

fn bezier(control: &[Point3]) -> Curve {
    let order = control.len();
    Curve::from(
        BSplineCurve::create_canonical(&CanonicalData {
            dim: 3,
            degree: order - 1,
            control_points: control,
            form: BSplineCurveForm::default(),
            knot_multiplicities: &[order, order],
            knots: &[0.0, 1.0],
            knot_type: KnotType::default(),
            weights: None,
            trim: None,
        })
        .unwrap(),
    )
}

/// The parabola y = `c` x² over -1 <= x <= 1 (a quadratic Bezier with controls (-1, c),
/// (0, -c), (1, c), closed form: x = 2t - 1, y = c (2t - 1)²) touches the x axis from (-1, 0)
/// to (1, 0) at the origin and nowhere else. They stay within the tolerance where
/// c x² <= TOL, |x| <= sqrt(TOL / c), inside both curves, on one side. The answer is that
/// one touch point, at the origin (t = 1/2 on both), as upstream already gives (its range
/// classification takes the diverging range for its point); the merged-range rule keeps it.
#[test]
fn wr_647_a_parabola_touching_a_line_meets_it_at_one_point() {
    let k = Kernel::default();
    for c in [1e-2, 1e-3] {
        let line = bezier(&[Point3::new(-1.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)]);
        let parabola = bezier(&[
            Point3::new(-1.0, c, 0.0),
            Point3::new(0.0, -c, 0.0),
            Point3::new(1.0, c, 0.0),
        ]);
        let all = intersect(&line, &parabola);
        assert_eq!(all.len(), 1, "c {c}: {all:?}");
        let s = &all[0];
        assert_eq!(s.solution_type, SolutionType::SingleValue, "c {c}: {s:?}");
        let p = line.evaluate_point(&k, s.start.parameters[0]).unwrap();
        let q = parabola.evaluate_point(&k, s.start.parameters[1]).unwrap();
        let origin = Point3::new(0.0, 0.0, 0.0);
        assert!(
            p.distance(origin) <= TOL && q.distance(origin) <= TOL,
            "c {c}: {p:?} {q:?}"
        );
    }
}

/// The half parabola y = `c` x² over 0 <= x <= 1 (controls (0, 0), (1/2, 0), (1, c), closed
/// form: x = t, y = c t²) leaves the x axis from (0, 0) to (1, 0) tangentially at their
/// shared start, as a round meets a floor it is tangent to. They stay within the tolerance
/// for x <= sqrt(TOL / c), a range that ends inside both curves on one side: where they
/// part. The answer is that one touch point, the shared start, as upstream already gives
/// here; the merged-range rule keeps it. (Where the range classification keeps such a
/// range, the touch rule takes it for the point: the solidcore-ops round-on-floor cases.)
#[test]
fn wr_647_a_parabola_leaving_a_line_at_their_start_meets_it_at_one_point() {
    let k = Kernel::default();
    for c in [1e-2, 1e-3, 1e-4, 2e-5] {
        let line = bezier(&[Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)]);
        let parabola = bezier(&[
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(0.5, 0.0, 0.0),
            Point3::new(1.0, c, 0.0),
        ]);
        let all = intersect(&line, &parabola);
        assert_eq!(all.len(), 1, "c {c}: {all:?}");
        let s = &all[0];
        assert_eq!(s.solution_type, SolutionType::SingleValue, "c {c}: {s:?}");
        let p = line.evaluate_point(&k, s.start.parameters[0]).unwrap();
        let q = parabola.evaluate_point(&k, s.start.parameters[1]).unwrap();
        let origin = Point3::new(0.0, 0.0, 0.0);
        assert!(
            p.distance(origin) <= TOL && q.distance(origin) <= TOL,
            "c {c}: {p:?} {q:?}"
        );
    }
}

/// The quadratic B-spline on the knots 0, 1, 2, 3, 4 (end multiplicities 3) with the
/// control points (-2, 0.5), (-1.25, `y1`), (-0.5, `y2`), (0.5, `y2`), (1.25, `y1`), (2, 0.5):
/// it runs along the x axis from x = -0.875 (the midpoint of its second and third control
/// points, the knot 1) to x = 0.875 (the knot 3), at heights between `y2` and the mean of
/// `y1` and `y2` (convex combinations of its control heights there), and leaves the axis
/// steeply on the same side beyond, inside the line from (-3, 0) to (3, 0) below.
fn flat_quadratic(y1: f64, y2: f64) -> Curve {
    let control = [
        Point3::new(-2.0, 0.5, 0.0),
        Point3::new(-1.25, y1, 0.0),
        Point3::new(-0.5, y2, 0.0),
        Point3::new(0.5, y2, 0.0),
        Point3::new(1.25, y1, 0.0),
        Point3::new(2.0, 0.5, 0.0),
    ];
    Curve::from(
        BSplineCurve::create_canonical(&CanonicalData {
            dim: 3,
            degree: 2,
            control_points: &control,
            form: BSplineCurveForm::default(),
            knot_multiplicities: &[3, 1, 1, 1, 3],
            knots: &[0.0, 1.0, 2.0, 3.0, 4.0],
            knot_type: KnotType::default(),
            weights: None,
            trim: None,
        })
        .unwrap(),
    )
}

fn axis_line() -> Curve {
    bezier(&[Point3::new(-3.0, 0.0, 0.0), Point3::new(3.0, 0.0, 0.0)])
}

/// The flat quadratic 2e-6 to 3e-6 above the x axis over -0.875 <= x <= 0.875 (closed
/// form: its control heights there are 2e-6 and 4e-6), and the line along the axis: within
/// the tolerance (1e-5) of each other over a length of 1.75, and they never meet, so they
/// leave each other on one side inside both curves. That is a coincidence at the tolerance
/// over that part of both (an edge against a fitted copy of part of it), not a touch: the
/// answer is one range covering it, as upstream gives. (The touch rule without the
/// condition that the curves meet took the range for one point, at x = -0.0085.)
#[test]
fn wr_647_a_curve_lying_within_tolerance_on_part_of_a_line_stays_a_range() {
    let k = Kernel::default();
    let (line, curve) = (axis_line(), flat_quadratic(4e-6, 2e-6));
    let all = intersect(&line, &curve);
    assert_eq!(all.len(), 1, "{all:?}");
    let s = &all[0];
    assert_eq!(s.solution_type, SolutionType::RangeOfValues, "{s:?}");
    // the range covers -0.875 <= x <= 0.875 on the line (x = 6 t - 3)
    let (lo, hi) = (
        s.start.parameters[0].min(s.end.parameters[0]),
        s.start.parameters[0].max(s.end.parameters[0]),
    );
    assert!(
        lo <= (3.0 - 0.875) / 6.0 && hi >= (3.0 + 0.875) / 6.0,
        "{s:?}"
    );
    // and pairs points of the two curves within the tolerance at its ends
    for (t, u) in [
        (s.start.parameters[0], s.start.parameters[1]),
        (s.end.parameters[0], s.end.parameters[1]),
    ] {
        let gap = line
            .evaluate_point(&k, t)
            .unwrap()
            .distance(curve.evaluate_point(&k, u).unwrap());
        assert!(gap <= TOL, "{gap}");
    }
}

/// The flat quadratic lying ON the x axis over -0.875 <= x <= 0.875 (control heights 0
/// there: those spans are the axis itself) and leaving it on one side beyond: the two
/// curves lie on each other over that part, an exact overlap, and the answer is one range
/// covering it, not one point of it.
#[test]
fn wr_647_a_curve_lying_on_part_of_a_line_stays_a_range() {
    let (line, curve) = (axis_line(), flat_quadratic(0.0, 0.0));
    let all = intersect(&line, &curve);
    assert_eq!(all.len(), 1, "{all:?}");
    let s = &all[0];
    assert_eq!(s.solution_type, SolutionType::RangeOfValues, "{s:?}");
    let (lo, hi) = (
        s.start.parameters[0].min(s.end.parameters[0]),
        s.start.parameters[0].max(s.end.parameters[0]),
    );
    assert!(
        lo <= (3.0 - 0.875) / 6.0 && hi >= (3.0 + 0.875) / 6.0,
        "{s:?}"
    );
}

/// The flat quadratic touching the x axis at the origin only (control heights 4e-6, 0, 0,
/// 4e-6: on the knot 2 its point is the midpoint of (-0.5, 0) and (0.5, 0), the origin, with
/// the tangent along the axis; on either side its height is the positive multiple t² of 2e-6
/// of the local span parameter t, closed form). They meet at the origin and nowhere else,
/// but the curve stays within the meeting zero (about 4e-12 here) of the axis while
/// 2e-6 t² <= 4e-12, |t| <= 1.4e-3: a run about 2.8e-3 long, which the tolerance (1e-5)
/// resolves. At the zero that run cannot be told from a near overlap that never meets (the
/// review r8 rows), so the touch is not certified and the stretch keeps upstream's range,
/// covering the origin. (An earlier head of this PR asserted one point here.)
#[test]
fn wr_647_a_flat_touch_whose_run_within_the_zero_the_tolerance_resolves_stays_a_range() {
    let (line, curve) = (axis_line(), flat_quadratic(4e-6, 0.0));
    let all = intersect(&line, &curve);
    // the line is x = 6 t - 3: the origin at t = 1/2
    assert!(
        all.iter()
            .any(|s| s.solution_type == SolutionType::RangeOfValues
                && s.start.parameters[0].min(s.end.parameters[0]) < 0.5
                && s.start.parameters[0].max(s.end.parameters[0]) > 0.5),
        "{all:?}"
    );
}

const ZA: [f64; 5] = [
    -0.00010781025415934847,
    -0.00012670256036516824,
    0.00021511861334269806,
    0.0002595283662315377,
    -0.00009592442162159323,
];
const ZB: [f64; 5] = [
    0.00020936248929282104,
    -0.00012160629484595162,
    0.00012094250528638395,
    0.00026362296693292704,
    -0.0001239771010247445,
];

/// The quadratic B-spline (knots 0, 0, 0, 1/3, 2/3, 1, 1, 1) in the plane x = 0.001 with
/// y control values 0, 0.00025, ..., 0.001 and the z control values `z`.
fn quadratic(z: &[f64; 5]) -> Curve {
    let control: Vec<Point3> = z
        .iter()
        .enumerate()
        .map(|(i, &z)| Point3::new(0.001, 0.00025 * i as f64, z))
        .collect();
    Curve::from(
        BSplineCurve::create_canonical(&CanonicalData {
            dim: 3,
            degree: 2,
            control_points: &control,
            form: BSplineCurveForm::default(),
            knot_multiplicities: &[3, 1, 1, 3],
            knots: &[0.0, 1.0 / 3.0, 2.0 / 3.0, 1.0],
            knot_type: KnotType::default(),
            weights: None,
            trim: None,
        })
        .unwrap(),
    )
}

/// Two boundary curves of a face pair (the goldens row `intersect.surface_intersector_p2`
/// gen1228), at tolerance 1e-4. They share x and y on the same knots, so they differ only
/// in z, by the quadratic B-spline `d` with control values ZA - ZB (closed form, no
/// sampling): on [1/3, 1] the Bezier control values of `d` on [1/3, 2/3] are all positive
/// and below 1e-4, and on [2/3, 1] they are below 1e-4 with a negative discriminant and a
/// positive leading coefficient, so 0 < d < 1e-4 there: the curves never meet on [1/3, 1]
/// and stay within the tolerance to the curve end. That is a coincidence running to the
/// end, not a crossing: the answer keeps a range reaching t = 1 and has no point on
/// [1/3, 1]. (The crossing rule without the both-ends parting condition took the stretch
/// for a crossing at t = 0.8926, where the curves are 1.05e-5 apart.)
#[test]
fn wr_647_a_coincidence_running_to_a_curve_end_is_not_a_crossing() {
    let d: Vec<f64> = ZA.iter().zip(&ZB).map(|(a, b)| a - b).collect();
    let mid = |a: f64, b: f64| (a + b) * 0.5;
    // span [1/3, 2/3]: Bezier control values mid(d1, d2), d2, mid(d2, d3)
    for b in [mid(d[1], d[2]), d[2], mid(d[2], d[3])] {
        assert!(b > 1e-5 && b < 1e-4, "{b}");
    }
    // span [2/3, 1]: mid(d2, d3), d3, d4
    let (b0, b1, b2) = (mid(d[2], d[3]), d[3], d[4]);
    for b in [b0, b1, b2] {
        assert!(b < 1e-4, "{b}");
    }
    let lead = b0 - 2.0 * b1 + b2;
    let lin = 2.0 * (b1 - b0);
    let disc = lin * lin - 4.0 * lead * b0;
    // margins of 1e-9 against values of order 1e-8: rounding is below 1e-20
    assert!(lead > 1e-5 && disc < -1e-9, "lead {lead} disc {disc}");

    let k = Kernel::default();
    let (a, b) = (quadratic(&ZA), quadratic(&ZB));
    let (ia, ib) = (a.natural_interval(), b.natural_interval());
    let sols = a
        .global_curve_intersect(
            &k,
            &ia,
            &b,
            &ib,
            1e-4,
            &CurveIntersectOptions::<H>::default(),
        )
        .unwrap();
    let all = sols.as_slice();
    for s in all {
        if s.solution_type == SolutionType::SingleValue {
            assert!(s.start.parameters[0] < 1.0 / 3.0, "{all:?}");
        }
    }
    assert!(
        all.iter()
            .any(|s| s.solution_type == SolutionType::RangeOfValues
                && s.start.parameters[0] <= 1.0 / 3.0
                && (s.end.parameters[0] - 1.0).abs() < 1e-9),
        "{all:?}"
    );
}

/// The quadratic B-spline on the knots 0, 1, 2, 3, 4, 5 (end multiplicities 3) through
/// the control points `(x, y)` in the plane z = 0.
fn quadratic_on_five_spans(control: &[(f64, f64); 7]) -> Curve {
    let control: Vec<Point3> = control
        .iter()
        .map(|&(x, y)| Point3::new(x, y, 0.0))
        .collect();
    Curve::from(
        BSplineCurve::create_canonical(&CanonicalData {
            dim: 3,
            degree: 2,
            control_points: &control,
            form: BSplineCurveForm::default(),
            knot_multiplicities: &[3, 1, 1, 1, 1, 3],
            knots: &[0.0, 1.0, 2.0, 3.0, 4.0, 5.0],
            knot_type: KnotType::default(),
            weights: None,
            trim: None,
        })
        .unwrap(),
    )
}

/// Three crossings in one stretch within the tolerance (the r2 review's probe of
/// tangency-labs/solidcraft#18392): the quadratic B-spline with control points (-2, 0.5),
/// (-1.25, h), (-0.6, -h), (0, h), (0.6, -h), (1.25, -h), (2, -0.5), h = 4e-3, against the
/// line from (-3, 0) to (3, 0), at tolerance 1e-2. Closed form: on the knots 1, 2 and 3
/// the curve is the midpoint of two control points, (-0.925, 0), (-0.3, 0) and (0.3, 0);
/// the Bezier heights of the spans between are (h/2 + h/2 = 0, -h, 0), (0, h, 0) and
/// (0, -h, -h), and of the first span (0.5, h, 0): the curve is above the axis on [0, 1)
/// and [2, 3) and below it on (1, 2) and (3, 5], so it crosses the axis at exactly those
/// three points. All of it from x = -1.6 on lies within the tolerance of the line (its
/// heights there are below 1e-2), one stretch whose ends lie inside both curves and whose
/// end separations point to opposite sides. One point cannot stand for it (the head
/// before the certificate returned the crossing at x = 0.3 alone): every crossing must
/// lie in the answer, as a point within 1e-6 of it or inside a range.
#[test]
fn wr_647_three_crossings_in_one_stretch_are_not_one_point() {
    let k = Kernel::default();
    let h = 4e-3;
    let curve = quadratic_on_five_spans(&[
        (-2.0, 0.5),
        (-1.25, h),
        (-0.6, -h),
        (0.0, h),
        (0.6, -h),
        (1.25, -h),
        (2.0, -0.5),
    ]);
    // the three crossings, on the curve's knots 1, 2, 3 (closed form above)
    let crossings = [(-0.925, 1.0), (-0.3, 2.0), (0.3, 3.0)];
    for &(x, u) in &crossings {
        let p = curve.evaluate_point(&k, u).unwrap();
        assert!((p.x - x).abs() <= 1e-15 && p.y.abs() <= 1e-15, "{p:?}");
    }
    let all = intersect_at(&axis_line(), &curve, 1e-2);
    for &(x, u) in &crossings {
        let at = Point3::new(x, 0.0, 0.0);
        let held = all.iter().any(|s| match s.solution_type {
            SolutionType::RangeOfValues => {
                let (lo, hi) = (
                    s.start.parameters[1].min(s.end.parameters[1]),
                    s.start.parameters[1].max(s.end.parameters[1]),
                );
                lo <= u && u <= hi
            }
            _ => {
                curve
                    .evaluate_point(&k, s.start.parameters[1])
                    .unwrap()
                    .distance(at)
                    <= 1e-6
            }
        });
        assert!(held, "the crossing at x = {x} is lost: {all:?}");
    }
}

/// Two touches in one stretch within the tolerance: the quadratic B-spline with control
/// points (-2, 0.5), (-1.25, 0), (-0.5, 0), (0, c), (0.5, 0), (1.25, 0), (2, 0.5), c = 4e-6,
/// against the line along the x axis, at tolerance 1e-5. Closed form: its Bezier heights
/// are (0.5, 0, 0) on [0, 1], (0, 0, c/2) on [1, 2], (c/2, c, c/2) on [2, 3] and their
/// mirror images beyond, so it meets the axis at x = -0.875 and x = 0.875 (the knots 1 and
/// 4, the tangent along the axis), lies above it between them at most 3c/4 = 3e-6 high, and
/// leaves it steeply outside. Its second derivative across the axis changes sign (the
/// heights of [2, 3] are concave), so the touch certificate fails, and the stretch keeps a
/// range covering both touches: one point would lose the other touch.
#[test]
fn wr_647_two_touches_in_one_stretch_keep_their_range() {
    let k = Kernel::default();
    let c = 4e-6;
    let curve = quadratic_on_five_spans(&[
        (-2.0, 0.5),
        (-1.25, 0.0),
        (-0.5, 0.0),
        (0.0, c),
        (0.5, 0.0),
        (1.25, 0.0),
        (2.0, 0.5),
    ]);
    let touches = [(-0.875, 1.0), (0.875, 4.0)];
    for &(x, u) in &touches {
        let p = curve.evaluate_point(&k, u).unwrap();
        assert!((p.x - x).abs() <= 1e-15 && p.y.abs() <= 1e-15, "{p:?}");
    }
    let all = intersect(&axis_line(), &curve);
    for &(x, u) in &touches {
        let at = Point3::new(x, 0.0, 0.0);
        let held = all.iter().any(|s| match s.solution_type {
            SolutionType::RangeOfValues => {
                let (lo, hi) = (
                    s.start.parameters[1].min(s.end.parameters[1]),
                    s.start.parameters[1].max(s.end.parameters[1]),
                );
                lo <= u && u <= hi
            }
            _ => {
                curve
                    .evaluate_point(&k, s.start.parameters[1])
                    .unwrap()
                    .distance(at)
                    <= 1e-6
            }
        });
        assert!(held, "the touch at x = {x} is lost: {all:?}");
    }
    assert!(
        all.iter()
            .any(|s| s.solution_type == SolutionType::RangeOfValues),
        "{all:?}"
    );
}

/// Review r3 (B1): two meetings of a planar curve and a curve leaving its plane only at
/// the fourth order. A is the parabola y = x²/2 in z = 0 (the quadratic Bezier (-1, 1/2),
/// (0, -1/2), (1, 1/2), x = 2t - 1). B is the quartic Bezier `(X, X²/2 + (X² - δ²)/16,
/// 2⁻²⁰ (X² - δ²)²)`, X = 2t - 1, δ = 1/32 (its control points below are those
/// polynomials' Bernstein coefficients). In the plane B crosses A at X = ±δ and is 6.1e-5
/// from it at X = 0; across the plane it is at most 2⁻²⁰ δ⁴ = 8.9e-16 off it between the
/// meetings. So it meets A exactly twice, at t = (1 ± δ)/2 on both, and every answer must
/// hold both meetings (a point within 1e-5 of each, or a range over it). The head before
/// this test took the stretch for one touch (the band of A's plane alone put both in one
/// interval) and lost one meeting at tolerances 1e-3, 3e-3 and 1e-2. The stretch now has
/// no certificate (neither curve is straight, and `curves_meet_at_most_once` refuses two
/// meetings), so it keeps main's range.
#[test]
fn wr_647_two_meetings_within_a_planar_curves_band_are_both_kept() {
    let k = Kernel::default();
    let a = bezier(&[
        Point3::new(-1.0, 0.5, 0.0),
        Point3::new(0.0, -0.5, 0.0),
        Point3::new(1.0, 0.5, 0.0),
    ]);
    let b = bezier(&[
        Point3::new(-1.0, 0.56243896484375, 9.518125807517208e-07),
        Point3::new(-0.5, -6.103515625e-05, -9.536734069115482e-07),
        Point3::new(0.0, -0.18756103515625, 9.542961076173622e-07),
        Point3::new(0.5, -6.103515625e-05, -9.536734069115482e-07),
        Point3::new(1.0, 0.56243896484375, 9.518125807517208e-07),
    ]);
    let d = 1.0 / 32.0;
    for x in [-d, d] {
        let t = 0.5 * (x + 1.0);
        let gap = b
            .evaluate_point(&k, t)
            .unwrap()
            .distance(a.evaluate_point(&k, t).unwrap());
        assert!(gap <= 1e-15, "X {x}: {gap:e}");
    }
    let middle = b
        .evaluate_point(&k, 0.5)
        .unwrap()
        .distance(a.evaluate_point(&k, 0.5).unwrap());
    assert!(middle > 6e-5, "{middle:e}");
    for tol in [3e-4, 1e-3, 3e-3, 1e-2] {
        let all = intersect_at(&a, &b, tol);
        for x in [-d, d] {
            let u = 0.5 * (x + 1.0);
            let at = b.evaluate_point(&k, u).unwrap();
            let held = all.iter().any(|s| match s.solution_type {
                SolutionType::RangeOfValues => {
                    let (lo, hi) = (
                        s.start.parameters[1].min(s.end.parameters[1]),
                        s.start.parameters[1].max(s.end.parameters[1]),
                    );
                    lo <= u && u <= hi
                }
                _ => {
                    b.evaluate_point(&k, s.start.parameters[1])
                        .unwrap()
                        .distance(at)
                        <= 1e-5
                }
            });
            assert!(held, "tol {tol}: the meeting at X = {x} is lost: {all:?}");
        }
    }
}

/// The unit circle about (0.5, 1, 0) in the plane z = 0 (review r4).
fn floor_circle(k: &Kernel) -> Curve {
    Curve::from(
        Ellipse::new(
            k,
            Vec3::new(0.5, 1.0, 0.0),
            Vec3::X,
            Vec3::Y,
            Extent1d::from_min_max(0.0, 360.0),
            1.0,
            1.0,
            &EllipseOptions::default(),
        )
        .unwrap(),
    )
}

/// A rational quadratic spline (knots 0, 1, 2, 3) whose middle span is exactly the 60
/// degree arc of [`floor_circle`] below its centre (weights 1, cos 30°, 1), and whose outer
/// spans leave the circle's plane by `lift` at their inner controls and 4 `lift` at the
/// ends (review r4's probe).
fn arc_spline(lift: f64) -> Curve {
    let co = 0.866_025_403_784_438_6_f64;
    let p2 = Point3::new(0.0, 1.0 - co, 0.0);
    let p4 = Point3::new(1.0, 1.0 - co, 0.0);
    let pts = [
        p2 + Vec3::new(-0.2, 0.1155, 4.0 * lift),
        p2 + Vec3::new(-0.1, 0.0577, lift),
        p2,
        Point3::new(0.5, 1.0 - 1.0 / co, 0.0),
        p4,
        p4 + Vec3::new(0.1, 0.0577, lift),
        p4 + Vec3::new(0.2, 0.1155, 4.0 * lift),
    ];
    let w = [1.0, 1.0, 1.0, co, 1.0, 1.0, 1.0];
    Curve::from(
        BSplineCurve::create_canonical(&CanonicalData {
            dim: 3,
            degree: 2,
            control_points: &pts,
            form: BSplineCurveForm::default(),
            knot_multiplicities: &[3, 2, 2, 3],
            knots: &[0.0, 1.0, 2.0, 3.0],
            knot_type: KnotType::default(),
            weights: Some(&w),
            trim: None,
        })
        .unwrap(),
    )
}

/// Review r4: a curve lying exactly on a planar conic over 60 degrees and leaving its
/// plane on both sides overlaps it over that arc. The overlap is not one meeting: a range
/// must still cover it (main's answer), at a tolerance where the stretch reaches past it,
/// both ways round.
#[test]
fn wr_647_an_exact_overlap_with_a_conic_stays_a_range() {
    let k = Kernel::default();
    let circle = floor_circle(&k);
    for lift in [1e-3, 1e-4] {
        let s = arc_spline(lift);
        for i in 0..=20 {
            let p = s.evaluate_point(&k, 1.0 + f64::from(i) / 20.0).unwrap();
            let rho = ((p.x - 0.5).powi(2) + (p.y - 1.0).powi(2)).sqrt();
            let off = ((rho - 1.0).powi(2) + p.z * p.z).sqrt();
            assert!(
                off <= 1e-15,
                "lift {lift}: the middle span is off the circle by {off:e}"
            );
        }
        for (all, side) in [
            (intersect_at(&circle, &s, 1e-2), 1),
            (intersect_at(&s, &circle, 1e-2), 0),
        ] {
            let covered = all.iter().any(|x| {
                x.solution_type == SolutionType::RangeOfValues && {
                    let (a, b) = (x.start.parameters[side], x.end.parameters[side]);
                    a.min(b) <= 1.0 && 2.0 <= a.max(b)
                }
            });
            assert!(
                covered,
                "lift {lift}: the overlap s in [1, 2] is lost: {all:?}"
            );
        }
    }
}

/// A rational quadratic spline (knots 0, 1, 2, 3) whose middle span is exactly the
/// `theta` degree arc of [`floor_circle`] about its lowest point (weights 1, cos of half
/// `theta`, 1), and whose outer spans run back along the arc's end tangents by `ell` and
/// leave the circle's plane by `lift` at their inner controls and 4 `lift` at the ends
/// (review r5's probe).
fn arc_spline_of(theta: f64, ell: f64, lift: f64) -> Curve {
    arc_spline_near(theta, ell, lift, 0.0, 0.0, 1.0)
}

/// [`arc_spline_of`] with every control point moved radially about the circle's axis by
/// the factor `1 + dr` (the middle span a concentric arc of radius `1 + dr`) and lifted by
/// `dz`, then the whole curve scaled about the origin by `scale` (review r6's probe; the
/// control points unchanged when `dr`, `dz` are 0 and `scale` is 1).
fn arc_spline_near(theta: f64, ell: f64, lift: f64, dr: f64, dz: f64, scale: f64) -> Curve {
    arc_spline_stretched(theta, ell, lift, dr, dz, scale, 1.0)
}

/// [`arc_spline_near`] with knots 0, `l`, `l` + 1, 2 `l` + 1: the same control points and
/// weights, each span the same quadratic Bezier, so the same geometry, but the arc span is
/// 1 / (2 `l` + 1) of the spline's natural interval (review r7's probe E3).
fn arc_spline_stretched(
    theta: f64,
    ell: f64,
    lift: f64,
    dr: f64,
    dz: f64,
    scale: f64,
    l: f64,
) -> Curve {
    let h = (theta / 2.0).to_radians();
    let (a0, a1) = (
        -std::f64::consts::FRAC_PI_2 - h,
        -std::f64::consts::FRAC_PI_2 + h,
    );
    let on = |a: f64| Point3::new(0.5 + a.cos(), 1.0 + a.sin(), 0.0);
    let (p2, p4) = (on(a0), on(a1));
    let back = Vec3::new(a0.sin(), -a0.cos(), 0.0);
    let ahead = Vec3::new(-a1.sin(), a1.cos(), 0.0);
    let z = Vec3::new(0.0, 0.0, 1.0);
    let pts = [
        p2 + back * ell + z * (4.0 * lift),
        p2 + back * (ell / 2.0) + z * lift,
        p2,
        Point3::new(0.5, 1.0 - 1.0 / h.cos(), 0.0),
        p4,
        p4 + ahead * (ell / 2.0) + z * lift,
        p4 + ahead * ell + z * (4.0 * lift),
    ];
    let pts = if dr == 0.0 && dz == 0.0 && scale == 1.0 {
        pts
    } else {
        pts.map(|p| {
            Point3::new(
                0.5 + (p.x - 0.5) * (1.0 + dr),
                1.0 + (p.y - 1.0) * (1.0 + dr),
                p.z + dz,
            ) * scale
        })
    };
    let w = [1.0, 1.0, 1.0, h.cos(), 1.0, 1.0, 1.0];
    Curve::from(
        BSplineCurve::create_canonical(&CanonicalData {
            dim: 3,
            degree: 2,
            control_points: &pts,
            form: BSplineCurveForm::default(),
            knot_multiplicities: &[3, 2, 2, 3],
            knots: &[0.0, l, l + 1.0, 2.0 * l + 1.0],
            knot_type: KnotType::default(),
            weights: Some(&w),
            trim: None,
        })
        .unwrap(),
    )
}

/// Review r5: exact overlaps with a planar conic shorter than a tenth of the shorter curve
/// (30 degrees with outer spans 3 long, 10 and 5 degrees with outer spans 1 long) are
/// overlaps too, not one meeting: a range must still cover s in [1, 2] (main's answer),
/// both ways round, at a tolerance where the stretch reaches past the overlap. No length
/// decides it: the exact curves coincide over the arc.
#[test]
fn wr_647_short_exact_overlaps_with_a_conic_stay_ranges() {
    let k = Kernel::default();
    let circle = floor_circle(&k);
    for (theta, ell) in [(30.0, 3.0), (10.0, 1.0), (5.0, 1.0)] {
        for lift in [1e-3, 1e-4] {
            let s = arc_spline_of(theta, ell, lift);
            for i in 0..=20 {
                let p = s.evaluate_point(&k, 1.0 + f64::from(i) / 20.0).unwrap();
                let rho = ((p.x - 0.5).powi(2) + (p.y - 1.0).powi(2)).sqrt();
                let off = ((rho - 1.0).powi(2) + p.z * p.z).sqrt();
                assert!(
                    off <= 1e-15,
                    "{theta} degrees, lift {lift}: the middle span is off the circle by {off:e}"
                );
            }
            for (all, side) in [
                (intersect_at(&circle, &s, 1e-2), 1),
                (intersect_at(&s, &circle, 1e-2), 0),
            ] {
                let covered = all.iter().any(|x| {
                    x.solution_type == SolutionType::RangeOfValues && {
                        let (a, b) = (x.start.parameters[side], x.end.parameters[side]);
                        a.min(b) <= 1.0 && 2.0 <= a.max(b)
                    }
                });
                assert!(
                    covered,
                    "{theta} degrees, lift {lift}: the overlap s in [1, 2] is lost: {all:?}"
                );
            }
        }
    }
}

/// Review r6: a curve within the meeting zero of a planar conic over an arc, but not on
/// it (a concentric arc `dr` off the circle, or lifted `dz` off its plane, both between
/// the rounding and the scaled zero, about 3e-12 here), meets it at every point of the
/// arc: a run of meetings, not one. A range must still cover s in [1, 2] (main's answer),
/// both ways round, at 30 and 60 degrees. The last rows are the same at 50 times the size
/// (the scaled zero about 1.5e-10, the ops tangent-floor case's scale), `dr` 1e-12.
#[test]
fn wr_647_near_overlaps_within_the_zero_stay_ranges() {
    let k = Kernel::default();
    let rows = [
        // (theta, ell, dr, dz, scale)
        (30.0, 3.0, 1e-13, 0.0, 1.0),
        (30.0, 3.0, 5e-13, 0.0, 1.0),
        (60.0, 0.23094, 1e-13, 0.0, 1.0),
        (60.0, 0.23094, 5e-13, 0.0, 1.0),
        (30.0, 3.0, 0.0, 1e-13, 1.0),
        (30.0, 3.0, 0.0, 1e-12, 1.0),
        (60.0, 0.23094, 0.0, 1e-13, 1.0),
        (30.0, 3.0, 1e-12, 0.0, 50.0),
        (60.0, 0.23094, 1e-12, 0.0, 50.0),
    ];
    for (theta, ell, dr, dz, scale) in rows {
        let circle = if scale == 1.0 {
            floor_circle(&k)
        } else {
            Curve::from(
                Ellipse::new(
                    &k,
                    Vec3::new(0.5, 1.0, 0.0) * scale,
                    Vec3::X,
                    Vec3::Y,
                    Extent1d::from_min_max(0.0, 360.0),
                    scale,
                    scale,
                    &EllipseOptions::default(),
                )
                .unwrap(),
            )
        };
        let s = arc_spline_near(theta, ell, 1e-3, dr, dz, scale);
        let tag = format!("{theta} degrees, dr {dr:e}, dz {dz:e}, scale {scale}");
        // the middle span is off the circle by about dr (radially) or dz (lifted), within
        // the scaled zero: every point of it is a meeting
        let zero = 1e-12 * (1.0 + 2.0 * scale);
        for i in 0..=20 {
            let p = s.evaluate_point(&k, 1.0 + f64::from(i) / 20.0).unwrap() * (1.0 / scale);
            let rho = ((p.x - 0.5).powi(2) + (p.y - 1.0).powi(2)).sqrt();
            let off = ((rho - 1.0).powi(2) + p.z * p.z).sqrt() * scale;
            assert!(
                off <= zero / 2.0 && off >= (dr + dz) * scale / 4.0,
                "{tag}: the middle span is off the circle by {off:e}"
            );
        }
        for (all, side) in [
            (intersect_at(&circle, &s, 1e-2 * scale), 1),
            (intersect_at(&s, &circle, 1e-2 * scale), 0),
        ] {
            let covered = all.iter().any(|x| {
                x.solution_type == SolutionType::RangeOfValues && {
                    let (a, b) = (x.start.parameters[side], x.end.parameters[side]);
                    a.min(b) <= 1.0 && 2.0 <= a.max(b)
                }
            });
            assert!(covered, "{tag}: the run s in [1, 2] is lost: {all:?}");
        }
    }
}

/// `GlobalCurveIntersect` of `a` against `b` at `tol`.
fn intersect_with(a: &Curve, b: &Curve, tol: f64) -> Vec<Solution<H>> {
    let k = kernel();
    let (ia, ib) = (a.natural_interval(), b.natural_interval());
    a.global_curve_intersect(&k, &ia, b, &ib, tol, &CurveIntersectOptions::<H>::default())
        .unwrap()
        .as_slice()
        .to_vec()
}

/// Review r7 (probe E3): the near and exact overlaps above, with the spline's knots
/// stretched to 0, `L`, `L` + 1, 2 `L` + 1 (the geometry unchanged), so the arc span is
/// below a tenth of the spline's natural interval (1/21 at `L` = 10, 1/61 at `L` = 30) and
/// the 10 and 5 degree arcs below a tenth of the circle's. Each is still a run of
/// meetings, not one: a range must cover s in [`L`, `L` + 1] (main's answer), both ways
/// round, at tolerance 1e-2.
/// A zero-run cut at a tenth of both curves collapsed 11 of these rows at `L` = 10 to one
/// point at the end of the overlap.
#[test]
fn wr_647_sub_tenth_near_overlaps_with_a_conic_stay_ranges() {
    let k = Kernel::default();
    let circle = floor_circle(&k);
    let mut rows = Vec::new();
    for dr in [1e-13, 5e-13] {
        for (theta, ell) in [(30.0, 3.0), (10.0, 1.0), (5.0, 1.0)] {
            rows.push((theta, ell, dr, 0.0));
        }
    }
    for dz in [1e-13, 1e-12] {
        for (theta, ell) in [(30.0, 3.0), (10.0, 1.0)] {
            rows.push((theta, ell, 0.0, dz));
        }
    }
    // exact overlaps
    rows.push((30.0, 3.0, 0.0, 0.0));
    rows.push((5.0, 1.0, 0.0, 0.0));
    for l in [10.0, 30.0] {
        for &(theta, ell, dr, dz) in &rows {
            let s = arc_spline_stretched(theta, ell, 1e-3, dr, dz, 1.0, l);
            let tag = format!("L {l}, {theta} degrees, dr {dr:e}, dz {dz:e}");
            for (all, side) in [
                (intersect_with(&circle, &s, 1e-2), 1),
                (intersect_with(&s, &circle, 1e-2), 0),
            ] {
                let covered = all.iter().any(|x| {
                    x.solution_type == SolutionType::RangeOfValues && {
                        let (a, b) = (x.start.parameters[side], x.end.parameters[side]);
                        a.min(b) <= l + 1e-9 && l + 1.0 - 1e-9 <= a.max(b)
                    }
                });
                assert!(covered, "{tag}: the run s in [L, L + 1] is lost: {all:?}");
            }
        }
    }
}

/// The quadratic C1 B-spline over u in [0, 1] with x = 2u - 1 that is exactly
/// `y = dr + c (u - 1/2)²` for |u - 1/2| <= `h` and continues C1 beyond with `y'' = e`
/// (`A + B t + e t² / 2`, t = |u - 1/2| - h), on the interior knots `inner` (simple, holding
/// 1/2 ± h); its control points are the blossoms of those polynomials (review r8's probe).
fn flat_then_bending(h: f64, dr: f64, c: f64, e: f64, inner: &[f64]) -> Curve {
    let (a, b) = (dr + c * h * h, 2.0 * c * h);
    // the piece (alpha, beta, gamma), y = alpha + beta u + gamma u², of the span at `um`
    let piece = |um: f64| -> (f64, f64, f64) {
        if (um - 0.5).abs() <= h {
            (dr + c / 4.0, -c, c)
        } else if um > 0.5 {
            let o = 0.5 + h;
            (a - b * o + e / 2.0 * o * o, b - e * o, e / 2.0)
        } else {
            let o = 0.5 - h;
            (a + b * o + e / 2.0 * o * o, -b - e * o, e / 2.0)
        }
    };
    let mut knots = vec![0.0, 0.0, 0.0];
    knots.extend_from_slice(inner);
    knots.extend([1.0, 1.0, 1.0]);
    let mut control = Vec::new();
    for i in 0..knots.len() - 3 {
        let (x, y) = (knots[i + 1], knots[i + 2]);
        let um = if y > x {
            (x + y) / 2.0
        } else if x == 0.0 {
            knots[3] / 2.0
        } else {
            (x + 1.0) / 2.0
        };
        let (al, be, ga) = piece(um);
        control.push(Point3::new(
            x + y - 1.0,
            al + be * (x + y) / 2.0 + ga * x * y,
            0.0,
        ));
    }
    let mut mults = vec![3];
    mults.extend(inner.iter().map(|_| 1));
    mults.push(3);
    let mut distinct = vec![0.0];
    distinct.extend_from_slice(inner);
    distinct.push(1.0);
    Curve::from(
        BSplineCurve::create_canonical(&CanonicalData {
            dim: 3,
            degree: 2,
            control_points: &control,
            form: BSplineCurveForm::default(),
            knot_multiplicities: &mults,
            knots: &distinct,
            knot_type: KnotType::default(),
            weights: None,
            trim: None,
        })
        .unwrap(),
    )
}

/// Review r8 (B1): a C1 quadratic spline that lies within 0.85e-12 + `dr` of the line
/// from (-1.5, 0) to (1.5, 0) over -0.1 <= x <= 0.1 (u in [0.45, 0.55]; under the meeting
/// zero, 1e-12 (1 + max coordinate)) and bends gently away beyond, within the tolerance
/// over most of the curve. With `dr` = 1e-13 it never meets the line: a near overlap at the
/// zero 0.2 long, which upstream keeps as a range. With `dr` = 0 it meets the line once,
/// at the origin, but runs within the zero of it over the same 0.2: the tolerance (1e-5,
/// 1e-3) resolves that run, and the meeting is not certified the only contact either. Every
/// row, both orders, must keep a range covering the run (main: line [0.3, 0.7], curve
/// [0.2, 0.8] for the first). The half-stretch cut of the previous head took the first,
/// second and fourth rows for one point at u = 1/2.
#[test]
fn wr_647_a_run_within_the_zero_resolved_by_the_tolerance_stays_a_range() {
    let line = bezier(&[Point3::new(-1.5, 0.0, 0.0), Point3::new(1.5, 0.0, 0.0)]);
    let rows: [(f64, f64, f64, &[f64], f64); 6] = [
        (0.05, 1e-13, 2.2e-4, &[0.2, 0.45, 0.55, 0.8], 1e-5),
        (0.05, 1e-13, 2.2e-4, &[0.1, 0.2, 0.45, 0.55, 0.8, 0.9], 1e-5),
        (
            0.04,
            1e-13,
            2.2e-4,
            &[0.15, 0.3, 0.46, 0.54, 0.7, 0.85],
            1e-5,
        ),
        (0.05, 1e-13, 2.2e-2, &[0.2, 0.45, 0.55, 0.8], 1e-3),
        (
            0.05,
            1e-13,
            2.2e-2,
            &[0.1, 0.2, 0.3, 0.45, 0.55, 0.7, 0.8, 0.9],
            1e-3,
        ),
        (0.05, 0.0, 2.2e-4, &[0.2, 0.45, 0.55, 0.8], 1e-5),
    ];
    // near the cut (review r9): runs within the zero of 4.0e-3 (h 1e-3) and 1.015e-3
    // (h 2.5e-4), 4 and 1.015 times the tolerance 1e-3, which a cut loosened to a few
    // tolerances would take for points
    let mut rows: Vec<(f64, f64, f64, Vec<f64>, f64)> = rows
        .iter()
        .map(|&(h, dr, e, inner, tol)| (h, dr, e, inner.to_vec(), tol))
        .collect();
    for h in [1e-3, 2.5e-4] {
        for dr in [1e-13, 0.0] {
            rows.push((h, dr, 2.2e-2, vec![0.2, 0.5 - h, 0.5 + h, 0.8], 1e-3));
        }
    }
    let mut lost = Vec::new();
    for (h, dr, e, inner, tol) in rows {
        let c = (0.85e-12 - dr) / (h * h);
        let curve = flat_then_bending(h, dr, c, e, &inner);
        for (order, all, on_curve) in [
            ("line, curve", intersect_at(&line, &curve, tol), 1),
            ("curve, line", intersect_at(&curve, &line, tol), 0),
        ] {
            let covered = all.iter().any(|s| {
                let (u0, u1) = (s.start.parameters[on_curve], s.end.parameters[on_curve]);
                s.solution_type == SolutionType::RangeOfValues
                    && u0.min(u1) <= 0.5 - h
                    && u0.max(u1) >= 0.5 + h
            });
            if !covered {
                lost.push(format!("h {h} dr {dr:e} tol {tol:e} ({order}): {all:?}"));
            }
        }
    }
    // the other side of the cut: a run within the zero of 1.07e-4 (h 2.5e-5), a tenth of
    // the tolerance 1e-3, is a contact the tolerance does not resolve, one point
    for dr in [1e-13, 0.0] {
        let h = 2.5e-5;
        let c = (0.85e-12 - dr) / (h * h);
        let curve = flat_then_bending(h, dr, c, 2.2e-2, &[0.2, 0.5 - h, 0.5 + h, 0.8]);
        let k = kernel();
        for (order, all, on_curve, first, second) in [
            (
                "line, curve",
                intersect_at(&line, &curve, 1e-3),
                1,
                &line,
                &curve,
            ),
            (
                "curve, line",
                intersect_at(&curve, &line, 1e-3),
                0,
                &curve,
                &line,
            ),
        ] {
            // both parameters must give the contact at the origin: a wrong line parameter
            // with the right curve parameter is a point 3 |dt| away
            let near_origin = |c: &Curve, t: f64| {
                c.evaluate_point(&k, t)
                    .is_ok_and(|p| p.distance(Point3::new(0.0, 0.0, 0.0)) <= 1e-3)
            };
            let point = matches!(all.as_slice(), [s]
                if s.solution_type == SolutionType::SingleValue
                    && (s.start.parameters[on_curve] - 0.5).abs() <= 1e-3
                    && near_origin(first, s.start.parameters[0])
                    && near_origin(second, s.start.parameters[1]));
            if !point {
                lost.push(format!(
                    "h {h} dr {dr:e} tol 1e-3 ({order}), not one point: {all:?}"
                ));
            }
        }
    }
    assert!(lost.is_empty(), "{lost:#?}");
}
