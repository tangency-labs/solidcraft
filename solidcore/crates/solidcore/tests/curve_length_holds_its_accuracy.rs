//! A curve's length holds the accuracy it is asked for, from 1e-3 to 1e-10, against its
//! closed form. Each span of the curve's cache (within one knot span) is the adaptive
//! Cash–Karp integral of `|C'|`:
//! - each span's end was evaluated in the knot span above it (upstream's
//!   `bFromLeft = TRUE`). Where the speed jumps at a C0 knot (the double knot between
//!   unequal rational arcs) the span's last sample was the next span's value; its
//!   steps shrank toward the knot and underflowed (-0.19 at 1e-10: a 70 degree span
//!   failed, its length taken as zero). The end is now evaluated in its own span, as the
//!   area integrals' iso-u pieces (WR-670).
//! - its steps were accepted on the embedded error estimate alone, a null rule of
//!   degree 3 that vanishes where `g''''` changes sign; they are now accepted on the
//!   larger of it and the fifth divided difference (`OdeOptions::quadrature`, as the
//!   precise-property integrals).
//! - each step's error was held to the accuracy relative to the length so far, so the
//!   steps' errors could add up past the accuracy asked for the whole curve. Each step
//!   is now held to half the accuracy relative to its share of the curve (its parameter
//!   step times the estimated length per unit of parameter) plus its own increment
//!   (`OdeOptions::rate`), shares that add up to at most twice the length.
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]
use solidcore::curve::curve::Curve;
use solidcore::geom::extent1d::Extent1d;
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::math::{CPoint, NOW};
use solidcore::Kernel;

const RADIUS: f64 = 5.0;

/// A rational quadratic B-spline of `RADIUS` through the arcs `arcs_deg` in turn, one
/// arc a unit knot span, joined at double knots: G2, but where two arcs differ its
/// speed jumps (C0 in its parameter).
fn arcs(arcs_deg: &[f64]) -> Curve {
    arcs_on_knots(arcs_deg, 0.0, 1.0)
}

/// [`arcs`] with the knot `k` at `start + k * step`.
fn arcs_on_knots(arcs_deg: &[f64], first: f64, step: f64) -> Curve {
    let mut start = 0.0_f64;
    let mut cpw = vec![CPoint::new(RADIUS, 0.0, 0.0, 1.0)];
    let mut knots = vec![first; 3];
    for (i, deg) in arcs_deg.iter().enumerate() {
        let half = deg.to_radians() / 2.0;
        let mid = start + half;
        let end = start + 2.0 * half;
        let w = half.cos();
        // the middle point at RADIUS / w, weighted by w
        cpw.push(CPoint::new(RADIUS * mid.cos(), RADIUS * mid.sin(), 0.0, w));
        cpw.push(CPoint::new(
            RADIUS * end.cos(),
            RADIUS * end.sin(),
            0.0,
            1.0,
        ));
        let k = first + (i + 1) as f64 * step;
        knots.extend([k, k]);
        start = end;
    }
    knots.push(first + arcs_deg.len() as f64 * step);
    Curve::from(
        BSplineCurve::from_data(3, cpw.len() - 1, knots.len() - 1, 2, &cpw, &knots).unwrap(),
    )
}

fn holds(curve: &Curve, exact: f64) {
    holds_down_to(curve, exact, 10);
}

/// The length at 1e-3 .. `10^-tightest`, failures propagated, within the accuracy of
/// `exact`.
fn holds_down_to(curve: &Curve, exact: f64, tightest: i32) {
    let mut k = Kernel::default();
    k.fixes.integrator_propagates_failure = true;
    let interval = curve.natural_interval();
    for e in 3..=tightest {
        let accuracy = 10f64.powi(-e);
        let length = curve
            .length(&k, &interval, accuracy)
            .unwrap_or_else(|err| panic!("at accuracy {accuracy:e}: {err:?}"));
        let rel = (length - exact) / exact;
        assert!(
            rel.abs() <= accuracy,
            "at accuracy {accuracy:e}: length {length} is off by {rel:e}"
        );
    }
}

/// One 120 degree arc: a single smooth span.
#[test]
fn a_smooth_arc_holds_every_requested_accuracy() {
    holds(&arcs(&[120.0]), RADIUS * 120f64.to_radians());
}

/// The parabola `(t, t^2)` for `t` in `[0, 10]`, one quadratic span: its speed
/// `sqrt(1 + 4 t^2)` turns from flat to linear near the vertex.
#[test]
fn a_parabola_holds_every_requested_accuracy() {
    let a = 10.0_f64;
    let cpw = [
        CPoint::new(0.0, 0.0, 0.0, 1.0),
        CPoint::new(a / 2.0, 0.0, 0.0, 1.0),
        CPoint::new(a, a * a, 0.0, 1.0),
    ];
    let knots = [0.0, 0.0, 0.0, 1.0, 1.0, 1.0];
    let curve = Curve::from(BSplineCurve::from_data(3, 2, 5, 2, &cpw, &knots).unwrap());
    let exact = a * (1.0 + 4.0 * a * a).sqrt() / 2.0 + (2.0 * a).asinh() / 4.0;
    holds(&curve, exact);
}

/// A full circle of six unequal arcs, its speed jumping at each of five double knots.
#[test]
fn a_circle_of_unequal_arcs_holds_every_requested_accuracy() {
    holds(
        &arcs(&[30.0, 50.0, 70.0, 40.0, 80.0, 90.0]),
        RADIUS * 2.0 * std::f64::consts::PI,
    );
}

/// The same circle on knots offset to straddle 0: a final step from `x` to a knot `b`
/// with `x < b / 2` has its last node `x + (b - x)`, which can round past the knot
/// (`-0.1 + 0.4` is `0.30000000000000004`); the speed was then read beyond the span
/// (an error, and the length refused with step control 1001, on the first two knot
/// vectors at 1e-3). The final step's last node is the span's end exactly.
#[test]
fn a_circle_on_offset_knots_holds_every_requested_accuracy() {
    let six = [30.0, 50.0, 70.0, 40.0, 80.0, 90.0];
    for (first, step) in [
        (-0.01 * 9.0 - 0.003, 0.017 * 19.0 + 0.001),
        (-0.01 * 18.0 - 0.003, 0.017 * 38.0 + 0.001),
        (-0.1, 0.4),
        (-2.7, 3.3),
        (-0.37, 0.13),
    ] {
        holds_down_to(
            &arcs_on_knots(&six, first, step),
            RADIUS * 2.0 * std::f64::consts::PI,
            12,
        );
    }
}

/// The control points of a degree-5 curve, small integers: scaled by a power of two
/// and moved by `1e6` they stay exact, so the moved curve is the same shape.
const QUINTIC: [[f64; 3]; 6] = [
    [0.0, 0.0, 0.0],
    [3.0, 1.0, 0.0],
    [5.0, 4.0, 1.0],
    [6.0, 2.0, 3.0],
    [9.0, 5.0, 2.0],
    [10.0, 7.0, 4.0],
];

/// The length of [`QUINTIC`] (one span on `[0, 1]`) by composite five-point
/// Gauss–Legendre over 400 pieces, from its hodograph `5 sum_i B_(i,4) (P_(i+1) - P_i)`
/// at the origin: exact to rounding (the rule's error is `O(h^10)`).
fn quintic_length() -> f64 {
    let s = (10.0_f64 / 7.0).sqrt();
    let inner = (5.0 - 2.0 * s).sqrt() / 3.0;
    let outer = (5.0 + 2.0 * s).sqrt() / 3.0;
    let w_inner = (322.0 + 13.0 * 70f64.sqrt()) / 900.0;
    let w_outer = (322.0 - 13.0 * 70f64.sqrt()) / 900.0;
    let rule = [
        (0.0, 128.0 / 225.0),
        (-inner, w_inner),
        (inner, w_inner),
        (-outer, w_outer),
        (outer, w_outer),
    ];
    let speed = |t: f64| {
        let b = [
            (1.0 - t).powi(4),
            4.0 * t * (1.0 - t).powi(3),
            6.0 * t * t * (1.0 - t).powi(2),
            4.0 * t.powi(3) * (1.0 - t),
            t.powi(4),
        ];
        let mut d = [0.0; 3];
        for (i, bi) in b.iter().enumerate() {
            for (c, dc) in d.iter_mut().enumerate() {
                *dc += 5.0 * bi * (QUINTIC[i + 1][c] - QUINTIC[i][c]);
            }
        }
        (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
    };
    let pieces = 400;
    let h = 1.0 / f64::from(pieces);
    (0..pieces)
        .map(|j| {
            let mid = (f64::from(j) + 0.5) * h;
            rule.iter()
                .map(|(x, w)| w * speed(mid + 0.5 * h * x))
                .sum::<f64>()
                * 0.5
                * h
        })
        .sum()
}

/// A degree-5 curve of size `2^-10` (about 1e-3) and `2^-7` placed `1e6` from the
/// origin holds every requested accuracy down to 1e-12. Its speed is evaluated in
/// difference form, `sum_i R'_i (P_i - P_0)`, so its rounding and the bound on it scale
/// with the curve's size, not its position: evaluated from the points themselves the
/// speed's rounding was about `1e-8` of it, and the step control, which accepted steps
/// down to a worst-case bound of that rounding, returned 9.8e-9 off at 1e-10.
#[test]
fn a_small_curve_far_from_the_origin_holds_every_requested_accuracy() {
    let exact = quintic_length();
    for scale in [2f64.powi(-10), 2f64.powi(-7)] {
        for offset in [0.0, 1e6] {
            let cpw: Vec<CPoint> = QUINTIC
                .iter()
                .map(|p| {
                    let q = p.map(|c| offset + scale * c);
                    // (exact: a power-of-two scale of a small integer, moved by 1e6)
                    assert_eq!(q.map(|c| (c - offset) / scale), *p);
                    CPoint::new(q[0], q[1], q[2], NOW)
                })
                .collect();
            let knots = [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0];
            let curve = Curve::from(BSplineCurve::from_data(3, 5, 11, 5, &cpw, &knots).unwrap());
            holds_down_to(&curve, scale * exact, 12);
        }
    }
}

/// The knots of [`cubic`]: two knot spans, `[0, 0.4]` and `[0.4, 1]`.
const CUBIC_KNOTS: [f64; 9] = [0.0, 0.0, 0.0, 0.0, 0.4, 1.0, 1.0, 1.0, 1.0];

/// A cubic of two knot spans through the control points `points`, weighted by `w`
/// (stored homogeneous) or non-rational for `None`.
fn cubic(points: &[[f64; 3]; 5], w: Option<[f64; 5]>) -> Curve {
    let cpw: Vec<CPoint> = match w {
        Some(w) => points
            .iter()
            .zip(w)
            .map(|(p, w)| CPoint::new(p[0] * w, p[1] * w, p[2] * w, w))
            .collect(),
        None => points
            .iter()
            .map(|p| CPoint::new(p[0], p[1], p[2], NOW))
            .collect(),
    };
    Curve::from(BSplineCurve::from_data(3, 4, 8, 3, &cpw, &CUBIC_KNOTS).unwrap())
}

/// Lengths at 1e-3 .. 1e-12, by default and with failures propagated.
fn lengths(curve: &Curve) -> Vec<(f64, bool, f64)> {
    let mut out = Vec::new();
    for propagate in [false, true] {
        let mut k = Kernel::default();
        k.fixes.integrator_propagates_failure = propagate;
        for e in 3..=12 {
            let accuracy = 10f64.powi(-e);
            let length = curve
                .length(&k, &curve.natural_interval(), accuracy)
                .unwrap_or_else(|err| panic!("at {accuracy:e}, propagate {propagate}: {err:?}"));
            out.push((accuracy, propagate, length));
        }
    }
    out
}

/// A curve whose control points are all one point has length 0, exactly, at every
/// accuracy and whether failures propagate or not. Its computed speed is rounding noise
/// around zero, which the steps used to refuse at 1e-9 (step control 1001: 0 by
/// default, an error with failures propagated). Weights that are powers of two keep the
/// rational points exactly equal.
#[test]
fn a_point_curve_has_length_zero() {
    for m in [1e-3, 1.0, 1e6] {
        let p = [0.3 * m, -0.7 * m, 0.11 * m];
        for w in [None, Some([1.0, 0.5, 2.0, 0.25, 1.0])] {
            for (accuracy, propagate, length) in lengths(&cubic(&[p; 5], w)) {
                assert_eq!(
                    length, 0.0,
                    "|P| ~ {m:e}, weights {w:?}, at {accuracy:e} (propagate {propagate})"
                );
            }
        }
    }
}

/// `a b - c d` to `2u` relative (Kahan's algorithm with a fused multiply-add).
fn difference_of_products(a: f64, b: f64, c: f64, d: f64) -> f64 {
    let cd = c * d;
    a.mul_add(b, -cd) - c.mul_add(d, -cd)
}

/// A curve whose control points differ by a few ulps is a point to rounding: its length
/// is at most its control polygon's, a few ulps, plus the rounding of its computed speed
/// (`SpanSpeed::rounding_bound` over each knot span), at every accuracy. Rational
/// weights that are not powers of two make the stored homogeneous points `P_i w_i`
/// round, so the curve is that of the stored points: its polygon is measured from them,
/// `P_(i+1) - P_i = (h_(i+1) w_i - h_i w_(i+1)) / (w_i w_(i+1))`, to a few ulps of itself.
#[test]
fn a_near_point_curve_has_length_within_its_rounding() {
    for m in [1e-3, 1.0, 1e6] {
        let base: [f64; 3] = [0.3 * m, -0.7 * m, 0.11 * m];
        let mut points = [base; 5];
        for (i, q) in points.iter_mut().enumerate() {
            for _ in 0..i {
                q[0] = q[0].next_up();
                q[2] = q[2].next_down();
            }
        }
        for w in [None, Some([1.0, 0.7, 1.3, 0.9, 1.0])] {
            let curve = cubic(&points, w);
            // the stored points, homogeneous, as `cubic` stores them
            let weights = w.unwrap_or([1.0; 5]);
            let stored: Vec<([f64; 3], f64)> = points
                .iter()
                .zip(weights)
                .map(|(p, w)| (p.map(|c| c * w), w))
                .collect();
            let polygon: f64 = stored
                .iter()
                .zip(stored.iter().skip(1))
                .map(|((ha, wa), (hb, wb))| {
                    let d: Vec<f64> = (0..3)
                        .map(|c| difference_of_products(hb[c], *wa, ha[c], *wb) / (wa * wb))
                        .collect();
                    (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
                })
                .sum::<f64>()
                * (1.0 + 1e-12);
            let nurb = curve.b_spline_base().unwrap();
            let rounding: f64 = [(0.0, 0.4), (0.4, 1.0)]
                .iter()
                .map(|&(a, b)| {
                    let span = Extent1d::from_min_max(a, b);
                    nurb.span_speed(&span)
                        .unwrap()
                        .map_or(0.0, |s| s.rounding_bound())
                        * (b - a)
                })
                .sum();
            for (accuracy, propagate, length) in lengths(&curve) {
                assert!(
                    (0.0..=polygon + rounding).contains(&length),
                    "|P| ~ {m:e}, weights {w:?}, at {accuracy:e} (propagate {propagate}): \
                     length {length:e} beyond polygon {polygon:e} + rounding {rounding:e}"
                );
            }
        }
    }
}

/// The parabola `(x, x^2)`, `0 <= x <= a`, as a rational quadratic whose weights
/// `1, w, w^2` reparametrise it by `x(t) = a w^2 t / (1 + (w^2 - 1) t)`: the same curve,
/// so the same length, whatever `w`. Stored homogeneous, its points are exact.
fn rational_parabola(a: f64, w: f64) -> Curve {
    let cpw = [
        CPoint::new(0.0, 0.0, 0.0, 1.0),
        CPoint::new(w * a / 2.0, 0.0, 0.0, w),
        CPoint::new(w * w * a, w * w * a * a, 0.0, w * w),
    ];
    let knots = [0.0, 0.0, 0.0, 1.0, 1.0, 1.0];
    Curve::from(BSplineCurve::from_data(3, 2, 5, 2, &cpw, &knots).unwrap())
}

/// Rational reparametrisations of the parabola hold every requested accuracy down to
/// 1e-12, as the polynomial one does. At `a = 16` with weights `1, 4, 16` the length was
/// 1.16e-12 off at 1e-12.
#[test]
fn a_rationally_parametrised_parabola_holds_every_requested_accuracy() {
    for a in [1.0_f64, 4.0, 10.0, 16.0] {
        let exact = a * (1.0 + 4.0 * a * a).sqrt() / 2.0 + (2.0 * a).asinh() / 4.0;
        for w in [1.0, 2.0, 4.0, 0.5, 0.25] {
            holds_down_to(&rational_parabola(a, w), exact, 12);
        }
    }
}
