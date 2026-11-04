// SPDX-License-Identifier: Apache-2.0
//! Bind bounded-face nodes to complete historical face boundaries.

use crate::history_records::AsmHistoricalTopology;
use crate::records::topology::face::{DesignFaceOperand, DesignFaceRecipeStructure};
use std::collections::{BTreeSet, HashMap, HashSet};

struct Corner {
    face: i64,
    edges: [i64; 2],
}

pub(super) struct FaceRecipeIndex {
    boundaries: HashMap<i64, Vec<i64>>,
    edge_faces: HashMap<i64, Vec<i64>>,
    endpoints: HashMap<i64, Option<[i64; 2]>>,
    corners: HashMap<i64, Vec<Corner>>,
}

impl FaceRecipeIndex {
    pub(super) fn new(topology: &AsmHistoricalTopology) -> Self {
        let endpoints = super::topology_cache::endpoint_index(topology);
        let mut boundaries = HashMap::new();
        let mut edge_faces = HashMap::<_, Vec<_>>::new();
        let mut corners = HashMap::<_, Vec<_>>::new();
        let mut coedge_rows = HashMap::new();
        for row in &topology.coedge_topology {
            coedge_rows
                .entry(row.coedge)
                .and_modify(|entry| *entry = None)
                .or_insert(Some(row));
        }
        for &face in &topology.faces {
            let Some(loops) = super::relation_members(&topology.face_loops, face) else {
                continue;
            };
            let mut boundary = Vec::new();
            let mut face_corners = Vec::new();
            let complete = loops.iter().all(|&loop_| {
                let Some(coedges) = super::relation_members(&topology.loop_coedges, loop_) else {
                    return false;
                };
                if coedges.is_empty() {
                    return false;
                }
                let mut edges = Vec::new();
                for &coedge in coedges {
                    let Some(Some(row)) = coedge_rows.get(&coedge) else {
                        return false;
                    };
                    if row.owner_loop != loop_ {
                        return false;
                    }
                    edges.push(row.edge);
                }
                for (ordinal, &edge) in edges.iter().enumerate() {
                    let next = edges[(ordinal + 1) % edges.len()];
                    let (Some(Some(left)), Some(Some(right))) =
                        (endpoints.get(&edge), endpoints.get(&next))
                    else {
                        return false;
                    };
                    let shared: BTreeSet<_> = left
                        .iter()
                        .filter(|vertex| right.contains(vertex))
                        .copied()
                        .collect();
                    let shared: Vec<_> = shared.into_iter().collect();
                    let vertices = match shared.as_slice() {
                        [vertex] => std::slice::from_ref(vertex),
                        [_, _] if edges.len() == 2 && ordinal == 0 => shared.as_slice(),
                        [_, _] if edges.len() == 2 => continue,
                        _ => return false,
                    };
                    for &vertex in vertices {
                        face_corners.push((
                            vertex,
                            Corner {
                                face,
                                edges: [edge, next],
                            },
                        ));
                    }
                }
                boundary.extend(edges);
                true
            });
            if complete && !boundary.is_empty() {
                for &edge in &boundary {
                    edge_faces.entry(edge).or_default().push(face);
                }
                for (vertex, corner) in face_corners {
                    corners.entry(vertex).or_default().push(corner);
                }
                boundaries.insert(face, boundary);
            }
        }
        Self {
            boundaries,
            edge_faces,
            endpoints,
            corners,
        }
    }

    fn fan(&self, edge: i64, vertex: i64, first_face: i64) -> Option<Vec<i64>> {
        let corners = self.corners.get(&vertex)?;
        let mut current_edge = edge;
        let mut previous = None;
        let mut visited = HashSet::new();
        let mut fan = Vec::new();
        loop {
            let mut matches = corners.iter().enumerate().filter(|(index, corner)| {
                Some(*index) != previous
                    && corner.edges.contains(&current_edge)
                    && (!fan.is_empty() || corner.face == first_face)
            });
            let (index, corner) = matches.next()?;
            if matches.next().is_some() || !visited.insert(index) {
                return None;
            }
            fan.push(corner.face);
            current_edge = if corner.edges[0] == current_edge {
                corner.edges[1]
            } else {
                corner.edges[0]
            };
            if current_edge == edge {
                return (visited.len() == corners.len()).then_some(fan);
            }
            previous = Some(index);
        }
    }

    fn node_matches(
        &self,
        node: &DesignFaceRecipeStructure,
        edge: i64,
        lanes: &[BTreeSet<i64>],
    ) -> bool {
        let Some(faces) = self.edge_faces.get(&edge) else {
            return false;
        };
        let [left, right] = faces.as_slice() else {
            return false;
        };
        let Some(Some(vertices)) = self.endpoints.get(&edge) else {
            return false;
        };
        let Some(root) = usize::try_from(node.root).ok().and_then(|i| lanes.get(i)) else {
            return false;
        };
        let Some(other) = usize::try_from(node.prelude[0])
            .ok()
            .and_then(|i| lanes.get(i))
        else {
            return false;
        };
        if !(root.contains(left) && other.contains(right)
            || root.contains(right) && other.contains(left))
        {
            return false;
        }
        let (Some(first), Some(second)) = (
            self.fan(edge, vertices[0], *left),
            self.fan(edge, vertices[1], *left),
        ) else {
            return false;
        };
        let matches = |side: usize, fan: &[i64]| {
            let side = &node.sides[side];
            let ordinals = std::iter::once(side.header_value)
                .chain(side.scalars.iter().copied())
                .collect::<Vec<_>>();
            fan_matches(&ordinals, fan, lanes)
        };
        matches(0, &first) && matches(1, &second) || matches(1, &first) && matches(0, &second)
    }

    pub(super) fn resolve(&self, operand: &DesignFaceOperand) -> Option<i64> {
        use crate::design::decode::operands::{face_recipe_program_kind, FaceRecipeProgramKind};
        if operand.recipe_kind != crate::records::recipes::ConstructionRecipeKind::BoundedFace {
            return None;
        }
        let FaceRecipeProgramKind::Counted { header_value } =
            face_recipe_program_kind(&operand.recipe_program)?
        else {
            return None;
        };
        if header_value != operand.recipe_nodes.len() || header_value == 0 {
            return None;
        }
        let mut keys = Vec::new();
        let mut lanes = Vec::<BTreeSet<i64>>::new();
        for reference in &operand.recipe_references {
            let key = (reference.selector_offset, reference.token_offset);
            let ordinal = keys
                .iter()
                .position(|other| *other == key)
                .unwrap_or_else(|| {
                    keys.push(key);
                    lanes.push(BTreeSet::new());
                    keys.len() - 1
                });
            for face in super::effective_faces(reference) {
                if let Some(slot) = super::stable_ref(face.as_str()) {
                    if self.boundaries.contains_key(&slot) {
                        lanes[ordinal].insert(slot);
                    }
                }
            }
        }
        let nodes = operand
            .recipe_nodes
            .iter()
            .map(|node| node.recipe_structure.as_ref())
            .collect::<Option<Vec<_>>>()?;
        if nodes
            .iter()
            .any(|node| !node.has_valid_operand_ordinals(&operand.recipe_references))
        {
            return None;
        }
        let mut candidates = None::<BTreeSet<i64>>;
        for node in &nodes {
            let mut incident = lanes[usize::try_from(node.root).ok()?].clone();
            incident.extend(&lanes[usize::try_from(node.prelude[0]).ok()?]);
            candidates = Some(match candidates {
                Some(previous) => previous.intersection(&incident).copied().collect(),
                None => incident,
            });
        }
        let mut resolved = None;
        for face in candidates? {
            let boundary = self.boundaries.get(&face)?;
            if boundary.len() != nodes.len()
                || boundary.iter().collect::<HashSet<_>>().len() != boundary.len()
            {
                continue;
            }
            let mut covered = HashSet::new();
            let complete = nodes.iter().all(|node| {
                let mut matches = boundary
                    .iter()
                    .copied()
                    .filter(|edge| self.node_matches(node, *edge, &lanes));
                let Some(edge) = matches.next() else {
                    return false;
                };
                matches.next().is_none() && covered.insert(edge)
            });
            if complete && covered.len() == boundary.len() {
                if resolved.is_some() {
                    return None;
                }
                resolved = Some(face);
            }
        }
        resolved
    }
}

fn fan_matches(ordinals: &[i32], fan: &[i64], lanes: &[BTreeSet<i64>]) -> bool {
    ordinals.len() == fan.len()
        && (0..fan.len()).any(|start| {
            [false, true].into_iter().any(|reverse| {
                ordinals.iter().enumerate().all(|(offset, ordinal)| {
                    let index = if reverse {
                        (start + fan.len() - offset) % fan.len()
                    } else {
                        (start + offset) % fan.len()
                    };
                    usize::try_from(*ordinal)
                        .ok()
                        .and_then(|ordinal| lanes.get(ordinal))
                        .is_some_and(|lane| lane.contains(&fan[index]))
                })
            })
        })
}

#[cfg(test)]
mod tests;
