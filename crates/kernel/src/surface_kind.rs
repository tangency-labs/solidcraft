//! What surface a face lies on (plane, cylinder, cone, sphere, torus), for construction
//! geometry: axes through round faces, centres of spheres and tori.

pub use solidcore_ops::axial_section::AxialPiece;
use solidcore_ops::axial_section::Revolved;
use solidcore_ops::face_surface::{Exact, FaceSurface as ExactSurface};
use solidcore_record::Vec3;
use solidcore_record::brep::{Brep, Curve, Face, Surface};
use solidcraft_geom::UnitVec3;
use solidcraft_geom::brep::eval::{CurveEval, SurfaceEval};
use solidcraft_geom::prelude::*;

use crate::offset::{Surf, surf_of};
use crate::{Body, KernelError, Result};

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
    surface_of(b.record(), face).or_else(|| torus_of_face(b, face))
}

/// Whether `a` and `b` are one surface within `tol` inside `region`, the box of the faces
/// compared ([`solidcore_ops::face_surface::same_surface`]). A surface or box the kernel
/// refuses (a number that is not finite, an empty box) is not one surface with any.
pub fn same_surface(a: &FaceSurface, b: &FaceSurface, tol: f64, region: &solidcraft_geom::Aabb3) -> bool {
    use solidcore_ops::face_surface::{Analytic, same_surface};
    let arr = |v: Vec3| [v.x, v.y, v.z];
    let ops = |s: &FaceSurface| match *s {
        FaceSurface::Plane { normal, point } => Analytic::Plane { point: arr(point), normal: arr(normal.vec()) },
        FaceSurface::Cylinder { origin, axis, radius } => Analytic::Cylinder { origin: arr(origin), axis: arr(axis.vec()), radius },
        FaceSurface::Cone { apex, axis, half_angle } => Analytic::Cone { apex: arr(apex), axis: arr(axis.vec()), half_angle },
        FaceSurface::Sphere { center, radius } => Analytic::Sphere { centre: arr(center), radius },
        FaceSurface::Torus { center, axis, major, minor } => Analytic::Torus { centre: arr(center), axis: arr(axis.vec()), major, minor },
    };
    same_surface(ops(a), ops(b), tol, arr(region.min), arr(region.max)).unwrap_or(false)
}

/// Whether two non-analytic face records lie on one surface within `tol`: identical definitions,
/// or tensor-product B-spline / NURBS nets with matching knots (and weights, for rational nets)
/// whose control points agree within `tol` everywhere. By the non-negative partition-of-unity
/// property of B-spline basis functions, control-point agreement within `tol` bounds the surface
/// distance over the entire parameter domain by `tol`, absorbing the last-bit rounding when a
/// boolean shifts operands far from the origin to the origin and back without conflating distinct
/// surfaces. For `Surface::Blend` and `Surface::Sweep` faces, falls back to comparing their certified
/// `nurbs` nets.
pub fn same_face_record_surface(a: &Face, b: &Face, tol: f64) -> bool {
    same_record_surface(&a.surface, &b.surface, tol)
        || (matches!((&a.surface, &b.surface), (Surface::Blend { .. }, Surface::Blend { .. }) | (Surface::Sweep { .. }, Surface::Sweep { .. }))
            && matches!((&a.nurbs, &b.nurbs), (Some(na), Some(nb)) if same_record_surface(na, nb, tol)))
}

/// Whether two record surfaces `a` and `b` are bit-for-bit identical or represent the same
/// B-spline / NURBS surface within `tol`.
pub fn same_record_surface(a: &Surface, b: &Surface, tol: f64) -> bool {
    if !(tol.is_finite() && tol >= 0.0) {
        return false;
    }
    if a == b {
        return true;
    }
    match (a, b) {
        (Surface::Spline { u_knots: ua, v_knots: va, points: pa }, Surface::Spline { u_knots: ub, v_knots: vb, points: pb }) => {
            knots_close(ua, ub)
                && knots_close(va, vb)
                && pa.len() == pb.len()
                && pa.iter().zip(pb).all(|(ra, rb)| ra.len() == rb.len() && ra.iter().zip(rb).all(|(p, q)| point_within(*p, *q, tol)))
        }
        (
            Surface::RationalSpline { u_knots: ua, v_knots: va, homogeneous: ha },
            Surface::RationalSpline { u_knots: ub, v_knots: vb, homogeneous: hb },
        ) => {
            knots_close(ua, ub)
                && knots_close(va, vb)
                && ha.len() == hb.len()
                && ha.iter().zip(hb).all(|(ra, rb)| ra.len() == rb.len() && ra.iter().zip(rb).all(|(p, q)| rational_point_within(*p, *q, tol)))
        }
        _ => false,
    }
}

fn knots_close(a: &[f64], b: &[f64]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(&x, &y)| x.is_finite() && y.is_finite() && (x - y).abs() <= 1e-12 * (1.0 + x.abs().max(y.abs())))
}

fn point_within(a: [f64; 3], b: [f64; 3], tol: f64) -> bool {
    if !(a[0].is_finite() && a[1].is_finite() && a[2].is_finite() && b[0].is_finite() && b[1].is_finite() && b[2].is_finite()) {
        return false;
    }
    let (dx, dy, dz) = ((a[0] - b[0]).abs(), (a[1] - b[1]).abs(), (a[2] - b[2]).abs());
    if !(dx <= tol && dy <= tol && dz <= tol) {
        return false;
    }
    if tol == 0.0 {
        return true;
    }
    let (sx, sy, sz) = (dx / tol, dy / tol, dz / tol);
    sx * sx + sy * sy + sz * sz <= 1.0
}

fn rational_point_within(a: [f64; 4], b: [f64; 4], tol: f64) -> bool {
    let (wa, wb) = (a[3], b[3]);
    if !(wa.is_finite() && wb.is_finite() && wa > 0.0 && wb > 0.0) {
        return false;
    }
    if !((wa - wb).abs() <= 1e-12 * wa.max(wb)) {
        return false;
    }
    point_within([a[0] / wa, a[1] / wa, a[2] / wa], [b[0] / wb, b[1] / wb, b[2] / wb], tol)
}

/// What torus face `face` lies on, if its surface is a torus held as a spline (such as a
/// constant-radius fillet).
fn torus_of_face(b: &Body, face: usize) -> Option<FaceSurface> {
    let rec = b.record();
    let f = rec.faces.get(face)?;
    match &f.surface {
        solidcore_record::brep::Surface::RationalSpline { .. } | solidcore_record::brep::Surface::Spline { .. } => {}
        _ => return None,
    }
    let eval = SurfaceEval::new(f.parameters()).ok()?;
    let [ru, rv] = f.domain;
    if !(ru[0].is_finite() && ru[1].is_finite() && rv[0].is_finite() && rv[1].is_finite()) {
        return None;
    }
    let (u0, u1) = (ru[0], ru[1]);
    let (v0, v1) = (rv[0], rv[1]);
    if u0 >= u1 || v0 >= v1 {
        return None;
    }
    let tol = (b.size() * 1e-6).max(1e-7) * 100.0;
    find_torus(&eval, u0, u1, v0, v1, tol, false).or_else(|| find_torus(&eval, u0, u1, v0, v1, tol, true))
}

/// Test if iso-curves in one parameter at stations of the other are circles of equal radius whose
/// centers lie on a circular axis of the torus.
fn find_torus(eval: &SurfaceEval, u0: f64, u1: f64, v0: f64, v1: f64, tol: f64, swapped: bool) -> Option<FaceSurface> {
    const N_STATIONS: usize = 5;
    const N_ARC: usize = 16;
    let (s0, s1, t0, t1) = if swapped { (v0, v1, u0, u1) } else { (u0, u1, v0, v1) };
    let point_at = |s: f64, t: f64| -> Option<Vec3> {
        let (u, v) = if swapped { (t, s) } else { (s, t) };
        eval.derivs(u, v).map(|(p, _, _)| p)
    };
    let mut minor_circles = Vec::with_capacity(N_STATIONS);
    for i in 0..N_STATIONS {
        let s = s0 + (s1 - s0) * (i as f64) / ((N_STATIONS - 1) as f64);
        let samples: Vec<Vec3> = (0..=N_ARC)
            .map(|j| {
                let t = t0 + (t1 - t0) * (j as f64) / (N_ARC as f64);
                point_at(s, t)
            })
            .collect::<Option<_>>()?;
        let c = circle_through(&samples, tol)?;
        minor_circles.push(c);
    }
    let minor = minor_circles.first()?.radius;
    if !minor_circles.iter().all(|c| (c.radius - minor).abs() <= tol) {
        return None;
    }
    let centers: Vec<Vec3> = minor_circles.iter().map(|c| c.center).collect();
    let major_circle = circle_through(&centers, tol)?;
    let center = major_circle.center;
    let axis = major_circle.axis;
    let major = major_circle.radius;
    if !(major.is_finite() && minor.is_finite() && major > 0.0 && minor > 0.0) {
        return None;
    }
    for i in 0..=3 {
        let s = s0 + (s1 - s0) * (i as f64) / 3.0;
        for j in 0..=3 {
            let t = t0 + (t1 - t0) * (j as f64) / 3.0;
            let p = point_at(s, t)?;
            let to_axis = p - center;
            let h = to_axis.dot(axis);
            let radial = to_axis - axis * h;
            let d = radial.len();
            let dist = ((d - major).powi(2) + h.powi(2)).sqrt();
            if (dist - minor).abs() > tol * 5.0 {
                return None;
            }
        }
    }
    Some(FaceSurface::Torus { center, axis, major, minor })
}

/// The analytic surface of face `face` of the record `b`, as the record holds it; a round kept
/// as its definition is the torus that definition gives, when it is one.
pub(crate) fn surface_of(b: &Brep, face: usize) -> Option<FaceSurface> {
    if matches!(b.faces.get(face)?.surface, Surface::Blend { .. }) {
        return match *ExactSurface::of(b, face).ok()?.exact() {
            Exact::Torus { centre, axis, major, minor } => {
                Some(FaceSurface::Torus { center: Vec3::from(centre), axis: Vec3::from(axis).unit()?, major, minor })
            }
            _ => None,
        };
    }
    Some(match surf_of(b, face)? {
        Surf::Plane { n, d } => FaceSurface::Plane { normal: n.unit()?, point: n * d },
        Surf::Cylinder { o, a, r, .. } => FaceSurface::Cylinder { origin: o, axis: a.unit()?, radius: r },
        Surf::Cone { v, a, half, .. } => FaceSurface::Cone { apex: v, axis: a.unit()?, half_angle: half },
        Surf::Sphere { c, r, .. } => FaceSurface::Sphere { center: c, radius: r },
        Surf::Torus { o, a, big, h, r, .. } => FaceSurface::Torus { center: o + a * h, axis: a.unit()?, major: big, minor: r },
    })
}

/// The section of face `face` of `b` by `plane` when the face's surface is one of revolution
/// as the record holds it (an analytic cylinder, cone, sphere or torus, not a spline that only
/// follows one) and the plane holds its axis wherever the axis passes the body's exact box
/// ([`solidcore_ops::axial_section::holds_axis`]): the surface's meridians in closed form
/// (lines, arcs and circles), cut where the face's edges cross the plane
/// ([`solidcore_ops::curve::plane_crossings`]) and kept where they lie on the face
/// ([`Body::point_lies_on_face`]), all at the body's model tolerance
/// ([`solidcore_ops::axial_section::axial_section`]). `Ok(None)` for any other face or plane, a
/// face with an edge held as the intersection of two surfaces (whose crossings are not exact),
/// a mesh body or an index out of range; `Ok(Some(vec![]))` when the plane misses the face.
///
/// # Errors
///
/// The kernel's failure to box the body, to cross an edge with the plane or to place a point on
/// the face.
pub fn axial_face_section(b: &Body, face: usize, plane: &solidcore_record::Plane) -> Result<Option<Vec<AxialPiece>>> {
    let arr = |v: Vec3| [v.x, v.y, v.z];
    if b.is_mesh() {
        return Ok(None);
    }
    let record = b.record();
    let on = match surface_of(record, face) {
        Some(FaceSurface::Cylinder { origin, axis, radius }) => Revolved::Cylinder { origin: arr(origin), axis: arr(axis.vec()), radius },
        Some(FaceSurface::Cone { apex, axis, half_angle }) => Revolved::Cone { apex: arr(apex), axis: arr(axis.vec()), half_angle },
        Some(FaceSurface::Sphere { center, radius }) => Revolved::Sphere { centre: arr(center), radius },
        Some(FaceSurface::Torus { center, axis, major, minor }) => Revolved::Torus { centre: arr(center), axis: arr(axis.vec()), major, minor },
        Some(FaceSurface::Plane { .. }) | None => return Ok(None),
    };
    let tol = record.tolerance;
    let (origin, normal) = (arr(plane.origin), arr(plane.normal()));
    let bounds = b.exact_bounds()?;
    let (lo, hi) = (bounds.min, bounds.max);
    let corners: Vec<[f64; 3]> =
        [lo.x, hi.x].into_iter().flat_map(|x| [lo.y, hi.y].into_iter().flat_map(move |y| [lo.z, hi.z].into_iter().map(move |z| [x, y, z]))).collect();
    if !solidcore_ops::axial_section::holds_axis(&on, origin, normal, &corners, tol) {
        return Ok(None);
    }
    let edges: Vec<&solidcore_record::brep::Edge> = face_edge_indices(b, face).into_iter().filter_map(|e| record.edges.get(e)).collect();
    if edges.iter().any(|edge| matches!(edge.curve, Curve::SurfaceIntersection { .. })) {
        return Ok(None);
    }
    let mut cuts = Vec::new();
    for edge in edges {
        let hits = solidcore_ops::curve::plane_crossings(edge.parameters(), edge.interval, plane)
            .map_err(|err| KernelError::Failed(format!("where an edge of face {face} crosses the sketch plane: {err}")))?;
        cuts.extend(hits.into_iter().map(|p| arr(plane.to_world(p))));
    }
    let on_face = |[x, y, z]: [f64; 3]| b.point_lies_on_face(face, Vec3::new(x, y, z), tol).map(|found| found.is_some());
    solidcore_ops::axial_section::axial_section(&on, origin, normal, &corners, tol, &cuts, on_face)
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

/// The record edges that bound face `face` of `b` (none for a face out of range).
pub fn face_edge_indices(b: &Body, face: usize) -> Vec<usize> {
    crate::offset::face_edges(b.record(), face).into_iter().map(|e| e as usize).collect()
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
        let [a0, a1] = solidcore_ops::label::edge_curve_interval(e)?;
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

/// The levels along `plane`'s normal where the line through each of `points` (in the plane) along
/// it meets `surface` carried on without end (a torus: the span of its bounding sphere, which
/// holds every meeting): where an extrusion over those points must run to reach it
/// ([`solidcore_ops::extended_levels::extended_levels`]). Empty for a plane that spans none.
#[must_use]
pub fn extended_levels(surface: &FaceSurface, plane: &solidcore_record::Plane, points: &[Vec3]) -> Vec<f64> {
    use solidcore_ops::extended_levels::{Carried, extended_levels};
    let arr = |v: Vec3| [v.x, v.y, v.z];
    let unit = |u: UnitVec3| arr(u.vec());
    let carried = match *surface {
        FaceSurface::Plane { normal, point } => Carried::Plane { point: arr(point), normal: unit(normal) },
        FaceSurface::Cylinder { origin, axis, radius } => Carried::Cylinder { origin: arr(origin), axis: unit(axis), radius },
        FaceSurface::Cone { apex, axis, half_angle } => Carried::Cone { apex: arr(apex), axis: unit(axis), half_angle },
        FaceSurface::Sphere { center, radius } => Carried::Sphere { center: arr(center), radius },
        FaceSurface::Torus { center, major, minor, .. } => Carried::Torus { center: arr(center), major, minor },
    };
    let points: Vec<[f64; 3]> = points.iter().map(|p| arr(*p)).collect();
    extended_levels(&carried, arr(plane.origin), arr(plane.x), arr(plane.y), &points).unwrap_or_default()
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

    /// #11876: A constant-radius fillet (held as a rational spline from SMLib) is recognized as a
    /// torus by face_surface so Quick Measure and Measure report its radius and diameter.
    #[test]
    fn a_fillet_face_is_recognized_as_a_torus() {
        let cyl = crate::cylinder(Vec3::ZERO, Vec3::new(0.0, 0.0, 1.0), 5.0, 10.0).unwrap();
        let rounded = crate::fillet(&cyl, &[Vec3::new(5.0, 0.0, 10.0)], 2.0).unwrap();
        let tori: Vec<_> = (0..rounded.face_count())
            .filter_map(|i| match face_surface(&rounded, i) {
                Some(FaceSurface::Torus { major, minor, .. }) => Some((major, minor)),
                _ => None,
            })
            .collect();
        assert_eq!(tori.len(), 1, "found: {tori:?}");
        assert!((tori[0].0 - 3.0).abs() < 1e-3, "major radius: {:?}", tori[0].0);
        assert!((tori[0].1 - 2.0).abs() < 1e-3, "minor radius: {:?}", tori[0].1);
    }

    #[test]
    fn same_record_surface_accepts_spline_rounding_and_rejects_shifted_spline() {
        let base = Surface::Spline {
            u_knots: vec![0.0, 0.0, 1.0, 1.0],
            v_knots: vec![0.0, 0.0, 1.0, 1.0],
            points: vec![vec![[-1200.0, 0.0, 0.0], [-1200.0, -2.0, 0.0]], vec![[-1204.0, -4.0, 3.0], [-1202.0, -4.0, 3.0]]],
        };
        let rounded = Surface::Spline {
            u_knots: vec![0.0, 0.0, 1.0, 1.0],
            v_knots: vec![0.0, 0.0, 1.0, 1.0],
            points: vec![
                vec![[-1200.0, 1.776e-15, 0.0], [-1200.0, -2.0 + 1.776e-15, 0.0]],
                vec![[-1204.0, -4.0, 3.0 - 1.776e-15], [-1202.0, -4.0, 3.0]],
            ],
        };
        let shifted = Surface::Spline {
            u_knots: vec![0.0, 0.0, 1.0, 1.0],
            v_knots: vec![0.0, 0.0, 1.0, 1.0],
            points: vec![vec![[-1200.0, 1e-3, 0.0], [-1200.0, -2.0, 0.0]], vec![[-1204.0, -4.0, 3.0], [-1202.0, -4.0, 3.0]]],
        };
        let diff_knots = Surface::Spline {
            u_knots: vec![0.0, 0.0, 2.0, 2.0],
            v_knots: vec![0.0, 0.0, 1.0, 1.0],
            points: vec![vec![[-1200.0, 0.0, 0.0], [-1200.0, -2.0, 0.0]], vec![[-1204.0, -4.0, 3.0], [-1202.0, -4.0, 3.0]]],
        };
        assert!(same_record_surface(&base, &rounded, 1e-6));
        assert!(same_record_surface(&rounded, &base, 1e-6));
        assert!(!same_record_surface(&base, &rounded, 0.0));
        assert!(!same_record_surface(&base, &shifted, 1e-6));
        assert!(!same_record_surface(&base, &diff_knots, 1e-6));
        assert!(!same_record_surface(&base, &base, -1.0));
        assert!(!same_record_surface(&base, &base, f64::NAN));

        let face = |surface: Surface| Face { surface, nurbs: None, domain: [[0.0, 1.0], [0.0, 1.0]], loops: Vec::new() };
        assert!(same_face_record_surface(&face(base.clone()), &face(rounded), 1e-6));
        assert!(!same_face_record_surface(&face(base), &face(shifted), 1e-6));

        let rat_a = Surface::RationalSpline {
            u_knots: vec![0.0, 0.0, 1.0, 1.0],
            v_knots: vec![0.0, 0.0, 1.0, 1.0],
            homogeneous: vec![vec![[0.0, 0.0, 0.0, 1.0], [2.0, 0.0, 0.0, 2.0]], vec![[0.0, 1.0, 0.0, 1.0], [2.0, 2.0, 0.0, 2.0]]],
        };
        let rat_rounded = Surface::RationalSpline {
            u_knots: vec![0.0, 0.0, 1.0, 1.0],
            v_knots: vec![0.0, 0.0, 1.0, 1.0],
            homogeneous: vec![vec![[0.0, 1e-15, 0.0, 1.0], [2.0 + 2e-15, 0.0, 0.0, 2.0]], vec![[0.0, 1.0, 0.0, 1.0], [2.0, 2.0, 0.0, 2.0]]],
        };
        let rat_diff_weight = Surface::RationalSpline {
            u_knots: vec![0.0, 0.0, 1.0, 1.0],
            v_knots: vec![0.0, 0.0, 1.0, 1.0],
            homogeneous: vec![vec![[0.0, 0.0, 0.0, 1.0], [1.0, 0.0, 0.0, 1.0]], vec![[0.0, 1.0, 0.0, 1.0], [2.0, 2.0, 0.0, 2.0]]],
        };
        assert!(same_record_surface(&rat_a, &rat_rounded, 1e-6));
        assert!(!same_record_surface(&rat_a, &rat_diff_weight, 1e-6));
    }
}
