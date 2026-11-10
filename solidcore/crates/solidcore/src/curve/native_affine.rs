//! Native affine definition enclosures shared by certified projected queries.
//!
//! Upstream: the stored Line parameter map; interval certificates are solidcore's.
//! Deviations: no query solver, tolerance, or cache continuity is used; no recursion.

use crate::blend::fillet_section::interval::Ivl;
use crate::curve::curve::{Curve, CurveKind};
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

fn iv_point(p: Vec3) -> [Ivl; 3] {
    [p.x, p.y, p.z].map(Ivl::from)
}

fn iv_sub(a: [Ivl; 3], b: [Ivl; 3]) -> [Ivl; 3] {
    let [ax, ay, az] = a;
    let [bx, by, bz] = b;
    [ax - bx, ay - by, az - bz]
}

fn iv_lerp(a: [Ivl; 3], b: [Ivl; 3], t: Ivl) -> [Ivl; 3] {
    let [ax, ay, az] = a;
    let [bx, by, bz] = b;
    [ax + (bx - ax) * t, ay + (by - ay) * t, az + (bz - az) * t]
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
