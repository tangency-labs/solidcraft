// SPDX-License-Identifier: Apache-2.0

fn native(valid: bool) -> crate::native::F3dNative {
    use crate::records::{
        decal::DesignRecordHeader,
        identity::Located,
        references::DesignClassTag,
        topology::construction::{
            DesignConstructionOperandGroupFrame, DesignConstructionOperandGroupFrameDraft,
            DesignConstructionOperandIdentity, DesignConstructionOperandIdentityDraft,
            DesignIdentityWrapper,
        },
    };
    let mut native = super::construction_group::native(valid, false);
    native.design_construction_operand_groups[0].frame =
        DesignConstructionOperandGroupFrame::try_from(DesignConstructionOperandGroupFrameDraft {
            member_count_offset: 1_021,
            auxiliary_records: Vec::new(),
            auxiliary_paths: Vec::new(),
            trailing_records: vec![Located {
                value: 101,
                offset: 1_025,
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
        })
        .unwrap();
    if valid {
        native.design_record_headers[1].class_tag =
            DesignClassTag::try_from("384".to_owned()).unwrap();
        native.design_record_headers.push(DesignRecordHeader {
            id: "f3d:Design/BulkStream.dat:design-record-header#102".into(),
            record_index: 102,
            class_tag: DesignClassTag::try_from("395".to_owned()).unwrap(),
            byte_offset: 1_124,
        });
    }
    native.design_construction_operand_identities.push(
        DesignConstructionOperandIdentity::try_new(DesignConstructionOperandIdentityDraft {
            id: "f3d:Design/BulkStream.dat:operand-identity#101".into(),
            group_record_index: 100,
            wrappers: vec![DesignIdentityWrapper {
                record_index: 101,
                byte_offset: 1_100,
                class_tag: DesignClassTag::try_from("384".to_owned()).unwrap(),
            }],
            following_record_index: 102,
            following_byte_offset: 1_124,
            following_class_tag: DesignClassTag::try_from("395".to_owned()).unwrap(),
            tracking_path: None,
            persistent_identity: None,
        })
        .unwrap(),
    );
    native
}

#[test]
fn construction_identity_valid_group_has_no_finding() {
    crate::test_support::with_decode_context(|service_ctx| {
        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let native = native(true);
        let ctx = super::super::Ctx::new(&ir, &native, service_ctx);
        let mut findings = Vec::new();
        let groups = super::super::validate_construction_operand_identities(&ctx, &mut findings);
        assert!(findings.is_empty());
        assert!(groups.contains(&("f3d:Design/BulkStream.dat", 100)));
    })
}

fn transform_native() -> crate::native::F3dNative {
    use crate::records::{
        decal::DesignRecordHeader,
        sketch_placement::SketchPlacementMatrix,
        topology::construction::{
            DesignConstructionOperandIdentity, DesignConstructionOperandTransform,
            DesignConstructionOperandTransformDraft,
        },
    };
    let mut native = native(true);
    let mut identity = native
        .design_construction_operand_identities
        .remove(0)
        .into_draft();
    identity.wrappers[0].record_index = 102;
    identity.wrappers[0].byte_offset = 1_252;
    identity.following_record_index = 103;
    identity.following_byte_offset = 1_276;
    native
        .design_construction_operand_identities
        .push(DesignConstructionOperandIdentity::try_new(identity).unwrap());
    native.design_construction_operand_groups[0]
        .frame
        .try_set_trailing_transforms(vec![DesignConstructionOperandTransform::try_new(
            DesignConstructionOperandTransformDraft {
                record_index: 101,
                byte_offset: 1_100,
                class_tag: "280".to_owned().try_into().unwrap(),
                transform: SketchPlacementMatrix::IDENTITY,
                transform_offset: 1_122,
                following_record_index: 102,
                following_byte_offset: 1_252,
                following_class_tag: "384".to_owned().try_into().unwrap(),
            },
        )
        .unwrap()])
        .unwrap();
    native
        .design_record_headers
        .retain(|header| header.record_index == 100);
    for (record_index, byte_offset, class_tag) in [
        (101, 1_100, "280"),
        (102, 1_252, "384"),
        (103, 1_276, "395"),
    ] {
        native.design_record_headers.push(DesignRecordHeader {
            id: format!("f3d:Design/BulkStream.dat:design-record-header#{record_index}"),
            record_index,
            byte_offset,
            class_tag: class_tag.to_owned().try_into().unwrap(),
        });
    }
    native
}

fn transform_findings(native: &crate::native::F3dNative, valid: bool) {
    crate::test_support::with_decode_context(|decode| {
        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let ctx = super::super::Ctx::new(&ir, native, decode);
        let mut findings = Vec::new();
        let groups = super::super::validate_construction_operand_identities(&ctx, &mut findings);
        assert_eq!(findings.is_empty(), valid, "{findings:?}");
        assert_eq!(groups.contains(&("f3d:Design/BulkStream.dat", 100)), valid);
        if !valid {
            assert_eq!(findings.len(), 1);
            assert_eq!(
                findings[0].message,
                "Fusion Design construction operand identity has an invalid nested frame"
            );
        }
        findings.clear();
        let empty = std::collections::HashSet::new();
        super::super::validate_operand_group_carriers(
            &ctx,
            &mut findings,
            &groups,
            &empty,
            &empty,
            &empty,
            &empty,
        );
        assert_eq!(findings.is_empty(), valid, "{findings:?}");
        if !valid {
            assert!(findings.iter().any(|finding| finding.message
                == "Fusion Design construction operand group has no exact typed member"));
        }
    });
}

#[test]
fn construction_identity_accepts_transform_wrapper_terminal_chain() {
    transform_findings(&transform_native(), true);
}

#[test]
fn construction_identity_rejects_transform_chain_link_mutations() {
    use crate::records::topology::construction::{
        DesignConstructionOperandTransform, DesignConstructionOperandTransformDraft,
    };
    for (record_index, following_record_index, byte_offset, following_class_tag) in [
        (100, 102, 1_100, "384"),
        (101, 103, 1_100, "384"),
        (101, 102, 1_101, "384"),
        (101, 102, 1_100, "395"),
    ] {
        let mut native = transform_native();
        let frame = &mut native.design_construction_operand_groups[0].frame;
        let mut draft =
            DesignConstructionOperandTransformDraft::from(frame.trailing_transforms()[0].clone());
        draft.record_index = record_index;
        draft.following_record_index = following_record_index;
        draft.byte_offset = byte_offset;
        draft.transform_offset = byte_offset + 22;
        draft.following_byte_offset = byte_offset + 152;
        draft.following_class_tag = following_class_tag.to_owned().try_into().unwrap();
        frame
            .try_set_trailing_transforms(vec![
                DesignConstructionOperandTransform::try_new(draft).unwrap()
            ])
            .unwrap();
        transform_findings(&native, false);
    }
    for record_index in [102, 103] {
        for (offset_delta, class_tag) in [(1, None), (0, Some("277"))] {
            let mut native = transform_native();
            let header = native
                .design_record_headers
                .iter_mut()
                .find(|header| header.record_index == record_index)
                .unwrap();
            header.byte_offset += offset_delta;
            if let Some(class_tag) = class_tag {
                header.class_tag = class_tag.to_owned().try_into().unwrap();
            }
            transform_findings(&native, false);
        }
    }
}
