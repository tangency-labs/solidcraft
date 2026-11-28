//! A solid cut through along several axes, such as a block bored on two: the intersection of its
//! straight extrusions, one along each axis whose two end caps match.
//!
//! The extrusion of a solid's end caps holds the solid when every point of it projects into the
//! caps' section, and the intersection of such extrusions is the solid's visual hull from those
//! directions (A. Laurentini, "The visual hull concept for silhouette-based image understanding",
//! IEEE PAMI 16(2), 1994). The hull is the solid when it holds no more material. So a solid is
//! taken only when every source vertex lies inside every extrusion, every source triangle lies
//! whole on a cap or within the tolerance of a wall of one of them (the chord certificate of
//! [`super::curves`]), and its volume is the source's within the tolerance times the source's
//! area. The solid built is the first extrusion less the holes of the others' sections, which
//! holds their intersection.
//!
//! Deviations: no upstream counterpart. No recursion.

use crate::kernel::Kernel;
use crate::math::Vec3;
use crate::prism::profile::Region2;
use crate::{Brep, Result};

use super::curves::certified;
use super::geometry::{distance_to_segment, MeshPlane};
use super::{invalid, native, parts, prism, Prism, Sides, TriMesh};

/// Point and triangle tests against section segments, the same work bound as the wall check of
/// [`super::extrusion`].
const MAX_WORK: usize = 32_000_000;
/// Faces of the intersection, the shoulder decomposition's bound.
const MAX_FACES: usize = 256;

/// Intersect the extrusions of every axis with matching caps, at least two, when the source is
/// the boundary of that intersection and has its volume; `None` when a certificate fails.
pub(super) fn reconstruct(k: &mut Kernel, mesh: &TriMesh, tolerance: f64) -> Result<Option<Brep>> {
    let size = mesh.size();
    let epsilon = (size.max(1.) * 1e-7).min(tolerance * 0.01).max(1e-9);
    let mut prisms: Vec<Prism> = Vec::new();
    for (axis, _) in parts::axes(mesh) {
        // A normal within the source's rounding of an axis already taken is that axis.
        if prisms
            .iter()
            .any(|p| size * p.plane.normal().cross(axis).length() <= epsilon * 3.)
        {
            continue;
        }
        if let Some(found) = prism(k, mesh, axis, tolerance, Sides::Any)? {
            prisms.push(found);
        }
    }
    if prisms.len() < 2 {
        return Ok(None);
    }
    let segments: usize = prisms
        .iter()
        .flat_map(|p| &p.loops)
        .map(|lp| lp.segs.len())
        .sum();
    let samples = mesh.positions.len() + mesh.triangles.len();
    if samples.saturating_mul(segments) > MAX_WORK {
        return Ok(None);
    }
    for &p in &mesh.positions {
        for prism in &prisms {
            if !inside(prism, p, tolerance) {
                return Ok(None);
            }
        }
    }
    let mut area = 0.;
    for &[a, b, c] in &mesh.triangles {
        let (Some(&a), Some(&b), Some(&c)) = (
            mesh.positions.get(a as usize),
            mesh.positions.get(b as usize),
            mesh.positions.get(c as usize),
        ) else {
            return Err(invalid("missing triangle vertex"));
        };
        area += (b - a).cross(c - a).length() * 0.5;
        if !prisms
            .iter()
            .any(|prism| bounds(prism, [a, b, c], tolerance, epsilon))
        {
            return Ok(None);
        }
    }
    // The first extrusion less each hole of the others' sections, swept past the solid at both
    // ends: no face of a tool lies on a face of the first, as the faces of two extrusions of one
    // block would. It holds their intersection, and is that when the volumes agree below.
    let mut prisms = prisms.into_iter();
    let Some(first) = prisms.next() else {
        return Ok(None);
    };
    let mut body = first.body;
    for prism in prisms {
        let normal = prism.plane.normal();
        let Some(start) = MeshPlane::from_normal(normal * (prism.lo - size), normal) else {
            return Ok(None);
        };
        for hole in prism.regions.iter().flat_map(|region| &region.holes) {
            let tool = Region2 {
                outer: hole.reversed(),
                holes: Vec::new(),
            };
            let length = prism.hi - prism.lo + 2. * size;
            let Some(tool) = native::extrude(k, &start.frame(), &[tool], length)? else {
                return Ok(None);
            };
            let Some(rest) = native::difference(k, body, tool)? else {
                return Ok(None);
            };
            body = rest;
        }
    }
    if body.face_list().len() > MAX_FACES || !native::is_valid(k, &mut body)? {
        return Ok(None);
    }
    let original = mesh.volume();
    let Some(volume) = native::volume(k, &mut body)? else {
        return Ok(None);
    };
    if !original.is_finite()
        || original <= 0.
        || (volume - original).abs() > area * tolerance + epsilon * size.powi(2)
    {
        return Ok(None);
    }
    Ok(Some(body))
}

/// Whether the point is inside the extrusion or within `tolerance` of its boundary.
fn inside(prism: &Prism, p: Vec3, tolerance: f64) -> bool {
    let h = p.dot(prism.plane.normal());
    if !(prism.lo - tolerance..=prism.hi + tolerance).contains(&h) {
        return false;
    }
    let q = prism.plane.to_local(p);
    let mut held = false;
    for lp in &prism.loops {
        if lp
            .segs
            .iter()
            .any(|s| distance_to_segment(s, q) <= tolerance)
        {
            return true;
        }
        match lp.holds(q) {
            Some(holds) => held ^= holds,
            None => return false,
        }
    }
    held
}

/// Whether the triangle is one of the extrusion's cap triangles (within `epsilon` of a cap's
/// plane, as [`prism`] reads them) or lies whole within `tolerance` of one of its walls.
fn bounds(prism: &Prism, corners: [Vec3; 3], tolerance: f64, epsilon: f64) -> bool {
    let normal = prism.plane.normal();
    let heights = corners.map(|p| p.dot(normal));
    if [prism.lo, prism.hi]
        .iter()
        .any(|level| heights.iter().all(|h| (h - level).abs() <= epsilon))
    {
        return true;
    }
    if heights
        .iter()
        .any(|h| *h < prism.lo - tolerance || *h > prism.hi + tolerance)
    {
        return false;
    }
    let projected = corners.map(|p| prism.plane.to_local(p));
    prism
        .loops
        .iter()
        .any(|lp| certified(&projected, &lp.segs, tolerance))
}
