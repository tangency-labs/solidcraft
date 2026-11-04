// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::u64_from_index;

use crate::design::feature_project::{
    project_delete_face, project_split, project_split_face, selected_work_planes,
};
use crate::ids::feature_input_topology_id;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::topology::construction::DesignConstructionOperandGroup;
use crate::records::topology::{
    construction::DesignConstructionOperandGroupFrame, extrude_selection::DesignOperandRole,
};
use cadmpeg_ir::features::FaceSelection;
use cadmpeg_ir::features::FeatureDefinition;
use cadmpeg_ir::features::FeatureOperation;

fn group(
    scope_record_index: u32,
    scope_reference_ordinal: u32,
    record_index: u32,
    members: Vec<u32>,
    role: DesignOperandRole,
) -> DesignConstructionOperandGroup {
    DesignConstructionOperandGroup::try_from(
        crate::records::topology::construction::DesignConstructionOperandGroupDraft {
            id: format!("f3d:Design/BulkStream.dat:group#{record_index}"),
            scope_record_index,
            scope_reference_ordinal,
            record_index,
            byte_offset: 0,
            class_tag: crate::records::references::DesignClassTag::try_from("262".to_owned())
                .unwrap(),

            members: members
                .into_iter()
                .enumerate()
                .map(|(index, value)| crate::records::identity::Located {
                    value,
                    offset: u64_from_index(index) * 11,
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
                    opaque_index: 1,
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

fn split_body_scope() -> DesignParameterScope {
    split_body_scope_referencing(vec![100, 101, 102, 103])
}

fn split_body_scope_referencing(references: Vec<u32>) -> DesignParameterScope {
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:scope#77",
        crate::records::feature::scope::DesignFeatureKind::Split,
        77,
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

fn split_body_groups(tool_role: DesignOperandRole) -> [DesignConstructionOperandGroup; 2] {
    [
        group(77, 0, 100, vec![101], tool_role),
        group(77, 2, 102, vec![103], DesignOperandRole::BODIES_A),
    ]
}

fn split_body_face_tool() -> crate::records::topology::face::DesignFaceOperand {
    use crate::records::recipes::ConstructionRecipeKind;
    crate::records::topology::face::DesignFaceOperand::try_new(
        crate::records::topology::face::DesignFaceOperandDraft {
            id: "f3d:Design/BulkStream.dat:face-operand#101".into(),
            scope_record_index: 77,
            scope_reference_ordinal: 1,
            group: Some(crate::records::topology::body_recipe::DesignOperandGroup {
                group_record_index: 100,
                group_member_ordinal: 0,
            }),
            record_index: 101,
            byte_offset: 1200,
            class_tag: "297".to_owned().try_into().unwrap(),
            paired_byte_offset: 1250,
            paired_class_tag: "259".to_owned().try_into().unwrap(),
            recipe_record_index: 104,
            recipe_record_byte_offset: 1300,
            recipe_id: "f3d:Design/BulkStream.dat:recipe#104".into(),
            recipe_prefix_offset: 1311,
            recipe_prefix_bytes: Vec::new(),
            recipe_references: Vec::new(),
            recipe_kind: ConstructionRecipeKind::Face,
            recipe_program_offset: 1350,
            recipe_program: vec![0, -1],
            recipe_nodes: Vec::new(),
            candidate_faces: Vec::new(),
            unreferenced_candidate_faces: Vec::new(),
            alternate_selector_candidate_faces: Vec::new(),
            preceding_candidate_faces: Vec::new(),
            changed_candidate_faces: Vec::new(),
            historical_support_contexts: Vec::new(),
            resolved_face_slots: Vec::new(),
            resolved_active_face: None,
            next_record_index: 105,
            next_byte_offset: 1411,
        },
    )
    .unwrap()
}

#[test]
fn split_body_path_tool_keeps_group_id() {
    let scope = split_body_scope();
    let groups = split_body_groups(DesignOperandRole::ROLE_0X21);
    let definition = project_split(&scope, &[], &groups, &[], &[])
        .unwrap()
        .unwrap();
    assert!(matches!(
        definition,
        FeatureDefinition::Operation(FeatureOperation::SplitBody {
            tool: cadmpeg_ir::features::SplitBodyTool::Faces(FaceSelection::Native(ref tool)), ..
        }) if tool == &groups[0].id
    ));
}

#[test]
fn split_body_face_tool_keeps_native_id() {
    let scope = split_body_scope();
    let groups = split_body_groups(DesignOperandRole::ROLE_0X9);
    let tool = split_body_face_tool();
    let definition = project_split(&scope, &[], &groups, &[], std::slice::from_ref(&tool))
        .unwrap()
        .unwrap();
    assert!(matches!(
        definition,
        FeatureDefinition::Operation(FeatureOperation::SplitBody {
            tool: cadmpeg_ir::features::SplitBodyTool::Faces(FaceSelection::Native(ref native)), ..
        }) if native == &tool.id
    ));
}

#[test]
fn split_body_historical_face_tool_keeps_native_id() {
    let mut scope = split_body_scope();
    scope
        .try_edit(|draft| {
            draft.previous_history_state_id = Some(7);
            draft.layout_fixture_tail();
        })
        .unwrap();
    let groups = split_body_groups(DesignOperandRole::ROLE_0X9);
    let mut tool = split_body_face_tool();
    tool.resolved_face_slots = vec![42];
    let definition = project_split(&scope, &[], &groups, &[], std::slice::from_ref(&tool))
        .unwrap()
        .unwrap();
    assert!(matches!(
        definition,
        FeatureDefinition::Operation(FeatureOperation::SplitBody {
            tool: cadmpeg_ir::features::SplitBodyTool::Faces(FaceSelection::Historical { ref native, .. }), ..
        }) if native.as_str() == tool.id
    ));
}

#[test]
fn delete_face_falls_back_to_group_id() {
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:scope#77",
        crate::records::feature::scope::DesignFeatureKind::DeleteFace,
        77,
    );
    scope
        .try_edit(|draft| {
            draft.frame_length = 258;
            draft.kind_offset = draft.byte_offset + 161;
            draft.reference_members =
                crate::records::identity::ReferenceRun::unlocated(vec![100, 200]);
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.reference_count_offset = draft.kind_offset - 34;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    let selected = group(77, 0, 100, vec![200], DesignOperandRole::ROLE_0X10);
    let definition = project_delete_face(&scope, std::slice::from_ref(&selected), &[])
        .unwrap()
        .unwrap();
    assert!(matches!(
        definition,
        FeatureDefinition::Operation(FeatureOperation::DeleteFace {
            faces: FaceSelection::Native(ref native), ..
        }) if native == &selected.id
    ));

    // The compact frame admits the 78-byte fixed scope tail, one byte
    // longer; a frame two bytes longer is not compact.
    for (frame_length, admitted) in [(259, true), (260, false)] {
        let mut longer = scope.clone();
        longer
            .try_edit(|draft| {
                draft.frame_length = frame_length;
                draft.paired_byte_offset = draft.byte_offset + frame_length;
                draft.layout_fixture_tail();
            })
            .unwrap();
        assert_eq!(
            project_delete_face(&longer, std::slice::from_ref(&selected), &[])
                .unwrap()
                .is_some(),
            admitted
        );
    }

    // The legacy frame is four bytes shorter in front of the kind and in all.
    // Its class pair, here one from the system76 Launch files, does not
    // select it; one byte longer is not that frame.
    for (frame_length, admitted) in [(254, true), (255, false)] {
        let mut legacy = scope.clone();
        legacy.class_tag =
            crate::records::references::DesignClassTag::try_from("399".to_owned()).unwrap();
        legacy.paired_class_tag =
            crate::records::references::DesignClassTag::try_from("257".to_owned()).unwrap();
        legacy
            .try_edit(|draft| {
                draft.frame_length = frame_length;
                draft.kind_offset = draft.byte_offset + 157;
                draft.reference_count_offset = draft.kind_offset - 34;
                draft.paired_byte_offset = draft.byte_offset + frame_length;
                draft.layout_fixture_references();
                draft.layout_fixture_tail();
            })
            .unwrap();
        assert_eq!(
            project_delete_face(&legacy, std::slice::from_ref(&selected), &[])
                .unwrap()
                .is_some(),
            admitted
        );
    }
}

fn compact_split_face_fixture() -> (DesignParameterScope, [DesignConstructionOperandGroup; 2]) {
    let scope_record_index = 77;
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:scope#77",
        crate::records::feature::scope::DesignFeatureKind::SplitFace,
        scope_record_index,
    );
    scope.class_tag =
        crate::records::references::DesignClassTag::try_from("277".to_owned()).unwrap();
    scope.paired_class_tag =
        crate::records::references::DesignClassTag::try_from("258".to_owned()).unwrap();
    scope
        .try_edit(|draft| {
            draft.frame_length = 407;
            draft.reference_members =
                crate::records::identity::ReferenceRun::unlocated((100..112).collect());
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();

    let groups = [
        group(
            scope_record_index,
            0,
            100,
            vec![101],
            DesignOperandRole::ROLE_0X21,
        ),
        group(
            scope_record_index,
            2,
            102,
            (103..112).collect(),
            DesignOperandRole::ROLE_0X10,
        ),
    ];
    (scope, groups)
}

#[test]
fn class_277_258_compact_split_face_frame_projects() {
    let (mut scope, groups) = compact_split_face_fixture();
    let definition = project_split_face(&scope, &[scope.clone()], &groups, &[], &[], &[])
        .expect("project the split face feature")
        .expect("class-277 SplitFace frame");
    assert!(matches!(
        definition,
        FeatureDefinition::Operation(FeatureOperation::SplitFace {
            targets: FaceSelection::Native(targets),
            tool: cadmpeg_ir::features::SplitFaceTool::Path(
                cadmpeg_ir::features::PathRef::Native(tool)
            ),
        }) if targets.ends_with("group#102") && tool.ends_with("group#100")
    ));

    scope.class_tag =
        crate::records::references::DesignClassTag::try_from("418".to_owned()).unwrap();
    scope.paired_class_tag =
        crate::records::references::DesignClassTag::try_from("266".to_owned()).unwrap();
    assert!(
        project_split_face(&scope, &[scope.clone()], &groups, &[], &[], &[])
            .expect("project the split face feature")
            .is_some()
    );

    scope.class_tag =
        crate::records::references::DesignClassTag::try_from("277".to_owned()).unwrap();
    scope.paired_class_tag =
        crate::records::references::DesignClassTag::try_from("266".to_owned()).unwrap();
    assert!(
        project_split_face(&scope, &[scope.clone()], &groups, &[], &[], &[])
            .expect("project the split face feature")
            .is_none()
    );
}

fn work_plane(record_index: u32) -> DesignParameterScope {
    let mut plane = DesignParameterScope::empty(
        &format!("f3d:Design/BulkStream.dat:scope#{record_index}"),
        crate::records::feature::scope::DesignFeatureKind::WorkPlane,
        record_index,
    );
    plane.with_work_plane_transform(
        [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ]
        .try_into()
        .unwrap(),
    );
    plane
}

fn compact_plane_selection(
    record_index: u32,
    group_member_ordinal: u32,
    primary_identity: u64,
) -> crate::records::topology::entity_selection::DesignEntitySelectionOperand {
    crate::records::topology::entity_selection::DesignEntitySelectionOperand::try_new(
        crate::records::topology::entity_selection::DesignEntitySelectionOperandDraft {
            id: format!("f3d:Design/BulkStream.dat:entity-selection#{record_index}"),
            scope_record_index: 77,
            group_record_index: 100,
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
            identity_record_index: record_index + 3,
            identity_record_offset: 0,
            primary_identity,
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

fn selected_plane_fixture() -> (
    DesignParameterScope,
    DesignConstructionOperandGroup,
    crate::records::topology::entity_selection::DesignEntitySelectionOperand,
    DesignParameterScope,
) {
    let (scope, groups) = compact_split_face_fixture();
    (
        scope,
        groups[0].clone(),
        compact_plane_selection(101, 0, 600),
        work_plane(601),
    )
}

#[test]
fn selected_work_planes_select_target_plane() {
    let (scope, group, selection, plane) = selected_plane_fixture();
    let selected = selected_work_planes(
        &scope,
        &group,
        std::slice::from_ref(&selection),
        std::slice::from_ref(&plane),
    )
    .unwrap();
    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0].record_index, plane.record_index);
}

fn split_body_tool(definition: &FeatureDefinition) -> &cadmpeg_ir::features::SplitBodyTool {
    let FeatureDefinition::Operation(FeatureOperation::SplitBody { tool, .. }) = definition else {
        panic!("expected a SplitBody operation");
    };
    tool
}

#[test]
fn split_body_work_plane_tool_selects_the_plane_feature() {
    let scope = split_body_scope();
    let groups = split_body_groups(DesignOperandRole::ROLE_0X21);
    let plane = work_plane(601);
    let definition = project_split(
        &scope,
        std::slice::from_ref(&plane),
        &groups,
        &[compact_plane_selection(101, 0, 600)],
        &[],
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        split_body_tool(&definition),
        &cadmpeg_ir::features::SplitBodyTool::Plane {
            plane: crate::ids::neutral_feature_id(&plane),
        }
    );
}

#[test]
fn split_body_tool_without_exactly_one_work_plane_keeps_the_group() {
    let native = |group: &DesignConstructionOperandGroup| {
        cadmpeg_ir::features::SplitBodyTool::Faces(FaceSelection::Native(group.id.clone()))
    };
    let scope = split_body_scope();
    let groups = split_body_groups(DesignOperandRole::ROLE_0X21);
    let planes = [work_plane(601), work_plane(701)];

    // The identity names the record before a scope that is not a WorkPlane.
    let definition = project_split(
        &scope,
        &planes,
        &groups,
        &[compact_plane_selection(101, 0, 601)],
        &[],
    )
    .unwrap()
    .unwrap();
    assert_eq!(split_body_tool(&definition), &native(&groups[0]));

    // Two WorkPlane members have no single splitting plane.
    let scope = split_body_scope_referencing(vec![100, 101, 102, 103, 104]);
    let groups = [
        group(77, 0, 100, vec![101, 102], DesignOperandRole::ROLE_0X21),
        group(77, 3, 103, vec![104], DesignOperandRole::BODIES_A),
    ];
    let selections = [
        compact_plane_selection(101, 0, 600),
        compact_plane_selection(102, 1, 700),
    ];
    let definition = project_split(&scope, &planes, &groups, &selections, &[])
        .unwrap()
        .unwrap();
    assert_eq!(split_body_tool(&definition), &native(&groups[0]));
}

#[test]
fn a_split_body_plane_tool_is_a_dependency_of_the_split() {
    let mut plane = work_plane(601);
    plane.feature_ordinal = std::num::NonZeroU32::new(1).unwrap();
    let mut split = split_body_scope();
    split.feature_ordinal = std::num::NonZeroU32::new(2).unwrap();
    let scopes = [plane, split];
    let timelines = crate::design::test_support::synthetic_feature_timelines(&scopes);
    let groups = split_body_groups(DesignOperandRole::ROLE_0X21);
    let selections = [compact_plane_selection(101, 0, 600)];
    let projected = crate::test_support::with_decode_context(|ctx| {
        crate::design::feature_project::project_parameter_design_with_edge_identities(
            ctx,
            &crate::design::feature_project::ProjectInputs {
                scopes: &scopes,
                timelines: &timelines,
                construction_groups: &groups,
                entity_selection_operands: &selections,
                ..Default::default()
            },
        )
    })
    .expect("exact synthetic feature timeline");
    let plane_feature = crate::ids::neutral_feature_id(&scopes[0]);
    let split_feature = projected
        .features
        .iter()
        .find(|feature| feature.id == crate::ids::neutral_feature_id(&scopes[1]))
        .expect("projected Split");
    assert_eq!(
        split_body_tool(split_feature.evaluation.definition()),
        &cadmpeg_ir::features::SplitBodyTool::Plane {
            plane: plane_feature.clone(),
        }
    );
    assert_eq!(split_feature.dependencies.as_slice(), [plane_feature]);
}

#[test]
fn direct_single_identity_split_face_member_projects_historical_edge_path() {
    let scope_record_index = 77;
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:scope#77",
        crate::records::feature::scope::DesignFeatureKind::SplitFace,
        scope_record_index,
    );
    scope.class_tag =
        crate::records::references::DesignClassTag::try_from("277".to_owned()).unwrap();
    scope.paired_class_tag =
        crate::records::references::DesignClassTag::try_from("258".to_owned()).unwrap();
    scope
        .try_edit(|draft| {
            draft.frame_length = 407;
            draft.previous_history_state_id = Some(7);
            draft.reference_members =
                crate::records::identity::ReferenceRun::unlocated((100..112).collect());
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();

    let groups = [
        group(
            scope_record_index,
            0,
            100,
            vec![101],
            DesignOperandRole::ROLE_0X21,
        ),
        group(
            scope_record_index,
            2,
            102,
            (103..112).collect(),
            DesignOperandRole::ROLE_0X10,
        ),
    ];
    let selections = [
        crate::records::topology::entity_selection::DesignEntitySelectionOperand::try_new(
            crate::records::topology::entity_selection::DesignEntitySelectionOperandDraft {
                id: "f3d:Design/BulkStream.dat:entity-selection#101".into(),
                scope_record_index,
                group_record_index: 100,
                group_member_ordinal: 0,
                record_index: 101,
                byte_offset: 0,
                class_tag: crate::records::references::DesignClassTag::try_from("277".to_owned())
                    .unwrap(),
                asset_id: crate::records::mesh::DesignRelaxedGuidText::try_from(
                    "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d".to_owned(),
                )
                .unwrap(),
                asset_id_offset: 0,
                context_id: crate::records::mesh::DesignRelaxedGuidText::try_from(
                    "1b2c3d4e-5f6a-4b7c-8d9e-0f1a2b3c4d5e".to_owned(),
                )
                .unwrap(),
                context_id_offset: 0,
                identity_record_index: 104,
                identity_record_offset: 0,
                primary_identity: 225,
                primary_identity_offset: 21,
                secondary: None,
                historical_edge_candidates: Vec::new(),
                historical_face_candidates: Vec::new(),
                resolved_edge_slot: Some(42),
                next_record_index: 103,
                next_byte_offset: 29,
            },
        )
        .unwrap(),
    ];

    let definition = project_split_face(&scope, &[scope.clone()], &groups, &selections, &[], &[])
        .expect("project the split face feature")
        .expect("class-277 direct edge path");
    let FeatureDefinition::Operation(FeatureOperation::SplitFace {
        tool:
            cadmpeg_ir::features::SplitFaceTool::Path(cadmpeg_ir::features::PathRef::HistoricalEdges {
                state,
                edges,
                native,
            }),
        ..
    }) = definition
    else {
        panic!("expected historical edge path");
    };
    let feature = crate::ids::neutral_feature_id(&scope);
    let prefix = crate::ids::history_input_prefix(&feature.key(), 7);
    assert_eq!(state, feature_input_topology_id(&feature, 7),);
    assert_eq!(
        edges.as_slice(),
        vec![crate::ids::history_input_edge_id(&prefix, 42)],
    );
    assert_eq!(native.as_str(), groups[0].id);
}
