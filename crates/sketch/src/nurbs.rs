//! A control spline's own knots and weights ([`Nurbs`]): the exact form of a free-form edge a
//! sketch projects, whose knots need not be evenly spaced and whose weights need not be 1. A
//! control spline without them is the clamped uniform B-spline over its points.

use serde::{Deserialize, Serialize};
use solidcraft_geom::bspline::{self, HPoint};
use solidcraft_geom::{Seg2, Vec2, Vec3};

use crate::model::{Result, SketchError};

/// Most knots a spline may have (its points are capped at [`crate::MAX_SPLINE_POINTS`]).
const MAX_KNOTS: usize = crate::curves::MAX_SPLINE_POINTS + solidcraft_geom::MAX_BEZIER_DEGREE + 1;
/// Most a weight may exceed another by: past it the curve is all but its heaviest points.
const MAX_WEIGHT_RATIO: f64 = 1e12;
/// Polyline samples per knot span.
const SPAN_SAMPLES: usize = 24;

/// The knot vector and weights of a control spline of `degree` over `n` points: clamped (each
/// end knot `degree + 1` times, no interior knot more than `degree` times), `n + degree + 1`
/// non-decreasing knots, and one positive weight per point (none: all 1).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Nurbs {
    pub knots: Vec<f64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub weights: Vec<f64>,
}

fn bad<T>(why: &str) -> Result<T> {
    Err(SketchError::Invalid(format!("spline knots: {why}")))
}

impl Nurbs {
    /// Whether these knots and weights fit a control spline of `degree` over `n` points.
    pub fn check(&self, n: usize, degree: u8) -> Result<()> {
        let d = usize::from(degree);
        if !(1..=solidcraft_geom::MAX_BEZIER_DEGREE).contains(&d) || n <= d || n > crate::curves::MAX_SPLINE_POINTS {
            return bad("a knotted spline needs more points than its degree, 1 to 9");
        }
        let k = &self.knots;
        if k.len() != n + d + 1 || k.len() > MAX_KNOTS {
            return bad("a spline needs its point count plus its degree plus one knots");
        }
        if k.iter().any(|x| !x.is_finite()) || k.windows(2).any(|w| w[1] < w[0]) {
            return bad("knots must be finite and never decrease");
        }
        let (lo, hi) = (k[d], k[n]);
        let span = hi - lo;
        if !(span > 0.0 && span.is_finite()) || k[..=d].iter().any(|x| *x != lo) || k[n..].iter().any(|x| *x != hi) {
            return bad("the knots must be clamped at both ends");
        }
        let mut run = 0usize;
        for (i, x) in k.iter().enumerate().take(n).skip(d + 1) {
            run = if k.get(i - 1) == Some(x) { run + 1 } else { 1 };
            if *x == lo || *x == hi || run > d {
                return bad("no knot inside the spline may repeat more than its degree");
            }
        }
        if !self.weights.is_empty() {
            if self.weights.len() != n || self.weights.iter().any(|w| !(w.is_finite() && *w > 0.0)) {
                return bad("a rational spline needs one positive weight per point");
            }
            let (min, max) = self.weights.iter().fold((f64::INFINITY, 0.0f64), |(a, b), w| (a.min(*w), b.max(*w)));
            if max / min > MAX_WEIGHT_RATIO {
                return bad("the weights differ too much");
            }
        }
        Ok(())
    }

    fn weight(&self, i: usize) -> f64 {
        self.weights.get(i).copied().unwrap_or(1.0)
    }

    /// The control points `p` with their weights, homogeneous.
    fn homogeneous(&self, p: &[Vec2]) -> Vec<HPoint> {
        p.iter().enumerate().map(|(i, q)| HPoint::weighted(Vec3::new(q.x, q.y, 0.0), self.weight(i))).collect()
    }

    /// The knot domain `[lo, hi]` for `n` points of `degree`.
    fn domain(&self, n: usize, degree: usize) -> Option<(f64, f64)> {
        Some((*self.knots.get(degree)?, *self.knots.get(n)?))
    }

    /// The same curve run the other way: the knots mirrored, the weights reversed (the points
    /// reverse with them).
    pub fn reversed(&self) -> Nurbs {
        let sum = self.knots.first().copied().unwrap_or(0.0) + self.knots.last().copied().unwrap_or(0.0);
        Nurbs { knots: self.knots.iter().rev().map(|k| sum - k).collect(), weights: self.weights.iter().rev().copied().collect() }
    }

    /// The point of the spline over `p` (of `degree`) at `t` in [0, 1] across its knot domain.
    /// `None` when the knots don't fit the points.
    pub fn point(&self, p: &[Vec2], degree: u8, t: f64) -> Option<Vec2> {
        self.check(p.len(), degree).ok()?;
        let d = usize::from(degree);
        let (lo, hi) = self.domain(p.len(), d)?;
        self.eval(&self.homogeneous(p), d, lo + (hi - lo) * t.clamp(0.0, 1.0))
    }

    /// The point at knot parameter `u` of the spline of degree `d` over the homogeneous points
    /// `h` (de Boor), the knots already checked.
    fn eval(&self, h: &[HPoint], d: usize, u: f64) -> Option<Vec2> {
        let k = &self.knots;
        let n = h.len();
        // The span holding u: the last knot ≤ u that starts a non-empty span.
        let mut s = d;
        while s + 1 < n && k.get(s + 1).is_some_and(|x| *x <= u) {
            s += 1;
        }
        let mut c: Vec<HPoint> = h.get(s - d..=s)?.to_vec();
        for r in 1..=d {
            for j in (r..=d).rev() {
                let i = s + j - d;
                let (a, b) = (k.get(i).copied()?, k.get(i + d + 1 - r).copied()?);
                let alpha = if b > a { (u - a) / (b - a) } else { 0.0 };
                c[j] = c[j - 1] * (1.0 - alpha) + c[j] * alpha;
            }
        }
        let q = c.get(d)?.project()?;
        Some(Vec2::new(q.x, q.y))
    }

    /// The Bézier spans of the spline over `p`, homogeneous, in parameter order: every
    /// interior knot inserted until it repeats `degree` times (Boehm). Exact up to rounding.
    fn spans(&self, p: &[Vec2], degree: u8) -> Option<Vec<Vec<HPoint>>> {
        self.check(p.len(), degree).ok()?;
        let d = usize::from(degree);
        let (mut k, mut c) = (self.knots.clone(), self.homogeneous(p));
        let (lo, hi) = self.domain(p.len(), d)?;
        let mut interior: Vec<f64> = self.knots.iter().copied().filter(|x| *x > lo && *x < hi).collect();
        interior.dedup();
        for u in interior {
            while k.iter().filter(|x| **x == u).count() < d {
                (k, c) = bspline::insert_knot(d, &k, &c, u).ok()?;
            }
        }
        let mut spans = Vec::new();
        for i in d..c.len() {
            if k.get(i)? < k.get(i + 1)? {
                spans.push(c.get(i - d..=i)?.to_vec());
            }
        }
        Some(spans)
    }

    /// The spline over `p` as its exact [`Seg2`] pieces, one a knot span: Béziers, or conics
    /// for a rational quadratic. `None` for other rational splines, which have no such pieces.
    pub fn segs(&self, p: &[Vec2], degree: u8) -> Option<Vec<Seg2>> {
        let spans = self.spans(p, degree)?;
        let rational = self.weights.windows(2).any(|w| w[0] != w[1]);
        let flat = |q: &HPoint| q.project().map(|v| Vec2::new(v.x, v.y));
        spans
            .iter()
            .map(|span| {
                let pts: Vec<Vec2> = span.iter().map(flat).collect::<Option<_>>()?;
                match (rational, pts.as_slice()) {
                    (false, _) => solidcraft_geom::bezier(&pts),
                    (true, [a, apex, b]) => {
                        let w = span.get(1)?.w / (span.first()?.w * span.get(2)?.w).sqrt();
                        (w.is_finite() && w > 0.0).then_some(Seg2::Conic { a: *a, apex: *apex, b: *b, w })
                    }
                    _ => None,
                }
            })
            .collect()
    }

    /// The spline over `p`, sampled evenly within each knot span.
    pub fn polyline(&self, p: &[Vec2], degree: u8) -> Vec<Vec2> {
        let d = usize::from(degree);
        let Some((lo, hi)) = self.check(p.len(), degree).ok().and_then(|()| self.domain(p.len(), d)) else { return Vec::new() };
        let mut breaks: Vec<f64> = self.knots.iter().copied().filter(|x| *x >= lo && *x <= hi).collect();
        breaks.dedup();
        let h = self.homogeneous(p);
        let mut out = Vec::with_capacity(breaks.len() * SPAN_SAMPLES + 1);
        for w in breaks.windows(2) {
            for i in 0..SPAN_SAMPLES {
                out.extend(self.eval(&h, d, w[0] + (w[1] - w[0]) * i as f64 / SPAN_SAMPLES as f64));
            }
        }
        out.extend(self.eval(&h, d, hi));
        out
    }
}

impl crate::Sketch {
    /// Every spline's own knots and weights fit it: on an open control spline, the right count,
    /// clamped, with positive weights (a loaded design is checked before it is used).
    pub fn validate_spline_knots(&self) -> Result<()> {
        for curve in &self.curves {
            if let crate::CurveKind::Spline { pts, control, degree, periodic, nurbs: Some(nurbs) } = &curve.kind {
                if !*control || *periodic {
                    return bad("only an open control spline has its own knots");
                }
                nurbs.check(pts.len(), *degree)?;
            }
        }
        Ok(())
    }
}
