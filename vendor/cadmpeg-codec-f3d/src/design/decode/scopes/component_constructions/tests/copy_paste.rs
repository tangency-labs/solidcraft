// SPDX-License-Identifier: Apache-2.0
use super::super::exact_copy_paste_component_operation;
use crate::records::feature::assembly_features::{
    DesignComponentOccurrence, DesignComponentOccurrenceDraft, DesignComponentOccurrencePlacement,
};
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use crate::records::identity::ReferenceRun;
use crate::records::sketch_placement::SketchPlacementMatrix;
use std::num::NonZeroU32;

#[test]
fn copy_paste_matrices_keep_their_source_locations() {
    for (length, source_at) in [(525, 34), (529, 38)] {
        let start = 88;
        let mut bytes = vec![0; start + length + 11];
        let header = |bytes: &mut [u8], at: usize, tag: &[u8; 3], index: u32| {
            bytes[at..at + 4].copy_from_slice(&3_u32.to_le_bytes());
            bytes[at + 4..at + 7].copy_from_slice(tag);
            bytes[at + 7..at + 11].copy_from_slice(&index.to_le_bytes());
        };
        header(&mut bytes, 20, b"264", 1500);
        header(&mut bytes, 77, b"259", 1500);
        header(&mut bytes, start, b"268", 1400);
        header(&mut bytes, start + length, b"261", 1400);
        bytes[41] = 1;
        bytes[42..46].copy_from_slice(&1601_u32.to_le_bytes());
        bytes[54] = 1;
        bytes[66] = 1;
        bytes[67..71].copy_from_slice(&1400_u32.to_le_bytes());
        let source = SketchPlacementMatrix::IDENTITY;
        let mut copied_rows: [[f64; 4]; 4] = source.into();
        copied_rows[0][3] = 2.0;
        let copied = SketchPlacementMatrix::try_from(copied_rows).unwrap();
        for (at, rows) in [
            (start + source_at, source),
            (start + source_at + 156, copied),
        ] {
            let rows: [[f64; 4]; 4] = rows.into();
            for (ordinal, value) in rows.into_iter().flatten().enumerate() {
                bytes[at + ordinal * 8..at + ordinal * 8 + 8].copy_from_slice(&value.to_le_bytes());
            }
        }
        let mut scope = DesignParameterScope::empty(
            "f3d:Design/BulkStream.dat:copy#88",
            DesignFeatureKind::CopyPaste,
            1400,
        );
        scope
            .try_edit(|draft| {
                draft.byte_offset = 88;
                draft.reference_count_offset = 97;
                draft.frame_length = u64::try_from(length).unwrap();
                draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
                draft.reference_members = ReferenceRun::unlocated(vec![1500]);
                draft.layout_fixture_references();
                draft.layout_fixture_tail();
            })
            .unwrap();
        let occurrence = |index: u32, offset: u64, guid: &str, placement| {
            DesignComponentOccurrence::try_new(DesignComponentOccurrenceDraft {
                id: format!("f3d:Design/BulkStream.dat:occurrence#{index}"),
                class_tag: "269".to_owned().try_into().unwrap(),
                record_index: index,
                byte_offset: offset,
                component_record_index: 1800,
                component_guid: "11111111-2222-3333-4444-555555555555"
                    .to_owned()
                    .try_into()
                    .unwrap(),
                occurrence_guid: guid.to_owned().try_into().unwrap(),
                placement,
                owner_entity: 0,
            })
            .unwrap()
        };
        let occurrences = [
            occurrence(
                1600,
                0,
                "aaaaaaaa-1111-2222-3333-bbbbbbbbbbbb",
                DesignComponentOccurrencePlacement::Base,
            ),
            occurrence(
                1601,
                1,
                "cccccccc-1111-2222-3333-dddddddddddd",
                DesignComponentOccurrencePlacement::Explicit {
                    ordinal: NonZeroU32::new(2).unwrap(),
                    transform: copied,
                },
            ),
        ];
        let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
        let operation =
            { exact_copy_paste_component_operation(&bytes, &records, &scope, &occurrences) }
                .unwrap()
                .unwrap();
        assert_eq!(operation.source_transform, source);
        assert_eq!(operation.copied_transform, copied);
        assert_eq!(
            operation.source_transform_offset,
            u64::try_from(start + source_at).unwrap()
        );
        assert_eq!(
            operation.copied_transform_offset,
            u64::try_from(start + source_at + 156).unwrap()
        );
    }
}
