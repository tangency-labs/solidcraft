// SPDX-License-Identifier: Apache-2.0

use std::io::{Cursor, Write};

use crate::records::decal::DesignRecordHeader;
use crate::records::entity_header::{
    DesignEntityHeader, DesignEntityRegistration, SketchHeaderReferences, DESIGN_MODULE_SKETCH,
};
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use crate::records::identity::ReferenceRun;
use crate::test_support::{indexed_header, lp_utf16};

pub(crate) fn fixture(
    count: u32,
) -> (
    Vec<u8>,
    DesignParameterScope,
    Vec<DesignRecordHeader>,
    Vec<DesignEntityHeader>,
) {
    fixture_with_flags(count, None)
}

pub(crate) fn fixture_with_flags(
    count: u32,
    flags: Option<([u8; 2], usize)>,
) -> (
    Vec<u8>,
    DesignParameterScope,
    Vec<DesignRecordHeader>,
    Vec<DesignEntityHeader>,
) {
    let stream = "f3d:FusionAssetName[Active]/Design1/BulkStream.dat";
    let mut bytes = Vec::new();
    let mut headers = Vec::new();
    let mut entities = Vec::new();
    for ordinal in 0..count {
        let record = (ordinal + 1) * 100;
        let entity = 172 + ordinal;
        let start = u64::try_from(bytes.len()).unwrap();
        indexed_header(&mut bytes, *b"308", record);
        bytes.extend_from_slice(&[0; 10]);
        bytes.push(1);
        bytes.extend_from_slice(&(record + 3).to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
        bytes.extend_from_slice(&1u32.to_le_bytes());
        lp_utf16(&mut bytes, "11111111-2222-3333-4444-555555555555");
        lp_utf16(&mut bytes, &entity.to_string());
        if let Some((flags, tail_length)) = flags {
            let mut tail = [0; 93];
            tail[0] = 1;
            tail[8] = 1;
            tail[41..45].copy_from_slice(&99u32.to_le_bytes());
            tail[53..57].copy_from_slice(&99u32.to_le_bytes());
            tail[57] = 1;
            tail[58..62].copy_from_slice(&(record + 2).to_le_bytes());
            tail[68..70].copy_from_slice(&flags);
            tail[70] = 1;
            tail[71..75].copy_from_slice(&(record + 1).to_le_bytes());
            tail[82] = 1;
            tail[83..87].copy_from_slice(&1u32.to_le_bytes());
            if tail_length == 89 {
                bytes.extend_from_slice(&tail[..53]);
                bytes.extend_from_slice(&tail[57..]);
            } else {
                bytes.extend_from_slice(&tail);
            }
        } else {
            bytes.extend_from_slice(&[0; 94]);
        }
        indexed_header(&mut bytes, *b"259", record);
        headers.push(DesignRecordHeader {
            id: format!("{stream}:design-record-header#{start}"),
            record_index: record,
            class_tag: "308".to_owned().try_into().unwrap(),
            byte_offset: start,
        });
        entities.push(DesignEntityHeader {
            id: format!("{stream}:design-entity-header#{entity}"),
            byte_offset: u64::from(entity),
            entity_id: format!("0_{entity}").try_into().unwrap(),
            class_tag: "269".to_owned().try_into().unwrap(),
            optional_slot_present: false,
            registration: DesignEntityRegistration::new(
                Some(DESIGN_MODULE_SKETCH.to_owned()),
                Some(SketchHeaderReferences {
                    record_reference: Some(record),
                    record_reference_offset: start + 1,
                    references: Vec::new(),
                }),
                ReferenceRun::unlocated(Vec::new()),
            )
            .unwrap(),
        });
    }
    let mut scope = DesignParameterScope::empty(
        &format!("{stream}:design-parameter-scope#1"),
        DesignFeatureKind::Extrude,
        1,
    );
    scope
        .try_edit(|draft| {
            draft.reference_members =
                ReferenceRun::unlocated(headers.iter().map(|header| header.record_index).collect());
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = crate::zip_write::file_options(zip::CompressionMethod::Stored);
    crate::test_support::manifest_test::write_synthetic_manifests(&mut zip, options);
    zip.start_file("FusionAssetName[Active]/Design1/BulkStream.dat", options)
        .unwrap();
    zip.write_all(&bytes).unwrap();
    (zip.finish().unwrap().into_inner(), scope, headers, entities)
}
