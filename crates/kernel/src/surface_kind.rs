//! What surface a face lies on (plane, cylinder, cone, sphere, torus), for construction
//! geometry: axes through round faces, centres of spheres and tori.

use solidcraft_geom::{UnitVec3, Vec3};

use truck_modeling as mt;

use crate::Body;
use crate::body::from_p3;
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
    /// `major` is signed: negative on a spindle torus's inner sheet, whose tube centre
    /// lies across the axis from the face (a revolved barrel).
    Torus {
        center: Vec3,
        axis: UnitVec3,
        major: f64,
        minor: f64,
    },
}

/// The surface of face `face` (in face order), if it is one of the analytic kinds.
pub fn face_surface(b: &Body, face: usize) -> Option<FaceSurface> {
    surface_of(b.solid.face_iter().nth(face)?, b.size())
}

/// The analytic surface of face `f` of a body whose box diagonal is `size`.
pub(crate) fn surface_of(f: &mt::Face, size: f64) -> Option<FaceSurface> {
    let tol = (size * 1e-7).max(1e-9) * 100.0;
    Some(match surf_of(f, tol)? {
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
/// lies on, if its curve is a circle or an arc of one. `None` for other curves, mesh bodies
/// and an index out of range.
pub fn edge_circle(b: &Body, edge: usize) -> Option<EdgeCircle> {
    use mt::{BoundedCurve, ParametricCurve};
    if b.mesh.is_some() {
        return None;
    }
    let e = Body::unique_edges(&b.solid).into_iter().nth(edge)?;
    let tol = (b.size() * 1e-7).max(1e-9) * 100.0;
    crate::guard("edge circle", || {
        let c = e.oriented_curve();
        let (t0, t1) = c.range_tuple();
        let at = |f: f64| from_p3(c.subs(t0 + (t1 - t0) * f));
        let samples: Vec<Vec3> = (0..=24).map(|i| at(f64::from(i) / 24.0)).collect();
        Ok(circle_through(&samples, tol))
    })
    .ok()
    .flatten()
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

/// A body's B-rep corners, and the corners each edge runs between.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Corners {
    /// The corners' points, in the body's vertex order.
    pub points: Vec<Vec3>,
    /// Per edge (in the order of [`Body::edges`]): the corners at its start and end (the same
    /// one for a closed edge), as indices into `points`.
    pub ends: Vec<[usize; 2]>,
}

/// The corners of `b` and the edges' ends (empty for a mesh body).
pub fn corners(b: &Body) -> Corners {
    if b.mesh.is_some() {
        return Corners::default();
    }
    let mut index = std::collections::HashMap::new();
    let mut points = Vec::new();
    for v in b.solid.vertex_iter() {
        index.entry(v.id()).or_insert_with(|| {
            points.push(from_p3(v.point()));
            points.len() - 1
        });
    }
    let ends = Body::unique_edges(&b.solid)
        .iter()
        .map(|e| [index.get(&e.front().id()).copied().unwrap_or(0), index.get(&e.back().id()).copied().unwrap_or(0)])
        .collect();
    Corners { points, ends }
}

/// A straight edge: its ends.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EdgeLine {
    pub start: Vec3,
    pub end: Vec3,
}

/// Edge `edge` (in the order of [`Body::edges`]) if its exact curve is a straight line, sampled
/// along it. `None` for other curves, mesh bodies and an index out of range.
pub fn edge_line(b: &Body, edge: usize) -> Option<EdgeLine> {
    use mt::{BoundedCurve, ParametricCurve};
    if b.mesh.is_some() {
        return None;
    }
    let e = Body::unique_edges(&b.solid).into_iter().nth(edge)?;
    let tol = (b.size() * 1e-7).max(1e-9) * 100.0;
    crate::guard("edge line", || {
        let c = e.oriented_curve();
        let (t0, t1) = c.range_tuple();
        let at = |f: f64| from_p3(c.subs(t0 + (t1 - t0) * f));
        let (start, end) = (at(0.0), at(1.0));
        let Some(dir) = (end - start).normalized() else { return Ok(None) };
        let straight = (1..24).map(|i| at(f64::from(i) / 24.0)).all(|p| {
            let w = p - start;
            (w - dir * w.dot(dir)).len() <= tol
        });
        Ok(straight.then_some(EdgeLine { start, end }))
    })
    .ok()
    .flatten()
}

/// A face's free-form surface as a NURBS patch: `points[i][j]` with `weights[i][j]` over the
/// knots `knots_u` (index i) and `knots_v` (index j). The patch is the face's surface oriented as
/// the face: ∂S/∂u × ∂S/∂v points out of the material. The face is the part of it its edges
/// bound.
#[derive(Clone, Debug, PartialEq)]
pub struct FaceNurbs {
    pub degree_u: usize,
    pub degree_v: usize,
    pub knots_u: Vec<f64>,
    pub knots_v: Vec<f64>,
    pub points: Vec<Vec<Vec3>>,
    pub weights: Vec<Vec<f64>>,
}

/// The NURBS patch of face `face` (in face order) when its surface is a B-spline or NURBS
/// surface. `None` for planes and surfaces of revolution (their analytic kinds are in
/// [`face_surface`]), for mesh bodies, and for an index out of range.
pub fn face_nurbs(b: &Body, face: usize) -> Option<FaceNurbs> {
    if b.mesh.is_some() {
        return None;
    }
    let f = b.solid.face_iter().nth(face)?;
    let (knots, points, weights) = match f.oriented_surface() {
        mt::Surface::BSplineSurface(s) => {
            let points: Vec<Vec<Vec3>> = s.control_points().iter().map(|row| row.iter().map(|p| from_p3(*p)).collect()).collect();
            let weights = points.iter().map(|row| vec![1.0; row.len()]).collect();
            (s.knot_vecs().clone(), points, weights)
        }
        mt::Surface::NurbsSurface(s) => {
            let mut points = Vec::new();
            let mut weights = Vec::new();
            for row in s.control_points() {
                let mut pr = Vec::with_capacity(row.len());
                let mut wr = Vec::with_capacity(row.len());
                for h in row {
                    if !(h.w.is_finite() && h.w > 0.0) {
                        return None;
                    }
                    pr.push(Vec3::new(h.x / h.w, h.y / h.w, h.z / h.w));
                    wr.push(h.w);
                }
                points.push(pr);
                weights.push(wr);
            }
            (s.knot_vecs().clone(), points, weights)
        }
        _ => return None,
    };
    let (ku, kv): (Vec<f64>, Vec<f64>) = (knots.0.to_vec(), knots.1.to_vec());
    let nu = points.len();
    let nv = points.first().map_or(0, Vec::len);
    if nu == 0 || nv == 0 || ku.len() <= nu || kv.len() <= nv || points.iter().any(|r| r.len() != nv) {
        return None;
    }
    let finite = points.iter().flatten().all(|p| p.is_finite()) && ku.iter().chain(&kv).all(|k| k.is_finite());
    finite.then(|| FaceNurbs { degree_u: ku.len() - nu - 1, degree_v: kv.len() - nv - 1, knots_u: ku, knots_v: kv, points, weights })
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
