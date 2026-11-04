//! Shared-face replacement qualified by exact positive-weight support bounds.
use solidcore_record::{Plane, Vec3};
use solidcraft_geom::prelude::*;
use solidcraft_geom::tess::EvalSurface;

use crate::{Body, FaceGeometry, Result, eval_geometry::face_geometry};

mod coincident;

fn points(homogeneous: &[[f64; 4]]) -> Option<Vec<Vec3>> {
    if homogeneous.is_empty() || homogeneous.len() > 262_144 {
        return None;
    }
    homogeneous
        .iter()
        .map(|h| {
            if h.iter().any(|v| !v.is_finite()) || h[3] <= 1e-12 {
                return None;
            }
            let p = Vec3::new(h[0] / h[3], h[1] / h[3], h[2] / h[3]);
            p.is_finite().then_some(p)
        })
        .collect()
}

// Exact support-height bounds from the positive-weight NURBS convex hull
// (Piegl and Tiller, The NURBS Book, §4.4) and the full orbit of each
// revolved profile control. Untrimmed surfaces give conservative bounds.
fn maximum_height(geometry: &FaceGeometry, plane: &Plane) -> Option<f64> {
    let values = match &geometry.surface {
        EvalSurface::Plane { .. } => geometry
            .loops
            .iter()
            .flatten()
            .map(|edge| edge.control_points().and_then(points))
            .collect::<Option<Vec<_>>>()?
            .into_iter()
            .flatten()
            .map(|p| plane.height(p))
            .collect::<Vec<_>>(),
        EvalSurface::Nurbs { ctrl, .. } => points(ctrl)?.into_iter().map(|p| plane.height(p)).collect(),
        EvalSurface::Revolved { profile, origin, axis, transform, .. } => {
            let n = plane.normal();
            let [x, y, z] = *transform;
            let pullback = Vec3::new(n.dot(Vec3::new(x[0], y[0], z[0])), n.dot(Vec3::new(x[1], y[1], z[1])), n.dot(Vec3::new(x[2], y[2], z[2])));
            let parallel = pullback.dot(*axis);
            let radial = (pullback - *axis * parallel).len();
            let offset = (Vec3::new(x[3], y[3], z[3]) - plane.origin).dot(n) + origin.dot(pullback);
            points(&profile.ctrl)?
                .into_iter()
                .map(|p| {
                    let d = p - *origin;
                    let axial = d.dot(*axis);
                    offset + axial * parallel + (d - *axis * axial).len() * radial
                })
                .collect()
        }
    };
    if values.is_empty() || values.iter().any(|h| !h.is_finite()) { None } else { values.into_iter().reduce(f64::max) }
}

// Positive basis weights make each nonempty span leave the plane in its
// interior if any of its supporting controls is strictly inside.
fn strictly_inside(surface: &EvalSurface, hull: &[Vec3], planes: &[Plane], epsilon: f64) -> bool {
    let EvalSurface::Nurbs { degree, knots, count, .. } = surface else {
        return planes.iter().all(|plane| hull.iter().any(|p| plane.height(*p) < -epsilon));
    };
    let spans: [Vec<usize>; 2] =
        std::array::from_fn(|d| (degree[d]..count[d]).filter(|&i| knots[d].get(i).zip(knots[d].get(i + 1)).is_some_and(|(a, b)| b > a)).collect());
    let work = spans[0].len().saturating_mul(spans[1].len()).saturating_mul(planes.len()).saturating_mul(degree[0] + 1).saturating_mul(degree[1] + 1);
    if work > 5_000_000 || spans.iter().any(Vec::is_empty) {
        return false;
    }
    spans[1].iter().all(|&v| {
        spans[0].iter().all(|&u| {
            planes.iter().all(|plane| {
                (v - degree[1]..=v).any(|j| (u - degree[0]..=u).any(|i| hull.get(j * count[0] + i).is_some_and(|p| plane.height(*p) < -epsilon)))
            })
        })
    })
}

/// Replace a complete planar face by a matched open skin.
/// Returns `None` unless positive-weight control hulls prove that the skin lies
/// on the opposite side of a separating plane (union), or inside every half-space of a convex polyhedron (cut).
/// The sewn result must be one valid closed shell. History is over `[base, skin]`.
/// This deliberately does not qualify intersecting or unsupported surfaces.
pub fn replace_matched_face(base: &Body, face: usize, skin: &Body, cut: bool, tolerance: f64) -> Result<Option<Body>> {
    if !tolerance.is_finite()
        || !(1e-9..=0.1).contains(&tolerance)
        || base.is_surface()
        || base.is_mesh()
        || !skin.is_surface()
        || base.face_count() > 256
        || skin.face_count() > 4096
        || base.shell_count() != 1
        || !base.validity().is_empty()
    {
        return Ok(None);
    }
    let Some(selected) = base.planar_face(face) else { return Ok(None) };
    let planar_epsilon = (base.size() * 1e-9).max(1e-9);
    let planes = if cut {
        let mut planes = Vec::new();
        let mut corners = Vec::new();
        for i in 0..base.face_count() {
            let Some(plane) = base.planar_face(i) else { return Ok(None) };
            let geometry = face_geometry(base, i)?;
            if geometry.loops.len() != 1 {
                return Ok(None);
            }
            for edge in geometry.loops.iter().flatten() {
                if !edge.is_linear() {
                    return Ok(None);
                }
                let Some(p) = edge.control_points().and_then(points) else { return Ok(None) };
                corners.extend(p);
            }
            planes.push(plane);
        }
        if corners.len() > 8192 || corners.iter().any(|p| planes.iter().any(|plane| plane.height(*p) > tolerance)) {
            return Ok(None);
        }
        planes
    } else {
        // The selected plane itself separates the bodies. The base can be
        // concave or curved wherever it stays behind that plane.
        for i in 0..base.face_count() {
            let geometry = face_geometry(base, i)?;
            if maximum_height(&geometry, &selected).is_none_or(|height| height > planar_epsilon.min(tolerance)) {
                return Ok(None);
            }
        }
        Vec::new()
    };
    let opposite = Plane { y: -selected.y, ..selected };
    let mut rim_count = 0usize;
    let mut coincident = std::collections::BTreeMap::<usize, Vec<usize>>::new();
    for i in 0..skin.face_count() {
        let geometry = face_geometry(skin, i)?;
        if cut {
            let hull = match &geometry.surface {
                EvalSurface::Nurbs { ctrl, closed, .. } if !closed.iter().any(|x| *x) => points(ctrl),
                EvalSurface::Plane { .. } => {
                    let mut hull = Vec::new();
                    for edge in geometry.loops.iter().flatten() {
                        let Some(p) = edge.control_points().and_then(points) else { return Ok(None) };
                        hull.extend(p);
                    }
                    Some(hull)
                }
                _ => None,
            };
            let Some(hull) = hull else { return Ok(None) };
            if hull.iter().any(|p| planes.iter().any(|plane| plane.height(*p) > tolerance)) {
                return Ok(None);
            }
            let mut contacts = planes.iter().enumerate().filter(|(_, plane)| hull.iter().all(|p| plane.height(*p).abs() <= planar_epsilon));
            if let Some((wall, _)) = contacts.next() {
                if wall == face || contacts.next().is_some() {
                    return Ok(None);
                }
                coincident.entry(wall).or_default().push(i);
            } else if !strictly_inside(&geometry.surface, &hull, &planes, (base.size() * 1e-10).max(1e-10)) {
                return Ok(None);
            }
        } else {
            if maximum_height(&geometry, &opposite).is_none_or(|height| height > planar_epsilon.min(tolerance)) {
                return Ok(None);
            }
            let departs_plane = match &geometry.surface {
                EvalSurface::Nurbs { ctrl, .. } => {
                    points(ctrl).is_some_and(|hull| strictly_inside(&geometry.surface, &hull, std::slice::from_ref(&opposite), planar_epsilon))
                }
                _ => maximum_height(&geometry, &selected).is_some_and(|height| height > planar_epsilon),
            };
            if !departs_plane {
                return Ok(None);
            }
        }
        rim_count = rim_count.saturating_add(geometry.loops.iter().map(Vec::len).sum::<usize>());
    }
    if rim_count > 16_384 {
        return Ok(None);
    }
    if !coincident.is_empty() {
        return coincident::cut(base, face, skin, &coincident, tolerance);
    }
    let open = crate::remove_faces(base, &[face])?;
    let result = crate::stitch_split(&[&open, skin], tolerance)?;
    if result.is_surface() || result.shell_count() != 1 || !result.validity().is_empty() {
        return Ok(None);
    }
    let history = crate::boolean_record::through(&result, 0, open.history());
    Ok(Some(result.with_checked_history(history, &[(base.face_count(), base.edge_count()), (skin.face_count(), skin.edge_count())])))
}
