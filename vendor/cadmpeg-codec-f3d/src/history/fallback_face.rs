// SPDX-License-Identifier: Apache-2.0
//! Check the faces that a candidate fallback selects for a bounded-face
//! recipe against the faces that the recipe's prefix operands name.
//!
//! The nodes of a bounded-face recipe name, by prefix-operand ordinal, faces
//! around the boundary of the recipe's face. For a recipe whose nodes have no
//! unique binding, the check takes the operands that its parsed nodes name,
//! or every prefix operand when no node parses, as naming such faces: a
//! fallback selection that shares no boundary vertex with the faces of one
//! such operand is withdrawn.

use std::collections::{HashMap, HashSet};

use cadmpeg_ir::report::loss::LossNote;

use super::topology_cache::{endpoint_index, TopologyQueryCache, VertexBoundaryIndex};
use super::{boundary_vertices_for_faces, stable_ref};
use crate::history_records::AsmHistoricalTopology;
use crate::loss::F3dLossCode;
use crate::records::topology::face::{prefix_operand_ordinals, DesignFaceOperand};

/// How a face set meets the prefix-operand lanes of a recipe.
#[derive(Debug, PartialEq)]
pub(super) enum LaneFit {
    /// The faces share a boundary vertex with every lane whose faces can be
    /// read.
    Touches,
    /// The faces share no boundary vertex with the lane of this
    /// prefix-operand ordinal.
    Apart(usize),
    /// The boundary of one of the faces cannot be read.
    Unreadable,
}

/// The faces that each prefix operand of one recipe names in one topology.
/// A lane has boundary vertices only when its operand is one that the parsed
/// nodes name (every operand when no node parses), no negative Design
/// reference of the operand names a face, every other reference of it names
/// faces, every named face lies in the topology, and every one of those faces
/// can be read.
pub(super) struct OperandLanes {
    lanes: Vec<(Vec<i64>, Option<HashSet<i64>>)>,
}

impl OperandLanes {
    /// `None` when the recipe's prefix operands cannot be numbered.
    pub(super) fn of<'a>(
        operand: &DesignFaceOperand,
        topology: &'a AsmHistoricalTopology,
        topology_queries: &mut TopologyQueryCache<'a>,
    ) -> Option<Self> {
        let ordinals = prefix_operand_ordinals(&operand.recipe_references)?;
        let named = node_ordinals(operand);
        let index = topology_queries
            .vertices
            .get(topology, VertexBoundaryIndex::new);
        let edge_endpoints = topology_queries.endpoints.get(topology, endpoint_index);
        let mut faces: Vec<(Vec<i64>, bool)> = Vec::new();
        for (reference, &ordinal) in operand.recipe_references.iter().zip(&ordinals) {
            if ordinal == faces.len() {
                faces.push((Vec::new(), true));
            }
            let (lane, complete) = faces.get_mut(ordinal)?;
            if reference.design_reference < 0 {
                *complete &= reference.candidate_faces.is_empty();
                continue;
            }
            let named: Vec<Option<i64>> = reference
                .candidate_faces
                .iter()
                .map(|face| stable_ref(face.as_str()).filter(|face| index.faces.contains(face)))
                .collect();
            *complete &= !named.is_empty() && named.iter().all(Option::is_some);
            lane.extend(named.into_iter().flatten());
        }
        let lanes = faces
            .into_iter()
            .zip(0..)
            .map(|((mut lane, complete), ordinal)| {
                let complete =
                    complete && named.as_ref().is_none_or(|named| named.contains(&ordinal));
                lane.sort_unstable();
                lane.dedup();
                let vertices = complete
                    .then(|| {
                        lane.iter()
                            .map(|face| {
                                boundary_vertices_for_faces(
                                    std::iter::once(*face),
                                    index,
                                    edge_endpoints,
                                )
                            })
                            .collect::<Option<Vec<_>>>()
                    })
                    .flatten()
                    .map(|sets| sets.into_iter().flatten().collect());
                (lane, vertices)
            })
            .collect();
        Some(Self { lanes })
    }

    pub(super) fn fit<'a>(
        &self,
        faces: &[i64],
        topology: &'a AsmHistoricalTopology,
        topology_queries: &mut TopologyQueryCache<'a>,
    ) -> LaneFit {
        let index = topology_queries
            .vertices
            .get(topology, VertexBoundaryIndex::new);
        let edge_endpoints = topology_queries.endpoints.get(topology, endpoint_index);
        let Some(vertices) =
            boundary_vertices_for_faces(faces.iter().copied(), index, edge_endpoints)
        else {
            return LaneFit::Unreadable;
        };
        self.lanes
            .iter()
            .position(|(lane, lane_vertices)| {
                !lane.is_empty()
                    && lane_vertices
                        .as_ref()
                        .is_some_and(|lane_vertices| lane_vertices.is_disjoint(&vertices))
            })
            .map_or(LaneFit::Touches, LaneFit::Apart)
    }

    /// The faces that the prefix operand of ordinal `lane` names.
    fn names(&self, lane: usize) -> &[i64] {
        self.lanes.get(lane).map_or(&[], |(faces, _)| faces)
    }
}

/// The prefix-operand ordinals that the recipe's parsed nodes name: each
/// node's root, first prelude value and fan values. A node without a parsed
/// structure, or one that names an ordinal outside the operands, names none.
/// `None` when no node names an ordinal.
fn node_ordinals(operand: &DesignFaceOperand) -> Option<HashSet<usize>> {
    let nodes: Vec<_> = operand
        .recipe_nodes
        .iter()
        .filter_map(|node| node.recipe_structure.as_ref())
        .filter(|node| node.has_valid_operand_ordinals(&operand.recipe_references))
        .collect();
    if nodes.is_empty() {
        return None;
    }
    Some(
        nodes
            .iter()
            .flat_map(|node| {
                [node.root, node.prelude[0]]
                    .into_iter()
                    .chain(node.sides.iter().flat_map(|side| {
                        std::iter::once(side.header_value).chain(side.scalars.iter().copied())
                    }))
            })
            .filter_map(|ordinal| usize::try_from(ordinal).ok())
            .collect(),
    )
}

/// The fallback faces that the check withdrew, by operand, and their losses.
#[derive(Default)]
pub(super) struct Withdrawals {
    pub(super) losses: Vec<LossNote>,
    pub(super) faces: HashMap<String, Vec<i64>>,
}

impl Withdrawals {
    /// Whether the check withdrew faces from `operand`.
    pub(super) fn withdrew(&self, operand: &DesignFaceOperand) -> bool {
        self.faces.contains_key(&operand.id)
    }

    /// Whether a later rule may give `faces` to `operand`: the check withdrew
    /// nothing from it, or `faces` hold no withdrawn face and share a
    /// boundary vertex with every lane of its recipe.
    pub(super) fn admits<'a>(
        &self,
        operand: &DesignFaceOperand,
        faces: &[i64],
        topology: &'a AsmHistoricalTopology,
        topology_queries: &mut TopologyQueryCache<'a>,
    ) -> bool {
        let Some(dropped) = self.faces.get(&operand.id) else {
            return true;
        };
        !dropped.iter().any(|face| faces.contains(face))
            && OperandLanes::of(operand, topology, topology_queries).is_some_and(|lanes| {
                lanes.fit(faces, topology, topology_queries) == LaneFit::Touches
            })
    }

    /// Report that `rule` offered `candidate` to `owner` after a withdrawal, and
    /// that the offer was refused.
    pub(super) fn refuse(&mut self, owner: &str, candidate: &str, rule: &str) {
        self.losses
            .push(F3dLossCode::FeatureFaceOperandFallbackContradicted.note(format!(
                "{owner} refuses {candidate} from the {rule}: a face is withdrawn, or a withdrawn operand's face shares no boundary vertex with the faces of one of its recipe operands."
            )));
    }

    /// Whether a group rule may give each face of `pairs` to its operand: no
    /// face of the group is withdrawn from any of its operands, and each
    /// operand admits its own face.
    pub(super) fn admits_pairs<'a>(
        &self,
        pairs: &[(&DesignFaceOperand, i64)],
        topology: &'a AsmHistoricalTopology,
        topology_queries: &mut TopologyQueryCache<'a>,
    ) -> bool {
        let faces: Vec<i64> = pairs.iter().map(|(_, face)| *face).collect();
        pairs.iter().all(|(operand, _)| {
            self.faces
                .get(&operand.id)
                .is_none_or(|dropped| !dropped.iter().any(|face| faces.contains(face)))
        }) && pairs
            .iter()
            .all(|(operand, face)| self.admits(operand, &[*face], topology, topology_queries))
    }
}

/// Withdraw the fallback faces of a bounded-face operand when they share no
/// boundary vertex with the faces of one of its prefix operands.
pub(super) fn check_fallback_faces<'a>(
    operand: &mut DesignFaceOperand,
    topology: &'a AsmHistoricalTopology,
    topology_queries: &mut TopologyQueryCache<'a>,
    withdrawals: &mut Withdrawals,
) {
    if operand.resolved_face_slots.is_empty() {
        return;
    }
    let Some(lanes) = OperandLanes::of(operand, topology, topology_queries) else {
        return;
    };
    let LaneFit::Apart(lane) = lanes.fit(&operand.resolved_face_slots, topology, topology_queries)
    else {
        return;
    };
    let fallback = std::mem::take(&mut operand.resolved_face_slots);
    withdrawals.losses.push(F3dLossCode::FeatureFaceOperandFallbackContradicted.note(format!(
        "Extrude face operand {} withdraws the faces {fallback:?} that its history fallback selects: they share no boundary vertex with the faces {:?} that its recipe operand {lane} names, taken as faces around the selected face's boundary.",
        operand.id,
        lanes.names(lane),
    )));
    withdrawals.faces.insert(operand.id.clone(), fallback);
}

#[cfg(test)]
mod tests;
