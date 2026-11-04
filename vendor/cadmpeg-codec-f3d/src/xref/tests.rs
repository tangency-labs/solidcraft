// SPDX-License-Identifier: Apache-2.0
//! XREF and BREP-less document tests.
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

use cadmpeg_core::decode::u64_from_index;

use cadmpeg_test_support::EditableDecodeResult;

use std::io::{Cursor, Write};

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use zip::CompressionMethod;

use crate::loss::F3dLossCode;
use crate::test_support::assembly_test::{
    f3d_with_redirections_json, f3d_without_brep, redirections_json,
};
use crate::test_support::manifest_test::write_synthetic_manifests;
use crate::test_support::native_test::f3d_native;
use crate::test_support::streams_test::design_metastream_with_records;
use crate::test_support::{cross_document_reference, local_reference, lp_utf16};
use crate::F3dCodec;

use super::OccurrencePlacement;

const EPS_PLACEMENT_TRANSLATION: f64 = 1.0e-12;

fn with_docstruct_scan(
    f: impl FnOnce(&cadmpeg_core::decode::DecodeArena, &crate::container::ContainerScan<'_>),
) {
    let bytes = f3d_without_brep("assembly-design", "root.f3d", &[]);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::default();
    let (ctx, root) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let scan = crate::container::scan(&ctx, root).unwrap();
    f(&arena, &scan);
}

#[test]
fn docstruct_json_refuses_recursion_limit() {
    with_docstruct_scan(|arena, scan| {
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_recursion_depth = 0;
        let ctx = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], arena, &policy)
            .unwrap()
            .0;
        let error = super::docstruct(&ctx, scan).unwrap_err();
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "parse F3D properties JSON")
        );
    });
}

#[test]
fn xref_record_frame_scan_preserves_boundaries() {
    let mut bytes = vec![0; 35];
    bytes[..4].copy_from_slice(&3_u32.to_le_bytes());
    bytes[4..7].copy_from_slice(b"123");
    bytes[16..20].copy_from_slice(&3_u32.to_le_bytes());
    bytes[20..23].copy_from_slice(b"456");
    let frames = super::indexed_records(&bytes);
    assert_eq!(frames.len(), 2);
    assert_eq!((frames[0].offset, frames[0].end), (0, 16));
    assert_eq!((frames[1].offset, frames[1].end), (16, 35));
}

#[test]
fn redirections_keep_neutron_role_and_data_independent() {
    let table = super::parse(&cadmpeg_test_support::service_decode_context(),
        br#"{"name":"RedirectionsStream","schema-version":0,"designs":[{"file-version":1,"targetFileName":"root.f3d","displayName":"root","lineageUrn":"urn:l","versionUrn":"urn:v"}],"references":[{"from":"root.f3d","relativePath":"part.f3d","type":"XREF","properties":[{"neutronRole":{"value":"role-guid","dataType":"STRING"}},{"neutronData":{"value":"data-guid","dataType":"STRING"}}]}]}"#,
    )
    .expect("redirections JSON");
    assert_eq!(table.references.len(), 1);
    assert_eq!(table.references[0].neutron_role.as_str(), "role-guid");
    assert_eq!(table.references[0].neutron_data, "data-guid");
}

#[test]
fn malformed_redirections_shapes_are_not_admitted_as_leaf_tables() {
    let cases: [&[u8]; 5] = [
        br#"{"name":"RedirectionsStream","schema-version":0,"designs":[{"file-version":1,"targetFileName":"root.f3d","displayName":"root","lineageUrn":"urn:l","versionUrn":"urn:v"}],"references":null}"#,
        br#"{"name":"RedirectionsStream","schema-version":0,"designs":[{"file-version":1,"targetFileName":"root.f3d","displayName":"root","lineageUrn":"urn:l","versionUrn":"urn:v"}],"references":{"unexpected":1}}"#,
        br#"{"name":"RedirectionsStream","schema-version":0,"designs":[{"file-version":1,"targetFileName":"root.f3d","displayName":"root","lineageUrn":"urn:l","versionUrn":"urn:v"}],"references":[{}]}"#,
        br#"{"name":"RedirectionsStream","schema-version":0,"designs":[{"file-version":1,"targetFileName":"root.f3d","displayName":"root","lineageUrn":"urn:l","versionUrn":"urn:v"}],"references":[{"from":"root.f3d","relativePath":"part.f3d","type":"XREF","properties":[]}]}"#,
        br#"{"name":"RedirectionsStream","schema-version":0,"designs":[{"file-version":1,"targetFileName":"root.f3d","displayName":"root","lineageUrn":"urn:l","versionUrn":"urn:v"}],"references":[{"from":"root.f3d","relativePath":"part.f3d","type":"XREF","properties":[{"neutronRole":{"value":"role","dataType":"NUMBER"}},{"neutronData":{"value":"data","dataType":"STRING"}}]}]}"#,
    ];
    for bytes in cases {
        assert!(
            super::parse(&cadmpeg_test_support::service_decode_context(), bytes).is_err(),
            "malformed table admitted: {bytes:?}"
        );
    }
}

#[test]
fn malformed_redirections_are_reported_by_complete_and_container_routes() {
    let redirections = br#"{"name":"RedirectionsStream","schema-version":0,"designs":[{"file-version":1,"targetFileName":"root.f3d","displayName":"root","lineageUrn":"urn:l","versionUrn":"urn:v"}],"references":[{"from":"root.f3d","relativePath":"part.f3d","type":"XREF","properties":[]}] }"#;
    for options in [
        DecodeOptions::default(),
        DecodeOptions {
            depth: cadmpeg_core::decode::DecodeDepth::Container,
            ..DecodeOptions::default()
        },
    ] {
        let decoded = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_redirections_json("assembly-design", redirections)),
                &options,
            )
            .expect("malformed Redirections remains a reported decode");
        let loss = decoded
            .report()
            .losses
            .iter()
            .find(|loss| loss.code == F3dLossCode::XrefTableUndecoded.kind())
            .expect("malformed Redirections loss");
        assert!(loss.message.contains("RedirectionsStream.dat"), "{loss:?}");
        assert!(loss.message.contains("properties"), "{loss:?}");
    }
}

#[test]
fn redirections_property_keys_are_not_collapsed_before_admission() {
    const ROLE: &str = "aaaabbbb-cccc-dddd-eeee-ffff00001111";
    let envelope = redirections_json("root.f3d", &[("part.f3d", ROLE)]);
    let (prefix, _) = envelope.split_once(",\"references\":").unwrap();
    let role = format!(r#""neutronRole":{{"value":"{ROLE}","dataType":"STRING"}}"#);
    let data = r#""neutronData":{"value":"independent","dataType":"STRING"}"#;
    let empty_data = r#""neutronData":{"value":"","dataType":"STRING"}"#;
    for (label, properties, valid) in [
        ("role then data", format!("[{{{role}}},{{{data}}}]"), true),
        ("data then role", format!("[{{{data}}},{{{role}}}]"), true),
        ("empty independent data", format!("[{{{role}}},{{{empty_data}}}]"), true),
        ("repeated role key", format!("[{{{role},{role}}},{{{data}}}]"), false),
        ("repeated data key", format!("[{{{role}}},{{{data},{data}}}]"), false),
        ("two different keys", format!("[{{{role},{data}}}]"), false),
        ("repeated role item", format!("[{{{role}}},{{{role}}},{{{data}}}]"), false),
        ("empty property object", format!("[{{}},{{{role}}},{{{data}}}]"), false),
        ("missing role", format!("[{{{data}}}]"), false),
        ("unknown property", format!("[{{\"extension\":{{\"value\":\"x\",\"dataType\":\"STRING\"}}}},{{{role}}},{{{data}}}]"), false),
        ("duplicate type", format!("[{{{role}}},{{\"neutronData\":{{\"value\":\"independent\",\"dataType\":\"STRING\",\"dataType\":\"STRING\"}}}}]"), false),
        ("wrong type", format!("[{{{role}}},{{\"neutronData\":{{\"value\":\"independent\",\"dataType\":\"NUMBER\"}}}}]"), false),
    ] {
        let json = format!(r#"{prefix},"references":[{{"from":"root.f3d","relativePath":"part.f3d","type":"XREF","properties":{properties}}}]}}"#);
        assert_eq!(super::parse(&cadmpeg_test_support::service_decode_context(), json.as_bytes()).is_ok(), valid, "{label}");
        let document = f3d_with_redirections_json("assembly-design", json.as_bytes());
        for depth in [cadmpeg_core::decode::DecodeDepth::Full, cadmpeg_core::decode::DecodeDepth::Container] {
            let container_only = depth == cadmpeg_core::decode::DecodeDepth::Container;
            let decoded = EditableDecodeResult::from(F3dCodec.decode(
                &mut Cursor::new(&document),
                &DecodeOptions { depth, ..DecodeOptions::default() },
            ).expect("property refusal preserves the source document"));
            let losses = decoded.report().losses.iter()
                .filter(|loss| loss.code == F3dLossCode::XrefTableUndecoded.kind())
                .collect::<Vec<_>>();
            assert_eq!(losses.len(), usize::from(!valid), "{label}, container_only={container_only}: {losses:?}");
            for loss in losses {
                assert!(loss.message.contains("RedirectionsStream.dat"), "{loss:?}");
            }
            if !container_only {
                let native = f3d_native(decoded.ir());
                assert_eq!(native.xref_references.len(), usize::from(valid), "{label}");
                if valid {
                    assert_eq!(native.xref_references[0].neutron_role.as_str(), ROLE);
                    assert_eq!(native.xref_references[0].neutron_data,
                        if label == "empty independent data" { "" } else { "independent" });
                }
            }
            assert_eq!(decoded.source_fidelity()
                .retained_record(crate::ids::FILE_SOURCE_IMAGE_ID)
                .and_then(|record| record.data()), Some(document.as_slice()), "{label}");
            let _: cadmpeg_ir::CadIr = serde_json::from_slice(
                &serde_json::to_vec(decoded.ir()).unwrap()
            ).expect("complete CADIR admission");
        }
    }
}

#[test]
fn external_reference_placements_project_as_root_occurrences_in_millimetres() {
    let transform = [
        [0.0, -1.0, 0.0, 1.0],
        [1.0, 0.0, 0.0, 2.0],
        [0.0, 0.0, 1.0, 3.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let table = super::XrefTable {
        designs: Vec::new(),
        references: vec![crate::records::xref::XrefReference {
            current_pose: None,
            operation_pose: None,
            contained_poses: Vec::new(),
            owned_poses: Vec::new(),
            id: "f3d:xref:reference#0-occurrence-0".into(),
            ordinal: 0,
            occurrence_ordinal: 0,
            placement_source: None,
            from: "root.f3d".to_owned().try_into().unwrap(),
            relative_path: "part.f3d".to_owned().try_into().unwrap(),
            neutron_role: "role".to_owned().try_into().unwrap(),
            neutron_data: "data".into(),
            transform: Some(transform.try_into().unwrap()),
        }],
        placement_failures: Vec::new(),
        placement_overrides: Vec::new(),
        placement_metadata_missing: false,
        ..Default::default()
    };

    let occurrences = super::root_occurrences(&table).unwrap();

    assert_eq!(occurrences.len(), 1);
    assert_eq!(occurrences[0].id.as_str(), "f3d:model:occurrence#xref-0-0");
    assert_eq!(
        occurrences[0].transform.rows(),
        [
            [0.0, -1.0, 0.0, 10.0],
            [1.0, 0.0, 0.0, 20.0],
            [0.0, 0.0, 1.0, 30.0],
            [0.0, 0.0, 0.0, 1.0],
        ]
    );
    assert_eq!(
        occurrences[0].parent,
        cadmpeg_ir::products::OccurrenceParent::Root {}
    );
    assert_eq!(
        occurrences[0].prototype,
        cadmpeg_ir::products::PrototypeReference::External {
            document: cadmpeg_ir::products::ExternalDocument::path("part.f3d"),
            object: None,
        }
    );
}

#[test]
fn external_reference_admission_and_projection_check_affine_transforms() {
    let mut table = super::parse(&cadmpeg_test_support::service_decode_context(),
        br#"{"name":"RedirectionsStream","schema-version":0,"designs":[{"file-version":1,"targetFileName":"root.f3d","displayName":"root","lineageUrn":"urn:l","versionUrn":"urn:v"}],"references":[{"from":"root.f3d","relativePath":"part.f3d","type":"XREF","properties":[{"neutronRole":{"value":"role","dataType":"STRING"}},{"neutronData":{"value":"data","dataType":"STRING"}}]}]}"#,
    ).unwrap();
    let mut rows = cadmpeg_ir::transform::Transform::identity().rows();
    rows[0][3] = 1.0;
    table.references[0].transform = Some(rows.try_into().unwrap());
    let wire = serde_json::to_value(&table.references[0]).unwrap();
    assert_eq!(
        serde_json::from_value::<crate::records::xref::XrefReference>(wire.clone()).unwrap(),
        table.references[0]
    );
    let mut invalid = wire;
    invalid["transform"][3][0] = serde_json::json!(1.0);
    assert!(serde_json::from_value::<crate::records::xref::XrefReference>(invalid).is_err());
    let reflected = serde_json::json!({
        "id": "f3d:xref:reference#0",
        "ordinal": 0,
        "occurrence_ordinal": 0,
        "from": "root.f3d",
        "relative_path": "part.f3d",
        "neutron_role": "role",
        "neutron_data": "data",
        "transform": [
            [-1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0]
        ]
    });
    assert!(serde_json::from_value::<crate::records::xref::XrefReference>(reflected).is_err());
    rows[0][3] = f64::MAX;
    table.references[0].transform = Some(rows.try_into().unwrap());
    assert!(matches!(
        super::root_occurrences(&table),
        Err(cadmpeg_core::CodecError::NotImplemented(message))
            if message.contains("finite affine transform")
    ));
}

#[test]
fn reflected_matrix_is_not_a_placement() {
    let reflection = [
        [-1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let bytes = occurrence_record("reflection-role", 10, &[1], Some(reflection));
    let placements = super::occurrence_placements(&bytes, &super::indexed_records(&bytes), None);
    assert!(placements.is_empty(), "reflected placement admitted");
}

#[test]
fn reflected_matrix_is_reported_by_complete_document_admission() {
    let role = "aaaabbbb-cccc-dddd-eeee-ffff00001111";
    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(document_with_modern_placement(
                role,
                [
                    [-1.0, 0.0, 0.0, 0.0],
                    [0.0, 1.0, 0.0, 0.0],
                    [0.0, 0.0, 1.0, 0.0],
                    [0.0, 0.0, 0.0, 1.0],
                ],
            )),
            &DecodeOptions::default(),
        )
        .expect("reflected placement is reported");
    let native = f3d_native(decoded.ir());
    assert_eq!(native.xref_references.len(), 1);
    assert!(native.xref_references[0].transform.is_none());
    assert!(decoded
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == F3dLossCode::XrefPlacementUndecoded.kind()));
}

#[test]
fn component_reference_data_is_an_open_json_object() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let value = super::parse_component_reference_data(
        &ctx,
        br#"{"schema":7,"references":[{"id":"component"}],"extension":{"x":true}}"#,
    )
    .expect("open component-reference object");
    assert_eq!(value["schema"], 7);
    assert!(super::parse_component_reference_data(&ctx, br"[]").is_err());
    assert!(super::parse_component_reference_data(&ctx, b"not-json").is_err());
}

/// One occurrence-placement record: a target path whose last element
/// carries `role` as its cross-document link name, the identity marker,
/// and the three closing reference runs.
fn occurrence_record(
    role: &str,
    entity_id: u64,
    discriminators: &[u32],
    transform: Option<[[f64; 4]; 4]>,
) -> Vec<u8> {
    occurrence_record_with_serializer_magic(role, entity_id, discriminators, transform, None)
}

fn occurrence_record_with_serializer_magic(
    role: &str,
    entity_id: u64,
    discriminators: &[u32],
    transform: Option<[[f64; 4]; 4]>,
    serializer_magic: Option<u32>,
) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3_u32.to_le_bytes());
    bytes.extend_from_slice(b"380");
    bytes.extend_from_slice(&entity_id.to_le_bytes());
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(
        &(u32::try_from(discriminators.len()).expect("fixture value fits u32")).to_le_bytes(),
    );
    for (ordinal, discriminator) in discriminators.iter().enumerate() {
        let target = 100 + u64_from_index(ordinal);
        if ordinal + 1 == discriminators.len() {
            bytes.extend(cross_document_reference(target, role));
        } else {
            bytes.extend(local_reference(target));
        }
        bytes.extend_from_slice(&discriminator.to_le_bytes());
    }
    bytes.push(0);
    match transform {
        Some(transform) => {
            bytes.push(0);
            for value in transform.into_iter().flatten() {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
        None => bytes.push(1),
    }
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend(local_reference(7));
    if serializer_magic == Some(crate::metastream::MODERN_SERIALIZER_MAGIC) {
        bytes.push(2);
        bytes.extend_from_slice(&1_u32.to_le_bytes());
        bytes.extend_from_slice(&42_u32.to_le_bytes());
        bytes.extend(local_reference(8));
    }
    bytes.extend(local_reference(3));
    bytes.extend(local_reference(6));
    bytes
}

fn document_with_modern_placement(role: &str, matrix: [[f64; 4]; 4]) -> Vec<u8> {
    let placement = occurrence_record_with_serializer_magic(
        role,
        10,
        &[1],
        Some(matrix),
        Some(crate::metastream::MODERN_SERIALIZER_MAGIC),
    );
    document_with_placement(role, placement)
}

fn document_with_placement(role: &str, mut placement: Vec<u8>) -> Vec<u8> {
    let records = super::indexed_records(&placement);
    let entities: Vec<_> = records
        .iter()
        .map(|record| cadmpeg_core::decode::View::u64_le_at(&placement, record.offset + 7).unwrap())
        .collect();
    let offsets: Vec<_> = records
        .iter()
        .zip(&entities)
        .map(|(record, entity)| (*entity, u64::try_from(record.offset).unwrap()))
        .collect();
    for record in &records {
        placement[record.offset + 4..record.offset + 7].copy_from_slice(b"256");
    }
    let properties =
        br#"{"docstruct":{"version":"1.0.0","type":"assembly-design","subtype":"synthetic","attributes":{}}}"#;
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    write_synthetic_manifests(&mut zip, stored);
    zip.start_file("Properties.dat", stored).unwrap();
    zip.write_all(
        &(u32::try_from(properties.len()).expect("fixture value fits u32")).to_le_bytes(),
    )
    .unwrap();
    zip.write_all(properties).unwrap();
    zip.start_file("RedirectionsStream.dat", stored).unwrap();
    zip.write_all(redirections_json("root.f3d", &[("part.f3d", role)]).as_bytes())
        .unwrap();
    zip.start_file("FusionAssetName[Active]/Design1/MetaStream.dat", stored)
        .unwrap();
    zip.write_all(&design_metastream_with_records(
        &[(
            super::OCCURRENCE_PLACEMENT_TYPE_GUID,
            "",
            2,
            "Component",
            &entities,
        )],
        &offsets,
    ))
    .unwrap();
    zip.start_file("FusionAssetName[Active]/Design1/BulkStream.dat", stored)
        .unwrap();
    zip.write_all(&placement).unwrap();
    zip.finish().unwrap().into_inner()
}

fn repeated_target_occurrence_record(
    role: &str,
    entity_id: u64,
    envelope_discriminator: u32,
    transform: Option<[[f64; 4]; 4]>,
) -> Vec<u8> {
    repeated_target_occurrence_record_with_path_role(
        role,
        role,
        entity_id,
        envelope_discriminator,
        transform,
    )
}

fn repeated_target_occurrence_record_with_path_role(
    path_role: &str,
    role: &str,
    entity_id: u64,
    envelope_discriminator: u32,
    transform: Option<[[f64; 4]; 4]>,
) -> Vec<u8> {
    let metadata_guid_a = "66666666-7777-8888-9999-aaaaaaaaaaaa";
    let metadata_guid_b = "bbbbbbbb-cccc-dddd-eeee-ffffffffffff";
    let mut bytes = occurrence_record(path_role, entity_id, &[1], None);
    let path_end = super::occurrence_path(&bytes).unwrap().1;
    bytes.truncate(path_end);
    bytes.extend_from_slice(&envelope_discriminator.to_le_bytes());
    bytes.extend(
        crate::bytes::lp_utf16_bytes(metadata_guid_a)
            .expect("fixture UTF-16 code-unit count fits u32"),
    );
    bytes.extend(
        crate::bytes::lp_utf16_bytes(metadata_guid_b)
            .expect("fixture UTF-16 code-unit count fits u32"),
    );
    bytes.push(0);
    bytes.extend(cross_document_reference(100, path_role));
    match transform {
        Some(transform) => {
            bytes.push(0);
            for value in transform.into_iter().flatten() {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
        None => bytes.push(1),
    }
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.extend(
        crate::bytes::lp_utf16_bytes(role).expect("fixture UTF-16 code-unit count fits u32"),
    );
    bytes.push(0);
    bytes.extend(local_reference(3));
    bytes
}

#[test]
fn repeated_target_placements_decode_identity_and_matrix_forms() {
    let matrix = [
        [0.0, -1.0, 0.0, 2.0],
        [1.0, 0.0, 0.0, 3.0],
        [0.0, 0.0, 1.0, 4.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let role = "aaaabbbb-cccc-dddd-eeee-ffff00001111";
    let identity_1 = repeated_target_occurrence_record(role, 10, 1, None);
    let identity_2 = repeated_target_occurrence_record(role, 11, 2, None);
    let identity_3 = repeated_target_occurrence_record(role, 12, 3, None);
    let matrix_4 = repeated_target_occurrence_record(role, 13, 4, Some(matrix));
    let matrix_5 = repeated_target_occurrence_record(role, 14, 5, Some(matrix));
    let matrix_6 = repeated_target_occurrence_record(role, 15, 6, Some(matrix));
    assert_eq!(identity_1.len(), 695);
    assert_eq!(identity_2.len(), 695);
    assert_eq!(identity_3.len(), 695);
    assert_eq!(matrix_4.len(), 823);
    assert_eq!(matrix_5.len(), 823);
    assert_eq!(matrix_6.len(), 823);
    let (matrix_role, _, matrix_offset) =
        super::repeated_target_component_insert(&matrix_6, 0, matrix_6.len(), 15, matrix)
            .unwrap()
            .expect("matrix carrier matching the scope transform");
    assert_eq!(matrix_role, role);
    assert!(matrix_offset.is_some());
    let mut bytes = identity_1;
    bytes.extend(identity_2);
    bytes.extend(identity_3);
    bytes.extend(matrix_4);
    bytes.extend(matrix_5);
    bytes.extend(matrix_6);

    let placements = super::occurrence_placements(&bytes, &super::indexed_records(&bytes), None);

    assert_eq!(placements.len(), 6);
    assert!(placements.iter().all(|placement| {
        placement
            .occurrence_guid
            .as_ref()
            .map(crate::records::mesh::DesignRelaxedGuidText::as_str)
            == Some("bbbbbbbb-cccc-dddd-eeee-ffffffffffff")
    }));
    assert_eq!(
        super::occurrence_transforms(&placements, role),
        vec![None, None, None, Some(matrix), Some(matrix), Some(matrix)]
    );

    let uri_role = "urn:example:component";
    let uri_carrier = repeated_target_occurrence_record(uri_role, 18, 27, Some(matrix));
    let details = super::repeated_target_occurrence_placement_details(&uri_carrier)
        .unwrap()
        .unwrap();
    assert_eq!(details.role, uri_role);
    assert_eq!(details.transform.unwrap().1, matrix);

    let retained_role = format!("{role}_urn:example:component");
    let local_carrier = repeated_target_occurrence_record(&retained_role, 16, 1, None);
    let (decoded_role, role_offset, transform_offset) = super::repeated_target_component_insert(
        &local_carrier,
        0,
        local_carrier.len(),
        16,
        cadmpeg_ir::transform::Transform::identity().rows(),
    )
    .unwrap()
    .expect("identity carrier");
    assert_eq!(decoded_role, retained_role);
    assert_eq!(transform_offset, None);
    let encoded_role = crate::bytes::lp_utf16_bytes(&retained_role).unwrap();
    assert_eq!(
        &local_carrier[role_offset - 4..role_offset - 4 + encoded_role.len()],
        encoded_role
    );
    let independent_role =
        repeated_target_occurrence_record_with_path_role(role, &retained_role, 16, 1, None);
    assert!(
        super::repeated_target_occurrence_placement_details(&independent_role)
            .unwrap()
            .is_none()
    );
}

#[test]
fn repeated_target_frames_accept_both_reference_and_separator_forms() {
    let role = "aaaabbbb-cccc-dddd-eeee-ffff00001111";
    for inline in [false, true] {
        for separator in [false, true] {
            let mut reference = vec![1];
            reference.extend_from_slice(&71_u64.to_le_bytes());
            if inline {
                reference.extend_from_slice(&36_u32.to_le_bytes());
                reference.extend_from_slice(b"aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee");
            }
            reference.push(1);
            reference.extend_from_slice(&5_u32.to_le_bytes());
            lp_utf16(&mut reference, "11111111-2222-3333-4444-555555555555");
            reference.push(0);
            if !inline {
                reference.extend_from_slice(&36_u32.to_le_bytes());
                reference.extend_from_slice(b"aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee");
            }
            lp_utf16(&mut reference, role);
            reference.push(1);
            lp_utf16(&mut reference, "22222222-3333-4444-5555-666666666666");
            lp_utf16(&mut reference, "urn:example:version=9");
            let mut body = repeated_target_occurrence_record(role, 10, 27, None);
            let (_, path_end) = super::occurrence_path(&body).unwrap();
            let repeated_start = path_end + 4 + 2 * 76 + 1;
            let mut repeated_end = repeated_start;
            super::take_reference(&body, &mut repeated_end).unwrap();
            body.splice(
                repeated_start - usize::from(!separator)..repeated_end,
                reference.clone(),
            );
            let path_start = 24;
            let mut path_reference_end = path_start;
            super::take_reference(&body, &mut path_reference_end).unwrap();
            body.splice(path_start..path_reference_end, reference);
            let owner_start = body.len() - 11;
            let mut owner = vec![1];
            owner.extend_from_slice(&19_u64.to_le_bytes());
            owner.extend_from_slice(&36_u32.to_le_bytes());
            owner.extend_from_slice(b"bbbbbbbb-cccc-dddd-eeee-ffffffffffff");
            owner.extend_from_slice(&[0, 0]);
            body.splice(owner_start.., owner);
            let placement = super::repeated_target_occurrence_placement(&body)
                .unwrap()
                .unwrap();
            assert_eq!(placement.carrier_record_index, 10);
            assert_eq!(
                placement.owner,
                Some(crate::records::xref::XrefLocalOwner {
                    target: 19,
                    type_guid: Some("bbbbbbbb-cccc-dddd-eeee-ffffffffffff".into()),
                })
            );
            assert_eq!(placement.link_names, vec![role]);
            assert_eq!(placement.transform, None);
            let details = super::repeated_target_occurrence_placement_details(&body)
                .unwrap()
                .unwrap();
            assert_eq!(
                &body[details.role_offset..details.role_offset + 2],
                &[b'a', 0]
            );
        }
    }
}

#[test]
fn local_occurrence_paths_belong_to_the_local_component_decoder() {
    let (bulk, _) = crate::test_support::streams_test::nested_local_component_design(&[300]);
    let records = super::indexed_records(&bulk);
    let local_records: Vec<_> = records
        .into_iter()
        .filter(|record| {
            super::occurrence_path_elements(&bulk[record.offset..record.end])
                .is_some_and(|(names, _, _)| names.is_empty())
        })
        .collect();
    assert!(!local_records.is_empty());
    let (placements, failures) =
        super::occurrence_placements_with_failures(&bulk, &local_records, None, None).unwrap();
    assert!(placements.is_empty());
    assert!(failures.is_empty());
    let mut root = Vec::new();
    crate::test_support::indexed_header(&mut root, *b"256", 31);
    root.extend_from_slice(&0_u32.to_le_bytes());
    crate::test_support::lp_ascii(&mut root, "rootInstance");
    root.push(1);
    root.extend_from_slice(&0_u32.to_le_bytes());
    root.push(0);
    let records = [super::IndexedRecord {
        offset: 0,
        end: root.len(),
    }];
    let (placements, failures) =
        super::occurrence_placements_with_failures(&root, &records, None, None).unwrap();
    assert!(placements.is_empty());
    assert!(failures.is_empty());
}

#[test]
fn malformed_repeated_frames_keep_losses_beside_valid_placements() {
    let role = "aaaabbbb-cccc-dddd-eeee-ffff00001111";
    let valid = repeated_target_occurrence_record(role, 10, 27, None);
    let (_, path_end) = super::occurrence_path(&valid).unwrap();
    let repeated_start = path_end + 4 + 2 * 76 + 1;
    let mut repeated_end = repeated_start;
    super::take_reference(&valid, &mut repeated_end).unwrap();
    let mut malformed = Vec::new();
    for offset in [
        path_end,
        repeated_start + 1,
        repeated_start + 10,
        repeated_start + 19,
        repeated_start + 95,
        super::reference_role_offset(&valid, repeated_start).unwrap(),
        repeated_end,
        repeated_end + 1,
        repeated_end + 9,
        valid.len() - 11,
    ] {
        let mut body = valid.clone();
        body[offset] = if offset == path_end { 0 } else { 2 };
        malformed.push(body);
    }
    let mut version_mismatch = valid.clone();
    version_mismatch[repeated_end - 1] = 1;
    let mut version = Vec::new();
    lp_utf16(&mut version, "22222222-3333-4444-5555-666666666666");
    lp_utf16(&mut version, "urn:example:version=9");
    version_mismatch.splice(repeated_end..repeated_end, version);
    malformed.push(version_mismatch);
    let mut doubled_separator = valid.clone();
    doubled_separator.insert(repeated_start, 0);
    malformed.push(doubled_separator);
    let mut cross_segment_owner = valid.clone();
    *cross_segment_owner.last_mut().unwrap() = 1;
    cross_segment_owner.extend_from_slice(&5_u32.to_le_bytes());
    malformed.push(cross_segment_owner);
    let mut reflected = cadmpeg_ir::transform::Transform::identity().rows();
    reflected[0][0] = -1.0;
    malformed.push(repeated_target_occurrence_record(
        role,
        10,
        27,
        Some(reflected),
    ));
    let mut extra = valid.clone();
    extra.push(0);
    malformed.push(extra);
    for end in [24, repeated_start, repeated_end, valid.len() - 1] {
        malformed.push(valid[..end].to_vec());
    }
    let mut expected_frame = valid.clone();
    expected_frame[4..7].copy_from_slice(b"256");
    for mut body in malformed {
        body[7..15].copy_from_slice(&11_u64.to_le_bytes());
        let mut records = valid.clone();
        records.extend(body);
        let document = document_with_placement(role, records);
        let decoded = F3dCodec
            .decode(&mut Cursor::new(document), &DecodeOptions::default())
            .unwrap();
        let native = f3d_native(decoded.ir());
        assert_eq!(native.xref_references.len(), 1);
        assert_eq!(
            native.xref_references[0]
                .placement_source
                .as_ref()
                .unwrap()
                .frame_bytes,
            expected_frame
        );
        assert_eq!(
            native.xref_references[0]
                .placement_source
                .as_ref()
                .unwrap()
                .carrier_record_index,
            10
        );
        let losses: Vec<_> = decoded
            .report()
            .losses
            .iter()
            .filter(|loss| loss.code == F3dLossCode::XrefPlacementUndecoded.kind())
            .collect();
        assert_eq!(losses.len(), 1);
        assert!(losses[0].message.contains("11"));
    }
}

fn grouped_identity_carrier(role: &str, record_index: u32) -> Vec<u8> {
    let component_guid = "11111111-2222-3333-4444-555555555555";
    let type_guid = "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee";
    let metadata_guid_a = "66666666-7777-8888-9999-aaaaaaaaaaaa";
    let metadata_guid_b = "bbbbbbbb-cccc-dddd-eeee-ffffffffffff";
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3_u32.to_le_bytes());
    bytes.extend_from_slice(b"382");
    bytes.extend_from_slice(&record_index.to_le_bytes());
    bytes.extend_from_slice(&[0; 8]);
    bytes.push(1);
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&3_u64.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&[0; 4]);
    bytes.extend(
        crate::bytes::lp_utf16_bytes(component_guid)
            .expect("fixture UTF-16 code-unit count fits u32"),
    );
    bytes.push(0);
    bytes.extend_from_slice(&36_u32.to_le_bytes());
    bytes.extend_from_slice(type_guid.as_bytes());
    bytes.extend(
        crate::bytes::lp_utf16_bytes(role).expect("fixture UTF-16 code-unit count fits u32"),
    );
    bytes.extend_from_slice(&[0, 1, 0, 0, 0, 0, 1, 0, 0, 0]);
    bytes.extend(
        crate::bytes::lp_utf16_bytes(metadata_guid_a)
            .expect("fixture UTF-16 code-unit count fits u32"),
    );
    bytes.extend(
        crate::bytes::lp_utf16_bytes(metadata_guid_b)
            .expect("fixture UTF-16 code-unit count fits u32"),
    );
    bytes.extend_from_slice(&[0, 1, 3, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0]);
    bytes.extend(
        crate::bytes::lp_utf16_bytes(component_guid)
            .expect("fixture UTF-16 code-unit count fits u32"),
    );
    bytes.push(0);
    bytes.extend_from_slice(&36_u32.to_le_bytes());
    bytes.extend_from_slice(type_guid.as_bytes());
    bytes.extend(
        crate::bytes::lp_utf16_bytes(role).expect("fixture UTF-16 code-unit count fits u32"),
    );
    bytes.extend_from_slice(&[0, 1, 0, 0, 0, 0]);
    bytes.extend(
        crate::bytes::lp_utf16_bytes(role).expect("fixture UTF-16 code-unit count fits u32"),
    );
    bytes.extend_from_slice(&[0, 1, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    assert_eq!(bytes.len(), 695);
    bytes
}

#[test]
fn grouped_identity_carriers_decode_as_identity_placements() {
    let role = "cccccccc-dddd-eeee-ffff-000000000000";
    let bytes = grouped_identity_carrier(role, 10);
    let placements = super::occurrence_placements(&bytes, &super::indexed_records(&bytes), None);

    assert_eq!(
        placements,
        vec![OccurrencePlacement {
            frame_bytes: bytes.clone(),
            occurrence_guid: Some(
                "bbbbbbbb-cccc-dddd-eeee-ffffffffffff"
                    .to_owned()
                    .try_into()
                    .unwrap()
            ),
            carrier_record_index: 10,
            owner: Some(crate::records::xref::XrefLocalOwner {
                target: 4,
                type_guid: None
            }),
            link_names: vec![role.into()],
            transform: None,
        }]
    );
}

fn legacy_occurrence_reference(target: u64, identity: u64) -> Vec<u8> {
    let mut bytes = vec![1];
    bytes.extend_from_slice(&target.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&identity.to_le_bytes());
    bytes.push(0);
    bytes.extend_from_slice(&36_u32.to_le_bytes());
    bytes.extend_from_slice(b"aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee");
    bytes.push(0);
    bytes
}

/// One legacy typed placement envelope.
fn legacy_occurrence_record(
    role: &str,
    entity_id: u64,
    transform: Option<[[f64; 4]; 4]>,
) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3_u32.to_le_bytes());
    bytes.extend_from_slice(b"380");
    bytes.extend_from_slice(&entity_id.to_le_bytes());
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend(legacy_occurrence_reference(3, 0x0102_0304_0506_0708));
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.push(0);
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend(
        crate::bytes::lp_utf16_bytes("11111111-2222-3333-4444-555555555555")
            .expect("fixture UTF-16 code-unit count fits u32"),
    );
    bytes.extend(
        crate::bytes::lp_utf16_bytes("66666666-7777-8888-9999-aaaaaaaaaaaa")
            .expect("fixture UTF-16 code-unit count fits u32"),
    );
    bytes.push(0);
    bytes.extend(legacy_occurrence_reference(3, 0x0102_0304_0506_0708));
    match transform {
        Some(transform) => {
            bytes.push(0);
            for value in transform.into_iter().flatten() {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
        None => bytes.push(1),
    }
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.extend(
        crate::bytes::lp_utf16_bytes(role).expect("fixture UTF-16 code-unit count fits u32"),
    );
    bytes.extend_from_slice(&[0, 1, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    bytes
}

#[test]
fn legacy_typed_placements_decode_identity_and_matrix_forms() {
    let matrix = [
        [0.0, -1.0, 0.0, 2.0],
        [1.0, 0.0, 0.0, 3.0],
        [0.0, 0.0, 1.0, 4.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let role = "aaaabbbb-cccc-dddd-eeee-ffff00001111";
    let identity = legacy_occurrence_record(role, 10, None);
    let matrix_record = legacy_occurrence_record(role, 11, Some(matrix));
    assert_eq!(identity.len(), 403);
    assert_eq!(matrix_record.len(), 531);
    let mut bytes = identity;
    bytes.extend(matrix_record);
    let placements = super::occurrence_placements(&bytes, &super::indexed_records(&bytes), None);

    assert_eq!(placements.len(), 2);
    assert_eq!(
        super::occurrence_transforms(&placements, role),
        vec![None, Some(matrix)]
    );
}

#[test]
fn attributed_legacy_external_placements_keep_the_transform_owner_and_source() {
    use crate::test_support::legacy_attributed_external_placement;

    let role = "aaaabbbb-cccc-dddd-eeee-ffff00001111";
    let matrix = [
        [0.0, -1.0, 0.0, 2.0],
        [1.0, 0.0, 0.0, 3.0],
        [0.0, 0.0, 1.0, 4.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    for (attributes, transform, length) in [
        (false, None, 420),
        (false, Some(matrix), 548),
        (true, None, 552),
        (true, Some(matrix), 680),
    ] {
        let frame = legacy_attributed_external_placement(role, attributes, transform);
        assert_eq!(frame.len(), length);
        let document = document_with_placement(role, frame.clone());
        let decoded = F3dCodec
            .decode(&mut Cursor::new(document), &DecodeOptions::default())
            .unwrap();
        let native = f3d_native(decoded.ir());
        let reference = &native.xref_references[0];
        assert_eq!(
            reference
                .transform
                .map(crate::records::xref::XrefPlacementTransform::rows),
            transform
        );
        let source = reference.placement_source.as_ref().unwrap();
        assert_eq!(source.carrier_record_index, 10);
        assert_eq!(source.owner.as_ref().unwrap().target, 7);
        assert_eq!(source.owner.as_ref().unwrap().type_guid, None);
        assert_eq!(
            source
                .occurrence_guid
                .as_ref()
                .map(crate::records::mesh::DesignRelaxedGuidText::as_str),
            Some("66666666-7777-8888-9999-aaaaaaaaaaaa")
        );
        let mut expected_frame = frame;
        expected_frame[4..7].copy_from_slice(b"256");
        assert_eq!(source.frame_bytes, expected_frame);
        assert!(super::placement_source_matches(reference).unwrap());
        let occurrence = decoded
            .ir()
            .model
            .occurrences
            .iter()
            .find(|occurrence| occurrence.native_ref.as_deref() == Some(reference.id.as_str()))
            .unwrap();
        let expected = if transform.is_some() {
            [
                [0.0, -1.0, 0.0, 20.0],
                [1.0, 0.0, 0.0, 30.0],
                [0.0, 0.0, 1.0, 40.0],
            ]
        } else {
            [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
            ]
        };
        assert_eq!(occurrence.transform.affine_rows(), expected);
        let losses = &decoded.report().losses;
        assert!(!losses
            .iter()
            .any(|loss| loss.code == F3dLossCode::XrefPlacementUndecoded.kind()));
        assert_eq!(
            losses
                .iter()
                .filter(|loss| loss.code == F3dLossCode::XrefPlacementMetadataNative.kind())
                .count(),
            1
        );
        assert!(losses
            .iter()
            .any(|loss| loss.code == F3dLossCode::XrefOwnerUnresolved.kind()));
    }
}

#[test]
fn attributed_legacy_placement_without_a_container_role_reports_its_loss() {
    let role = "aaaabbbb-cccc-dddd-eeee-ffff00001111";
    let other_role = "bbbbbbbb-cccc-dddd-eeee-ffffffffffff";
    let frame = crate::test_support::legacy_attributed_external_placement(other_role, true, None);
    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(document_with_placement(role, frame)),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert!(f3d_native(decoded.ir()).xref_references[0]
        .placement_source
        .is_none());
    let losses = &decoded.report().losses;
    let unbound = losses
        .iter()
        .filter(|loss| loss.code == F3dLossCode::XrefPlacementRoleUnbound.kind())
        .collect::<Vec<_>>();
    assert_eq!(unbound.len(), 1);
    assert!(unbound[0].message.contains(other_role));
    assert!(!losses
        .iter()
        .any(|loss| loss.code == F3dLossCode::XrefPlacementUndecoded.kind()));
    assert!(!losses
        .iter()
        .any(|loss| loss.code == F3dLossCode::XrefPlacementMetadataNative.kind()));
}

#[test]
fn refused_attributed_legacy_placements_keep_a_placement_loss() {
    use crate::layout::legacy_attributed_external_placement_attributes as attributes;
    use crate::layout::legacy_attributed_external_placement_members as members;
    use crate::layout::legacy_attributed_external_placement_prefix as prefix;
    use crate::layout::legacy_occurrence_target_reference as target;
    use crate::test_support::legacy_attributed_external_placement;

    let role = "aaaabbbb-cccc-dddd-eeee-ffff00001111";
    let matrix = cadmpeg_ir::transform::Transform::identity().rows();
    let valid = legacy_attributed_external_placement(role, true, Some(matrix));
    let members_start = prefix::LEN + attributes::LEN;
    let mut refused = Vec::new();
    for field in [target::TARGET_ENTITY, target::SOURCE_OCCURRENCE_IDENTITY] {
        let mut body = valid.clone();
        for start in [
            prefix::TARGET_REFERENCE,
            members_start + members::REPEATED_TARGET_REFERENCE,
        ] {
            body[start + field..start + field + 8].copy_from_slice(&0_u64.to_le_bytes());
        }
        refused.push(body);
    }
    let mut unequal = valid.clone();
    let repeated_entity =
        members_start + members::REPEATED_TARGET_REFERENCE + target::TARGET_ENTITY;
    unequal[repeated_entity..repeated_entity + 8].copy_from_slice(&4_u64.to_le_bytes());
    refused.push(unequal);
    for offset in [
        prefix::LEN + attributes::COUNT,
        prefix::LEN + attributes::PARENT_VALUE,
        prefix::LEN + attributes::RESOLUTION_VALUE,
        prefix::LEN + attributes::SELF_VALUE,
        members_start + members::CONTROL,
        members_start + members::ORDINAL,
        members_start + members::METADATA_GUID_LENGTH,
        members_start + members::METADATA_REFERENCE,
        members_start + members::REFERENCE_SEPARATOR,
        members_start + members::REPEATED_TARGET_REFERENCE,
        members_start + members::IDENTITY_FLAG,
    ] {
        let mut body = valid.clone();
        body[offset] = if [
            prefix::LEN + attributes::PARENT_VALUE,
            prefix::LEN + attributes::SELF_VALUE,
        ]
        .contains(&offset)
        {
            1
        } else {
            2
        };
        if offset == members_start + members::ORDINAL {
            body[offset..offset + 4].copy_from_slice(&0_u32.to_le_bytes());
        }
        refused.push(body);
    }
    let mut extra = valid.clone();
    extra.push(0);
    refused.push(extra);
    let mut truncated = valid;
    truncated.pop();
    refused.push(truncated);
    let mut nonzero_control = legacy_attributed_external_placement(role, false, Some(matrix));
    nonzero_control[prefix::LEN + super::legacy_empty_attributes::LEN + members::CONTROL] = 1;
    refused.push(nonzero_control);
    let mut reflection = matrix;
    reflection[0][0] = -1.0;
    refused.push(legacy_attributed_external_placement(
        role,
        true,
        Some(reflection),
    ));
    for body in refused {
        let decoded = F3dCodec
            .decode(
                &mut Cursor::new(document_with_placement(role, body)),
                &DecodeOptions::default(),
            )
            .unwrap();
        let native = f3d_native(decoded.ir());
        assert!(native.xref_references[0].placement_source.is_none());
        assert_eq!(
            decoded
                .report()
                .losses
                .iter()
                .filter(|loss| loss.code == F3dLossCode::XrefPlacementUndecoded.kind())
                .count(),
            1
        );
        assert!(!decoded
            .report()
            .losses
            .iter()
            .any(|loss| loss.code == F3dLossCode::XrefPlacementMetadataNative.kind()));
    }
}

#[test]
fn malformed_legacy_typed_placement_reports_its_role() {
    let role = "aaaabbbb-cccc-dddd-eeee-ffff00001111";
    let mut bytes = legacy_occurrence_record(role, 10, None);
    bytes.pop();
    let (placements, failures) = super::occurrence_placements_with_failures(
        &bytes,
        &super::indexed_records(&bytes),
        None,
        None,
    )
    .unwrap();

    assert!(placements.is_empty());
    assert_eq!(failures.len(), 1);
    assert_eq!(failures[0].link_names, vec![role]);
}

#[test]
fn occurrence_records_expand_shared_roles_and_decode_rigid_matrices() {
    let first = [
        [0.0, -1.0, 0.0, 2.0],
        [1.0, 0.0, 0.0, 3.0],
        [0.0, 0.0, 1.0, 4.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let second = [
        [1.0, 0.0, 0.0, -5.0],
        [0.0, 1.0, 0.0, 6.0],
        [0.0, 0.0, 1.0, 7.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let mut bytes = occurrence_record("role", 10, &[1], Some(first));
    bytes.extend_from_slice(&occurrence_record("role", 11, &[1, 2], Some(second)));
    let placements = super::occurrence_placements(&bytes, &super::indexed_records(&bytes), None);

    assert_eq!(
        super::occurrence_transforms(&placements, "role"),
        vec![Some(first), Some(second)]
    );
}

#[test]
fn identity_marked_placement_stores_no_matrix() {
    let mut bytes = occurrence_record("role", 10, &[1], None);
    bytes.extend_from_slice(&occurrence_record("role", 11, &[3], None));
    let placements = super::occurrence_placements(&bytes, &super::indexed_records(&bytes), None);

    assert_eq!(placements.len(), 2);
    assert_eq!(
        super::occurrence_transforms(&placements, "role"),
        vec![None, None]
    );
}

#[test]
fn malformed_role_placement_is_retained_as_a_decode_failure() {
    let mut bytes = occurrence_record("role", 10, &[1], None);
    bytes.pop();
    let records = super::indexed_records(&bytes);

    let (placements, failures) =
        super::occurrence_placements_with_failures(&bytes, &records, None, None).unwrap();

    assert!(placements.is_empty());
    assert_eq!(failures.len(), 1);
    assert_eq!(failures[0].link_names, vec!["role"]);
}

#[test]
fn tagged_placement_tail_requires_the_modern_serializer_magic() {
    let modern = occurrence_record_with_serializer_magic(
        "role",
        10,
        &[1],
        None,
        Some(crate::metastream::MODERN_SERIALIZER_MAGIC),
    );
    let records = super::indexed_records(&modern);
    assert_eq!(
        super::occurrence_placements(
            &modern,
            &records,
            Some(crate::metastream::MODERN_SERIALIZER_MAGIC)
        )
        .len(),
        1
    );
    assert!(
        super::occurrence_placements(&modern, &records, Some(999)).is_empty(),
        "a legacy MetaStream must not admit the modern tagged tail"
    );

    let legacy = occurrence_record("role", 11, &[2], None);
    let records = super::indexed_records(&legacy);
    assert_eq!(
        super::occurrence_placements(&legacy, &records, Some(999)).len(),
        1
    );
    assert!(
        super::occurrence_placements(
            &legacy,
            &records,
            Some(crate::metastream::MODERN_SERIALIZER_MAGIC)
        )
        .is_empty(),
        "the modern form requires its tagged tail"
    );
}

#[test]
fn paired_design_metastream_selects_the_tagged_placement_form() {
    let role = "aaaabbbb-cccc-dddd-eeee-ffff00001111";
    let properties =
        br#"{"docstruct":{"version":"1.0.0","type":"assembly-design","subtype":"synthetic","attributes":{}}}"#;
    let placement = occurrence_record_with_serializer_magic(
        role,
        10,
        &[1],
        Some([
            [1.0, 0.0, 0.0, 7.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ]),
        Some(crate::metastream::MODERN_SERIALIZER_MAGIC),
    );
    let mut placement = placement;
    placement[4..7].copy_from_slice(b"256");
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    write_synthetic_manifests(&mut zip, stored);
    zip.start_file("Properties.dat", stored).unwrap();
    zip.write_all(
        &(u32::try_from(properties.len()).expect("fixture value fits u32")).to_le_bytes(),
    )
    .unwrap();
    zip.write_all(properties).unwrap();
    zip.start_file("RedirectionsStream.dat", stored).unwrap();
    zip.write_all(redirections_json("root.f3d", &[("part.f3d", role)]).as_bytes())
        .unwrap();
    zip.start_file("FusionAssetName[Active]/Design1/MetaStream.dat", stored)
        .unwrap();
    zip.write_all(&design_metastream_with_records(
        &[(
            super::OCCURRENCE_PLACEMENT_TYPE_GUID,
            "",
            2,
            "Component",
            &[10],
        )],
        &[(10, 0)],
    ))
    .unwrap();
    zip.start_file("FusionAssetName[Active]/Design1/BulkStream.dat", stored)
        .unwrap();
    zip.write_all(&placement).unwrap();
    let archive = zip.finish().unwrap().into_inner();

    let decoded = F3dCodec
        .decode(&mut Cursor::new(archive), &DecodeOptions::default())
        .expect("synthetic XRef archive");
    let native = f3d_native(decoded.ir());
    assert_eq!(native.xref_references.len(), 1);
    let transform = native.xref_references[0]
        .transform
        .expect("tagged placement transform");
    assert!((transform.rows()[0][3] - 7.0).abs() < EPS_PLACEMENT_TRANSLATION);
    assert!(decoded
        .report()
        .losses
        .iter()
        .all(|loss| loss.code != F3dLossCode::XrefPlacementUndecoded.kind()));
}

#[test]
fn paired_design_metastream_selects_the_legacy_typed_placement_form() {
    let role = "aaaabbbb-cccc-dddd-eeee-ffff00001111";
    let properties =
        br#"{"docstruct":{"version":"1.0.0","type":"assembly-design","subtype":"synthetic","attributes":{}}}"#;
    let placement = legacy_occurrence_record(
        role,
        10,
        Some([
            [1.0, 0.0, 0.0, 7.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ]),
    );
    let mut placement = placement;
    placement[4..7].copy_from_slice(b"256");
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    write_synthetic_manifests(&mut zip, stored);
    zip.start_file("Properties.dat", stored).unwrap();
    zip.write_all(
        &(u32::try_from(properties.len()).expect("fixture value fits u32")).to_le_bytes(),
    )
    .unwrap();
    zip.write_all(properties).unwrap();
    zip.start_file("RedirectionsStream.dat", stored).unwrap();
    zip.write_all(redirections_json("root.f3d", &[("part.f3d", role)]).as_bytes())
        .unwrap();
    zip.start_file("FusionAssetName[Active]/Design1/MetaStream.dat", stored)
        .unwrap();
    zip.write_all(&design_metastream_with_records(
        &[(
            super::OCCURRENCE_PLACEMENT_TYPE_GUID,
            "",
            2,
            "Component",
            &[10],
        )],
        &[(10, 0)],
    ))
    .unwrap();
    zip.start_file("FusionAssetName[Active]/Design1/BulkStream.dat", stored)
        .unwrap();
    zip.write_all(&placement).unwrap();
    let archive = zip.finish().unwrap().into_inner();

    let decoded = F3dCodec
        .decode(&mut Cursor::new(archive), &DecodeOptions::default())
        .expect("synthetic legacy XRef archive");
    let native = f3d_native(decoded.ir());
    assert_eq!(native.xref_references.len(), 1);
    let transform = native.xref_references[0]
        .transform
        .expect("legacy placement transform");
    assert!((transform.rows()[0][3] - 7.0).abs() < EPS_PLACEMENT_TRANSLATION);
    assert!(decoded
        .report()
        .losses
        .iter()
        .all(|loss| loss.code != F3dLossCode::XrefPlacementUndecoded.kind()));
}

#[test]
fn malformed_typed_role_placement_reports_a_loss() {
    let role = "aaaabbbb-cccc-dddd-eeee-ffff00001111";
    let properties =
        br#"{"docstruct":{"version":"1.0.0","type":"assembly-design","subtype":"synthetic","attributes":{}}}"#;
    let mut placement = occurrence_record_with_serializer_magic(
        role,
        10,
        &[1],
        None,
        Some(crate::metastream::MODERN_SERIALIZER_MAGIC),
    );
    placement.pop();
    placement[4..7].copy_from_slice(b"256");

    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    write_synthetic_manifests(&mut zip, stored);
    zip.start_file("Properties.dat", stored).unwrap();
    zip.write_all(
        &(u32::try_from(properties.len()).expect("fixture value fits u32")).to_le_bytes(),
    )
    .unwrap();
    zip.write_all(properties).unwrap();
    zip.start_file("RedirectionsStream.dat", stored).unwrap();
    zip.write_all(redirections_json("root.f3d", &[("part.f3d", role)]).as_bytes())
        .unwrap();
    zip.start_file("FusionAssetName[Active]/Design1/MetaStream.dat", stored)
        .unwrap();
    zip.write_all(&design_metastream_with_records(
        &[(
            super::OCCURRENCE_PLACEMENT_TYPE_GUID,
            "",
            2,
            "Component",
            &[10],
        )],
        &[(10, 0)],
    ))
    .unwrap();
    zip.start_file("FusionAssetName[Active]/Design1/BulkStream.dat", stored)
        .unwrap();
    zip.write_all(&placement).unwrap();
    let archive = zip.finish().unwrap().into_inner();

    let decoded = F3dCodec
        .decode(&mut Cursor::new(archive), &DecodeOptions::default())
        .expect("synthetic malformed placement archive");
    let loss = decoded
        .report()
        .losses
        .iter()
        .find(|loss| loss.code == F3dLossCode::XrefPlacementUndecoded.kind())
        .expect("typed placement loss");
    assert!(loss.message.contains("part.f3d"));
    assert!(loss.message.contains(role));
}

#[test]
fn placement_keeps_the_link_name_of_a_multi_element_path() {
    let bytes = occurrence_record("role", 10, &[7, 4, 2], None);
    let placements = super::occurrence_placements(&bytes, &super::indexed_records(&bytes), None);

    assert_eq!(placements[0].link_names, vec!["role".to_owned()]);
}

#[test]
fn a_placement_that_does_not_close_on_the_record_end_is_not_a_placement() {
    let mut bytes = occurrence_record("role", 10, &[1], None);
    bytes.push(0);
    let records = super::indexed_records(&bytes);

    assert_eq!(
        super::occurrence_placements(&bytes, &records, None),
        Vec::new()
    );
}

#[test]
fn a_nonrigid_matrix_is_not_a_placement() {
    let mut nonrigid = [[0.0; 4]; 4];
    nonrigid[0][0] = 2.0;
    nonrigid[1][1] = 1.0;
    nonrigid[2][2] = 1.0;
    nonrigid[3][3] = 1.0;
    let bytes = occurrence_record("role", 10, &[1], Some(nonrigid));
    let records = super::indexed_records(&bytes);

    assert_eq!(
        super::occurrence_placements(&bytes, &records, None),
        Vec::new()
    );
}

#[test]
fn a_role_that_no_path_element_names_places_nothing() {
    let bytes = occurrence_record("role", 10, &[1], None);
    let placements = super::occurrence_placements(&bytes, &super::indexed_records(&bytes), None);

    assert_eq!(
        super::occurrence_transforms(&placements, "other"),
        Vec::new()
    );
}

#[test]
fn exact_component_insert_carriers_precede_structured_placements() {
    let direct = [
        [1.0, 0.0, 0.0, 7.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let structured = OccurrencePlacement {
        frame_bytes: {
            let mut frame = repeated_target_occurrence_record("role", 23, 27, None);
            let owner = local_reference(8);
            let start = frame.len() - owner.len();
            frame[start..].copy_from_slice(&owner);
            frame
        },
        occurrence_guid: Some(
            "bbbbbbbb-cccc-dddd-eeee-ffffffffffff"
                .to_owned()
                .try_into()
                .unwrap(),
        ),
        carrier_record_index: 23,
        owner: Some(crate::records::xref::XrefLocalOwner {
            target: 8,
            type_guid: None,
        }),
        link_names: vec!["role".into()],
        transform: Some([
            [1.0, 0.0, 0.0, -5.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ]),
    };

    let other = OccurrencePlacement {
        frame_bytes: Vec::new(),
        carrier_record_index: 24,
        transform: None,
        ..structured.clone()
    };
    let selected = super::occurrence_transforms_with_precedence(
        vec![(23, direct), (23, direct)],
        &[structured.clone(), other.clone()],
        "role",
    );
    assert_eq!(selected.len(), 2);
    assert_eq!(selected[0].transform, Some(direct));
    assert_eq!(selected[0].carrier_record_index, 23);
    assert_eq!(selected[0].owner, structured.owner);
    assert_eq!(selected[0].occurrence_guid, structured.occurrence_guid);
    assert_eq!(selected[0].frame_bytes, structured.frame_bytes);
    assert_eq!(selected[1], other);
    assert_eq!(
        super::superseded_placement_count(&[(23, direct)], &[structured, other], "role"),
        1
    );
    assert_eq!(
        super::occurrence_transforms_with_precedence(vec![(25, direct)], &selected, "role").len(),
        3
    );
}

#[test]
fn component_insert_selection_uses_stream_and_role_not_class_tag() {
    let selected = [
        [1.0, 0.0, 0.0, 7.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let ignored = [
        [1.0, 0.0, 0.0, 9.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let selected_construction =
        crate::records::feature::assembly_features::DesignComponentInsertConstruction {
            relation_record_index: 1,
            carrier_record_index: 2,
            occurrence_identity: None,
            neutron_role: "role".into(),
            neutron_role_offset: 0,
            placement: Some(
                crate::records::feature::assembly_features::DesignComponentInsertMatrix {
                    scope: crate::records::identity::Located {
                        value: selected.try_into().unwrap(),
                        offset: 0,
                    },
                    carrier_offset: Some(0),
                },
            ),
        };
    let ignored_construction =
        crate::records::feature::assembly_features::DesignComponentInsertConstruction {
            neutron_role: "other".into(),
            placement: Some(
                crate::records::feature::assembly_features::DesignComponentInsertMatrix {
                    scope: crate::records::identity::Located {
                        value: ignored.try_into().unwrap(),
                        offset: 0,
                    },
                    carrier_offset: Some(0),
                },
            ),
            ..selected_construction.clone()
        };

    assert_eq!(
        super::select_component_insert_transforms(
            [
                ("stream", &selected_construction),
                ("stream", &ignored_construction),
                ("other-stream", &selected_construction),
            ],
            "stream",
            "role"
        ),
        vec![(2, selected)]
    );
    let mut reference = super::parse(
        &cadmpeg_test_support::service_decode_context(),
        redirections_json("root.f3d", &[("part.f3d", "role")]).as_bytes(),
    )
    .unwrap()
    .references
    .remove(0);
    reference.placement_source = Some(crate::records::xref::XrefPlacementSource {
        frame_bytes: Vec::new().into(),
        occurrence_guid: None,
        stream: "stream".into(),
        carrier_record_index: 2,
        owner: None,
    });
    reference.transform = Some(selected.try_into().unwrap());
    let mut other = reference.clone();
    other.ordinal = 1;
    other
        .placement_source
        .as_mut()
        .unwrap()
        .carrier_record_index = 3;
    let references = [other, reference];
    assert_eq!(
        super::component_insert_reference(&references, "stream", &selected_construction)
            .unwrap()
            .ordinal,
        0
    );
    assert!(
        super::component_insert_reference(&references, "other-stream", &selected_construction)
            .is_none()
    );
}

#[test]
fn redirections_leaf_form_parses_empty_object_references() {
    let table = crate::xref::parse(&cadmpeg_test_support::service_decode_context(),
        br#"{"name":"RedirectionsStream","schema-version":0,"designs":[{"file-version":1,"targetFileName":"part.f3d","displayName":"part","lineageUrn":"urn:l","versionUrn":"urn:v"}],"references":{}}"#,
    )
    .unwrap();
    assert_eq!(table.designs.len(), 1);
    assert_eq!(table.designs[0].target_file_name.as_str(), "part.f3d");
    assert!(table.references.is_empty());
}

mod schema;

mod document_admission;

#[test]
fn identity_insert_carrier_requires_duplicate_targets_and_exact_closure() {
    let role = "aaaabbbb-cccc-dddd-eeee-ffff00001111";
    let mut body = crate::test_support::component_insert_test::identity_insert_carrier(role, 10);
    let metadata_tail = 251;
    for flag in [0, 1] {
        for token in [1_u64, 2] {
            body[93] = flag;
            body[metadata_tail + 1..metadata_tail + 9].copy_from_slice(&token.to_le_bytes());
            assert!(super::identity_component_insert_carrier(&body, 0, body.len(), 10).is_some());
        }
    }
    body[metadata_tail + 1..metadata_tail + 9].copy_from_slice(&0_u64.to_le_bytes());
    assert!(super::identity_component_insert_carrier(&body, 0, body.len(), 10).is_none());
    body[metadata_tail + 1..metadata_tail + 9].copy_from_slice(&1_u64.to_le_bytes());
    let closure = body.len() - 10;
    assert_eq!(body.len(), 420);
    let decoded = super::identity_component_insert_carrier(&body, 0, body.len(), 10).unwrap();
    assert_eq!(decoded, (role.to_owned(), 336));
    assert!(super::identity_component_insert_carrier(&body, 0, body.len(), 11).is_none());
    for offset in [93, metadata_tail, metadata_tail + 17, 327, 328, closure] {
        let saved = body[offset];
        body[offset] ^= 2;
        assert!(super::identity_component_insert_carrier(&body, 0, body.len(), 10).is_none());
        body[offset] = saved;
    }
    for end in 0..body.len() {
        assert!(super::identity_component_insert_carrier(&body, 0, end, 10).is_none());
    }
    body.push(0);
    assert!(super::identity_component_insert_carrier(&body, 0, body.len(), 10).is_none());
}

/// A `CommonData` component-path carrier whose path is the occurrence's
/// cross-document element followed by a local element naming its placement.
fn component_path_carrier(
    role: &str,
    entity_id: u64,
    placement_record: u64,
    transform: [[f64; 4]; 4],
    local_first: bool,
) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3_u32.to_le_bytes());
    bytes.extend_from_slice(b"257");
    bytes.extend_from_slice(&entity_id.to_le_bytes());
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&2_u32.to_le_bytes());
    let mut document = cross_document_reference(100, role);
    document.extend_from_slice(&1_u32.to_le_bytes());
    let mut placement = local_reference(placement_record);
    placement.extend_from_slice(&0_u32.to_le_bytes());
    if local_first {
        bytes.extend(placement);
        bytes.extend(document);
    } else {
        bytes.extend(document);
        bytes.extend(placement);
    }
    bytes.push(0);
    bytes.push(0);
    for value in transform.into_iter().flatten() {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend(local_reference(7));
    bytes.push(2);
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend_from_slice(&42_u32.to_le_bytes());
    bytes.extend(local_reference(8));
    bytes.extend(local_reference(3));
    bytes.extend(local_reference(6));
    bytes
}

#[test]
fn a_component_path_carrier_supplies_the_current_root_pose() {
    let role = "aaaabbbb-cccc-dddd-eeee-ffff00001111";
    let translation = |x: f64| {
        [
            [1.0, 0.0, 0.0, x],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ]
    };
    let decode = |carrier_names: u64, local_first: bool, second: Option<f64>| {
        let mut placement = occurrence_record_with_serializer_magic(
            role,
            10,
            &[1],
            Some(translation(7.0)),
            Some(crate::metastream::MODERN_SERIALIZER_MAGIC),
        );
        placement[4..7].copy_from_slice(b"256");
        let carrier =
            component_path_carrier(role, 11, carrier_names, translation(12.0), local_first);
        let carrier_at = u64_from_index(placement.len());
        let mut bulk = placement;
        bulk.extend(carrier);
        let second_at = u64_from_index(bulk.len());
        if let Some(x) = second {
            bulk.extend(component_path_carrier(role, 12, 10, translation(x), false));
        }
        let carriers: &[u64] = if second.is_some() { &[11, 12] } else { &[11] };
        let carrier_records: Vec<(u64, u64)> = if second.is_some() {
            vec![(10, 0), (11, carrier_at), (12, second_at)]
        } else {
            vec![(10, 0), (11, carrier_at)]
        };
        let properties =
            br#"{"docstruct":{"version":"1.0.0","type":"assembly-design","subtype":"synthetic","attributes":{}}}"#;
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let stored = crate::zip_write::file_options(CompressionMethod::Stored);
        write_synthetic_manifests(&mut zip, stored);
        zip.start_file("Properties.dat", stored).unwrap();
        zip.write_all(&u32::try_from(properties.len()).unwrap().to_le_bytes())
            .unwrap();
        zip.write_all(properties).unwrap();
        zip.start_file("RedirectionsStream.dat", stored).unwrap();
        zip.write_all(redirections_json("root.f3d", &[("part.f3d", role)]).as_bytes())
            .unwrap();
        zip.start_file("FusionAssetName[Active]/Design1/MetaStream.dat", stored)
            .unwrap();
        zip.write_all(&design_metastream_with_records(
            &[
                (
                    super::OCCURRENCE_PLACEMENT_TYPE_GUID,
                    "",
                    2,
                    "Component",
                    &[10],
                ),
                (
                    super::COMPONENT_PATH_TYPE_GUID,
                    "",
                    0,
                    "CommonData",
                    carriers,
                ),
            ],
            &carrier_records,
        ))
        .unwrap();
        zip.start_file("FusionAssetName[Active]/Design1/BulkStream.dat", stored)
            .unwrap();
        zip.write_all(&bulk).unwrap();
        F3dCodec
            .decode(
                &mut Cursor::new(zip.finish().unwrap().into_inner()),
                &DecodeOptions::default(),
            )
            .expect("synthetic XRef archive")
    };

    for local_first in [false, true] {
        let decoded = decode(10, local_first, None);
        let native = f3d_native(decoded.ir());
        assert_eq!(
            native.xref_references[0]
                .current_pose
                .as_ref()
                .and_then(|pose| pose.transform)
                .map(|transform| transform.rows()[0][3]),
            Some(12.0),
            "local element first: {local_first}"
        );
    }
    let decoded = decode(10, false, None);
    let native = f3d_native(decoded.ir());
    let reference = &native.xref_references[0];
    assert_eq!(reference.transform.unwrap().rows()[0][3], 7.0);
    let current = reference.current_pose.as_ref().expect("current pose");
    assert_eq!(current.carrier_record_index, 11);
    assert_eq!(current.transform.unwrap().rows()[0][3], 12.0);
    assert!(super::current_pose_matches(reference));
    let occurrence = decoded
        .ir()
        .model
        .occurrences
        .iter()
        .find(|occurrence| occurrence.native_ref.as_deref() == Some(reference.id.as_str()))
        .expect("root occurrence");
    assert!((occurrence.transform.rows()[0][3] - 120.0).abs() < EPS_PLACEMENT_TRANSLATION);
    assert!(decoded
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == F3dLossCode::XrefPlacementCurrentPose.kind()));

    let mut changed = reference.clone();
    let mut pose = changed.current_pose.clone().unwrap();
    pose.transform = Some(translation(13.0).try_into().unwrap());
    changed.current_pose = Some(pose);
    assert!(!super::current_pose_matches(&changed));

    let unbound = decode(99, false, None);
    let native = f3d_native(unbound.ir());
    assert!(native.xref_references[0].current_pose.is_none());
    assert!(unbound
        .report()
        .losses
        .iter()
        .all(|loss| loss.code != F3dLossCode::XrefPlacementCurrentPose.kind()));

    let ambiguous = decode(10, false, Some(13.0));
    let native = f3d_native(ambiguous.ir());
    assert!(native.xref_references[0].current_pose.is_none());
    assert_eq!(
        native.xref_references[0].transform.unwrap().rows()[0][3],
        7.0
    );
    let notes = ambiguous
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code == F3dLossCode::XrefCurrentPoseAmbiguous.kind())
        .collect::<Vec<_>>();
    assert_eq!(notes.len(), 1);
    assert!(notes[0].message.contains("[11, 12]"));
}

#[test]
fn pathless_occurrence_placements_bind_the_target_role_and_close() {
    use crate::test_support::{lp_ascii, typed_cross_document_reference};
    let role = "aaaabbbb-cccc-dddd-eeee-ffff00001111";
    let matrix = [
        [1.0, 0.0, 0.0, 2.0],
        [0.0, 1.0, 0.0, -3.0],
        [0.0, 0.0, 1.0, 4.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    for transform in [None, Some(matrix)] {
        let original = repeated_target_occurrence_record(role, 42, 1, transform);
        let path_end = super::occurrence_path(&original).unwrap().1;
        let repeated_start = path_end + 4 + 2 * 76 + 1;
        let mut repeated_end = repeated_start;
        super::take_reference(&original, &mut repeated_end).unwrap();
        let mut bytes = original[..15].to_vec();
        lp_ascii(&mut bytes, "");
        bytes.extend_from_slice(&[0, 0]);
        bytes.extend_from_slice(&original[path_end..repeated_start]);
        bytes.extend(typed_cross_document_reference(
            100,
            super::ROOT_COMPONENT_TYPE_GUID,
            role,
        ));
        bytes.extend_from_slice(&original[repeated_end..]);
        let placement = super::occurrence_placement(&bytes, None).unwrap().unwrap();
        assert_eq!(placement.link_names, [role]);
        assert_eq!(placement.carrier_record_index, 42);
        assert_eq!(placement.owner.unwrap().target, 3);
        assert_eq!(placement.transform, transform);
        assert_eq!(
            placement.occurrence_guid.unwrap().as_str(),
            "bbbbbbbb-cccc-dddd-eeee-ffffffffffff"
        );
        let details = super::repeated_target_occurrence_placement_details(&bytes)
            .unwrap()
            .unwrap();
        assert_eq!(details.role, role);
        for offset in [0, 4, 15, 19, 20, 21, 25, bytes.len() - 1] {
            let mut bad = bytes.clone();
            bad[offset] = 2;
            assert!(
                super::occurrence_placement(&bad, None).unwrap().is_none(),
                "offset {offset}"
            );
        }
        let mut extra = bytes.clone();
        extra.push(0);
        assert!(super::occurrence_placement(&extra, None).unwrap().is_none());
        let mut wrong = bytes.clone();
        let target_at = 25 + 2 * 76 + 1;
        let mut target_end = target_at;
        super::take_reference(&wrong, &mut target_end).unwrap();
        wrong.splice(
            target_at..target_end,
            typed_cross_document_reference(100, super::OCCURRENCE_PLACEMENT_TYPE_GUID, role),
        );
        assert!(super::occurrence_placement(&wrong, None).unwrap().is_none());
    }
}

mod reference_rows;
