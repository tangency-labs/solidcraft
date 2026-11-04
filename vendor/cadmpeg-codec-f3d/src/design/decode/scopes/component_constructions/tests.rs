// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::u64_from_index;

use super::exact_component_insert_construction;
use crate::records::feature::scope::DesignParameterScope;
use crate::test_support::lp_utf16;

fn tested_component_insert_construction(
    bytes: &[u8],
    records: &crate::design::decode::sketch::IndexedRecordOffsets,
    scope: &DesignParameterScope,
) -> Option<crate::records::feature::assembly_features::DesignComponentInsertConstruction> {
    exact_component_insert_construction(bytes, records, scope).unwrap()
}

#[test]
fn component_insert_scope_joins_its_relation_carrier_role_and_transform() {
    run_component_insert_scope_fixture();
}

fn run_component_insert_scope_fixture() {
    let header = |bytes: &mut Vec<u8>, class_tag: &[u8; 3], record_index: u32| {
        bytes.extend_from_slice(&3_u32.to_le_bytes());
        bytes.extend_from_slice(class_tag);
        bytes.extend_from_slice(&record_index.to_le_bytes());
    };
    let transform: [[f64; 4]; 4] = [
        [1.0, 0.0, 0.0, -2.1],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let role = "b2231f72-46dc-40fa-b8e8-10cd208d7df8";
    let mut bytes = Vec::new();
    header(&mut bytes, b"256", 10);
    let role_at = bytes.len();
    bytes.extend_from_slice(&36_u32.to_le_bytes());
    bytes.extend(role.encode_utf16().flat_map(u16::to_le_bytes));
    bytes.extend_from_slice(&[0, 0]);
    let carrier_transform_at = bytes.len();
    for value in transform.into_iter().flatten() {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    let relation_at = bytes.len();
    header(&mut bytes, b"325", 20);
    bytes.extend_from_slice(&[0; 10]);
    for (ordinal, reference) in [10_u32, 11, 30].into_iter().enumerate() {
        bytes.push(1);
        bytes.extend_from_slice(&reference.to_le_bytes());
        bytes.extend(std::iter::repeat_n(0, [8, 7, 6][ordinal]));
    }
    assert_eq!(bytes.len(), relation_at + 57);
    header(&mut bytes, b"259", 20);
    let scope_at = bytes.len();
    bytes.resize(scope_at + 399, 0);
    bytes[scope_at..scope_at + 4].copy_from_slice(&3_u32.to_le_bytes());
    bytes[scope_at + 4..scope_at + 7].copy_from_slice(b"451");
    bytes[scope_at + 7..scope_at + 11].copy_from_slice(&30_u32.to_le_bytes());
    bytes[scope_at + 20] = 1;
    bytes[scope_at + 37] = 1;
    bytes[scope_at + 38..scope_at + 42].copy_from_slice(&20_u32.to_le_bytes());
    bytes[scope_at + 48] = 1;
    for (ordinal, value) in transform.into_iter().flatten().enumerate() {
        let at = scope_at + 50 + ordinal * 8;
        bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    header(&mut bytes, b"259", 30);
    let scope = DesignParameterScope::try_new(
        crate::records::feature::scope::DesignParameterScopeDraft {
            id: "f3d:Design/BulkStream.dat:design-parameter-scope#30".into(),
            byte_offset: u64_from_index(scope_at),
            class_tag: crate::records::references::DesignClassTag::try_from("451".to_owned())
                .unwrap(),
            record_index: 30,
            frame_length: 399,
            kind_offset: 0,
            feature_ordinal: std::num::NonZeroU32::MIN,
            feature_ordinal_offset: 0,
            history_state_id: None,

            previous_history_state_id: None,
            previous_history_state_id_offset: None,
            reference_count_offset: (u64_from_index(scope_at)) + 9,
            reference_members: crate::records::identity::ReferenceRun::from_columns(
                vec![20],
                vec![u64_from_index(scope_at) + 38],
                "reference_members",
            )
            .unwrap(),
            payload: crate::records::feature::scope::DesignFeatureKind::ComponentInsert
                .try_into()
                .unwrap(),
            unclosed_construction_operand_groups: Vec::new(),
            paired_class_tag: crate::records::references::DesignClassTag::try_from(
                "259".to_owned(),
            )
            .unwrap(),
            paired_byte_offset: u64_from_index(scope_at + 399),
        }
        .with_fixture_layout(),
    )
    .unwrap();

    let construction = tested_component_insert_construction(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        &scope,
    )
    .expect("component insert construction");

    assert_eq!(construction.relation_record_index, 20);
    assert_eq!(construction.carrier_record_index, 10);
    assert_eq!(construction.neutron_role, role);
    assert_eq!(
        construction.neutron_role_offset,
        u64_from_index(role_at + 4)
    );
    assert_eq!(*construction.transform(), transform.try_into().unwrap());
    assert_eq!(
        construction.transform_offset(),
        Some(u64_from_index(scope_at + 50))
    );
    assert_eq!(
        construction.carrier_transform_offset(),
        Some(u64_from_index(carrier_transform_at))
    );

    for (frame_length, paired_class_tag, transform_at, relation_at, expanded_prologue) in [
        (381_usize, "261", 49_usize, 38_usize, true),
        (395, "258", 46, 34, false),
    ] {
        let mut legacy = bytes[..scope_at].to_vec();
        legacy.resize(scope_at + frame_length, 0);
        legacy[scope_at..scope_at + 4].copy_from_slice(&3_u32.to_le_bytes());
        legacy[scope_at + 4..scope_at + 7].copy_from_slice(b"451");
        legacy[scope_at + 7..scope_at + 11].copy_from_slice(&30_u32.to_le_bytes());
        if expanded_prologue {
            legacy[scope_at + 20] = 1;
            legacy[scope_at + 37] = 1;
            legacy[scope_at + 48] = 1;
        } else {
            legacy[scope_at + 33] = 1;
        }
        legacy[scope_at + relation_at..scope_at + relation_at + 4]
            .copy_from_slice(&20_u32.to_le_bytes());
        if !expanded_prologue {
            legacy[scope_at + transform_at - 2] = 1;
        }
        for (ordinal, value) in transform.into_iter().flatten().enumerate() {
            let at = scope_at + transform_at + ordinal * 8;
            legacy[at..at + 8].copy_from_slice(&value.to_le_bytes());
        }
        header(
            &mut legacy,
            paired_class_tag
                .as_bytes()
                .try_into()
                .expect("three-byte tag"),
            30,
        );
        let legacy_scope = DesignParameterScope::try_new(
            crate::records::feature::scope::DesignParameterScopeDraft {
                frame_length: u64_from_index(frame_length),
                paired_class_tag: crate::records::references::DesignClassTag::try_from(
                    paired_class_tag.to_owned(),
                )
                .unwrap(),
                paired_byte_offset: u64_from_index(scope_at + frame_length),
                ..scope.clone().into_draft()
            }
            .with_fixture_layout(),
        )
        .unwrap();
        let construction = tested_component_insert_construction(
            &legacy,
            &crate::design::test_support::indexed_record_offsets_for_test(&legacy),
            &legacy_scope,
        )
        .unwrap_or_else(|| panic!("{frame_length}-byte component insert construction"));
        assert_eq!(
            construction.transform_offset(),
            Some(u64_from_index(scope_at + transform_at))
        );
        assert_eq!(*construction.transform(), transform.try_into().unwrap());
    }

    let mut expanded = Vec::new();
    header(&mut expanded, b"312", 10);
    let expanded_carrier_transform_at = expanded.len();
    for value in transform.into_iter().flatten() {
        expanded.extend_from_slice(&value.to_le_bytes());
    }
    let expanded_role_at = expanded.len();
    expanded.extend_from_slice(&36_u32.to_le_bytes());
    expanded.extend(role.encode_utf16().flat_map(u16::to_le_bytes));
    expanded.extend_from_slice(&[0, 1, 6, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    let expanded_relation_at = expanded.len();
    header(&mut expanded, b"338", 20);
    expanded.resize(expanded_relation_at + 58, 0);
    expanded[expanded_relation_at + 21] = 1;
    expanded[expanded_relation_at + 22..expanded_relation_at + 26]
        .copy_from_slice(&10_u32.to_le_bytes());
    expanded[expanded_relation_at + 32..expanded_relation_at + 35].copy_from_slice(&[1, 0, 0]);
    expanded[expanded_relation_at + 35] = 1;
    expanded[expanded_relation_at + 36..expanded_relation_at + 40]
        .copy_from_slice(&99_u32.to_le_bytes());
    expanded[expanded_relation_at + 47] = 1;
    expanded[expanded_relation_at + 48..expanded_relation_at + 52]
        .copy_from_slice(&30_u32.to_le_bytes());
    let expanded_scope_at = expanded.len();
    header(&mut expanded, b"335", 30);
    expanded.resize(expanded_scope_at + 404, 0);
    expanded[expanded_scope_at + 20] = 1;
    let occurrence_identity = 0x0102_0304_0506_0708_u64;
    expanded[expanded_scope_at + 29..expanded_scope_at + 37]
        .copy_from_slice(&occurrence_identity.to_le_bytes());
    expanded[expanded_scope_at + 41] = 1;
    expanded[expanded_scope_at + 42..expanded_scope_at + 46].copy_from_slice(&20_u32.to_le_bytes());
    expanded[expanded_scope_at + 52..expanded_scope_at + 54].copy_from_slice(&[1, 0]);
    for (ordinal, value) in transform.into_iter().flatten().enumerate() {
        let at = expanded_scope_at + 54 + ordinal * 8;
        expanded[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    header(&mut expanded, b"260", 30);
    let expanded_scope = DesignParameterScope::try_new(
        crate::records::feature::scope::DesignParameterScopeDraft {
            byte_offset: u64_from_index(expanded_scope_at),
            class_tag: crate::records::references::DesignClassTag::try_from("335".to_owned())
                .unwrap(),
            frame_length: 404,
            reference_members: crate::records::identity::ReferenceRun::located(vec![
                crate::records::identity::Located {
                    value: 20,
                    offset: u64_from_index(expanded_scope_at + 42),
                },
            ]),
            payload: scope.kind().try_into().unwrap(),
            paired_class_tag: crate::records::references::DesignClassTag::try_from(
                "260".to_owned(),
            )
            .unwrap(),
            paired_byte_offset: u64_from_index(expanded_scope_at + 404),
            ..scope.clone().into_draft()
        }
        .with_fixture_layout(),
    )
    .unwrap();
    let construction = tested_component_insert_construction(
        &expanded,
        &crate::design::test_support::indexed_record_offsets_for_test(&expanded),
        &expanded_scope,
    )
    .expect("404-byte component insert construction");
    assert_eq!(construction.relation_record_index, 20);
    assert_eq!(construction.carrier_record_index, 10);
    assert_eq!(construction.occurrence_identity, Some(occurrence_identity));
    assert_eq!(construction.neutron_role, role);
    assert_eq!(
        construction.neutron_role_offset,
        u64_from_index(expanded_role_at + 4)
    );
    assert_eq!(*construction.transform(), transform.try_into().unwrap());
    assert_eq!(
        construction.transform_offset(),
        Some(u64_from_index(expanded_scope_at + 54))
    );
    assert_eq!(
        construction.carrier_transform_offset(),
        Some(u64_from_index(expanded_carrier_transform_at))
    );

    let mut legacy = Vec::new();
    header(&mut legacy, b"288", 10);
    legacy.resize(30, 0);
    lp_utf16(&mut legacy, "95cc7c78-04aa-4ffc-a36d-a512f02e0dda");
    let legacy_role_at = legacy.len();
    lp_utf16(&mut legacy, role);
    legacy.extend_from_slice(&[1, 2, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0]);
    lp_utf16(&mut legacy, "96e2c767-721c-4c81-bbbc-8cc143d323fb");
    legacy.push(0);
    let asset_identity = "864a8a41-7ed8-4c94-8871-ee9e87ab7648_urn:asset";
    lp_utf16(&mut legacy, asset_identity);
    legacy.push(0);
    let legacy_carrier_transform_at = legacy.len();
    for value in transform.into_iter().flatten() {
        legacy.extend_from_slice(&value.to_le_bytes());
    }
    legacy.extend_from_slice(&[0; 4]);
    lp_utf16(&mut legacy, asset_identity);
    legacy.extend_from_slice(&[0, 1, 3, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    let legacy_relation_at = legacy.len();
    header(&mut legacy, b"325", 20);
    legacy.extend_from_slice(&[0; 10]);
    for (ordinal, reference) in [10_u32, 11, 30].into_iter().enumerate() {
        legacy.push(1);
        legacy.extend_from_slice(&reference.to_le_bytes());
        legacy.extend(std::iter::repeat_n(0, [8, 7, 6][ordinal]));
    }
    let legacy_scope_at = legacy.len();
    header(&mut legacy, b"346", 30);
    legacy.resize(legacy_scope_at + 381, 0);
    legacy[legacy_scope_at + 20] = 1;
    legacy[legacy_scope_at + 37] = 1;
    legacy[legacy_scope_at + 38..legacy_scope_at + 42].copy_from_slice(&20_u32.to_le_bytes());
    legacy[legacy_scope_at + 48] = 1;
    for (ordinal, value) in transform.into_iter().flatten().enumerate() {
        let at = legacy_scope_at + 49 + ordinal * 8;
        legacy[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    header(&mut legacy, b"261", 30);
    let legacy_scope = DesignParameterScope::try_new(
        crate::records::feature::scope::DesignParameterScopeDraft {
            byte_offset: u64_from_index(legacy_scope_at),
            reference_count_offset: u64_from_index(legacy_scope_at + 33),
            frame_length: 381,
            paired_class_tag: crate::records::references::DesignClassTag::try_from(
                "261".to_owned(),
            )
            .unwrap(),
            paired_byte_offset: u64_from_index(legacy_scope_at + 381),
            ..scope.clone().into_draft()
        }
        .with_fixture_layout(),
    )
    .unwrap();
    let construction = tested_component_insert_construction(
        &legacy,
        &crate::design::test_support::indexed_record_offsets_for_test(&legacy),
        &legacy_scope,
    )
    .expect("class-288 legacy component insert construction");
    assert_eq!(construction.neutron_role, role);
    assert_eq!(
        construction.neutron_role_offset,
        u64_from_index(legacy_role_at + 4)
    );
    assert_eq!(
        construction.carrier_transform_offset(),
        Some(u64_from_index(legacy_carrier_transform_at))
    );
    assert_eq!(construction.relation_record_index, 20);
    assert_eq!(legacy_relation_at + 57, legacy_scope_at);
}

#[test]
fn compact_component_insert_identity_form_joins_grouped_carrier() {
    let header = |bytes: &mut Vec<u8>, class_tag: &[u8; 3], record_index: u32| {
        bytes.extend_from_slice(&3_u32.to_le_bytes());
        bytes.extend_from_slice(class_tag);
        bytes.extend_from_slice(&record_index.to_le_bytes());
    };
    let push_ascii = |bytes: &mut Vec<u8>, value: &str| {
        bytes.extend_from_slice(
            &(u32::try_from(value.len()).expect("fixture value fits u32")).to_le_bytes(),
        );
        bytes.extend_from_slice(value.as_bytes());
    };
    let component_guid = "11111111-2222-3333-4444-555555555555";
    let type_guid = "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee";
    let metadata_guid_a = "66666666-7777-8888-9999-aaaaaaaaaaaa";
    let metadata_guid_b = "bbbbbbbb-cccc-dddd-eeee-ffffffffffff";
    let role = "cccccccc-dddd-eeee-ffff-000000000000";
    let mut bytes = Vec::new();
    header(&mut bytes, b"382", 10);
    bytes.extend_from_slice(&[0; 8]);
    bytes.push(1);
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&17_u64.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&[0; 4]);
    lp_utf16(&mut bytes, component_guid);
    bytes.push(0);
    push_ascii(&mut bytes, type_guid);
    lp_utf16(&mut bytes, role);
    bytes.extend_from_slice(&[0, 1, 0, 0, 0, 0, 1, 0, 0, 0]);
    lp_utf16(&mut bytes, metadata_guid_a);
    lp_utf16(&mut bytes, metadata_guid_b);
    bytes.extend_from_slice(&[0, 1, 3, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0]);
    lp_utf16(&mut bytes, component_guid);
    bytes.push(0);
    push_ascii(&mut bytes, type_guid);
    lp_utf16(&mut bytes, role);
    bytes.extend_from_slice(&[0, 1, 0, 0, 0, 0]);
    lp_utf16(&mut bytes, role);
    bytes.extend_from_slice(&[0, 1, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    assert_eq!(bytes.len(), 695);

    let relation_at = bytes.len();
    header(&mut bytes, b"399", 20);
    bytes.extend_from_slice(&[0; 10]);
    bytes.push(1);
    bytes.extend_from_slice(&10_u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 8]);
    bytes.push(1);
    bytes.extend_from_slice(&99_u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 7]);
    bytes.push(1);
    bytes.extend_from_slice(&30_u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    assert_eq!(bytes.len(), relation_at + 57);
    header(&mut bytes, b"263", 20);

    let scope_at = bytes.len();
    header(&mut bytes, b"296", 30);
    bytes.resize(scope_at + 261, 0);
    bytes[scope_at + 20] = 1;
    bytes[scope_at + 25..scope_at + 33].copy_from_slice(&17_u64.to_le_bytes());
    bytes[scope_at + 37] = 1;
    bytes[scope_at + 38..scope_at + 42].copy_from_slice(&20_u32.to_le_bytes());
    bytes[scope_at + 48..scope_at + 50].copy_from_slice(&[1, 1]);
    bytes[scope_at + 50..scope_at + 54].copy_from_slice(&36_u32.to_le_bytes());
    bytes[scope_at + 54..scope_at + 126].copy_from_slice(
        &"00000000-0000-0000-0000-000000000000"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>(),
    );
    header(&mut bytes, b"263", 30);

    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#30",
        crate::records::feature::scope::DesignFeatureKind::ComponentInsert,
        30,
    );
    scope
        .try_edit(|draft| {
            draft.byte_offset = u64_from_index(scope_at);
            draft.reference_count_offset = draft.byte_offset + 9;
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    scope.class_tag =
        crate::records::references::DesignClassTag::try_from("296".to_owned()).unwrap();
    scope
        .try_edit(|draft| {
            draft.frame_length = 261;
            draft.reference_members = crate::records::identity::ReferenceRun::from_columns(
                vec![20],
                vec![u64_from_index(scope_at + 38)],
                "reference_members",
            )
            .unwrap();
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.reference_count_offset = *draft.reference_members.offsets().next().unwrap() - 5;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    scope.paired_class_tag =
        crate::records::references::DesignClassTag::try_from("263".to_owned()).unwrap();
    scope
        .try_edit(|draft| {
            draft.paired_byte_offset = u64_from_index(scope_at + 261);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();

    let construction = tested_component_insert_construction(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        &scope,
    )
    .expect("compact identity component insert construction");
    assert_eq!(construction.relation_record_index, 20);
    assert_eq!(construction.carrier_record_index, 10);
    assert_eq!(construction.occurrence_identity, Some(17));
    assert_eq!(construction.neutron_role, role);
    assert_eq!(construction.neutron_role_offset, 159);
    assert_eq!(
        *construction.transform(),
        crate::records::sketch_placement::SketchPlacementMatrix::IDENTITY
    );
    assert_eq!(construction.transform_offset(), None);
    assert_eq!(construction.carrier_transform_offset(), None);
}

#[test]
fn class_410_component_insert_identity_form_joins_class_380_carrier() {
    let header = |bytes: &mut Vec<u8>, class_tag: &[u8; 3], record_index: u32| {
        bytes.extend_from_slice(&3_u32.to_le_bytes());
        bytes.extend_from_slice(class_tag);
        bytes.extend_from_slice(&record_index.to_le_bytes());
    };
    let push_ascii = |bytes: &mut Vec<u8>, value: &str| {
        bytes.extend_from_slice(
            &(u32::try_from(value.len()).expect("fixture value fits u32")).to_le_bytes(),
        );
        bytes.extend_from_slice(value.as_bytes());
    };
    let component_guid = "11111111-2222-3333-4444-555555555555";
    let type_guid = "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee";
    let metadata_guid_a = "66666666-7777-8888-9999-aaaaaaaaaaaa";
    let metadata_guid_b = "bbbbbbbb-cccc-dddd-eeee-ffffffffffff";
    let role = "cccccccc-dddd-eeee-ffff-000000000000";
    let mut bytes = Vec::new();
    header(&mut bytes, b"380", 166);
    bytes.extend_from_slice(&[0; 8]);
    bytes.push(1);
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&17_u64.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&[0; 4]);
    lp_utf16(&mut bytes, component_guid);
    bytes.push(0);
    push_ascii(&mut bytes, type_guid);
    lp_utf16(&mut bytes, role);
    bytes.extend_from_slice(&[0, 1, 0, 0, 0, 0, 1, 0, 0, 0]);
    lp_utf16(&mut bytes, metadata_guid_a);
    lp_utf16(&mut bytes, metadata_guid_b);
    bytes.extend_from_slice(&[0, 1, 3, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0]);
    lp_utf16(&mut bytes, component_guid);
    bytes.push(0);
    push_ascii(&mut bytes, type_guid);
    lp_utf16(&mut bytes, role);
    bytes.extend_from_slice(&[0, 1, 0, 0, 0, 0]);
    lp_utf16(&mut bytes, role);
    bytes.extend_from_slice(&[0, 1, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    assert_eq!(bytes.len(), 695);

    let relation_at = bytes.len();
    header(&mut bytes, b"310", 167);
    bytes.extend_from_slice(&[0; 10]);
    for (ordinal, reference) in [166_u32, 168, 169].into_iter().enumerate() {
        bytes.push(1);
        bytes.extend_from_slice(&reference.to_le_bytes());
        bytes.extend(std::iter::repeat_n(0, [8, 7, 6][ordinal]));
    }
    assert_eq!(bytes.len(), relation_at + 57);
    header(&mut bytes, b"261", 167);

    let scope_at = bytes.len();
    header(&mut bytes, b"410", 169);
    bytes.resize(scope_at + 261, 0);
    bytes[scope_at + 20] = 1;
    bytes[scope_at + 25..scope_at + 33].copy_from_slice(&17_u64.to_le_bytes());
    bytes[scope_at + 37] = 1;
    bytes[scope_at + 38..scope_at + 42].copy_from_slice(&167_u32.to_le_bytes());
    bytes[scope_at + 48..scope_at + 50].copy_from_slice(&[1, 1]);
    bytes[scope_at + 50..scope_at + 54].copy_from_slice(&36_u32.to_le_bytes());
    bytes[scope_at + 54..scope_at + 126].copy_from_slice(
        &"00000000-0000-0000-0000-000000000000"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>(),
    );
    header(&mut bytes, b"261", 169);

    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#169",
        crate::records::feature::scope::DesignFeatureKind::ComponentInsert,
        169,
    );
    scope
        .try_edit(|draft| {
            draft.byte_offset = u64_from_index(scope_at);
            draft.reference_count_offset = draft.byte_offset + 9;
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    scope.class_tag =
        crate::records::references::DesignClassTag::try_from("410".to_owned()).unwrap();
    scope
        .try_edit(|draft| {
            draft.frame_length = 261;
            draft.reference_members = crate::records::identity::ReferenceRun::from_columns(
                vec![167],
                vec![u64_from_index(scope_at + 38)],
                "reference_members",
            )
            .unwrap();
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.reference_count_offset = *draft.reference_members.offsets().next().unwrap() - 5;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    scope.paired_class_tag =
        crate::records::references::DesignClassTag::try_from("261".to_owned()).unwrap();
    scope
        .try_edit(|draft| {
            draft.paired_byte_offset = u64_from_index(scope_at + 261);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();

    let construction = tested_component_insert_construction(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        &scope,
    )
    .expect("class-410 component insert construction");
    assert_eq!(construction.relation_record_index, 167);
    assert_eq!(construction.carrier_record_index, 166);
    assert_eq!(construction.occurrence_identity, Some(17));
    assert_eq!(construction.neutron_role, role);
    assert_eq!(construction.neutron_role_offset, 159);
    assert_eq!(
        *construction.transform(),
        crate::records::sketch_placement::SketchPlacementMatrix::IDENTITY
    );
    assert_eq!(construction.transform_offset(), None);
    assert_eq!(construction.carrier_transform_offset(), None);

    bytes[4..7].copy_from_slice(b"382");
    assert!(tested_component_insert_construction(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        &scope
    )
    .is_none());
}

#[test]
fn class_434_component_insert_identity_form_joins_variable_role_class_341_carrier() {
    let header = |bytes: &mut Vec<u8>, class_tag: &[u8; 3], record_index: u32| {
        bytes.extend_from_slice(&3_u32.to_le_bytes());
        bytes.extend_from_slice(class_tag);
        bytes.extend_from_slice(&record_index.to_le_bytes());
    };
    let push_ascii = |bytes: &mut Vec<u8>, value: &str| {
        bytes.extend_from_slice(
            &(u32::try_from(value.len()).expect("fixture value fits u32")).to_le_bytes(),
        );
        bytes.extend_from_slice(value.as_bytes());
    };
    let component_guid = "11111111-2222-3333-4444-555555555555";
    let type_guid = "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee";
    let metadata_guid_a = "66666666-7777-8888-9999-aaaaaaaaaaaa";
    let metadata_guid_b = "bbbbbbbb-cccc-dddd-eeee-ffffffffffff";
    let role = "cccccccc-dddd-eeee-ffff-000000000000_urn:test";
    let mut bytes = Vec::new();
    header(&mut bytes, b"341", 166);
    bytes.extend_from_slice(&[0; 8]);
    bytes.push(1);
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&17_u64.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&[0; 4]);
    lp_utf16(&mut bytes, component_guid);
    bytes.push(0);
    push_ascii(&mut bytes, type_guid);
    lp_utf16(&mut bytes, role);
    bytes.extend_from_slice(&[0, 1, 0, 0, 0, 0, 1, 0, 0, 0]);
    lp_utf16(&mut bytes, metadata_guid_a);
    lp_utf16(&mut bytes, metadata_guid_b);
    bytes.extend_from_slice(&[0, 1]);
    bytes.extend_from_slice(&17_u64.to_le_bytes());
    bytes.extend_from_slice(&[1, 0, 0, 0, 0]);
    lp_utf16(&mut bytes, component_guid);
    bytes.push(0);
    push_ascii(&mut bytes, type_guid);
    lp_utf16(&mut bytes, role);
    bytes.extend_from_slice(&[0, 1, 0, 0, 0, 0]);
    lp_utf16(&mut bytes, role);
    bytes.extend_from_slice(&[0, 1, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0]);

    let relation_at = bytes.len();
    header(&mut bytes, b"348", 167);
    bytes.extend_from_slice(&[0; 10]);
    for (ordinal, reference) in [166_u32, 168, 169].into_iter().enumerate() {
        bytes.push(1);
        bytes.extend_from_slice(&reference.to_le_bytes());
        bytes.extend(std::iter::repeat_n(0, [8, 7, 6][ordinal]));
    }
    assert_eq!(bytes.len(), relation_at + 57);
    header(&mut bytes, b"266", 167);

    let scope_at = bytes.len();
    header(&mut bytes, b"434", 169);
    bytes.resize(scope_at + 261, 0);
    bytes[scope_at + 20] = 1;
    bytes[scope_at + 25..scope_at + 33].copy_from_slice(&17_u64.to_le_bytes());
    bytes[scope_at + 37] = 1;
    bytes[scope_at + 38..scope_at + 42].copy_from_slice(&167_u32.to_le_bytes());
    bytes[scope_at + 48..scope_at + 50].copy_from_slice(&[1, 1]);
    bytes[scope_at + 50..scope_at + 54].copy_from_slice(&36_u32.to_le_bytes());
    bytes[scope_at + 54..scope_at + 126].copy_from_slice(
        &"00000000-0000-0000-0000-000000000000"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>(),
    );
    header(&mut bytes, b"266", 169);

    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#169",
        crate::records::feature::scope::DesignFeatureKind::ComponentInsert,
        169,
    );
    scope
        .try_edit(|draft| {
            draft.byte_offset = u64_from_index(scope_at);
            draft.reference_count_offset = draft.byte_offset + 9;
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    scope.class_tag =
        crate::records::references::DesignClassTag::try_from("434".to_owned()).unwrap();
    scope
        .try_edit(|draft| {
            draft.frame_length = 261;
            draft.reference_members = crate::records::identity::ReferenceRun::from_columns(
                vec![167],
                vec![u64_from_index(scope_at + 38)],
                "reference_members",
            )
            .unwrap();
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.reference_count_offset = *draft.reference_members.offsets().next().unwrap() - 5;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    scope.paired_class_tag =
        crate::records::references::DesignClassTag::try_from("266".to_owned()).unwrap();
    scope
        .try_edit(|draft| {
            draft.paired_byte_offset = u64_from_index(scope_at + 261);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();

    let construction = tested_component_insert_construction(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        &scope,
    )
    .expect("class-434 component insert construction");
    assert_eq!(construction.relation_record_index, 167);
    assert_eq!(construction.carrier_record_index, 166);
    assert_eq!(construction.occurrence_identity, Some(17));
    assert_eq!(construction.neutron_role, role);
    assert_eq!(construction.neutron_role_offset, 159);
    assert_eq!(
        *construction.transform(),
        crate::records::sketch_placement::SketchPlacementMatrix::IDENTITY
    );
    assert_eq!(construction.transform_offset(), None);
    assert_eq!(construction.carrier_transform_offset(), None);
}

#[test]
fn class_426_component_insert_joins_legacy_relation_and_class_369_carrier() {
    let header = |bytes: &mut Vec<u8>, class_tag: &[u8; 3], record_index: u32| {
        bytes.extend_from_slice(&3_u32.to_le_bytes());
        bytes.extend_from_slice(class_tag);
        bytes.extend_from_slice(&record_index.to_le_bytes());
    };
    let push_ascii = |bytes: &mut Vec<u8>, value: &str| {
        bytes.extend_from_slice(
            &(u32::try_from(value.len()).expect("fixture value fits u32")).to_le_bytes(),
        );
        bytes.extend_from_slice(value.as_bytes());
    };
    let component_guid = "11111111-2222-3333-4444-555555555555";
    let type_guid = "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee";
    let metadata_guid_a = "66666666-7777-8888-9999-aaaaaaaaaaaa";
    let metadata_guid_b = "bbbbbbbb-cccc-dddd-eeee-ffffffffffff";
    let role = "cccccccc-dddd-eeee-ffff-000000000000";
    let mut bytes = Vec::new();
    header(&mut bytes, b"369", 10);
    bytes.extend_from_slice(&[0; 8]);
    bytes.push(1);
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&17_u64.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&[0; 4]);
    lp_utf16(&mut bytes, component_guid);
    bytes.push(0);
    push_ascii(&mut bytes, type_guid);
    lp_utf16(&mut bytes, role);
    bytes.extend_from_slice(&[0, 3, 0, 0, 0, 0, 1, 0, 0, 0]);
    lp_utf16(&mut bytes, metadata_guid_a);
    lp_utf16(&mut bytes, metadata_guid_b);
    bytes.extend_from_slice(&[0, 1, 3, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0]);
    lp_utf16(&mut bytes, component_guid);
    bytes.push(0);
    push_ascii(&mut bytes, type_guid);
    lp_utf16(&mut bytes, role);
    bytes.extend_from_slice(&[0, 1, 0, 0, 0, 0]);
    lp_utf16(&mut bytes, role);
    bytes.extend_from_slice(&[0, 1, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    assert_eq!(bytes.len(), 695);

    let relation_at = bytes.len();
    header(&mut bytes, b"345", 20);
    bytes.extend_from_slice(&[0; 10]);
    bytes.push(1);
    bytes.extend_from_slice(&10_u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 8]);
    bytes.push(1);
    bytes.extend_from_slice(&21_u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 7]);
    bytes.push(1);
    bytes.extend_from_slice(&30_u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    assert_eq!(bytes.len(), relation_at + 57);
    header(&mut bytes, b"258", 20);
    bytes.extend_from_slice(&[0; 19]);

    let child_at = bytes.len();
    header(&mut bytes, b"393", 21);
    bytes.extend_from_slice(&[0; 20]);
    bytes.push(1);
    bytes.extend_from_slice(&20_u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    bytes.extend_from_slice(&42_u64.to_le_bytes());
    bytes.extend_from_slice(&[0; 8]);
    assert_eq!(bytes.len(), child_at + 58);

    let scope_at = bytes.len();
    header(&mut bytes, b"426", 30);
    bytes.resize(scope_at + 261, 0);
    bytes[scope_at + 20] = 1;
    bytes[scope_at + 25..scope_at + 33].copy_from_slice(&17_u64.to_le_bytes());
    bytes[scope_at + 37] = 1;
    bytes[scope_at + 38..scope_at + 42].copy_from_slice(&20_u32.to_le_bytes());
    bytes[scope_at + 48..scope_at + 50].copy_from_slice(&[1, 1]);
    bytes[scope_at + 50..scope_at + 54].copy_from_slice(&36_u32.to_le_bytes());
    bytes[scope_at + 54..scope_at + 126].copy_from_slice(
        &"00000000-0000-0000-0000-000000000000"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>(),
    );
    header(&mut bytes, b"258", 30);

    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#30",
        crate::records::feature::scope::DesignFeatureKind::ComponentInsert,
        30,
    );
    scope
        .try_edit(|draft| {
            draft.byte_offset = u64_from_index(scope_at);
            draft.reference_count_offset = draft.byte_offset + 9;
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    scope.class_tag =
        crate::records::references::DesignClassTag::try_from("426".to_owned()).unwrap();
    scope
        .try_edit(|draft| {
            draft.frame_length = 261;
            draft.reference_members = crate::records::identity::ReferenceRun::from_columns(
                vec![20],
                vec![u64_from_index(scope_at + 38)],
                "reference_members",
            )
            .unwrap();
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.reference_count_offset = *draft.reference_members.offsets().next().unwrap() - 5;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    scope.paired_class_tag =
        crate::records::references::DesignClassTag::try_from("258".to_owned()).unwrap();
    scope
        .try_edit(|draft| {
            draft.paired_byte_offset = u64_from_index(scope_at + 261);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();

    let construction = tested_component_insert_construction(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        &scope,
    )
    .expect("class-426 component insert construction");
    assert_eq!(construction.relation_record_index, 20);
    assert_eq!(construction.carrier_record_index, 10);
    assert_eq!(construction.occurrence_identity, Some(17));
    assert_eq!(construction.neutron_role, role);
    assert_eq!(construction.neutron_role_offset, 159);
    assert_eq!(
        *construction.transform(),
        crate::records::sketch_placement::SketchPlacementMatrix::IDENTITY
    );
    assert_eq!(construction.transform_offset(), None);
    assert_eq!(construction.carrier_transform_offset(), None);

    let external_role = "cccccccc-dddd-eeee-ffff-000000000000_urn:adsk.test:asset";
    let mut external_bytes = bytes[..155].to_vec();
    external_bytes.extend_from_slice(
        &crate::bytes::lp_utf16_bytes(external_role)
            .expect("fixture UTF-16 code-unit count fits u32"),
    );
    external_bytes.extend_from_slice(&[0, 4, 0, 0, 0, 0, 1, 0, 0, 0]);
    external_bytes.extend_from_slice(&bytes[241..525]);
    external_bytes.extend_from_slice(
        &crate::bytes::lp_utf16_bytes(external_role)
            .expect("fixture UTF-16 code-unit count fits u32"),
    );
    external_bytes.extend_from_slice(&bytes[601..607]);
    external_bytes.extend_from_slice(
        &crate::bytes::lp_utf16_bytes(external_role)
            .expect("fixture UTF-16 code-unit count fits u32"),
    );
    external_bytes.extend_from_slice(&bytes[683..695]);
    let carrier_shift = external_bytes.len() - 695;
    external_bytes.extend_from_slice(&bytes[695..]);
    let external_scope_at = scope_at + carrier_shift;
    let mut external_scope = scope.clone();
    external_scope
        .try_edit(|draft| {
            draft.byte_offset = u64_from_index(external_scope_at);
            draft.reference_members = crate::records::identity::ReferenceRun::from_columns(
                draft.reference_members.values().copied().collect(),
                vec![u64_from_index(external_scope_at + 38)],
                "reference_members",
            )
            .unwrap();
            draft.paired_byte_offset = u64_from_index(external_scope_at + 261);
            draft.reference_count_offset = draft.byte_offset + 9;
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.reference_count_offset = *draft.reference_members.offsets().next().unwrap() - 5;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    let external_construction = tested_component_insert_construction(
        &external_bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&external_bytes),
        &external_scope,
    )
    .expect("class-426 external-role component insert construction");
    assert_eq!(external_construction.neutron_role, external_role);
    assert_eq!(external_construction.neutron_role_offset, 159);

    bytes[4..7].copy_from_slice(b"380");
    assert!(tested_component_insert_construction(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        &scope
    )
    .is_none());
}

#[test]
fn class_283_component_insert_admits_compact_and_transformed_scopes() {
    let header = |bytes: &mut Vec<u8>, class_tag: &[u8; 3], record_index: u32| {
        bytes.extend_from_slice(&3_u32.to_le_bytes());
        bytes.extend_from_slice(class_tag);
        bytes.extend_from_slice(&record_index.to_le_bytes());
    };
    let role = "b2231f72-46dc-40fa-b8e8-10cd208d7df8_urn:adsk.test:asset";
    let null_guid = "00000000-0000-0000-0000-000000000000";
    let component_guid = "11111111-2222-3333-4444-555555555555";

    let make_fixture = |frame_length: usize, transform: [[f64; 4]; 4]| {
        let mut bytes = Vec::new();
        let carrier_at = bytes.len();
        header(&mut bytes, b"334", 10);
        bytes.resize(
            carrier_at + crate::layout::component_insert_carrier_334_prefix::NEUTRON_ROLE,
            0,
        );
        bytes[carrier_at + crate::layout::component_insert_carrier_334_prefix::COMPONENT_IDENTITY
            ..carrier_at
                + crate::layout::component_insert_carrier_334_prefix::COMPONENT_IDENTITY
                + 76]
            .copy_from_slice(
                &crate::bytes::lp_utf16_bytes(component_guid)
                    .expect("fixture UTF-16 code-unit count fits u32"),
            );
        let role_at = carrier_at + crate::layout::component_insert_carrier_334_prefix::NEUTRON_ROLE;
        bytes.extend(role.encode_utf16().flat_map(u16::to_le_bytes));
        bytes.extend_from_slice(&[0, 0x21, 0, 0, 0, 0, 1, 0, 0, 0]);
        bytes.extend_from_slice(
            &crate::bytes::lp_utf16_bytes(component_guid)
                .expect("fixture UTF-16 code-unit count fits u32"),
        );
        assert_eq!(
            role_at
                + role.encode_utf16().count() * 2
                + 10
                + crate::bytes::lp_utf16_bytes(component_guid)
                    .expect("fixture UTF-16 code-unit count fits u32")
                    .len(),
            bytes.len()
        );

        let relation_at = bytes.len();
        header(&mut bytes, b"365", 20);
        bytes.extend_from_slice(&[0; 10]);
        for (reference, zero_count) in [(10_u32, 8), (99, 7), (30, 6)] {
            bytes.push(1);
            bytes.extend_from_slice(&reference.to_le_bytes());
            bytes.extend(std::iter::repeat_n(0, zero_count));
        }
        assert_eq!(bytes.len(), relation_at + 57);
        header(&mut bytes, b"262", 20);

        let scope_at = bytes.len();
        header(&mut bytes, b"283", 30);
        bytes.resize(scope_at + frame_length, 0);
        bytes[scope_at + 21..scope_at + 29].copy_from_slice(&17_u64.to_le_bytes());
        bytes[scope_at + 33] = 1;
        bytes[scope_at + 34..scope_at + 38].copy_from_slice(&20_u32.to_le_bytes());
        if frame_length == 257 {
            bytes[scope_at + 44..scope_at + 46].copy_from_slice(&[1, 1]);
            bytes[scope_at + 46..scope_at + 122].copy_from_slice(
                &crate::bytes::lp_utf16_bytes(null_guid)
                    .expect("fixture UTF-16 code-unit count fits u32"),
            );
            bytes[scope_at + 125..scope_at + 129].copy_from_slice(&1_u32.to_le_bytes());
            bytes[scope_at + 129] = 1;
            bytes[scope_at + 130..scope_at + 134].copy_from_slice(&20_u32.to_le_bytes());
            bytes[scope_at + 140..scope_at + 144].copy_from_slice(&u32::MAX.to_le_bytes());
            bytes[scope_at + 211..scope_at + 215].copy_from_slice(&u32::MAX.to_le_bytes());
        } else {
            bytes[scope_at + 44..scope_at + 46].copy_from_slice(&[1, 0]);
            for (ordinal, value) in transform.into_iter().flatten().enumerate() {
                let at = scope_at + 46 + ordinal * 8;
                bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
            }
            bytes[scope_at + 174..scope_at + 250].copy_from_slice(
                &crate::bytes::lp_utf16_bytes(null_guid)
                    .expect("fixture UTF-16 code-unit count fits u32"),
            );
            bytes[scope_at + 253..scope_at + 257].copy_from_slice(&1_u32.to_le_bytes());
            bytes[scope_at + 257] = 1;
            bytes[scope_at + 258..scope_at + 262].copy_from_slice(&20_u32.to_le_bytes());
            bytes[scope_at + 268..scope_at + 272].copy_from_slice(&u32::MAX.to_le_bytes());
            bytes[scope_at + 339..scope_at + 343].copy_from_slice(&u32::MAX.to_le_bytes());
        }
        header(&mut bytes, b"262", 30);

        let mut scope = DesignParameterScope::empty(
            "f3d:Design/BulkStream.dat:design-parameter-scope#30",
            crate::records::feature::scope::DesignFeatureKind::ComponentInsert,
            30,
        );
        scope
            .try_edit(|draft| {
                draft.byte_offset = u64_from_index(scope_at);
                draft.reference_count_offset = draft.byte_offset + 9;
                draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
                draft.layout_fixture_references();
                draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
                draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
                draft.layout_fixture_tail();
            })
            .unwrap();
        scope.class_tag =
            crate::records::references::DesignClassTag::try_from("283".to_owned()).unwrap();
        scope
            .try_edit(|draft| {
                draft.frame_length = u64_from_index(frame_length);
                draft.reference_members = crate::records::identity::ReferenceRun::from_columns(
                    vec![20],
                    vec![u64_from_index(scope_at + 34)],
                    "reference_members",
                )
                .unwrap();
                draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
                draft.reference_count_offset =
                    *draft.reference_members.offsets().next().unwrap() - 5;
                draft.layout_fixture_references();
                draft.layout_fixture_tail();
            })
            .unwrap();
        scope.paired_class_tag =
            crate::records::references::DesignClassTag::try_from("262".to_owned()).unwrap();
        scope
            .try_edit(|draft| {
                draft.paired_byte_offset = u64_from_index(scope_at + frame_length);
                draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
                draft.layout_fixture_tail();
            })
            .unwrap();
        (bytes, scope, scope_at)
    };

    let identity = crate::records::sketch_placement::SketchPlacementMatrix::IDENTITY.rows();
    let (bytes, scope, _) = make_fixture(257, identity);
    let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
    let construction = tested_component_insert_construction(&bytes, &records, &scope)
        .expect("class-283 compact component insert construction");
    assert_eq!(construction.carrier_record_index, 10);
    assert_eq!(construction.occurrence_identity, Some(17));
    assert_eq!(construction.neutron_role, role);
    assert_eq!(
        construction.neutron_role_offset,
        u64_from_index(crate::layout::component_insert_carrier_334_prefix::NEUTRON_ROLE)
    );
    assert_eq!(*construction.transform(), identity.try_into().unwrap());
    assert_eq!(construction.transform_offset(), None);
    assert_eq!(construction.carrier_transform_offset(), None);

    let transformed = [
        [1.0, 0.0, 0.0, -2.1],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let (bytes, scope, scope_at) = make_fixture(385, transformed);
    let construction = tested_component_insert_construction(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        &scope,
    )
    .expect("class-283 transformed component insert construction");
    assert_eq!(construction.occurrence_identity, Some(17));
    assert_eq!(construction.neutron_role, role);
    assert_eq!(
        construction.neutron_role_offset,
        u64_from_index(crate::layout::component_insert_carrier_334_prefix::NEUTRON_ROLE)
    );
    assert_eq!(*construction.transform(), transformed.try_into().unwrap());
    assert_eq!(
        construction.transform_offset(),
        Some(u64_from_index(scope_at + 46))
    );
    assert_eq!(construction.carrier_transform_offset(), None);
}

#[test]
fn class_414_component_insert_admits_shifted_identity_and_matrix_prologues() {
    let relation_record_index = 20_u32;
    let occurrence_identity = 17_u64;
    let null_guid = crate::bytes::lp_utf16_bytes("00000000-0000-0000-0000-000000000000")
        .expect("fixture UTF-16 code-unit count fits u32");

    let mut identity = vec![0_u8; 267];
    identity[21..29].copy_from_slice(&occurrence_identity.to_le_bytes());
    identity[33] = 1;
    identity[34..38].copy_from_slice(&relation_record_index.to_le_bytes());
    identity[44..46].copy_from_slice(&[1, 1]);
    identity[46..122].copy_from_slice(&null_guid);
    assert_eq!(
        super::exact_component_insert_identity_scope_shifted(&identity, 0, relation_record_index,),
        Some(occurrence_identity)
    );

    let transform: [[f64; 4]; 4] = [
        [1.0, 0.0, 0.0, 4.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let mut matrix = vec![0_u8; 389];
    matrix[20] = 1;
    matrix[25..33].copy_from_slice(&occurrence_identity.to_le_bytes());
    matrix[37] = 1;
    matrix[38..42].copy_from_slice(&relation_record_index.to_le_bytes());
    matrix[48..50].copy_from_slice(&[1, 0]);
    for (ordinal, value) in transform.into_iter().flatten().enumerate() {
        let at = 50 + ordinal * 8;
        matrix[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    matrix[178..254].copy_from_slice(&null_guid);
    assert_eq!(
        super::exact_component_insert_scope_414_264_389(&matrix, 0, relation_record_index,),
        Some((transform.try_into().unwrap(), Some(50), occurrence_identity))
    );
}

mod copy_paste;

#[test]
fn identity_insert_prologues_validate_identity_relation_and_null_guid() {
    for shifted in [false, true] {
        let delta = if shifted { 4 } else { 0 };
        let mut bytes = vec![0; 126 + delta];
        if shifted {
            bytes[20] = 1;
        }
        bytes[25 + delta..33 + delta].copy_from_slice(&17_u64.to_le_bytes());
        bytes[37 + delta] = 1;
        bytes[38 + delta..42 + delta].copy_from_slice(&20_u32.to_le_bytes());
        bytes[48 + delta..50 + delta].copy_from_slice(&[1, 1]);
        bytes[50 + delta..].copy_from_slice(
            &crate::bytes::lp_utf16_bytes("00000000-0000-0000-0000-000000000000").unwrap(),
        );
        assert_eq!(
            super::exact_identity_insert_scope(&bytes, 0, 20, shifted),
            Some(17)
        );
        assert!(super::exact_identity_insert_scope(&bytes, 0, 21, shifted).is_none());
        for offset in [
            11,
            33 + delta,
            37 + delta,
            42 + delta,
            48 + delta,
            54 + delta,
        ] {
            let saved = bytes[offset];
            bytes[offset] ^= 2;
            assert!(super::exact_identity_insert_scope(&bytes, 0, 20, shifted).is_none());
            bytes[offset] = saved;
        }
        for end in 0..bytes.len() {
            assert!(super::exact_identity_insert_scope(&bytes[..end], 0, 20, shifted).is_none());
        }
    }
}

#[test]
fn identity_insert_scopes_join_relation_and_external_carrier() {
    let role = "aaaabbbb-cccc-dddd-eeee-ffff00001111";
    for (class, frame) in [("357", 272), ("357", 276), ("360", 276), ("362", 276)] {
        let delta = usize::from(frame == 276) * 4;
        let mut bytes =
            crate::test_support::component_insert_test::identity_insert_carrier(role, 10);
        let relation_at = bytes.len();
        let header = |bytes: &mut Vec<u8>, class: &str, record: u32| {
            bytes.extend_from_slice(&3_u32.to_le_bytes());
            bytes.extend_from_slice(class.as_bytes());
            bytes.extend_from_slice(&record.to_le_bytes());
        };
        header(&mut bytes, "343", 20);
        bytes.extend_from_slice(&[0; 10]);
        bytes.push(1);
        bytes.extend_from_slice(&10_u32.to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
        bytes.extend_from_slice(&[1, 0, 0, 1]);
        bytes.extend_from_slice(&21_u32.to_le_bytes());
        bytes.extend_from_slice(&[0; 7]);
        bytes.push(1);
        bytes.extend_from_slice(&30_u32.to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
        assert_eq!(bytes.len() - relation_at, 58);
        header(&mut bytes, "262", 20);
        let start = bytes.len();
        header(&mut bytes, class, 30);
        bytes.resize(start + frame, 0);
        if delta != 0 {
            bytes[start + 20] = 1;
        }
        bytes[start + 25 + delta..start + 33 + delta].copy_from_slice(&19_u64.to_le_bytes());
        bytes[start + 37 + delta] = 1;
        bytes[start + 38 + delta..start + 42 + delta].copy_from_slice(&20_u32.to_le_bytes());
        bytes[start + 48 + delta..start + 50 + delta].copy_from_slice(&[1, 1]);
        bytes[start + 50 + delta..start + 126 + delta].copy_from_slice(
            &crate::bytes::lp_utf16_bytes("00000000-0000-0000-0000-000000000000").unwrap(),
        );
        header(&mut bytes, "262", 30);
        let mut scope = DesignParameterScope::empty(
            "f3d:Design/BulkStream.dat:design-parameter-scope#30",
            crate::records::feature::scope::DesignFeatureKind::ComponentInsert,
            30,
        );
        scope
            .try_edit(|draft| {
                draft.byte_offset = u64_from_index(start);
                draft.class_tag =
                    crate::records::references::DesignClassTag::try_from(class.to_owned()).unwrap();
                draft.frame_length = u64_from_index(frame);
                draft.paired_byte_offset = u64_from_index(start + frame);
                draft.paired_class_tag =
                    crate::records::references::DesignClassTag::try_from("262".to_owned()).unwrap();
                draft.reference_count_offset = u64_from_index(start + 129 + delta);
                draft.reference_members = crate::records::identity::ReferenceRun::from_columns(
                    vec![20],
                    vec![u64_from_index(start + 134 + delta)],
                    "reference_members",
                )
                .unwrap();
                draft.kind_offset = u64_from_index(start + 152 + delta);
                draft.feature_ordinal_offset = u64_from_index(start + 184 + delta);
            })
            .unwrap();
        let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
        let construction = tested_component_insert_construction(&bytes, &records, &scope).unwrap();
        assert_eq!(construction.neutron_role, role);
        assert_eq!(construction.occurrence_identity, Some(19));
        assert_eq!(construction.carrier_record_index, 10);
        assert!(construction.placement.is_none());
        bytes[relation_at + 48..relation_at + 52].copy_from_slice(&31_u32.to_le_bytes());
        assert!(tested_component_insert_construction(&bytes, &records, &scope).is_none());
    }
}
