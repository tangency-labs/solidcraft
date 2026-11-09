// SPDX-License-Identifier: Apache-2.0
use cadmpeg_test_support::wire;

use crate::design::feature_project::{
    bind_surface_trim_cell_selections, project_replace_face, project_surface_trim,
};
use crate::records::feature::scope::DesignParameterScope;
use crate::records::topology::body_recipe::DesignBodyRecipeOperand;
use crate::records::topology::construction::DesignConstructionOperandGroup;
use crate::records::topology::extrude_selection::DesignOperandRole;
use crate::records::topology::face::DesignFaceOperand;

use crate::records::{
    feature::surface_ops::{DesignSurfaceTrimCellEntry, DesignSurfaceTrimOperation},
    recipes::ConstructionRecipeKind,
    topology::{
        body_recipe::DesignBodyRecipeReference, body_recipe::DesignOperandOwner,
        construction::DesignConstructionOperandGroupFrame,
    },
};
use cadmpeg_ir::features::{FaceSelection, FeatureDefinition, FeatureOperation};

fn body_recipe_limit_fixture() -> (
    DesignParameterScope,
    DesignConstructionOperandGroup,
    DesignBodyRecipeOperand,
) {
    let scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:scope#12",
        crate::records::feature::scope::DesignFeatureKind::ReplaceFace,
        12,
    );
    let group = group(12, 0, 100, 200, DesignOperandRole::ROLE_0X9);
    let operand = DesignBodyRecipeOperand::try_new(
        crate::records::topology::body_recipe::DesignBodyRecipeOperandDraft {
            id: "f3d:Design/BulkStream.dat:body-recipe#200".into(),
            scope_record_index: 12,
            owner: DesignOperandOwner::Group {
                group_record_index: 100,
                group_member_ordinal: 0,
            },
            record_index: 200,
            byte_offset: 0,
            class_tag: crate::records::references::DesignClassTag::try_from("316".to_owned())
                .unwrap(),
            asset_id: crate::records::mesh::DesignRelaxedGuidText::try_from(
                "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d".to_owned(),
            )
            .unwrap(),
            asset_id_offset: 56,
            context_id: crate::records::mesh::DesignRelaxedGuidText::try_from(
                "1b2c3d4e-5f6a-4b7c-8d9e-0f1a2b3c4d5e".to_owned(),
            )
            .unwrap(),
            context_id_offset: 132,
            selector_tail: None,
            references: vec![DesignBodyRecipeReference {
                design_reference: 326,
                design_reference_offset: 25,
                form: 33,
                form_offset: 33,
                candidate_faces: Vec::new(),
                preceding_candidate_faces: Vec::new(),
                preceding_body_slots: Vec::new(),
            }],
            nested_record_index: 203,
            nested_record_index_offset: 38,
            recipe_id: "f3d:Design/BulkStream.dat:recipe#202".into(),
            resolved_face_slot: Some(20),
            resolved_body_state_id: Some(7),
            resolved_body_slot: Some(3),
            resolved_body_face_slots: vec![20, 21],
            next_record_index: 204,
            next_byte_offset: 256,
        },
    )
    .unwrap();
    (scope, group, operand)
}

#[test]
fn body_recipe_selection_resolves_historical_faces() {
    let (scope, group, operand) = body_recipe_limit_fixture();
    let operands = [operand];
    assert!(matches!(
        { crate::design::face_resolve::resolved_body_recipe_selection(&scope, &group, &operands,) },
        Some(FaceSelection::Historical { .. })
    ));
}

fn group(
    scope_record_index: u32,
    scope_reference_ordinal: u32,
    record_index: u32,
    member: u32,
    role: DesignOperandRole,
) -> DesignConstructionOperandGroup {
    group_with_members(
        scope_record_index,
        scope_reference_ordinal,
        record_index,
        &[member],
        role,
    )
}

fn group_with_members(
    scope_record_index: u32,
    scope_reference_ordinal: u32,
    record_index: u32,
    members: &[u32],
    role: DesignOperandRole,
) -> DesignConstructionOperandGroup {
    DesignConstructionOperandGroup::try_from(
        crate::records::topology::construction::DesignConstructionOperandGroupDraft {
            id: format!("f3d:Design/BulkStream.dat:group#{record_index}"),
            scope_record_index,
            scope_reference_ordinal,
            record_index,
            byte_offset: 0,
            class_tag: crate::records::references::DesignClassTag::try_from("277".to_owned())
                .unwrap(),
            members: members
                .iter()
                .zip((0..).step_by(11))
                .map(|(member, offset)| crate::records::identity::Located {
                    value: *member,
                    offset,
                })
                .collect(),
            lost_edge_references: Vec::new(),
            frame: DesignConstructionOperandGroupFrame::try_from(
                crate::records::topology::construction::DesignConstructionOperandGroupFrameDraft {
                    member_count_offset: 0,
                    auxiliary_records: Vec::new(),
                    auxiliary_paths: Vec::new(),
                    trailing_records: Vec::new(),
                    trailing_transforms: Vec::new(),
                    trailing_dual_transforms: Vec::new(),
                    trailing_flags: Vec::new(),
                    opaque_index: 88,
                    opaque_index_offset: 18,
                    opaque_scalar: 0.0,
                    opaque_scalar_offset: 22,
                    variant: false,
                    closing_flag: false,
                },
            )
            .unwrap(),
            operand_role:
                crate::records::topology::construction::DesignConstructionOperandRole::Other(role),
            role_offset: 0,
            paired_class_tag: crate::records::references::DesignClassTag::try_from(
                "258".to_owned(),
            )
            .unwrap(),
            paired_byte_offset: 0,
        },
    )
    .unwrap()
}

#[test]
fn replace_face_projects_role_order_and_historical_inputs() {
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:scope#1129",
        crate::records::feature::scope::DesignFeatureKind::ReplaceFace,
        1129,
    );
    scope.class_tag =
        crate::records::references::DesignClassTag::try_from("301".to_owned()).unwrap();
    scope.paired_class_tag =
        crate::records::references::DesignClassTag::try_from("258".to_owned()).unwrap();
    scope
        .try_edit(|draft| {
            draft.frame_length = 290;
            draft.previous_history_state_id = Some(254);
            draft.reference_members =
                crate::records::identity::ReferenceRun::unlocated(vec![1130, 1133, 1137, 1140]);
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();

    let replacement_group = group(1129, 0, 1130, 1133, DesignOperandRole::ROLE_0X9);
    let target_group = group(1129, 2, 1137, 1140, DesignOperandRole::ROLE_0X10);
    let replacement = DesignBodyRecipeOperand::try_new(
        crate::records::topology::body_recipe::DesignBodyRecipeOperandDraft {
            id: "f3d:Design/BulkStream.dat:body-recipe#1133".into(),
            scope_record_index: 1129,
            owner: DesignOperandOwner::Group {
                group_record_index: 1130,
                group_member_ordinal: 0,
            },
            record_index: 1133,
            byte_offset: 0,
            class_tag: crate::records::references::DesignClassTag::try_from("316".to_owned())
                .unwrap(),
            asset_id: crate::records::mesh::DesignRelaxedGuidText::try_from(
                "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d".to_owned(),
            )
            .unwrap(),
            asset_id_offset: 56,
            context_id: crate::records::mesh::DesignRelaxedGuidText::try_from(
                "1b2c3d4e-5f6a-4b7c-8d9e-0f1a2b3c4d5e".to_owned(),
            )
            .unwrap(),
            context_id_offset: 132,
            selector_tail: None,

            references: vec![DesignBodyRecipeReference {
                design_reference: 326,
                design_reference_offset: 25,
                form: 33,
                form_offset: 33,
                candidate_faces: Vec::new(),
                preceding_candidate_faces: Vec::new(),
                preceding_body_slots: Vec::new(),
            }],
            nested_record_index: 1136,
            nested_record_index_offset: 38,
            recipe_id: "f3d:Design/BulkStream.dat:recipe#1135".into(),
            resolved_face_slot: Some(20),
            resolved_body_state_id: Some(254),
            resolved_body_slot: Some(3),
            resolved_body_face_slots: vec![20],
            next_record_index: 1137,
            next_byte_offset: 256,
        },
    )
    .unwrap();
    let target =
        DesignFaceOperand::try_new(crate::records::topology::face::DesignFaceOperandDraft {
            id: "f3d:Design/BulkStream.dat:face-operand#1140".into(),
            scope_record_index: 1129,
            scope_reference_ordinal: 3,
            group: Some(crate::records::topology::body_recipe::DesignOperandGroup {
                group_record_index: 1137,
                group_member_ordinal: 0,
            }),
            record_index: 1140,
            byte_offset: 0,
            class_tag: crate::records::references::DesignClassTag::try_from("272".to_owned())
                .unwrap(),
            paired_byte_offset: 16,
            paired_class_tag: crate::records::references::DesignClassTag::try_from(
                "258".to_owned(),
            )
            .unwrap(),
            recipe_record_index: 1143,
            recipe_record_byte_offset: 32,
            recipe_id: "f3d:Design/BulkStream.dat:recipe#1142".into(),
            recipe_prefix_offset: 43,
            recipe_prefix_bytes: Vec::new(),
            recipe_references: Vec::new(),
            recipe_kind: ConstructionRecipeKind::BoundedFace,
            recipe_program_offset: 0,
            recipe_program: Vec::new(),

            recipe_nodes: Vec::new(),
            candidate_faces: Vec::new(),
            unreferenced_candidate_faces: Vec::new(),
            alternate_selector_candidate_faces: Vec::new(),
            preceding_candidate_faces: Vec::new(),
            changed_candidate_faces: Vec::new(),
            historical_support_contexts: Vec::new(),
            resolved_face_slots: vec![622],
            resolved_active_face: None,
            next_record_index: 1144,
            next_byte_offset: 200,
        })
        .unwrap();

    let definition = project_replace_face(
        &scope,
        &[replacement_group.clone(), target_group.clone()],
        std::slice::from_ref(&target),
        std::slice::from_ref(&replacement),
    )
    .expect("typed ReplaceFace");
    assert!(matches!(
        definition, FeatureDefinition::Operation(FeatureOperation::ReplaceFace {
            operands,

        }) if matches!((operands.targets(), operands.replacements(),), (FaceSelection::Historical { ref faces, ref native, .. }, FaceSelection::Historical {
                faces: ref replacement_faces,
                native: ref replacement_native,
                ..
            },) if faces.len() == 1
            && replacement_faces.len() == 1
            && native.as_str() == target_group.id
            && replacement_native.as_str() == replacement_group.id)));

    let reversed = project_replace_face(
        &scope,
        &[target_group.clone(), replacement_group.clone()],
        std::slice::from_ref(&target),
        std::slice::from_ref(&replacement),
    );
    assert!(matches!(
        reversed,
        Some(FeatureDefinition::Operation(
            FeatureOperation::ReplaceFace { .. }
        ))
    ));
    assert!(project_replace_face(
        &scope,
        &[
            replacement_group.clone(),
            target_group.clone(),
            target_group.clone()
        ],
        std::slice::from_ref(&target),
        std::slice::from_ref(&replacement)
    )
    .is_none());

    let mut invalid_scope = scope;
    invalid_scope
        .try_edit(|draft| {
            draft.frame_length = 291;
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_tail();
        })
        .unwrap();
    assert!(project_replace_face(
        &invalid_scope,
        &[replacement_group, target_group],
        std::slice::from_ref(&target),
        std::slice::from_ref(&replacement)
    )
    .is_none());
}

fn trim_body_draft(
    group_record_index: u32,
    group_member_ordinal: u32,
    record_index: u32,
) -> crate::records::topology::body_recipe::DesignBodyRecipeOperandDraft {
    crate::records::topology::body_recipe::DesignBodyRecipeOperandDraft {
        id: format!("f3d:Design/BulkStream.dat:body-recipe#{record_index}"),
        scope_record_index: 1200,
        owner: DesignOperandOwner::Group {
            group_record_index,
            group_member_ordinal,
        },
        record_index,
        byte_offset: 0,
        class_tag: crate::records::references::DesignClassTag::try_from("316".to_owned()).unwrap(),
        asset_id: crate::records::mesh::DesignRelaxedGuidText::try_from(
            "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d".to_owned(),
        )
        .unwrap(),
        asset_id_offset: 56,
        context_id: crate::records::mesh::DesignRelaxedGuidText::try_from(
            "1b2c3d4e-5f6a-4b7c-8d9e-0f1a2b3c4d5e".to_owned(),
        )
        .unwrap(),
        context_id_offset: 132,
        selector_tail: None,

        references: vec![DesignBodyRecipeReference {
            design_reference: 326,
            design_reference_offset: 25,
            form: 33,
            form_offset: 33,
            candidate_faces: Vec::new(),
            preceding_candidate_faces: Vec::new(),
            preceding_body_slots: Vec::new(),
        }],
        nested_record_index: u64::from(record_index) + 3,
        nested_record_index_offset: 38,
        recipe_id: format!("f3d:Design/BulkStream.dat:recipe#{}", record_index + 4),
        resolved_face_slot: None,
        resolved_body_state_id: None,
        resolved_body_slot: None,
        resolved_body_face_slots: Vec::new(),
        next_record_index: record_index + 4,
        next_byte_offset: 256,
    }
}

fn trim_body(
    group_record_index: u32,
    group_member_ordinal: u32,
    record_index: u32,
) -> DesignBodyRecipeOperand {
    DesignBodyRecipeOperand::try_new(trim_body_draft(
        group_record_index,
        group_member_ordinal,
        record_index,
    ))
    .unwrap()
}

fn trim_curves(
    group_record_index: u32,
    group_member_ordinal: u32,
    record_index: u32,
) -> crate::records::topology::entity_selection::DesignEntitySelectionOperand {
    crate::records::topology::entity_selection::DesignEntitySelectionOperand::try_new(
        crate::records::topology::entity_selection::DesignEntitySelectionOperandDraft {
            id: format!("f3d:Design/BulkStream.dat:entity-selection#{record_index}"),
            scope_record_index: 1200,
            group_record_index,
            group_member_ordinal,
            record_index,
            byte_offset: 0,
            class_tag: "372".to_owned().try_into().unwrap(),
            asset_id: "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d"
                .to_owned()
                .try_into()
                .unwrap(),
            asset_id_offset: 0,
            context_id: "1b2c3d4e-5f6a-4b7c-8d9e-0f1a2b3c4d5e"
                .to_owned()
                .try_into()
                .unwrap(),
            context_id_offset: 0,
            serialization_form:
                crate::records::topology::entity_selection::SelectionSerializationForm::Zero,
            identity_record_index: record_index + 3,
            identity_record_offset: 0,
            primary_identity: 178,
            primary_identity_offset: 21,
            secondary: None,
            historical_edge_candidates: Vec::new(),
            historical_face_candidates: Vec::new(),
            resolved_edge_slot: None,
            next_record_index: record_index + 4,
            next_byte_offset: 29,
        },
    )
    .unwrap()
}

fn surface_trim_fixture() -> (
    DesignParameterScope,
    DesignConstructionOperandGroup,
    DesignConstructionOperandGroup,
    DesignBodyRecipeOperand,
    crate::records::topology::entity_selection::DesignEntitySelectionOperand,
) {
    let scope = surface_trim_scope(vec![1201, 1202, 1203, 1204]);
    let target_group = group(1200, 0, 1201, 1202, DesignOperandRole::BODIES_A);
    let tool_group = group(1200, 2, 1203, 1204, DesignOperandRole::ROLE_0X21);
    let mut body = trim_body_draft(1201, 0, 1202);
    body.resolved_face_slot = Some(20);
    body.resolved_body_state_id = Some(254);
    body.resolved_body_slot = Some(3);
    body.resolved_body_face_slots = vec![20];
    let body = DesignBodyRecipeOperand::try_new(body).unwrap();
    (
        scope,
        target_group,
        tool_group,
        body,
        trim_curves(1203, 0, 1204),
    )
}

#[test]
fn surface_trim_projects_body_target_and_curve_tool() {
    let (scope, target_group, tool_group, body, curves) = surface_trim_fixture();
    let mut losses = Vec::new();
    let definition = project_surface_trim(
        &scope,
        &[target_group.clone(), tool_group.clone()],
        std::slice::from_ref(&body),
        std::slice::from_ref(&curves),
        &mut losses,
    )
    .expect("typed SurfaceTrim");
    assert!(losses.is_empty());
    assert!(matches!(
        definition,
        FeatureDefinition::Operation(FeatureOperation::TrimSurface {
            faces: FaceSelection::Historical { ref faces, ref native, .. },
            tool: cadmpeg_ir::features::PathRef::Native(ref tool),
            keep: cadmpeg_ir::features::TrimRegion::Unresolved,
        }) if faces.len() == 1
            && native.as_str() == target_group.id
            && tool == &tool_group.id
    ));
}

fn surface_trim_scope(references: Vec<u32>) -> DesignParameterScope {
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:scope#1200",
        crate::records::feature::scope::DesignFeatureKind::SurfaceTrim,
        1200,
    );
    scope
        .try_edit(|draft| {
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(references);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    scope
}

#[test]
fn surface_trim_keeps_native_target_when_its_body_is_unresolved() {
    let (scope, target_group, tool_group, _, curves) = surface_trim_fixture();
    let mut losses = Vec::new();
    let definition = project_surface_trim(
        &scope,
        &[target_group.clone(), tool_group.clone()],
        &[trim_body(1201, 0, 1202)],
        std::slice::from_ref(&curves),
        &mut losses,
    )
    .expect("typed SurfaceTrim");
    assert!(losses.is_empty());
    assert!(matches!(
        definition,
        FeatureDefinition::Operation(FeatureOperation::TrimSurface {
            faces: FaceSelection::Native(ref faces),
            tool: cadmpeg_ir::features::PathRef::Native(ref tool),
            keep: cadmpeg_ir::features::TrimRegion::Unresolved,
        }) if faces == &target_group.id && tool == &tool_group.id
    ));
}

#[test]
fn surface_trim_projects_multi_member_target_and_tool_groups() {
    let scope = surface_trim_scope(vec![1201, 1202, 1203, 1204, 1205, 1206, 1207]);
    let target_group =
        group_with_members(1200, 0, 1201, &[1202, 1203], DesignOperandRole::BODIES_A);
    let tool_group = group_with_members(
        1200,
        3,
        1204,
        &[1205, 1206, 1207],
        DesignOperandRole::ROLE_0X21,
    );
    let mut losses = Vec::new();
    let definition = project_surface_trim(
        &scope,
        &[tool_group.clone(), target_group.clone()],
        &[
            trim_body(1201, 0, 1202),
            trim_body(1201, 1, 1203),
            trim_body(1204, 1, 1206),
        ],
        &[trim_curves(1204, 0, 1205), trim_curves(1204, 2, 1207)],
        &mut losses,
    )
    .expect("typed SurfaceTrim");
    assert!(losses.is_empty());
    assert!(matches!(
        definition,
        FeatureDefinition::Operation(FeatureOperation::TrimSurface {
            faces: FaceSelection::Native(ref faces),
            tool: cadmpeg_ir::features::PathRef::Native(ref tool),
            ..
        }) if faces == &target_group.id && tool == &tool_group.id
    ));
}

#[test]
fn surface_trim_requires_groups_that_partition_the_references() {
    let scope = surface_trim_scope(vec![1201, 1202, 1203, 1204, 1205, 1206, 1207]);
    let target_group =
        group_with_members(1200, 0, 1201, &[1202, 1203], DesignOperandRole::BODIES_A);
    let bodies = [trim_body(1201, 0, 1202), trim_body(1201, 1, 1203)];
    let short_tool = group_with_members(1200, 3, 1204, &[1205, 1206], DesignOperandRole::ROLE_0X21);
    let mut losses = Vec::new();
    assert!(project_surface_trim(
        &scope,
        &[target_group.clone(), short_tool],
        &bodies,
        &[trim_curves(1204, 0, 1205), trim_curves(1204, 1, 1206)],
        &mut losses,
    )
    .is_none());
    let misplaced_tool = group_with_members(
        1200,
        2,
        1203,
        &[1204, 1205, 1206, 1207],
        DesignOperandRole::ROLE_0X21,
    );
    assert!(project_surface_trim(
        &scope,
        &[target_group, misplaced_tool],
        &bodies,
        &[
            trim_curves(1203, 0, 1204),
            trim_curves(1203, 1, 1205),
            trim_curves(1203, 2, 1206),
            trim_curves(1203, 3, 1207),
        ],
        &mut losses,
    )
    .is_none());
    assert!(losses.is_empty());
}

fn untyped_trim_member_loss(members: usize) -> cadmpeg_ir::report::loss::LossNote {
    crate::loss::F3dLossCode::FeatureTrimMemberTypeAssumed.note(format!(
        "SurfaceTrim f3d:Design/BulkStream.dat:scope#1200 projects {members} group member(s) that have no decoded operand record as target bodies or trimming tools by the role of their group."
    ))
}

#[test]
fn surface_trim_projects_untyped_members_by_group_role_with_a_loss() {
    let (scope, target_group, tool_group, body, curves) = surface_trim_fixture();
    let groups = [target_group.clone(), tool_group.clone()];

    let mut losses = Vec::new();
    let definition = project_surface_trim(
        &scope,
        &groups,
        &[],
        std::slice::from_ref(&curves),
        &mut losses,
    )
    .expect("SurfaceTrim with an untyped target member");
    assert!(matches!(
        definition,
        FeatureDefinition::Operation(FeatureOperation::TrimSurface {
            faces: FaceSelection::Native(ref faces),
            tool: cadmpeg_ir::features::PathRef::Native(ref tool),
            keep: cadmpeg_ir::features::TrimRegion::Unresolved,
        }) if faces == &target_group.id && tool == &tool_group.id
    ));
    assert_eq!(losses, vec![untyped_trim_member_loss(1)]);

    for tool_records in [Vec::new(), vec![trim_curves(1203, 1, 1204)]] {
        let mut losses = Vec::new();
        let definition = project_surface_trim(
            &scope,
            &groups,
            std::slice::from_ref(&body),
            &tool_records,
            &mut losses,
        )
        .expect("SurfaceTrim with an untyped tool member");
        assert!(matches!(
            definition,
            FeatureDefinition::Operation(FeatureOperation::TrimSurface {
                faces: FaceSelection::Historical { ref faces, ref native, .. },
                tool: cadmpeg_ir::features::PathRef::Native(ref tool),
                keep: cadmpeg_ir::features::TrimRegion::Unresolved,
            }) if faces.len() == 1
                && native.as_str() == target_group.id
                && tool == &tool_group.id
        ));
        assert_eq!(losses, vec![untyped_trim_member_loss(1)]);
    }

    let mut losses = Vec::new();
    assert!(project_surface_trim(&scope, &groups, &[], &[], &mut losses).is_some());
    assert_eq!(losses, vec![untyped_trim_member_loss(2)]);
}

#[test]
fn surface_trim_projects_a_trimming_body_tool() {
    let (scope, target_group, tool_group, body, _) = surface_trim_fixture();
    let mut losses = Vec::new();
    let definition = project_surface_trim(
        &scope,
        &[target_group, tool_group.clone()],
        &[body, trim_body(1203, 0, 1204)],
        &[],
        &mut losses,
    )
    .expect("typed SurfaceTrim");
    assert!(matches!(
        definition,
        FeatureDefinition::Operation(FeatureOperation::TrimSurface {
            tool: cadmpeg_ir::features::PathRef::Native(ref tool),
            ..
        }) if tool == &tool_group.id
    ));
    assert!(losses.is_empty());
}

#[test]
fn surface_trim_rejects_an_entity_selection_target_member() {
    let (scope, target_group, tool_group, _, curves) = surface_trim_fixture();
    let mut losses = Vec::new();
    assert!(project_surface_trim(
        &scope,
        &[target_group, tool_group],
        &[],
        &[trim_curves(1201, 0, 1202), curves],
        &mut losses,
    )
    .is_none());
    assert!(losses.is_empty());
}

fn assert_native_cells(ordinals: &[u64], trailing_value: u32) {
    let scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#1200",
        crate::records::feature::scope::DesignFeatureKind::SurfaceTrim,
        1200,
    );
    let mut features = [unresolved_trim_feature(&scope)];
    let operation = trim_operation(ordinals, trailing_value);
    let mut losses = Vec::new();
    bind_surface_trim_cell_selections(
        &mut features,
        std::slice::from_ref(&scope),
        std::slice::from_ref(&operation),
        &mut losses,
    );
    assert!(matches!(
        features[0].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::TrimSurface {
            keep: cadmpeg_ir::features::TrimRegion::Unresolved,
            ..
        })
    ));
    assert_eq!(
        operation
            .cell_entries()
            .iter()
            .map(|entry| entry.ordinal)
            .collect::<Vec<_>>(),
        ordinals
    );
    assert_eq!(operation.trailing_value, trailing_value);
    assert_eq!(
        losses
            .iter()
            .map(|loss| (loss.code.clone(), loss.message.as_str()))
            .collect::<Vec<_>>(),
        [(
            crate::loss::F3dLossCode::FeatureTrimCellTableNative.kind(),
            "1 SurfaceTrim cell table(s) keep native entries because their entry count reaches the trailing value or their entry values repeat."
        )]
    );
}

#[test]
fn surface_trim_keeps_native_cells_when_entries_fill_the_trailing_value() {
    assert_native_cells(&[1, 2], 2);
    assert_native_cells(&[1, 1], 2);
}

#[test]
fn surface_trim_keeps_native_cells_when_entry_values_repeat_below_the_trailing_value() {
    assert_native_cells(&[1, 1], 3);
}

fn unresolved_trim_feature(scope: &DesignParameterScope) -> cadmpeg_ir::features::Feature {
    cadmpeg_ir::features::Feature {
        product_definition: None,
        id: cadmpeg_ir::features::FeatureId::mint("f3d:test:feature#1200").unwrap(),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: Default::default(),
        source_properties: Default::default(),
        source_tag: None,
        source_text: None,
        source_content: Default::default(),
        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::TrimSurface {
                faces: FaceSelection::Unresolved,
                tool: cadmpeg_ir::features::PathRef::Unresolved("tool".into()),
                keep: cadmpeg_ir::features::TrimRegion::Unresolved,
            }),
        ),
        native_ref: Some(scope.id.clone()),
    }
}

fn trim_operation(ordinals: &[u64], trailing_value: u32) -> DesignSurfaceTrimOperation {
    DesignSurfaceTrimOperation::try_from(
        crate::records::feature::surface_ops::DesignSurfaceTrimOperationWire {
            id: "f3d:Design/BulkStream.dat:design-surface-trim-operation#1200".into(),
            scope_record_index: 1200,
            chain_records: [
                crate::records::feature::surface_ops::DesignSurfaceTrimChainRecord {
                    record_index: 2,
                    byte_offset: 0,
                    class_tag: "288".to_owned().try_into().unwrap(),
                    frame_length: 96,
                },
                crate::records::feature::surface_ops::DesignSurfaceTrimChainRecord {
                    record_index: 6,
                    byte_offset: 96,
                    class_tag: "271".to_owned().try_into().unwrap(),
                    frame_length: 21,
                },
            ],
            cell_table_record_index: 3,
            cell_table_byte_offset: 0,
            cell_table_class_tag: "350".to_owned().try_into().unwrap(),
            cell_table_frame_length: 0,
            cell_table_paired_class_tag: "266".to_owned().try_into().unwrap(),
            cell_table_paired_byte_offset: 0,
            cell_count_offset: 0,
            cell_entries: ordinals
                .iter()
                .zip(4..)
                .map(|(ordinal, record_index)| DesignSurfaceTrimCellEntry {
                    record_index,
                    record_reference_offset: 0,
                    ordinal: *ordinal,
                    ordinal_offset: 0,
                })
                .collect(),
            trailing_value,
            trailing_value_offset: 0,
            trailing_zero_offset: 0,
        },
    )
    .unwrap()
}

#[test]
fn surface_trim_binds_selected_cells_without_inventing_a_side() {
    let scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#1200",
        crate::records::feature::scope::DesignFeatureKind::SurfaceTrim,
        1200,
    );
    let mut feature = cadmpeg_ir::features::Feature {
        product_definition: None,
        id: cadmpeg_ir::features::FeatureId::mint("f3d:test:feature#1200")
            .expect("identity grammar"),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: Default::default(),
        source_properties: Default::default(),
        source_tag: None,
        source_text: None,
        source_content: Default::default(),
        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::TrimSurface {
                faces: FaceSelection::Unresolved,
                tool: cadmpeg_ir::features::PathRef::Unresolved("tool".into()),
                keep: cadmpeg_ir::features::TrimRegion::Unresolved,
            }),
        ),
        native_ref: None,
    };
    feature.native_ref = Some(scope.id.clone());
    let operation = DesignSurfaceTrimOperation::try_from(
        crate::records::feature::surface_ops::DesignSurfaceTrimOperationWire {
            id: "f3d:Design/BulkStream.dat:design-surface-trim-operation#1200".into(),
            scope_record_index: 1200,
            chain_records: [
                crate::records::feature::surface_ops::DesignSurfaceTrimChainRecord {
                    record_index: 2,
                    byte_offset: 0,
                    class_tag: "288".to_owned().try_into().unwrap(),
                    frame_length: 11,
                },
                crate::records::feature::surface_ops::DesignSurfaceTrimChainRecord {
                    record_index: 6,
                    byte_offset: 11,
                    class_tag: "271".to_owned().try_into().unwrap(),
                    frame_length: 11,
                },
            ],
            cell_table_record_index: 3,
            cell_table_byte_offset: 0,
            cell_table_class_tag: crate::records::references::DesignClassTag::try_from(
                "325".to_owned(),
            )
            .unwrap(),
            cell_table_frame_length: 0,
            cell_table_paired_class_tag: crate::records::references::DesignClassTag::try_from(
                "257".to_owned(),
            )
            .unwrap(),
            cell_table_paired_byte_offset: 0,
            cell_count_offset: 0,
            cell_entries: vec![
                DesignSurfaceTrimCellEntry {
                    record_index: 4,
                    record_reference_offset: 0,
                    ordinal: 1,
                    ordinal_offset: 0,
                },
                DesignSurfaceTrimCellEntry {
                    record_index: 5,
                    record_reference_offset: 0,
                    ordinal: 4,
                    ordinal_offset: 0,
                },
            ],
            trailing_value: 5,
            trailing_value_offset: 0,
            trailing_zero_offset: 0,
        },
    )
    .unwrap();

    let mut losses = Vec::new();
    bind_surface_trim_cell_selections(
        std::slice::from_mut(&mut feature),
        std::slice::from_ref(&scope),
        std::slice::from_ref(&operation),
        &mut losses,
    );
    assert!(losses.is_empty());

    assert!(matches!(
        feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::TrimSurface {
            keep: cadmpeg_ir::features::TrimRegion::Cells(ref selection),
            ..
        }) if wire::field::<Vec<u64>>(&selection, "removed") == [1, 4] && wire::field::<u64>(&selection, "total") == 5
    ));
}

#[test]
fn surface_trim_selected_cells_keep_cells() {
    let scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#1200",
        crate::records::feature::scope::DesignFeatureKind::SurfaceTrim,
        1200,
    );
    let feature = cadmpeg_ir::features::Feature {
        product_definition: None,
        id: cadmpeg_ir::features::FeatureId::mint("f3d:test:feature#1200").unwrap(),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: Default::default(),
        source_properties: Default::default(),
        source_tag: None,
        source_text: None,
        source_content: Default::default(),
        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::TrimSurface {
                faces: FaceSelection::Unresolved,
                tool: cadmpeg_ir::features::PathRef::Unresolved("tool".into()),
                keep: cadmpeg_ir::features::TrimRegion::Unresolved,
            }),
        ),
        native_ref: Some(scope.id.clone()),
    };
    let operation = DesignSurfaceTrimOperation::try_from(
        crate::records::feature::surface_ops::DesignSurfaceTrimOperationWire {
            id: "f3d:Design/BulkStream.dat:design-surface-trim-operation#1200".into(),
            scope_record_index: 1200,
            chain_records: [
                crate::records::feature::surface_ops::DesignSurfaceTrimChainRecord {
                    record_index: 2,
                    byte_offset: 0,
                    class_tag: "288".to_owned().try_into().unwrap(),
                    frame_length: 11,
                },
                crate::records::feature::surface_ops::DesignSurfaceTrimChainRecord {
                    record_index: 6,
                    byte_offset: 11,
                    class_tag: "271".to_owned().try_into().unwrap(),
                    frame_length: 11,
                },
            ],
            cell_table_record_index: 3,
            cell_table_byte_offset: 0,
            cell_table_class_tag: "325".to_owned().try_into().unwrap(),
            cell_table_frame_length: 0,
            cell_table_paired_class_tag: "257".to_owned().try_into().unwrap(),
            cell_table_paired_byte_offset: 0,
            cell_count_offset: 0,
            cell_entries: vec![
                DesignSurfaceTrimCellEntry {
                    record_index: 4,
                    record_reference_offset: 0,
                    ordinal: 1,
                    ordinal_offset: 0,
                },
                DesignSurfaceTrimCellEntry {
                    record_index: 5,
                    record_reference_offset: 0,
                    ordinal: 4,
                    ordinal_offset: 0,
                },
            ],
            trailing_value: 5,
            trailing_value_offset: 0,
            trailing_zero_offset: 0,
        },
    )
    .unwrap();
    let mut unchanged = [feature.clone()];
    bind_surface_trim_cell_selections(
        &mut unchanged,
        std::slice::from_ref(&scope),
        std::slice::from_ref(&operation),
        &mut Vec::new(),
    );
    assert!(matches!(
        unchanged[0].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::TrimSurface {
            keep: cadmpeg_ir::features::TrimRegion::Cells(_),
            ..
        })
    ));
}
