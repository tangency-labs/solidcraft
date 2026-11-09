// SPDX-License-Identifier: Apache-2.0
//! Bound construction-history fixtures.

use crate::history_records::{
    AsmDeltaState, AsmEntityVersion, AsmHistoricalCoedge, AsmHistoricalEdge, AsmHistoricalRelation,
    AsmHistoricalTopology, AsmHistoricalTransition, AsmHistory, AsmPreamble, AsmStateSnapshot,
};
use std::collections::BTreeMap;

pub(crate) type State = (
    AsmDeltaState,
    Option<AsmHistoricalTopology>,
    Vec<AsmEntityVersion>,
);

/// Compute transitions from fixture topologies and full record-version maps.
pub(crate) fn history(
    id: String,
    byte_offset: u64,
    preamble: Option<AsmPreamble>,
    budget_exceeded: bool,
    mut states: Vec<State>,
) -> AsmHistory {
    // A fixture without link metadata names its states from newest to oldest.
    if states.len() > 1 && states.iter().all(|s| s.0.next_ref.is_none()) {
        for index in 0..states.len() - 1 {
            states[index].0.next_ref = Some(states[index + 1].0.node_index);
        }
    }
    for index in 0..states.len() {
        if let Some(next) = states[index].0.next_ref {
            let node = states[index].0.node_index;
            if let Some(older) = states.iter_mut().find(|s| s.0.node_index == next) {
                older.0.previous_ref = Some(node);
            }
        }
    }
    let versions: Vec<BTreeMap<_, _>> = states
        .iter()
        .map(|(_, _, versions)| {
            versions
                .iter()
                .map(|row| (row.entity_ref, row.record_ref))
                .collect()
        })
        .collect();
    let bound = states.iter().all(|s| s.1.is_some());
    let transitions: Vec<_> = states
        .iter()
        .enumerate()
        .map(|(index, (state, _, _))| {
            if !bound {
                return None;
            }
            let snapshot = |index: usize| AsmStateSnapshot {
                state_id: states[index].0.state_id,
                topology: states[index].1.as_ref().unwrap(),
                versions: &versions[index],
            };
            let older = state.next_ref.map(|node| {
                let index = states.iter().position(|s| s.0.node_index == node).unwrap();
                snapshot(index)
            });
            Some(AsmHistoricalTransition::between(older, snapshot(index)).unwrap())
        })
        .collect();
    let states = states
        .into_iter()
        .zip(transitions)
        .map(|((mut state, _, _), transition)| {
            state.transition = transition;
            state
        })
        .collect();
    AsmHistory::new(id, byte_offset, preamble, budget_exceeded, states)
}

/// Recompute a fixture chain after changing one state's decode inputs.
pub(crate) fn edit(
    target: &mut AsmHistory,
    ordinal: usize,
    change: impl FnOnce(&mut AsmHistoricalTopology, &mut Vec<AsmEntityVersion>),
) {
    let mut states: Vec<_> = target
        .states()
        .iter()
        .map(|state| {
            (
                state.clone(),
                target.topology(state).cloned(),
                target.entity_versions(state),
            )
        })
        .collect();
    let (_, topology, versions) = &mut states[ordinal];
    change(topology.as_mut().unwrap(), versions);
    let scope = target.topology_scope;
    *target = history(
        target.id.clone(),
        target.byte_offset,
        target.preamble,
        target.binding_budget_exceeded,
        states,
    );
    target.topology_scope = scope;
}

/// Top 10, front 20, back 30 and bottom 40 of a box with vertices 1 to 8,
/// face 50 with no loop, and face 60 apart from the box on vertices 9 to 12.
pub(crate) fn box_topology() -> AsmHistoricalTopology {
    let faces: [(i64, [i64; 4]); 5] = [
        (10, [1, 2, 3, 4]),
        (20, [1, 2, 6, 5]),
        (30, [3, 4, 8, 7]),
        (40, [5, 6, 7, 8]),
        (60, [9, 10, 11, 12]),
    ];
    let mut topology = AsmHistoricalTopology {
        vertices: (1..=12).collect(),
        faces: vec![10, 20, 30, 40, 50, 60],
        ..AsmHistoricalTopology::default()
    };
    for (face, vertices) in faces {
        let loop_ = face + 1;
        let mut coedges = Vec::new();
        for k in 0..4 {
            let offset = i64::try_from(k).unwrap();
            let (coedge, edge) = (face * 10 + offset, face * 100 + offset);
            topology.coedge_topology.push(AsmHistoricalCoedge {
                coedge,
                owner_loop: loop_,
                edge,
                next: 0,
                previous: 0,
                radial_next: 0,
            });
            topology.edge_vertices.push(AsmHistoricalEdge {
                edge,
                start_vertex: vertices[k],
                end_vertex: vertices[(k + 1) % 4],
            });
            coedges.push(coedge);
        }
        topology.face_loops.push(AsmHistoricalRelation {
            owner_ref: face,
            member_refs: vec![loop_],
        });
        topology.loop_coedges.push(AsmHistoricalRelation {
            owner_ref: loop_,
            member_refs: coedges,
        });
    }
    topology.coedge_topology.sort_by_key(|row| row.coedge);
    topology.edge_vertices.sort_by_key(|row| row.edge);
    topology
}

/// A face reference of recipe operand `operand` that names `faces` under
/// `token`.
pub(crate) fn recipe_operand_reference(
    operand: u64,
    token: &str,
    design_reference: i64,
    faces: &[i64],
) -> crate::records::dimensions::DesignRecipeReference {
    crate::records::dimensions::DesignRecipeReference {
        selector: 1,
        selector_offset: 0,
        token: token.into(),
        token_offset: 10,
        design_reference,
        design_reference_offset: 0,
        candidate_faces: faces
            .iter()
            .map(|face| cadmpeg_ir::ids::FaceId::mint(format!("f3d:brep:entity#{face}")).unwrap())
            .collect(),
        candidate_edges: Vec::new(),
        alternate_selector_faces: Vec::new(),
        alternate_selector_edges: Vec::new(),
        operand_offset: Some(100 * operand),
    }
}

/// A bounded-face member 110 of group 100 in scope 42 with the given recipe
/// references and an empty counted program.
pub(crate) fn bounded_face_operand(
    references: Vec<crate::records::dimensions::DesignRecipeReference>,
) -> crate::records::topology::face::DesignFaceOperand {
    let mut operand: crate::records::topology::face::DesignFaceOperand =
        serde_json::from_value(serde_json::json!({
            "id": "f3d:Design/BulkStream.dat:design-face-operand#110",
            "scope_record_index": 42,
            "scope_reference_ordinal": 1,
            "group_record_index": 100,
            "group_member_ordinal": 0,
            "record_index": 110,
            "byte_offset": 0,
            "class_tag": "310",
            "paired_byte_offset": 16,
            "paired_class_tag": "259",
            "recipe_record_index": 113,
            "recipe_record_byte_offset": 32,
            "recipe_id": "f3d:Design/BulkStream.dat:construction-recipe#113",
            "recipe_prefix_offset": 43,
            "recipe_prefix_bytes": "",
            "recipe_references": [],
            "recipe_kind": "bounded_face",
            "recipe_program_offset": 0,
            "recipe_program": [0],
            "recipe_node_offsets": [],
            "recipe_nodes": [],
            "next_record_index": 114,
            "next_byte_offset": 160
        }))
        .unwrap();
    operand.recipe_references = references;
    operand
}
