// SPDX-License-Identifier: Apache-2.0
use crate::design::decode::sketch::decode_sketch_visibility_member;
use crate::design::decode::sketch::CURRENT_SKETCH_CONTAINER_VERSION;
use crate::design::decode::sketch::SKETCH_CONTAINER_MEMBER_BASE_TYPE_GUID;
use crate::design::decode::sketch::SKETCH_CONTAINER_MEMBER_TYPE_GUID;
use crate::design::decode::sketch::SKETCH_CONTAINER_MEMBER_VERSION;
use crate::design::decode::sketch::SKETCH_CONTAINER_TYPE_GUID;
use crate::records::entity_header::DESIGN_MODULE_SKETCH;

const ENTITY_SUFFIX: u64 = 201;

use crate::design::decode::sketch::decode_sketch_visibilities_in_stream;

fn member(stream_ordinal: u32, visible: u8) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"256");
    bytes.extend_from_slice(&ENTITY_SUFFIX.to_le_bytes());
    bytes.extend_from_slice(&[0; 4]);
    bytes.push(1);
    bytes.extend_from_slice(&203u64.to_le_bytes());
    bytes.extend_from_slice(&[0, 0]);
    bytes.extend_from_slice(&stream_ordinal.to_le_bytes());
    bytes.push(0);
    bytes.push(visible);
    bytes.push(1);
    bytes.extend_from_slice(&300u64.to_le_bytes());
    bytes.extend_from_slice(&[0; 2]);
    bytes
}

#[test]
fn sketch_visibility_member_decodes_both_boolean_values() {
    let hidden =
        decode_sketch_visibility_member(&member(1, 0), 0, ENTITY_SUFFIX).expect("hidden member");
    assert_eq!(hidden.stream_ordinal.get(), 1);
    assert_eq!(hidden.stream_ordinal_offset(), 30);
    assert_eq!(hidden.visible_offset(), 35);
    assert!(!hidden.visible);

    let visible =
        decode_sketch_visibility_member(&member(513, 1), 0, ENTITY_SUFFIX).expect("visible member");
    assert_eq!(visible.stream_ordinal.get(), 513);
    assert!(visible.visible);
}

#[test]
fn sketch_visibility_member_admits_a_null_ordinal_collection() {
    let mut bytes = member(1, 0);
    bytes.truncate(36);
    bytes.push(0);
    let visibility =
        decode_sketch_visibility_member(&bytes, 0, ENTITY_SUFFIX).expect("collection-less member");
    assert_eq!(visibility.ordinal_collection, None);
    assert!(!visibility.visible);

    let collected = decode_sketch_visibility_member(&member(1, 0), 0, ENTITY_SUFFIX)
        .expect("collection member");
    assert_eq!(
        collected.ordinal_collection.map(std::num::NonZeroU64::get),
        Some(300)
    );

    let mut invalid_marker = member(1, 0);
    invalid_marker[36] = 2;
    assert!(decode_sketch_visibility_member(&invalid_marker, 0, ENTITY_SUFFIX).is_none());
}

#[test]
fn sketch_visibility_member_rejects_invalid_ordinal_or_owner() {
    assert!(decode_sketch_visibility_member(&member(1, 2), 0, ENTITY_SUFFIX).is_none());
    assert!(decode_sketch_visibility_member(&member(0, 1), 0, ENTITY_SUFFIX).is_none());
    assert!(decode_sketch_visibility_member(&member(1, 1), 0, ENTITY_SUFFIX + 1).is_none());

    let mut external_owner = member(1, 1);
    external_owner[28] = 1;
    assert!(decode_sketch_visibility_member(&external_owner, 0, ENTITY_SUFFIX).is_none());
}

fn visibility_stream() -> (Vec<u8>, crate::metastream::MetaStream) {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"256");
    bytes.extend_from_slice(&ENTITY_SUFFIX.to_le_bytes());
    bytes.extend_from_slice(&[0; 5]);
    bytes.push(0);
    bytes.extend_from_slice(&5u32.to_le_bytes());
    for unit in "0_201".encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    let mut visibility = member(1, 1);
    visibility[4..7].copy_from_slice(b"257");
    bytes.extend_from_slice(&visibility);

    let segment_type = |type_guid: &str,
                        base_type_guid: Option<&str>,
                        version: u32,
                        module: &str,
                        entity_ids: Vec<u64>| {
        crate::records::entity_header::SegmentTypeData {
            byte_offset: 0,
            type_guid: type_guid.to_owned().try_into().expect("type GUID"),
            type_guid_offset: 0,
            base_type_guid: base_type_guid.map_or(
                crate::records::entity_header::BaseTypeGuid::Absent,
                |value| crate::records::entity_header::BaseTypeGuid::Guid {
                    value: value.to_owned().try_into().expect("base GUID"),
                    offset: 0,
                },
            ),
            version,
            version_offset: 0,
            module: module.into(),
            entities: crate::records::identity::ReferenceRun::located(
                entity_ids
                    .into_iter()
                    .map(|value| crate::records::identity::Located { value, offset: 0 })
                    .collect(),
            ),
        }
    };
    let metadata = crate::metastream::MetaStream {
        types: vec![
            segment_type(
                SKETCH_CONTAINER_TYPE_GUID,
                Some(SKETCH_CONTAINER_MEMBER_TYPE_GUID),
                CURRENT_SKETCH_CONTAINER_VERSION,
                DESIGN_MODULE_SKETCH,
                vec![ENTITY_SUFFIX],
            ),
            segment_type(
                SKETCH_CONTAINER_MEMBER_TYPE_GUID,
                Some(SKETCH_CONTAINER_MEMBER_BASE_TYPE_GUID),
                SKETCH_CONTAINER_MEMBER_VERSION,
                "Geometry",
                Vec::new(),
            ),
        ],
        records: vec![crate::metastream::RecordIndexEntry {
            entity_id: ENTITY_SUFFIX,
            bulk_offset: 0,
        }],
        secondary_records: Vec::new(),
    };

    (bytes, metadata)
}

fn collection_stream(
    named_collection: u64,
    collections: &[(u64, &[u64])],
) -> (Vec<u8>, crate::metastream::MetaStream) {
    collection_stream_version(named_collection, collections, 4)
}

fn collection_stream_version(
    named_collection: u64,
    collections: &[(u64, &[u64])],
    version: u32,
) -> (Vec<u8>, crate::metastream::MetaStream) {
    let (mut bytes, mut metadata) = visibility_stream();
    let member_at = bytes.len() - member(1, 1).len();
    bytes[member_at + 37..member_at + 45].copy_from_slice(&named_collection.to_le_bytes());
    let mut collection_type = metadata.types[1].clone();
    collection_type.type_guid = "5275CBA4-3D7D-40DC-A651-1DFF3FC8AFBF"
        .to_owned()
        .try_into()
        .unwrap();
    collection_type.version = version;
    collection_type.entities = crate::records::identity::ReferenceRun::unlocated(
        collections.iter().map(|(id, _)| *id).collect(),
    );
    metadata.types.push(collection_type);
    for (id, members) in collections {
        let collection_at = bytes.len();
        crate::test_support::lp_ascii(&mut bytes, "258");
        bytes.extend_from_slice(&u32::try_from(*id).unwrap().to_le_bytes());
        bytes.extend_from_slice(&[0; 9]);
        bytes.push(1);
        bytes.extend_from_slice(&1u32.to_le_bytes());
        crate::test_support::lp_ascii(&mut bytes, "crv_tag_id_idx");
        crate::test_support::lp_ascii(&mut bytes, "IntrinsicMetaTypeuint64");
        bytes.extend_from_slice(&7u64.to_le_bytes());
        if version == 3 {
            bytes.extend_from_slice(&[0; 6]);
        }
        bytes.extend_from_slice(&u32::try_from(members.len()).unwrap().to_le_bytes());
        for target in *members {
            bytes.push(1);
            bytes.extend_from_slice(&target.to_le_bytes());
            bytes.extend_from_slice(&[0; 2]);
        }
        metadata.records.push(crate::metastream::RecordIndexEntry {
            entity_id: *id,
            bulk_offset: cadmpeg_core::decode::u64_from_index(collection_at),
        });
    }
    (bytes, metadata)
}

#[test]
fn sketch_visibility_accepts_settled_container_header() {
    let (bytes, metadata) = collection_stream(300, &[(300, &[ENTITY_SUFFIX])]);

    let visibilities =
        decode_sketch_visibilities_in_stream(&bytes, &metadata).expect("settled header");
    assert_eq!(visibilities.len(), 1);
    assert_eq!(visibilities[0].0, ENTITY_SUFFIX);
    assert!(visibilities[0].1.visible);
}

#[test]
fn sketch_visibility_retains_its_explicit_ordinal_collection() {
    let mut bytes = member(2, 0);
    bytes[37..45].copy_from_slice(&400u64.to_le_bytes());
    let visibility = decode_sketch_visibility_member(&bytes, 0, ENTITY_SUFFIX).unwrap();
    assert_eq!(visibility.ordinal_collection.unwrap().get(), 400);
}

#[test]
fn sketch_listed_twice_in_collection_is_rejected() {
    let (bytes, metadata) = collection_stream(300, &[(300, &[ENTITY_SUFFIX, ENTITY_SUFFIX])]);
    assert!(decode_sketch_visibilities_in_stream(&bytes, &metadata).is_err());
}

#[test]
fn sketch_listed_in_two_collections_is_rejected() {
    let (bytes, metadata) =
        collection_stream(300, &[(300, &[ENTITY_SUFFIX]), (400, &[ENTITY_SUFFIX])]);
    assert!(decode_sketch_visibilities_in_stream(&bytes, &metadata).is_err());
}

#[test]
fn collection_listing_sketch_that_names_another_collection_is_rejected() {
    let (bytes, metadata) =
        collection_stream(400, &[(300, &[ENTITY_SUFFIX]), (400, &[ENTITY_SUFFIX])]);
    let error = decode_sketch_visibilities_in_stream(&bytes, &metadata).unwrap_err();
    assert!(error
        .to_string()
        .contains("sketch ordinal collection 300 has an invalid sketch member 201"));
}

#[test]
fn collection_run_target_must_resolve_to_sketch_container() {
    let (bytes, metadata) = collection_stream(300, &[(300, &[ENTITY_SUFFIX, 202])]);
    assert!(decode_sketch_visibilities_in_stream(&bytes, &metadata).is_err());
}

#[test]
fn typed_sketch_visibility_requires_reciprocal_collection_membership() {
    let (bytes, metadata) = collection_stream(300, &[(300, &[ENTITY_SUFFIX])]);
    let visibilities = decode_sketch_visibilities_in_stream(&bytes, &metadata).unwrap();
    assert_eq!(visibilities[0].1.ordinal_collection.unwrap().get(), 300);
    let (bytes, metadata) = collection_stream(300, &[(300, &[])]);
    assert!(decode_sketch_visibilities_in_stream(&bytes, &metadata).is_err());
}

#[test]
fn collection_run_allows_uninterpreted_frame_tail() {
    let (mut bytes, metadata) = collection_stream(300, &[(300, &[ENTITY_SUFFIX])]);
    bytes.extend_from_slice(&[0; 3]);
    assert!(decode_sketch_visibilities_in_stream(&bytes, &metadata).is_ok());
    bytes.push(1);
    assert!(decode_sketch_visibilities_in_stream(&bytes, &metadata).is_ok());
}

#[test]
fn unreferenced_collection_registration_needs_no_primary_frame() {
    let (mut bytes, mut metadata) = collection_stream(300, &[(300, &[ENTITY_SUFFIX])]);
    bytes.truncate(usize::try_from(metadata.records.pop().unwrap().bulk_offset).unwrap());
    metadata.types[0].version = CURRENT_SKETCH_CONTAINER_VERSION - 1;
    assert!(decode_sketch_visibilities_in_stream(&bytes, &metadata)
        .unwrap()
        .is_empty());
}

#[test]
fn sketch_visibility_requires_plain_nonzero_local_collection_reference() {
    let valid = member(1, 1);
    for end in 36..47 {
        assert!(decode_sketch_visibility_member(&valid[..end], 0, ENTITY_SUFFIX).is_none());
    }
    let mut null_target = valid.clone();
    null_target[37..45].fill(0);
    assert!(decode_sketch_visibility_member(&null_target, 0, ENTITY_SUFFIX).is_none());
    let mut cross_segment = valid.clone();
    cross_segment[46] = 1;
    cross_segment.extend_from_slice(&1u32.to_le_bytes());
    assert!(decode_sketch_visibility_member(&cross_segment, 0, ENTITY_SUFFIX).is_none());
    let mut typed = valid[..45].to_vec();
    crate::test_support::lp_ascii(&mut typed, SKETCH_CONTAINER_TYPE_GUID);
    typed.extend_from_slice(&[0; 2]);
    assert!(decode_sketch_visibility_member(&typed, 0, ENTITY_SUFFIX).is_none());
    let (mut bytes, metadata) = visibility_stream();
    bytes.truncate(bytes.len() - 10);
    assert!(decode_sketch_visibilities_in_stream(&bytes, &metadata).is_err());
}

#[test]
fn collection_counted_run_requires_complete_plain_local_references() {
    let (valid, metadata) = collection_stream(300, &[(300, &[ENTITY_SUFFIX])]);
    let reference_at = valid.len() - 11;
    for end in reference_at..valid.len() {
        assert!(decode_sketch_visibilities_in_stream(&valid[..end], &metadata).is_err());
    }
    let mut null_target = valid.clone();
    null_target[reference_at + 1..reference_at + 9].fill(0);
    assert!(decode_sketch_visibilities_in_stream(&null_target, &metadata).is_err());
    let mut cross_segment = valid;
    cross_segment[reference_at + 10] = 1;
    cross_segment.extend_from_slice(&1u32.to_le_bytes());
    assert!(decode_sketch_visibilities_in_stream(&cross_segment, &metadata).is_err());
}

#[test]
fn version_three_collection_has_six_bytes_before_its_member_run() {
    let (bytes, metadata) = collection_stream_version(300, &[(300, &[ENTITY_SUFFIX])], 3);
    let visibilities = decode_sketch_visibilities_in_stream(&bytes, &metadata).unwrap();
    assert_eq!(visibilities.len(), 1);
    assert_eq!(visibilities[0].0, ENTITY_SUFFIX);
    assert_eq!(visibilities[0].1.ordinal_collection.unwrap().get(), 300);
    assert!(visibilities[0].1.visible);
}
