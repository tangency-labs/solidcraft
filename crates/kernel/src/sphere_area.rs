//! The exact area of a face on a sphere bounded by circles (#22556): a sphere cut by planes.
//!
//! By Gauss–Bonnet, a piece of a sphere of radius R bounded by `n` loops has area
//! `A = R²·(2π·χ − Σ∫κ_g ds − Σ turns)`, χ = 2 − n, where κ_g is the geodesic curvature of the
//! edges and the turns are the exterior angles at the corners, all taken with the face on the
//! left of its loops. A circle on the sphere whose plane lies at distance d from the centre bends
//! by d/(R·r): along an arc of turn θ, by θ·d/R.
//!
//! The record stores a face's loops relative to its surface's natural normal (the outer loop runs
//! counter-clockwise seen from it), so the face lies on the left of its loops seen from that
//! normal. Whether that normal points out of the sphere or into it is read from the surface's
//! own derivatives: that alone says which side of the loops the face is on.

use std::f64::consts::PI;

use solidcore_record::Vec3;
use solidcore_record::brep::Loop;
use solidcraft_geom::brep::eval::{CurveEval, SurfaceEval};
use solidcraft_geom::prelude::*;

use crate::Body;
use crate::surface_kind::{FaceSurface, edge_circle, face_surface};

/// How far, relative to the radius, a point or a circle may lie off the sphere and still be on it.
const ON_SPHERE: f64 = 1e-6;

/// How much of the squared length of the two surface tangents their cross product must hold for
/// the surface's normal to be read at a point (not at a pole, where a tangent vanishes).
const NORMAL_READABLE: f64 = 1e-12;

/// One edge of a face on a sphere, run along its loop.
struct Arc {
    start: Vec3,
    end: Vec3,
    leaving: Vec3,
    arriving: Vec3,
    /// `∫κ_g ds` along the edge: positive where it bends to its left, seen from outside.
    bend: f64,
}

/// The exact area of face `face` of `b` when it lies on a sphere and every edge bounding it is a
/// circle or an arc of one (what the plane of a cut leaves). A seam the face runs both ways along,
/// an edge shrunk to a point and a vertex loop (a pole) bound nothing and are left out. `None`
/// for any other face, a mesh body or an index out of range.
pub fn sphere_face_area(b: &Body, face: usize) -> Option<f64> {
    let Some(FaceSurface::Sphere { center, radius }) = face_surface(b, face) else { return None };
    if !(radius.is_finite() && radius > 0.0) {
        return None;
    }
    let tol = radius * ON_SPHERE;
    let rec = b.record();
    let f = rec.faces.get(face)?;
    let mut used: Vec<(usize, bool)> = Vec::new();
    for l in &f.loops {
        match l {
            Loop::Edges { edges } => used.extend(edges.iter().map(|u| (u.edge as usize, u.forward))),
            Loop::Vertex { .. } => {}
        }
    }
    let mut arcs: Vec<Arc> = Vec::new();
    for &(edge, forward) in &used {
        if used.iter().filter(|(e, _)| *e == edge).count() > 1 {
            continue;
        }
        let Some(c) = edge_circle(b, edge) else {
            // An edge shrunk to a point: a pole of the sphere's parameters.
            let e = rec.edges.get(edge)?;
            let curve = CurveEval::new(e.parameters()).ok()?;
            let [t0, t1] = e.interval;
            let (p, m, q) = (curve.point(t0)?, curve.point(0.5 * (t0 + t1))?, curve.point(t1)?);
            if p.dist(m) <= tol && p.dist(q) <= tol {
                continue;
            }
            return None;
        };
        if c.radius <= tol {
            continue;
        }
        // The circle lies on the sphere.
        let d = (c.center - center).dot(c.axis.vec());
        let off = c.center - center - c.axis.vec() * d;
        if off.len() > tol || (d * d + c.radius * c.radius - radius * radius).abs() > 2.0 * radius * tol {
            return None;
        }
        // The arc runs counter-clockwise about its axis from its start to its end.
        let along = |p: Vec3| c.axis.vec().cross(p - c.center).normalized();
        let (start, end, sign) = if forward { (c.start, c.end, 1.0) } else { (c.end, c.start, -1.0) };
        let (leaving, arriving) = (along(start)? * sign, along(end)? * sign);
        arcs.push(Arc { start, end, leaving, arriving, bend: sign * c.angle * d / radius });
    }
    // Each arc's next: the one arc that leaves from where it ends. The loops are the cycles of
    // that order, and the corners are where one arc meets its next.
    let mut next: Vec<usize> = Vec::with_capacity(arcs.len());
    for a in &arcs {
        let mut after = arcs.iter().enumerate().filter(|(_, n)| n.start.dist(a.end) <= tol).map(|(i, _)| i);
        let (Some(n), None) = (after.next(), after.next()) else { return None };
        next.push(n);
    }
    let mut taken = vec![false; arcs.len()];
    for &n in &next {
        if std::mem::replace(taken.get_mut(n)?, true) {
            return None;
        }
    }
    let mut seen = vec![false; arcs.len()];
    let mut loops = 0;
    for first in 0..arcs.len() {
        if seen.get(first) != Some(&false) {
            continue;
        }
        loops += 1;
        let mut a = first;
        while let Some(slot) = seen.get_mut(a).filter(|s| !**s) {
            *slot = true;
            a = *next.get(a)?;
        }
    }
    let mut turned = 0.0;
    for (a, &n) in arcs.iter().zip(&next) {
        let (into, out) = (a.arriving, arcs.get(n)?.leaving);
        let outward = (a.end - center).normalized()?;
        turned += a.bend + into.cross(out).dot(outward).atan2(into.dot(out));
    }
    let r2 = radius * radius;
    let chi = 2.0 - f64::from(loops);
    // Seen from outside, the area left of the loops and the area right of them.
    let left = r2 * (2.0 * PI * chi - turned);
    let right = r2 * (2.0 * PI * chi + turned);
    let area = if natural_normal_is_outward(b, face, center)? { left } else { right };
    (area.is_finite() && area > 0.0 && area <= 4.0 * PI * r2 * (1.0 + ON_SPHERE)).then_some(area)
}

/// Whether the natural normal (∂u × ∂v) of face `face`'s parameterised surface points out of the
/// sphere about `center`, read where the face's parameter rectangle is not degenerate.
fn natural_normal_is_outward(b: &Body, face: usize, center: Vec3) -> Option<bool> {
    let f = b.record().faces.get(face)?;
    let eval = SurfaceEval::new(f.parameters()).ok()?;
    let [[u0, u1], [v0, v1]] = f.domain;
    if !(u0.is_finite() && u1.is_finite() && v0.is_finite() && v1.is_finite()) {
        return None;
    }
    for (s, t) in [(0.5, 0.5), (0.3, 0.4), (0.7, 0.6), (0.25, 0.75)] {
        let Some((p, du, dv)) = eval.derivs(u0 + (u1 - u0) * s, v0 + (v1 - v0) * t) else { continue };
        let n = du.cross(dv);
        if n.len2() <= du.len2() * dv.len2() * NORMAL_READABLE {
            continue;
        }
        let out = n.dot(p - center);
        if out != 0.0 {
            return Some(out > 0.0);
        }
    }
    None
}
