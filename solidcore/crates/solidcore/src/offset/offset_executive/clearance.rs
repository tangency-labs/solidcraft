//! Not upstream: proving that a local operation may keep an edge clear of a moved surface.
//!
//! | Rust | Upstream |
//! |------|----------|
//! | [`separated`] | none |
//!
//! A curve interval is enclosed by rounded interval evaluation, then by a ball B(c,r).
//! Distance to a set is 1-Lipschitz: d(C,S) >= d(c,S) - r. The surface distance is the
//! existing certified bound (positive-weight Bernstein hulls, including the definition's
//! deviation and arithmetic rounding), not a projection or a sample. Thus every accepted
//! cell proves all its curve points clear, including between evaluated parameters.
//!
//! Deviations: no recursion; at most 127 curve cells and six halvings per branch. A kind
//! without an enclosure, a nonfinite enclosure, or an exhausted budget proves nothing.
//! NURBS degree is at most 18 here: the factorials in `curve_derivs_over` are then exact
//! binary64 integers. Lines are evaluated on their definition, not a rounded fitted form.

use crate::blend::fillet_section::certificate::{curve_derivs_over, NurbsCurve};
use crate::blend::fillet_section::interval::Ivl;
use crate::curve::curve::{Curve, CurveKind};
use crate::curve::line::Line;
use crate::geom::extent1d::Extent1d;
use crate::kernel::Kernel;
use crate::math::Point3;
use crate::nurbs::math::{NOW, NOZ};
use crate::query::distance_bound::surface_distance_lower_bound;
use crate::surface::surface::Surface;
use crate::{Result, SolidError};

const OP: &str = "offset::offset_executive::clearance";
const MAX_DEPTH: usize = 6;
const MAX_CELLS: usize = 127;

enum EnclosedCurve<'a> {
    Line(&'a Line),
    Nurbs(NurbsCurve),
}

impl<'a> EnclosedCurve<'a> {
    fn of(curve: &'a Curve) -> Option<Self> {
        let base = match curve.kind() {
            CurveKind::Line(line) => return Some(Self::Line(line)),
            CurveKind::BSpline(c) => c,
            // Curve::evaluate_point reads these conics' stored rational forms too.
            CurveKind::Ellipse(c) => c.base(),
            CurveKind::Parabola(c) => c.base(),
            CurveKind::Hyperbola(c) => c.base(),
            _ => return None,
        };
        let net = base.gw_nurb()?;
        let (p, n) = (net.degree(), net.cpts().len());
        if !(1..=18).contains(&p) || n <= p || net.knots().len() != n + p + 1 {
            return None;
        }
        // Read the stored homogeneous coefficients directly: control_polygon followed
        // by NurbsCurve::of would divide and multiply rational coordinates again.
        let mut pw = Vec::new();
        for q in net.cpts() {
            let w = if q.w == NOW { 1.0 } else { q.w };
            if !(q.x.is_finite() && q.y.is_finite() && q.z.is_finite() && w.is_finite())
                || q.z == NOZ
                || w <= 0.0
            {
                return None;
            }
            pw.extend_from_slice(&[q.x, q.y, q.z, w]);
        }
        let u = net.knots().to_vec();
        if u.iter().any(|x| !x.is_finite()) || u.iter().zip(u.iter().skip(1)).any(|(a, b)| a > b) {
            return None;
        }
        Some(Self::Nurbs(NurbsCurve { p, n, u, pw }))
    }

    fn over(&self, t: Ivl) -> Result<Option<[Ivl; 3]>> {
        match self {
            Self::Line(line) => {
                // The affine NURBS-to-STEP parameter map, with every subtraction,
                // division and multiplication enclosed (Line::evaluate_point).
                let mut step = t;
                if line.is_bounded() {
                    let nurb = line.natural_interval();
                    let domain = line.step_interval();
                    let length = Ivl::from(nurb.max()) - Ivl::from(nurb.min());
                    if length.lo.is_nan()
                        || length.lo <= 0.0
                        || crate::geom::tol::is_zero(nurb.length())
                    {
                        return Ok(None);
                    }
                    let along = if line.inside_out() {
                        (Ivl::from(nurb.max()) - t) / length
                    } else {
                        (t - Ivl::from(nurb.min())) / length
                    };
                    step = Ivl::from(domain.min())
                        + along * (Ivl::from(domain.max()) - Ivl::from(domain.min()));
                } else if line.inside_out() {
                    step = Ivl::from(0.0) - step;
                }
                let p = line.line_point();
                let v = line.line_vector();
                let s = step * Ivl::from(line.line_scale());
                Ok(Some([
                    Ivl::from(p.x) + s * Ivl::from(v.x),
                    Ivl::from(p.y) + s * Ivl::from(v.y),
                    Ivl::from(p.z) + s * Ivl::from(v.z),
                ]))
            }
            Self::Nurbs(net) => {
                let (Some(&lo), Some(&hi)) = (net.u.get(net.p), net.u.get(net.n)) else {
                    return Ok(None);
                };
                if !(t.lo >= lo && t.hi <= hi) {
                    return Ok(None);
                }
                let derivatives = curve_derivs_over(net, t, 0, None)?;
                Ok(derivatives.first().map(|p| [p.x, p.y, p.z]))
            }
        }
    }
}

/// Every point of `curve` over `interval` is proved farther than `tolerance` from the
/// whole finite image of `surface` (the caller supplies the moved surface's extension).
/// `false` means unproved, not necessarily intersecting. No curve or surface samples can
/// make this return true.
///
/// # Errors
/// Cancellation is propagated. An unavailable certificate returns `false`, so the
/// executive takes the ordinary re-intersection path instead of keeping this edge.
pub(super) fn separated(
    kernel: &Kernel,
    curve: &Curve,
    interval: &Extent1d,
    surface: &Surface,
    tolerance: f64,
) -> Result<bool> {
    let Some(curve) = EnclosedCurve::of(curve) else {
        return Ok(false);
    };
    let (lo, hi) = (interval.min(), interval.max());
    if !(lo.is_finite() && hi.is_finite() && lo < hi && tolerance.is_finite() && tolerance >= 0.0) {
        return Ok(false);
    }
    let mut stack = vec![(Ivl::new(lo, hi), 0_usize)];
    let mut cells = 0;
    while let Some((t, depth)) = stack.pop() {
        kernel.check_cancelled(OP)?;
        cells += 1;
        if cells > MAX_CELLS {
            return Ok(false);
        }
        let box_ = match curve.over(t) {
            Ok(Some(box_)) => box_,
            Err(e @ SolidError::Cancelled { .. }) => return Err(e),
            _ => return Ok(false),
        };
        if box_
            .iter()
            .any(|x| !(x.lo.is_finite() && x.hi.is_finite() && x.lo <= x.hi))
        {
            return Ok(false);
        }
        // The centre need not lie on the curve. Its computed binary64 value is exact
        // data for both bounds: interval subtraction encloses the ball's radius.
        let centre = box_.map(|x| 0.5 * x.lo + 0.5 * x.hi);
        let mut radius2 = Ivl::from(0.0);
        for (x, c) in box_.into_iter().zip(centre) {
            let delta = x - Ivl::from(c);
            radius2 = radius2 + delta * delta;
        }
        let radius = radius2.sqrt().hi;
        let want = (Ivl::from(radius) + Ivl::from(tolerance)).hi;
        if !want.is_finite() {
            return Ok(false);
        }
        let [cx, cy, cz] = centre;
        let lower = match surface_distance_lower_bound(
            kernel,
            surface,
            None,
            Point3::new(cx, cy, cz),
            want,
        ) {
            Ok(lower) => lower,
            Err(e @ SolidError::Cancelled { .. }) => return Err(e),
            Err(_) => return Ok(false),
        };
        if (Ivl::from(lower) - Ivl::from(radius)).lo > tolerance {
            continue;
        }
        if depth >= MAX_DEPTH {
            return Ok(false);
        }
        let mid = 0.5 * t.lo + 0.5 * t.hi;
        if !(mid > t.lo && mid < t.hi) {
            return Ok(false);
        }
        stack.push((Ivl::new(mid, t.hi), depth + 1));
        stack.push((Ivl::new(t.lo, mid), depth + 1));
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::Vec3;
    use crate::nurbs::b_spline_curve::p1::{BSplineCurveForm, CanonicalData, KnotType};
    use crate::nurbs::b_spline_curve::BSplineCurve;
    use crate::surface::offset_surface::OffsetSurface;
    use crate::surface::plane::Plane;

    fn floor() -> Surface {
        Surface::from(Plane::from_normal(Vec3::ZERO, Vec3::Z))
    }

    #[test]
    fn a_long_clear_edge_is_proved_after_subdivision() {
        let k = Kernel::default();
        let interval = Extent1d::from_min_max(0.0, 1.0);
        for curve in [
            Curve::from(Line::from_points(
                Point3::new(-10.0, 0.0, 1.0),
                Point3::new(10.0, 0.0, 1.0),
                3,
            )),
            Curve::from(
                BSplineCurve::create_line_segment(
                    3,
                    Point3::new(-10.0, 0.0, 1.0),
                    Point3::new(10.0, 0.0, 1.0),
                )
                .unwrap(),
            ),
        ] {
            assert!(separated(&k, &curve, &interval, &floor(), 0.01).unwrap());
        }
    }

    #[test]
    fn a_crossing_contact_or_tolerance_band_is_never_certified_clear() {
        let k = Kernel::default();
        let interval = Extent1d::from_min_max(0.0, 1.0);
        for (a, b, tolerance) in [
            (-1.0, 1.0, 0.01),
            (0.0, 0.0, 0.01),
            (0.005, 0.005, 0.01),
            (0.01, 0.01, 0.01),
        ] {
            let curve = Curve::from(Line::from_points(
                Point3::new(-1.0, 0.0, a),
                Point3::new(1.0, 0.0, b),
                3,
            ));
            assert!(!separated(&k, &curve, &interval, &floor(), tolerance).unwrap());
        }
    }

    #[test]
    fn rational_curve_interiors_and_the_trim_interval_are_bounded() {
        let k = Kernel::default();
        let curve = Curve::from(
            BSplineCurve::create_canonical(&CanonicalData {
                dim: 3,
                degree: 2,
                control_points: &[
                    Point3::new(-1.0, 0.0, 1.0),
                    Point3::new(0.0, 0.0, -2.0),
                    Point3::new(1.0, 0.0, 1.0),
                ],
                form: BSplineCurveForm::Unspecified,
                knot_multiplicities: &[3, 3],
                knots: &[0.0, 1.0],
                knot_type: KnotType::Unspecified,
                weights: Some(&[1.0, 2.0, 1.0]),
                trim: None,
            })
            .unwrap(),
        );
        // The endpoints are above the plane but C(1/2).z = -1: it crosses.
        assert!(!separated(
            &k,
            &curve,
            &Extent1d::from_min_max(0.0, 1.0),
            &floor(),
            0.01,
        )
        .unwrap());
        assert!(separated(
            &k,
            &curve,
            &Extent1d::from_min_max(0.0, 0.03125),
            &floor(),
            0.01,
        )
        .unwrap());
    }

    #[test]
    fn unavailable_or_nonfinite_bounds_do_not_keep_an_edge() {
        let k = Kernel::default();
        let curve = Curve::from(Line::from_points(Vec3::ZERO, Vec3::X, 3));
        let interval = Extent1d::from_min_max(0.0, 1.0);
        let implicit = Surface::from(OffsetSurface::new(&k, 2.0, floor(), true, 0).unwrap());
        assert!(!separated(&k, &curve, &interval, &implicit, 0.01).unwrap());
        assert!(!separated(&k, &curve, &interval, &floor(), f64::NAN).unwrap());
        assert!(!separated(
            &k,
            &curve,
            &Extent1d::from_min_max(-1.0, f64::INFINITY),
            &floor(),
            0.01,
        )
        .unwrap());
    }
}
