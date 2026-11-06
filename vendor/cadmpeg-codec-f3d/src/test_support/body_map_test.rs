// SPDX-License-Identifier: Apache-2.0
//! Archives whose Design body map classifies several model BREP blobs.
//!
//! Each body-map carrier names one `.smbh` blob and pairs a body selector with
//! a Design entity suffix. When the carriers classify every BREP entry exactly
//! once, every named blob contributes model bodies.
#![allow(clippy::unwrap_used)]

use std::io::{Cursor, Write};

use zip::CompressionMethod;

use super::manifest_test::write_synthetic_manifests;
use super::streams_test::design_metastream_with_records;
use crate::design::body::{
    BODY_MAP_CARRIER_BASE_TYPE_GUID, BODY_MAP_CARRIER_TYPE_GUID, BODY_MAP_CARRIER_TYPE_VERSION,
    GENERATED_BODY_MAP_ZERO_PREFIX_LEN,
};
use crate::records::entity_header::DESIGN_MODULE_BODY;
use crate::test_support::lp_utf16;

/// One model blob: its archive basename, its bytes, and the selector of the
/// body the Design body map binds.
pub(crate) struct ModelBlob<'a> {
    pub(crate) basename: &'a str,
    pub(crate) smbh: &'a [u8],
    pub(crate) selector: u64,
}

const FIRST_CARRIER: u32 = 899;
const FIRST_ENTITY_SUFFIX: u64 = 985;

/// A Design stream of body-map carriers, one per blob, and its `MetaStream`.
/// The carrier type is the `MetaStream`'s first type, so its class tag is 256.
fn body_map_design(blobs: &[ModelBlob<'_>]) -> (Vec<u8>, Vec<u8>) {
    let mut bulk = Vec::new();
    let mut records = Vec::new();
    let mut carriers = Vec::new();
    for ((carrier, suffix), blob) in (FIRST_CARRIER..).zip(FIRST_ENTITY_SUFFIX..).zip(blobs) {
        records.push((u64::from(carrier), u64::try_from(bulk.len()).unwrap()));
        carriers.push(u64::from(carrier));
        bulk.extend_from_slice(&3_u32.to_le_bytes());
        bulk.extend_from_slice(b"256");
        bulk.extend_from_slice(&carrier.to_le_bytes());
        bulk.extend_from_slice(&[0; GENERATED_BODY_MAP_ZERO_PREFIX_LEN]);
        bulk.extend_from_slice(&1_u32.to_le_bytes());
        bulk.extend_from_slice(&blob.selector.to_le_bytes());
        bulk.extend_from_slice(&suffix.to_le_bytes());
        bulk.extend_from_slice(&1793_u64.to_le_bytes());
        bulk.extend_from_slice(&0_u32.to_le_bytes());
        lp_utf16(&mut bulk, blob.basename);
    }
    let types: [(&str, &str, u32, &str, &[u64]); 1] = [(
        BODY_MAP_CARRIER_TYPE_GUID,
        BODY_MAP_CARRIER_BASE_TYPE_GUID,
        BODY_MAP_CARRIER_TYPE_VERSION,
        DESIGN_MODULE_BODY,
        &carriers,
    )];
    let meta = design_metastream_with_records(&types, &records);
    (bulk, meta)
}

/// An archive of `blobs`, each a model BREP that one Design body-map carrier
/// classifies.
pub(crate) fn f3d_with_model_breps(blobs: &[ModelBlob<'_>]) -> Vec<u8> {
    let (bulk, meta) = body_map_design(blobs);
    let breps: Vec<_> = blobs
        .iter()
        .map(|blob| (blob.basename, blob.smbh))
        .collect();
    f3d_with_breps_and_design(&breps, &bulk, &meta)
}

/// An archive of `(basename, smbh)` BREP blobs and one Design segment.
pub(crate) fn f3d_with_breps_and_design(
    breps: &[(&str, &[u8])],
    bulk: &[u8],
    meta: &[u8],
) -> Vec<u8> {
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    write_synthetic_manifests(&mut zip, stored);
    for (basename, smbh) in breps {
        zip.start_file(
            format!("FusionAssetName[Active]/Breps.BlobParts/{basename}"),
            stored,
        )
        .unwrap();
        zip.write_all(smbh).unwrap();
    }
    zip.start_file("FusionAssetName[Active]/Design1/BulkStream.dat", stored)
        .unwrap();
    zip.write_all(bulk).unwrap();
    zip.start_file("FusionAssetName[Active]/Design1/MetaStream.dat", stored)
        .unwrap();
    zip.write_all(meta).unwrap();
    zip.finish().unwrap().into_inner()
}

/// An empty modern model map and a named, zero-pair snapshot map.
pub(crate) fn f3d_with_empty_model_maps(snapshot: &[u8], targets: &[(&str, &str)]) -> Vec<u8> {
    let mut bulk = Vec::new();
    bulk.extend_from_slice(&3_u32.to_le_bytes());
    bulk.extend_from_slice(b"256");
    bulk.extend_from_slice(&10_u32.to_le_bytes());
    bulk.extend_from_slice(&[0; GENERATED_BODY_MAP_ZERO_PREFIX_LEN]);
    bulk.extend_from_slice(&0_u32.to_le_bytes());
    bulk.extend_from_slice(&12_u64.to_le_bytes());
    bulk.extend_from_slice(&0_u32.to_le_bytes());
    lp_utf16(&mut bulk, "");
    let snapshot_offset = u64::try_from(bulk.len()).unwrap();
    bulk.extend_from_slice(&3_u32.to_le_bytes());
    bulk.extend_from_slice(b"257");
    bulk.extend_from_slice(&20_u64.to_le_bytes());
    bulk.extend_from_slice(&[0; 6]);
    bulk.extend(super::local_reference(21));
    bulk.extend_from_slice(&0_u32.to_le_bytes());
    bulk.extend(super::local_reference(22));
    bulk.push(0);
    lp_utf16(&mut bulk, "BREP.snapshot.smb");
    let meta = design_metastream_with_records(
        &[
            (
                BODY_MAP_CARRIER_TYPE_GUID,
                BODY_MAP_CARRIER_BASE_TYPE_GUID,
                BODY_MAP_CARRIER_TYPE_VERSION,
                DESIGN_MODULE_BODY,
                &[10],
            ),
            (
                "40DCC3F41D-E94F-B3B1-3E95-9B6ABE572C",
                BODY_MAP_CARRIER_BASE_TYPE_GUID,
                0,
                DESIGN_MODULE_BODY,
                &[20],
            ),
            (
                "E55D0457-898F-4A8E-B1A5-C85BDBA8794F",
                "",
                0,
                DESIGN_MODULE_BODY,
                &[21],
            ),
            (
                crate::design::presentation::BREP_CONTAINER_TYPE_GUID,
                "",
                0,
                DESIGN_MODULE_BODY,
                &[22],
            ),
        ],
        &[(10, 0), (20, snapshot_offset)],
    );
    let bytes = f3d_with_breps_and_design(&[("BREP.snapshot.smb", snapshot)], &bulk, &meta);
    let mut zip = zip::ZipWriter::new_append(Cursor::new(bytes)).unwrap();
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    zip.start_file("RedirectionsStream.dat", stored).unwrap();
    zip.write_all(super::assembly_test::redirections_json("root.f3d", targets).as_bytes())
        .unwrap();
    zip.finish().unwrap().into_inner()
}
