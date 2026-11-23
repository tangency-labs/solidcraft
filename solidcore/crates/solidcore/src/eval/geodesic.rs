//! Geodesics on a surface held as plain data: the shortest path between two points, its
//! intrinsic length, and the metric quantities a sketch constraint needs. One numerical path
//! for every surface kind, B-spline included; the analytic kinds go through the same code.
//!
//! The geodesic is a boundary-value solve: shoot from the first point in a tangent direction
//! and a length, then Newton-correct the direction and length until the shot ends at the
//! second point. Several starting directions are tried and the shortest path wins.
//!
//! Nothing panics: input outside the surface domain, a singular point and a solve that does
//! not converge are typed errors.

use super::pcurve;
use super::surface::{Metric, SurfaceEval};
use super::uv::{self, Domain};
use super::vec::VecExt;
use crate::math::Vec3;
use std::f64::consts::TAU;

const STEPS: usize = 32;
const MAX_NEWTON: usize = 24;
const SEEDS: [f64; 5] = [0.0, 0.4, -0.4, 0.8, -0.8];
/// The distance tolerance a free function uses, in millimetres.
pub const DEFAULT_TOLERANCE: f64 = 1e-6;
const PARAM_TOL: f64 = 1e-9;

/// Why a geodesic or an intrinsic length cannot be had.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum GeodesicError {
    /// Two distinct geodesics of the same length join the points.
    #[error("the shortest geodesic is ambiguous")]
    Ambiguous,
    /// The surface is singular (a pole, a collapsed parameter) on the way.
    #[error("the surface is singular on the geodesic")]
    Singular,
    /// A point or a step left the surface's parameter domain.
    #[error("the geodesic leaves the surface")]
    OffDomain,
    /// The boundary-value solve did not converge.
    #[error("the geodesic did not converge")]
    NotConverged,
    /// The surface kind or curve cannot be evaluated.
    #[error("the geodesic is not supported on this surface")]
    Unsupported,
}

/// A geodesic between two points.
#[derive(Clone, Debug, PartialEq)]
pub struct Geodesic {
    /// Samples in the surface's own (u, v), from the start to the end.
    pub samples: Vec<[f64; 2]>,
    /// The intrinsic length, in millimetres.
    pub length: f64,
    /// The tangent direction in (u, v) at the start.
    pub tan0: [f64; 2],
    /// The tangent direction in (u, v) at the end.
    pub tan1: [f64; 2],
    /// Whole turns taken along u and v (0 for the first slice).
    pub windings: [i32; 2],
}

struct Jet {
    su: Vec3,
    sv: Vec3,
    suu: Vec3,
    suv: Vec3,
    svv: Vec3,
}

/// A geodesic circle: the points at one intrinsic distance from a centre.
#[derive(Clone, Debug, PartialEq)]
pub struct GeodesicCircle {
    /// The centre in the surface's (u, v).
    pub center: [f64; 2],
    /// The intrinsic radius, in millimetres.
    pub radius: f64,
    /// The circle's points in the surface's (u, v), in angle order.
    pub samples: Vec<[f64; 2]>,
}

/// Geodesics on one surface, at one tolerance.
pub struct GeodesicSolver<'a> {
    surface: &'a SurfaceEval,
    domain: Domain,
    tolerance: f64,
}

impl<'a> GeodesicSolver<'a> {
    /// A solver on `surface`; `tolerance` is in millimetres.
    #[must_use]
    pub fn new(surface: &'a SurfaceEval, tolerance: f64) -> Self {
        let tolerance = if tolerance.is_finite() && tolerance > 0.0 {
            tolerance
        } else {
            DEFAULT_TOLERANCE
        };
        Self {
            surface,
            domain: surface.domain(),
            tolerance,
        }
    }

    /// The surface this solver reads.
    #[must_use]
    pub fn surface(&self) -> &'a SurfaceEval {
        self.surface
    }

    fn inside(&self, uv: [f64; 2]) -> Result<[f64; 2], GeodesicError> {
        let [u, v] = uv;
        let [ru, rv] = self.domain.range;
        let [pu, pv] = self.domain.period;
        Ok([Self::one(u, ru, pu)?, Self::one(v, rv, pv)?])
    }

    fn one(x: f64, range: Option<(f64, f64)>, period: Option<f64>) -> Result<f64, GeodesicError> {
        if !x.is_finite() {
            return Err(GeodesicError::OffDomain);
        }
        if let Some(p) = period.filter(|p| *p > 0.0) {
            let (lo, _) = range.unwrap_or((0.0, p));
            return Ok(lo + (x - lo).rem_euclid(p));
        }
        match range {
            Some((lo, hi)) if x >= lo - PARAM_TOL && x <= hi + PARAM_TOL => Ok(x.clamp(lo, hi)),
            Some(_) => Err(GeodesicError::OffDomain),
            None => Ok(x),
        }
    }

    fn jet(&self, uv: [f64; 2]) -> Result<Jet, GeodesicError> {
        let [u, v] = self.inside(uv)?;
        let (_, su, sv) = self.surface.derivs(u, v).ok_or(GeodesicError::OffDomain)?;
        let (suu, suv, svv) = match self.surface.second(u, v) {
            Some(t) => t,
            None => self.second_by_difference(u, v)?,
        };
        Ok(Jet {
            su,
            sv,
            suu,
            suv,
            svv,
        })
    }

    fn second_by_difference(&self, u: f64, v: f64) -> Result<(Vec3, Vec3, Vec3), GeodesicError> {
        let hu = 1e-5 * (1.0 + u.abs());
        let hv = 1e-5 * (1.0 + v.abs());
        let at = |u: f64, v: f64| {
            self.surface
                .derivs(u, v)
                .map(|(_, su, sv)| (su, sv))
                .ok_or(GeodesicError::OffDomain)
        };
        let (su_p, _) = at(u + hu, v)?;
        let (su_m, _) = at(u - hu, v)?;
        let (_, sv_p) = at(u, v + hv)?;
        let (_, sv_m) = at(u, v - hv)?;
        let (_, suv_p) = at(u, v + hv)?;
        let (_, suv_m) = at(u, v - hv)?;
        Ok((
            (su_p - su_m) * (0.5 / hu),
            (suv_p - suv_m) * (0.5 / hv),
            (sv_p - sv_m) * (0.5 / hv),
        ))
    }

    fn basis(jet: &Jet) -> Result<(Vec3, Vec3), GeodesicError> {
        let e1 = jet.su.unitize().map_err(|_| GeodesicError::Singular)?;
        let e2 = (jet.sv - e1 * jet.sv.dot(e1))
            .unitize()
            .map_err(|_| GeodesicError::Singular)?;
        Ok((e1, e2))
    }

    fn parameter_direction(jet: &Jet, w: Vec3) -> Result<(f64, f64), GeodesicError> {
        let m = Metric {
            e: jet.su.dot(jet.su),
            f: jet.su.dot(jet.sv),
            g: jet.sv.dot(jet.sv),
        };
        let det = m.det();
        if !(det.is_finite() && det > 0.0) {
            return Err(GeodesicError::Singular);
        }
        let wu = w.dot(jet.su);
        let wv = w.dot(jet.sv);
        Ok(((m.g * wu - m.f * wv) / det, (m.e * wv - m.f * wu) / det))
    }

    fn rate(&self, s: [f64; 4]) -> Result<[f64; 4], GeodesicError> {
        let [u, v, a, b] = s;
        let jet = self.jet([u, v])?;
        let m = Metric {
            e: jet.su.dot(jet.su),
            f: jet.su.dot(jet.sv),
            g: jet.sv.dot(jet.sv),
        };
        let det = m.det();
        if !(det.is_finite() && det > 0.0) {
            return Err(GeodesicError::Singular);
        }
        let q = jet.suu * (a * a) + jet.suv * (2.0 * a * b) + jet.svv * (b * b);
        let (ru, rv) = (-jet.su.dot(q), -jet.sv.dot(q));
        Ok([
            a,
            b,
            (m.g * ru - m.f * rv) / det,
            (m.e * rv - m.f * ru) / det,
        ])
    }

    fn integrate(
        &self,
        start: [f64; 4],
        length: f64,
        steps: usize,
    ) -> Result<(Vec<[f64; 2]>, [f64; 4]), GeodesicError> {
        if !(length > 0.0 && length.is_finite()) || steps == 0 {
            return Err(GeodesicError::NotConverged);
        }
        let h = length / steps as f64;
        let add = |s: [f64; 4], r: [f64; 4], f: f64| {
            let [a0, a1, a2, a3] = s;
            let [b0, b1, b2, b3] = r;
            [a0 + b0 * f, a1 + b1 * f, a2 + b2 * f, a3 + b3 * f]
        };
        let mut s = start;
        let mut samples = Vec::new();
        let [u0, v0, _, _] = s;
        samples.push([u0, v0]);
        for _ in 0..steps {
            let k1 = self.rate(s)?;
            let k2 = self.rate(add(s, k1, 0.5 * h))?;
            let k3 = self.rate(add(s, k2, 0.5 * h))?;
            let k4 = self.rate(add(s, k3, h))?;
            let mut next = s;
            for (x, a, b, c, d) in next
                .iter_mut()
                .zip(k1)
                .zip(k2)
                .zip(k3)
                .zip(k4)
                .map(|((((x, a), b), c), d)| (x, a, b, c, d))
            {
                *x += h / 6.0 * (a + 2.0 * b + 2.0 * c + d);
            }
            s = next;
            let [u, v, _, _] = s;
            samples.push([u, v]);
        }
        Ok((samples, s))
    }

    fn residual(&self, end: [f64; 2], to: [f64; 2]) -> Result<[f64; 2], GeodesicError> {
        let end = self.inside(end)?;
        let to = self.inside(to)?;
        let [tu0, tv0] = to;
        let (_, tu, tv) = self
            .surface
            .derivs(tu0, tv0)
            .ok_or(GeodesicError::OffDomain)?;
        let [period_u, period_v] = self.domain.period;
        let [eu, ev] = end;
        let du = uv::unwrap(eu, tu0, period_u) - tu0;
        let dv = uv::unwrap(ev, tv0, period_v) - tv0;
        let d = tu * du + tv * dv;
        let e1 = tu.unitize().map_err(|_| GeodesicError::Singular)?;
        let e2 = (tv - e1 * tv.dot(e1))
            .unitize()
            .map_err(|_| GeodesicError::Singular)?;
        Ok([d.dot(e1), d.dot(e2)])
    }

    fn try_shot(
        &self,
        start: [f64; 2],
        to: [f64; 2],
        jet: &Jet,
        e1: Vec3,
        e2: Vec3,
        theta: f64,
        length: f64,
    ) -> Result<[f64; 2], GeodesicError> {
        let w = e1 * theta.cos() + e2 * theta.sin();
        let (a, b) = Self::parameter_direction(jet, w)?;
        let [su, sv] = start;
        let (_, s) = self.integrate([su, sv, a, b], length, STEPS)?;
        let [u, v, _, _] = s;
        self.residual([u, v], to)
    }

    fn shoot(
        &self,
        start: [f64; 2],
        to: [f64; 2],
        jet: &Jet,
        e1: Vec3,
        e2: Vec3,
        mut theta: f64,
        mut length: f64,
    ) -> Result<Geodesic, GeodesicError> {
        for _ in 0..MAX_NEWTON {
            let r = self.try_shot(start, to, jet, e1, e2, theta, length)?;
            let [r0, r1] = r;
            if (r0 * r0 + r1 * r1).sqrt() <= self.tolerance {
                let w = e1 * theta.cos() + e2 * theta.sin();
                let (a, b) = Self::parameter_direction(jet, w)?;
                let [su, sv] = start;
                let (samples, s) = self.integrate([su, sv, a, b], length, STEPS)?;
                let [_, _, ta, tb] = s;
                return Ok(Geodesic {
                    samples,
                    length,
                    tan0: [a, b],
                    tan1: [ta, tb],
                    windings: [0, 0],
                });
            }
            let ht = 1e-6;
            let hl = 1e-6 * length.max(1.0);
            let rt = self.try_shot(start, to, jet, e1, e2, theta + ht, length)?;
            let rl = self.try_shot(start, to, jet, e1, e2, theta, length + hl)?;
            let [rt0, rt1] = rt;
            let [rl0, rl1] = rl;
            let j00 = (rt0 - r0) / ht;
            let j10 = (rt1 - r1) / ht;
            let j01 = (rl0 - r0) / hl;
            let j11 = (rl1 - r1) / hl;
            let det = j00 * j11 - j01 * j10;
            if !(det.abs().is_finite() && det.abs() > 1e-14) {
                break;
            }
            let dtheta = (-r0 * j11 + r1 * j01) / det;
            let dlength = (-r1 * j00 + r0 * j10) / det;
            if !(dtheta.is_finite() && dlength.is_finite()) {
                break;
            }
            theta += dtheta;
            length += dlength;
            if length <= 1e-9 {
                break;
            }
        }
        Err(GeodesicError::NotConverged)
    }

    /// The shortest geodesic between two points.
    pub fn geodesic(&self, from: [f64; 2], to: [f64; 2]) -> Result<Geodesic, GeodesicError> {
        let start = self.inside(from)?;
        let target = self.inside(to)?;
        let pa = self.surface.point(start).ok_or(GeodesicError::OffDomain)?;
        let pb = self.surface.point(target).ok_or(GeodesicError::OffDomain)?;
        let chord = pb - pa;
        let chord_len = chord.len();
        if chord_len <= self.tolerance {
            return Ok(Geodesic {
                samples: vec![start, target],
                length: 0.0,
                tan0: [0.0, 0.0],
                tan1: [0.0, 0.0],
                windings: [0, 0],
            });
        }
        let jet = self.jet(start)?;
        let (e1, e2) = Self::basis(&jet)?;
        let base = chord.dot(e2).atan2(chord.dot(e1));
        let mut best: Option<Geodesic> = None;
        for offset in SEEDS {
            if let Ok(g) = self.shoot(start, target, &jet, e1, e2, base + offset, chord_len) {
                if best.as_ref().is_none_or(|b| g.length < b.length) {
                    best = Some(g);
                }
            }
        }
        best.ok_or(GeodesicError::NotConverged)
    }

    /// The shortest geodesic's length, in millimetres.
    pub fn distance(&self, from: [f64; 2], to: [f64; 2]) -> Result<f64, GeodesicError> {
        Ok(self.geodesic(from, to)?.length)
    }

    /// A geodesic circle of `radius` around `center`, sampled `samples` times.
    pub fn circle(
        &self,
        center: [f64; 2],
        radius: f64,
        samples: usize,
    ) -> Result<GeodesicCircle, GeodesicError> {
        if !(radius.is_finite() && radius > 0.0) || samples < 3 {
            return Err(GeodesicError::NotConverged);
        }
        let center = self.inside(center)?;
        let jet = self.jet(center)?;
        let (e1, e2) = Self::basis(&jet)?;
        let [cu, cv] = center;
        let mut points = Vec::new();
        for i in 0..samples {
            let theta = TAU * i as f64 / samples as f64;
            let w = e1 * theta.cos() + e2 * theta.sin();
            let (a, b) = Self::parameter_direction(&jet, w)?;
            let (_, state) = self.integrate([cu, cv, a, b], radius, STEPS)?;
            let [u, v, _, _] = state;
            points.push([u, v]);
        }
        Ok(GeodesicCircle {
            center,
            radius,
            samples: points,
        })
    }

    /// An arc of a geodesic circle, from angle `start` to `end` (radians).
    pub fn arc(
        &self,
        center: [f64; 2],
        radius: f64,
        start: f64,
        end: f64,
        samples: usize,
    ) -> Result<Geodesic, GeodesicError> {
        if !(radius.is_finite() && radius > 0.0) || samples < 2 {
            return Err(GeodesicError::NotConverged);
        }
        let center = self.inside(center)?;
        let jet = self.jet(center)?;
        let (e1, e2) = Self::basis(&jet)?;
        let [cu, cv] = center;
        let mut points = Vec::new();
        let mut tan0 = [0.0, 0.0];
        let mut tan1 = [0.0, 0.0];
        for i in 0..samples {
            let t = i as f64 / (samples - 1) as f64;
            let theta = start + (end - start) * t;
            let w = e1 * theta.cos() + e2 * theta.sin();
            let (a, b) = Self::parameter_direction(&jet, w)?;
            if i == 0 {
                tan0 = [a, b];
            }
            if i + 1 == samples {
                tan1 = [a, b];
            }
            let (_, state) = self.integrate([cu, cv, a, b], radius, STEPS)?;
            let [u, v, _, _] = state;
            points.push([u, v]);
        }
        let mut length = 0.0;
        for pair in points.windows(2) {
            let (Some(&[u0, v0]), Some(&[u1, v1])) = (pair.first(), pair.get(1)) else {
                continue;
            };
            let (um, vm) = (0.5 * (u0 + u1), 0.5 * (v0 + v1));
            let m = self
                .surface
                .metric(um, vm)
                .ok_or(GeodesicError::OffDomain)?;
            length += m.norm(u1 - u0, v1 - v0);
        }
        Ok(Geodesic {
            samples: points,
            length,
            tan0,
            tan1,
            windings: [0, 0],
        })
    }

    /// The intrinsic length of a pcurve over `interval`.
    pub fn intrinsic_length(
        &self,
        curve: &super::geometry::Curve2,
        interval: (f64, f64),
    ) -> Result<f64, GeodesicError> {
        let (a, b) = interval;
        if !(b > a && a.is_finite() && b.is_finite()) {
            return Err(GeodesicError::OffDomain);
        }
        let steps = 128usize;
        let h = (b - a) / steps as f64;
        let mut sum = 0.0;
        for k in 0..=steps {
            let t = a + h * k as f64;
            let (uv, d) = pcurve::eval_d1(curve, t).ok_or(GeodesicError::OffDomain)?;
            let [u, v] = uv;
            let [du, dv] = d;
            let m = self.surface.metric(u, v).ok_or(GeodesicError::OffDomain)?;
            let speed = m.norm(du, dv);
            let weight = if k == 0 || k == steps {
                1.0
            } else if k % 2 == 0 {
                2.0
            } else {
                4.0
            };
            sum += weight * speed;
        }
        Ok(sum * h / 3.0)
    }

    /// The 3D tangent of a chart direction at a point.
    pub fn tangent_at(&self, uv: [f64; 2], direction: [f64; 2]) -> Result<Vec3, GeodesicError> {
        let [u, v] = self.inside(uv)?;
        let (_, su, sv) = self.surface.derivs(u, v).ok_or(GeodesicError::OffDomain)?;
        let [du, dv] = direction;
        Ok(su * du + sv * dv)
    }

    /// The metric angle between two chart directions at a point.
    pub fn metric_angle(
        &self,
        uv: [f64; 2],
        a: [f64; 2],
        b: [f64; 2],
    ) -> Result<f64, GeodesicError> {
        let [u, v] = self.inside(uv)?;
        let m = self.surface.metric(u, v).ok_or(GeodesicError::OffDomain)?;
        let [a0, a1] = a;
        let [b0, b1] = b;
        let angle = m.angle((a0, a1), (b0, b1));
        if angle.is_finite() {
            Ok(angle)
        } else {
            Err(GeodesicError::Singular)
        }
    }
}

/// The shortest geodesic between two points of `surface`.
pub fn geodesic(
    surface: &SurfaceEval,
    from: [f64; 2],
    to: [f64; 2],
) -> Result<Geodesic, GeodesicError> {
    GeodesicSolver::new(surface, DEFAULT_TOLERANCE).geodesic(from, to)
}

/// The shortest geodesic's length, in millimetres.
pub fn distance(surface: &SurfaceEval, from: [f64; 2], to: [f64; 2]) -> Result<f64, GeodesicError> {
    GeodesicSolver::new(surface, DEFAULT_TOLERANCE).distance(from, to)
}

/// A geodesic circle on `surface`.
pub fn circle(
    surface: &SurfaceEval,
    center: [f64; 2],
    radius: f64,
    samples: usize,
) -> Result<GeodesicCircle, GeodesicError> {
    GeodesicSolver::new(surface, DEFAULT_TOLERANCE).circle(center, radius, samples)
}

/// An arc of a geodesic circle on `surface`.
pub fn arc(
    surface: &SurfaceEval,
    center: [f64; 2],
    radius: f64,
    start: f64,
    end: f64,
    samples: usize,
) -> Result<Geodesic, GeodesicError> {
    GeodesicSolver::new(surface, DEFAULT_TOLERANCE).arc(center, radius, start, end, samples)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval::geometry::{Curve2, Frame, Surface};
    use std::f64::consts::{FRAC_PI_2, PI};

    fn frame() -> Frame {
        Frame {
            origin: [0.0, 0.0, 0.0],
            x_axis: [1.0, 0.0, 0.0],
            y_axis: [0.0, 1.0, 0.0],
        }
    }

    fn sphere(radius: f64) -> SurfaceEval {
        SurfaceEval::new(&Surface::Sphere {
            frame: frame(),
            radius,
        })
        .expect("a sphere")
    }

    fn plane() -> SurfaceEval {
        SurfaceEval::new(&Surface::Plane {
            origin: [0.0, 0.0, 0.0],
            u_axis: [1.0, 0.0, 0.0],
            v_axis: [0.0, 1.0, 0.0],
        })
        .expect("a plane")
    }

    fn cylinder(radius: f64) -> SurfaceEval {
        SurfaceEval::new(&Surface::Cylinder {
            frame: frame(),
            radius,
        })
        .expect("a cylinder")
    }

    #[test]
    fn a_sphere_quarter_is_a_quarter_great_circle() {
        let s = sphere(2.0);
        let d = distance(&s, [0.0, 0.0], [FRAC_PI_2, 0.0]).expect("a geodesic");
        assert!((d - PI).abs() <= 1e-6, "{d}");
    }

    #[test]
    fn a_plane_keeps_euclidean_distance() {
        let s = plane();
        let d = distance(&s, [0.0, 0.0], [3.0, 4.0]).expect("a geodesic");
        assert!((d - 5.0).abs() <= 1e-6, "{d}");
    }

    #[test]
    fn a_cylinders_shortest_path_is_its_helix() {
        let s = cylinder(2.0);
        let d = distance(&s, [0.0, 0.0], [PI, 2.0]).expect("a geodesic");
        let expected = (4.0 * PI * PI + 4.0).sqrt();
        assert!((d - expected).abs() <= 1e-5, "{d} vs {expected}");
    }

    #[test]
    fn a_b_spline_plane_matches_the_plane_it_represents() {
        let s = SurfaceEval::new(&Surface::Spline {
            u_knots: vec![0.0, 0.0, 1.0, 1.0],
            v_knots: vec![0.0, 0.0, 1.0, 1.0],
            points: vec![
                vec![[0.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
                vec![[1.0, 0.0, 0.0], [1.0, 1.0, 0.0]],
            ],
        })
        .expect("a spline surface");
        let d = distance(&s, [0.1, 0.1], [0.9, 0.9]).expect("a geodesic");
        let expected = 2.0f64.sqrt() * 0.8;
        assert!((d - expected).abs() <= 1e-6, "{d} vs {expected}");
    }

    #[test]
    fn a_sphere_circle_keeps_its_geodesic_radius() {
        let s = sphere(2.0);
        let solver = GeodesicSolver::new(&s, DEFAULT_TOLERANCE);
        let c = solver.circle([0.0, 0.0], 0.5, 12).expect("a circle");
        assert_eq!(c.samples.len(), 12);
        for p in c.samples {
            let d = solver.distance([0.0, 0.0], p).expect("a radius");
            assert!((d - 0.5).abs() <= 1e-5, "{d}");
        }
    }

    #[test]
    fn an_arc_starts_and_ends_on_the_circle() {
        let s = sphere(2.0);
        let solver = GeodesicSolver::new(&s, DEFAULT_TOLERANCE);
        let a = solver
            .arc([0.0, 0.0], 0.5, 0.0, FRAC_PI_2, 8)
            .expect("an arc");
        assert_eq!(a.samples.len(), 8);
        for p in [a.samples.first(), a.samples.last()].into_iter().flatten() {
            let d = solver.distance([0.0, 0.0], *p).expect("a radius");
            assert!((d - 0.5).abs() <= 1e-5, "{d}");
        }
        assert!(a.length > 0.0);
    }

    #[test]
    fn a_curved_b_spline_matches_the_cylinder_it_represents() {
        let w = std::f64::consts::FRAC_1_SQRT_2;
        let mut points = Vec::new();
        for i in 0..9 {
            let angle = std::f64::consts::FRAC_PI_4 * i as f64;
            let wt = if i % 2 == 0 { 1.0 } else { w };
            let (s, c) = angle.sin_cos();
            points.push(vec![
                [2.0 * c, 2.0 * s, 0.0, wt],
                [2.0 * c, 2.0 * s, 3.0, wt],
            ]);
        }
        let b = SurfaceEval::new(&Surface::RationalSpline {
            u_knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 2.0, 2.0, 3.0, 3.0, 4.0, 4.0, 4.0],
            v_knots: vec![0.0, 0.0, 1.0, 1.0],
            homogeneous: points,
        })
        .expect("a B-spline cylinder");
        let c = cylinder(2.0);
        let db = distance(&b, [1.0, 2.0 / 3.0], [2.0, 2.0 / 3.0]).expect("a B-spline geodesic");
        let dc = distance(&c, [FRAC_PI_2, 2.0], [PI, 2.0]).expect("a cylinder geodesic");
        assert!((db - dc).abs() <= 1e-4, "{db} vs {dc}");
    }

    #[test]
    fn a_pcurve_measures_its_surface_length() {
        let s = plane();
        let curve = Curve2::Spline {
            knots: vec![0.0, 0.0, 1.0, 1.0],
            points: vec![[0.0, 0.0], [3.0, 4.0]],
        };
        let solver = GeodesicSolver::new(&s, DEFAULT_TOLERANCE);
        let l = solver
            .intrinsic_length(&curve, (0.0, 1.0))
            .expect("a length");
        assert!((l - 5.0).abs() <= 1e-6, "{l}");
    }
}
