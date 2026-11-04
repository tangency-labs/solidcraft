// SPDX-License-Identifier: Apache-2.0

#![allow(
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::trivially_copy_pass_by_ref,
    clippy::uninlined_format_args
)]

use cadmpeg_core::decode::u64_from_index;

use std::io::{Cursor, Write};

use zip::CompressionMethod;

use crate::test_support::lp_ascii;
use crate::test_support::manifest_test::write_synthetic_manifests;
use crate::test_support::streams_test::design_metastream_with_records;
use crate::test_support::zip_test::with_scan;

#[test]
fn flat_pattern_settings_retain_the_complete_type_table_and_related_headers() {
    use crate::records::sheet_metal_settings::{PAIRED_GUID, SETTINGS_GUID};

    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    let meta_name = "FlatPatternAssetSubType/Design1/MetaStream.dat";
    let bulk_name = "FlatPatternAssetSubType/Design1/BulkStream.dat";
    for (version, expected_types) in [(2, 3), (1, 0)] {
        let meta = design_metastream_with_records(
            &[
                (SETTINGS_GUID, "", version, "Fusion", &[50]),
                (PAIRED_GUID, "", 0, "Fusion", &[]),
                (
                    super::FEATURE_TIMELINE_TYPE_GUID,
                    super::FEATURE_TIMELINE_BASE_TYPE_GUID,
                    2,
                    "Fusion",
                    &[35],
                ),
            ],
            &[(50, 0), (35, 11)],
        );
        let mut bulk = Vec::new();
        lp_ascii(&mut bulk, "256");
        bulk.extend_from_slice(&50u32.to_le_bytes());
        lp_ascii(&mut bulk, "258");
        bulk.extend_from_slice(&35u64.to_le_bytes());
        lp_ascii(&mut bulk, "Timeline");
        bulk.extend_from_slice(&[0, 0, 1]);
        bulk.extend_from_slice(&50u64.to_le_bytes());
        bulk.extend_from_slice(&[0, 0]);
        bulk.extend_from_slice(&1u32.to_le_bytes());
        bulk.push(1);
        bulk.extend_from_slice(&50u64.to_le_bytes());
        bulk.extend_from_slice(&[0, 0]);
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        write_synthetic_manifests(&mut zip, stored);
        zip.start_file(meta_name, stored).unwrap();
        zip.write_all(&meta).unwrap();
        zip.start_file(bulk_name, stored).unwrap();
        zip.write_all(&bulk).unwrap();
        let archive = zip.finish().unwrap().into_inner();
        with_scan(&archive, |scan| {
            let _ctx = cadmpeg_test_support::service_decode_context();
            let types = super::decode_types(scan).unwrap();
            assert_eq!(types.len(), expected_types);
            let timelines = super::decode_feature_timelines(scan).unwrap();
            if version == 2 {
                assert_eq!(timelines.len(), 1);
                assert_eq!(timelines[0].record_index.get(), 35);
                assert_eq!(timelines[0].context_record_index.get(), 50);
                assert_eq!(timelines[0].frame().items()[0].value, 50);
                assert_eq!(types[0].type_guid.as_str(), SETTINGS_GUID);
                assert_eq!(types[1].type_guid.as_str(), PAIRED_GUID);
                assert_eq!(
                    types[0].entities.values().copied().collect::<Vec<_>>(),
                    [50]
                );
                let headers = crate::design::decode::sketch::decode_related_record_headers(
                    scan,
                    &[(crate::ids::native_scope(bulk_name), 50)],
                )
                .unwrap();
                assert_eq!(headers.len(), 1);
                assert_eq!(headers[0].record_index, 50);
                assert_eq!(headers[0].byte_offset, 0);
                assert_eq!(headers[0].class_tag.as_str(), "256");
                assert_eq!(
                    crate::ids::native_stream(&headers[0].id),
                    Some(crate::ids::native_scope(bulk_name).as_str())
                );
            } else {
                assert!(timelines.is_empty());
            }
        });
    }
}

use super::decode_component_naming_spaces;

#[test]
fn bulk_metadata_reuses_the_parsed_type_table() {
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    let meta = design_metastream_with_records(
        &[(
            "11111111-2222-3333-4444-555555555555",
            "21F379C8-CAFD-4985-B461-767673A4C502",
            0,
            "Component",
            &[17],
        )],
        &[],
    );
    let bulk_name = "FusionAssetName[Active]/Design1/BulkStream.dat";
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    write_synthetic_manifests(&mut zip, stored);
    zip.start_file(bulk_name, stored).unwrap();
    zip.write_all(&[]).unwrap();
    zip.start_file("FusionAssetName[Active]/Design1/MetaStream.dat", stored)
        .unwrap();
    zip.write_all(&meta).unwrap();
    let bytes = zip.finish().unwrap().into_inner();
    with_scan(&bytes, |scan| {
        let first = super::metadata_for_bulk_stream(scan, bulk_name)?.unwrap();
        let second = super::metadata_for_bulk_stream(scan, bulk_name)?.unwrap();
        assert!(std::sync::Arc::ptr_eq(&first, &second));
        assert_eq!(first.types.len(), 1);
        assert_eq!(
            first.types[0]
                .entities
                .values()
                .copied()
                .collect::<Vec<_>>(),
            [17]
        );
        Ok::<_, cadmpeg_core::CodecError>(())
    })
    .unwrap();
}

#[test]
fn design_type_copy_keeps_table_entities_module_and_id() {
    let meta_name = "FusionAssetName[Active]/Design1/MetaStream.dat";
    let meta = design_metastream_with_records(
        &[(
            "11111111-2222-3333-4444-555555555555",
            "21F379C8-CAFD-4985-B461-767673A4C502",
            0,
            "Component",
            &[17],
        )],
        &[],
    );
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    write_synthetic_manifests(&mut zip, stored);
    zip.start_file("FusionAssetName[Active]/Design1/BulkStream.dat", stored)
        .unwrap();
    zip.write_all(&[]).unwrap();
    zip.start_file(meta_name, stored).unwrap();
    zip.write_all(&meta).unwrap();
    let archive = zip.finish().unwrap().into_inner();
    let types = with_scan(&archive, super::decode_types).unwrap();
    assert_eq!(types.len(), 1);
    assert_eq!(types[0].module, "Component");
    assert_eq!(
        types[0].entities.values().copied().collect::<Vec<_>>(),
        [17]
    );
    assert_eq!(
        types[0].id(),
        &crate::ids::native_design_type_id(meta_name, types[0].byte_offset)
    );
}

#[test]
fn stream_type_indexes_refuse_limits_and_match_escaped_scope() {
    let prefix = "Fusion% Asset:Name[Active]/Design1/";
    let bulk_name = format!("{prefix}BulkStream.dat");
    let meta_name = format!("{prefix}MetaStream.dat");
    let type_guid = "11111111-2222-3333-4444-555555555555";
    let design_type =
        crate::design::test_support::design_type(type_guid, None, 7, "Fusion", vec![17]);
    let design_type = crate::records::entity_header::SegmentType::try_new(
        crate::ids::native_design_type_id(&meta_name, 0),
        design_type,
    )
    .unwrap();
    let types = [design_type];
    let by_entity = super::stream_types_by_entity(&types, &bulk_name);
    assert_eq!(by_entity.get(&17), Some(&(type_guid, 7)));
    let by_class = super::stream_types_by_class_tag(&types, &bulk_name);
    assert_eq!(
        by_class
            .get(&256)
            .map(|design_type| design_type.type_guid.as_str()),
        Some(type_guid)
    );
    assert!(super::stream_types_by_entity(&types, "other/BulkStream.dat").is_empty());
}

#[test]
fn design_primary_frames_index_one_registered_frame() {
    let mut bytes = Vec::new();
    lp_ascii(&mut bytes, "256");
    bytes.extend_from_slice(&42_u32.to_le_bytes());
    let meta = crate::metastream::MetaStream {
        types: vec![crate::design::test_support::design_type(
            "00000000-0000-0000-0000-000000000001",
            None,
            0,
            "Test",
            vec![42],
        )],
        records: vec![crate::design::test_support::primary_record(42, 0)],
        secondary_records: Vec::new(),
    };
    let frames = super::design_primary_frames(&bytes, &meta).unwrap();
    assert_eq!(frames.len(), 1);
    assert_eq!(frames[0].entity_id, 42);
}

#[test]
fn typed_primary_frames_index_one_typed_frame() {
    let mut bytes = Vec::new();
    lp_ascii(&mut bytes, "256");
    bytes.extend_from_slice(&42_u32.to_le_bytes());
    let type_guid = "00000000-0000-0000-0000-000000000001";
    let meta = crate::metastream::MetaStream {
        types: vec![crate::design::test_support::design_type(
            type_guid,
            None,
            0,
            "Test",
            vec![42],
        )],
        records: vec![crate::design::test_support::primary_record(42, 0)],
        secondary_records: Vec::new(),
    };
    let frames = super::typed_primary_frames(&bytes, &meta, type_guid, "test").unwrap();
    assert_eq!(frames.len(), 1);
    assert_eq!(frames[0].entity_id, 42);
}

#[test]
fn feature_timeline_items_follow_the_counted_vector() {
    use std::collections::HashMap;

    let mut bulk = Vec::new();
    lp_ascii(&mut bulk, "256");
    bulk.extend_from_slice(&35_u64.to_le_bytes());
    lp_ascii(&mut bulk, "Timeline");
    bulk.extend_from_slice(&[0, 0]);
    for target in [17_u64, 101, 102] {
        bulk.push(1);
        bulk.extend_from_slice(&target.to_le_bytes());
        bulk.extend_from_slice(&[0, 0]);
        if target == 17 {
            bulk.extend_from_slice(&2_u32.to_le_bytes());
        }
    }
    let frame = 0..bulk.len();
    let admitted = super::parse_feature_timeline_record(
        &bulk,
        "Design/BulkStream.dat",
        frame,
        ("256", 35),
        0,
        &HashMap::new(),
    )
    .unwrap();
    assert_eq!(
        admitted
            .frame()
            .items()
            .iter()
            .map(|item| item.value)
            .collect::<Vec<_>>(),
        [101, 102]
    );
}

/// A timeline record whose item run is `items`, where `None` is a null item.
fn timeline_with_items(items: &[Option<u64>]) -> Vec<u8> {
    let mut bulk = Vec::new();
    lp_ascii(&mut bulk, "256");
    bulk.extend_from_slice(&35_u64.to_le_bytes());
    lp_ascii(&mut bulk, "Timeline");
    bulk.extend_from_slice(&[0, 0, 1]);
    bulk.extend_from_slice(&17_u64.to_le_bytes());
    bulk.extend_from_slice(&[0, 0]);
    bulk.extend_from_slice(&u32::try_from(items.len()).unwrap().to_le_bytes());
    for item in items {
        match item {
            Some(target) => {
                bulk.push(1);
                bulk.extend_from_slice(&target.to_le_bytes());
                bulk.extend_from_slice(&[0, 0]);
            }
            None => bulk.push(0),
        }
    }
    bulk
}

fn timeline_item_values(bulk: &[u8]) -> Option<Vec<u64>> {
    super::parse_feature_timeline_record(
        bulk,
        "Design/BulkStream.dat",
        0..bulk.len(),
        ("256", 35),
        0,
        &std::collections::HashMap::new(),
    )
    .map(|timeline| {
        timeline
            .frame()
            .items()
            .iter()
            .map(|item| item.value)
            .collect()
    })
}

#[test]
fn feature_timeline_trailing_null_items_take_no_position() {
    assert_eq!(
        timeline_item_values(&timeline_with_items(&[Some(101), Some(102), None])),
        Some(vec![101, 102])
    );
    assert_eq!(
        timeline_item_values(&timeline_with_items(&[Some(101), None, None])),
        Some(vec![101])
    );
}

#[test]
fn feature_timeline_rejects_a_reference_after_a_null_item() {
    assert_eq!(
        timeline_item_values(&timeline_with_items(&[Some(101), None, Some(102)])),
        None
    );
}

#[test]
fn feature_timeline_id_escapes_its_stream_scope() {
    use std::collections::HashMap;

    let stream = "Fusion% Asset:Name[Active]/Design1/BulkStream.dat";
    let mut bulk = Vec::new();
    lp_ascii(&mut bulk, "256");
    bulk.extend_from_slice(&35_u64.to_le_bytes());
    lp_ascii(&mut bulk, "Timeline");
    bulk.extend_from_slice(&[0, 0, 1]);
    bulk.extend_from_slice(&17_u64.to_le_bytes());
    bulk.extend_from_slice(&[0, 0]);
    bulk.extend_from_slice(&0_u32.to_le_bytes());
    let timeline = super::parse_feature_timeline_record(
        &bulk,
        stream,
        0..bulk.len(),
        ("256", 35),
        0,
        &HashMap::new(),
    )
    .unwrap();
    let expected_id = crate::ids::native_design_feature_timeline_id(stream, 0);
    assert_eq!(timeline.id(), &expected_id);
}

#[test]
fn feature_timelines_decode_one_timeline_record() {
    let mut bulk = Vec::new();
    lp_ascii(&mut bulk, "256");
    bulk.extend_from_slice(&35_u64.to_le_bytes());
    lp_ascii(&mut bulk, "Timeline");
    bulk.extend_from_slice(&[0, 0, 1]);
    bulk.extend_from_slice(&17_u64.to_le_bytes());
    bulk.extend_from_slice(&[0, 0]);
    bulk.extend_from_slice(&0_u32.to_le_bytes());
    let meta = design_metastream_with_records(
        &[
            (
                super::FEATURE_TIMELINE_TYPE_GUID,
                super::FEATURE_TIMELINE_BASE_TYPE_GUID,
                super::FEATURE_TIMELINE_TYPE_VERSIONS[0],
                "Fusion",
                &[35],
            ),
            (
                "11111111-2222-3333-4444-555555555555",
                "",
                0,
                "Fusion",
                &[17],
            ),
        ],
        &[(35, 0)],
    );
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    write_synthetic_manifests(&mut zip, stored);
    zip.start_file("FusionAssetName[Active]/Design1/BulkStream.dat", stored)
        .unwrap();
    zip.write_all(&bulk).unwrap();
    zip.start_file("FusionAssetName[Active]/Design1/MetaStream.dat", stored)
        .unwrap();
    zip.write_all(&meta).unwrap();
    let archive = zip.finish().unwrap().into_inner();
    let admitted = with_scan(&archive, super::decode_feature_timelines).unwrap();
    assert_eq!(admitted.len(), 1);
    assert_eq!(admitted[0].record_index.get(), 35);
}

#[test]
fn component_naming_space_binds_component_entity_to_context_uuid() {
    const COMPONENT_TYPE_GUID: &str = "11111111-2222-3333-4444-555555555555";
    const CONTEXT_UUID: &str = "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee";

    fn archive(bulk: &[u8]) -> Vec<u8> {
        let stored = crate::zip_write::file_options(CompressionMethod::Stored);
        let meta = design_metastream_with_records(
            &[(
                COMPONENT_TYPE_GUID,
                "21F379C8-CAFD-4985-B461-767673A4C502",
                0,
                "Component",
                &[17],
            )],
            &[],
        );
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        write_synthetic_manifests(&mut zip, stored);
        zip.start_file("FusionAssetName[Active]/Design1/BulkStream.dat", stored)
            .unwrap();
        zip.write_all(bulk).unwrap();
        zip.start_file("FusionAssetName[Active]/Design1/MetaStream.dat", stored)
            .unwrap();
        zip.write_all(&meta).unwrap();
        zip.finish().unwrap().into_inner()
    }

    fn binding(out: &mut Vec<u8>, component: u64, reserved_len: usize, context_uuid: &str) {
        out.push(1);
        out.extend_from_slice(&component.to_le_bytes());
        out.extend(std::iter::repeat_n(0, reserved_len));
        out.extend_from_slice(&36_u32.to_le_bytes());
        for code_unit in context_uuid.encode_utf16() {
            out.extend_from_slice(&code_unit.to_le_bytes());
        }
    }

    fn typed_binding(out: &mut Vec<u8>, component: u64, reserved_len: usize, context_uuid: &str) {
        out.push(1);
        out.extend_from_slice(&component.to_le_bytes());
        out.extend_from_slice(
            &(u32::try_from(COMPONENT_TYPE_GUID.len()).expect("fixture value fits u32"))
                .to_le_bytes(),
        );
        out.extend_from_slice(COMPONENT_TYPE_GUID.as_bytes());
        out.extend(std::iter::repeat_n(0, reserved_len));
        out.extend_from_slice(&36_u32.to_le_bytes());
        for code_unit in context_uuid.encode_utf16() {
            out.extend_from_slice(&code_unit.to_le_bytes());
        }
    }

    for reserved_len in [2, 3] {
        let mut bulk = vec![0xaa, 0xbb];
        let marker = bulk.len();
        binding(&mut bulk, 17, reserved_len, CONTEXT_UUID);
        let decoded = with_scan(&archive(&bulk), decode_component_naming_spaces)
            .expect("component naming space")
            .spaces;
        let [space] = decoded.as_slice() else {
            panic!("expected one component naming space");
        };
        assert_eq!(space.component_record_index, 17);
        assert_eq!(space.context_uuid.as_str(), CONTEXT_UUID);
        assert_eq!(space.byte_offset, u64_from_index(marker));
        assert_eq!(
            space.context_uuid_offset,
            u64_from_index(marker + 9 + reserved_len)
        );
    }

    for reserved_len in [2, 3] {
        let mut typed = vec![0xaa, 0xbb];
        let typed_marker = typed.len();
        typed_binding(&mut typed, 17, reserved_len, CONTEXT_UUID);
        let decoded = with_scan(&archive(&typed), |scan| {
            decode_component_naming_spaces(scan).map(|decoded| decoded.spaces)
        })
        .expect("typed component naming space");
        let [space] = decoded.as_slice() else {
            panic!("expected one typed component naming space");
        };
        assert_eq!(space.component_record_index, 17);
        assert_eq!(space.context_uuid.as_str(), CONTEXT_UUID);
        assert_eq!(space.byte_offset, u64_from_index(typed_marker));
        assert_eq!(
            space.context_uuid_offset,
            u64_from_index(typed_marker + 9 + 4 + COMPONENT_TYPE_GUID.len() + reserved_len)
        );
    }

    // The component's definition record ends with its name after the UUID;
    // a name that does not end the record is not the component's.
    for (record_end, expected) in [(true, Some("Base0")), (false, None)] {
        let mut named = vec![0xaa, 0xbb];
        binding(&mut named, 17, 2, CONTEXT_UUID);
        let name_at = named.len();
        named.extend_from_slice(&5_u32.to_le_bytes());
        for code_unit in "Base0".encode_utf16() {
            named.extend_from_slice(&code_unit.to_le_bytes());
        }
        named.extend_from_slice(&[0, 0]);
        if record_end {
            named.extend_from_slice(&3_u32.to_le_bytes());
            named.extend_from_slice(b"262");
            named.extend_from_slice(&18_u32.to_le_bytes());
        } else {
            named.extend_from_slice(&[7; 11]);
        }
        let decoded = with_scan(&archive(&named), decode_component_naming_spaces)
            .expect("named component naming space")
            .spaces;
        let [space] = decoded.as_slice() else {
            panic!("expected one component naming space");
        };
        assert_eq!(space.name.as_deref(), expected);
        assert_eq!(space.name_offset, expected.map(|_| u64_from_index(name_at)));
    }

    let mut overlapping_reference = vec![1];
    binding(
        &mut overlapping_reference,
        17,
        2,
        "ffffffff-eeee-4ddd-8ccc-bbbbbbbbbbbb",
    );
    typed_binding(&mut overlapping_reference, 17, 2, CONTEXT_UUID);
    let decoded = with_scan(&archive(&overlapping_reference), |scan| {
        decode_component_naming_spaces(scan).map(|decoded| decoded.spaces)
    })
    .expect("typed binding beside an overlapping 01 01 reference");
    let [space] = decoded.as_slice() else {
        panic!("expected one component naming space");
    };
    assert_eq!(space.context_uuid.as_str(), CONTEXT_UUID);

    // A component definition stored as two records that repeat one record
    // index, each naming the component, binds one context UUID in each.
    let revised_uuid = "ffffffff-eeee-4ddd-8ccc-bbbbbbbbbbbb";
    let header = |out: &mut Vec<u8>, class_tag: &[u8; 3], record_index: u32| {
        out.extend_from_slice(&3_u32.to_le_bytes());
        out.extend_from_slice(class_tag);
        out.extend_from_slice(&record_index.to_le_bytes());
    };
    let named = |out: &mut Vec<u8>, name: &str| {
        out.extend_from_slice(
            &(u32::try_from(name.len()).expect("fixture value fits u32")).to_le_bytes(),
        );
        for code_unit in name.encode_utf16() {
            out.extend_from_slice(&code_unit.to_le_bytes());
        }
        out.extend_from_slice(&[0, 0]);
    };
    let revisions = |second_index: u32, second_name: &str| {
        let mut bulk = Vec::new();
        header(&mut bulk, b"290", 16);
        binding(&mut bulk, 17, 3, CONTEXT_UUID);
        named(&mut bulk, "Feet");
        header(&mut bulk, b"334", second_index);
        binding(&mut bulk, 17, 3, revised_uuid);
        named(&mut bulk, second_name);
        header(&mut bulk, b"300", 99);
        bulk
    };
    let decoded = with_scan(
        &archive(&revisions(16, "Feet")),
        decode_component_naming_spaces,
    )
    .expect("one binding per revision")
    .spaces;
    let mut bound = decoded
        .iter()
        .map(|space| (space.context_uuid.as_str(), space.name.as_deref()))
        .collect::<Vec<_>>();
    bound.sort_unstable();
    assert_eq!(
        bound,
        [(CONTEXT_UUID, Some("Feet")), (revised_uuid, Some("Feet"))]
    );
    // A second UUID in a record with another index, or under another name,
    // is no revision.
    assert!(with_scan(
        &archive(&revisions(18, "Feet")),
        decode_component_naming_spaces
    )
    .is_err());
    assert!(with_scan(
        &archive(&revisions(16, "Legs")),
        decode_component_naming_spaces
    )
    .is_err());

    let mut conflicting = Vec::new();
    binding(&mut conflicting, 17, 3, CONTEXT_UUID);
    binding(
        &mut conflicting,
        17,
        3,
        "ffffffff-eeee-4ddd-8ccc-bbbbbbbbbbbb",
    );
    let error = with_scan(&archive(&conflicting), |scan| {
        decode_component_naming_spaces(scan)
    })
    .expect_err("conflicting component UUIDs must be rejected");
    assert!(matches!(error, cadmpeg_core::CodecError::Malformed(_)));
}

#[test]
fn component_naming_space_binds_through_a_linked_entity() {
    const CONTEXT_UUID: &str = "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee";

    /// The link entity 5 is registered to `link_type_guid`, or to no type.
    fn archive(link_type_guid: Option<&str>) -> Vec<u8> {
        let component = (
            "11111111-2222-3333-4444-555555555555",
            "21F379C8-CAFD-4985-B461-767673A4C502",
            0,
            "Component",
            &[17_u64][..],
        );
        let types = match link_type_guid {
            Some(link_type_guid) => vec![
                component,
                (
                    link_type_guid,
                    "046D6232-EDAB-414D-9CB0-A2E3F115AE1C",
                    4,
                    "Fusion",
                    &[5_u64][..],
                ),
            ],
            None => vec![component],
        };
        let meta = design_metastream_with_records(&types, &[]);
        let mut bulk = vec![0xaa, 0xbb, 1];
        bulk.extend_from_slice(&17_u64.to_le_bytes());
        bulk.extend_from_slice(&[0, 0, 1]);
        bulk.extend_from_slice(&5_u64.to_le_bytes());
        bulk.extend_from_slice(&[0, 0]);
        crate::test_support::lp_utf16(&mut bulk, CONTEXT_UUID);
        let stored = crate::zip_write::file_options(CompressionMethod::Stored);
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        write_synthetic_manifests(&mut zip, stored);
        zip.start_file("FusionAssetName[Active]/Design1/BulkStream.dat", stored)
            .unwrap();
        zip.write_all(&bulk).unwrap();
        zip.start_file("FusionAssetName[Active]/Design1/MetaStream.dat", stored)
            .unwrap();
        zip.write_all(&meta).unwrap();
        zip.finish().unwrap().into_inner()
    }

    let spaces = with_scan(
        &archive(Some("8ACBA2E5-5A4C-4438-8351-276DE96E6982")),
        decode_component_naming_spaces,
    )
    .expect("linked component naming space")
    .spaces;
    let [space] = spaces.as_slice() else {
        panic!("expected one component naming space");
    };
    assert_eq!(space.component_record_index, 17);
    assert_eq!(space.context_uuid.as_str(), CONTEXT_UUID);
    assert_eq!(space.byte_offset, 2);
    assert_eq!(space.context_uuid_offset, u64_from_index(2 + 11 + 11));

    // A link entity the stream registers to no type binds the same way.
    let unregistered = with_scan(&archive(None), decode_component_naming_spaces)
        .expect("component naming space through an unregistered entity")
        .spaces;
    assert_eq!(unregistered, spaces);

    let unbound = with_scan(
        &archive(Some("99999999-8888-7777-6666-555555555555")),
        decode_component_naming_spaces,
    )
    .expect("a link entity of another type leaves the component unbound");
    assert!(unbound.spaces.is_empty());
    assert_eq!(unbound.unbound, 1);
}

#[test]
fn component_naming_space_keeps_context_and_id() {
    let bulk_name = "FusionAssetName[Active]/Design1/BulkStream.dat";
    let context_uuid = "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee";
    let meta = design_metastream_with_records(
        &[(
            "11111111-2222-3333-4444-555555555555",
            "21F379C8-CAFD-4985-B461-767673A4C502",
            0,
            "Component",
            &[17],
        )],
        &[],
    );
    let mut bulk = vec![0xaa, 0xbb, 1];
    bulk.extend_from_slice(&17_u64.to_le_bytes());
    bulk.extend_from_slice(&[0, 0]);
    crate::test_support::lp_utf16(&mut bulk, context_uuid);
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    write_synthetic_manifests(&mut zip, stored);
    zip.start_file(bulk_name, stored).unwrap();
    zip.write_all(&bulk).unwrap();
    zip.start_file("FusionAssetName[Active]/Design1/MetaStream.dat", stored)
        .unwrap();
    zip.write_all(&meta).unwrap();
    let archive = zip.finish().unwrap().into_inner();
    let spaces = with_scan(&archive, super::decode_component_naming_spaces)
        .unwrap()
        .spaces;
    assert_eq!(spaces.len(), 1);
    assert_eq!(spaces[0].context_uuid.as_str(), context_uuid);
    assert_eq!(
        spaces[0].id,
        crate::ids::native_design_component_naming_space_id(bulk_name, 2)
    );
}

#[test]
fn unbound_component_entity_is_tolerated_and_reported() {
    fn archive(bulk: &[u8]) -> Vec<u8> {
        let stored = crate::zip_write::file_options(CompressionMethod::Stored);
        let meta = design_metastream_with_records(
            &[(
                "11111111-2222-3333-4444-555555555555",
                "21F379C8-CAFD-4985-B461-767673A4C502",
                0,
                "Component",
                &[17],
            )],
            &[],
        );
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        write_synthetic_manifests(&mut zip, stored);
        zip.start_file("FusionAssetName[Active]/Design1/BulkStream.dat", stored)
            .unwrap();
        zip.write_all(bulk).unwrap();
        zip.start_file("FusionAssetName[Active]/Design1/MetaStream.dat", stored)
            .unwrap();
        zip.write_all(&meta).unwrap();
        zip.finish().unwrap().into_inner()
    }

    let decoded = with_scan(&archive(&[]), decode_component_naming_spaces)
        .expect("an unbound component entity is tolerated");
    assert!(decoded.spaces.is_empty());
    assert_eq!(decoded.unbound, 1);
}

#[test]
fn partially_unbound_stream_keeps_the_bound_component() {
    const COMPONENT_TYPE_GUID: &str = "11111111-2222-3333-4444-555555555555";
    const CONTEXT_UUID: &str = "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee";

    fn archive(bulk: &[u8], entities: &[u64]) -> Vec<u8> {
        let stored = crate::zip_write::file_options(CompressionMethod::Stored);
        let meta = design_metastream_with_records(
            &[(
                COMPONENT_TYPE_GUID,
                "21F379C8-CAFD-4985-B461-767673A4C502",
                0,
                "Component",
                entities,
            )],
            &[],
        );
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        write_synthetic_manifests(&mut zip, stored);
        zip.start_file("FusionAssetName[Active]/Design1/BulkStream.dat", stored)
            .unwrap();
        zip.write_all(bulk).unwrap();
        zip.start_file("FusionAssetName[Active]/Design1/MetaStream.dat", stored)
            .unwrap();
        zip.write_all(&meta).unwrap();
        zip.finish().unwrap().into_inner()
    }

    let mut bulk = vec![0xaa, 0xbb, 1];
    bulk.extend_from_slice(&17_u64.to_le_bytes());
    bulk.extend_from_slice(&[0, 0]);
    crate::test_support::lp_utf16(&mut bulk, CONTEXT_UUID);
    let decoded = with_scan(&archive(&bulk, &[17, 18]), decode_component_naming_spaces)
        .expect("a partially unbound stream keeps its bound component");
    let [space] = decoded.spaces.as_slice() else {
        panic!("expected one component naming space");
    };
    assert_eq!(space.component_record_index, 17);
    assert_eq!(space.context_uuid.as_str(), CONTEXT_UUID);
    assert_eq!(decoded.unbound, 1);
}

#[test]
fn design_feature_timeline_versions_share_variable_width_local_references() {
    const INLINE_TYPE_GUID: &str = "11111111-2222-3333-4444-555555555555";

    fn local_reference(out: &mut Vec<u8>, target: u64, inline_type: bool) {
        out.push(1);
        out.extend_from_slice(&target.to_le_bytes());
        if inline_type {
            lp_ascii(out, INLINE_TYPE_GUID);
        }
        out.extend_from_slice(&[0, 0]);
    }
    fn archive(meta: &[u8], bulk: &[u8]) -> Vec<u8> {
        let stored = crate::zip_write::file_options(CompressionMethod::Stored);
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        write_synthetic_manifests(&mut zip, stored);
        zip.start_file("FusionAssetName[Active]/Design1/BulkStream.dat", stored)
            .unwrap();
        zip.write_all(bulk).unwrap();
        zip.start_file("FusionAssetName[Active]/Design1/MetaStream.dat", stored)
            .unwrap();
        zip.write_all(meta).unwrap();
        zip.finish().unwrap().into_inner()
    }

    let mut bulk = Vec::new();
    lp_ascii(&mut bulk, "256");
    bulk.extend_from_slice(&35_u64.to_le_bytes());
    lp_ascii(&mut bulk, "Timeline");
    bulk.extend_from_slice(&[0, 0]);
    local_reference(&mut bulk, 17, false);
    bulk.extend_from_slice(&2_u32.to_le_bytes());
    local_reference(&mut bulk, 101, false);
    local_reference(&mut bulk, 102, true);
    for version in crate::design::decode::meta::FEATURE_TIMELINE_TYPE_VERSIONS {
        let meta = design_metastream_with_records(
            &[
                (
                    crate::design::decode::meta::FEATURE_TIMELINE_TYPE_GUID,
                    crate::design::decode::meta::FEATURE_TIMELINE_BASE_TYPE_GUID,
                    version,
                    "Fusion",
                    &[35],
                ),
                (INLINE_TYPE_GUID, "", 0, "Fusion", &[17, 101, 102]),
            ],
            &[(35, 0)],
        );
        let decoded = with_scan(&archive(&meta, &bulk), |scan| {
            crate::design::decode::meta::decode_feature_timelines(scan)
        })
        .expect("exact feature timeline");
        let [timeline] = decoded.as_slice() else {
            panic!("expected one timeline record");
        };
        assert_eq!(timeline.record_index.get(), 35);
        assert_eq!(timeline.context_record_index.get(), 17);
        assert_eq!(
            timeline
                .frame()
                .items()
                .iter()
                .map(|item| item.value)
                .collect::<Vec<_>>(),
            [101, 102]
        );
        assert_eq!(timeline.frame().frame_length(), u64_from_index(bulk.len()));
        for item in timeline.frame().items() {
            assert_eq!(
                u64::from_le_bytes(
                    bulk[usize::try_from(item.offset).expect("fixture offset fits address space")
                        ..usize::try_from(item.offset).expect("fixture offset fits address space")
                            + 8]
                        .try_into()
                        .expect("timeline target")
                ),
                item.value
            );
        }

        let mut duplicate = bulk.clone();
        let second_offset = usize::try_from(timeline.frame().items()[1].offset)
            .expect("fixture offset fits address space");
        duplicate[second_offset..second_offset + 8].copy_from_slice(&101_u64.to_le_bytes());
        let error = with_scan(&archive(&meta, &duplicate), |scan| {
            crate::design::decode::meta::decode_feature_timelines(scan)
        })
        .expect_err("duplicate timeline items must be rejected");
        assert!(matches!(error, cadmpeg_core::CodecError::Malformed(_)));

        let mut mismatched_inline_type = bulk.clone();
        let inline_type_at = mismatched_inline_type
            .windows(INLINE_TYPE_GUID.len())
            .position(|window| window == INLINE_TYPE_GUID.as_bytes())
            .expect("inline type GUID");
        mismatched_inline_type[inline_type_at] = b'2';
        let error = with_scan(&archive(&meta, &mismatched_inline_type), |scan| {
            crate::design::decode::meta::decode_feature_timelines(scan)
        })
        .expect_err("an inline type GUID must match the target registration");
        assert!(matches!(error, cadmpeg_core::CodecError::Malformed(_)));
    }

    let unsupported_meta = design_metastream_with_records(
        &[(
            crate::design::decode::meta::FEATURE_TIMELINE_TYPE_GUID,
            crate::design::decode::meta::FEATURE_TIMELINE_BASE_TYPE_GUID,
            4,
            "Fusion",
            &[35],
        )],
        &[(35, 0)],
    );
    let error = with_scan(&archive(&unsupported_meta, &bulk), |scan| {
        crate::design::decode::meta::decode_feature_timelines(scan)
    })
    .expect_err("an unsupported timeline version must not use a known frame speculatively");
    assert!(matches!(error, cadmpeg_core::CodecError::NotImplemented(_)));

    for (base_type_guid, module) in [
        ("22222222-3333-4444-5555-666666666666", "Fusion"),
        (
            crate::design::decode::meta::FEATURE_TIMELINE_BASE_TYPE_GUID,
            "Other",
        ),
    ] {
        let incompatible_meta = design_metastream_with_records(
            &[(
                crate::design::decode::meta::FEATURE_TIMELINE_TYPE_GUID,
                base_type_guid,
                crate::design::decode::meta::FEATURE_TIMELINE_TYPE_VERSIONS[1],
                module,
                &[35],
            )],
            &[(35, 0)],
        );
        let error = with_scan(&archive(&incompatible_meta, &bulk), |scan| {
            crate::design::decode::meta::decode_feature_timelines(scan)
        })
        .expect_err("incompatible timeline registration metadata must be rejected");
        assert!(matches!(error, cadmpeg_core::CodecError::Malformed(_)));
    }
}

#[test]
fn a_design_bulk_stream_ends_before_an_unindexed_record_run() {
    fn record(out: &mut Vec<u8>, class: &[u8; 3], record_index: u32) {
        out.extend_from_slice(&3_u32.to_le_bytes());
        out.extend_from_slice(class);
        out.extend_from_slice(&record_index.to_le_bytes());
        out.extend_from_slice(&[0; 10]);
    }
    fn archive(trailing_index: u32) -> Vec<u8> {
        let mut bulk = Vec::new();
        record(&mut bulk, b"300", 1);
        record(&mut bulk, b"300", 2);
        record(&mut bulk, b"301", trailing_index);
        record(&mut bulk, b"302", trailing_index + 1);
        let meta = design_metastream_with_records(&[], &[(1, 0), (2, 21)]);
        let stored = crate::zip_write::file_options(CompressionMethod::Stored);
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        write_synthetic_manifests(&mut zip, stored);
        zip.start_file("FusionAssetName[Active]/Design1/BulkStream.dat", stored)
            .unwrap();
        zip.write_all(&bulk).unwrap();
        zip.start_file("FusionAssetName[Active]/Design1/MetaStream.dat", stored)
            .unwrap();
        zip.write_all(&meta).unwrap();
        zip.finish().unwrap().into_inner()
    }
    let bulk = "FusionAssetName[Active]/Design1/BulkStream.dat";

    with_scan(&archive(1), |scan| {
        assert_eq!(scan.entry_bytes(bulk).unwrap().len(), 42);
        let [(name, run, stored)] = scan.unindexed_record_runs.as_slice() else {
            panic!("one unindexed record run");
        };
        assert_eq!(name, bulk);
        assert_eq!(
            *run,
            super::UnindexedRecordRun {
                start: 42,
                first_record_index: 1,
                headers: 2,
            }
        );
        assert_eq!(*stored, 84);
        let mut losses = Vec::new();
        crate::report::header_losses(scan, &mut losses).unwrap();
        assert!(losses
            .iter()
            .any(|loss| loss.code == crate::loss::F3dLossCode::UnindexedRecordsRetained.kind()));
    });

    with_scan(&archive(2), |scan| {
        assert_eq!(scan.entry_bytes(bulk).unwrap().len(), 84);
        assert!(scan.unindexed_record_runs.is_empty());
    });
}
