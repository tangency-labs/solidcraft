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
use solidcore::curve::ellipse::{Ellipse, EllipseOptions};
use solidcore::geom::extent1d::Extent1d;
use solidcore::math::{Point3, Vec3};
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
    Curve::from(arcs_nurb(arcs_deg, first, step))
}

/// The B-spline of [`arcs_on_knots`].
fn arcs_nurb(arcs_deg: &[f64], first: f64, step: f64) -> BSplineCurve {
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
    BSplineCurve::from_data(3, cpw.len() - 1, knots.len() - 1, 2, &cpw, &knots).unwrap()
}

fn holds(curve: &Curve, exact: f64) {
    holds_down_to(curve, exact, 10);
}

/// The length at 1e-3 .. `10^-tightest`, failures propagated, within the accuracy of
/// `exact`.
fn holds_down_to(curve: &Curve, exact: f64, tightest: i32) {
    let k = Kernel::default();
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

/// Lengths at 1e-3 .. 1e-12 (failures propagate).
fn lengths(curve: &Curve) -> Vec<(f64, f64)> {
    let k = Kernel::default();
    let mut out = Vec::new();
    for e in 3..=12 {
        let accuracy = 10f64.powi(-e);
        let length = curve
            .length(&k, &curve.natural_interval(), accuracy)
            .unwrap_or_else(|err| panic!("at {accuracy:e}: {err:?}"));
        out.push((accuracy, length));
    }
    out
}

/// A curve whose control points are all one point has length 0, exactly, at every
/// accuracy. Its computed speed is rounding noise around zero, which the steps used to
/// refuse at 1e-9 (step control 1001). Weights that are powers of two keep the
/// rational points exactly equal.
#[test]
fn a_point_curve_has_length_zero() {
    for m in [1e-3, 1.0, 1e6] {
        let p = [0.3 * m, -0.7 * m, 0.11 * m];
        for w in [None, Some([1.0, 0.5, 2.0, 0.25, 1.0])] {
            for (accuracy, length) in lengths(&cubic(&[p; 5], w)) {
                assert_eq!(length, 0.0, "|P| ~ {m:e}, weights {w:?}, at {accuracy:e}");
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
            for (accuracy, length) in lengths(&curve) {
                assert!(
                    (0.0..=polygon + rounding).contains(&length),
                    "|P| ~ {m:e}, weights {w:?}, at {accuracy:e}: \
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

/// The ulps a circle's length may be off by, of `size` plus its length. The angle of
/// each piece between knots is the `atan2` of its end points about the centre, each the
/// span's first control point about the centre plus the point about that control point in
/// difference form: the latter a few ulps of the radius off, so the radius times the
/// angle a few ulps of the radius, and the sum a few ulps of the length; at most six
/// pieces here. `size` is the radius where the control points and the centre are exact
/// and their differences too, and `|centre| + radius` where they are the kernel's own,
/// each rounded to ulps of its coordinates (the circle stored is that far off the exact
/// one).
const ARC_ULPS: f64 = 64.0;

/// `length` within [`ARC_ULPS`] of `size + exact` of `exact`, at every accuracy from 1e-3
/// to 1e-10.
fn is_radius_times_angle(curve: &Curve, interval: &Extent1d, exact: f64, size: f64, what: &str) {
    let k = Kernel::default();
    let bound = ARC_ULPS * f64::EPSILON * (size + exact);
    for e in 3..=10 {
        let accuracy = 10f64.powi(-e);
        let length = curve
            .length(&k, interval, accuracy)
            .unwrap_or_else(|err| panic!("{what} at accuracy {accuracy:e}: {err:?}"));
        assert!(
            (length - exact).abs() <= bound,
            "{what} at accuracy {accuracy:e}: length {length} is off {exact} by {:e} (bound {bound:e})",
            length - exact
        );
    }
}

/// A circle's arc is its radius times its angle, to rounding, whatever accuracy is asked:
/// full circles and arcs across the angle 0, small and large, with the B-spline running
/// along or against the angle (`inside_out`), in a tilted plane away from the origin.
/// The quadrature's estimate was only within the accuracy asked. (The B-spline is made
/// here, its control points rounded at their distance from the origin.)
#[test]
fn a_circular_arc_is_its_radius_times_its_angle() {
    let k = Kernel::default();
    let center = Point3::new(3.0, -7.0, 11.0);
    let x = Vec3::new(1.0, 2.0, 2.0) / 3.0;
    let y = Vec3::new(2.0, 1.0, -2.0) / 3.0;
    for radius in [5.0, 1e-3, 1e4] {
        for (lo, hi) in [
            (0.0, 360.0),
            (30.0, 250.0),
            (-90.0, 45.0),
            (0.0, 1.0),
            (10.0, 359.0),
        ] {
            for inside_out in [false, true] {
                let opts = EllipseOptions {
                    inside_out,
                    ..EllipseOptions::default()
                };
                let domain = Extent1d::from_min_max(lo, hi);
                let circle = Ellipse::new_circle(&k, center, x, y, domain, radius, &opts).unwrap();
                let curve = Curve::from(circle);
                let exact = radius * (hi - lo).to_radians();
                let what = format!("r {radius}, [{lo}, {hi}] deg, inside out {inside_out}");
                let size = center.length() + radius;
                is_radius_times_angle(&curve, &curve.natural_interval(), exact, size, &what);
            }
        }
    }
}

/// A piece of a circle is its radius times the angle of that piece: on a circle whose
/// B-spline is six unequal arcs, one per knot span (each span's middle parameter is
/// the middle of its arc, the arc being symmetric in its weights), every interval from
/// a knot to a later knot or to a span's middle.
#[test]
fn a_piece_of_a_circle_is_its_radius_times_its_angle() {
    let k = Kernel::default();
    let six = [30.0, 50.0, 70.0, 40.0, 80.0, 90.0];
    let (first, step) = (-0.37, 0.13);
    let nurb = arcs_nurb(&six, first, step);
    let opts = EllipseOptions {
        nurb: Some(&nurb),
        ..EllipseOptions::default()
    };
    let domain = Extent1d::from_min_max(0.0, 360.0);
    let circle = Ellipse::new_circle(
        &k,
        Point3::new(0.0, 0.0, 0.0),
        Vec3::new(1.0, 0.0, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
        domain,
        RADIUS,
        &opts,
    )
    .unwrap();
    let curve = Curve::from(circle);
    // the circle keeps the B-spline given, so its knots are those of the arcs
    assert_eq!(curve.natural_interval(), nurb.natural_interval());
    let knot = |i: usize| first + i as f64 * step;
    let angle_to = |i: usize| six.iter().take(i).sum::<f64>();
    for i in 0..six.len() {
        for j in i + 1..=six.len() {
            let interval = Extent1d::from_min_max(knot(i), knot(j));
            let exact = RADIUS * (angle_to(j) - angle_to(i)).to_radians();
            let what = format!("knots {i} to {j}");
            is_radius_times_angle(&curve, &interval, exact, 0.0, &what);
            let middle = (knot(j - 1) + knot(j)) / 2.0;
            let interval = Extent1d::from_min_max(knot(i), middle);
            let exact = RADIUS * (angle_to(j - 1) + six[j - 1] / 2.0 - angle_to(i)).to_radians();
            let what = format!("knot {i} to span {j}'s middle");
            is_radius_times_angle(&curve, &interval, exact, 0.0, &what);
        }
    }
}

/// A circle of radius `radius` about `center` in the XY plane, three 120 degree arcs on
/// the knots `-0.37 + 0.13 k`, middle weights 1/2. Its control points are on a grid of
/// `2^-40`, so that with a centre of whole numbers below `2^10` every coordinate, and its
/// product by the weight, is exact: the circle moved is the same B-spline, moved.
fn grid_circle(radius: f64, center: [f64; 3]) -> (Curve, Vec<f64>) {
    let k = Kernel::default();
    let grid = |v: f64| (v * 2f64.powi(40)).round() / 2f64.powi(40);
    let mut cpw = Vec::new();
    for i in 0..7 {
        let angle = (60.0 * f64::from(i)).to_radians();
        // the middle points at twice the radius, weighted 1/2
        let (r, w) = if i % 2 == 1 {
            (2.0 * radius, 0.5)
        } else {
            (radius, 1.0)
        };
        let p = [
            center[0] + grid(r * angle.cos()),
            center[1] + grid(r * angle.sin()),
            center[2],
        ];
        cpw.push(CPoint::new(p[0] * w, p[1] * w, p[2] * w, w));
    }
    let knots: Vec<f64> = [0, 0, 0, 1, 1, 2, 2, 3, 3, 3]
        .iter()
        .map(|&i| -0.37 + 0.13 * f64::from(i))
        .collect();
    let nurb = BSplineCurve::from_data(3, 6, 9, 2, &cpw, &knots).unwrap();
    let opts = EllipseOptions {
        nurb: Some(&nurb),
        ..EllipseOptions::default()
    };
    let circle = Ellipse::new_circle(
        &k,
        Point3::new(center[0], center[1], center[2]),
        Vec3::new(1.0, 0.0, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
        Extent1d::from_min_max(0.0, 360.0),
        radius,
        &opts,
    )
    .unwrap();
    let curve = Curve::from(circle);
    // the circle keeps the B-spline given
    assert_eq!(curve.natural_interval(), nurb.natural_interval());
    let mut breaks: Vec<f64> = knots[2..8].to_vec();
    breaks.dedup();
    (curve, breaks)
}

/// A small circle far from the origin has the length of the same circle at the origin,
/// to rounding of its own size: the points come in difference form from the span's first
/// control point, not evaluated whole, whose rounding would be that of their distance
/// from the origin (here `2^20` times the radius).
#[test]
fn a_small_circle_far_from_the_origin_has_the_length_it_has_at_the_origin() {
    let radius = 2f64.powi(-10);
    let (near, breaks) = grid_circle(radius, [0.0, 0.0, 0.0]);
    let (far, _) = grid_circle(radius, [1000.0, -1000.0, 1000.0]);
    let k = Kernel::default();
    for (i, &a) in breaks.iter().enumerate() {
        for &b in &breaks[i + 1..] {
            for interval in [
                Extent1d::from_min_max(a, b),
                Extent1d::from_min_max(a, (a + b) / 2.0),
            ] {
                let near_length = near.length(&k, &interval, 1e-3).unwrap();
                let what = format!("[{}, {}]", interval.min(), interval.max());
                is_radius_times_angle(&far, &interval, near_length, radius, &what);
            }
        }
    }
}

/// An ellipse that is not a circle still integrates, and holds every requested accuracy:
/// its perimeter, by the trapezoidal rule on the periodic `sqrt(a^2 sin^2 + b^2 cos^2)`,
/// which converges geometrically (2000 points: exact to rounding).
#[test]
fn an_ellipse_holds_every_requested_accuracy() {
    let k = Kernel::default();
    let (a, b) = (5.0_f64, 3.0_f64);
    let ellipse = Ellipse::new(
        &k,
        Point3::new(3.0, -7.0, 11.0),
        Vec3::new(1.0, 2.0, 2.0) / 3.0,
        Vec3::new(2.0, 1.0, -2.0) / 3.0,
        Extent1d::from_min_max(0.0, 360.0),
        a,
        b,
        &EllipseOptions::default(),
    )
    .unwrap();
    let n = 2000;
    let h = 2.0 * std::f64::consts::PI / f64::from(n);
    let exact: f64 = (0..n)
        .map(|i| {
            let s = f64::from(i) * h;
            (a * a * s.sin().powi(2) + b * b * s.cos().powi(2)).sqrt()
        })
        .sum::<f64>()
        * h;
    holds(&Curve::from(ellipse), exact);
}
