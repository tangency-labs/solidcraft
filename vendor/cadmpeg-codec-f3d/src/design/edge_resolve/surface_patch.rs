// SPDX-License-Identifier: Apache-2.0
//! Bind grouped patch members to their serialized edge and supporting-face roles.

use crate::records::topology::{
    edge_identity::DesignEdgeOperand, historical_context::AsmFaceBoundary,
};
use cadmpeg_ir::ids::EdgeId;
use std::collections::{HashMap, HashSet};

#[derive(Debug, PartialEq)]
pub(super) enum SurfacePatchRecipeEdges {
    Absent,
    Inconclusive,
    Resolved(Vec<EdgeId>),
}

pub(super) fn surface_patch_grouped_recipe_edges(
    operands: &[&DesignEdgeOperand],
    face_boundaries: &[AsmFaceBoundary],
) -> SurfacePatchRecipeEdges {
    if !operands.iter().any(|operand| {
        !operand.recipe_references.is_empty()
            || operand.recipe_structure.is_some()
            || operand.surface_patch_recipe_structure.is_some()
    }) {
        return SurfacePatchRecipeEdges::Absent;
    }
    let mut boundaries = HashMap::new();
    for boundary in face_boundaries {
        boundaries
            .entry(boundary.id.as_str())
            .and_modify(|found| *found = None)
            .or_insert(Some(boundary));
    }
    let mut edges = Vec::new();
    let mut distinct = HashSet::new();
    for operand in operands {
        let Some(edge) = member_edge(operand, &boundaries) else {
            return SurfacePatchRecipeEdges::Inconclusive;
        };
        if !distinct.insert(edge.clone()) {
            return SurfacePatchRecipeEdges::Inconclusive;
        }
        edges.push(edge);
    }
    SurfacePatchRecipeEdges::Resolved(edges)
}

fn member_edge(
    operand: &DesignEdgeOperand,
    boundaries: &HashMap<&str, Option<&AsmFaceBoundary>>,
) -> Option<EdgeId> {
    if let Some(structure) = &operand.surface_patch_recipe_structure {
        let [first, second] = &structure.clauses;
        let edge_ordinal = common_reference(
            first.edge_reference_ordinals,
            second.edge_reference_ordinals,
        )?;
        let face_ordinal = common_reference(
            first.face_reference_ordinals,
            second.face_reference_ordinals,
        )?;
        let edge_reference = operand.recipe_references.get(edge_ordinal)?;
        let boundary_edges = support_edges(operand, face_ordinal, boundaries)?;
        let mut candidates = edge_reference.candidate_edges.iter().filter(|edge| {
            super::stable_edge_slot(edge).is_some_and(|slot| boundary_edges.contains(&slot))
        });
        let edge = candidates.next()?;
        return candidates.next().is_none().then(|| edge.clone());
    }
    let structure = operand.recipe_structure.as_ref()?;
    let first = structure.sides.first()?;
    if usize::try_from(structure.root).ok()? != structure.sides.len() {
        return None;
    }
    let mut ordinals = HashSet::new();
    for field in std::iter::once(first.header_value).chain(first.scalars.iter().copied()) {
        if field == 0 {
            continue;
        }
        let ordinal = usize::try_from(field.checked_sub(1)?).ok()?;
        operand.recipe_references.get(ordinal)?;
        ordinals.insert(ordinal);
    }
    let mut targets = ordinals.into_iter().filter_map(|ordinal| {
        let reference = &operand.recipe_references[ordinal];
        (!reference.candidate_edges.is_empty()).then_some(reference)
    });
    let target = targets.next()?;
    if targets.next().is_some() {
        return None;
    }
    let [edge] = target.candidate_edges.as_slice() else {
        return None;
    };
    Some(edge.clone())
}

fn common_reference(left: [u32; 2], right: [u32; 2]) -> Option<usize> {
    let mut common = left.into_iter().collect::<HashSet<_>>();
    common.retain(|ordinal| right.contains(ordinal));
    if common.len() != 1 {
        return None;
    }
    usize::try_from(common.into_iter().next()?).ok()
}

fn support_edges(
    operand: &DesignEdgeOperand,
    face_ordinal: usize,
    boundaries: &HashMap<&str, Option<&AsmFaceBoundary>>,
) -> Option<HashSet<i64>> {
    let reference = operand.recipe_references.get(face_ordinal)?;
    if reference.candidate_faces.is_empty() {
        return None;
    }
    let mut faces = HashSet::new();
    for face in &reference.candidate_faces {
        let slot = face.as_str().rsplit_once('#')?.1.parse::<i64>().ok()?;
        if !faces.insert(slot) {
            return None;
        }
    }
    let mut contexts = operand
        .recipe_reference_contexts
        .iter()
        .filter(|context| usize::try_from(context.reference_ordinal).ok() == Some(face_ordinal));
    let context = contexts.next()?;
    if contexts.next().is_some() || context.preceding_face_boundary_ids.is_empty() {
        return None;
    }
    let mut found_faces = HashSet::new();
    let mut edges = HashSet::new();
    let mut parent = None;
    for id in &context.preceding_face_boundary_ids {
        let boundary = boundaries.get(id.as_str())?.as_ref()?;
        if !faces.contains(&boundary.face_slot)
            || !found_faces.insert(boundary.face_slot)
            || parent.is_some_and(|parent| parent != boundary.parent.as_str())
        {
            return None;
        }
        parent = Some(boundary.parent.as_str());
        edges.extend(
            boundary
                .loops
                .iter()
                .flat_map(|loop_| loop_.boundary.coedges().map(|coedge| coedge.edge_slot)),
        );
    }
    Some(edges)
}
