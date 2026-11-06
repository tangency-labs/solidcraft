// SPDX-License-Identifier: Apache-2.0
//! Edges that a `Fillet` feature rounds, from the blend faces of its transition.

use super::blend_spine::{blend_spines, edge_geometry};
use crate::history_records::{AsmHistoricalTopology, AsmHistoricalTransition, AsmHistory};
use cadmpeg_ir::features::edge_treatments::RadiusSpec;
use cadmpeg_ir::features::{EdgeSelection, FeatureDefinition, FeatureOperation};
use std::collections::{BTreeSet, HashMap, HashSet};

/// Millimetres between a blend radius and the radius of its group.
const FILLET_BLEND_RADIUS_TOLERANCE: f64 = 1.0e-6;

/// One new blend face: its radius and the deleted edge along its spine.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Blend {
    radius: f64,
    edge: i64,
}

/// The blend faces that a transition inserts on new carriers, each with the
/// one deleted edge that it replaces, or `None` when a new carrier has no
/// spine or radius row, a blend has no replaced edge or more than one, or two
/// blends replace one edge.
fn transition_blends(
    transition: &AsmHistoricalTransition,
    input: &AsmHistoricalTopology,
    output: &AsmHistoricalTopology,
) -> Option<Vec<Blend>> {
    let inserted = &transition.topology.faces.inserted;
    let carriers = super::unique_face_carriers(output);
    let input_surfaces = input.surfaces.iter().copied().collect::<HashSet<_>>();
    let mut blend_faces = Vec::new();
    for face in inserted {
        let carrier = carriers.get(face).copied().flatten()?;
        if !input_surfaces.contains(&carrier) {
            blend_faces.push(*face);
        }
    }
    if blend_faces.is_empty() {
        return None;
    }
    let output_boundaries = super::face_boundary_edge_index(output);
    let input_boundaries = super::face_boundary_edge_index(input);
    let supports = super::treatment_face_supports(&blend_faces, output, input, &output_boundaries);
    if supports.len() != blend_faces.len() {
        return None;
    }
    let spines = blend_spines(output);
    let mut radii = HashMap::<i64, Option<f64>>::new();
    for row in &output.surface_radii {
        radii
            .entry(row.surface)
            .and_modify(|radius| *radius = None)
            .or_insert(Some(row.radius));
    }
    let edges = edge_geometry(input);
    let deleted = transition
        .topology
        .edges
        .deleted
        .iter()
        .copied()
        .collect::<HashSet<_>>();
    let mut blends = Vec::new();
    let mut replaced = HashSet::new();
    for (_, carrier, supports) in supports {
        let spine = spines.get(&carrier)?;
        let radius = radii.get(&carrier).copied().flatten()?;
        let mut candidates = BTreeSet::new();
        for (ordinal, left) in supports.iter().enumerate() {
            let Some(left_edges) = input_boundaries.get(left) else {
                continue;
            };
            for right in &supports[ordinal + 1..] {
                let Some(right_edges) = input_boundaries.get(right) else {
                    continue;
                };
                candidates.extend(
                    left_edges
                        .intersection(right_edges)
                        .copied()
                        .filter(|edge| {
                            deleted.contains(edge)
                                && spine.admits(edges.get(edge).copied().unwrap_or_default())
                        }),
                );
            }
        }
        let mut candidates = candidates.into_iter();
        let (Some(edge), None) = (candidates.next(), candidates.next()) else {
            return None;
        };
        if !replaced.insert(edge) {
            return None;
        }
        blends.push(Blend { radius, edge });
    }
    Some(blends)
}

/// The edges each group rounds, sorted, by the constant radius of its blends,
/// or `None` when a blend matches no group radius or more than one.
fn group_edges(radii: &[Option<f64>], blends: &[Blend]) -> Option<Vec<Vec<i64>>> {
    let mut edges = vec![Vec::new(); radii.len()];
    for blend in blends {
        let mut owners = radii.iter().enumerate().filter(|(_, radius)| {
            radius.is_some_and(|radius| {
                (radius - blend.radius).abs() <= FILLET_BLEND_RADIUS_TOLERANCE
            })
        });
        let (Some((owner, _)), None) = (owners.next(), owners.next()) else {
            return None;
        };
        edges.get_mut(owner)?.push(blend.edge);
    }
    for group in &mut edges {
        group.sort_unstable();
    }
    Some(edges)
}

fn native_group_id(selection: &EdgeSelection) -> Option<&str> {
    match selection {
        EdgeSelection::Native(native) => Some(native.as_str()),
        EdgeSelection::Historical { native, .. }
        | EdgeSelection::HistoricalPartial { native, .. } => Some(native.as_str()),
        _ => None,
    }
}

/// Whether a selection names an edge of the input state that the transition
/// keeps.
fn names_kept_edge(
    selection: &EdgeSelection,
    feature_id: &cadmpeg_ir::features::FeatureId,
    previous_state_id: i64,
    deleted: &HashSet<i64>,
) -> bool {
    let edges = match selection {
        EdgeSelection::Historical { edges, .. } => edges.as_slice(),
        EdgeSelection::HistoricalPartial { edges, .. } => edges.as_slice(),
        _ => return false,
    };
    edges.iter().any(|edge| {
        crate::ids::feature_input_edge_slot(feature_id, previous_state_id, edge)
            .is_none_or(|slot| !deleted.contains(&slot))
    })
}

/// Bind each `Fillet` group whose blends replace one edge per distinct group
/// member to those edges.
///
/// A constant-radius fillet replaces each edge that it rounds with a blend
/// face at its group's radius. When every new carrier of the transition is a
/// blend whose spine names one deleted edge, those edges hold every rounded
/// edge, so a group whose blends name one edge per distinct member selects
/// exactly those edges. A selection that names an edge the transition keeps
/// stays, because DR-224 asks whether a rounded edge can keep its slot.
pub(super) fn bind_fillet_groups(
    definition: &mut FeatureDefinition,
    feature_id: &cadmpeg_ir::features::FeatureId,
    scope: &crate::records::feature::scope::DesignParameterScope,
    groups: &[crate::records::topology::construction::DesignConstructionOperandGroup],
    previous_state_id: i64,
    states: (
        &AsmHistoricalTransition,
        &AsmHistoricalTopology,
        &AsmHistoricalTopology,
    ),
) {
    let FeatureDefinition::Operation(FeatureOperation::Fillet {
        groups: fillet_groups,
    }) = definition
    else {
        return;
    };
    let (transition, input, output) = states;
    let Some(blends) = transition_blends(transition, input, output) else {
        return;
    };
    let radii = fillet_groups
        .iter()
        .map(|group| match group.radius {
            RadiusSpec::Constant { radius } => Some(radius.get()),
            _ => None,
        })
        .collect::<Vec<_>>();
    let Some(edges) = group_edges(&radii, &blends) else {
        return;
    };
    let deleted = transition
        .topology
        .edges
        .deleted
        .iter()
        .copied()
        .collect::<HashSet<_>>();
    let stream = crate::ids::native_stream(&scope.id);
    for (fillet_group, slots) in fillet_groups.iter_mut().zip(edges) {
        if names_kept_edge(&fillet_group.edges, feature_id, previous_state_id, &deleted) {
            continue;
        }
        let Some(native) = native_group_id(&fillet_group.edges).map(str::to_owned) else {
            continue;
        };
        let mut matching = groups.iter().filter(|group| {
            group.id == native
                && group.scope_record_index == scope.record_index
                && crate::ids::native_stream(&group.id) == stream
        });
        let (Some(group), None) = (matching.next(), matching.next()) else {
            continue;
        };
        let members = group
            .members()
            .iter()
            .map(|member| member.value)
            .collect::<HashSet<_>>();
        if slots.is_empty() || slots.len() != members.len() {
            continue;
        }
        let prefix = crate::ids::feature_input_prefix(feature_id, previous_state_id);
        let selected = slots
            .into_iter()
            .map(|slot| crate::ids::history_input_edge_id(&prefix, slot))
            .collect();
        if let Ok(historical) = EdgeSelection::historical(
            crate::ids::feature_input_topology_id(feature_id, previous_state_id),
            selected,
            native,
        ) {
            fillet_group.edges = historical;
        }
    }
}

/// Design records that `Fillet` binding reads.
pub(crate) struct FilletBlendInputs<'a> {
    pub(crate) scopes: &'a [crate::records::feature::scope::DesignParameterScope],
    pub(crate) groups:
        &'a [crate::records::topology::construction::DesignConstructionOperandGroup],
    pub(crate) histories: &'a mut [AsmHistory],
}

/// Bind the edges of each `Fillet` feature from the blend faces of its
/// transition.
pub(crate) fn bind_fillet_blend_edges(
    features: &mut [cadmpeg_ir::features::Feature],
    input: FilletBlendInputs<'_>,
) {
    let FilletBlendInputs {
        scopes,
        groups,
        histories,
    } = input;
    let states = features
        .iter()
        .map(|feature| {
            matches!(
                feature.evaluation.definition(),
                FeatureDefinition::Operation(FeatureOperation::Fillet { .. })
            )
            .then(|| super::face_selection_state(feature, scopes, histories))
            .flatten()
        })
        .collect::<Vec<_>>();
    let gates = states
        .iter()
        .enumerate()
        .filter_map(|(position, state)| {
            state.map(|state| (state.history, state.previous, position))
        })
        .collect::<Vec<_>>();
    let mut available = HashSet::new();
    super::visit_states_oldest_first(histories, gates, |history, previous, position| {
        if history.topology(previous).is_some() {
            available.insert(position);
        }
    });
    let histories = &*histories;
    for (position, (feature, state)) in features.iter_mut().zip(states).enumerate() {
        let Some(state) = state.filter(|_| available.contains(&position)) else {
            continue;
        };
        let Some(history) = histories.get(state.history) else {
            continue;
        };
        let (Some(output_state), Some(input_state)) = (
            history.states().get(state.state),
            history.states().get(state.previous),
        ) else {
            continue;
        };
        let (Some(transition), Some(input), Some(output)) = (
            output_state.transition.as_ref(),
            history.topology(input_state),
            history.topology(output_state),
        ) else {
            continue;
        };
        let feature_id = feature.id.clone();
        feature.evaluation.edit(|definition, _| {
            bind_fillet_groups(
                definition,
                &feature_id,
                state.scope,
                groups,
                state.previous_state_id,
                (transition, input, output),
            );
        });
    }
}

#[cfg(test)]
mod tests;
