//! A control spline's own knots and weights ([`Nurbs`]): the exact form of a free-form edge a
//! sketch projects, whose knots need not be evenly spaced and whose weights need not be 1. A
//! control spline without them is the clamped uniform B-spline over its points.

use serde::{Deserialize, Serialize};
use solidcore_record::{Seg2, Vec2};
use solidcraft_geom::bspline::{self, HPoint};

use crate::model::{Result, SketchError};

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
        solidcore_ops::fit_projection::fit_splines(points, periodic, crate::MAX_SPLINE_POINTS)
            .map(|curves| curves.into_iter().map(|s| (s.points, Nurbs { knots: s.knots, weights: s.weights })).collect())
            .map_err(|e| SketchError::Invalid(e.to_string()))
    }

    /// Whether these knots and weights fit a control spline of `degree` over `n` points.
    pub fn check(&self, n: usize, degree: u8) -> Result<()> {
        solidcore_ops::fit_projection::check_control(n, usize::from(degree), &self.knots, &self.weights, crate::MAX_SPLINE_POINTS)
            .map_err(|e| SketchError::Invalid(e.to_string()))
    }

    /// The control points with a common homogeneous weight normalization.
    fn homogeneous(&self, p: &[Vec2]) -> Vec<HPoint> {
        solidcore_ops::fit_projection::homogeneous_controls(p, &self.weights)
            .unwrap_or_default()
            .into_iter()
            .map(|[x, y, z, w]| HPoint { x, y, z, w })
            .collect()
    }

    /// The knot domain `[lo, hi]` for `n` points of `degree`.
    fn domain(&self, n: usize, degree: usize) -> Option<(f64, f64)> {
        Some((*self.knots.get(degree)?, *self.knots.get(n)?))
    }

    /// The same curve run the other way: negate and reverse the knots and reverse weights
    /// (the points reverse with them). Reflection about zero preserves every knot exactly,
    /// including intervals too narrow to survive subtraction from a large domain endpoint.
    pub fn reversed(&self) -> Nurbs {
        let (knots, weights) = solidcore_ops::fit_projection::reversed_metadata(&self.knots, &self.weights);
        Nurbs { knots, weights }
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
        solidcore_ops::fit_projection::linear_image(p, usize::from(degree), &self.knots, &self.weights, roundoff)
            .map_err(|e| SketchError::Invalid(e.to_string()))
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
