//! Shared-face replacement qualified by exact positive-weight support bounds.
use solidcore_record::Plane;
use solidcraft_geom::tess::EvalSurface;

use crate::{Body, FaceGeometry, Result, eval_geometry::face_geometry};

mod coincident;

use solidcore_ops::form_surface_replace::{coplanar, epsilon, inside, points, strictly_inside};

fn boundary(geometry: &FaceGeometry) -> Option<Vec<[f64; 4]>> {
    geometry
        .loops
        .iter()
        .flatten()
        .map(|edge| edge.control_points().map(<[_]>::to_vec))
        .collect::<Option<Vec<_>>>()
        .map(|rows| rows.into_iter().flatten().collect())
}

fn maximum_height(geometry: &FaceGeometry, plane: &Plane) -> Option<f64> {
    let boundary = if matches!(geometry.surface, EvalSurface::Plane { .. }) { boundary(geometry)? } else { Vec::new() };
    solidcore_ops::form_surface_replace::maximum_height(&geometry.surface, &boundary, plane)
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
    let planar_epsilon = epsilon(base.size(), 1e-9);
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
        if corners.len() > 8192 || !inside(&corners, &planes, tolerance) {
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
    let opposite = solidcore_ops::form_surface_replace::opposite(selected);
    let mut rim_count = 0usize;
    let mut coincident = std::collections::BTreeMap::<usize, Vec<usize>>::new();
    for i in 0..skin.face_count() {
        let geometry = face_geometry(skin, i)?;
        if cut {
            let boundary = if matches!(geometry.surface, EvalSurface::Plane { .. }) { boundary(&geometry) } else { Some(Vec::new()) };
            let hull = boundary.and_then(|boundary| solidcore_ops::form_surface_replace::hull(&geometry.surface, &boundary));
            let Some(hull) = hull else { return Ok(None) };
            if !inside(&hull, &planes, tolerance) {
                return Ok(None);
            }
            let mut contacts = planes.iter().enumerate().filter(|(_, plane)| coplanar(&hull, plane, planar_epsilon));
            if let Some((wall, _)) = contacts.next() {
                if wall == face || contacts.next().is_some() {
                    return Ok(None);
                }
                coincident.entry(wall).or_default().push(i);
            } else if !strictly_inside(&geometry.surface, &hull, &planes, epsilon(base.size(), 1e-10)) {
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
    Ok(Some(
        result
            .with_checked_history(history, &[(base.face_count(), base.edge_count()), (skin.face_count(), skin.edge_count())])
            .inherit_regularization_protection(base)
            .inherit_regularization_protection(skin)
            .inherit_split_lines(&[base, skin]),
    ))
}
