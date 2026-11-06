// SPDX-License-Identifier: Apache-2.0

fn selection_operand() -> crate::records::topology::entity_selection::DesignEntitySelectionOperand {
    use crate::records::{
        identity::{DesignSecondaryIdentity, Located},
        mesh::DesignRelaxedGuidText,
        topology::entity_selection::{
            DesignEntitySelectionOperand, DesignEntitySelectionOperandDraft,
        },
    };
    DesignEntitySelectionOperand::try_new(DesignEntitySelectionOperandDraft {
        id: "f3d:Design/BulkStream.dat:design-entity-selection-operand#200".into(),
        scope_record_index: 10,
        group_record_index: 100,
        group_member_ordinal: 0,
        record_index: 200,
        byte_offset: 1_000,
        class_tag: "338".to_owned().try_into().unwrap(),
        asset_id: DesignRelaxedGuidText::try_from(
            "11111111-2222-4333-8444-555555555555".to_owned(),
        )
        .unwrap(),
        asset_id_offset: 1_034,
        context_id: DesignRelaxedGuidText::try_from(
            "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee".to_owned(),
        )
        .unwrap(),
        context_id_offset: 1_100,
        identity_record_index: 203,
        identity_record_offset: 2_000,
        primary_identity: 949,
        primary_identity_offset: 2033,
        secondary: Some(DesignSecondaryIdentity {
            identity: Located {
                value: 249,
                offset: 2041,
            },
            curve_identity: None,
        }),
        historical_edge_candidates: Vec::new(),
        historical_face_candidates: Vec::new(),
        resolved_edge_slot: None,
        next_record_index: 204,
        next_byte_offset: 2049,
    })
    .unwrap()
}

fn operand_native(valid: bool) -> crate::native::F3dNative {
    use crate::records::{
        decal::DesignRecordHeader,
        identity::Located,
        topology::construction::{
            DesignConstructionOperandGroup, DesignConstructionOperandGroupDraft,
            DesignConstructionOperandGroupFrame, DesignConstructionOperandGroupFrameDraft,
            DesignConstructionOperandRole,
        },
    };
    let mut native = crate::native::F3dNative::default();
    native
        .design_entity_selection_operands
        .push(selection_operand());
    if valid {
        native.design_construction_operand_groups.push(
            DesignConstructionOperandGroup::try_from(DesignConstructionOperandGroupDraft {
                id: "f3d:Design/BulkStream.dat:design-construction-operand-group#100".into(),
                scope_record_index: 10,
                scope_reference_ordinal: 0,
                record_index: 100,
                byte_offset: 900,
                class_tag: "277".to_owned().try_into().unwrap(),
                members: vec![Located {
                    value: 200,
                    offset: 926,
                }],
                lost_edge_references: Vec::new(),
                frame: DesignConstructionOperandGroupFrame::try_from(
                    DesignConstructionOperandGroupFrameDraft {
                        member_count_offset: 921,
                        auxiliary_records: Vec::new(),
                        auxiliary_paths: Vec::new(),
                        trailing_records: Vec::new(),
                        trailing_transforms: Vec::new(),
                        trailing_dual_transforms: Vec::new(),
                        trailing_flags: Vec::new(),
                        opaque_index: 1,
                        opaque_index_offset: 958,
                        opaque_scalar: 0.0,
                        opaque_scalar_offset: 962,
                        variant: false,
                        closing_flag: false,
                    },
                )
                .unwrap(),
                operand_role: DesignConstructionOperandRole::Other(
                    crate::records::topology::extrude_selection::DesignOperandRole::BODIES_A,
                ),
                role_offset: 940,
                paired_class_tag: "258".to_owned().try_into().unwrap(),
                paired_byte_offset: 980,
            })
            .unwrap(),
        );
        native.design_record_headers.push(DesignRecordHeader {
            id: "f3d:Design/BulkStream.dat:design-record-header#200".into(),
            record_index: 200,
            class_tag: "338".to_owned().try_into().unwrap(),
            byte_offset: 1_000,
        });
    }
    native
}

#[test]
fn entity_selection_valid_slot_has_no_finding() {
    crate::test_support::with_decode_context(|service_ctx| {
        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let native = operand_native(true);
        let ctx = super::super::Ctx::new(&ir, &native, service_ctx);
        let mut findings = Vec::new();
        super::super::validate_entity_selection_operands(&ctx, &mut findings);
        assert!(findings.is_empty());
    })
}
