// SPDX-License-Identifier: Apache-2.0
//! Rigid alignment scalar and member validation.

use crate::validate::{rigid_alignment_scalars_hold, rigid_group_members_link};

/// An owner-less rigid alignment holds only a zero angle and offset.
#[test]
fn rigid_alignment_without_owners_requires_zero_scalars() {
    use crate::records::feature::assembly::DesignAssemblyAlignment;
    let zero = DesignAssemblyAlignment::try_new(0.0, [0.0; 3], Vec::new(), None).unwrap();
    let turned = DesignAssemblyAlignment::try_new(0.5, [0.0; 3], Vec::new(), None).unwrap();
    let moved = DesignAssemblyAlignment::try_new(0.0, [0.0, 1.0, 0.0], Vec::new(), None).unwrap();
    assert!(rigid_alignment_scalars_hold(&zero, true));
    assert!(!rigid_alignment_scalars_hold(&turned, true));
    assert!(!rigid_alignment_scalars_hold(&moved, true));
    assert!(rigid_alignment_scalars_hold(&turned, false));
}

/// A rigid group's frame fixes its member count, so a group whose operands
/// were read needs the members after its first two. A group whose operands
/// were not read has no members to keep.
#[test]
fn rigid_group_frame_requires_its_further_members() {
    use crate::records::feature::assembly::{
        DesignAssemblyAlignment, DesignAssemblyAlignmentForm, DesignAssemblyOperandFrame,
    };
    use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
    let scope = |frame_length: u64, tail_length: u64| {
        let mut scope = DesignParameterScope::empty(
            "f3d:Design/BulkStream.dat:design-parameter-scope#3",
            DesignFeatureKind::AsBuilt,
            3,
        );
        scope
            .try_edit(|draft| {
                crate::design::test_support::lengthen_scope_frame(draft, frame_length);
                draft.reference_count_offset = draft.byte_offset + frame_length - tail_length - 28;
                draft.reference_members =
                    crate::records::identity::ReferenceRun::unlocated(Vec::new());
                draft.layout_fixture_references();
                draft.feature_ordinal_offset = draft.paired_byte_offset - tail_length;
            })
            .unwrap();
        scope
    };
    let frames = [10, 20].map(|index| DesignAssemblyOperandFrame {
        reference_record_index: index,
        reference_offset: 0,
        transform: cadmpeg_ir::transform::Transform::identity()
            .rows()
            .try_into()
            .unwrap(),
        transform_offset: 0,
    });
    let read = DesignAssemblyAlignment::try_new(
        0.0,
        [0.0; 3],
        Vec::new(),
        Some(DesignAssemblyAlignmentForm::Frames { frames }),
    )
    .unwrap();
    let unread = DesignAssemblyAlignment::try_new(0.0, [0.0; 3], Vec::new(), None).unwrap();
    for (frame, tail) in [
        (257, 76),
        (258, 77),
        (268, 87),
        (269, 88),
        (261, 76),
        (262, 77),
        (272, 87),
        (273, 88),
    ] {
        assert!(rigid_group_members_link(
            &scope(frame, tail),
            &read,
            true,
            &Default::default()
        ));
        assert!(!rigid_group_members_link(
            &scope(frame + 11, tail),
            &read,
            true,
            &Default::default()
        ));
        assert!(rigid_group_members_link(
            &scope(frame + 11, tail),
            &unread,
            true,
            &Default::default()
        ));
        assert!(rigid_group_members_link(
            &scope(frame + 11, tail),
            &read,
            false,
            &Default::default()
        ));
    }
}

#[test]
fn rigid_group_locator_version_mismatch_reports_a_scope_finding() {
    use crate::records::entity_header::{BaseTypeGuid, SegmentType, SegmentTypeData};
    use crate::records::feature::assembly::{
        DesignAssemblyAlignment, DesignAssemblyAlignmentForm, DesignAssemblyOperandFrame,
        DesignAssemblyOperandPath, DesignAssemblyOperandPathLink, DesignAssemblyOperandQualifier,
        DesignQualifiedAssemblyOperand, TYPED_OCCURRENCE_PATH_TYPE_GUID,
    };
    use crate::records::feature::scope::{
        DesignFeatureKind, DesignParameterScope, DesignScopePayload,
    };
    use crate::records::identity::{Located, ReferenceRun};

    let cases = [
        (309, 2, 201, 252, 73, true),
        (181, 2, 73, 124, 72, true),
        (259, 2, 151, 202, 23, false),
        (131, 2, 23, 74, 22, false),
        (180, 3, 152, 163, 23, false),
        (52, 3, 24, 35, 22, false),
        (190, 3, 162, 173, 33, true),
        (62, 3, 34, 45, 32, true),
        (225, 5, 197, 208, 68, true),
        (97, 5, 69, 80, 67, true),
    ];
    for (segment, (length, version, backlink, wrapper, transform, construction)) in
        ["f3d:Design", "f3d:xref/member/occurrence-0/Design"]
            .into_iter()
            .flat_map(|segment| cases.into_iter().map(move |case| (segment, case)))
    {
        let mut scope = DesignParameterScope::empty(
            &format!("{segment}/BulkStream.dat:design-parameter-scope#3"),
            DesignFeatureKind::AsBuilt,
            3,
        );
        let operands = [64, 67].map(|index| {
            let locator = 1000 * u64::from(index);
            let path_at = locator + length;
            let wrapper_at = path_at + 425;
            let path = DesignAssemblyOperandPath::try_new_typed(
                DesignAssemblyOperandPathLink {
                    locator_reference_offset: if index == 64 { 42 } else { 53 },
                    locator_record_index: index,
                    locator_class_tag: "283".to_owned().try_into().unwrap(),
                    locator_byte_offset: locator,
                    locator_scope_reference_offset: locator + backlink + 1,
                    wrapper_record_index: index + 2,
                    wrapper_reference_offset: locator + wrapper + 1,
                    wrapper_class_tag: "327".to_owned().try_into().unwrap(),
                    wrapper_byte_offset: wrapper_at,
                    path_reference_offset: wrapper_at + 27,
                },
                index + 1,
                "366".to_owned().try_into().unwrap(),
                Some(
                    TYPED_OCCURRENCE_PATH_TYPE_GUID
                        .to_owned()
                        .try_into()
                        .unwrap(),
                ),
                path_at,
                vec![Located {
                    value: "11111111-1111-1111-1111-111111111111"
                        .to_owned()
                        .try_into()
                        .unwrap(),
                    offset: path_at + 29,
                }],
                [105, 181, 265, 341]
                    .map(|offset| Located {
                        value: "22222222-2222-2222-2222-222222222222"
                            .to_owned()
                            .try_into()
                            .unwrap(),
                        offset: path_at + offset,
                    })
                    .to_vec(),
            )
            .unwrap();
            DesignQualifiedAssemblyOperand {
                frame: DesignAssemblyOperandFrame {
                    reference_record_index: if construction { index + 900 } else { index },
                    reference_offset: locator
                        + if construction {
                            if version == 5 {
                                57
                            } else {
                                22
                            }
                        } else {
                            7
                        },
                    transform: cadmpeg_ir::transform::Transform::identity()
                        .rows()
                        .try_into()
                        .unwrap(),
                    transform_offset: locator + transform,
                },
                qualifier: DesignAssemblyOperandQualifier::OccurrencePath { path },
            }
        });
        let placement_operands = operands.clone();
        scope
            .try_edit(|draft| {
                crate::design::test_support::lengthen_scope_frame(draft, 257);
                draft.reference_count_offset = 153;
                draft.reference_members = ReferenceRun::unlocated(Vec::new());
                draft.layout_fixture_references();
                draft.feature_ordinal_offset = draft.paired_byte_offset - 76;
                draft.payload = DesignScopePayload::AsBuilt(Some(
                    DesignAssemblyAlignment::try_new(
                        0.0,
                        [0.0; 3],
                        Vec::new(),
                        Some(DesignAssemblyAlignmentForm::Qualified(operands)),
                    )
                    .unwrap(),
                ));
            })
            .unwrap();
        let mut native = crate::native::F3dNative {
            design_record_headers: vec![crate::records::decal::DesignRecordHeader {
                id: format!("{segment}/BulkStream.dat:design-record-header#3"),
                record_index: 3,
                class_tag: scope.class_tag.clone(),
                byte_offset: scope.byte_offset(),
            }],
            design_types: vec![SegmentType::try_new(
                format!("{segment}/MetaStream.dat:design-type#0"),
                SegmentTypeData {
                    byte_offset: 0,
                    type_guid: "2E3AC990-483D-4A55-8662-C064B976A957"
                        .to_owned()
                        .try_into()
                        .unwrap(),
                    type_guid_offset: 4,
                    base_type_guid: BaseTypeGuid::Absent,
                    version,
                    version_offset: 44,
                    module: "Fusion".into(),
                    entities: ReferenceRun::unlocated(vec![64, 67]),
                },
            )
            .unwrap()],
            design_parameter_scopes: vec![scope],
            ..Default::default()
        };
        crate::test_support::with_decode_context(|decode| {
            let ir = cadmpeg_ir::examples::unit_cube().unwrap();
            let findings = |native: &crate::native::F3dNative| {
                let ctx = crate::validate::Ctx::new(&ir, native, decode);
                let mut findings = Vec::new();
                crate::validate::validate_parameter_scopes(&ctx, &mut findings).unwrap();
                findings
            };
            assert!(findings(&native).is_empty());
            for wrong_version in [1, 2, 3, 4, 5, 6]
                .into_iter()
                .filter(|other| *other != version)
            {
                native.design_types[0].set_version(wrong_version);
                let rejected = findings(&native);
                assert_eq!(rejected.len(), 1);
                assert_eq!(
                    rejected[0].check,
                    cadmpeg_ir::report::check::Check::NativeLinks
                );
                assert_eq!(
                    rejected[0].entity.as_deref(),
                    Some(native.design_parameter_scopes[0].id.as_str())
                );
                assert_eq!(
                    rejected[0].message,
                    "Fusion Design parameter scope has an invalid paired frame"
                );
            }
        });
        if !matches!(length, 97 | 225) {
            let mut placement = DesignParameterScope::empty(
                &format!("{segment}/BulkStream.dat:design-parameter-scope#4"),
                DesignFeatureKind::AsBuilt,
                4,
            );
            placement
                .try_edit(|draft| {
                    draft.frame_length = 422;
                    draft.paired_byte_offset = draft.byte_offset + 422;
                    draft.reference_members =
                        ReferenceRun::unlocated(vec![80, 50, 51, 52, 53, 81, 82, 83, 84, 54, 55]);
                    draft.layout_fixture_references();
                    draft.layout_fixture_tail();
                })
                .unwrap();
            assert!(crate::design::assembly::placement_ordered_as_built_scope(
                &placement
            ));
            for operand in &placement_operands {
                let DesignAssemblyOperandQualifier::OccurrencePath { path } = &operand.qualifier
                else {
                    panic!("occurrence path operand");
                };
                assert!(
                    !crate::validate::as_built_operand_frame_link(
                        &placement,
                        path,
                        &operand.frame,
                        &std::collections::HashMap::new(),
                    ),
                    "{length}-byte locator under a placement-ordered scope"
                );
            }
        }
    }
}
