// SPDX-License-Identifier: Apache-2.0
//! Materials-module unit tests and appearance suites.
#![allow(clippy::unwrap_used)]
#![allow(
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::if_not_else,
    clippy::needless_pass_by_value,
    clippy::range_plus_one,
    clippy::semicolon_if_nothing_returned,
    clippy::trivially_copy_pass_by_ref
)]

use cadmpeg_ir::codec::write::target::TargetRequest;
use cadmpeg_ir::codec::write::EncodeInput;
use std::io::{Cursor, Write};

use cadmpeg_ir::codec::write::Encoder;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use zip::CompressionMethod;

use crate::loss::F3dLossCode;
use crate::test_support::manifest_test::write_synthetic_manifests;
use crate::test_support::native_test::{f3d_native, f3d_native_mut, update_f3d_native, TestEncode};
use crate::test_support::protein_test::generated_instance_properties_for;
use crate::test_support::smbh_geometry_test::{
    synthetic_geometry_smbh, synthetic_geometry_with_attribute_smbh,
    synthetic_geometry_with_sketch_link_smbh, synthetic_mixed_smbh, SketchLinkForm,
};
use crate::test_support::streams_test::design_metastream_with_records;
use crate::test_support::zip_test::{
    f3d_with_smbh, f3d_with_smbh_and_instance_properties, f3d_with_smbh_and_protein,
    f3d_with_smbh_and_protein_guids, with_scan,
};
use crate::test_support::{lp_ascii, lp_utf16};
use crate::F3dCodec;

use super::{
    decode_definition_catalog_record, merge_definition_catalog_record, DefinitionCatalog,
    RECORD_MARKER, STREAM_HEADER_LEN,
};

#[test]
fn legacy_face_selector_refuses_short_carriers_without_indexing() {
    assert_eq!(super::legacy_face_selector_kind(Some(&[])), None);
    assert_eq!(super::legacy_face_selector_kind(Some(&[1, 1])), None);
    let mut carrier = [0; 12];
    carrier[..2].copy_from_slice(&[1, 1]);
    carrier[11] = 1;
    assert_eq!(super::legacy_face_selector_kind(Some(&carrier)), Some(1));
}

fn raw_body_map_pair(
    asm_key_offset: usize,
    entity_suffix: u64,
) -> crate::design::decode::body::BodyBinding {
    crate::design::decode::body::BodyBinding {
        asm_key: 7,
        asm_key_offset,
        entity_suffix,
    }
}

fn resolved_body_binding(
    stream: &str,
    asm_key_offset: u64,
    entity_suffix: u64,
    blob_name: &str,
    body: &str,
) -> crate::records::bodies::DesignBodyBinding {
    crate::records::bodies::DesignBodyBinding::try_from(
        crate::records::bodies::DesignBodyBindingWire {
            component_owner: None,
            component_owner_unresolved: false,
            history_only: false,
            id: crate::ids::native_design_body_binding_id(stream, asm_key_offset),
            stream: stream.into(),
            pair_count: 1,
            pair_ordinal: 0,
            asm_body_key: 7,
            asm_body_key_offset: asm_key_offset,
            entity_suffix,
            entity_suffix_offset: asm_key_offset + 8,
            blob_name: blob_name.into(),
            blob_name_offset: asm_key_offset + 32,
            body: Some(cadmpeg_ir::ids::BodyId::mint(body).expect("identity grammar")),
        },
    )
    .unwrap()
}

/// `InstanceProperties.bin` with three appearance records: two of `KnownSchema`
/// and, between them, one of a schema the archive does not define.
fn three_record_instance() -> Vec<u8> {
    let mut instance = Vec::new();
    for (schema, guid) in [
        ("KnownSchema", "first-guid"),
        ("MissingSchema", "rejected-guid"),
        ("KnownSchema", "third-guid"),
    ] {
        let mut logical = RECORD_MARKER.to_vec();
        for value in [schema, guid, "base", ""] {
            super::push_lp(&mut logical, value).unwrap();
        }
        if instance.is_empty() {
            instance.extend_from_slice(
                &(u32::try_from(super::PAGE_SIZE).expect("fixture value fits u32")).to_le_bytes(),
            );
            instance.extend_from_slice(&[0; STREAM_HEADER_LEN - 4]);
        }
        let body = &logical[RECORD_MARKER.len()..];
        let mut page = super::TERMINAL_MARKER.to_vec();
        page.extend_from_slice(&u16::try_from(body.len()).unwrap().to_le_bytes());
        page.extend_from_slice(&[0; 2]);
        page.extend_from_slice(body);
        page.resize(super::PAGE_SIZE, 0);
        instance.extend_from_slice(&page);
    }
    instance
}

#[test]
fn protein_rejections_preserve_valid_records_and_report_notes() {
    let instance = three_record_instance();
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    archive
        .start_file("Schemas/KnownSchema.xml", stored)
        .unwrap();
    archive
        .write_all(br#"<Schema><UID val="KnownSchema"/></Schema>"#)
        .unwrap();
    archive
        .start_file("AssetData/InstanceProperties.bin", stored)
        .unwrap();
    archive.write_all(&instance).unwrap();
    let protein = archive.finish().unwrap().into_inner();
    let edits = ["first-guid", "third-guid"]
        .into_iter()
        .map(|guid| (guid.to_owned(), super::ProteinAppearanceEdit::default()))
        .collect();
    let mut notes = Vec::new();
    let (patched, guids) = super::patch_protein_appearances(&protein, &edits, &mut notes).unwrap();
    assert_eq!(
        guids.into_iter().collect::<Vec<_>>(),
        ["first-guid", "third-guid"]
    );
    assert_eq!(notes.len(), 1);
    assert!(notes[0].contains("record 1 rejected") && notes[0].contains("MissingSchema"));
    let mut patched_archive = zip::ZipArchive::new(Cursor::new(patched)).unwrap();
    let mut retained = Vec::new();
    std::io::Read::read_to_end(
        &mut patched_archive
            .by_name("AssetData/InstanceProperties.bin")
            .unwrap(),
        &mut retained,
    )
    .unwrap();
    assert_eq!(retained, instance);

    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    write_synthetic_manifests(&mut archive, stored);
    archive
        .start_file(
            "FusionAssetName[Active]/ProteinAssets.BlobParts/ProteinAsset.0.protein",
            stored,
        )
        .unwrap();
    archive.write_all(&protein).unwrap();
    let bytes = archive.finish().unwrap().into_inner();
    let decoded = F3dCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert_eq!(
        decoded
            .ir()
            .model
            .appearances
            .iter()
            .map(|appearance| appearance.asset_guid.as_deref())
            .collect::<Vec<_>>(),
        [Some("first-guid"), Some("third-guid")]
    );
    assert!(decoded.report().notes.iter().any(|note| note
        .contains("ProteinAsset.0.protein record 1 rejected")
        && note.contains("MissingSchema")));
}

/// A Protein ZIP whose members are Zstandard (method 93) is decoded and patched.
///
/// `zip` cannot expand or write method 93. The schema member is expanded by
/// `cadmpeg-protein` on decode, and on patch both members, which the default edit
/// leaves unchanged, are copied as stored.
#[test]
fn a_zstandard_protein_archive_decodes_and_patches() {
    use cadmpeg_test_support::zip_fixture::{self, RawEntry};
    let schema = br#"<Schema><UID val="KnownSchema"/></Schema>"#;
    let instance = three_record_instance();
    let zstd = |name: &str, plain: &[u8]| {
        RawEntry::compressed(
            name,
            zip_fixture::ZSTD,
            &zip_fixture::zstd_stored_frame(plain),
            plain,
        )
    };
    let protein = zip_fixture::archive(&[
        zstd("Schemas/KnownSchema.xml", schema),
        zstd("AssetData/InstanceProperties.bin", &instance),
    ]);

    let edits = ["first-guid", "third-guid"]
        .into_iter()
        .map(|guid| (guid.to_owned(), super::ProteinAppearanceEdit::default()))
        .collect();
    let mut notes = Vec::new();
    let (patched, guids) = super::patch_protein_appearances(&protein, &edits, &mut notes)
        .expect("patch a Zstandard Protein archive");
    assert_eq!(
        guids.into_iter().collect::<Vec<_>>(),
        ["first-guid", "third-guid"]
    );
    assert!(
        notes.iter().all(|note| !note.contains("Deflate")),
        "an unchanged member was re-encoded: {notes:?}"
    );
    let stored = |archive: &[u8]| {
        let mut archive = zip::ZipArchive::new(Cursor::new(archive)).unwrap();
        (0..archive.len())
            .map(|index| {
                let mut entry = archive.by_index_raw(index).unwrap();
                let mut bytes = Vec::new();
                std::io::Read::read_to_end(&mut entry, &mut bytes).unwrap();
                (
                    entry.name().expect("entry name").into_owned(),
                    entry.compression(),
                    bytes,
                )
            })
            .collect::<Vec<_>>()
    };
    let copied = stored(&patched);
    assert!(copied
        .iter()
        .all(|(_, method, _)| *method == CompressionMethod::ZSTD));
    assert_eq!(
        copied,
        stored(&protein),
        "both members are copied as stored"
    );

    let stored_options = crate::zip_write::file_options(CompressionMethod::Stored);
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    write_synthetic_manifests(&mut archive, stored_options);
    archive
        .start_file(
            "FusionAssetName[Active]/ProteinAssets.BlobParts/ProteinAsset.0.protein",
            stored_options,
        )
        .unwrap();
    archive.write_all(&protein).unwrap();
    let bytes = archive.finish().unwrap().into_inner();
    let decoded = F3dCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode an F3D carrying a Zstandard Protein archive");
    assert_eq!(
        decoded
            .ir()
            .model
            .appearances
            .iter()
            .map(|appearance| appearance.asset_guid.as_deref())
            .collect::<Vec<_>>(),
        [Some("first-guid"), Some("third-guid")]
    );
}

#[test]
fn definition_catalog_uses_page_boundaries_when_payload_contains_a_start_marker() {
    let category: String = std::iter::repeat_n('x', 0x1_0080).collect();
    let mut logical = RECORD_MARKER.to_vec();
    lp_ascii(&mut logical, "GenericSchema");
    logical.push(0);
    lp_ascii(&mut logical, "Prism-001");
    lp_ascii(&mut logical, "Prism-001");
    logical.extend_from_slice(&2_u32.to_le_bytes());
    lp_ascii(&mut logical, &category);
    lp_ascii(&mut logical, "Default");
    lp_ascii(&mut logical, "Generated appearance");
    logical.extend_from_slice(&0_u32.to_le_bytes());
    logical.extend_from_slice(&1_u32.to_le_bytes());
    lp_ascii(&mut logical, "");

    let paged = super::page_logical(&logical).expect("page catalog record");
    let frames = cadmpeg_protein::framing::record_frames(&paged).expect("frame catalog pages");
    let [frame] = frames.as_slice() else {
        panic!("marker-shaped length prefix must remain inside one logical record")
    };
    let decoded =
        decode_definition_catalog_record(frame.bytes()).expect("decode framed definition record");
    assert_eq!(decoded.schema, "GenericSchema");
    assert_eq!(decoded.asset_id, "Prism-001");
    assert_eq!(decoded.category.as_deref(), Some(category.as_str()));
}

#[test]
fn definition_catalog_version_one_omits_category() {
    let mut logical = RECORD_MARKER.to_vec();
    lp_ascii(&mut logical, "PrismOpaqueSchema");
    logical.push(1);
    lp_ascii(&mut logical, "Opaque(246,246,243)");
    lp_ascii(&mut logical, "EFD2D83C-576F-3A9B-8535-31523D8D8432");
    logical.extend_from_slice(&1_u32.to_le_bytes());
    lp_ascii(&mut logical, "Default");
    lp_ascii(&mut logical, "Prism opaque material.");
    logical.extend_from_slice(&2_u32.to_le_bytes());
    lp_ascii(&mut logical, "materials");
    lp_ascii(&mut logical, "opaque");
    logical.extend_from_slice(&0_u32.to_le_bytes());

    let decoded =
        decode_definition_catalog_record(&logical).expect("decode version-one definition record");
    assert_eq!(decoded.schema, "PrismOpaqueSchema");
    assert_eq!(decoded.asset_id, "Opaque(246,246,243)");
    assert_eq!(decoded.category, None);
}

#[test]
fn definition_catalog_version_zero_omits_category_and_group() {
    let mut logical = RECORD_MARKER.to_vec();
    lp_ascii(&mut logical, "UnifiedBitmapSchema");
    logical.push(0);
    lp_ascii(&mut logical, "Metal-045_metal_pattern_shader");
    lp_ascii(&mut logical, "Metal-045_metal_pattern_shader");
    logical.extend_from_slice(&0_u32.to_le_bytes());
    lp_ascii(&mut logical, "Unified Bitmap.");
    logical.extend_from_slice(&2_u32.to_le_bytes());
    lp_ascii(&mut logical, "maps");
    lp_ascii(&mut logical, "misc");
    logical.extend_from_slice(&1_u32.to_le_bytes());
    lp_ascii(&mut logical, "Maps/UnifiedBitmap/UnifiedBitmap.png");

    let decoded =
        decode_definition_catalog_record(&logical).expect("decode version-zero definition record");
    assert_eq!(decoded.category, None);
    assert_eq!(decoded.schema, "UnifiedBitmapSchema");
    assert_eq!(decoded.asset_id, "Metal-045_metal_pattern_shader");
}

#[test]
fn definition_catalog_version_three_adds_subgroup() {
    let mut logical = RECORD_MARKER.to_vec();
    lp_ascii(&mut logical, "GenericSchema");
    logical.push(0);
    lp_ascii(&mut logical, "InvGen-063");
    lp_ascii(&mut logical, "InvGen-063");
    logical.extend_from_slice(&3_u32.to_le_bytes());
    for value in ["Metal", "Default", "Miscellaneous", "Generic material."] {
        lp_ascii(&mut logical, value);
    }
    logical.extend_from_slice(&0_u32.to_le_bytes());
    logical.extend_from_slice(&0_u32.to_le_bytes());

    let decoded =
        decode_definition_catalog_record(&logical).expect("decode version-three definition record");
    assert_eq!(decoded.category.as_deref(), Some("Metal"));
    assert_eq!(decoded.schema, "GenericSchema");
    assert_eq!(decoded.asset_id, "InvGen-063");
}

#[test]
fn definition_catalog_uses_asset_and_schema_identity() {
    fn definition(asset: &str, category: &str) -> DefinitionCatalog {
        DefinitionCatalog {
            schema: "PrismMetalSchema".into(),
            asset_id: asset.into(),
            category: Some(category.into()),
        }
    }

    let mut definitions = std::collections::HashMap::new();
    merge_definition_catalog_record(&mut definitions, definition("Prism-256", "Metal/Steel"));
    merge_definition_catalog_record(&mut definitions, definition("Prism-256", "Metal/Steel"));
    assert_eq!(definitions.len(), 1);

    merge_definition_catalog_record(&mut definitions, definition("Prism-256", "Metal/Stainless"));
    let key = ("Prism-256".to_owned(), "PrismMetalSchema".to_owned());
    assert_eq!(definitions[&key].category, None);

    let mut second_schema = definition("Prism-256", "Metal/Steel");
    second_schema.schema = "GenericSchema".into();
    merge_definition_catalog_record(&mut definitions, second_schema);
    assert_eq!(definitions.len(), 2);
}

#[test]
fn material_owner_rejects_more_than_one_pair_for_its_entity_suffix() {
    let body_map = [raw_body_map_pair(25, 100), raw_body_map_pair(41, 100)];
    let Err(error) = super::unique_body_map_pair(&body_map, 100, "material assignment") else {
        panic!("one Design entity must not select two map pairs")
    };
    assert!(error
        .to_string()
        .contains("matches multiple body-map pairs"));
}

#[test]
fn equal_keys_in_different_brep_namespaces_resolve_by_exact_map_pair() {
    let stream = "FusionAssetName[Active]/Design1/BulkStream.dat";
    let first = resolved_body_binding(
        stream,
        25,
        100,
        "BREP.first.smbh",
        "f3d:brep/first/brep:entity#1",
    );
    let second_body =
        cadmpeg_ir::ids::BodyId::mint("f3d:brep/second/brep:entity#1").expect("identity grammar");
    let second = resolved_body_binding(stream, 125, 200, "BREP.second.smbh", second_body.as_str());
    let owner = crate::ids::native_scoped_id(stream, "material-assignment", 500);
    let visual_guid = "11111111-2222-3333-4444-555555555555";
    let appearance = cadmpeg_ir::appearance::Appearance {
        id: cadmpeg_ir::ids::AppearanceId::mint("f3d:test:appearance#second")
            .expect("identity grammar"),
        name: None,
        asset_guid: Some(visual_guid.into()),
        library_id: None,
        visual_guid: Some(visual_guid.into()),
        physical_token: None,
        schema: None,
        category: None,
        base_color: None,
        properties: std::collections::BTreeMap::new(),
        textures: Vec::new(),
    };
    let assignment = crate::records::references::DesignMaterialAssignment {
        id: owner,
        asm_body_key: 7,
        asm_body_key_offset: 125,

        entity_suffix_offset: 133,
        entity_id: crate::records::identity::DesignEntityId::try_from("0_200".to_owned())
            .expect("valid entity ID"),
        entity_id_offset: 500,
        visual_guid: crate::records::references::DesignVisualToken::try_from(
            visual_guid.to_owned(),
        )
        .unwrap(),
        visual_guid_offset: 600,
        physical_token: None,
        visual_preset: None,
    };
    let projected = super::bind_bodies(
        &[appearance],
        &[assignment],
        &std::collections::HashMap::new(),
        &std::collections::HashMap::new(),
        &[first, second],
    )
    .expect("blob-qualified material binding");
    let [binding] = projected.as_slice() else {
        panic!("one appearance binding expected")
    };
    assert_eq!(
        binding.target,
        cadmpeg_ir::appearance::AppearanceTarget::Body(second_body)
    );
}

#[test]
fn presetless_assignment_matches_only_its_visual_guid() {
    let appearance_guid = "11111111-2222-3333-4444-555555555555";
    let mut appearance = cadmpeg_ir::appearance::Appearance {
        id: cadmpeg_ir::ids::AppearanceId::mint("f3d:test:appearance#catalog")
            .expect("identity grammar"),
        name: None,
        asset_guid: Some(appearance_guid.into()),
        library_id: None,
        visual_guid: Some(appearance_guid.into()),
        physical_token: None,
        schema: None,
        category: None,
        base_color: None,
        properties: std::collections::BTreeMap::new(),
        textures: Vec::new(),
    };
    let mut assignment = crate::records::references::DesignMaterialAssignment {
        id: "f3d:design:material-assignment#1".into(),
        asm_body_key: 7,
        asm_body_key_offset: 25,

        entity_suffix_offset: 33,
        entity_id: crate::records::identity::DesignEntityId::try_from("0_100".to_owned())
            .expect("valid entity ID"),
        entity_id_offset: 500,
        visual_guid: crate::records::references::DesignVisualToken::try_from(
            "AAAAAAAA-BBBB-CCCC-DDDD-EEEEEEEEEEEE".to_owned(),
        )
        .unwrap(),
        visual_guid_offset: 600,
        physical_token: None,
        visual_preset: None,
    };

    assert!(
        super::appearance_for_assignment(std::slice::from_ref(&appearance), &assignment)
            .expect("valid preset-less assignment")
            .is_none()
    );

    assignment.visual_guid =
        crate::records::references::DesignVisualToken::try_from(appearance_guid.to_owned())
            .unwrap();
    assert!(
        super::appearance_for_assignment(std::slice::from_ref(&appearance), &assignment)
            .expect("exact visual-token assignment")
            .is_some()
    );

    assignment.visual_guid = crate::records::references::DesignVisualToken::try_from(
        "AAAAAAAA-BBBB-CCCC-DDDD-EEEEEEEEEEEE".to_owned(),
    )
    .unwrap();
    assignment.visual_preset = Some(crate::records::identity::RecordedValue {
        value: "Prism-017".into(),
        offset: 0,
    });
    appearance.name = Some("Prism-017".into());
    assert!(
        super::appearance_for_assignment(std::slice::from_ref(&appearance), &assignment)
            .expect("present preset-name fallback")
            .is_some()
    );
}

#[test]
fn complete_visual_token_selects_one_revision_record() {
    let base_token = "11111111-2222-3333-4444-555555555555";
    let revised_token = "11111111-2222-3333-4444-555555555555_Post2015";
    let appearance = |id: &str, token: &str| cadmpeg_ir::appearance::Appearance {
        id: cadmpeg_ir::ids::AppearanceId::mint(id).expect("identity grammar"),
        name: None,
        asset_guid: Some(token.into()),
        library_id: None,
        visual_guid: Some(token.into()),
        physical_token: None,
        schema: None,
        category: None,
        base_color: None,
        properties: std::collections::BTreeMap::new(),
        textures: Vec::new(),
    };
    let appearances = [
        appearance("f3d:test:appearance#base", base_token),
        appearance("f3d:test:appearance#revised", revised_token),
    ];

    let selected = super::appearance_for_visual_token(
        &appearances,
        &crate::records::references::DesignVisualToken::try_from(revised_token.to_owned()).unwrap(),
        None,
    )
    .expect("unique complete visual token")
    .expect("revised appearance exists");
    assert_eq!(selected.id.as_str(), "f3d:test:appearance#revised");

    let duplicates = [
        appearance("f3d:test:appearance#first", revised_token),
        appearance("f3d:test:appearance#second", revised_token),
    ];
    assert!(matches!(
        super::appearance_for_visual_token(
            &duplicates,
            &crate::records::references::DesignVisualToken::try_from(revised_token.to_owned())
                .unwrap(),
            None
        ),
        Err(cadmpeg_core::CodecError::Malformed(_))
    ));
}

#[test]
fn visual_preset_fallback_requires_one_record() {
    let appearance = |id: &str| cadmpeg_ir::appearance::Appearance {
        id: cadmpeg_ir::ids::AppearanceId::mint(id).expect("identity grammar"),
        name: Some("Prism-017".into()),
        asset_guid: None,
        library_id: None,
        visual_guid: None,
        physical_token: None,
        schema: None,
        category: None,
        base_color: None,
        properties: std::collections::BTreeMap::new(),
        textures: Vec::new(),
    };
    let appearances = [
        appearance("f3d:test:appearance#first"),
        appearance("f3d:test:appearance#second"),
    ];

    assert!(matches!(
        super::appearance_for_visual_token(
            &appearances,
            &crate::records::references::DesignVisualToken::try_from(
                "11111111-2222-3333-4444-555555555555".to_owned()
            )
            .unwrap(),
            Some("Prism-017"),
        ),
        Err(cadmpeg_core::CodecError::Malformed(_))
    ));
}

#[test]
fn generic_connection_delta_rejects_unknown_and_truncated_forms() {
    let mut record = vec![0; 120];
    record[102] = 2;
    assert_eq!(super::generic_connection_delta(&record, 0), None);

    record[102] = 1;
    record[104..108].copy_from_slice(&1u32.to_le_bytes());
    record[108..112].copy_from_slice(&16u32.to_le_bytes());
    assert_eq!(super::generic_connection_delta(&record, 0), None);
}

#[test]
fn decoded_color_requires_finite_normalized_channels() {
    assert!(super::decoded_color([0.0, 0.25, 0.5, 1.0]).is_some());
    for invalid in [f64::NAN, f64::INFINITY, -0.01, 1.01] {
        assert!(super::decoded_color([invalid, 0.25, 0.5, 1.0]).is_none());
    }
}

#[test]
fn schema_primary_colour_wins_over_rival_colour_members() {
    for (schema, primary_id) in [
        ("GenericSchema", "generic_diffuse"),
        ("MetalSchema", "metal_color"),
        ("MetallicPaintSchema", "metallicpaint_base_color"),
        ("PlasticVinylSchema", "plasticvinyl_color"),
        ("PrismLayeredSchema", "layered_diffuse"),
        ("PrismMetalSchema", "metal_f0"),
        ("PrismOpaqueSchema", "opaque_albedo"),
        ("PrismTransparentSchema", "transparent_color"),
        ("PrismWoodSchema", "surface_albedo"),
    ] {
        let mut properties = std::collections::BTreeMap::from([
            color_property("common_Tint_color", [0.75, 0.75, 0.75, 1.0]),
            color_property("surface_albedo", [0.5, 0.5, 0.5, 1.0]),
            (
                "common_Tint_toggle".to_owned(),
                cadmpeg_protein::property::DecodedProperty {
                    value_offset: 0,
                    content: cadmpeg_protein::property::PropertyContent::Value {
                        value: cadmpeg_protein::property::PropertyValue::Boolean(false),
                        connections: Vec::new(),
                    },
                },
            ),
        ]);
        properties.insert(
            primary_id.to_owned(),
            cadmpeg_protein::property::DecodedProperty {
                value_offset: 0,
                content: cadmpeg_protein::property::PropertyContent::Value {
                    value: cadmpeg_protein::property::PropertyValue::Color(
                        [0.125, 0.25, 0.375, 1.0].map(|value| {
                            cadmpeg_ir::scalar::FiniteReal::new(value).expect("finite")
                        }),
                    ),
                    connections: Vec::new(),
                },
            },
        );
        let record = appearance_record(schema, properties);
        assert_eq!(
            super::appearance_base_color(&record).map(cadmpeg_ir::topology::Color::g),
            Some(0.25),
            "{schema} selects {primary_id}"
        );
    }
}

#[test]
fn enabled_common_tint_replaces_the_schema_primary_colour() {
    let mut properties = std::collections::BTreeMap::from([
        color_property("opaque_albedo", [0.125, 0.25, 0.375, 1.0]),
        color_property("surface_albedo", [0.5, 0.5, 0.5, 1.0]),
        color_property("common_Tint_color", [0.75, 0.625, 0.5, 1.0]),
    ]);
    properties.insert(
        "common_Tint_toggle".to_owned(),
        cadmpeg_protein::property::DecodedProperty {
            value_offset: 0,
            content: cadmpeg_protein::property::PropertyContent::Value {
                value: cadmpeg_protein::property::PropertyValue::Boolean(true),
                connections: Vec::new(),
            },
        },
    );
    let record = appearance_record("PrismOpaqueSchema", properties);
    assert_eq!(
        super::appearance_base_color(&record).map(cadmpeg_ir::topology::Color::g),
        Some(0.625)
    );
}

fn color_property(
    id: &str,
    color: [f64; 4],
) -> (String, cadmpeg_protein::property::DecodedProperty) {
    (
        id.to_owned(),
        cadmpeg_protein::property::DecodedProperty {
            value_offset: 0,
            content: cadmpeg_protein::property::PropertyContent::Value {
                value: cadmpeg_protein::property::PropertyValue::Color(
                    color.map(|value| cadmpeg_ir::scalar::FiniteReal::new(value).expect("finite")),
                ),
                connections: Vec::new(),
            },
        },
    )
}

fn appearance_record(
    schema: &str,
    properties: std::collections::BTreeMap<String, cadmpeg_protein::property::DecodedProperty>,
) -> cadmpeg_protein::DecodedRecord {
    cadmpeg_protein::DecodedRecord {
        ordinal: 0,
        logical_offset: 0,
        schema: schema.to_owned(),
        guid: "11111111-2222-3333-4444-555555555555".to_owned(),
        base: "Prism-001".to_owned(),
        asset_lib_id: String::new(),
        properties,
    }
}

fn texture_record(guid: &str, path: &str) -> cadmpeg_protein::DecodedRecord {
    cadmpeg_protein::DecodedRecord {
        ordinal: 0,
        logical_offset: 0,
        schema: "UnifiedBitmapSchema".to_owned(),
        guid: guid.to_owned(),
        base: "Texture-001".to_owned(),
        asset_lib_id: String::new(),
        properties: std::collections::BTreeMap::from([(
            "unifiedbitmap_Bitmap".to_owned(),
            cadmpeg_protein::property::DecodedProperty {
                value_offset: 0,
                content: cadmpeg_protein::property::PropertyContent::Value {
                    value: cadmpeg_protein::property::PropertyValue::TextureUri(vec![
                        path.to_owned()
                    ]),
                    connections: Vec::new(),
                },
            },
        )]),
    }
}

fn appearance_connected_to(texture_guid: &str) -> cadmpeg_protein::DecodedRecord {
    appearance_record(
        "GenericSchema",
        std::collections::BTreeMap::from([(
            "generic_diffuse".to_owned(),
            cadmpeg_protein::property::DecodedProperty {
                value_offset: 0,
                content: cadmpeg_protein::property::PropertyContent::Value {
                    value: cadmpeg_protein::property::PropertyValue::Color(
                        [0.25, 0.5, 0.75, 1.0].map(|value| {
                            cadmpeg_ir::scalar::FiniteReal::new(value).expect("finite")
                        }),
                    ),
                    connections: vec![texture_guid.to_owned()],
                },
            },
        )]),
    )
}

#[test]
fn equivalent_duplicate_texture_guids_bind_once() {
    let guid = "aaaaaaaa-1111-2222-3333-bbbbbbbbbbbb";
    let texture = texture_record(guid, "textures/albedo.png");
    let (appearances, untyped_count) =
        schema_appearances(&[appearance_connected_to(guid), texture.clone(), texture])
            .expect("equivalent texture records deduplicate");

    assert_eq!(untyped_count, 0);
    assert_eq!(appearances.len(), 1);
    assert_eq!(appearances[0].textures.len(), 1);
    assert_eq!(appearances[0].textures[0].paths, ["textures/albedo.png"]);
}

#[test]
fn conflicting_duplicate_texture_guids_reject_in_both_orders() {
    let guid = "aaaaaaaa-1111-2222-3333-bbbbbbbbbbbb";
    let first = texture_record(guid, "textures/first.png");
    let second = texture_record(guid, "textures/second.png");
    for textures in [[first.clone(), second.clone()], [second, first]] {
        let error = schema_appearances(&[
            appearance_connected_to(guid),
            textures[0].clone(),
            textures[1].clone(),
        ])
        .expect_err("one texture GUID cannot select conflicting payloads");

        assert!(matches!(error, cadmpeg_core::CodecError::Malformed(_)));
    }
}

#[test]
fn unknown_texture_distance_unit_omits_typed_texture_and_counts_loss() {
    let guid = "aaaaaaaa-1111-2222-3333-bbbbbbbbbbbb";
    let mut texture = texture_record(guid, "textures/albedo.png");
    texture.properties.insert(
        "unifiedbitmap_RealWorldScaleX".into(),
        cadmpeg_protein::property::DecodedProperty {
            value_offset: 0,
            content: cadmpeg_protein::property::PropertyContent::Value {
                value: cadmpeg_protein::property::PropertyValue::Distance {
                    unit: 0x0002_1008,
                    value: cadmpeg_ir::scalar::FiniteReal::new(3.0).expect("finite"),
                },
                connections: Vec::new(),
            },
        },
    );

    let (appearances, untyped_count) =
        schema_appearances(&[appearance_connected_to(guid), texture])
            .expect("unknown unit is retained as a typed-projection loss");

    assert_eq!(untyped_count, 1);
    assert!(appearances[0].textures.is_empty());
}

fn schema_appearances(
    records: &[cadmpeg_protein::DecodedRecord],
) -> Result<(Vec<cadmpeg_ir::appearance::Appearance>, usize), cadmpeg_core::CodecError> {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )?;
    super::appearances_from_schema_records(&ctx, records)
}

#[test]
fn face_appearance_bindings_stay_unique_when_one_appearance_binds_many_faces() {
    use cadmpeg_ir::appearance::{Appearance, AppearanceTarget};
    use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue, SourceAttribute};

    // One appearance attribute GUID reaches every face carrying it, so the
    // assignment pair repeats across those faces. The face id has to enter the
    // binding id, or the arena holds colliding ids and fails both the global
    // identity check and the strict arena-order check.
    let face_guid = "aaaaaaaa-1111-2222-3333-bbbbbbbbbbbb";
    let visual_family = "11111111-2222-3333-4444-555555555555";
    let visual_guid = "11111111-2222-3333-4444-555555555555_Post2015";
    let mut ir = cadmpeg_ir::CadIr::empty();
    for face in [
        "test:model:face#1",
        "test:model:face#2",
        "test:model:face#3",
    ] {
        ir.model.attributes.push(SourceAttribute {
            id: format!("test:model:attribute#{}", face.rsplit_once('#').unwrap().1)
                .try_into()
                .expect("valid identity"),
            target: AttributeTarget::Face(face.try_into().expect("valid identity")),
            name: "ATTRIB_CUSTOM-attrib".into(),
            values: vec![
                AttributeValue::String("NEUTRON_Material_attrib_def".into()),
                AttributeValue::String(face_guid.into()),
            ],
        });
    }
    let appearance = |id: &str, token: &str| Appearance {
        id: id.try_into().expect("valid identity"),
        name: None,
        asset_guid: None,
        library_id: None,
        visual_guid: Some(token.into()),
        physical_token: None,
        schema: None,
        category: None,
        base_color: None,
        properties: std::collections::BTreeMap::new(),
        textures: Vec::new(),
    };
    // Base record first so prefix-only selection cannot bind it.
    ir.model.appearances.extend([
        appearance("test:model:appearance#base", visual_family),
        appearance("test:model:appearance#revision", visual_guid),
    ]);

    crate::decode::resolve_face_appearance_bindings(
        &mut ir,
        &[crate::materials::FaceAppearanceAssignment {
            face_guid: face_guid.into(),
            visual_guid: crate::records::references::DesignVisualToken::try_from(
                visual_guid.to_owned(),
            )
            .unwrap(),
            color: None,
        }],
    )
    .expect("unique face appearance");

    assert_eq!(ir.model.appearance_bindings.len(), 3);
    let ids = ir
        .model
        .appearance_bindings
        .iter()
        .map(|binding| binding.id.clone())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(ids.len(), 3);
    assert!(ir
        .model
        .appearance_bindings
        .iter()
        .all(|binding| binding.appearance.as_str() == "test:model:appearance#revision"));
    let targets = ir
        .model
        .appearance_bindings
        .iter()
        .map(|binding| binding.target.clone())
        .collect::<Vec<_>>();
    assert_eq!(
        targets,
        vec![
            AppearanceTarget::Face("test:model:face#1".try_into().expect("valid identity")),
            AppearanceTarget::Face("test:model:face#2".try_into().expect("valid identity")),
            AppearanceTarget::Face("test:model:face#3".try_into().expect("valid identity")),
        ]
    );

    // The decode path sorts by id after resolving; that sort is only a total
    // order over the arena when the ids are distinct.
    ir.model.appearance_bindings.sort_by(|a, b| a.id.cmp(&b.id));
    assert!(ir
        .model
        .appearance_bindings
        .windows(2)
        .all(|pair| pair[0].id < pair[1].id));

    ir.model.attributes[0].values.push(AttributeValue::String(
        "cccccccc-1111-2222-3333-dddddddddddd".into(),
    ));
    let error = crate::decode::resolve_face_appearance_bindings(
        &mut ir,
        &[crate::materials::FaceAppearanceAssignment {
            face_guid: face_guid.into(),
            visual_guid: crate::records::references::DesignVisualToken::try_from(
                visual_guid.to_owned(),
            )
            .unwrap(),
            color: None,
        }],
    )
    .expect_err("a face material attribute with two GUID operands is ambiguous");
    assert!(error
        .to_string()
        .contains("exactly one lower-case face GUID"));
}

#[test]
fn legacy_face_assignment_color_precedes_appearance_base_but_not_brep_color() {
    use cadmpeg_ir::appearance::Appearance;
    use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue, SourceAttribute};
    use cadmpeg_ir::topology::Color;

    let face_guid = "aaaaaaaa-1111-2222-3333-bbbbbbbbbbbb";
    let visual_guid = "11111111-2222-3333-4444-555555555555_Post2015";
    let assignment_color = Color::new(0.75, 0.25, 0.125, 1.0).expect("valid color");
    let explicit_color = Color::new(0.1, 0.8, 0.2, 1.0).expect("valid color");
    let make_ir = || {
        let mut ir = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted");
        let face = ir.model.faces[0].id.clone();
        ir.model.attributes.push(SourceAttribute {
            id: "f3d:test:attribute#face-material"
                .try_into()
                .expect("valid identity"),
            target: AttributeTarget::Face(face),
            name: "ATTRIB_CUSTOM-attrib".into(),
            values: vec![
                AttributeValue::String("NEUTRON_Material_attrib_def".into()),
                AttributeValue::String(face_guid.into()),
            ],
        });
        ir.model.appearances.push(Appearance {
            id: "test:model:appearance#face"
                .try_into()
                .expect("valid identity"),
            name: None,
            asset_guid: None,
            library_id: None,
            visual_guid: Some(visual_guid.into()),
            physical_token: None,
            schema: None,
            category: None,
            base_color: Some(Color::new(0.0, 0.0, 1.0, 1.0).expect("valid color")),
            properties: std::collections::BTreeMap::new(),
            textures: Vec::new(),
        });
        ir
    };
    let assignment = crate::materials::FaceAppearanceAssignment {
        face_guid: face_guid.into(),
        visual_guid: crate::records::references::DesignVisualToken::try_from(
            visual_guid.to_owned(),
        )
        .unwrap(),
        color: Some(assignment_color),
    };

    let mut ir = make_ir();
    crate::decode::resolve_face_appearance_bindings(&mut ir, std::slice::from_ref(&assignment))
        .expect("legacy face assignment");
    assert_eq!(ir.model.faces[0].color, Some(assignment_color));
    assert_eq!(ir.model.appearance_bindings.len(), 1);

    let mut explicit = make_ir();
    explicit.model.faces[0].color = Some(explicit_color);
    crate::decode::resolve_face_appearance_bindings(
        &mut explicit,
        std::slice::from_ref(&assignment),
    )
    .expect("explicit face color");
    assert_eq!(explicit.model.faces[0].color, Some(explicit_color));

    let mut no_asset = make_ir();
    no_asset.model.appearances.clear();
    crate::decode::resolve_face_appearance_bindings(
        &mut no_asset,
        std::slice::from_ref(&assignment),
    )
    .expect("face color independent of appearance lookup");
    assert_eq!(no_asset.model.faces[0].color, Some(assignment_color));
    assert!(no_asset.model.appearance_bindings.is_empty());
}

#[test]
fn duplicate_face_assignments_reject_conflicting_colors() {
    use cadmpeg_ir::topology::Color;

    let face_guid = "aaaaaaaa-1111-2222-3333-bbbbbbbbbbbb";
    let visual_guid = "11111111-2222-3333-4444-555555555555_Post2015";
    let assignment = |r| crate::materials::FaceAppearanceAssignment {
        face_guid: face_guid.into(),
        visual_guid: crate::records::references::DesignVisualToken::try_from(
            visual_guid.to_owned(),
        )
        .unwrap(),
        color: Some(Color::new(r, 0.25, 0.5, 1.0).expect("valid color")),
    };
    let mut ir = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted");
    let error = crate::decode::resolve_face_appearance_bindings(
        &mut ir,
        &[assignment(0.25), assignment(0.75)],
    )
    .expect_err("one face material GUID cannot carry two neutral colors");
    assert!(error.to_string().contains("conflicting neutral colors"));
}

mod generated_appearance;

#[test]
fn decode_rejects_invalid_instance_property_page_framing() {
    let mut properties = generated_instance_properties_for("11111111-2222-3333-4444-555555555555");
    properties[STREAM_HEADER_LEN + 4] ^= 1;
    let f3d = f3d_with_smbh_and_instance_properties(&synthetic_geometry_smbh(), &[properties]);

    let error = F3dCodec
        .decode(&mut Cursor::new(f3d), &DecodeOptions::default())
        .expect_err("invalid Protein page framing must reject material decode");

    assert!(matches!(
        error,
        cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::Malformed(_))
    ));
}

#[test]
fn generated_act_native_validation_rejects_structural_drift() {
    let source = f3d_with_smbh_and_protein(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated ACT decode");

    let mut colliding_root = decoded.ir().clone();
    update_f3d_native(&mut colliding_root, |native| {
        native.act_component_links[0].record_index = native.act_entities[0].record_index();
    });
    assert!(crate::test_support::with_decode_context(|ctx| {
        crate::validate::validate_native(ctx, &colliding_root)
            .expect("validate the native namespace")
    })
    .iter()
    .any(|finding| finding.message.contains("ACT component link")));

    let (mut wrong_registry, _, _) = decoded.into_parts();
    update_f3d_native(&mut wrong_registry, |native| {
        native.act_registry_channels[1].ordinal = 0;
    });
    assert!(crate::test_support::with_decode_context(|ctx| {
        crate::validate::validate_native(ctx, &wrong_registry)
            .expect("validate the native namespace")
    })
    .iter()
    .any(|finding| finding.message.contains("ACT channel-registry entry")));
}

#[test]
fn decode_binds_revision_suffixed_protein_visual_guid() {
    let visual = "11111111-2222-3333-4444-555555555555_Post2015_Post2015";
    let f3d = f3d_with_smbh_and_protein_guids(&synthetic_geometry_smbh(), &[visual]);
    let result = F3dCodec
        .decode(&mut Cursor::new(f3d), &DecodeOptions::default())
        .expect("revision-suffixed Protein decode");

    assert_eq!(result.ir().model.appearances.len(), 1);
    assert_eq!(
        result.ir().model.appearances[0].visual_guid.as_deref(),
        Some(visual)
    );
    assert_eq!(result.ir().model.appearance_bindings.len(), 1);
    assert_eq!(
        result.ir().model.appearance_bindings[0].appearance,
        result.ir().model.appearances[0].id
    );
}

#[test]
fn decode_transfers_generated_custom_attribute() {
    let f3d = f3d_with_smbh(&synthetic_geometry_with_attribute_smbh());
    let mut cur = Cursor::new(f3d);
    let result = F3dCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();

    assert_eq!(result.ir().model.attributes.len(), 2);
    let attribute = result
        .ir()
        .model
        .attributes
        .iter()
        .find(|attribute| {
            attribute.values.iter().any(|value| {
                matches!(
                    value,
                    cadmpeg_ir::attributes::AttributeValue::String(text)
                        if text == "generic_tag_attrib_def"
                )
            })
        })
        .expect("generic tag attribute");
    assert_eq!(attribute.name, "ATTRIB_CUSTOM-attrib");
    assert!(matches!(
        &attribute.target,
        cadmpeg_ir::attributes::AttributeTarget::Body(body) if body == &result.ir().model.bodies[0].id
    ));
    assert!(attribute.values.iter().any(|value| matches!(
        value,
        cadmpeg_ir::attributes::AttributeValue::String(text) if text == "322"
    )));
    assert_eq!(f3d_native(result.ir()).persistent_design_links.len(), 2);
    assert_eq!(
        f3d_native(result.ir()).persistent_design_links[1]
            .design_id
            .as_str(),
        "322"
    );
    assert_eq!(
        f3d_native(result.ir()).persistent_design_links[1].design_reference,
        7
    );
    let native = f3d_native(result.ir());
    let current = crate::records::sketch_links::current_persistent_design_links(
        &native.persistent_design_links,
    );
    assert_eq!(
        current
            .values()
            .map(|link| link.ordinal)
            .collect::<Vec<_>>(),
        [native.persistent_design_links[1].ordinal]
    );
    assert!(attribute.values.iter().any(|value| matches!(
        value,
        cadmpeg_ir::attributes::AttributeValue::String(text) if text == "900"
    )));
    assert_eq!(f3d_native(result.ir()).creation_timestamps.len(), 1);
    assert_eq!(
        f3d_native(result.ir()).creation_timestamps[0]
            .unix_microseconds
            .get(),
        1_579_392_000_000_007.0
    );
}

#[test]
fn source_less_tolerant_vertex_retains_custom_attribute_ownership() {
    use cadmpeg_ir::attributes::AttributeTarget;

    let mut source = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted");
    source.source = None;
    source
        .set_native_unknowns(&cadmpeg_test_support::service_decode_context(), "f3d", &[])
        .unwrap();
    let vertex = source.model.vertices[0].id.clone();
    source.model.vertices[0].tolerance =
        Some(cadmpeg_ir::scalar::PositiveReal::new(0.025).expect("positive finite tolerance"));
    f3d_native_mut(&mut source).creation_timestamps =
        vec![crate::records::recipes::CreationTimestamp {
            id: "f3d:asm:creation-timestamp#generated".into(),
            target: AttributeTarget::Vertex(vertex),
            record_index: 0,
            unix_microseconds: cadmpeg_ir::scalar::FiniteReal::new(1_579_392_000_000_037.0)
                .unwrap(),
        }];

    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less tolerant vertex encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less tolerant vertex decode");

    let tolerant_vertex = round_trip
        .ir()
        .model
        .vertices
        .iter()
        .find(|vertex| vertex.tolerance.map(cadmpeg_ir::scalar::PositiveReal::get) == Some(0.025))
        .expect("tolerant vertex");
    let attribute = round_trip
        .ir()
        .model
        .attributes
        .iter()
        .find(|attribute| {
            attribute.name == "ATTRIB_CUSTOM-attrib"
                && attribute.target == AttributeTarget::Vertex(tolerant_vertex.id.clone())
        })
        .expect("tolerant vertex attribute");
    assert_eq!(
        attribute.target,
        AttributeTarget::Vertex(tolerant_vertex.id.clone())
    );
    assert_eq!(
        f3d_native(round_trip.ir()).creation_timestamps[0]
            .unix_microseconds
            .get(),
        1_579_392_000_000_037.0
    );
}

#[test]
fn generated_f3d_rewrites_creation_timestamp() {
    let source = f3d_with_smbh(&synthetic_geometry_with_attribute_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated timestamp decode");
    let (mut edited, _, fidelity) = decoded.into_parts();
    let expected = 1_704_067_200_000_009.0;
    update_f3d_native(&mut edited, |native| {
        assert_eq!(native.creation_timestamps[0].record_index, 20);
        native.creation_timestamps[0].unix_microseconds =
            cadmpeg_ir::scalar::FiniteReal::new(expected).unwrap();
    });

    let mut regenerated = Vec::new();
    crate::test_support::plan_inherited_write(&edited, &fidelity, &mut regenerated)
        .expect("timestamp regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated timestamp decode");
    assert_eq!(
        f3d_native(round_trip.ir()).creation_timestamps[0]
            .unix_microseconds
            .get(),
        expected
    );
}

#[test]
fn decode_transfers_generated_sketch_curve_link() {
    let f3d = f3d_with_smbh(&synthetic_geometry_with_sketch_link_smbh(
        SketchLinkForm::Tagged("113 0 1 0 2 3"),
    ));
    let result = F3dCodec
        .decode(&mut Cursor::new(f3d), &DecodeOptions::default())
        .unwrap();

    let link = f3d_native(result.ir())
        .sketch_curve_links
        .first()
        .cloned()
        .unwrap();
    assert_eq!(
        link.target,
        cadmpeg_ir::attributes::AttributeTarget::Coedge(
            cadmpeg_ir::ids::CoedgeId::mint("f3d:brep:entity#7").expect("identity grammar")
        )
    );
    assert_eq!(link.sketch_curve_id, 113);
    assert_eq!(link.sense.map(i64::from), Some(1));
    assert_eq!((link.role, link.closure), (2, 3));
}

/// The one sketch-curve link a synthetic archive carries under `form`.
fn decoded_sketch_link(
    form: SketchLinkForm<'_>,
) -> Option<crate::records::sketch_links::SketchCurveLink> {
    let f3d = f3d_with_smbh(&synthetic_geometry_with_sketch_link_smbh(form));
    let result = F3dCodec
        .decode(&mut Cursor::new(f3d), &DecodeOptions::default())
        .unwrap();
    f3d_native(result.ir()).sketch_curve_links.first().cloned()
}

#[test]
fn a_sketch_link_keeps_the_second_tuple_member_the_source_writes() {
    let link = decoded_sketch_link(SketchLinkForm::Tagged("113 4550 1 0 2 3"))
        .expect("a non-zero second member does not refuse the link");
    assert_eq!((link.sketch_curve_id, link.ref_b), (113, 4550));
    assert_eq!(
        (link.sense.map(i64::from), link.role, link.closure),
        (Some(1), 2, 3)
    );
    // The member reaches the full unsigned 64-bit range, so it does not fit the
    // signed reading the other members take.
    let link = decoded_sketch_link(SketchLinkForm::Tagged("113 18446744073709551615 1 0 2 3"))
        .expect("a second member above i64::MAX does not refuse the link");
    assert_eq!(link.ref_b, u64::MAX);
}

#[test]
fn a_sketch_link_decodes_in_every_payload_form() {
    // Form 2 writes the five members as integers with a trailing `0`; form 0
    // writes them with no trailing member at all.
    for form in [
        SketchLinkForm::Integers(2, &[113, 4550, 1, 2, 3, 0]),
        SketchLinkForm::Integers(0, &[113, 4550, 1, 2, 3]),
    ] {
        let link = decoded_sketch_link(form).expect("integer-form sketch link");
        assert_eq!((link.sketch_curve_id, link.ref_b), (113, 4550));
        assert_eq!(
            (link.sense.map(i64::from), link.role, link.closure),
            (Some(1), 2, 3)
        );
    }
    // An integer form spells the unconstrained sense as the signed `-1` of the
    // same 32-bit pattern the tagged field spells as `4294967295`.
    assert_eq!(
        decoded_sketch_link(SketchLinkForm::Integers(2, &[113, 0, -1, 2, 3, 0]))
            .expect("integer-form sketch link")
            .sense,
        None
    );
    assert!(decoded_sketch_link(SketchLinkForm::Integers(2, &[113, 0, 1, 2, 3])).is_none());
    assert!(decoded_sketch_link(SketchLinkForm::Integers(0, &[113, 0, 1, 2, 3, 0])).is_none());
}

#[test]
fn an_unconstrained_sketch_link_sense_round_trips_in_its_source_spelling() {
    use crate::records::sketch_links::SketchCurveLink;

    let f3d = f3d_with_smbh(&synthetic_geometry_with_sketch_link_smbh(
        SketchLinkForm::Tagged("113 0 4294967295 0 2 3"),
    ));
    let decoded = F3dCodec
        .decode(&mut Cursor::new(f3d), &DecodeOptions::default())
        .unwrap();
    assert_eq!(
        f3d_native(decoded.ir()).sketch_curve_links[0]
            .sense
            .map(i64::from),
        None,
        "4294967295 is the disabled sense, not a stored one"
    );

    let mut source_less = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted");
    let coedge = source_less.model.coedges[0].id.clone();
    f3d_native_mut(&mut source_less).sketch_curve_links = vec![SketchCurveLink {
        id: "f3d:generated:sketch-curve-link#0".into(),
        target: cadmpeg_ir::attributes::AttributeTarget::Coedge(coedge),
        sketch_curve_id: 113,
        ref_b: 0,
        sense: None,
        role: 2,
        closure: 3,
    }];
    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("source-less sketch-link encode");
    assert!(
        encoded
            .windows(b"113 0 4294967295 0 2 3".len())
            .any(|window| window == b"113 0 4294967295 0 2 3"),
        "the writer must re-emit the disabled sense in its source spelling"
    );
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less sketch-link round trip");
    let link = &f3d_native(round_trip.ir()).sketch_curve_links[0];
    assert_eq!(
        (link.sketch_curve_id, link.sense.map(i64::from)),
        (113, None)
    );
    assert_eq!((link.role, link.closure), (2, 3));
}

#[test]
fn decode_mixed_analytic_and_unknown_faces_sharing_an_edge() {
    use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};

    let f3d = f3d_with_smbh(&synthetic_mixed_smbh());
    let mut cur = Cursor::new(f3d);
    let result = F3dCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();

    assert!(result.report().geometry_transferred());
    // Two faces (one plane, one spline), sharing one edge; five edges total.
    assert_eq!(result.ir().model.faces.len(), 2);
    assert_eq!(result.ir().model.edges.len(), 5);
    assert_eq!(result.ir().model.vertices.len(), 4);
    assert_eq!(result.ir().model.coedges.len(), 6);

    // Exactly one analytic (plane) and one unknown surface.
    let planes = result
        .ir()
        .model
        .surfaces
        .iter()
        .filter(|s| {
            matches!(
                s.geometry,
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(_))
            )
        })
        .count();
    let unknowns = result
        .ir()
        .model
        .surfaces
        .iter()
        .filter(|s| {
            matches!(
                s.geometry,
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { .. })
            )
        })
        .count();
    assert_eq!((planes, unknowns), (1, 1));

    // The shared edge is used by two mutually-referencing coedges of opposite
    // sense (the manifold invariant), which coedge-pairing validation enforces.
    let paired = result
        .ir()
        .model
        .coedges
        .iter()
        .filter(|c| c.radial_next != c.id)
        .count();
    assert_eq!(paired, 2);

    let report = cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new())
        .expect("validate the neutral model");
    assert!(report.is_ok(), "findings: {:?}", report.findings);
    assert_eq!(result.ir().model.surfaces.len(), 2);
}

#[test]
fn body_visibility_maps_asm_keys_through_member_nodes() {
    let mut bulk = Vec::new();
    let mut primary_records = vec![(899u64, 0u64)];
    // Typed body-map record: indexed header, ten zero bytes, pair count,
    // (ASM key, member) pairs, the 12-byte tail, then the blob name.
    bulk.extend_from_slice(&3u32.to_le_bytes());
    bulk.extend_from_slice(b"256");
    bulk.extend_from_slice(&899u32.to_le_bytes());
    bulk.extend_from_slice(&[0; crate::design::body::GENERATED_BODY_MAP_ZERO_PREFIX_LEN]);
    bulk.extend_from_slice(&2u32.to_le_bytes());
    for (key, member) in [(3u64, 269u64), (6, 533)] {
        bulk.extend_from_slice(&key.to_le_bytes());
        bulk.extend_from_slice(&member.to_le_bytes());
    }
    bulk.extend_from_slice(&1793u64.to_le_bytes());
    bulk.extend_from_slice(&0u32.to_le_bytes());
    lp_utf16(&mut bulk, "BREP.synthetic.smbh");
    // Typed browser-node records: indexed header, ten-byte base payload,
    // GUID, hidden flag, `01 01` marker, and member id.
    for (record_index, guid, hidden, member) in [
        (900u32, "b412e170-dc0c-4932-b699-43fc72cc8b13", 0u8, 269u64),
        (901, "d4b1078c-43bf-4f6d-a50a-963f94273901", 1, 533),
    ] {
        primary_records.push((
            u64::from(record_index),
            u64::try_from(bulk.len()).expect("synthetic BulkStream offset"),
        ));
        bulk.extend_from_slice(&3u32.to_le_bytes());
        bulk.extend_from_slice(b"257");
        bulk.extend_from_slice(&record_index.to_le_bytes());
        bulk.extend_from_slice(&[0; 10]);
        lp_utf16(&mut bulk, guid);
        bulk.push(hidden);
        bulk.extend_from_slice(&[0x01, 0x01]);
        bulk.extend_from_slice(&member.to_le_bytes());
    }

    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    write_synthetic_manifests(&mut zip, stored);
    zip.start_file("FusionAssetName[Active]/Design1/BulkStream.dat", stored)
        .unwrap();
    zip.write_all(&bulk).unwrap();
    zip.start_file("FusionAssetName[Active]/Design1/MetaStream.dat", stored)
        .unwrap();
    zip.write_all(&design_metastream_with_records(
        &[
            (
                crate::design::body::BODY_MAP_CARRIER_TYPE_GUID,
                crate::design::body::BODY_MAP_CARRIER_BASE_TYPE_GUID,
                crate::design::body::BODY_MAP_CARRIER_TYPE_VERSION,
                crate::records::entity_header::DESIGN_MODULE_BODY,
                &[899],
            ),
            (
                crate::design::presentation::BROWSER_NODE_TYPE_GUID,
                crate::design::presentation::BROWSER_NODE_BASE_TYPE_GUID,
                crate::design::presentation::BROWSER_NODE_TYPE_VERSION,
                crate::records::entity_header::DESIGN_MODULE_FUSION,
                &[900, 901],
            ),
        ],
        &primary_records,
    ))
    .unwrap();
    let bytes = zip.finish().unwrap().into_inner();

    with_scan(&bytes, |scan| {
        let visibility = crate::design::decode::body::decode_all_body_visibility(scan).unwrap();
        assert_eq!(
            visibility
                .get(&("BREP.synthetic.smbh".into(), 3))
                .map(|item| item.visible),
            Some(true),
            "flag 0 decodes visible"
        );
        assert_eq!(
            visibility
                .get(&("BREP.synthetic.smbh".into(), 6))
                .map(|item| item.visible),
            Some(false),
            "flag 1 decodes hidden"
        );

        assert!(!visibility.contains_key(&("BREP.other.smbh".into(), 3)));
    });
}

#[test]
fn protein_revision_suffix_distinguishes_visual_record_identity() {
    let token = |text: &str| {
        crate::records::references::DesignVisualToken::try_from(text.to_owned()).unwrap()
    };
    assert!(
        !token("7DD7765D-CA8C-4A38-B156-B3B4916E0C17_Post2015_Post2015")
            .matches(&token("7dd7765d-ca8c-4a38-b156-b3b4916e0c17"))
    );
    assert!(token("7DD7765D-CA8C-4A38-B156-B3B4916E0C17_Post2015")
        .matches(&token("7dd7765d-ca8c-4a38-b156-b3b4916e0c17_Post2015")));
    assert!(crate::records::references::DesignVisualToken::try_from(
        "not-a-guid_Post2015".to_owned()
    )
    .is_err());
}

/// A report carrying the unconditional appearance loss that
/// `build_container_report` and `build_geometry_report` state before appearance
/// decoding runs.
fn appearance_loss_report() -> cadmpeg_ir::codec::DecodeBody {
    cadmpeg_ir::codec::DecodeBody {
        transfer: cadmpeg_ir::report::decode::DecodeTransfer::full(false),
        coverage: cadmpeg_ir::report::decode::Coverage::default(),
        losses: vec![F3dLossCode::MaterialNotTransferred.note(
            "Materials/appearances (.protein assets, ACT/design assignments) were not \
             transferred.",
        )],
        notes: Vec::new(),
        transfer_ledger: cadmpeg_ir::report::decode::TransferLedger::default(),
    }
}

fn opaque_appearance(guid: &str) -> cadmpeg_ir::appearance::Appearance {
    cadmpeg_ir::appearance::Appearance {
        id: cadmpeg_ir::ids::AppearanceId::mint(format!("f3d:design:appearance#{guid}"))
            .expect("identity grammar"),
        name: Some("Prism-Opaque".to_owned()),
        asset_guid: Some(guid.to_owned()),
        library_id: None,
        visual_guid: Some(guid.to_owned()),
        physical_token: None,
        schema: Some("PrismOpaqueSchema".to_owned()),
        category: None,
        base_color: Some(
            cadmpeg_ir::topology::Color::new(0.5, 0.5, 0.5, 1.0).expect("valid color"),
        ),
        properties: std::collections::BTreeMap::new(),
        textures: Vec::new(),
    }
}

fn material_losses(report: &cadmpeg_ir::codec::DecodeBody) -> Vec<&str> {
    report
        .losses
        .iter()
        .filter(|loss| loss.code.category() == cadmpeg_ir::report::loss::LossCategory::Material)
        .map(|loss| loss.message.as_str())
        .collect()
}

/// The candidate window is the strings between the appearance marker and the
/// visual token, and the token itself is outside it.
#[test]
fn a_body_node_candidate_reads_the_strings_between_the_marker_and_the_visual_token() {
    const APPEARANCE_MARKER: &str = "C1EEA57C-3F56-45FC-B8CB-A9EC46A9994C";
    let strings = vec![
        (0usize, "prefix".to_string()),
        (1, APPEARANCE_MARKER.to_string()),
        (2, "node-a".to_string()),
        (3, "node-b".to_string()),
        (4, "visual".to_string()),
    ];
    let nodes = std::collections::HashMap::from([
        ("node-a".to_string(), 7u64),
        ("node-b".to_string(), 9u64),
    ]);
    assert_eq!(
        crate::materials::body_node_candidate(&strings, 3, &nodes),
        Some(7),
        "the window ends before the visual token"
    );
    assert_eq!(
        crate::materials::body_node_candidate(&strings, 2, &nodes),
        None,
        "a window with no candidate names no entity"
    );
    assert_eq!(
        crate::materials::body_node_candidate(&strings, 4, &nodes),
        None,
        "two candidates that disagree name no entity"
    );
}

#[test]
fn a_protein_appearance_record_truncated_past_its_guid_is_refused() {
    // The record states its schema and GUID and then declares a third string
    // no remaining byte can carry. The two strings after the GUID are never
    // read, but their width is what places the colour carrier, so the record
    // states no writable offset and the patch is a refusal.
    let guid = "11111111-2222-3333-4444-555555555555";
    let mut logical = RECORD_MARKER.to_vec();
    super::push_lp(&mut logical, "GenericSchema").unwrap();
    super::push_lp(&mut logical, guid).unwrap();
    logical.extend_from_slice(&u32::MAX.to_le_bytes());
    let instance = super::page_logical(&logical).unwrap();

    let options = crate::zip_write::file_options(CompressionMethod::Stored);
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    zip.start_file("AssetData/InstanceProperties.bin", options)
        .unwrap();
    zip.write_all(&instance).unwrap();
    let protein = zip.finish().unwrap().into_inner();

    let edits = std::collections::BTreeMap::from([(
        guid.to_string(),
        crate::materials::ProteinAppearanceEdit {
            color: Some(
                cadmpeg_ir::topology::Color::new(0.5, 0.25, 0.125, 1.0).expect("valid color"),
            ),
            properties: std::collections::BTreeMap::new(),
        },
    )]);
    let error = crate::materials::patch_protein_appearances(&protein, &edits, &mut Vec::new())
        .expect_err("a record that ends at its GUID places no colour carrier");
    assert!(
        error
            .to_string()
            .contains("truncated at the string after its GUID"),
        "{error}"
    );
}

mod assignment_losses;

mod appearance_assignments;

#[test]
fn generic_transparency_sets_opacity_without_changing_rgb() {
    use cadmpeg_protein::property::{DecodedProperty, PropertyContent, PropertyValue};
    for transparency in [0.0, 0.25, 0.75, 1.0] {
        let record = appearance_record(
            "GenericSchema",
            std::collections::BTreeMap::from([
                color_property("generic_diffuse", [0.125, 0.25, 0.375, 1.0]),
                (
                    "generic_transparency".to_owned(),
                    DecodedProperty {
                        value_offset: 0,
                        content: PropertyContent::Value {
                            value: PropertyValue::Float(
                                cadmpeg_ir::scalar::FiniteReal::new(transparency).unwrap(),
                            ),
                            connections: Vec::new(),
                        },
                    },
                ),
            ]),
        );
        let (appearances, _) = schema_appearances(&[record]).unwrap();
        let color = appearances[0].base_color.unwrap();
        assert_eq!([color.r(), color.g(), color.b()], [0.125, 0.25, 0.375]);
        assert_eq!(f64::from(color.a()), 1.0 - transparency);
    }
}

#[test]
fn generic_transparency_refuses_values_outside_the_unit_interval() {
    use cadmpeg_protein::property::{DecodedProperty, PropertyContent, PropertyValue};
    for transparency in [-0.25, 1.25] {
        let record = appearance_record(
            "GenericSchema",
            std::collections::BTreeMap::from([
                color_property("generic_diffuse", [0.125, 0.25, 0.375, 1.0]),
                (
                    "generic_transparency".to_owned(),
                    DecodedProperty {
                        value_offset: 0,
                        content: PropertyContent::Value {
                            value: PropertyValue::Float(
                                cadmpeg_ir::scalar::FiniteReal::new(transparency).unwrap(),
                            ),
                            connections: Vec::new(),
                        },
                    },
                ),
            ]),
        );
        let error = schema_appearances(&[record]).unwrap_err();
        assert!(error.to_string().contains("generic_transparency"));
    }
}

#[test]
fn generic_transparency_absence_preserves_carrier_alpha() {
    let record = appearance_record(
        "GenericSchema",
        std::collections::BTreeMap::from([color_property(
            "generic_diffuse",
            [0.125, 0.25, 0.375, 0.5],
        )]),
    );
    let (appearances, _) = schema_appearances(&[record]).unwrap();
    assert_eq!(appearances[0].base_color.unwrap().a(), 0.5);
}
