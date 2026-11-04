// SPDX-License-Identifier: Apache-2.0

fn group() -> crate::records::topology::extrude_selection::DesignExtrudeSelectionGroup {
    use crate::records::topology::extrude_selection::{
        DesignExtrudeSelectionGroup, DesignExtrudeSelectionGroupWire,
    };
    DesignExtrudeSelectionGroup::try_from(DesignExtrudeSelectionGroupWire {
        id: "f3d:Design/BulkStream.dat:design-extrude-selection-group#9".into(),
        scope_record_index: 7,
        scope_reference_ordinal: 0,
        record_index: 9,
        byte_offset: 0,
        class_tag: "277".into(),
        member_count_offset: 32,
        members: vec![10, 11],
        member_offsets: vec![37, 48],
        opaque_index: 1,
        opaque_index_offset: 58,
        opaque_scalar: 0.0,
        opaque_scalar_offset: 62,
        variant: Some(false),
        typed_references: false,
        paired_class_tag: "259".into(),
        paired_byte_offset: 111,
    })
    .unwrap()
}

fn scope() -> crate::records::feature::scope::DesignParameterScope {
    use crate::records::feature::scope::{
        DesignParameterScope, DesignParameterScopeDraft, DesignScopePayload,
    };
    DesignParameterScope::try_new(
        DesignParameterScopeDraft {
            id: "f3d:Design/BulkStream.dat:design-parameter-scope#7".into(),
            byte_offset: 100,
            class_tag: "301".to_owned().try_into().unwrap(),
            record_index: 7,
            frame_length: 200,
            kind_offset: 0,
            payload: DesignScopePayload::Extrude(None),
            feature_ordinal: std::num::NonZeroU32::MIN,
            feature_ordinal_offset: 0,
            history_state_id: None,
            previous_history_state_id: None,
            previous_history_state_id_offset: None,
            reference_count_offset: 150,
            reference_members: crate::records::identity::ReferenceRun::unlocated(vec![9]),
            unclosed_construction_operand_groups: Vec::new(),
            paired_class_tag: "261".to_owned().try_into().unwrap(),
            paired_byte_offset: 0,
        }
        .with_fixture_layout(),
    )
    .unwrap()
}

pub(super) fn native(valid: bool, duplicate: bool) -> crate::native::F3dNative {
    let group = group();
    let mut native = crate::native::F3dNative::default();
    native.design_extrude_selection_groups.push(group.clone());
    if duplicate {
        native.design_extrude_selection_groups.push(group);
    }
    if valid {
        native.design_parameter_scopes.push(scope());
        for (index, offset, tag) in [(9, 0, "277"), (10, 1000, "277"), (11, 1100, "277")] {
            native
                .design_record_headers
                .push(crate::records::decal::DesignRecordHeader {
                    id: format!("f3d:Design/BulkStream.dat:design-record-header#{index}"),
                    record_index: index,
                    class_tag: tag.to_owned().try_into().unwrap(),
                    byte_offset: offset,
                });
        }
    }
    native
}

#[test]
fn extrude_group_valid_slot_has_no_finding() {
    crate::test_support::with_decode_context(|service_ctx| {
        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let native = native(true, false);
        let ctx = super::super::Ctx::new(&ir, &native, service_ctx);
        let mut findings = Vec::new();
        super::super::validate_extrude_selection_groups(&ctx, &mut findings);
        assert!(findings.is_empty());
    })
}
