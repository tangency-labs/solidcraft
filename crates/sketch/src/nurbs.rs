//! A control spline's own knots and weights ([`Nurbs`]): the exact form of a free-form edge a
//! sketch projects, whose knots need not be evenly spaced and whose weights need not be 1. A
//! control spline without them is the clamped uniform B-spline over its points.

use serde::{Deserialize, Serialize};
use solidcore_record::{Seg2, Vec2, Vec3};
use solidcraft_geom::bspline::{self, HPoint};
use solidcraft_geom::prelude::*;

use crate::model::{Result, SketchError};

/// Most knots a spline may have (its points are capped at [`crate::MAX_SPLINE_POINTS`]).
const MAX_KNOTS: usize = crate::curves::MAX_SPLINE_POINTS + solidcore_record::MAX_BEZIER_DEGREE + 1;
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
    /// The existing fit curve as exact control splines. Chord knots compact its C2 spans;
    /// very ill-conditioned joins retain their original Bezier controls instead.
    pub fn from_fit_points(points: &[Vec2], periodic: bool) -> Result<Vec<(Vec<Vec2>, Nurbs)>> {
        if !(2..=crate::MAX_SPLINE_POINTS).contains(&points.len()) || points.iter().any(|p| !p.is_finite()) {
            return bad("fit points must be finite and within the spline point limit");
        }
        let h: Vec<f64> = points.windows(2).map(|p| p[0].dist(p[1]).max(1e-9)).collect();
        if h.iter().any(|h| !h.is_finite()) {
            return bad("fit chord lengths are not finite");
        }
        let spans = crate::fit_beziers_mode(points, periodic)?;
        if spans.len() != h.len() || spans.iter().flatten().any(|p| !p.is_finite()) {
            return bad("fit spans cannot be represented accurately");
        }
        let mut out = Vec::new();
        for (spans, h) in spans.chunks(crate::MAX_SPLINE_POINTS - 3).zip(h.chunks(crate::MAX_SPLINE_POINTS - 3)) {
            if let Some(compact) = Self::compact_fit(spans, h) {
                out.push(compact);
                continue;
            }
            for spans in spans.chunks((crate::MAX_SPLINE_POINTS - 1) / 3) {
                let mut controls = Vec::with_capacity(spans.len() * 3 + 1);
                let mut knots = vec![0.0; 4];
                for (i, [a, b, c, d]) in spans.iter().enumerate() {
                    if i == 0 {
                        controls.push(*a);
                    }
                    controls.extend([*b, *c, *d]);
                    knots.extend(std::iter::repeat_n((i + 1) as f64, if i + 1 == spans.len() { 4 } else { 3 }));
                }
                let nurbs = Nurbs { knots, weights: Vec::new() };
                nurbs.check(controls.len(), 3)?;
                out.push((controls, nurbs));
            }
        }
        Ok(out)
    }

    fn compact_fit(spans: &[[Vec2; 4]], h: &[f64]) -> Option<(Vec<Vec2>, Nurbs)> {
        let (first, last) = (spans.first()?, spans.last()?);
        let mut knots = vec![0.0; 4];
        let mut widths = Vec::with_capacity(h.len());
        let mut t = 0.0;
        for (i, length) in h.iter().enumerate() {
            let next = t + length;
            if !(next > t && next.is_finite()) {
                return None;
            }
            widths.push(next - t);
            t = next;
            knots.extend(std::iter::repeat_n(t, if i + 1 == h.len() { 4 } else { 1 }));
        }
        let mut controls = vec![first[0], first[1]];
        for (pair, h) in spans.windows(2).zip(widths.windows(2)) {
            // Cubic blossom at the three neighbouring knots. Extrapolating from the
            // longer span keeps the coefficient ratio <= 1 even for uneven chords.
            let (b, c, ratio) = if h[1] >= h[0] { (pair[1][1], pair[1][2], h[0] / h[1]) } else { (pair[0][2], pair[0][1], h[1] / h[0]) };
            controls.push(b + (b - c) * ratio);
        }
        controls.extend([last[2], last[3]]);
        let nurbs = Nurbs { knots, weights: Vec::new() };
        nurbs.check(controls.len(), 3).ok()?;
        // Reconstruct the Bezier coefficients in linear time. Their largest difference
        // bounds the difference of the entire curves by the convex hull property.
        let inner: Vec<[Vec2; 2]> = widths
            .iter()
            .enumerate()
            .map(|(i, b)| {
                let a = i.checked_sub(1).and_then(|j| widths.get(j)).copied().unwrap_or(0.0);
                let c = widths.get(i + 1).copied().unwrap_or(0.0);
                let sum = a + b + c;
                [controls[i + 1].lerp(controls[i + 2], a / sum), controls[i + 2].lerp(controls[i + 1], c / sum)]
            })
            .collect();
        let scale = spans.iter().flatten().fold(1.0_f64, |s, p| s.max(p.x.abs()).max(p.y.abs()));
        let tolerance = scale * f64::EPSILON * 128.0;
        let certified = spans.iter().enumerate().all(|(i, expected)| {
            let [b, c] = inner[i];
            let a = if i == 0 { first[0] } else { inner[i - 1][1].lerp(b, widths[i - 1] / (widths[i - 1] + widths[i])) };
            let d = if i + 1 == spans.len() { last[3] } else { c.lerp(inner[i + 1][0], widths[i] / (widths[i] + widths[i + 1])) };
            [a, b, c, d].iter().zip(expected).all(|(a, b)| a.dist(*b) <= tolerance)
        });
        certified.then_some((controls, nurbs))
    }

    /// Whether these knots and weights fit a control spline of `degree` over `n` points.
    pub fn check(&self, n: usize, degree: u8) -> Result<()> {
        let d = usize::from(degree);
        if !(1..=solidcore_record::MAX_BEZIER_DEGREE).contains(&d) || n <= d || n > crate::curves::MAX_SPLINE_POINTS {
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
        // A common weight scale has no geometric meaning. Normalize before multiplying
        // coordinates so finite accepted weights cannot overflow or underflow the controls.
        let max = self.weights.iter().copied().reduce(f64::max).unwrap_or(1.0);
        p.iter().enumerate().map(|(i, q)| HPoint::weighted(Vec3::new(q.x, q.y, 0.0), self.weight(i) / max)).collect()
    }

    /// The knot domain `[lo, hi]` for `n` points of `degree`.
    fn domain(&self, n: usize, degree: usize) -> Option<(f64, f64)> {
        Some((*self.knots.get(degree)?, *self.knots.get(n)?))
    }

    /// The same curve run the other way: negate and reverse the knots and reverse weights
    /// (the points reverse with them). Reflection about zero preserves every knot exactly,
    /// including intervals too narrow to survive subtraction from a large domain endpoint.
    pub fn reversed(&self) -> Nurbs {
        Nurbs { knots: self.knots.iter().rev().map(|k| -k).collect(), weights: self.weights.iter().rev().copied().collect() }
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

    /// A point or the complete line interval when every control lies on one line
    /// within affine-map roundoff. `None` retains a genuinely two-dimensional curve.
    pub fn linear_image(&self, p: &[Vec2], degree: u8, roundoff: f64) -> Result<Option<[Vec2; 2]>> {
        self.check(p.len(), degree)?;
        if !(roundoff.is_finite() && roundoff >= 0.0) || p.iter().any(|p| !p.is_finite()) {
            return bad("invalid projected controls or roundoff budget");
        }
        let origin = *p.first().ok_or_else(|| SketchError::Invalid("empty spline".into()))?;
        let far = p.iter().copied().max_by(|a, b| a.dist(origin).total_cmp(&b.dist(origin))).unwrap_or(origin);
        if far.dist(origin) <= roundoff {
            return Ok(Some([origin; 2]));
        }
        let length = far.dist(origin);
        if !(length.is_finite() && length > 0.0) {
            return bad("invalid projected direction");
        }
        let axis = (far - origin) / length;
        if p.iter().any(|p| axis.cross(*p - origin).abs() > roundoff) {
            return Ok(None);
        }
        // Translate before extraction: range arithmetic depends on extent rather than the
        // curve's world position. Boehm insertion is a convex combination for positive weights.
        let scalar: Vec<_> = p.iter().map(|p| Vec2::new((*p - origin).dot(axis), 0.0)).collect();
        let scale = scalar.iter().fold(1.0_f64, |s, p| s.max(p.x.abs()));
        let accuracy = (scale * f64::EPSILON * 8192.0).max(1e-10);
        let spans = self.spans(&scalar, degree).ok_or_else(|| SketchError::Invalid("cannot extract projected spline spans".into()))?;
        let mut work = (64 * spans.len() + 4096) * (usize::from(degree) + 1).pow(2);
        let mut range = [f64::INFINITY, f64::NEG_INFINITY];
        for span in spans {
            let r = bspline::bezier_scalar_range(&span, accuracy, &mut work).map_err(|e| SketchError::Invalid(e.to_string()))?;
            range = [range[0].min(r[0]), range[1].max(r[1])];
        }
        // Reserve a second accuracy allowance for scalar mapping and positive-weight
        // Boehm extraction (degree <= 9), outside the directed subdivision arithmetic.
        Ok(Some([origin + axis * (range[0] - accuracy), origin + axis * (range[1] + accuracy)]))
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
            if let crate::CurveKind::Spline { pts, control, degree, periodic, nurbs: Some(nurbs), .. } = &curve.kind {
                if !*control || *periodic {
                    return bad("only an open control spline has its own knots");
                }
                nurbs.check(pts.len(), *degree)?;
            }
        }
        Ok(())
    }
}
