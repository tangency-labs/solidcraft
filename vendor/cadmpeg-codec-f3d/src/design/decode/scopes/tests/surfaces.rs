// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::u64_from_index;

use crate::design::decode::scopes::base_feature::exact_base_feature_construction;
use crate::design::decode::scopes::surfaces::{
    exact_ruled_surface_operation, exact_surface_stitch_operation,
};
use crate::records::feature::base_feature::DesignBaseFeatureConstruction;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::feature::surface_ops::{
    DesignRuledSurfaceCorner, DesignRuledSurfaceMethod, DesignSurfaceStitchOperation,
};
use crate::test_support::indexed_header;

fn ruled_surface_fixture() -> Vec<u8> {
    let mut bytes = vec![0; 366];
    bytes[20..24].copy_from_slice(&1u32.to_le_bytes());
    let reference = |bytes: &mut [u8], at: usize, record_index: u32| {
        bytes[at] = 1;
        bytes[at + 1..at + 5].copy_from_slice(&record_index.to_le_bytes());
    };
    bytes[27] = 1;
    reference(&mut bytes, 28, 12);
    reference(&mut bytes, 39, 11);
    bytes[54..58].copy_from_slice(&1u32.to_le_bytes());
    reference(&mut bytes, 58, 13);
    bytes[73..77].copy_from_slice(&1u32.to_le_bytes());
    reference(&mut bytes, 77, 99);
    bytes[92..96].copy_from_slice(&1u32.to_le_bytes());
    reference(&mut bytes, 96, 15);
    bytes[107..111].copy_from_slice(&36u32.to_le_bytes());
    for (ordinal, byte) in b"00000000-0000-0000-0000-000000000000".iter().enumerate() {
        bytes[111 + ordinal * 2] = *byte;
    }
    bytes[186..190].copy_from_slice(&6u32.to_le_bytes());
    bytes
}

#[test]
fn ruled_surface_operation_reads_mode_parameters_and_ordered_edge_groups() {
    let mut bytes = ruled_surface_fixture();

    let operation = exact_ruled_surface_operation(&bytes, 0, 366, 186, &[11, 12, 13, 14, 15, 16])
        .unwrap()
        .expect("exact SurfaceRuled operation");
    assert_eq!(operation.method, DesignRuledSurfaceMethod::Normal);
    assert_eq!(operation.method_offset, 20);
    assert_eq!(operation.corner, DesignRuledSurfaceCorner::Rounded);
    assert_eq!(operation.corner_offset, 50);
    assert!(operation.alternate_face);
    assert_eq!(operation.alternate_face_offset, 27);
    assert_eq!(operation.angle_owner_record_index, 12);
    assert_eq!(operation.distance_owner_record_index, 11);
    assert_eq!(operation.edge_group_record_indices, [13, 15]);
    assert_eq!(operation.auxiliary_record_indices, [99]);
    assert_eq!(operation.direction_entity_id, None);

    bytes[20..24].copy_from_slice(&2u32.to_le_bytes());
    for (ordinal, byte) in b"01234567-89ab-cdef-0123-456789abcdef".iter().enumerate() {
        bytes[111 + ordinal * 2] = *byte;
    }
    let operation = exact_ruled_surface_operation(&bytes, 0, 366, 186, &[11, 12, 13, 14, 15, 16])
        .unwrap()
        .expect("directed SurfaceRuled operation");
    assert_eq!(operation.method, DesignRuledSurfaceMethod::Direction);
    assert_eq!(
        operation
            .direction_entity_id
            .as_ref()
            .map(crate::records::mesh::DesignRelaxedGuidText::as_str),
        Some("01234567-89ab-cdef-0123-456789abcdef")
    );
}

#[test]
fn surface_stitch_tolerance_uses_its_fixed_scope_owned_frame() {
    let mut bytes = Vec::new();
    indexed_header(&mut bytes, *b"308", 300);
    bytes.extend_from_slice(&[0; 8]);
    bytes.extend_from_slice(&[1, 1, 0, 0, 0]);
    bytes.push(1);
    bytes.extend_from_slice(&12u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 11]);
    bytes.extend_from_slice(&0.01f64.to_le_bytes());
    bytes.resize(104, 0);
    indexed_header(&mut bytes, *b"258", 300);
    bytes.extend_from_slice(&[0; 20]);
    indexed_header(&mut bytes, *b"331", 301);
    bytes.extend_from_slice(&[0; 20]);
    indexed_header(&mut bytes, *b"258", 301);

    assert_eq!(
        exact_surface_stitch_operation(
            &bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
            12,
            &[100, 200, 300, 301]
        ),
        Some(DesignSurfaceStitchOperation {
            gap_tolerance: cadmpeg_ir::scalar::PositiveReal::new(0.01).unwrap(),
            gap_tolerance_offset: 40,
            tolerance_record_index: 300,
            settings_record_index: 301,
        })
    );
}

#[test]
fn base_feature_scope_decodes_parallel_result_body_runs() {
    let mut bytes = vec![0u8; 375];
    bytes[19] = 1;
    bytes[20..24].copy_from_slice(&4u32.to_le_bytes());
    let mut cursor = 24;
    for (value, field) in [
        (101u64, [0, 0, 1, 0, 0, 0]),
        (202, [0; 6]),
        (301, [0; 6]),
        (302, [0, 0, 2, 0, 0, 0]),
    ] {
        bytes[cursor] = 1;
        bytes[cursor + 1..cursor + 9].copy_from_slice(&value.to_le_bytes());
        bytes[cursor + 9..cursor + 15].copy_from_slice(&field);
        cursor += 15;
    }
    bytes[cursor] = 1;
    cursor += 11;
    bytes[cursor..cursor + 4].copy_from_slice(&2u32.to_le_bytes());
    cursor += 4;
    for reference in [301u32, 302] {
        bytes[cursor] = 1;
        bytes[cursor + 1..cursor + 5].copy_from_slice(&reference.to_le_bytes());
        cursor += 11;
    }
    cursor += 1;
    bytes[cursor] = 1;
    bytes[cursor + 1..cursor + 9].copy_from_slice(&401u64.to_le_bytes());
    cursor += 15;
    bytes[cursor..cursor + 4].copy_from_slice(&2u32.to_le_bytes());
    cursor += 4;
    for result in [501u32, 502] {
        bytes[cursor] = 1;
        bytes[cursor + 1..cursor + 5].copy_from_slice(&result.to_le_bytes());
        cursor += 11;
    }
    assert!(cursor <= 171);

    let scope = DesignParameterScope::try_new(
        crate::records::feature::scope::DesignParameterScopeDraft {
            id: "f3d:Design/BulkStream.dat:design-parameter-scope#0".into(),
            byte_offset: 0,
            class_tag: crate::records::references::DesignClassTag::try_from("306".to_owned())
                .unwrap(),
            record_index: 1,
            frame_length: 375,
            kind_offset: 273,
            feature_ordinal: std::num::NonZeroU32::MIN,
            feature_ordinal_offset: 0,
            history_state_id: Some(2),

            previous_history_state_id: Some(2),
            previous_history_state_id_offset: None,
            reference_count_offset: 250,
            reference_members: crate::records::identity::ReferenceRun::from_columns(
                vec![301],
                vec![0],
                "reference_members",
            )
            .unwrap(),
            payload: crate::records::feature::scope::DesignFeatureKind::BaseFeature
                .try_into()
                .unwrap(),
            unclosed_construction_operand_groups: Vec::new(),
            paired_class_tag: crate::records::references::DesignClassTag::try_from(
                "261".to_owned(),
            )
            .unwrap(),
            paired_byte_offset: 375,
        }
        .with_fixture_layout(),
    )
    .unwrap();
    let construction = exact_base_feature_construction(&bytes, &scope)
        .unwrap()
        .expect("generated Base Feature frame is canonical");
    let DesignBaseFeatureConstruction::ResultBodies {
        bodies,
        metadata_record,
        ..
    } = &construction
    else {
        panic!("parallel Base Feature frame selected the wrong form");
    };
    let body_entity_suffixes = &bodies
        .iter()
        .map(|body| body.entity.value)
        .collect::<Vec<_>>();
    let body_entity_fields = &bodies
        .iter()
        .map(|body| body.entity.field)
        .collect::<Vec<_>>();
    let body_reference_records = &bodies
        .iter()
        .map(|body| body.reference.value)
        .collect::<Vec<_>>();
    let result_records = &bodies
        .iter()
        .map(|body| body.result.value)
        .collect::<Vec<_>>();
    assert_eq!(body_entity_suffixes, &[101, 202]);
    assert_eq!(body_reference_records, &[301, 302]);
    assert_eq!(*metadata_record, 401);
    assert_eq!(result_records, &[501, 502]);
    assert_eq!(body_entity_fields[0], [0, 0, 1, 0, 0, 0]);
    let serialized = serde_json::to_value(&construction).expect("serialize legacy form");
    assert!(serialized.get("form").is_none());
    assert_eq!(
        serde_json::from_value::<DesignBaseFeatureConstruction>(serialized)
            .expect("deserialize legacy form"),
        construction
    );

    // The ordinary form is sized by its kind offset, so a shorter scope
    // tail admits the same construction; a moved kind does not.
    let mut short_tail_scope = scope.clone();
    short_tail_scope
        .try_edit(|draft| {
            draft.frame_length = 374;
            draft.paired_byte_offset = 374;
            draft.layout_fixture_tail();
        })
        .unwrap();
    assert!(exact_base_feature_construction(&bytes, &short_tail_scope)
        .unwrap()
        .is_some());
    let mut moved_kind_scope = scope.clone();
    moved_kind_scope
        .try_edit(|draft| {
            draft.reference_count_offset += 1;
            draft.layout_fixture_references();
        })
        .unwrap();
    assert!(exact_base_feature_construction(&bytes, &moved_kind_scope)
        .unwrap()
        .is_none());

    let mut expanded_bytes = Vec::new();
    expanded_bytes.extend_from_slice(&bytes[..84]);
    expanded_bytes.push(1);
    expanded_bytes.extend_from_slice(&[0; 6]);
    expanded_bytes.extend_from_slice(&2u32.to_le_bytes());
    expanded_bytes.extend_from_slice(&bytes[99..131]);
    expanded_bytes.extend_from_slice(&bytes[131..133]);
    expanded_bytes.extend_from_slice(&bytes[137..]);
    expanded_bytes.resize(366, 0);
    let mut expanded_scope = scope.clone();
    expanded_scope.class_tag =
        crate::records::references::DesignClassTag::try_from("384".to_owned()).unwrap();
    expanded_scope.paired_class_tag =
        crate::records::references::DesignClassTag::try_from("264".to_owned()).unwrap();
    expanded_scope
        .try_edit(|draft| {
            draft.frame_length = 366;
            draft.kind_offset = 265;
            draft.paired_byte_offset = 366;
            draft.reference_count_offset =
                draft.kind_offset - 12 - 11 * u64_from_index(draft.reference_members.len());
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    let expanded = exact_base_feature_construction(&expanded_bytes, &expanded_scope)
        .unwrap()
        .expect("expanded Base Feature frame is canonical");
    let DesignBaseFeatureConstruction::ResultBodies {
        bodies,
        metadata_field,
        ..
    } = &expanded
    else {
        panic!("expanded Base Feature frame selected the wrong form");
    };
    let body_entity_suffixes = &bodies
        .iter()
        .map(|body| body.entity.value)
        .collect::<Vec<_>>();
    let result_records = &bodies
        .iter()
        .map(|body| body.result.value)
        .collect::<Vec<_>>();
    assert_eq!(body_entity_suffixes, &[101, 202]);
    assert_eq!(result_records, &[501, 502]);
    assert_eq!(metadata_field, &[0, 0]);

    let mut legacy_compact_bytes = expanded_bytes.clone();
    legacy_compact_bytes[90] = 1;
    legacy_compact_bytes[96..100].copy_from_slice(&101u32.to_le_bytes());
    legacy_compact_bytes[107..111].copy_from_slice(&202u32.to_le_bytes());
    let mut legacy_compact_scope = expanded_scope.clone();
    legacy_compact_scope.class_tag =
        crate::records::references::DesignClassTag::try_from("420".to_owned()).unwrap();
    legacy_compact_scope.paired_class_tag =
        crate::records::references::DesignClassTag::try_from("258".to_owned()).unwrap();
    let legacy_compact =
        exact_base_feature_construction(&legacy_compact_bytes, &legacy_compact_scope)
            .unwrap()
            .expect("legacy compact Base Feature frame is canonical");
    let DesignBaseFeatureConstruction::ResultBodies {
        bodies,
        metadata_field,
        ..
    } = &legacy_compact
    else {
        panic!("legacy compact Base Feature frame selected the wrong form");
    };
    let body_entity_suffixes = &bodies
        .iter()
        .map(|body| body.entity.value)
        .collect::<Vec<_>>();
    let body_reference_records = &bodies
        .iter()
        .map(|body| body.reference.value)
        .collect::<Vec<_>>();
    let result_records = &bodies
        .iter()
        .map(|body| body.result.value)
        .collect::<Vec<_>>();
    assert_eq!(body_entity_suffixes, &[101, 202]);
    assert_eq!(body_reference_records, &[301, 302]);
    assert_eq!(result_records, &[501, 502]);
    assert_eq!(metadata_field, &[0, 0]);

    legacy_compact_bytes[96..100].copy_from_slice(&301u32.to_le_bytes());
    assert!(
        exact_base_feature_construction(&legacy_compact_bytes, &legacy_compact_scope)
            .unwrap()
            .is_none()
    );

    let mut snapshot_bytes = vec![0u8; 485];
    snapshot_bytes[19] = 1;
    snapshot_bytes[20..24].copy_from_slice(&2u32.to_le_bytes());
    let mut cursor = 24;
    for (value, field) in [(101u64, [0u8, 0, 3, 4, 5, 6]), (202, [0u8, 0, 4, 3, 2, 1])] {
        snapshot_bytes[cursor] = 1;
        snapshot_bytes[cursor + 1..cursor + 9].copy_from_slice(&value.to_le_bytes());
        snapshot_bytes[cursor + 9..cursor + 15].copy_from_slice(&field);
        cursor += 15;
    }
    snapshot_bytes[cursor..cursor + 4].copy_from_slice(&1u32.to_le_bytes());
    snapshot_bytes[cursor + 4..cursor + 8].copy_from_slice(&1u32.to_le_bytes());
    cursor += 8;
    let related_guids = [
        "11111111-2222-3333-4444-555555555555",
        "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee",
    ];
    for guid in related_guids {
        let encoded =
            crate::bytes::lp_utf16_bytes(guid).expect("fixture UTF-16 code-unit count fits u32");
        snapshot_bytes[cursor..cursor + encoded.len()].copy_from_slice(&encoded);
        cursor += encoded.len();
    }
    snapshot_bytes[cursor..cursor + 7].copy_from_slice(&[0, 0, 1, 1, 0, 0, 0]);
    cursor += 7;
    snapshot_bytes[cursor] = 1;
    cursor += 1;
    snapshot_bytes[cursor..cursor + 8].copy_from_slice(&101u64.to_le_bytes());
    cursor += 8;
    cursor += 3;
    snapshot_bytes[cursor] = 1;
    cursor += 1;
    snapshot_bytes[cursor..cursor + 8].copy_from_slice(&301u64.to_le_bytes());
    cursor += 8;
    cursor += 6;
    snapshot_bytes[cursor..cursor + 4].copy_from_slice(&1u32.to_le_bytes());
    cursor += 4;
    snapshot_bytes[cursor] = 1;
    cursor += 1;
    snapshot_bytes[cursor..cursor + 8].copy_from_slice(&401u64.to_le_bytes());
    cursor += 8;
    cursor += 6;
    cursor += 4;
    let third_guid = "00000000-0000-0000-0000-000000000000";
    let encoded =
        crate::bytes::lp_utf16_bytes(third_guid).expect("fixture UTF-16 code-unit count fits u32");
    snapshot_bytes[cursor..cursor + encoded.len()].copy_from_slice(&encoded);
    cursor += encoded.len();
    cursor += 3;
    snapshot_bytes[cursor..cursor + 4].copy_from_slice(&1u32.to_le_bytes());
    cursor += 4;
    snapshot_bytes[cursor] = 1;
    cursor += 1;
    snapshot_bytes[cursor..cursor + 4].copy_from_slice(&301u32.to_le_bytes());
    cursor += 4;
    cursor += 6;
    snapshot_bytes[cursor..cursor + 4].copy_from_slice(&7u32.to_le_bytes());
    cursor += 4;
    let encoded = crate::bytes::lp_utf16_bytes("Base Feature")
        .expect("fixture UTF-16 code-unit count fits u32");
    snapshot_bytes[cursor..cursor + encoded.len()].copy_from_slice(&encoded);
    cursor += encoded.len();
    snapshot_bytes[cursor..cursor + 4].copy_from_slice(&1u32.to_le_bytes());
    cursor += 4;
    assert_eq!(cursor, 401);

    let mut snapshot_scope = scope;
    snapshot_scope.class_tag =
        crate::records::references::DesignClassTag::try_from("314".to_owned()).unwrap();
    snapshot_scope
        .try_edit(|draft| {
            draft.frame_length = 485;
            draft.paired_byte_offset = 485;
            draft.kind_offset = 373;
            draft.feature_ordinal_offset = 397;
            draft.history_state_id = Some(7);

            draft.previous_history_state_id = None;
            draft.previous_history_state_id_offset = None;
            draft.reference_count_offset = 350;
            draft.reference_members = crate::records::identity::ReferenceRun::from_columns(
                vec![301],
                vec![355],
                "reference_members",
            )
            .unwrap();
        })
        .unwrap();
    snapshot_scope.paired_class_tag =
        crate::records::references::DesignClassTag::try_from("259".to_owned()).unwrap();
    let construction = exact_base_feature_construction(&snapshot_bytes, &snapshot_scope)
        .unwrap()
        .expect("body-snapshot Base Feature frame is canonical");
    let serialized = serde_json::to_value(&construction).expect("serialize snapshot form");
    assert!(serialized.get("form").is_none());
    assert_eq!(
        serde_json::from_value::<DesignBaseFeatureConstruction>(serialized)
            .expect("deserialize snapshot form"),
        construction
    );
    let DesignBaseFeatureConstruction::BodySnapshot {
        bodies,
        related_guids: decoded_guids,
        related_guid_offsets,
        linkage_record,
        linkage_record_offset,
        auxiliary_record,
        auxiliary_record_offset,
        ..
    } = construction
    else {
        panic!("body-snapshot Base Feature frame selected the wrong form");
    };
    assert_eq!(
        bodies.iter().map(|body| body.value).collect::<Vec<_>>(),
        [101, 202]
    );
    assert_eq!(bodies[0].field, [0, 0, 3, 4, 5, 6]);
    assert_eq!(
        decoded_guids.map(String::from),
        [
            related_guids[0].to_owned(),
            related_guids[1].to_owned(),
            third_guid.to_owned()
        ]
    );
    assert_eq!(related_guid_offsets, [66, 142, 275]);
    assert_eq!(linkage_record, 301);
    assert_eq!(linkage_record_offset, 234);
    assert_eq!(auxiliary_record, 401);
    assert_eq!(auxiliary_record_offset, 253);

    let mut packed_snapshot_bytes = vec![0u8; 485];
    packed_snapshot_bytes[..54].copy_from_slice(&snapshot_bytes[..54]);
    packed_snapshot_bytes[54..58].copy_from_slice(&snapshot_bytes[54..58]);
    packed_snapshot_bytes[58] = 0;
    packed_snapshot_bytes[59..63].copy_from_slice(&snapshot_bytes[58..62]);
    packed_snapshot_bytes[63..215].copy_from_slice(&snapshot_bytes[62..214]);
    packed_snapshot_bytes[214..].copy_from_slice(&snapshot_bytes[214..]);
    let packed = exact_base_feature_construction(&packed_snapshot_bytes, &snapshot_scope)
        .unwrap()
        .expect("packed body-snapshot Base Feature frame is canonical");
    let DesignBaseFeatureConstruction::BodySnapshot {
        related_guid_offsets,
        ..
    } = packed
    else {
        panic!("packed body-snapshot Base Feature frame selected the wrong form");
    };
    assert_eq!(related_guid_offsets, [67, 143, 275]);

    let mut invalid_scope = snapshot_scope;
    invalid_scope
        .try_edit(|draft| {
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(vec![302]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    assert!(
        exact_base_feature_construction(&snapshot_bytes, &invalid_scope)
            .unwrap()
            .is_none()
    );
}

#[test]
fn base_feature_scope_decodes_class_452_compact_result_body_run() {
    let mut bytes = vec![0u8; 314];
    bytes[19] = 1;
    bytes[20..24].copy_from_slice(&2u32.to_le_bytes());
    let mut cursor = 24;
    for (value, field) in [(101u64, [0; 6]), (201, [0; 6])] {
        bytes[cursor] = 1;
        bytes[cursor + 1..cursor + 9].copy_from_slice(&value.to_le_bytes());
        bytes[cursor + 9..cursor + 15].copy_from_slice(&field);
        cursor += 15;
    }
    bytes[cursor] = 1;
    bytes[cursor + 6] = 1;
    bytes[cursor + 7..cursor + 11].copy_from_slice(&1u32.to_le_bytes());
    cursor += 11;
    bytes[cursor] = 1;
    bytes[cursor + 1..cursor + 5].copy_from_slice(&101u32.to_le_bytes());
    cursor += 11;
    bytes[cursor] = 0;
    cursor += 1;
    bytes[cursor] = 1;
    bytes[cursor + 1..cursor + 9].copy_from_slice(&301u64.to_le_bytes());
    bytes[cursor + 9..cursor + 11].copy_from_slice(&[0, 0]);
    cursor += 11;
    bytes[cursor..cursor + 4].copy_from_slice(&1u32.to_le_bytes());
    cursor += 4;
    bytes[cursor] = 1;
    bytes[cursor + 1..cursor + 5].copy_from_slice(&401u32.to_le_bytes());
    assert_eq!(cursor + 11, 103);

    let mut scope = DesignParameterScope::empty(
        "f3d:scope#base-feature-compact",
        crate::records::feature::scope::DesignFeatureKind::BaseFeature,
        70,
    );
    scope.class_tag =
        crate::records::references::DesignClassTag::try_from("452".to_owned()).unwrap();
    scope
        .try_edit(|draft| {
            draft.frame_length = 314;
            draft.kind_offset = 213;
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(vec![301]);
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.reference_count_offset =
                draft.kind_offset - 12 - 11 * u64_from_index(draft.reference_members.len());
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    scope.paired_class_tag =
        crate::records::references::DesignClassTag::try_from("266".to_owned()).unwrap();
    scope
        .try_edit(|draft| {
            draft.paired_byte_offset = 314;
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    let construction = exact_base_feature_construction(&bytes, &scope)
        .unwrap()
        .expect("class-452 compact Base Feature frame is canonical");
    let DesignBaseFeatureConstruction::ResultBodies {
        bodies,
        metadata_record,
        ..
    } = construction
    else {
        panic!("class-452 compact Base Feature frame selected the wrong form");
    };
    let body_entity_suffixes = bodies
        .iter()
        .map(|body| body.entity.value)
        .collect::<Vec<_>>();
    let body_reference_records = bodies
        .iter()
        .map(|body| body.reference.value)
        .collect::<Vec<_>>();
    let result_records = bodies
        .iter()
        .map(|body| body.result.value)
        .collect::<Vec<_>>();
    assert_eq!(body_entity_suffixes, [101]);
    assert_eq!(body_reference_records, [201]);
    assert_eq!(metadata_record, 301);
    assert_eq!(result_records, [401]);

    let mut nonzero_prefix = bytes;
    nonzero_prefix[11] = 1;
    assert!(exact_base_feature_construction(&nonzero_prefix, &scope)
        .unwrap()
        .is_none());
}

#[test]
fn base_feature_scope_decodes_class_409_262_result_body_variants() {
    fn frame(body_count: usize) -> (Vec<u8>, DesignParameterScope) {
        let frame_length = 262 + 52 * body_count;
        let mut bytes = vec![0u8; frame_length];
        bytes[19] = 1;
        bytes[20..24].copy_from_slice(
            &(2 * u32::try_from(body_count).expect("fixture value fits u32")).to_le_bytes(),
        );
        let mut cursor = 24;
        for ordinal in 0..body_count {
            let value = 101 + u64_from_index(ordinal);
            bytes[cursor] = 1;
            bytes[cursor + 1..cursor + 9].copy_from_slice(&value.to_le_bytes());
            cursor += 15;
        }
        for ordinal in 0..body_count {
            let value = 201 + u64_from_index(ordinal);
            bytes[cursor] = 1;
            bytes[cursor + 1..cursor + 9].copy_from_slice(&value.to_le_bytes());
            cursor += 15;
        }
        bytes[cursor] = 1;
        bytes[cursor + 7..cursor + 11].copy_from_slice(
            &(u32::try_from(body_count).expect("fixture value fits u32")).to_le_bytes(),
        );
        cursor += 11;
        for ordinal in 0..body_count {
            bytes[cursor] = 1;
            bytes[cursor + 1..cursor + 5].copy_from_slice(
                &(201 + u32::try_from(ordinal).expect("fixture value fits u32")).to_le_bytes(),
            );
            cursor += 11;
        }
        bytes[cursor] = 0;
        cursor += 1;
        bytes[cursor] = 1;
        bytes[cursor + 1..cursor + 9].copy_from_slice(&301u64.to_le_bytes());
        cursor += 11;
        bytes[cursor..cursor + 4].copy_from_slice(
            &(u32::try_from(body_count).expect("fixture value fits u32")).to_le_bytes(),
        );
        cursor += 4;
        for ordinal in 0..body_count {
            bytes[cursor] = 1;
            bytes[cursor + 1..cursor + 5].copy_from_slice(
                &(401 + u32::try_from(ordinal).expect("fixture value fits u32")).to_le_bytes(),
            );
            cursor += 11;
        }
        let mut scope = DesignParameterScope::empty(
            "f3d:scope#base-feature-409-262",
            crate::records::feature::scope::DesignFeatureKind::BaseFeature,
            70,
        );
        scope.class_tag =
            crate::records::references::DesignClassTag::try_from("409".to_owned()).unwrap();
        scope.paired_class_tag =
            crate::records::references::DesignClassTag::try_from("262".to_owned()).unwrap();
        scope
            .try_edit(|draft| {
                draft.frame_length = u64_from_index(frame_length);
                draft.kind_offset = u64_from_index(frame_length - 102);
                draft.paired_byte_offset = u64_from_index(frame_length);
                draft.reference_members =
                    crate::records::identity::ReferenceRun::unlocated(vec![301]);
                draft.reference_count_offset =
                    draft.kind_offset - 12 - 11 * u64_from_index(draft.reference_members.len());
                draft.layout_fixture_references();
                draft.layout_fixture_tail();
            })
            .unwrap();
        assert!(cursor <= frame_length);
        (bytes, scope)
    }

    for body_count in [1, 3, 4] {
        let (bytes, scope) = frame(body_count);
        let construction = exact_base_feature_construction(&bytes, &scope)
            .unwrap()
            .expect("class-409/class-262 result-body frame is canonical");
        let DesignBaseFeatureConstruction::ResultBodies {
            bodies,
            metadata_record,
            ..
        } = construction
        else {
            panic!("class-409/class-262 frame selected the wrong form");
        };
        let body_entity_suffixes = bodies
            .iter()
            .map(|body| body.entity.value)
            .collect::<Vec<_>>();
        let body_reference_records = bodies
            .iter()
            .map(|body| body.reference.value)
            .collect::<Vec<_>>();
        let result_records = bodies
            .iter()
            .map(|body| body.result.value)
            .collect::<Vec<_>>();
        assert_eq!(body_entity_suffixes.len(), body_count);
        assert_eq!(body_reference_records.len(), body_count);
        assert_eq!(metadata_record, 301);
        assert_eq!(result_records.len(), body_count);

        let mut class_360_scope = scope.clone();
        class_360_scope.class_tag =
            crate::records::references::DesignClassTag::try_from("360".to_owned()).unwrap();
        class_360_scope.paired_class_tag =
            crate::records::references::DesignClassTag::try_from("258".to_owned()).unwrap();
        let construction = exact_base_feature_construction(&bytes, &class_360_scope)
            .unwrap()
            .expect("class-360/class-258 result-body frame is canonical");
        let DesignBaseFeatureConstruction::ResultBodies {
            bodies,
            metadata_record,
            ..
        } = construction
        else {
            panic!("class-360/class-258 frame selected the wrong form");
        };
        let body_entity_suffixes = bodies
            .iter()
            .map(|body| body.entity.value)
            .collect::<Vec<_>>();
        let body_reference_records = bodies
            .iter()
            .map(|body| body.reference.value)
            .collect::<Vec<_>>();
        let result_records = bodies
            .iter()
            .map(|body| body.result.value)
            .collect::<Vec<_>>();
        assert_eq!(body_entity_suffixes.len(), body_count);
        assert_eq!(body_reference_records.len(), body_count);
        assert_eq!(metadata_record, 301);
        assert_eq!(result_records.len(), body_count);
    }

    let prefix = 17;
    let mut zero_body = vec![0u8; prefix + 258];
    zero_body[prefix + 20] = 1;
    zero_body[prefix + 32] = 1;
    zero_body[prefix + 33..prefix + 41].copy_from_slice(&701u64.to_le_bytes());
    let mut zero_scope = DesignParameterScope::empty(
        "f3d:scope#base-feature-409-262-zero",
        crate::records::feature::scope::DesignFeatureKind::BaseFeature,
        71,
    );
    zero_scope.class_tag =
        crate::records::references::DesignClassTag::try_from("409".to_owned()).unwrap();
    zero_scope.paired_class_tag =
        crate::records::references::DesignClassTag::try_from("262".to_owned()).unwrap();
    zero_scope
        .try_edit(|draft| {
            draft.byte_offset = u64_from_index(prefix);
            draft.frame_length = 258;
            draft.kind_offset = u64_from_index(prefix + 157);
            draft.paired_byte_offset = u64_from_index(prefix + 258);
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(vec![701]);
            draft.reference_count_offset = draft.byte_offset + 9;
            draft.reference_count_offset =
                draft.kind_offset - 12 - 11 * u64_from_index(draft.reference_members.len());
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    let construction = exact_base_feature_construction(&zero_body, &zero_scope)
        .unwrap()
        .expect("class-409/class-262 zero-body frame is canonical");
    let DesignBaseFeatureConstruction::ResultBodies {
        bodies,
        metadata_record,
        metadata_record_offset,
        metadata_field,
        ..
    } = construction
    else {
        panic!("class-409/class-262 zero-body frame selected the wrong form");
    };
    let body_entity_suffixes = bodies
        .iter()
        .map(|body| body.entity.value)
        .collect::<Vec<_>>();
    assert!(body_entity_suffixes.is_empty());
    assert_eq!(metadata_record, 701);
    assert_eq!(metadata_record_offset, u64_from_index(prefix + 33));
    assert_eq!(metadata_field, [0; 6]);

    let mut nonzero_padding = zero_body.clone();
    nonzero_padding[prefix + 47] = 1;
    assert!(
        exact_base_feature_construction(&nonzero_padding, &zero_scope)
            .unwrap()
            .is_none()
    );
}

#[test]
fn base_feature_scope_decodes_class_290_261_result_body_variant() {
    let body_count = 2;
    let frame_length = 261 + 52 * body_count;
    let mut bytes = vec![0u8; frame_length];
    bytes[19] = 1;
    bytes[20..24].copy_from_slice(
        &(2 * u32::try_from(body_count).expect("fixture value fits u32")).to_le_bytes(),
    );
    let mut cursor = 24;
    for value in [101u64, 102] {
        bytes[cursor] = 1;
        bytes[cursor + 1..cursor + 9].copy_from_slice(&value.to_le_bytes());
        cursor += 15;
    }
    for value in [201u64, 202] {
        bytes[cursor] = 1;
        bytes[cursor + 1..cursor + 9].copy_from_slice(&value.to_le_bytes());
        cursor += 15;
    }
    bytes[cursor] = 1;
    bytes[cursor + 7..cursor + 11].copy_from_slice(
        &(u32::try_from(body_count).expect("fixture value fits u32")).to_le_bytes(),
    );
    cursor += 11;
    for value in [201u32, 202] {
        bytes[cursor] = 1;
        bytes[cursor + 1..cursor + 5].copy_from_slice(&value.to_le_bytes());
        cursor += 11;
    }
    bytes[cursor] = 0;
    cursor += 1;
    bytes[cursor] = 1;
    bytes[cursor + 1..cursor + 9].copy_from_slice(&301u64.to_le_bytes());
    cursor += 11;
    bytes[cursor..cursor + 4].copy_from_slice(
        &(u32::try_from(body_count).expect("fixture value fits u32")).to_le_bytes(),
    );
    cursor += 4;
    for value in [401u32, 402] {
        bytes[cursor] = 1;
        bytes[cursor + 1..cursor + 5].copy_from_slice(&value.to_le_bytes());
        cursor += 11;
    }
    let mut scope = DesignParameterScope::empty(
        "f3d:scope#base-feature-290-261",
        crate::records::feature::scope::DesignFeatureKind::BaseFeature,
        74,
    );
    scope.class_tag =
        crate::records::references::DesignClassTag::try_from("290".to_owned()).unwrap();
    scope.paired_class_tag =
        crate::records::references::DesignClassTag::try_from("261".to_owned()).unwrap();
    scope
        .try_edit(|draft| {
            draft.frame_length = u64_from_index(frame_length);
            draft.kind_offset = u64_from_index(frame_length - 102);
            draft.paired_byte_offset = u64_from_index(frame_length);
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(vec![301]);
            draft.reference_count_offset =
                draft.kind_offset - 12 - 11 * u64_from_index(draft.reference_members.len());
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    assert_eq!(cursor, 155);

    let construction = exact_base_feature_construction(&bytes, &scope)
        .unwrap()
        .expect("class-290/class-261 result-body frame is canonical");
    let DesignBaseFeatureConstruction::ResultBodies {
        bodies,
        metadata_record,
        metadata_field,
        ..
    } = construction
    else {
        panic!("class-290/class-261 frame selected the wrong form");
    };
    let body_entity_suffixes = bodies
        .iter()
        .map(|body| body.entity.value)
        .collect::<Vec<_>>();
    let body_reference_records = bodies
        .iter()
        .map(|body| body.reference.value)
        .collect::<Vec<_>>();
    let result_records = bodies
        .iter()
        .map(|body| body.result.value)
        .collect::<Vec<_>>();
    assert_eq!(body_entity_suffixes, [101, 102]);
    assert_eq!(body_reference_records, [201, 202]);
    assert_eq!(metadata_record, 301);
    assert_eq!(result_records, [401, 402]);
    assert_eq!(metadata_field, [0, 0]);
}

#[test]
fn base_feature_scope_decodes_class_444_263_result_body_variants() {
    fn frame(body_count: usize) -> (Vec<u8>, DesignParameterScope) {
        let frame_length = 262 + 52 * body_count;
        let mut bytes = vec![0u8; frame_length];
        bytes[19] = 1;
        bytes[20..24].copy_from_slice(
            &(2 * u32::try_from(body_count).expect("fixture value fits u32")).to_le_bytes(),
        );
        let mut cursor = 24;
        for ordinal in 0..body_count {
            let value = 101 + u64_from_index(ordinal);
            bytes[cursor] = 1;
            bytes[cursor + 1..cursor + 9].copy_from_slice(&value.to_le_bytes());
            cursor += 15;
        }
        for ordinal in 0..body_count {
            let value = 201 + u64_from_index(ordinal);
            bytes[cursor] = 1;
            bytes[cursor + 1..cursor + 9].copy_from_slice(&value.to_le_bytes());
            cursor += 15;
        }
        bytes[cursor] = 1;
        bytes[cursor + 7..cursor + 11].copy_from_slice(
            &(u32::try_from(body_count).expect("fixture value fits u32")).to_le_bytes(),
        );
        cursor += 11;
        for ordinal in 0..body_count {
            bytes[cursor] = 1;
            bytes[cursor + 1..cursor + 5].copy_from_slice(
                &(201 + u32::try_from(ordinal).expect("fixture value fits u32")).to_le_bytes(),
            );
            cursor += 11;
        }
        bytes[cursor] = 0;
        cursor += 1;
        bytes[cursor] = 1;
        bytes[cursor + 1..cursor + 9].copy_from_slice(&301u64.to_le_bytes());
        cursor += 11;
        bytes[cursor..cursor + 4].copy_from_slice(
            &(u32::try_from(body_count).expect("fixture value fits u32")).to_le_bytes(),
        );
        cursor += 4;
        for ordinal in 0..body_count {
            bytes[cursor] = 1;
            bytes[cursor + 1..cursor + 5].copy_from_slice(
                &(401 + u32::try_from(ordinal).expect("fixture value fits u32")).to_le_bytes(),
            );
            cursor += 11;
        }
        let mut scope = DesignParameterScope::empty(
            "f3d:scope#base-feature-444-263",
            crate::records::feature::scope::DesignFeatureKind::BaseFeature,
            72,
        );
        scope.class_tag =
            crate::records::references::DesignClassTag::try_from("444".to_owned()).unwrap();
        scope.paired_class_tag =
            crate::records::references::DesignClassTag::try_from("263".to_owned()).unwrap();
        scope
            .try_edit(|draft| {
                draft.frame_length = u64_from_index(frame_length);
                draft.kind_offset = u64_from_index(frame_length - 102);
                draft.paired_byte_offset = u64_from_index(frame_length);
                draft.reference_members =
                    crate::records::identity::ReferenceRun::unlocated(vec![301]);
                draft.reference_count_offset =
                    draft.kind_offset - 12 - 11 * u64_from_index(draft.reference_members.len());
                draft.layout_fixture_references();
                draft.layout_fixture_tail();
            })
            .unwrap();
        assert!(cursor <= frame_length);
        (bytes, scope)
    }

    for body_count in [1, 3, 4] {
        let (bytes, scope) = frame(body_count);
        let construction = exact_base_feature_construction(&bytes, &scope)
            .unwrap()
            .expect("class-444/class-263 result-body frame is canonical");
        let DesignBaseFeatureConstruction::ResultBodies {
            bodies,
            metadata_record,
            ..
        } = construction
        else {
            panic!("class-444/class-263 frame selected the wrong form");
        };
        let body_entity_suffixes = bodies
            .iter()
            .map(|body| body.entity.value)
            .collect::<Vec<_>>();
        let body_reference_records = bodies
            .iter()
            .map(|body| body.reference.value)
            .collect::<Vec<_>>();
        let result_records = bodies
            .iter()
            .map(|body| body.result.value)
            .collect::<Vec<_>>();
        assert_eq!(body_entity_suffixes.len(), body_count);
        assert_eq!(body_reference_records.len(), body_count);
        assert_eq!(metadata_record, 301);
        assert_eq!(result_records.len(), body_count);
    }

    let (mut bytes, scope) = frame(1);
    bytes[24 + 30 + 6] = 1;
    assert!(exact_base_feature_construction(&bytes, &scope)
        .unwrap()
        .is_none());

    let prefix = 17;
    let mut zero_body = vec![0u8; prefix + 258];
    zero_body[prefix + 20] = 1;
    zero_body[prefix + 32] = 1;
    zero_body[prefix + 33..prefix + 41].copy_from_slice(&701u64.to_le_bytes());
    zero_body[prefix + 55..prefix + 59].copy_from_slice(&36u32.to_le_bytes());
    let guid = "00000000-0000-0000-0000-000000000000";
    let guid_utf16 = guid.encode_utf16().collect::<Vec<_>>();
    for (ordinal, code_unit) in guid_utf16.into_iter().enumerate() {
        zero_body[prefix + 59 + ordinal * 2..prefix + 61 + ordinal * 2]
            .copy_from_slice(&code_unit.to_le_bytes());
    }
    zero_body[prefix + 134..prefix + 138].copy_from_slice(&1u32.to_le_bytes());
    zero_body[prefix + 138] = 1;
    zero_body[prefix + 139..prefix + 143].copy_from_slice(&701u32.to_le_bytes());
    zero_body[prefix + 149..prefix + 153].copy_from_slice(&17u32.to_le_bytes());
    zero_body[prefix + 153..prefix + 157].copy_from_slice(&12u32.to_le_bytes());
    for (ordinal, code_unit) in "Base Feature".encode_utf16().enumerate() {
        zero_body[prefix + 157 + ordinal * 2..prefix + 159 + ordinal * 2]
            .copy_from_slice(&code_unit.to_le_bytes());
    }
    zero_body[prefix + 181..prefix + 185].copy_from_slice(&1u32.to_le_bytes());
    zero_body[prefix + 212..prefix + 216].copy_from_slice(&2u32.to_le_bytes());
    let mut zero_scope = DesignParameterScope::empty(
        "f3d:scope#base-feature-444-263-zero",
        crate::records::feature::scope::DesignFeatureKind::BaseFeature,
        73,
    );
    zero_scope.class_tag =
        crate::records::references::DesignClassTag::try_from("444".to_owned()).unwrap();
    zero_scope.paired_class_tag =
        crate::records::references::DesignClassTag::try_from("263".to_owned()).unwrap();
    zero_scope
        .try_edit(|draft| {
            draft.byte_offset = u64_from_index(prefix);
            draft.frame_length = 258;
            draft.kind_offset = u64_from_index(prefix + 157);
            draft.paired_byte_offset = u64_from_index(prefix + 258);
            draft.reference_count_offset = u64_from_index(prefix + 134);
            draft.reference_members = crate::records::identity::ReferenceRun::from_columns(
                vec![701],
                vec![u64_from_index(prefix + 139)],
                "reference_members",
            )
            .unwrap();
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    let construction = exact_base_feature_construction(&zero_body, &zero_scope)
        .unwrap()
        .expect("class-444/class-263 zero-body frame is canonical");
    let DesignBaseFeatureConstruction::ResultBodies {
        bodies,
        metadata_record,
        metadata_record_offset,
        metadata_field,
        ..
    } = construction
    else {
        panic!("class-444/class-263 zero-body frame selected the wrong form");
    };
    let body_entity_suffixes = bodies
        .iter()
        .map(|body| body.entity.value)
        .collect::<Vec<_>>();
    assert!(body_entity_suffixes.is_empty());
    assert_eq!(metadata_record, 701);
    assert_eq!(metadata_record_offset, u64_from_index(prefix + 33));
    assert_eq!(metadata_field, [0; 14]);

    let mut nonzero_tail = zero_body.clone();
    nonzero_tail[prefix + 41] = 1;
    assert!(exact_base_feature_construction(&nonzero_tail, &zero_scope)
        .unwrap()
        .is_none());

    let mut mismatched_reference = zero_body.clone();
    mismatched_reference[prefix + 139] = 1;
    assert!(
        exact_base_feature_construction(&mismatched_reference, &zero_scope)
            .unwrap()
            .is_none()
    );
}

#[test]
fn base_feature_scope_decodes_shared_body_based_on_faces_envelope() {
    use crate::layout::base_feature_class_377_prefix as class_377;

    let mut bytes = vec![0u8; class_377::LEN];
    bytes[class_377::BODY_REFERENCE_COUNT_MARKER] = class_377::BODY_REFERENCE_COUNT_MARKER_VALUE;
    bytes[class_377::BODY_REFERENCE_COUNT..class_377::PARAMETER_BODY_REFERENCE_MARKER]
        .copy_from_slice(&class_377::BODY_REFERENCE_COUNT_VALUE.to_le_bytes());
    for (offset, record) in [
        (class_377::PARAMETER_BODY_REFERENCE_MARKER, 198u32),
        (class_377::BODY_ENTITY_REFERENCE_MARKER, 201),
    ] {
        bytes[offset] = 1;
        bytes[offset + 1..offset + 5].copy_from_slice(&record.to_le_bytes());
    }
    bytes[class_377::TAG_BODY_BASED_ON_FACES_MARKER] =
        class_377::TAG_BODY_BASED_ON_FACES_MARKER_VALUE;
    bytes[class_377::TAG_BODY_BASED_ON_FACES_COUNT..class_377::TAG_BODY_BASED_ON_FACES_KEY_LENGTH]
        .copy_from_slice(&class_377::TAG_BODY_BASED_ON_FACES_COUNT_VALUE.to_le_bytes());
    bytes[class_377::TAG_BODY_BASED_ON_FACES_KEY_LENGTH..class_377::TAG_BODY_BASED_ON_FACES_KEY]
        .copy_from_slice(&class_377::TAG_BODY_BASED_ON_FACES_KEY_LENGTH_VALUE.to_le_bytes());
    bytes[class_377::TAG_BODY_BASED_ON_FACES_KEY..class_377::TAG_BODY_BASED_ON_FACES_TYPE_LENGTH]
        .copy_from_slice(b"TagBodyBasedOnFaces");
    bytes[class_377::TAG_BODY_BASED_ON_FACES_TYPE_LENGTH..class_377::TAG_BODY_BASED_ON_FACES_TYPE]
        .copy_from_slice(&class_377::TAG_BODY_BASED_ON_FACES_TYPE_LENGTH_VALUE.to_le_bytes());
    bytes[class_377::TAG_BODY_BASED_ON_FACES_TYPE..class_377::TAG_BODY_BASED_ON_FACES_VALUE]
        .copy_from_slice(b"IntrinsicMetaTypebool");
    bytes[class_377::TAG_BODY_BASED_ON_FACES_VALUE..class_377::PARAMETER_REFERENCE_GROUP_MARKER]
        .copy_from_slice(&class_377::TAG_BODY_BASED_ON_FACES_VALUE_VALUE.to_le_bytes());
    bytes[class_377::PARAMETER_REFERENCE_GROUP_MARKER] =
        class_377::PARAMETER_REFERENCE_GROUP_MARKER_VALUE;
    bytes[class_377::PARAMETER_REFERENCE_GROUP_COUNT..class_377::PARAMETER_REFERENCE_MARKER]
        .copy_from_slice(&class_377::PARAMETER_REFERENCE_GROUP_COUNT_VALUE.to_le_bytes());
    bytes[class_377::PARAMETER_REFERENCE_MARKER] = class_377::PARAMETER_REFERENCE_MARKER_VALUE;
    bytes[class_377::PARAMETER_REFERENCE_RECORD..class_377::PARAMETER_REFERENCE_FIELD]
        .copy_from_slice(&198u32.to_le_bytes());
    bytes[class_377::SCOPE_REFERENCE_MEMBER_MARKER] =
        class_377::SCOPE_REFERENCE_MEMBER_MARKER_VALUE;
    bytes[class_377::SCOPE_REFERENCE_MEMBER_RECORD..class_377::SCOPE_REFERENCE_MEMBER_FIELD]
        .copy_from_slice(&196u32.to_le_bytes());
    bytes[class_377::AUXILIARY_GROUP_MARKER] = class_377::AUXILIARY_GROUP_MARKER_VALUE;
    bytes[class_377::AUXILIARY_REFERENCE_MARKER] = class_377::AUXILIARY_REFERENCE_MARKER_VALUE;
    bytes[class_377::AUXILIARY_RECORD..class_377::AUXILIARY_REFERENCE_FIELD]
        .copy_from_slice(&202u32.to_le_bytes());
    let guid = crate::bytes::lp_utf16_bytes("fcec56e3-832f-4468-88a4-d710e62e629f")
        .expect("fixture UTF-16 code-unit count fits u32");
    bytes[class_377::ENVELOPE_GUID_CODE_UNIT_COUNT..class_377::ZERO_RUN_3].copy_from_slice(&guid);
    bytes[class_377::REFERENCE_COUNT..class_377::GENERIC_SCOPE_REFERENCE_MARKER]
        .copy_from_slice(&class_377::REFERENCE_COUNT_VALUE.to_le_bytes());
    bytes[class_377::GENERIC_SCOPE_REFERENCE_MARKER] =
        class_377::GENERIC_SCOPE_REFERENCE_MARKER_VALUE;
    bytes[class_377::GENERIC_SCOPE_REFERENCE_RECORD..class_377::GENERIC_SCOPE_REFERENCE_FIELD]
        .copy_from_slice(&196u32.to_le_bytes());
    bytes[class_377::HISTORY_STATE_ID..class_377::KIND_LENGTH]
        .copy_from_slice(&20u32.to_le_bytes());
    bytes[class_377::KIND_LENGTH..class_377::KIND]
        .copy_from_slice(&class_377::KIND_LENGTH_VALUE.to_le_bytes());
    bytes[class_377::KIND..class_377::FEATURE_ORDINAL].copy_from_slice(
        &crate::bytes::lp_utf16_bytes("Base Feature")
            .expect("fixture UTF-16 code-unit count fits u32")[4..],
    );
    bytes[class_377::FEATURE_ORDINAL..class_377::FEATURE_ORDINAL + 4]
        .copy_from_slice(&1u32.to_le_bytes());
    bytes[class_377::PREVIOUS_HISTORY_STATE_ID..class_377::PREVIOUS_HISTORY_STATE_ID + 4]
        .copy_from_slice(&19u32.to_le_bytes());

    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#193",
        crate::records::feature::scope::DesignFeatureKind::BaseFeature,
        193,
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
        crate::records::references::DesignClassTag::try_from("377".to_owned()).unwrap();
    scope.paired_class_tag =
        crate::records::references::DesignClassTag::try_from("259".to_owned()).unwrap();
    scope
        .try_edit(|draft| {
            draft.paired_byte_offset = u64_from_index(class_377::LEN);
            draft.frame_length = u64_from_index(class_377::LEN);
            draft.kind_offset = u64_from_index(class_377::KIND);
            draft.reference_count_offset =
                draft.kind_offset - 12 - 11 * u64_from_index(draft.reference_members.len());
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    scope.feature_ordinal = std::num::NonZeroU32::new(1).expect("nonzero ordinal");
    scope
        .try_edit(|draft| {
            draft.feature_ordinal_offset = u64_from_index(class_377::FEATURE_ORDINAL);
            draft.history_state_id = Some(20);

            draft.previous_history_state_id = Some(19);
            draft.previous_history_state_id_offset =
                Some(u64_from_index(class_377::PREVIOUS_HISTORY_STATE_ID));
            draft.reference_count_offset = u64_from_index(class_377::REFERENCE_COUNT);
            draft.reference_members = crate::records::identity::ReferenceRun::from_columns(
                vec![196],
                vec![u64_from_index(class_377::GENERIC_SCOPE_REFERENCE_RECORD)],
                "reference_members",
            )
            .unwrap();
        })
        .unwrap();

    let construction = exact_base_feature_construction(&bytes, &scope)
        .unwrap()
        .expect("class-377/class-259 Base Feature frame is canonical");
    let DesignBaseFeatureConstruction::BodyBasedOnFaces {
        body,
        parameter_body_record,
        parameter_body_record_offset,
        auxiliary_record,
        auxiliary_record_offset,
        envelope_guid,
        envelope_guid_offset,
        tag_body_based_on_faces_offset,
        ..
    } = &construction
    else {
        panic!("class-377/class-259 frame selected the wrong form");
    };
    assert_eq!(
        *body,
        crate::records::identity::Located {
            value: 201,
            offset: u64_from_index(class_377::BODY_ENTITY_SUFFIX)
        }
    );
    assert_eq!(
        construction.body_reference_records().collect::<Vec<_>>(),
        [201]
    );
    assert_eq!(*parameter_body_record, 198);
    assert_eq!(
        *parameter_body_record_offset,
        u64_from_index(class_377::PARAMETER_BODY_RECORD)
    );
    assert_eq!(*auxiliary_record, 202);
    assert_eq!(
        *auxiliary_record_offset,
        u64_from_index(class_377::AUXILIARY_RECORD)
    );
    assert_eq!(
        envelope_guid.as_str(),
        "fcec56e3-832f-4468-88a4-d710e62e629f"
    );
    assert_eq!(
        *envelope_guid_offset,
        u64_from_index(class_377::ENVELOPE_GUID)
    );
    assert_eq!(
        *tag_body_based_on_faces_offset,
        u64_from_index(class_377::TAG_BODY_BASED_ON_FACES_VALUE)
    );

    let mut class_365_scope = scope.clone();
    class_365_scope.class_tag =
        crate::records::references::DesignClassTag::try_from("365".to_owned()).unwrap();
    class_365_scope.paired_class_tag =
        crate::records::references::DesignClassTag::try_from("262".to_owned()).unwrap();
    let class_365_construction = exact_base_feature_construction(&bytes, &class_365_scope)
        .unwrap()
        .expect("class-365/class-262 Base Feature frame is canonical");
    assert_eq!(class_365_construction, construction);

    let serialized = serde_json::to_value(&construction).expect("serialize body-reference form");
    assert_eq!(
        serde_json::from_value::<DesignBaseFeatureConstruction>(serialized)
            .expect("deserialize body-reference form"),
        construction
    );

    let mut nonzero_field = bytes.clone();
    nonzero_field[class_377::PARAMETER_REFERENCE_FIELD] = 1;
    assert!(exact_base_feature_construction(&nonzero_field, &scope)
        .unwrap()
        .is_none());

    let mut mismatched_previous = scope.clone();
    mismatched_previous
        .try_edit(|draft| {
            draft.previous_history_state_id = Some(18);
            draft.layout_fixture_tail();
        })
        .unwrap();
    assert!(
        exact_base_feature_construction(&bytes, &mismatched_previous)
            .unwrap()
            .is_none()
    );

    let mut mismatched_pair = scope;
    mismatched_pair.paired_class_tag =
        crate::records::references::DesignClassTag::try_from("263".to_owned()).unwrap();
    assert!(exact_base_feature_construction(&bytes, &mismatched_pair)
        .unwrap()
        .is_none());
}

#[test]
fn surface_patch_boundary_settings_decode_the_fixed_payload() {
    use crate::design::decode::patch::surface_patch_boundaries;
    use crate::records::feature::surface_ops::{DesignPatchContinuity, DesignSurfacePatchBoundary};

    let mut bytes = vec![0_u8; 49];
    bytes[0..4].copy_from_slice(&3_u32.to_le_bytes());
    bytes[4..7].copy_from_slice(b"999");
    bytes[7..11].copy_from_slice(&42_u32.to_le_bytes());
    bytes[21] = 1;
    bytes[22..26].copy_from_slice(&2_u32.to_le_bytes());
    bytes[26..30].copy_from_slice(&2_u32.to_le_bytes());
    bytes[30..38].copy_from_slice(&(-1.0_f64).to_le_bytes());
    bytes[38] = 1;
    bytes[39..43].copy_from_slice(&100_u32.to_le_bytes());

    crate::test_support::indexed_header(&mut bytes, *b"998", 42);
    let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
    let mut expected = DesignSurfacePatchBoundary {
        scope_reference_ordinal: 0,
        record_index: 42,
        is_seed_selection: true,
        continuity: DesignPatchContinuity::Curvature,
        flip: 2,
        scale: crate::test_support::real(-1.0),
        model_reference: 100,
    };
    assert_eq!(
        surface_patch_boundaries(&bytes, &records, &[42]),
        vec![expected.clone()]
    );

    bytes[26..30].copy_from_slice(&0_u32.to_le_bytes());
    expected.flip = 0;
    assert_eq!(
        surface_patch_boundaries(&bytes, &records, &[42]),
        vec![expected]
    );
}

#[test]
fn surface_patch_boundary_settings_reject_invalid_fixed_fields() {
    use crate::design::decode::patch::surface_patch_boundaries;

    let mut bytes = vec![0_u8; 49];
    bytes[0..4].copy_from_slice(&3_u32.to_le_bytes());
    bytes[4..7].copy_from_slice(b"999");
    bytes[7..11].copy_from_slice(&42_u32.to_le_bytes());
    bytes[22..26].copy_from_slice(&1_u32.to_le_bytes());
    bytes[26..30].copy_from_slice(&2_u32.to_le_bytes());
    bytes[30..38].copy_from_slice(&(-1.0_f64).to_le_bytes());
    bytes[38] = 1;
    bytes[39..43].copy_from_slice(&100_u32.to_le_bytes());

    crate::test_support::indexed_header(&mut bytes, *b"998", 42);
    let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
    bytes[21] = 2;
    assert!(surface_patch_boundaries(&bytes, &records, &[42]).is_empty());

    bytes[21] = 0;
    bytes[30..38].copy_from_slice(&f64::NAN.to_le_bytes());
    assert!(surface_patch_boundaries(&bytes, &records, &[42]).is_empty());

    bytes[30..38].copy_from_slice(&(-1.0_f64).to_le_bytes());
    bytes[38] = 0;
    assert!(surface_patch_boundaries(&bytes, &records, &[42]).is_empty());
}

mod base_feature_legacy;
