// SPDX-License-Identifier: Apache-2.0

use super::extrude_fallbacks::{profile_group, scope};
use crate::design::feature_project::project_fixed_sweep;
use crate::records::feature::extrude::DesignExtrudeOperation;
use crate::records::feature::path_features::DesignSweepConstruction;
use crate::records::feature::scope::{DesignScopePayload, DesignSweepScope};
use crate::records::topology::construction::DesignConstructionOperandRole;
use crate::records::topology::extrude_selection::DesignOperandRole;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};
use cadmpeg_ir::scalar::FiniteReal;

fn sweep_input() -> (
    crate::records::feature::scope::DesignParameterScope,
    [crate::records::topology::construction::DesignConstructionOperandGroup; 2],
) {
    let mut scope = scope();
    scope
        .try_edit(|draft| {
            draft.payload = DesignScopePayload::Sweep(Some(DesignSweepScope {
                construction: Some(DesignSweepConstruction {
                    operation: DesignExtrudeOperation::NewBody,
                    operation_offset: 128,
                    values: [0.0, 0.0, 1.0, 1.0, 0.0, 0.0]
                        .map(|value| FiniteReal::new(value).unwrap())
                        .to_vec(),
                    record_indexes: vec![0; 6],
                    value_offsets: vec![128; 6],
                }),
                sweep_profile: None,
            }));
        })
        .unwrap();
    let mut profile = profile_group(100, 0);
    profile.operand_role = DesignConstructionOperandRole::Other(DesignOperandRole::PROFILE);
    let mut path = profile_group(101, 1);
    path.operand_role = DesignConstructionOperandRole::Other(DesignOperandRole::ROLE_0X5);
    (scope, [profile, path])
}

fn project(
    ctx: &DecodeContext<'_>,
    scope: &crate::records::feature::scope::DesignParameterScope,
    groups: &[crate::records::topology::construction::DesignConstructionOperandGroup],
) -> Result<Option<FeatureDefinition>, CodecError> {
    project_fixed_sweep(scope, groups, &[], &[], &[], &[], ctx)
}

#[test]
fn sweep_projects_profile_and_path() {
    let (scope, groups) = sweep_input();
    assert!(matches!(
        crate::test_support::with_decode_context(|decode_ctx| project(decode_ctx, &scope, &groups))
            .unwrap()
            .unwrap(),
        FeatureDefinition::Operation(FeatureOperation::Sweep { .. })
    ));
}

#[test]
fn sweep_with_body_group_is_not_projected() {
    let (scope, [profile, path]) = sweep_input();
    let mut body = profile_group(102, 2);
    body.operand_role = DesignConstructionOperandRole::Other(DesignOperandRole::BODIES_A);
    let groups = [profile, path, body];
    assert!(
        crate::test_support::with_decode_context(|decode_ctx| project(decode_ctx, &scope, &groups))
            .unwrap()
            .is_none()
    );
}

#[test]
fn boolean_sweep_takes_a_leading_body_group_of_its_ordinal_role() {
    let (mut scope, [profile, path]) = sweep_input();
    let set_operation = |scope: &mut crate::records::feature::scope::DesignParameterScope,
                         operation: DesignExtrudeOperation| {
        if let crate::records::feature::scope::DesignScopePayloadMut::Sweep(Some(sweep)) =
            scope.payload_mut()
        {
            sweep.construction.as_mut().unwrap().operation = operation;
        }
    };
    set_operation(&mut scope, DesignExtrudeOperation::Cut);
    let body = |record_index: u32, ordinal: u32, role: DesignOperandRole| {
        let mut body = profile_group(record_index, ordinal);
        body.operand_role = DesignConstructionOperandRole::Other(role);
        body
    };
    let after = |ordinal: u32| {
        let mut profile = profile.clone();
        let mut path = path.clone();
        profile.scope_reference_ordinal = ordinal + 2;
        path.scope_reference_ordinal = ordinal + 4;
        [profile, path]
    };
    let projected = |scope: &crate::records::feature::scope::DesignParameterScope,
                     groups: &[crate::records::topology::construction::DesignConstructionOperandGroup]| {
        crate::test_support::with_decode_context(|decode_ctx| project(decode_ctx, scope, groups))
            .unwrap()
    };
    for (role, ordinal) in [
        (DesignOperandRole::BODIES_A, 0),
        (DesignOperandRole::BODIES_B, 7),
    ] {
        let [profile, path] = after(ordinal);
        let groups = [body(102, ordinal, role), profile, path];
        assert!(
            matches!(
                projected(&scope, &groups),
                Some(FeatureDefinition::Operation(FeatureOperation::Sweep { .. }))
            ),
            "{role:?}"
        );
    }
    for (role, ordinal) in [
        (DesignOperandRole::BODIES_A, 7),
        (DesignOperandRole::BODIES_B, 0),
    ] {
        let [profile, path] = after(ordinal);
        let groups = [body(102, ordinal, role), profile, path];
        assert!(
            projected(&scope, &groups).is_none(),
            "{role:?} at {ordinal}"
        );
    }
    let [profile_7, path_7] = after(7);
    let trailing_body = [
        profile_7.clone(),
        path_7.clone(),
        body(102, 13, DesignOperandRole::BODIES_B),
    ];
    assert!(projected(&scope, &trailing_body).is_none());
    let two_bodies = [
        body(102, 7, DesignOperandRole::BODIES_B),
        body(103, 8, DesignOperandRole::BODIES_B),
        profile_7.clone(),
        path_7.clone(),
    ];
    assert!(projected(&scope, &two_bodies).is_none());
    set_operation(&mut scope, DesignExtrudeOperation::NewBody);
    let groups = [body(102, 7, DesignOperandRole::BODIES_B), profile_7, path_7];
    assert!(projected(&scope, &groups).is_none());
}

fn guide_surface_input() -> (
    crate::records::feature::scope::DesignParameterScope,
    Vec<crate::records::topology::construction::DesignConstructionOperandGroup>,
    crate::records::topology::entity_selection::DesignEntitySelectionOperand,
) {
    let (mut scope, [mut selected, path]) = sweep_input();
    selected
        .try_set_members(vec![crate::records::identity::Located {
            value: 2788,
            offset: selected.members()[0].offset,
        }])
        .unwrap();
    let mut carrier = profile_group(102, 3);
    carrier.operand_role = DesignConstructionOperandRole::Other(DesignOperandRole::PROFILE);
    carrier
        .try_set_members(vec![crate::records::identity::Located {
            value: 2795,
            offset: carrier.members()[0].offset,
        }])
        .unwrap();
    let mut guide = profile_group(103, 4);
    guide.operand_role = DesignConstructionOperandRole::Other(DesignOperandRole::FACES);
    let profile = crate::records::topology::sketch_profile::DesignSketchProfileOperand::try_new(
        crate::records::topology::sketch_profile::DesignSketchProfileOperandDraft {
            scope_reference_ordinal: 3,
            record_index: 2795,
            byte_offset: 32_000,
            class_tag: "312".to_owned().try_into().unwrap(),
            asset_id: "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d"
                .to_owned()
                .try_into()
                .unwrap(),
            asset_id_offset: 32_040,
            entity_id: "0_2718".to_owned().try_into().unwrap(),
            entity_reference_offset: 32_080,
            compact_tail_flags: None,
            region_selection: None,
            paired_class_tag: "258".to_owned().try_into().unwrap(),
            paired_byte_offset: 32_180,
        },
    )
    .unwrap();
    if let crate::records::feature::scope::DesignScopePayloadMut::Sweep(slot) = scope.payload_mut()
    {
        slot.get_or_insert_with(Default::default).sweep_profile = Some(profile);
    }
    let selection =
        crate::records::topology::entity_selection::DesignEntitySelectionOperand::try_new(
            crate::records::topology::entity_selection::DesignEntitySelectionOperandDraft {
                id: "f3d:Design/BulkStream.dat:entity-selection#2788".into(),
                scope_record_index: scope.record_index,
                group_record_index: selected.record_index,
                group_member_ordinal: 0,
                record_index: 2788,
                byte_offset: 31_000,
                class_tag: "310".to_owned().try_into().unwrap(),
                asset_id: "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d"
                    .to_owned()
                    .try_into()
                    .unwrap(),
                asset_id_offset: 31_040,
                context_id: "1b2c3d4e-5f6a-4b7c-8d9e-0f1a2b3c4d5e"
                    .to_owned()
                    .try_into()
                    .unwrap(),
                context_id_offset: 31_080,
                serialization_form:
                    crate::records::topology::entity_selection::SelectionSerializationForm::Zero,
                identity_record_index: 2791,
                identity_record_offset: 31_180,
                primary_identity: 2718,
                primary_identity_offset: 31209,
                secondary: Some(crate::records::identity::DesignSecondaryIdentity {
                    identity: crate::records::identity::Located {
                        value: 164,
                        offset: 31217,
                    },
                    curve_identity: None,
                }),
                historical_edge_candidates: Vec::new(),
                historical_face_candidates: Vec::new(),
                resolved_edge_slot: None,
                next_record_index: 2792,
                next_byte_offset: 31225,
            },
        )
        .unwrap();
    (scope, vec![selected, carrier, path, guide], selection)
}

#[test]
fn sweep_projects_guide_surface() {
    let (scope, groups, selection) = guide_surface_input();
    assert!(matches!(
        crate::test_support::with_decode_context(|decode_ctx| project_fixed_sweep(
            &scope,
            &groups,
            &[],
            &[],
            std::slice::from_ref(&selection),
            &[],
            decode_ctx
        ))
        .unwrap()
        .unwrap(),
        FeatureDefinition::Operation(FeatureOperation::Sweep {
            orientation: Some(cadmpeg_ir::features::SweepOrientation::GuideSurface {
                faces: cadmpeg_ir::features::FaceSelection::Native(_),
            }),
            ..
        })
    ));
}

#[test]
fn two_lane_sweep_projects_with_along_and_against_extents() {
    let (mut scope, groups) = sweep_input();
    scope
        .try_edit(|draft| {
            draft.payload = DesignScopePayload::Sweep(Some(DesignSweepScope {
                construction: Some(DesignSweepConstruction {
                    operation: DesignExtrudeOperation::NewBody,
                    operation_offset: 128,
                    values: vec![FiniteReal::new(1.0).unwrap(), FiniteReal::new(0.0).unwrap()],
                    record_indexes: vec![97580, 97583],
                    value_offsets: vec![1_152_647, 1_152_971],
                }),
                sweep_profile: None,
            }));
        })
        .unwrap();
    let projected =
        crate::test_support::with_decode_context(|decode_ctx| project(decode_ctx, &scope, &groups))
            .unwrap()
            .unwrap();
    assert!(matches!(
        projected,
        FeatureDefinition::Operation(FeatureOperation::Sweep {
            guide_rail: None,
            twist: None,
            taper: None,
            path_extent: Some(cadmpeg_ir::features::SweepPathExtent {
                along_fraction,
                against_fraction,
            }),
            ..
        }) if along_fraction.get() == 1.0 && against_fraction.get() == 0.0
    ));
}

#[test]
fn sweep_profile_of_resolved_body_faces_projects_historical_faces() {
    use crate::records::topology::face::{DesignFaceOperand, DesignFaceOperandDraft};
    use cadmpeg_ir::features::{PlanarProfileRef, SweepSection, SweepShape};

    let (mut scope, groups) = sweep_input();
    scope
        .try_edit(|draft| {
            draft.history_state_id = Some(8);
            draft.previous_history_state_id = Some(7);
            draft.layout_fixture_tail();
        })
        .unwrap();
    let face = DesignFaceOperand::try_new(DesignFaceOperandDraft {
        id: "f3d:Design/BulkStream.dat:face-operand#200".into(),
        scope_record_index: scope.record_index,
        scope_reference_ordinal: 0,
        group: Some(crate::records::topology::body_recipe::DesignOperandGroup {
            group_record_index: groups[0].record_index,
            group_member_ordinal: 0,
        }),
        record_index: groups[0].members()[0].value,
        byte_offset: 10,
        class_tag: "378".to_owned().try_into().unwrap(),
        paired_byte_offset: 30,
        paired_class_tag: "258".to_owned().try_into().unwrap(),
        recipe_record_index: 203,
        recipe_record_byte_offset: 40,
        recipe_id: "f3d:Design/BulkStream.dat:construction-recipe#203".into(),
        recipe_prefix_offset: 51,
        recipe_prefix_bytes: Vec::new(),
        recipe_references: Vec::new(),
        recipe_kind: crate::records::recipes::ConstructionRecipeKind::BoundedFace,
        recipe_program_offset: 60,
        recipe_program: vec![0, -1],
        recipe_nodes: Vec::new(),
        candidate_faces: Vec::new(),
        unreferenced_candidate_faces: Vec::new(),
        alternate_selector_candidate_faces: Vec::new(),
        preceding_candidate_faces: Vec::new(),
        changed_candidate_faces: Vec::new(),
        historical_support_contexts: Vec::new(),
        resolved_face_slots: vec![42],
        resolved_active_face: None,
        next_record_index: 220,
        next_byte_offset: 70,
    })
    .unwrap();
    let section = |faces: &[DesignFaceOperand]| {
        let projected = crate::test_support::with_decode_context(|decode_ctx| {
            project_fixed_sweep(&scope, &groups, &[], &[], &[], faces, decode_ctx)
        })
        .unwrap();
        let Some(FeatureDefinition::Operation(FeatureOperation::Sweep {
            shape:
                SweepShape::Solid {
                    section: SweepSection::Profile(profile),
                    ..
                },
            ..
        })) = projected
        else {
            panic!("a solid Sweep");
        };
        profile
    };
    assert!(matches!(
        section(std::slice::from_ref(&face)),
        PlanarProfileRef::HistoricalFaces { faces, .. } if faces.len() == 1
    ));
    assert!(matches!(section(&[]), PlanarProfileRef::Native(_)));
}
