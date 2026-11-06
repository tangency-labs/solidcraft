// SPDX-License-Identifier: Apache-2.0
use super::{fan_matches, FaceRecipeIndex};
use crate::history_records::{
    AsmHistoricalCoedge, AsmHistoricalEdge, AsmHistoricalRelation, AsmHistoricalTopology,
};
use crate::records::topology::face::{DesignFaceOperand, DesignFaceRecipeNode};
use std::collections::BTreeSet;

fn topology() -> AsmHistoricalTopology {
    let mut topology = AsmHistoricalTopology {
        faces: vec![100, 101, 102, 103],
        edges: vec![10, 11, 12, 13, 14, 15],
        vertices: vec![1, 2, 3, 4],
        edge_vertices: [
            (10, 1, 2),
            (11, 2, 3),
            (12, 3, 1),
            (13, 1, 4),
            (14, 4, 2),
            (15, 3, 4),
        ]
        .into_iter()
        .map(|(edge, start_vertex, end_vertex)| AsmHistoricalEdge {
            edge,
            start_vertex,
            end_vertex,
        })
        .collect(),
        ..Default::default()
    };
    for (ordinal, edges) in [[10, 11, 12], [10, 13, 14], [11, 14, 15], [12, 15, 13]]
        .into_iter()
        .enumerate()
    {
        let face = 100 + i64::try_from(ordinal).unwrap();
        let loop_ = face + 100;
        let first = face * 10;
        topology.face_loops.push(AsmHistoricalRelation {
            owner_ref: face,
            member_refs: vec![loop_],
        });
        let coedges: Vec<_> = (first..first + 3).collect();
        topology.loop_coedges.push(AsmHistoricalRelation {
            owner_ref: loop_,
            member_refs: coedges.clone(),
        });
        for (position, edge) in edges.into_iter().enumerate() {
            topology.coedge_topology.push(AsmHistoricalCoedge {
                coedge: coedges[position],
                owner_loop: loop_,
                edge,
                next: coedges[(position + 1) % 3],
                previous: coedges[(position + 2) % 3],
                radial_next: 0,
            });
        }
    }
    topology
}

fn operand() -> DesignFaceOperand {
    let mut operand: DesignFaceOperand = serde_json::from_value(serde_json::json!({
        "id": "f3d:test:face-operand#21", "scope_record_index": 10, "scope_reference_ordinal": 1,
        "group_record_index": 20, "group_member_ordinal": 0, "record_index": 21, "byte_offset": 0,
        "class_tag": "282", "paired_byte_offset": 12, "paired_class_tag": "259",
        "recipe_record_index": 24, "recipe_record_byte_offset": 24, "recipe_id": "f3d:test:recipe#22",
        "recipe_prefix_offset": 35, "recipe_prefix_bytes": "", "recipe_references": [],
        "recipe_kind": "bounded_face", "recipe_program_offset": 0, "recipe_program": [0, -1, 3],
        "recipe_node_offsets": [], "recipe_nodes": [], "next_record_index": 23, "next_byte_offset": 36
    })).unwrap();
    for ordinal in 0..4 {
        operand
            .recipe_references
            .push(crate::records::dimensions::DesignRecipeReference {
                selector: 1,
                selector_offset: ordinal,
                token: format!("face{ordinal}"),
                token_offset: ordinal,
                design_reference: i64::try_from(ordinal).unwrap(),
                design_reference_offset: ordinal,
                candidate_faces: vec![crate::ids::brep_face_id(
                    100 + i64::try_from(ordinal).unwrap(),
                )],
                alternate_selector_faces: Vec::new(),
                candidate_edges: Vec::new(),
                alternate_selector_edges: Vec::new(),
            });
    }
    for (other, fans) in [
        (1, [[0, 1, 3], [0, 1, 2]]),
        (2, [[0, 1, 2], [0, 2, 3]]),
        (3, [[0, 2, 3], [0, 1, 3]]),
    ] {
        let mut fields = vec![0, -1, other, -1, 2, -1];
        for fan in fans {
            fields.extend([3, fan[0], -1, fan[1], -1, fan[2], -1, 0, 0, -1]);
        }
        let structure = crate::test_support::with_decode_context(|ctx| {
            crate::design::decode::operands::face_recipe_structure(ctx, &fields)
                .unwrap()
                .unwrap()
        });
        let mut program = vec![-1, -1, 2];
        program.extend(fields);
        operand.recipe_program.extend(&program);
        operand.recipe_nodes.push(DesignFaceRecipeNode {
            byte_offset: 0,
            end_byte_offset: 12,
            program,
            recipe_structure: Some(structure),
        });
    }
    operand
}

#[test]
fn bounded_face_nodes_resolve_the_complete_referenced_boundary() {
    let topology = topology();
    let operand = operand();
    assert_eq!(FaceRecipeIndex::new(&topology).resolve(&operand), Some(100));
    assert!(operand.preceding_candidate_faces.is_empty());
    assert!(operand.candidate_faces.is_empty());
}

#[test]
fn bounded_face_nodes_keep_grouped_reference_ordinals() {
    let topology = topology();
    let mut operand = operand();
    let mut additional = operand.recipe_references[0].clone();
    additional.design_reference = 999;
    additional.candidate_faces.clear();
    operand.recipe_references.insert(1, additional);
    assert_eq!(FaceRecipeIndex::new(&topology).resolve(&operand), Some(100));
}

#[test]
fn bounded_face_nodes_require_every_distinct_boundary_edge_and_fan() {
    let topology = topology();
    let index = FaceRecipeIndex::new(&topology);
    let mut operand = operand();
    operand.recipe_nodes[2] = operand.recipe_nodes[0].clone();
    assert_eq!(index.resolve(&operand), None);
    let mut operand = self::operand();
    operand.recipe_nodes[0]
        .recipe_structure
        .as_mut()
        .unwrap()
        .sides[0]
        .scalars[0] = 2;
    assert_eq!(index.resolve(&operand), None);
    let mut operand = self::operand();
    operand.recipe_nodes[0].recipe_structure = None;
    assert_eq!(index.resolve(&operand), None);
    let mut operand = self::operand();
    operand.recipe_program[2] = 4;
    assert_eq!(index.resolve(&operand), None);
}

#[test]
fn bounded_face_nodes_require_unique_topology_rows_and_candidate_faces() {
    let mut topology = topology();
    topology
        .edge_vertices
        .push(topology.edge_vertices[0].clone());
    assert_eq!(FaceRecipeIndex::new(&topology).resolve(&operand()), None);
    let mut topology = self::topology();
    topology.face_loops.push(topology.face_loops[0].clone());
    assert_eq!(FaceRecipeIndex::new(&topology).resolve(&operand()), None);
    let topology = self::topology();
    let mut operand = operand();
    operand.recipe_references[0].candidate_faces.clear();
    assert_eq!(FaceRecipeIndex::new(&topology).resolve(&operand), None);
}

#[test]
fn endpoint_fans_retain_multiplicity_and_cyclic_adjacency() {
    let lanes: Vec<BTreeSet<_>> = (10..14).map(|face| BTreeSet::from([face])).collect();
    assert!(fan_matches(&[0, 1, 2, 3], &[12, 13, 10, 11], &lanes));
    assert!(fan_matches(&[0, 1, 2, 3], &[13, 12, 11, 10], &lanes));
    assert!(!fan_matches(&[0, 1, 2, 3], &[10, 12, 11, 13], &lanes));
    assert!(!fan_matches(&[0, 0, 2, 3], &[10, 11, 12, 13], &lanes));
    assert!(!fan_matches(&[0, 1, 2], &[10, 11, 12, 13], &lanes));
}

#[test]
fn delete_face_selection_retains_native_group_until_every_member_has_boundary_proof() {
    use cadmpeg_ir::features::FaceSelection;
    let mut scope = crate::records::feature::scope::DesignParameterScope::empty(
        "f3d:test:scope#10",
        crate::records::feature::scope::DesignFeatureKind::DeleteFace,
        10,
    );
    scope
        .try_edit(|draft| {
            draft.feature_ordinal_offset = draft.paired_byte_offset - 76;
            draft.history_state_id = Some(2);
            draft.previous_history_state_id = Some(1);
            draft.previous_history_state_id_offset = Some(draft.feature_ordinal_offset + 30);
        })
        .unwrap();
    let group: crate::records::topology::construction::DesignConstructionOperandGroup =
        serde_json::from_value(serde_json::json!({
            "id": "f3d:test:group#20", "scope_record_index": 10, "scope_reference_ordinal": 0,
            "record_index": 20, "byte_offset": 0, "class_tag": "338", "role": 0,
            "members": [21], "member_offsets": [0], "frame": {
                "member_count_offset": 0, "opaque_index": 1, "opaque_index_offset": 18,
                "opaque_scalar": 0.0, "opaque_scalar_offset": 22, "variant": false
            }, "role_offset": 0, "paired_class_tag": "261", "paired_byte_offset": 0
        }))
        .unwrap();
    let native = FaceSelection::Native(group.id.clone());
    let mut selection = native.clone();
    let mut operand = operand();
    operand.candidate_faces = vec![crate::ids::brep_face_id(101)];
    operand.preceding_candidate_faces = operand.candidate_faces.clone();
    super::super::selection::bind_face_selection(
        &mut selection,
        &scope,
        std::slice::from_ref(&group),
        std::slice::from_ref(&operand),
        &[101],
    );
    assert_eq!(selection, native);
    operand.resolved_face_slots = FaceRecipeIndex::new(&topology())
        .resolve(&operand)
        .into_iter()
        .collect();
    super::super::selection::bind_face_selection(
        &mut selection,
        &scope,
        std::slice::from_ref(&group),
        std::slice::from_ref(&operand),
        &[101],
    );
    let feature = crate::ids::neutral_feature_id(&scope);
    let expected =
        crate::ids::history_input_face_id(&crate::ids::feature_input_prefix(&feature, 1), 100);
    assert!(
        matches!(selection, FaceSelection::Historical { faces, .. } if faces.as_slice() == [expected])
    );
}

#[test]
fn bounded_face_nodes_select_by_incidence_across_both_pair_lanes() {
    let mut operand = operand();
    operand.recipe_references.swap(0, 1);
    let remap = |value| match value {
        0 => 1,
        1 => 0,
        other => other,
    };
    for node in &mut operand.recipe_nodes {
        let node = node.recipe_structure.as_mut().unwrap();
        node.root = remap(node.root);
        node.prelude[0] = remap(node.prelude[0]);
        for side in &mut node.sides {
            side.header_value = remap(side.header_value);
            for scalar in &mut side.scalars {
                *scalar = remap(*scalar);
            }
        }
    }
    assert_eq!(
        FaceRecipeIndex::new(&topology()).resolve(&operand),
        Some(100)
    );
}

#[test]
fn bounded_face_nodes_bind_two_edge_loops_with_two_shared_vertices() {
    let mut topology = topology();
    topology.faces = vec![100, 101, 102];
    topology.edge_vertices.truncate(3);
    for edge in &mut topology.edge_vertices {
        edge.start_vertex = 1;
        edge.end_vertex = 2;
    }
    topology.face_loops.truncate(3);
    topology.loop_coedges.truncate(3);
    topology.coedge_topology.clear();
    for (ordinal, edges) in [[10, 11], [11, 12], [12, 10]].into_iter().enumerate() {
        let loop_ = &mut topology.loop_coedges[ordinal];
        loop_.member_refs.truncate(2);
        for (index, edge) in edges.into_iter().enumerate() {
            topology.coedge_topology.push(AsmHistoricalCoedge {
                coedge: loop_.member_refs[index],
                owner_loop: loop_.owner_ref,
                edge,
                next: loop_.member_refs[1 - index],
                previous: loop_.member_refs[1 - index],
                radial_next: 0,
            });
        }
    }
    let mut operand = operand();
    operand.recipe_program[2] = 2;
    operand.recipe_nodes.truncate(2);
    for (index, node) in operand.recipe_nodes.iter_mut().enumerate() {
        let node = node.recipe_structure.as_mut().unwrap();
        node.root = 0;
        node.prelude[0] = i32::try_from(index + 1).unwrap();
        for side in &mut node.sides {
            side.header_value = 0;
            side.scalars = vec![1, 2];
        }
    }
    assert_eq!(FaceRecipeIndex::new(&topology).resolve(&operand), Some(100));
}
