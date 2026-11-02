//! Evaluation of portable B-rep curves: lines, circle and ellipse arcs, polynomial and rational
//! splines (fitted intersection records are ordinary splines with a stated fitting tolerance).

use super::HlrError;
use crate::Vec3;
use crate::brep::{Curve, Frame};
use crate::section::CurveAccuracy;

#[derive(Clone, Debug)]
pub(crate) enum C3 {
    Line {
        a: Vec3,
        b: Vec3,
    },
    /// origin + x*rx*cos(t) + y*ry*sin(t), t in `range`.
    Arc {
        o: Vec3,
        x: Vec3,
        y: Vec3,
        r: [f64; 2],
        range: [f64; 2],
    },
    /// Homogeneous control points `[x*w, y*w, z*w, w]` (w = 1 for polynomial splines), and the
    /// same curve as Bézier pieces (each piece's parameter span and control points; empty when
    /// it could not be split).
    Spline {
        degree: usize,
        knots: Vec<f64>,
        points: Vec<[f64; 4]>,
        pieces: Vec<([f64; 2], Vec<[f64; 4]>)>,
    },
    Polyline(Vec<Vec3>),
}

pub(crate) fn v3(p: [f64; 3]) -> Vec3 {
    Vec3::new(p[0], p[1], p[2])
}

pub(crate) fn frame_axes(f: &Frame) -> (Vec3, Vec3, Vec3) {
    let (x, y) = (v3(f.x_axis), v3(f.y_axis));
    (v3(f.origin), x, y)
}

impl C3 {
    pub fn new(c: &Curve) -> Result<(C3, CurveAccuracy), HlrError> {
        Ok(match c {
            Curve::Line { start, end } => (C3::Line { a: v3(*start), b: v3(*end) }, CurveAccuracy::Analytic),
            Curve::CircleArc { frame, radius, angle } => {
                let (o, x, y) = frame_axes(frame);
                (C3::Arc { o, x, y, r: [*radius, *radius], range: *angle }, CurveAccuracy::Analytic)
            }
            Curve::EllipseArc { frame, radii, angle } => {
                let (o, x, y) = frame_axes(frame);
                (C3::Arc { o, x, y, r: *radii, range: *angle }, CurveAccuracy::Analytic)
            }
            Curve::Polyline { points } => (C3::Polyline(points.iter().copied().map(v3).collect()), CurveAccuracy::Analytic),
            Curve::Spline { knots, points } => (spline(knots, points.iter().map(|p| [p[0], p[1], p[2], 1.]).collect())?, CurveAccuracy::Analytic),
            Curve::RationalSpline { knots, homogeneous } => (spline(knots, homogeneous.clone())?, CurveAccuracy::Analytic),
            Curve::Intersection { knots, points, fitting_tolerance, .. } => (
                spline(knots, points.iter().map(|p| [p[0], p[1], p[2], 1.]).collect())?,
                CurveAccuracy::Bounded { max_deviation_mm: *fitting_tolerance },
            ),
            // Procedural: the curve is traced on demand from its leader, which carries no bound.
            Curve::SurfaceIntersection { .. } => return Err(HlrError::Unsupported("surface intersection curve")),
        })
    }

    pub fn domain(&self) -> [f64; 2] {
        match self {
            C3::Line { .. } => [0., 1.],
            C3::Arc { range, .. } => *range,
            C3::Spline { degree, knots, points, .. } => [knots.get(*degree).copied().unwrap_or(0.), knots.get(points.len()).copied().unwrap_or(1.)],
            C3::Polyline(p) => [0., p.len().saturating_sub(1) as f64],
        }
    }

    pub fn at(&self, t: f64) -> Vec3 {
        match self {
            C3::Line { a, b } => *a + (*b - *a) * t,
            C3::Arc { o, x, y, r, .. } => *o + *x * (r[0] * t.cos()) + *y * (r[1] * t.sin()),
            C3::Spline { degree, knots, points, .. } => de_boor(*degree, knots, points, t),
            C3::Polyline(p) => {
                let n = p.len();
                if n == 0 {
                    return Vec3::ZERO;
                }
                let t = t.clamp(0., (n - 1) as f64);
                let i = (t.floor() as usize).min(n.saturating_sub(2));
                match (p.get(i), p.get(i + 1)) {
                    (Some(a), Some(b)) => a.lerp(*b, t - i as f64),
                    (Some(a), None) => *a,
                    _ => Vec3::ZERO,
                }
            }
        }
    }

    /// First derivative by a central difference scaled to the domain.
    pub fn d1(&self, t: f64) -> Vec3 {
        match self {
            C3::Line { a, b } => *b - *a,
            C3::Arc { x, y, r, .. } => *x * (-r[0] * t.sin()) + *y * (r[1] * t.cos()),
            _ => {
                let [t0, t1] = self.domain();
                let h = (t1 - t0) * 1e-6;
                let (a, b) = ((t - h).max(t0), (t + h).min(t1));
                if b - a <= 0. { Vec3::ZERO } else { (self.at(b) - self.at(a)) / (b - a) }
            }
        }
    }

    /// Parameters that split the curve into pieces each turning at most `max_turn` radians or
    /// spanning at most a quarter of the domain, plus all interior knots.
    pub fn breaks(&self) -> Vec<f64> {
        let [t0, t1] = self.domain();
        let mut out = vec![t0, t1];
        match self {
            C3::Spline { knots, .. } => out.extend(knots.iter().copied().filter(|k| *k > t0 && *k < t1)),
            C3::Polyline(p) => out.extend((1..p.len().saturating_sub(1)).map(|i| i as f64)),
            C3::Arc { .. } => {
                let n = ((t1 - t0) / (std::f64::consts::PI / 8.)).ceil().max(1.) as usize;
                out.extend((1..n).map(|i| t0 + (t1 - t0) * i as f64 / n as f64));
            }
            C3::Line { .. } => {}
        }
        out.sort_by(f64::total_cmp);
        // Only equal knots merge: a span narrower than rounding can still carry a whole arc.
        out.dedup();
        out
    }
}

fn spline(knots: &[f64], points: Vec<[f64; 4]>) -> Result<C3, HlrError> {
    let degree = knots.len().checked_sub(points.len() + 1).ok_or(HlrError::Unsupported("spline knot count"))?;
    if degree == 0 || degree > 16 || points.is_empty() {
        return Err(HlrError::Unsupported("spline degree"));
    }
    let pieces = bezier_pieces(degree, knots, &points);
    Ok(C3::Spline { degree, knots: knots.to_vec(), points, pieces })
}

/// A spline split into Bézier pieces: clamped, then every inner knot raised to the degree
/// (NURBS Book §5.2); empty when that fails.
fn bezier_pieces(p: usize, knots: &[f64], points: &[[f64; 4]]) -> Vec<([f64; 2], Vec<[f64; 4]>)> {
    use crate::bspline::{HPoint, clamp_row, insert_knot};
    let row: Vec<HPoint> = points.iter().map(|h| HPoint { x: h[0], y: h[1], z: h[2], w: h[3] }).collect();
    let Ok((mut k, mut c)) = clamp_row(p, knots, &row) else { return Vec::new() };
    let (Some(&lo), Some(&hi)) = (k.get(p), k.get(c.len())) else { return Vec::new() };
    let mut inner: Vec<f64> = k.iter().copied().filter(|x| *x > lo && *x < hi).collect();
    inner.dedup();
    for u in &inner {
        let mut guard = 0;
        while k.iter().filter(|x| *x == u).count() < p {
            guard += 1;
            let Ok((nk, nc)) = insert_knot(p, &k, &c, *u) else { return Vec::new() };
            if guard > p + 1 {
                return Vec::new();
            }
            (k, c) = (nk, nc);
        }
    }
    let breaks: Vec<f64> = std::iter::once(lo).chain(inner).chain(std::iter::once(hi)).collect();
    let mut out = Vec::with_capacity(breaks.len());
    for (j, w) in breaks.windows(2).enumerate() {
        let (Some(&s0), Some(&s1)) = (w.first(), w.get(1)) else { return Vec::new() };
        let Some(ctrl) = c.get(j * p..=j * p + p) else { return Vec::new() };
        out.push(([s0, s1], ctrl.iter().map(|h| [h.x, h.y, h.z, h.w]).collect()));
    }
    out
}

/// The Bernstein coefficients of the product of two polynomials given by theirs.
fn bernstein_product(a: &[f64], b: &[f64]) -> Vec<f64> {
    use crate::bspline::binomial;
    let (m, n) = (a.len().saturating_sub(1), b.len().saturating_sub(1));
    (0..=m + n)
        .map(|k| {
            let terms = (k.saturating_sub(n)..=k.min(m))
                .map(|i| binomial(m, i) * binomial(n, k - i) * a.get(i).copied().unwrap_or(0.) * b.get(k - i).copied().unwrap_or(0.));
            terms.sum::<f64>() / binomial(m + n, k)
        })
        .collect()
}

/// The sign a polynomial in Bernstein form keeps over its interval: +1, -1, 0 when it is zero
/// (to rounding) all along, `None` when that cannot be certified. All coefficients of one sign
/// mean the polynomial has it (their hull holds its graph); otherwise the interval is halved.
fn definite_sign(c: &[f64], depth: u32) -> Option<f64> {
    let scale = c.iter().fold(0f64, |m, x| m.max(x.abs()));
    let eps = 1e-12 * scale;
    if c.iter().all(|x| x.abs() <= eps) {
        return Some(0.);
    }
    if c.iter().all(|x| *x >= -eps) {
        return Some(1.);
    }
    if c.iter().all(|x| *x <= eps) {
        return Some(-1.);
    }
    if depth >= 16 {
        return None;
    }
    // de Casteljau at the middle.
    let (mut left, mut right, mut pts) = (Vec::with_capacity(c.len()), Vec::with_capacity(c.len()), c.to_vec());
    while let (Some(&f), Some(&l)) = (pts.first(), pts.last()) {
        left.push(f);
        right.push(l);
        pts = pts.windows(2).map(|w| 0.5 * (w.first().copied().unwrap_or(0.) + w.get(1).copied().unwrap_or(0.))).collect();
    }
    right.reverse();
    match (definite_sign(&left, depth + 1)?, definite_sign(&right, depth + 1)?) {
        (a, b) if a == b || b == 0. => Some(a),
        (0., b) => Some(b),
        _ => None,
    }
}

impl C3 {
    /// The way a spline's part `r` turns about the centre `o` in the plane of `u` and `v`:
    /// +1 or -1 when its angular speed keeps that sign all along (certified on each Bézier piece
    /// from the Bernstein coefficients of `X Y' - Y X'`, which has the speed's sign as the
    /// weights are positive), 0 when it does not turn, `None` when it turns back or this cannot
    /// be certified.
    pub fn turn_sign(&self, r: [f64; 2], o: Vec3, u: Vec3, v: Vec3) -> Option<f64> {
        use crate::bspline::{HPoint, bezier_segment};
        let C3::Spline { pieces, .. } = self else { return None };
        let mut sign = 0f64;
        for (span, ctrl) in pieces {
            let (a, b) = (r[0].max(span[0]), r[1].min(span[1]));
            let len = span[1] - span[0];
            if !(b > a && len > 0.) {
                continue;
            }
            let (fa, fb) = (((a - span[0]) / len).clamp(0., 1.), ((b - span[0]) / len).clamp(0., 1.));
            if !(fb > fa) {
                continue;
            }
            let h: Vec<HPoint> = ctrl.iter().map(|q| HPoint { x: q[0], y: q[1], z: q[2], w: q[3] }).collect();
            let seg = bezier_segment(&h, fa, fb).ok()?;
            if seg.iter().any(|q| !(q.w > 0.)) {
                return None;
            }
            let rel = |q: &HPoint| Vec3::new(q.x, q.y, q.z) - o * q.w;
            let x: Vec<f64> = seg.iter().map(|q| rel(q).dot(u)).collect();
            let y: Vec<f64> = seg.iter().map(|q| rel(q).dot(v)).collect();
            let p = x.len().saturating_sub(1);
            if p == 0 {
                return None;
            }
            let d = |c: &[f64]| {
                c.windows(2).map(|w| p as f64 * (w.get(1).copied().unwrap_or(0.) - w.first().copied().unwrap_or(0.))).collect::<Vec<f64>>()
            };
            let n: Vec<f64> = bernstein_product(&x, &d(&y)).iter().zip(bernstein_product(&y, &d(&x))).map(|(a, b)| a - b).collect();
            let s = definite_sign(&n, 0)?;
            if s != 0. {
                if sign != 0. && s != sign {
                    return None;
                }
                sign = s;
            }
        }
        Some(sign)
    }

    /// Points whose hull holds the spline's part `r`: the projected control points of its
    /// Bézier pieces cut to `r` (de Casteljau). They close in on the curve as `r` shrinks.
    /// `None` for other curves, or when a weight is not positive.
    pub fn hull(&self, r: [f64; 2]) -> Option<Vec<Vec3>> {
        use crate::bspline::{HPoint, bezier_segment};
        let C3::Spline { pieces, .. } = self else { return None };
        let mut out = Vec::new();
        for (span, ctrl) in pieces {
            let (a, b) = (r[0].max(span[0]), r[1].min(span[1]));
            let len = span[1] - span[0];
            if !(b > a && len > 0.) {
                continue;
            }
            let (fa, fb) = (((a - span[0]) / len).clamp(0., 1.), ((b - span[0]) / len).clamp(0., 1.));
            if !(fb > fa) {
                continue;
            }
            let h: Vec<HPoint> = ctrl.iter().map(|q| HPoint { x: q[0], y: q[1], z: q[2], w: q[3] }).collect();
            for q in bezier_segment(&h, fa, fb).ok()? {
                if !(q.w > 0.) {
                    return None;
                }
                out.push(q.project()?);
            }
        }
        (!out.is_empty()).then_some(out)
    }
}

/// De Boor evaluation of a (rational) B-spline at `t`, clamped to the active domain.
pub(crate) fn de_boor(p: usize, knots: &[f64], ctrl: &[[f64; 4]], t: f64) -> Vec3 {
    let n = ctrl.len();
    let (Some(&lo), Some(&hi)) = (knots.get(p), knots.get(n)) else { return Vec3::ZERO };
    let t = t.clamp(lo, hi);
    // Span k with knots[k] <= t < knots[k+1], last nonempty span at the end.
    let mut k = p;
    while k + 1 < n && knots.get(k + 1).is_some_and(|u| *u <= t) {
        k += 1;
    }
    let mut d: Vec<[f64; 4]> = (0..=p).map(|j| ctrl.get(j + k - p).copied().unwrap_or([0.; 4])).collect();
    for r in 1..=p {
        for j in (r..=p).rev() {
            let (a, b) = (knots.get(j + k - p).copied().unwrap_or(0.), knots.get(j + 1 + k - r).copied().unwrap_or(0.));
            let alpha = if b - a == 0. { 0. } else { (t - a) / (b - a) };
            let (prev, cur) = (d.get(j - 1).copied().unwrap_or([0.; 4]), d.get(j).copied().unwrap_or([0.; 4]));
            if let Some(slot) = d.get_mut(j) {
                for c in 0..4 {
                    slot[c] = (1. - alpha) * prev[c] + alpha * cur[c];
                }
            }
        }
    }
    let h = d.get(p).copied().unwrap_or([0.; 4]);
    if h[3] == 0. { Vec3::ZERO } else { Vec3::new(h[0] / h[3], h[1] / h[3], h[2] / h[3]) }
}

/// The circle a curve lies on, if it is one: centre, unit normal, radius. Checked at many
/// parameters against `tol` (relative to the radius), so rational circle splines are recognised.
pub(crate) fn as_circle(c: &C3, tol: f64) -> Option<(Vec3, Vec3, f64)> {
    match c {
        C3::Arc { o, x, y, r, .. } if (r[0] - r[1]).abs() <= tol * r[0] => Some((*o, x.cross(*y).normalized()?, r[0])),
        C3::Spline { degree, points, .. } => {
            let [t0, t1] = c.domain();
            let at = |f: f64| c.at(t0 + (t1 - t0) * f);
            let (a, b, m) = (at(0.), at(1.), at(0.5));
            // Circumcentre of a, m and a third point (b may equal a for a full circle).
            let third = if a.dist(b) < 1e-9 * (1. + a.len()) { at(0.25) } else { b };
            let (u, v) = (m - a, third - a);
            let n = u.cross(v);
            let n2 = n.len2();
            if n2 <= 1e-24 {
                return None;
            }
            let centre = a + (v * u.len2() - u * v.len2()).cross(n) * (1. / (2. * n2));
            let normal = n.normalized()?;
            let r = centre.dist(a);
            // On one knot span of a (rational) spline of degree p, f = w^2 (|P - c|^2 - r^2) is
            // a polynomial of degree 2p. At 2p + 3 Chebyshev points of the span it is at most
            // w_max^2 2 r d (d the largest distance from the circle there), so everywhere at most
            // 4 times that (their Lebesgue constant is below 4), and as w >= w_min the distance
            // anywhere is at most 4 (w_max / w_min)^2 d: weights near zero cannot hide a swerve.
            let weights = points.iter().map(|h| h[3]);
            let (w_min, w_max) = weights.fold((f64::MAX, f64::MIN), |(lo, hi), w| (lo.min(w), hi.max(w)));
            if !(w_min > 0. && w_max.is_finite()) {
                return None;
            }
            let off = |p: Vec3| (p.dist(centre) - r).abs().max((p - centre).dot(normal).abs());
            let n = (2 * degree + 3).max(9);
            let mut d = 0f64;
            for w in c.breaks().windows(2) {
                let (Some(&s0), Some(&s1)) = (w.first(), w.get(1)) else { continue };
                let (mid, half) = (0.5 * (s0 + s1), 0.5 * (s1 - s0));
                d = d.max(off(c.at(s0))).max(off(c.at(s1)));
                for k in 0..n {
                    d = d.max(off(c.at(mid + half * (std::f64::consts::PI * (2 * k + 1) as f64 / (2 * n) as f64).cos())));
                }
            }
            // Relative to the curve's own size when that is smaller than the radius: a nearly
            // straight curve fits a huge circle, whose radius says nothing about the error.
            let pts: Vec<Vec3> = points.iter().filter(|h| h[3] != 0.).map(|h| Vec3::new(h[0] / h[3], h[1] / h[3], h[2] / h[3])).collect();
            let size = pts.iter().skip(1).fold(pts.first().map(|p| (*p, *p)), |acc, p| acc.map(|(lo, hi)| (lo.min(*p), hi.max(*p))));
            let size = size.map_or(r, |(lo, hi)| (hi - lo).len());
            (4. * (w_max / w_min).powi(2) * d <= tol * r.min(size)).then_some((centre, normal, r))
        }
        _ => None,
    }
}
