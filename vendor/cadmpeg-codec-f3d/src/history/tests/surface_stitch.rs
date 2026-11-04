// SPDX-License-Identifier: Apache-2.0
//! `SurfaceStitch` history-selection tests.
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

use crate::history::bind_feature_face_selections;
use crate::history_records::AsmHistory;
use crate::history_records::{AsmDeltaState, AsmHistoricalTopology};
use crate::records::topology::extrude_selection::DesignOperandRole;

struct StitchFixture {
    scope_id: String,
    feature_id: cadmpeg_ir::features::FeatureId,
    scope: crate::records::feature::scope::DesignParameterScope,
    groups: Vec<crate::records::topology::construction::DesignConstructionOperandGroup>,
    operands: Vec<crate::records::topology::entity_selection::DesignEntitySelectionOperand>,
    history: AsmHistory,
    feature: cadmpeg_ir::features::Feature,
    input_topologies: Vec<cadmpeg_ir::features::FeatureInputTopology>,
}

fn surface_stitch_fixture() -> StitchFixture {
    use crate::records::{
        feature::scope::DesignParameterScope,
        topology::{
            body_recipe::AsmHistoricalEntityKind, construction::DesignConstructionOperandGroup,
            construction::DesignConstructionOperandGroupFrame,
            entity_selection::DesignEntitySelectionFaceCandidate,
            entity_selection::DesignEntitySelectionOperand,
        },
    };
    use cadmpeg_ir::features::{
        FaceSelection, Feature, FeatureDefinition, FeatureId, FeatureInputTopology,
        FeatureOperation,
    };

    let stream = "f3d:Design/BulkStream.dat";
    let scope_id = format!("{stream}:design-parameter-scope#42");
    let history_id = format!("{stream}/BREP.surface:asm-1");
    let mut scope = DesignParameterScope::empty(
        &scope_id,
        crate::records::feature::scope::DesignScopePayload::SurfaceStitch(
            crate::records::feature::surface_ops::DesignSurfaceStitchOperation {
                gap_tolerance: cadmpeg_ir::scalar::PositiveReal::new(0.01).unwrap(),
                gap_tolerance_offset: 0,
                tolerance_record_index: 300,
                settings_record_index: 301,
            },
        ),
        42,
    );
    scope
        .try_edit(|draft| {
            draft.history_state_id = Some(2);
            draft.previous_history_state_id = Some(1);
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(vec![
                100, 200, 110, 210, 300, 301,
            ]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    let group = |record_index, scope_reference_ordinal, member| {
        DesignConstructionOperandGroup::try_from(
            crate::records::topology::construction::DesignConstructionOperandGroupDraft {
                id: format!("{stream}:design-construction-operand-group#{record_index}"),
                scope_record_index: 42,
                scope_reference_ordinal,
                record_index,
                byte_offset: 0,
                class_tag: crate::records::references::DesignClassTag::try_from("282".to_owned())
                    .unwrap(),
                members: vec![crate::records::identity::Located {
                    value: member,
                    offset: 0,
                }],
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
                operand_role: crate::records::topology::construction::DesignConstructionOperandRole::Other(
                    DesignOperandRole::ROLE_0X5,
                ),
                role_offset: 0,
                paired_class_tag: crate::records::references::DesignClassTag::try_from(
                    "261".to_owned(),
                )
                .unwrap(),
                paired_byte_offset: 0,
            },
        )
        .unwrap()
    };
    let groups = vec![group(100, 0, 200), group(110, 2, 210)];
    let operand = |group_record_index, record_index, face_slot| {
        DesignEntitySelectionOperand::try_new(
            crate::records::topology::entity_selection::DesignEntitySelectionOperandDraft {
                id: format!("{stream}:design-entity-selection-operand#{record_index}"),
                scope_record_index: 42,
                group_record_index,
                group_member_ordinal: 0,
                record_index,
                byte_offset: 0,
                class_tag: crate::records::references::DesignClassTag::try_from("377".to_owned())
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
                identity_record_index: record_index + 3,
                identity_record_offset: 0,
                primary_identity: u64::try_from(face_slot)
                    .expect("fixture reference is nonnegative"),
                primary_identity_offset: 21,
                secondary: None,
                historical_edge_candidates: Vec::new(),
                historical_face_candidates: vec![DesignEntitySelectionFaceCandidate {
                    history_id: history_id.clone(),
                    historical: crate::records::topology::fillet::HistoricalBinding {
                        kind: AsmHistoricalEntityKind::Coedge,
                        entity_ref: face_slot,
                        state_ids: vec![1],
                    },
                    face_slot,
                }],
                resolved_edge_slot: None,
                next_record_index: record_index + 2,
                next_byte_offset: 29,
            },
        )
        .unwrap()
    };
    let operands = vec![operand(100, 200, 30), operand(110, 210, 31)];
    let state = |state_id, next_ref| {
        (
            AsmDeltaState {
                id: format!("{history_id}:state#{state_id}"),
                parent: history_id.clone(),
                byte_offset: 0,
                state_id,
                version_flag: 1,
                state_flag: 0,
                previous_ref: None,
                next_ref,
                node_index: state_id,
                partner_ref: None,
                owner_ref: 0,
                bulletin_boards: Vec::new(),
                records: Vec::new(),
                transition: None,
            },
            Some(AsmHistoricalTopology::default()),
            Vec::new(),
        )
    };
    let history = crate::test_support::history::history(
        history_id.clone(),
        0,
        None,
        false,
        vec![state(2, Some(1)), state(1, None)],
    );
    let feature_id = FeatureId::mint("f3d:model:feature#42").expect("identity grammar");
    let mut feature = Feature {
        product_definition: None,
        id: feature_id.clone(),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: Default::default(),
        source_properties: Default::default(),
        source_tag: None,
        source_text: None,
        source_content: Default::default(),
        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::KnitSurface {
                faces: FaceSelection::Native(scope_id.clone()),
                merge_entities: Some(true),
                create_solid: Some(true),
                gap_tolerance: Some(cadmpeg_ir::scalar::NonNegativeLength::new(0.1).unwrap()),
            }),
        ),
        native_ref: None,
    };
    feature.native_ref = Some(scope_id.clone());
    let input_topologies = vec![FeatureInputTopology {
        id: crate::ids::feature_input_topology_id(&feature_id, 1),
        input_of: feature_id.clone(),
        bodies: cadmpeg_ir::features::DistinctMembers::try_from(Vec::new()).unwrap(),
        faces: cadmpeg_ir::features::DistinctMembers::try_from(Vec::new()).unwrap(),
        edges: cadmpeg_ir::features::DistinctMembers::try_from(Vec::new()).unwrap(),
        vertices: cadmpeg_ir::features::DistinctMembers::try_from(Vec::new()).unwrap(),
        edge_geometry: Vec::new(),
        native_ref: None,
    }];
    StitchFixture {
        scope_id,
        feature_id,
        scope,
        groups,
        operands,
        history,
        feature,
        input_topologies,
    }
}

#[test]
fn surface_stitch_binds_all_unique_entity_face_candidates() {
    use crate::records::topology::{
        body_recipe::AsmHistoricalEntityKind, entity_selection::DesignEntitySelectionFaceCandidate,
    };
    use cadmpeg_ir::features::{FaceSelection, FeatureDefinition, FeatureOperation};
    let StitchFixture {
        scope_id,
        feature_id,
        scope,
        groups,
        operands,
        mut history,
        mut feature,
        mut input_topologies,
    } = surface_stitch_fixture();
    let mut ambiguous_feature = feature.clone();
    let mut ambiguous_operands = operands.clone();
    ambiguous_operands[1]
        .historical_face_candidates
        .push(DesignEntitySelectionFaceCandidate {
            history_id: "other-history/BREP.other:asm-1".into(),
            historical: crate::records::topology::fillet::HistoricalBinding {
                kind: AsmHistoricalEntityKind::Coedge,
                entity_ref: 99,
                state_ids: vec![1],
            },
            face_slot: 99,
        });
    let mut ambiguous_topologies = input_topologies.clone();

    bind_feature_face_selections(
        std::slice::from_mut(&mut feature),
        &mut input_topologies,
        crate::history::FeatureFaceSelectionInputs {
            scopes: std::slice::from_ref(&scope),
            groups: &groups,
            operands: &[],
            entity_operands: &operands,
            body_recipe_operands: &[],
            histories: std::slice::from_mut(&mut history),
        },
    );

    let FeatureDefinition::Operation(FeatureOperation::KnitSurface {
        faces:
            FaceSelection::Historical {
                state,
                faces,
                native,
            },
        ..
    }) = feature.evaluation.definition()
    else {
        panic!("SurfaceStitch face selection remains unresolved");
    };
    let prefix = crate::ids::history_input_prefix(&cadmpeg_ir::identity_key!("42"), 1);
    assert_eq!(
        state,
        &crate::ids::feature_input_topology_id(&feature_id, 1)
    );
    assert_eq!(
        faces.as_slice(),
        &vec![
            crate::ids::history_input_face_id(&prefix, 30),
            crate::ids::history_input_face_id(&prefix, 31),
        ]
    );
    assert_eq!(native.as_str(), &scope_id);
    assert_eq!(input_topologies[0].faces.as_slice(), faces.as_slice());

    bind_feature_face_selections(
        std::slice::from_mut(&mut ambiguous_feature),
        &mut ambiguous_topologies,
        crate::history::FeatureFaceSelectionInputs {
            scopes: std::slice::from_ref(&scope),
            groups: &groups,
            operands: &[],
            entity_operands: &ambiguous_operands,
            body_recipe_operands: &[],
            histories: std::slice::from_mut(&mut history),
        },
    );
    assert!(matches!(
        ambiguous_feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::KnitSurface {
            faces: FaceSelection::Native(native),
            ..
        }) if native == &scope_id
    ));
    assert!(ambiguous_topologies[0].faces.is_empty());
}

#[test]
fn a_selection_other_than_move_face_reads_one_transition() {
    use cadmpeg_ir::features::{FaceSelection, FeatureDefinition, FeatureOperation};
    let StitchFixture {
        scope_id,
        mut scope,
        groups,
        operands,
        history,
        mut feature,
        mut input_topologies,
        ..
    } = surface_stitch_fixture();
    let state = |state_id, next_ref| {
        (
            AsmDeltaState {
                id: format!("{}:state#{state_id}", history.id),
                parent: history.id.clone(),
                byte_offset: 0,
                state_id,
                version_flag: 1,
                state_flag: 0,
                previous_ref: None,
                next_ref,
                node_index: state_id,
                partner_ref: None,
                owner_ref: 0,
                bulletin_boards: Vec::new(),
                records: Vec::new(),
                transition: None,
            },
            Some(AsmHistoricalTopology::default()),
            Vec::new(),
        )
    };
    let mut chained = crate::test_support::history::history(
        history.id.clone(),
        0,
        None,
        false,
        vec![state(3, Some(2)), state(2, Some(1)), state(1, None)],
    );
    scope
        .try_edit(|draft| draft.history_state_id = Some(3))
        .unwrap();

    bind_feature_face_selections(
        std::slice::from_mut(&mut feature),
        &mut input_topologies,
        crate::history::FeatureFaceSelectionInputs {
            scopes: std::slice::from_ref(&scope),
            groups: &groups,
            operands: &[],
            entity_operands: &operands,
            body_recipe_operands: &[],
            histories: std::slice::from_mut(&mut chained),
        },
    );
    assert!(matches!(
        feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::KnitSurface {
            faces: FaceSelection::Native(native),
            ..
        }) if native == &scope_id
    ));
}
