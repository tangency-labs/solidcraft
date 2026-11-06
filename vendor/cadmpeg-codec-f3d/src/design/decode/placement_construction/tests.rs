// SPDX-License-Identifier: Apache-2.0
use crate::records::placement_construction::DesignPlacementConstruction;
use cadmpeg_core::decode::View;
use cadmpeg_ir::native::bytes::NativeBytes;

#[test]
fn placement_source_frames_preserve_versioned_members_and_absolute_offsets() {
    crate::test_support::with_decode_context(|ctx| {
        for fixture in crate::test_support::placement_construction::frames() {
            let fixture: serde_json::Value = serde_json::from_str(&fixture).unwrap();
            let raw: NativeBytes<Vec<u8>> =
                serde_json::from_value(fixture["source_bytes"].clone()).unwrap();
            let expected: DesignPlacementConstruction =
                serde_json::from_value(fixture["native"].clone()).unwrap();
            let mut bytes = vec![0; 17];
            bytes.extend_from_slice(&raw);
            let view = View::over_retained(&bytes).child(17, bytes.len()).unwrap();
            let record = super::decode_frame(ctx, expected.id.clone(), view, expected.type_version)
                .unwrap()
                .unwrap();
            assert_eq!(record, expected);
            assert!(super::decode_frame(ctx, record.id.clone(), view, 99)
                .unwrap()
                .is_none());
            let truncated = View::over_retained(&bytes)
                .child(17, usize::try_from(record.fields.ref_type.offset).unwrap())
                .unwrap();
            assert!(
                super::decode_frame(ctx, record.id, truncated, record.type_version)
                    .unwrap()
                    .is_none()
            );
        }
    });
}

#[test]
fn placement_registration_requires_semantic_identity_matching_headers_and_a_paired_bound() {
    use crate::test_support::placement_construction::{frames, metadata};
    let fixture: serde_json::Value = serde_json::from_str(&frames()[0]).unwrap();
    let raw: NativeBytes<Vec<u8>> =
        serde_json::from_value(fixture["source_bytes"].clone()).unwrap();
    let expected: DesignPlacementConstruction =
        serde_json::from_value(fixture["native"].clone()).unwrap();
    let mut bytes = vec![0; 17];
    bytes.extend_from_slice(&raw);
    bytes[21..24].copy_from_slice(b"256");
    let member_end = bytes.len();
    crate::test_support::indexed_header(
        &mut bytes,
        *b"257",
        u32::try_from(expected.record_index).unwrap(),
    );
    let frame = crate::metastream::PrimaryRecordFrame {
        entity_id: expected.record_index,
        start: 17,
        member_end,
        end: bytes.len(),
    };
    for (version, type_guid, base_guid, secondary, wanted, unparsed) in [
        (14, super::TYPE_GUID, super::BASE_GUID, true, 1, 0),
        (99, super::TYPE_GUID, super::BASE_GUID, true, 0, 1),
        (14, super::BASE_GUID, super::BASE_GUID, true, 0, 0),
        (
            14,
            super::TYPE_GUID,
            "6144EA59-3ACA-4172-9F14-A6531E94D69B",
            true,
            0,
            1,
        ),
        (14, super::TYPE_GUID, super::BASE_GUID, false, 0, 1),
    ] {
        let meta = metadata(frame, version, type_guid, base_guid, secondary);
        let archive = crate::test_support::body_reference_tail_test::archive(&bytes, &meta);
        crate::test_support::with_decode_context(|ctx| {
            crate::test_support::zip_test::with_scan(&archive, |scan| {
                let decoded = super::decode(ctx, scan).unwrap();
                assert_eq!(decoded.records.len(), wanted);
                assert_eq!(decoded.unparsed, unparsed);
            });
        });
    }
    for (offset, corruption) in [
        (21, b"257".as_slice()),
        (24, &[0, 0, 0, 0][..]),
        (member_end, &[2, 0, 0, 0][..]),
        (member_end + 4, b"bad".as_slice()),
        (member_end + 7, &[0, 0, 0, 0][..]),
    ] {
        let mut corrupt = bytes.clone();
        corrupt[offset..offset + corruption.len()].copy_from_slice(corruption);
        let meta = metadata(frame, 14, super::TYPE_GUID, super::BASE_GUID, true);
        let archive = crate::test_support::body_reference_tail_test::archive(&corrupt, &meta);
        crate::test_support::with_decode_context(|ctx| {
            crate::test_support::zip_test::with_scan(&archive, |scan| {
                let decoded = super::decode(ctx, scan).unwrap();
                assert!(decoded.records.is_empty());
                assert_eq!(decoded.unparsed, 1);
            });
        });
    }
}

#[test]
fn placement_reader_refuses_invalid_identity_and_unbounded_reference_counts() {
    let fixture: serde_json::Value =
        serde_json::from_str(&crate::test_support::placement_construction::frames()[0]).unwrap();
    let raw: NativeBytes<Vec<u8>> =
        serde_json::from_value(fixture["source_bytes"].clone()).unwrap();
    let expected: DesignPlacementConstruction =
        serde_json::from_value(fixture["native"].clone()).unwrap();
    let mut source = vec![0; 17];
    source.extend_from_slice(&raw);
    let identity = usize::try_from(expected.fields.identity.as_ref().unwrap().offset).unwrap();
    let counts = [
        expected
            .fields
            .edge_references
            .as_ref()
            .unwrap()
            .count
            .offset,
        expected.fields.plane_references.count.offset,
        expected.fields.values.count.offset,
    ];
    crate::test_support::with_decode_context(|ctx| {
        let mut invalid = source.clone();
        invalid[identity] = 2;
        let view = View::over_retained(&invalid)
            .child(17, invalid.len())
            .unwrap();
        assert!(
            super::decode_frame(ctx, expected.id.clone(), view, expected.type_version)
                .unwrap()
                .is_none()
        );
        for count in counts {
            let mut invalid = source.clone();
            let offset = usize::try_from(count).unwrap();
            invalid[offset..offset + 4].copy_from_slice(&u32::MAX.to_le_bytes());
            let view = View::over_retained(&invalid)
                .child(17, invalid.len())
                .unwrap();
            assert!(
                super::decode_frame(ctx, expected.id.clone(), view, expected.type_version)
                    .unwrap()
                    .is_none()
            );
        }
    });
}

#[test]
fn placement_modern_versions_decode_explicit_and_identity_frames() {
    use crate::test_support::placement_construction::modern_frame;
    crate::test_support::with_decode_context(|ctx| {
        for version in [15, 16] {
            for identity in [false, true] {
                let bytes = modern_frame(version, identity);
                let view = View::over_retained(&bytes);
                let record = super::decode_frame(ctx, "f3d:test:placement".into(), view, version)
                    .unwrap()
                    .unwrap();
                assert_eq!(record.fields.identity.as_ref().unwrap().value, identity);
                assert_eq!(record.fields.matrix.is_some(), !identity);
                assert_eq!(record.fields.periodic_face_mode.is_some(), version == 16);
                assert_eq!(
                    record.fields.arc_length_for_periodic_face.is_some(),
                    version == 15
                );
                assert_eq!(
                    record.fields.extra_offset.as_ref().unwrap().local_target(),
                    Some(100)
                );
                assert_eq!(record.fields.values.members[0].local_target(), Some(200));
                assert_eq!(record.fields.ref_type.value, 3);
                if let Some(matrix) = record.fields.matrix {
                    assert_eq!(matrix.value.rows()[0][3], 2.0);
                    assert_eq!(matrix.value.rows()[1][3], 3.0);
                    assert_eq!(matrix.value.rows()[2][3], 4.0);
                }
                let wire = serde_json::to_value(&record).unwrap();
                let restored: DesignPlacementConstruction =
                    serde_json::from_value(wire.clone()).unwrap();
                assert_eq!(restored, record);
                for field in ["periodic_face_mode", "arc_length_for_periodic_face"] {
                    let mut invalid = wire.clone();
                    invalid["fields"][field] = serde_json::Value::Null;
                    if (field == "periodic_face_mode") == (version == 16) {
                        assert!(
                            serde_json::from_value::<DesignPlacementConstruction>(invalid).is_err()
                        );
                    }
                }
                let mut wrong_version = wire.clone();
                wrong_version["type_version"] =
                    serde_json::json!(if version == 16 { 15 } else { 16 });
                assert!(
                    serde_json::from_value::<DesignPlacementConstruction>(wrong_version).is_err()
                );
                if version == 16 {
                    let mut wrong_offset = wire.clone();
                    wrong_offset["fields"]["periodic_face_mode"]["offset"] = serde_json::json!(47);
                    assert!(
                        serde_json::from_value::<DesignPlacementConstruction>(wrong_offset)
                            .is_err()
                    );
                }
                let mut invalid = wire.clone();
                invalid["type_version"] = serde_json::json!(17);
                assert!(serde_json::from_value::<DesignPlacementConstruction>(invalid).is_err());
                let mut invalid = bytes.clone();
                let identity_offset =
                    usize::try_from(record.fields.identity.unwrap().offset).unwrap();
                invalid[identity_offset] = 2;
                assert!(super::decode_frame(
                    ctx,
                    record.id.clone(),
                    View::over_retained(&invalid),
                    version
                )
                .unwrap()
                .is_none());
                assert!(super::decode_frame(ctx, record.id, view, 17)
                    .unwrap()
                    .is_none());
            }
        }
    });
}

#[test]
fn placement_modern_registration_uses_the_same_version_gate_as_native_validation() {
    use crate::test_support::placement_construction::{metadata, modern_frame};
    for version in [15, 16] {
        for identity in [false, true] {
            let mut bytes = modern_frame(version, identity);
            bytes[4..7].copy_from_slice(b"256");
            let member_end = bytes.len();
            crate::test_support::indexed_header(&mut bytes, *b"257", 10);
            let frame = crate::metastream::PrimaryRecordFrame {
                entity_id: 10,
                start: 0,
                member_end,
                end: bytes.len(),
            };
            let meta = metadata(frame, version, super::TYPE_GUID, super::BASE_GUID, true);
            let archive = crate::test_support::body_reference_tail_test::archive(&bytes, &meta);
            crate::test_support::with_decode_context(|ctx| {
                crate::test_support::zip_test::with_scan(&archive, |scan| {
                    let decoded = super::decode(ctx, scan).unwrap();
                    assert_eq!(decoded.records.len(), 1);
                    assert_eq!(decoded.unparsed, 0);
                    let record = &decoded.records[0];
                    assert_eq!(record.type_version, version);
                    assert_eq!(record.fields.identity.as_ref().unwrap().value, identity);
                });
            });
        }
    }
}
