// SPDX-License-Identifier: Apache-2.0
//! Leading-flag local component-occurrence frames.

use super::super::leading_flag_component_occurrence;
use super::{guid, COMPONENT, OCCURRENCE};
use crate::records::feature::assembly_features::DesignComponentOccurrence;
use crate::test_support::indexed_header;

const STREAM: &str = "f3d:Design/BulkStream.dat";
const MATRIX: [f64; 16] = [
    1.0, 0.0, 0.0, 5.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
];

fn write_matrix(bytes: &mut [u8], at: usize) {
    for (ordinal, value) in MATRIX.into_iter().enumerate() {
        bytes[at + ordinal * 8..at + ordinal * 8 + 8].copy_from_slice(&value.to_le_bytes());
    }
}

/// A leading-flag frame for component record 202, record reference 11 and owner
/// entity 6, with a matrix when `matrix` is set.
fn frame(matrix: bool) -> Vec<u8> {
    let extra = if matrix { 128 } else { 0 };
    let mut bytes = Vec::new();
    indexed_header(&mut bytes, *b"332", 253);
    bytes.resize(251 + 2 * extra, 0);
    bytes[19] = 1;
    bytes[20..24].copy_from_slice(&1_u32.to_le_bytes());
    bytes[24] = 1;
    bytes[25..33].copy_from_slice(&202_u64.to_le_bytes());
    bytes[35..39].copy_from_slice(&1_u32.to_le_bytes());
    bytes[39] = 1;
    bytes[44] = 1;
    bytes[46..50].copy_from_slice(&1_u32.to_le_bytes());
    bytes[50] = u8::from(!matrix);
    if matrix {
        write_matrix(&mut bytes, 51);
    }
    let body = 51 + extra;
    guid(&mut bytes, body, COMPONENT);
    guid(&mut bytes, body + 76, OCCURRENCE);
    bytes[body + 152] = 1;
    bytes[body + 153..body + 161].copy_from_slice(&11_u64.to_le_bytes());
    bytes[body + 162] = 1;
    bytes[body + 163..body + 167].copy_from_slice(&1_u32.to_le_bytes());
    bytes[body + 168] = 1;
    bytes[body + 169..body + 177].copy_from_slice(&202_u64.to_le_bytes());
    bytes[body + 179] = bytes[50];
    let mut tail = body + 180;
    if matrix {
        write_matrix(&mut bytes, tail);
        tail += 128;
    }
    bytes[tail + 9] = 1;
    bytes[tail + 10..tail + 18].copy_from_slice(&6_u64.to_le_bytes());
    bytes
}

fn parse(bytes: &[u8]) -> Option<DesignComponentOccurrence> {
    leading_flag_component_occurrence(bytes, 0, bytes.len(), STREAM)
}

#[test]
fn leading_flag_frames_state_the_occurrence_and_its_offsets() {
    for matrix in [false, true] {
        let extra = if matrix { 128 } else { 0 };
        let record = parse(&frame(matrix)).unwrap();
        assert_eq!(record.record_index, 253);
        assert_eq!(record.component_record_index, 202);
        assert_eq!(record.owner_entity, 6);
        assert_eq!(record.component_guid.as_str(), COMPONENT);
        assert_eq!(record.occurrence_guid.as_str(), OCCURRENCE);
        assert_eq!(record.occurrence_ordinal(), 1);
        assert_eq!(
            record.path_discriminator.map(std::num::NonZeroU32::get),
            Some(1)
        );
        assert_eq!(
            record.transform().map(|transform| transform.offset),
            matrix.then_some(51)
        );
        if let Some(transform) = record.transform() {
            let expected: crate::records::sketch_placement::SketchPlacementMatrix = [
                [1.0, 0.0, 0.0, 5.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ]
            .try_into()
            .unwrap();
            assert_eq!(transform.value, expected);
        }
        let wire = serde_json::to_value(&record).unwrap();
        assert_eq!(wire["component_guid_offset"], 55 + extra);
        assert_eq!(wire["occurrence_guid_offset"], 131 + extra);
        assert_eq!(wire["owner_entity_offset"], 241 + 2 * extra);
        assert_eq!(wire["leading_flag"]["ordinal_prefix"], 1);
        assert_eq!(wire["leading_flag"]["record_reference"], 11);
        let decoded: DesignComponentOccurrence = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(decoded, record);
        for field in [
            "component_guid_offset",
            "occurrence_guid_offset",
            "owner_entity_offset",
        ] {
            let mut changed = wire.clone();
            changed[field] = serde_json::json!(wire[field].as_u64().unwrap() + 1);
            assert!(serde_json::from_value::<DesignComponentOccurrence>(changed).is_err());
        }
        let mut owned = wire.clone();
        owned["owner_type_guid"] = serde_json::json!(OWNER_TYPE);
        assert!(serde_json::from_value::<DesignComponentOccurrence>(owned).is_err());
    }
}

#[test]
fn a_leading_flag_frame_with_one_wrong_field_states_no_occurrence() {
    for matrix in [false, true] {
        let extra = if matrix { 128 } else { 0 };
        let body = 51 + extra;
        let tail = body + 180 + extra;
        let mut cases: Vec<Vec<u8>> = Vec::new();
        let mut edit = |change: &dyn Fn(&mut Vec<u8>)| {
            let mut bytes = frame(matrix);
            change(&mut bytes);
            cases.push(bytes);
        };
        edit(&|bytes| bytes[40..44].copy_from_slice(&1_u32.to_le_bytes()));
        edit(&|bytes| bytes[46..50].copy_from_slice(&0_u32.to_le_bytes()));
        edit(&|bytes| bytes[50] = 2);
        edit(&|bytes| bytes[33] = 1);
        edit(&|bytes| bytes[body + 152] = 0);
        edit(&|bytes| bytes[body + 161] = 1);
        edit(&|bytes| bytes[body + 163..body + 167].copy_from_slice(&2_u32.to_le_bytes()));
        edit(&|bytes| bytes[body + 169..body + 177].copy_from_slice(&203_u64.to_le_bytes()));
        edit(&|bytes| bytes[body + 177] = 1);
        edit(&|bytes| bytes[body + 179] ^= 1);
        edit(&|bytes| bytes[tail] = 1);
        edit(&|bytes| bytes[tail + 9] = 0);
        edit(&|bytes| bytes.push(0));
        edit(&|bytes| {
            bytes.pop();
        });
        if matrix {
            edit(&|bytes| {
                bytes[body + 180 + 24..body + 180 + 32].copy_from_slice(&6.0_f64.to_le_bytes());
            });
        }
        for bytes in cases {
            assert!(parse(&bytes).is_none());
        }
    }
}

/// Component types each registering one entity at one offset.
fn registered(rows: &[(&str, u32, u64)]) -> crate::metastream::MetaStream {
    use crate::metastream::{MetaStream, RecordIndexEntry};
    use crate::records::entity_header::{BaseTypeGuid, SegmentTypeData};
    use crate::records::identity::ReferenceRun;
    MetaStream {
        types: rows
            .iter()
            .map(|&(type_guid, entity, _)| SegmentTypeData {
                byte_offset: 0,
                type_guid: type_guid.to_owned().try_into().unwrap(),
                type_guid_offset: 0,
                base_type_guid: BaseTypeGuid::Absent,
                version: 2,
                version_offset: 0,
                module: "Component".to_owned(),
                entities: ReferenceRun::unlocated(vec![u64::from(entity)]),
            })
            .collect(),
        records: rows
            .iter()
            .map(|&(_, entity, bulk_offset)| RecordIndexEntry {
                entity_id: u64::from(entity),
                bulk_offset,
            })
            .collect(),
        secondary_records: Vec::new(),
    }
}

/// Discover and admit the occurrences of one stream as the decoder does.
fn discover(
    bytes: &[u8],
    meta: &crate::metastream::MetaStream,
) -> (
    Vec<DesignComponentOccurrence>,
    Vec<cadmpeg_ir::report::loss::LossNote>,
    usize,
) {
    let key = |entity: u64, start: usize| (entity, u64::try_from(start).unwrap());
    let frames = crate::metastream::primary_record_frames(meta, bytes.len()).unwrap();
    let typed = super::super::occurrence_frames(bytes, meta).unwrap();
    let primary = frames
        .iter()
        .map(|frame| key(frame.entity_id, frame.start))
        .collect();
    let typed_primary = typed
        .iter()
        .map(|frame| key(frame.entity_id, frame.start))
        .collect();
    let frame_ends = frames
        .iter()
        .map(|frame| (frame.start, frame.member_end))
        .collect();
    let admitted = super::super::admitted_stream_occurrences(
        bytes,
        STREAM,
        &frame_ends,
        &super::super::occurrence_source_entities(meta),
        &primary,
        &typed_primary,
    );
    (admitted.occurrences, admitted.losses, admitted.superseded)
}

const OWNER_TYPE: &str = "904F885E-D859-4755-A7EF-6EBED8732FAD";

fn header(bytes: &mut [u8], class_tag: &[u8; 3], record_index: u32) {
    bytes[4..7].copy_from_slice(class_tag);
    bytes[7..11].copy_from_slice(&record_index.to_le_bytes());
}

#[test]
fn a_leading_flag_frame_under_another_type_is_kept_with_a_registration_loss() {
    let placement = registered(&[(super::super::OCCURRENCE_PLACEMENT_TYPE_GUID, 253, 0)]);
    let other = registered(&[(OWNER_TYPE, 253, 0)]);
    for matrix in [false, true] {
        let mut bytes = frame(matrix);
        header(&mut bytes, b"256", 253);
        if matrix {
            bytes[46..50].copy_from_slice(&2_u32.to_le_bytes());
        }
        let (occurrences, losses, superseded) = discover(&bytes, &placement);
        assert_eq!(occurrences.len(), 1);
        assert!(losses.is_empty());
        assert_eq!(superseded, 0);
        let (occurrences, losses, superseded) = discover(&bytes, &other);
        assert_eq!(occurrences.len(), 1);
        assert_eq!(occurrences[0].component_record_index, 202);
        assert_eq!(superseded, 0);
        assert_eq!(losses.len(), 1);
        assert_eq!(
            losses[0].code,
            crate::loss::F3dLossCode::LocalOccurrenceTypeUnregistered.kind()
        );
        assert!(losses[0].message.contains(&occurrences[0].id));
    }
}

fn codes(losses: &[cadmpeg_ir::report::loss::LossNote]) -> Vec<crate::loss::F3dLossCode> {
    use crate::loss::F3dLossCode;
    [
        F3dLossCode::LocalOccurrenceTypeUnregistered,
        F3dLossCode::LocalOccurrenceYieldedToRegistered,
        F3dLossCode::LocalOccurrenceKeyRepeated,
        F3dLossCode::LocalOccurrenceFirstMatrixRefused,
        F3dLossCode::LocalOccurrenceIdentityOrdinalUnresolved,
    ]
    .into_iter()
    .filter(|code| losses.iter().any(|loss| loss.code == code.kind()))
    .collect()
}

/// `first` at offset 0 and `second` after it, with the offset of `second`.
fn two_frames(first: Vec<u8>, second: &[u8]) -> (Vec<u8>, u64) {
    let mut bytes = first;
    let second_offset = u64::try_from(bytes.len()).unwrap();
    bytes.extend_from_slice(second);
    (bytes, second_offset)
}

#[test]
fn an_occurrence_with_a_registered_and_an_unregistered_frame_takes_the_registered_one() {
    use crate::loss::F3dLossCode;
    let mut registered_frame = frame(false);
    header(&mut registered_frame, b"256", 253);
    let mut unregistered = frame(false);
    header(&mut unregistered, b"257", 254);
    let (bytes, second) = two_frames(registered_frame, &unregistered);
    let meta = registered(&[
        (super::super::OCCURRENCE_PLACEMENT_TYPE_GUID, 253, 0),
        (OWNER_TYPE, 254, second),
    ]);
    let (occurrences, losses, superseded) = discover(&bytes, &meta);
    assert_eq!(occurrences.len(), 1);
    assert_eq!(occurrences[0].byte_offset(), 0);
    assert_eq!(superseded, 0);
    assert_eq!(
        codes(&losses),
        [F3dLossCode::LocalOccurrenceYieldedToRegistered]
    );

    let mut upper = unregistered.clone();
    let guid_at = 51 + 76 + 4;
    for unit in upper[guid_at..guid_at + 72].chunks_mut(2) {
        unit[0] = unit[0].to_ascii_uppercase();
    }
    let mut lower = frame(false);
    header(&mut lower, b"256", 253);
    let (bytes, _) = two_frames(lower, &upper);
    let (occurrences, losses, _) = discover(&bytes, &meta);
    assert_eq!(occurrences.len(), 1);
    assert_eq!(
        codes(&losses),
        [F3dLossCode::LocalOccurrenceYieldedToRegistered]
    );

    let mut unproven = frame(false);
    header(&mut unproven, b"256", 253);
    unproven[46..50].copy_from_slice(&2_u32.to_le_bytes());
    let (bytes, _) = two_frames(unproven, &unregistered);
    let (occurrences, losses, _) = discover(&bytes, &meta);
    assert_eq!(occurrences.len(), 1);
    assert_eq!(occurrences[0].record_index, 254);
    assert_eq!(
        codes(&losses),
        [
            F3dLossCode::LocalOccurrenceTypeUnregistered,
            F3dLossCode::LocalOccurrenceIdentityOrdinalUnresolved
        ]
    );

    let mut first = frame(false);
    header(&mut first, b"256", 253);
    let (bytes, second) = two_frames(first, &unregistered);
    let both_unregistered = registered(&[(OWNER_TYPE, 253, 0), (OWNER_TYPE, 254, second)]);
    let (occurrences, losses, superseded) = discover(&bytes, &both_unregistered);
    assert!(occurrences.is_empty());
    assert_eq!(superseded, 0);
    assert_eq!(codes(&losses), [F3dLossCode::LocalOccurrenceKeyRepeated]);
    assert!(losses[0].message.contains("#0, "));

    let mut first = frame(false);
    header(&mut first, b"256", 253);
    let (bytes, second) = two_frames(first, &upper);
    let both_registered = registered(&[
        (super::super::OCCURRENCE_PLACEMENT_TYPE_GUID, 253, 0),
        (super::super::OCCURRENCE_PLACEMENT_TYPE_GUID, 254, second),
    ]);
    let (occurrences, losses, _) = discover(&bytes, &both_registered);
    assert!(occurrences.is_empty());
    assert_eq!(codes(&losses), [F3dLossCode::LocalOccurrenceKeyRepeated]);
}

#[test]
fn a_leading_flag_frame_is_read_only_at_a_primary_record_start() {
    let mut later = frame(false);
    header(&mut later, b"256", 253);
    later[46..50].copy_from_slice(&2_u32.to_le_bytes());
    indexed_header(&mut later, *b"300", 999);
    let (occurrences, losses, superseded) = discover(&later, &registered(&[]));
    assert!(occurrences.is_empty());
    assert!(losses.is_empty());
    assert_eq!(superseded, 0);

    let mut spanning = vec![0_u8; 40];
    spanning[0..4].copy_from_slice(&3_u32.to_le_bytes());
    header(&mut spanning, b"300", 300);
    let mut inside = frame(false);
    header(&mut inside, b"257", 253);
    indexed_header(&mut inside, *b"300", 999);
    let (bytes, _) = two_frames(spanning, &inside);
    let (occurrences, losses, superseded) = discover(&bytes, &registered(&[(OWNER_TYPE, 300, 0)]));
    assert!(occurrences.is_empty());
    assert!(losses.is_empty());
    assert_eq!(superseded, 0);
}

#[test]
fn a_class_256_leading_flag_frame_stores_a_matrix_only_after_ordinal_one() {
    use crate::loss::F3dLossCode;
    let placement = registered(&[(super::super::OCCURRENCE_PLACEMENT_TYPE_GUID, 253, 0)]);
    let mut first = frame(true);
    header(&mut first, b"256", 253);
    let (occurrences, losses, superseded) = discover(&first, &placement);
    assert!(occurrences.is_empty());
    assert_eq!(superseded, 0);
    assert_eq!(
        codes(&losses),
        [F3dLossCode::LocalOccurrenceFirstMatrixRefused]
    );
    assert!(losses[0].message.contains("#0."));

    let mut later = first.clone();
    later[46..50].copy_from_slice(&2_u32.to_le_bytes());
    let (occurrences, losses, _) = discover(&later, &placement);
    assert_eq!(occurrences.len(), 1);
    assert!(losses.is_empty());

    let mut identity = frame(false);
    header(&mut identity, b"256", 253);
    let (occurrences, losses, _) = discover(&identity, &placement);
    assert_eq!(occurrences.len(), 1);
    assert!(losses.is_empty());

    assert!(!parse(&frame(true)).unwrap().is_class_256_first_matrix());
}
