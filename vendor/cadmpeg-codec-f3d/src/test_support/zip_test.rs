// SPDX-License-Identifier: Apache-2.0
//! Synthetic `.f3d` ZIP archive builders.
#![allow(clippy::unwrap_used)]

use cadmpeg_ir::codec::write::target::TargetRequest;
use cadmpeg_ir::codec::write::EncodeInput;
use std::io::{Cursor, Write};

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::codec::write::Encoder;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use zip::CompressionMethod;

use crate::container;
use crate::test_support::lost_edge_test::remove_body_lost_edge_bulkstream;
use crate::test_support::manifest_test::{
    write_synthetic_manifests, write_synthetic_manifests_with_version,
};
use crate::test_support::protein_test::{
    generated_definition_catalog_for, generated_instance_properties_for,
    generated_schema_from_paged,
};
use crate::test_support::smbh_geometry_test::synthetic_geometry_smbh;
use crate::test_support::smbh_header_test::synthetic_smbh;
use crate::test_support::smbh_revision_test::scrubbed_definition;
use crate::test_support::streams_test::{
    design_metastream_with_records, generated_act_bulkstream, generated_act_metastream,
    generated_design_base_feature_bulkstream, generated_design_base_feature_metastream,
    generated_design_base_flange_bulkstream, generated_design_base_flange_metastream,
    generated_design_bulkstream, generated_design_copy_paste_bodies_bulkstream,
    generated_design_copy_paste_bodies_metastream, generated_design_copy_paste_bulkstream,
    generated_design_copy_paste_metastream, generated_design_form_bulkstream,
    generated_design_form_metastream, generated_design_metastream,
    generated_design_remove_body_bulkstream, generated_design_remove_body_metastream,
    generated_design_sketch_dimension_bulkstream, generated_design_sketch_dimension_metastream,
    generated_design_surface_stitch_bulkstream, generated_design_surface_stitch_metastream,
    generated_design_types, GeneratedDesignMetastreamVariant,
};
use crate::F3dCodec;

pub(crate) fn with_scan<T>(bytes: &[u8], f: impl FnOnce(&container::ContainerScan<'_>) -> T) -> T {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, root) = DecodeContext::from_root_bytes(bytes, &arena, &policy).unwrap();
    let scan = container::scan(&ctx, root).unwrap();
    f(&scan)
}

pub(crate) fn assert_revision_surface_round_trip(smbh: Vec<u8>, expected_kind: &str) {
    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&smbh)),
            &DecodeOptions::default(),
        )
        .expect("revision surface decode");
    let procedural = result
        .ir()
        .model
        .procedural_surfaces
        .first()
        .expect("revision surface construction");
    let expected = scrubbed_definition(procedural.definition());
    let kind = serde_json::to_value(procedural.definition()).expect("kind")["kind"]
        .as_str()
        .expect("kind string")
        .to_string();
    assert_eq!(kind, expected_kind);
    let (mut source_less, _, _) = result.into_parts();
    source_less.source = None;
    source_less
        .set_native_unknowns(&cadmpeg_test_support::service_decode_context(), "f3d", &[])
        .unwrap();
    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less revision surface encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less revision surface round trip");
    let actual = scrubbed_definition(
        round_trip
            .ir()
            .model
            .procedural_surfaces
            .first()
            .expect("round-trip construction")
            .definition(),
    );
    assert_eq!(actual, expected);
}

/// Wrap an ASM stream byte blob into a `.f3d` ZIP as `Body1.smbh`.
pub(crate) fn f3d_with_smbh(smbh: &[u8]) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    write_synthetic_manifests(&mut zip, stored);
    zip.start_file("FusionAssetName[Active]/Breps.BlobParts/Body1.smbh", stored)
        .unwrap();
    zip.write_all(smbh).unwrap();
    zip.finish().unwrap().into_inner()
}

/// The same archive as [`f3d_with_smbh`], with the top-level manifest
/// declaring `version`. The two archives differ in the version field alone.
pub(crate) fn f3d_with_smbh_and_manifest_version(smbh: &[u8], version: &str) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    write_synthetic_manifests_with_version(&mut zip, stored, version);
    zip.start_file("FusionAssetName[Active]/Breps.BlobParts/Body1.smbh", stored)
        .unwrap();
    zip.write_all(smbh).unwrap();
    zip.finish().unwrap().into_inner()
}

pub(crate) fn f3d_with_deflated_smbh(smbh: &[u8]) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    let deflated = crate::zip_write::file_options(CompressionMethod::Deflated);
    write_synthetic_manifests(&mut zip, stored);
    zip.start_file(
        "FusionAssetName[Active]/Breps.BlobParts/Body1.smbh",
        deflated,
    )
    .unwrap();
    zip.write_all(smbh).unwrap();
    zip.finish().unwrap().into_inner()
}

pub(crate) fn set_zip_entry_uncompressed_size(archive: &mut [u8], target: &[u8], size: u32) {
    let central = archive
        .windows(4)
        .enumerate()
        .find_map(|(offset, signature)| {
            if signature != b"PK\x01\x02" || offset + 46 > archive.len() {
                return None;
            }
            let name_length = usize::from(u16::from_le_bytes(
                archive[offset + 28..offset + 30]
                    .try_into()
                    .expect("central name-length field"),
            ));
            (archive.get(offset + 46..offset + 46 + name_length) == Some(target)).then_some(offset)
        })
        .expect("generated ZIP central-directory entry");
    archive[central + 24..central + 28].copy_from_slice(&size.to_le_bytes());
}

pub(crate) fn f3d_with_configuration(smbh: &[u8], name: &str, payload: &[u8]) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    write_synthetic_manifests(&mut zip, stored);
    zip.start_file("FusionAssetName[Active]/Breps.BlobParts/Body1.smbh", stored)
        .unwrap();
    zip.write_all(smbh).unwrap();
    zip.start_file(name, stored).unwrap();
    zip.write_all(payload).unwrap();
    zip.finish().unwrap().into_inner()
}

pub(crate) fn f3d_with_smbh_and_protein(smbh: &[u8]) -> Vec<u8> {
    f3d_with_smbh_and_protein_guids(smbh, &["11111111-2222-3333-4444-555555555555"])
}

pub(crate) fn f3d_with_smbh_and_protein_guids(smbh: &[u8], guids: &[&str]) -> Vec<u8> {
    let properties = guids
        .iter()
        .map(|guid| generated_instance_properties_for(guid))
        .collect::<Vec<_>>();
    let (design_bulk, design_records) = generated_design_bulkstream();
    f3d_with_smbh_and_instance_properties_and_design(
        smbh,
        &properties,
        &design_bulk,
        &design_records,
    )
}

pub(crate) fn f3d_with_smbh_and_instance_properties(
    smbh: &[u8],
    properties: &[Vec<u8>],
) -> Vec<u8> {
    let (design_bulk, design_records) = generated_design_bulkstream();
    f3d_with_smbh_and_instance_properties_and_design(
        smbh,
        properties,
        &design_bulk,
        &design_records,
    )
}

pub(crate) fn f3d_with_smbh_and_protein_with_generated_sketch_dimension(smbh: &[u8]) -> Vec<u8> {
    let properties = vec![generated_instance_properties_for(
        "11111111-2222-3333-4444-555555555555",
    )];
    let (design_bulk, design_records) = generated_design_sketch_dimension_bulkstream();
    let design_metastream = generated_design_sketch_dimension_metastream(&design_records);
    f3d_with_smbh_and_instance_properties_and_design_with_metastream(
        smbh,
        &properties,
        &design_bulk,
        &design_metastream,
    )
}

pub(crate) fn f3d_with_smbh_and_protein_with_generated_base_feature(smbh: &[u8]) -> Vec<u8> {
    let properties = vec![generated_instance_properties_for(
        "11111111-2222-3333-4444-555555555555",
    )];
    let (design_bulk, design_records) = generated_design_base_feature_bulkstream();
    let design_metastream = generated_design_base_feature_metastream(&design_records);
    f3d_with_smbh_and_instance_properties_and_design_with_metastream(
        smbh,
        &properties,
        &design_bulk,
        &design_metastream,
    )
}

pub(crate) fn f3d_with_smbh_and_protein_with_generated_base_flange(smbh: &[u8]) -> Vec<u8> {
    let properties = vec![generated_instance_properties_for(
        "11111111-2222-3333-4444-555555555555",
    )];
    let (design_bulk, design_records) = generated_design_base_flange_bulkstream();
    let design_metastream = generated_design_base_flange_metastream(&design_records);
    f3d_with_smbh_and_instance_properties_and_design_with_metastream(
        smbh,
        &properties,
        &design_bulk,
        &design_metastream,
    )
}

pub(crate) fn f3d_with_smbh_and_protein_with_generated_remove_body(smbh: &[u8]) -> Vec<u8> {
    let properties = vec![generated_instance_properties_for(
        "11111111-2222-3333-4444-555555555555",
    )];
    let (design_bulk, design_records) = generated_design_remove_body_bulkstream();
    let design_metastream = generated_design_remove_body_metastream(&design_records);
    f3d_with_smbh_and_instance_properties_and_design_with_metastream(
        smbh,
        &properties,
        &design_bulk,
        &design_metastream,
    )
}

/// The generated `RemoveBody` design whose two-operand construction group ends
/// an unresolved edge run of `run` records.
pub(crate) fn f3d_with_smbh_and_remove_body_lost_edge_run(smbh: &[u8], run: u32) -> Vec<u8> {
    let properties = vec![generated_instance_properties_for(
        "11111111-2222-3333-4444-555555555555",
    )];
    let (design_bulk, design_records) = remove_body_lost_edge_bulkstream(run);
    let design_metastream = generated_design_remove_body_metastream(&design_records);
    f3d_with_smbh_and_instance_properties_and_design_with_metastream(
        smbh,
        &properties,
        &design_bulk,
        &design_metastream,
    )
}

pub(crate) fn f3d_with_smbh_and_protein_with_generated_surface_stitch(smbh: &[u8]) -> Vec<u8> {
    let properties = vec![generated_instance_properties_for(
        "11111111-2222-3333-4444-555555555555",
    )];
    let (design_bulk, design_records) = generated_design_surface_stitch_bulkstream();
    let design_metastream = generated_design_surface_stitch_metastream(&design_records);
    f3d_with_smbh_and_instance_properties_and_design_with_metastream(
        smbh,
        &properties,
        &design_bulk,
        &design_metastream,
    )
}

pub(crate) fn f3d_with_smbh_and_protein_with_generated_copy_paste(smbh: &[u8]) -> Vec<u8> {
    let properties = vec![generated_instance_properties_for(
        "11111111-2222-3333-4444-555555555555",
    )];
    let (design_bulk, design_records) = generated_design_copy_paste_bulkstream();
    let design_metastream = generated_design_copy_paste_metastream(&design_records);
    f3d_with_smbh_and_instance_properties_and_design_with_metastream(
        smbh,
        &properties,
        &design_bulk,
        &design_metastream,
    )
}

pub(crate) fn f3d_with_smbh_and_protein_with_generated_copy_paste_bodies(smbh: &[u8]) -> Vec<u8> {
    let (design_bulk, design_records) = generated_design_copy_paste_bodies_bulkstream();
    let design_metastream = generated_design_copy_paste_bodies_metastream(&design_records);
    f3d_with_smbh_and_instance_properties_and_design_with_metastream(
        smbh,
        &[],
        &design_bulk,
        &design_metastream,
    )
}

pub(crate) fn f3d_with_smbh_and_protein_with_generated_form(smbh: &[u8]) -> Vec<u8> {
    let properties = vec![generated_instance_properties_for(
        "11111111-2222-3333-4444-555555555555",
    )];
    let (design_bulk, design_records) = generated_design_form_bulkstream();
    let design_metastream = generated_design_form_metastream(&design_records);
    let tsm = generated_form_tsm();
    f3d_with_smbh_and_instance_properties_and_design_with_metastream_and_entries(
        smbh,
        &properties,
        &design_bulk,
        &design_metastream,
        &[(
            "FusionAssetName[Active]/TSplines.BlobParts/TSpline.synthetic.tsm",
            &tsm,
        )],
    )
}

pub(crate) fn generated_form_tsm() -> Vec<u8> {
    br"#TS0200
degree 3
cap-type G1CAPS
star-smoothness 0
units 1 meters
end-conditions SUBD_CREASES
star-knot-rule NURCCS
f 0 0
e 0 1
e 2 1
e 4 1
e 6 1
v 0 NORTH
v 2 NORTH
v 4 NORTH
v 6 NORTH
l 2 6 1 0 0 0 0
l 7 3 0 3 -1 0 0
l 4 0 3 1 0 0 0
l 1 5 2 0 -1 0 0
l 6 2 5 2 0 0 0
l 3 7 4 1 -1 0 0
l 0 4 7 3 0 0 0
l 5 1 6 2 -1 0 0
ec 0 0
ec 1 0
ec 2 0
ec 3 0
0g 0 0 0 1
0g 1 0 0 1
0g 1 1 0 1
0g 0 1 0 1
"
    .to_vec()
}

fn f3d_with_smbh_and_instance_properties_and_design(
    smbh: &[u8],
    properties: &[Vec<u8>],
    design_bulk: &[u8],
    design_records: &[(u64, u64)],
) -> Vec<u8> {
    let design_metastream = generated_design_metastream(design_records);
    f3d_with_smbh_and_instance_properties_and_design_with_metastream(
        smbh,
        properties,
        design_bulk,
        &design_metastream,
    )
}

fn f3d_with_smbh_and_instance_properties_and_design_with_metastream(
    smbh: &[u8],
    properties: &[Vec<u8>],
    design_bulk: &[u8],
    design_metastream: &[u8],
) -> Vec<u8> {
    f3d_with_smbh_and_instance_properties_and_design_with_metastream_and_entries(
        smbh,
        properties,
        design_bulk,
        design_metastream,
        &[],
    )
}

fn f3d_with_smbh_and_instance_properties_and_design_with_metastream_and_entries(
    smbh: &[u8],
    properties: &[Vec<u8>],
    design_bulk: &[u8],
    design_metastream: &[u8],
    extra_entries: &[(&str, &[u8])],
) -> Vec<u8> {
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    let proteins = properties
        .iter()
        .map(|properties| {
            let mut nested = zip::ZipWriter::new(Cursor::new(Vec::new()));
            nested
                .start_file("AssetData/InstanceProperties.bin", stored)
                .unwrap();
            nested.write_all(properties).unwrap();
            nested
                .start_file("AssetData/DefinitionIteratorProperties.bin", stored)
                .unwrap();
            nested
                .write_all(&generated_definition_catalog_for(
                    generated_schema_from_paged(properties),
                ))
                .unwrap();
            nested.finish().unwrap().into_inner()
        })
        .collect::<Vec<_>>();

    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    write_synthetic_manifests(&mut zip, stored);
    zip.start_file(
        "FusionAssetName[Active]/Breps.BlobParts/BREP.synthetic.smbh",
        stored,
    )
    .unwrap();
    zip.write_all(smbh).unwrap();
    for (ordinal, protein) in proteins.iter().enumerate() {
        zip.start_file(
            format!(
                "FusionAssetName[Active]/ProteinAssets.BlobParts/ProteinAsset.{ordinal}.protein"
            ),
            stored,
        )
        .unwrap();
        zip.write_all(protein).unwrap();
    }
    zip.start_file("FusionAssetName[Active]/Design1/BulkStream.dat", stored)
        .unwrap();
    zip.write_all(design_bulk).unwrap();
    zip.start_file("FusionAssetName[Active]/Design1/MetaStream.dat", stored)
        .unwrap();
    zip.write_all(design_metastream).unwrap();
    let (act_bulk, act_records) = generated_act_bulkstream();
    zip.start_file(
        "FusionAssetName[Active]/FusionACTSegmentType1/BulkStream.dat",
        stored,
    )
    .unwrap();
    zip.write_all(&act_bulk).unwrap();
    zip.start_file(
        "FusionAssetName[Active]/FusionACTSegmentType1/MetaStream.dat",
        stored,
    )
    .unwrap();
    zip.write_all(&generated_act_metastream(&act_records))
        .unwrap();
    for (name, payload) in extra_entries {
        zip.start_file(*name, stored).unwrap();
        zip.write_all(payload).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

/// Assemble a synthetic `.f3d` ZIP with a manifest, a BREP `.smbh`, a `.smb`
/// snapshot, and a few side entries, mirroring the spec's naming families.
pub(crate) fn synthetic_f3d(include_smbh: bool) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    let deflated = crate::zip_write::file_options(CompressionMethod::Deflated);

    let folder = "FusionAssetName[Active]";
    write_synthetic_manifests(&mut zip, stored);

    if include_smbh {
        zip.start_file(format!("{folder}/Breps.BlobParts/Body1.smbh"), deflated)
            .unwrap();
        zip.write_all(&synthetic_smbh()).unwrap();
    }

    // A history-less .smb (header only, no history partition).
    let mut smb = synthetic_smbh();
    smb[39..47].copy_from_slice(&2u64.to_le_bytes());
    smb.truncate(60); // header prefix only, no history partition
    zip.start_file(format!("{folder}/Breps.BlobParts/Body1.smb"), stored)
        .unwrap();
    zip.write_all(&smb).unwrap();

    zip.start_file(
        format!("{folder}/FusionDesignSegmentType1/BulkStream.dat"),
        stored,
    )
    .unwrap();
    zip.write_all(b"design-bulk").unwrap();

    zip.start_file(format!("{folder}/Previews/thumbnail.png"), stored)
        .unwrap();
    zip.write_all(b"\x89PNG").unwrap();

    let cursor = zip.finish().unwrap();
    cursor.into_inner()
}

pub(crate) fn synthetic_multi_asset_f3d(include_design_brep: bool) -> Vec<u8> {
    const DESIGN_GUID: &str = "10000000-0000-4000-8000-000000000001";
    const SIBLING_GUID: &str = "20000000-0000-4000-8000-000000000002";
    const SECONDARY_GUID: &str = "30000000-0000-4000-8000-000000000003";

    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    zip.start_file("Manifest.dat", stored).unwrap();
    zip.write_all(
        &crate::manifest::encode_top_level(DESIGN_GUID, &["Simulation", "DesignAsset"]).unwrap(),
    )
    .unwrap();
    zip.start_file("Simulation/Manifest.dat", stored).unwrap();
    zip.write_all(
        &crate::manifest::encode_asset_header(
            "Simulation",
            SIBLING_GUID,
            SECONDARY_GUID,
            "SimulationAssetType",
        )
        .unwrap(),
    )
    .unwrap();
    zip.start_file("Simulation/FusionDesignSegmentType1/BulkStream.dat", stored)
        .unwrap();
    zip.write_all(b"sibling Design-name decoy").unwrap();
    zip.start_file("Simulation/Breps.BlobParts/BREP.sibling.smbh", stored)
        .unwrap();
    zip.write_all(&synthetic_smbh()).unwrap();
    zip.start_file("DesignAsset[Active]/Manifest.dat", stored)
        .unwrap();
    zip.write_all(&crate::manifest::encode_design_asset("DesignAsset", DESIGN_GUID).unwrap())
        .unwrap();
    zip.start_file(
        "DesignAsset[Active]/FusionDesignSegmentType1/BulkStream.dat",
        stored,
    )
    .unwrap();
    zip.write_all(b"selected Design stream").unwrap();
    zip.start_file(
        "DesignAsset[Active]/FusionNonDesignSegmentType1/BulkStream.dat",
        stored,
    )
    .unwrap();
    zip.write_all(b"selected-folder name decoy").unwrap();
    if include_design_brep {
        let mut design_brep = synthetic_geometry_smbh();
        design_brep[39..47].copy_from_slice(&2u64.to_le_bytes());
        zip.start_file(
            "DesignAsset[Active]/Breps.BlobParts/BREP.design.smb",
            stored,
        )
        .unwrap();
        zip.write_all(&design_brep).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum SketchAnnotationCase {
    Valid,
    Ambiguous,
    UnknownOperand,
    MetadataCount,
}

pub(crate) fn f3d_with_generated_sketch_annotation(case: SketchAnnotationCase) -> Vec<u8> {
    let (mut bulk, mut records) = generated_design_sketch_dimension_bulkstream();
    let offset = |index| {
        usize::try_from(
            records
                .iter()
                .find(|(record, _)| *record == index)
                .unwrap()
                .1,
        )
        .unwrap()
    };
    let owner = offset(1203);
    let parameter = offset(1204);
    let companion = offset(1205);
    if case != SketchAnnotationCase::Valid {
        let second_point_v = offset(1302) + 97;
        bulk[second_point_v..second_point_v + 8].copy_from_slice(&1.0f64.to_le_bytes());
    }
    if matches!(
        case,
        SketchAnnotationCase::UnknownOperand | SketchAnnotationCase::MetadataCount
    ) {
        let mut replacement = crate::design::test_support::parameter_record(
            Some(1203),
            "1e6mm",
            "Linear Dimension-2",
            Some("mm"),
            "d2",
            100_000.0,
        );
        replacement[4..7].copy_from_slice(b"271");
        replacement[7..11].copy_from_slice(&1204u32.to_le_bytes());
        bulk[parameter..companion].copy_from_slice(&replacement);
        bulk[owner + 40..owner + 48].copy_from_slice(&100_000.0f64.to_le_bytes());
    }
    let mut frame = Vec::new();
    crate::test_support::indexed_header(&mut frame, *b"274", 1304);
    frame.extend_from_slice(&[0; 8]);
    frame.push(1);
    frame.extend_from_slice(&3u32.to_le_bytes());
    let first = if case == SketchAnnotationCase::UnknownOperand {
        9999
    } else {
        1300
    };
    for (index, role) in [(0u32, 1u32), (first, 2), (1302, 2)] {
        crate::test_support::push_marked_reference(&mut frame, index);
        frame.extend_from_slice(&role.to_le_bytes());
    }
    frame.push(1);
    frame.extend_from_slice(
        &(if case == SketchAnnotationCase::MetadataCount {
            2u32
        } else {
            0
        })
        .to_le_bytes(),
    );
    frame.extend_from_slice(&[0xaa, 0xbb, 0xcc]);
    crate::test_support::push_marked_reference(&mut frame, 1203);
    frame.extend_from_slice(&2u32.to_le_bytes());
    for index in [1302, 1300] {
        crate::test_support::push_marked_reference(&mut frame, index);
    }
    frame.push(0);
    let paired = frame.len();
    crate::test_support::indexed_header(&mut frame, *b"273", 1304);
    frame.extend_from_slice(&[0; 8]);
    crate::test_support::push_marked_reference(&mut frame, 277);
    frame.resize(paired + 59, 0);
    insert_frame_in_preceding_companion(&mut bulk, &mut records, &frame);
    let meta = generated_design_sketch_dimension_metastream(&records);
    f3d_with_smbh_and_instance_properties_and_design_with_metastream(
        &synthetic_geometry_smbh(),
        &[],
        &bulk,
        &meta,
    )
}

fn insert_frame_in_preceding_companion(
    bulk: &mut Vec<u8>,
    records: &mut [(u64, u64)],
    inserted: &[u8],
) {
    let offset = |index| {
        usize::try_from(
            records
                .iter()
                .find(|(record, _)| *record == index)
                .unwrap()
                .1,
        )
        .unwrap()
    };
    let start = offset(1202) + 58;
    let owner = offset(1203);
    let parameter = offset(1204);
    let companion = offset(1205);
    let mut frame = inserted.to_vec();
    let new_owner = start + frame.len();
    let new_parameter = new_owner + parameter - owner;
    frame.extend_from_slice(&bulk[owner..companion]);
    let new_companion = start + frame.len();
    frame.extend_from_slice(&bulk[companion..]);
    bulk.truncate(start);
    bulk.extend_from_slice(&frame);
    for (record, offset) in records.iter_mut() {
        *offset = match *record {
            1204 => u64::try_from(new_parameter).unwrap(),
            1203 => u64::try_from(new_owner).unwrap(),
            _ if *offset >= u64::try_from(companion).unwrap() => {
                *offset - u64::try_from(companion).unwrap() + u64::try_from(new_companion).unwrap()
            }
            _ => *offset,
        };
    }
}

/// An archive whose preceding companion holds a two-point presentation frame.
/// The frame's class tags are the type-table ordinals of `primary` and `paired`,
/// and neither type lists an entity.
pub(crate) fn f3d_with_generated_sketch_presentation(
    primary: &'static str,
    paired: &'static str,
) -> Vec<u8> {
    let (mut bulk, mut records) = generated_design_sketch_dimension_bulkstream();
    let mut frame = Vec::new();
    crate::test_support::indexed_header(&mut frame, *b"273", 1304);
    frame.extend_from_slice(&[0; 8]);
    frame.push(1);
    frame.extend_from_slice(&2u32.to_le_bytes());
    for (index, role) in [(1300u32, 0u32), (1302, 1)] {
        crate::test_support::push_marked_reference(&mut frame, index);
        frame.extend_from_slice(&role.to_le_bytes());
    }
    frame.extend_from_slice(&[0xaa, 0xbb, 0xcc]);
    let pair = frame.len();
    crate::test_support::indexed_header(&mut frame, *b"274", 1304);
    frame.extend_from_slice(&[0; 8]);
    crate::test_support::push_marked_reference(&mut frame, 277);
    frame.resize(pair + 59, 0);
    insert_frame_in_preceding_companion(&mut bulk, &mut records, &frame);
    let mut types = generated_design_types(GeneratedDesignMetastreamVariant::SketchDimension);
    types.push((primary, "", 3, "Fusion", &[]));
    types.push((paired, "", 1, "Dimension", &[]));
    let meta = design_metastream_with_records(&types, &records);
    f3d_with_smbh_and_instance_properties_and_design_with_metastream(
        &synthetic_geometry_smbh(),
        &[],
        &bulk,
        &meta,
    )
}
