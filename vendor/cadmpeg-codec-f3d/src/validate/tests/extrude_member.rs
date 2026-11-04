// SPDX-License-Identifier: Apache-2.0

fn native(valid: bool) -> crate::native::F3dNative {
    use crate::records::{
        mesh::DesignRelaxedGuidText,
        references::DesignClassTag,
        topology::extrude_selection::{
            DesignExtrudeSelectionMember, DesignExtrudeSelectionMemberDraft,
        },
    };
    let mut native = super::extrude_group::native(valid, false);
    native.design_extrude_selection_members.push(
        DesignExtrudeSelectionMember::try_new(DesignExtrudeSelectionMemberDraft {
            id: "f3d:Design/BulkStream.dat:extrude-selection-member#10".into(),
            group_record_index: 9,
            group_member_ordinal: 0,
            record_index: 10,
            byte_offset: 1_000,
            class_tag: DesignClassTag::try_from("277".to_owned()).unwrap(),
            local_id: 42,
            local_id_offset: 1_021,
            asset_id: DesignRelaxedGuidText::try_from(
                "11111111-2222-4333-8444-555555555555".to_owned(),
            )
            .unwrap(),
            asset_id_offset: 1_033,
            context_id: DesignRelaxedGuidText::try_from(
                "ffffffff-eeee-4ddd-8ccc-bbbbbbbbbbbb".to_owned(),
            )
            .unwrap(),
            context_id_offset: 1_105,
            tail_slot_present: false,
            tail_slot_offset: 1_181,
            scope_tail: None,
            resolved_geometry: None,
            operand_identity_ids: Vec::new(),
            historical: None,
            next_record_index: 11,
            next_byte_offset: 1_190,
        })
        .unwrap(),
    );
    native
}

#[test]
fn extrude_member_valid_slot_has_no_finding() {
    crate::test_support::with_decode_context(|service_ctx| {
        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let native = native(true);
        let ctx = super::super::Ctx::new(&ir, &native, service_ctx);
        let mut findings = Vec::new();
        super::super::validate_extrude_selection_members(&ctx, &mut findings);
        assert!(findings.is_empty());
    })
}

fn native_with_scope_tail(scope_record_index: u32) -> crate::native::F3dNative {
    use crate::records::topology::extrude_selection::{
        DesignExtrudeSelectionMember, DesignExtrudeSelectionScopeTail,
    };
    let mut native = native(true);
    let member = native.design_extrude_selection_members.remove(0);
    let mut draft = member.into_draft();
    draft.scope_tail = Some(DesignExtrudeSelectionScopeTail {
        opaque_scalar_bits: 0x3f50_0000_0000_0000,
        opaque_word: 0x104,
        chain_references: [30, 31],
        scope_record_index,
    });
    draft.next_byte_offset = draft.byte_offset + 240;
    native
        .design_extrude_selection_members
        .push(DesignExtrudeSelectionMember::try_new(draft).unwrap());
    native
}

#[test]
fn extrude_member_scope_tail_naming_the_group_scope_has_no_finding() {
    crate::test_support::with_decode_context(|service_ctx| {
        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let native = native_with_scope_tail(7);
        let ctx = super::super::Ctx::new(&ir, &native, service_ctx);
        let mut findings = Vec::new();
        super::super::validate_extrude_selection_members(&ctx, &mut findings);
        assert!(findings.is_empty());
    })
}

#[test]
fn extrude_member_scope_tail_naming_another_scope_is_reported() {
    crate::test_support::with_decode_context(|service_ctx| {
        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let native = native_with_scope_tail(8);
        let ctx = super::super::Ctx::new(&ir, &native, service_ctx);
        let mut findings = Vec::new();
        super::super::validate_extrude_selection_members(&ctx, &mut findings);
        assert_eq!(findings.len(), 1);
    })
}

#[test]
fn extrude_member_scope_tail_needs_the_240_byte_frame() {
    use crate::records::topology::extrude_selection::DesignExtrudeSelectionMember;
    let mut native = native_with_scope_tail(7);
    let mut draft = native
        .design_extrude_selection_members
        .remove(0)
        .into_draft();
    draft.next_byte_offset = draft.byte_offset + 190;
    assert!(DesignExtrudeSelectionMember::try_new(draft).is_err());
}
