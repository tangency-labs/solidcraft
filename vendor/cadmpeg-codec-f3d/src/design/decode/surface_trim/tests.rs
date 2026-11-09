// SPDX-License-Identifier: Apache-2.0
#![allow(
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::trivially_copy_pass_by_ref,
    clippy::uninlined_format_args
)]
use super::exact_surface_trim_operation;
use crate::design::decode::deleted_members::timestamp_table_spans_by_owner;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::feature::surface_ops::DesignSurfaceTrimOperation;
use crate::test_support::indexed_header;
use std::io::{Cursor, Write};
use zip::CompressionMethod;

fn surface_trim_carrier(
    owner: u32,
    table_class: [u8; 3],
    entries: &[(u32, u64)],
    total: u32,
) -> (Vec<u8>, DesignParameterScope) {
    let mut bytes = Vec::new();
    indexed_header(&mut bytes, *b"344", 811);
    bytes.extend_from_slice(&[0; 10]);
    indexed_header(&mut bytes, *b"288", 815);
    bytes.extend_from_slice(&[0; 14]);
    bytes.extend_from_slice(&[1, 1]);
    bytes.extend_from_slice(&2u32.to_le_bytes());
    for record_index in [801u32, 808, owner] {
        bytes.push(1);
        bytes.extend_from_slice(&record_index.to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
        bytes.extend_from_slice(&1_700_000_000_000_000u64.to_le_bytes());
    }
    bytes.extend_from_slice(&[0; 8]);
    indexed_header(&mut bytes, *b"271", 816);
    bytes.extend_from_slice(&[0; 10]);
    indexed_header(&mut bytes, table_class, 817);
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&u32::try_from(entries.len()).unwrap().to_le_bytes());
    for (record_index, ordinal) in entries {
        bytes.push(1);
        bytes.extend_from_slice(&record_index.to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
        bytes.extend_from_slice(&ordinal.to_le_bytes());
    }
    bytes.extend_from_slice(&total.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    indexed_header(&mut bytes, *b"257", 817);
    indexed_header(&mut bytes, *b"286", 819);
    indexed_header(&mut bytes, *b"351", 820);

    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#800",
        crate::records::feature::scope::DesignFeatureKind::SurfaceTrim,
        800,
    );
    scope
        .try_edit(|draft| {
            draft.reference_members =
                crate::records::identity::ReferenceRun::unlocated(vec![801, 804, 808, 811]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    (bytes, scope)
}

fn surface_trim_selection_and_cell_table() -> (Vec<u8>, DesignParameterScope) {
    surface_trim_carrier(800, *b"325", &[(819, 1), (820, 4)], 5)
}

fn decode(bytes: &[u8], scope: &DesignParameterScope) -> Option<DesignSurfaceTrimOperation> {
    exact_surface_trim_operation(
        bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(bytes),
        &timestamp_table_spans_by_owner(bytes),
        scope,
    )
}

#[test]
fn surface_trim_output_carries_its_scope_identifier() {
    const ENTRY: &str = "FusionAssetName[Active]/Design1/BulkStream.dat";
    let (bytes, mut scope) = surface_trim_selection_and_cell_table();
    scope.id = format!(
        "{}:design-parameter-scope#800",
        crate::ids::native_scope(ENTRY)
    );
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    crate::test_support::manifest_test::write_synthetic_manifests(&mut zip, stored);
    zip.start_file(ENTRY, stored).unwrap();
    zip.write_all(&bytes).unwrap();
    let archive = zip.finish().unwrap().into_inner();
    crate::test_support::zip_test::with_scan(&archive, |scan| {
        let operations = super::decode_surface_trim_operations(scan, &[scope.clone()]).unwrap();
        assert_eq!(operations.len(), 1);
        assert_eq!(
            operations[0].id,
            format!(
                "{}:design-surface-trim-operation#{}",
                crate::ids::native_scope(ENTRY),
                scope.byte_offset(),
            ),
        );
    });
}

#[test]
fn surface_trim_decodes_timestamp_chain_and_cell_table() {
    let (bytes, scope) = surface_trim_selection_and_cell_table();
    let operation = decode(&bytes, &scope).expect("exact SurfaceTrim cell carrier");

    assert_eq!(
        operation
            .chain_records
            .iter()
            .map(|record| (
                record.record_index,
                record.class_tag.as_str(),
                record.frame_length
            ))
            .collect::<Vec<_>>(),
        vec![(815, "288", 96), (816, "271", 21)]
    );
    assert_eq!(operation.cell_table_record_index, 817);
    assert_eq!(operation.cell_table_class_tag.as_str(), "325");
    assert_eq!(operation.cell_table_paired_class_tag.as_str(), "257");
    assert_eq!(
        operation
            .cell_entries()
            .iter()
            .map(|entry| (entry.record_index, entry.ordinal))
            .collect::<Vec<_>>(),
        vec![(819, 1), (820, 4)]
    );
    assert_eq!(operation.trailing_value, 5);
    assert_eq!(
        operation.trailing_zero_offset,
        operation.trailing_value_offset + 4
    );
}

#[test]
fn surface_trim_cell_table_is_recognized_under_any_class_tag() {
    let (bytes, scope) = surface_trim_carrier(800, *b"350", &[(819, 1), (820, 2)], 3);
    let operation = decode(&bytes, &scope).expect("cell table under a segment-local tag");
    assert_eq!(operation.cell_table_class_tag.as_str(), "350");
    assert_eq!(operation.trailing_value, 3);
}

#[test]
fn surface_trim_keeps_repeated_entry_values() {
    for trailing_value in [2, 3] {
        let (bytes, scope) =
            surface_trim_carrier(800, *b"263", &[(819, 1), (820, 1)], trailing_value);
        let operation = decode(&bytes, &scope).expect("repeated entry values");
        assert_eq!(
            operation
                .cell_entries()
                .iter()
                .map(|entry| (entry.record_index, entry.ordinal))
                .collect::<Vec<_>>(),
            vec![(819, 1), (820, 1)]
        );
        assert_eq!(operation.trailing_value, trailing_value);
    }
}

#[test]
fn surface_trim_requires_the_scope_timestamp_table() {
    let (bytes, scope) = surface_trim_carrier(799, *b"325", &[(819, 1), (820, 4)], 5);
    assert!(decode(&bytes, &scope).is_none());
}

#[test]
fn surface_trim_rejects_cell_ordinal_outside_partition() {
    let (bytes, scope) = surface_trim_carrier(800, *b"325", &[(819, 6), (820, 4)], 5);
    assert!(decode(&bytes, &scope).is_none());
}

#[test]
fn surface_trim_rejects_nonzero_cell_table_tail() {
    let (mut bytes, scope) = surface_trim_selection_and_cell_table();
    let table_start = bytes
        .windows(11)
        .position(|window| window == [3, 0, 0, 0, b'3', b'2', b'5', 0x31, 3, 0, 0])
        .expect("cell table header");
    let tail_zero = table_start + 67;
    bytes[tail_zero] = 1;
    assert!(decode(&bytes, &scope).is_none());
}
