//! Native affine definition enclosures shared by certified projected queries.
//!
//! Upstream: the stored Line parameter map; interval certificates are solidcore's.
//! Deviations: no query solver, tolerance, or cache continuity is used. Exact
//! product sums are error-free expansions ([`crate::geom::expansion`]): a product out of
//! their range is not shown zero. No input-driven recursion.

use crate::blend::fillet_section::interval::Ivl;
use crate::curve::curve::{Curve, CurveKind};
use crate::geom::expansion;
use crate::geom::extent1d::Extent1d;
use crate::geom::tol::is_zero;
use crate::math::Vec3;

/// Enclosed endpoints and the native parameter interval of an affine definition.
#[derive(Clone)]
pub(crate) struct LineDefinition {
    start: [Ivl; 3],
    end: [Ivl; 3],
    interval: Extent1d,
}
impl LineDefinition {
    /// Enclose a point and derivative in the native parameter map.
    pub(crate) fn jet_over(&self, t: Ivl) -> ([Ivl; 3], [Ivl; 3]) {
        if self.interval.min() == self.interval.max() {
            return (self.start, [Ivl::from(0.0); 3]);
        }
        let span = Ivl::from(self.interval.max()) - Ivl::from(self.interval.min());
        let fraction = (t - Ivl::from(self.interval.min())) / span;
        let derivative = iv_sub(self.end, self.start).map(|x| x / span);
        (iv_lerp(self.start, self.end, fraction), derivative)
    }
    fn point(&self, t: f64) -> [Ivl; 3] {
        self.jet_over(Ivl::from(t)).0
    }
}
/// Enclose an actual analytic Line using its stored factors and native map.
pub(crate) fn of_line(curve: &Curve, interval: Extent1d) -> Option<LineDefinition> {
    let CurveKind::Line(line) = curve.kind() else {
        return None;
    };
    let point = iv_point(line.line_point());
    let vector = iv_point(line.line_vector());
    let natural = line.natural_interval();
    let analytic = line.step_interval();
    let bounded_map = line.is_bounded() && !is_zero(natural.length());
    let endpoint = |t: f64| {
        let mut step = Ivl::from(t);
        if bounded_map {
            let fraction = if line.inside_out() {
                (step - Ivl::from(natural.max()))
                    / (Ivl::from(natural.min()) - Ivl::from(natural.max()))
            } else {
                (step - Ivl::from(natural.min()))
                    / (Ivl::from(natural.max()) - Ivl::from(natural.min()))
            };
            step = Ivl::from(analytic.min())
                + fraction * (Ivl::from(analytic.max()) - Ivl::from(analytic.min()));
        } else if !line.is_bounded() && line.inside_out() {
            step = Ivl::from(0.0) - step;
        }
        let [x, y, z] = point;
        let [dx, dy, dz] = vector;
        let scale = step * Ivl::from(line.line_scale());
        [x + scale * dx, y + scale * dy, z + scale * dz]
    };
    Some(LineDefinition {
        start: endpoint(interval.min()),
        end: endpoint(interval.max()),
        interval,
    })
}
/// Enclose an equal-weight native degree-one span without a rounded fit.
fn affine_definition(curve: &Curve, interval: Extent1d) -> Option<LineDefinition> {
    if matches!(curve.kind(), CurveKind::Line(_)) {
        return of_line(curve, interval);
    }
    let CurveKind::BSpline(spline) = curve.kind() else {
        return None;
    };
    let nurb = spline.gw_nurb()?;
    if nurb.degree() != 1 {
        return None;
    }
    // A right-sided lookup at the lower boundary avoids rounding a tiny span's
    // midpoint onto a knot. A leaf crossing a knot is not certified affine here.
    let index = nurb
        .knots()
        .partition_point(|t| *t <= interval.min())
        .saturating_sub(1);
    let (&lo, &hi) = nurb.knots().get(index).zip(nurb.knots().get(index + 1))?;
    if hi <= lo || interval.min() < lo || interval.max() > hi || index == 0 {
        return None;
    }
    let (a, b) = nurb.cpts().get(index - 1).zip(nurb.cpts().get(index))?;
    let nonrational = a.w == crate::nurbs::math::NOW && b.w == crate::nurbs::math::NOW;
    if !(nonrational || a.w == b.w && a.w > 0.0 && a.w.is_finite()) {
        return None;
    }
    let coords = |p: &crate::nurbs::math::CPoint| {
        let z = if p.z == crate::nurbs::math::NOZ {
            0.0
        } else {
            p.z
        };
        [p.x, p.y, z]
    };
    let ac = coords(a);
    let bc = coords(b);
    let w = Ivl::from(if nonrational { 1.0 } else { a.w });
    Some(LineDefinition {
        start: ac.map(|x| Ivl::from(x) / w),
        end: bc.map(|x| Ivl::from(x) / w),
        interval: Extent1d::from_min_max(lo, hi),
    })
}

/// Enclose the actual native Line or degree-one spline point projected metric.
pub(crate) fn affine_projected_point_enclosure(
    curve: &Curve,
    t: f64,
    point: Vec3,
    normal: Vec3,
) -> Option<Ivl> {
    let natural = curve.natural_interval();
    if !t.is_finite() || t < natural.min() || t > natural.max() {
        return None;
    }
    let position = match curve.kind() {
        CurveKind::Line(_) => affine_definition(curve, Extent1d::from_min_max(t, t))?.start,
        CurveKind::BSpline(spline) => {
            let nurb = spline.gw_nurb()?;
            if nurb.degree() != 1 {
                return None;
            }
            let control = if t == natural.min() {
                nurb.cpts().first()
            } else if t == natural.max() {
                nurb.cpts().last()
            } else {
                let index = nurb
                    .knots()
                    .partition_point(|knot| *knot <= t)
                    .checked_sub(1)?;
                if nurb.knots().get(index) == Some(&t) {
                    index
                        .checked_sub(1)
                        .and_then(|index| nurb.cpts().get(index))
                } else {
                    None
                }
            };
            if let Some(control) = control {
                let weight = if control.w == crate::nurbs::math::NOW {
                    1.0
                } else {
                    control.w
                };
                if !weight.is_finite() || weight <= 0.0 {
                    return None;
                }
                [
                    control.x,
                    control.y,
                    if control.z == crate::nurbs::math::NOZ {
                        0.0
                    } else {
                        control.z
                    },
                ]
                .map(|x| Ivl::from(x) / Ivl::from(weight))
            } else {
                affine_definition(curve, Extent1d::from_min_max(t, t))?.point(t)
            }
        }
        _ => return None,
    };
    let bound = iv_length(iv_cross(
        iv_cross(iv_point(normal), iv_sub(position, iv_point(point))),
        iv_point(normal),
    ));
    (bound.lo.is_finite() && bound.hi.is_finite()).then_some(bound)
}

fn iv_point(p: Vec3) -> [Ivl; 3] {
    [p.x, p.y, p.z].map(Ivl::from)
}

fn iv_sub(a: [Ivl; 3], b: [Ivl; 3]) -> [Ivl; 3] {
    let [ax, ay, az] = a;
    let [bx, by, bz] = b;
    [ax - bx, ay - by, az - bz]
}

fn iv_cross(a: [Ivl; 3], b: [Ivl; 3]) -> [Ivl; 3] {
    let [ax, ay, az] = a;
    let [bx, by, bz] = b;
    [ay * bz - az * by, az * bx - ax * bz, ax * by - ay * bx]
}

fn iv_length(a: [Ivl; 3]) -> Ivl {
    a.into_iter()
        .fold(Ivl::from(0.0), |s, v| {
            s + Ivl::checked(v.mig() * v.mig(), v.mag() * v.mag())
        })
        .sqrt()
}

fn iv_lerp(a: [Ivl; 3], b: [Ivl; 3], t: Ivl) -> [Ivl; 3] {
    let [ax, ay, az] = a;
    let [bx, by, bz] = b;
    [ax + (bx - ax) * t, ay + (by - ay) * t, az + (bz - az) * t]
}

/// Whether `sum ±a b` over the four `(a, b, negative)` terms is exactly zero, each
/// product negated when its flag is set. The sum is an error-free expansion
/// ([`expansion`]); a product out of its exact range (a subnormal or non-finite factor,
/// one that overflows or could underflow) is not shown zero: false.
pub(crate) fn exact_four_products_zero(terms: [(f64, f64, bool); 4]) -> bool {
    let sum = terms
        .into_iter()
        .try_fold(Vec::new(), |sum, (a, b, negative)| {
            let p = expansion::product(a, b)?;
            Some(if negative {
                expansion::sub(&sum, &p)
            } else {
                expansion::add(&sum, &p)
            })
        });
    sum.is_some_and(|sum| expansion::is_zero(&sum))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nurbs::b_spline_curve::BSplineCurve;
    #[test]
    fn affine_point_enclosure_uses_actual_right_value_at_a_jump() {
        use crate::nurbs::math::CPoint;
        let net = crate::nurbs::crv_basic::Curve::new(
            vec![
                CPoint::new(100.0, 0.0, 0.0, 1.0),
                CPoint::new(1.0, 0.0, 0.0, 1.0),
                CPoint::new(20.0, 0.0, 0.0, 2.0),
                CPoint::new(10.0, 1.0, 0.0, 1.0),
            ],
            1,
            vec![0.0, 0.0, 1.0, 1.0, 2.0, 2.0],
        )
        .unwrap();
        let curve = Curve::from(BSplineCurve::from_nurb(net, 3, true));
        let at = affine_projected_point_enclosure(&curve, 1.0, Vec3::ZERO, Vec3::Z).unwrap();
        assert!(at.lo <= 10.0 && at.hi >= 10.0 && at.lo > 9.0);
        let left = affine_projected_point_enclosure(
            &curve,
            f64::from_bits(1.0_f64.to_bits() - 1),
            Vec3::ZERO,
            Vec3::Z,
        )
        .unwrap();
        assert!(left.lo < 1.000001 && left.hi < 1.000001);
        let end = affine_projected_point_enclosure(&curve, 2.0, Vec3::ZERO, Vec3::Z).unwrap();
        assert!(end.lo <= 101.0_f64.sqrt() && end.hi >= 101.0_f64.sqrt());
        assert!(affine_projected_point_enclosure(&curve, 3.0, Vec3::ZERO, Vec3::Z).is_none());
        let line = Curve::from(crate::curve::line::Line::infinite(
            Vec3::new(1.0, 2.0, 3.0),
            Vec3::X * 2.0,
            3,
            false,
        ));
        let metric = affine_projected_point_enclosure(&line, 0.5, Vec3::ZERO, Vec3::Z).unwrap();
        assert!(metric.lo <= 8.0_f64.sqrt() && metric.hi >= 8.0_f64.sqrt());
    }

    /// Native Line jets preserve the mapped evaluator and derivative orientation.
    #[test]
    fn native_line_jets_follow_stored_mapping() {
        use crate::curve::line::Line;
        for curve in [
            Curve::from(Line::from_points(
                Vec3::new(1.0, 2.0, 3.0),
                Vec3::new(5.0, 4.0, 3.0),
                3,
            )),
            Curve::from(Line::infinite(
                Vec3::new(1.0, 2.0, 3.0),
                Vec3::X * 2.0,
                3,
                false,
            )),
            Curve::from(Line::infinite(
                Vec3::new(1.0, 2.0, 3.0),
                Vec3::X * 2.0,
                3,
                true,
            )),
        ] {
            let definition = of_line(&curve, Extent1d::from_min_max(0.0, 1.0)).unwrap();
            let (point, derivative) = definition.jet_over(Ivl::from(0.25));
            let actual = curve
                .evaluate(&crate::Kernel::default(), 0.25, 1, true, true)
                .unwrap();
            for (bound, value) in point
                .into_iter()
                .zip([actual[0].x, actual[0].y, actual[0].z])
            {
                assert!(bound.lo <= value && value <= bound.hi);
            }
            for (bound, value) in
                derivative
                    .into_iter()
                    .zip([actual[1].x, actual[1].y, actual[1].z])
            {
                assert!(bound.lo <= value && value <= bound.hi);
            }
        }
    }
    /// Exact identities must survive unlike-term cancellation at any in-range magnitude;
    /// a product out of the exact range (one that overflows, a subnormal or non-finite
    /// factor) is not shown zero, so the trim is not shown rectangular.
    #[test]
    fn exact_trim_products_keep_extreme_identity() {
        for x in [2.0_f64.powi(1000), 2.0_f64.powi(-480), 0.1, 1.0] {
            assert!(exact_four_products_zero([
                (x, 2.0, false),
                (x, 3.0, false),
                (x, 4.0, true),
                (x, 1.0, true)
            ]));
            assert!(!exact_four_products_zero([
                (x, 2.0, false),
                (x, 3.0, false),
                (x, 4.0, true),
                (x, 0.0, true)
            ]));
        }
        // 1e16 + 1 - 1e16 - 1 rounds to -1 left to right; exactly it is zero
        assert!(exact_four_products_zero([
            (1e16, 1.0, false),
            (1.0, 1.0, false),
            (1e16, 1.0, true),
            (1.0, 1.0, true)
        ]));
        for x in [f64::MAX, f64::from_bits(1)] {
            assert!(!exact_four_products_zero([
                (x, 2.0, false),
                (x, 3.0, false),
                (x, 4.0, true),
                (x, 1.0, true)
            ]));
        }
        assert!(!exact_four_products_zero([(f64::INFINITY, 0.0, false); 4]));
    }

    /// A product that could underflow (`1e-160 * 1e-160`, below `2^-968`) is out of the
    /// exact range: the sum is not shown zero, though it cancels.
    #[test]
    fn exact_trim_products_refuse_an_out_of_range_product() {
        assert!(!exact_four_products_zero([
            (1e-160, 1e-160, false),
            (1e-160, 1e-160, true),
            (0.0, 0.0, false),
            (0.0, 0.0, false)
        ]));
        assert!(exact_four_products_zero([
            (1e-140, 1e-140, false),
            (1e-140, 1e-140, true),
            (0.0, 0.0, false),
            (0.0, 0.0, false)
        ]));
    }
}
