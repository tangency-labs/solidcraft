// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn local_references_distinguish_null_present_and_malformed_frames() {
    assert_eq!(local(&mut View::over_retained(&[0])), Ok(None));
    let mut present = vec![1];
    present.extend_from_slice(&51u64.to_le_bytes());
    present.extend_from_slice(&[0, 0]);
    assert_eq!(local(&mut View::over_retained(&present)), Ok(Some(51)));
    for length in 0..present.len() {
        assert_eq!(local(&mut View::over_retained(&present[..length])), Err(()));
    }
    present[10] = 1;
    assert_eq!(local(&mut View::over_retained(&present)), Err(()));
    assert_eq!(local(&mut View::over_retained(&[2])), Err(()));
}

fn fixture(version: u32, rule_reference_tail: &[u8]) -> (Vec<u8>, PrimaryRecordFrame) {
    fn header(bytes: &mut Vec<u8>, tag: [u8; 3]) {
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(&tag);
        bytes.extend_from_slice(&50u32.to_le_bytes());
    }
    fn reference(bytes: &mut Vec<u8>, index: u64) {
        bytes.push(1);
        bytes.extend_from_slice(&index.to_le_bytes());
        bytes.extend_from_slice(&[0, 0]);
    }
    let mut bytes = Vec::new();
    header(&mut bytes, *b"256");
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&4u32.to_le_bytes());
    bytes.extend_from_slice(if version == 2 { &[0; 7] } else { &[0; 9] });
    reference(&mut bytes, 51);
    bytes.extend_from_slice(rule_reference_tail);
    reference(&mut bytes, 52);
    bytes.push(0);
    reference(&mut bytes, 53);
    let member_end = bytes.len();
    header(&mut bytes, *b"257");
    bytes.extend_from_slice(&[0; 8]);
    reference(&mut bytes, 54);
    let end = bytes.len();
    (
        bytes,
        PrimaryRecordFrame {
            entity_id: 50,
            start: 0,
            member_end,
            end,
        },
    )
}

#[test]
fn settings_frames_require_exact_boundary_version_flags_and_typed_links() {
    let primary = HashSet::from([50, 51, 52, 53, 54]);
    let types = HashMap::from([(51, (RULE_GUID, 0)), (52, (PRESENTATION_GUID, 0))]);
    for (version, tail) in [(2, &[0; 4][..]), (3, &[1, 0, 0][..]), (3, &[0, 0][..])] {
        let (bytes, frame) = fixture(version, tail);
        let registrations: Vec<_> =
            [(SETTINGS_GUID, version), (PAIRED_GUID, 0)]
                .into_iter()
                .enumerate()
                .map(
                    |(index, (guid, version))| crate::records::entity_header::SegmentTypeData {
                        byte_offset: u64_from_index(index),
                        type_guid: guid.to_owned().try_into().unwrap(),
                        type_guid_offset: 4,
                        base_type_guid: crate::records::entity_header::BaseTypeGuid::Absent,
                        version,
                        version_offset: 44,
                        module: "Fusion".into(),
                        entities: crate::records::identity::ReferenceRun::unlocated(
                            if index == 0 { vec![50] } else { Vec::new() },
                        ),
                    },
                )
                .collect();
        let fields =
            parse_settings(&bytes, frame, version, &primary, &types, &registrations).unwrap();
        assert_eq!(fields.mask, 4);
        assert_eq!(fields.inputs.version(), version);
        assert_eq!(
            fields.inputs.rule_reference_tail_width(),
            u64_from_index(tail.len())
        );
        if version == 3 {
            assert_eq!(
                fields.inputs,
                DesignSheetMetalSettingsInputs::V3 {
                    slots: [None; 9],
                    rule_reference_tail: if tail == [0, 0] {
                        V3RuleReferenceTail::ZeroPair
                    } else {
                        V3RuleReferenceTail::OneZeroPair
                    },
                }
            );
        }
        assert_eq!(
            (
                fields.rule,
                fields.presentation,
                fields.owner,
                fields.context
            ),
            (51, 52, 53, 54)
        );
        for offset in [
            4,
            frame.member_end + 4,
            11,
            25,
            25 + if version == 2 { 7 } else { 9 } + 11,
            frame.member_end + 11,
            frame.end - 1,
        ] {
            let mut invalid = bytes.clone();
            invalid[offset] = 2;
            assert!(
                parse_settings(&invalid, frame, version, &primary, &types, &registrations)
                    .is_none()
            );
        }
        assert!(parse_settings(
            &bytes,
            PrimaryRecordFrame {
                member_end: frame.member_end + 1,
                ..frame
            },
            version,
            &primary,
            &types,
            &registrations
        )
        .is_none());
        assert!(parse_settings(
            &bytes,
            frame,
            version,
            &HashSet::from([50, 51, 52, 53]),
            &types,
            &registrations
        )
        .is_none());
        let mut wrong_class = bytes.clone();
        wrong_class[6] = b'7';
        assert!(parse_settings(
            &wrong_class,
            frame,
            version,
            &primary,
            &types,
            &registrations
        )
        .is_none());
        let mut wrong_paired_class = bytes.clone();
        wrong_paired_class[frame.member_end + 6] = b'6';
        assert!(parse_settings(
            &wrong_paired_class,
            frame,
            version,
            &primary,
            &types,
            &registrations
        )
        .is_none());
        let wrong_types = HashMap::from([(51, (PRESENTATION_GUID, 0)), (52, (RULE_GUID, 0))]);
        assert!(parse_settings(
            &bytes,
            frame,
            version,
            &primary,
            &wrong_types,
            &registrations
        )
        .is_none());
        assert!(parse_settings(&bytes, frame, 4, &primary, &types, &registrations).is_none());
    }
}

#[test]
fn settings_decoder_skips_unregistered_bulk_without_requesting_a_metastream() {
    use std::io::{Cursor, Write};
    let stored = crate::zip_write::file_options(zip::CompressionMethod::Stored);
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    crate::test_support::manifest_test::write_synthetic_manifests(&mut zip, stored);
    zip.start_file("FusionAssetName[Active]/Design1/BulkStream.dat", stored)
        .unwrap();
    zip.write_all(&[]).unwrap();
    let bytes = zip.finish().unwrap().into_inner();
    crate::test_support::zip_test::with_scan(&bytes, |scan| {
        assert!(super::decode_settings(scan).unwrap().is_empty());
    });
}

#[test]
fn settings_name_every_linked_record_for_header_retention() {
    let settings: crate::records::sheet_metal_settings::DesignSheetMetalSettings =
        serde_json::from_value(serde_json::json!({
            "id": "f3d:Design1/BulkStream.dat:design-sheet-metal-settings#20", "record_index": 50,
            "byte_offset": 20, "paired_byte_offset": 100, "mask": 4,
            "inputs": {"version": "v2", "slots": [null, 55, null, null, null, null, null]},
            "rule_record_index": 51, "presentation_record_index": 52, "owner_record_index": 53,
            "context_record_index": 54,
        }))
        .unwrap();
    let indices = super::linked_record_indices(&[settings]);
    assert_eq!(
        indices,
        [50, 55, 51, 52, 53, 54].map(|index| ("f3d:Design1/BulkStream.dat".to_owned(), index))
    );
}

#[test]
fn rule_thickness_is_the_first_entry_named_thickness() {
    let rule = |name: &str| {
        let mut bytes = vec![0xaa; 7];
        let start = bytes.len();
        bytes.extend_from_slice(&3_u32.to_le_bytes());
        bytes.extend_from_slice(b"285");
        bytes.extend_from_slice(&273_u32.to_le_bytes());
        bytes.extend_from_slice(&[0; 10]);
        bytes.extend_from_slice(&3_u32.to_le_bytes());
        bytes.extend_from_slice(&u32::try_from(name.len()).unwrap().to_le_bytes());
        for unit in name.encode_utf16() {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        bytes.extend_from_slice(&0.25_f64.to_le_bytes());
        (bytes, start)
    };
    let (bytes, start) = rule("Thickness");
    let (thickness, offset) = super::rule_thickness(&bytes, start).expect("rule thickness");
    assert_eq!(thickness.get(), 0.25);
    assert_eq!(offset, u64::try_from(start + 47).unwrap());
    let (other, start) = rule("KFactor00");
    assert!(super::rule_thickness(&other, start).is_none());
}
