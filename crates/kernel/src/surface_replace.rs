//! Shared-face replacement qualified by exact positive-weight support bounds.
use solidcore_ops::form_surface_replace::{self, SupportFace};
use solidcraft_geom::tess::EvalSurface;

use crate::{Body, FaceGeometry, Result, eval_geometry::face_geometry};

mod coincident;

fn support_face(geometry: FaceGeometry, check_wall: bool) -> SupportFace {
    let edges = || geometry.loops.iter().flatten();
    let linear_loop = check_wall && geometry.loops.len() == 1 && edges().all(|e| e.is_linear() && e.control_points().is_some_and(|p| !p.is_empty()));
    let boundary = if matches!(geometry.surface, EvalSurface::Plane { .. }) || linear_loop {
        edges().map(|edge| edge.control_points()).collect::<Option<Vec<_>>>().map(|curves| curves.concat())
    } else {
        None
    };
    SupportFace { boundary, linear_loop, edge_count: edges().count(), surface: geometry.surface }
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
    let planes = if cut {
        let Some(planes) = (0..base.face_count()).map(|i| base.planar_face(i)).collect::<Option<Vec<_>>>() else { return Ok(None) };
        Some(planes)
    } else {
        None
    };
    let base_faces = (0..base.face_count()).map(|i| face_geometry(base, i).map(|geometry| support_face(geometry, cut)));
    let skin_faces = (0..skin.face_count()).map(|i| face_geometry(skin, i).map(|geometry| support_face(geometry, false)));
    let Some(coincident) =
        form_surface_replace::replacement_contacts(face, selected, planes.as_deref(), base.size(), tolerance, base_faces, skin_faces)?
    else {
        return Ok(None);
    };
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
