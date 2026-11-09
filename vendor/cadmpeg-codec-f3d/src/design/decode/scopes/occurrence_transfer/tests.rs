// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::records::feature::assembly_features::OCCURRENCE_PLACEMENT_TYPE_GUID;
use crate::test_support::lp_utf16;

fn header(out: &mut Vec<u8>, index: u32) {
    out.extend_from_slice(&3u32.to_le_bytes());
    out.extend_from_slice(b"333");
    out.extend_from_slice(&index.to_le_bytes());
}
fn reference(out: &mut Vec<u8>, index: u32) {
    out.push(1);
    out.extend_from_slice(&index.to_le_bytes());
    out.extend_from_slice(&[0; 6]);
}
fn matrix(out: &mut Vec<u8>, translation: f64) {
    let mut rows: [[f64; 4]; 4] = SketchPlacementMatrix::IDENTITY.into();
    rows[0][3] = translation;
    for value in rows.into_iter().flatten() {
        out.extend_from_slice(&value.to_le_bytes());
    }
}
fn relation(out: &mut Vec<u8>, index: u32, scope: u32, owner: u32, marked_gap: bool) {
    header(out, index);
    out.extend_from_slice(&[0; 10]);
    reference(out, 10);
    if marked_gap {
        out.push(1);
    }
    out.extend_from_slice(&[0; 2]);
    reference(out, 99);
    out.push(0);
    reference(out, scope);
    header(out, index);
    out.extend_from_slice(&[0; 8]);
    reference(out, owner);
}
fn types() -> std::collections::HashMap<u64, (&'static str, u32)> {
    [
        (10, (OCCURRENCE_PLACEMENT_TYPE_GUID, 0)),
        (11, (COMPONENT_TYPE, 0)),
        (2, (COMPONENT_TYPE, 0)),
        (3, (COMPONENT_TYPE, 0)),
        (5, (CONTEXT_TYPE, 0)),
        (6, (CONTEXT_TYPE, 0)),
        (7, (OWNER_TYPE, 0)),
        (8, (OWNER_TYPE, 0)),
    ]
    .into_iter()
    .collect()
}
fn context(out: &mut Vec<u8>, component: u32, context: u32, collection: u32) {
    header(out, component);
    header(out, context);
    reference(out, collection);
    reference(out, 90);
    header(out, context);
    out.extend_from_slice(&[0; 8]);
    reference(out, component);
    header(out, collection);
}
/// Type GUID that `CopyPaste` and `CutPaste` scopes share.
const PASTE_TYPE: &str = "D57B1A22-9D5A-4D7D-A909-E08C579612CA";

fn fixture(cut: bool, identity: bool, shift: usize) -> (Vec<u8>, DesignParameterScope) {
    fixture_with(cut, identity, shift, false)
}
/// A paste scope with a localized kind, an 88-byte tail, and for a copy the
/// 58-byte relation, typed through the shared paste GUID.
fn localized_fixture(cut: bool, shift: usize) -> (Vec<u8>, DesignParameterScope) {
    fixture_with(cut, false, shift, true)
}
fn parse(bytes: &[u8], start: u64, localized: bool) -> Option<DesignParameterScope> {
    let records = IndexedRecordOffsets::build(bytes);
    crate::design::decode::scopes::parameter_scope::parse_parameter_scope_with_type_guid(
        bytes,
        &records,
        21,
        &"333".to_owned().try_into().unwrap(),
        start,
        localized.then_some(PASTE_TYPE),
    )
    .unwrap()
}
fn fixture_with(
    cut: bool,
    identity: bool,
    shift: usize,
    localized: bool,
) -> (Vec<u8>, DesignParameterScope) {
    let key = "11111111-2222-3333-4444-555555555555";
    let mut bytes = Vec::new();
    context(&mut bytes, 2, 5, 7);
    context(&mut bytes, 3, 6, 8);
    header(&mut bytes, 10);
    bytes.extend_from_slice(&[0; 8]);
    bytes.push(1);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    reference(&mut bytes, 11);
    header(&mut bytes, 11);
    relation(&mut bytes, 12, 20, 7, false);
    relation(&mut bytes, 14, 21, 8, localized && !cut);
    header(&mut bytes, 13);
    if cut {
        bytes.extend_from_slice(&[0; 22]);
        reference(&mut bytes, 12);
        lp_utf16(&mut bytes, key);
        bytes.extend_from_slice(&[0; 7]);
        bytes.extend_from_slice(&u32::MAX.to_le_bytes());
        lp_utf16(&mut bytes, "RemoveInstance");
        header(&mut bytes, 13);
        bytes.extend_from_slice(&[0; 8]);
        reference(&mut bytes, 7);
    }
    let start = bytes.len();
    header(&mut bytes, 21);
    if cut {
        if shift == 4 {
            bytes.extend_from_slice(&[0; 9]);
            bytes.push(1);
            bytes.extend_from_slice(&[0; 4]);
        } else {
            bytes.extend_from_slice(&[0; 10]);
        }
        reference(&mut bytes, 13);
        reference(&mut bytes, 22);
        bytes.push(1);
        reference(&mut bytes, 23);
        bytes.extend_from_slice(&[0; 4]);
        reference(&mut bytes, 14);
        bytes.push(1);
        bytes.push(u8::from(identity));
        if !identity {
            matrix(&mut bytes, 3.0);
        }
        lp_utf16(&mut bytes, key);
    } else {
        bytes.resize(bytes.len() + 11 + shift, 0);
        reference(&mut bytes, 22);
        bytes.push(0);
        matrix(&mut bytes, 2.0);
        reference(&mut bytes, 23);
        bytes.extend_from_slice(&[0; 4]);
        reference(&mut bytes, 14);
        bytes.extend_from_slice(&[1, 0]);
        matrix(&mut bytes, 3.0);
        lp_utf16(&mut bytes, "00000000-0000-0000-0000-000000000000");
    }
    bytes.extend_from_slice(&[0; 3]);
    let count_at = bytes.len();
    bytes.extend_from_slice(&1u32.to_le_bytes());
    reference(&mut bytes, 14);
    bytes.extend_from_slice(&u32::MAX.to_le_bytes());
    let kind = match (cut, localized) {
        (true, false) => "CutPaste",
        (false, false) => "CopyPaste",
        (true, true) => "CouperColler",
        (false, true) => "CopierColler",
    };
    lp_utf16(&mut bytes, kind);
    let ordinal_at = bytes.len();
    bytes.resize(ordinal_at + if localized { 88 } else { 87 }, 0);
    bytes[ordinal_at..ordinal_at + 4].copy_from_slice(&1u32.to_le_bytes());
    let paired = bytes.len();
    header(&mut bytes, 21);
    let scope = parse(&bytes, u64::try_from(start).unwrap(), localized).unwrap();
    assert_eq!(scope.paired_byte_offset(), u64::try_from(paired).unwrap());
    assert_eq!(
        scope.reference_count_offset(),
        u64::try_from(count_at).unwrap()
    );
    (bytes, scope)
}

#[test]
fn occurrence_transfer_decodes_cut_owners_and_identity_or_matrix_placement() {
    for (identity, shift) in [(true, 0), (false, 0), (false, 4)] {
        let (bytes, scope) = fixture(true, identity, shift);
        let records = IndexedRecordOffsets::build(&bytes);
        let value = exact_occurrence_transfer(&bytes, &records, &scope, &types()).unwrap();
        assert_eq!(value.target_owner.component, 3);
        assert_eq!(value.target_owner.context, 6);
        assert_eq!(value.target_owner.collection, 8);
        assert_eq!(value.component_record_index, 11);
        assert_eq!(value.target_transform_offset.is_none(), identity);
        let DesignOccurrenceTransferSource::Cut {
            source_owner,
            removal_record_index,
            ..
        } = value.source
        else {
            panic!("cut source");
        };
        assert_eq!(source_owner.component, 2);
        assert_eq!(source_owner.context, 5);
        assert_eq!(source_owner.collection, 7);
        assert_eq!(removal_record_index, 13);
    }
}

#[test]
fn occurrence_transfer_decodes_copy_placements_without_guessing_source_occurrence() {
    for shift in [0, 4] {
        let (bytes, scope) = fixture(false, false, shift);
        let records = IndexedRecordOffsets::build(&bytes);
        let value = exact_occurrence_transfer(&bytes, &records, &scope, &types()).unwrap();
        let DesignOccurrenceTransferSource::Copy {
            source_transform, ..
        } = value.source
        else {
            panic!("copy source");
        };
        assert_eq!(source_transform[0][3], 2.0);
        assert_eq!(value.target_transform[0][3], 3.0);
    }
}

#[test]
fn occurrence_transfer_rejects_changed_relation_marker_and_source_owner() {
    let (mut bytes, scope) = fixture(false, false, 0);
    let records = IndexedRecordOffsets::build(&bytes);
    bytes[usize::try_from(scope.byte_offset()).unwrap() + 177] = 0;
    assert!(exact_occurrence_transfer(&bytes, &records, &scope, &types()).is_none());

    let (mut bytes, scope) = fixture(true, false, 0);
    let records = IndexedRecordOffsets::build(&bytes);
    let relation_at = records.first_at_or_after(0, 12).unwrap();
    bytes[relation_at + 77..relation_at + 81].copy_from_slice(&9u32.to_le_bytes());
    assert!(exact_occurrence_transfer(&bytes, &records, &scope, &types()).is_none());
}

#[test]
fn occurrence_transfer_rejects_non_rigid_and_truncated_placement() {
    let (mut bytes, scope) = fixture(false, false, 0);
    let records = IndexedRecordOffsets::build(&bytes);
    let at = usize::try_from(scope.byte_offset()).unwrap() + 190;
    bytes[at..at + 8].copy_from_slice(&f64::NAN.to_le_bytes());
    assert!(exact_occurrence_transfer(&bytes, &records, &scope, &types()).is_none());
    assert!(exact_occurrence_transfer(&bytes[..at + 8], &records, &scope, &types()).is_none());
}

#[test]
fn occurrence_transfer_requires_typed_context_and_collection_ownership() {
    let (bytes, scope) = fixture(false, false, 0);
    let records = IndexedRecordOffsets::build(&bytes);
    let mut incomplete = types();
    incomplete.remove(&6);
    assert!(exact_occurrence_transfer(&bytes, &records, &scope, &incomplete).is_none());
    let mut wrong_type = types();
    wrong_type.insert(8, (COMPONENT_TYPE, 0));
    assert!(exact_occurrence_transfer(&bytes, &records, &scope, &wrong_type).is_none());
}

#[test]
fn occurrence_transfer_rejects_other_types_with_an_occurrence_prefix() {
    let (bytes, scope) = fixture(false, false, 0);
    let records = IndexedRecordOffsets::build(&bytes);
    let mut wrong_type = types();
    wrong_type.insert(10, (COMPONENT_TYPE, 0));
    assert!(exact_occurrence_transfer(&bytes, &records, &scope, &wrong_type).is_none());
}

#[test]
fn a_localized_compact_copy_prefix_selects_copy_paste_and_decodes_its_transfer() {
    for shift in [0, 4] {
        let (bytes, scope) = localized_fixture(false, shift);
        assert_eq!(
            scope.kind(),
            crate::records::feature::scope::DesignFeatureKind::CopyPaste
        );
        assert_eq!(scope.source_kind_name(), "CopierColler");
        let records = IndexedRecordOffsets::build(&bytes);
        let value = exact_occurrence_transfer(&bytes, &records, &scope, &types()).unwrap();
        assert_eq!(value.occurrence_record_index, 10);
        assert_eq!(value.target_owner.collection, 8);
        let DesignOccurrenceTransferSource::Copy {
            source_transform, ..
        } = value.source
        else {
            panic!("copy source");
        };
        assert_eq!(source_transform[0][3], 2.0);
        assert_eq!(value.target_transform[0][3], 3.0);
    }
}

#[test]
fn the_shared_paste_type_keeps_a_cut_or_changed_copy_prefix_native() {
    let native = |kind: &str| {
        crate::records::feature::scope::DesignFeatureKind::try_from(kind.to_owned()).unwrap()
    };
    let (bytes, scope) = localized_fixture(true, 0);
    assert_eq!(scope.kind(), native("CouperColler"));
    let records = IndexedRecordOffsets::build(&bytes);
    assert!(exact_occurrence_transfer(&bytes, &records, &scope, &types()).is_none());

    for marker in [20, 21] {
        let (mut bytes, scope) = localized_fixture(false, 0);
        bytes[usize::try_from(scope.byte_offset()).unwrap() + marker] = 1;
        let changed = parse(&bytes, scope.byte_offset(), true).unwrap();
        assert_eq!(changed.kind(), native("CopierColler"));
    }
}

#[test]
fn the_58_byte_relation_is_admitted_for_a_copy_and_refused_for_a_cut() {
    for cut in [false, true] {
        let (mut bytes, scope) = fixture(cut, false, 0);
        let relation_at = IndexedRecordOffsets::build(&bytes)
            .first_at_or_after(0, 14)
            .unwrap();
        bytes.insert(relation_at + 32, 1);
        let scope = parse(&bytes, scope.byte_offset() + 1, false).unwrap();
        let records = IndexedRecordOffsets::build(&bytes);
        assert_eq!(
            exact_occurrence_transfer(&bytes, &records, &scope, &types()).is_some(),
            !cut
        );
    }
}
