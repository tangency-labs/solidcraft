//! Original bounded smooth dielectric optics; no renderer source adapted.
//! Air is 1.0. Distances and absorption coefficients use millimetres.
use super::{Bounds, Result, TraceError, Triangle, unit};
use crate::{ProductScene, job::DielectricOptions};
use solidcraft_geom::Vec3;

pub const MAX_OVERRIDES: usize = 8;
pub const MAX_BOUNDARIES: u8 = 8;
pub const MAX_PATHS: u16 = 511;

#[derive(Clone, Copy, Debug)]
pub struct Interface {
    pub reflected: Vec3,
    pub transmitted: Option<Vec3>,
    pub reflectance: f64,
}

/// Exact unpolarized dielectric Fresnel and Snell, with the normal towards
/// the incident medium. Equal indices have no interface, including grazing.
pub fn interface(direction: Vec3, normal: Vec3, incident: f64, transmitted: f64) -> Result<Interface> {
    polarized_interface(direction, normal, incident, transmitted).map(|(optics, _)| optics)
}

fn polarized_interface(direction: Vec3, normal: Vec3, incident: f64, transmitted: f64) -> Result<(Interface, [f64; 2])> {
    if ![incident, transmitted].into_iter().all(|n| n.is_finite() && (1.0..=3.0).contains(&n)) {
        return Err(TraceError::InvalidInput("dielectric IOR outside 1..3"));
    }
    let d = unit(direction).ok_or(TraceError::InvalidInput("dielectric direction"))?;
    let n = unit(normal).ok_or(TraceError::InvalidInput("dielectric normal"))?;
    let cosine = -d.dot(n);
    if cosine < 0.0 {
        return Err(TraceError::InvalidInput("dielectric incident hemisphere"));
    }
    let cosine = cosine.clamp(0.0, 1.0);
    let reflected = unit(d + n * (2.0 * cosine)).ok_or(TraceError::InvalidInput("reflected direction"))?;
    if incident == transmitted {
        return Ok((Interface { reflected, transmitted: Some(d), reflectance: 0.0 }, [0.0; 2]));
    }
    let ratio = incident / transmitted;
    let sine2 = ratio * ratio * (1.0 - cosine * cosine).max(0.0);
    if sine2 >= 1.0 {
        return Ok((Interface { reflected, transmitted: None, reflectance: 1.0 }, [1.0; 2]));
    }
    let cos_t = (1.0 - sine2).sqrt();
    let rs = (incident * cosine - transmitted * cos_t) / (incident * cosine + transmitted * cos_t);
    let rp = (transmitted * cosine - incident * cos_t) / (transmitted * cosine + incident * cos_t);
    let refracted = unit(d * ratio + n * (ratio * cosine - cos_t)).ok_or(TraceError::InvalidInput("refracted direction"))?;
    let polarized = [(rs * rs).clamp(0.0, 1.0), (rp * rp).clamp(0.0, 1.0)];
    Ok((Interface { reflected, transmitted: Some(refracted), reflectance: (polarized[0] + polarized[1]) * 0.5 }, polarized))
}

/// Incoherent parallel-sheet returns for unpolarized incident light. Sum each
/// polarization's internal returns before averaging; virtual thickness affects
/// absorption only. This closure does not transport Stokes state between hits.
pub(super) fn thin_sheet(direction: Vec3, normal: Vec3, material: &DielectricOptions) -> Result<(Interface, Vec3, Vec3)> {
    let direction = unit(direction).ok_or(TraceError::InvalidInput("dielectric direction"))?;
    let normal = unit(normal).ok_or(TraceError::InvalidInput("dielectric normal"))?;
    let (optics, polarized) = polarized_interface(direction, normal, 1.0, material.ior)?;
    let inside = optics.transmitted.ok_or(TraceError::InvalidInput("air to sheet transmission"))?;
    let cos_t = -inside.dot(normal);
    // Zero virtual thickness has zero optical distance, including the index-matched
    // grazing limit. A positive thickness with no crossing direction is undefined.
    let distance = if material.thickness_mm == 0.0 {
        0.0
    } else if cos_t > 0.0 {
        material.thickness_mm / cos_t
    } else {
        return Err(TraceError::InvalidInput("grazing sheet thickness is unsupported"));
    };
    let a = attenuation(material.absorption_per_mm, distance)?;
    let weights = |a: f64| {
        let series = |f: f64| {
            let denominator = 1.0 - f * f * a * a;
            if denominator == 0.0 { (1.0, 0.0) } else { (f + (1.0 - f).powi(2) * f * a * a / denominator, (1.0 - f).powi(2) * a / denominator) }
        };
        let s = series(polarized[0]);
        let p = series(polarized[1]);
        ((s.0 + p.0) * 0.5, (s.1 + p.1) * 0.5)
    };
    let (rr, tr) = weights(a.x);
    let (rg, tg) = weights(a.y);
    let (rb, tb) = weights(a.z);
    Ok((Interface { transmitted: Some(direction), ..optics }, Vec3::new(rr, rg, rb), Vec3::new(tr, tg, tb)))
}

pub fn attenuation(sigma: [f64; 3], distance_mm: f64) -> Result<Vec3> {
    if !distance_mm.is_finite()
        || !(0.0..=super::MAX_DISTANCE_MM).contains(&distance_mm)
        || !sigma.into_iter().all(|v| v.is_finite() && (0.0..=100.0).contains(&v))
    {
        return Err(TraceError::InvalidInput("dielectric absorption or distance"));
    }
    let [r, g, b] = sigma.map(|s| (-s * distance_mm).exp());
    Ok(Vec3::new(r, g, b))
}

pub fn multiply(a: Vec3, b: Vec3) -> Vec3 {
    Vec3::new(a.x * b.x, a.y * b.y, a.z * b.z)
}

#[derive(Clone, Copy, Debug)]
pub enum Certificate {
    ThinSheet,
    ClosedBox(Bounds),
}

/// Most triangles a closed box's boundary may come in (its mesh is read in full, sorting its
/// edges).
pub const MAX_BOX_TRIANGLES: usize = 1 << 16;

/// A strict geometric subset, never a trusted mesh.closed flag. A thin sheet is one or two
/// coplanar triangles; a closed box is a mesh that is exactly the boundary of an axis-aligned
/// box, in however many triangles ([`certify_box`]).
pub fn certify(scene: &ProductScene, input: &DielectricOptions) -> Result<Certificate> {
    let body = scene.meshes.get(input.body_index).ok_or(TraceError::InvalidInput("dielectric body ordinal"))?;
    if body.name != input.body_name {
        return Err(TraceError::InvalidInput("dielectric captured body identity mismatch"));
    }
    let mesh = &body.mesh;
    let closed = matches!(input.boundary, crate::job::DielectricBoundary::ClosedBox);
    let unsupported = TraceError::InvalidInput("unsupported dielectric boundary tessellation");
    if closed {
        if !(12..=MAX_BOX_TRIANGLES).contains(&mesh.triangles.len()) {
            return Err(unsupported);
        }
        return certify_box(mesh);
    }
    if !(1..=2).contains(&mesh.triangles.len()) || mesh.positions.len() > 36 {
        return Err(unsupported);
    }
    let mut tris = [None; 2];
    for (slot, indices) in tris.iter_mut().zip(&mesh.triangles) {
        let triangle = triangle(mesh, indices)?;
        *slot = Some(triangle);
    }
    let first = tris.first().copied().flatten().ok_or(TraceError::InvalidInput("empty thin sheet"))?;
    let normal = first.geometric_normal().ok_or(TraceError::InvalidInput("thin sheet normal"))?;
    let [anchor, _, _] = first.vertices();
    for triangle in tris.into_iter().flatten() {
        if triangle.geometric_normal() != Some(normal) || triangle.vertices().into_iter().any(|p| (p - anchor).dot(normal) != 0.0) {
            return Err(TraceError::InvalidInput("thin sheet must be one planar consistently wound surface"));
        }
    }
    if let Some(second) = tris.get(1).copied().flatten() {
        let first = first.vertices();
        let second = second.vertices();
        let mut shared = [Vec3::ZERO; 2];
        let mut shared_count = 0usize;
        let mut other_first = None;
        let mut other_second = None;
        for p in first {
            if second.contains(&p) {
                *shared.get_mut(shared_count).ok_or(TraceError::InvalidInput("duplicate thin sheet triangle"))? = p;
                shared_count += 1;
            } else {
                other_first = Some(p);
            }
        }
        for p in second {
            if !first.contains(&p) {
                other_second = Some(p);
            }
        }
        let [a, b] = shared;
        let c = other_first.ok_or(TraceError::InvalidInput("thin sheet shared edge"))?;
        let d = other_second.ok_or(TraceError::InvalidInput("thin sheet shared edge"))?;
        if shared_count != 2 || (b - a).cross(c - a).dot(normal) * (b - a).cross(d - a).dot(normal) >= 0.0 {
            return Err(TraceError::InvalidInput("thin sheet triangles overlap or have no unique shared edge"));
        }
    }
    Ok(Certificate::ThinSheet)
}

/// The mesh's triangle with these indices, refused where it is degenerate.
fn triangle(mesh: &solidcraft_geom::Mesh, indices: &[u32; 3]) -> Result<Triangle> {
    let mut vertices = [Vec3::ZERO; 3];
    for (v, index) in vertices.iter_mut().zip(indices) {
        *v = *mesh.positions.get(*index as usize).ok_or(TraceError::InvalidInput("dielectric vertex index"))?;
    }
    let triangle = Triangle::new(vertices)?;
    if triangle.geometric_normal().is_none() {
        return Err(TraceError::InvalidInput("degenerate dielectric boundary"));
    }
    Ok(triangle)
}

/// A closed box: `mesh` is exactly the boundary of the axis-aligned box its points span, each
/// point of that boundary covered once, by exact tests on the mesh's coordinates:
/// - every triangle lies in the plane of one of the box's faces (its three corners on it, its
///   normal exactly that axis), facing out of the box;
/// - the triangles make a closed, consistently oriented surface: each edge, between two points
///   given by their exact coordinates, is run once each way.
///
/// Such a surface is a 2-cycle on the box's boundary, a sphere, so it covers every point of the
/// boundary the same number of times k, with every triangle facing out (k ≥ 1), and its area is
/// k times the boundary's. k is 1 when the triangles' area is under 1.5 times the boundary's.
/// The count of triangles is no part of it: a face may come in any tiling.
fn certify_box(mesh: &solidcraft_geom::Mesh) -> Result<Certificate> {
    let mut lo = Vec3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY);
    let mut hi = -lo;
    let mut tris = Vec::new();
    tris.try_reserve_exact(mesh.triangles.len()).map_err(|_| TraceError::AllocationFailed)?;
    for indices in &mesh.triangles {
        let triangle = triangle(mesh, indices)?;
        for v in triangle.vertices() {
            lo = lo.min(v);
            hi = hi.max(v);
        }
        tris.push(triangle);
    }
    if lo.x >= hi.x || lo.y >= hi.y || lo.z >= hi.z {
        return Err(TraceError::InvalidInput("closed box needs positive extents"));
    }
    let bounds = Bounds::new(lo, hi)?;
    let mut edges: Vec<([u64; 3], [u64; 3])> = Vec::new();
    edges.try_reserve_exact(tris.len() * 3).map_err(|_| TraceError::AllocationFailed)?;
    let key = |p: Vec3| [p.x.to_bits(), p.y.to_bits(), p.z.to_bits()];
    let mut area = 0.0;
    for triangle in &tris {
        let vertices = triangle.vertices();
        let n = triangle.geometric_normal().ok_or(TraceError::InvalidInput("box normal"))?;
        let axis = if n.x.abs() == 1.0 {
            0
        } else if n.y.abs() == 1.0 {
            1
        } else if n.z.abs() == 1.0 {
            2
        } else {
            return Err(TraceError::InvalidInput("box face is not axis aligned"));
        };
        let positive = super::component(n, axis) > 0.0;
        let plane = super::component(if positive { hi } else { lo }, axis);
        if vertices.iter().any(|p| super::component(*p, axis) != plane) {
            return Err(TraceError::InvalidInput("box winding or face plane"));
        }
        let [a, b, c] = vertices;
        area += (b - a).cross(c - a).len() * 0.5;
        edges.extend([(key(a), key(b)), (key(b), key(c)), (key(c), key(a))]);
    }
    // Each edge run once each way: sorted, every directed edge is there once and so is its
    // reverse.
    edges.sort_unstable();
    if edges.windows(2).any(|w| w[0] == w[1]) || edges.iter().any(|(a, b)| edges.binary_search(&(*b, *a)).is_err()) {
        return Err(TraceError::InvalidInput("incomplete or ambiguous closed box face"));
    }
    let size = hi - lo;
    let boundary = 2.0 * (size.x * size.y + size.y * size.z + size.z * size.x);
    if !(area < 1.5 * boundary) {
        return Err(TraceError::InvalidInput("closed box boundary covered more than once"));
    }
    Ok(Certificate::ClosedBox(bounds))
}

pub fn contains(bounds: Bounds, point: Vec3) -> bool {
    let lo = bounds.min();
    let hi = bounds.max();
    point.x >= lo.x && point.x <= hi.x && point.y >= lo.y && point.y <= hi.y && point.z >= lo.z && point.z <= hi.z
}

pub fn overlaps(a: Bounds, b: Bounds) -> bool {
    let lo = a.min().max(b.min());
    let hi = a.max().min(b.max());
    lo.x <= hi.x && lo.y <= hi.y && lo.z <= hi.z
}

#[cfg(test)]
#[path = "dielectric_tests.rs"]
pub(super) mod tests;
