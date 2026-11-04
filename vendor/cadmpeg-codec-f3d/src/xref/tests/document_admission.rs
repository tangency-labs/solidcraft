// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use std::collections::HashSet;
use std::io::Cursor;

use crate::test_support::assembly_test::{f3d_without_brep, XREF_ROLE};

use crate::F3dCodec;

use super::occurrence_record;

fn single_type_metadata(
    type_guid: &str,
    version: u32,
    module: &str,
    class_tag: &str,
    entity: u64,
) -> crate::metastream::MetaStream {
    let entities = [entity];
    let ordinal = class_tag.parse::<usize>().unwrap() - 256;
    let mut types = vec![
        (
            "00000000-0000-0000-0000-000000000000",
            "",
            0,
            "Other",
            &[][..]
        );
        ordinal
    ];
    types.push((type_guid, "", version, module, entities.as_slice()));
    let bytes =
        crate::test_support::streams_test::design_metastream_with_records(&types, &[(entity, 0)]);
    crate::metastream::parse(&bytes, "Design/MetaStream.dat").unwrap()
}

#[test]
fn three_property_control_one_identity_requires_the_complete_registered_form() {
    use crate::layout::legacy_attributed_external_placement_attributes as attributes;
    use crate::layout::legacy_attributed_external_placement_members as members;
    use crate::layout::legacy_attributed_external_placement_prefix as prefix;

    let role = "aaaabbbb-cccc-dddd-eeee-ffff00001111";
    let mut frame = crate::test_support::legacy_attributed_external_placement(role, true, None);
    frame[4..7].copy_from_slice(b"256");
    frame[prefix::LEN + attributes::LEN + members::CONTROL..][..2]
        .copy_from_slice(&1_u16.to_le_bytes());
    assert_eq!(
        frame.len(),
        prefix::LEN + attributes::LEN + members::LEN + super::super::legacy_identity_tail::LEN
    );
    let meta = single_type_metadata(
        super::super::OCCURRENCE_PLACEMENT_TYPE_GUID,
        2,
        "Component",
        "256",
        10,
    );
    let records = super::super::typed_occurrence_placement_records(&frame, &meta).unwrap();
    let placements = super::super::occurrence_placements(&frame, &records, None);
    assert_eq!(placements.len(), 1);
    assert_eq!(placements[0].link_names, [role]);
    assert!(placements[0].transform.is_none());
    assert_eq!(placements[0].owner.as_ref().unwrap().target, 7);
    for end in 0..frame.len() {
        assert!(
            super::super::occurrence_placement(&frame[..end], None)
                .unwrap()
                .is_none(),
            "end={end}"
        );
    }
    let wrong = single_type_metadata(
        "00000000-0000-0000-0000-000000000000",
        2,
        "Component",
        "256",
        10,
    );
    assert!(
        super::super::typed_occurrence_placement_records(&frame, &wrong)
            .unwrap()
            .is_empty()
    );
    let document = super::document_with_placement(role, frame);
    let result = F3dCodec
        .decode(&mut Cursor::new(document), &DecodeOptions::default())
        .unwrap();
    let mut reference =
        crate::test_support::native_test::f3d_native(result.ir()).xref_references[0].clone();
    assert_eq!(reference.id, "f3d:xref:reference#0");
    assert_eq!(
        result.ir().model.occurrences[0].native_ref.as_deref(),
        Some(reference.id.as_str())
    );
    assert!(super::super::placement_source_matches(&reference).unwrap());
    reference.neutron_role = "bbbbbbbb-cccc-dddd-eeee-ffffffffffff"
        .to_owned()
        .try_into()
        .unwrap();
    assert!(!super::super::placement_source_matches(&reference).unwrap());
}

#[test]
fn control_one_requires_the_three_property_identity_envelope() {
    use crate::layout::legacy_attributed_external_placement_attributes as attributes;
    use crate::layout::legacy_attributed_external_placement_empty_attributes as empty;
    use crate::layout::legacy_attributed_external_placement_members as members;
    use crate::layout::legacy_attributed_external_placement_prefix as prefix;

    let role = "aaaabbbb-cccc-dddd-eeee-ffff00001111";
    for (properties, matrix, control) in [
        (true, Some(crate::xref::IDENTITY_ROWS), 1_u16),
        (false, None, 1),
        (true, None, 2),
    ] {
        let mut frame =
            crate::test_support::legacy_attributed_external_placement(role, properties, matrix);
        let start = prefix::LEN
            + if properties {
                attributes::LEN
            } else {
                empty::LEN
            };
        frame[start + members::CONTROL..][..2].copy_from_slice(&control.to_le_bytes());
        assert!(super::super::occurrence_placement(&frame, None)
            .unwrap()
            .is_none());
    }
}

#[test]
fn fixture_placement_frames_require_registered_component_types() {
    #[derive(serde::Deserialize)]
    struct Case {
        label: String,
        type_guid: String,
        version: u32,
        module: String,
        class_tag: String,
        entity: u64,
        source_hex: cadmpeg_ir::native::bytes::NativeBytes,
        role: Option<String>,
        matrix: Option<[f64; 16]>,
    }
    let cases: Vec<Case> = serde_json::from_str(&crate::test_support::verification::text(
        "xref/original_records.json",
        include_str!("../../test_support/xref/original_records.json"),
    ))
    .unwrap();
    for case in cases {
        let meta = single_type_metadata(
            &case.type_guid,
            case.version,
            &case.module,
            &case.class_tag,
            case.entity,
        );
        let bytes = case.source_hex.as_ref();
        let records = super::super::typed_occurrence_placement_records(bytes, &meta).unwrap();
        let placements = super::super::occurrence_placements(bytes, &records, None);
        let Some(role) = case.role else {
            assert!(records.is_empty(), "{}", case.label);
            assert!(placements.is_empty(), "{}", case.label);
            continue;
        };
        assert_eq!(placements.len(), 1, "{}", case.label);
        assert_eq!(placements[0].link_names, [role], "{}", case.label);
        assert_eq!(
            placements[0].transform.map(|rows| rows.concat()),
            case.matrix.map(|values| values.to_vec()),
            "{}",
            case.label
        );
    }
}

#[test]
fn placement_metadata_boundaries_ignore_header_bytes_in_a_rigid_transform() {
    let role = "cccccccc-dddd-eeee-ffff-000000000000";
    let translation = f64::from_le_bytes([3, 0, 0, 0, b'3', b'8', b'0', 0]);
    let matrix = [
        [1.0, 0.0, 0.0, translation],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let bytes = super::legacy_occurrence_record(role, 10, Some(matrix));
    let meta = single_type_metadata(
        super::super::OCCURRENCE_PLACEMENT_TYPE_GUID,
        2,
        "Component",
        "380",
        10,
    );
    let records = super::super::typed_occurrence_placement_records(&bytes, &meta).unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].end, bytes.len());
    let placements = super::super::occurrence_placements(&bytes, &records, None);
    assert_eq!(placements.len(), 1);
    assert_eq!(placements[0].transform, Some(matrix));
}

#[test]
fn nested_headers_do_not_create_additional_placement_records() {
    let role = "cccccccc-dddd-eeee-ffff-000000000000";
    let mut bytes = super::legacy_occurrence_record(role, 10, None);
    let boundary = bytes.len();
    bytes.extend_from_within(..);
    let mut meta = single_type_metadata(
        super::super::OCCURRENCE_PLACEMENT_TYPE_GUID,
        2,
        "Component",
        "380",
        10,
    );
    meta.secondary_records
        .push(crate::metastream::RecordIndexEntry {
            entity_id: 10,
            bulk_offset: u64::try_from(boundary).unwrap(),
        });
    let records = super::super::typed_occurrence_placement_records(&bytes, &meta).unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].end, boundary);
    assert_eq!(
        super::super::occurrence_placements(&bytes, &records, None).len(),
        1
    );
    bytes[4..7].copy_from_slice(b"381");
    assert!(super::super::typed_occurrence_placement_records(&bytes, &meta).is_err());
}

#[test]
fn inconsistent_legacy_target_fields_retain_the_role_as_a_placement_failure() {
    let role = "cccccccc-dddd-eeee-ffff-000000000000";
    let original = super::legacy_occurrence_record(role, 10, None);
    let target = super::legacy_occurrence_reference(3, 0x0102_0304_0506_0708);
    let positions: Vec<_> = original
        .windows(target.len())
        .enumerate()
        .filter_map(|(at, bytes)| (bytes == target).then_some(at))
        .collect();
    assert_eq!(positions.len(), 2);
    let meta = single_type_metadata(
        super::super::OCCURRENCE_PLACEMENT_TYPE_GUID,
        2,
        "Component",
        "380",
        10,
    );
    for field in [1, 10, 23] {
        let mut bytes = original.clone();
        bytes[positions[1] + field] += 1;
        let records = super::super::typed_occurrence_placement_records(&bytes, &meta).unwrap();
        let (placements, failures) =
            super::super::occurrence_placements_with_failures(&bytes, &records, None, None)
                .unwrap();
        assert!(placements.is_empty());
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0].link_names, [role]);
    }
}

#[test]
fn missing_metadata_does_not_infer_an_external_transform_from_record_shape() {
    use std::io::{Read, Write};
    let matrix = [
        [1.0, 0.0, 0.0, 3.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let archive = super::document_with_modern_placement(XREF_ROLE, matrix);
    let mut source = zip::ZipArchive::new(Cursor::new(archive)).unwrap();
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for ordinal in 0..source.len() {
        let mut entry = source.by_index(ordinal).unwrap();
        let name = entry.name().unwrap().into_owned();
        if name.ends_with("/MetaStream.dat") {
            continue;
        }
        zip.start_file(
            name,
            crate::zip_write::file_options(zip::CompressionMethod::Stored),
        )
        .unwrap();
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).unwrap();
        zip.write_all(&bytes).unwrap();
    }
    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(zip.finish().unwrap().into_inner()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let native = crate::test_support::native_test::f3d_native(decoded.ir());
    assert_eq!(native.xref_references.len(), 1);
    assert_eq!(native.xref_references[0].transform, None);
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.code == crate::loss::F3dLossCode::XrefPlacementMetadataMissing.kind()
    }));
}
#[test]
fn typed_placement_admission_rejects_shape_collision() {
    let bytes = occurrence_record("role", 10, &[2, 3], None);
    let records = super::super::indexed_records(&bytes);
    let no_registered_placements = HashSet::new();
    assert!(super::super::occurrence_placements_filtered(
        &bytes,
        &records,
        None,
        Some(&no_registered_placements),
    )
    .is_empty());

    let registered_placement = HashSet::from([0]);
    assert_eq!(
        super::super::occurrence_placements_filtered(
            &bytes,
            &records,
            None,
            Some(&registered_placement),
        )
        .len(),
        1
    );
}

#[test]
fn assembly_root_without_brep_is_not_a_blocking_loss() {
    let archive = f3d_without_brep("assembly-design", "root.f3d", &[("comp.f3d", XREF_ROLE)]);
    let decoded = F3dCodec
        .decode(&mut Cursor::new(archive), &DecodeOptions::default())
        .unwrap();
    assert!(
        decoded
            .report()
            .losses
            .iter()
            .all(|loss| loss.severity < cadmpeg_ir::report::Severity::Error),
        "assembly document must not report blocking/error losses: {:?}",
        decoded.report().losses
    );
    assert!(decoded
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("assembly document")));
    assert!(decoded
        .report()
        .notes
        .iter()
        .any(|note| note.contains("comp.f3d") && note.contains(XREF_ROLE)));
    let native =
        crate::native::F3dNative::load(decoded.ir().native.namespace("f3d").unwrap()).unwrap();
    assert_eq!(native.xref_designs.len(), 2);
    assert_eq!(native.xref_references.len(), 1);
    assert_eq!(native.xref_references[0].relative_path.as_str(), "comp.f3d");
    assert_eq!(native.xref_references[0].neutron_role.as_str(), XREF_ROLE);
    let source = decoded.ir().source.as_ref().unwrap();
    assert_eq!(
        source.attributes.get("docstruct_type").map(String::as_str),
        Some("assembly-design")
    );
}

#[test]
fn part_without_brep_keeps_blocking_losses() {
    // A leaf redirections table (no outgoing references) does not make a
    // BREP-less part a valid assembly.
    let archive = f3d_without_brep("part-design", "part.f3d", &[]);
    let decoded = F3dCodec
        .decode(&mut Cursor::new(archive), &DecodeOptions::default())
        .unwrap();
    assert!(decoded
        .report()
        .losses
        .iter()
        .any(|loss| loss.severity == cadmpeg_ir::report::Severity::Blocking));
}

#[test]
fn empty_model_maps_with_outgoing_references_are_an_assembly_without_docstruct() {
    let snapshot = crate::test_support::smbh_geometry_test::synthetic_wire_body_smbh();
    let archive = crate::test_support::body_map_test::f3d_with_empty_model_maps(
        &snapshot,
        &[("part.f3d", XREF_ROLE)],
    );
    let decoded = F3dCodec
        .decode(&mut Cursor::new(archive), &DecodeOptions::default())
        .unwrap();
    assert!(decoded.ir().model.bodies.is_empty());
    assert!(!decoded.report().geometry_transferred());
    assert!(decoded
        .report()
        .losses
        .iter()
        .all(|loss| loss.code != crate::loss::F3dLossCode::BodylessDesignCarrier.kind()));
    assert_eq!(decoded.ir().model.occurrences.len(), 1);
    assert_eq!(
        decoded
            .report()
            .losses
            .iter()
            .filter(
                |loss| loss.code == crate::loss::F3dLossCode::XrefPlacementIdentityAssumed.kind()
            )
            .count(),
        1
    );
    assert!(decoded
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == crate::loss::F3dLossCode::AssemblyComponentsExternal.kind()));
    assert!(decoded.report().losses.iter().all(|loss| loss.code
        != crate::loss::F3dLossCode::MissingGeometryStream.kind()
        && !loss.message.contains("not a decodable BinaryFile")));
    assert!(!decoded
        .ir()
        .source
        .as_ref()
        .unwrap()
        .attributes
        .contains_key("docstruct_type"));
}
