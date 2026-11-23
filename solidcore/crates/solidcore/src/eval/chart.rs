//! A chart on a surface: an affine map from a local (x, y) to the surface's own (u, v),
//! metric-orthonormal at its origin. Over the whole plane of (u, v), so the surface's domain
//! bounds it; it has no box of its own.
//!
//! The axes are chosen so a length at the origin is a length of the chart: the reference
//! direction, projected onto the tangent plane, is the u axis, and the metric-orthogonal
//! direction is v. Away from the origin the map is not an isometry; constraints measure with
//! the surface metric, not the chart's.

use super::surface::{Metric, SurfaceEval};
use super::vec::VecExt;
use crate::math::Vec3;

/// A chart on a surface: origin, u axis and v axis in the surface's own (u, v).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Chart {
    /// The origin in the surface's (u, v).
    pub origin: [f64; 2],
    /// The u axis in (u, v), metric-unit at the origin.
    pub u_axis: [f64; 2],
    /// The v axis in (u, v), metric-unit and metric-orthogonal to u.
    pub v_axis: [f64; 2],
}

impl Chart {
    /// A chart at `origin` whose u axis follows `reference` projected onto the tangent
    /// plane. `None` where the surface cannot be evaluated, is singular, or `reference` has no
    /// tangent component (the surface's u direction is used then).
    #[must_use]
    pub fn build(surface: &SurfaceEval, origin: [f64; 2], reference: Vec3) -> Option<Chart> {
        let [u, v] = origin;
        let m = surface.metric(u, v)?;
        let (_, su, sv) = surface.derivs(u, v)?;
        let normal = su.cross(sv).unitize().ok()?;
        let mut w = reference - normal * reference.dot(normal);
        if w.len() <= 1e-12 {
            w = su;
        }
        let (a, b) = Self::parameter_direction(&m, su, sv, w)?;
        let (a, b) = Self::unit(&m, a, b)?;
        let (p, q) = (m.f * a + m.g * b, -(m.e * a + m.f * b));
        let (p, q) = Self::unit(&m, p, q)?;
        Some(Chart {
            origin,
            u_axis: [a, b],
            v_axis: [p, q],
        })
    }

    /// The tangent direction `(a, b)` with `S_u a + S_v b = w`.
    fn parameter_direction(m: &Metric, su: Vec3, sv: Vec3, w: Vec3) -> Option<(f64, f64)> {
        let det = m.det();
        if !(det.is_finite() && det > 0.0) {
            return None;
        }
        let (wu, wv) = (w.dot(su), w.dot(sv));
        let a = (m.g * wu - m.f * wv) / det;
        let b = (m.e * wv - m.f * wu) / det;
        [a, b].into_iter().all(f64::is_finite).then_some((a, b))
    }

    /// `(a, b)` made unit under `m`.
    fn unit(m: &Metric, a: f64, b: f64) -> Option<(f64, f64)> {
        let n = m.norm(a, b);
        if !(n.is_finite() && n > 0.0) {
            return None;
        }
        Some((a / n, b / n))
    }

    /// The chart (x, y) as the surface's (u, v).
    #[must_use]
    pub fn lift(&self, x: f64, y: f64) -> [f64; 2] {
        [
            self.origin[0] + x * self.u_axis[0] + y * self.v_axis[0],
            self.origin[1] + x * self.u_axis[1] + y * self.v_axis[1],
        ]
    }

    /// The surface's (u, v) as the chart (x, y), or `None` where the axes are degenerate.
    #[must_use]
    pub fn lower(&self, uv: [f64; 2]) -> Option<[f64; 2]> {
        let [a, b] = self.u_axis;
        let [c, d] = self.v_axis;
        let det = a * d - b * c;
        if !(det.is_finite() && det.abs() > 1e-15) {
            return None;
        }
        let [du, dv] = [uv[0] - self.origin[0], uv[1] - self.origin[1]];
        let x = (d * du - c * dv) / det;
        let y = (-b * du + a * dv) / det;
        [x, y].into_iter().all(f64::is_finite).then_some([x, y])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval::geometry::{Frame, Surface};

    fn sphere(radius: f64) -> SurfaceEval {
        SurfaceEval::new(&Surface::Sphere {
            frame: Frame {
                origin: [0.0, 0.0, 0.0],
                x_axis: [1.0, 0.0, 0.0],
                y_axis: [0.0, 1.0, 0.0],
            },
            radius,
        })
        .expect("a sphere")
    }

    #[test]
    fn a_chart_is_metric_orthonormal_at_its_origin() {
        let s = sphere(2.0);
        let c = Chart::build(&s, [0.0, 0.0], Vec3::Z).expect("a chart");
        let m = s.metric(c.origin[0], c.origin[1]).expect("a metric");
        assert!((m.norm(c.u_axis[0], c.u_axis[1]) - 1.0).abs() <= 1e-12);
        assert!((m.norm(c.v_axis[0], c.v_axis[1]) - 1.0).abs() <= 1e-12);
        assert!(
            m.dot((c.u_axis[0], c.u_axis[1]), (c.v_axis[0], c.v_axis[1]))
                .abs()
                <= 1e-12
        );
    }

    #[test]
    fn lift_and_lower_are_inverse() {
        let s = sphere(2.0);
        let c = Chart::build(&s, [0.0, 0.0], Vec3::Z).expect("a chart");
        let uv = c.lift(0.3, -0.2);
        let xy = c.lower(uv).expect("a lower");
        assert!((xy[0] - 0.3).abs() <= 1e-12 && (xy[1] + 0.2).abs() <= 1e-12);
    }
}
