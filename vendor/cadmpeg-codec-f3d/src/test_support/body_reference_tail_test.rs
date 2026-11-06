// SPDX-License-Identifier: Apache-2.0
use super::{lp_ascii, lp_utf16};
use crate::metastream::PrimaryRecordFrame;
use cadmpeg_core::decode::u64_from_index;
use std::io::{Cursor, Write};

fn local(bytes: &mut Vec<u8>, target: u64, inline: bool) {
    bytes.push(1);
    bytes.extend_from_slice(&target.to_le_bytes());
    if inline {
        lp_ascii(bytes, "7ACC2A03-0261-4879-A14A-A93D661A5BDC");
    }
    bytes.extend_from_slice(&[0, 0]);
}

pub(crate) fn frame(inline: bool, trailer: u32, leading: usize) -> (Vec<u8>, PrimaryRecordFrame) {
    let mut bytes = vec![0; leading];
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"256");
    bytes.extend_from_slice(&17u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&0u32.to_le_bytes());
    local(&mut bytes, 19, inline);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    lp_utf16(&mut bytes, "11111111-2222-3333-4444-555555555555");
    lp_utf16(&mut bytes, "66666666-7777-8888-9999-AAAAAAAAAAAA");
    for word in [2u32, 0, 0, 1] {
        bytes.extend_from_slice(&word.to_le_bytes());
    }
    local(&mut bytes, u64::MAX, false);
    bytes.extend_from_slice(&trailer.to_le_bytes());
    let member_end = bytes.len();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"257");
    bytes.extend_from_slice(&17u32.to_le_bytes());
    let end = bytes.len();
    (
        bytes,
        PrimaryRecordFrame {
            entity_id: 17,
            start: leading,
            member_end,
            end,
        },
    )
}

pub(crate) fn metadata(frame: PrimaryRecordFrame, version: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    lp_ascii(&mut bytes, "Design");
    bytes.extend_from_slice(&0u32.to_le_bytes());
    lp_utf16(&mut bytes, "11111111-2222-3333-4444-555555555555");
    for value in [1234u32, 0, 0, 0] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    lp_ascii(&mut bytes, "Design");
    lp_ascii(&mut bytes, "");
    bytes.extend_from_slice(&0u64.to_le_bytes());
    bytes.extend_from_slice(&2u32.to_le_bytes());
    lp_ascii(&mut bytes, "9716F783-676D-42E0-93F9-EBF273E7C035");
    lp_ascii(&mut bytes, "6144EA59-3ACA-4172-9F14-A6531E94D69B");
    bytes.extend_from_slice(&version.to_le_bytes());
    lp_ascii(&mut bytes, "Fusion");
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&frame.entity_id.to_le_bytes());
    lp_ascii(&mut bytes, "6144EA59-3ACA-4172-9F14-A6531E94D69B");
    lp_ascii(&mut bytes, "");
    bytes.extend_from_slice(&0u32.to_le_bytes());
    lp_ascii(&mut bytes, "Fusion");
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    for offset in [frame.start, frame.member_end] {
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&frame.entity_id.to_le_bytes());
        bytes.extend_from_slice(&u64_from_index(offset).to_le_bytes());
    }
    bytes
}

pub(crate) fn archive(bytes: &[u8], meta: &[u8]) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = crate::zip_write::file_options(zip::CompressionMethod::Stored);
    crate::test_support::manifest_test::write_synthetic_manifests(&mut zip, options);
    zip.start_file("FusionAssetName[Active]/Design1/BulkStream.dat", options)
        .unwrap();
    zip.write_all(bytes).unwrap();
    zip.start_file("FusionAssetName[Active]/Design1/MetaStream.dat", options)
        .unwrap();
    zip.write_all(meta).unwrap();
    zip.start_file(
        "FusionAssetName[Active]/Breps.BlobParts/Body1.smbh",
        options,
    )
    .unwrap();
    zip.write_all(&super::smbh_geometry_test::synthetic_geometry_smbh())
        .unwrap();
    zip.finish().unwrap().into_inner()
}
