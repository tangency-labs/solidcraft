// SPDX-License-Identifier: Apache-2.0

use std::io::{Cursor, Read, Write};

use cadmpeg_core::decode::u64_from_index;
use zip::CompressionMethod;

use super::smbh_geometry_test::synthetic_geometry_smbh;
use super::streams_test::{generated_act_bulkstream, generated_act_metastream};
use super::zip_test::f3d_with_smbh_and_protein;
use super::{indexed_header, lp_utf16};

fn generated_act_with_component_links(links: &[(u32, usize)]) -> (Vec<u8>, Vec<(u64, u64)>) {
    assert!(!links.is_empty());
    let (original, records) = generated_act_bulkstream();
    let mut bulk = original[..usize::try_from(records[2].1).unwrap()].to_vec();
    let mut index = records[..2].to_vec();
    for (ordinal, &(tracked, padding)) in links.iter().enumerate() {
        let record = 9 + u32::try_from(ordinal).unwrap();
        index.push((u64::from(record), u64_from_index(bulk.len())));
        indexed_header(&mut bulk, *b"267", record);
        bulk.extend_from_slice(&[0; 10]);
        bulk.push(1);
        bulk.extend_from_slice(&12_u32.to_le_bytes());
        bulk.extend_from_slice(&[0; 6]);
        lp_utf16(&mut bulk, &format!("0_{tracked}"));
        bulk.push(1);
        bulk.extend_from_slice(&tracked.to_le_bytes());
        bulk.extend_from_slice(&[0; 5]);
        bulk.push(1);
        bulk.extend_from_slice(&1_u32.to_le_bytes());
        lp_utf16(&mut bulk, "Component");
        bulk.extend(std::iter::repeat_n(0, padding));
        bulk.push(1);
        bulk.extend_from_slice(&7_u32.to_le_bytes());
        bulk.extend_from_slice(&[0; 6]);
    }
    index.push((7, u64_from_index(bulk.len())));
    bulk.extend_from_slice(&original[usize::try_from(records[3].1).unwrap()..]);
    (bulk, index)
}

pub(crate) fn f3d_with_act_component_links(links: &[(u32, usize)]) -> Vec<u8> {
    let source = f3d_with_smbh_and_protein(&synthetic_geometry_smbh());
    let (bulk, records) = generated_act_with_component_links(links);
    let meta = generated_act_metastream(&records);
    let mut archive = zip::ZipArchive::new(Cursor::new(source)).unwrap();
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = crate::zip_write::file_options(CompressionMethod::Stored);
    for ordinal in 0..archive.len() {
        let mut entry = archive.by_index(ordinal).unwrap();
        let mut payload = Vec::new();
        entry.read_to_end(&mut payload).unwrap();
        if entry.name().unwrap() == "FusionAssetName[Active]/FusionACTSegmentType1/BulkStream.dat" {
            payload.clone_from(&bulk);
        } else if entry.name().unwrap()
            == "FusionAssetName[Active]/FusionACTSegmentType1/MetaStream.dat"
        {
            payload.clone_from(&meta);
        }
        out.start_file(entry.name().unwrap(), options).unwrap();
        out.write_all(&payload).unwrap();
    }
    out.finish().unwrap().into_inner()
}
