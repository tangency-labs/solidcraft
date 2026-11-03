//! What a face measures on the surface behind it (#9086).
//!
//! A curved face's area is the area of the surface it lies on: a cylinder's band is
//! r·θ·h and a whole sphere 4πr². The display mesh's triangles run inside a curve, so
//! their sum falls short of it.

use std::f64::consts::PI;

use solidcraft_geom::{Mesh, UnitVec3, Vec3};
use solidcraft_kernel::{Body, FaceSurface};

/// The area of the patch of faces `patch` of `body`, worked out on the surface behind
/// them: a cylinder's band, or a sphere. `None` where the surface does not give one, and
/// the caller keeps the measure of the mesh.
pub(crate) fn patch_area(body: &Body, mesh: &Mesh, patch: &[usize]) -> Option<f64> {
    match solidcraft_kernel::face_surface(body, *patch.first()?)? {
        FaceSurface::Cylinder { origin, axis, radius } => cylinder_band(body, mesh, patch, origin, axis, radius),
        FaceSurface::Sphere { radius, .. } => sphere(body, patch, radius),
        _ => None,
    }
}

/// The edges on the border of the patch: the ones a face of the patch shares with a face
/// outside it, and the ones it keeps to itself.
fn border_edges(mesh: &Mesh, patch: &[usize]) -> Vec<usize> {
    mesh.edge_faces
        .iter()
        .enumerate()
        .filter(|(_, faces)| {
            let inside = faces.iter().filter(|f| patch.contains(&(**f as usize))).count();
            inside > 0 && (faces.len() == 1 || inside < faces.len())
        })
        .map(|(edge, _)| edge)
        .collect()
}

/// The area of a cylindrical band: the radius of the surface, the turn the band covers
/// and the distance between its two rims.
fn cylinder_band(body: &Body, mesh: &Mesh, patch: &[usize], origin: Vec3, axis: UnitVec3, radius: f64) -> Option<f64> {
    if !(radius.is_finite() && radius > 0.0) {
        return None;
    }
    let tol = radius * 1e-6;
    // The whole patch lies on one cylinder.
    for f in patch {
        match solidcraft_kernel::face_surface(body, *f)? {
            FaceSurface::Cylinder { origin: o, axis: a, radius: r } => {
                if (r - radius).abs() > tol || o.dist(origin) > tol || a.vec().dot(axis.vec()).abs() < 1.0 - 1e-9 {
                    return None;
                }
            }
            _ => return None,
        }
    }
    // The rims of the band: the circles that bound it, each across the axis.
    let mut rims: Vec<(f64, f64)> = Vec::new();
    for edge in border_edges(mesh, patch) {
        // An edge that is not a circle (a straight cut along the band) leaves the turn of
        // the band to its rims alone, which is what they say.
        let Some(circle) = solidcraft_kernel::edge_circle(body, edge) else { continue };
        if circle.axis.vec().dot(axis.vec()).abs() < 1.0 - 1e-9 || (circle.radius - radius).abs() > tol {
            return None;
        }
        let d = circle.center - origin;
        let height = d.dot(axis.vec());
        if (d - axis.vec() * height).len() > tol {
            return None;
        }
        rims.push((height, circle.angle));
    }
    // The two rims: the band runs between them, and both cover the same turn.
    let mut levels: Vec<(f64, f64)> = Vec::new();
    for (height, angle) in rims {
        match levels.iter_mut().find(|(h, _)| (*h - height).abs() <= tol) {
            Some((_, a)) => *a += angle,
            None => levels.push((height, angle)),
        }
    }
    if levels.len() != 2 {
        return None;
    }
    levels.sort_by(|a, b| a.0.total_cmp(&b.0));
    let (low, low_turn) = levels[0];
    let (high, high_turn) = levels[1];
    let height = high - low;
    if !(height > 0.0) || (low_turn - high_turn).abs() > low_turn.max(high_turn) * 1e-6 {
        return None;
    }
    Some(radius * low_turn * height)
}

/// The area of a sphere: 4πr², when the body is the whole of it.
fn sphere(body: &Body, patch: &[usize], radius: f64) -> Option<f64> {
    if !(radius.is_finite() && radius > 0.0) {
        return None;
    }
    // The whole patch lies on one sphere.
    for f in patch {
        match solidcraft_kernel::face_surface(body, *f)? {
            FaceSurface::Sphere { radius: r, .. } if (r - radius).abs() <= radius * 1e-6 => {}
            _ => return None,
        }
    }
    // The body is the whole sphere: every face of it lies on that sphere and the patch
    // holds them all. A body with a face of its own (a sphere that was cut) is a piece of
    // the sphere, whose area this does not work out: the measure of the mesh stands in
    // for it.
    for f in 0..body.record().faces.len() {
        match solidcraft_kernel::face_surface(body, f)? {
            FaceSurface::Sphere { radius: r, .. } if (r - radius).abs() <= radius * 1e-6 => {}
            _ => return None,
        }
        if !patch.contains(&f) {
            return None;
        }
    }
    Some(4.0 * PI * radius * radius)
}
