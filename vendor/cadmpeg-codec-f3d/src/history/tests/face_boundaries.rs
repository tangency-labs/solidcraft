// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]
#![allow(
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::if_not_else,
    clippy::needless_pass_by_value,
    clippy::range_plus_one,
    clippy::semicolon_if_nothing_returned,
    clippy::trivially_copy_pass_by_ref
)]

use crate::history::{
    cyclic_point_subsequence, resolve_direct_face_recipe_clauses,
    selection::bind_edge_identity_bounded_face_rules,
};
use crate::history_records::AsmHistoricalTopology;

#[test]
fn direct_face_recipe_clauses_resolve_ordered_changed_intersections() {
    use cadmpeg_ir::ids::FaceId;

    let reference =
        |selector_offset, candidates: &[i64]| crate::records::dimensions::DesignRecipeReference {
            selector: 1,
            selector_offset,
            token: "x".into(),
            token_offset: selector_offset + 1,
            design_reference: 1,
            design_reference_offset: selector_offset + 2,
            candidate_faces: candidates
                .iter()
                .map(|face| {
                    FaceId::mint(format!("f3d:brep:entity#{face}")).expect("identity grammar")
                })
                .collect(),
            candidate_edges: Vec::new(),
            alternate_selector_faces: Vec::new(),
            alternate_selector_edges: Vec::new(),
        };
    let references = [
        reference(10, &[1, 2]),
        reference(10, &[2, 3]),
        reference(20, &[4, 5]),
        reference(20, &[4, 6]),
        reference(30, &[2]),
    ];
    let topology = AsmHistoricalTopology {
        faces: vec![1, 2, 3, 4, 5, 6],
        ..AsmHistoricalTopology::default()
    };

    assert_eq!(
        resolve_direct_face_recipe_clauses(&references, &topology, &[2, 4].into_iter().collect()),
        [2, 4]
    );
    assert!(
        resolve_direct_face_recipe_clauses(&references, &topology, &[2].into_iter().collect())
            .is_empty()
    );
}

#[test]
fn bounded_face_copy_matches_cyclic_boundary_with_split_vertices() {
    use cadmpeg_ir::math::Point3;

    let point = |x, y| Point3 { x, y, z: 0.0 };
    let source = [
        point(0.0, 0.0),
        point(2.0, 0.0),
        point(2.0, 2.0),
        point(0.0, 2.0),
    ];
    let split_copy = [
        point(2.0, 2.0),
        point(1.0, 2.0),
        point(0.0, 2.0),
        point(0.0, 0.0),
        point(2.0, 0.0),
    ];
    assert!(cyclic_point_subsequence(&source, &split_copy));

    let reversed = split_copy.iter().copied().rev().collect::<Vec<_>>();
    assert!(cyclic_point_subsequence(&source, &reversed));

    let wrong_order = [
        point(0.0, 0.0),
        point(2.0, 2.0),
        point(2.0, 0.0),
        point(0.0, 2.0),
    ];
    assert!(!cyclic_point_subsequence(&source, &wrong_order));
    assert!(!cyclic_point_subsequence(&source, &split_copy[..3]));
}

fn bounded_face_rule_fixture() -> (
    Vec<crate::records::topology::edge_identity::DesignEdgeIdentityOperand>,
    crate::records::topology::face::DesignFaceOperand,
) {
    use crate::records::{
        recipes::ConstructionRecipeKind,
        topology::{
            edge_identity::DesignEdgeIdentityOperand, face::DesignFaceOperand,
            historical_context::DesignHistoricalFaceBoundaryContext,
            historical_context::DesignHistoricalFaceLoopContext,
            historical_context::DesignHistoricalFaceSupportContext,
        },
    };

    let identities = vec![DesignEdgeIdentityOperand::try_new(
        crate::records::topology::edge_identity::DesignEdgeIdentityOperandDraft {
            id: "f3d:Design/BulkStream.dat:edge-identity#10".into(),
            scope_record_index: 1,
            group_record_index: 2,
            group_member_ordinal: 0,
            record_index: 10,
            byte_offset: 100,
            class_tag: crate::records::references::DesignClassTag::try_from("297".to_owned())
                .unwrap(),
            layout: crate::records::topology::edge_identity::DesignEdgeIdentityLayout::Full,
            local_id: 13,
            asset_id: crate::records::mesh::DesignRelaxedGuidText::try_from(
                "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d".to_owned(),
            )
            .unwrap(),
            asset_id_offset: 142,
            context_id: crate::records::mesh::DesignRelaxedGuidText::try_from(
                "1b2c3d4e-5f6a-4b7c-8d9e-0f1a2b3c4d5e".to_owned(),
            )
            .unwrap(),
            context_id_offset: 218,
            historical: None,
            treatment_radius_candidates: Vec::new(),
            transition_edge_candidates: vec![7, 8, 9],
            resolved_edge_slots: Vec::new(),
            resolved_edge_slot: None,
            resolution_identity_id: None,
        },
    )
    .unwrap()];
    let face = DesignFaceOperand::try_new(crate::records::topology::face::DesignFaceOperandDraft {
        id: "f3d:Design/BulkStream.dat:design-face-operand#10".into(),
        scope_record_index: 1,
        scope_reference_ordinal: 0,
        group: Some(crate::records::topology::body_recipe::DesignOperandGroup {
            group_record_index: 2,
            group_member_ordinal: 0,
        }),
        record_index: 10,
        byte_offset: 100,
        class_tag: crate::records::references::DesignClassTag::try_from("297".to_owned()).unwrap(),
        paired_byte_offset: 200,
        paired_class_tag: crate::records::references::DesignClassTag::try_from("259".to_owned())
            .unwrap(),
        recipe_record_index: 13,
        recipe_record_byte_offset: 300,
        recipe_id: "recipe".into(),
        recipe_prefix_offset: 311,
        recipe_prefix_bytes: Vec::new(),
        recipe_references: Vec::new(),
        recipe_kind: ConstructionRecipeKind::BoundedFace,
        recipe_program_offset: 0,
        recipe_program: vec![0],

        recipe_nodes: Vec::new(),
        candidate_faces: Vec::new(),
        unreferenced_candidate_faces: Vec::new(),
        alternate_selector_candidate_faces: Vec::new(),
        preceding_candidate_faces: Vec::new(),
        changed_candidate_faces: Vec::new(),
        historical_support_contexts: vec![DesignHistoricalFaceSupportContext {
            active_face_slot: 30,
            surface_slot: 40,
            preceding_face_slots: vec![50],
            preceding_face_boundaries: vec![DesignHistoricalFaceBoundaryContext {
                face_slot: 50,
                loops: vec![DesignHistoricalFaceLoopContext {
                    loop_slot: 60,
                    boundary: crate::records::topology::historical_context::DesignHistoricalLoopBoundary::Coedges(
                        vec![
                            crate::records::topology::historical_context::DesignHistoricalLoopCoedge {
                                coedge_slot: 70,
                                edge_slot: 8,
                            },
                            crate::records::topology::historical_context::DesignHistoricalLoopCoedge {
                                coedge_slot: 71,
                                edge_slot: 6,
                            },
                            crate::records::topology::historical_context::DesignHistoricalLoopCoedge {
                                coedge_slot: 72,
                                edge_slot: 7,
                            },
                        ],
                    ),
                }],
            }],
            changed_preceding_face_slots: vec![50],
        }],
        resolved_face_slots: Vec::new(),
        resolved_active_face: None,
        next_record_index: 14,
        next_byte_offset: 400,
    })
    .unwrap();
    (identities, face)
}

#[test]
fn bounded_face_identity_selects_ordered_deleted_treatment_edges() {
    let (mut identities, face) = bounded_face_rule_fixture();
    bind_edge_identity_bounded_face_rules(&mut identities, &[face.clone()]);
    assert_eq!(identities[0].resolved_edge_slots, [8, 7]);
    assert_eq!(
        identities[0].resolution_identity_id.as_deref(),
        Some(face.id.as_str())
    );

    let mut inconsistent = face;
    inconsistent.historical_support_contexts[0]
        .changed_preceding_face_slots
        .clear();
    bind_edge_identity_bounded_face_rules(&mut identities, &[inconsistent]);
    assert!(identities[0].resolved_edge_slots.is_empty());
}
