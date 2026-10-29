//! A qualified shared-face union/difference avoids intersecting coincident trims.
use solidcraft_geom::{Plane, Vec3, tess::EvalSurface};

use crate::{Body, Result, eval_geometry::face_geometry};

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

// Every nonempty spline span must leave each original face's plane in its
// interior. A whole coincident wall would need trimming, not face replacement.
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

/// Replace a convex polyhedron's complete planar face by a matched open skin.
/// Returns `None` unless positive-weight control hulls prove that the skin lies
/// outside the selected plane (union), or inside every base half-space (cut).
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
    let mut planes = Vec::new();
    let mut corners = Vec::new();
    for i in 0..base.face_count() {
        let Some(plane) = base.planar_face(i) else { return Ok(None) };
        let geometry = face_geometry(base, i)?;
        if geometry.loops.len() != 1 {
            return Ok(None);
        }
        for edge in geometry.loops.iter().flatten() {
            if edge.curve.degree != 1 {
                return Ok(None);
            }
            let Some(p) = points(&edge.curve.ctrl) else { return Ok(None) };
            corners.extend(p);
        }
        planes.push(plane);
    }
    if corners.len() > 8192 || corners.iter().any(|p| planes.iter().any(|plane| plane.height(*p) > tolerance)) {
        return Ok(None);
    }
    let mut rim = Vec::new();
    for i in 0..skin.face_count() {
        let geometry = face_geometry(skin, i)?;
        let hull = match &geometry.surface {
            EvalSurface::Nurbs { ctrl, closed, .. } if !closed.iter().any(|x| *x) => points(ctrl),
            EvalSurface::Plane { .. } => {
                let mut hull = Vec::new();
                for edge in geometry.loops.iter().flatten() {
                    let Some(p) = points(&edge.curve.ctrl) else { return Ok(None) };
                    hull.extend(p);
                }
                Some(hull)
            }
            _ => None,
        };
        let Some(hull) = hull else { return Ok(None) };
        if cut && !strictly_inside(&geometry.surface, &hull, &planes, (base.size() * 1e-10).max(1e-10)) {
            return Ok(None);
        }
        if hull.iter().any(|p| if cut { planes.iter().any(|plane| plane.height(*p) > tolerance) } else { selected.height(*p) < -tolerance }) {
            return Ok(None);
        }
        for edge in geometry.loops.iter().flatten() {
            if let Some((p, _)) = edge.at(0.0) {
                rim.push(p);
            }
        }
    }
    if rim.len() > 16_384 {
        return Ok(None);
    }
    let open = crate::remove_faces(base, &[face])?;
    let split = crate::splitface::split_edges_at(&open, &rim)?;
    let split = if split.same_shape(&open) { open } else { crate::record::after(split, &open, &[(base.face_count(), base.edge_count())]) };
    let result = crate::stitch(&[&split, skin], tolerance)?;
    if result.is_surface() || result.shell_count() != 1 || !result.validity().is_empty() {
        return Ok(None);
    }
    let history = crate::boolean_record::through(&result, 0, split.history());
    Ok(Some(result.with_checked_history(history, &[(base.face_count(), base.edge_count()), (skin.face_count(), skin.edge_count())])))
}
