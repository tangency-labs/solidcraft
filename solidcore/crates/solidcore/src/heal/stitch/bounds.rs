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
//! coefficients. Hermite, iso, offset, projected, on-surface, in-volume,
//! tangent-field and composite curves have no enclosure here and are refused.
//!
//! Deviations: no recursion. Work is bounded by the stored knot count and the
//! kernel's NURBS degree limit. Curves without an implemented exact enclosure,
//! non-positive weights, extrapolation and non-finite bounds are refused.

use crate::blend::fillet_section::interval::Ivl;
use crate::curve::conic::{Conic, ConicType};
use crate::curve::curve::{Curve, CurveKind};
use crate::curve::line::Line;
use crate::geom::extent1d::Extent1d;
use crate::math::Point3;
use crate::nurbs::crv_basic::Curve as Nurbs;
use crate::nurbs::knots::MAX_DEGREE;
use crate::nurbs::math::{CPoint, NOW, NOZ};
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
                 and planar conics only",
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
pub(super) fn points(a: Point3, b: Point3) -> Result<f64> {
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
    if let CurveKind::Line(l) = curve.kind() {
        return line_box(l, range);
    }
    if let CurveKind::Conic(c) = curve.kind() {
        return conic_box(c, range);
    }
    match rational_base(curve)? {
        Some(n) => spline_box(n, range),
        None => Err(unsupported()),
    }
}

/// An upper bound for point-to-curve distance using an exact point of the curve.
pub(super) fn point_at_curve(point: Point3, curve: &Curve, parameter: f64) -> Result<f64> {
    norm(difference(
        point_box(point)?,
        curve_box(curve, checked_range([parameter, parameter])?)?,
    ))
}

/// Bounds the whole curve's distance from a point (the geometry a squeeze removes).
pub(super) fn curve_to_point(curve: &Curve, interval: &Extent1d, point: Point3) -> Result<f64> {
    norm(difference(
        curve_box(curve, checked_range([interval.min(), interval.max()])?)?,
        point_box(point)?,
    ))
}

/// Bounds both directed curve gaps under the stated endpoint correspondence.
pub(super) fn curves(first: &Curve, a: [f64; 2], second: &Curve, b: [f64; 2]) -> Result<f64> {
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
    use crate::curve::hermite_curve::HermiteCurve;
    use crate::nurbs::b_spline_curve::BSplineCurve;
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
        let hermite: Curve = HermiteCurve::new(
            Vec3::ZERO,
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            3,
        )
        .into();
        assert!(matches!(
            point_at_curve(Vec3::ZERO, &hermite, 0.5),
            Err(SolidError::Unsupported { .. })
        ));
    }
}
