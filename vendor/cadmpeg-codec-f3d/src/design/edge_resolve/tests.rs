// SPDX-License-Identifier: Apache-2.0
#![allow(
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::trivially_copy_pass_by_ref,
    clippy::uninlined_format_args
)]

use super::contextual_deleted_edge_group_candidates;
use super::deleted_boundary_edge_group_candidates;
use super::partial_historical_edge_selection;
use super::project_fixed_fillet;
use super::radius_edge_identity_group_candidates;
use super::resolved_edge_flange_group;
use super::resolved_edge_group;
use super::resolved_edge_operand;
use super::resolved_edge_treatment_group;
use super::resolved_edge_treatment_group_with_corners;
use super::resolved_surface_patch_edge_group;
use super::result_boundary_reference_edge_group_candidates;
use super::surface_patch_grouped_recipe_edges;
use super::transition_chain_is_supported_by_recipe;
use super::unique_hem_transition_edge_candidate;
use super::SurfacePatchRecipeEdges;
use crate::records::dimensions::DesignRecipeReference;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::topology::construction::DesignConstructionOperandGroup;
use crate::records::topology::edge_identity::DesignEdgeIdentityOperand;
use crate::records::topology::edge_identity::DesignEdgeOperand;
use cadmpeg_ir::ids::EdgeId;

fn identity(record_index: u32, candidates: &[(i64, f64)]) -> DesignEdgeIdentityOperand {
    serde_json::from_value(serde_json::json!({
        "id": format!("f3d:test:identity#{record_index}"),
        "scope_record_index": 1,
        "group_record_index": 2,
        "group_member_ordinal": record_index,
        "record_index": record_index,
        "byte_offset": 0,
        "class_tag": "277",
        "compact_layout": true,
        "local_id": record_index,
        "local_id_offset": 23,
        "asset_id": "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d",
        "asset_id_offset": 41,
        "context_id": "1b2c3d4e-5f6a-4b7c-8d9e-0f1a2b3c4d5e",
        "context_id_offset": 117,
        "transition_edge_candidates": candidates
            .iter()
            .map(|(edge, _)| *edge)
            .collect::<Vec<_>>(),
        "treatment_radius_candidates": candidates
            .iter()
            .map(|(edge, radius)| serde_json::json!({
                "edge_slot": edge,
                "radius": radius
            }))
            .collect::<Vec<_>>()
    }))
    .expect("edge identity")
}

#[test]
fn identity_radius_candidates_require_member_local_evidence() {
    let mut first = identity(10, &[(17, 3.0), (18, 5.0)]);
    let mut second = identity(11, &[(19, 3.0), (20, 5.0)]);
    assert_eq!(
        radius_edge_identity_group_candidates(&[&first], 3.0),
        Some(vec![17])
    );
    assert_eq!(
        radius_edge_identity_group_candidates(&[&first, &second], 3.0),
        None
    );
    let copied = identity(11, &[(17, 3.0), (18, 5.0)]);
    assert_eq!(
        radius_edge_identity_group_candidates(&[&first, &copied], 3.0),
        None
    );

    first.resolved_edge_slot = Some(17);
    second.resolved_edge_slots = vec![19, 20];
    assert_eq!(
        radius_edge_identity_group_candidates(&[&first, &second], 3.0),
        Some(vec![17, 19, 20])
    );
}

fn fixed_scope() -> DesignParameterScope {
    serde_json::from_value(serde_json::json!({
        "id": "f3d:test:scope",
        "byte_offset": 0,
        "class_tag": "300",
        "record_index": 1,
        "frame_length": 200,
        "kind": "Fillet",
        "kind_offset": 32,
        "feature_ordinal": 1,
        "feature_ordinal_offset": 128,
        "history_state_id": 8,
        "history_state_id_offset": 24,
        "previous_history_state_id": 7,
        "previous_history_state_id_offset": 158,
        "reference_count_offset": 9,
        "reference_members": [2],
        "reference_member_offsets": [14],
        "fixed_fillet_parameters": {
            "groups": [{
                "tangency_weight": {
                    "value": 1.0,
                    "record_index": 4,
                    "value_offset": 0
                },
                "radii": [0.3],
                "radius_record_indexes": [5],
                "radius_offsets": [0],
                "intermediate_parameters": [],
                "intermediate_parameter_record_indexes": [],
                "intermediate_parameter_offsets": []
            }]
        },
        "paired_class_tag": "261",
        "paired_byte_offset": 200
    }))
    .expect("fixed Fillet scope")
}

fn group(record_index: u32, member: u32) -> DesignConstructionOperandGroup {
    serde_json::from_value(serde_json::json!({
        "id": format!("f3d:test:group-{record_index}"),
        "scope_record_index": 1,
        "scope_reference_ordinal": 0,
        "record_index": record_index,
        "byte_offset": 0,
        "class_tag": "300",
        "members": [member],
        "member_offsets": [0],
        "frame": {
            "layout": "counted",
            "member_count_offset": 0,
            "identity_record_index": 9,
            "identity_record_offset": 0,
            "opaque_index": 1,
            "opaque_index_offset": 18,
            "opaque_scalar": 1.0,
            "opaque_scalar_offset": 22,
            "variant": false
        },
        "role": 0x10_0000_0000u64,
        "role_offset": 0,
        "paired_class_tag": "258",
        "paired_byte_offset": 0
    }))
    .expect("construction operand group")
}

#[test]
fn sole_compact_identity_group_projects_fixed_fillet_transition_chain() {
    let scope = fixed_scope();
    let group = group(2, 10);
    let identity = identity(10, &[(17, 3.0), (18, 5.0), (19, 3.0)]);
    let definition = crate::test_support::with_decode_context(|decode_ctx| {
        project_fixed_fillet(&scope, &[group], &[], &[identity], decode_ctx)
    })
    .unwrap()
    .expect("fixed Fillet from sole compact identity group");
    let cadmpeg_ir::features::FeatureDefinition::Operation(
        cadmpeg_ir::features::FeatureOperation::Fillet { groups },
    ) = definition
    else {
        panic!("expected Fillet");
    };
    assert!(matches!(
        groups.as_slice(),
        [cadmpeg_ir::features::edge_treatments::FilletGroup {
            edges: cadmpeg_ir::features::EdgeSelection::Historical { edges, .. },
            radius: cadmpeg_ir::features::edge_treatments::RadiusSpec::Constant {
                radius: actual_radius
            },
            tangency_weight: Some(weight),
        }] if weight.get() == 1.0 && (edges.len() == 2) && actual_radius.get() == 3.0
    ));
}

#[test]
fn full_layout_identity_does_not_assign_the_fixed_fillet_edge_role() {
    let scope = fixed_scope();
    let group = group(2, 10);
    let mut identity = identity(10, &[(17, 3.0), (19, 3.0)]);
    let mut draft = identity.into_draft();
    draft.layout = crate::records::topology::edge_identity::DesignEdgeIdentityLayout::Full;
    draft.asset_id_offset = draft.byte_offset + draft.layout.local_id_offset() + 18;
    draft.context_id_offset = draft.asset_id_offset + 76;
    identity =
        crate::records::topology::edge_identity::DesignEdgeIdentityOperand::try_new(draft).unwrap();

    assert!(
        crate::test_support::with_decode_context(|decode_ctx| project_fixed_fillet(
            &scope,
            &[group],
            &[],
            &[identity],
            decode_ctx
        ))
        .unwrap()
        .is_none()
    );
}

#[test]
fn only_edge_treatments_use_single_member_transition_chains() {
    let group = group(2, 10);
    let mut generic_identity = identity(10, &[(17, 3.0), (19, 3.0)]);
    let mut draft = generic_identity.into_draft();
    draft.layout = crate::records::topology::edge_identity::DesignEdgeIdentityLayout::Full;
    draft.asset_id_offset = draft.byte_offset + draft.layout.local_id_offset() + 18;
    draft.context_id_offset = draft.asset_id_offset + 76;
    generic_identity =
        crate::records::topology::edge_identity::DesignEdgeIdentityOperand::try_new(draft).unwrap();
    generic_identity.treatment_radius_candidates.clear();
    let generic_feature_id =
        cadmpeg_ir::features::FeatureId::mint("f3d:model:feature#ruled-surface")
            .expect("identity grammar");

    assert!(matches!(
        crate::test_support::with_decode_context(|decode_ctx| resolved_edge_group(
            &group,
            std::slice::from_ref(&group),
            &[],
            &[generic_identity],
            Some(7),
            &generic_feature_id,
            decode_ctx
        ))
        .unwrap(),
        cadmpeg_ir::features::EdgeSelection::Native(_)
    ));
    let treatment_feature_id = cadmpeg_ir::features::FeatureId::mint("f3d:model:feature#fillet")
        .expect("identity grammar");
    let mut identity = identity(10, &[(17, 3.0), (19, 3.0)]);
    let mut draft = identity.into_draft();
    draft.layout = crate::records::topology::edge_identity::DesignEdgeIdentityLayout::Full;
    draft.asset_id_offset = draft.byte_offset + draft.layout.local_id_offset() + 18;
    draft.context_id_offset = draft.asset_id_offset + 76;
    identity =
        crate::records::topology::edge_identity::DesignEdgeIdentityOperand::try_new(draft).unwrap();
    identity.treatment_radius_candidates.clear();
    assert!(matches!(
        resolved_edge_treatment_group(
            &group,
            std::slice::from_ref(&group),
            &[],
            &[identity],
            Some(7),
            &treatment_feature_id,
            None,
        ).unwrap(),
        cadmpeg_ir::features::EdgeSelection::Historical { edges, .. }
            if edges.as_slice() == [
                cadmpeg_ir::ids::HistoricalEdgeId::mint("f3d:history-input:edge#6:fillet:7:17").expect("identity grammar"),
                cadmpeg_ir::ids::HistoricalEdgeId::mint("f3d:history-input:edge#6:fillet:7:19").expect("identity grammar"),
            ]
    ));
}

#[test]
fn multiple_full_layout_members_do_not_use_the_operation_transition_chain() {
    let mut selection_group = group(2, 10);
    selection_group
        .try_set_members(vec![
            crate::records::identity::Located {
                value: 10,
                offset: 0,
            },
            crate::records::identity::Located {
                value: 11,
                offset: 11,
            },
        ])
        .unwrap();
    let mut first = identity(10, &[(17, 1.0), (18, 1.0), (19, 1.0)]);
    let mut draft = first.into_draft();
    draft.layout = crate::records::topology::edge_identity::DesignEdgeIdentityLayout::Full;
    draft.asset_id_offset = draft.byte_offset + draft.layout.local_id_offset() + 18;
    draft.context_id_offset = draft.asset_id_offset + 76;
    first =
        crate::records::topology::edge_identity::DesignEdgeIdentityOperand::try_new(draft).unwrap();
    let mut second = identity(11, &[(17, 1.0), (18, 1.0), (19, 1.0)]);
    let mut draft = second.into_draft();
    draft.layout = crate::records::topology::edge_identity::DesignEdgeIdentityLayout::Full;
    draft.asset_id_offset = draft.byte_offset + draft.layout.local_id_offset() + 18;
    draft.context_id_offset = draft.asset_id_offset + 76;
    second =
        crate::records::topology::edge_identity::DesignEdgeIdentityOperand::try_new(draft).unwrap();
    second.group_member_ordinal = 1;
    let feature_id = cadmpeg_ir::features::FeatureId::mint("f3d:model:feature#fillet")
        .expect("identity grammar");

    assert!(matches!(
        resolved_edge_treatment_group(
            &selection_group,
            std::slice::from_ref(&selection_group),
            &[],
            &[first, second],
            Some(7),
            &feature_id,
            None,
        )
        .unwrap(),
        cadmpeg_ir::features::EdgeSelection::Native(_)
    ));
}

fn recipe_edge_operand(
    record_index: u32,
    changed_boundary_edge_slots: &[i64],
    deleted_boundary_edge_slots: &[i64],
) -> DesignEdgeOperand {
    serde_json::from_value(serde_json::json!({
        "id": format!("f3d:test:edge-operand#{record_index}"),
        "scope_record_index": 1,
        "scope_reference_ordinal": 0,
        "record_index": record_index,
        "byte_offset": 0,
        "class_tag": "297",
        "paired_byte_offset": 16,
        "paired_class_tag": "259",
        "recipe_record_index": record_index + 3,
        "recipe_record_byte_offset": 32,
        "recipe_id": "f3d:test:recipe",
        "recipe_prefix_offset": 43,
        "recipe_prefix_bytes": "",
        "recipe_references": [],
        "recipe_program_offset": 0,
        "recipe_program": [],
        "changed_boundary_edge_slots": changed_boundary_edge_slots,
        "deleted_boundary_edge_slots": deleted_boundary_edge_slots,
        "next_record_index": record_index + 4,
        "next_byte_offset": 160,
    }))
    .expect("edge recipe operand")
}

#[test]
fn unresolved_standard_recipe_is_not_replaced_by_identity_or_transition_context() {
    let selection_group = group(2, 10);
    let mut operand = recipe_edge_operand(10, &[], &[]);
    operand.recipe_state_id = Some(7);
    operand.recipe_structure = Some(
        crate::records::topology::edge_recipe::DesignEdgeRecipeStructure {
            root: 1,
            sides: vec![
                crate::records::topology::edge_recipe::DesignTopologyRecipeSide {
                    header_value: 1,
                    scalars: Vec::new(),
                    payload_prefix: Vec::new(),

                    entries: Vec::new(),
                },
            ],
        },
    );
    let mut persistent_identity = identity(10, &[]);
    persistent_identity.resolved_edge_slot = Some(19);
    let feature_id = cadmpeg_ir::features::FeatureId::mint("f3d:model:feature#fillet")
        .expect("identity grammar");

    assert!(matches!(
        resolved_edge_treatment_group(
            &selection_group,
            std::slice::from_ref(&selection_group),
            std::slice::from_ref(&operand),
            std::slice::from_ref(&persistent_identity),
            Some(7),
            &feature_id,
            None,
        )
        .unwrap(),
        cadmpeg_ir::features::EdgeSelection::Native(_)
    ));

    operand.changed_boundary_edge_slots = vec![17];
    operand.deleted_boundary_edge_slots = vec![17];
    assert!(matches!(
        resolved_edge_treatment_group(
            &selection_group,
            std::slice::from_ref(&selection_group),
            &[operand],
            &[],
            Some(7),
            &feature_id,
            None,
        )
        .unwrap(),
        cadmpeg_ir::features::EdgeSelection::Native(_)
    ));
}

#[test]
fn unstructured_recipe_is_not_replaced_by_identity_or_transition_context() {
    let selection_group = group(2, 10);
    let mut operand = recipe_edge_operand(10, &[17], &[17]);
    operand.recipe_program = vec![1];
    operand.recipe_state_id = Some(7);
    let mut persistent_identity = identity(10, &[(17, 1.0)]);
    persistent_identity.resolved_edge_slot = Some(17);
    let feature_id = cadmpeg_ir::features::FeatureId::mint("f3d:model:feature#fillet")
        .expect("identity grammar");

    assert!(matches!(
        resolved_edge_treatment_group(
            &selection_group,
            std::slice::from_ref(&selection_group),
            std::slice::from_ref(&operand),
            std::slice::from_ref(&persistent_identity),
            Some(7),
            &feature_id,
            None,
        )
        .unwrap(),
        cadmpeg_ir::features::EdgeSelection::Native(_)
    ));
}

#[test]
fn treatment_corner_context_admits_only_edge_endpoints_and_collapses_recipe_repeats() {
    use crate::history_records::{AsmDeltaState, AsmHistoricalEdge, AsmHistoricalTopology};
    use crate::records::feature::work_geometry::{
        DesignEdgeTreatmentVertexOperand, DesignVertexRecipe,
    };

    let mut selection_group = group(2, 10);
    selection_group
        .try_set_members(vec![
            crate::records::identity::Located {
                value: 10,
                offset: 0,
            },
            crate::records::identity::Located {
                value: 11,
                offset: 11,
            },
            crate::records::identity::Located {
                value: 12,
                offset: 22,
            },
            crate::records::identity::Located {
                value: 13,
                offset: 33,
            },
        ])
        .unwrap();
    let mut first_edge = recipe_edge_operand(11, &[], &[]);
    first_edge.recipe_state_id = Some(7);
    first_edge.resolved_edge_slot = Some(17);
    let mut repeated_edge = recipe_edge_operand(13, &[], &[]);
    repeated_edge.recipe_state_id = Some(7);
    repeated_edge.resolved_edge_slot = Some(17);
    let corner = |record_index, group_member_ordinal, vertex| DesignEdgeTreatmentVertexOperand {
        id: format!("f3d:test:edge-treatment-vertex-operand#{record_index}"),
        scope_record_index: 1,
        scope_reference_ordinal: group_member_ordinal,
        group_record_index: 2,
        group_member_ordinal,
        recipe: DesignVertexRecipe::try_new(
            crate::records::feature::work_geometry::DesignVertexRecipeDraft {
                record_index,
                byte_offset: u64::from(record_index),
                class_tag: crate::records::references::DesignClassTag::try_from("306".to_owned())
                    .unwrap(),
                paired_byte_offset: u64::from(record_index) + 16,
                paired_class_tag: crate::records::references::DesignClassTag::try_from(
                    "261".to_owned(),
                )
                .unwrap(),
                recipe_record_index: record_index + 3,
                recipe_record_byte_offset: u64::from(record_index) + 32,
                recipe_id: format!("f3d:test:construction-recipe#{record_index}"),
                recipe_prefix_offset: u64::from(record_index) + 43,
                recipe_prefix_bytes: Vec::new(),
                recipe_references: Vec::new(),
                recipe_program_offset: 4,
                recipe_program: vec![0],
                resolution: Some(
                    crate::records::feature::work_geometry::DesignVertexResolution::new(7, vertex)
                        .expect("valid vertex slot"),
                ),
                next_record_index: record_index + 5,
                next_byte_offset: u64::from(record_index) + 200,
            },
        )
        .unwrap(),
    };
    let state = (
        AsmDeltaState {
            id: "f3d:test:state#7".into(),
            parent: "f3d:test:history".into(),
            byte_offset: 0,
            state_id: 7,
            version_flag: 1,
            state_flag: 0,
            previous_ref: None,
            next_ref: None,
            node_index: 7,
            partner_ref: None,
            owner_ref: 0,
            bulletin_boards: Vec::new(),
            records: Vec::new(),
            transition: None,
        },
        Some(AsmHistoricalTopology {
            edges: vec![17],
            vertices: vec![3, 4, 5],
            edge_vertices: vec![AsmHistoricalEdge {
                edge: 17,
                start_vertex: 3,
                end_vertex: 4,
            }],
            ..AsmHistoricalTopology::default()
        }),
        Vec::new(),
    );
    let history = crate::test_support::history::history(
        "f3d:test:history".into(),
        0,
        None,
        false,
        vec![state],
    );
    let feature_id = cadmpeg_ir::features::FeatureId::mint("f3d:model:feature#fillet")
        .expect("identity grammar");
    let corners = [corner(10, 0, 3), corner(12, 2, 4)];
    let edges = [first_edge.clone(), repeated_edge.clone()];

    assert!(matches!(
            crate::test_support::with_decode_context(|decode_ctx| resolved_edge_treatment_group_with_corners(
    &selection_group,
    crate::design::edge_resolve::EdgeTreatmentInputs { groups: std::slice::from_ref(&selection_group), operands: &edges, identity_operands: &[], vertex_operands: &corners, histories: std::slice::from_ref(&history), previous_state_id: Some(7), feature_id: &feature_id, treatment_radius: None },
    decode_ctx,
    )).unwrap(),
            cadmpeg_ir::features::EdgeSelection::Historical { edges, .. }
                if edges.len() == 1 && edges[0].as_str().ends_with(":17")
        ));

    let invalid_corners = [corner(10, 0, 3), corner(12, 2, 5)];
    assert!(matches!(
        crate::test_support::with_decode_context(|decode_ctx| {
            resolved_edge_treatment_group_with_corners(
                &selection_group,
                crate::design::edge_resolve::EdgeTreatmentInputs {
                    groups: std::slice::from_ref(&selection_group),
                    operands: &[first_edge, repeated_edge],
                    identity_operands: &[],
                    vertex_operands: &invalid_corners,
                    histories: std::slice::from_ref(&history),
                    previous_state_id: Some(7),
                    feature_id: &feature_id,
                    treatment_radius: None,
                },
                decode_ctx,
            )
        })
        .unwrap(),
        cadmpeg_ir::features::EdgeSelection::Native(_)
    ));
}

fn recipe_reference(candidate_edges: &[i64]) -> DesignRecipeReference {
    DesignRecipeReference {
        selector: 1,
        selector_offset: 0,
        token: "97".into(),
        token_offset: 0,
        design_reference: 1,
        design_reference_offset: 0,
        candidate_faces: Vec::new(),
        candidate_edges: candidate_edges
            .iter()
            .map(|edge| EdgeId::mint(format!("f3d:test:edge#{edge}")).expect("identity grammar"))
            .collect(),
        alternate_selector_faces: Vec::new(),
        alternate_selector_edges: Vec::new(),
    }
}

fn standard_patch_operand(
    record: u32,
    header: i32,
    scalars: Vec<i32>,
    references: Vec<DesignRecipeReference>,
) -> DesignEdgeOperand {
    let mut operand = recipe_edge_operand(record, &[], &[]);
    operand.recipe_references = references;
    operand.recipe_structure = Some(
        crate::records::topology::edge_recipe::DesignEdgeRecipeStructure {
            root: 1,
            sides: vec![
                crate::records::topology::edge_recipe::DesignTopologyRecipeSide {
                    header_value: header,
                    scalars,
                    payload_prefix: vec![0],
                    entries: Vec::new(),
                },
            ],
        },
    );
    operand
}

#[test]
fn grouped_surface_patch_recipe_uses_roles_instead_of_reference_order() {
    let first = standard_patch_operand(
        10,
        2,
        vec![0],
        vec![
            recipe_reference(&[88]),
            recipe_reference(&[17]),
            recipe_reference(&[99]),
        ],
    );
    let second = standard_patch_operand(
        11,
        0,
        vec![3],
        vec![
            recipe_reference(&[99]),
            recipe_reference(&[]),
            recipe_reference(&[18]),
        ],
    );
    assert_eq!(
        surface_patch_grouped_recipe_edges(&[&first, &second], &[]),
        SurfacePatchRecipeEdges::Resolved(vec![
            EdgeId::mint("f3d:test:edge#17").expect("identity grammar"),
            EdgeId::mint("f3d:test:edge#18").expect("identity grammar")
        ])
    );
    for (header, scalars) in [
        (1, vec![0, 0]),
        (0, vec![1, 0]),
        (0, vec![0, 1]),
        (1, vec![1, 0]),
    ] {
        let operand = standard_patch_operand(10, header, scalars, vec![recipe_reference(&[17])]);
        assert!(matches!(
            surface_patch_grouped_recipe_edges(&[&operand], &[]),
            SurfacePatchRecipeEdges::Resolved(_)
        ));
    }
}

#[test]
fn grouped_surface_patch_recipe_rejects_ambiguous_or_repeated_edges() {
    let ambiguous = standard_patch_operand(10, 1, vec![], vec![recipe_reference(&[17, 18])]);
    let contradictory = standard_patch_operand(
        11,
        1,
        vec![2],
        vec![recipe_reference(&[18]), recipe_reference(&[19])],
    );
    let absent = recipe_edge_operand(12, &[], &[]);
    let repeated = standard_patch_operand(11, 1, vec![], vec![recipe_reference(&[17])]);

    assert_eq!(
        surface_patch_grouped_recipe_edges(&[&ambiguous], &[]),
        SurfacePatchRecipeEdges::Inconclusive
    );
    assert_eq!(
        surface_patch_grouped_recipe_edges(&[&contradictory], &[]),
        SurfacePatchRecipeEdges::Inconclusive
    );
    assert_eq!(
        surface_patch_grouped_recipe_edges(&[&absent, &contradictory], &[]),
        SurfacePatchRecipeEdges::Inconclusive
    );
    assert_eq!(
        surface_patch_grouped_recipe_edges(&[&repeated, &repeated], &[]),
        SurfacePatchRecipeEdges::Inconclusive
    );
}

#[test]
fn grouped_surface_patch_recipe_projects_historical_edges() {
    let mut group = group(2, 10);
    group
        .try_set_members(vec![
            crate::records::identity::Located {
                value: 10,
                offset: 0,
            },
            crate::records::identity::Located {
                value: 11,
                offset: 11,
            },
        ])
        .unwrap();
    let first = standard_patch_operand(10, 1, vec![], vec![recipe_reference(&[17])]);
    let second = standard_patch_operand(11, 1, vec![], vec![recipe_reference(&[18])]);
    let feature_id = cadmpeg_ir::features::FeatureId::mint("f3d:model:feature#surface-patch")
        .expect("identity grammar");

    let selection = crate::test_support::with_decode_context(|decode_ctx| {
        resolved_surface_patch_edge_group(
            &group,
            std::slice::from_ref(&group),
            &[first, second],
            &[],
            &[],
            super::EdgeGroupTransition {
                previous_state_id: Some(7),
                feature_id: &feature_id,
            },
            decode_ctx,
        )
    })
    .unwrap();
    let prefix = crate::ids::history_input_prefix(&cadmpeg_ir::identity_key!("surface-patch"), 7);
    assert!(matches!(
        selection,
        cadmpeg_ir::features::EdgeSelection::Historical { edges, .. }
            if edges.as_slice() == [
                crate::ids::history_input_edge_id(&prefix, 17),
                crate::ids::history_input_edge_id(&prefix, 18),
            ]
    ));
}

#[test]
fn contradictory_surface_patch_references_suppress_generic_resolution() {
    let group = group(2, 10);
    let mut operand = standard_patch_operand(
        10,
        1,
        vec![2],
        vec![recipe_reference(&[17]), recipe_reference(&[18])],
    );
    operand.resolved_edge_slot = Some(17);
    let feature_id = cadmpeg_ir::features::FeatureId::mint("f3d:model:feature#surface-patch")
        .expect("identity grammar");

    assert!(matches!(
        crate::test_support::with_decode_context(|decode_ctx| resolved_surface_patch_edge_group(
            &group,
            std::slice::from_ref(&group),
            std::slice::from_ref(&operand),
            &[],
            &[],
            super::EdgeGroupTransition {
                previous_state_id: Some(7),
                feature_id: &feature_id
            },
            decode_ctx
        ))
        .unwrap(),
        cadmpeg_ir::features::EdgeSelection::Native(_)
    ));
}

fn alternate_patch_operand() -> (
    DesignEdgeOperand,
    crate::records::topology::historical_context::AsmFaceBoundary,
) {
    use crate::records::topology::edge_recipe::{
        DesignSurfacePatchRecipeClause, DesignSurfacePatchRecipeStructure,
    };
    let mut operand = recipe_edge_operand(10, &[], &[]);
    operand.recipe_references = vec![
        recipe_reference(&[17, 18]),
        recipe_reference(&[]),
        recipe_reference(&[]),
        recipe_reference(&[19]),
        recipe_reference(&[]),
        recipe_reference(&[20]),
    ];
    operand.recipe_references[1].candidate_faces =
        vec![cadmpeg_ir::ids::FaceId::mint("f3d:test:face#91").expect("identity grammar")];
    operand.surface_patch_recipe_structure = Some(DesignSurfacePatchRecipeStructure {
        clauses: [([1, 2], [0, 3]), ([4, 1], [5, 0])].map(|(faces, edges)| {
            DesignSurfacePatchRecipeClause {
                fields: vec![
                    vec![i32::try_from(faces[0]).unwrap()],
                    vec![i32::try_from(faces[1]).unwrap()],
                    vec![2, i32::try_from(edges[0]).unwrap()],
                    vec![0, 0],
                    vec![i32::try_from(edges[1]).unwrap()],
                    vec![0, 0],
                ],
                face_reference_ordinals: faces,
                edge_reference_ordinals: edges,
                entries: Vec::new(),
            }
        }),
    });
    let boundary = crate::records::topology::historical_context::AsmFaceBoundary {
        id: "f3d:test:asm-face-boundary#7:91".into(),
        parent: "f3d:test:asm-delta-state#7".into(),
        face_slot: 91,
        loops: vec![serde_json::from_value(serde_json::json!({
            "loop_slot": 5,
            "coedge_slots": [6],
            "edge_slots": [17],
        }))
        .unwrap()],
    };
    operand.recipe_reference_contexts = vec![serde_json::from_value(serde_json::json!({
        "reference_ordinal": 1,
        "result_faces": [],
        "result_shared_edge_slots": [],
        "preceding_faces": ["f3d:test:face#91"],
        "preceding_face_boundary_ids": [boundary.id],
        "shared_edge_slots": [],
        "changed_shared_edge_slots": [],
    }))
    .unwrap()];
    (operand, boundary)
}

#[test]
fn grouped_surface_patch_alternate_intersects_the_exact_face_boundary() {
    let (mut operand, boundary) = alternate_patch_operand();
    let expected = SurfacePatchRecipeEdges::Resolved(vec![
        EdgeId::mint("f3d:test:edge#17").expect("identity grammar")
    ]);
    assert_eq!(
        surface_patch_grouped_recipe_edges(&[&operand], std::slice::from_ref(&boundary)),
        expected,
    );
    let structure = operand.surface_patch_recipe_structure.as_mut().unwrap();
    structure.clauses[0].face_reference_ordinals = [1, 1];
    structure.clauses[0].edge_reference_ordinals = [0, 0];
    assert_eq!(
        surface_patch_grouped_recipe_edges(&[&operand], &[boundary]),
        expected,
    );
}

#[test]
fn grouped_surface_patch_alternate_rejects_wrong_or_missing_exact_faces() {
    let (operand, boundary) = alternate_patch_operand();
    let resolve = |operand: &DesignEdgeOperand, boundaries: &[_]| {
        surface_patch_grouped_recipe_edges(&[operand], boundaries)
    };
    let mut wrong = boundary.clone();
    wrong.face_slot = 92;
    assert_eq!(
        resolve(&operand, &[wrong]),
        SurfacePatchRecipeEdges::Inconclusive
    );
    let mut wrong = operand.clone();
    wrong.recipe_references[1].candidate_faces[0] =
        cadmpeg_ir::ids::FaceId::mint("f3d:test:face#92").expect("identity grammar");
    assert_eq!(
        resolve(&wrong, std::slice::from_ref(&boundary)),
        SurfacePatchRecipeEdges::Inconclusive
    );
    let mut missing = operand.clone();
    missing.recipe_references[1].alternate_selector_faces =
        missing.recipe_references[1].candidate_faces.clone();
    missing.recipe_references[1].candidate_faces.clear();
    assert_eq!(
        resolve(&missing, std::slice::from_ref(&boundary)),
        SurfacePatchRecipeEdges::Inconclusive
    );
    let mut duplicate = operand.clone();
    let face = duplicate.recipe_references[1].candidate_faces[0].clone();
    duplicate.recipe_references[1].candidate_faces.push(face);
    assert_eq!(
        resolve(&duplicate, std::slice::from_ref(&boundary)),
        SurfacePatchRecipeEdges::Inconclusive
    );
    let mut repeated = boundary.clone();
    repeated.id.push_str(":duplicate");
    let mut wrong = operand.clone();
    wrong.recipe_reference_contexts[0]
        .preceding_face_boundary_ids
        .push(repeated.id.clone());
    assert_eq!(
        resolve(&wrong, &[boundary.clone(), repeated]),
        SurfacePatchRecipeEdges::Inconclusive
    );
    assert_eq!(
        resolve(&operand, &[]),
        SurfacePatchRecipeEdges::Inconclusive
    );
    assert_eq!(
        resolve(&operand, &[boundary.clone(), boundary]),
        SurfacePatchRecipeEdges::Inconclusive
    );
}

#[test]
fn grouped_surface_patch_alternate_rejects_ambiguous_roles_and_remaining_edges() {
    let (operand, mut boundary) = alternate_patch_operand();
    boundary.loops[0] = serde_json::from_value(
        serde_json::json!({"loop_slot":5,"coedge_slots":[6,9],"edge_slots":[17,18]}),
    )
    .unwrap();
    assert_eq!(
        surface_patch_grouped_recipe_edges(&[&operand], std::slice::from_ref(&boundary)),
        SurfacePatchRecipeEdges::Inconclusive,
    );
    boundary.loops[0] = serde_json::from_value(
        serde_json::json!({"loop_slot":5,"coedge_slots":[6],"edge_slots":[99]}),
    )
    .unwrap();
    assert_eq!(
        surface_patch_grouped_recipe_edges(&[&operand], &[boundary]),
        SurfacePatchRecipeEdges::Inconclusive,
    );
    let (mut operand, boundary) = alternate_patch_operand();
    operand
        .surface_patch_recipe_structure
        .as_mut()
        .unwrap()
        .clauses[1]
        .edge_reference_ordinals = [0, 3];
    assert_eq!(
        surface_patch_grouped_recipe_edges(&[&operand], &[boundary]),
        SurfacePatchRecipeEdges::Inconclusive,
    );
}

#[test]
fn unstructured_local_support_face_references_do_not_resolve_an_edge() {
    let context = |reference_ordinal, shared_edge_slots: &[i64]| {
        serde_json::from_value(serde_json::json!({
            "reference_ordinal": reference_ordinal,
            "result_faces": [],
            "result_shared_edge_slots": shared_edge_slots,
            "preceding_faces": [],
            "shared_edge_slots": shared_edge_slots,
            "changed_shared_edge_slots": []
        }))
        .expect("edge recipe reference context")
    };
    let mut operand = recipe_edge_operand(10, &[63, 106, 109, 164], &[]);
    operand.recipe_program = vec![1];
    operand.resolved_edge_slot = Some(180);
    operand.local_topology_references = Some(
        [2, 1, 3, 1]
            .into_iter()
            .map(|ordinal| std::num::NonZeroU32::new(ordinal).expect("nonzero ordinal"))
            .collect(),
    );
    operand.recipe_reference_contexts = vec![
        context(0, &[]),
        context(1, &[164, 180, 183, 210]),
        context(2, &[106, 139, 180, 195]),
        context(3, &[109, 142, 183, 197]),
    ];
    operand.preceding_boundary_edge_slots =
        vec![63, 106, 109, 139, 142, 164, 168, 180, 183, 195, 197, 210];

    assert_eq!(resolved_edge_operand(&operand), None);
}

#[test]
fn primary_terminal_support_references_resolve_one_shared_edge() {
    let side = |header_value, scalars: [i32; 2]| {
        serde_json::from_value(serde_json::json!({
            "field_count": 3,
            "header_value": header_value,
            "scalars": scalars,
            "payload_prefix": [0],
            "payload_entry_count": 0,
            "entries": []
        }))
        .expect("terminal edge recipe side")
    };
    let mut operand = recipe_edge_operand(10, &[], &[]);
    operand.recipe_structure = Some(
        crate::records::topology::edge_recipe::DesignEdgeRecipeStructure {
            root: 2,
            sides: vec![side(0, [2, 1]), side(0, [1, 3])],
        },
    );
    operand.terminal_reference_edge_slots = vec![
        vec![25, 38, 39, 62],
        vec![25, 35, 55, 69],
        vec![35, 39, 57, 83],
    ];
    operand.terminal_boundary_edge_slots = vec![25, 35, 38, 39, 55, 57, 62, 69, 83];

    assert_eq!(resolved_edge_operand(&operand), Some(25));

    operand.terminal_reference_edge_slots[0].push(69);
    assert_eq!(resolved_edge_operand(&operand), None);

    operand.terminal_reference_edge_slots[0].pop();
    operand.recipe_structure.as_mut().unwrap().sides[0] = side(0, [2, 0]);
    assert_eq!(resolved_edge_operand(&operand), None);

    operand.recipe_structure.as_mut().unwrap().sides[0] = side(0, [2, 1]);
    operand.recipe_reference_contexts = vec![serde_json::from_value(serde_json::json!({
        "reference_ordinal": 0,
        "result_faces": [],
        "result_shared_edge_slots": [],
        "preceding_faces": [],
        "shared_edge_slots": [],
        "changed_shared_edge_slots": []
    }))
    .expect("historical edge recipe reference context")];
    assert_eq!(resolved_edge_operand(&operand), None);
}

#[test]
fn edge_flange_uses_one_updated_edge_without_recipe_context() {
    let group = group(2, 10);
    let mut operand = recipe_edge_operand(10, &[], &[]);
    operand.preceding_boundary_edge_slots = vec![17, 18, 19];
    operand.changed_boundary_edge_slots = vec![17, 18];
    operand.updated_boundary_edge_slots = vec![17];
    operand.result_boundary_edge_slots = vec![17, 20];
    let feature_id = cadmpeg_ir::features::FeatureId::mint("f3d:model:feature#edge-flange")
        .expect("identity grammar");

    let selection = crate::test_support::with_decode_context(|decode_ctx| {
        resolved_edge_flange_group(
            &group,
            std::slice::from_ref(&group),
            &[operand],
            &[],
            Some(7),
            &feature_id,
            decode_ctx,
        )
    })
    .unwrap();
    assert!(matches!(
        selection,
        cadmpeg_ir::features::EdgeSelection::Historical { edges, .. }
            if edges.as_slice() == [cadmpeg_ir::ids::HistoricalEdgeId::mint("f3d:history-input:edge#11:edge-flange:7:17").expect("identity grammar")]
    ));
}

#[test]
fn edge_flange_does_not_choose_an_ambiguous_updated_boundary() {
    let group = group(2, 10);
    let mut operand = recipe_edge_operand(10, &[], &[]);
    operand.preceding_boundary_edge_slots = vec![17, 18, 19];
    operand.changed_boundary_edge_slots = vec![17, 18];
    operand.updated_boundary_edge_slots = vec![17, 18];
    operand.result_boundary_edge_slots = vec![17, 18, 20];
    let feature_id = cadmpeg_ir::features::FeatureId::mint("f3d:model:feature#edge-flange")
        .expect("identity grammar");

    assert!(matches!(
        crate::test_support::with_decode_context(|decode_ctx| resolved_edge_flange_group(
            &group,
            std::slice::from_ref(&group),
            &[operand],
            &[],
            Some(7),
            &feature_id,
            decode_ctx
        ))
        .unwrap(),
        cadmpeg_ir::features::EdgeSelection::Native(_)
    ));
}

#[test]
fn edge_treatment_chain_requires_complete_recipe_boundary_coverage() {
    let first = recipe_edge_operand(10, &[17], &[17]);
    let second = recipe_edge_operand(11, &[], &[]);
    assert!(!transition_chain_is_supported_by_recipe(
        &[17, 18],
        2,
        &[&first, &second]
    ));

    let context = |changed_reference_edge_slots| {
        serde_json::from_value(serde_json::json!({
            "reference_ordinal": 0,
            "result_faces": [],
            "result_shared_edge_slots": [],
            "preceding_faces": [],
            "shared_edge_slots": [],
            "changed_shared_edge_slots": [],
            "changed_reference_edge_slots": changed_reference_edge_slots,
        }))
        .expect("historical edge recipe reference context")
    };
    let mut first = recipe_edge_operand(10, &[], &[17]);
    first.local_topology_references = Some(vec![
        std::num::NonZeroU32::new(1).expect("nonzero reference ordinal")
    ]);
    first.recipe_reference_contexts = vec![context(vec![18])];
    let mut second = recipe_edge_operand(11, &[], &[]);
    second.local_topology_references = Some(vec![
        std::num::NonZeroU32::new(1).expect("nonzero reference ordinal")
    ]);
    second.recipe_reference_contexts = vec![context(vec![17, 18])];
    assert!(transition_chain_is_supported_by_recipe(
        &[17, 18],
        2,
        &[&first, &second]
    ));

    let first = recipe_edge_operand(10, &[17, 18], &[17]);
    assert!(transition_chain_is_supported_by_recipe(
        &[17, 18],
        2,
        &[&first, &second]
    ));
}

#[test]
fn compact_identity_group_uses_selected_recipe_context_boundaries() {
    let mut selection_group = group(2, 10);
    selection_group
        .try_set_members(vec![
            crate::records::identity::Located {
                value: 10,
                offset: 0,
            },
            crate::records::identity::Located {
                value: 11,
                offset: 11,
            },
        ])
        .unwrap();
    let first_identity = identity(10, &[(17, 1.0), (18, 1.0)]);
    let mut second_identity = identity(11, &[(17, 1.0), (18, 1.0)]);
    second_identity.group_member_ordinal = 1;
    let context = |changed_reference_edge_slots| {
        serde_json::from_value(serde_json::json!({
            "reference_ordinal": 0,
            "result_faces": [],
            "result_shared_edge_slots": [],
            "preceding_faces": [],
            "shared_edge_slots": [],
            "changed_shared_edge_slots": [],
            "changed_reference_edge_slots": changed_reference_edge_slots,
        }))
        .expect("historical edge recipe reference context")
    };
    let mut first = recipe_edge_operand(10, &[], &[17]);
    first.local_topology_references = Some(vec![
        std::num::NonZeroU32::new(1).expect("nonzero reference ordinal")
    ]);
    first.recipe_reference_contexts = vec![context(vec![18])];
    let mut second = recipe_edge_operand(11, &[], &[]);
    second.local_topology_references = Some(vec![
        std::num::NonZeroU32::new(1).expect("nonzero reference ordinal")
    ]);
    second.recipe_reference_contexts = vec![context(vec![17, 18])];
    let feature_id = cadmpeg_ir::features::FeatureId::mint("f3d:model:feature#chamfer")
        .expect("identity grammar");

    let selection = resolved_edge_treatment_group(
        &selection_group,
        std::slice::from_ref(&selection_group),
        &[first, second],
        &[first_identity, second_identity],
        Some(7),
        &feature_id,
        None,
    )
    .unwrap();
    assert!(matches!(
        selection,
        cadmpeg_ir::features::EdgeSelection::Historical { edges, .. }
            if edges.as_slice() == [
                cadmpeg_ir::ids::HistoricalEdgeId::mint("f3d:history-input:edge#7:chamfer:7:17").expect("identity grammar"),
                cadmpeg_ir::ids::HistoricalEdgeId::mint("f3d:history-input:edge#7:chamfer:7:18").expect("identity grammar"),
            ]
    ));
}

#[test]
fn lost_references_preserve_a_complete_compact_transition_chain() {
    let mut selection_group = group(2, 10);
    selection_group
        .try_set_members(vec![
            crate::records::identity::Located {
                value: 10,
                offset: 0,
            },
            crate::records::identity::Located {
                value: 11,
                offset: 11,
            },
        ])
        .unwrap();
    let first_identity = identity(10, &[(17, 1.0), (18, 1.0)]);
    let mut second_identity = identity(11, &[(17, 1.0), (18, 1.0)]);
    second_identity.group_member_ordinal = 1;
    let recipe_operands = [
        recipe_edge_operand(10, &[19], &[19]),
        recipe_edge_operand(11, &[19], &[19]),
    ];
    let feature_id = cadmpeg_ir::features::FeatureId::mint("f3d:model:feature#chamfer")
        .expect("identity grammar");

    assert!(matches!(
        resolved_edge_treatment_group(
            &selection_group,
            std::slice::from_ref(&selection_group),
            &recipe_operands,
            &[first_identity.clone(), second_identity.clone()],
            Some(7),
            &feature_id,
            None,
        )
        .unwrap(),
        cadmpeg_ir::features::EdgeSelection::Native(_)
    ));

    selection_group.lost_edge_references = vec!["f3d:test:lost#0".into()];
    assert!(matches!(
        resolved_edge_treatment_group(
            &selection_group,
            std::slice::from_ref(&selection_group),
            &recipe_operands,
            &[first_identity.clone(), second_identity.clone()],
            Some(7),
            &feature_id,
            None,
        )
        .unwrap(),
        cadmpeg_ir::features::EdgeSelection::Unresolved
    ));

    selection_group
        .lost_edge_references
        .push("f3d:test:lost#1".into());
    assert!(matches!(
        resolved_edge_treatment_group(
            &selection_group,
            std::slice::from_ref(&selection_group),
            &recipe_operands,
            &[first_identity, second_identity],
            Some(7),
            &feature_id,
            None,
        ).unwrap(),
        cadmpeg_ir::features::EdgeSelection::Historical { edges, .. }
            if edges.as_slice() == [
                cadmpeg_ir::ids::HistoricalEdgeId::mint("f3d:history-input:edge#7:chamfer:7:17").expect("identity grammar"),
                cadmpeg_ir::ids::HistoricalEdgeId::mint("f3d:history-input:edge#7:chamfer:7:18").expect("identity grammar"),
            ]
    ));
}

#[test]
fn compact_identity_group_does_not_displace_a_possible_support_group() {
    let scope = fixed_scope();
    let first = group(2, 10);
    let second = group(3, 11);
    let first_identity = identity(10, &[(17, 3.0)]);
    let mut second_identity = identity(11, &[(19, 3.0)]);
    second_identity.group_record_index = 3;
    assert!(
        crate::test_support::with_decode_context(|decode_ctx| project_fixed_fillet(
            &scope,
            &[first, second],
            &[],
            &[first_identity, second_identity],
            decode_ctx
        ))
        .unwrap()
        .is_none()
    );
}

#[test]
fn compact_edge_treatment_group_selects_exact_deleted_edge_cardinality() {
    let mut selection_group = group(2, 10);
    selection_group
        .try_set_members(vec![
            crate::records::identity::Located {
                value: 10,
                offset: 0,
            },
            crate::records::identity::Located {
                value: 11,
                offset: 11,
            },
        ])
        .unwrap();
    let first = identity(10, &[(17, 5.0), (19, 5.0)]);
    let mut second = identity(11, &[(17, 5.0), (19, 5.0)]);
    second.group_member_ordinal = 1;
    let feature_id = cadmpeg_ir::features::FeatureId::mint("test:model:feature#feature")
        .expect("identity grammar");

    assert!(matches!(
        crate::test_support::with_decode_context(|decode_ctx| resolved_edge_group(
            &selection_group,
            std::slice::from_ref(&selection_group),
            &[],
            &[first.clone(), second.clone()],
            Some(7),
            &feature_id,
            decode_ctx
        ))
        .unwrap(),
        cadmpeg_ir::features::EdgeSelection::Native(_)
    ));
    assert!(matches!(
        resolved_edge_treatment_group(
            &selection_group,
            std::slice::from_ref(&selection_group),
            &[],
            &[first.clone(), second.clone()],
            Some(7),
            &feature_id,
            Some(3.0),
        ).unwrap(),
        cadmpeg_ir::features::EdgeSelection::Historical { edges, .. }
            if edges.len() == 2
    ));

    let first = identity(10, &[(17, 5.0), (18, 5.0), (19, 5.0)]);
    let mut second = identity(11, &[(17, 5.0), (18, 5.0), (19, 5.0)]);
    second.group_member_ordinal = 1;
    assert!(matches!(
        resolved_edge_treatment_group(
            &selection_group,
            std::slice::from_ref(&selection_group),
            &[],
            &[first, second],
            Some(7),
            &feature_id,
            Some(3.0),
        )
        .unwrap(),
        cadmpeg_ir::features::EdgeSelection::Native(_)
    ));

    let recipe_operands = [
        recipe_edge_operand(10, &[17, 18, 19], &[17]),
        recipe_edge_operand(11, &[17, 18, 19], &[18]),
    ];
    let mut recipe_second_identity = identity(11, &[(17, 5.0), (18, 5.0), (19, 5.0)]);
    recipe_second_identity.group_member_ordinal = 1;
    let recipe_identities = [
        identity(10, &[(17, 5.0), (18, 5.0), (19, 5.0)]),
        recipe_second_identity,
    ];
    assert!(matches!(
        resolved_edge_treatment_group(
            &selection_group,
            std::slice::from_ref(&selection_group),
            &recipe_operands,
            &recipe_identities,
            Some(7),
            &feature_id,
            None,
        ).unwrap(),
        cadmpeg_ir::features::EdgeSelection::Historical { edges, .. }
            if edges.len() == 3
    ));

    let incomplete_recipe_operands = [
        recipe_edge_operand(10, &[17, 18], &[17]),
        recipe_edge_operand(11, &[17, 18], &[18]),
    ];
    assert!(matches!(
        resolved_edge_treatment_group(
            &selection_group,
            std::slice::from_ref(&selection_group),
            &incomplete_recipe_operands,
            &recipe_identities,
            Some(7),
            &feature_id,
            None,
        )
        .unwrap(),
        cadmpeg_ir::features::EdgeSelection::Native(_)
    ));
}

#[test]
fn hem_transition_edge_is_the_unique_non_support_boundary() {
    let support = [[167], [106], [110]];
    assert_eq!(
        unique_hem_transition_edge_candidate(
            &[63, 106, 110, 167],
            [
                &[][..],
                support[0].as_slice(),
                support[1].as_slice(),
                support[2].as_slice()
            ]
        ),
        Some(63)
    );
    assert_eq!(
        unique_hem_transition_edge_candidate(
            &[63, 106, 110, 167],
            [
                &[][..],
                &[63][..],
                support[1].as_slice(),
                support[0].as_slice()
            ]
        ),
        Some(110)
    );
    assert_eq!(
        unique_hem_transition_edge_candidate(
            &[63, 106, 110, 167],
            [
                support[0].as_slice(),
                support[1].as_slice(),
                support[2].as_slice()
            ]
        ),
        None
    );
    assert_eq!(
        unique_hem_transition_edge_candidate(
            &[63, 106, 110, 167],
            [&[][..], &[106, 111][..], support[1].as_slice()]
        ),
        None
    );
}

#[test]
fn partial_historical_edge_selection_retains_proofs_and_unresolved_operands() {
    use cadmpeg_ir::features::EdgeSelection;
    use cadmpeg_ir::ids::FeatureInputTopologyId;

    let state =
        FeatureInputTopologyId::mint("f3d:history-input:state#feature").expect("identity grammar");
    let feature = cadmpeg_ir::features::FeatureId::mint("f3d:model:feature#feature")
        .expect("identity grammar");
    let selection = partial_historical_edge_selection(
        [
            ("operand-a", Some(17)),
            ("operand-b", None),
            ("operand-c", Some(17)),
        ],
        41,
        &feature,
        state.clone(),
        "group",
    )
    .expect("mixed proof state");
    assert_eq!(
        selection,
        EdgeSelection::historical_partial(
            state,
            vec![
                cadmpeg_ir::ids::HistoricalEdgeId::mint("f3d:history-input:edge#7:feature:41:17")
                    .expect("identity grammar")
            ],
            vec!["operand-b".into()],
            "group".into(),
        )
        .unwrap()
    );
    assert!(partial_historical_edge_selection(
        [("operand-a", Some(17)), ("operand-b", Some(18))],
        41,
        &feature,
        FeatureInputTopologyId::mint("test:model:feature-input#state").expect("identity grammar"),
        "group",
    )
    .is_none());
    assert_eq!(
        partial_historical_edge_selection(
            [("operand-a", None), ("operand-b", None)],
            41,
            &feature,
            FeatureInputTopologyId::mint("test:model:feature-input#state")
                .expect("identity grammar"),
            "group"
        ),
        None
    );
}

#[test]
fn edge_group_cardinality_resolves_one_common_deleted_candidate_set() {
    let selector = |candidates: &[i64]| {
        crate::records::topology::edge_recipe::DesignEdgeRecipeSelectorContext {
            selector: 0,
            clauses: vec![None, None],
            incidence_matching_edge_slots: Vec::new(),

            boundary_count_matching_edge_slots: candidates.to_vec(),
        }
    };
    let first = [selector(&[19, 17, 18])];
    let context = [selector(&[])];
    let last = [selector(&[18, 19, 17])];
    assert_eq!(
        crate::design::edge_resolve::changed_boundary_count_edge_group_candidates([
            first.as_slice(),
            context.as_slice(),
            last.as_slice()
        ],),
        Some(vec![17, 18, 19])
    );
    assert_eq!(
        crate::design::edge_resolve::changed_boundary_count_edge_group_candidates([
            first.as_slice(),
            last.as_slice()
        ],),
        None
    );
    assert_eq!(
        crate::design::edge_resolve::changed_boundary_count_edge_group_candidates([
            first.as_slice(),
            context.as_slice(),
            &[]
        ]),
        None
    );
    assert_eq!(
        crate::design::edge_resolve::common_deleted_edge_group_candidates([
            (true, &[19, 17, 18, 17][..]),
            (true, &[18, 19, 17][..]),
            (true, &[17, 18, 19][..]),
        ]),
        Some(vec![17, 18, 19])
    );
    assert_eq!(
        crate::design::edge_resolve::common_deleted_edge_group_candidates([
            (true, &[17, 18, 19][..]),
            (true, &[17, 18][..]),
            (true, &[17, 18, 19][..]),
        ]),
        None
    );
    assert_eq!(
        crate::design::edge_resolve::common_deleted_edge_group_candidates([
            (true, &[17, 18, 19][..]),
            (true, &[17, 18, 19][..]),
        ]),
        None
    );
    assert_eq!(
        crate::design::edge_resolve::common_deleted_edge_group_candidates([
            (true, &[17, 18][..]),
            (false, &[][..]),
            (true, &[18, 17][..]),
        ]),
        Some(vec![17, 18])
    );
    assert_eq!(
        crate::design::edge_resolve::common_deleted_edge_group_candidates(std::iter::empty::<(
            bool,
            &[i64],
        )>()),
        None
    );
    let deleted = vec![17, 18, 19, 20];
    let member = |identity, resolved_edge| crate::design::edge_resolve::EdgeGroupMember {
        identity,
        resolved_edge,
        deleted_boundary_edges: &deleted,
    };
    let groups = vec![
        vec![member(10, Some(17)), member(11, Some(19))],
        vec![member(12, None), member(13, None)],
    ];
    assert_eq!(
        crate::design::edge_resolve::partition_unique_incomplete_edge_group(1, &groups),
        Some(vec![18, 20])
    );
    assert_eq!(
        crate::design::edge_resolve::partition_unique_incomplete_edge_group(0, &groups),
        None
    );
    let mut two_incomplete = groups.clone();
    two_incomplete[0][0].resolved_edge = None;
    assert_eq!(
        crate::design::edge_resolve::partition_unique_incomplete_edge_group(1, &two_incomplete),
        None
    );
    let mut duplicate_identity = groups;
    duplicate_identity[1][0].identity = 11;
    assert_eq!(
        crate::design::edge_resolve::partition_unique_incomplete_edge_group(1, &duplicate_identity,),
        None
    );
}

#[test]
fn deleted_boundary_group_requires_complete_contextual_group_cardinality() {
    let context = |changed_reference_edge_slots: &[i64]| {
        serde_json::from_value(serde_json::json!({
            "reference_ordinal": 0,
            "result_faces": [],
            "result_shared_edge_slots": [],
            "preceding_faces": [],
            "shared_edge_slots": [],
            "changed_shared_edge_slots": [],
            "changed_reference_edge_slots": changed_reference_edge_slots,
        }))
        .expect("edge recipe reference context")
    };
    let operand = |record_index: u32, deleted: &[i64]| {
        let mut operand = recipe_edge_operand(record_index, deleted, deleted);
        operand.preceding_boundary_edge_slots = deleted.to_vec();
        operand.recipe_structure = Some(
            crate::records::topology::edge_recipe::DesignEdgeRecipeStructure {
                root: 2,
                sides: Vec::new(),
            },
        );
        operand.recipe_references = vec![recipe_reference(&[])];
        operand.recipe_reference_contexts = vec![context(deleted)];
        operand
    };

    let first = operand(10, &[17, 18]);
    let second = operand(11, &[17, 18]);
    let third = operand(12, &[19, 20]);
    let fourth = operand(13, &[19, 20]);
    assert_eq!(
        deleted_boundary_edge_group_candidates(&[&first, &second, &third, &fourth]),
        Some(vec![17, 18, 19, 20])
    );

    let too_many_edges = operand(11, &[17, 18, 19]);
    assert_eq!(
        deleted_boundary_edge_group_candidates(&[&first, &too_many_edges]),
        None
    );

    let mut unreferenced = operand(12, &[19, 20]);
    unreferenced.recipe_reference_contexts = vec![context(&[21])];
    assert_eq!(
        deleted_boundary_edge_group_candidates(&[&first, &second, &unreferenced, &fourth]),
        None
    );
}

#[test]
fn contextual_deleted_group_assigns_a_consolidated_legacy_member() {
    let context = |changed_reference_edge_slots: &[i64]| {
        serde_json::from_value(serde_json::json!({
            "reference_ordinal": 0,
            "result_faces": [],
            "result_shared_edge_slots": [],
            "preceding_faces": [],
            "shared_edge_slots": [],
            "changed_shared_edge_slots": [],
            "changed_reference_edge_slots": changed_reference_edge_slots,
        }))
        .expect("edge recipe reference context")
    };
    let structure = || crate::records::topology::edge_recipe::DesignEdgeRecipeStructure {
        root: 2,
        sides: Vec::new(),
    };

    let mut consolidated = recipe_edge_operand(10, &[], &[]);
    consolidated.preceding_boundary_edge_slots = vec![5630, 5675];
    consolidated.recipe_structure = Some(structure());
    consolidated.recipe_references = vec![recipe_reference(&[])];
    consolidated.recipe_reference_contexts = vec![context(&[5630])];

    let mut deleted = recipe_edge_operand(11, &[5630, 5675], &[5630, 5675]);
    deleted.preceding_boundary_edge_slots = vec![5630, 5675];
    deleted.recipe_structure = Some(structure());
    deleted.recipe_references = vec![recipe_reference(&[]), recipe_reference(&[])];
    deleted.recipe_reference_contexts = vec![context(&[5630]), context(&[5675])];

    assert_eq!(
        crate::test_support::with_decode_context(|decode_ctx| {
            contextual_deleted_edge_group_candidates(&[&consolidated, &deleted], decode_ctx)
        })
        .unwrap(),
        Some(vec![5630, 5675])
    );

    deleted.recipe_reference_contexts[1] = context(&[5630]);
    assert_eq!(
        crate::test_support::with_decode_context(|decode_ctx| {
            contextual_deleted_edge_group_candidates(&[&consolidated, &deleted], decode_ctx)
        })
        .unwrap(),
        None
    );
}

#[test]
fn result_boundary_reference_group_requires_one_persistent_contextual_edge() {
    let context = |changed_reference_edge_slots: &[i64]| {
        serde_json::from_value(serde_json::json!({
            "reference_ordinal": 0,
            "result_faces": [],
            "result_shared_edge_slots": [],
            "preceding_faces": [],
            "shared_edge_slots": [],
            "changed_shared_edge_slots": [],
            "changed_reference_edge_slots": changed_reference_edge_slots,
        }))
        .expect("edge recipe reference context")
    };
    let mut operand = recipe_edge_operand(10, &[], &[]);
    operand.recipe_structure = Some(
        crate::records::topology::edge_recipe::DesignEdgeRecipeStructure {
            root: 2,
            sides: (0..2)
                .map(
                    |_| crate::records::topology::edge_recipe::DesignTopologyRecipeSide {
                        header_value: 2,
                        scalars: vec![1, 0],
                        payload_prefix: vec![0],

                        entries: Vec::new(),
                    },
                )
                .collect(),
        },
    );
    operand.recipe_references = vec![
        recipe_reference(&[]),
        recipe_reference(&[]),
        recipe_reference(&[]),
    ];
    operand.recipe_reference_contexts = vec![context(&[19, 21]), context(&[19]), context(&[22])];
    operand.preceding_boundary_edge_slots = vec![17, 18];
    operand.result_boundary_edge_slots = vec![19, 20];
    assert_eq!(
        result_boundary_reference_edge_group_candidates(&[&operand]),
        Some(vec![19])
    );

    operand.recipe_reference_contexts[1] = context(&[20]);
    assert_eq!(
        result_boundary_reference_edge_group_candidates(&[&operand]),
        None
    );

    operand.recipe_reference_contexts[1] = context(&[19]);
    operand.preceding_boundary_edge_slots.push(19);
    assert_eq!(
        result_boundary_reference_edge_group_candidates(&[&operand]),
        None
    );
}

#[test]
fn edge_group_ignores_members_without_changed_edge_candidates() {
    assert_eq!(
        crate::design::edge_resolve::context_only_edge_group_candidates([
            (None, &[][..]),
            (Some(17), &[17, 18][..]),
            (Some(17), &[17][..]),
            (None, &[][..]),
        ]),
        Some(vec![17])
    );
    assert_eq!(
        crate::design::edge_resolve::context_only_edge_group_candidates([
            (Some(17), &[17][..]),
            (None, &[18][..]),
        ]),
        None
    );
    assert_eq!(
        crate::design::edge_resolve::context_only_edge_group_candidates([(None, &[][..])]),
        None
    );
}

#[test]
fn edge_group_resolves_only_one_perfect_candidate_assignment() {
    assert_eq!(
        crate::design::edge_resolve::edge_group_assignment_candidates(
            &[],
            [&[17, 18][..], &[18, 19][..], &[20][..]]
        ),
        Some(crate::design::edge_resolve::EdgeAssignmentCandidates::Edges(vec![18]))
    );
    assert_eq!(
        crate::design::edge_resolve::edge_group_assignment_candidates(&[], [&[][..], &[18][..]]),
        Some(crate::design::edge_resolve::EdgeAssignmentCandidates::Context)
    );
    assert_eq!(
        crate::design::edge_resolve::edge_group_assignment_candidates(&[], [&[17][..], &[18][..]]),
        None
    );
    assert_eq!(
        crate::design::edge_resolve::edge_group_assignment_candidates(&[], [&[17][..]]),
        Some(crate::design::edge_resolve::EdgeAssignmentCandidates::Context)
    );
    assert_eq!(
        crate::test_support::with_decode_context(|decode_ctx| {
            crate::design::edge_resolve::unique_bipartite_assignment(
                &[vec![17, 18], vec![18, 19], vec![19]],
                decode_ctx,
            )
        })
        .unwrap(),
        Some(vec![17, 18, 19])
    );
    assert_eq!(
        crate::test_support::with_decode_context(|decode_ctx| {
            crate::design::edge_resolve::unique_bipartite_assignment(
                &[vec![17, 18], vec![17, 18]],
                decode_ctx,
            )
        })
        .unwrap(),
        None
    );
    assert_eq!(
        crate::test_support::with_decode_context(|decode_ctx| {
            crate::design::edge_resolve::unique_bipartite_assignment(
                &[vec![17], vec![17]],
                decode_ctx,
            )
        })
        .unwrap(),
        None
    );
    assert_eq!(
        crate::test_support::with_decode_context(|decode_ctx| {
            crate::design::edge_resolve::unique_bipartite_assignment(
                &[vec![17], Vec::new()],
                decode_ctx,
            )
        })
        .unwrap(),
        None
    );
    assert_eq!(
        crate::test_support::with_decode_context(|decode_ctx| {
            crate::design::edge_resolve::unique_bipartite_assignment(&[], decode_ctx)
        })
        .unwrap(),
        None
    );
    assert_eq!(
        crate::test_support::with_decode_context(|decode_ctx| {
            crate::design::edge_resolve::unique_edge_assignment(
                &[
                    crate::design::edge_resolve::EdgeAssignmentCandidates::Edges(vec![17, 18]),
                    crate::design::edge_resolve::EdgeAssignmentCandidates::Context,
                    crate::design::edge_resolve::EdgeAssignmentCandidates::Edges(vec![18]),
                ],
                decode_ctx,
            )
        })
        .unwrap(),
        Some(vec![17, 18])
    );
    assert_eq!(
        crate::test_support::with_decode_context(|decode_ctx| {
            crate::design::edge_resolve::unique_edge_assignment(
                &[
                    crate::design::edge_resolve::EdgeAssignmentCandidates::Context,
                    crate::design::edge_resolve::EdgeAssignmentCandidates::Context,
                ],
                decode_ctx,
            )
        })
        .unwrap(),
        None
    );
    assert_eq!(
        crate::test_support::with_decode_context(|decode_ctx| {
            crate::design::edge_resolve::unique_deleted_reference_assignment(
                &[vec![16, 17, 18], vec![19, 20, 21]],
                &[vec![17, 20, 22], vec![17, 20, 22]],
                decode_ctx,
            )
        })
        .unwrap(),
        Some(vec![17, 20])
    );
    assert_eq!(
        crate::test_support::with_decode_context(|decode_ctx| {
            crate::design::edge_resolve::unique_deleted_reference_assignment(
                &[vec![16, 17, 18], vec![16, 17, 18]],
                &[vec![17, 18], vec![17, 18]],
                decode_ctx,
            )
        })
        .unwrap(),
        None
    );
    assert_eq!(
        crate::test_support::with_decode_context(|decode_ctx| {
            crate::design::edge_resolve::unique_deleted_reference_assignment(
                &[vec![17]],
                &[vec![]],
                decode_ctx,
            )
        })
        .unwrap(),
        None
    );
}

#[test]
fn edge_assignment_pairs_single_candidates() {
    let candidates = [vec![17], vec![18]];
    assert_eq!(
        crate::test_support::with_decode_context(|decode_ctx| {
            crate::design::edge_resolve::unique_bipartite_assignment(&candidates, decode_ctx)
        })
        .unwrap(),
        Some(vec![17, 18])
    );
}

mod assignment_recursion;
mod group_candidates;
mod recipe_state;

#[test]
fn sweep_recipe_edge_requires_incidence_and_two_reference_faces() {
    use crate::design::edge_resolve::unique_incidence_edge_shared_by_reference_faces;

    let selector = |edges| crate::records::topology::edge_recipe::DesignEdgeRecipeSelectorContext {
        selector: 0,
        clauses: Vec::new(),
        incidence_matching_edge_slots: edges,

        boundary_count_matching_edge_slots: Vec::new(),
    };
    let selectors = [selector(vec![11, 12]), selector(vec![13])];
    assert_eq!(
        unique_incidence_edge_shared_by_reference_faces(
            &selectors,
            [&[10, 11][..], &[11, 12][..], &[13, 14][..]],
        ),
        Some(11)
    );
    assert_eq!(
        unique_incidence_edge_shared_by_reference_faces(&selectors, [&[11, 13][..], &[11, 13][..]],),
        None
    );
    assert_eq!(
        unique_incidence_edge_shared_by_reference_faces(&[selector(vec![11])], [&[10, 11][..]],),
        None
    );
    assert_eq!(
        unique_incidence_edge_shared_by_reference_faces(
            &[selector(vec![11])],
            [&[10, 11][..], &[10, 11][..]],
        ),
        None
    );
    assert_eq!(
        unique_incidence_edge_shared_by_reference_faces(
            &[selector(vec![11])],
            [&[10, 11, 11][..], &[11, 10][..]],
        ),
        None
    );
}

mod candidate_intersection;

mod standard_recipes;
