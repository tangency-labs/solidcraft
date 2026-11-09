// SPDX-License-Identifier: Apache-2.0
use crate::design::decode::sketch::decode_sketch_visibility_member;
use crate::design::decode::sketch::CURRENT_SKETCH_CONTAINER_VERSION;
use crate::design::decode::sketch::SKETCH_CONTAINER_MEMBER_BASE_TYPE_GUID;
use crate::design::decode::sketch::SKETCH_CONTAINER_MEMBER_TYPE_GUID;
use crate::design::decode::sketch::SKETCH_CONTAINER_MEMBER_VERSION;
use crate::design::decode::sketch::SKETCH_CONTAINER_TYPE_GUID;
use crate::records::entity_header::DESIGN_MODULE_SKETCH;

const ENTITY_SUFFIX: u64 = 201;

fn decode_sketch_visibilities_in_stream(
    bytes: &[u8],
    metadata: &crate::metastream::MetaStream,
) -> Result<
    Vec<(
        u64,
        crate::records::sketch_placement::DesignSketchVisibility,
    )>,
    cadmpeg_core::CodecError,
> {
    super::super::decode_sketch_visibilities_in_stream(bytes, metadata, &mut Vec::new())
}

fn decode_with_losses(
    bytes: &[u8],
    metadata: &crate::metastream::MetaStream,
) -> (
    Vec<(
        u64,
        crate::records::sketch_placement::DesignSketchVisibility,
    )>,
    Vec<cadmpeg_ir::report::loss::LossNote>,
) {
    let mut losses = Vec::new();
    let visibilities =
        super::super::decode_sketch_visibilities_in_stream(bytes, metadata, &mut losses).unwrap();
    (visibilities, losses)
}

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
    assert!(!hidden.sketch_flag);

    let visible =
        decode_sketch_visibility_member(&member(513, 1), 0, ENTITY_SUFFIX).expect("visible member");
    assert_eq!(visible.stream_ordinal.get(), 513);
    assert!(visible.visible);
}

#[test]
fn sketch_visibility_member_accepts_sketch_flag_one() {
    let mut hidden = member(1, 0);
    hidden[34] = 1;
    let hidden = decode_sketch_visibility_member(&hidden, 0, ENTITY_SUFFIX).expect("hidden member");
    assert!(!hidden.visible);
    assert!(hidden.sketch_flag);

    let mut visible = member(1, 1);
    visible[34] = 1;
    let visible =
        decode_sketch_visibility_member(&visible, 0, ENTITY_SUFFIX).expect("visible member");
    assert!(visible.visible);
    assert!(visible.sketch_flag);
}

#[test]
fn sketch_visibility_member_rejects_invalid_sketch_flag() {
    let mut bytes = member(1, 0);
    bytes[34] = 2;
    assert!(decode_sketch_visibility_member(&bytes, 0, ENTITY_SUFFIX).is_none());
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
fn repeated_or_conflicting_members_keep_the_direct_binding() {
    for collections in [
        vec![(300, vec![ENTITY_SUFFIX, ENTITY_SUFFIX])],
        vec![(300, vec![ENTITY_SUFFIX]), (400, vec![ENTITY_SUFFIX])],
    ] {
        let borrowed: Vec<_> = collections
            .iter()
            .map(|(id, members)| (*id, members.as_slice()))
            .collect();
        let (bytes, metadata) = collection_stream(300, &borrowed);
        let (visibilities, losses) = decode_with_losses(&bytes, &metadata);
        assert_eq!(visibilities[0].1.ordinal_collection.unwrap().get(), 300);
        assert_eq!(losses.len(), 1);
        assert_eq!(
            losses[0].code,
            crate::loss::F3dLossCode::SketchMembershipConflict.kind()
        );
    }
}

#[test]
fn collection_member_that_names_another_collection_keeps_its_binding() {
    let (bytes, metadata) = collection_stream(400, &[(300, &[ENTITY_SUFFIX]), (400, &[])]);
    let (visibilities, losses) = decode_with_losses(&bytes, &metadata);
    assert_eq!(visibilities[0].1.ordinal_collection.unwrap().get(), 400);
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].code,
        crate::loss::F3dLossCode::SketchMembershipConflict.kind()
    );
    assert!(losses[0].message.contains("container 300"));
}

#[test]
fn unresolved_direct_container_drops_the_binding_and_reports_loss() {
    let (bytes, metadata) = collection_stream(400, &[(300, &[])]);
    let (visibilities, losses) = decode_with_losses(&bytes, &metadata);
    assert_eq!(visibilities[0].1.ordinal_collection, None);
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].code,
        crate::loss::F3dLossCode::SketchOwnerUnresolved.kind()
    );
    assert!(losses[0].message.contains("container 400"));
}

#[test]
fn unknown_collection_run_target_reports_a_loss() {
    let (bytes, metadata) = collection_stream(300, &[(300, &[ENTITY_SUFFIX, 202])]);
    let (visibilities, losses) = decode_with_losses(&bytes, &metadata);
    assert_eq!(visibilities[0].1.ordinal_collection.unwrap().get(), 300);
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].code,
        crate::loss::F3dLossCode::SketchOwnerUnresolved.kind()
    );
    assert!(losses[0]
        .message
        .contains("Sketch 202 listed by container 300"));
}

#[test]
fn unlisted_sketch_keeps_its_collection_without_loss() {
    let (bytes, metadata) = collection_stream(300, &[(300, &[])]);
    let (visibilities, losses) = decode_with_losses(&bytes, &metadata);
    assert_eq!(visibilities[0].1.ordinal_collection.unwrap().get(), 300);
    assert!(losses.is_empty());
}

fn append_folder(
    bytes: &mut Vec<u8>,
    metadata: &mut crate::metastream::MetaStream,
    name: &str,
    flag: u8,
    zero_count: usize,
) {
    let offset = bytes.len();
    crate::test_support::lp_ascii(bytes, "259");
    bytes.extend_from_slice(&500u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 9]);
    bytes.push(1);
    bytes.extend_from_slice(&[0; 5]);
    bytes.push(flag);
    bytes.extend_from_slice(&[0; 5]);
    let units: Vec<_> = name.encode_utf16().collect();
    bytes.extend_from_slice(&u32::try_from(units.len()).unwrap().to_le_bytes());
    for unit in units {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&1u32.to_le_bytes());
    for target in [ENTITY_SUFFIX, 300] {
        bytes.push(1);
        bytes.extend_from_slice(&target.to_le_bytes());
        bytes.extend_from_slice(&[0; 2]);
    }
    bytes.extend(vec![0; zero_count]);
    bytes.push(1);
    bytes.extend_from_slice(&21u64.to_le_bytes());
    bytes.extend_from_slice(&[0; 2]);
    let mut folder_type = metadata.types[1].clone();
    folder_type.type_guid = "45B3FDEA-5EBA-4B33-ADF8-47041CAC3602"
        .to_owned()
        .try_into()
        .unwrap();
    folder_type.base_type_guid = crate::records::entity_header::BaseTypeGuid::Guid {
        value: "A193F53F-86D7-4F45-A01C-51F5ECEEFC0E"
            .to_owned()
            .try_into()
            .unwrap(),
        offset: 0,
    };
    folder_type.version = 1;
    folder_type.entities = crate::records::identity::ReferenceRun::unlocated(vec![500]);
    metadata.types.push(folder_type);
    metadata.records.push(crate::metastream::RecordIndexEntry {
        entity_id: 500,
        bulk_offset: cadmpeg_core::decode::u64_from_index(offset),
    });
}

#[test]
fn folder_example_frames_decode() {
    let examples = [
        ("03 00 00 00 34 31 33 24 4d 00 00 00 00 00 00 00 00 00 00 00 01 00 00 00 00 00 00 00 00 00 00 00 0b 00 00 00 46 00 6f 00 6c 00 64 00 65 00 72 00 20 00 4e 00 61 00 6d 00 65 00 01 00 00 00 08 00 00 00 01 10 01 00 00 00 00 00 00 00 00 01 11 01 00 00 00 00 00 00 00 00 01 12 01 00 00 00 00 00 00 00 00 01 13 01 00 00 00 00 00 00 00 00 01 14 01 00 00 00 00 00 00 00 00 01 15 01 00 00 00 00 00 00 00 00 01 16 01 00 00 00 00 00 00 00 00 01 17 01 00 00 00 00 00 00 00 00 01 09 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 01 15 00 00 00 00 00 00 00 00 00", "Folder Name", 8),
        ("03 00 00 00 34 32 34 63 09 00 00 00 00 00 00 00 00 00 00 00 01 00 00 00 00 00 01 00 00 00 00 00 00 00 00 00 01 00 00 00 02 00 00 00 01 18 01 00 00 00 00 00 00 00 00 01 19 01 00 00 00 00 00 00 00 00 01 09 00 00 00 00 00 00 00 00 00 00 00 00 00 01 15 00 00 00 00 00 00 00 00 00", "", 2),
    ];
    for (hex, name, count) in examples {
        let bytes: Vec<_> = hex
            .split_whitespace()
            .map(|byte| u8::from_str_radix(byte, 16).unwrap())
            .collect();
        let folder = super::super::parse_sketch_folder(&bytes).unwrap();
        assert_eq!(folder.name, name);
        assert_eq!(folder.number, 1);
        assert_eq!(folder.members.len(), count);
        assert_eq!(folder.parent, 9);
        assert_eq!(folder.closing, 21);
        for end in 0..bytes.len() {
            assert!(super::super::parse_sketch_folder(&bytes[..end]).is_none());
        }
    }
}

#[test]
fn folder_sketch_uses_parent_collection() {
    for (name, flag, zero_count) in [("Folder Name", 0, 8), ("", 1, 4)] {
        let (mut bytes, mut metadata) = collection_stream(500, &[(300, &[500])]);
        append_folder(&mut bytes, &mut metadata, name, flag, zero_count);
        let (visibilities, losses) = decode_with_losses(&bytes, &metadata);
        assert_eq!(visibilities[0].1.ordinal_collection.unwrap().get(), 300);
        assert_eq!(losses.len(), 2);
        assert_eq!(
            losses[0].code,
            crate::loss::F3dLossCode::SketchFolderUnprojected.kind()
        );
        assert_eq!(
            losses[1].code,
            crate::loss::F3dLossCode::SketchFolderClosingReference.kind()
        );
        assert!(losses[1].message.contains("record 21"));
    }
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
