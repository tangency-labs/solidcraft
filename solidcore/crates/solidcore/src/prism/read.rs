//! A curve read exactly in a plane: an edge's curve, over its interval and in the direction it
//! runs, as the lines, circular arcs and free-form segments it is in a frame's coordinates. The
//! S→B contract for exact classes (owner rule 13564): a line or arc is returned only when the
//! curve's net B is proved on it span by span, to the rounding of checking it.
//!
//! - **A line kind** is its label: its points at the interval's ends, at one level.
//! - **A curve with a net** (a spline, a conic): its Bézier spans (knot insertion of its net,
//!   exact up to rounding) mapped into the frame, each control point at the curve's level within
//!   the tolerance. A line is proposed by the ends and proved by every span: control points
//!   within the tolerance of the chord, and `W T' - T W'` positive (an oriented traversal, not a
//!   retracing). A circle is proposed by the curve's label (a circle kind's centre and radius)
//!   or by three of its points, and proved by every span: the Bernstein coefficients of
//!   `x^2 + y^2 - r^2 w^2` within the rounding of computing them and the tolerance, the angular
//!   determinant of one sign, the span in one open half plane about the centre (split in halves
//!   up to twelve times where needed). Else the one span is the free-form segment it is (a
//!   conic, a Bézier), its ends the edge's; with `each_span`, a curve of several spans is read
//!   span by span.
//!
//! Not upstream: SolidCraft's planar prism boundary reading (`prism_curve.rs`), moved into the
//! kernel and read off the kernel's curves (docs/layers.md §1).
//!
//! References: Farin, *Curves and Surfaces for CAGD* (5th ed.), §5.2 (Bernstein products and
//! derivatives), §12 (rational curves); Piegl & Tiller, *The NURBS Book* (2nd ed.), A5.1 (knot
//! insertion), A5.6 (Bézier decomposition).
//!
//! Deviations: none (no upstream counterpart). No recursion.

#![allow(clippy::neg_cmp_op_on_partial_ord)] // NaN-refusing tests, kept as written

use std::f64::consts::{PI, TAU};

use super::profile::{cross2, derivative, halves, point_of, product, Seg2, H2};
use super::profile::{height, to_local, MAX_BEZIER_DEGREE};
use crate::curve::curve::{Curve, CurveKind};
use crate::eval::Frame;
use crate::geom::extent1d::Extent1d;
use crate::geom::vector3d::{Point3, Vec3};
use crate::kernel::Kernel;
use crate::math::{Point2, Vec2};
use crate::nurbs::b_spline_curve::BSplineCurve;
use crate::nurbs::math::{NOW, NOZ};
use crate::topology::{edge, Brep, EdgeId};

const MAX_DEGREE: usize = 32;
const MAX_CONTROLS: usize = 4096;
/// How far an arc read off a curve may end from the curve's end (mm): half the gap a profile
/// loop may have between its segments and still close.
const ARC_END_TOL: f64 = 5e-7;

/// A curve over `interval` (its own parameter), run from `ends[0]` to `ends[1]`: from the
/// interval's min to its max when `forward`, back otherwise. The ends are the points it must
/// reach there (an edge's vertices).
#[derive(Clone, Copy, Debug)]
pub struct Run<'a> {
    /// The curve.
    pub curve: &'a Curve,
    /// Its interval.
    pub interval: Extent1d,
    /// Whether it runs from the interval's min.
    pub forward: bool,
    /// The points it starts and ends at.
    pub ends: [Point3; 2],
}

impl Run<'_> {
    /// The parameters it runs from and to.
    fn ends_along(&self) -> (f64, f64) {
        let (t0, t1) = (self.interval.min(), self.interval.max());
        if self.forward {
            (t0, t1)
        } else {
            (t1, t0)
        }
    }
}

/// One piece of an edge read exactly in a plane: a line, arc or single free-form segment, or a
/// free-form curve of several spans as the one B-spline it is ([`PlanarSpline`]).
#[derive(Clone, Debug, PartialEq)]
pub enum PlanarPiece {
    /// A segment.
    Seg(Seg2),
    /// A B-spline.
    Spline(PlanarSpline),
}

/// A free-form curve of several Bézier spans in a plane, as the one B-spline it is: its degree,
/// clamped knots, control points in the frame's coordinates, and weights (empty: polynomial).
/// Its first and last control points are its ends.
#[derive(Clone, Debug, PartialEq)]
pub struct PlanarSpline {
    /// Its degree.
    pub degree: usize,
    /// Its clamped knots.
    pub knots: Vec<f64>,
    /// Its control points.
    pub points: Vec<Point2>,
    /// Its weights (empty: all one).
    pub weights: Vec<f64>,
}

fn charge(work: &mut usize, n: usize) -> Option<()> {
    *work = work.checked_sub(n)?;
    Some(())
}

/// The curve's net (`SmBSplineCurve`) when its kind has one.
fn net(curve: &Curve) -> Option<&BSplineCurve> {
    match curve.kind() {
        CurveKind::BSpline(b) => Some(b),
        CurveKind::Ellipse(e) => Some(&e.base),
        CurveKind::Hyperbola(h) => Some(&h.base),
        _ => None,
    }
}

/// The circle a circle kind is labelled with: its centre, its plane's unit normal and radius.
fn circle_label(curve: &Curve) -> Option<(Point3, Vec3, f64)> {
    match curve.kind() {
        CurveKind::Ellipse(e) if e.circle && e.radius_x == e.radius_y => {
            let n = e.position.x_axis().cross(e.position.y_axis());
            Some((e.position.origin(), n, e.radius_x))
        }
        _ => None,
    }
}

fn hp(c: crate::nurbs::math::CPoint) -> [f64; 4] {
    let w = if c.w == NOW { 1.0 } else { c.w };
    let z = if c.z == NOZ { 0.0 } else { c.z };
    // a non-rational point is stored unweighted
    if c.w == NOW {
        [c.x, c.y, z, 1.0]
    } else {
        [c.x, c.y, z, w]
    }
}

/// The net's clamped segment over the run's interval (knot insertion), in the curve's own
/// direction: its degree, knots and homogeneous control points.
fn segment(run: &Run, work: &mut usize) -> Option<(usize, Vec<f64>, Vec<[f64; 4]>)> {
    let nurb = net(run.curve)?.nurb.as_ref()?;
    let (degree, n) = (nurb.degree(), nurb.cpts().len());
    if !(1..=MAX_DEGREE).contains(&degree) || n > MAX_CONTROLS || nurb.knots().len() > 8192 {
        return None;
    }
    charge(
        work,
        n.checked_mul(nurb.knots().len())?.checked_mul(degree + 1)?,
    )?;
    let (u0, u1) = nurb.param_bounds();
    let (t0, t1) = (run.interval.min(), run.interval.max());
    if !(t0 < t1 && t0 >= u0 && t1 <= u1) {
        return None;
    }
    let seg = if t0 == u0 && t1 == u1 {
        nurb.clone()
    } else {
        nurb.extract_seg(t0, t1).ok()?
    };
    let pts: Vec<[f64; 4]> = seg.cpts().iter().map(|c| hp(*c)).collect();
    pts.iter()
        .all(|p| p.iter().all(|x| x.is_finite()))
        .then(|| (seg.degree(), seg.knots().to_vec(), pts))
}

/// The run's Bézier spans, homogeneous, in the direction it runs.
fn spans_of(run: &Run, work: &mut usize) -> Option<Vec<Vec<[f64; 4]>>> {
    let nurb = net(run.curve)?.nurb.as_ref()?;
    let (t0, t1) = (run.interval.min(), run.interval.max());
    let (u0, u1) = nurb.param_bounds();
    if !(t0 < t1 && t0 >= u0 && t1 <= u1) {
        return None;
    }
    charge(work, nurb.cpts().len().checked_mul(nurb.knots().len())?)?;
    let seg = if t0 == u0 && t1 == u1 {
        nurb.clone()
    } else {
        nurb.extract_seg(t0, t1).ok()?
    };
    let spans: Vec<Vec<[f64; 4]>> = seg
        .decompose_bez()
        .ok()?
        .iter()
        .map(|b| b.cpts().iter().map(|c| hp(*c)).collect())
        .collect();
    Some(if run.forward {
        spans
    } else {
        spans
            .into_iter()
            .rev()
            .map(|s| s.into_iter().rev().collect())
            .collect()
    })
}

/// The spans in `frame`: `[x w, y w, w]` per control point (weights scaled to at most 1), and
/// the rounding error of reading them; `None` where a control point lies off the level `h`.
fn controls(
    spans: Vec<Vec<[f64; 4]>>,
    frame: &Frame,
    h: f64,
    tol: f64,
) -> Option<(Vec<Vec<H2>>, f64)> {
    if spans.is_empty() || spans.len() > 256 || spans.iter().map(Vec::len).sum::<usize>() > 8192 {
        return None;
    }
    let o = Vec3::new(frame.origin[0], frame.origin[1], frame.origin[2]);
    let mut error: f64 = 0.0;
    let mut out = Vec::new();
    for s in spans {
        if !(2..=MAX_DEGREE + 1).contains(&s.len()) {
            return None;
        }
        let max_w = s.iter().map(|p| p[3]).fold(0.0, f64::max);
        if !max_w.is_finite() || max_w <= 0.0 {
            return None;
        }
        let mut span = Vec::new();
        for p in s {
            if p[3] / max_w < 1e-12 {
                return None;
            }
            let v = Point3::new(p[0] / p[3], p[1] / p[3], p[2] / p[3]);
            let local = to_local(frame, v);
            let e = (v.x.abs() + v.y.abs() + v.z.abs() + o.length()) * f64::EPSILON * 256.0;
            if !(local.x.is_finite() && local.y.is_finite())
                || (height(frame, v) - h).abs() + e > tol
            {
                return None;
            }
            error = error.max(e);
            let w = p[3] / max_w;
            span.push([local.x * w, local.y * w, w]);
        }
        out.push(span);
    }
    (error < tol * 0.1).then_some((out, error))
}

/// `a b' - b a'` in Bernstein form, and a bound on its rounding.
fn determinant(a: &[f64], b: &[f64]) -> Option<(Vec<f64>, f64)> {
    let (ab, e1) = product(a, &derivative(b))?;
    let (ba, e2) = product(b, &derivative(a))?;
    let max = |v: &[f64]| v.iter().map(|x| x.abs()).fold(0.0, f64::max);
    let scale = max(a) * max(&derivative(b)) + max(b) * max(&derivative(a));
    #[allow(clippy::cast_precision_loss)]
    let error = scale * f64::EPSILON * 128.0 * (a.len() * a.len()) as f64 + e1 + e2;
    Some((ab.iter().zip(&ba).map(|(x, y)| x - y).collect(), error))
}

/// Whether the span `c` runs along the line from `a` to `b` within `tol`, forward all the way.
fn line_span(c: &[H2], a: Point2, b: Point2, tol: f64, error: f64) -> Option<()> {
    let d = b - a;
    let length = d.length();
    if length <= tol {
        return None;
    }
    let d = d / length;
    let mut projected = Vec::new();
    let weights: Vec<f64> = c.iter().map(|p| p[2]).collect();
    for p in c {
        let v = point_of(*p) - a;
        let t = v.dot(d);
        if cross2(v, d).abs() + error > tol || t < -tol + error || t > length + tol - error {
            return None;
        }
        projected.push(t * p[2]);
    }
    // W T' - T W' > 0 proves the traversal oriented, not merely collinear
    let (der, mut err) = determinant(&weights, &projected)?;
    #[allow(clippy::cast_precision_loss)]
    let n2 = (c.len() * c.len()) as f64;
    err += error * n2 * 32.0;
    der.iter().all(|x| *x > err).then_some(())
}

/// The signed sweep of the span `c` about `center` on the circle of `radius`, proved: every
/// Bernstein coefficient of `x^2 + y^2 - r^2 w^2` within its rounding and the tolerance, the
/// angular determinant of one sign, the span in one open half plane about the centre; split in
/// halves (to depth twelve) where the last two are not proved whole.
fn circle_span(
    c: &[H2],
    center: Point2,
    radius: f64,
    tol: f64,
    source_error: f64,
    work: &mut usize,
) -> Option<f64> {
    let mut total = 0.0_f64;
    let mut todo = vec![(c.to_vec(), 0u32)];
    while let Some((c, depth)) = todo.pop() {
        charge(work, c.len())?;
        let x: Vec<f64> = c.iter().map(|p| p[0] - center.x * p[2]).collect();
        let y: Vec<f64> = c.iter().map(|p| p[1] - center.y * p[2]).collect();
        let w: Vec<f64> = c.iter().map(|p| p[2]).collect();
        let min_w = w.iter().copied().fold(f64::INFINITY, f64::min);
        if min_w < 1e-12 {
            return None;
        }
        let (xx, ex) = product(&x, &x)?;
        let (yy, ey) = product(&y, &y)?;
        let (ww, ew) = product(&w, &w)?;
        let max = |v: &[f64]| v.iter().map(|x| x.abs()).fold(0.0, f64::max);
        let scale = max(&x).powi(2) + max(&y).powi(2) + radius * radius * max(&w).powi(2);
        #[allow(clippy::cast_precision_loss)]
        let (n, n2) = (c.len() as f64, (c.len() * c.len()) as f64);
        let coordinate_error = source_error
            + (center.length() + radius) * f64::EPSILON * (f64::from(depth) + 1.0) * 128.0 * n;
        let roundoff = scale * f64::EPSILON * 256.0 * n2 + ex + ey + radius * radius * ew;
        let residual = xx
            .iter()
            .zip(&yy)
            .zip(&ww)
            .map(|((a, b), w)| (a + b - radius * radius * w).abs())
            .fold(0.0, f64::max);
        let radial_tol = tol - coordinate_error;
        if radial_tol <= 0.0
            || (residual + roundoff) / (min_w * min_w) > radial_tol * (2.0 * radius - radial_tol)
        {
            return None;
        }
        let (angular, mut error) = determinant(&x, &y)?;
        let extent = x.iter().chain(&y).map(|v| v.abs()).fold(0.0, f64::max);
        error += coordinate_error * extent * n2 * 32.0;
        let positive = angular.iter().all(|v| *v > error);
        let negative = angular.iter().all(|v| *v < -error);
        let a = point_of(*c.first()?) - center;
        let b = point_of(*c.last()?) - center;
        let mid = a + b;
        // one open half plane and a derivative of one sign: no hidden turn
        let half_plane = mid.length() > radius * 1e-8
            && c.iter()
                .all(|p| (point_of(*p) - center).dot(mid) > coordinate_error * mid.length());
        if (positive || negative) && half_plane {
            let sweep = cross2(a, b).atan2(a.dot(b));
            if !((positive && sweep > 0.0) || (negative && sweep < 0.0))
                || (total != 0.0 && total.signum() != sweep.signum())
            {
                return None;
            }
            total += sweep;
            continue;
        }
        if depth >= 12 {
            return None;
        }
        // the right half first off the stack after the left: sweeps add in order either way
        let (left, right) = halves(&c);
        todo.push((right, depth + 1));
        todo.push((left, depth + 1));
    }
    Some(total)
}

/// The run read in `frame` exactly, as lines, circular arcs and free-form segments (each
/// free-form curve one segment); with `each_span`, a curve of several Bézier spans that is no
/// line or arc as one segment per span. `None` when it does not lie in a plane parallel to the
/// frame within `tol`, is not read exactly, is not finite, or goes over the `work` budget.
#[must_use]
pub fn planar_pieces(
    k: &Kernel,
    run: &Run,
    frame: &Frame,
    tol: f64,
    work: &mut usize,
    each_span: bool,
) -> Option<Vec<Seg2>> {
    if !(tol.is_finite() && tol > 0.0) {
        return None;
    }
    charge(work, 1)?;
    let (ts, te) = run.ends_along();
    if !(ts.is_finite() && te.is_finite() && run.interval.min() < run.interval.max()) {
        return None;
    }
    let (a3, b3) = (
        run.curve.evaluate_point(k, ts).ok()?,
        run.curve.evaluate_point(k, te).ok()?,
    );
    let (a, b) = (to_local(frame, a3), to_local(frame, b3));
    let finite = |p: Point3| p.x.is_finite() && p.y.is_finite() && p.z.is_finite();
    if !finite(a3)
        || !finite(b3)
        || a3.distance(run.ends[0]) > tol
        || b3.distance(run.ends[1]) > tol
    {
        return None;
    }
    let level = height(frame, a3);
    let o = Vec3::new(frame.origin[0], frame.origin[1], frame.origin[2]);
    if let CurveKind::Line(_) = run.curve.kind() {
        let error = (a3.length() + b3.length() + o.length()) * f64::EPSILON * 256.0;
        return (error < tol * 0.1
            && a.distance(b) > tol
            && (level - height(frame, b3)).abs() + error <= tol)
            .then_some(vec![Seg2::Line { a, b }]);
    }
    let (spans, error) = controls(spans_of(run, work)?, frame, level, tol)?;
    let first = point_of(*spans.first()?.first()?);
    let last = point_of(*spans.last()?.last()?);
    let joins = spans.iter().zip(spans.iter().skip(1)).all(|(p, q)| {
        p.last()
            .zip(q.first())
            .is_some_and(|(x, y)| point_of(*x).distance(point_of(*y)) + 2.0 * error <= tol)
    });
    if !joins || first.distance(a) + error > tol || last.distance(b) + error > tol {
        return None;
    }
    if a.distance(b) > tol
        && spans
            .iter()
            .all(|s| line_span(s, a, b, tol, error).is_some())
    {
        return Some(vec![Seg2::Line { a, b }]);
    }
    let label = circle_label(run.curve).and_then(|(c, n, r)| {
        let nl = n.dot(Vec3::new(0.0, 0.0, 0.0) + super::profile::axes(frame).3);
        // the label's plane parallel to the frame's, within the tolerance over the radius
        let tilt = r * n.cross(super::profile::axes(frame).3).length();
        (nl.is_finite() && tilt <= tol).then(|| (to_local(frame, c), r))
    });
    let candidate = label.or_else(|| through_three(k, run, (ts, te), frame, a, tol));
    candidate
        .and_then(|(center, radius)| arc(&spans, center, radius, a, b, tol, error, work))
        .or_else(|| free_form(&spans, a, b, tol))
        .or_else(|| {
            if each_span {
                span_by_span(&spans, a, b, tol)
            } else {
                None
            }
        })
}

/// The circle through `a` and the run's points 5/16 and 11/16 along: a proposal only.
fn through_three(
    k: &Kernel,
    run: &Run,
    (ts, te): (f64, f64),
    frame: &Frame,
    a: Point2,
    tol: f64,
) -> Option<(Point2, f64)> {
    let at = |f: f64| {
        run.curve
            .evaluate_point(k, ts + (te - ts) * f)
            .ok()
            .map(|p| to_local(frame, p) - a)
    };
    let (q, r) = (at(5.0 / 16.0)?, at(11.0 / 16.0)?);
    let det = 2.0 * cross2(q, r);
    if !det.is_finite() || det.abs() < 1e-300 {
        return None;
    }
    let offset = Vec2::new(
        (q.dot(q) * r.y - r.dot(r) * q.y) / det,
        (q.x * r.dot(r) - r.x * q.dot(q)) / det,
    );
    let (center, radius) = (a + offset, offset.length());
    (radius.is_finite() && radius > tol && center.x.is_finite() && center.y.is_finite())
        .then_some((center, radius))
}

/// The spans (from `a` to `b`) as one circular arc about `center` of `radius`, when every span
/// is proved on it ([`circle_span`]) and turning one way; a whole turn as its two halves.
#[allow(clippy::too_many_arguments)]
fn arc(
    spans: &[Vec<H2>],
    center: Point2,
    radius: f64,
    a: Point2,
    b: Point2,
    tol: f64,
    error: f64,
    work: &mut usize,
) -> Option<Vec<Seg2>> {
    let mut sweep: f64 = 0.0;
    for span in spans {
        let ds = circle_span(span, center, radius, tol, error, work)?;
        if sweep != 0.0 && sweep.signum() != ds.signum() {
            return None;
        }
        sweep += ds;
    }
    let ac = a - center;
    let start = ac.y.atan2(ac.x);
    let end = center + Vec2::new((start + sweep).cos(), (start + sweep).sin()) * radius;
    // the arc ends where the curve does, as closely as a profile loop must close
    if end.distance(b) + error > tol.min(ARC_END_TOL) {
        return None;
    }
    let angular_tol = tol / radius;
    if sweep.abs() > TAU + angular_tol || sweep.abs() < angular_tol {
        return None;
    }
    if a.distance(b) < tol {
        if (sweep.abs() - TAU).abs() > angular_tol {
            return None;
        }
        let h = sweep.signum() * PI;
        return Some(vec![
            Seg2::Arc {
                center,
                radius,
                start: principal(start),
                sweep: h,
            },
            Seg2::Arc {
                center,
                radius,
                start: principal(start + h),
                sweep: h,
            },
        ]);
    }
    Some(vec![Seg2::Arc {
        center,
        radius,
        start: principal(start),
        sweep,
    }])
}

/// The angle `a` in (-π, π]: unchanged when it is already (so an angle in range keeps its bits),
/// else turned by whole turns (`atan2` answers -π for a point on the negative x axis at y = -0,
/// and a whole turn's second half starts past π).
fn principal(a: f64) -> f64 {
    if a > -PI && a <= PI {
        return a;
    }
    let r = a.rem_euclid(TAU);
    if r > PI {
        r - TAU
    } else {
        r
    }
}

/// Several spans end to end from `a` to `b`, each the segment it is: a polynomial span along its
/// chord a line, the others as [`free_form`] reads them. Spans whose ends meet are left out;
/// each segment starts where the one before ended.
fn span_by_span(spans: &[Vec<H2>], a: Point2, b: Point2, tol: f64) -> Option<Vec<Seg2>> {
    let spans: Vec<&Vec<H2>> = spans
        .iter()
        .filter(|s| {
            s.first()
                .zip(s.last())
                .is_some_and(|(p, q)| point_of(*p).distance(point_of(*q)) > tol)
        })
        .collect();
    if spans.len() < 2 {
        return None;
    }
    let mut out = Vec::new();
    let mut start = a;
    let count = spans.len();
    for (i, span) in spans.into_iter().enumerate() {
        let end = if i + 1 == count {
            b
        } else {
            point_of(*span.last()?)
        };
        let pts: Vec<Point2> = span.iter().map(|p| point_of(*p)).collect();
        let seg = match free_form(std::slice::from_ref(span), start, end, tol) {
            Some(mut s) => s.pop()?,
            None if span.iter().all(|p| (p[2] - 1.0).abs() <= 1e-12) && off_chord(&pts) <= tol => {
                Seg2::Line { a: start, b: end }
            }
            None => return None,
        };
        out.push(seg);
        start = end;
    }
    Some(out)
}

/// How far the points lie off the chord between the first and the last.
fn off_chord(pts: &[Point2]) -> f64 {
    let (Some(first), Some(last)) = (pts.first().copied(), pts.last().copied()) else {
        return f64::INFINITY;
    };
    let chord = last - first;
    let len = chord.length();
    if !(len > 0.0) {
        return f64::INFINITY;
    }
    pts.iter()
        .map(|p| cross2(chord, *p - first).abs())
        .fold(0.0, f64::max)
        / len
}

/// One Bézier span that is neither a line nor an arc, as the exact free-form segment it is (a
/// polynomial Bézier, a polynomial quadratic as the conic of weight 1, a conic), its ends `a` and
/// `b`. `None` for several spans, degrees past [`MAX_BEZIER_DEGREE`], rational spans other than
/// conics, and control polygons with no area.
fn free_form(spans: &[Vec<H2>], a: Point2, b: Point2, tol: f64) -> Option<Vec<Seg2>> {
    let [span] = spans else { return None };
    let pts: Vec<Point2> = span.iter().map(|p| point_of(*p)).collect();
    let (first, last) = (*pts.first()?, *pts.last()?);
    if first.distance(a) > tol
        || last.distance(b) > tol
        || first.distance(last) <= tol
        || !(off_chord(&pts) > tol)
    {
        return None;
    }
    let rational = span.iter().any(|p| (p[2] - 1.0).abs() > 1e-12);
    let seg = match (pts.len(), rational) {
        (4, false) => Seg2::Cubic {
            p0: a,
            p1: *pts.get(1)?,
            p2: *pts.get(2)?,
            p3: b,
        },
        // a polynomial quadratic is the conic of weight 1, as a profile writes one (a font's
        // quadratic curves)
        (3, false) => Seg2::Conic {
            a,
            apex: *pts.get(1)?,
            b,
            w: 1.0,
        },
        (n, false) if (2..=MAX_BEZIER_DEGREE + 1).contains(&n) => {
            let mut p = [Vec2::ZERO; super::profile::MAX_BEZIER_CTRL];
            for (slot, q) in p.iter_mut().zip(&pts) {
                *slot = *q;
            }
            *p.first_mut()? = a;
            *p.get_mut(n - 1)? = b;
            Seg2::Bezier {
                n: u8::try_from(n - 1).ok()?,
                p,
            }
        }
        (3, true) => {
            // weights normalised to 1 at the ends: the middle one is the conic's
            let (w0, w1, w2) = (span.first()?[2], span.get(1)?[2], span.get(2)?[2]);
            let w = w1 / (w0 * w2).sqrt();
            if !(w.is_finite() && w > 1e-9) {
                return None;
            }
            Seg2::Conic {
                a,
                apex: *pts.get(1)?,
                b,
                w,
            }
        }
        _ => return None,
    };
    Some(vec![seg])
}

/// The run in `frame` exactly, for a sketch to project it: as [`planar_pieces`], and else a
/// curve of several Bézier spans as the one B-spline it is (its clamped segment over the run's
/// interval, its knots and weights its own). `None` when it does not lie in a plane parallel to
/// the frame or is not read exactly.
#[must_use]
pub fn planar_curves(
    k: &Kernel,
    run: &Run,
    frame: &Frame,
    tol: f64,
    work: &mut usize,
) -> Option<Vec<PlanarPiece>> {
    if let Some(segs) = planar_pieces(k, run, frame, tol, work, false) {
        return Some(segs.into_iter().map(PlanarPiece::Seg).collect());
    }
    one_spline(run, frame, tol, work).map(|s| vec![PlanarPiece::Spline(s)])
}

/// The run's net over its interval, the way it runs, as one clamped B-spline in `frame`.
fn one_spline(run: &Run, frame: &Frame, tol: f64, work: &mut usize) -> Option<PlanarSpline> {
    if !(tol.is_finite() && tol > 0.0) {
        return None;
    }
    let (degree, knots, ctrl) = segment(run, work)?;
    let (knots, ctrl) = if run.forward {
        (knots, ctrl)
    } else {
        let sum = knots.first()? + knots.last()?;
        (
            knots.iter().rev().map(|t| sum - t).collect(),
            ctrl.into_iter().rev().collect(),
        )
    };
    let max_w = ctrl.iter().map(|p| p[3]).fold(0.0, f64::max);
    if !(max_w.is_finite() && max_w > 0.0) {
        return None;
    }
    let project = |p: &[f64; 4]| Point3::new(p[0] / p[3], p[1] / p[3], p[2] / p[3]);
    let (first, last) = (project(ctrl.first()?), project(ctrl.last()?));
    if first.distance(run.ends[0]) > tol || last.distance(run.ends[1]) > tol {
        return None;
    }
    let level = height(frame, first);
    let o = Vec3::new(frame.origin[0], frame.origin[1], frame.origin[2]);
    let mut points = Vec::new();
    let mut weights = Vec::new();
    for p in &ctrl {
        if p[3] / max_w < 1e-12 {
            return None;
        }
        let v = project(p);
        let error = (v.x.abs() + v.y.abs() + v.z.abs() + o.length()) * f64::EPSILON * 256.0;
        if (height(frame, v) - level).abs() + error > tol || error > tol * 0.1 {
            return None;
        }
        let local = to_local(frame, v);
        if !(local.x.is_finite() && local.y.is_finite()) {
            return None;
        }
        points.push(local);
        weights.push(p[3] / max_w);
    }
    if weights.iter().all(|w| (w - 1.0).abs() <= 1e-12) {
        weights.clear();
    }
    Some(PlanarSpline {
        degree,
        knots,
        points,
        weights,
    })
}

/// Edge `e` of `b` run forward (from its start vertex) or back, its vertices' points its ends.
#[must_use]
pub fn edge_run(b: &Brep, e: EdgeId, forward: bool) -> Option<Run<'_>> {
    let ed = b.edge(e).ok()?;
    let curve = ed.curve.as_ref()?;
    let v0 = edge::start_vertex(b, e).ok()??;
    let v1 = edge::end_vertex(b, e).ok()??;
    let (p0, p1) = (b.vertex(v0).ok()?.point, b.vertex(v1).ok()?.point);
    Some(Run {
        curve,
        interval: ed.interval,
        forward,
        ends: if forward { [p0, p1] } else { [p1, p0] },
    })
}

/// How far [`planar_edges`] lets a piece it reads as a line or an arc lie from the edge's curve,
/// so how far a piece's ends may lie from the edge's vertices: ten times the larger of 1e-9 and
/// 1e-7 of the body's size. `None` for a body without edges.
#[must_use]
pub fn planar_edges_tolerance(b: &Brep) -> Option<f64> {
    super::measure::body_size(b).map(|size| (size * 1e-7).max(1e-9) * 10.0)
}

/// Edges `edges` of `b`, each run forward, in `frame`'s coordinates exactly
/// ([`planar_curves`], to [`planar_edges_tolerance`]): `None` for an edge that does not lie in a
/// plane parallel to the frame, is not read exactly, or is not there.
#[must_use]
pub fn planar_edges(
    k: &Kernel,
    b: &Brep,
    frame: &Frame,
    edges: &[EdgeId],
) -> Vec<Option<Vec<PlanarPiece>>> {
    let Some(tol) = planar_edges_tolerance(b) else {
        return vec![None; edges.len()];
    };
    edges
        .iter()
        .map(|e| {
            let mut work = 262_144usize;
            edge_run(b, *e, true).and_then(|run| planar_curves(k, &run, frame, tol, &mut work))
        })
        .collect()
}
