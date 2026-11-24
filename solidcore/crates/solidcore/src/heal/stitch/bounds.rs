//! Outward-rounded bounds for the geometry a stitch closure identifies.
//!
//! No upstream counterpart. The curve-pair bound is a bound on a parameter
//! correspondence, hence on both directed Hausdorff distances. B-splines are
//! enclosed on every knot span by homogeneous interval de Boor evaluation; no
//! rounded control-polygon conversion or sampled distance enters the proof.
//! Ellipses, circles, hyperbolas and parabolas are evaluated through their stored
//! rational B-spline (`Curve::evaluate_point`), so that is what is enclosed. Planar
//! conics (`Conic`) are enclosed by interval evaluation of their rational
//! parametrisation, each ellipse branch on its own; two conics of one kind and
//! rotation at the same parameters are bounded through the difference of their
//! coefficients. A Hermite curve is the cubic `P1 + t (D1 + t (C + t D))` its
//! evaluation computes from the stored coefficients; it is enclosed by the hull of
//! the Bernstein coefficients of that cubic over the parameter range, computed in
//! outward-rounded intervals, and two Hermite curves at the same parameters through
//! their coefficient differences. A composite curve is enclosed by the union of its
//! segments' enclosures over the parts of the range they cover. Iso, offset,
//! projected, on-surface, in-volume and tangent-field curves have no enclosure here
//! and are refused.
//!
//! Deviations: the only recursion is into a composite's segments, refused past
//! `MAX_GEOMETRY_NESTING` (32) composites, the construction limit. Other work is
//! bounded by the stored knot and segment counts and the kernel's NURBS degree
//! limit. Curves without an implemented exact enclosure, non-positive weights,
//! extrapolation and non-finite bounds are refused.

use crate::blend::fillet_section::interval::Ivl;
use crate::curve::composite_curve::CompositeCurve;
use crate::curve::conic::{Conic, ConicType};
use crate::curve::curve::{Curve, CurveKind};
use crate::curve::hermite_curve::HermiteCurve;
use crate::curve::line::Line;
use crate::geom::extent1d::Extent1d;
use crate::math::Point3;
use crate::nurbs::crv_basic::Curve as Nurbs;
use crate::nurbs::knots::MAX_DEGREE;
use crate::nurbs::math::{CPoint, NOW, NOZ};
use crate::surface::surface::p2::MAX_GEOMETRY_NESTING;
use crate::{Result, SolidError};

const OP: &str = "heal::stitch::bounds";
type Box3 = [Ivl; 3];
type Box4 = [Ivl; 4];

fn invalid(reason: &'static str) -> SolidError {
    SolidError::invalid_input(OP, reason)
}

fn unsupported() -> SolidError {
    SolidError::Unsupported {
        operation: OP,
        reason: "stitch gap enclosure is implemented for lines, positive-weight B-splines, \
                 the ellipses, circles, hyperbolas and parabolas evaluated through them, \
                 planar conics, Hermite curves and composites of these only",
    }
}

fn at<T: Copy>(values: &[T], index: usize) -> Result<T> {
    values
        .get(index)
        .copied()
        .ok_or_else(|| invalid("invalid spline storage"))
}

fn finite(interval: Ivl) -> bool {
    interval.lo.is_finite() && interval.hi.is_finite() && interval.lo <= interval.hi
}

fn norm(v: Box3) -> Result<f64> {
    if !v.into_iter().all(finite) {
        return Err(invalid("gap enclosure is not finite"));
    }
    let scale = v.into_iter().map(Ivl::mag).fold(0.0_f64, f64::max);
    if scale == 0.0 {
        return Ok(0.0);
    }
    // Scale before squaring: small distances must not underflow to a zero bound.
    let mut sum = Ivl::from(0.0);
    for coordinate in v {
        // Direct endpoint division avoids overflowing 1/scale for subnormals.
        let scaled = Ivl::checked(coordinate.lo / scale, coordinate.hi / scale);
        sum = sum + scaled * scaled;
    }
    let length = sum.sqrt() * Ivl::from(scale);
    if !finite(length) {
        return Err(invalid("gap enclosure overflows"));
    }
    Ok(length.hi.max(0.0))
}

fn point_box(p: Point3) -> Result<Box3> {
    let coordinates = [p.x, p.y, p.z];
    if !coordinates.into_iter().all(f64::is_finite) {
        return Err(invalid("non-finite point"));
    }
    Ok(coordinates.map(Ivl::from))
}

fn difference(a: Box3, b: Box3) -> Box3 {
    core::array::from_fn(|i| {
        let x = a.get(i).copied().unwrap_or(Ivl::whole());
        let y = b.get(i).copied().unwrap_or(Ivl::whole());
        if x.lo == x.hi && x == y {
            Ivl::from(0.0)
        } else {
            x - y
        }
    })
}

/// Upper bound on the exact Euclidean distance between the stored points.
pub(crate) fn points(a: Point3, b: Point3) -> Result<f64> {
    norm(difference(point_box(a)?, point_box(b)?))
}

/// Upper bound on the positive increase between two stored tolerances.
pub(super) fn growth(before: f64, after: f64) -> Result<f64> {
    if ![before, after]
        .into_iter()
        .all(|v| v.is_finite() && v >= 0.0)
    {
        return Err(invalid("non-finite or negative tolerance"));
    }
    if after <= before {
        return Ok(0.0);
    }
    let increase = Ivl::from(after) - Ivl::from(before);
    if !finite(increase) {
        return Err(invalid("tolerance growth overflows"));
    }
    Ok(increase.hi)
}

fn checked_range(range: [f64; 2]) -> Result<Ivl> {
    if !range.into_iter().all(f64::is_finite) {
        return Err(invalid("non-finite curve interval"));
    }
    Ok(Ivl::new(range[0].min(range[1]), range[0].max(range[1])))
}

fn homogeneous(c: CPoint) -> Result<Box4> {
    let coordinates = [
        c.x,
        c.y,
        if c.z == NOZ { 0.0 } else { c.z },
        if c.w == NOW { 1.0 } else { c.w },
    ];
    if !coordinates.into_iter().all(f64::is_finite) || coordinates.last().is_none_or(|w| *w <= 0.0)
    {
        return Err(invalid("non-finite spline data or non-positive weight"));
    }
    Ok(coordinates.map(Ivl::from))
}

fn validate(n: &Nurbs, range: Ivl) -> Result<()> {
    let p = n.degree();
    let count = n.cpts().len();
    if p > MAX_DEGREE + 1 || count <= p || count.checked_add(p + 1) != Some(n.knots().len()) {
        return Err(invalid("invalid spline degree or storage"));
    }
    if !n.knots().iter().all(|k| k.is_finite())
        || n.knots().windows(2).any(|w| matches!(w, [a, b] if a > b))
    {
        return Err(invalid("invalid knot vector"));
    }
    if at(n.knots(), p)? >= at(n.knots(), count)?
        || range.lo < at(n.knots(), p)?
        || range.hi > at(n.knots(), count)?
    {
        return Err(invalid("curve gap enclosure does not extrapolate"));
    }
    let first = at(n.cpts(), 0)?;
    for &c in n.cpts() {
        if (c.w == NOW) != (first.w == NOW) || (c.z == NOZ) != (first.z == NOZ) {
            return Err(invalid("mixed homogeneous coordinate storage"));
        }
        homogeneous(c)?;
    }
    Ok(())
}

/// The exact rational polynomial on one nonempty knot span, evaluated in intervals.
fn piece(n: &Nurbs, span: usize, t: Ivl) -> Result<Box3> {
    let p = n.degree();
    let first = span
        .checked_sub(p)
        .ok_or_else(|| invalid("invalid spline span"))?;
    let mut d = Vec::new();
    let mut min_weight = f64::INFINITY;
    for j in 0..=p {
        let h = homogeneous(at(n.cpts(), first + j)?)?;
        min_weight = min_weight.min(at(&h, 3)?.lo);
        d.push(h);
    }
    for r in 1..=p {
        for j in (r..=p).rev() {
            let i = first + j;
            let denominator =
                Ivl::from(at(n.knots(), i + p + 1 - r)?) - Ivl::from(at(n.knots(), i)?);
            if denominator.lo <= 0.0 {
                return Err(invalid("unproved knot denominator"));
            }
            let alpha = (t - Ivl::from(at(n.knots(), i)?)) / denominator;
            // On the selected span, the exact alpha is in [0, 1]. Intersecting
            // with that proved range removes rounding spill, not geometric error.
            let alpha = Ivl::new(alpha.lo.max(0.0), alpha.hi.min(1.0));
            if !finite(alpha) {
                return Err(invalid("invalid spline parameter enclosure"));
            }
            let left = at(&d, j - 1)?;
            let right = at(&d, j)?;
            let value = core::array::from_fn(|axis| {
                let a = left.get(axis).copied().unwrap_or(Ivl::whole());
                let b = right.get(axis).copied().unwrap_or(Ivl::whole());
                (Ivl::from(1.0) - alpha) * a + alpha * b
            });
            *d.get_mut(j)
                .ok_or_else(|| invalid("invalid spline workspace"))? = value;
        }
    }
    let h = at(&d, p)?;
    // Positive basis functions sum to one, so W is at least the least active weight.
    let w = at(&h, 3)?;
    let w = Ivl::new(w.lo.max(min_weight), w.hi);
    if !finite(w) || w.lo <= 0.0 {
        return Err(invalid("unproved spline denominator"));
    }
    let result = [at(&h, 0)? / w, at(&h, 1)? / w, at(&h, 2)? / w];
    if !result.into_iter().all(finite) {
        return Err(invalid("curve enclosure overflows"));
    }
    Ok(result)
}

fn spline_box(n: &Nurbs, range: Ivl) -> Result<Box3> {
    validate(n, range)?;
    let mut result: Option<Box3> = None;
    for (span, pair) in n.knots().windows(2).enumerate() {
        let [a, b] = pair else {
            continue;
        };
        if span < n.degree() || span >= n.cpts().len() || a >= b || *b < range.lo || *a > range.hi {
            continue;
        }
        let t = Ivl::new(range.lo.max(*a), range.hi.min(*b));
        let part = piece(n, span, t)?;
        result = Some(match result {
            None => part,
            Some(old) => core::array::from_fn(|i| {
                old.get(i)
                    .copied()
                    .unwrap_or(Ivl::whole())
                    .hull(part.get(i).copied().unwrap_or(Ivl::whole()))
            }),
        });
    }
    result.ok_or_else(|| invalid("no knot span covers the curve interval"))
}

fn line_box(line: &Line, t: Ivl) -> Result<Box3> {
    let natural = line.natural_interval();
    let step = line.step_interval();
    if !finite(t)
        || !finite(checked_range([natural.min(), natural.max()])?)
        || !finite(checked_range([step.min(), step.max()])?)
    {
        return Err(invalid("non-finite line parameterization"));
    }
    if t.lo < natural.min() || t.hi > natural.max() {
        return Err(invalid("curve gap enclosure does not extrapolate"));
    }
    let u = if line.is_bounded() {
        let length = Ivl::from(natural.max()) - Ivl::from(natural.min());
        if length.lo <= 0.0 || crate::geom::tol::is_zero(natural.length()) {
            return Err(invalid("degenerate line parameterization"));
        }
        let (start, direction) = if line.inside_out() {
            (natural.max(), -1.0)
        } else {
            (natural.min(), 1.0)
        };
        Ivl::from(step.min())
            + (t - Ivl::from(start)) / length
                * Ivl::from(direction)
                * (Ivl::from(step.max()) - Ivl::from(step.min()))
    } else if line.inside_out() {
        Ivl::from(0.0) - t
    } else {
        t
    };
    let p = point_box(line.line_point())?;
    let v = point_box(line.line_vector())?;
    if !line.line_scale().is_finite() {
        return Err(invalid("non-finite line scale"));
    }
    let result = core::array::from_fn(|i| {
        p.get(i).copied().unwrap_or(Ivl::whole())
            + u * v.get(i).copied().unwrap_or(Ivl::whole()) * Ivl::from(line.line_scale())
    });
    if !result.into_iter().all(finite) {
        return Err(invalid("line enclosure overflows"));
    }
    Ok(result)
}

/// Power-basis coefficients of a curve of degree at most three in its own parameter:
/// `P(t) = c[0] + c[1] t + c[2] t^2 + c[3] t^3`, each an enclosure of the exact
/// coefficient.
type Cubic = [Box3; 4];

fn axis(b: Box3, i: usize) -> Ivl {
    b.get(i).copied().unwrap_or(Ivl::whole())
}

fn coefficient(c: &Cubic, k: usize) -> Box3 {
    c.get(k).copied().unwrap_or([Ivl::whole(); 3])
}

/// The stored cubic of a Hermite curve, `P1 + t (D1 + t (C + t D))`: the polynomial
/// `HermiteCurve::evaluate_point` evaluates, with the stored power-basis `C` and `D`
/// (not recomputed from the end data), on its natural interval `[0, 1]`.
fn hermite_cubic(h: &HermiteCurve, t: Ivl) -> Result<Cubic> {
    let natural = h.natural_interval();
    if !finite(t) || t.lo < natural.min() || t.hi > natural.max() {
        return Err(invalid("curve gap enclosure does not extrapolate"));
    }
    let (c, d) = h.power_basis();
    let p1 = point_box(h.start_point())?;
    let d1 = point_box(h.start_derivative())?;
    Ok([p1, d1, point_box(c)?, point_box(d)?])
}

/// The coefficients in `s` of `P(t0 + (t1 - t0) s)`, the Taylor shift of `c` to `t0`
/// scaled by `h = t1 - t0`:
/// `e0 = P(t0)`, `e1 = h P'(t0)`, `e2 = h^2 P''(t0) / 2`, `e3 = h^3 P'''(t0) / 6`.
/// Every operation is an outward-rounded interval operation ([`Ivl`]: each end moved
/// past its rounding error), so each `e_k` holds the exact coefficient.
fn reparameterize(c: &Cubic, t0: f64, t1: f64) -> Cubic {
    let t = Ivl::from(t0);
    let h = Ivl::from(t1) - Ivl::from(t0);
    let (two, three) = (Ivl::from(2.0), Ivl::from(3.0));
    let [a0, a1, a2, a3] = [0, 1, 2, 3].map(|k| coefficient(c, k));
    let at = |b: Box3, i: usize| axis(b, i);
    [
        core::array::from_fn(|i| at(a0, i) + t * (at(a1, i) + t * (at(a2, i) + t * at(a3, i)))),
        core::array::from_fn(|i| h * (at(a1, i) + t * (two * at(a2, i) + three * t * at(a3, i)))),
        core::array::from_fn(|i| h * h * (at(a2, i) + three * t * at(a3, i))),
        core::array::from_fn(|i| h * h * h * at(a3, i)),
    ]
}

/// A box holding the cubic `e0 + e1 s + e2 s^2 + e3 s^3` for every `s` in `[0, 1]`:
/// the hull of its Bernstein coefficients `e0`, `e0 + e1/3`, `e0 + (2 e1 + e2)/3` and
/// `e0 + e1 + e2 + e3`. On `[0, 1]` the cubic is their convex combination (the
/// Bernstein basis is non-negative and sums to one), so it lies in their hull; each is
/// computed in outward-rounded intervals and holds the exact coefficient.
fn bernstein_hull(e: &Cubic) -> Result<Box3> {
    let three = Ivl::from(3.0);
    let two = Ivl::from(2.0);
    let [e0, e1, e2, e3] = [0, 1, 2, 3].map(|k| coefficient(e, k));
    let result: Box3 = core::array::from_fn(|i| {
        let (a, b, c, d) = (axis(e0, i), axis(e1, i), axis(e2, i), axis(e3, i));
        a.hull(a + b / three)
            .hull(a + (two * b + c) / three)
            .hull(a + b + c + d)
    });
    if !result.into_iter().all(finite) {
        return Err(invalid("cubic enclosure overflows"));
    }
    Ok(result)
}

fn hermite_box(h: &HermiteCurve, t: Ivl) -> Result<Box3> {
    bernstein_hull(&reparameterize(&hermite_cubic(h, t)?, t.lo, t.hi))
}

/// Bounds `|P(t) - Q(t)|` over `t` for two Hermite curves at the same parameters: their
/// difference is the cubic with the coefficient differences, enclosed like one curve.
/// Equal stored coefficients give exactly zero.
fn hermite_difference(first: &HermiteCurve, second: &HermiteCurve, t: Ivl) -> Result<f64> {
    let p = hermite_cubic(first, t)?;
    let q = hermite_cubic(second, t)?;
    let delta: Cubic = core::array::from_fn(|k| difference(coefficient(&p, k), coefficient(&q, k)));
    if delta.iter().flatten().all(|d| *d == Ivl::from(0.0)) {
        return Ok(0.0);
    }
    norm(bernstein_hull(&reparameterize(&delta, t.lo, t.hi))?)
}

/// The coefficients in `s` of a Hermite curve or a line over the affine parameter map
/// `t = range[0] + (range[1] - range[0]) s`, `s` in `[0, 1]`: the cubic of
/// [`hermite_cubic`] or the affine `p + u(t) v scale` of [`line_box`]; `None` for the
/// other kinds.
fn polynomial(curve: &Curve, range: [f64; 2]) -> Result<Option<Cubic>> {
    let [r0, r1] = range;
    let t = checked_range(range)?;
    match curve.kind() {
        CurveKind::Hermite(h) => Ok(Some(reparameterize(&hermite_cubic(h, t)?, r0, r1))),
        CurveKind::Line(line) => {
            // The same parameterization checks as the enclosure.
            line_box(line, t)?;
            let natural = line.natural_interval();
            let step = line.step_interval();
            let ds = Ivl::from(r1) - Ivl::from(r0);
            // u = u0 + slope s
            let (u0, slope) = if line.is_bounded() {
                let length = Ivl::from(natural.max()) - Ivl::from(natural.min());
                let (start, direction) = if line.inside_out() {
                    (natural.max(), -1.0)
                } else {
                    (natural.min(), 1.0)
                };
                let rate = Ivl::from(direction) * (Ivl::from(step.max()) - Ivl::from(step.min()));
                (
                    Ivl::from(step.min()) + (Ivl::from(r0) - Ivl::from(start)) / length * rate,
                    ds / length * rate,
                )
            } else if line.inside_out() {
                (Ivl::from(0.0) - Ivl::from(r0), Ivl::from(0.0) - ds)
            } else {
                (Ivl::from(r0), ds)
            };
            let p = point_box(line.line_point())?;
            let v = point_box(line.line_vector())?;
            let scale = Ivl::from(line.line_scale());
            let zero = [Ivl::from(0.0); 3];
            Ok(Some([
                core::array::from_fn(|i| axis(p, i) + u0 * axis(v, i) * scale),
                core::array::from_fn(|i| slope * axis(v, i) * scale),
                zero,
                zero,
            ]))
        }
        _ => Ok(None),
    }
}

/// Encloses a composite over `t`: the hull of its segments' enclosures over the parts of
/// `t` they cover. The segments lie end to end from the first segment's natural minimum,
/// each over its natural interval's length; a segment's parameter is its start plus
/// (`same_sense`) or its end minus the offset into its span, as
/// `CompositeCurve::calculate_bounding_box` maps them. The span ends are accumulated in
/// outward-rounded intervals, so every segment that may meet `t` is included over a range
/// that holds its exact part, clipped to its natural interval. `depth` counts the
/// composites above this one and is refused at [`MAX_GEOMETRY_NESTING`].
fn composite_box(c: &CompositeCurve, t: Ivl, depth: usize) -> Result<Box3> {
    if depth >= MAX_GEOMETRY_NESTING {
        return Err(invalid("composite curves nest too deeply"));
    }
    let natural = c.natural_interval();
    if c.segments().is_empty()
        || !finite(t)
        || !finite(checked_range([natural.min(), natural.max()])?)
        || natural.has_negative_length()
        || t.lo < natural.min()
        || t.hi > natural.max()
    {
        return Err(invalid("curve gap enclosure does not extrapolate"));
    }
    let mut result: Option<Box3> = None;
    let mut start: Option<Ivl> = None;
    for s in c.segments() {
        let seg = s.curve.natural_interval();
        if !seg.min().is_finite() || !seg.max().is_finite() || seg.has_negative_length() {
            return Err(invalid("invalid composite segment interval"));
        }
        let length = Ivl::from(seg.max()) - Ivl::from(seg.min());
        let span_start = start.unwrap_or_else(|| Ivl::from(seg.min()));
        let span_end = span_start + length;
        start = Some(span_end);
        if span_start.lo > t.hi || span_end.hi < t.lo {
            continue;
        }
        // The offset into the span: in [0, length] and in t - span_start.
        let offset = Ivl::new(t.lo, t.hi) - span_start;
        let offset = Ivl::new(offset.lo.max(0.0), offset.hi.min(length.hi));
        if offset.lo > offset.hi || offset.lo.is_nan() || offset.hi.is_nan() {
            continue;
        }
        let local = if s.same_sense {
            Ivl::from(seg.min()) + offset
        } else {
            Ivl::from(seg.max()) - offset
        };
        // The exact segment parameter is in the segment's natural interval.
        let local = Ivl::new(local.lo.max(seg.min()), local.hi.min(seg.max()));
        if local.lo > local.hi || local.lo.is_nan() || local.hi.is_nan() {
            continue;
        }
        let part = curve_box_at(&s.curve, local, depth + 1)?;
        result = Some(match result {
            None => part,
            Some(old) => core::array::from_fn(|i| axis(old, i).hull(axis(part, i))),
        });
    }
    result.ok_or_else(|| invalid("no composite segment covers the curve interval"))
}

/// A conic's position in its natural frame, as coefficients over functions of `t`:
/// `x = cx g_x(t) + x0(t)`, `y = cy g_y(t)` (`Conic::evaluate_point` before its
/// rotation and translation). `x0` is the line's `t`, the same for two lines at the
/// same parameter, so a difference of two conics of one kind is
/// `(cx - cx') g_x, (cy - cy') g_y` exactly.
struct ConicTerms {
    cx: f64,
    gx: Ivl,
    x0: Ivl,
    cy: f64,
    gy: Ivl,
}

fn square(t: Ivl) -> Ivl {
    let p = t * t;
    // The exact square is non-negative; only rounding spill is clipped.
    Ivl::new(p.lo.max(0.0), p.hi)
}

/// `g_x`, `g_y` and `x0` of one conic kind over a parameter range that does not
/// cross an ellipse's branch point `t = 1`.
fn conic_branch(conic: &Conic, t: Ivl, sign: f64) -> Result<ConicTerms> {
    let one = Ivl::from(1.0);
    let two = Ivl::from(2.0);
    let zero = Ivl::from(0.0);
    let (cx, gx, x0, cy, gy) = match conic.conic_type() {
        ConicType::Ellipse => {
            let t2 = square(t);
            let denominator = one + t2;
            (
                conic.a(),
                Ivl::from(sign) * ((one - t2) / denominator),
                zero,
                conic.b(),
                Ivl::from(sign) * ((two * t) / denominator),
            )
        }
        ConicType::Hyperbola => {
            let t2 = square(t);
            let denominator = one - t2;
            if denominator.lo <= 0.0 {
                return Err(invalid("hyperbola parameter at or past its asymptote"));
            }
            (
                conic.a(),
                (one + t2) / denominator,
                zero,
                conic.b(),
                (two * t) / denominator,
            )
        }
        ConicType::Parabola => (conic.a(), square(t), zero, conic.a(), two * t),
        ConicType::Line => (0.0, zero, t, conic.a(), one),
        ConicType::Unknown => return Err(unsupported()),
    };
    Ok(ConicTerms { cx, gx, x0, cy, gy })
}

/// [`ConicTerms`] over `t`, the ellipse's two branches (`t <= 1`, and `t > 1` as
/// `t - 2` with the sign flipped) each enclosed and hulled.
fn conic_terms(conic: &Conic, t: Ivl) -> Result<ConicTerms> {
    let natural = conic.natural_interval();
    if !finite(t) || t.lo < natural.min() || t.hi > natural.max() {
        return Err(invalid("curve gap enclosure does not extrapolate"));
    }
    if !(conic.a().is_finite() && conic.b().is_finite()) {
        return Err(invalid("non-finite conic"));
    }
    if conic.conic_type() != ConicType::Ellipse || t.hi <= 1.0 {
        return conic_branch(conic, t, 1.0);
    }
    let right = conic_branch(conic, Ivl::new(t.lo.max(1.0), t.hi) - Ivl::from(2.0), -1.0)?;
    if t.lo > 1.0 {
        return Ok(right);
    }
    let left = conic_branch(conic, Ivl::new(t.lo, 1.0), 1.0)?;
    Ok(ConicTerms {
        gx: left.gx.hull(right.gx),
        gy: left.gy.hull(right.gy),
        x0: left.x0.hull(right.x0),
        ..left
    })
}

/// Rotates a natural-frame vector as `Conic::evaluate_point` does (its stored
/// sine and cosine, only when the angle is not zero).
fn conic_rotate(conic: &Conic, x: Ivl, y: Ivl) -> [Ivl; 2] {
    if conic.theta_rad() == 0.0 {
        return [x, y];
    }
    let (sin, cos) = conic.sin_cos_theta();
    let (sin, cos) = (Ivl::from(sin), Ivl::from(cos));
    [x * cos + y * sin, y * cos - x * sin]
}

/// Encloses a planar conic (`Conic::evaluate_point`, `z = 0`) over `t`.
fn conic_box(conic: &Conic, t: Ivl) -> Result<Box3> {
    let terms = conic_terms(conic, t)?;
    let x = Ivl::from(terms.cx) * terms.gx + terms.x0;
    let y = Ivl::from(terms.cy) * terms.gy;
    let [x, y] = conic_rotate(conic, x, y);
    let origin = conic.origin();
    if !(origin.x.is_finite() && origin.y.is_finite()) {
        return Err(invalid("non-finite conic"));
    }
    let result = [
        x + Ivl::from(origin.x),
        y + Ivl::from(origin.y),
        Ivl::from(0.0),
    ];
    if !result.into_iter().all(finite) {
        return Err(invalid("conic enclosure overflows"));
    }
    Ok(result)
}

/// Two conics of one kind and one rotation at the same parameters differ by
/// `R ((cx - cx') g_x, (cy - cy') g_y) + (o - o')` exactly, so the bound follows
/// the difference of their coefficients rather than the two curves' widths.
/// `None` when the two are not of that form.
fn conic_difference(first: &Conic, second: &Conic, t: Ivl) -> Result<Option<f64>> {
    if first.conic_type() != second.conic_type()
        || first.theta_rad() != second.theta_rad()
        || first.sin_cos_theta() != second.sin_cos_theta()
    {
        return Ok(None);
    }
    let p = conic_terms(first, t)?;
    let q = conic_terms(second, t)?;
    let coefficient = |a: f64, b: f64| {
        if a == b {
            Ivl::from(0.0)
        } else {
            Ivl::from(a) - Ivl::from(b)
        }
    };
    // The same kind at the same parameter: the same g and x0, so only the
    // coefficients differ.
    let (o, r) = (first.origin(), second.origin());
    if ![o.x, o.y, r.x, r.y].into_iter().all(f64::is_finite) {
        return Err(invalid("non-finite conic"));
    }
    if p.cx == q.cx && p.cy == q.cy && o == r {
        // The same numbers: the same point at every parameter.
        return Ok(Some(0.0));
    }
    let [x, y] = conic_rotate(
        first,
        coefficient(p.cx, q.cx) * p.gx,
        coefficient(p.cy, q.cy) * p.gy,
    );
    norm([
        x + coefficient(o.x, r.x),
        y + coefficient(o.y, r.y),
        Ivl::from(0.0),
    ])
    .map(Some)
}

/// The stored rational B-spline a curve is evaluated through: the B-spline itself,
/// and the conics derived from `SmBSplineCurve` (ellipse or circle, hyperbola,
/// parabola), whose evaluation is their base's (`Curve::evaluate_point`). Their
/// analytic placement and radii are not read: the base is what the edge is.
fn rational_base(curve: &Curve) -> Result<Option<&Nurbs>> {
    let base = match curve.kind() {
        CurveKind::BSpline(b) => b,
        CurveKind::Ellipse(e) => e.base(),
        CurveKind::Hyperbola(h) => h.base(),
        CurveKind::Parabola(p) => p.base(),
        _ => return Ok(None),
    };
    base.gw_nurb()
        .map(Some)
        .ok_or_else(|| invalid("missing spline data"))
}

fn curve_box(curve: &Curve, range: Ivl) -> Result<Box3> {
    curve_box_at(curve, range, 0)
}

/// [`curve_box`] for a curve nested `depth` composites deep.
fn curve_box_at(curve: &Curve, range: Ivl, depth: usize) -> Result<Box3> {
    match curve.kind() {
        CurveKind::Line(l) => return line_box(l, range),
        CurveKind::Conic(c) => return conic_box(c, range),
        CurveKind::Hermite(h) => return hermite_box(h, range),
        CurveKind::Composite(c) => return composite_box(c, range, depth),
        _ => {}
    }
    match rational_base(curve)? {
        Some(n) => spline_box(n, range),
        None => Err(unsupported()),
    }
}

/// Outward-rounded boxes that together hold every point of `curve` over the parameters
/// between `a` and `b`: lines, positive-weight B-splines (and the conics evaluated through
/// them), planar conics, Hermite curves and composites; an error for the other kinds, as [`curve_to_point`]'s.
///
/// A B-spline is covered on each knot span that meets the range, in `parts` equal pieces of
/// the span's part of it: each piece's interval evaluation, cut down to the box of the span's
/// control points (with positive weights the span lies in their convex hull; an interval
/// evaluation of a rational span can be far wider than the curve). A line, planar conic,
/// Hermite curve or composite is covered by its enclosure over `parts` equal pieces of the
/// range.
pub(crate) fn curve_cover(curve: &Curve, a: f64, b: f64, parts: u32) -> Result<Vec<Box3>> {
    let range = checked_range([a, b])?;
    let parts = parts.max(1);
    let cut = |t: Ivl, i: u32| {
        let at = |j: u32| {
            if j == parts {
                t.hi
            } else {
                t.lo + (t.hi - t.lo) * f64::from(j) / f64::from(parts)
            }
        };
        Ivl::new(at(i), at(i + 1))
    };
    let base = match curve.kind() {
        CurveKind::Line(_)
        | CurveKind::Conic(_)
        | CurveKind::Hermite(_)
        | CurveKind::Composite(_) => None,
        _ => Some(rational_base(curve)?.ok_or_else(unsupported)?),
    };
    let mut out = Vec::new();
    let Some(n) = base else {
        for i in 0..parts {
            out.push(curve_box(curve, cut(range, i))?);
        }
        return Ok(out);
    };
    validate(n, range)?;
    let p = n.degree();
    for (span, pair) in n.knots().windows(2).enumerate() {
        let [ka, kb] = pair else {
            continue;
        };
        if span < p || span >= n.cpts().len() || ka >= kb || *kb < range.lo || *ka > range.hi {
            continue;
        }
        let first = span
            .checked_sub(p)
            .ok_or_else(|| invalid("invalid spline span"))?;
        let mut hull: Option<Box3> = None;
        for j in 0..=p {
            let h = homogeneous(at(n.cpts(), first + j)?)?;
            let w = at(&h, 3)?;
            let point = [at(&h, 0)? / w, at(&h, 1)? / w, at(&h, 2)? / w];
            hull = Some(match hull {
                None => point,
                Some(old) => core::array::from_fn(|i| {
                    let (o, q) = (old.get(i).copied(), point.get(i).copied());
                    o.unwrap_or(Ivl::whole()).hull(q.unwrap_or(Ivl::whole()))
                }),
            });
        }
        let hull = hull.ok_or_else(|| invalid("invalid spline span"))?;
        let t = Ivl::new(range.lo.max(*ka), range.hi.min(*kb));
        for i in 0..parts {
            let eval = piece(n, span, cut(t, i))?;
            let both: Box3 = core::array::from_fn(|axis| {
                let h = hull.get(axis).copied().unwrap_or(Ivl::whole());
                let e = eval.get(axis).copied().unwrap_or(Ivl::whole());
                Ivl::new(h.lo.max(e.lo), h.hi.min(e.hi))
            });
            // both hold the curve, so they meet; if rounding says otherwise, the hull alone
            out.push(if both.iter().all(|x| x.lo <= x.hi) {
                both
            } else {
                hull
            });
        }
    }
    if out.is_empty() {
        return Err(invalid("no knot span covers the curve interval"));
    }
    Ok(out)
}

/// An upper bound for point-to-curve distance using an exact point of the curve.
pub(crate) fn point_at_curve(point: Point3, curve: &Curve, parameter: f64) -> Result<f64> {
    norm(difference(
        point_box(point)?,
        curve_box(curve, checked_range([parameter, parameter])?)?,
    ))
}

/// Bounds the whole curve's distance from a point (the geometry a squeeze removes).
pub(crate) fn curve_to_point(curve: &Curve, interval: &Extent1d, point: Point3) -> Result<f64> {
    norm(difference(
        curve_box(curve, checked_range([interval.min(), interval.max()])?)?,
        point_box(point)?,
    ))
}

/// Bounds both directed curve gaps under the stated endpoint correspondence.
pub(crate) fn curves(first: &Curve, a: [f64; 2], second: &Curve, b: [f64; 2]) -> Result<f64> {
    let ar = checked_range(a)?;
    let br = checked_range(b)?;
    // With a common positive rational basis the difference is a convex combination
    // of the dehomogenized control-point differences. This also proves identity
    // exactly, without subtracting independently widened position boxes.
    if a == b {
        if let (CurveKind::Conic(x), CurveKind::Conic(y)) = (first.kind(), second.kind()) {
            if let Some(bound) = conic_difference(x, y, ar)? {
                return Ok(bound);
            }
        }
        if let (CurveKind::Hermite(x), CurveKind::Hermite(y)) = (first.kind(), second.kind()) {
            return hermite_difference(x, y, ar);
        }
        let x = rational_base(first)?;
        let y = if x.is_some() {
            rational_base(second)?
        } else {
            None
        };
        if let (Some(x), Some(y)) = (x, y) {
            validate(x, ar)?;
            validate(y, br)?;
            if x.degree() == y.degree()
                && x.knots() == y.knots()
                && x.cpts().iter().zip(y.cpts()).all(|(p, q)| p.w == q.w)
            {
                let mut upper = 0.0_f64;
                for (&p, &q) in x.cpts().iter().zip(y.cpts()) {
                    let p = homogeneous(p)?;
                    let q = homogeneous(q)?;
                    let delta = difference(
                        [at(&p, 0)?, at(&p, 1)?, at(&p, 2)?],
                        [at(&q, 0)?, at(&q, 1)?, at(&q, 2)?],
                    );
                    let w = at(&p, 3)?;
                    upper = upper.max(norm(delta.map(|d| {
                        if d == Ivl::from(0.0) {
                            d
                        } else {
                            d / w
                        }
                    }))?);
                }
                return Ok(upper);
            }
        }
    }
    // A Hermite curve against a Hermite curve or a line: under the affine
    // correspondence their difference is one cubic in the shared parameter, enclosed
    // by the hull of its Bernstein coefficients.
    if matches!(first.kind(), CurveKind::Hermite(_))
        || matches!(second.kind(), CurveKind::Hermite(_))
    {
        if let (Some(p), Some(q)) = (polynomial(first, a)?, polynomial(second, b)?) {
            let delta: Cubic =
                core::array::from_fn(|k| difference(coefficient(&p, k), coefficient(&q, k)));
            return norm(bernstein_hull(&delta)?);
        }
    }
    // Affine curves attain the maximum norm of their difference at an endpoint.
    // Otherwise enclose every parameter cell, never only its sample stations.
    let cells = if matches!(
        (first.kind(), second.kind()),
        (CurveKind::Line(_), CurveKind::Line(_))
    ) {
        1_u32
    } else {
        64_u32
    };
    let mut upper = 0.0_f64;
    for i in 0..cells {
        let s = Ivl::new(
            f64::from(i) / f64::from(cells),
            f64::from(i + 1) / f64::from(cells),
        );
        let mapped =
            |range: [f64; 2]| Ivl::from(range[0]) * (Ivl::from(1.0) - s) + Ivl::from(range[1]) * s;
        if cells == 1 {
            for j in [0, 1] {
                let t = at(&a, j)?;
                let u = at(&b, j)?;
                upper = upper.max(norm(difference(
                    curve_box(first, Ivl::from(t))?,
                    curve_box(second, Ivl::from(u))?,
                ))?);
            }
        } else {
            // Clip only the rounding spill of the affine parameter map.
            let x = mapped(a);
            let y = mapped(b);
            upper = upper.max(norm(difference(
                curve_box(first, Ivl::new(x.lo.max(ar.lo), x.hi.min(ar.hi)))?,
                curve_box(second, Ivl::new(y.lo.max(br.lo), y.hi.min(br.hi)))?,
            ))?);
        }
    }
    Ok(upper)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::curve::crv_in_volume::CrvInVolume;
    use crate::geom::transform::Transform;
    use crate::nurbs::b_spline_curve::BSplineCurve;
    use crate::surface::volume::{Volume, VolumeKind};
    use crate::{Kernel, Vec3};

    /// Retain the raw data: an ordinary clone/owned constructor tunes the knots.
    fn spline(points: Vec<CPoint>, degree: usize, knots: Vec<f64>) -> Curve {
        BSplineCurve::from_nurb(Nurbs::new(points, degree, knots).unwrap(), 3, true).into()
    }

    fn cp(x: f64, y: f64, w: f64) -> CPoint {
        CPoint { x, y, z: 0.0, w }
    }

    #[test]
    fn whole_curve_bound_catches_the_interior_bump_five_samples_miss() {
        // A degree-one hat on [1/16, 3/16], peaking at 1/8. Every old sample
        // at 0, 1/4, 1/2, 3/4, 1 is exactly on the flat curve.
        let knots = vec![0.0, 0.0, 0.0625, 0.125, 0.1875, 1.0, 1.0];
        let flat = spline(
            vec![
                cp(0.0, 0.0, NOW),
                cp(0.0625, 0.0, NOW),
                cp(0.125, 0.0, NOW),
                cp(0.1875, 0.0, NOW),
                cp(1.0, 0.0, NOW),
            ],
            1,
            knots.clone(),
        );
        let bump = spline(
            vec![
                cp(0.0, 0.0, NOW),
                cp(0.0625, 0.0, NOW),
                cp(0.125, 1.0, NOW),
                cp(0.1875, 0.0, NOW),
                cp(1.0, 0.0, NOW),
            ],
            1,
            knots,
        );
        let k = Kernel::default();
        for t in [0.0, 0.25, 0.5, 0.75, 1.0] {
            assert_eq!(
                flat.evaluate_point(&k, t).unwrap(),
                bump.evaluate_point(&k, t).unwrap()
            );
        }
        assert_eq!(bump.evaluate_point(&k, 0.125).unwrap().y, 1.0);
        let bound = curves(&flat, [0.0, 1.0], &bump, [0.0, 1.0]).unwrap();
        assert!((1.0..1.0 + 1e-12).contains(&bound), "{bound}");
        let squeezed =
            curve_to_point(&bump, &Extent1d::from_min_max(0.0, 1.0), Vec3::ZERO).unwrap();
        assert!(squeezed >= 1.0);
    }

    #[test]
    fn positive_rational_weights_are_enclosed_without_a_polynomial_substitute() {
        // Y(t) = 4t(1-t)/(1+2t(1-t)), maximum 2/3 at 1/2.
        let rational = spline(
            vec![cp(0.0, 0.0, 1.0), cp(1.0, 2.0, 2.0), cp(1.0, 0.0, 1.0)],
            2,
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        );
        let line: Curve = Line::from_points(Vec3::ZERO, Vec3::new(1.0, 0.0, 0.0), 3).into();
        let bound = curves(&rational, [0.0, 1.0], &line, [0.0, 1.0]).unwrap();
        assert!((2.0 / 3.0..1.0).contains(&bound), "{bound}");
        let at_mid = point_at_curve(Vec3::new(0.5, 0.0, 0.0), &rational, 0.5).unwrap();
        assert!((2.0 / 3.0..2.0 / 3.0 + 1e-12).contains(&at_mid), "{at_mid}");
        assert_eq!(
            curves(&rational, [0.0, 1.0], &rational, [0.0, 1.0]).unwrap(),
            0.0
        );
    }

    #[test]
    fn reversed_and_subdomain_correspondences_cover_every_parameter() {
        let forward = spline(
            vec![cp(0.0, 0.0, NOW), cp(1.0, 0.0, NOW)],
            1,
            vec![0.0, 0.0, 1.0, 1.0],
        );
        let reverse = spline(
            vec![cp(1.0, 0.0, NOW), cp(0.0, 0.0, NOW)],
            1,
            vec![0.0, 0.0, 1.0, 1.0],
        );
        let bound = curves(&forward, [0.25, 0.75], &reverse, [0.75, 0.25]).unwrap();
        assert!(bound.is_finite() && bound < 0.1, "{bound}");
        let endpoint_bound = point_at_curve(Vec3::ZERO, &forward, 0.75).unwrap();
        assert!((0.75..0.75 + 1e-12).contains(&endpoint_bound));
        assert!(curves(&forward, [-0.1, 0.75], &reverse, [0.75, 0.25]).is_err());
    }

    #[test]
    fn point_and_growth_bounds_do_not_underflow_or_accept_nonfinite_results() {
        assert_eq!(points(Vec3::ZERO, Vec3::ZERO).unwrap(), 0.0);
        for tiny in [1e-200, f64::from_bits(1)] {
            let bound = points(Vec3::ZERO, Vec3::new(tiny, 0.0, 0.0)).unwrap();
            assert!(bound.is_finite() && bound >= tiny && bound > 0.0, "{bound}");
        }
        assert!(points(Vec3::ZERO, Vec3::new(1e300, 0.0, 0.0)).unwrap() >= 1e300);
        assert!(points(
            Vec3::new(-f64::MAX, 0.0, 0.0),
            Vec3::new(f64::MAX, 0.0, 0.0)
        )
        .is_err());
        assert!(points(Vec3::ZERO, Vec3::new(f64::NAN, 0.0, 0.0)).is_err());
        assert_eq!(growth(1.0, 1.0).unwrap(), 0.0);
        assert_eq!(growth(2.0, 1.0).unwrap(), 0.0);
        assert!(growth(1.0, 2.0).unwrap() >= 1.0);
        assert!(growth(0.0, f64::from_bits(1)).unwrap() > 0.0);
        assert!(growth(-1.0, 0.0).is_err());
        assert!(growth(0.0, f64::INFINITY).is_err());
    }

    #[test]
    fn planar_conic_enclosures_hold_their_evaluations_on_every_cell() {
        use crate::curve::conic::{Conic, ConicType};
        let mut kinds = Vec::new();
        for kind in [
            ConicType::Ellipse,
            ConicType::Hyperbola,
            ConicType::Parabola,
            ConicType::Line,
        ] {
            let mut c = Conic::new(kind, 2.0, 0.5);
            c.transform_2d(0.7, crate::math::Point2::new(3.0, -1.0));
            kinds.push(c);
        }
        for c in kinds {
            let natural = c.natural_interval();
            // The ellipse's branch point t = 1 falls inside a cell.
            let (lo, hi) = match c.conic_type() {
                ConicType::Ellipse => (-1.0, 3.0),
                ConicType::Hyperbola => (-0.9, 0.9),
                _ => (-5.0, 5.0),
            };
            assert!(lo >= natural.min() && hi <= natural.max());
            let cells = 37;
            for i in 0..cells {
                let a = lo + (hi - lo) * f64::from(i) / f64::from(cells);
                let b = lo + (hi - lo) * f64::from(i + 1) / f64::from(cells);
                let enclosure = conic_box(&c, Ivl::new(a, b)).unwrap();
                for j in 0..=20 {
                    let t = a + (b - a) * f64::from(j) / 20.0;
                    let p = c.evaluate_point(t);
                    for (value, ivl) in [p.x, p.y, p.z].into_iter().zip(enclosure) {
                        assert!(ivl.lo <= value && value <= ivl.hi, "{c:?} at {t}");
                    }
                }
            }
        }
        // Past the hyperbola's asymptote, or outside the natural interval: refused.
        let h = Conic::new(ConicType::Hyperbola, 1.0, 1.0);
        assert!(conic_box(&h, Ivl::new(0.0, 1.0)).is_err());
        let unknown = Conic::new(ConicType::Unknown, 1.0, 1.0);
        assert!(matches!(
            conic_box(&unknown, Ivl::new(0.0, 0.5)),
            Err(SolidError::Unsupported { .. })
        ));
    }

    #[test]
    fn two_conics_of_one_kind_are_bounded_by_their_coefficient_difference() {
        use crate::curve::conic::{Conic, ConicType};
        // 2^-23 is exact, and so are 2 + 2^-23 and the origin it moves to.
        let d = 1.0 / 8_388_608.0;
        let first = Conic::new(ConicType::Ellipse, 2.0, 1.0);
        let mut second = Conic::new(ConicType::Ellipse, 2.0 + d, 1.0);
        second.transform_2d(0.0, crate::math::Point2::new(0.0, d));
        let a: Curve = first.into();
        let b: Curve = second.into();
        let bound = curves(&a, [-1.0, 3.0], &b, [-1.0, 3.0]).unwrap();
        // Closed form: the difference is (d g_x(t), d) with |g_x| <= 1, so its
        // largest norm is d sqrt(2), at t = 0 (g_x = 1).
        let exact = d * core::f64::consts::SQRT_2;
        assert!(
            bound >= exact * (1.0 - 1e-15) && bound <= exact * (1.0 + 1e-12),
            "{bound:e} against {exact:e}"
        );
        assert_eq!(curves(&a, [-1.0, 3.0], &a, [-1.0, 3.0]).unwrap(), 0.0);
    }

    #[test]
    fn malformed_weights_knots_storage_and_unsupported_kinds_fail_explicitly() {
        for weight in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            let c = spline(
                vec![cp(0.0, 0.0, weight), cp(1.0, 0.0, 1.0)],
                1,
                vec![0.0, 0.0, 1.0, 1.0],
            );
            assert!(point_at_curve(Vec3::ZERO, &c, 0.5).is_err());
        }
        for (degree, knots) in [
            (1, vec![0.0, 1.0]),
            (1, vec![0.0; 4]),
            (1, vec![0.0, 1.0, 0.0, 1.0]),
            (usize::MAX, vec![0.0, 0.0, 1.0, 1.0]),
        ] {
            let c = spline(vec![cp(0.0, 0.0, NOW), cp(1.0, 0.0, NOW)], degree, knots);
            assert!(point_at_curve(Vec3::ZERO, &c, 0.5).is_err());
        }
        let mixed = spline(
            vec![cp(0.0, 0.0, NOW), cp(1.0, 0.0, 1.0)],
            1,
            vec![0.0, 0.0, 1.0, 1.0],
        );
        assert!(point_at_curve(Vec3::ZERO, &mixed, 0.5).is_err());
        // A curve in a volume has no enclosure here (#13724 parts (b) and (c) leave it out).
        let in_volume: Curve = CrvInVolume::new(
            Line::from_points(Vec3::ZERO, Vec3::new(1.0, 1.0, 1.0), 3).into(),
            true,
            Volume::new(VolumeKind::Transform(Transform::IDENTITY)),
            3,
            0,
        )
        .unwrap()
        .into();
        assert!(matches!(
            point_at_curve(Vec3::ZERO, &in_volume, 0.5),
            Err(SolidError::Unsupported { .. })
        ));
    }

    fn hermite(p1: [f64; 3], d1: [f64; 3], p2: [f64; 3], d2: [f64; 3]) -> HermiteCurve {
        let v = |a: [f64; 3]| Vec3::new(a[0], a[1], a[2]);
        HermiteCurve::new(v(p1), v(d1), v(p2), v(d2), 3)
    }

    fn contains(b: Box3, p: Point3) -> bool {
        [p.x, p.y, p.z]
            .into_iter()
            .zip(b)
            .all(|(x, i)| i.lo <= x && x <= i.hi)
    }

    /// x = t, y = t - t^2: the cubic Hermite with these end data (h10 - h11 for y,
    /// h01 + h10 + h11 = t for x). Its maximum y is 1/4 at t = 1/2, and the Bernstein
    /// coefficients of y over [0, 1] are 0, 1/3, 1/3, 0.
    fn parabola_hermite() -> HermiteCurve {
        hermite([0.0; 3], [1.0, 1.0, 0.0], [1.0, 0.0, 0.0], [1.0, -1.0, 0.0])
    }

    #[test]
    fn hermite_enclosure_holds_dense_samples_and_the_interior_extreme() {
        let k = Kernel::default();
        let cases = [
            parabola_hermite(),
            // Tangents ten times the chord: the curve overshoots both ends.
            hermite(
                [0.0; 3],
                [10.0, 10.0, -3.0],
                [1.0, 0.0, 0.0],
                [10.0, -10.0, 7.0],
            ),
            // Closed: p1 == p2.
            hermite(
                [1.0, 2.0, 3.0],
                [4.0, 0.0, 1.0],
                [1.0, 2.0, 3.0],
                [-4.0, 1.0, 0.0],
            ),
            // Far from the origin, small and non-dyadic.
            hermite(
                [1.0e6 + 0.1, -2.0e6, 0.3],
                [1.0e-3, 0.7, 0.0],
                [1.0e6 + 0.2, -2.0e6 + 1.0, 0.3],
                [-0.3, 1.1, 1.0e-9],
            ),
            // Degenerate: a point.
            hermite([0.5; 3], [0.0; 3], [0.5; 3], [0.0; 3]),
        ];
        for h in cases {
            let c: Curve = h.into();
            for (lo, hi) in [
                (0.0, 1.0),
                (0.0, 0.5),
                (0.3, 0.7),
                (0.1, 0.1 + 1.0e-7),
                (0.4, 0.4),
            ] {
                let b = hermite_box(&h, Ivl::new(lo, hi)).unwrap();
                for j in 0..=400 {
                    let t = if j == 400 {
                        hi
                    } else {
                        lo + (hi - lo) * f64::from(j) / 400.0
                    };
                    let p = c.evaluate_point(&k, t).unwrap();
                    assert!(
                        contains(b, p),
                        "{h:?} on [{lo}, {hi}] at {t}: {p:?} not in {b:?}"
                    );
                }
            }
        }
        // The interior maximum, missed by both ends, is inside and the hull is tight.
        let b = hermite_box(&parabola_hermite(), Ivl::new(0.0, 1.0)).unwrap();
        let [x, y, z] = b;
        assert!(y.hi >= 0.25 && y.hi <= 1.0 / 3.0 + 1.0e-12, "{y:?}");
        assert!(y.lo <= 0.0 && y.lo > -1.0e-12, "{y:?}");
        assert!(x.lo <= 0.0 && x.hi >= 1.0 && x.hi < 1.0 + 1.0e-12, "{x:?}");
        assert!(z.lo <= 0.0 && z.hi >= 0.0 && z.hi < 1.0e-12, "{z:?}");
        // Past the natural interval: refused, not extrapolated.
        assert!(hermite_box(&parabola_hermite(), Ivl::new(0.5, 1.5)).is_err());
        assert!(hermite_box(&parabola_hermite(), Ivl::new(-0.1, 0.5)).is_err());
    }

    #[test]
    fn hermite_pair_bound_holds_the_interior_gap_and_proves_identity() {
        let curve = |h: HermiteCurve| -> Curve { h.into() };
        let same = curves(
            &curve(parabola_hermite()),
            [0.0, 1.0],
            &curve(parabola_hermite()),
            [0.0, 1.0],
        )
        .unwrap();
        assert_eq!(same, 0.0);
        // Against the straight chord y = 0 the gap is t - t^2, 1/4 at t = 1/2; the
        // ends agree exactly.
        let chord = hermite([0.0; 3], [1.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 0.0, 0.0]);
        let bound = curves(
            &curve(parabola_hermite()),
            [0.0, 1.0],
            &curve(chord),
            [0.0, 1.0],
        )
        .unwrap();
        assert!((0.25..=1.0 / 3.0 + 1.0e-12).contains(&bound), "{bound}");
        // A rigid lift by 1/2 along z: every coefficient difference is exact.
        let lifted = hermite(
            [0.0, 0.0, 0.5],
            [1.0, 1.0, 0.0],
            [1.0, 0.0, 0.5],
            [1.0, -1.0, 0.0],
        );
        let bound = curves(
            &curve(parabola_hermite()),
            [0.0, 1.0],
            &curve(lifted),
            [0.0, 1.0],
        )
        .unwrap();
        assert!((0.5..0.5 + 1.0e-12).contains(&bound), "{bound}");
        // Against the chord as a line, forward and with the line reversed under the
        // reversed correspondence (the same point pairs): the gap is again 1/4, and
        // a cubic for a line that is a Hermite (exact zero coefficients) gives zero.
        let line = |a: [f64; 3], b: [f64; 3]| -> Curve {
            Line::from_points(Vec3::new(a[0], a[1], a[2]), Vec3::new(b[0], b[1], b[2]), 3).into()
        };
        let parabola = curve(parabola_hermite());
        let forward = line([0.0; 3], [1.0, 0.0, 0.0]);
        let backward = line([1.0, 0.0, 0.0], [0.0; 3]);
        for (l, range) in [(&forward, [0.0, 1.0]), (&backward, [1.0, 0.0])] {
            let bound = curves(&parabola, [0.0, 1.0], l, range).unwrap();
            assert!((0.25..=1.0 / 3.0 + 1.0e-12).contains(&bound), "{bound}");
            let bound = curves(l, range, &parabola, [0.0, 1.0]).unwrap();
            assert!((0.25..=1.0 / 3.0 + 1.0e-12).contains(&bound), "{bound}");
            let bound = curves(&curve(chord), [0.0, 1.0], l, range).unwrap();
            // Closed form 0; the bound is only rounding. Each Bernstein coefficient
            // takes under 20 outward-rounded operations on magnitudes at most 3, each
            // widening an end by at most 2.3e-16 * 3 + 1e-300, so under 1.4e-14 per
            // coordinate and 2.4e-14 in norm.
            assert!(bound < 2.4e-14, "{bound}");
        }
        // The wrong correspondence pairs t with 1 - t on the chord: gap up to 1.
        let crossed = curves(&curve(chord), [0.0, 1.0], &forward, [1.0, 0.0]).unwrap();
        assert!(crossed >= 1.0, "{crossed}");
        // Point to curve over the whole curve: the far end is sqrt(2) from (0, 1, 0)
        // ... and the start 1 away.
        let far = curve_to_point(
            &curve(parabola_hermite()),
            &Extent1d::from_min_max(0.0, 1.0),
            Vec3::new(0.0, 1.0, 0.0),
        )
        .unwrap();
        assert!(far >= 2.0_f64.sqrt(), "{far}");
    }

    /// A line, a reversed Hermite and a nested composite of a B-spline, end to end.
    fn mixed_composite() -> (CompositeCurve, Vec<(Curve, bool)>) {
        let line: Curve = Line::from_points(Vec3::ZERO, Vec3::new(1.0, 0.0, 0.0), 3).into();
        // x = 2 - u, y = 3(u - u^2): reversed, it runs from (1, 0, 0) to (2, 0, 0).
        let h: Curve = hermite(
            [2.0, 0.0, 0.0],
            [-1.0, 3.0, 0.0],
            [1.0, 0.0, 0.0],
            [-1.0, -3.0, 0.0],
        )
        .into();
        let s = spline(
            vec![cp(2.0, 0.0, NOW), cp(2.5, -1.0, NOW), cp(3.0, 0.0, NOW)],
            2,
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        );
        let inner: Curve = CompositeCurve::with_curves(3, vec![s.clone()], false, None, None)
            .unwrap()
            .into();
        let c = CompositeCurve::with_curves(
            3,
            vec![line.clone(), h.clone(), inner],
            false,
            Some(&[true, false, true]),
            None,
        )
        .unwrap();
        (c, vec![(line, true), (h, false), (s, true)])
    }

    #[test]
    fn composite_enclosure_is_the_union_of_its_mapped_segments() {
        let k = Kernel::default();
        let (c, pieces) = mixed_composite();
        let natural = c.natural_interval();
        assert_eq!((natural.min(), natural.max()), (0.0, 3.0));
        // The composite's point at t: segment i = floor(t) at its own parameter,
        // reversed for the Hermite.
        let point = |t: f64| {
            let i = (t.floor() as usize).min(2);
            let (curve, same) = &pieces[i];
            let local = t - i as f64;
            let u = if *same { local } else { 1.0 - local };
            curve.evaluate_point(&k, u).unwrap()
        };
        let whole = composite_box(&c, Ivl::new(0.0, 3.0), 0).unwrap();
        let cells = 30;
        for i in 0..cells {
            let a = 3.0 * f64::from(i) / f64::from(cells);
            let b = 3.0 * f64::from(i + 1) / f64::from(cells);
            let cell = composite_box(&c, Ivl::new(a, b), 0).unwrap();
            for j in 0..=50 {
                let t = a + (b - a) * f64::from(j) / 50.0;
                let p = point(t);
                assert!(contains(cell, p), "t = {t}: {p:?} not in {cell:?}");
                assert!(contains(whole, p), "t = {t}");
            }
        }
        // Just after t = 1 the reversed Hermite is near its own end (1, 0, 0); read
        // forward it would be near (2, 0, 0).
        let near = composite_box(&c, Ivl::new(1.0, 1.05), 0).unwrap();
        assert!(near[0].lo <= 1.0 && near[0].hi < 1.1, "{near:?}");
        let late = composite_box(&c, Ivl::new(1.95, 2.0), 0).unwrap();
        assert!(late[0].lo > 1.9 && late[0].hi >= 2.0, "{late:?}");
        // Through the public bounds: the composite edge's far point.
        let far = curve_to_point(&c.clone().into(), &natural, Vec3::ZERO).unwrap();
        assert!(far >= 3.0, "{far}");
        // Outside the natural interval: refused.
        assert!(composite_box(&c, Ivl::new(2.5, 3.5), 0).is_err());
    }

    #[test]
    fn composite_recursion_is_refused_past_the_nesting_limit() {
        let (c, _) = mixed_composite();
        assert!(composite_box(&c, Ivl::new(0.0, 3.0), MAX_GEOMETRY_NESTING - 2).is_ok());
        // The nested composite is one level further down.
        assert!(matches!(
            composite_box(&c, Ivl::new(0.0, 3.0), MAX_GEOMETRY_NESTING - 1),
            Err(SolidError::InvalidInput { .. })
        ));
        assert!(composite_box(&c, Ivl::new(0.0, 0.5), MAX_GEOMETRY_NESTING - 1).is_ok());
        assert!(matches!(
            composite_box(&c, Ivl::new(0.0, 0.5), MAX_GEOMETRY_NESTING),
            Err(SolidError::InvalidInput { .. })
        ));
        let empty = CompositeCurve::with_curves(3, Vec::new(), false, None, None).unwrap();
        assert!(composite_box(&empty, Ivl::new(0.0, 0.0), 0).is_err());
    }
}
