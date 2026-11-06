// SPDX-License-Identifier: Apache-2.0

use std::io::{Cursor, Write};

use super::super::decode_parameters;
use crate::design::test_support::parameter_record;
use crate::test_support::manifest_test::write_synthetic_manifests;
use crate::test_support::streams_test::design_metastream_with_records;
use crate::test_support::zip_test::with_scan;

const TAIL_BASE_TYPE_GUID: &str = "A7AEA631-985B-4DD1-8CE2-DE2C-14B54081";

fn registered_tail_archive(frame: Vec<u8>, type_guid: &str, target: u64) -> Vec<u8> {
    let metadata = design_metastream_with_records(
        &[(type_guid, TAIL_BASE_TYPE_GUID, 3, "CommonData", &[target])],
        &[],
    );
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = crate::zip_write::file_options(zip::CompressionMethod::Stored);
    write_synthetic_manifests(&mut zip, stored);
    for (name, payload) in [("BulkStream.dat", frame), ("MetaStream.dat", metadata)] {
        zip.start_file(format!("FusionAssetName[Active]/Design1/{name}"), stored)
            .unwrap();
        zip.write_all(&payload).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

fn with_registered_tail(mut frame: Vec<u8>, target: u64) -> Vec<u8> {
    let tail = frame.len() - 12;
    frame[tail + 2..tail + 10].copy_from_slice(&target.to_le_bytes());
    frame
}

fn utf16(out: &mut Vec<u8>, text: &str) {
    let units = text.encode_utf16().collect::<Vec<_>>();
    out.extend_from_slice(&u32::try_from(units.len()).unwrap().to_le_bytes());
    for unit in units {
        out.extend_from_slice(&unit.to_le_bytes());
    }
}

#[test]
fn parameter_registered_tail_resolves_full_record_identity_and_refuses_wrong_type() {
    for target in [181u64, 513] {
        for (type_guid, discriminator, admitted, unparsed) in [
            (super::super::PARAMETER_TAIL_TYPE_GUID, 5u64, true, 0),
            (super::super::PARAMETER_TAIL_TYPE_GUID, 9u64, false, 1),
            ("00000000-0000-0000-0000-000000000000", 5u64, false, 0),
        ] {
            let mut frame =
                parameter_record(None, "3 mm", "User Parameter", Some("mm"), "Width", 0.3);
            frame[22..30].copy_from_slice(&discriminator.to_le_bytes());
            let tail = frame.len() - 12;
            frame[tail + 2..tail + 10].copy_from_slice(&target.to_le_bytes());
            let archive = registered_tail_archive(frame, type_guid, target);
            let decoded = with_scan(&archive, decode_parameters).unwrap();
            assert_eq!(decoded.parameters.len(), usize::from(admitted));
            assert_eq!(
                decoded.unparsed.iter().map(|row| row.count).sum::<usize>(),
                unparsed
            );
            if admitted {
                assert_eq!(decoded.parameters[0].name(), "Width");
                assert_eq!(decoded.parameters[0].expression(), "3 mm");
                assert!(decoded.unparsed.is_empty());
            }
        }
    }
}

/// A tail target's low byte carries no family meaning: discriminator `6`
/// reads with every source kind at every target.
#[test]
fn parameter_registered_tail_target_byte_is_not_a_family_marker() {
    for target in [0x110u64, 0x111] {
        let mut frame = parameter_record(None, "3 mm", "User Parameter", Some("mm"), "Width", 0.3);
        frame[22..30].copy_from_slice(&6u64.to_le_bytes());
        let archive = registered_tail_archive(
            with_registered_tail(frame, target),
            super::super::PARAMETER_TAIL_TYPE_GUID,
            target,
        );
        let decoded = with_scan(&archive, decode_parameters).unwrap();
        assert_eq!(decoded.parameters.len(), 1, "target {target:#x}");
        assert!(decoded.unparsed.is_empty());
    }
}

/// Every header form ends with the parameter tail, so a compact owned frame
/// names a registered collection like a discriminated frame.
#[test]
fn parameter_registered_tail_admits_compact_owned_frame() {
    let target = 181u64;
    let mut frame = Vec::new();
    frame.extend_from_slice(&3u32.to_le_bytes());
    frame.extend_from_slice(b"305");
    frame.extend_from_slice(&71u32.to_le_bytes());
    frame.extend_from_slice(&[0; 15]);
    frame.extend_from_slice(&9u32.to_le_bytes());
    frame.push(1);
    frame.extend_from_slice(&40u32.to_le_bytes());
    frame.extend_from_slice(&[0; 6]);
    utf16(&mut frame, "3 mm");
    frame.extend_from_slice(&[0; 5]);
    utf16(&mut frame, "Length");
    utf16(&mut frame, "mm");
    utf16(&mut frame, "Width");
    frame.extend_from_slice(&0.3f64.to_le_bytes());
    frame.extend_from_slice(&[0, 1]);
    frame.extend_from_slice(&target.to_le_bytes());
    frame.extend_from_slice(&[0, 0]);
    let archive = registered_tail_archive(frame, super::super::PARAMETER_TAIL_TYPE_GUID, target);
    let decoded = with_scan(&archive, decode_parameters).unwrap();
    assert_eq!(decoded.parameters.len(), 1);
    assert_eq!(decoded.parameters[0].name(), "Width");
    assert!(decoded.unparsed.is_empty());
}

/// When the stream registers a parameter collection, a parameter tail names it.
#[test]
fn parameter_tail_names_a_registered_collection() {
    let mut frame = parameter_record(Some(40), "3 mm", "Radius", Some("mm"), "Width", 0.3);
    frame[22..30].copy_from_slice(&6u64.to_le_bytes());
    let tail = frame.len() - 12;
    frame[tail + 2] = 0x11;
    let archive =
        registered_tail_archive(frame.clone(), super::super::PARAMETER_TAIL_TYPE_GUID, 0x11);
    let decoded = with_scan(&archive, decode_parameters).unwrap();
    assert_eq!(decoded.parameters.len(), 1);
    assert_eq!(decoded.parameters[0].name(), "Width");
    assert!(decoded.unparsed.is_empty());

    // A nonzero byte after the low byte names another record.
    let mut corrupt = frame.clone();
    corrupt[tail + 5] = 1;
    let archive = registered_tail_archive(corrupt, super::super::PARAMETER_TAIL_TYPE_GUID, 0x11);
    let decoded = with_scan(&archive, decode_parameters).unwrap();
    assert!(decoded.parameters.is_empty());

    // A frame that names a record outside the registered collections is
    // reported.
    let archive = registered_tail_archive(frame, super::super::PARAMETER_TAIL_TYPE_GUID, 0x12);
    let decoded = with_scan(&archive, decode_parameters).unwrap();
    assert!(decoded.parameters.is_empty());
    assert_eq!(
        decoded.unparsed.iter().map(|row| row.count).sum::<usize>(),
        1
    );
}

/// A feature input or dimension with discriminator `6` and a registered plain
/// tail is a parameter; a stray flag byte after the target and a truncated tail
/// are not.
#[test]
fn parameter_registered_tail_admits_discriminator_six_feature_inputs() {
    let radius = |owner: Option<u32>, kind: &str, unit: Option<&str>| {
        let mut frame = parameter_record(owner, "0.6 mm", kind, unit, "d302", 0.06);
        frame[22..30].copy_from_slice(&6u64.to_le_bytes());
        frame
    };
    for target in [181u64, 513, 0x110] {
        for (kind, unit) in [("Radius", Some("mm")), ("Linear Dimension-9", Some("mm"))] {
            let frame = with_registered_tail(radius(Some(40), kind, unit), target);
            let archive =
                registered_tail_archive(frame, super::super::PARAMETER_TAIL_TYPE_GUID, target);
            let decoded = with_scan(&archive, decode_parameters).unwrap();
            assert_eq!(decoded.parameters.len(), 1, "{kind} {target:#x}");
            assert_eq!(decoded.parameters[0].source_kind(), kind);
            assert_eq!(decoded.parameters[0].name(), "d302");
            assert!(decoded.unparsed.is_empty());
        }
    }
    let target = 181u64;
    let good = with_registered_tail(radius(Some(40), "Radius", Some("mm")), target);
    let refused = |frame: Vec<u8>| {
        let archive =
            registered_tail_archive(frame, super::super::PARAMETER_TAIL_TYPE_GUID, target);
        let decoded = with_scan(&archive, decode_parameters).unwrap();
        assert!(decoded.parameters.is_empty());
    };
    let tail = good.len() - 12;
    let mut flagged = good.clone();
    flagged[tail + 11] = 1;
    refused(flagged);
    let mut truncated = good.clone();
    truncated.truncate(good.len() - 1);
    refused(truncated);
    let mut other_discriminator = good.clone();
    other_discriminator[22..30].copy_from_slice(&7u64.to_le_bytes());
    refused(other_discriminator);
}
