//! What surface a face lies on (plane, cylinder, cone, sphere, torus), for construction
//! geometry: axes through round faces, centres of spheres and tori.

use solidcraft_geom::brep::eval::CurveEval;
use solidcraft_geom::brep::{Brep, Curve};
use solidcraft_geom::{UnitVec3, Vec3};

use crate::Body;
use crate::offset::{Surf, surf_of};

/// A face's analytic surface.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FaceSurface {
    /// Outward unit normal and a point on the plane.
    Plane {
        normal: UnitVec3,
        point: Vec3,
    },
    Cylinder {
        origin: Vec3,
        axis: UnitVec3,
        radius: f64,
    },
    /// Apex, axis pointing into the cone, half-angle (radians).
    Cone {
        apex: Vec3,
        axis: UnitVec3,
        half_angle: f64,
    },
    Sphere {
        center: Vec3,
        radius: f64,
    },
    /// Centre of the ring, its axis, the ring (major) and tube (minor) radii.
    Torus {
        center: Vec3,
        axis: UnitVec3,
        major: f64,
        minor: f64,
    },
}

/// The surface of face `face` (in face order), if it is one of the analytic kinds.
pub fn face_surface(b: &Body, face: usize) -> Option<FaceSurface> {
    if b.is_mesh() {
        return None;
    }
    surface_of(b.record(), face)
}

/// The analytic surface of face `face` of the record `b`, as the record holds it.
pub(crate) fn surface_of(b: &Brep, face: usize) -> Option<FaceSurface> {
    Some(match surf_of(b, face)? {
        Surf::Plane { n, d } => FaceSurface::Plane { normal: n.unit()?, point: n * d },
        Surf::Cylinder { o, a, r, .. } => FaceSurface::Cylinder { origin: o, axis: a.unit()?, radius: r },
        Surf::Cone { v, a, half, .. } => FaceSurface::Cone { apex: v, axis: a.unit()?, half_angle: half },
        Surf::Sphere { c, r, .. } => FaceSurface::Sphere { center: c, radius: r },
        Surf::Torus { o, a, big, h, r, .. } => FaceSurface::Torus { center: o + a * h, axis: a.unit()?, major: big, minor: r },
    })
}

/// The circle (or arc of one) an edge's curve lies on, from its exact curve.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EdgeCircle {
    pub center: Vec3,
    /// Normal of the circle's plane, oriented so the edge runs counter-clockwise about it.
    pub axis: UnitVec3,
    pub radius: f64,
    /// The edge's ends (the same point for a closed circle).
    pub start: Vec3,
    pub end: Vec3,
    /// The angle the edge spans (radians, 2π for a whole circle).
    pub angle: f64,
}

/// The circle edge `edge` (in the order of [`Body::edges`] and the tessellated mesh's edges)
/// lies on, if its curve is a circle or an arc of one: exactly from a circle record, and for any
/// other curve when its points all lie on one circle (a circle held as a rational spline). `None`
/// for other curves, mesh bodies and an index out of range.
pub fn edge_circle(b: &Body, edge: usize) -> Option<EdgeCircle> {
    if b.is_mesh() {
        return None;
    }
    let record = b.record();
    let e = record.edges.get(edge)?;
    let c = CurveEval::new(e.parameters()).ok()?;
    let [t0, t1] = e.interval;
    let (start, end) = (c.point(t0)?, c.point(t1)?);
    if let Curve::Circle { frame, radius } = &e.curve {
        // The edge runs counter-clockwise about the frame's z from its start to its end, over the
        // angles of its interval on the circle itself (a NURBS form is parameterised otherwise).
        let axis = Vec3::from(frame.x_axis).cross(Vec3::from(frame.y_axis)).unit()?;
        let closed = e.vertices[0] == e.vertices[1];
        let [a0, a1] = e.curve_interval()?;
        let angle = if closed { std::f64::consts::TAU } else { (a1 - a0).min(std::f64::consts::TAU) };
        return Some(EdgeCircle { center: Vec3::from(frame.origin), axis, radius: *radius, start, end, angle });
    }
    let tol = (b.size() * 1e-7).max(1e-9) * 100.0;
    let samples: Vec<Vec3> = (0..=24).map(|i| c.point(t0 + (t1 - t0) * f64::from(i) / 24.0)).collect::<Option<_>>()?;
    circle_through(&samples, tol)
}

/// The circle through evenly spaced samples of a curve (first to last), if they all lie on one.
fn circle_through(s: &[Vec3], tol: f64) -> Option<EdgeCircle> {
    let (start, end) = (*s.first()?, *s.last()?);
    let closed = start.dist(end) <= tol;
    // Three well-spread samples: a quarter, half and three quarters for a closed curve.
    let n = s.len().checked_sub(1)?;
    let (a, b, c) = if closed { (*s.get(n / 4)?, *s.get(n / 2)?, *s.get(3 * n / 4)?) } else { (start, *s.get(n / 2)?, end) };
    let (u, v) = (b - a, c - a);
    let w = u.cross(v);
    if !(w.len2() > u.len2() * v.len2() * 1e-16) {
        return None;
    }
    let center = a + (v * u.len2() - u * v.len2()).cross(w) * (1.0 / (2.0 * w.len2()));
    let axis = w.unit()?;
    let radius = center.dist(a);
    if !(radius.is_finite() && radius > tol) || s.iter().any(|p| (p.dist(center) - radius).abs() > tol || (*p - center).dot(axis).abs() > tol) {
        return None;
    }
    let x = (start - center).normalized()?;
    let y = axis.cross(x);
    let turn = |p: Vec3| {
        let d = p - center;
        let t = d.dot(y).atan2(d.dot(x));
        if t < 0.0 { t + std::f64::consts::TAU } else { t }
    };
    // Accumulate the turn sample to sample so the span is right whichever way the curve runs.
    let mut angle = 0.0;
    let mut last = 0.0;
    for p in s.iter().skip(1) {
        let t = turn(*p);
        let mut d = t - last;
        if d < -std::f64::consts::PI {
            d += std::f64::consts::TAU;
        } else if d > std::f64::consts::PI {
            d -= std::f64::consts::TAU;
        }
        angle += d;
        last = t;
    }
    let (axis, angle) = if angle < 0.0 { (-axis, -angle) } else { (axis, angle) };
    let angle = if closed { std::f64::consts::TAU } else { angle.min(std::f64::consts::TAU) };
    Some(EdgeCircle { center, axis, radius, start, end, angle })
}

impl FaceSurface {
    /// The point of the whole surface (not only of its face) nearest `p`. `None` where every
    /// point of a ring is as near (on a cylinder's or cone's axis, at a sphere's or torus's
    /// centre), and for a point that is not finite.
    pub fn nearest(&self, p: Vec3) -> Option<Vec3> {
        if !p.is_finite() {
            return None;
        }
        match *self {
            FaceSurface::Plane { normal, point } => Some(p - normal * (p - point).dot(normal)),
            FaceSurface::Cylinder { origin, axis, radius } => {
                let w = p - origin;
                let along = axis * w.dot(axis);
                Some(origin + along + (w - along).normalized()? * radius)
            }
            FaceSurface::Cone { apex, axis, half_angle } => {
                let w = p - apex;
                let out = (w - axis * w.dot(axis)).normalized()?;
                // The cone's line through the apex that p is nearest, in the plane of the axis and p.
                let line = axis * half_angle.cos() + out * half_angle.sin();
                Some(apex + line * w.dot(line).max(0.0))
            }
            FaceSurface::Sphere { center, radius } => Some(center + (p - center).normalized()? * radius),
            FaceSurface::Torus { center, axis, major, minor } => {
                let w = p - center;
                let tube = center + (w - axis * w.dot(axis)).normalized()? * major;
                Some(tube + (p - tube).normalized()? * minor)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn near(a: Option<Vec3>, b: Vec3) -> bool {
        a.is_some_and(|a| a.dist(b) < 1e-12)
    }

    /// Each surface's nearest point, worked out by hand.
    #[test]
    fn the_nearest_point_of_each_surface() {
        let plane = FaceSurface::Plane { normal: UnitVec3::Z, point: Vec3::new(0.0, 0.0, 4.0) };
        assert!(near(plane.nearest(Vec3::new(3.0, 5.0, 9.0)), Vec3::new(3.0, 5.0, 4.0)));
        // A cylinder of radius 5 about the line x = 1, y = 2: (7, 10, 3) is 10 from it along (3, 4)/5.
        let cylinder = FaceSurface::Cylinder { origin: Vec3::new(1.0, 2.0, 0.0), axis: UnitVec3::Z, radius: 5.0 };
        assert!(near(cylinder.nearest(Vec3::new(7.0, 10.0, 3.0)), Vec3::new(4.0, 6.0, 3.0)));
        assert!(cylinder.nearest(Vec3::new(1.0, 2.0, 3.0)).is_none(), "on the axis every direction is as near");
        let sphere = FaceSurface::Sphere { center: Vec3::new(1.0, 2.0, 3.0), radius: 5.0 };
        assert!(near(sphere.nearest(Vec3::new(1.0, 2.0, 13.0)), Vec3::new(1.0, 2.0, 8.0)));
        // A torus of ring radius 10 and tube radius 2 about Z: (14, 0, 0) is 2 beyond the tube's outside.
        let torus = FaceSurface::Torus { center: Vec3::ZERO, axis: UnitVec3::Z, major: 10.0, minor: 2.0 };
        assert!(near(torus.nearest(Vec3::new(14.0, 0.0, 0.0)), Vec3::new(12.0, 0.0, 0.0)));
        // A cone of half-angle 45 degrees: (2, 0, 0) is √2 along its line (1, 0, 1)/√2.
        let cone = FaceSurface::Cone { apex: Vec3::ZERO, axis: UnitVec3::Z, half_angle: std::f64::consts::FRAC_PI_4 };
        assert!(cone.nearest(Vec3::new(2.0, 0.0, 0.0)).is_some_and(|q| q.dist(Vec3::new(1.0, 0.0, 1.0)) < 1e-12));
        assert!(plane.nearest(Vec3::new(f64::NAN, 0.0, 0.0)).is_none());
    }
}
