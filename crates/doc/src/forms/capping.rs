//! A matched face can close an additive/subtractive tool without changing its
//! editable cage. Standalone Forms keep their free boundary.
use super::{Definition, SupportRef};
use crate::{DocError, ModelState, Result, kernel};

pub(crate) fn cap(definition: &Definition, state: &ModelState, body: &kernel::Body, tolerance: f64) -> Result<Option<(kernel::Body, kernel::Body)>> {
    let Some(matching) = &definition.matching else {
        return Ok(None);
    };
    let SupportRef::Face { body: source, name, pick } = &matching.source else {
        return Ok(None);
    };
    if !body.is_surface() {
        return Ok(None);
    }
    if definition.cage.boundary_loops().map_err(|e| DocError::Invalid(e.to_string()))?.len() != 1 {
        return Err(DocError::Invalid("a matched tool cap requires one complete free opening".into()));
    }
    let source = state.body(source).ok_or_else(|| DocError::Invalid("the cap's support body is missing".into()))?;
    let (index, _, _) =
        crate::naming::face_on_body(source, name, *pick).ok_or_else(|| DocError::Invalid("the cap's named support face is missing".into()))?;
    let seam_tolerance = matching.config.tolerances.position_mm.min(tolerance).max(1e-7);
    let mesh = body.display_mesh(tolerance.min(0.01))?;
    let edges: Vec<_> = mesh.edge_faces.iter().enumerate().filter_map(|(i, faces)| (faces.len() == 1).then_some(i)).collect();
    let picks = kernel::edge_geometry(body, &edges)?
        .iter()
        .map(|c| c.at(0.5).map(|p| p.0).ok_or_else(|| DocError::Invalid("a cap boundary edge has no midpoint".into())))
        .collect::<Result<Vec<_>>>()?;
    let cap = kernel::patch_edges_on_face(body, &picks, &source.body, index, seam_tolerance)?;
    let solid = kernel::stitch(&[body, &cap], seam_tolerance)?;
    if solid.is_surface() || solid.shell_count() != 1 {
        return Err(DocError::Invalid("the matched support did not sew into one closed tool".into()));
    }
    let issues = solid.validity();
    if !issues.is_empty() {
        return Err(DocError::Invalid(format!("the matched tool cap is invalid: {}", issues.join("; "))));
    }
    Ok(Some((solid, cap)))
}
