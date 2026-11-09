// SPDX-License-Identifier: Apache-2.0

use std::collections::HashSet;

fn native(trailing_only: bool) -> crate::native::F3dNative {
    use crate::records::{
        identity::Located,
        topology::construction::{
            DesignConstructionOperandGroupFrame, DesignConstructionOperandGroupFrameDraft,
        },
    };
    let mut native = super::construction_group::native(false, false);
    if trailing_only {
        let mut member = native.design_construction_operand_groups[0].clone();
        member.id = "f3d:Design/BulkStream.dat:design-construction-operand-group#101".into();
        member.record_index = 101;
        member.scope_reference_ordinal = 1;
        native.design_construction_operand_groups.push(member);
        native.design_construction_operand_groups[0].frame =
            DesignConstructionOperandGroupFrame::try_from(
                DesignConstructionOperandGroupFrameDraft {
                    member_count_offset: 1_021,
                    auxiliary_records: Vec::new(),
                    auxiliary_paths: Vec::new(),
                    trailing_records: vec![Located {
                        value: 102,
                        offset: 1_038,
                    }],
                    trailing_transforms: Vec::new(),
                    trailing_dual_transforms: Vec::new(),
                    trailing_flags: Vec::new(),
                    opaque_index: 1,
                    opaque_index_offset: 1_058,
                    opaque_scalar: 0.0,
                    opaque_scalar_offset: 1_062,
                    variant: false,
                    closing_flag: false,
                },
            )
            .unwrap();
    }
    native
}

pub(super) fn native_with_identity() -> crate::native::F3dNative {
    use crate::records::{
        mesh::DesignRelaxedGuidText,
        references::DesignClassTag,
        topology::edge_identity::{
            DesignEdgeIdentityLayout, DesignEdgeIdentityOperand, DesignEdgeIdentityOperandDraft,
        },
    };
    let mut native = native(false);
    native.design_edge_identity_operands.push(
        DesignEdgeIdentityOperand::try_new(DesignEdgeIdentityOperandDraft {
            id: "f3d:Design/BulkStream.dat:edge-identity#101".into(),
            scope_record_index: 10,
            group_record_index: 100,
            group_member_ordinal: 0,
            record_index: 101,
            byte_offset: 100,
            class_tag: DesignClassTag::try_from("297".to_owned()).unwrap(),
            layout: DesignEdgeIdentityLayout::Full,
            local_id: 13,
            asset_id: DesignRelaxedGuidText::try_from(
                "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d".to_owned(),
            )
            .unwrap(),
            asset_id_offset: 142,
            context_id: DesignRelaxedGuidText::try_from(
                "1b2c3d4e-5f6a-4b7c-8d9e-0f1a2b3c4d5e".to_owned(),
            )
            .unwrap(),
            context_id_offset: 218,
            historical: None,
            treatment_radius_candidates: Vec::new(),
            transition_edge_candidates: Vec::new(),
            resolved_edge_slots: Vec::new(),
            resolved_edge_slot: None,
            resolution_identity_id: None,
        })
        .unwrap(),
    );
    native
}

#[test]
fn operand_group_missing_member_preserves_finding_text() {
    crate::test_support::with_decode_context(|service_ctx| {
        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let native = native(false);
        let ctx = super::super::Ctx::new(&ir, &native, service_ctx);
        let mut findings = Vec::new();
        super::super::validate_operand_group_carriers(
            &ctx,
            &mut findings,
            &HashSet::new(),
            &HashSet::new(),
            &HashSet::new(),
            &HashSet::new(),
            &HashSet::new(),
        );
        assert_eq!(findings.len(), 1);
        assert_eq!(
            findings[0].message,
            "Fusion Design construction operand group has no exact typed member"
        );
    })
}

#[test]
fn operand_group_exact_identity_member_has_no_finding() {
    crate::test_support::with_decode_context(|service_ctx| {
        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let native = native_with_identity();
        let ctx = super::super::Ctx::new(&ir, &native, service_ctx);
        let mut findings = Vec::new();
        let exact_members = HashSet::from([("f3d:Design/BulkStream.dat", 101)]);
        super::super::validate_operand_group_carriers(
            &ctx,
            &mut findings,
            &HashSet::new(),
            &exact_members,
            &HashSet::new(),
            &HashSet::new(),
            &HashSet::new(),
        );
        assert!(findings.is_empty());
    })
}

#[test]
fn a_pattern_face_group_needs_a_typed_member_or_an_identity() {
    use crate::records::topology::{
        construction::DesignConstructionOperandRole, extrude_selection::DesignOperandRole,
    };
    crate::test_support::with_decode_context(|service_ctx| {
        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let mut native = native(false);
        native.design_construction_operand_groups[0].operand_role =
            DesignConstructionOperandRole::Other(DesignOperandRole::ROLE_0X10);
        let ctx = super::super::Ctx::new(&ir, &native, service_ctx);
        let findings_with = |identity: &HashSet<(&str, u32)>| {
            let mut findings = Vec::new();
            super::super::validate_operand_group_carriers(
                &ctx,
                &mut findings,
                identity,
                &HashSet::new(),
                &HashSet::new(),
                &HashSet::new(),
                &HashSet::new(),
            );
            findings
        };
        let refused = findings_with(&HashSet::new());
        assert_eq!(refused.len(), 1);
        assert_eq!(
            refused[0].message,
            "Fusion Design construction operand group has no exact typed member"
        );
        assert!(findings_with(&HashSet::from([("f3d:Design/BulkStream.dat", 100)])).is_empty());
    })
}

fn native_profile(
    record_index: u32,
) -> crate::records::topology::sketch_profile::DesignSketchProfileOperand {
    crate::records::topology::sketch_profile::DesignSketchProfileOperand::try_new(
        crate::records::topology::sketch_profile::DesignSketchProfileOperandDraft {
            scope_reference_ordinal: 0,
            record_index,
            byte_offset: 1_900 + u64::from(record_index),
            class_tag: "300".to_owned().try_into().unwrap(),
            asset_id: "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d"
                .to_owned()
                .try_into()
                .unwrap(),
            asset_id_offset: 2_030,
            entity_id: "0_10".to_owned().try_into().unwrap(),
            entity_reference_offset: 2_050,
            compact_tail_flags: None,
            region_selection: None,
            paired_class_tag: "260".to_owned().try_into().unwrap(),
            paired_byte_offset: 2_100,
        },
    )
    .unwrap()
}

/// A profile group whose members are the scope's several native profile
/// operands, in the group's order.
fn native_with_native_profiles(members: [u32; 2]) -> crate::native::F3dNative {
    use crate::records::{
        feature::scope::{DesignExtrudeScope, DesignScopePayload},
        identity::Located,
        topology::construction::DesignConstructionOperandRole,
    };
    let mut native = super::construction_group::native(true, false);
    native.design_parameter_scopes[0]
        .try_edit(|draft| {
            draft.payload = DesignScopePayload::Extrude(Some(DesignExtrudeScope {
                extrude_native_profiles: Some(vec![native_profile(101), native_profile(102)]),
                ..DesignExtrudeScope::default()
            }));
        })
        .unwrap();
    let group = &mut native.design_construction_operand_groups[0];
    group.operand_role = DesignConstructionOperandRole::ExtrudeProfile;
    group
        .try_set_members(
            members
                .into_iter()
                .zip([1_026, 1_037])
                .map(|(value, offset)| Located { value, offset })
                .collect(),
        )
        .unwrap();
    native
}

fn carrier_findings(native: &crate::native::F3dNative) -> Vec<cadmpeg_ir::report::check::Finding> {
    crate::test_support::with_decode_context(|service_ctx| {
        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let ctx = super::super::Ctx::new(&ir, native, service_ctx);
        let mut findings = Vec::new();
        super::super::validate_operand_group_carriers(
            &ctx,
            &mut findings,
            &HashSet::new(),
            &HashSet::new(),
            &HashSet::new(),
            &HashSet::new(),
            &HashSet::new(),
        );
        findings
    })
}

#[test]
fn profile_group_of_native_profiles_has_an_exact_member_carrier() {
    let findings = carrier_findings(&native_with_native_profiles([101, 102]));
    assert!(findings.is_empty(), "{findings:?}");
}

#[test]
fn profile_group_must_list_the_native_profiles_in_order() {
    for members in [[102, 101], [101, 101]] {
        let findings = carrier_findings(&native_with_native_profiles(members));
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert_eq!(
            findings[0].message,
            "Fusion Design construction operand group has no exact typed member"
        );
    }
}
