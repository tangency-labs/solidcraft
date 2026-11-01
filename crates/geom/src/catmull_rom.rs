//! The centripetal Catmull–Rom spline through fit points: the smooth curve a 3D sketch draws
//! through the points picked for it (no overshoot loops, no cusps).
//!
//! Between fit points `P1` and `P2` the curve is the cubic of the Barry–Goldman pyramid over the
//! four points `P0…P3` around them, at the knots `t0 = 0`, `t(i+1) = t(i) + |P(i+1) - P(i)|^½`,
//! run from `t1` to `t2` (Barry and Goldman, "A recursive evaluation algorithm for a class of
//! Catmull–Rom splines", SIGGRAPH 1988; the centripetal exponent ½ after Yuksel, Schaefer and
//! Keyser, "Parameterization and applications of Catmull–Rom curves", CAD 43(7), 2011). Its
//! derivative at `P1`,
//!
//! `m1 = (P1 - P0)/(t1 - t0) - (P2 - P0)/(t2 - t0) + (P2 - P1)/(t2 - t1)`,
//!
//! depends only on `P1`, its two neighbours and the two knot intervals between them, so the spans
//! on either side of a fit point leave it with the same derivative: in the parameter that runs on
//! from span to span by each span's knot interval, the spline is C1, exactly one cubic Bézier per
//! span ([`beziers`]). [`samples`] is the polyline through [`SPAN_SAMPLES`] points per span of the
//! same curve.
//!
//! Ends: an open spline mirrors its end points (`P(-1) = 2 P(0) - P(1)`, and so at the far end),
//! which leaves it leaving each end straight towards the next fit point. A spline of more than
//! three points whose last point is its first (within 1e-9) is closed: its neighbours wrap round,
//! so it is as smooth where it closes as anywhere else.

use crate::Vec3;

/// Samples per span of [`samples`].
pub const SPAN_SAMPLES: usize = 24;

/// One span: the four points around it and their knots (`p[1]` to `p[2]` is the span).
struct Span {
    p: [Vec3; 4],
    t: [f64; 4],
}

/// Whether the spline through `fit` closes on itself (see the module notes).
pub fn is_closed(fit: &[Vec3]) -> bool {
    let n = fit.len();
    n > 3 && fit.first().zip(fit.last()).is_some_and(|(a, b)| a.dist(*b) < 1e-9)
}

/// The spans of the spline through `fit` (three or more points), in order.
fn spans(fit: &[Vec3]) -> Vec<Span> {
    let n = fit.len();
    let (Some(&first), Some(&second), Some(&last), Some(&before_last)) = (fit.first(), fit.get(1), fit.last(), fit.get(n.wrapping_sub(2))) else {
        return Vec::new();
    };
    let closed = is_closed(fit);
    let at = |i: isize| -> Vec3 {
        match usize::try_from(i) {
            Ok(i) if i < n => fit.get(i).copied().unwrap_or(last),
            Ok(_) if closed => second,
            Err(_) if closed => before_last,
            // Mirrored end tangents.
            Ok(_) => last * 2.0 - before_last,
            Err(_) => first * 2.0 - second,
        }
    };
    let tj = |a: Vec3, b: Vec3| a.dist(b).sqrt().max(1e-9);
    (0..n.saturating_sub(1))
        .map(|k| {
            let k = k as isize;
            let (p0, p1, p2, p3) = (at(k - 1), at(k), at(k + 1), at(k + 2));
            let t0 = 0.0;
            let t1 = t0 + tj(p0, p1);
            let t2 = t1 + tj(p1, p2);
            let t3 = t2 + tj(p2, p3);
            Span { p: [p0, p1, p2, p3], t: [t0, t1, t2, t3] }
        })
        .collect()
}

/// The polyline of the spline through `fit`: the point itself (one point), the line (two), or
/// [`SPAN_SAMPLES`] points per span after the first point, the last at the last fit point.
pub fn samples(fit: &[Vec3]) -> Vec<Vec3> {
    if fit.len() <= 2 {
        return fit.to_vec();
    }
    let mut out = fit.first().copied().into_iter().collect::<Vec<_>>();
    for Span { p: [p0, p1, p2, p3], t: [t0, t1, t2, t3] } in spans(fit) {
        for s in 1..=SPAN_SAMPLES {
            let t = t1 + (t2 - t1) * s as f64 / SPAN_SAMPLES as f64;
            let a1 = p0 * ((t1 - t) / (t1 - t0)) + p1 * ((t - t0) / (t1 - t0));
            let a2 = p1 * ((t2 - t) / (t2 - t1)) + p2 * ((t - t1) / (t2 - t1));
            let a3 = p2 * ((t3 - t) / (t3 - t2)) + p3 * ((t - t2) / (t3 - t2));
            let b1 = a1 * ((t2 - t) / (t2 - t0)) + a2 * ((t - t0) / (t2 - t0));
            let b2 = a2 * ((t3 - t) / (t3 - t1)) + a3 * ((t - t1) / (t3 - t1));
            out.push(b1 * ((t2 - t) / (t2 - t1)) + b2 * ((t - t1) / (t2 - t1)));
        }
    }
    out
}

/// The spline through `fit` (three or more points) exactly, as one cubic Bézier per span (from
/// fit point to fit point): `P1`, `P1 + m1 h/3`, `P2 - m2 h/3`, `P2` (`h = t2 - t1`, `m` the
/// derivatives in the module notes), its parameter `(t - t1) / h`. Neighbouring spans meet with
/// the same derivative in `t`, so the spans make a C1 cubic B-spline with double interior knots.
/// Empty for fewer than three points (a point or a line); `None` for a spline that does not come
/// out finite.
pub fn beziers(fit: &[Vec3]) -> Option<Vec<[Vec3; 4]>> {
    if fit.len() < 3 {
        return Some(Vec::new());
    }
    let out: Vec<[Vec3; 4]> = spans(fit)
        .into_iter()
        .map(|Span { p: [p0, p1, p2, p3], t: [t0, t1, t2, t3] }| {
            let h = t2 - t1;
            let m1 = (p1 - p0) * (1.0 / (t1 - t0)) - (p2 - p0) * (1.0 / (t2 - t0)) + (p2 - p1) * (1.0 / h);
            let m2 = (p2 - p1) * (1.0 / h) - (p3 - p1) * (1.0 / (t3 - t1)) + (p3 - p2) * (1.0 / (t3 - t2));
            [p1, p1 + m1 * (h / 3.0), p2 - m2 * (h / 3.0), p2]
        })
        .collect();
    out.iter().flatten().all(|p| p.is_finite()).then_some(out)
}
