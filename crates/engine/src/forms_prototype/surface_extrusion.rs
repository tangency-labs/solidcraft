//! Absolute evaluated-surface extrusion targets, qualified before one atomic edit.
use super::*;
use solidcraft_geom::subdivision::{hierarchy::Hierarchy, matching::evaluated_vertices};
use std::collections::BTreeMap;

const TARGET_RESIDUAL: f64 = 1e-7;
const MATCH_PASSES: usize = 8;

impl Form {
    /// A topology change alters the limit stencil. Moving cap coefficients by
    /// the pointer displacement alone therefore does not move its surface by it.
    pub(crate) fn extrude_surface_drag(
        &mut self,
        state: Option<&crate::doc::ModelState>,
        faces: &[usize],
        edges: &[[usize; 2]],
        displacement: Vec3,
        checkpoint: bool,
    ) -> Result<()> {
        let mut next = self.surface_extrusion_candidate(state, faces, edges, displacement, checkpoint, true)?;
        let mut gesture = next.extrusion.take().ok_or_else(|| error("missing surface extrusion gesture"))?;
        // Preserve staged objects, authoring frame, tool state and the live
        // draft identity. The fresh candidate owns only the edited source.
        let mut candidate = self.clone();
        candidate.replace_primitive(next);
        gesture.revision = candidate.revision;
        candidate.extrusion = Some(gesture);
        if checkpoint {
            self.checkpoint();
        }
        candidate.undo = std::mem::take(&mut self.undo);
        candidate.redo.clear();
        *self = candidate;
        Ok(())
    }

    /// Worker-only preparation. An unqualified candidate can be sampled for
    /// feedback, but only the qualified variant may enter the editor/history.
    pub(super) fn surface_extrusion_candidate(
        &self,
        state: Option<&crate::doc::ModelState>,
        faces: &[usize],
        edges: &[[usize; 2]],
        displacement: Vec3,
        checkpoint: bool,
        qualify: bool,
    ) -> Result<Self> {
        let mut original = if checkpoint {
            self.definition()
        } else {
            let gesture = self.extrusion.as_ref().ok_or_else(|| error("start a surface extrusion gesture first"))?;
            if gesture.revision != self.revision || gesture.faces != faces || gesture.edges != edges {
                return Err(error("surface extrusion gesture is stale"));
            }
            gesture.surface_source.as_deref().ok_or_else(|| error("continue the extrusion with its original control mode"))?.clone()
        };
        if qualify && let Some(matching) = &original.matching {
            let state = state.ok_or_else(|| error("matched extrusion requires its source document"))?;
            original = matching.recompute(&original, state, 0.01)?.0;
        }
        let refine_cap = coarse_box_face(&original, faces, edges)?;
        let mut evaluator = original.clone();
        if refine_cap {
            // A uniform subdivision step preserves the original limit surface.
            // Keep the six logical leaves while giving the new cap a centre
            // and edge-midpoint stations that can retain its rounded profile.
            let mut hierarchy = Hierarchy::new(evaluator.cage.clone()).map_err(error)?;
            hierarchy.depth = 1;
            hierarchy.details.push(Vec::new());
            evaluator.cage = hierarchy.evaluate().map_err(error)?;
            evaluator.hierarchy = Some(hierarchy);
        }
        let source = Self::from_definition(evaluator, self.levels, self.editing_feature)?;
        let mut candidate = if original.matching.is_some() {
            source.matched_extrusion_candidate(faces, edges, displacement, true)?.0
        } else {
            let mut candidate = source.clone();
            candidate.extrude_drag(faces, edges, displacement, true)?;
            candidate
        };
        let targets = if refine_cap {
            let face = *faces.first().ok_or_else(|| error("missing coarse extrusion face"))?;
            refined_cap_targets(&source, &candidate, face, source.local_vector(displacement))?
        } else {
            surface_targets(&source, &candidate, faces)?
        };
        let mut gesture = candidate.extrusion.take().ok_or_else(|| error("missing extrusion topology source"))?;
        if let Some(matching) = &original.matching {
            if qualify {
                let state = state.ok_or_else(|| error("matched extrusion requires its source document"))?;
                let (definition, report) = matching.rebind_refinement(&original, candidate.definition(), state, 0.01)?;
                candidate.refresh_match_preview(definition, report)?;
            } else {
                // Reuse the authoritative continuation evaluator for feedback;
                // this is not a continuity qualification or an authored edit.
                candidate.matching = original.matching.clone();
                let surface = Self::preview(&candidate.cage, candidate.hierarchy.as_ref(), candidate.matching.as_deref(), None, candidate.levels)?.0;
                candidate.install_surface(surface);
            }
        }
        let indices: Vec<_> = targets.keys().copied().collect();
        let positions: Vec<_> = targets.values().map(|p| [p.x, p.y, p.z]).collect();
        let mut qualified = false;
        for _ in 0..if qualify && original.matching.is_some() { MATCH_PASSES } else { 1 } {
            if refine_cap {
                candidate.solve_terminal_cap(&targets)?;
            } else {
                candidate.set_surface_local(&indices, &positions, false)?;
            }
            if qualify && let Some(matching) = candidate.matching.clone() {
                let state = state.ok_or_else(|| error("matched extrusion requires its source document"))?;
                let (definition, report) = matching.recompute(&candidate.definition(), state, 0.01)?;
                candidate.refresh_match_preview(definition, report)?;
            }
            if targets.iter().all(|(&id, &target)| candidate.local_vertices.get(id).is_some_and(|point| point.dist(target) <= TARGET_RESIDUAL)) {
                qualified = true;
                break;
            }
        }
        if !qualified {
            return Err(error("surface extrusion could not reach its evaluated targets while preserving the source constraints"));
        }
        if let Some(symmetry) = &candidate.symmetry {
            symmetry.validate_reflection(&candidate.cage).map_err(error)?;
            candidate.validate_logical_symmetry(symmetry)?;
        }
        gesture.surface_source = Some(Box::new(original));
        gesture.revision = candidate.revision;
        candidate.extrusion = Some(gesture);
        Ok(candidate)
    }

    /// Edit the nine terminal evaluator controls of a refined cap. The centre
    /// remains an evaluator station, so it needs no selectable control entry.
    /// Terminal details also preserve the base cage and existing control births.
    fn solve_terminal_cap(&mut self, targets: &BTreeMap<usize, Vec3>) -> Result<()> {
        if targets.len() != 9 || self.surface_solve_cost(targets.len()) > 4_000_000 {
            return Err(error("coarse extrusion cap exceeds its surface solve budget"));
        }
        let ids: Vec<_> = targets.keys().copied().collect();
        let mut matrix = vec![vec![0.0; ids.len()]; ids.len()];
        for (column, &control) in ids.iter().enumerate() {
            let mut basis = self.cage.clone();
            basis.points.fill(Vec3::ZERO);
            basis.points.get_mut(control).ok_or_else(|| error("missing refined cap control"))?.x = 1.0;
            let response = evaluated_vertices(&basis, 0, self.levels).map_err(error)?;
            for (row, &id) in matrix.iter_mut().zip(&ids) {
                *row.get_mut(column).ok_or_else(|| error("missing refined cap response column"))? =
                    response.get(id).ok_or_else(|| error("missing refined cap response station"))?.x;
            }
        }
        let rhs = targets
            .iter()
            .map(|(&id, &target)| {
                if !target.is_finite() {
                    return Err(error("coarse extrusion cap target must be finite"));
                }
                Ok(target - *self.local_vertices.get(id).ok_or_else(|| error("missing evaluated refined cap station"))?)
            })
            .collect::<Result<Vec<_>>>()?;
        let deltas = solve_response(&matrix, rhs)?;
        let mut cage = self.cage.clone();
        for (&id, delta) in ids.iter().zip(deltas) {
            *cage.points.get_mut(id).ok_or_else(|| error("missing terminal cap control"))? += delta;
        }
        let hierarchy = self.hierarchy.as_ref().ok_or_else(|| error("missing refined extrusion hierarchy"))?.with_evaluator(&cage).map_err(error)?;
        self.adopt_hierarchy(hierarchy, false)
    }
}

/// The coarse Box stencil needs more than four corner targets. Choose the
/// same evaluator topology for every displacement of this source and for all
/// affine ghost samples. Other authoring modes keep their existing evaluator.
fn coarse_box_face(definition: &crate::doc::forms::Definition, faces: &[usize], edges: &[[usize; 2]]) -> Result<bool> {
    let cage = &definition.cage;
    if faces.len() != 1
        || !edges.is_empty()
        || definition.parameters.kind != "box"
        || definition.parameters.segments != [1; 3]
        || definition.hierarchy.is_some()
        || definition.symmetry.is_some()
        || definition.matching.is_some()
        || definition.profile.is_some()
        || definition.pipe.is_some()
        || definition.imported.is_some()
        || cage.points.len() != 8
        || !cage.edge_creases.is_empty()
        || !cage.vertex_creases.is_empty()
    {
        return Ok(false);
    }
    Ok(cage.faces == definition.parameters.build().map_err(error)?.faces)
}

fn refined_cap_targets(source: &Form, candidate: &Form, face: usize, displacement: Vec3) -> Result<BTreeMap<usize, Vec3>> {
    let layout = source.hierarchy.as_ref().ok_or_else(|| error("missing refined source hierarchy"))?.layout().map_err(error)?;
    let mut pairs = BTreeMap::new();
    for (cell, owner) in layout.owners.into_iter().enumerate() {
        if owner != Some(face) {
            continue;
        }
        // Hierarchical extrusion keeps each selected evaluator cell's slot
        // and ordered corner correspondence when it installs the cap.
        let original = source.cage.faces.get(cell).ok_or_else(|| error("missing refined source cell"))?;
        let cap = candidate.cage.faces.get(cell).ok_or_else(|| error("missing refined cap cell"))?;
        if original.len() != 4 || cap.len() != 4 {
            return Err(error("refined extrusion changed a cap cell's corner correspondence"));
        }
        for (&old, &new) in original.iter().zip(cap) {
            if pairs.insert(new, old).is_some_and(|previous| previous != old) {
                return Err(error("refined cap has conflicting source stations"));
            }
        }
    }
    pairs
        .into_iter()
        .map(|(new, old)| {
            let point = *source.local_vertices.get(old).ok_or_else(|| error("missing original refined cap station"))?;
            Ok((new, point + displacement))
        })
        .collect()
}

fn surface_targets(source: &Form, candidate: &Form, faces: &[usize]) -> Result<BTreeMap<usize, Vec3>> {
    let mut pairs = BTreeMap::new();
    if !faces.is_empty() {
        for face in source.mirror_face_selection(faces)? {
            let old = source.logical.faces.get(face).ok_or_else(|| error("missing source extrusion face"))?;
            let cap = candidate.logical.faces.get(face).ok_or_else(|| error("missing translated extrusion face"))?;
            if old.len() != cap.len() {
                return Err(error("extrusion cap changed its logical corner correspondence"));
            }
            for (&old, &new) in old.iter().zip(cap) {
                if pairs.insert(new, old).is_some_and(|previous| previous != old) {
                    return Err(error("extrusion cap has conflicting source identities"));
                }
            }
        }
    } else {
        // Both extrusion implementations append [baseA,baseB,capB,capA]
        // walls (with either base orientation). No coordinate-nearness lookup
        // decides which source boundary point a translated rim point means.
        let walls = if candidate.hierarchy.as_ref().is_some_and(|h| h.has_trim_wall()) {
            candidate.logical.faces.iter().skip(source.logical.faces.len())
        } else {
            candidate.cage.faces.iter().skip(source.cage.faces.len())
        };
        for wall in walls {
            let [a, b, v, u] = wall.as_slice() else { return Err(error("extrusion wall is not a quad")) };
            for (new, old) in [(*u, *a), (*v, *b)] {
                if pairs.insert(new, old).is_some_and(|previous| previous != old) {
                    return Err(error("extrusion rim has conflicting source identities"));
                }
            }
        }
    }
    if pairs.is_empty() || pairs.len() > 256 {
        return Err(error("surface extrusion needs 1..256 translated logical points"));
    }
    pairs
        .into_iter()
        .map(|(new, old)| {
            let original = *source.local_vertices.get(old).ok_or_else(|| error("missing evaluated extrusion source"))?;
            // The paired control displacement includes the operation's exact
            // mirrored/component offset; it is not a display-mesh measurement.
            let delta = candidate.logical.point(new).map_err(error)? - source.logical.point(old).map_err(error)?;
            Ok((new, original + delta))
        })
        .collect()
}
