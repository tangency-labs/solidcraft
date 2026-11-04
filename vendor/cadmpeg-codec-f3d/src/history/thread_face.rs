// SPDX-License-Identifier: Apache-2.0
//! Resolve a topology-preserving Thread selection in its active BREP member.

use super::output_chain::OutputChain;
use crate::history_records::{AsmHistoricalTopologyDelta, AsmHistoricalTransition, AsmHistory};
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use cadmpeg_ir::features::{
    FaceSelection, Feature, FeatureDefinition, FeatureInputTopology, FeatureOperation,
};
use cadmpeg_ir::ids::FaceId;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::topology::Face;

pub(crate) struct Inputs<'a> {
    pub(crate) scopes: &'a [DesignParameterScope],
    pub(crate) topologies: &'a [FeatureInputTopology],
    pub(crate) histories: &'a [AsmHistory],
    pub(crate) faces: &'a [Face],
    pub(crate) primary_brep: Option<&'a str>,
}

fn unique<T>(mut items: impl Iterator<Item = T>) -> Option<T> {
    let item = items.next()?;
    items.next().is_none().then_some(item)
}

fn preserves_connectivity(transition: &AsmHistoricalTransition) -> bool {
    let delta = &transition.topology;
    let rows = &transition.rows;
    [
        &delta.bodies,
        &delta.regions,
        &delta.shells,
        &delta.faces,
        &delta.loops,
        &delta.coedges,
        &delta.edges,
        &delta.vertices,
    ]
    .into_iter()
    .all(|family| family.inserted.is_empty() && family.deleted.is_empty())
        && rows.body_regions.is_empty()
        && rows.region_shells.is_empty()
        && rows.shell_faces.is_empty()
        && rows.shell_wire_edges.is_empty()
        && rows.shell_free_vertices.is_empty()
        && rows.face_loops.is_empty()
        && rows.loop_coedges.is_empty()
        && rows.coedge_topology.is_empty()
        && rows.edge_vertices.is_empty()
}

fn preserves_graph(delta: &AsmHistoricalTopologyDelta) -> bool {
    let empty = crate::history_records::AsmHistoricalEntityDelta::default();
    delta.bodies.inserted.is_empty()
        && delta.bodies.deleted.is_empty()
        && [
            &delta.regions,
            &delta.shells,
            &delta.faces,
            &delta.loops,
            &delta.coedges,
            &delta.edges,
            &delta.vertices,
            &delta.points,
            &delta.surfaces,
            &delta.curves,
            &delta.pcurves,
        ]
        .into_iter()
        .all(|family| *family == empty)
}

fn live_faces(
    feature: &Feature,
    selection: &FaceSelection,
    input: &Inputs<'_>,
) -> Result<Vec<FaceId>, &'static str> {
    let FaceSelection::Historical { state, faces, .. } = selection else {
        return Err("the input selection is incomplete");
    };
    if faces.as_slice().is_empty() {
        return Err("the input selection is empty");
    }
    let scope = unique(input.scopes.iter().filter(|scope| {
        feature.native_ref.as_deref() == Some(scope.id.as_str())
            && scope.kind() == DesignFeatureKind::Thread
    }))
    .ok_or("the Thread scope is ambiguous")?;
    let topology = unique(
        input
            .topologies
            .iter()
            .filter(|topology| topology.input_of == feature.id && topology.id == *state),
    )
    .ok_or("the input topology is ambiguous")?;
    let (history, previous) = unique(input.histories.iter().flat_map(|history| {
        history
            .states()
            .iter()
            .filter(|previous| topology.native_ref.as_deref() == Some(previous.id.as_str()))
            .map(move |previous| (history, previous))
    }))
    .ok_or("the input history member is ambiguous")?;
    let prefix = crate::ids::feature_input_prefix(&feature.id, previous.state_id);
    if *state != crate::ids::history_input_state_id(&prefix)
        || !faces
            .as_slice()
            .iter()
            .all(|face| topology.faces.contains(face))
    {
        return Err("the selection does not belong to the emitted input state");
    }
    let result = scope
        .history_state_id()
        .ok_or("the Thread result state is absent")?;
    let chain = OutputChain::resolve(
        std::slice::from_ref(history),
        result,
        previous.state_id,
        Some(&history.id),
    )
    .ok_or("the Thread transition chain is incomplete")?;
    for state in &chain.states {
        let transition = unique(
            history
                .states()
                .iter()
                .filter(|member| member.state_id == *state),
        )
        .and_then(|member| member.transition.as_ref())
        .ok_or("a Thread transition is ambiguous")?;
        if !preserves_connectivity(transition) {
            return Err("the Thread changes topology membership or connectivity");
        }
    }
    let head = unique(
        history
            .states()
            .iter()
            .filter(|member| member.previous_ref.is_none()),
    )
    .ok_or("the active history head is ambiguous")?;
    if head.state_id != result {
        let following = OutputChain::resolve(
            std::slice::from_ref(history),
            head.state_id,
            result,
            Some(&history.id),
        )
        .ok_or("the transition chain to the active head is incomplete")?;
        for state in &following.states {
            let transition = unique(
                history
                    .states()
                    .iter()
                    .filter(|member| member.state_id == *state),
            )
            .and_then(|member| member.transition.as_ref())
            .ok_or("a later transition is ambiguous")?;
            if !preserves_graph(&transition.topology) {
                return Err("a later transition rewrites topology or geometry");
            }
        }
    }
    let source =
        super::historical_brep_source(&history.id).ok_or("the history BREP identity is absent")?;
    let member = history
        .id
        .strip_prefix("f3d:")
        .and_then(|id| id.split_once(":asm-history#"))
        .map(|(member, _)| member);
    let mut resolved = Vec::new();
    for historical in faces.as_slice() {
        let slot = historical
            .as_str()
            .rsplit(':')
            .next()
            .and_then(|slot| slot.parse::<i64>().ok())
            .filter(|slot| *slot >= 0)
            .ok_or("the historical face slot is invalid")?;
        if *historical != crate::ids::history_input_face_id(&prefix, slot) {
            return Err("the historical face belongs to another state or member");
        }
        let face = unique(input.faces.iter().filter(|face| {
            super::stable_ref(face.id.as_str()) == Some(slot)
                && if face.id.as_str().starts_with("f3d:brep:entity#") {
                    member.is_some() && member == input.primary_brep
                } else {
                    super::entity_in_brep_source(face.id.as_str(), source)
                }
        }))
        .ok_or("the stable slot has no unique live face in its BREP member")?;
        resolved.push(face.id.clone());
    }
    Ok(resolved)
}

pub(crate) fn bind(features: &mut [Feature], input: &Inputs<'_>) -> Vec<LossNote> {
    let mut losses = Vec::new();
    for feature in features {
        let FeatureDefinition::Operation(FeatureOperation::CosmeticThread { face, .. }) =
            feature.evaluation.definition()
        else {
            continue;
        };
        let FaceSelection::Historical { native, .. } = face else {
            continue;
        };
        match live_faces(feature, face, input) {
            Ok(faces) => {
                let native = native.as_str().to_owned();
                feature.evaluation.edit(|definition, _| {
                    if let FeatureDefinition::Operation(FeatureOperation::CosmeticThread {
                        face,
                        ..
                    }) = definition
                    {
                        *face = FaceSelection::Resolved { faces, native };
                    }
                });
            }
            Err(reason) => losses.push(crate::loss::F3dLossCode::ThreadLiveFaceUnproven.note(
                format!(
                    "{} keeps its historical Thread face selection because {reason}.",
                    feature.id
                ),
            )),
        }
    }
    losses
}

#[cfg(test)]
mod tests;
