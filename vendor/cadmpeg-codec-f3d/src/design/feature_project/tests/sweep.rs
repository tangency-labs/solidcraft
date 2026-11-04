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
