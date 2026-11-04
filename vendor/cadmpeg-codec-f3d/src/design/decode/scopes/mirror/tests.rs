// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::u64_from_index;

use super::compact_feature_reference;
use super::exact_legacy_mirror_scope_count;
use super::exact_legacy_mirror_scope_tolerance;
use crate::records::decal::DesignRecordHeader;
use crate::records::feature::scope::DesignParameterScope;
use crate::test_support::lp_utf16;

fn indexed_header(bytes: &mut Vec<u8>, class_tag: [u8; 3], record_index: u32) -> usize {
    let start = bytes.len();
    crate::test_support::indexed_header(bytes, class_tag, record_index);
    start
}

fn compact_reference_fixture() -> (Vec<u8>, DesignRecordHeader, usize, u32) {
    let record_index = 40;
    let reference = 17_u32;
    let mut bytes = Vec::new();
    let start = indexed_header(&mut bytes, *b"320", record_index);
    bytes.extend_from_slice(&[0; 10]);
    bytes.push(1);
    bytes.extend_from_slice(&(record_index + 3).to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    lp_utf16(&mut bytes, "dfa12ed5-41e3-47c2-947d-286843e235df");
    lp_utf16(&mut bytes, "15afb570-2968-417f-8485-96c81b2d332f");
    bytes.extend_from_slice(&2_u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 4]);
    indexed_header(&mut bytes, *b"259", record_index);
    indexed_header(&mut bytes, *b"306", record_index + 1);
    indexed_header(&mut bytes, *b"291", record_index + 2);
    let identity = indexed_header(&mut bytes, *b"428", record_index + 3);
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&reference.to_le_bytes());
    bytes.extend_from_slice(&[0; 4]);
    indexed_header(&mut bytes, *b"457", record_index + 4);
    let header = DesignRecordHeader {
        id: String::new(),
        record_index,
        class_tag: crate::records::references::DesignClassTag::try_from("320".to_owned()).unwrap(),
        byte_offset: u64_from_index(start),
    };

    (bytes, header, identity, reference)
}

#[test]
fn compact_mirror_reference_uses_the_identity_record_lane() {
    let (mut bytes, header, identity, reference) = compact_reference_fixture();
    assert_eq!(
        compact_feature_reference(&bytes, &header),
        Some((reference, u64_from_index(identity + 21)))
    );
    bytes[identity + 20] = 1;
    assert_eq!(compact_feature_reference(&bytes, &header), None);
}

#[test]
fn compact_mirror_reference_reads_its_guid_text() {
    let (bytes, header, identity, reference) = compact_reference_fixture();
    assert_eq!(
        compact_feature_reference(&bytes, &header),
        Some((reference, u64_from_index(identity + 21)))
    );
}

#[test]
fn class_413_mirror_scope_decodes_inline_tolerance() {
    let mut bytes = vec![0; 32 + 89];
    let mut scope = DesignParameterScope::empty(
        "scope",
        crate::records::feature::scope::DesignFeatureKind::Mirror,
        10,
    );
    scope.class_tag =
        crate::records::references::DesignClassTag::try_from("413".to_owned()).unwrap();
    scope.paired_class_tag =
        crate::records::references::DesignClassTag::try_from("262".to_owned()).unwrap();
    scope
        .try_edit(|draft| {
            draft.kind_offset = 32;
            draft.feature_ordinal_offset = 44;
            draft.previous_history_state_id_offset = Some(32 + 43);
            draft.frame_length = 32 + 89;
            draft.paired_byte_offset = 32 + 89;
        })
        .unwrap();
    bytes[32 + 47..32 + 51].copy_from_slice(&89_u32.to_le_bytes());
    bytes[32 + 51..32 + 59].copy_from_slice(&0.25_f64.to_le_bytes());
    bytes[32 + 59..32 + 63].copy_from_slice(&89_u32.to_le_bytes());
    bytes[32 + 63] = 1;
    bytes[32 + 64..32 + 68].copy_from_slice(&12_u32.to_le_bytes());
    bytes[32 + 76] = 1;
    bytes[32 + 77..32 + 81].copy_from_slice(&11_u32.to_le_bytes());
    let (value, offset, carrier) =
        exact_legacy_mirror_scope_tolerance(&bytes, &scope).expect("class-413 tolerance");
    assert_eq!(value.get(), 0.25);
    assert_eq!(offset, 32 + 51);
    assert_eq!(carrier.first_reference, 12);
    assert_eq!(carrier.second_reference, 11);
}

#[test]
fn class_369_mirror_scope_decodes_inline_tolerance() {
    let mut bytes = vec![0; 32 + 89];
    let mut scope = DesignParameterScope::empty(
        "scope",
        crate::records::feature::scope::DesignFeatureKind::Mirror,
        10,
    );
    scope.class_tag =
        crate::records::references::DesignClassTag::try_from("369".to_owned()).unwrap();
    scope.paired_class_tag =
        crate::records::references::DesignClassTag::try_from("261".to_owned()).unwrap();
    scope
        .try_edit(|draft| {
            draft.kind_offset = 32;
            draft.feature_ordinal_offset = 44;
            draft.previous_history_state_id_offset = Some(32 + 43);
            draft.frame_length = 32 + 89;
            draft.paired_byte_offset = 32 + 89;
        })
        .unwrap();
    bytes[32 + 47..32 + 51].copy_from_slice(&89_u32.to_le_bytes());
    bytes[32 + 51..32 + 59].copy_from_slice(&0.25_f64.to_le_bytes());
    bytes[32 + 59..32 + 63].copy_from_slice(&89_u32.to_le_bytes());
    bytes[32 + 63] = 1;
    bytes[32 + 64..32 + 68].copy_from_slice(&12_u32.to_le_bytes());
    bytes[32 + 76] = 1;
    bytes[32 + 77..32 + 81].copy_from_slice(&11_u32.to_le_bytes());
    let (value, offset, carrier) =
        exact_legacy_mirror_scope_tolerance(&bytes, &scope).expect("class-369 tolerance");
    assert_eq!(value.get(), 0.25);
    assert_eq!(offset, 32 + 51);
    assert_eq!(carrier.marker.code(), 89);
    assert_eq!(carrier.marker.repeated_offset(), Some(32 + 59));
    assert_eq!(carrier.first_reference, 12);
    assert_eq!(carrier.second_reference, 11);

    bytes[32 + 59..32 + 63].copy_from_slice(&90_u32.to_le_bytes());
    assert_eq!(exact_legacy_mirror_scope_tolerance(&bytes, &scope), None);
}

#[test]
fn class_391_mirror_scope_decodes_inline_tolerance() {
    let mut bytes = vec![0; 32 + 88];
    let mut scope = DesignParameterScope::empty(
        "scope",
        crate::records::feature::scope::DesignFeatureKind::Mirror,
        10,
    );
    scope.class_tag =
        crate::records::references::DesignClassTag::try_from("391".to_owned()).unwrap();
    scope.paired_class_tag =
        crate::records::references::DesignClassTag::try_from("261".to_owned()).unwrap();
    scope
        .try_edit(|draft| {
            draft.kind_offset = 32;
            draft.feature_ordinal_offset = 44;
            draft.previous_history_state_id_offset = Some(32 + 42);
            draft.frame_length = 32 + 88;
            draft.paired_byte_offset = 32 + 88;
        })
        .unwrap();
    bytes[32 + 46..32 + 50].copy_from_slice(&94_u32.to_le_bytes());
    bytes[32 + 50..32 + 58].copy_from_slice(&0.25_f64.to_le_bytes());
    bytes[32 + 58..32 + 62].copy_from_slice(&94_u32.to_le_bytes());
    bytes[32 + 62] = 1;
    bytes[32 + 63..32 + 67].copy_from_slice(&12_u32.to_le_bytes());
    bytes[32 + 75] = 1;
    bytes[32 + 76..32 + 80].copy_from_slice(&11_u32.to_le_bytes());
    let (value, offset, carrier) =
        exact_legacy_mirror_scope_tolerance(&bytes, &scope).expect("class-391 tolerance");
    assert_eq!(value.get(), 0.25);
    assert_eq!(offset, 32 + 50);
    assert_eq!(carrier.marker.code(), 94);
    assert_eq!(carrier.marker.repeated_offset(), Some(32 + 58));
    assert_eq!(carrier.first_reference, 12);
    assert_eq!(carrier.second_reference, 11);

    bytes[32 + 58..32 + 62].copy_from_slice(&95_u32.to_le_bytes());
    assert_eq!(exact_legacy_mirror_scope_tolerance(&bytes, &scope), None);
}

#[test]
fn class_440_mirror_scope_decodes_inline_tolerance() {
    let mut bytes = vec![0; 32 + 89];
    let mut scope = DesignParameterScope::empty(
        "scope",
        crate::records::feature::scope::DesignFeatureKind::Mirror,
        10,
    );
    scope.class_tag =
        crate::records::references::DesignClassTag::try_from("440".to_owned()).unwrap();
    scope.paired_class_tag =
        crate::records::references::DesignClassTag::try_from("258".to_owned()).unwrap();
    scope
        .try_edit(|draft| {
            draft.kind_offset = 32;
            draft.feature_ordinal_offset = 44;
            draft.previous_history_state_id_offset = Some(32 + 43);
            draft.frame_length = 32 + 89;
            draft.paired_byte_offset = 32 + 89;
        })
        .unwrap();
    bytes[32 + 47..32 + 51].copy_from_slice(&100_u32.to_le_bytes());
    bytes[32 + 51..32 + 59].copy_from_slice(&0.25_f64.to_le_bytes());
    bytes[32 + 59..32 + 63].copy_from_slice(&100_u32.to_le_bytes());
    bytes[32 + 63] = 1;
    bytes[32 + 64..32 + 68].copy_from_slice(&12_u32.to_le_bytes());
    bytes[32 + 76] = 1;
    bytes[32 + 77..32 + 81].copy_from_slice(&11_u32.to_le_bytes());
    let (value, offset, carrier) =
        exact_legacy_mirror_scope_tolerance(&bytes, &scope).expect("class-440 tolerance");
    assert_eq!(value.get(), 0.25);
    assert_eq!(offset, 32 + 51);
    assert_eq!(carrier.marker.code(), 100);
    assert_eq!(carrier.first_reference, 12);
    assert_eq!(carrier.second_reference, 11);
}

#[test]
fn class_441_mirror_scope_decodes_the_unrepeated_inline_tolerance() {
    let mut bytes = vec![0; 32 + 84];
    let mut scope = DesignParameterScope::empty(
        "scope",
        crate::records::feature::scope::DesignFeatureKind::Mirror,
        10,
    );
    scope.class_tag =
        crate::records::references::DesignClassTag::try_from("441".to_owned()).unwrap();
    scope.paired_class_tag =
        crate::records::references::DesignClassTag::try_from("267".to_owned()).unwrap();
    scope
        .try_edit(|draft| {
            draft.kind_offset = 32;
            draft.feature_ordinal_offset = 44;
            draft.previous_history_state_id_offset = Some(32 + 42);
            draft.frame_length = 32 + 84;
            draft.paired_byte_offset = 32 + 84;
        })
        .unwrap();
    bytes[32 + 46..32 + 50].copy_from_slice(&61_u32.to_le_bytes());
    bytes[32 + 50..32 + 58].copy_from_slice(&0.125_f64.to_le_bytes());
    bytes[32 + 58] = 1;
    bytes[32 + 59..32 + 63].copy_from_slice(&12_u32.to_le_bytes());
    bytes[32 + 71] = 1;
    bytes[32 + 72..32 + 76].copy_from_slice(&11_u32.to_le_bytes());
    let (value, offset, carrier) =
        exact_legacy_mirror_scope_tolerance(&bytes, &scope).expect("class-441 tolerance");
    assert_eq!(value.get(), 0.125);
    assert_eq!(offset, 32 + 50);
    assert_eq!(carrier.marker.code(), 61);
    assert_eq!(carrier.marker.repeated_offset(), None);
    assert_eq!(carrier.first_reference, 12);
    assert_eq!(carrier.second_reference, 11);
}

#[test]
fn class_441_mirror_scope_decodes_the_inline_count_owner() {
    let scope_record_index: u32 = 65;
    let count_record_index: u32 = 80;
    let mut bytes = vec![0; 99];
    bytes[0..4].copy_from_slice(&3_u32.to_le_bytes());
    bytes[4..7].copy_from_slice(b"426");
    bytes[7..11].copy_from_slice(&count_record_index.to_le_bytes());
    bytes[19] = 1;
    bytes[20..24].copy_from_slice(&1_u32.to_le_bytes());
    bytes[24] = 1;
    bytes[25..29].copy_from_slice(&scope_record_index.to_le_bytes());
    bytes[35..39].copy_from_slice(&0_u32.to_le_bytes());
    bytes[40..44].copy_from_slice(&2_u32.to_le_bytes());
    bytes[44] = 1;
    bytes[45..49].copy_from_slice(&(count_record_index + 2).to_le_bytes());
    bytes[55..59].copy_from_slice(&1_u32.to_le_bytes());
    bytes[63] = 1;
    bytes[64..68].copy_from_slice(&scope_record_index.to_le_bytes());
    bytes[76] = 1;
    bytes[77..81].copy_from_slice(&(count_record_index + 1).to_le_bytes());
    bytes[88] = 1;
    bytes[89..93].copy_from_slice(&scope_record_index.to_le_bytes());
    indexed_header(&mut bytes, *b"267", count_record_index);

    let mut scope = DesignParameterScope::empty(
        "scope",
        crate::records::feature::scope::DesignFeatureKind::Mirror,
        scope_record_index,
    );
    scope.class_tag =
        crate::records::references::DesignClassTag::try_from("441".to_owned()).unwrap();
    scope.paired_class_tag =
        crate::records::references::DesignClassTag::try_from("267".to_owned()).unwrap();
    scope
        .try_edit(|draft| {
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(vec![
                1,
                2,
                3,
                count_record_index,
            ]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);

    assert_eq!(
        exact_legacy_mirror_scope_count(&bytes, &records, &scope),
        Some((count_record_index, 40))
    );
}

#[test]
fn mirror_unique_match_preserves_zero_one_and_many() {
    assert!(matches!(
        super::unique_match(std::iter::empty::<u32>()),
        super::UniqueMatch::Zero
    ));
    assert!(matches!(
        super::unique_match([7].into_iter()),
        super::UniqueMatch::One(7)
    ));
    assert!(matches!(
        super::unique_match([7, 8].into_iter()),
        super::UniqueMatch::Many
    ));
}
