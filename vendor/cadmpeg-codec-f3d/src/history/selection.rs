// SPDX-License-Identifier: Apache-2.0
//! Historical feature selection and persistent identity resolution.

use super::{
    bound_scope_history, design_axis, effective_scope_previous_history_state_id,
    historical_edge_axis, history_state_reaches, projection_was_finalized, same_axis_line,
    stable_ref, treatment_edge_candidates, unique_history_state_pair,
    EPS_HISTORY_BIND_MIRROR_SELECTION_PLANES_E9, EPS_HISTORY_HISTORICAL_LOOP_PLANE_E9,
    EPS_HISTORY_MIRROR_PLANES_COINCIDENT_E8, EPS_HISTORY_MIRROR_PLANES_COINCIDENT_E9,
};
use crate::history_records::{
    AsmHistoricalCoedge, AsmHistoricalTopology, AsmHistory, AsmTopologyScope,
};
use crate::records::{
    bodies::DesignBodyBinding,
    recipes::DesignComponentNamingSpace,
    topology::{
        body_recipe::AsmHistoricalEntityKind, edge_identity::DesignEdgeIdentityOperand,
        extrude_selection::DesignExtrudeSelectionMember, extrude_selection::DesignOperandRole,
    },
};
use std::collections::{BTreeSet, HashMap, HashSet};

pub(super) fn boundary_edges_in_changes(boundary_edges: &[i64], changes: &[i64]) -> Vec<i64> {
    boundary_edges
        .iter()
        .copied()
        .filter(|edge| changes.contains(edge))
        .collect::<Vec<_>>()
}

pub(super) fn recipe_selector_candidates(
    structure: Option<&crate::records::topology::edge_recipe::DesignEdgeRecipeStructure>,
    contexts: &[crate::records::topology::historical_context::DesignHistoricalEdgeContext],
) -> Vec<crate::records::topology::edge_recipe::DesignEdgeRecipeSelectorContext> {
    let Some(structure) = structure else {
        return Vec::new();
    };
    let mut selectors = BTreeSet::new();
    for selector in structure
        .sides
        .iter()
        .flat_map(|side| side.entries.iter().map(|entry| entry.selector))
    {
        selectors.insert(selector);
    }
    let mut selected = Vec::new();
    for selector in selectors {
        let mut clauses = Vec::new();
        for side in &structure.sides {
            let clause = if let Some(entry) =
                side.entries.iter().find(|entry| entry.selector == selector)
            {
                let [first, second] = entry.topology_triplets.each_ref().map(|triplet| {
                    contexts
                        .iter()
                        .filter(|context| {
                            context.incident_loops.iter().any(|incident| {
                                incident.boundary_edge_count == entry.boundary_edge_count.get()
                                    && triplet
                                        .incident
                                        .map(|incident| incident.ordinal)
                                        .is_some_and(|ordinal| incident.coedge_ordinal == ordinal)
                            })
                        })
                        .map(|context| context.edge_slot)
                        .collect::<Vec<_>>()
                });
                Some(
                    crate::records::topology::edge_recipe::DesignEdgeRecipeSelectorClause {
                        entry: *entry,
                        triplet_edge_slots: [first, second],
                    },
                )
            } else {
                None
            };

            clauses.push(clause);
        }
        let required = clauses
            .iter()
            .map(|entry| {
                entry
                    .as_ref()
                    .map(|entry| i64::from(entry.entry.boundary_edge_count.get()))
            })
            .collect::<Vec<_>>();
        let mut boundary_count_matching_edge_slots = Vec::new();
        for context in contexts {
            let counts = context
                .incident_loops
                .iter()
                .map(|incident| i64::from(incident.boundary_edge_count))
                .collect::<Vec<_>>();
            if incident_loop_counts_satisfy_sides(&counts, &required) {
                boundary_count_matching_edge_slots.push(context.edge_slot);
            }
        }
        let incidence_matching_edge_slots = contexts
            .iter()
            .filter(|context| {
                clauses.iter().flatten().all(|clause| {
                    let entry = &clause.entry;
                    entry.topology_triplets.iter().all(|triplet| {
                        context.incident_loops.iter().any(|incident| {
                            incident.boundary_edge_count == entry.boundary_edge_count.get()
                                && triplet
                                    .incident
                                    .map(|incident| incident.ordinal)
                                    .is_some_and(|ordinal| incident.coedge_ordinal == ordinal)
                        })
                    })
                })
            })
            .map(|context| context.edge_slot)
            .collect::<Vec<_>>();

        selected.push(
            crate::records::topology::edge_recipe::DesignEdgeRecipeSelectorContext {
                selector,
                clauses,
                incidence_matching_edge_slots,
                boundary_count_matching_edge_slots,
            },
        );
    }
    selected
}

pub(super) fn historical_edge_context(
    edge: i64,
    topology: &AsmHistoricalTopology,
) -> crate::records::topology::historical_context::DesignHistoricalEdgeContext {
    edge_context(edge, topology)
}

/// The rows an edge context reads. Each lookup returns the row a forward
/// scan of the topology finds first.
trait EdgeContextRows {
    fn incident_coedges(&self, edge: i64) -> impl Iterator<Item = &AsmHistoricalCoedge>;
    fn loop_members(&self, loop_slot: i64) -> Option<&[i64]>;
    fn coedge_edge(&self, coedge: i64) -> Option<i64>;
    fn loop_face(&self, loop_slot: i64) -> Option<i64>;
}

impl EdgeContextRows for AsmHistoricalTopology {
    fn incident_coedges(&self, edge: i64) -> impl Iterator<Item = &AsmHistoricalCoedge> {
        self.coedge_topology
            .iter()
            .filter(move |coedge| coedge.edge == edge)
    }

    fn loop_members(&self, loop_slot: i64) -> Option<&[i64]> {
        self.loop_coedges
            .iter()
            .find(|relation| relation.owner_ref == loop_slot)
            .map(|relation| relation.member_refs.as_slice())
    }

    fn coedge_edge(&self, coedge: i64) -> Option<i64> {
        self.coedge_topology
            .iter()
            .find(|candidate| candidate.coedge == coedge)
            .map(|candidate| candidate.edge)
    }

    fn loop_face(&self, loop_slot: i64) -> Option<i64> {
        self.face_loops
            .iter()
            .find(|relation| relation.member_refs.contains(&loop_slot))
            .map(|relation| relation.owner_ref)
    }
}

/// Edge contexts of one topology, indexed for passes that query many edges.
pub(super) struct EdgeContextIndex<'a> {
    incident: HashMap<i64, Vec<&'a AsmHistoricalCoedge>>,
    loops: HashMap<i64, &'a [i64]>,
    edges: HashMap<i64, i64>,
    faces: HashMap<i64, i64>,
}

impl<'a> EdgeContextIndex<'a> {
    pub(super) fn new(topology: &'a AsmHistoricalTopology) -> Self {
        let mut incident = HashMap::<_, Vec<_>>::new();
        let mut edges = HashMap::new();
        for coedge in &topology.coedge_topology {
            incident.entry(coedge.edge).or_default().push(coedge);
            edges.entry(coedge.coedge).or_insert(coedge.edge);
        }
        let mut loops = HashMap::new();
        for relation in &topology.loop_coedges {
            loops
                .entry(relation.owner_ref)
                .or_insert(relation.member_refs.as_slice());
        }
        let mut faces = HashMap::new();
        for relation in &topology.face_loops {
            for &loop_slot in &relation.member_refs {
                faces.entry(loop_slot).or_insert(relation.owner_ref);
            }
        }
        Self {
            incident,
            loops,
            edges,
            faces,
        }
    }

    pub(super) fn context(
        &self,
        edge: i64,
    ) -> crate::records::topology::historical_context::DesignHistoricalEdgeContext {
        edge_context(edge, self)
    }
}

impl EdgeContextRows for EdgeContextIndex<'_> {
    fn incident_coedges(&self, edge: i64) -> impl Iterator<Item = &AsmHistoricalCoedge> {
        self.incident.get(&edge).into_iter().flatten().copied()
    }

    fn loop_members(&self, loop_slot: i64) -> Option<&[i64]> {
        self.loops.get(&loop_slot).copied()
    }

    fn coedge_edge(&self, coedge: i64) -> Option<i64> {
        self.edges.get(&coedge).copied()
    }

    fn loop_face(&self, loop_slot: i64) -> Option<i64> {
        self.faces.get(&loop_slot).copied()
    }
}

fn edge_context(
    edge: i64,
    rows: &impl EdgeContextRows,
) -> crate::records::topology::historical_context::DesignHistoricalEdgeContext {
    let mut incident_loops = rows
        .incident_coedges(edge)
        .filter_map(|coedge| {
            let members = rows.loop_members(coedge.owner_loop)?;
            let ordinal = members
                .iter()
                .position(|candidate| *candidate == coedge.coedge)?;
            let boundary_edge_count = u32::try_from(members.len()).ok()?;
            let coedge_ordinal = u32::try_from(ordinal).ok()?;
            let previous_coedge = members.get((ordinal + members.len() - 1) % members.len())?;
            let next_coedge = members.get((ordinal + 1) % members.len())?;
            let face_slot = rows.loop_face(coedge.owner_loop)?;
            Some(
                crate::records::topology::historical_context::DesignHistoricalEdgeLoopContext {
                    coedge_slot: coedge.coedge,
                    loop_slot: coedge.owner_loop,
                    face_slot,
                    boundary_edge_count,
                    coedge_ordinal,
                    previous_edge_slot: rows.coedge_edge(*previous_coedge)?,
                    next_edge_slot: rows.coedge_edge(*next_coedge)?,
                },
            )
        })
        .collect::<Vec<_>>();
    incident_loops.sort_by_key(|left| left.coedge_slot);
    crate::records::topology::historical_context::DesignHistoricalEdgeContext {
        edge_slot: edge,
        incident_loops,
    }
}

pub(super) fn incident_loop_counts_satisfy_sides(counts: &[i64], required: &[Option<i64>]) -> bool {
    required.iter().enumerate().all(|(ordinal, value)| {
        let Some(value) = value else {
            return true;
        };
        let available = counts.iter().filter(|count| *count == value).count();
        let needed = required[..=ordinal]
            .iter()
            .filter(|candidate| *candidate == &Some(*value))
            .count();
        available >= needed
    })
}

pub(super) fn bind_face_selection(
    selection: &mut cadmpeg_ir::features::FaceSelection,
    scope: &crate::records::feature::scope::DesignParameterScope,
    groups: &[crate::records::topology::construction::DesignConstructionOperandGroup],
    operands: &[crate::records::topology::face::DesignFaceOperand],
    updated_face_slots: &[i64],
) {
    let cadmpeg_ir::features::FaceSelection::Native(native) = selection else {
        return;
    };
    if native == &scope.id {
        if let Some(resolved) =
            crate::design::feature_project::direct_face_selection(scope, operands)
        {
            if !matches!(resolved, cadmpeg_ir::features::FaceSelection::Native(_)) {
                *selection = resolved;
            }
        }
        return;
    }
    let mut matching_groups = groups.iter().filter(|group| group.id == *native);
    let Some(group) = matching_groups.next() else {
        return;
    };
    if matching_groups.next().is_some()
        || group.scope_record_index != scope.record_index
        || crate::ids::native_stream(&group.id) != crate::ids::native_stream(&scope.id)
    {
        return;
    }
    if scope.kind() == crate::records::feature::scope::DesignFeatureKind::DeleteFace {
        if let Some(resolved) = crate::design::face_resolve::resolved_historical_face_group(
            scope,
            scope.previous_history_state_id(),
            group,
            operands,
        ) {
            *selection = resolved;
        }
        return;
    }
    if let Some(resolved) =
        crate::design::face_resolve::resolved_historical_split_face_target_group_with_updated_faces(
            scope,
            scope.previous_history_state_id(),
            group,
            operands,
            updated_face_slots,
        )
    {
        *selection = resolved;
        return;
    }
    let Some(stream) = crate::ids::native_stream(&scope.id) else {
        return;
    };
    let mut faces = Vec::new();
    for record_index in group.members().iter().map(|member| &member.value) {
        let mut matches = operands.iter().filter(|operand| {
            crate::ids::native_stream(&operand.id) == Some(stream)
                && operand.scope_record_index == scope.record_index
                && operand.record_index() == *record_index
        });
        let Some(operand) = matches.next() else {
            return;
        };
        if matches.next().is_some() {
            return;
        }
        let previous_candidates = &operand.preceding_candidate_faces;
        let candidate = match previous_candidates.as_slice() {
            [face] => face,
            _ => {
                let [face] = operand.changed_candidate_faces.as_slice() else {
                    return;
                };
                face
            }
        };
        if faces.contains(candidate) {
            continue;
        }
        if !operand.candidate_faces.contains(candidate) {
            return;
        }

        faces.push(candidate.clone());
    }
    if !faces.is_empty() {
        *selection = cadmpeg_ir::features::FaceSelection::Resolved {
            faces,
            native: native.to_owned(),
        };
    }
}

pub(super) fn bind_body_recipe_face_selection(
    selection: &mut cadmpeg_ir::features::FaceSelection,
    feature_id: &cadmpeg_ir::features::FeatureId,
    previous_state_id: i64,
    scope: &crate::records::feature::scope::DesignParameterScope,
    groups: &[crate::records::topology::construction::DesignConstructionOperandGroup],
    operands: &[crate::records::topology::body_recipe::DesignBodyRecipeOperand],
) {
    use cadmpeg_ir::features::FaceSelection;

    let FaceSelection::Native(native) = selection else {
        return;
    };
    let mut matching_groups = groups.iter().filter(|group| {
        group.id == *native
            && group.scope_record_index == scope.record_index
            && group.role() == DesignOperandRole::ROLE_0X5
            && crate::ids::native_stream(&group.id) == crate::ids::native_stream(&scope.id)
    });
    let Some(group) = matching_groups.next() else {
        return;
    };
    if matching_groups.next().is_some() || group.members().is_empty() {
        return;
    }
    let stream = crate::ids::native_stream(&scope.id);
    let mut slots = Vec::new();
    for (ordinal, record_index) in group
        .members()
        .iter()
        .map(|member| &member.value)
        .enumerate()
    {
        let Ok(ordinal) = u32::try_from(ordinal) else {
            return;
        };
        let mut matching_operands = operands.iter().filter(|operand| {
            operand.owner.group() == Some((group.record_index, ordinal))
                && operand.record_index() == *record_index
                && crate::ids::native_stream(&operand.id) == stream
        });
        let Some(operand) = matching_operands.next() else {
            return;
        };
        if matching_operands.next().is_some() {
            return;
        }
        let Some(slot) = operand.resolved_face_slot else {
            return;
        };
        if !slots.contains(&slot) {
            slots.push(slot);
        }
    }
    let prefix = crate::ids::feature_input_prefix(feature_id, previous_state_id);
    let state = crate::ids::history_input_state_id(&prefix);
    let mut faces = Vec::new();
    for slot in slots {
        let face = crate::ids::history_input_face_id(&prefix, slot);

        faces.push(face);
    }
    let native = native.to_owned();
    if let Ok(historical) = cadmpeg_ir::features::FaceSelection::historical(state, faces, native) {
        *selection = historical;
    }
}

/// Membership in one topology family. Replayed families are in slot order and
/// are searched in place; any other family is hashed.
pub(super) enum FamilySlots<'a> {
    Sorted(&'a [i64]),
    Hashed(HashSet<i64>),
}

impl<'a> FamilySlots<'a> {
    pub(super) fn new(family: &'a [i64]) -> Self {
        if family.is_sorted() {
            Self::Sorted(family)
        } else {
            Self::Hashed(family.iter().copied().collect())
        }
    }

    pub(super) fn contains(&self, slot: i64) -> bool {
        match self {
            Self::Sorted(family) => family.binary_search(&slot).is_ok(),
            Self::Hashed(family) => family.contains(&slot),
        }
    }
}

pub(super) fn faces_in_topology<'a>(
    candidates: impl IntoIterator<Item = &'a cadmpeg_ir::ids::FaceId>,
    topology: &AsmHistoricalTopology,
) -> Vec<cadmpeg_ir::ids::FaceId> {
    let faces = FamilySlots::new(&topology.faces);
    candidates
        .into_iter()
        .filter(|face| stable_ref(face.as_str()).is_some_and(|slot| faces.contains(slot)))
        .cloned()
        .collect()
}

/// Topology families and states containing one requested stable ASM slot.
#[derive(Default)]
struct HistoricalIdentityMembership {
    kinds: HashSet<AsmHistoricalEntityKind>,
    states: Vec<i64>,
}

#[derive(Default)]
struct HistoricalRevisionMembership {
    entity_refs: HashSet<i64>,
    states: Vec<i64>,
}

/// Ambiguity-aware ASM identity data restricted to the Design identities a
/// binding pass requests.
pub(super) struct HistoricalIdentityIndex {
    identities: HashMap<i64, HistoricalIdentityMembership>,
    revisions: HashMap<i64, HistoricalRevisionMembership>,
}

impl HistoricalIdentityIndex {
    pub(super) fn build<'a>(
        histories: impl IntoIterator<Item = &'a AsmHistory>,
        local_ids: impl IntoIterator<Item = u64>,
    ) -> Self {
        let histories = histories.into_iter().collect::<Vec<_>>();
        let mut record_refs = HashSet::new();
        for record_ref in local_ids
            .into_iter()
            .filter_map(|local_id| i64::try_from(local_id).ok())
        {
            record_refs.insert(record_ref);
        }
        let mut identities = HashMap::<i64, HistoricalIdentityMembership>::new();
        let mut revisions = HashMap::<i64, HistoricalRevisionMembership>::new();
        if record_refs.is_empty() {
            return Self {
                identities,
                revisions,
            };
        }
        if histories
            .iter()
            .any(|history| history.binding_budget_exceeded)
        {
            return Self {
                identities,
                revisions,
            };
        }
        let ambiguous_states = ambiguous_history_state_ids(&histories);
        let mut queried_entities = record_refs.clone();
        for state in histories.iter().flat_map(|history| history.states()) {
            if let Some(transition) = &state.transition {
                queried_entities.extend(
                    transition
                        .record_revisions
                        .iter()
                        .filter(|version| record_refs.contains(&version.record_ref))
                        .map(|version| version.entity_ref),
                );
            }
            queried_entities.extend(
                state
                    .bulletin_boards
                    .iter()
                    .flat_map(|board| &board.changes)
                    .filter(|change| {
                        change
                            .old_ref()
                            .is_some_and(|old| record_refs.contains(&old))
                    })
                    .filter_map(|change| change.new_ref().or_else(|| change.old_ref())),
            );
        }
        let memberships: Vec<_> = histories
            .iter()
            .map(|history| history.identity_memberships(&record_refs, &queried_entities))
            .collect();
        for (history, query) in histories.iter().zip(&memberships) {
            for (index, version) in &query.revisions {
                let state = &history.states()[*index];
                if ambiguous_states.contains(&state.state_id) {
                    continue;
                }
                let membership = revisions.entry(version.record_ref).or_default();
                membership.entity_refs.insert(version.entity_ref);
                if !membership.states.contains(&state.state_id) {
                    membership.states.push(state.state_id);
                }
            }
        }
        let mut versioned_revisions = HashSet::new();
        for revision in revisions.keys() {
            versioned_revisions.insert(*revision);
        }
        let mut reconstructed_revisions = HashSet::new();
        // Finalized histories retain the entity-slot membership and the
        // bulletin-board chain after their geometry caches are compacted.
        for (history, _) in histories
            .iter()
            .zip(&memberships)
            .filter(|(history, query)| {
                let states = history.states();
                !states.is_empty()
                    && states.iter().all(|state| state.transition.is_some())
                    && match history.topology_scope {
                        AsmTopologyScope::Complete => true,
                        AsmTopologyScope::Retained => query.all_states_available,
                    }
            })
        {
            for change in history
                .states()
                .iter()
                .flat_map(|state| &state.bulletin_boards)
                .flat_map(|board| &board.changes)
            {
                let Some(record_ref) = change.old_ref().filter(|old| record_refs.contains(old))
                else {
                    continue;
                };
                let entity_ref = change.new_ref().unwrap_or(record_ref);
                let revision = revisions.entry(record_ref).or_default();
                revision.entity_refs.insert(entity_ref);
                if !versioned_revisions.contains(&record_ref) {
                    reconstructed_revisions.insert(record_ref);
                }
            }
        }
        let mut entity_refs = HashSet::new();
        for entity_ref in record_refs.iter().copied().chain(
            revisions
                .values()
                .flat_map(|revision| revision.entity_refs.iter().copied()),
        ) {
            entity_refs.insert(entity_ref);
        }
        for (history, query) in histories.iter().zip(&memberships) {
            for &(index, entity_ref, kind) in &query.entities {
                let state = &history.states()[index];
                if ambiguous_states.contains(&state.state_id) || !entity_refs.contains(&entity_ref)
                {
                    continue;
                }
                let membership = identities.entry(entity_ref).or_default();
                membership.kinds.insert(kind);
                if !membership.states.contains(&state.state_id) {
                    membership.states.push(state.state_id);
                }
            }
        }
        for record_ref in reconstructed_revisions {
            let Some(revision) = revisions.get_mut(&record_ref) else {
                continue;
            };
            revision.states = revision
                .entity_refs
                .iter()
                .filter_map(|entity_ref| identities.get(entity_ref))
                .flat_map(|membership| membership.states.iter().copied())
                .collect::<Vec<_>>();
            revision.states.sort_unstable_by(Ord::cmp);
            revision.states.dedup();
        }
        Self {
            identities,
            revisions,
        }
    }

    fn identity_kind(&self, local_id: u64) -> Option<(AsmHistoricalEntityKind, Vec<i64>)> {
        let entity_ref = i64::try_from(local_id).ok()?;
        let membership = self.identities.get(&entity_ref)?;
        let mut kinds = membership.kinds.iter();
        let kind = kinds.next().copied()?;
        if kinds.next().is_some() {
            return None;
        }
        let states = membership.states.clone();
        Some((kind, states))
    }

    fn selection_identity_kind(
        &self,
        local_id: u64,
    ) -> Option<(AsmHistoricalEntityKind, i64, Vec<i64>)> {
        let record_ref = i64::try_from(local_id).ok()?;
        let revision = self.revisions.get(&record_ref);
        if let Some((kind, states)) = self.identity_kind(local_id) {
            return revision
                .is_none_or(|revision| {
                    revision.entity_refs.is_empty()
                        || (revision.entity_refs.len() == 1
                            && revision.entity_refs.contains(&record_ref))
                })
                .then_some((kind, record_ref, states));
        }
        let revision = revision?;
        let mut entity_refs = revision.entity_refs.iter();
        let entity_ref = entity_refs.next().copied()?;
        if entity_refs.next().is_some() {
            return None;
        }
        let entity_id = u64::try_from(entity_ref).ok()?;
        let (kind, _) = self.identity_kind(entity_id)?;
        let states = revision.states.clone();
        Some((kind, entity_ref, states))
    }
}

#[cfg(test)]
pub(super) fn historical_identity_kind(
    histories: &[AsmHistory],
    local_id: u64,
) -> Option<(AsmHistoricalEntityKind, Vec<i64>)> {
    HistoricalIdentityIndex::build(histories, [local_id]).identity_kind(local_id)
}

pub(super) fn historical_selection_identity_kind(
    histories: &[AsmHistory],
    local_id: u64,
) -> Option<(AsmHistoricalEntityKind, i64, Vec<i64>)> {
    HistoricalIdentityIndex::build(histories, [local_id]).selection_identity_kind(local_id)
}

fn ambiguous_history_state_ids(histories: &[&AsmHistory]) -> HashSet<i64> {
    let mut unique = HashSet::new();
    let mut ambiguous = HashSet::new();
    for state in histories.iter().flat_map(|history| history.states()) {
        if !unique.insert(state.state_id) {
            ambiguous.insert(state.state_id);
        }
    }
    ambiguous
}

/// Select the complete BREP-history set owned by one component context.
/// `None` means the stream predates component naming-space registrations and
/// retains the aggregate compatibility path. `Some(empty)` is a known
/// component with no history-bearing BREP.
fn component_histories<'a>(
    context_id: &str,
    naming_spaces: &[DesignComponentNamingSpace],
    body_bindings: &[DesignBodyBinding],
    histories: &'a [AsmHistory],
) -> Option<Vec<&'a AsmHistory>> {
    if naming_spaces.is_empty() {
        return None;
    }
    let mut matching_spaces = naming_spaces
        .iter()
        .filter(|space| space.context_uuid.as_str().eq_ignore_ascii_case(context_id));
    let Some(space) = matching_spaces.next() else {
        return Some(Vec::new());
    };
    if matching_spaces.next().is_some() {
        return Some(Vec::new());
    }
    let Some(stream) = crate::ids::native_stream(&space.id) else {
        return Some(Vec::new());
    };
    let cluster_end = naming_spaces
        .iter()
        .filter(|candidate| {
            crate::ids::native_stream(&candidate.id) == Some(stream)
                && candidate.component_record_index > space.component_record_index
        })
        .map(|candidate| candidate.component_record_index)
        .min();
    let mut blobs = HashSet::new();
    for blob in body_bindings
        .iter()
        .filter(|binding| {
            crate::ids::native_stream(binding.id()) == Some(stream)
                && binding.entity_suffix >= space.component_record_index
                && cluster_end.is_none_or(|end| binding.entity_suffix < end)
        })
        .map(crate::records::bodies::DesignBodyBinding::blob_name)
    {
        blobs.insert(blob);
    }
    let mut selected = Vec::new();
    for history in histories {
        let Some(encoded_basename) = crate::ids::native_stream(&history.id)
            .and_then(|stream| stream.strip_prefix(crate::ids::SCHEME_PREFIX))
            .and_then(|entry| entry.rsplit('/').next())
        else {
            continue;
        };
        let mut matches_blob = false;
        for blob in &blobs {
            {}
            if crate::ids::encoded_identity_key_component_matches(encoded_basename, blob) {
                matches_blob = true;
                break;
            }
        }
        if matches_blob {
            selected.push(history);
        }
    }
    selected.sort_by(|left, right| left.id.cmp(&right.id));
    selected.dedup_by(|left, right| left.id == right.id);
    Some(selected)
}

pub(crate) fn historical_extrude_selection_identity_kind(
    member: &DesignExtrudeSelectionMember,
    naming_spaces: &[DesignComponentNamingSpace],
    body_bindings: &[DesignBodyBinding],
    histories: &[AsmHistory],
) -> Option<(AsmHistoricalEntityKind, i64, Vec<i64>)> {
    match component_histories(
        member.context_id.as_str(),
        naming_spaces,
        body_bindings,
        histories,
    ) {
        Some(selected) => HistoricalIdentityIndex::build(selected, [member.local_id])
            .selection_identity_kind(member.local_id),
        None => historical_selection_identity_kind(histories, member.local_id),
    }
}

pub(crate) fn bind_extrude_selection_history(
    members: &mut [DesignExtrudeSelectionMember],
    naming_spaces: &[DesignComponentNamingSpace],
    body_bindings: &[DesignBodyBinding],
    histories: &[AsmHistory],
) {
    for member in members {
        member.historical = None;
        if let Some((kind, entity_ref, states)) = historical_extrude_selection_identity_kind(
            member,
            naming_spaces,
            body_bindings,
            histories,
        ) {
            member.historical = Some(crate::records::topology::fillet::HistoricalBinding {
                kind,
                entity_ref,
                state_ids: states,
            });
        }
    }
}

/// Resolve both identities in nested entity-selection operands against the
/// owning feature's exact input topology.
pub(crate) fn bind_entity_selection_history(
    operands: &mut [crate::records::topology::entity_selection::DesignEntitySelectionOperand],
    scopes: &[crate::records::feature::scope::DesignParameterScope],
    histories: &[AsmHistory],
) {
    let identities = HistoricalIdentityIndex::build(
        histories,
        operands.iter().flat_map(|operand| {
            std::iter::once(operand.primary_identity).chain(
                operand
                    .secondary()
                    .map(|secondary| secondary.identity.value),
            )
        }),
    );
    for operand in operands {
        operand.historical_edge_candidates.clear();
        operand.historical_face_candidates.clear();
        operand.resolved_edge_slot = None;
        operand.historical_face_candidates =
            entity_selection_face_candidates(operand.primary_identity, histories);
        let stream = crate::ids::native_stream(&operand.id);
        let mut matching_scopes = scopes.iter().filter(|scope| {
            scope.record_index == operand.scope_record_index
                && crate::ids::native_stream(&scope.id) == stream
        });
        let Some(scope) = matching_scopes.next() else {
            continue;
        };
        if matching_scopes.next().is_some() {
            continue;
        }
        let Some(previous_state_id) = scope.previous_history_state_id() else {
            continue;
        };
        let mut matching_states = histories
            .iter()
            .flat_map(|history| history.states().iter().map(move |state| (history, state)))
            .filter(|(_, state)| state.state_id == previous_state_id);
        let Some((history, state)) = matching_states.next() else {
            continue;
        };
        if matching_states.next().is_some() {
            continue;
        }
        let Some(topology) = history.topology(state) else {
            continue;
        };
        let selections = std::iter::once((0, operand.primary_identity)).chain(
            operand
                .secondary()
                .map(|secondary| (1, secondary.identity.value)),
        );
        operand.historical_edge_candidates =
            entity_selection_edge_candidates(selections, previous_state_id, &identities, topology);
        operand.resolved_edge_slot =
            unique_entity_selection_edge(&operand.historical_edge_candidates);
    }
}

/// Resolve direct persistent face selections carried by Hole constructions.
pub(crate) fn bind_hole_selection_history(
    scopes: &mut [crate::records::feature::scope::DesignParameterScope],
    histories: &[AsmHistory],
) {
    for scope in scopes {
        let Some(construction) = scope.hole_construction_mut() else {
            continue;
        };
        let Some(selection) = &mut construction.face_selection else {
            continue;
        };
        selection.historical_face_candidates =
            entity_selection_face_candidates(selection.primary_identity, histories);
    }
}

/// Resolve persistent circular-pattern axis identities in the feature input topology.
pub(crate) fn bind_circular_pattern_axes(
    scopes: &mut [crate::records::feature::scope::DesignParameterScope],
    histories: &[AsmHistory],
    scope_histories: &HashMap<String, String>,
) {
    use crate::records::feature::patterns::DesignCircularPatternAxis;
    for scope in scopes {
        let mut matching_histories = histories.iter().filter(|history| {
            scope_histories
                .get(&scope.id)
                .is_some_and(|id| history.id == *id)
                || (scope_histories.get(&scope.id).is_none() && histories.len() == 1)
        });
        let Some(history) = matching_histories.next() else {
            continue;
        };
        if matching_histories.next().is_some() {
            continue;
        }
        let input_state_id =
            effective_scope_previous_history_state_id(scope, std::slice::from_ref(history));
        let Some(construction) = scope.circular_pattern_construction_mut() else {
            continue;
        };
        let DesignCircularPatternAxis::HistoricalEdge {
            persistent_identity,
            resolved,
            ..
        } = &mut construction.axis
        else {
            continue;
        };
        *resolved = None;
        let identities =
            HistoricalIdentityIndex::build(std::slice::from_ref(history), [*persistent_identity]);
        let mut axes = historical_pattern_identity_axes(
            *persistent_identity,
            &identities,
            history,
            input_state_id,
        )
        .into_iter();
        let Some(axis) = axes.next() else {
            continue;
        };
        let axis_line = (axis.origin.get(), *axis.direction.as_raw());
        if axes.any(|candidate| {
            !same_axis_line(
                axis_line,
                (candidate.origin.get(), *candidate.direction.as_raw()),
            )
        }) {
            continue;
        }
        *resolved = Some(axis);
    }
}

pub(super) fn historical_pattern_identity_axes(
    identity: u64,
    identities: &HistoricalIdentityIndex,
    history: &AsmHistory,
    input_state_id: Option<i64>,
) -> Vec<crate::records::feature::patterns::DesignAxis> {
    if let Some((kind, entity_ref, state_ids)) = identities.selection_identity_kind(identity) {
        let state_ids = if let Some(input_state_id) = input_state_id {
            if !state_ids.contains(&input_state_id) {
                return Vec::new();
            }
            vec![input_state_id]
        } else {
            state_ids
        };
        return historical_pattern_identity_axes_for_selection(
            Some((kind, entity_ref, &state_ids)),
            history,
        );
    }
    let Some(revision) = snapshot_edge_identity_revision(identity, history) else {
        return Vec::new();
    };
    let archived = HistoricalIdentityIndex::build(std::slice::from_ref(history), [revision]);
    let Some((kind, entity_ref, state_ids)) = archived.selection_identity_kind(revision) else {
        return Vec::new();
    };
    let state_ids = if let Some(input_state_id) = input_state_id {
        if !state_ids.contains(&input_state_id) {
            return Vec::new();
        }
        vec![input_state_id]
    } else {
        state_ids
    };
    historical_pattern_identity_axes_for_selection(Some((kind, entity_ref, &state_ids)), history)
}

pub(super) fn snapshot_edge_identity_revision(identity: u64, history: &AsmHistory) -> Option<u64> {
    let mut matches = history
        .states()
        .iter()
        .flat_map(|state| &state.records)
        .filter(|record| match &record.framing {
            crate::history_records::AsmHistoryRecordFraming::Framed { index, .. } => {
                *index == identity
            }
            crate::history_records::AsmHistoryRecordFraming::Opaque { .. } => identity == 0,
        });
    let record = matches.next()?;
    if matches.next().is_some() {
        return None;
    }
    (record.name() == "edge")
        .then_some(record.revision_id?)
        .filter(|revision| *revision > 0)
        .and_then(|revision| u64::try_from(revision).ok())
}

pub(super) fn historical_pattern_identity_axes_for_selection(
    selected: Option<(AsmHistoricalEntityKind, i64, &[i64])>,
    history: &AsmHistory,
) -> Vec<crate::records::feature::patterns::DesignAxis> {
    let Some((kind, entity_ref, state_ids)) = selected else {
        return Vec::new();
    };
    if state_ids.is_empty() {
        return Vec::new();
    }
    let mut axes = Vec::new();
    let mut matched_state_ids = HashSet::new();
    for state in history
        .states()
        .iter()
        .filter(|state| state_ids.contains(&state.state_id))
    {
        matched_state_ids.insert(state.state_id);
        let Some(topology) = history.topology(state) else {
            return Vec::new();
        };
        let state_axes =
            historical_pattern_identity_axis_candidates(Some((kind, entity_ref)), topology)
                .into_iter()
                .filter_map(|(origin, direction)| design_axis(origin, direction))
                .collect::<Vec<_>>();
        if state_axes.is_empty() {
            return Vec::new();
        }
        for axis in state_axes {
            axes.push(axis);
        }
    }
    if matched_state_ids.len() != state_ids.len() || axes.is_empty() {
        return Vec::new();
    }
    axes
}

fn historical_pattern_identity_axis_candidates(
    selected: Option<(AsmHistoricalEntityKind, i64)>,
    topology: &AsmHistoricalTopology,
) -> Vec<(cadmpeg_ir::math::Point3, cadmpeg_ir::math::Vector3)> {
    let Some((kind, entity_ref)) = selected else {
        return Vec::new();
    };
    match kind {
        AsmHistoricalEntityKind::Face => historical_face_surface_axis(entity_ref, topology)
            .into_iter()
            .collect::<Vec<_>>(),
        AsmHistoricalEntityKind::Surface => historical_surface_axis(entity_ref, topology)
            .into_iter()
            .collect::<Vec<_>>(),
        _ => {
            let mut axes = Vec::new();
            for edge in historical_identity_edges(kind, entity_ref, topology) {
                let Some(axis) = historical_edge_axis(edge, topology) else {
                    continue;
                };

                axes.push(axis);
            }
            axes
        }
    }
}

fn historical_face_surface_axis(
    face: i64,
    topology: &AsmHistoricalTopology,
) -> Option<(cadmpeg_ir::math::Point3, cadmpeg_ir::math::Vector3)> {
    let mut bindings = topology
        .face_surfaces
        .iter()
        .filter(|binding| binding.entity == face);
    let binding = bindings.next()?;
    bindings
        .next()
        .is_none()
        .then(|| historical_surface_axis(binding.carrier, topology))?
}

fn historical_surface_axis(
    surface: i64,
    topology: &AsmHistoricalTopology,
) -> Option<(cadmpeg_ir::math::Point3, cadmpeg_ir::math::Vector3)> {
    let mut candidates = topology
        .surface_axes
        .iter()
        .filter(|axis| axis.surface == surface)
        .map(|axis| (axis.origin, axis.direction))
        .chain(
            topology
                .surface_planes
                .iter()
                .filter(|plane| plane.surface == surface)
                .map(|plane| (plane.origin, plane.normal)),
        );
    let candidate = candidates.next()?;
    candidates.next().is_none().then_some(candidate)
}

/// Bind persistent Mirror plane selections to exact planes in the selected
/// historical topology.
pub(crate) fn bind_mirror_selection_planes(
    scopes: &mut [crate::records::feature::scope::DesignParameterScope],
    groups: &[crate::records::topology::construction::DesignConstructionOperandGroup],
    operands: &[crate::records::topology::entity_selection::DesignEntitySelectionOperand],
    face_operands: &[crate::records::topology::face::DesignFaceOperand],
    identities: &[crate::records::topology::construction::DesignConstructionOperandIdentity],
    histories: &[AsmHistory],
) {
    for scope in scopes {
        let stream = crate::ids::native_stream(&scope.id).map(str::to_owned);
        let record_index = scope.record_index;
        let history_state_id = scope.history_state_id();
        let previous_history_state_id = scope.previous_history_state_id();
        let Some(construction) = scope.mirror_construction_mut() else {
            continue;
        };
        construction.plane = None;
        let (Some(selection_record_index), Some(state_id), Some(previous_state_id)) = (
            construction.plane_selection_record_index,
            history_state_id,
            previous_history_state_id,
        ) else {
            continue;
        };
        let Some((history, _, _)) =
            unique_history_state_pair(histories, state_id, previous_state_id)
        else {
            continue;
        };
        let mut matching_groups = groups.iter().filter(|group| {
            crate::ids::native_stream(&group.id) == stream.as_deref()
                && group.scope_record_index == record_index
                && group.record_index == construction.plane_group_record_index
                && group.role() == DesignOperandRole::ROLE_0X5
                && group
                    .members()
                    .iter()
                    .map(|member| member.value)
                    .eq([selection_record_index])
        });
        let Some(group) = matching_groups.next() else {
            continue;
        };
        if matching_groups.next().is_some() {
            continue;
        }
        let mut matching_operands = operands.iter().filter(|operand| {
            crate::ids::native_stream(&operand.id) == stream.as_deref()
                && operand.scope_record_index == record_index
                && operand.group_record_index == group.record_index
                && operand.group_member_ordinal == 0
                && operand.record_index() == selection_record_index
        });
        let matching_operand = matching_operands.next();
        if matching_operands.next().is_some() {
            continue;
        }
        let mut matching_face_operands = face_operands.iter().filter(|operand| {
            crate::ids::native_stream(&operand.id) == stream.as_deref()
                && operand.scope_record_index == record_index
                && operand.group_record_index() == Some(group.record_index)
                && operand.group_member_ordinal() == Some(0)
                && operand.record_index() == selection_record_index
        });
        let matching_face_operand = matching_face_operands.next();
        if matching_face_operands.next().is_some() {
            continue;
        }
        let plane = if let Some(operand) = matching_operand {
            if matching_face_operand.is_some() {
                continue;
            }
            let mut matching_identities = identities.iter().filter(|identity| {
                crate::ids::native_stream(&identity.id) == stream.as_deref()
                    && identity.group_record_index == group.record_index
            });
            let identity = matching_identities.next();
            if matching_identities.next().is_some() {
                continue;
            }
            if let Some(plane) = design_geometry_mirror_plane(operand.primary_identity) {
                Some(plane)
            } else {
                let persistent_candidates = if let Some(identity) = identity
                    .and_then(crate::records::topology::construction::DesignConstructionOperandIdentity::persistent_identity) {
                    entity_selection_face_candidates(identity.local_id, histories)
                } else {
                    Vec::new()
                };
                let primary_candidates =
                    entity_selection_face_candidates(operand.primary_identity, histories);
                let Some(candidate) =
                    unique_mirror_plane_candidate(primary_candidates, persistent_candidates)
                else {
                    continue;
                };
                historical_mirror_plane(&candidate, previous_state_id, histories)
            }
        } else if let Some(operand) = matching_face_operand {
            historical_mirror_face_operand_plane(operand, history, previous_state_id)
        } else {
            continue;
        };
        let Some(plane) = plane else { continue };
        let norm = (plane.normal.x * plane.normal.x
            + plane.normal.y * plane.normal.y
            + plane.normal.z * plane.normal.z)
            .sqrt();
        if !norm.is_finite() || (norm - 1.0).abs() > EPS_HISTORY_BIND_MIRROR_SELECTION_PLANES_E9 {
            continue;
        }
        let (Some(origin), Some(normal)) = (
            cadmpeg_ir::features::FinitePoint3::new(plane.origin),
            cadmpeg_ir::features::FiniteVector3::new(plane.normal),
        ) else {
            continue;
        };
        construction.plane =
            crate::records::feature::patterns::DesignPlane::from_parts(origin, normal);
    }
}

pub(super) fn historical_mirror_face_operand_plane(
    operand: &crate::records::topology::face::DesignFaceOperand,
    history: &AsmHistory,
    previous_state_id: i64,
) -> Option<HistoricalMirrorPlane> {
    if !operand.resolved_face_slots.is_empty() {
        return coincident_mirror_plane(
            operand.resolved_face_slots.iter().copied(),
            previous_state_id,
            history,
        );
    }
    if !operand.preceding_candidate_faces.is_empty() {
        return coincident_mirror_plane(
            operand
                .preceding_candidate_faces
                .iter()
                .filter_map(|face| stable_ref(face.as_str())),
            previous_state_id,
            history,
        );
    }
    let referenced = || {
        operand.recipe_references.iter().flat_map(|reference| {
            reference
                .candidate_faces
                .iter()
                .chain(&reference.alternate_selector_faces)
        })
    };
    if operand.recipe_kind == crate::records::recipes::ConstructionRecipeKind::Face
        && referenced().next().is_some()
    {
        return coincident_mirror_plane(
            referenced().filter_map(|face| stable_ref(face.as_str())),
            previous_state_id,
            history,
        );
    }
    coincident_mirror_plane(
        crate::design::face_resolve::face_operand_candidates(operand)
            .iter()
            .filter_map(|face| stable_ref(face.as_str())),
        previous_state_id,
        history,
    )
}

fn coincident_mirror_plane(
    slots: impl Iterator<Item = i64> + Clone,
    previous_state_id: i64,
    history: &AsmHistory,
) -> Option<HistoricalMirrorPlane> {
    let first_slot = slots.clone().min()?;
    let first = historical_mirror_plane_for_face_slot(first_slot, previous_state_id, history)?;
    for slot in slots {
        let candidate = historical_mirror_plane_for_face_slot(slot, previous_state_id, history)?;
        if !mirror_planes_coincident(&first, &candidate) {
            return None;
        }
    }
    Some(first)
}

pub(super) fn unique_mirror_plane_candidate(
    mut primary: Vec<
        crate::records::topology::entity_selection::DesignEntitySelectionFaceCandidate,
    >,
    mut persistent: Vec<
        crate::records::topology::entity_selection::DesignEntitySelectionFaceCandidate,
    >,
) -> Option<crate::records::topology::entity_selection::DesignEntitySelectionFaceCandidate> {
    primary.sort_by(|left, right| left.history_id.cmp(&right.history_id));
    primary.dedup();
    persistent.retain(|candidate| {
        primary
            .iter()
            .any(|context| context.history_id == candidate.history_id)
    });
    persistent.sort_by(|left, right| left.history_id.cmp(&right.history_id));
    persistent.dedup();
    match persistent.len() {
        1 => persistent.pop(),
        0 if primary.len() == 1 => primary.pop(),
        _ => None,
    }
}

#[derive(Clone)]
pub(crate) struct HistoricalMirrorPlane {
    pub(super) origin: cadmpeg_ir::math::Point3,
    pub(crate) normal: cadmpeg_ir::math::Vector3,
}

/// Resolve the three built-in Design Geometry origin planes.
///
/// A body face selected as a plane is a face-recipe operand, so an ASM entity
/// that carries the same identity value is a different entity.
pub(crate) fn design_geometry_mirror_plane(primary_identity: u64) -> Option<HistoricalMirrorPlane> {
    let normal = match primary_identity {
        42 => cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
        43 => cadmpeg_ir::math::Vector3::new(0.0, 1.0, 0.0),
        44 => cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
        _ => return None,
    };
    Some(HistoricalMirrorPlane {
        origin: cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
        normal,
    })
}

pub(super) fn historical_mirror_plane(
    candidate: &crate::records::topology::entity_selection::DesignEntitySelectionFaceCandidate,
    preferred_state_id: i64,
    histories: &[AsmHistory],
) -> Option<HistoricalMirrorPlane> {
    if candidate.historical.state_ids.contains(&preferred_state_id) {
        return historical_mirror_plane_in_state(candidate, preferred_state_id, histories);
    }
    let mut resolved = None;
    for state_id in &candidate.historical.state_ids {
        let plane = historical_mirror_plane_in_state(candidate, *state_id, histories)?;
        if resolved
            .as_ref()
            .is_some_and(|exact: &HistoricalMirrorPlane| !mirror_planes_coincident(exact, &plane))
        {
            return None;
        }
        resolved = Some(plane);
    }
    resolved
}

fn mirror_planes_coincident(left: &HistoricalMirrorPlane, right: &HistoricalMirrorPlane) -> bool {
    let dot = left.normal.dot(right.normal);
    let normal_distance = right.origin.vector_from(left.origin).dot(left.normal);
    (dot.abs() - 1.0).abs() <= EPS_HISTORY_MIRROR_PLANES_COINCIDENT_E9
        && normal_distance.abs() <= EPS_HISTORY_MIRROR_PLANES_COINCIDENT_E8
}

fn historical_mirror_plane_in_state(
    candidate: &crate::records::topology::entity_selection::DesignEntitySelectionFaceCandidate,
    state_id: i64,
    histories: &[AsmHistory],
) -> Option<HistoricalMirrorPlane> {
    let mut matching_histories = histories
        .iter()
        .filter(|history| history.id == candidate.history_id);
    let history = matching_histories.next()?;
    if matching_histories.next().is_some() {
        return None;
    }
    let mut matching_states = history
        .states()
        .iter()
        .filter(|state| state.state_id == state_id);
    let state = matching_states.next()?;
    if matching_states.next().is_some() {
        return None;
    }
    let topology = history.topology(state)?;
    match candidate.historical.kind {
        AsmHistoricalEntityKind::Coedge => {
            historical_mirror_coedge_plane(candidate.historical.entity_ref, topology)
        }
        AsmHistoricalEntityKind::Loop => {
            historical_loop_plane(candidate.historical.entity_ref, topology)
        }
        _ => historical_mirror_plane_for_face_slot_in_topology(candidate.face_slot, topology),
    }
}

fn historical_mirror_plane_for_face_slot(
    face_slot: i64,
    state_id: i64,
    history: &AsmHistory,
) -> Option<HistoricalMirrorPlane> {
    let mut matching_states = history
        .states()
        .iter()
        .filter(|state| state.state_id == state_id);
    let state = matching_states.next()?;
    if matching_states.next().is_some() {
        return None;
    }
    let topology = history.topology(state)?;
    historical_mirror_plane_for_face_slot_in_topology(face_slot, topology)
}

fn historical_mirror_plane_for_face_slot_in_topology(
    face_slot: i64,
    topology: &AsmHistoricalTopology,
) -> Option<HistoricalMirrorPlane> {
    let mut bindings = topology
        .face_surfaces
        .iter()
        .filter(|binding| binding.entity == face_slot);
    let binding = bindings.next()?;
    if bindings.next().is_some() {
        return None;
    }
    let mut planes = topology
        .surface_planes
        .iter()
        .filter(|plane| plane.surface == binding.carrier);
    let plane = planes.next()?;
    planes.next().is_none().then_some(HistoricalMirrorPlane {
        origin: plane.origin,
        normal: plane.normal,
    })
}

pub(super) fn historical_loop_plane(
    loop_ref: i64,
    topology: &AsmHistoricalTopology,
) -> Option<HistoricalMirrorPlane> {
    let mut loop_relations = topology
        .loop_coedges
        .iter()
        .filter(|relation| relation.owner_ref == loop_ref);
    let relation = loop_relations.next()?;
    if loop_relations.next().is_some() || relation.member_refs.is_empty() {
        return None;
    }
    let mut planes = Vec::new();
    for coedge_ref in &relation.member_refs {
        let mut coedges = topology
            .coedge_topology
            .iter()
            .filter(|coedge| coedge.coedge == *coedge_ref && coedge.owner_loop == loop_ref);
        let coedge = coedges.next()?;
        if coedges.next().is_some() {
            return None;
        }
        let mut bindings = topology
            .edge_curves
            .iter()
            .filter(|binding| binding.entity == coedge.edge);
        let binding = bindings.next()?;
        if bindings.next().is_some() {
            return None;
        }
        let curve = binding.carrier?;
        let mut axes = topology
            .curve_axes
            .iter()
            .filter(|axis| axis.curve == curve);
        let axis = axes.next()?;
        if axes.next().is_some() {
            return None;
        }
        let norm = (axis.direction.x * axis.direction.x
            + axis.direction.y * axis.direction.y
            + axis.direction.z * axis.direction.z)
            .sqrt();
        if !norm.is_finite() || (norm - 1.0).abs() > EPS_HISTORY_HISTORICAL_LOOP_PLANE_E9 {
            return None;
        }

        planes.push(HistoricalMirrorPlane {
            origin: axis.origin,
            normal: axis.direction,
        });
    }
    let [first, remaining @ ..] = planes.as_slice() else {
        return None;
    };
    remaining
        .iter()
        .all(|candidate| mirror_planes_coincident(first, candidate))
        .then_some(first.clone())
}

/// Resolve a Mirror plane from a selected coedge's complete radial cycle.
///
/// A coedge selects a boundary occurrence, not a face. Its owning face can be
/// non-planar while another face incident to the same topological edge is the
/// selected plane. The cycle and relation checks keep that inference exact:
/// every coedge for the edge must participate in one closed radial cycle, each
/// cycle member must belong to one loop and one face, and the incident faces
/// must expose one coincident set of exact plane carriers.
pub(super) fn historical_mirror_coedge_plane(
    coedge_ref: i64,
    topology: &AsmHistoricalTopology,
) -> Option<HistoricalMirrorPlane> {
    let mut coedges = HashMap::new();
    for coedge in &topology.coedge_topology {
        if coedges.insert(coedge.coedge, coedge).is_some() {
            return None;
        }
    }
    let selected = coedges.get(&coedge_ref)?;
    let edge = selected.edge;
    let mut same_edge = HashSet::new();
    for coedge in coedges.values().filter(|coedge| coedge.edge == edge) {
        same_edge.insert(coedge.coedge);
    }
    let mut radial_cycle = Vec::new();
    let mut current = coedge_ref;
    loop {
        if current == coedge_ref && !radial_cycle.is_empty() {
            break;
        }
        if radial_cycle.contains(&current) {
            return None;
        }
        let coedge = coedges.get(&current)?;
        if coedge.edge != edge {
            return None;
        }

        radial_cycle.push(current);
        current = coedge.radial_next;
    }
    let mut radial_refs = HashSet::new();
    for coedge in &radial_cycle {
        radial_refs.insert(*coedge);
    }
    if radial_refs != same_edge {
        return None;
    }

    let mut faces = BTreeSet::new();
    for coedge_ref in radial_cycle {
        let coedge = coedges.get(&coedge_ref)?;
        let mut loop_relations = topology.loop_coedges.iter().filter(|relation| {
            relation.owner_ref == coedge.owner_loop && relation.member_refs.contains(&coedge_ref)
        });
        let loop_relation = loop_relations.next()?;
        if loop_relations.next().is_some()
            || loop_relation
                .member_refs
                .iter()
                .filter(|member| **member == coedge_ref)
                .count()
                != 1
        {
            return None;
        }
        let mut face_relations = topology
            .face_loops
            .iter()
            .filter(|relation| relation.member_refs.contains(&coedge.owner_loop));
        let face_relation = face_relations.next()?;
        if face_relations.next().is_some()
            || face_relation
                .member_refs
                .iter()
                .filter(|member| **member == coedge.owner_loop)
                .count()
                != 1
        {
            return None;
        }
        faces.insert(face_relation.owner_ref);
    }

    let mut planes = Vec::new();
    for face in faces {
        let mut surface_bindings = topology
            .face_surfaces
            .iter()
            .filter(|binding| binding.entity == face);
        let surface = surface_bindings.next()?;
        if surface_bindings.next().is_some() {
            return None;
        }
        let mut surface_planes = topology
            .surface_planes
            .iter()
            .filter(|plane| plane.surface == surface.carrier);
        if let Some(plane) = surface_planes.next() {
            if surface_planes.next().is_some() {
                return None;
            }

            planes.push(HistoricalMirrorPlane {
                origin: plane.origin,
                normal: plane.normal,
            });
        }
    }
    let [first, remaining @ ..] = planes.as_slice() else {
        return None;
    };
    remaining
        .iter()
        .all(|candidate| mirror_planes_coincident(first, candidate))
        .then_some(first.clone())
}

pub(super) fn entity_selection_face_candidates(
    local_id: u64,
    histories: &[AsmHistory],
) -> Vec<crate::records::topology::entity_selection::DesignEntitySelectionFaceCandidate> {
    use crate::records::topology::entity_selection::DesignEntitySelectionFaceCandidate;

    let mut candidates = Vec::new();
    'histories: for history in histories {
        let identities = HistoricalIdentityIndex::build(std::slice::from_ref(history), [local_id]);
        let Some((kind, entity_ref, state_ids)) = identities.selection_identity_kind(local_id)
        else {
            continue;
        };
        let mut face_slot = None;
        for state_id in &state_ids {
            let mut states = history
                .states()
                .iter()
                .filter(|state| state.state_id == *state_id);
            let Some(state) = states.next() else {
                continue 'histories;
            };
            if states.next().is_some() {
                continue 'histories;
            }
            let Some(topology) = history.topology(state) else {
                continue 'histories;
            };
            let mut faces = historical_identity_faces(kind, entity_ref, topology).into_iter();
            let Some(state_face) = faces.next() else {
                continue 'histories;
            };
            if faces.next().is_some() || face_slot.is_some_and(|face| face != state_face) {
                continue 'histories;
            }
            face_slot = Some(state_face);
        }
        let Some(face_slot) = face_slot else {
            continue;
        };
        let history_id = history.id.clone();

        candidates.push(DesignEntitySelectionFaceCandidate {
            history_id,
            historical: crate::records::topology::fillet::HistoricalBinding {
                kind,
                entity_ref,
                state_ids,
            },
            face_slot,
        });
    }
    candidates
}

pub(super) fn entity_selection_edge_candidates(
    identities: impl IntoIterator<Item = (u32, u64)>,
    previous_state_id: i64,
    history_identities: &HistoricalIdentityIndex,
    topology: &AsmHistoricalTopology,
) -> Vec<crate::records::topology::entity_selection::DesignEntitySelectionEdgeCandidate> {
    use crate::records::topology::entity_selection::DesignEntitySelectionEdgeCandidate;

    let mut candidates = Vec::new();
    for (identity_ordinal, local_id) in identities {
        let Some((kind, entity_ref, states)) = history_identities.selection_identity_kind(local_id)
        else {
            continue;
        };
        if !states.contains(&previous_state_id) {
            continue;
        }
        let edge_slots = historical_identity_edges(kind, entity_ref, topology)
            .into_iter()
            .collect::<Vec<_>>();
        if edge_slots.is_empty() {
            continue;
        }

        candidates.push(DesignEntitySelectionEdgeCandidate {
            identity_ordinal,
            local_id,
            historical_entity_kind: kind,
            historical_entity_ref: entity_ref,
            edge_slots,
        });
    }
    candidates
}

pub(super) fn unique_entity_selection_edge(
    candidates: &[crate::records::topology::entity_selection::DesignEntitySelectionEdgeCandidate],
) -> Option<i64> {
    let first = candidates.first()?;
    let mut unique = None;
    for edge in &first.edge_slots {
        if candidates[1..]
            .iter()
            .all(|candidate| candidate.edge_slots.contains(edge))
        {
            if unique.is_some_and(|prior| prior != *edge) {
                return None;
            }
            unique = Some(*edge);
        }
    }
    unique
}

pub(crate) fn bind_edge_identity_history(
    operands: &mut [DesignEdgeIdentityOperand],
    identities: &[crate::records::topology::construction::DesignConstructionOperandIdentity],
    scopes: &[crate::records::feature::scope::DesignParameterScope],
    histories: super::BindingHistories<'_>,
    scope_histories: &HashMap<String, String>,
    lineage_scopes: &HashSet<String>,
) {
    struct EdgeTreatmentTransitionCandidates {
        radii: Vec<crate::records::topology::edge_identity::DesignEdgeTreatmentRadiusCandidate>,
        treatment_edges: Vec<i64>,
        deleted_edges: Vec<i64>,
    }

    if projection_was_finalized(histories.get()) {
        return;
    }
    let mut compact_group_counts = HashMap::<(String, u32, u32), Option<usize>>::new();
    for operand in operands.iter() {
        let Some(stream) = crate::ids::native_stream(&operand.id) else {
            continue;
        };
        let stream = stream.to_owned();
        let key = (
            stream,
            operand.scope_record_index,
            operand.group_record_index,
        );
        compact_group_counts
            .entry(key)
            .and_modify(|count| {
                *count = count.and_then(|count| operand.layout().is_compact().then_some(count + 1));
            })
            .or_insert(operand.layout().is_compact().then_some(1));
    }
    let local_ids = operands
        .iter()
        .map(|operand| operand.local_id)
        .chain(identities.iter().filter_map(|identity| {
            identity
                .persistent_identity()
                .map(|persistent| persistent.local_id)
        }))
        .collect::<Vec<_>>();
    let history_identities =
        HistoricalIdentityIndex::build(histories.get(), local_ids.iter().copied());
    let identities_by_history = histories
        .get()
        .iter()
        .map(|history| {
            HistoricalIdentityIndex::build(std::slice::from_ref(history), local_ids.iter().copied())
        })
        .collect::<Vec<_>>();
    let mut jobs = Vec::new();
    for (position, operand) in operands.iter_mut().enumerate() {
        operand.historical = None;
        operand.treatment_radius_candidates.clear();
        operand.transition_edge_candidates.clear();
        operand.resolved_edge_slots.clear();
        operand.resolved_edge_slot = None;
        operand.resolution_identity_id = None;
        let Some(stream) = crate::ids::native_stream(&operand.id) else {
            continue;
        };
        let mut matching_scopes = scopes.iter().filter(|scope| {
            crate::ids::native_stream(&scope.id) == Some(stream)
                && scope.record_index == operand.scope_record_index
        });
        let Some(scope) = matching_scopes.next() else {
            continue;
        };
        if matching_scopes.next().is_some() {
            continue;
        }
        let Some(previous_state_id) = scope.previous_history_state_id() else {
            continue;
        };
        let current_state_id = scope.history_state_id();
        let bound_history = bound_scope_history(&scope.id, scope_histories, histories.get())
            .and_then(|history| {
                histories
                    .get()
                    .iter()
                    .position(|item| std::ptr::eq(item, history))
            });
        let scoped_identities = bound_history
            .and_then(|history_index| identities_by_history.get(history_index))
            .unwrap_or(&history_identities);
        if let Some((kind, entity_ref, states)) = scoped_identities
            .selection_identity_kind(operand.local_id)
            .filter(|(_, _, states)| {
                !lineage_scopes.contains(&scope.id) && states.contains(&previous_state_id)
            })
        {
            operand.historical = Some(crate::records::topology::fillet::HistoricalBinding {
                kind,
                entity_ref,
                state_ids: states,
            });
        }
        let Some((history_index, history)) = bound_history
            .and_then(|history_index| Some((history_index, histories.get().get(history_index)?)))
        else {
            continue;
        };
        let mut previous_states = history
            .states()
            .iter()
            .enumerate()
            .filter(|(_, state)| state.state_id == previous_state_id);
        let Some((previous_index, _)) = previous_states.next() else {
            continue;
        };
        if previous_states.next().is_some() {
            continue;
        }
        jobs.push((
            history_index,
            previous_index,
            (position, scope, current_state_id),
        ));
    }
    let mut treatment_candidates_by_transition =
        HashMap::<(String, i64, i64), EdgeTreatmentTransitionCandidates>::new();
    super::visit_state_groups_oldest_first(
        histories,
        jobs,
        |histories, history_index, previous_index, group| {
            let Some(history) = histories.get(history_index) else {
                return;
            };
            let Some(previous_state) = history.states().get(previous_index) else {
                return;
            };
            let Some(topology) = history.topology(previous_state) else {
                return;
            };
            let previous_state_id = previous_state.state_id;
            let scoped_identities = identities_by_history
                .get(history_index)
                .unwrap_or(&history_identities);
            let mut treatment_cache = super::treatment_index::TreatmentCache::default();
            for (position, scope, current_state_id) in group {
                let Some(operand) = operands.get_mut(position) else {
                    continue;
                };
                let Some(stream) = crate::ids::native_stream(&operand.id) else {
                    continue;
                };
                if let Some(current_state_id) = current_state_id {
                    let mut current_states = history
                        .states()
                        .iter()
                        .filter(|state| state.state_id == current_state_id);
                    let current_state = current_states.next();
                    if current_states.next().is_none()
                        && current_state.is_some_and(|state| {
                            history_state_reaches(history, state, previous_state_id)
                        })
                    {
                        let key = (history.id.clone(), current_state_id, previous_state_id);
                        if !treatment_candidates_by_transition.contains_key(&key) {
                            if let Some(result) =
                                current_state.and_then(|state| history.topology(state))
                            {
                                let mut preceding_faces = HashSet::new();
                                for face in &topology.faces {
                                    preceding_faces.insert(*face);
                                }
                                let inserted_faces = result
                                    .faces
                                    .iter()
                                    .copied()
                                    .filter(|face| !preceding_faces.contains(face))
                                    .collect::<Vec<_>>();
                                let mut result_edges = HashSet::new();
                                for edge in &result.edges {
                                    result_edges.insert(*edge);
                                }
                                let deleted_edges = topology
                                    .edges
                                    .iter()
                                    .copied()
                                    .filter(|edge| !result_edges.contains(edge))
                                    .collect::<Vec<_>>();
                                let (radii, treatment_edges) = treatment_edge_candidates(
                                    None,
                                    &inserted_faces,
                                    result,
                                    topology,
                                    &deleted_edges,
                                    &mut treatment_cache,
                                );

                                treatment_candidates_by_transition.insert(
                                    (history.id.clone(), current_state_id, previous_state_id),
                                    EdgeTreatmentTransitionCandidates {
                                        radii,
                                        treatment_edges,
                                        deleted_edges,
                                    },
                                );
                            }
                        }
                        if let Some(candidates) = treatment_candidates_by_transition.get(&key) {
                            operand
                                .treatment_radius_candidates
                                .clone_from(&candidates.radii);
                            let mut treatment_edges = candidates.treatment_edges.clone();
                            // The geometric chain is transition-scoped. The deleted-
                            // set fallback is group-scoped because its proof depends
                            // on this operand group's compact member count.
                            if lineage_scopes.contains(&scope.id) {
                                if treatment_edges.len() != 1 {
                                    treatment_edges.clear();
                                    operand.treatment_radius_candidates.clear();
                                }
                            } else if treatment_edges.is_empty() {
                                let is_edge_treatment = matches!(
                                    crate::design::design_feature_family(&scope.kind()),
                                    Some(
                                        crate::design::DesignFeatureFamily::Fillet
                                            | crate::design::DesignFeatureFamily::Chamfer
                                    )
                                );
                                let compact_member_count = compact_group_counts
                                    .get(&(
                                        stream.to_owned(),
                                        operand.scope_record_index,
                                        operand.group_record_index,
                                    ))
                                    .copied()
                                    .flatten();
                                treatment_edges = complete_compact_edge_treatment_deletions(
                                    is_edge_treatment,
                                    compact_member_count,
                                    &candidates.deleted_edges,
                                );
                            }
                            operand.transition_edge_candidates =
                                treatment_edges.into_iter().collect::<Vec<_>>();
                        }
                    }
                }
                if lineage_scopes.contains(&scope.id) {
                    continue;
                }
                let direct = if let Some(binding) = operand
                    .historical
                    .as_ref()
                    .filter(|binding| binding.state_ids.contains(&previous_state_id))
                {
                    historical_identity_edge(binding.kind, binding.entity_ref, topology)
                } else {
                    None
                };
                if let Some(edge) = direct {
                    operand.resolved_edge_slot = Some(edge);
                    operand.resolution_identity_id = Some(operand.id.clone());
                    continue;
                }
                let mut resolved = Vec::new();
                for identity in identities.iter().filter(|identity| {
                    crate::ids::native_stream(&identity.id) == Some(stream)
                        && identity.group_record_index == operand.group_record_index
                }) {
                    let Some(persistent) = identity.persistent_identity() else {
                        continue;
                    };
                    let Some((kind, entity_ref, states)) =
                        scoped_identities.selection_identity_kind(persistent.local_id)
                    else {
                        continue;
                    };
                    if !states.contains(&previous_state_id) {
                        continue;
                    }
                    let Some(edge) = historical_identity_edge(kind, entity_ref, topology) else {
                        continue;
                    };

                    resolved.push((edge, identity.id.as_str()));
                }
                let mut resolved = resolved.into_iter();
                let Some((edge, identity_id)) = resolved.next() else {
                    continue;
                };
                if resolved.any(|candidate| candidate.0 != edge) {
                    continue;
                }
                operand.resolved_edge_slot = Some(edge);
                operand.resolution_identity_id = Some(identity_id.to_owned());
            }
        },
    );
}

pub(super) fn complete_compact_edge_treatment_deletions(
    is_edge_treatment: bool,
    compact_member_count: Option<usize>,
    deleted_edges: &[i64],
) -> Vec<i64> {
    if is_edge_treatment
        && !deleted_edges.is_empty()
        && compact_member_count == Some(deleted_edges.len())
    {
        deleted_edges.to_vec()
    } else {
        Vec::new()
    }
}

/// Resolve a class-297 edge-treatment member whose persistent local identity
/// names the member's embedded bounded-face recipe. The rule selects every
/// deleted treatment edge on the recipe's exact preceding support face.
pub(crate) fn bind_edge_identity_bounded_face_rules(
    operands: &mut [DesignEdgeIdentityOperand],
    face_operands: &[crate::records::topology::face::DesignFaceOperand],
) {
    use crate::records::recipes::ConstructionRecipeKind;

    for operand in operands {
        operand.resolved_edge_slots.clear();
        if operand.resolved_edge_slot.is_some() {
            continue;
        }
        let mut matches = face_operands.iter().filter(|face| {
            crate::ids::native_stream(&face.id) == crate::ids::native_stream(&operand.id)
                && face.scope_record_index == operand.scope_record_index
                && face.group_record_index() == Some(operand.group_record_index)
                && face.group_member_ordinal() == Some(operand.group_member_ordinal)
                && face.record_index() == operand.record_index()
                && face.class_tag == operand.class_tag
                && face.recipe_kind == ConstructionRecipeKind::BoundedFace
                && u64::from(face.recipe_record_index()) == operand.local_id
        });
        let Some(face) = matches.next() else { continue };
        if matches.next().is_some() {
            continue;
        }
        let [support] = face.historical_support_contexts.as_slice() else {
            continue;
        };
        let mut unique_faces = HashSet::new();
        for face in &support.preceding_face_slots {
            unique_faces.insert(*face);
        }
        if support.preceding_face_slots.is_empty()
            || support.changed_preceding_face_slots != support.preceding_face_slots
            || support.preceding_face_boundaries.len() != support.preceding_face_slots.len()
            || unique_faces.len() != support.preceding_face_slots.len()
            || support.preceding_face_boundaries.iter().any(|boundary| {
                support
                    .preceding_face_boundaries
                    .iter()
                    .filter(|candidate| candidate.face_slot == boundary.face_slot)
                    .count()
                    != 1
                    || !support.preceding_face_slots.contains(&boundary.face_slot)
                    || boundary
                        .loops
                        .iter()
                        .any(|loop_| loop_.boundary.coedges().next().is_none())
            })
        {
            continue;
        }
        let mut transition = HashSet::new();
        for edge in &operand.transition_edge_candidates {
            transition.insert(*edge);
        }
        if transition.is_empty() {
            continue;
        }
        let mut seen = HashSet::new();
        let mut resolved = Vec::new();
        for edge in support
            .preceding_face_boundaries
            .iter()
            .flat_map(|boundary| &boundary.loops)
            .flat_map(|loop_| loop_.boundary.coedges().map(|row| row.edge_slot))
        {
            if transition.contains(&edge) && seen.insert(edge) {
                resolved.push(edge);
            }
        }
        operand.resolved_edge_slots = resolved;
        if !operand.resolved_edge_slots.is_empty() {
            operand.resolution_identity_id = Some(face.id.clone());
        }
    }
}

pub(super) fn historical_identity_edge(
    kind: AsmHistoricalEntityKind,
    entity_ref: i64,
    topology: &AsmHistoricalTopology,
) -> Option<i64> {
    let candidates = historical_identity_edges(kind, entity_ref, topology);
    let mut candidates = candidates.into_iter();
    let edge = candidates.next()?;
    candidates.next().is_none().then_some(edge)
}

fn historical_identity_edges(
    kind: AsmHistoricalEntityKind,
    entity_ref: i64,
    topology: &AsmHistoricalTopology,
) -> BTreeSet<i64> {
    let mut candidates = BTreeSet::new();
    match kind {
        AsmHistoricalEntityKind::Edge => {
            if topology.edges.contains(&entity_ref) {
                candidates.insert(entity_ref);
            }
        }
        AsmHistoricalEntityKind::Coedge => {
            for edge in topology
                .coedge_topology
                .iter()
                .filter(|coedge| coedge.coedge == entity_ref)
                .map(|coedge| coedge.edge)
            {
                candidates.insert(edge);
            }
        }
        AsmHistoricalEntityKind::Pcurve => {
            let mut coedges = HashSet::new();
            for coedge in topology
                .coedge_pcurves
                .iter()
                .filter(|binding| binding.carrier == Some(entity_ref))
                .map(|binding| binding.entity)
            {
                coedges.insert(coedge);
            }
            for edge in topology
                .coedge_topology
                .iter()
                .filter(|coedge| coedges.contains(&coedge.coedge))
                .map(|coedge| coedge.edge)
            {
                candidates.insert(edge);
            }
        }
        AsmHistoricalEntityKind::Curve => {
            for edge in topology
                .edge_curves
                .iter()
                .filter(|binding| binding.carrier == Some(entity_ref))
                .map(|binding| binding.entity)
            {
                candidates.insert(edge);
            }
        }
        AsmHistoricalEntityKind::Vertex | AsmHistoricalEntityKind::Point => {
            let mut vertices = HashSet::new();
            if kind == AsmHistoricalEntityKind::Vertex {
                vertices.insert(entity_ref);
            } else {
                for vertex in topology
                    .vertex_points
                    .iter()
                    .filter(|binding| binding.carrier == entity_ref)
                    .map(|binding| binding.entity)
                {
                    vertices.insert(vertex);
                }
            }
            for edge in topology
                .edge_vertices
                .iter()
                .filter(|edge| {
                    vertices.contains(&edge.start_vertex) || vertices.contains(&edge.end_vertex)
                })
                .map(|edge| edge.edge)
            {
                candidates.insert(edge);
            }
        }
        AsmHistoricalEntityKind::Body
        | AsmHistoricalEntityKind::Region
        | AsmHistoricalEntityKind::Shell
        | AsmHistoricalEntityKind::Face
        | AsmHistoricalEntityKind::Loop
        | AsmHistoricalEntityKind::Surface => {}
    }
    candidates
}

fn historical_identity_faces(
    kind: AsmHistoricalEntityKind,
    entity_ref: i64,
    topology: &AsmHistoricalTopology,
) -> HashSet<i64> {
    let mut carriers = HashSet::new();
    match kind {
        AsmHistoricalEntityKind::Face => {
            carriers.insert(entity_ref);
            return carriers;
        }
        AsmHistoricalEntityKind::Loop => {
            carriers.insert(entity_ref);
        }
        AsmHistoricalEntityKind::Coedge => {
            for loop_slot in topology
                .loop_coedges
                .iter()
                .filter(|relation| relation.member_refs.contains(&entity_ref))
                .map(|relation| relation.owner_ref)
            {
                carriers.insert(loop_slot);
            }
        }
        AsmHistoricalEntityKind::Pcurve => {
            let mut coedges = HashSet::new();
            for coedge in topology
                .coedge_pcurves
                .iter()
                .filter(|binding| binding.carrier == Some(entity_ref))
                .map(|binding| binding.entity)
            {
                coedges.insert(coedge);
            }
            for loop_slot in topology
                .loop_coedges
                .iter()
                .filter(|relation| {
                    relation
                        .member_refs
                        .iter()
                        .any(|coedge| coedges.contains(coedge))
                })
                .map(|relation| relation.owner_ref)
            {
                carriers.insert(loop_slot);
            }
        }
        AsmHistoricalEntityKind::Surface => {
            for face in topology
                .face_surfaces
                .iter()
                .filter(|binding| binding.carrier == entity_ref)
                .map(|binding| binding.entity)
            {
                carriers.insert(face);
            }
            return carriers;
        }
        AsmHistoricalEntityKind::Body
        | AsmHistoricalEntityKind::Region
        | AsmHistoricalEntityKind::Shell
        | AsmHistoricalEntityKind::Edge
        | AsmHistoricalEntityKind::Vertex
        | AsmHistoricalEntityKind::Point
        | AsmHistoricalEntityKind::Curve => return HashSet::new(),
    }
    let mut faces = HashSet::new();
    for face in topology
        .face_loops
        .iter()
        .filter(|relation| {
            relation
                .member_refs
                .iter()
                .any(|loop_slot| carriers.contains(loop_slot))
        })
        .map(|relation| relation.owner_ref)
    {
        faces.insert(face);
    }
    faces
}
