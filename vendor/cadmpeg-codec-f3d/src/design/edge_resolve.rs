// SPDX-License-Identifier: Apache-2.0
//! Resolve edge-selection operands to stable edge identities.

use crate::ids::{self, native_stream};
use crate::records::{
    feature::{scope::DesignParameterScope, work_geometry::DesignEdgeTreatmentVertexOperand},
    topology::{
        construction::DesignConstructionOperandGroup, edge_identity::DesignEdgeIdentityOperand,
        edge_identity::DesignEdgeOperand,
    },
};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use std::collections::{HashMap, HashSet};

mod surface_patch;
use surface_patch::{surface_patch_grouped_recipe_edges, SurfacePatchRecipeEdges};

const EPS_EDGE_RESOLVE_RADIUS_EDGE_GROUP_CANDIDATES_E9: f64 = 1.0e-9;
const EPS_EDGE_RESOLVE_RADIUS_EDGE_IDENTITY_GROUP_CANDIDATES_E9: f64 = 1.0e-9;

macro_rules! or_none {
    ($value:expr) => {
        match $value {
            Some(value) => value,
            None => return Ok(None),
        }
    };
}

fn sorted_transition_slots(slots: &[i64]) -> Vec<i64> {
    let mut sorted = slots.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    sorted
}

fn native_edge_selection(
    group: &DesignConstructionOperandGroup,
) -> cadmpeg_ir::features::EdgeSelection {
    cadmpeg_ir::features::EdgeSelection::Native(group.id.clone())
}

fn historical_identity_slots(
    group: &DesignConstructionOperandGroup,
    state: cadmpeg_ir::ids::FeatureInputTopologyId,
    feature_id: &cadmpeg_ir::features::FeatureId,
    previous_state_id: i64,
    slots: &[i64],
) -> cadmpeg_ir::features::EdgeSelection {
    let prefix = crate::ids::feature_input_prefix(feature_id, previous_state_id);
    let mut edges = Vec::new();
    for &slot in slots {
        edges.push(crate::ids::history_input_edge_id(&prefix, slot));
    }
    let native = group.id.clone();
    cadmpeg_ir::features::EdgeSelection::historical(state, edges, native)
        .unwrap_or_else(|_| native_edge_selection(group))
}

pub(super) fn resolved_edge_group(
    group: &DesignConstructionOperandGroup,
    groups: &[DesignConstructionOperandGroup],
    operands: &[DesignEdgeOperand],
    identity_operands: &[DesignEdgeIdentityOperand],
    previous_state_id: Option<i64>,
    feature_id: &cadmpeg_ir::features::FeatureId,
    ctx: &DecodeContext<'_>,
) -> Result<cadmpeg_ir::features::EdgeSelection, CodecError> {
    resolved_edge_group_with_transition_chain(
        group,
        groups,
        operands,
        identity_operands,
        EdgeGroupTransition {
            previous_state_id,
            feature_id,
        },
        (EdgeGroupProof::Generic, None),
        ctx,
    )
}

/// Resolve a grouped patch boundary through the serialized recipe roles.
pub(super) fn resolved_surface_patch_edge_group(
    group: &DesignConstructionOperandGroup,
    groups: &[DesignConstructionOperandGroup],
    operands: &[DesignEdgeOperand],
    identity_operands: &[DesignEdgeIdentityOperand],
    face_boundaries: &[crate::records::topology::historical_context::AsmFaceBoundary],
    transition: EdgeGroupTransition<'_>,
    ctx: &DecodeContext<'_>,
) -> Result<cadmpeg_ir::features::EdgeSelection, CodecError> {
    let EdgeGroupTransition {
        previous_state_id,
        feature_id,
    } = transition;
    let fallback = || {
        resolved_edge_group(
            group,
            groups,
            operands,
            identity_operands,
            previous_state_id,
            feature_id,
            ctx,
        )
    };
    let stream = native_stream(&group.id);
    let mut member_ids = HashSet::new();
    for member in group.members() {
        if !member_ids.insert(member.value) {
            return Ok(native_edge_selection(group));
        }
    }
    let mut matched_operands = Vec::new();
    for member in group.members() {
        let mut matches = operands.iter().filter(|operand| {
            native_stream(&operand.id) == stream
                && operand.scope_record_index == group.scope_record_index
                && operand.record_index() == member.value
        });
        let Some(operand) = matches.next() else {
            return Ok(native_edge_selection(group));
        };
        if matches.next().is_some() {
            return Ok(native_edge_selection(group));
        }
        matched_operands.push(operand);
    }
    let edges = match surface_patch_grouped_recipe_edges(&matched_operands, face_boundaries) {
        SurfacePatchRecipeEdges::Absent => return fallback(),
        SurfacePatchRecipeEdges::Inconclusive => {
            return Ok(cadmpeg_ir::features::EdgeSelection::Native(
                group.id.clone(),
            ));
        }
        SurfacePatchRecipeEdges::Resolved(edges) => edges,
    };
    if previous_state_id.is_some_and(|state| {
        matched_operands.iter().any(|operand| {
            operand
                .recipe_state_id
                .is_some_and(|candidate| candidate != state)
        })
    }) {
        return Ok(native_edge_selection(group));
    }
    let state_id = previous_state_id.or_else(|| {
        let mut states = matched_operands
            .iter()
            .filter_map(|operand| operand.recipe_state_id);
        let state_id = states.next()?;
        (states.all(|candidate| candidate == state_id)
            && matched_operands
                .iter()
                .all(|operand| operand.recipe_state_id == Some(state_id)))
        .then_some(state_id)
    });
    let Some(state_id) = state_id else {
        return Ok(cadmpeg_ir::features::EdgeSelection::Edges(edges));
    };
    let mut edge_slots = Vec::new();
    for edge in &edges {
        let Some(slot) = stable_edge_slot(edge) else {
            return fallback();
        };
        edge_slots.push(slot);
    }
    let mut historical_edges = Vec::new();
    for edge_slot in edge_slots {
        let id = crate::ids::history_input_edge_id(
            &crate::ids::feature_input_prefix(feature_id, state_id),
            edge_slot,
        );
        historical_edges.push(id);
    }
    let native = group.id.clone();
    let resolved = cadmpeg_ir::features::EdgeSelection::historical(
        crate::ids::feature_input_topology_id(feature_id, state_id),
        historical_edges,
        native,
    );
    Ok(match resolved {
        Ok(selection) => selection,
        Err(_) => cadmpeg_ir::features::EdgeSelection::Native(group.id.clone()),
    })
}

fn stable_edge_slot(edge: &cadmpeg_ir::ids::EdgeId) -> Option<i64> {
    edge.as_str()
        .rsplit_once('#')?
        .1
        .split(':')
        .next()?
        .parse::<i64>()
        .ok()
}

/// Resolve a selectorless `EdgeFlange` group from its updated source edges.
///
/// An `EdgeFlange` operation preserves the selected source edge as an updated
/// edge. This fallback is admitted only when each group member has exactly one
/// such edge and carries no selector or reference-context evidence. A
/// multi-edge update therefore remains native instead of assigning every
/// changed boundary edge to the group.
pub(super) fn resolved_edge_flange_group(
    group: &DesignConstructionOperandGroup,
    groups: &[DesignConstructionOperandGroup],
    operands: &[DesignEdgeOperand],
    identity_operands: &[DesignEdgeIdentityOperand],
    previous_state_id: Option<i64>,
    feature_id: &cadmpeg_ir::features::FeatureId,
    ctx: &DecodeContext<'_>,
) -> Result<cadmpeg_ir::features::EdgeSelection, CodecError> {
    use cadmpeg_ir::features::EdgeSelection;

    let selection = resolved_edge_group(
        group,
        groups,
        operands,
        identity_operands,
        previous_state_id,
        feature_id,
        ctx,
    )?;
    if !matches!(selection, EdgeSelection::Native(_)) {
        return Ok(selection);
    }
    let Some(previous_state_id) = previous_state_id else {
        return Ok(selection);
    };
    let stream = native_stream(&group.id);
    let mut members = HashSet::new();
    let mut candidate_sets = Vec::new();
    for member in group.members() {
        if !members.insert(member.value) {
            return Ok(selection);
        }
        let mut matching = operands.iter().filter(|operand| {
            native_stream(&operand.id) == stream
                && operand.scope_record_index == group.scope_record_index
                && operand.record_index() == member.value
        });
        let Some(operand) = matching.next() else {
            return Ok(selection);
        };
        if matching.next().is_some() {
            return Ok(selection);
        }
        let Some(candidate) = edge_flange_updated_edge_candidate(operand) else {
            return Ok(selection);
        };
        candidate_sets.push(candidate);
    }
    let Some(edges) = unique_bipartite_assignment(&candidate_sets, ctx)? else {
        return Ok(selection);
    };
    if edges.is_empty() {
        return Ok(selection);
    }
    let state = crate::ids::feature_input_topology_id(feature_id, previous_state_id);
    let mut historical_edges = Vec::new();
    for edge_slot in edges {
        let id = crate::ids::history_input_edge_id(
            &crate::ids::feature_input_prefix(feature_id, previous_state_id),
            edge_slot,
        );
        historical_edges.push(id);
    }
    let native = group.id.clone();
    let historical =
        cadmpeg_ir::features::EdgeSelection::historical(state, historical_edges, native);
    Ok(match historical {
        Ok(selection) => selection,
        Err(_) => EdgeSelection::Native(group.id.clone()),
    })
}

fn edge_flange_updated_edge_candidate(operand: &DesignEdgeOperand) -> Option<Vec<i64>> {
    if !operand.recipe_references.is_empty()
        || !operand.recipe_selectors.is_empty()
        || !operand.recipe_reference_contexts.is_empty()
        || operand.local_topology_references.is_some()
    {
        return None;
    }
    let [edge] = operand.updated_boundary_edge_slots.as_slice() else {
        return None;
    };
    if !operand.preceding_boundary_edge_slots.contains(edge)
        || !operand.changed_boundary_edge_slots.contains(edge)
        || operand.deleted_boundary_edge_slots.contains(edge)
        || !operand.result_boundary_edge_slots.contains(edge)
    {
        return None;
    }
    Some(vec![*edge])
}

/// Resolve an edge-treatment group with the exact transition chain available
/// to Fillet and Chamfer operations.
#[cfg(test)]
pub(super) fn resolved_edge_treatment_group(
    group: &DesignConstructionOperandGroup,
    groups: &[DesignConstructionOperandGroup],
    operands: &[DesignEdgeOperand],
    identity_operands: &[DesignEdgeIdentityOperand],
    previous_state_id: Option<i64>,
    feature_id: &cadmpeg_ir::features::FeatureId,
    treatment_radius: Option<f64>,
) -> Result<cadmpeg_ir::features::EdgeSelection, CodecError> {
    crate::test_support::with_decode_context(|decode_ctx| {
        resolved_edge_treatment_group_with_corners(
            group,
            crate::design::edge_resolve::EdgeTreatmentInputs {
                groups,
                operands,
                identity_operands,
                vertex_operands: &[],
                histories: &[],
                previous_state_id,
                feature_id,
                treatment_radius,
            },
            decode_ctx,
        )
    })
}

#[derive(Clone, Copy)]
pub(crate) struct EdgeTreatmentInputs<'a> {
    pub(crate) groups: &'a [DesignConstructionOperandGroup],
    pub(crate) operands: &'a [DesignEdgeOperand],
    pub(crate) identity_operands: &'a [DesignEdgeIdentityOperand],
    pub(crate) vertex_operands: &'a [DesignEdgeTreatmentVertexOperand],
    pub(crate) histories: &'a [crate::history_records::AsmHistory],
    pub(crate) previous_state_id: Option<i64>,
    pub(crate) feature_id: &'a cadmpeg_ir::features::FeatureId,
    pub(crate) treatment_radius: Option<f64>,
}

/// The history state that the edge operands of a scope without its own
/// state identity share. Edge-treatment selections of such a scope resolve
/// in this state.
pub(crate) fn edge_treatment_recipe_state_id(
    scope: &crate::records::feature::scope::DesignParameterScope,
    operands: &[DesignEdgeOperand],
) -> Option<i64> {
    let stream = native_stream(&scope.id);
    let mut scope_operands = operands.iter().filter(|operand| {
        native_stream(&operand.id) == stream && operand.scope_record_index == scope.record_index
    });
    let state = scope_operands.next()?.recipe_state_id?;
    scope_operands
        .all(|operand| operand.recipe_state_id == Some(state))
        .then_some(state)
}

pub(super) fn resolved_edge_treatment_group_with_corners(
    group: &DesignConstructionOperandGroup,
    input: EdgeTreatmentInputs<'_>,
    ctx: &DecodeContext<'_>,
) -> Result<cadmpeg_ir::features::EdgeSelection, CodecError> {
    use cadmpeg_ir::features::EdgeSelection;
    let EdgeTreatmentInputs {
        groups,
        operands,
        identity_operands,
        vertex_operands,
        histories,
        previous_state_id,
        feature_id,
        treatment_radius,
    } = input;

    let stream = native_stream(&group.id);
    let has_group_corner = vertex_operands.iter().any(|operand| {
        native_stream(&operand.id) == stream
            && operand.scope_record_index == group.scope_record_index
            && operand.group_record_index == group.record_index
    });
    if !has_group_corner {
        return resolved_edge_group_with_transition_chain(
            group,
            groups,
            operands,
            identity_operands,
            EdgeGroupTransition {
                previous_state_id,
                feature_id,
            },
            (
                EdgeGroupProof::Treatment {
                    radius: treatment_radius,
                },
                None,
            ),
            ctx,
        );
    }
    let mut corner_slots = Vec::new();
    let mut edge_members = Vec::new();
    for (ordinal, member) in group.members().iter().copied().enumerate() {
        let edge_count = operands
            .iter()
            .filter(|operand| {
                native_stream(&operand.id) == stream
                    && operand.scope_record_index == group.scope_record_index
                    && operand.record_index() == member.value
            })
            .count();
        let corner = u32::try_from(ordinal).ok().and_then(|ordinal| {
            let mut matches = vertex_operands.iter().filter(|operand| {
                native_stream(&operand.id) == stream
                    && operand.scope_record_index == group.scope_record_index
                    && operand.group_record_index == group.record_index
                    && operand.group_member_ordinal == ordinal
                    && operand.recipe.record_index() == member.value
            });
            let corner = matches.next()?;
            matches.next().is_none().then_some(corner)
        });
        match (edge_count, corner) {
            (1, None) => {
                edge_members.push(member);
            }
            (0, Some(corner)) => {
                let Some(resolution) = corner.recipe.resolution else {
                    return Ok(native_edge_selection(group));
                };
                if Some(resolution.state_id) != previous_state_id {
                    return Ok(native_edge_selection(group));
                }
                corner_slots.push(resolution.vertex_slot());
            }
            _ => return Ok(native_edge_selection(group)),
        }
    }
    if edge_members.is_empty() {
        return Ok(native_edge_selection(group));
    }
    let selection = resolved_edge_group_with_transition_chain(
        group,
        groups,
        operands,
        identity_operands,
        EdgeGroupTransition {
            previous_state_id,
            feature_id,
        },
        (
            EdgeGroupProof::Treatment {
                radius: treatment_radius,
            },
            Some(&edge_members),
        ),
        ctx,
    )?;
    if corner_slots.is_empty() {
        return Ok(selection);
    }
    let EdgeSelection::Historical { edges, .. } = &selection else {
        return Ok(native_edge_selection(group));
    };
    let Some(state_id) = previous_state_id else {
        return Ok(native_edge_selection(group));
    };
    let mut states = histories
        .iter()
        .filter(|history| ids::same_native_occurrence(&history.id, &group.id))
        .flat_map(|history| history.states().iter().map(move |state| (history, state)))
        .filter(|(_, state)| state.state_id == state_id);
    let Some((history, state)) = states.next() else {
        return Ok(native_edge_selection(group));
    };
    let Some(topology) = history.topology(state) else {
        return Ok(native_edge_selection(group));
    };
    if states.next().is_some() {
        return Ok(native_edge_selection(group));
    }
    let mut endpoints = HashSet::new();
    for edge in edges {
        let Some(edge_slot) = edge
            .as_str()
            .rsplit(':')
            .next()
            .and_then(|slot| slot.parse::<i64>().ok())
        else {
            return Ok(native_edge_selection(group));
        };
        let mut matches = topology
            .edge_vertices
            .iter()
            .filter(|edge| edge.edge == edge_slot);
        let Some(edge) = matches.next() else {
            return Ok(native_edge_selection(group));
        };
        if matches.next().is_some() {
            return Ok(native_edge_selection(group));
        }
        endpoints.insert(edge.start_vertex);
        endpoints.insert(edge.end_vertex);
    }
    if corner_slots.iter().all(|corner| endpoints.contains(corner)) {
        Ok(selection)
    } else {
        Ok(native_edge_selection(group))
    }
}

#[derive(Clone, Copy)]
enum EdgeGroupProof {
    /// Member-local recipe and persistent-identity proofs only.
    Generic,
    /// Fillet or Chamfer proofs that may consume an exact operation transition.
    Treatment { radius: Option<f64> },
}

#[derive(Clone, Copy)]
pub(super) struct EdgeGroupTransition<'a> {
    pub(super) previous_state_id: Option<i64>,
    pub(super) feature_id: &'a cadmpeg_ir::features::FeatureId,
}

fn resolved_edge_group_with_transition_chain(
    group: &DesignConstructionOperandGroup,
    groups: &[DesignConstructionOperandGroup],
    operands: &[DesignEdgeOperand],
    identity_operands: &[DesignEdgeIdentityOperand],
    transition: EdgeGroupTransition<'_>,
    (proof, members_override): (
        EdgeGroupProof,
        Option<&[crate::records::identity::Located<u32>]>,
    ),
    ctx: &DecodeContext<'_>,
) -> Result<cadmpeg_ir::features::EdgeSelection, CodecError> {
    use cadmpeg_ir::features::EdgeSelection;

    let EdgeGroupTransition {
        previous_state_id,
        feature_id,
    } = transition;
    let members = members_override.unwrap_or_else(|| group.members());

    let (allow_edge_treatment_transition_chain, treatment_radius) = match proof {
        EdgeGroupProof::Generic => (false, None),
        EdgeGroupProof::Treatment { radius } => (true, radius),
    };

    let unmatched_selection = |state_id: Option<i64>| -> Result<EdgeSelection, CodecError> {
        if group.lost_edge_references.is_empty() {
            Ok(native_edge_selection(group))
        } else {
            let Some(state_id) = state_id else {
                return Ok(EdgeSelection::Unresolved);
            };
            Ok(partial_historical_edge_selection(
                group
                    .lost_edge_references
                    .iter()
                    .map(|identity| (identity.as_str(), None)),
                state_id,
                feature_id,
                crate::ids::feature_input_topology_id(feature_id, state_id),
                &group.id,
            )
            .unwrap_or(EdgeSelection::Unresolved))
        }
    };
    let stream = native_stream(&group.id);
    let has_surface_patch_operand = operands.iter().any(|operand| {
        native_stream(&operand.id) == stream
            && operand.scope_record_index == group.scope_record_index
            && members
                .iter()
                .any(|member| member.value == operand.record_index())
            && operand.surface_patch_recipe_structure.is_some()
    });
    if has_surface_patch_operand {
        let mut member_ids = HashSet::new();
        for member in members {
            if !member_ids.insert(member.value) {
                return unmatched_selection(previous_state_id);
            }
        }
        let mut matched_operands = Vec::new();
        for member in members.iter().map(|member| &member.value) {
            let mut matches = operands.iter().filter(|operand| {
                native_stream(&operand.id) == stream
                    && operand.scope_record_index == group.scope_record_index
                    && operand.record_index() == *member
            });
            let Some(operand) = matches.next().filter(|_| matches.next().is_none()) else {
                return unmatched_selection(previous_state_id);
            };
            matched_operands.push(operand);
        }
        if matched_operands
            .iter()
            .any(|operand| operand.surface_patch_recipe_structure.is_none())
        {
            return unmatched_selection(previous_state_id);
        }
        let state_id = previous_state_id.or_else(|| {
            let mut states = matched_operands
                .iter()
                .filter_map(|operand| operand.recipe_state_id);
            let state_id = states.next()?;
            (states.all(|candidate| candidate == state_id)
                && matched_operands
                    .iter()
                    .all(|operand| operand.recipe_state_id == Some(state_id)))
            .then_some(state_id)
        });
        let Some(state_id) = state_id else {
            return unmatched_selection(previous_state_id);
        };
        let mut edges = Vec::new();
        for operand in &matched_operands {
            let Some(edge) = operand.resolved_edge_slot else {
                return unmatched_selection(Some(state_id));
            };
            edges.push(edge);
        }
        let mut resolved_edges = Vec::new();
        for edge_slot in edges {
            if !resolved_edges.contains(&edge_slot) {
                resolved_edges.push(edge_slot);
            }
        }
        if resolved_edges.is_empty() {
            return unmatched_selection(Some(state_id));
        }
        let mut historical_edges = Vec::new();
        for edge_slot in resolved_edges {
            let edge = crate::ids::history_input_edge_id(
                &crate::ids::feature_input_prefix(feature_id, state_id),
                edge_slot,
            );
            historical_edges.push(edge);
        }
        let native = group.id.clone();
        return match cadmpeg_ir::features::EdgeSelection::historical(
            crate::ids::feature_input_topology_id(feature_id, state_id),
            historical_edges,
            native,
        ) {
            Ok(selection) => Ok(selection),
            Err(_) => Ok(native_edge_selection(group)),
        };
    }
    let mut identity_matches = Vec::new();
    let mut identities_complete = true;
    for member in members.iter().map(|member| &member.value) {
        let mut matches = identity_operands.iter().filter(|operand| {
            native_stream(&operand.id) == stream
                && operand.scope_record_index == group.scope_record_index
                && operand.group_record_index == group.record_index
                && operand.record_index() == *member
        });
        let Some(operand) = matches.next().filter(|_| matches.next().is_none()) else {
            identities_complete = false;
            break;
        };
        identity_matches.push(operand);
    }
    let identity_matches = identities_complete.then_some(identity_matches);
    let has_recipe_operands = members.iter().map(|member| &member.value).all(|member| {
        let mut matches = operands.iter().filter(|operand| {
            native_stream(&operand.id) == stream
                && operand.scope_record_index == group.scope_record_index
                && operand.record_index() == *member
        });
        matches.next().is_some() && matches.next().is_none()
    });
    let has_unstructured_recipe_operand =
        members.iter().map(|member| &member.value).any(|member| {
            operands.iter().any(|operand| {
                native_stream(&operand.id) == stream
                    && operand.scope_record_index == group.scope_record_index
                    && operand.record_index() == *member
                    && !operand.recipe_program.is_empty()
                    && operand.recipe_structure.is_none()
            })
        });
    if has_recipe_operands && has_unstructured_recipe_operand {
        return unmatched_selection(previous_state_id);
    }
    let has_standard_recipe_operands = members.iter().map(|member| &member.value).any(|member| {
        operands.iter().any(|operand| {
            native_stream(&operand.id) == stream
                && operand.scope_record_index == group.scope_record_index
                && operand.record_index() == *member
                && operand.recipe_structure.is_some()
        })
    });
    let has_concrete_recipe_evidence = members.iter().map(|member| &member.value).any(|member| {
        let mut matches = operands.iter().filter(|operand| {
            native_stream(&operand.id) == stream
                && operand.scope_record_index == group.scope_record_index
                && operand.record_index() == *member
        });
        matches.next().is_some_and(|operand| {
            matches.next().is_none()
                && (operand.resolved_edge_slot.is_some()
                    || !operand.changed_boundary_edge_slots.is_empty()
                    || !operand.deleted_boundary_edge_slots.is_empty()
                    || !operand.treatment_radius_candidates.is_empty())
        })
    });
    let identity_transition_slots = if allow_edge_treatment_transition_chain
        && treatment_radius.is_none()
        && members.len() == 1
    {
        match identity_matches.as_deref() {
            Some([operand]) => {
                let edges = sorted_transition_slots(&operand.transition_edge_candidates);
                (!edges.is_empty()).then_some(edges)
            }
            _ => None,
        }
    } else {
        None
    };
    let identity_group_transition_slots = if let Some(first) = identity_matches
        .as_ref()
        .and_then(|matches| matches.first())
    {
        let edges = sorted_transition_slots(&first.transition_edge_candidates);
        if allow_edge_treatment_transition_chain && !edges.is_empty() {
            let mut uniform = true;
            if let Some(matches) = identity_matches.as_ref() {
                for operand in matches {
                    if !operand.layout().is_compact() {
                        uniform = false;
                        break;
                    }
                    let candidate = sorted_transition_slots(&operand.transition_edge_candidates);
                    if candidate != edges {
                        uniform = false;
                        break;
                    }
                }
            }
            uniform.then_some(edges)
        } else {
            None
        }
    } else {
        None
    };
    let recipe_supports_transition_chain = |chain: &[i64]| -> Result<bool, CodecError> {
        let mut member_operands = Vec::new();
        for member in members.iter().map(|member| &member.value) {
            let mut matches = operands.iter().filter(|operand| {
                native_stream(&operand.id) == stream
                    && operand.scope_record_index == group.scope_record_index
                    && operand.record_index() == *member
            });
            let Some(operand) = matches.next().filter(|_| matches.next().is_none()) else {
                return Ok(false);
            };
            member_operands.push(operand);
        }
        Ok(transition_chain_is_supported_by_recipe(
            chain,
            members.len(),
            &member_operands,
        ))
    };
    let identity_transition_is_supported = match identity_transition_slots
        .as_deref()
        .or(identity_group_transition_slots.as_deref())
    {
        Some(chain) => recipe_supports_transition_chain(chain)?,
        None => false,
    };
    // A cardinality mismatch is a group-level transition proof, not a
    // member-to-edge assignment. Recipe evidence must therefore cover the
    // complete chain before it can replace the unresolved member identities.
    let identity_group_transition_is_admitted = match identity_group_transition_slots.as_deref() {
        Some(edges) => edges.len() == members.len() || recipe_supports_transition_chain(edges)?,
        None => false,
    };
    let identity_radius_slots = match (treatment_radius, identity_matches.as_ref()) {
        (Some(radius), Some(matches)) => radius_edge_identity_group_candidates(matches, radius),
        _ => None,
    };
    let has_complete_identity_selection = identity_matches.as_ref().is_some_and(|operands| {
        !operands.is_empty()
            && (operands.iter().all(|operand| {
                operand.resolved_edge_slot.is_some() || !operand.resolved_edge_slots.is_empty()
            }) || identity_transition_slots.is_some()
                || identity_group_transition_is_admitted
                || identity_radius_slots.is_some())
    });
    let all_member_identities_are_lost =
        !members.is_empty() && group.lost_edge_references.len() == members.len();
    if let Some(identity_matches) = identity_matches.as_ref().filter(|_| {
        has_complete_identity_selection
            && !has_standard_recipe_operands
            && (!has_recipe_operands
                || !has_concrete_recipe_evidence
                || identity_transition_is_supported
                || all_member_identities_are_lost)
    }) {
        if identity_matches.is_empty() {
            return unmatched_selection(previous_state_id);
        }
        let Some(previous_state_id) = previous_state_id else {
            return unmatched_selection(None);
        };
        let state = crate::ids::feature_input_topology_id(feature_id, previous_state_id);
        if identity_matches.iter().all(|operand| {
            operand.resolved_edge_slot.is_some() || !operand.resolved_edge_slots.is_empty()
        }) {
            let mut seen = HashSet::new();
            let mut edges = Vec::new();
            for edge_slot in identity_matches.iter().flat_map(|operand| {
                operand
                    .resolved_edge_slot
                    .iter()
                    .copied()
                    .chain(operand.resolved_edge_slots.iter().copied())
            }) {
                if seen.insert(edge_slot) {
                    let edge = crate::ids::history_input_edge_id(
                        &crate::ids::feature_input_prefix(feature_id, previous_state_id),
                        edge_slot,
                    );
                    edges.push(edge);
                }
            }
            let native = group.id.clone();
            return match cadmpeg_ir::features::EdgeSelection::historical(state, edges, native) {
                Ok(selection) => Ok(selection),
                Err(_) => Ok(native_edge_selection(group)),
            };
        }
        if let Some(edges) = identity_radius_slots.as_ref() {
            return Ok(historical_identity_slots(
                group,
                state,
                feature_id,
                previous_state_id,
                edges,
            ));
        }
        if let Some(edges) = identity_group_transition_slots.as_ref() {
            return Ok(historical_identity_slots(
                group,
                state,
                feature_id,
                previous_state_id,
                edges,
            ));
        }
        if identity_matches.len() == 1 && identity_matches[0].resolved_edge_slot.is_none() {
            if let Some(edges) = identity_transition_slots.as_ref() {
                return Ok(historical_identity_slots(
                    group,
                    state,
                    feature_id,
                    previous_state_id,
                    edges,
                ));
            }
        }
        return match partial_historical_edge_selection(
            identity_matches
                .iter()
                .map(|operand| (operand.id.as_str(), operand.resolved_edge_slot)),
            previous_state_id,
            feature_id,
            state,
            &group.id,
        ) {
            Some(selection) => Ok(selection),
            None => Ok(native_edge_selection(group)),
        };
    }
    let mut matched_operands = Vec::new();
    let mut member_identities = HashSet::new();
    for member in members.iter().map(|member| &member.value) {
        if !member_identities.insert(*member) {
            return unmatched_selection(previous_state_id);
        }
        let mut matches = operands.iter().filter(|operand| {
            native_stream(&operand.id) == stream
                && operand.scope_record_index == group.scope_record_index
                && operand.record_index() == *member
        });
        let Some(operand) = matches.next() else {
            return unmatched_selection(previous_state_id);
        };
        if matches.next().is_some() {
            return unmatched_selection(previous_state_id);
        }
        matched_operands.push(operand);
    }
    let recipe_state_id = || {
        let mut states = matched_operands
            .iter()
            .filter_map(|operand| operand.recipe_state_id);
        let state = states.next()?;
        (states.all(|candidate| candidate == state)
            && matched_operands
                .iter()
                .all(|operand| operand.recipe_state_id == Some(state)))
        .then_some(state)
    };
    let transition_state_id = previous_state_id;
    let Some(previous_state_id) = transition_state_id.or_else(recipe_state_id) else {
        return if group.lost_edge_references.is_empty() {
            Ok(native_edge_selection(group))
        } else {
            Ok(EdgeSelection::Unresolved)
        };
    };
    let state = crate::ids::feature_input_topology_id(feature_id, previous_state_id);
    let lost_selection = || unmatched_selection(Some(previous_state_id));
    let mut exact_slots = Some(Vec::new());
    for operand in &matched_operands {
        let Some(slot) = resolved_edge_operand(operand) else {
            exact_slots = None;
            break;
        };
        if let Some(slots) = exact_slots.as_mut() {
            slots.push(slot);
        }
    }
    if exact_slots.is_none() {
        exact_slots = unique_edge_group_assignment(&matched_operands, ctx)?;
    }
    if exact_slots.is_none() {
        exact_slots = changed_reference_edge_group_candidates(&matched_operands, ctx)?;
    }
    let transition_slots = || -> Result<Option<Vec<i64>>, CodecError> {
        let mut slots = match treatment_radius {
            Some(radius) => radius_edge_group_candidates(&matched_operands, radius),
            None => None,
        };
        if slots.is_none() {
            slots = match (treatment_radius, identity_matches.as_ref()) {
                (Some(radius), Some(operands)) => {
                    radius_edge_identity_group_candidates(operands, radius)
                }
                _ => None,
            };
        }
        if slots.is_none() {
            slots = context_only_edge_group_candidates(matched_operands.iter().map(|operand| {
                (
                    resolved_edge_operand(operand),
                    operand.changed_boundary_edge_slots.as_slice(),
                )
            }));
        }
        if slots.is_none() {
            slots = changed_boundary_count_edge_group_candidates(
                matched_operands
                    .iter()
                    .map(|operand| operand.recipe_selectors.as_slice()),
            );
        }
        if slots.is_none() {
            slots = deleted_reference_edge_group_candidates(&matched_operands, ctx)?;
        }
        if slots.is_none() {
            slots = common_deleted_edge_group_candidates(matched_operands.iter().map(|operand| {
                (
                    !operand.changed_boundary_edge_slots.is_empty(),
                    operand.deleted_boundary_edge_slots.as_slice(),
                )
            }));
        }
        if slots.is_none() && allow_edge_treatment_transition_chain {
            slots = contextual_deleted_edge_group_candidates(&matched_operands, ctx)?;
        }
        if slots.is_none() && allow_edge_treatment_transition_chain {
            slots = result_boundary_reference_edge_group_candidates(&matched_operands);
        }
        if slots.is_none() && allow_edge_treatment_transition_chain {
            slots = deleted_boundary_edge_group_candidates(&matched_operands);
        }
        if slots.is_none() {
            slots = scope_partition_edge_group_candidates(group, groups, operands, members);
        }
        Ok(slots)
    };
    let transition_complete_slots = match (
        allow_edge_treatment_transition_chain,
        treatment_radius,
        identity_matches.as_deref(),
    ) {
        (true, Some(radius), Some(identities))
            if exact_slots.is_none() && transition_state_id.is_some() =>
        {
            transition_complete_edge_group_candidates(&matched_operands, identities, radius)
        }
        _ => None,
    };
    let resolved_slots = exact_slots.or(transition_complete_slots);
    let resolved_slots = if resolved_slots.is_some()
        || has_standard_recipe_operands
        || transition_state_id.is_none()
    {
        resolved_slots
    } else {
        transition_slots()?
    };
    let Some(resolved_slots) = resolved_slots else {
        if !group.lost_edge_references.is_empty() {
            return lost_selection();
        }
        if has_standard_recipe_operands {
            return Ok(partial_historical_edge_selection(
                matched_operands
                    .iter()
                    .map(|operand| (operand.id.as_str(), resolved_edge_operand(operand))),
                previous_state_id,
                feature_id,
                state,
                &group.id,
            )
            .unwrap_or_else(|| native_edge_selection(group)));
        }
        let mut combined_edges = Vec::new();
        for (index, operand) in matched_operands.iter().enumerate() {
            let recipe = resolved_edge_operand(operand);
            let identity = identity_matches
                .as_ref()
                .and_then(|identities| identities[index].resolved_edge_slot);
            let combined = match (recipe, identity) {
                (Some(recipe), Some(identity)) if recipe != identity => {
                    return unmatched_selection(Some(previous_state_id));
                }
                (recipe, identity) => recipe.or(identity),
            };
            combined_edges.push(combined);
        }
        if combined_edges.iter().all(Option::is_some) {
            let mut edges = Vec::new();
            for edge_slot in combined_edges.into_iter().flatten() {
                let edge = crate::ids::history_input_edge_id(
                    &crate::ids::feature_input_prefix(feature_id, previous_state_id),
                    edge_slot,
                );
                if !edges.contains(&edge) {
                    edges.push(edge);
                }
            }
            let native = group.id.clone();
            return match cadmpeg_ir::features::EdgeSelection::historical(state, edges, native) {
                Ok(selection) => Ok(selection),
                Err(_) => Ok(native_edge_selection(group)),
            };
        }
        let mut partial_members = Vec::new();
        for (operand, resolved) in matched_operands.iter().zip(combined_edges) {
            let carries_transition_evidence = identity_matches.is_some()
                || transition_state_id.is_none()
                || !operand.changed_boundary_edge_slots.is_empty();
            if resolved.is_some() || carries_transition_evidence {
                partial_members.push((operand.id.as_str(), resolved));
            }
        }
        return match partial_historical_edge_selection(
            partial_members,
            previous_state_id,
            feature_id,
            state,
            &group.id,
        ) {
            Some(selection) => Ok(selection),
            None => Ok(native_edge_selection(group)),
        };
    };
    let mut edges = Vec::new();
    for edge_slot in resolved_slots {
        let edge = crate::ids::history_input_edge_id(
            &crate::ids::feature_input_prefix(feature_id, previous_state_id),
            edge_slot,
        );
        if !edges.contains(&edge) {
            edges.push(edge);
        }
    }
    if edges.is_empty() {
        Ok(native_edge_selection(group))
    } else {
        let native = group.id.clone();
        match cadmpeg_ir::features::EdgeSelection::historical(state, edges, native) {
            Ok(selection) => Ok(selection),
            Err(_) => Ok(native_edge_selection(group)),
        }
    }
}

pub(super) fn resolved_hem_edge_group(
    group: &DesignConstructionOperandGroup,
    groups: &[DesignConstructionOperandGroup],
    operands: &[DesignEdgeOperand],
    identity_operands: &[DesignEdgeIdentityOperand],
    previous_state_id: Option<i64>,
    feature_id: &cadmpeg_ir::features::FeatureId,
    ctx: &DecodeContext<'_>,
) -> Result<cadmpeg_ir::features::EdgeSelection, CodecError> {
    use cadmpeg_ir::features::EdgeSelection;

    let selection = resolved_edge_group(
        group,
        groups,
        operands,
        identity_operands,
        previous_state_id,
        feature_id,
        ctx,
    )?;
    if !matches!(selection, EdgeSelection::Native(_)) {
        return Ok(selection);
    }
    let Some(previous_state_id) = previous_state_id else {
        return Ok(selection);
    };
    let [crate::records::identity::Located { value: member, .. }] = group.members() else {
        return Ok(selection);
    };
    let mut matching_operands = operands.iter().filter(|operand| {
        native_stream(&operand.id) == native_stream(&group.id)
            && operand.scope_record_index == group.scope_record_index
            && operand.record_index() == *member
    });
    let Some(operand) = matching_operands
        .next()
        .filter(|_| matching_operands.next().is_none())
    else {
        return Ok(selection);
    };
    let Some(edge) = hem_transition_edge_slot(operand) else {
        return Ok(selection);
    };
    Ok(cadmpeg_ir::features::EdgeSelection::historical(
        crate::ids::feature_input_topology_id(feature_id, previous_state_id),
        vec![crate::ids::history_input_edge_id(
            &crate::ids::feature_input_prefix(feature_id, previous_state_id),
            edge,
        )],
        group.id.clone(),
    )
    .unwrap_or(selection))
}

/// Return the one historical edge a single-member Hem operand identifies.
///
/// A directly resolved operand is preferred. The transition proof is the
/// fallback used by the compact recipe form, where the operand carries only
/// the changed support boundaries and the selectorless edge context.
pub(super) fn resolved_hem_edge_slot(
    operand: &DesignEdgeOperand,
    previous_state_id: Option<i64>,
) -> Option<i64> {
    if let Some(edge) = operand.resolved_edge_slot {
        return Some(edge);
    }
    previous_state_id?;
    hem_transition_edge_slot(operand)
}

/// Check recipe evidence before an exact edge-treatment transition chain
/// replaces persistent recipe or identity context.
///
/// Changed and deleted recipe boundaries are hard evidence. A deleted edge
/// proven by a recipe but absent from the treatment chain contradicts that
/// chain. A member with deleted boundaries must also share at least one edge
/// with the chain. When any member has changed or deleted boundary evidence,
/// every edge in the exact chain must occur in the group's combined recipe
/// boundary set. The set includes the selected reference contexts because a
/// structured recipe can carry the operation edge on a reference face that is
/// not the operand's primary candidate face.
fn transition_chain_is_supported_by_recipe(
    chain: &[i64],
    member_count: usize,
    operands: &[&DesignEdgeOperand],
) -> bool {
    if operands.len() != member_count {
        return false;
    }
    let mut all_recipe_edges = Vec::new();
    for operand in operands {
        let resolved = resolved_edge_operand(operand);
        if let Some(edge) = resolved {
            if operand.deleted_boundary_edge_slots.contains(&edge) && !chain.contains(&edge) {
                return false;
            }
        }
        if !operand.deleted_boundary_edge_slots.is_empty()
            && !operand
                .deleted_boundary_edge_slots
                .iter()
                .any(|edge| chain.contains(edge))
        {
            return false;
        }
        for edge in operand
            .changed_boundary_edge_slots
            .iter()
            .chain(&operand.deleted_boundary_edge_slots)
            .copied()
            .chain(edge_operand_reference_edge_sets(operand).flatten().copied())
        {
            all_recipe_edges.push(edge);
        }
    }
    all_recipe_edges.sort_unstable_by(Ord::cmp);
    all_recipe_edges.dedup();
    all_recipe_edges.is_empty() || chain.iter().all(|edge| all_recipe_edges.contains(edge))
}

fn hem_transition_edge_slot(operand: &DesignEdgeOperand) -> Option<i64> {
    let reference_contexts = operand.recipe_reference_contexts.as_slice();
    let mut empty_contexts = reference_contexts
        .iter()
        .filter(|context| context.changed_reference_edge_slots.is_empty());
    let empty_context = empty_contexts
        .next()
        .filter(|_| empty_contexts.next().is_none())?;
    if !empty_context.result_faces.is_empty()
        || !empty_context.preceding_faces.is_empty()
        || !empty_context.preceding_support_face_slots.is_empty()
        || reference_contexts.iter().any(|context| {
            !context.changed_reference_edge_slots.is_empty()
                && (context.result_faces.is_empty()
                    || context.preceding_support_face_slots.is_empty())
        })
    {
        return None;
    }
    unique_hem_transition_edge_candidate(
        &operand.changed_boundary_edge_slots,
        reference_contexts
            .iter()
            .map(|context| context.changed_reference_edge_slots.as_slice()),
    )
}

fn unique_hem_transition_edge_candidate<'a>(
    changed_boundary_edges: &[i64],
    reference_edge_sets_input: impl IntoIterator<Item = &'a [i64]>,
) -> Option<i64> {
    let mut reference_edge_sets: Vec<&'a [i64]> = Vec::new();
    for edges in reference_edge_sets_input {
        reference_edge_sets.push(edges);
    }
    if reference_edge_sets.is_empty()
        || reference_edge_sets
            .iter()
            .filter(|edges| edges.is_empty())
            .count()
            != 1
    {
        return None;
    }
    let mut support_edges = Vec::new();
    for edge in reference_edge_sets
        .iter()
        .flat_map(|edges| edges.iter().copied())
    {
        support_edges.push(edge);
    }
    support_edges.sort_unstable_by(Ord::cmp);
    support_edges.dedup();
    if support_edges.is_empty()
        || support_edges
            .iter()
            .any(|edge| !changed_boundary_edges.contains(edge))
    {
        return None;
    }
    let mut candidates = Vec::new();
    for edge in changed_boundary_edges
        .iter()
        .copied()
        .filter(|edge| !support_edges.contains(edge))
    {
        candidates.push(edge);
    }
    candidates.sort_unstable_by(Ord::cmp);
    candidates.dedup();
    match candidates.as_slice() {
        [edge] => Some(*edge),
        _ => None,
    }
}

fn partial_historical_edge_selection<'a>(
    members: impl IntoIterator<Item = (&'a str, Option<i64>)>,
    previous_state_id: i64,
    feature_id: &cadmpeg_ir::features::FeatureId,
    state: cadmpeg_ir::ids::FeatureInputTopologyId,
    native: &str,
) -> Option<cadmpeg_ir::features::EdgeSelection> {
    use cadmpeg_ir::features::EdgeSelection;

    let mut edges = Vec::new();
    let mut unresolved = Vec::new();
    for (identity, edge) in members {
        if let Some(edge) = edge {
            if !edges.contains(&edge) {
                edges.push(edge);
            }
        } else {
            let id = identity.to_owned();
            unresolved.push(id);
        }
    }
    if unresolved.is_empty() || edges.is_empty() {
        return None;
    }
    let mut historical_edges = Vec::new();
    for edge_slot in edges {
        let edge = crate::ids::history_input_edge_id(
            &crate::ids::feature_input_prefix(feature_id, previous_state_id),
            edge_slot,
        );
        historical_edges.push(edge);
    }
    let native_id = native.to_owned();
    Some(
        match cadmpeg_ir::features::EdgeSelection::historical_partial(
            state,
            historical_edges,
            unresolved,
            native_id,
        ) {
            Ok(selection) => selection,
            Err(_) => EdgeSelection::Native(native.to_owned()),
        },
    )
}

fn context_only_edge_group_candidates<'a>(
    members: impl IntoIterator<Item = (Option<i64>, &'a [i64])>,
) -> Option<Vec<i64>> {
    let mut edges = Vec::new();
    for (resolved, changed_candidates) in members {
        match resolved {
            Some(edge) => {
                if !edges.contains(&edge) {
                    edges.push(edge);
                }
            }
            None if changed_candidates.is_empty() => {}
            None => return None,
        }
    }
    (!edges.is_empty()).then_some(edges)
}

fn unique_edge_group_assignment(
    operands: &[&DesignEdgeOperand],
    ctx: &DecodeContext<'_>,
) -> Result<Option<Vec<i64>>, CodecError> {
    if operands.is_empty() {
        return Ok(None);
    }
    let mut candidate_sets = Vec::new();
    for operand in operands {
        let candidates = if let Some(edge) = resolved_edge_operand(operand) {
            Some(EdgeAssignmentCandidates::Edges(vec![edge]))
        } else {
            edge_group_assignment_candidates(
                &operand.recipe_selectors,
                edge_operand_reference_edge_sets(operand),
            )
        };
        let Some(candidates) = candidates else {
            return Ok(None);
        };
        candidate_sets.push(candidates);
    }
    unique_edge_assignment(&candidate_sets, ctx)
}

pub(super) fn changed_reference_edge_group_candidates(
    operands: &[&DesignEdgeOperand],
    ctx: &DecodeContext<'_>,
) -> Result<Option<Vec<i64>>, CodecError> {
    let mut candidate_sets = Vec::new();
    for operand in operands {
        let mut changed_sets = operand
            .recipe_reference_contexts
            .iter()
            .map(|context| context.changed_reference_edge_slots.as_slice())
            .filter(|edges| !edges.is_empty());
        let Some(first) = changed_sets.next() else {
            return Ok(None);
        };
        let mut candidates = Vec::new();
        for edge in first {
            candidates.push(*edge);
        }
        for changed in changed_sets {
            candidates.retain(|candidate| changed.contains(candidate));
        }
        candidates.sort_unstable_by(Ord::cmp);
        candidates.dedup();
        if candidates.is_empty() {
            return Ok(None);
        }
        candidate_sets.push(candidates);
    }
    unique_bipartite_assignment(&candidate_sets, ctx)
}

fn deleted_reference_edge_group_candidates(
    operands: &[&DesignEdgeOperand],
    ctx: &DecodeContext<'_>,
) -> Result<Option<Vec<i64>>, CodecError> {
    let mut reference_candidates = Vec::new();
    let mut deleted_candidates = Vec::new();
    for operand in operands {
        let mut candidates = Vec::new();
        for edge in operand
            .recipe_reference_contexts
            .iter()
            .flat_map(|context| context.changed_reference_edge_slots.iter().copied())
        {
            candidates.push(edge);
        }
        candidates.sort_unstable_by(Ord::cmp);
        candidates.dedup();
        reference_candidates.push(candidates);
        let mut deleted = Vec::new();
        for edge in &operand.deleted_boundary_edge_slots {
            deleted.push(*edge);
        }
        deleted_candidates.push(deleted);
    }
    unique_deleted_reference_assignment(&reference_candidates, &deleted_candidates, ctx)
}

fn unique_deleted_reference_assignment(
    reference_candidates: &[Vec<i64>],
    deleted_candidates: &[Vec<i64>],
    ctx: &DecodeContext<'_>,
) -> Result<Option<Vec<i64>>, CodecError> {
    if reference_candidates.len() != deleted_candidates.len() {
        return Ok(None);
    }
    let mut candidate_sets = Vec::new();
    for (references, deleted) in reference_candidates.iter().zip(deleted_candidates) {
        let mut candidates = Vec::new();
        for edge in references
            .iter()
            .copied()
            .filter(|edge| deleted.contains(edge))
        {
            candidates.push(edge);
        }
        candidates.sort_unstable_by(Ord::cmp);
        candidates.dedup();
        if candidates.is_empty() {
            return Ok(None);
        }
        candidate_sets.push(candidates);
    }
    unique_bipartite_assignment(&candidate_sets, ctx)
}

#[derive(Debug, PartialEq)]
enum EdgeAssignmentCandidates {
    Context,
    Edges(Vec<i64>),
}

// `None` means the record claims an edge operand but its proofs do not admit a
// candidate. `Context` means the recipe has no edge-assignment proof and the
// record only contributes topology context to its neighboring operands.
fn edge_group_assignment_candidates<'a>(
    selector_contexts: &[crate::records::topology::edge_recipe::DesignEdgeRecipeSelectorContext],
    reference_edge_sets: impl IntoIterator<Item = &'a [i64]>,
) -> Option<EdgeAssignmentCandidates> {
    let mut reference_edge_sets = reference_edge_sets
        .into_iter()
        .filter(|edges| !edges.is_empty());
    if !selector_contexts.is_empty() {
        return edge_assignment_candidates(selector_contexts, reference_edge_sets)
            .map(EdgeAssignmentCandidates::Edges);
    }
    let (Some(first), Some(second)) = (reference_edge_sets.next(), reference_edge_sets.next())
    else {
        return Some(EdgeAssignmentCandidates::Context);
    };
    let mut candidates = Vec::new();
    for &edge in first {
        candidates.push(edge);
    }
    candidates.retain(|candidate| second.contains(candidate));
    candidates.sort_unstable_by(Ord::cmp);
    candidates.dedup();
    (!candidates.is_empty()).then_some(EdgeAssignmentCandidates::Edges(candidates))
}

pub(super) fn radius_edge_group_candidates(
    operands: &[&DesignEdgeOperand],
    radius: f64,
) -> Option<Vec<i64>> {
    if operands.is_empty() || !radius.is_finite() || radius <= 0.0 {
        return None;
    }
    let tolerance = EPS_EDGE_RESOLVE_RADIUS_EDGE_GROUP_CANDIDATES_E9 * (1.0 + radius.abs());
    let mut chain = Vec::new();
    for operand in operands {
        if let Some(edge) = resolved_edge_operand(operand) {
            chain.push(edge);
        }
        for edge in operand
            .treatment_radius_candidates
            .iter()
            .filter(|candidate| (candidate.radius.get() - radius).abs() <= tolerance)
            .map(|candidate| candidate.edge_slot)
        {
            chain.push(edge);
        }
    }
    chain.sort_unstable_by(Ord::cmp);
    chain.dedup();
    if chain.is_empty() {
        return None;
    }
    for operand in operands {
        let has_radius_candidate = operand
            .treatment_radius_candidates
            .iter()
            .any(|candidate| (candidate.radius.get() - radius).abs() <= tolerance);
        if resolved_edge_operand(operand).is_none()
            && !has_radius_candidate
            && !operand.changed_boundary_edge_slots.is_empty()
        {
            return None;
        }
    }
    Some(chain)
}

/// Resolve a treatment group whose identity members have no exact slot. The
/// operation transition is the proof: every member identity carries the same
/// transition edge set, every edge in that set blends at the group radius,
/// every member's deleted predecessor edges are a nonempty subset of it, and
/// the members' union is the whole set. A single-member group also proves its
/// edge when the transition is the radius-qualified subset of its deleted
/// set: every radius candidate on the member lies in the transition, so no
/// deleted edge outside it carries competing treatment evidence.
fn transition_complete_edge_group_candidates(
    operands: &[&DesignEdgeOperand],
    identities: &[&DesignEdgeIdentityOperand],
    radius: f64,
) -> Option<Vec<i64>> {
    if operands.is_empty()
        || operands.len() != identities.len()
        || !radius.is_finite()
        || radius <= 0.0
        || identities.iter().any(|identity| {
            identity.resolved_edge_slot.is_some() || !identity.resolved_edge_slots.is_empty()
        })
    {
        return None;
    }
    let tolerance = EPS_EDGE_RESOLVE_RADIUS_EDGE_GROUP_CANDIDATES_E9 * (1.0 + radius.abs());
    let [first, rest @ ..] = identities else {
        return None;
    };
    let transition = sorted_transition_slots(&first.transition_edge_candidates);
    if transition.is_empty()
        || rest.iter().any(|identity| {
            sorted_transition_slots(&identity.transition_edge_candidates) != transition
                || identity.treatment_radius_candidates != first.treatment_radius_candidates
        })
    {
        return None;
    }
    let mut blended = first
        .treatment_radius_candidates
        .iter()
        .filter(|candidate| (candidate.radius.get() - radius).abs() <= tolerance)
        .map(|candidate| candidate.edge_slot)
        .collect::<Vec<_>>();
    blended.sort_unstable_by(Ord::cmp);
    blended.dedup();
    if blended != transition {
        return None;
    }
    let mut union = Vec::new();
    for operand in operands {
        if operand.deleted_boundary_edge_slots.is_empty() {
            return None;
        }
        if operands.len() != 1
            && operand
                .deleted_boundary_edge_slots
                .iter()
                .any(|edge| transition.binary_search(edge).is_err())
        {
            return None;
        }
        union.extend(operand.deleted_boundary_edge_slots.iter().copied());
    }
    union.sort_unstable_by(Ord::cmp);
    union.dedup();
    if union == transition {
        return Some(union);
    }
    if operands.len() == 1
        && transition
            .iter()
            .all(|edge| union.binary_search(edge).is_ok())
        && identities.iter().all(|identity| {
            identity
                .treatment_radius_candidates
                .iter()
                .all(|candidate| transition.binary_search(&candidate.edge_slot).is_ok())
        })
    {
        return Some(transition);
    }
    None
}

fn radius_edge_identity_group_candidates(
    operands: &[&DesignEdgeIdentityOperand],
    radius: f64,
) -> Option<Vec<i64>> {
    if operands.is_empty() || !radius.is_finite() || radius <= 0.0 {
        return None;
    }
    let tolerance =
        EPS_EDGE_RESOLVE_RADIUS_EDGE_IDENTITY_GROUP_CANDIDATES_E9 * (1.0 + radius.abs());
    // Radius candidates on identity operands describe the complete operation
    // transition, not one member. They prove a group only when that group has
    // one member. A multi-member group requires an exact contribution from
    // every identity; recipe-local radius candidates are handled separately.
    let use_transition_radius = operands.len() == 1;
    let mut chain = Vec::new();
    for operand in operands {
        let mut contribution = Vec::new();
        for edge in operand
            .resolved_edge_slot
            .iter()
            .copied()
            .chain(operand.resolved_edge_slots.iter().copied())
        {
            contribution.push(edge);
        }
        if use_transition_radius {
            for edge in operand
                .treatment_radius_candidates
                .iter()
                .filter(|candidate| (candidate.radius.get() - radius).abs() <= tolerance)
                .map(|candidate| candidate.edge_slot)
            {
                contribution.push(edge);
            }
        }
        if contribution.is_empty() {
            return None;
        }
        for edge in contribution {
            chain.push(edge);
        }
    }
    chain.sort_unstable_by(Ord::cmp);
    chain.dedup();
    (!chain.is_empty()).then_some(chain)
}

fn unique_edge_assignment(
    candidate_sets: &[EdgeAssignmentCandidates],
    ctx: &DecodeContext<'_>,
) -> Result<Option<Vec<i64>>, CodecError> {
    let mut edge_candidate_sets = Vec::new();
    for candidates in candidate_sets {
        let EdgeAssignmentCandidates::Edges(edges) = candidates else {
            continue;
        };
        let mut copied_edges = Vec::new();
        for edge in edges {
            copied_edges.push(*edge);
        }
        edge_candidate_sets.push(copied_edges);
    }
    unique_bipartite_assignment(&edge_candidate_sets, ctx)
}

fn edge_assignment_candidates<'a>(
    selector_contexts: &[crate::records::topology::edge_recipe::DesignEdgeRecipeSelectorContext],
    shared_edge_sets: impl IntoIterator<Item = &'a [i64]>,
) -> Option<Vec<i64>> {
    let slots = if !selector_contexts.is_empty()
        && selector_contexts
            .iter()
            .all(|selector| !selector.incidence_matching_edge_slots.is_empty())
    {
        SelectorSlots::Incidence
    } else {
        SelectorSlots::BoundaryCount
    };
    corroborated_edge_candidates(selector_contexts, shared_edge_sets, slots)
}

fn unique_bipartite_assignment(
    candidate_sets: &[Vec<i64>],
    ctx: &DecodeContext<'_>,
) -> Result<Option<Vec<i64>>, CodecError> {
    if candidate_sets.is_empty() {
        return Ok(None);
    }
    let mut normalized = ctx.alloc_filled(
        candidate_sets.len(),
        Vec::<i64>::new(),
        "f3d edge normalized groups",
    )?;
    for (source, candidates) in candidate_sets.iter().zip(&mut normalized) {
        *candidates = ctx.alloc_filled(source.len(), 0, "f3d edge normalized candidates")?;
        candidates.copy_from_slice(source);
        candidates.sort_unstable_by(Ord::cmp);
        candidates.dedup();
        if candidates.is_empty() {
            return Ok(None);
        }
    }
    let Some(assignment) = bipartite_assignment(&normalized, None, ctx)? else {
        return Ok(None);
    };
    for (member, edge) in assignment.iter().copied().enumerate() {
        if bipartite_assignment(&normalized, Some((member, edge)), ctx)?.is_some() {
            return Ok(None);
        }
    }
    Ok(Some(assignment))
}

fn bipartite_assignment(
    candidate_sets: &[Vec<i64>],
    forbidden: Option<(usize, i64)>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<Vec<i64>>, CodecError> {
    fn augment(
        member: usize,
        (candidate_sets, forbidden): (&[Vec<i64>], Option<(usize, i64)>),
        visited: &mut HashSet<i64>,
        edge_members: &mut HashMap<i64, usize>,
        ctx: &DecodeContext<'_>,
    ) -> Result<bool, CodecError> {
        let _depth = Some((ctx.enter_nested("f3d edge assignment search"))?);
        for edge in &candidate_sets[member] {
            if forbidden == Some((member, *edge)) || visited.contains(edge) {
                continue;
            }

            visited.insert(*edge);
            let displaced = edge_members.get(edge).copied();
            let assignable = match displaced {
                Some(displaced) => augment(
                    displaced,
                    (candidate_sets, forbidden),
                    visited,
                    edge_members,
                    ctx,
                )?,
                None => true,
            };
            if assignable {
                edge_members.insert(*edge, member);
                return Ok(true);
            }
        }
        Ok(false)
    }

    let mut edge_members = HashMap::new();
    for member in 0..candidate_sets.len() {
        let assigned = ({
            let mut visited = HashSet::new();
            augment(
                member,
                (candidate_sets, forbidden),
                &mut visited,
                &mut edge_members,
                ctx,
            )
        })?;
        if !assigned {
            return Ok(None);
        }
    }
    let mut assignment = ctx.alloc_filled(candidate_sets.len(), 0, "f3d edge assignment")?;
    for (edge, member) in edge_members {
        assignment[member] = edge;
    }
    Ok(Some(assignment))
}

/// Members of one construction operand group: `(identity, resolved edge slot,
/// deleted boundary edge slots)`.
#[derive(Clone)]
struct EdgeGroupMember<'a> {
    identity: u32,
    resolved_edge: Option<i64>,
    deleted_boundary_edges: &'a [i64],
}

fn scope_partition_edge_group_candidates<'a>(
    target: &DesignConstructionOperandGroup,
    groups: &'a [DesignConstructionOperandGroup],
    operands: &'a [DesignEdgeOperand],
    target_members: &[crate::records::identity::Located<u32>],
) -> Option<Vec<i64>> {
    let stream = native_stream(&target.id)?;
    let mut scope_groups = Vec::new();
    let mut target_ordinal = None;
    for group in groups.iter().filter(|group| {
        native_stream(&group.id) == Some(stream)
            && group.scope_record_index == target.scope_record_index
            && group.lost_edge_references.is_empty()
            && !(if group.id == target.id {
                target_members
            } else {
                group.members()
            })
            .is_empty()
    }) {
        let group_members = if group.id == target.id {
            target_members
        } else {
            group.members()
        };
        let mut members = Vec::new();
        let mut complete = true;
        for member in group_members.iter().map(|member| &member.value) {
            let mut matches = operands.iter().filter(|operand| {
                native_stream(&operand.id) == Some(stream)
                    && operand.scope_record_index == group.scope_record_index
                    && operand.record_index() == *member
            });
            let Some(operand) = matches.next().filter(|_| matches.next().is_none()) else {
                complete = false;
                break;
            };
            members.push(EdgeGroupMember {
                identity: operand.record_index(),
                resolved_edge: resolved_edge_operand(operand),
                deleted_boundary_edges: &operand.deleted_boundary_edge_slots,
            });
        }
        if !complete {
            continue;
        }
        if group.id == target.id {
            target_ordinal = Some(scope_groups.len());
        }
        scope_groups.push(members);
    }
    let target_ordinal = target_ordinal?;
    partition_unique_incomplete_edge_group(target_ordinal, &scope_groups)
}

fn partition_unique_incomplete_edge_group(
    target_ordinal: usize,
    groups: &[Vec<EdgeGroupMember<'_>>],
) -> Option<Vec<i64>> {
    if groups.len() < 2 || target_ordinal >= groups.len() {
        return None;
    }
    let mut identities = HashSet::new();
    let mut universe = None::<Vec<i64>>;
    for member in groups.iter().flatten() {
        if !identities.insert(member.identity) {
            return None;
        }
        let mut deleted = Vec::new();
        for edge in member.deleted_boundary_edges {
            deleted.push(*edge);
        }
        deleted.sort_unstable_by(Ord::cmp);
        deleted.dedup();
        if deleted.is_empty()
            || universe
                .as_ref()
                .is_some_and(|universe| *universe != deleted)
        {
            return None;
        }
        universe.get_or_insert(deleted);
    }
    let universe = universe?;
    if identities.len() != universe.len() {
        return None;
    }
    let mut incomplete = groups
        .iter()
        .enumerate()
        .filter(|(_, group)| group.iter().any(|member| member.resolved_edge.is_none()))
        .map(|(ordinal, _)| ordinal);
    if incomplete.next() != Some(target_ordinal) || incomplete.next().is_some() {
        return None;
    }
    let mut reserved = Vec::new();
    for (ordinal, group) in groups.iter().enumerate() {
        if ordinal == target_ordinal {
            continue;
        }
        for member in group {
            let resolved = member.resolved_edge.as_ref()?;
            if !universe.contains(resolved) || reserved.contains(resolved) {
                return None;
            }
            reserved.push(*resolved);
        }
    }
    let mut target = Vec::new();
    for candidate in universe
        .into_iter()
        .filter(|candidate| !reserved.contains(candidate))
    {
        target.push(candidate);
    }
    if target.len() != groups[target_ordinal].len()
        || groups[target_ordinal]
            .iter()
            .filter_map(|member| member.resolved_edge)
            .any(|resolved| !target.contains(&resolved))
    {
        return None;
    }
    Some(target)
}

fn common_deleted_edge_group_candidates<'a>(
    members: impl IntoIterator<Item = (bool, &'a [i64])>,
) -> Option<Vec<i64>> {
    let mut candidate_sets = members
        .into_iter()
        .filter_map(|(edge_bearing, candidates)| edge_bearing.then_some(candidates));
    let first = candidate_sets.next()?;
    let mut member_count = 1;
    let mut candidates = Vec::new();
    for candidate in first {
        candidates.push(*candidate);
    }
    candidates.sort_unstable_by(Ord::cmp);
    candidates.dedup();
    for candidate_set in candidate_sets {
        member_count += 1;
        let mut normalized = Vec::new();
        for candidate in candidate_set {
            normalized.push(*candidate);
        }
        normalized.sort_unstable_by(Ord::cmp);
        normalized.dedup();
        if normalized != candidates {
            return None;
        }
    }
    (candidates.len() == member_count).then_some(candidates)
}

/// Resolve a treatment group whose recipe members expose the complete deleted
/// predecessor-edge set, but do not expose one edge candidate per member.
///
/// The deleted set is an exact group proof only when every member has a full
/// structured recipe context, every deleted edge is a changed predecessor
/// boundary, each member contributes contextual evidence for at least one
/// deleted edge, and the group-wide set has one edge per member. The context
/// requirement excludes topology deletions that are visible only in the
/// feature transition; the cardinality requirement excludes a member recipe
/// that represents an edge chain rather than one selected edge.
fn deleted_boundary_edge_group_candidates(operands: &[&DesignEdgeOperand]) -> Option<Vec<i64>> {
    if operands.is_empty() {
        return None;
    }
    let mut deleted = Vec::new();
    let mut contextual = Vec::new();
    for operand in operands {
        if operand.recipe_structure.is_none()
            || operand.recipe_references.is_empty()
            || operand.recipe_references.len() != operand.recipe_reference_contexts.len()
            || operand.recipe_reference_contexts.is_empty()
            || operand.deleted_boundary_edge_slots.is_empty()
            || operand.deleted_boundary_edge_slots.iter().any(|edge| {
                !operand.preceding_boundary_edge_slots.contains(edge)
                    || !operand.changed_boundary_edge_slots.contains(edge)
            })
        {
            return None;
        }
        let mut member_contextual = false;
        for edge in operand
            .recipe_reference_contexts
            .iter()
            .flat_map(|context| context.changed_reference_edge_slots.iter().copied())
            .filter(|edge| operand.deleted_boundary_edge_slots.contains(edge))
        {
            member_contextual = true;
            contextual.push(edge);
        }
        if !member_contextual {
            return None;
        }
        for edge in &operand.deleted_boundary_edge_slots {
            deleted.push(*edge);
        }
    }
    deleted.sort_unstable_by(Ord::cmp);
    deleted.dedup();
    contextual.sort_unstable_by(Ord::cmp);
    contextual.dedup();
    (deleted.len() == operands.len()
        && deleted
            .iter()
            .all(|edge| contextual.binary_search(edge).is_ok()))
    .then_some(deleted)
}

/// Resolve a treatment group when the deleted predecessor-edge set is complete
/// at group level but one or more members lack an operation-level deletion.
///
/// A member is admitted only through a recipe context that names one of the
/// deleted predecessor edges. The contextual candidate sets must have exactly
/// one perfect assignment; otherwise the group remains native. This handles a
/// legacy group whose transition records consolidate one member's deletion
/// while retaining the member-to-edge relation in the recipe references.
fn contextual_deleted_edge_group_candidates(
    operands: &[&DesignEdgeOperand],
    ctx: &DecodeContext<'_>,
) -> Result<Option<Vec<i64>>, CodecError> {
    if operands.is_empty() {
        return Ok(None);
    }
    let mut deleted = Vec::new();
    let mut has_deleted_member = false;
    for operand in operands {
        if operand.recipe_structure.is_none()
            || operand.recipe_references.is_empty()
            || operand.recipe_references.len() != operand.recipe_reference_contexts.len()
            || operand.recipe_reference_contexts.is_empty()
        {
            return Ok(None);
        }
        for edge in &operand.deleted_boundary_edge_slots {
            if !operand.preceding_boundary_edge_slots.contains(edge)
                || !operand.changed_boundary_edge_slots.contains(edge)
            {
                return Ok(None);
            }
            deleted.push(*edge);
            has_deleted_member = true;
        }
    }
    if !has_deleted_member {
        return Ok(None);
    }
    deleted.sort_unstable_by(Ord::cmp);
    deleted.dedup();
    if deleted.len() != operands.len() {
        return Ok(None);
    }

    let mut candidate_sets = Vec::new();
    for operand in operands {
        let mut candidates = Vec::new();
        for edge in operand
            .recipe_reference_contexts
            .iter()
            .flat_map(|context| context.changed_reference_edge_slots.iter().copied())
            .filter(|edge| deleted.binary_search(edge).is_ok())
        {
            candidates.push(edge);
        }
        candidates.sort_unstable_by(Ord::cmp);
        candidates.dedup();
        candidate_sets.push(candidates);
    }
    let Some(mut assignment) = unique_bipartite_assignment(&candidate_sets, ctx)? else {
        return Ok(None);
    };
    assignment.sort_unstable_by(Ord::cmp);
    Ok(Some(assignment))
}

/// Resolve a legacy single-member treatment whose selected edge persists in
/// the result boundary instead of appearing in the operand boundary delta.
///
/// The zero-payload two-side recipe supplies support references but no direct
/// edge entry. A result-boundary edge named by at least two changed-reference
/// contexts is admitted only when it is the sole such edge and is absent from
/// the operand's preceding candidate boundary. This leaves deleted-edge and
/// ambiguous reference sets native.
fn result_boundary_reference_edge_group_candidates(
    operands: &[&DesignEdgeOperand],
) -> Option<Vec<i64>> {
    let [operand] = operands else {
        return None;
    };
    let structure = operand.recipe_structure.as_ref()?;
    if structure.root != 2
        || structure.sides.len() != 2
        || structure.sides.iter().any(|side| {
            side.scalars.len() != 2 || side.payload_prefix != [0] || !side.entries.is_empty()
        })
        || operand.recipe_references.is_empty()
        || operand.recipe_references.len() != operand.recipe_reference_contexts.len()
        || operand.recipe_reference_contexts.is_empty()
        || !operand.changed_boundary_edge_slots.is_empty()
        || !operand.deleted_boundary_edge_slots.is_empty()
    {
        return None;
    }
    let mut candidates = Vec::new();
    for edge in operand
        .recipe_reference_contexts
        .iter()
        .flat_map(|context| context.changed_reference_edge_slots.iter().copied())
        .filter(|edge| operand.result_boundary_edge_slots.contains(edge))
    {
        candidates.push(edge);
    }
    candidates.sort_unstable_by(Ord::cmp);
    candidates.dedup();
    let [candidate] = candidates.as_slice() else {
        return None;
    };
    if operand.preceding_boundary_edge_slots.contains(candidate)
        || operand
            .recipe_reference_contexts
            .iter()
            .filter(|context| context.changed_reference_edge_slots.contains(candidate))
            .count()
            < 2
    {
        return None;
    }
    Some(vec![*candidate])
}

fn changed_boundary_count_edge_group_candidates<'a>(
    members: impl IntoIterator<
        Item = &'a [crate::records::topology::edge_recipe::DesignEdgeRecipeSelectorContext],
    >,
) -> Option<Vec<i64>> {
    let mut member_count = 0;
    let mut candidates = Vec::new();
    for selectors in members {
        if selectors.is_empty() {
            return None;
        }
        member_count += 1;
        for edge in selectors
            .iter()
            .flat_map(|selector| selector.boundary_count_matching_edge_slots.iter().copied())
        {
            candidates.push(edge);
        }
    }
    candidates.sort_unstable_by(Ord::cmp);
    candidates.dedup();
    (member_count > 0 && candidates.len() == member_count).then_some(candidates)
}

pub(super) fn resolved_edge_operand(operand: &DesignEdgeOperand) -> Option<i64> {
    if !operand.recipe_program.is_empty() && operand.recipe_structure.is_none() {
        return None;
    }
    operand
        .resolved_edge_slot
        .or_else(|| primary_terminal_reference_shared_edge(operand))
}

/// Resolve the selected edge of a zero-payload terminal recipe from the two
/// support-face references in its primary side.
fn primary_terminal_reference_shared_edge(operand: &DesignEdgeOperand) -> Option<i64> {
    if !operand.recipe_reference_contexts.is_empty() {
        return None;
    }
    let structure = operand.recipe_structure.as_ref()?;
    if structure.root != 2
        || structure.sides.len() != 2
        || structure.sides.iter().any(|side| {
            side.scalars.len() != 2 || side.payload_prefix != [0] || !side.entries.is_empty()
        })
    {
        return None;
    }

    let primary = &structure.sides[0];
    let mut reference_ordinals = std::iter::once(primary.header_value)
        .chain(primary.scalars.iter().copied())
        .filter(|value| *value != 0)
        .map(|value| usize::try_from(value).ok()?.checked_sub(1));
    let (Some(first_ordinal), Some(second_ordinal)) =
        (reference_ordinals.next()?, reference_ordinals.next()?)
    else {
        return None;
    };
    if reference_ordinals.next().is_some() {
        return None;
    }
    if first_ordinal == second_ordinal {
        return None;
    }
    let first = operand.terminal_reference_edge_slots.get(first_ordinal)?;
    let second = operand.terminal_reference_edge_slots.get(second_ordinal)?;
    if first.is_empty() || second.is_empty() {
        return None;
    }

    let mut candidate = None;
    for edge in first.iter().copied().filter(|edge| second.contains(edge)) {
        if candidate.is_some_and(|selected| selected != edge) {
            return None;
        }
        candidate = Some(edge);
    }
    candidate.filter(|edge| operand.terminal_boundary_edge_slots.contains(edge))
}

pub(super) fn edge_operand_reference_edge_sets(
    operand: &DesignEdgeOperand,
) -> impl Iterator<Item = &[i64]> + '_ {
    let terminal = operand.recipe_reference_contexts.is_empty();
    let source_count = if terminal {
        operand.terminal_reference_edge_slots.len()
    } else {
        operand.recipe_reference_contexts.len()
    };
    let selected_count = operand
        .local_topology_references
        .as_ref()
        .map_or(source_count, Vec::len);
    (0..selected_count).filter_map(move |selected| {
        let source = match &operand.local_topology_references {
            Some(ordinals) => usize::try_from(ordinals.get(selected)?.get())
                .ok()?
                .checked_sub(1)?,
            None => selected,
        };
        if terminal {
            operand
                .terminal_reference_edge_slots
                .get(source)
                .map(Vec::as_slice)
        } else {
            operand
                .recipe_reference_contexts
                .get(source)
                .map(|context| context.changed_reference_edge_slots.as_slice())
        }
    })
}

pub(crate) fn resolved_edge_candidate_intersection<'a, I>(
    selector_contexts: &[crate::records::topology::edge_recipe::DesignEdgeRecipeSelectorContext],
    shared_edge_sets: I,
) -> Option<i64>
where
    I: IntoIterator<Item = &'a [i64]>,
    I::IntoIter: Clone,
{
    let ordered = shared_edge_sets.into_iter();
    let has_reference_sets = ordered.clone().next().is_some();
    let shared_edge_sets = ordered.filter(|edges| !edges.is_empty());
    let references_unavailable = has_reference_sets && shared_edge_sets.clone().next().is_none();
    let reference_candidates = (shared_edge_sets.clone().take(2).count() == 2)
        .then(|| unique_edge_set_intersection(&shared_edge_sets));
    if reference_candidates == Some(EdgeSetIntersection::Disjoint) {
        return None;
    }
    let reference = match reference_candidates {
        Some(EdgeSetIntersection::Unique(edge)) => Some(edge),
        _ => None,
    };
    let incidence = (!references_unavailable)
        .then(|| {
            corroborated_edge_intersection(
                selector_contexts,
                &shared_edge_sets,
                SelectorSlots::Incidence,
            )
        })
        .flatten();
    let boundary_count = (!references_unavailable)
        .then(|| {
            corroborated_edge_intersection(
                selector_contexts,
                &shared_edge_sets,
                SelectorSlots::BoundaryCount,
            )
        })
        .flatten();
    let common_triplet =
        corroborated_common_triplet_intersection(selector_contexts, &shared_edge_sets);
    let cross_clause_triplet =
        corroborated_cross_clause_triplet_intersection(selector_contexts, &shared_edge_sets);
    let mut proofs = [
        reference,
        incidence,
        boundary_count,
        common_triplet,
        cross_clause_triplet,
    ]
    .into_iter()
    .flatten();
    let edge = proofs.next()?;
    proofs.all(|proof| proof == edge).then_some(edge)
}

pub(crate) fn unique_incidence_edge_shared_by_reference_faces<'a, I>(
    selector_contexts: &[crate::records::topology::edge_recipe::DesignEdgeRecipeSelectorContext],
    reference_edge_sets: I,
) -> Option<i64>
where
    I: IntoIterator<Item = &'a [i64]>,
    I::IntoIter: Clone,
{
    let reference_edge_sets = reference_edge_sets.into_iter();
    let mut candidate = None;
    for edge in selector_contexts
        .iter()
        .flat_map(|selector| selector.incidence_matching_edge_slots.iter().copied())
    {
        if candidate == Some(edge) {
            continue;
        }
        let mut distinct_matches = 0;
        for (index, edges) in reference_edge_sets.clone().enumerate() {
            if !edges.contains(&edge)
                || reference_edge_sets.clone().take(index).any(|prior| {
                    prior.iter().all(|value| edges.contains(value))
                        && edges.iter().all(|value| prior.contains(value))
                })
            {
                continue;
            }
            distinct_matches += 1;
            if distinct_matches == 2 {
                break;
            }
        }
        if distinct_matches == 2 {
            if candidate.is_some() {
                return None;
            }
            candidate = Some(edge);
        }
    }
    candidate
}

fn corroborated_common_triplet_intersection<'a>(
    selector_contexts: &[crate::records::topology::edge_recipe::DesignEdgeRecipeSelectorContext],
    shared_edge_sets: &(impl Iterator<Item = &'a [i64]> + Clone),
) -> Option<i64> {
    let edge_pairs = selector_contexts.iter().flat_map(|selector| {
        selector.clauses.iter().flatten().filter_map(|clause| {
            clause.entry.common_incident_edge_ordinal()?;
            let [first, second] = &clause.triplet_edge_slots;
            Some((first.as_slice(), second.as_slice()))
        })
    });
    corroborated_edge_pair_intersection(edge_pairs, shared_edge_sets)
}

fn corroborated_cross_clause_triplet_intersection<'a>(
    selector_contexts: &[crate::records::topology::edge_recipe::DesignEdgeRecipeSelectorContext],
    shared_edge_sets: &(impl Iterator<Item = &'a [i64]> + Clone),
) -> Option<i64> {
    let edge_pairs = selector_contexts
        .iter()
        .filter_map(|selector| {
            let [Some(left), Some(right)] = selector.clauses.as_slice() else {
                return None;
            };
            Some(
                left.triplet_edge_slots
                    .iter()
                    .zip(&right.triplet_edge_slots)
                    .map(|(left, right)| (left.as_slice(), right.as_slice())),
            )
        })
        .flatten();
    corroborated_edge_pair_intersection(edge_pairs, shared_edge_sets)
}

fn corroborated_edge_pair_intersection<'a, 'b>(
    edge_pairs: impl Iterator<Item = (&'a [i64], &'a [i64])> + Clone,
    shared_edge_sets: &(impl Iterator<Item = &'b [i64]> + Clone),
) -> Option<i64> {
    let active_pairs =
        edge_pairs.filter(|(left, right)| left.iter().any(|edge| right.contains(edge)));
    let (first, second) = active_pairs.clone().next()?;
    let mut candidate = None;
    for &edge in first {
        if second.contains(&edge)
            && active_pairs
                .clone()
                .all(|(left, right)| left.contains(&edge) && right.contains(&edge))
            && shared_edge_sets.clone().all(|set| set.contains(&edge))
        {
            if candidate.is_some_and(|selected| selected != edge) {
                return None;
            }
            candidate = Some(edge);
        }
    }
    candidate
}

#[derive(Clone, Copy, PartialEq)]
enum EdgeSetIntersection {
    Disjoint,
    Unique(i64),
    Ambiguous,
}

fn unique_edge_set_intersection<'a>(
    edge_sets: &(impl Iterator<Item = &'a [i64]> + Clone),
) -> EdgeSetIntersection {
    let mut sets = edge_sets.clone();
    let Some(first) = sets.next() else {
        return EdgeSetIntersection::Disjoint;
    };
    let mut candidate = None;
    for &edge in first {
        if edge_sets.clone().all(|set| set.contains(&edge)) {
            if candidate.is_some_and(|selected| selected != edge) {
                return EdgeSetIntersection::Ambiguous;
            }
            candidate = Some(edge);
        }
    }
    candidate.map_or(EdgeSetIntersection::Disjoint, EdgeSetIntersection::Unique)
}

#[derive(Clone, Copy)]
enum SelectorSlots {
    Incidence,
    BoundaryCount,
}

fn corroborated_edge_intersection<'a>(
    selector_contexts: &[crate::records::topology::edge_recipe::DesignEdgeRecipeSelectorContext],
    shared_edge_sets: &(impl Iterator<Item = &'a [i64]> + Clone),
    slots: SelectorSlots,
) -> Option<i64> {
    let mut selectors = selector_contexts.iter();
    let first = selectors.next()?;
    let mut candidate = None;
    for &edge in selector_candidate_edges(first, slots) {
        if !selector_contexts
            .iter()
            .skip(1)
            .all(|selector| selector_candidate_edges(selector, slots).contains(&edge))
            || !shared_edge_sets.clone().all(|set| set.contains(&edge))
        {
            continue;
        }
        if candidate.is_some_and(|selected| selected != edge) {
            return None;
        }
        candidate = Some(edge);
    }
    candidate
}

fn corroborated_edge_candidates<'a>(
    selector_contexts: &[crate::records::topology::edge_recipe::DesignEdgeRecipeSelectorContext],
    shared_edge_sets: impl IntoIterator<Item = &'a [i64]>,
    slots: SelectorSlots,
) -> Option<Vec<i64>> {
    let mut selectors = selector_contexts.iter();
    let first_selector = selectors.next()?;
    let first = selector_candidate_edges(first_selector, slots);
    if first.is_empty() {
        return None;
    }
    let mut candidates = Vec::new();
    for &edge in first {
        candidates.push(edge);
    }
    candidates.sort_unstable_by(Ord::cmp);
    candidates.dedup();
    for selector in selectors {
        let selector_edges = selector_candidate_edges(selector, slots);
        if selector_edges.is_empty() {
            return None;
        }
        candidates.retain(|candidate| selector_edges.contains(candidate));
        if candidates.is_empty() {
            return None;
        }
    }
    for shared_edges in shared_edge_sets {
        candidates.retain(|candidate| shared_edges.contains(candidate));
        if candidates.is_empty() {
            return None;
        }
    }
    Some(candidates)
}

fn selector_candidate_edges(
    selector: &crate::records::topology::edge_recipe::DesignEdgeRecipeSelectorContext,
    slots: SelectorSlots,
) -> &[i64] {
    match slots {
        SelectorSlots::BoundaryCount => &selector.boundary_count_matching_edge_slots,
        SelectorSlots::Incidence => &selector.incidence_matching_edge_slots,
    }
}

#[cfg(test)]
fn project_fixed_fillet(
    scope: &DesignParameterScope,
    construction_groups: &[DesignConstructionOperandGroup],
    edge_operands: &[DesignEdgeOperand],
    edge_identity_operands: &[DesignEdgeIdentityOperand],
    ctx: &DecodeContext<'_>,
) -> Result<Option<cadmpeg_ir::features::FeatureDefinition>, CodecError> {
    project_fixed_fillet_with_corners(
        scope,
        construction_groups,
        edge_operands,
        edge_identity_operands,
        &[],
        &[],
        ctx,
    )
}

pub(super) fn project_fixed_fillet_with_corners(
    scope: &DesignParameterScope,
    construction_groups: &[DesignConstructionOperandGroup],
    edge_operands: &[DesignEdgeOperand],
    edge_identity_operands: &[DesignEdgeIdentityOperand],
    vertex_operands: &[DesignEdgeTreatmentVertexOperand],
    histories: &[crate::history_records::AsmHistory],
    ctx: &DecodeContext<'_>,
) -> Result<Option<cadmpeg_ir::features::FeatureDefinition>, CodecError> {
    use cadmpeg_ir::features::{
        edge_treatments::{FilletGroup, RadiusSpec, VariableRadius},
        FeatureDefinition, FeatureOperation,
    };
    use cadmpeg_ir::scalar::Length;

    let fixed = or_none!(scope.fixed_fillet_parameters());
    let stream = or_none!(native_stream(&scope.id));
    let radius_spec =
        |group: &crate::records::feature::fixed_parameters::DesignFixedFilletGroup| -> Result<Option<RadiusSpec>, CodecError> { Ok(match group.law() {
            crate::records::feature::fixed_parameters::DesignFixedFilletLaw::Constant(radius) => {
                let Some(radius) = cadmpeg_ir::scalar::PositiveLength::new(radius.value.get() * 10.0) else { return Ok(None); };
                Some(RadiusSpec::Constant {
                    radius,
                })
            }
            crate::records::feature::fixed_parameters::DesignFixedFilletLaw::Variable {
                start,
                end,
                intermediate,
            } => {
                let mut points = Vec::new();
                let Some(start_radius) = Length::new(start.value.get() * 10.0) else { return Ok(None); };
                points.push(VariableRadius {
                    parameter: 0.0,
                    radius: start_radius,
                });
                for row in intermediate {

                    let Some(radius) = Length::new(row.radius.value.get() * 10.0) else { return Ok(None); };
                    points.push(VariableRadius {
                        parameter: row.parameter.value.get(),
                        radius,
                    });
                }
                let Some(end_radius) = Length::new(end.value.get() * 10.0) else { return Ok(None); };
                points.push(VariableRadius {
                    parameter: 1.0,
                    radius: end_radius,
                });
                let Some(points) = cadmpeg_ir::features::edge_treatments::VariableRadii::new(points).ok() else { return Ok(None); };
                Some(RadiusSpec::Variable {
                    points,
                })
            }
        }) };
    let mut scope_groups = Vec::new();
    for group in construction_groups.iter().filter(|group| {
        native_stream(&group.id) == Some(stream)
            && group.scope_record_index == scope.record_index
            && !group.members().is_empty()
    }) {
        scope_groups.push(group);
    }
    scope_groups.sort_by(|left, right| {
        left.scope_reference_ordinal
            .cmp(&right.scope_reference_ordinal)
    });
    let mut complete_edge_groups = Vec::new();
    for group in scope_groups.iter().copied().filter(|group| {
        group
            .members()
            .iter()
            .map(|member| &member.value)
            .all(|member| {
                edge_operands.iter().any(|operand| {
                    native_stream(&operand.id) == Some(stream)
                        && operand.scope_record_index == scope.record_index
                        && operand.record_index() == *member
                })
            })
    }) {
        complete_edge_groups.push(group);
    }
    let selection_pair =
        crate::design::decode::operands::fillet_edge_and_face_groups(&scope_groups);
    let assignments = if let (Some(pair), [fixed_group]) = (selection_pair, fixed.groups.as_slice())
    {
        pair.map(|group| (fixed_group, group)).to_vec()
    } else if complete_edge_groups.len() == fixed.groups.len() {
        fixed.groups.iter().zip(complete_edge_groups).collect()
    } else if fixed.groups.len() == 1 && complete_edge_groups.is_empty() {
        let group = {
            // Support-face operands share the compact persistent-identity
            // prefix. A sole construction group cannot be a support group
            // alongside an unrepresented edge group: it is the Fillet's edge
            // selection when every member has one exact identity record and
            // the fixed radius selects a nonempty transition chain.
            let [group] = scope_groups.as_slice() else {
                return Ok(None);
            };
            let radius = radius_spec(&fixed.groups[0])?;
            let Some(RadiusSpec::Constant { radius }) = radius else {
                return Ok(None);
            };
            let mut identities = Vec::new();
            for member in group.members().iter().map(|member| &member.value) {
                let mut matches = edge_identity_operands.iter().filter(|operand| {
                    native_stream(&operand.id) == Some(stream)
                        && operand.scope_record_index == scope.record_index
                        && operand.group_record_index == group.record_index
                        && operand.record_index() == *member
                });
                let Some(operand) = matches.next().filter(|_| matches.next().is_none()) else {
                    return Ok(None);
                };
                if !operand.layout().is_compact() {
                    return Ok(None);
                }
                identities.push(operand);
            }
            or_none!(radius_edge_identity_group_candidates(
                &identities,
                radius.get()
            ));
            *group
        };
        vec![(&fixed.groups[0], group)]
    } else {
        return Ok(None);
    };
    let mut groups = Vec::new();
    for (fixed_group, edge_group) in assignments {
        let radius = or_none!(radius_spec(fixed_group)?);
        let edge_radius = match radius {
            RadiusSpec::Constant { radius } => Some(radius.get()),
            RadiusSpec::Chordal { .. }
            | RadiusSpec::Asymmetric { .. }
            | RadiusSpec::Variable { .. }
            | RadiusSpec::Unresolved { .. } => None,
        };
        let edges = resolved_edge_treatment_group_with_corners(
            edge_group,
            crate::design::edge_resolve::EdgeTreatmentInputs {
                groups: construction_groups,
                operands: edge_operands,
                identity_operands: edge_identity_operands,
                vertex_operands,
                histories,
                previous_state_id: scope.previous_history_state_id(),
                feature_id: &crate::ids::neutral_feature_id(scope),
                treatment_radius: edge_radius,
            },
            ctx,
        )?;
        groups.push(FilletGroup {
            edges,
            radius,
            tangency_weight: fixed_group.tangency_weight().map(|tangency| tangency.value),
        });
    }
    Ok(Some(FeatureDefinition::Operation(
        FeatureOperation::Fillet {
            groups: or_none!(groups.try_into().ok()),
        },
    )))
}

#[cfg(test)]
mod tests;
