//! Boolean input checks and the checks every retry's result must pass (issue #616).
//!
//! A retry moves things about (the tool nudged, faces pushed apart, pieces cut off and glued
//! back), so its result is held to what a join or a cut can do at all:
//! - it is closed;
//! - a join never loses volume and a cut never gains any;
//! - outside the tool's box nothing changes: there, the result's surface is the target's and
//!   the target's surface is the result's. A join or a cut can only change the target where the
//!   tool is, so geometry anywhere else (a slit through a wall, a face moved) is wrong.
//!
//! A result that fails is refused with an error: a wrong result that looks right is worse.

use solidcraft_geom::{Aabb3, Mesh, Vec3};

use crate::body::Body;
use crate::{BoolOp, KernelError, Result};

/// Maximum boundary samples per input check, or triangle samples per result surface.
const MAX_SAMPLES: usize = 20_000;

/// A boundary off its planar support cannot be intersected consistently: tracing on the
/// plane and splitting the edge produce different endpoints. Check only the meeting region,
/// before numerical retries repeatedly try to close those gaps.
pub(crate) fn check_planar_boundaries(a: &Body, b: &Body, ma: &Mesh, mb: &Mesh) -> Result<()> {
    use truck_meshalgo::prelude::TrySubs;
    use truck_modeling::{BoundedCurve, Surface};

    crate::guard("boolean input boundaries", || {
        // The finest operand-mesh resolution used by the boolean. Smaller fitting errors
        // remain eligible for the existing intersection and repair strategies.
        let tolerance = (a.size().max(b.size()) * 1e-5).max(1e-5);
        let mut left = MAX_SAMPLES;
        for (body, other, role) in [(a, mb, "target"), (b, ma, "tool")] {
            let bounds = other.bounds().inflated(tolerance);
            let mut proximity = None;
            for (face_index, face) in body.solid.face_iter().enumerate() {
                let Surface::Plane(plane) = face.surface() else { continue };
                let n = plane.normal();
                let Some(n) = Vec3::new(n.x, n.y, n.z).normalized() else { continue };
                let origin = crate::body::from_p3(plane.origin());
                for edge in face.boundaries().iter().flat_map(|wire| wire.edge_iter()) {
                    let curve = edge.curve();
                    let (lo, hi) = curve.range_tuple();
                    if !lo.is_finite() || !hi.is_finite() {
                        continue;
                    }
                    for t in [lo, lo * 0.5 + hi * 0.5, hi] {
                        if left == 0 {
                            return Ok(());
                        }
                        left -= 1;
                        let Some(p) = curve.try_subs(t).map(crate::body::from_p3) else { continue };
                        let offset = (p - origin).dot(n);
                        let gap = offset.abs();
                        // The support can meet the tool while its boundary lies just outside.
                        let projected = p - n * offset;
                        if gap.is_finite() && gap > tolerance && (bounds.contains(p) || bounds.contains(projected)) {
                            // A bounding box alone includes holes and spaces between lumps.
                            // Prepare these indices only for an actual off-plane sample.
                            let (inside, surface) = proximity.get_or_insert_with(|| (other.inside_index(), Near::new(other, tolerance)));
                            let meets = |q| inside.contains(q) || surface.as_ref().is_some_and(|s| s.close(q));
                            if !meets(p) && !meets(projected) {
                                continue;
                            }
                            return Err(KernelError::Invalid(format!(
                                "boolean: the {role}'s face {face_index} has a boundary {gap:.6} mm off its plane \
                                 (tolerance {tolerance:.6} mm) where the bodies meet; repair or reimport the body \
                                 with matching edge and surface geometry, or create the extrusion as a new body"
                            )));
                        }
                    }
                }
            }
        }
        Ok(())
    })
}

/// Distance from `p` to the triangle `abc`.
pub(crate) fn point_triangle_dist(p: Vec3, a: Vec3, b: Vec3, c: Vec3) -> f64 {
    let n = (b - a).cross(c - a);
    let Some(nn) = n.normalized() else {
        return [(a, b), (b, c), (c, a)].iter().map(|(u, v)| p.dist_to_segment(*u, *v)).fold(f64::INFINITY, f64::min);
    };
    let h = (p - a).dot(nn);
    let q = p - nn * h;
    let inside = [(a, b), (b, c), (c, a)].iter().all(|(u, v)| (*v - *u).cross(q - *u).dot(n) >= 0.0);
    if inside { h.abs() } else { [(a, b), (b, c), (c, a)].iter().map(|(u, v)| p.dist_to_segment(*u, *v)).fold(f64::INFINITY, f64::min) }
}

/// The triangles of a mesh in a uniform grid of cells, each listing the triangles within `reach`
/// of it, to answer "is this point within `reach` of the surface?".
struct Near<'a> {
    mesh: &'a Mesh,
    min: Vec3,
    cell: f64,
    dims: [usize; 3],
    cells: Vec<Vec<u32>>,
    reach: f64,
}

impl<'a> Near<'a> {
    fn new(mesh: &'a Mesh, reach: f64) -> Option<Self> {
        let bounds = mesh.bounds();
        if bounds.is_empty() {
            return None;
        }
        let min = bounds.min - Vec3::new(reach, reach, reach);
        let size = bounds.size() + Vec3::new(2.0 * reach, 2.0 * reach, 2.0 * reach);
        // About a few triangles a cell, at most 64^3 cells.
        let n = (mesh.triangles.len().max(1) as f64).cbrt().clamp(1.0, 64.0);
        let cell = (size.x.max(size.y).max(size.z) / n).max(reach * 2.0).max(1e-9);
        let dims = [size.x, size.y, size.z].map(|s| ((s / cell).ceil() as usize).clamp(1, 256));
        let mut cells = vec![Vec::new(); dims[0] * dims[1] * dims[2]];
        for (i, t) in mesh.triangles.iter().enumerate() {
            let Some(tri) = mesh.tri(t) else { continue };
            let mut bx = Aabb3::EMPTY;
            for p in tri {
                bx.add(p);
            }
            let lo = Self::index(min, cell, dims, bx.min - Vec3::new(reach, reach, reach));
            let hi = Self::index(min, cell, dims, bx.max + Vec3::new(reach, reach, reach));
            for x in lo[0]..=hi[0] {
                for y in lo[1]..=hi[1] {
                    for z in lo[2]..=hi[2] {
                        if let Some(c) = cells.get_mut((x * dims[1] + y) * dims[2] + z) {
                            c.push(i as u32);
                        }
                    }
                }
            }
        }
        Some(Near { mesh, min, cell, dims, cells, reach })
    }

    fn index(min: Vec3, cell: f64, dims: [usize; 3], p: Vec3) -> [usize; 3] {
        let d = p - min;
        let f = |v: f64, n: usize| ((v / cell).floor().max(0.0) as usize).min(n - 1);
        [f(d.x, dims[0]), f(d.y, dims[1]), f(d.z, dims[2])]
    }

    fn close(&self, p: Vec3) -> bool {
        let d = p - self.min;
        let s = [d.x, d.y, d.z];
        if s.iter().zip(self.dims).any(|(v, n)| *v < 0.0 || *v > self.cell * n as f64) {
            return false;
        }
        let [x, y, z] = Self::index(self.min, self.cell, self.dims, p);
        let Some(list) = self.cells.get((x * self.dims[1] + y) * self.dims[2] + z) else { return false };
        list.iter().any(|&t| {
            self.mesh.triangles.get(t as usize).and_then(|t| self.mesh.tri(t)).is_some_and(|[a, b, c]| point_triangle_dist(p, a, b, c) <= self.reach)
        })
    }
}

/// A point of `from`'s surface outside `region` that is not on `onto`'s surface (within
/// `reach`), if there is one.
fn stray_point(from: &Mesh, onto: &Near, region: &Aabb3) -> Option<Vec3> {
    let step = from.triangles.len().div_ceil(MAX_SAMPLES).max(1);
    from.triangles.iter().step_by(step).filter_map(|t| from.tri(t)).find_map(|[a, b, c]| {
        let p = (a + b + c) / 3.0;
        let outside =
            p.x < region.min.x || p.y < region.min.y || p.z < region.min.z || p.x > region.max.x || p.y > region.max.y || p.z > region.max.z;
        (outside && !onto.close(p)).then_some(p)
    })
}

/// The smallest radius of a body's cylinders, spheres and tori (their tubes).
fn smallest_radius(body: &Body) -> Option<f64> {
    use crate::surface_kind::{FaceSurface, face_surface};
    (0..body.face_count())
        .filter_map(|i| match face_surface(body, i)? {
            FaceSurface::Cylinder { radius, .. } | FaceSurface::Sphere { radius, .. } => Some(radius),
            FaceSurface::Torus { minor, .. } => Some(minor),
            _ => None,
        })
        .filter(|r| r.is_finite() && *r > 0.0)
        .min_by(f64::total_cmp)
}

fn bad(op: BoolOp, what: String) -> KernelError {
    let verb = match op {
        BoolOp::Union => "join",
        BoolOp::Cut => "cut",
        BoolOp::Intersect => "intersect",
    };
    KernelError::Failed(format!(
        "the {verb} could not be computed reliably: {what}. The result was refused rather than kept wrong; \
         where the bodies only touch (faces flush with each other), overlapping them slightly usually helps"
    ))
}

/// Check the result of a Boolean that needed a retry (see the module docs). `result` is `None`
/// for an empty result.
pub(crate) fn check_retry(a: &Body, b: &Body, op: BoolOp, result: Option<&Body>) -> Result<()> {
    if op == BoolOp::Intersect || a.is_mesh() || b.is_mesh() {
        return Ok(());
    }
    let Some(r) = result else {
        // Nothing left: only a cut whose tool covers the whole target (to within the meshes'
        // chords: a tool touching a round side of the target).
        let chord = a.size().max(b.size()) * 1e-3;
        let (Ok(ma), Ok(mb)) = (a.tessellate(chord), b.tessellate(chord)) else { return Ok(()) };
        let (ba, bb) = (ma.bounds(), mb.bounds());
        let tol = 2.0 * chord;
        let within = ba.min.x >= bb.min.x - tol
            && ba.min.y >= bb.min.y - tol
            && ba.min.z >= bb.min.z - tol
            && ba.max.x <= bb.max.x + tol
            && ba.max.y <= bb.max.y + tol
            && ba.max.z <= bb.max.z + tol;
        return if op == BoolOp::Cut && within { Ok(()) } else { Err(bad(op, "it came out empty".into())) };
    };
    if r.solid.boundaries().iter().any(|s| s.shell_condition() != truck_modeling::ShellCondition::Closed) {
        return Err(bad(op, "the result is not closed".into()));
    }
    let size = a.size().max(b.size());
    let t = (size * 2e-4).max(1e-5);
    let (Ok(ma), Ok(mb), Ok(mr)) = (a.tessellate_with(t, true), b.tessellate_with(t, true), r.tessellate_with(t, true)) else {
        return Err(bad(op, "the result could not be checked".into()));
    };
    // A mesh as coarse as `t` on a round smaller than it has no inside to speak of (a through
    // hole 0.13 mm across in a 900 mm slab measured −30272 mm³, #1255): the volumes are measured
    // at a tenth of the bodies' smallest radius, where that is finer. All three at the same
    // tolerance, so the chords' error on the faces they share cancels. `scale` refines it all.
    let finest = [a, b, r].iter().filter_map(|x| smallest_radius(x)).fold(f64::INFINITY, f64::min);
    let fine = if finest * 0.1 < t { (finest * 0.1).max(t * 1e-2) } else { t };
    let volume = |body: &Body, mesh: &Mesh, scale: f64| -> Result<f64> {
        if fine == t && scale == 1.0 {
            return Ok(mesh.measure().volume);
        }
        body.tessellate_with(fine * scale, true).map(|m| m.measure().volume).map_err(|_| bad(op, "the result could not be checked".into()))
    };
    let judge = |scale: f64| -> Result<(bool, f64, f64, f64)> {
        let (va, vb, vr) = (volume(a, &ma, scale)?, volume(b, &mb, scale)?, volume(r, &mr, scale)?);
        let slack = 2e-4 * (va.abs() + vb.abs()) + 1e-9;
        let ok = vr.is_finite()
            && match op {
                BoolOp::Union => vr >= va.max(vb) - slack && vr <= va + vb + slack,
                BoolOp::Cut => vr <= va + slack && vr >= va - vb - slack,
                BoolOp::Intersect => true,
            };
        Ok((ok, va, vb, vr))
    };
    // A volume out of bounds is measured again ten times finer before it refuses the result:
    // the coarse meshes of a part with many rounds can be off by more than the slack (a 0.8 mm
    // hole through a plastic part's wall measured 6.8 mm³ more than the part, #1255).
    let (mut volume_ok, mut va, mut vb, mut vr) = judge(1.0)?;
    if !volume_ok {
        (volume_ok, va, vb, vr) = judge(0.1)?;
    }
    if !volume_ok {
        let what = match op {
            BoolOp::Union => format!("the joined body's volume would be {vr:.4} mm³ (the target is {va:.4} mm³, the tool {vb:.4} mm³)"),
            _ => format!("the cut body's volume would be {vr:.4} mm³ (the target is {va:.4} mm³, the tool {vb:.4} mm³)"),
        };
        return Err(bad(op, what));
    }
    // Outside the tool's box (with room for the meshes' chords and a nudge) the result's
    // surface and the target's must be the same.
    let reach = 3.0 * t + size * 1e-5;
    let mut region = mb.bounds();
    region.min -= Vec3::new(reach, reach, reach) * 2.0;
    region.max += Vec3::new(reach, reach, reach) * 2.0;
    let (Some(near_a), Some(near_r)) = (Near::new(&ma, reach), Near::new(&mr, reach)) else {
        return Err(bad(op, "the result could not be checked".into()));
    };
    let at = |p: Vec3| format!("({:.3}, {:.3}, {:.3})", p.x, p.y, p.z);
    if let Some(p) = stray_point(&mr, &near_a, &region) {
        return Err(bad(op, format!("it would change the body away from the tool, at {} (outside the tool's region)", at(p))));
    }
    if let Some(p) = stray_point(&ma, &near_r, &region) {
        return Err(bad(op, format!("it would remove part of the body away from the tool, at {} (outside the tool's region)", at(p))));
    }
    Ok(())
}
