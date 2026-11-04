// SPDX-License-Identifier: Apache-2.0
#![allow(
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::trivially_copy_pass_by_ref,
    clippy::uninlined_format_args
)]

use cadmpeg_core::decode::u64_from_index;

use crate::design::decode::scopes::solid_primitive::exact_solid_primitive;
use crate::records::feature::extrude::DesignExtrudeOperation;
use crate::records::feature::primitives::DesignSolidPrimitive;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::parameters::DesignParameterOwner;

#[test]
fn fixed_guid_scan_matches_decoded_relaxed_guid_validation() {
    for value in [
        "00000000-0000-0000-0000-000000000000",
        "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789",
        "00000000-0000-0000-0000-00000000000!",
        "é0000000-0000-0000-0000-000000000000",
        "short",
    ] {
        let mut bytes = u32::try_from(value.encode_utf16().count())
            .unwrap()
            .to_le_bytes()
            .to_vec();
        for code_unit in value.encode_utf16() {
            bytes.extend_from_slice(&code_unit.to_le_bytes());
        }
        let prior = crate::bytes::lp_utf16_bounded(&bytes, 0, 36..=36)
            .and_then(|(guid, end)| crate::bytes::is_guid_relaxed(&guid).then_some(end));
        assert_eq!(
            crate::design::decode::text::fixed_guid_end(&bytes, 0),
            prior
        );
        bytes.pop();
        assert_eq!(crate::design::decode::text::fixed_guid_end(&bytes, 0), None);
    }
}

#[test]
fn named_solid_primitives_bind_ordered_parameter_owners() {
    fn owner(
        scope_record_index: u32,
        record_index: u32,
        local_ordinal: u32,
        value: f64,
    ) -> DesignParameterOwner {
        crate::records::parameters::DesignParameterOwner::try_from(
            crate::records::parameters::DesignParameterOwnerWire {
                id: format!("f3d:Design/BulkStream.dat:owner#{record_index}"),
                byte_offset: u64::from(record_index) + 60,
                frame_length: 104,
                class_tag: crate::records::references::DesignClassTag::try_from("272".to_owned())
                    .unwrap(),
                record_index,
                scope_record_index,
                local_ordinal,
                evaluated_value: value,
                evaluated_value_offset: u64::from(record_index) + 100,
                parameter_record_index: record_index + 1,
                owned_ordinal: local_ordinal,
                variant: Some(0),
                companion_record_index: record_index + 2,
            },
        )
        .unwrap()
    }

    let mut bytes = vec![0; 200];
    bytes[20..24].copy_from_slice(&1u32.to_le_bytes());
    bytes[24] = 0;
    bytes[25] = 1;
    let mut box_scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:scope#12",
        crate::records::feature::scope::DesignFeatureKind::BoxPrimitive,
        12,
    );
    box_scope
        .try_edit(|draft| {
            draft.frame_length = u64_from_index(bytes.len());
            draft.reference_members =
                crate::records::identity::ReferenceRun::unlocated(vec![20, 21, 22, 23, 24]);
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    let box_owners = vec![
        owner(12, 20, 0, 3.0),
        owner(12, 21, 1, 4.0),
        owner(12, 22, 2, 2.0),
        owner(12, 23, 3, 0.5),
        owner(12, 24, 4, -0.25),
    ];
    let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
    assert!(matches!(
        exact_solid_primitive(&bytes, &records, &box_scope, &box_owners),
        Some(DesignSolidPrimitive::Box(
            crate::records::feature::primitives::DesignBoxPrimitive {
                length,
                width,
                height,
                offset_x,
                offset_y,
                operation: DesignExtrudeOperation::Join,
                operation_offset: 20,
                ..
            }
        )) if length.get() == 3.0 && width.get() == 4.0 && height.get() == 2.0 && offset_x.get() == 0.5 && offset_y.get() == -0.25
    ));

    bytes[20..24].copy_from_slice(&4u32.to_le_bytes());
    let mut cylinder_scope = box_scope;
    cylinder_scope
        .try_edit(|draft| {
            draft.payload = crate::records::feature::scope::DesignFeatureKind::CylinderPrimitive
                .try_into()
                .unwrap();
        })
        .unwrap();
    cylinder_scope.record_index = 13;
    cylinder_scope
        .try_edit(|draft| {
            draft.reference_members =
                crate::records::identity::ReferenceRun::unlocated(vec![30, 31]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    let cylinder_owners = vec![owner(13, 30, 0, 0.7), owner(13, 31, 1, 3.0)];
    assert!(matches!(
        exact_solid_primitive(&bytes, &records, &cylinder_scope, &cylinder_owners,),
        Some(DesignSolidPrimitive::Cylinder(
            crate::records::feature::primitives::DesignCylinderPrimitive {
                height,
                diameter,
                operation: DesignExtrudeOperation::NewBody,
                operation_offset: 20,
                ..
            }
        )) if height.get() == 0.7 && diameter.get() == 3.0
    ));
}

#[test]
fn shifted_cylinder_primitives_bind_exact_generation_frames() {
    fn indexed_header(bytes: &mut [u8], class_tag: &[u8; 3], record_index: u32) {
        bytes[0..4].copy_from_slice(&3u32.to_le_bytes());
        bytes[4..7].copy_from_slice(class_tag);
        bytes[7..11].copy_from_slice(&record_index.to_le_bytes());
    }

    fn guid(bytes: &mut [u8], count_offset: usize) {
        bytes[count_offset..count_offset + 4].copy_from_slice(&36u32.to_le_bytes());
        let value = "00000000-0000-0000-0000-000000000000";
        for (ordinal, code_unit) in value.encode_utf16().enumerate() {
            let at = count_offset + 4 + ordinal * 2;
            bytes[at..at + 2].copy_from_slice(&code_unit.to_le_bytes());
        }
    }

    fn owner(
        scope_record_index: u32,
        record_index: u32,
        local_ordinal: u32,
        value: f64,
        stream: &str,
    ) -> DesignParameterOwner {
        crate::records::parameters::DesignParameterOwner::try_from(
            crate::records::parameters::DesignParameterOwnerWire {
                id: format!("f3d:{stream}:owner#{record_index}"),
                byte_offset: u64::from(record_index),
                frame_length: 103,
                class_tag: crate::records::references::DesignClassTag::try_from("294".to_owned())
                    .unwrap(),
                record_index,
                scope_record_index,
                local_ordinal,
                evaluated_value: value,
                evaluated_value_offset: u64::from(record_index) + 40,
                parameter_record_index: record_index + 1,
                owned_ordinal: local_ordinal,
                variant: None,
                companion_record_index: record_index + 2,
            },
        )
        .unwrap()
    }

    fn scope(
        class_tag: &str,
        paired_class_tag: &str,
        record_index: u32,
        frame_length: usize,
        reference_members: Vec<u32>,
    ) -> DesignParameterScope {
        let stream = "Design/BulkStream.dat";
        let id = format!("f3d:{stream}:scope#{record_index}");
        let mut scope = DesignParameterScope::empty(
            &id,
            crate::records::feature::scope::DesignFeatureKind::CylinderPrimitive,
            record_index,
        );
        scope
            .try_edit(|draft| {
                draft.byte_offset = 0;
                draft.reference_count_offset = draft.byte_offset + 9;
                draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
                draft.layout_fixture_references();
                draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
                draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
                draft.layout_fixture_tail();
            })
            .unwrap();
        scope.class_tag =
            crate::records::references::DesignClassTag::try_from(class_tag.to_owned()).unwrap();
        scope.paired_class_tag =
            crate::records::references::DesignClassTag::try_from(paired_class_tag.to_owned())
                .unwrap();
        scope
            .try_edit(|draft| {
                draft.paired_byte_offset = u64_from_index(frame_length);
                draft.frame_length = u64_from_index(frame_length);
                draft.reference_members =
                    crate::records::identity::ReferenceRun::unlocated(reference_members);
                draft.layout_fixture_references();
                draft.layout_fixture_tail();
            })
            .unwrap();
        let (reference_count, _, kind, feature_ordinal, previous) = if frame_length == 352 {
            (174, 233, 241, 275, 306)
        } else {
            (302, 383, 391, 425, 456)
        };
        scope
            .try_edit(|draft| {
                draft.reference_count_offset = reference_count;
                draft.layout_fixture_references();

                draft.kind_offset = kind;
                draft.feature_ordinal_offset = feature_ordinal;
                draft.previous_history_state_id_offset = Some(previous);
            })
            .unwrap();
        scope
    }

    fn common_prefix(bytes: &mut [u8], operation: u32, references: &[u32]) {
        bytes[21] = 1;
        bytes[22..26].copy_from_slice(&operation.to_le_bytes());
        let mut reversed = references.iter().rev().copied();
        let first = reversed.next().unwrap();
        bytes[26] = 1;
        bytes[27] = 1;
        bytes[28..32].copy_from_slice(&first.to_le_bytes());
        for (offset, record_index) in [38, 49, 60].into_iter().zip(reversed.take(3)) {
            bytes[offset] = 1;
            bytes[offset + 1..offset + 5].copy_from_slice(&record_index.to_le_bytes());
        }
    }

    let mut compact = vec![0; 352];
    indexed_header(&mut compact, b"297", 12);
    let compact_references = [100, 101, 102, 103, 104];
    common_prefix(&mut compact, 4, &compact_references);
    compact[71] = 1;
    compact[72..76].copy_from_slice(&1u32.to_le_bytes());
    compact[76] = 1;
    compact[77..81].copy_from_slice(&99u32.to_le_bytes());
    guid(&mut compact, 95);
    let compact_scope = scope("297", "258", 12, 352, compact_references.into());
    let compact_owners = vec![
        owner(12, 103, 0, 0.7, "Design/BulkStream.dat"),
        owner(12, 104, 1, 3.0, "Design/BulkStream.dat"),
    ];
    assert!(matches!(
        exact_solid_primitive(
            &compact,
            &crate::design::test_support::indexed_record_offsets_for_test(&compact),
            &compact_scope,
            &compact_owners,
        ),
        Some(DesignSolidPrimitive::Cylinder(
            crate::records::feature::primitives::DesignCylinderPrimitive {
                height,
                diameter,
                operation: DesignExtrudeOperation::NewBody,
                operation_offset: 22,
                transform: None,
                ..
            }
        )) if height.get() == 0.7 && diameter.get() == 3.0
    ));

    for (class_tag, paired_class_tag) in [("297", "258"), ("375", "258"), ("414", "272")] {
        let mut expanded = vec![0; 502];
        indexed_header(&mut expanded, class_tag.as_bytes().try_into().unwrap(), 12);
        let expanded_references = [100, 101, 102, 103, 104, 105, 106];
        common_prefix(&mut expanded, 1, &expanded_references);
        let transform: [[f64; 4]; 4] = [
            [-1.0, 0.0, 0.0, 0.0],
            [0.0, -1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ];
        for (ordinal, value) in transform.into_iter().flatten().enumerate() {
            let at = 72 + ordinal * 8;
            expanded[at..at + 8].copy_from_slice(&value.to_le_bytes());
        }
        expanded[208] = 1;
        expanded[209..213].copy_from_slice(&0x0100_0000u32.to_le_bytes());
        expanded[213..217].copy_from_slice(&100u32.to_le_bytes());
        guid(&mut expanded, 223);
        let expanded_scope = scope(
            class_tag,
            paired_class_tag,
            12,
            502,
            expanded_references.into(),
        );
        let expanded_owners = vec![
            owner(12, 105, 0, 0.7, "Design/BulkStream.dat"),
            owner(12, 106, 1, 3.0, "Design/BulkStream.dat"),
        ];
        assert!(matches!(
            exact_solid_primitive(
                &expanded,
                &crate::design::test_support::indexed_record_offsets_for_test(&expanded),
                &expanded_scope,
                &expanded_owners,
            ),
            Some(DesignSolidPrimitive::Cylinder(
                crate::records::feature::primitives::DesignCylinderPrimitive {
                    height,
                    diameter,
                    operation: DesignExtrudeOperation::Join,
                    operation_offset: 22,
                    transform: Some(crate::records::identity::Located { offset: 72, .. }),
                    ..
                }
            )) if height.get() == 0.7 && diameter.get() == 3.0
        ));

        let mut translated = expanded;
        translated[72 + 3 * 8..72 + 4 * 8].copy_from_slice(&1.0f64.to_le_bytes());
        assert!(exact_solid_primitive(
            &translated,
            &crate::design::test_support::indexed_record_offsets_for_test(&translated),
            &expanded_scope,
            &expanded_owners,
        )
        .is_none());
    }
}

mod typed_reference_box {
    use super::*;

    const TABLE_GUIDS: [&str; 8] = [
        "5A1BF548-241F-46FD-9FB5-E4B05126EB9D",
        "7B6C3C2D-4096-4E6D-B225-699E8C2D9355",
        "D91D429C-1BB5-4640-B49F-69AF14F463E9",
        "D91D429C-1BB5-4640-B49F-69AF14F463E9",
        "D91D429C-1BB5-4640-B49F-69AF14F463E9",
        "D91D429C-1BB5-4640-B49F-69AF14F463E9",
        "D91D429C-1BB5-4640-B49F-69AF14F463E9",
        "CAA90BED-408E-4F03-A4E5-B1552D95C8EE",
    ];
    const PREFIX_MEMBERS: [usize; 7] = [7, 2, 3, 4, 1, 5, 6];
    const MEMBERS: [u32; 8] = [300, 301, 302, 303, 304, 305, 306, 307];
    const STREAM: &str = "Design/BulkStream.dat";
    const FRAME: [[f64; 4]; 4] = [
        [0.0, 1.0, 0.0, 3.0],
        [0.0, 0.0, 1.0, -1.5],
        [1.0, 0.0, 0.0, 0.25],
        [0.0, 0.0, 0.0, 1.0],
    ];

    fn typed_reference(bytes: &mut [u8], at: usize, target: u32, guid: &str) {
        bytes[at] = 1;
        bytes[at + 1..at + 5].copy_from_slice(&target.to_le_bytes());
        bytes[at + 9..at + 13].copy_from_slice(&36u32.to_le_bytes());
        bytes[at + 13..at + 49].copy_from_slice(guid.as_bytes());
    }

    fn form(operation: u32, frame: [[f64; 4]; 4]) -> Vec<u8> {
        let mut bytes = vec![0; 1295];
        bytes[0..4].copy_from_slice(&3u32.to_le_bytes());
        bytes[4..7].copy_from_slice(b"264");
        bytes[7..11].copy_from_slice(&12u32.to_le_bytes());
        bytes[21] = 1;
        bytes[22..26].copy_from_slice(&operation.to_le_bytes());
        bytes[26] = 1;
        for (ordinal, member) in PREFIX_MEMBERS.into_iter().enumerate() {
            typed_reference(
                &mut bytes,
                27 + 51 * ordinal,
                MEMBERS[member],
                TABLE_GUIDS[member],
            );
        }
        for (ordinal, value) in frame.into_iter().flatten().enumerate() {
            let at = 385 + ordinal * 8;
            bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
        }
        bytes[513..517].copy_from_slice(&1u32.to_le_bytes());
        typed_reference(&mut bytes, 517, 99, "11111111-2222-3333-4444-555555555555");
        bytes[576..580].copy_from_slice(&36u32.to_le_bytes());
        for (ordinal, unit) in "00000000-0000-0000-0000-000000000000"
            .encode_utf16()
            .enumerate()
        {
            let at = 580 + ordinal * 2;
            bytes[at..at + 2].copy_from_slice(&unit.to_le_bytes());
        }
        bytes[655..659].copy_from_slice(&8u32.to_le_bytes());
        for (ordinal, member) in MEMBERS.into_iter().enumerate() {
            typed_reference(&mut bytes, 659 + 51 * ordinal, member, TABLE_GUIDS[ordinal]);
        }
        bytes
    }

    /// A scope whose layout is that of the 1295-byte form, shortened by one
    /// 51-byte entry for each reference fewer than eight.
    fn scope(members: &[u32]) -> DesignParameterScope {
        let shortfall = 51 * (8 - members.len());
        let frame_length = 1295 - shortfall;
        let mut scope = DesignParameterScope::empty(
            &format!("f3d:{STREAM}:scope#12"),
            crate::records::feature::scope::DesignFeatureKind::BoxPrimitive,
            12,
        );
        scope.class_tag =
            crate::records::references::DesignClassTag::try_from("264".to_owned()).unwrap();
        scope.paired_class_tag =
            crate::records::references::DesignClassTag::try_from("262".to_owned()).unwrap();
        scope
            .try_edit(|draft| {
                draft.byte_offset = 0;
                draft.paired_byte_offset = u64_from_index(frame_length);
                draft.frame_length = u64_from_index(frame_length);
                draft.reference_count_offset = 655;
                draft.reference_members = crate::records::identity::ReferenceRun::located(
                    members
                        .iter()
                        .enumerate()
                        .map(|(ordinal, value)| crate::records::identity::Located {
                            value: *value,
                            offset: 655 + 5 + 51 * u64_from_index(ordinal),
                        })
                        .collect(),
                );
                draft.kind_offset = u64_from_index(1075 - shortfall);
                draft.feature_ordinal_offset = u64_from_index(1099 - shortfall);
                draft.history_state_id = Some(66);
                draft.previous_history_state_id = Some(65);
                draft.previous_history_state_id_offset = Some(u64_from_index(1169 - shortfall));
            })
            .unwrap();
        scope
    }

    fn owner(record_index: u32, local_ordinal: u32, value: f64) -> DesignParameterOwner {
        DesignParameterOwner::try_from(crate::records::parameters::DesignParameterOwnerWire {
            id: format!("f3d:{STREAM}:owner#{record_index}"),
            byte_offset: u64::from(record_index),
            frame_length: 103,
            class_tag: crate::records::references::DesignClassTag::try_from("294".to_owned())
                .unwrap(),
            record_index,
            scope_record_index: 12,
            local_ordinal,
            evaluated_value: value,
            evaluated_value_offset: u64::from(record_index) + 40,
            parameter_record_index: record_index + 1,
            owned_ordinal: local_ordinal,
            variant: None,
            companion_record_index: record_index + 2,
        })
        .unwrap()
    }

    fn owners(offset_x: f64, offset_y: f64) -> Vec<DesignParameterOwner> {
        vec![
            owner(MEMBERS[2], 0, 1.2),
            owner(MEMBERS[3], 1, 0.8),
            owner(MEMBERS[4], 2, 2.5),
            owner(MEMBERS[5], 3, offset_x),
            owner(MEMBERS[6], 4, offset_y),
        ]
    }

    fn decode(
        bytes: &[u8],
        scope: &DesignParameterScope,
        owners: &[DesignParameterOwner],
    ) -> Option<DesignSolidPrimitive> {
        exact_solid_primitive(
            bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(bytes),
            scope,
            owners,
        )
    }

    #[test]
    fn binds_operation_dimensions_and_frame() {
        let bytes = form(4, FRAME);
        let Some(DesignSolidPrimitive::Box(primitive)) =
            decode(&bytes, &scope(&MEMBERS), &owners(0.0, 0.0))
        else {
            panic!("typed-reference box");
        };
        assert_eq!(primitive.operation, DesignExtrudeOperation::NewBody);
        assert_eq!(primitive.operation_offset, 22);
        assert_eq!(
            [
                primitive.length.get(),
                primitive.width.get(),
                primitive.height.get()
            ],
            [1.2, 0.8, 2.5]
        );
        assert_eq!(
            primitive
                .transform
                .map(crate::records::sketch_placement::SketchPlacementMatrix::rows),
            Some(FRAME)
        );
        assert_eq!(primitive.transform_offset, Some(385));
        for (value, expected) in [
            (1, DesignExtrudeOperation::Join),
            (2, DesignExtrudeOperation::Cut),
        ] {
            let bytes = form(value, FRAME);
            assert!(matches!(
                decode(&bytes, &scope(&MEMBERS), &owners(0.0, 0.0)),
                Some(DesignSolidPrimitive::Box(primitive)) if primitive.operation == expected
            ));
        }
    }

    #[test]
    fn refuses_each_deviation() {
        let scope = scope(&MEMBERS);
        let owners = owners(0.0, 0.0);
        let valid = form(4, FRAME);
        assert!(decode(&valid, &scope, &owners).is_some());

        let mutate = |edit: &dyn Fn(&mut Vec<u8>)| {
            let mut bytes = valid.clone();
            edit(&mut bytes);
            decode(&bytes, &scope, &owners)
        };
        // Header, prologue and fixed-byte deviations.
        assert!(mutate(&|bytes| bytes[21] = 0).is_none());
        assert!(mutate(&|bytes| bytes[15] = 1).is_none());
        assert!(mutate(&|bytes| bytes[24] = 1).is_none());
        assert!(mutate(&|bytes| bytes[26] = 0).is_none());
        assert!(mutate(&|bytes| bytes[384] = 1).is_none());
        assert!(mutate(&|bytes| bytes[513] = 2).is_none());
        assert!(mutate(&|bytes| bytes[570] = 1).is_none());
        assert!(mutate(&|bytes| bytes[653] = 1).is_none());
        assert!(mutate(&|bytes| bytes[22] = 5).is_none());
        assert!(mutate(&|bytes| bytes[22] = 0).is_none());
        // Null GUID and tail reference.
        assert!(mutate(&|bytes| bytes[580] = b'1').is_none());
        assert!(mutate(&|bytes| bytes[518..522].copy_from_slice(&0u32.to_le_bytes())).is_none());
        // Prefix reference target, type GUID and table type GUID.
        assert!(mutate(&|bytes| bytes[28] ^= 1).is_none());
        assert!(mutate(&|bytes| bytes[27 + 13] = b'6').is_none());
        assert!(mutate(&|bytes| bytes[27 + 51 * 4 + 13] = b'6').is_none());
        assert!(mutate(&|bytes| bytes[659 + 13] = b'6').is_none());
        assert!(mutate(&|bytes| bytes[659 + 51 * 7 + 13] = b'6').is_none());
        assert!(mutate(&|bytes| bytes[655] = 7).is_none());
        // Frame: left-handed, scaled and non-bottom rows.
        for edit in [
            FRAME.map(|row| [row[0], row[1], -row[2], row[3]]),
            FRAME.map(|row| [row[0] * 2.0, row[1], row[2], row[3]]),
            [FRAME[0], FRAME[1], FRAME[2], [0.0, 0.0, 1.0, 1.0]],
        ] {
            let bytes = form(4, edit);
            assert!(decode(&bytes, &scope, &owners).is_none());
        }
    }

    #[test]
    fn refuses_scope_and_owner_deviations() {
        let valid = form(4, FRAME);
        let good_owners = owners(0.0, 0.0);
        // A nonzero offset owner leaves the composition with the frame undecoded.
        for (offset_x, offset_y) in [(0.5, 0.0), (0.0, -0.5)] {
            assert!(decode(&valid, &scope(&MEMBERS), &owners(offset_x, offset_y)).is_none());
        }
        // Reference count and reference order.
        assert!(decode(&valid, &scope(&MEMBERS[..7]), &good_owners).is_none());
        let mut reordered = MEMBERS;
        reordered.swap(2, 3);
        assert!(decode(&valid, &scope(&reordered), &good_owners).is_none());
        // An owner whose record is not the ordered member of its ordinal.
        let mut shifted = good_owners.clone();
        shifted[0] = owner(MEMBERS[2] + 50, 0, 1.2);
        assert!(decode(&valid, &scope(&MEMBERS), &shifted).is_none());
        let mut swapped = good_owners.clone();
        swapped[0] = owner(MEMBERS[3], 0, 1.2);
        swapped[1] = owner(MEMBERS[2], 1, 0.8);
        assert!(decode(&valid, &scope(&MEMBERS), &swapped).is_none());
        // A missing owner.
        assert!(decode(&valid, &scope(&MEMBERS), &good_owners[..4]).is_none());
    }
}
