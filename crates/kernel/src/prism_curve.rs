//! Whole-span certificates for analytic planar prism boundaries, read off the curves of schema 4
//! record edges. A record's lines and circles are read as they are; a spline is read span by
//! span (its Bézier pieces): samples propose a circle, and only positive rational Bernstein
//! bounds accept it.
use solidcraft_geom::brep::eval::CurveEval;
use solidcraft_geom::brep::{Brep, Curve};
use solidcraft_geom::bspline::{self, HPoint};
use solidcraft_geom::{Plane, Seg2, Vec2, Vec3};

const MAX_DEGREE: usize = 32;
/// How far an arc read off a curve may end from the curve's end (mm): half the gap a profile
/// loop may have between segments and still close (`crate::build`).
const ARC_END_TOL: f64 = 5e-7;
const MAX_CONTROLS: usize = 4096;
type H = [f64; 3];

/// A curve over `interval` (in its own parameter), run from `ends[0]` to `ends[1]`: from the
/// interval's min to its max when `forward`, the other way otherwise. The ends are the points
/// the curve must reach there (a record edge's vertices).
#[derive(Clone, Copy, Debug)]
pub(crate) struct Run<'a> {
    pub curve: &'a Curve,
    pub interval: [f64; 2],
    pub forward: bool,
    pub ends: [Vec3; 2],
}

impl<'a> Run<'a> {
    /// Edge `edge` of `record`, run `forward` (from its start vertex to its end) or back.
    pub(crate) fn of(record: &'a Brep, edge: usize, forward: bool) -> Option<Run<'a>> {
        let e = record.edges.get(edge)?;
        let at = |v: u32| record.vertices.get(v as usize).map(|v| Vec3::new(v.point[0], v.point[1], v.point[2]));
        let (start, end) = (at(e.vertices[0])?, at(e.vertices[1])?);
        Some(Run { curve: &e.curve, interval: e.interval, forward, ends: if forward { [start, end] } else { [end, start] } })
    }

    /// The parameters it runs from and to.
    fn ends_along(&self) -> (f64, f64) {
        let [t0, t1] = self.interval;
        if self.forward { (t0, t1) } else { (t1, t0) }
    }
}

fn charge(work: &mut usize, n: usize) -> Option<()> {
    *work = work.checked_sub(n)?;
    Some(())
}

fn v3(p: [f64; 3]) -> Vec3 {
    Vec3::new(p[0], p[1], p[2])
}

/// The Bézier spans of a B-spline (homogeneous control points `ctrl` on `knots`) over `[t0, t1]`,
/// in parameter order: the piece clamped to the interval (Boehm insertion of its ends), then every
/// interior knot inserted until it repeats `degree` times; the span between two knots is the
/// `degree + 1` control points ending at the last of the knots below it. Exact up to rounding.
fn spline_spans(knots: &[f64], ctrl: &[HPoint], t0: f64, t1: f64, work: &mut usize) -> Option<Vec<Vec<HPoint>>> {
    let degree = knots.len().checked_sub(ctrl.len() + 1)?;
    if !(1..=MAX_DEGREE).contains(&degree) || ctrl.len() > MAX_CONTROLS || knots.len() > 8192 {
        return None;
    }
    if knots.iter().any(|t| !t.is_finite()) || knots.windows(2).any(|w| w[1] < w[0]) || ctrl.iter().any(|p| !p.is_finite()) {
        return None;
    }
    // Charge decomposition before its knot insertion allocations.
    charge(work, ctrl.len().checked_mul(knots.len())?.checked_mul(degree + 1)?)?;
    let (mut k, mut c) = bspline::segment(degree, knots, ctrl, t0, t1).ok()?;
    let mut interior: Vec<f64> = k.iter().copied().filter(|t| *t > t0 && *t < t1).collect();
    interior.dedup();
    for u in interior {
        while k.iter().filter(|t| **t == u).count() < degree {
            (k, c) = bspline::insert_knot(degree, &k, &c, u).ok()?;
        }
    }
    let n = c.len();
    let mut spans = Vec::new();
    for i in degree..n {
        let (Some(a), Some(b)) = (k.get(i), k.get(i + 1)) else { return None };
        if a < b {
            spans.push(c.get(i - degree..=i)?.to_vec());
        }
    }
    Some(spans)
}

/// An ellipse (`o + rx cos t x + ry sin t y`) from `t0` to `t1` (either way round) as rational
/// quadratic Bézier spans of at most a quarter turn each: each the conic through the span's ends
/// whose tangents meet at the apex, with the middle weight the cosine of half its angle. Exact:
/// an affine image of the circle's rational quadratic arcs.
fn ellipse_spans(o: Vec3, x: Vec3, y: Vec3, radii: [f64; 2], t0: f64, t1: f64, work: &mut usize) -> Option<Vec<Vec<HPoint>>> {
    let span = t1 - t0;
    if !span.is_finite() || span.abs() > std::f64::consts::TAU * 2.0 {
        return None;
    }
    let n = (span.abs() / std::f64::consts::FRAC_PI_2).ceil().max(1.0) as usize;
    charge(work, n.checked_mul(3)?)?;
    let at = |t: f64, scale: f64| o + (x * (radii[0] * t.cos()) + y * (radii[1] * t.sin())) * scale;
    let h = span / n as f64 / 2.0;
    let w = h.cos();
    Some(
        (0..n)
            .map(|i| {
                let a = t0 + span * i as f64 / n as f64;
                let m = a + h;
                vec![HPoint::weighted(at(a, 1.0), 1.0), HPoint::weighted(at(m, 1.0 / w), w), HPoint::weighted(at(a + 2.0 * h, 1.0), 1.0)]
            })
            .collect(),
    )
}

/// `run`'s curve as homogeneous Bézier spans in the direction it runs; `None` for a curve that
/// isn't a spline or an ellipse.
fn spans_of(run: &Run, work: &mut usize) -> Option<Vec<Vec<HPoint>>> {
    let [t0, t1] = run.interval;
    let spans = match run.curve {
        Curve::Spline { knots, points } => {
            let ctrl: Vec<HPoint> = points.iter().map(|p| HPoint::weighted(v3(*p), 1.0)).collect();
            spline_spans(knots, &ctrl, t0, t1, work)?
        }
        Curve::RationalSpline { knots, homogeneous } => {
            let ctrl: Vec<HPoint> = homogeneous.iter().map(|p| HPoint { x: p[0], y: p[1], z: p[2], w: p[3] }).collect();
            spline_spans(knots, &ctrl, t0, t1, work)?
        }
        Curve::Ellipse { frame, radii } => {
            let (x, y) = (v3(frame.x_axis), v3(frame.y_axis));
            let (a, b) = run.ends_along();
            return ellipse_spans(v3(frame.origin), x, y, *radii, a, b, work);
        }
        _ => return None,
    };
    Some(if run.forward { spans } else { spans.into_iter().rev().map(|s| s.into_iter().rev().collect()).collect() })
}

/// The spans in `plane`: `[x·w, y·w, w]` per control point (weights scaled to at most 1), and the
/// rounding error of reading them; `None` where a control point lies off the level `h`.
fn controls(spans: Vec<Vec<HPoint>>, plane: &Plane, h: f64, tol: f64) -> Option<(Vec<Vec<H>>, f64)> {
    if spans.is_empty() || spans.len() > 256 || spans.iter().map(Vec::len).sum::<usize>() > 8192 {
        return None;
    }
    let mut error: f64 = 0.;
    let mut out = Vec::new();
    for s in spans {
        if !(2..=MAX_DEGREE + 1).contains(&s.len()) {
            return None;
        }
        let max_w = s.iter().map(|p| p.w).fold(0., f64::max);
        if !max_w.is_finite() || max_w <= 0. {
            return None;
        }
        let mut span = Vec::new();
        for p in s {
            if !p.is_finite() || p.w / max_w < 1e-12 {
                return None;
            }
            let v = Vec3::new(p.x / p.w, p.y / p.w, p.z / p.w);
            let local = plane.to_local(v);
            let magnitude = v.x.abs() + v.y.abs() + v.z.abs() + plane.origin.len();
            let e = magnitude * f64::EPSILON * 256.;
            if !local.x.is_finite() || !local.y.is_finite() || (plane.height(v) - h).abs() + e > tol {
                return None;
            }
            error = error.max(e);
            let w = p.w / max_w;
            span.push([local.x * w, local.y * w, w]);
        }
        out.push(span);
    }
    (error < tol * 0.1).then_some((out, error))
}

fn product(a: &[f64], b: &[f64], work: &mut usize) -> Option<Vec<f64>> {
    charge(work, a.len().checked_mul(b.len())?)?;
    let (m, n) = (a.len() - 1, b.len() - 1);
    let choose = |degree: usize| {
        let mut v = vec![1.; degree + 1];
        for i in 1..=degree {
            v[i] = v[i - 1] * (degree + 1 - i) as f64 / i as f64;
        }
        v
    };
    let (cm, cn, ct) = (choose(m), choose(n), choose(m + n));
    let mut out = vec![0.; m + n + 1];
    for (i, ai) in a.iter().enumerate() {
        for (j, bj) in b.iter().enumerate() {
            out[i + j] += ai * bj * cm[i] * cn[j] / ct[i + j];
        }
    }
    out.iter().all(|x| x.is_finite()).then_some(out)
}

fn derivative(a: &[f64]) -> Vec<f64> {
    let n = (a.len() - 1) as f64;
    a.windows(2).map(|w| (w[1] - w[0]) * n).collect()
}

fn determinant(a: &[f64], b: &[f64], work: &mut usize) -> Option<(Vec<f64>, f64)> {
    let (ab, ba) = (product(a, &derivative(b), work)?, product(b, &derivative(a), work)?);
    let max = |v: &[f64]| v.iter().map(|x| x.abs()).fold(0., f64::max);
    // Bernstein product weights are positive and sum to one. Bound absolute
    // summands from the inputs, never from a possibly cancelled output sum.
    let scale = max(a) * max(&derivative(b)) + max(b) * max(&derivative(a));
    let error = scale * f64::EPSILON * 128. * (a.len() * a.len()) as f64;
    Some((ab.iter().zip(&ba).map(|(x, y)| x - y).collect(), error))
}

fn split(c: &[H]) -> (Vec<H>, Vec<H>) {
    let mut q = c.to_vec();
    let mut left = vec![q[0]];
    let mut right = vec![q[q.len() - 1]];
    while q.len() > 1 {
        q = q.windows(2).map(|p| std::array::from_fn(|i| (p[0][i] + p[1][i]) * 0.5)).collect();
        left.push(q[0]);
        right.push(q[q.len() - 1]);
    }
    right.reverse();
    (left, right)
}

fn point(p: H) -> Vec2 {
    Vec2::new(p[0] / p[2], p[1] / p[2])
}

fn line_span(c: &[H], a: Vec2, b: Vec2, tol: f64, error: f64, work: &mut usize) -> Option<()> {
    let d = b - a;
    let length = d.len();
    if length <= tol {
        return None;
    }
    let d = d / length;
    let mut projected = Vec::new();
    let weights: Vec<_> = c.iter().map(|p| p[2]).collect();
    for p in c {
        let v = point(*p) - a;
        let t = v.dot(d);
        if (v.x * d.y - v.y * d.x).abs() + error > tol || t < -tol + error || t > length + tol - error {
            return None;
        }
        projected.push(t * p[2]);
    }
    // W*T' - T*W' proves oriented traversal, rather than merely collinearity.
    let (der, mut err) = determinant(&weights, &projected, work)?;
    err += error * (c.len() * c.len()) as f64 * 32.;
    if !der.iter().all(|d| *d > err) {
        return None;
    }
    Some(())
}

fn circle_span(c: &[H], center: Vec2, radius: f64, tol: f64, source_error: f64, depth: usize, work: &mut usize) -> Option<f64> {
    charge(work, c.len())?;
    let x: Vec<_> = c.iter().map(|p| p[0] - center.x * p[2]).collect();
    let y: Vec<_> = c.iter().map(|p| p[1] - center.y * p[2]).collect();
    let w: Vec<_> = c.iter().map(|p| p[2]).collect();
    let min_w = w.iter().copied().fold(f64::INFINITY, f64::min);
    if min_w < 1e-12 {
        return None;
    }
    let (xx, yy, ww) = (product(&x, &x, work)?, product(&y, &y, work)?, product(&w, &w, work)?);
    let max = |v: &[f64]| v.iter().map(|x| x.abs()).fold(0., f64::max);
    let scale = max(&x).powi(2) + max(&y).powi(2) + radius * radius * max(&w).powi(2);
    let coordinate_error = source_error + (center.len() + radius) * f64::EPSILON * (depth + 1) as f64 * 128. * c.len() as f64;
    let roundoff = scale * f64::EPSILON * 256. * (c.len() * c.len()) as f64;
    let residual = xx.iter().zip(&yy).zip(&ww).map(|((a, b), w)| (a + b - radius * radius * w).abs()).fold(0., f64::max);
    let radial_tol = tol - coordinate_error;
    if radial_tol <= 0. || (residual + roundoff) / (min_w * min_w) > radial_tol * (2. * radius - radial_tol) {
        return None;
    }
    let (angular, mut error) = determinant(&x, &y, work)?;
    let extent = x.iter().chain(&y).map(|v| v.abs()).fold(0., f64::max);
    error += coordinate_error * extent * (c.len() * c.len()) as f64 * 32.;
    let positive = angular.iter().all(|v| *v > error);
    let negative = angular.iter().all(|v| *v < -error);
    let a = point(c[0]) - center;
    let b = point(c[c.len() - 1]) - center;
    let mid = a + b;
    // One open angular half-plane plus a derivative sign rules out hidden turns.
    let half_plane = mid.len() > radius * 1e-8 && c.iter().all(|p| (point(*p) - center).dot(mid) > coordinate_error * mid.len());
    if (positive || negative) && half_plane {
        let cross = a.x * b.y - a.y * b.x;
        let sweep = cross.atan2(a.dot(b));
        if (positive && sweep > 0.) || (negative && sweep < 0.) {
            return Some(sweep);
        }
        return None;
    }
    if depth >= 12 {
        return None;
    }
    let (left, right) = split(c);
    let a = circle_span(&left, center, radius, tol, source_error, depth + 1, work)?;
    let b = circle_span(&right, center, radius, tol, source_error, depth + 1, work)?;
    (a.signum() == b.signum()).then_some(a + b)
}

/// `run` in `plane` exactly, as lines, circular arcs and free-form segments (each free-form
/// curve one segment): `None` when it doesn't lie in a plane parallel to `plane`, isn't read
/// exactly, isn't finite or goes over the work budget.
pub(crate) fn segment(run: &Run, plane: &Plane, tol: f64, work: &mut usize) -> Option<Vec<Seg2>> {
    read(run, plane, tol, work, false)
}

/// `run` in `plane` exactly, for a sketch to project it: as [`segment`], and a curve of several
/// Bézier spans (a chain of a font's curves, a spline through points) as one segment per span,
/// each the line, conic or Bézier it is. `None` when the edge doesn't lie in a plane parallel to
/// `plane` or isn't read exactly.
pub(crate) fn projected(run: &Run, plane: &Plane, tol: f64, work: &mut usize) -> Option<Vec<Seg2>> {
    read(run, plane, tol, work, true)
}

fn read(run: &Run, plane: &Plane, tol: f64, work: &mut usize, each_span: bool) -> Option<Vec<Seg2>> {
    if !tol.is_finite() || tol <= 0. {
        return None;
    }
    charge(work, 1)?;
    if matches!(run.curve, Curve::SurfaceIntersection { .. }) {
        return None;
    }
    let (ts, te) = run.ends_along();
    if !ts.is_finite() || !te.is_finite() || run.interval[1] <= run.interval[0] {
        return None;
    }
    let eval = CurveEval::new(run.curve).ok()?;
    let (a3, b3) = (eval.point(ts)?, eval.point(te)?);
    let (a, b) = (plane.to_local(a3), plane.to_local(b3));
    if !a3.is_finite()
        || !b3.is_finite()
        || ![a.x, a.y, b.x, b.y].iter().all(|v| v.is_finite())
        || a3.dist(run.ends[0]) > tol
        || b3.dist(run.ends[1]) > tol
    {
        return None;
    }
    let level = plane.height(a3);
    match run.curve {
        Curve::Line { .. } => {
            let error = (a3.len() + b3.len() + plane.origin.len()) * f64::EPSILON * 256.;
            return (error < tol * 0.1 && a.dist(b) > tol && (level - plane.height(b3)).abs() + error <= tol).then_some(vec![Seg2::Line { a, b }]);
        }
        Curve::Polyline { points } => return polyline(points, ts, te, plane, level, tol, work),
        Curve::Circle { frame, radius } => return circle(v3(frame.origin), v3(frame.x_axis), v3(frame.y_axis), *radius, ts, te, a, b, plane, tol),
        _ => {}
    }
    let (spans, error) = controls(spans_of(run, work)?, plane, level, tol)?;
    if spans.windows(2).any(|pair| point(pair[0][pair[0].len() - 1]).dist(point(pair[1][0])) + 2. * error > tol)
        || point(spans.first()?.first().copied()?).dist(a) + error > tol
        || point(spans.last()?.last().copied()?).dist(b) + error > tol
    {
        return None;
    }
    if a.dist(b) > tol && spans.iter().all(|s| line_span(s, a, b, tol, error, work).is_some()) {
        return Some(vec![Seg2::Line { a, b }]);
    }
    arc(&eval, (ts, te), &spans, plane, a, b, tol, error, work)
        .or_else(|| free_form(&spans, a, b, tol))
        .or_else(|| if each_span { span_by_span(&spans, a, b, tol) } else { None })
}

/// A polyline from parameter `ts` to `te` (either way round; segment i spans [i, i + 1]) as its
/// lines, each at level `level` of `plane`.
fn polyline(points: &[[f64; 3]], ts: f64, te: f64, plane: &Plane, level: f64, tol: f64, work: &mut usize) -> Option<Vec<Seg2>> {
    charge(work, points.len())?;
    let (lo, hi) = (ts.min(te), ts.max(te));
    if !(lo >= 0. && hi <= points.len() as f64) {
        return None;
    }
    let at = |t: f64| -> Option<Vec3> {
        let last = points.len().checked_sub(2)?;
        let i = (t.floor().max(0.) as usize).min(last);
        let (p, q) = (v3(*points.get(i)?), v3(*points.get(i + 1)?));
        Some(p + (q - p) * (t - i as f64))
    };
    let mut stations = vec![lo];
    stations.extend((lo.floor() as i64 + 1..=hi.ceil() as i64 - 1).map(|k| k as f64).filter(|k| *k > lo && *k < hi));
    stations.push(hi);
    if ts > te {
        stations.reverse();
    }
    let mut out = Vec::with_capacity(stations.len());
    for pair in stations.windows(2) {
        let (p, q) = (at(pair[0])?, at(pair[1])?);
        let (u, v) = (plane.to_local(p), plane.to_local(q));
        if (plane.height(p) - level).abs() > tol || (plane.height(q) - level).abs() > tol || u.dist(v) <= tol {
            return None;
        }
        out.push(Seg2::Line { a: u, b: v });
    }
    Some(out)
}

/// A circle (`o + r (cos t x + sin t y)`) from `ts` to `te`, ending at `a` and `b` in `plane`, as
/// the arc it is: `None` when it doesn't lie in a plane parallel to `plane`. A whole circle comes
/// back as its two halves.
#[allow(clippy::too_many_arguments)]
fn circle(o: Vec3, x: Vec3, y: Vec3, radius: f64, ts: f64, te: f64, a: Vec2, b: Vec2, plane: &Plane, tol: f64) -> Option<Vec<Seg2>> {
    let n = plane.normal();
    let z = x.cross(y);
    let tilt = radius * z.cross(n).len();
    let error = (o.len() + radius + plane.origin.len()) * f64::EPSILON * 256.;
    if !(radius.is_finite() && radius > tol) || tilt + error > tol || error > tol * 0.1 {
        return None;
    }
    let center = plane.to_local(o);
    let sweep = (te - ts) * z.dot(n).signum();
    let angular_tol = tol / radius;
    if sweep.abs() > std::f64::consts::TAU + angular_tol || sweep.abs() < angular_tol {
        return None;
    }
    let start = (a - center).angle();
    let end = center + Vec2::new((start + sweep).cos(), (start + sweep).sin()) * radius;
    if end.dist(b) + error > tol.min(ARC_END_TOL) || (a.dist(center) - radius).abs() > tol {
        return None;
    }
    if (sweep.abs() - std::f64::consts::TAU).abs() <= angular_tol {
        let h = sweep.signum() * std::f64::consts::PI;
        return Some(vec![Seg2::Arc { center, radius, start, sweep: h }, Seg2::Arc { center, radius, start: start + h, sweep: h }]);
    }
    Some(vec![Seg2::Arc { center, radius, start, sweep }])
}

/// Several `spans` end to end, from `a` to `b`, each as the segment it is: a span whose control
/// polygon lies along its chord a line, the others as [`free_form`] reads them. Spans whose
/// ends meet are left out; each segment starts where the one before ended.
fn span_by_span(spans: &[Vec<H>], a: Vec2, b: Vec2, tol: f64) -> Option<Vec<Seg2>> {
    let spans: Vec<&Vec<H>> = spans.iter().filter(|s| s.first().zip(s.last()).is_some_and(|(p, q)| point(*p).dist(point(*q)) > tol)).collect();
    if spans.len() < 2 {
        return None;
    }
    let mut out = Vec::with_capacity(spans.len());
    let mut start = a;
    for (i, span) in spans.iter().enumerate() {
        let end = if i + 1 == spans.len() { b } else { point(*span.last()?) };
        let seg = match free_form(std::slice::from_ref(*span), start, end, tol) {
            Some(mut s) => s.pop()?,
            None if span.iter().all(|p| (p[2] - 1.).abs() <= 1e-12) && off_chord(&span.iter().map(|p| point(*p)).collect::<Vec<_>>()) <= tol => {
                Seg2::Line { a: start, b: end }
            }
            None => return None,
        };
        out.push(seg);
        start = end;
    }
    Some(out)
}

/// How far the control points `pts` lie off the chord between the first and the last.
fn off_chord(pts: &[Vec2]) -> f64 {
    let (Some(first), Some(last)) = (pts.first().copied(), pts.last().copied()) else { return f64::INFINITY };
    let chord = last - first;
    let len = chord.len();
    if !(len > 0.) {
        return f64::INFINITY;
    }
    pts.iter().map(|p| chord.cross(*p - first).abs()).fold(0., f64::max) / len
}

/// One Bézier span that is neither a line nor an arc, as the exact free-form segment it is
/// (a font's quadratic or cubic, a conic), its ends `a` and `b`. `None` for several spans (a
/// curve that may wind about more than once), degrees past [`solidcraft_geom::MAX_BEZIER_DEGREE`],
/// rational spans other than conics, and control polygons with no area (a curve doubling back
/// along a line).
fn free_form(spans: &[Vec<H>], a: Vec2, b: Vec2, tol: f64) -> Option<Vec<Seg2>> {
    let [span] = spans else { return None };
    let pts: Vec<Vec2> = span.iter().map(|p| point(*p)).collect();
    let (first, last) = (*pts.first()?, *pts.last()?);
    if first.dist(a) > tol || last.dist(b) > tol || first.dist(last) <= tol {
        return None;
    }
    // Twice the area the control polygon spans off the chord: none is a line, maybe doubling back.
    let chord = last - first;
    let off = pts.iter().map(|p| chord.cross(*p - first).abs()).fold(0., f64::max) / chord.len();
    if !(off > tol) {
        return None;
    }
    let rational = span.iter().any(|p| (p[2] - 1.).abs() > 1e-12);
    let seg = match (pts.len(), rational) {
        (_, false) => solidcraft_geom::bezier(&pts)?,
        (3, true) => {
            // Weights normalised to 1 at the ends: the middle one is the conic's.
            let (w0, w1, w2) = (span[0][2], span[1][2], span[2][2]);
            let w = w1 / (w0 * w2).sqrt();
            if !(w.is_finite() && w > 1e-9) {
                return None;
            }
            Seg2::Conic { a: first, apex: pts[1], b: last, w }
        }
        _ => return None,
    };
    // The exact ends of the edge (the span's agree within the tolerance).
    let seg = match seg {
        Seg2::Cubic { p1, p2, .. } => Seg2::Cubic { p0: a, p1, p2, p3: b },
        Seg2::Conic { apex, w, .. } => Seg2::Conic { a, apex, b, w },
        Seg2::Bezier { n, mut p } => {
            *p.first_mut()? = a;
            *p.get_mut(n as usize)? = b;
            Seg2::Bezier { n, p }
        }
        other => other,
    };
    Some(vec![seg])
}

/// The spline (`eval` from parameter `ts` to `te`; its Bézier `spans` in the plane, from `a` to
/// `b`) as a circular arc, when whole-span proofs show it is one.
#[allow(clippy::too_many_arguments)]
fn arc(
    eval: &CurveEval,
    (ts, te): (f64, f64),
    spans: &[Vec<H>],
    plane: &Plane,
    a: Vec2,
    b: Vec2,
    tol: f64,
    error: f64,
    work: &mut usize,
) -> Option<Vec<Seg2>> {
    // Samples are only a candidate proposal. Whole-span implicit and oriented
    // angular proofs below are required before converting any spline to an arc.
    let q = plane.to_local(eval.point(ts + (te - ts) * 5. / 16.)?) - a;
    let r = plane.to_local(eval.point(ts + (te - ts) * 11. / 16.)?) - a;
    let det = 2. * (q.x * r.y - q.y * r.x);
    if !det.is_finite() || det.abs() < 1e-300 {
        return None;
    }
    let offset = Vec2::new((q.dot(q) * r.y - r.dot(r) * q.y) / det, (q.x * r.dot(r) - r.x * q.dot(q)) / det);
    let (center, radius) = (a + offset, offset.len());
    if !radius.is_finite() || radius <= tol || !center.x.is_finite() || !center.y.is_finite() {
        return None;
    }
    let mut sweep: f64 = 0.;
    for span in spans {
        let ds = circle_span(span, center, radius, tol, error, 0, work)?;
        if sweep != 0. && sweep.signum() != ds.signum() {
            return None;
        }
        sweep += ds;
    }
    let start = (a - center).angle();
    let end = center + Vec2::new((start + sweep).cos(), (start + sweep).sin()) * radius;
    // The arc ends where the curve does, as closely as a profile loop must close: within `tol`
    // only (which grows with the model), a curve that is merely near a circle (a font's) would
    // come back as an arc ending off the next segment's start, and the loop would not close.
    if end.dist(b) + error > tol.min(ARC_END_TOL) {
        return None;
    }
    let closed = a.dist(b) < tol;
    let angular_tol = tol / radius;
    if sweep.abs() > std::f64::consts::TAU + angular_tol || sweep.abs() < angular_tol {
        return None;
    }
    if closed {
        if (sweep.abs() - std::f64::consts::TAU).abs() > angular_tol {
            return None;
        }
        let h = sweep.signum() * std::f64::consts::PI;
        Some(vec![Seg2::Arc { center, radius, start, sweep: h }, Seg2::Arc { center, radius, start: start + h, sweep: h }])
    } else {
        Some(vec![Seg2::Arc { center, radius, start, sweep }])
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    fn choose(n: usize, k: usize) -> f64 {
        (0..k).fold(1., |v, i| v * (n - i) as f64 / (i + 1) as f64)
    }
    fn bernstein(power: &[f64], degree: usize) -> Vec<f64> {
        (0..=degree).map(|i| (0..=i.min(power.len() - 1)).map(|k| power[k] * choose(i, k) / choose(degree, k)).sum()).collect()
    }
    fn multiply(a: &[f64], b: &[f64]) -> Vec<f64> {
        let mut out = vec![0.; a.len() + b.len() - 1];
        for (i, x) in a.iter().enumerate() {
            for (j, y) in b.iter().enumerate() {
                out[i + j] += x * y;
            }
        }
        out
    }
    fn trap() -> Vec<f64> {
        let mut p = vec![1.];
        for k in 0..=16 {
            p = multiply(&p, &[-(k as f64) / 16., 1.]);
        }
        let mid = (0..=16).map(|k| 1. / 32. - k as f64 / 16.).product::<f64>();
        p.iter_mut().for_each(|v| *v /= mid);
        p
    }
    fn bezier_knots(degree: usize) -> Vec<f64> {
        std::iter::repeat_n(0., degree + 1).chain(std::iter::repeat_n(1., degree + 1)).collect()
    }
    /// A NURBS curve of homogeneous control points `(x, y, 0, w)` (power bases in `x`, `y`, `w`).
    fn curve(x: &[f64], y: &[f64], w: &[f64], degree: usize) -> Curve {
        let (x, y, w) = (bernstein(x, degree), bernstein(y, degree), bernstein(w, degree));
        let homogeneous = x.iter().zip(y).zip(w).map(|((x, y), w)| [*x, y, 0., w]).collect();
        Curve::RationalSpline { knots: bezier_knots(degree), homogeneous }
    }
    fn at(c: &Curve, t: f64) -> Vec3 {
        CurveEval::new(c).unwrap().point(t).unwrap()
    }
    /// `c` over its whole domain, run forward (or back).
    fn run(c: &Curve, forward: bool) -> Run<'_> {
        let interval = match c.natural() {
            solidcraft_geom::brep::Natural::Range(a, b) => [a, b],
            solidcraft_geom::brep::Natural::Periodic => [0., std::f64::consts::TAU],
        };
        let ends = [at(c, interval[0]), at(c, interval[1])];
        Run { curve: c, interval, forward, ends: if forward { ends } else { [ends[1], ends[0]] } }
    }
    /// The quarter circle of radius 10 about the origin from +x to +y, as a rational quadratic
    /// spline.
    fn quarter() -> Curve {
        let w = std::f64::consts::FRAC_1_SQRT_2;
        Curve::RationalSpline { knots: bezier_knots(2), homogeneous: vec![[10., 0., 0., 1.], [10. * w, 10. * w, 0., w], [0., 10., 0., 1.]] }
    }

    #[test]
    fn sample_aliasing_does_not_certify_spline_caps() {
        let e = curve(&[0., 10.], &trap(), &[1.], 17);
        for k in 0..=16 {
            assert!(at(&e, k as f64 / 16.).y.abs() < 1e-4, "old sample locations must see a line");
        }
        assert!((at(&e, 1. / 32.).y - 1.).abs() < 1e-6);
        assert!(segment(&run(&e, true), &Plane::XY, 1e-3, &mut 2_000_000).is_none());
        let q = trap();
        let mut perturb = q.iter().map(|v| v * 0.1).collect::<Vec<_>>();
        perturb[0] += 1.;
        let w = std::f64::consts::FRAC_1_SQRT_2;
        let x = multiply(&[10., 20. * (w - 1.), 10. * (1. - 2. * w)], &perturb);
        let y = multiply(&[0., 20. * w, 10. * (1. - 2. * w)], &perturb);
        let circle = curve(&x, &y, &[1., 2. * (w - 1.), 2. * (1. - w)], 19);
        for k in 0..=16 {
            let p = at(&circle, k as f64 / 16.);
            assert!((p.x.hypot(p.y) - 10.).abs() < 1e-3, "old sample locations must see a circle");
        }
        let p = at(&circle, 1. / 32.);
        assert!((p.x.hypot(p.y) - 11.).abs() < 1e-5);
        assert!(segment(&run(&circle, true), &Plane::XY, 1e-3, &mut 2_000_000).is_none());
    }

    #[test]
    fn genuine_curves_and_oriented_traversal_are_certified() {
        let arc = quarter();
        for forward in [true, false] {
            let result = segment(&run(&arc, forward), &Plane::XY, 1e-5, &mut 2_000_000).unwrap();
            let Seg2::Arc { center, radius, sweep, .. } = result[0] else { panic!("expected arc") };
            assert!(center.len() < 1e-8 && (radius - 10.).abs() < 1e-8);
            assert!((sweep.abs() - std::f64::consts::FRAC_PI_2).abs() < 1e-8);
            assert_eq!(sweep > 0., forward);
        }
        let line = curve(&[0., 10.], &[0.], &[1.], 17);
        assert!(matches!(segment(&run(&line, true), &Plane::XY, 1e-5, &mut 2_000_000).unwrap()[0], Seg2::Line { .. }));
        // The longitudinal derivative is negative at t=1/3, independently
        // establishing genuine retracing rather than a stationary tangent.
        let backtrack = curve(&[0., 10., -35., 35.], &[0.], &[1.], 3);
        assert!((at(&backtrack, 1. / 3. + 1e-6).x - at(&backtrack, 1. / 3.).x) / 1e-6 < -1.);
        assert!(segment(&run(&backtrack, true), &Plane::XY, 1e-5, &mut 2_000_000).is_none());
        assert!(segment(&run(&arc, true), &Plane::XY, 1e-5, &mut 0).is_none());
    }

    /// A curve read as an arc ends where the curve does, however large the tolerance (it grows
    /// with the part): a font's quadratic that is within a micron of a circle, not on one, is
    /// kept as the curve it is. Read as an arc whose end is a micron off, the outline of a letter
    /// cut into a 2 m plate did not close.
    #[test]
    fn a_curve_near_a_circle_is_not_an_arc_ending_off_it() {
        // A parabola within a micron of the circle through its ends and middle.
        let e = curve(&[0., 2., 0.], &[0., 0., 0.2], &[1.], 2);
        let (a, b) = (Vec2::new(0., 0.), Vec2::new(2., 0.2));
        assert!(at(&e, 1.).dist(Vec3::new(2., 0.2, 0.)) < 1e-12);
        for tol in [1e-5, 2e-3] {
            let segs = segment(&run(&e, true), &Plane::XY, tol, &mut 2_000_000).unwrap();
            let (first, last) = (segs.first().unwrap(), segs.last().unwrap());
            assert!(first.start().dist(a) < 1e-9 && last.end().dist(b) < 1e-9, "{tol}: {segs:?}");
        }
        // A true arc is still one at either tolerance.
        let arc = quarter();
        for tol in [1e-5, 2e-3] {
            assert!(matches!(segment(&run(&arc, true), &Plane::XY, tol, &mut 2_000_000).unwrap()[..], [Seg2::Arc { .. }]));
        }
    }

    #[test]
    fn discontinuous_and_multiple_winding_curves_decline() {
        let spline = |knots: Vec<f64>, homogeneous: Vec<[f64; 4]>| Curve::RationalSpline { knots, homogeneous };
        let w = std::f64::consts::FRAC_1_SQRT_2;
        let p = |x, y, w| [x, y, 0., w];
        let disconnected = spline(
            vec![0., 0., 0., 1., 1., 1., 2., 2., 2.],
            vec![p(10., 0., 1.), p(10. * w, 10. * w, w), p(0., 10., 1.), p(-10., 0., 1.), p(-10. * w, -10. * w, w), p(0., -10., 1.)],
        );
        assert!(segment(&run(&disconnected, true), &Plane::XY, 1e-5, &mut 2_000_000).is_none());
        let skipped_line = spline(vec![0., 0., 1., 1., 2., 2.], vec![p(0., 0., 1.), p(2., 0., 1.), p(5., 0., 1.), p(10., 0., 1.)]);
        assert!(segment(&run(&skipped_line, true), &Plane::XY, 1e-5, &mut 2_000_000).is_none());
        let mut points = vec![p(10., 0., 1.)];
        let mut knots = vec![0., 0., 0.];
        for i in 0..8 {
            let a = i as f64 * std::f64::consts::FRAC_PI_2;
            let b = a + std::f64::consts::FRAC_PI_2;
            points.push(p(10. * ((a + b) / 2.).cos(), 10. * ((a + b) / 2.).sin(), w));
            points.push(p(10. * b.cos(), 10. * b.sin(), 1.));
            knots.extend([i as f64 + 1.; 2]);
        }
        knots.push(8.);
        let twice = spline(knots, points);
        assert!(segment(&run(&twice, true), &Plane::XY, 1e-5, &mut 2_000_000).is_none());
    }
}
