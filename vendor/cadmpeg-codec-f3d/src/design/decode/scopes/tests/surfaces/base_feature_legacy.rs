// SPDX-License-Identifier: Apache-2.0

use crate::design::decode::scopes::base_feature::exact_base_feature_construction;
use crate::records::feature::base_feature::DesignBaseFeatureConstruction;
use crate::records::feature::scope::DesignParameterScope;
use cadmpeg_core::decode::u64_from_index;
use cadmpeg_test_support::bytes::put_u32;
use cadmpeg_test_support::bytes::put_u64;

#[test]
fn base_feature_scope_decodes_class_452_262_legacy_body_reference_forms() {
    use crate::layout::base_feature_class_452_262_compact as compact;
    use crate::layout::base_feature_class_452_262_expanded as expanded;
    use crate::records::feature::base_feature::DesignBaseFeatureBodyReferenceForm;

    fn put_u64_reference(bytes: &mut [u8], marker: usize, value: u64) {
        bytes[marker] = compact::BODY_ENTITY_REFERENCE_MARKER_VALUE;
        put_u64(bytes, marker + 1, value);
    }

    fn put_u32_reference(bytes: &mut [u8], marker: usize, value: u32) {
        bytes[marker] = expanded::BODY_ENTITY_ONE_MARKER_VALUE;
        put_u32(bytes, marker + 1, value);
    }

    fn put_property(bytes: &mut [u8], marker: usize) {
        bytes[marker] = compact::TAG_BODY_BASED_ON_FACES_MARKER_VALUE;
        put_u32(
            bytes,
            marker + 1,
            crate::layout::base_feature_class_377_prefix::TAG_BODY_BASED_ON_FACES_COUNT_VALUE,
        );
        put_u32(
            bytes,
            marker + 5,
            crate::layout::base_feature_class_377_prefix::TAG_BODY_BASED_ON_FACES_KEY_LENGTH_VALUE,
        );
        bytes[marker + 9..marker + 28].copy_from_slice(b"TagBodyBasedOnFaces");
        put_u32(
            bytes,
            marker + 28,
            crate::layout::base_feature_class_377_prefix::TAG_BODY_BASED_ON_FACES_TYPE_LENGTH_VALUE,
        );
        bytes[marker + 32..marker + 53].copy_from_slice(b"IntrinsicMetaTypebool");
        bytes[marker + 53..marker + 55].copy_from_slice(
            &crate::layout::base_feature_class_377_prefix::TAG_BODY_BASED_ON_FACES_VALUE_VALUE
                .to_le_bytes(),
        );
    }

    fn put_kind_tail(
        bytes: &mut [u8],
        layout: (usize, usize, usize, usize, usize, usize, usize, usize),
        scope_reference: u32,
        current_state: u32,
        previous_state: u32,
        ordinal: u32,
    ) -> DesignParameterScope {
        let (
            reference_count,
            generic_marker,
            generic_record,
            _generic_field,
            history_state_id,
            kind_length,
            kind,
            feature_ordinal,
        ) = layout;
        put_u32(bytes, reference_count, 1);
        put_u32_reference(bytes, generic_marker, scope_reference);
        put_u32(bytes, history_state_id, current_state);
        put_u32(
            bytes,
            kind_length,
            crate::layout::base_feature_class_377_prefix::KIND_LENGTH_VALUE,
        );
        bytes[kind..feature_ordinal].copy_from_slice(
            &crate::bytes::lp_utf16_bytes("Base Feature")
                .expect("fixture UTF-16 code-unit count fits u32")[4..],
        );
        put_u32(bytes, feature_ordinal, ordinal);

        let mut scope = DesignParameterScope::empty(
            "f3d:Design/BulkStream.dat:design-parameter-scope#452",
            crate::records::feature::scope::DesignFeatureKind::BaseFeature,
            452,
        );
        scope
            .try_edit(|draft| {
                draft.byte_offset = 0;
                draft.reference_count_offset = draft.byte_offset + 9;
                draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
                draft.layout_fixture_references();
                draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
                draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
                draft.layout_fixture_tail();
            })
            .unwrap();
        scope.class_tag =
            crate::records::references::DesignClassTag::try_from("452".to_owned()).unwrap();
        scope.paired_class_tag =
            crate::records::references::DesignClassTag::try_from("262".to_owned()).unwrap();
        scope
            .try_edit(|draft| {
                draft.reference_members =
                    crate::records::identity::ReferenceRun::unlocated(vec![scope_reference]);
                draft.reference_count_offset = u64_from_index(reference_count);
                draft.reference_members = crate::records::identity::ReferenceRun::from_columns(
                    draft.reference_members.values().copied().collect(),
                    vec![u64_from_index(generic_record)],
                    "reference_members",
                )
                .unwrap();
                draft.history_state_id = Some(i64::from(current_state));

                draft.previous_history_state_id = Some(i64::from(previous_state));
                draft.frame_length = u64_from_index(bytes.len());
                draft.paired_byte_offset = u64_from_index(bytes.len());
                draft.feature_ordinal_offset = u64_from_index(feature_ordinal);
                draft.previous_history_state_id_offset = Some(
                    u64_from_index(feature_ordinal
                        + crate::design::decode::scopes::parameter_scope::parameter_scope_previous_history_offset(
                            "Base Feature",
                            bytes.len() - feature_ordinal,
                            false,
                            false,
                        )
                        .unwrap()),
                );
                draft.kind_offset = u64_from_index(kind);
            })
            .unwrap();
        scope.feature_ordinal = std::num::NonZeroU32::new(ordinal).expect("nonzero ordinal");
        scope
            .try_edit(|draft| {
                draft.feature_ordinal_offset = u64_from_index(feature_ordinal);
            })
            .unwrap();
        scope
    }

    let compact_frame = |mode: u8| {
        let mut bytes = vec![0_u8; compact::LEN];
        bytes[compact::BODY_COUNT_MARKER] = compact::BODY_COUNT_MARKER_VALUE;
        put_u32(&mut bytes, compact::BODY_COUNT, compact::BODY_COUNT_VALUE);
        put_u64_reference(&mut bytes, compact::BODY_ENTITY_REFERENCE_MARKER, 201);
        bytes[compact::BODY_ENTITY_REFERENCE_FIELD..compact::TAG_BODY_BASED_ON_FACES_MARKER]
            .copy_from_slice(&[0, 0, 1, 0, 0, 0]);
        put_property(&mut bytes, compact::TAG_BODY_BASED_ON_FACES_MARKER);
        bytes[compact::MODE] = mode;
        bytes[compact::PARAMETER_BODY_COUNT] = compact::PARAMETER_BODY_COUNT_VALUE;
        put_u64_reference(&mut bytes, compact::PARAMETER_BODY_REFERENCE_MARKER, 198);
        put_u64_reference(&mut bytes, compact::SCOPE_REFERENCE_MARKER, 196);
        bytes[compact::AUXILIARY_GROUP_MARKER] = 1;
        put_u64_reference(&mut bytes, compact::AUXILIARY_REFERENCE_MARKER, 202);
        bytes[compact::ENVELOPE_GUID_CODE_UNIT_COUNT..compact::ZERO_RUN_AFTER_GUID]
            .copy_from_slice(
                &crate::bytes::lp_utf16_bytes("fcec56e3-832f-4468-88a4-d710e62e629f")
                    .expect("fixture UTF-16 code-unit count fits u32"),
            );
        let _ = put_kind_tail(
            &mut bytes,
            (
                compact::REFERENCE_COUNT,
                compact::GENERIC_SCOPE_REFERENCE_MARKER,
                compact::GENERIC_SCOPE_REFERENCE_RECORD,
                compact::GENERIC_SCOPE_REFERENCE_FIELD,
                compact::HISTORY_STATE_ID,
                compact::KIND_LENGTH,
                compact::KIND,
                compact::FEATURE_ORDINAL,
            ),
            196,
            20,
            19,
            1,
        );
        put_u32(&mut bytes, compact::PREVIOUS_HISTORY_STATE_ID, 19);
        bytes
    };

    let mut compact_bytes = compact_frame(0);
    let mut compact_scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#452",
        crate::records::feature::scope::DesignFeatureKind::BaseFeature,
        452,
    );
    compact_scope.class_tag =
        crate::records::references::DesignClassTag::try_from("452".to_owned()).unwrap();
    compact_scope.paired_class_tag =
        crate::records::references::DesignClassTag::try_from("262".to_owned()).unwrap();
    compact_scope
        .try_edit(|draft| {
            draft.frame_length = u64_from_index(compact::LEN);
            draft.feature_ordinal_offset = u64_from_index(compact::FEATURE_ORDINAL);
            draft.paired_byte_offset = u64_from_index(compact::LEN);
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(vec![196]);
            draft.reference_count_offset = u64_from_index(compact::REFERENCE_COUNT);
            draft.reference_members = crate::records::identity::ReferenceRun::from_columns(
                draft.reference_members.values().copied().collect(),
                vec![u64_from_index(compact::GENERIC_SCOPE_REFERENCE_RECORD)],
                "reference_members",
            )
            .unwrap();
            draft.history_state_id = Some(20);

            draft.previous_history_state_id = Some(19);
            draft.previous_history_state_id_offset =
                Some(u64_from_index(compact::PREVIOUS_HISTORY_STATE_ID));
            draft.kind_offset = u64_from_index(compact::KIND);
        })
        .unwrap();
    compact_scope.feature_ordinal = std::num::NonZeroU32::new(1).expect("nonzero ordinal");
    compact_scope
        .try_edit(|draft| {
            draft.feature_ordinal_offset = u64_from_index(compact::FEATURE_ORDINAL);
        })
        .unwrap();
    let compact_construction = exact_base_feature_construction(&compact_bytes, &compact_scope)
        .unwrap()
        .expect("class-452 compact Base Feature frame is canonical");
    let DesignBaseFeatureConstruction::LegacyBodyBasedOnFaces {
        form,
        scope_reference,
        scope_reference_offset,
        envelope_guid,
        envelope_guid_offset,
        tag_body_based_on_faces_offset,
        ..
    } = &compact_construction
    else {
        panic!("class-452 compact frame selected the wrong form");
    };
    let DesignBaseFeatureBodyReferenceForm::CompactOneBody { mode, body } = form else {
        panic!("compact frame requires one body");
    };
    assert_eq!(
        crate::records::identity::Located {
            value: u8::from(mode.value),
            offset: mode.offset
        },
        crate::records::identity::Located {
            value: 0,
            offset: u64_from_index(compact::MODE)
        }
    );
    let body_entity_suffixes = &[u64::from(body.entity.value)];
    let body_entity_suffix_offsets = &[body.entity.offset];
    let body_entity_fields = &[body.entity.field];
    let body_reference_records = &[body.entity.value];
    let parameter_body_records = &[body.parameter_body.value];
    let parameter_body_record_offsets = &[body.parameter_body.offset];
    let auxiliary_records = &[body.auxiliary.value];
    let auxiliary_record_offsets = &[body.auxiliary.offset];
    assert_eq!(body_entity_suffixes, &[201]);
    assert_eq!(
        body_entity_suffix_offsets,
        &[u64_from_index(compact::BODY_ENTITY_SUFFIX)]
    );
    assert_eq!(body_entity_fields, &[[0, 0, 1, 0, 0, 0]]);
    assert_eq!(body_reference_records, &[201]);
    assert_eq!(parameter_body_records, &[198]);
    assert_eq!(
        parameter_body_record_offsets,
        &[u64_from_index(compact::PARAMETER_BODY_RECORD)]
    );
    assert_eq!(auxiliary_records, &[202]);
    assert_eq!(
        auxiliary_record_offsets,
        &[u64_from_index(compact::AUXILIARY_RECORD)]
    );
    assert_eq!(*scope_reference, 196);
    assert_eq!(
        *scope_reference_offset,
        u64_from_index(compact::SCOPE_REFERENCE)
    );
    assert_eq!(
        envelope_guid.as_str(),
        "fcec56e3-832f-4468-88a4-d710e62e629f"
    );
    assert_eq!(
        *envelope_guid_offset,
        u64_from_index(compact::ENVELOPE_GUID)
    );
    assert_eq!(
        serde_json::to_value(&compact_construction).unwrap()["tag_body_based_on_faces"],
        true
    );
    assert_eq!(
        *tag_body_based_on_faces_offset,
        u64_from_index(compact::TAG_BODY_BASED_ON_FACES_VALUE)
    );
    let serialized = serde_json::to_value(&compact_construction).expect("serialize compact form");
    assert_eq!(
        serde_json::from_value::<DesignBaseFeatureConstruction>(serialized)
            .expect("deserialize compact form"),
        compact_construction
    );

    compact_bytes[compact::MODE] = 1;
    let mode_one = exact_base_feature_construction(&compact_bytes, &compact_scope)
        .unwrap()
        .expect("mode-one class-452 compact frame is canonical");
    let DesignBaseFeatureConstruction::LegacyBodyBasedOnFaces { form, .. } = mode_one else {
        panic!("class-452 compact mode-one frame selected the wrong form");
    };
    let DesignBaseFeatureBodyReferenceForm::CompactOneBody { mode, .. } = form else {
        panic!("compact mode-one form");
    };
    assert_eq!(
        crate::records::identity::Located {
            value: u8::from(mode.value),
            offset: mode.offset
        },
        crate::records::identity::Located {
            value: 1,
            offset: u64_from_index(compact::MODE)
        }
    );

    let mut expanded_bytes = vec![0_u8; expanded::LEN];
    expanded_bytes[expanded::BODY_COUNT_MARKER] = expanded::BODY_COUNT_MARKER_VALUE;
    put_u32(
        &mut expanded_bytes,
        expanded::BODY_COUNT,
        expanded::BODY_COUNT_VALUE,
    );
    put_u64_reference(&mut expanded_bytes, expanded::BODY_ENTITY_ONE_MARKER, 401);
    put_u64_reference(&mut expanded_bytes, expanded::BODY_ENTITY_TWO_MARKER, 402);
    put_property(
        &mut expanded_bytes,
        expanded::TAG_BODY_BASED_ON_FACES_MARKER,
    );
    expanded_bytes[expanded::PARAMETER_BODY_GROUP_MARKER] =
        expanded::PARAMETER_BODY_GROUP_MARKER_VALUE;
    put_u32(
        &mut expanded_bytes,
        expanded::PARAMETER_BODY_COUNT,
        expanded::PARAMETER_BODY_COUNT_VALUE,
    );
    put_u32_reference(
        &mut expanded_bytes,
        expanded::PARAMETER_BODY_ONE_MARKER,
        301,
    );
    put_u32_reference(
        &mut expanded_bytes,
        expanded::PARAMETER_BODY_TWO_MARKER,
        302,
    );
    put_u32_reference(&mut expanded_bytes, expanded::SCOPE_REFERENCE_MARKER, 300);
    put_u32(
        &mut expanded_bytes,
        expanded::AUXILIARY_BODY_COUNT,
        expanded::AUXILIARY_BODY_COUNT_VALUE,
    );
    put_u32_reference(
        &mut expanded_bytes,
        expanded::AUXILIARY_BODY_ONE_MARKER,
        303,
    );
    put_u32_reference(
        &mut expanded_bytes,
        expanded::AUXILIARY_BODY_TWO_MARKER,
        304,
    );
    expanded_bytes[expanded::ENVELOPE_GUID_CODE_UNIT_COUNT..expanded::ZERO_RUN_AFTER_GUID]
        .copy_from_slice(
            &crate::bytes::lp_utf16_bytes("00000000-0000-0000-0000-000000000000")
                .expect("fixture UTF-16 code-unit count fits u32"),
        );
    let mut expanded_scope = put_kind_tail(
        &mut expanded_bytes,
        (
            expanded::REFERENCE_COUNT,
            expanded::GENERIC_SCOPE_REFERENCE_MARKER,
            expanded::GENERIC_SCOPE_REFERENCE_RECORD,
            expanded::GENERIC_SCOPE_REFERENCE_FIELD,
            expanded::HISTORY_STATE_ID,
            expanded::KIND_LENGTH,
            expanded::KIND,
            expanded::FEATURE_ORDINAL,
        ),
        300,
        55,
        54,
        2,
    );
    put_u32(&mut expanded_bytes, expanded::PREVIOUS_HISTORY_STATE_ID, 54);
    expanded_scope
        .try_edit(|draft| {
            draft.frame_length = u64_from_index(expanded::LEN);
            draft.paired_byte_offset = u64_from_index(expanded::LEN);
            draft.reference_count_offset = u64_from_index(expanded::REFERENCE_COUNT);
            draft.reference_members = crate::records::identity::ReferenceRun::from_columns(
                draft.reference_members.values().copied().collect(),
                vec![u64_from_index(expanded::GENERIC_SCOPE_REFERENCE_RECORD)],
                "reference_members",
            )
            .unwrap();

            draft.previous_history_state_id_offset =
                Some(u64_from_index(expanded::PREVIOUS_HISTORY_STATE_ID));
            draft.kind_offset = u64_from_index(expanded::KIND);
            draft.feature_ordinal_offset = u64_from_index(expanded::FEATURE_ORDINAL);
        })
        .unwrap();
    let expanded_construction = exact_base_feature_construction(&expanded_bytes, &expanded_scope)
        .unwrap()
        .expect("class-452 expanded Base Feature frame is canonical");
    let DesignBaseFeatureConstruction::LegacyBodyBasedOnFaces {
        form,
        scope_reference,
        ..
    } = &expanded_construction
    else {
        panic!("class-452 expanded frame selected the wrong form");
    };
    let DesignBaseFeatureBodyReferenceForm::ExpandedTwoBody { bodies } = form else {
        panic!("expanded frame requires two bodies");
    };
    let body_entity_suffixes = &bodies.map(|body| u64::from(body.entity.value));
    let body_entity_fields = &bodies.map(|body| body.entity.field);
    let parameter_body_records = &bodies.map(|body| body.parameter_body.value);
    let auxiliary_records = &bodies.map(|body| body.auxiliary.value);
    assert_eq!(body_entity_suffixes, &[401, 402]);
    assert_eq!(body_entity_fields, &[[0; 6], [0; 6]]);
    assert_eq!(parameter_body_records, &[301, 302]);
    assert_eq!(auxiliary_records, &[303, 304]);
    assert_eq!(*scope_reference, 300);
    let serialized = serde_json::to_value(&expanded_construction).expect("serialize expanded form");
    assert_eq!(
        serde_json::from_value::<DesignBaseFeatureConstruction>(serialized)
            .expect("deserialize expanded form"),
        expanded_construction
    );

    let mut invalid_mode = compact_bytes;
    invalid_mode[compact::MODE] = 2;
    assert!(
        exact_base_feature_construction(&invalid_mode, &compact_scope)
            .unwrap()
            .is_none()
    );

    let mut invalid_count = expanded_bytes.clone();
    put_u32(&mut invalid_count, expanded::BODY_COUNT, 1);
    assert!(
        exact_base_feature_construction(&invalid_count, &expanded_scope)
            .unwrap()
            .is_none()
    );

    let mut invalid_scope_reference = expanded_bytes;
    put_u32_reference(
        &mut invalid_scope_reference,
        expanded::SCOPE_REFERENCE_MARKER,
        999,
    );
    assert!(
        exact_base_feature_construction(&invalid_scope_reference, &expanded_scope)
            .unwrap()
            .is_none()
    );
}

#[test]
fn base_feature_scope_decodes_wide_shifted_zero_body_frame() {
    use crate::layout::base_feature_wide_shifted_zero_body as shifted;

    let kind = "Base Feature";
    let kind_units = kind.encode_utf16().collect::<Vec<_>>();
    let kind_offset = shifted::KIND_LENGTH + 4;
    let ordinal_offset = kind_offset + kind_units.len() * 2;
    let frame_length = ordinal_offset + 4;
    let mut bytes = vec![0u8; frame_length];
    bytes[shifted::ZERO_BODY_MARKER] = 1;
    bytes[shifted::SHARED_METADATA_MARKER] = 1;
    bytes[shifted::SHARED_METADATA_RECORD..shifted::SHARED_METADATA_RECORD + 8]
        .copy_from_slice(&701u64.to_le_bytes());
    bytes[shifted::GUID_CODE_UNIT_COUNT..shifted::GUID_CODE_UNIT_COUNT + 4]
        .copy_from_slice(&36u32.to_le_bytes());
    bytes[shifted::REFERENCE_COUNT..shifted::REFERENCE_COUNT + 4]
        .copy_from_slice(&shifted::REFERENCE_COUNT_VALUE.to_le_bytes());
    bytes[shifted::SCOPE_REFERENCE_MARKER] = shifted::SCOPE_REFERENCE_MARKER_VALUE;
    bytes[shifted::SCOPE_REFERENCE_RECORD..shifted::SCOPE_REFERENCE_RECORD + 4]
        .copy_from_slice(&701u32.to_le_bytes());
    bytes[shifted::HISTORY_STATE..shifted::HISTORY_STATE + 4].copy_from_slice(&17u32.to_le_bytes());
    bytes[shifted::KIND_LENGTH..shifted::KIND_LENGTH + 4].copy_from_slice(
        &u32::try_from(kind_units.len())
            .expect("fixture kind fits u32")
            .to_le_bytes(),
    );
    for (ordinal, unit) in kind_units.iter().enumerate() {
        bytes[kind_offset + ordinal * 2..kind_offset + ordinal * 2 + 2]
            .copy_from_slice(&unit.to_le_bytes());
    }
    bytes[ordinal_offset..ordinal_offset + 4].copy_from_slice(&1u32.to_le_bytes());

    let mut scope = DesignParameterScope::empty(
        "f3d:scope#base-feature-wide-shifted-zero",
        crate::records::feature::scope::DesignFeatureKind::BaseFeature,
        74,
    );
    scope
        .try_edit(|draft| {
            draft.frame_length = u64_from_index(frame_length);
            draft.paired_byte_offset = u64_from_index(frame_length);
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(vec![701]);
            draft.reference_count_offset = 9;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
            draft.history_state_id = Some(17);
        })
        .unwrap();
    scope.feature_ordinal = std::num::NonZeroU32::new(1).expect("nonzero ordinal");

    let construction = exact_base_feature_construction(&bytes, &scope)
        .unwrap()
        .expect("wide shifted zero-body frame is canonical");
    let DesignBaseFeatureConstruction::ResultBodies {
        bodies:
            crate::records::feature::base_feature::DesignBaseFeatureResults::WithoutRepeatedFields(
                bodies,
            ),
        metadata_record,
        metadata_record_offset,
        metadata_field,
        ..
    } = construction
    else {
        panic!("wide shifted zero-body frame selected the wrong form");
    };
    assert!(bodies.is_empty());
    assert_eq!(metadata_record, 701);
    assert_eq!(
        metadata_record_offset,
        u64_from_index(shifted::SHARED_METADATA_RECORD)
    );
    assert_eq!(metadata_field, [0; 6]);

    let mut nonzero_padding = bytes.clone();
    nonzero_padding[shifted::ZERO_PADDING_12] = 1;
    assert!(exact_base_feature_construction(&nonzero_padding, &scope)
        .unwrap()
        .is_none());

    let mut mismatched_member = bytes.clone();
    mismatched_member[shifted::SCOPE_REFERENCE_RECORD] = 2;
    assert!(exact_base_feature_construction(&mismatched_member, &scope)
        .unwrap()
        .is_none());

    let mut mismatched_history = scope.clone();
    mismatched_history
        .try_edit(|draft| {
            draft.history_state_id = Some(18);
        })
        .unwrap();
    assert!(exact_base_feature_construction(&bytes, &mismatched_history)
        .unwrap()
        .is_none());
}
