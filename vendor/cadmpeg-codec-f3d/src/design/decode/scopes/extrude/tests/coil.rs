// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::u64_from_index;

use crate::design::decode::scopes::parameter_scope::parse_parameter_scope;
use crate::records::decal::DesignRecordHeader;
use crate::records::feature::extrude::DesignExtrudeExtent;
use crate::records::feature::extrude::DesignExtrudeOperation;
use crate::records::feature::extrude::DesignExtrudePrologue;
use crate::records::feature::extrude::DesignExtrudeStart;
use crate::test_support::lp_utf16;

#[test]
fn extrude_scope_discriminators_follow_optional_indexed_reference() {
    let scope = |kind: &str,
                 operation: u32,
                 direction_face_extend: (u32, u32),
                 direction_reversed: u8,
                 structural_constant: u8,
                 start: u8,
                 reference_padding: Option<usize>,
                 reference_marker: bool,
                 current_side_extents: Option<((u32, u32), bool)>,
                 legacy_side_extents: Option<((u32, u32), bool)>,
                 legacy_reference_count_offset: Option<usize>| {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(b"301");
        bytes.extend_from_slice(&12u32.to_le_bytes());
        bytes.resize(120, 0);
        bytes[20..24].copy_from_slice(&1u32.to_le_bytes());
        let legacy_operation_marker = legacy_side_extents.is_some() && reference_marker;
        let legacy_field_shift = usize::from(legacy_operation_marker);
        let operation_offset = if legacy_side_extents.is_some() {
            27 + legacy_field_shift
        } else if let Some(reference_padding) = reference_padding {
            bytes[25] = 1;
            bytes[26..30].copy_from_slice(&77u32.to_le_bytes());
            30 + reference_padding
        } else {
            28
        };
        if reference_marker {
            if legacy_operation_marker {
                assert_eq!(reference_padding, None);
            } else {
                assert_eq!(reference_padding, Some(8));
            }
            bytes[operation_offset - 1] = 1;
        }
        bytes[operation_offset..operation_offset + 4].copy_from_slice(&operation.to_le_bytes());
        bytes[operation_offset + 4..operation_offset + 8]
            .copy_from_slice(&direction_face_extend.0.to_le_bytes());
        bytes[operation_offset + 8..operation_offset + 12]
            .copy_from_slice(&direction_face_extend.1.to_le_bytes());
        bytes[operation_offset + 12] = direction_reversed;
        bytes[operation_offset + 13] = structural_constant;
        bytes[operation_offset + 14] = start;
        if let Some((side_extents, target_ordinal)) = current_side_extents {
            let profile_normal_offset = operation_offset + 18;
            bytes[profile_normal_offset + 16..profile_normal_offset + 24]
                .copy_from_slice(&1.0f64.to_le_bytes());
            let reference_slots_offset = profile_normal_offset + 24;
            let first_side_extent_offset = if target_ordinal {
                assert_eq!(side_extents.0, 2);
                let target_slot_offset = reference_slots_offset + 6;
                bytes[target_slot_offset] = 1;
                bytes[target_slot_offset + 1..target_slot_offset + 5]
                    .copy_from_slice(&reference_padding.map_or(55, |_| 77_u32).to_le_bytes());
                let target_ordinal_offset = target_slot_offset + 11;
                bytes[target_ordinal_offset..target_ordinal_offset + 4]
                    .copy_from_slice(&0u32.to_le_bytes());
                target_ordinal_offset + 5
            } else {
                reference_slots_offset + 7
            };
            let reference_count_offset = 180;
            bytes.resize(reference_count_offset, 0);
            let second_side_extent_offset = if side_extents.0 == 2 {
                reference_count_offset - 4
            } else {
                first_side_extent_offset + 13
            };
            bytes[first_side_extent_offset..first_side_extent_offset + 4]
                .copy_from_slice(&side_extents.0.to_le_bytes());
            bytes[second_side_extent_offset..second_side_extent_offset + 4]
                .copy_from_slice(&side_extents.1.to_le_bytes());
        }
        if legacy_side_extents.is_some() {
            let reference_count_offset = legacy_reference_count_offset.unwrap_or_else(|| {
                if legacy_side_extents.is_some_and(|(_, widened)| widened)
                    || direction_face_extend.0 == 2
                {
                    272
                } else {
                    252
                }
            }) + legacy_field_shift;
            bytes.resize(reference_count_offset, 0);
        }
        let compact_two_sided =
            legacy_reference_count_offset == Some(283) && direction_face_extend.0 == 2;
        if compact_two_sided {
            for reference_at in [139, 170, 185] {
                let reference_at = reference_at + legacy_field_shift;
                bytes[reference_at] = 1;
                bytes[reference_at + 1..reference_at + 5].copy_from_slice(&55u32.to_le_bytes());
            }
        } else if legacy_side_extents.is_some_and(|(_, widened)| widened)
            || legacy_side_extents.is_some() && direction_face_extend.0 == 2
        {
            for reference_at in [139, 159, 182] {
                let reference_at = reference_at + legacy_field_shift;
                bytes[reference_at] = 1;
                bytes[reference_at + 1..reference_at + 5].copy_from_slice(&55u32.to_le_bytes());
            }
        }
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.push(1);
        bytes.extend_from_slice(&reference_padding.map_or(55, |_| 77_u32).to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
        bytes.extend_from_slice(&7u32.to_le_bytes());
        lp_utf16(&mut bytes, kind);
        let mut tail = [0; 78];
        tail[0..4].copy_from_slice(&1u32.to_le_bytes());
        tail[31..35].copy_from_slice(&2u32.to_le_bytes());
        bytes.extend_from_slice(&tail);
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(b"261");
        bytes.extend_from_slice(&12u32.to_le_bytes());
        if let Some((side_extents, widened)) = legacy_side_extents {
            if widened && direction_face_extend.0 != 2 {
                bytes[106..110].copy_from_slice(&1u32.to_le_bytes());
                bytes[110..114].copy_from_slice(&0u32.to_le_bytes());
            }
            let (first_extent_at, second_extent_at) = if compact_two_sided {
                (166, 181)
            } else if legacy_reference_count_offset == Some(294) {
                (116, 129)
            } else if direction_face_extend.0 == 2 {
                (155, 178)
            } else if widened {
                (116, if side_extents.0 == 2 { 268 } else { 130 })
            } else {
                (106, if side_extents.0 == 2 { 116 } else { 110 })
            };
            let first_extent_at = first_extent_at + legacy_field_shift;
            let second_extent_at = second_extent_at + legacy_field_shift;
            bytes[first_extent_at..first_extent_at + 4]
                .copy_from_slice(&side_extents.0.to_le_bytes());
            bytes[second_extent_at..second_extent_at + 4]
                .copy_from_slice(&side_extents.1.to_le_bytes());
        }
        let header = DesignRecordHeader {
            id: "generated:scope-header#0".into(),
            record_index: 12,
            class_tag: crate::records::references::DesignClassTag::try_from("301".to_owned())
                .unwrap(),
            byte_offset: 0,
        };
        parse_parameter_scope(
            &bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
            header.record_index,
            &header.class_tag,
            header.byte_offset,
        )
        .unwrap()
        .unwrap()
    };

    let direct = scope(
        "Extrude",
        1,
        (1, 2),
        0,
        1,
        0,
        None,
        false,
        Some(((1, 0), false)),
        None,
        None,
    );
    assert_eq!(
        direct.extrude_prologue(),
        Some(DesignExtrudePrologue::ReferenceAware {
            reference: None,
            operation: DesignExtrudeOperation::Join,
            operation_offset: 28,
            direction_face_extend_values: [1, 2],
            side_extent_discriminators: [1, 0],
            side_extent_discriminator_offsets: [77, 90],
            first_side_target_ordinal: None,
            extent: DesignExtrudeExtent::OneSidedDistance,
            direction_face_extend_offsets: [32, 36],
            direction_reversed: false,
            direction_reversed_offset: 40,
            solid_operation: true,
            solid_operation_offset: 41,
            start: DesignExtrudeStart::ProfilePlane,
            start_offset: 42,
        })
    );
    let referenced = scope(
        "Extrude",
        3,
        (2, 0),
        0,
        1,
        1,
        Some(8),
        false,
        Some(((1, 1), false)),
        None,
        None,
    );
    assert_eq!(
        referenced.extrude_prologue(),
        Some(DesignExtrudePrologue::ReferenceAware {
            reference: Some(
                crate::records::feature::extrude::DesignExtrudePrologueReference {
                    record_index: 77,
                    record_index_offset: 26,
                    trailing_zero_count: 8,
                    operation_prefix_marker_offset: None,
                }
            ),
            operation: DesignExtrudeOperation::Intersect,
            operation_offset: 38,
            direction_face_extend_values: [2, 0],
            side_extent_discriminators: [1, 1],
            side_extent_discriminator_offsets: [87, 100],
            first_side_target_ordinal: None,
            extent: DesignExtrudeExtent::TwoSidedDistance,
            direction_face_extend_offsets: [42, 46],
            direction_reversed: false,
            direction_reversed_offset: 50,
            solid_operation: true,
            solid_operation_offset: 51,
            start: DesignExtrudeStart::OffsetProfilePlane,
            start_offset: 52,
        })
    );
    let two_sided_to_faces = scope(
        "Extrude",
        1,
        (2, 1),
        0,
        1,
        0,
        None,
        false,
        Some(((2, 0), false)),
        None,
        None,
    );
    assert_eq!(
        two_sided_to_faces
            .extrude_prologue()
            .and_then(DesignExtrudePrologue::extent),
        Some(DesignExtrudeExtent::TwoSidedToFaces)
    );
    let compact_reference = scope(
        "Extrude",
        2,
        (1, 2),
        0,
        1,
        2,
        Some(7),
        false,
        Some(((1, 0), false)),
        None,
        None,
    );
    let Some(DesignExtrudePrologue::ReferenceAware {
        reference: Some(reference),
        operation_offset,
        ..
    }) = compact_reference.extrude_prologue()
    else {
        panic!("compact referenced Extrude prologue");
    };
    assert_eq!(reference.trailing_zero_count, 7);
    assert_eq!(operation_offset, 37);

    let marked_reference = scope(
        "Extrude",
        1,
        (1, 2),
        0,
        1,
        0,
        Some(8),
        true,
        Some(((1, 0), false)),
        None,
        None,
    );
    let Some(DesignExtrudePrologue::ReferenceAware {
        reference: Some(reference),
        operation_offset,
        ..
    }) = marked_reference.extrude_prologue()
    else {
        panic!("marked indexed-reference Extrude prologue");
    };
    assert_eq!(reference.operation_prefix_marker_offset, Some(37));
    assert_eq!(operation_offset, 38);

    let to_face = scope(
        "Extrusion",
        2,
        (1, 1),
        1,
        1,
        2,
        None,
        false,
        Some(((2, 0), false)),
        None,
        None,
    );
    assert_eq!(
        to_face.kind(),
        crate::records::feature::scope::DesignFeatureKind::Extrusion
    );
    let Some(prologue) = to_face.extrude_prologue() else {
        panic!("to-face Extrude prologue");
    };
    assert_eq!(prologue.extent(), Some(DesignExtrudeExtent::OneSidedToFace));
    assert!(prologue.direction_reversed());
    assert_eq!(prologue.start(), DesignExtrudeStart::FromFace);

    let same_face_extend_blind = scope(
        "Extrude",
        2,
        (1, 1),
        0,
        1,
        0,
        None,
        false,
        Some(((1, 0), false)),
        None,
        None,
    );
    assert_eq!(
        same_face_extend_blind
            .extrude_prologue()
            .and_then(DesignExtrudePrologue::extent),
        Some(DesignExtrudeExtent::OneSidedDistance)
    );
    let same_face_extend_through_all = scope(
        "Extrude",
        2,
        (1, 1),
        0,
        1,
        0,
        None,
        false,
        Some(((4, 0), false)),
        None,
        None,
    );
    assert_eq!(
        same_face_extend_through_all
            .extrude_prologue()
            .and_then(DesignExtrudePrologue::extent),
        Some(DesignExtrudeExtent::OneSidedThroughAll)
    );
    let target_ordinal = scope(
        "Extrude",
        2,
        (1, 1),
        0,
        1,
        0,
        None,
        false,
        Some(((2, 0), true)),
        None,
        None,
    );
    assert!(matches!(
        target_ordinal.extrude_prologue(),
        Some(DesignExtrudePrologue::ReferenceAware {
            side_extent_discriminators: [2, 0],
            side_extent_discriminator_offsets: [92, 176],
            first_side_target_ordinal: Some(
                crate::records::feature::extrude::DesignExtrudeTargetOrdinal {
                    scope_reference_ordinal: 0,
                    scope_reference_ordinal_offset: 87,
                }
            ),
            extent: DesignExtrudeExtent::OneSidedToFace,
            ..
        })
    ));

    let shifted_distance = scope(
        "Extrude",
        4,
        (1, 2),
        0,
        1,
        0,
        None,
        false,
        None,
        Some(((1, 0), false)),
        None,
    );
    assert_eq!(
        shifted_distance
            .extrude_prologue()
            .and_then(DesignExtrudePrologue::extent),
        Some(DesignExtrudeExtent::OneSidedDistance)
    );
    let shifted_symmetric = scope(
        "Extrude",
        4,
        (3, 2),
        0,
        1,
        0,
        None,
        false,
        None,
        Some(((1, 0), true)),
        None,
    );
    assert_eq!(
        shifted_symmetric
            .extrude_prologue()
            .and_then(DesignExtrudePrologue::extent),
        Some(DesignExtrudeExtent::SymmetricDistance)
    );
    assert!(matches!(
        shifted_symmetric.extrude_prologue(),
        Some(DesignExtrudePrologue::LegacyShifted {
            side_extent_discriminator_offsets: [116, 130],
            ..
        })
    ));
    let shifted_compact_symmetric = scope(
        "Extrude",
        4,
        (3, 2),
        0,
        1,
        0,
        None,
        false,
        None,
        Some(((1, 0), true)),
        Some(283),
    );
    assert!(matches!(
        shifted_compact_symmetric.extrude_prologue(),
        Some(DesignExtrudePrologue::LegacyShifted {
            operation_prefix_marker_offset: None,
            side_extent_discriminator_offsets: [116, 130],
            extent: Some(DesignExtrudeExtent::SymmetricDistance),
            ..
        })
    ));
    let shifted_marked_symmetric = scope(
        "Extrude",
        4,
        (3, 2),
        0,
        1,
        0,
        None,
        true,
        None,
        Some(((1, 0), true)),
        Some(283),
    );
    assert!(matches!(
        shifted_marked_symmetric.extrude_prologue(),
        Some(DesignExtrudePrologue::LegacyShifted {
            operation_prefix_marker_offset: Some(27),
            operation: DesignExtrudeOperation::NewBody,
            operation_offset: 28,
            direction_face_extend_values: [3, 2],
            side_extent_discriminator_offsets: [117, 131],
            extent: Some(DesignExtrudeExtent::SymmetricDistance),
            direction_face_extend_offsets: [32, 36],
            direction_reversed_offset: 40,
            solid_operation_offset: 41,
            start_offset: 42,
            ..
        })
    ));
    let shifted_marked_symmetric_tail_276 = scope(
        "Extrude",
        2,
        (3, 2),
        0,
        1,
        0,
        None,
        true,
        None,
        Some(((1, 0), true)),
        Some(276),
    );
    assert!(matches!(
        shifted_marked_symmetric_tail_276.extrude_prologue(),
        Some(DesignExtrudePrologue::LegacyShifted {
            operation_prefix_marker_offset: Some(27),
            operation: DesignExtrudeOperation::Cut,
            side_extent_discriminators: [1, 0],
            side_extent_discriminator_offsets: [117, 131],
            extent: Some(DesignExtrudeExtent::SymmetricDistance),
            ..
        })
    ));
    for refused_offset in [275, 277] {
        let refused = scope(
            "Extrude",
            2,
            (3, 2),
            0,
            1,
            0,
            None,
            true,
            None,
            Some(((1, 0), true)),
            Some(refused_offset),
        );
        assert_eq!(refused.extrude_prologue(), None);
    }
    let marked_symmetric_second_side_present = scope(
        "Extrude",
        2,
        (3, 2),
        0,
        1,
        0,
        None,
        true,
        None,
        Some(((1, 1), true)),
        Some(276),
    );
    assert_eq!(
        marked_symmetric_second_side_present.extrude_prologue(),
        None
    );
    let shifted_offset_profile = scope(
        "Extrude",
        2,
        (1, 2),
        0,
        1,
        1,
        None,
        false,
        None,
        Some(((1, 0), true)),
        Some(262),
    );
    assert!(matches!(
        shifted_offset_profile.extrude_prologue(),
        Some(DesignExtrudePrologue::LegacyShifted {
            operation_prefix_marker_offset: None,
            operation: DesignExtrudeOperation::Cut,
            operation_offset: 27,
            side_extent_discriminator_offsets: [116, 130],
            extent: Some(DesignExtrudeExtent::OneSidedDistance),
            start: DesignExtrudeStart::OffsetProfilePlane,
            start_offset: 41,
            ..
        })
    ));
    let shifted_two_sided = scope(
        "Extrude",
        2,
        (2, 0),
        0,
        1,
        0,
        None,
        false,
        None,
        Some(((1, 1), false)),
        None,
    );
    assert_eq!(
        shifted_two_sided
            .extrude_prologue()
            .and_then(DesignExtrudePrologue::extent),
        Some(DesignExtrudeExtent::TwoSidedDistance)
    );
    let shifted_compact_two_sided = scope(
        "Extrude",
        2,
        (2, 0),
        0,
        1,
        0,
        None,
        false,
        None,
        Some(((1, 1), false)),
        Some(283),
    );
    assert!(matches!(
        shifted_compact_two_sided.extrude_prologue(),
        Some(DesignExtrudePrologue::LegacyShifted {
            side_extent_discriminator_offsets: [166, 181],
            extent: Some(DesignExtrudeExtent::TwoSidedDistance),
            ..
        })
    ));

    let shifted_through_all = scope(
        "Extrude",
        2,
        (1, 0),
        1,
        1,
        0,
        None,
        false,
        None,
        Some(((4, 0), false)),
        None,
    );
    assert_eq!(
        shifted_through_all
            .extrude_prologue()
            .and_then(DesignExtrudePrologue::extent),
        Some(DesignExtrudeExtent::OneSidedThroughAll)
    );
    let shifted_to_face = scope(
        "Extrude",
        2,
        (1, 1),
        1,
        1,
        0,
        None,
        false,
        None,
        Some(((2, 0), true)),
        None,
    );
    assert_eq!(
        shifted_to_face
            .extrude_prologue()
            .and_then(DesignExtrudePrologue::extent),
        Some(DesignExtrudeExtent::OneSidedToFace)
    );
    assert!(matches!(
        shifted_to_face.extrude_prologue(),
        Some(DesignExtrudePrologue::LegacyShifted {
            side_extent_discriminator_offsets: [116, 268],
            ..
        })
    ));

    for reference_count_offset in [262, 263] {
        let shifted_compact_to_face = scope(
            "Extrude",
            2,
            (1, 1),
            1,
            1,
            0,
            None,
            false,
            None,
            Some(((2, 0), false)),
            Some(reference_count_offset),
        );
        assert!(matches!(
            shifted_compact_to_face.extrude_prologue(),
            Some(DesignExtrudePrologue::LegacyShifted {
                extent: Some(DesignExtrudeExtent::OneSidedToFace),
                side_extent_discriminator_offsets: [106, offset],
                ..
            }) if offset == u64::try_from(reference_count_offset - 4).unwrap()
        ));
    }

    let shifted_symmetric_through_all = scope(
        "Extrude",
        2,
        (3, 0),
        0,
        1,
        0,
        None,
        false,
        None,
        Some(((4, 4), true)),
        Some(294),
    );
    assert!(matches!(
        shifted_symmetric_through_all.extrude_prologue(),
        Some(DesignExtrudePrologue::LegacyShifted {
            extent: Some(DesignExtrudeExtent::SymmetricThroughAll),
            side_extent_discriminator_offsets: [116, 129],
            ..
        })
    ));

    let invalid_absent_first_side = scope(
        "Extrude",
        2,
        (3, 0),
        0,
        1,
        0,
        None,
        false,
        None,
        Some(((0, 0), false)),
        None,
    );
    assert_eq!(invalid_absent_first_side.extrude_prologue(), None);

    let contradictory_direction_and_sides = scope(
        "Extrude",
        2,
        (2, 0),
        0,
        1,
        0,
        None,
        false,
        Some(((1, 0), false)),
        None,
        None,
    );
    assert_eq!(contradictory_direction_and_sides.extrude_prologue(), None);

    let unrecognized = scope("Extrude", 2, (3, 0), 0, 1, 0, None, false, None, None, None);
    assert_eq!(
        unrecognized.kind(),
        crate::records::feature::scope::DesignFeatureKind::Extrude
    );
    assert_eq!(unrecognized.extrude_prologue(), None);
    assert_eq!(
        scope(
            "Extrude",
            2,
            (3, 0),
            2,
            1,
            0,
            None,
            false,
            Some(((1, 0), false)),
            None,
            None,
        )
        .extrude_prologue(),
        None
    );
    let sheet = scope(
        "Extrude",
        2,
        (3, 0),
        0,
        0,
        0,
        None,
        false,
        Some(((1, 0), false)),
        None,
        None,
    )
    .extrude_prologue()
    .expect("sheet Extrude prologue");
    assert!(!sheet.solid_operation());
    assert_eq!(
        scope(
            "Extrude",
            2,
            (3, 0),
            0,
            1,
            3,
            None,
            false,
            Some(((1, 0), false)),
            None,
            None,
        )
        .extrude_prologue(),
        None
    );
}

#[test]
fn legacy_distance_extrude_scope_decodes_nullable_prefix_forms() {
    let scope = |prefix_present: bool, operation: u32, geometry_kind: u32| {
        let reference_count_offset = if prefix_present { 212 } else { 208 };
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(b"376");
        bytes.extend_from_slice(&12u32.to_le_bytes());
        bytes.resize(reference_count_offset, 0);
        let operation_offset = if prefix_present {
            bytes[20] = 1;
            bytes[21..25].copy_from_slice(&0u32.to_le_bytes());
            25
        } else {
            21
        };
        bytes[operation_offset..operation_offset + 4].copy_from_slice(&operation.to_le_bytes());
        bytes[operation_offset + 4..operation_offset + 8].copy_from_slice(&2u32.to_le_bytes());
        bytes[operation_offset + 8] = 1;
        bytes[operation_offset + 9..operation_offset + 13]
            .copy_from_slice(&geometry_kind.to_le_bytes());
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.push(1);
        bytes.extend_from_slice(&55u32.to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
        bytes.extend_from_slice(&7u32.to_le_bytes());
        lp_utf16(&mut bytes, "Extrude");
        let mut tail = [0; 78];
        tail[0..4].copy_from_slice(&1u32.to_le_bytes());
        tail[31..35].copy_from_slice(&2u32.to_le_bytes());
        bytes.extend_from_slice(&tail);
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(b"261");
        bytes.extend_from_slice(&12u32.to_le_bytes());
        let header = DesignRecordHeader {
            id: "generated:scope-header#0".into(),
            record_index: 12,
            class_tag: crate::records::references::DesignClassTag::try_from("376".to_owned())
                .unwrap(),
            byte_offset: 0,
        };
        parse_parameter_scope(
            &bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
            header.record_index,
            &header.class_tag,
            header.byte_offset,
        )
        .unwrap()
        .unwrap()
    };

    assert_eq!(
        scope(false, 1, 1).extrude_prologue(),
        Some(DesignExtrudePrologue::LegacyDistance {
            prefix_zero_offset: None,
            operation: DesignExtrudeOperation::Join,
            operation_offset: 21,
            extent_kind_offset: 25,
            direction_reversed: true,
            direction_reversed_offset: 29,
            solid_operation: true,
            solid_operation_offset: 30,
        })
    );
    assert_eq!(
        scope(true, 4, 0).extrude_prologue(),
        Some(DesignExtrudePrologue::LegacyDistance {
            prefix_zero_offset: Some(21),
            operation: DesignExtrudeOperation::NewBody,
            operation_offset: 25,
            extent_kind_offset: 29,
            direction_reversed: true,
            direction_reversed_offset: 33,
            solid_operation: false,
            solid_operation_offset: 34,
        })
    );
}

#[test]
fn compact_shifted_extrude_scope_decodes_one_sided_distance() {
    const REFERENCE_COUNT_OFFSET: usize = 251;
    const FIRST_SIDE_EXTENT_OFFSET: usize = 105;
    const SECOND_SIDE_EXTENT_OFFSET: usize = 109;
    const OPERATION_OFFSET: usize = 26;

    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"304");
    bytes.extend_from_slice(&12u32.to_le_bytes());
    bytes.resize(REFERENCE_COUNT_OFFSET, 0);
    bytes[20..24].copy_from_slice(&1u32.to_le_bytes());
    bytes[OPERATION_OFFSET..OPERATION_OFFSET + 4].copy_from_slice(&4u32.to_le_bytes());
    bytes[OPERATION_OFFSET + 4..OPERATION_OFFSET + 8].copy_from_slice(&1u32.to_le_bytes());
    bytes[OPERATION_OFFSET + 8..OPERATION_OFFSET + 12].copy_from_slice(&2u32.to_le_bytes());
    bytes[OPERATION_OFFSET + 12] = 0;
    bytes[OPERATION_OFFSET + 13] = 1;
    bytes[OPERATION_OFFSET + 14] = 0;
    bytes[FIRST_SIDE_EXTENT_OFFSET..FIRST_SIDE_EXTENT_OFFSET + 4]
        .copy_from_slice(&1u32.to_le_bytes());
    bytes[SECOND_SIDE_EXTENT_OFFSET..SECOND_SIDE_EXTENT_OFFSET + 4]
        .copy_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&55u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    bytes.extend_from_slice(&7u32.to_le_bytes());
    lp_utf16(&mut bytes, "Extrude");
    let mut tail = [0; 78];
    tail[0..4].copy_from_slice(&1u32.to_le_bytes());
    tail[31..35].copy_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&tail);
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"261");
    bytes.extend_from_slice(&12u32.to_le_bytes());

    let header = DesignRecordHeader {
        id: "generated:scope-header#0".into(),
        record_index: 12,
        class_tag: crate::records::references::DesignClassTag::try_from("304".to_owned()).unwrap(),
        byte_offset: 0,
    };
    let scope = parse_parameter_scope(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        header.record_index,
        &header.class_tag,
        header.byte_offset,
    )
    .unwrap()
    .expect("compact shifted Extrude scope");
    assert_eq!(
        scope.reference_count_offset(),
        u64_from_index(REFERENCE_COUNT_OFFSET)
    );
    assert_eq!(
        scope.extrude_prologue(),
        Some(DesignExtrudePrologue::LegacyShifted {
            operation_prefix_marker_offset: None,
            operation: DesignExtrudeOperation::NewBody,
            operation_offset: u64_from_index(OPERATION_OFFSET),
            direction_face_extend_values: [1, 2],
            side_extent_discriminators: [1, 0],
            side_extent_discriminator_offsets: [
                u64_from_index(FIRST_SIDE_EXTENT_OFFSET),
                u64_from_index(SECOND_SIDE_EXTENT_OFFSET),
            ],
            extent: Some(DesignExtrudeExtent::OneSidedDistance),
            direction_face_extend_offsets: [30, 34],
            direction_reversed: false,
            direction_reversed_offset: 38,
            solid_operation: true,
            solid_operation_offset: 39,
            start: DesignExtrudeStart::ProfilePlane,
            start_offset: 40,
        })
    );
}

#[test]
fn compact_shifted_extrude_scope_decodes_mixed_distance_to_face() {
    const REFERENCE_COUNT_OFFSET: usize = 281;
    const FIRST_SIDE_EXTENT_OFFSET: usize = 124;
    const SECOND_SIDE_EXTENT_OFFSET: usize = 128;
    const OPERATION_OFFSET: usize = 26;
    let reference_members: [u32; 11] = [
        3225, 3228, 3234, 3237, 3730, 3733, 3737, 3740, 3744, 3747, 3752,
    ];

    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"304");
    bytes.extend_from_slice(&12u32.to_le_bytes());
    bytes.resize(REFERENCE_COUNT_OFFSET, 0);
    bytes[20..24].copy_from_slice(&1u32.to_le_bytes());
    bytes[OPERATION_OFFSET..OPERATION_OFFSET + 4].copy_from_slice(&1u32.to_le_bytes());
    bytes[OPERATION_OFFSET + 4..OPERATION_OFFSET + 8].copy_from_slice(&2u32.to_le_bytes());
    bytes[OPERATION_OFFSET + 8..OPERATION_OFFSET + 12].copy_from_slice(&0u32.to_le_bytes());
    bytes[OPERATION_OFFSET + 12] = 0;
    bytes[OPERATION_OFFSET + 13] = 1;
    bytes[OPERATION_OFFSET + 14] = 0;
    bytes[FIRST_SIDE_EXTENT_OFFSET..FIRST_SIDE_EXTENT_OFFSET + 4]
        .copy_from_slice(&1u32.to_le_bytes());
    bytes[SECOND_SIDE_EXTENT_OFFSET..SECOND_SIDE_EXTENT_OFFSET + 4]
        .copy_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(
        &(u32::try_from(reference_members.len()).expect("fixture value fits u32")).to_le_bytes(),
    );
    for reference in reference_members {
        bytes.push(1);
        bytes.extend_from_slice(&reference.to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
    }
    bytes.extend_from_slice(&7u32.to_le_bytes());
    lp_utf16(&mut bytes, "Extrude");
    let mut tail = [0; 78];
    tail[0..4].copy_from_slice(&1u32.to_le_bytes());
    tail[31..35].copy_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&tail);
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"261");
    bytes.extend_from_slice(&12u32.to_le_bytes());

    let header = DesignRecordHeader {
        id: "generated:scope-header#0".into(),
        record_index: 12,
        class_tag: crate::records::references::DesignClassTag::try_from("304".to_owned()).unwrap(),
        byte_offset: 0,
    };
    let scope = parse_parameter_scope(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        header.record_index,
        &header.class_tag,
        header.byte_offset,
    )
    .unwrap()
    .expect("compact mixed Extrude scope");
    assert_eq!(
        scope.reference_count_offset(),
        u64_from_index(REFERENCE_COUNT_OFFSET)
    );
    assert_eq!(
        scope
            .reference_members()
            .values()
            .copied()
            .collect::<Vec<_>>(),
        reference_members
    );
    assert_eq!(
        scope.extrude_prologue(),
        Some(DesignExtrudePrologue::LegacyShifted {
            operation_prefix_marker_offset: None,
            operation: DesignExtrudeOperation::Join,
            operation_offset: u64_from_index(OPERATION_OFFSET),
            direction_face_extend_values: [2, 0],
            side_extent_discriminators: [1, 2],
            side_extent_discriminator_offsets: [
                u64_from_index(FIRST_SIDE_EXTENT_OFFSET),
                u64_from_index(SECOND_SIDE_EXTENT_OFFSET),
            ],
            extent: Some(DesignExtrudeExtent::TwoSidedDistanceToFace),
            direction_face_extend_offsets: [30, 34],
            direction_reversed: false,
            direction_reversed_offset: 38,
            solid_operation: true,
            solid_operation_offset: 39,
            start: DesignExtrudeStart::ProfilePlane,
            start_offset: 40,
        })
    );
}

#[test]
fn legacy_class_415_symmetric_distance_scope_decodes_both_frame_lengths() {
    use crate::layout::legacy_class_415_symmetric_extrude_prefix as layout;

    const RECORD_INDEX: u32 = 4150;
    const REFERENCE_MEMBERS_5: [u32; 5] = [4151, 4152, 4153, 4154, 4155];
    const REFERENCE_MEMBERS_7: [u32; 7] = [4151, 4152, 4153, 4154, 4155, 4156, 4157];

    let make_bytes = |reference_members: &[u32], operation: u32| {
        let frame_length = match reference_members.len() {
            5 => 447,
            7 => 469,
            count => panic!("unsupported synthetic reference count {count}"),
        };
        let mut bytes = vec![0; layout::REFERENCE_COUNT + 4];
        bytes[0..4].copy_from_slice(&3u32.to_le_bytes());
        bytes[4..7].copy_from_slice(b"415");
        bytes[7..11].copy_from_slice(&RECORD_INDEX.to_le_bytes());
        bytes[layout::PREFIX_CONSTANT..layout::PREFIX_CONSTANT + 4]
            .copy_from_slice(&layout::PREFIX_CONSTANT_VALUE.to_le_bytes());
        bytes[layout::OPERATION..layout::OPERATION + 4].copy_from_slice(&operation.to_le_bytes());
        bytes[layout::DIRECTION..layout::DIRECTION + 4]
            .copy_from_slice(&layout::DIRECTION_VALUE.to_le_bytes());
        bytes[layout::FACE_EXTEND..layout::FACE_EXTEND + 4]
            .copy_from_slice(&layout::FACE_EXTEND_VALUE.to_le_bytes());
        bytes[layout::OPERATION_PREFIX_MARKER] = layout::OPERATION_PREFIX_MARKER_VALUE;
        bytes[layout::GEOMETRY_KIND] = 1;
        bytes[layout::PROFILE_NORMAL + 8..layout::PROFILE_NORMAL + 16]
            .copy_from_slice(&1.0f64.to_le_bytes());

        let put_reference = |bytes: &mut [u8], offset: usize, record_index: u32| {
            bytes[offset] = 1;
            bytes[offset + 1..offset + 5].copy_from_slice(&record_index.to_le_bytes());
        };
        for (offset, record_index) in [
            (layout::REFERENCE_SLOTS + 1, reference_members[0]),
            (layout::REFERENCE_SLOTS + 12, reference_members[1]),
            (layout::REFERENCE_SLOTS + 23, reference_members[2]),
            (layout::REFERENCE_SLOTS + 35, reference_members[3]),
        ] {
            put_reference(&mut bytes, offset, record_index);
        }
        bytes[layout::FIRST_SIDE_EXTENT..layout::FIRST_SIDE_EXTENT + 4]
            .copy_from_slice(&layout::FIRST_SIDE_EXTENT_VALUE.to_le_bytes());
        bytes[layout::SECOND_SIDE_EXTENT..layout::SECOND_SIDE_EXTENT + 4]
            .copy_from_slice(&layout::SECOND_SIDE_EXTENT_VALUE.to_le_bytes());
        bytes[layout::REFERENCE_COUNT..layout::REFERENCE_COUNT + 4].copy_from_slice(
            &(u32::try_from(reference_members.len()).expect("fixture value fits u32"))
                .to_le_bytes(),
        );
        for record_index in reference_members {
            let offset = bytes.len();
            bytes.resize(offset + 11, 0);
            put_reference(&mut bytes, offset, *record_index);
        }
        bytes.extend_from_slice(&1u32.to_le_bytes());
        lp_utf16(&mut bytes, "Extrude");
        let mut tail = [0; 78];
        tail[0..4].copy_from_slice(&1u32.to_le_bytes());
        tail[31..35].copy_from_slice(&2u32.to_le_bytes());
        bytes.extend_from_slice(&tail);
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(b"265");
        bytes.extend_from_slice(&RECORD_INDEX.to_le_bytes());
        assert_eq!(bytes.len(), frame_length + 11);
        bytes
    };

    let parse = |bytes: &[u8], class_tag: &str| {
        let header = DesignRecordHeader {
            id: "generated:scope-header#0".into(),
            record_index: RECORD_INDEX,
            class_tag: crate::records::references::DesignClassTag::try_from(class_tag.to_owned())
                .unwrap(),
            byte_offset: 0,
        };
        parse_parameter_scope(
            bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(bytes),
            header.record_index,
            &header.class_tag,
            header.byte_offset,
        )
        .unwrap()
        .expect("class-415 scope envelope")
    };

    for (reference_members, frame_length, operation) in [
        (
            &REFERENCE_MEMBERS_5[..],
            447_u64,
            DesignExtrudeOperation::Join,
        ),
        (
            &REFERENCE_MEMBERS_7[..],
            469_u64,
            DesignExtrudeOperation::NewBody,
        ),
    ] {
        let bytes = make_bytes(
            reference_members,
            match operation {
                DesignExtrudeOperation::Join => 1,
                DesignExtrudeOperation::NewBody => 4,
                DesignExtrudeOperation::Cut | DesignExtrudeOperation::Intersect => unreachable!(),
            },
        );
        let scope = parse(&bytes, "415");
        assert_eq!(scope.frame_length(), frame_length);
        assert_eq!(
            scope.reference_count_offset(),
            u64_from_index(layout::REFERENCE_COUNT)
        );
        assert_eq!(
            scope.extrude_prologue(),
            Some(DesignExtrudePrologue::ReferenceAware {
                reference: None,
                operation,
                operation_offset: u64_from_index(layout::OPERATION),
                direction_face_extend_values: [3, 2],
                side_extent_discriminators: [1, 1],
                side_extent_discriminator_offsets: [
                    u64_from_index(layout::FIRST_SIDE_EXTENT),
                    u64_from_index(layout::SECOND_SIDE_EXTENT),
                ],
                first_side_target_ordinal: None,
                extent: DesignExtrudeExtent::SymmetricDistance,
                direction_face_extend_offsets: [
                    u64_from_index(layout::DIRECTION),
                    u64_from_index(layout::FACE_EXTEND),
                ],
                direction_reversed: false,
                direction_reversed_offset: u64_from_index(layout::DIRECTION_REVERSED),
                solid_operation: true,
                solid_operation_offset: u64_from_index(layout::GEOMETRY_KIND),
                start: DesignExtrudeStart::ProfilePlane,
                start_offset: u64_from_index(layout::START_SUPPORT),
            })
        );
    }

    let valid = make_bytes(&REFERENCE_MEMBERS_5, 1);
    let mut invalid_marker = valid.clone();
    invalid_marker[layout::OPERATION_PREFIX_MARKER] = 0;
    assert!(parse(&invalid_marker, "415").extrude_prologue().is_none());

    let mut invalid_class = valid.clone();
    // The current reference-aware form admits dual-lane symmetric extents
    // under any stream-local class tag; only the legacy class-415 layout
    // keys on its tags.
    assert_eq!(
        parse(&invalid_class, "414")
            .extrude_prologue()
            .map(crate::records::feature::extrude::DesignExtrudePrologue::extent),
        Some(Some(DesignExtrudeExtent::SymmetricDistance))
    );
    invalid_class[447 + 4..447 + 7].copy_from_slice(b"264");
    invalid_class[layout::DIRECTION..layout::DIRECTION + 4].copy_from_slice(&0u32.to_le_bytes());
    assert!(parse(&invalid_class, "415").extrude_prologue().is_none());

    let mut invalid_slots = valid;
    invalid_slots[layout::REFERENCE_SLOTS + 1] = 0;
    assert!(parse(&invalid_slots, "415").extrude_prologue().is_none());
}

#[test]
fn legacy_class_415_one_sided_scope_decodes_distinct_extent_lanes() {
    use crate::layout::legacy_class_415_one_sided_distance_extrude_prefix as distance_layout;
    use crate::layout::legacy_class_415_one_sided_to_face_extrude_prefix as to_face_layout;

    const RECORD_INDEX: u32 = 8415;
    const TO_FACE_REFERENCES: [u32; 9] = [8416, 8417, 8418, 8419, 8420, 8421, 8422, 8423, 8424];
    const DISTANCE_REFERENCES: [u32; 7] = [8416, 8417, 8418, 8419, 8420, 8421, 8422];

    let make_bytes = |to_face: bool, references: &[u32]| {
        let (frame_length, reference_count_offset, face_extend, first_side_extent) = if to_face {
            (
                481,
                to_face_layout::REFERENCE_COUNT,
                to_face_layout::FACE_EXTEND_VALUE,
                to_face_layout::FIRST_SIDE_EXTENT_VALUE,
            )
        } else {
            (
                449,
                distance_layout::REFERENCE_COUNT,
                distance_layout::FACE_EXTEND_VALUE,
                distance_layout::FIRST_SIDE_EXTENT_VALUE,
            )
        };
        let mut bytes = vec![0; reference_count_offset + 4];
        bytes[0..4].copy_from_slice(&3u32.to_le_bytes());
        bytes[4..7].copy_from_slice(b"415");
        bytes[7..11].copy_from_slice(&RECORD_INDEX.to_le_bytes());
        bytes[to_face_layout::PREFIX_CONSTANT..to_face_layout::PREFIX_CONSTANT + 4]
            .copy_from_slice(&to_face_layout::PREFIX_CONSTANT_VALUE.to_le_bytes());
        bytes[to_face_layout::OPERATION_PREFIX_MARKER] =
            to_face_layout::OPERATION_PREFIX_MARKER_VALUE;
        bytes[to_face_layout::OPERATION..to_face_layout::OPERATION + 4]
            .copy_from_slice(&2u32.to_le_bytes());
        bytes[to_face_layout::DIRECTION..to_face_layout::DIRECTION + 4]
            .copy_from_slice(&to_face_layout::DIRECTION_VALUE.to_le_bytes());
        bytes[to_face_layout::FACE_EXTEND..to_face_layout::FACE_EXTEND + 4]
            .copy_from_slice(&face_extend.to_le_bytes());
        bytes[to_face_layout::DIRECTION_REVERSED] = u8::from(to_face);
        bytes[to_face_layout::GEOMETRY_KIND] = 1;
        bytes[to_face_layout::PROFILE_NORMAL + 8..to_face_layout::PROFILE_NORMAL + 16]
            .copy_from_slice(&1.0f64.to_le_bytes());
        bytes[to_face_layout::FIRST_SIDE_EXTENT..to_face_layout::FIRST_SIDE_EXTENT + 4]
            .copy_from_slice(&first_side_extent.to_le_bytes());
        if to_face {
            bytes[to_face_layout::FIRST_SIDE_OFFSET_REFERENCE] = 1;
            bytes[to_face_layout::FIRST_SIDE_OFFSET_REFERENCE + 1
                ..to_face_layout::FIRST_SIDE_OFFSET_REFERENCE + 5]
                .copy_from_slice(&references[0].to_le_bytes());
        }
        let second_side_extent = reference_count_offset - 4;
        bytes[second_side_extent..second_side_extent + 4].copy_from_slice(&0u32.to_le_bytes());
        bytes[reference_count_offset..reference_count_offset + 4].copy_from_slice(
            &(u32::try_from(references.len()).expect("fixture value fits u32")).to_le_bytes(),
        );
        for reference in references {
            bytes.push(1);
            bytes.extend_from_slice(&reference.to_le_bytes());
            bytes.extend_from_slice(&[0; 6]);
        }
        bytes.extend_from_slice(&1u32.to_le_bytes());
        lp_utf16(&mut bytes, "Extrude");
        let mut tail = [0; 78];
        tail[0..4].copy_from_slice(&1u32.to_le_bytes());
        tail[31..35].copy_from_slice(&2u32.to_le_bytes());
        bytes.extend_from_slice(&tail);
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(b"265");
        bytes.extend_from_slice(&RECORD_INDEX.to_le_bytes());
        assert_eq!(bytes.len(), frame_length + 11);
        bytes
    };

    let parse = |bytes: &[u8]| {
        let header = DesignRecordHeader {
            id: "generated:scope-header#0".into(),
            record_index: RECORD_INDEX,
            class_tag: crate::records::references::DesignClassTag::try_from("415".to_owned())
                .unwrap(),
            byte_offset: 0,
        };
        parse_parameter_scope(
            bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(bytes),
            header.record_index,
            &header.class_tag,
            header.byte_offset,
        )
        .unwrap()
        .expect("class-415 one-sided scope envelope")
    };

    let to_face = parse(&make_bytes(true, &TO_FACE_REFERENCES));
    assert_eq!(to_face.frame_length(), 481);
    assert_eq!(to_face.reference_count_offset(), 278);
    assert_eq!(
        to_face
            .reference_members()
            .values()
            .copied()
            .collect::<Vec<_>>(),
        TO_FACE_REFERENCES
    );
    let Some(DesignExtrudePrologue::ReferenceAware {
        operation,
        direction_face_extend_values,
        side_extent_discriminators,
        side_extent_discriminator_offsets,
        extent,
        direction_reversed,
        ..
    }) = to_face.extrude_prologue()
    else {
        panic!("class-415 one-sided to-face prologue");
    };
    assert_eq!(operation, DesignExtrudeOperation::Cut);
    assert_eq!(direction_face_extend_values, [1, 1]);
    assert_eq!(side_extent_discriminators, [2, 0]);
    assert_eq!(side_extent_discriminator_offsets, [107, 274]);
    assert_eq!(extent, DesignExtrudeExtent::OneSidedToFace);
    assert!(direction_reversed);

    let distance = parse(&make_bytes(false, &DISTANCE_REFERENCES));
    assert_eq!(distance.frame_length(), 449);
    assert_eq!(distance.reference_count_offset(), 268);
    assert_eq!(
        distance
            .reference_members()
            .values()
            .copied()
            .collect::<Vec<_>>(),
        DISTANCE_REFERENCES
    );
    let Some(DesignExtrudePrologue::ReferenceAware {
        direction_face_extend_values,
        side_extent_discriminators,
        side_extent_discriminator_offsets,
        extent,
        direction_reversed,
        ..
    }) = distance.extrude_prologue()
    else {
        panic!("class-415 one-sided distance prologue");
    };
    assert_eq!(direction_face_extend_values, [1, 2]);
    assert_eq!(side_extent_discriminators, [1, 0]);
    assert_eq!(side_extent_discriminator_offsets, [107, 264]);
    assert_eq!(extent, DesignExtrudeExtent::OneSidedDistance);
    assert!(!direction_reversed);

    let mut invalid_extent = make_bytes(false, &DISTANCE_REFERENCES);
    invalid_extent[distance_layout::SECOND_SIDE_EXTENT..distance_layout::SECOND_SIDE_EXTENT + 4]
        .copy_from_slice(&1u32.to_le_bytes());
    assert!(parse(&invalid_extent).extrude_prologue().is_none());

    let mut invalid_paired_class = make_bytes(true, &TO_FACE_REFERENCES);
    invalid_paired_class[481 + 4..481 + 7].copy_from_slice(b"264");
    assert!(parse(&invalid_paired_class).extrude_prologue().is_none());
}

#[test]
fn shifted_reference_aware_extrude_scope_decodes_538_byte_face_targets() {
    const REFERENCE_COUNT_OFFSET: usize = 292;
    const FRAME_LENGTH: usize = 538;
    const RECORD_INDEX: u32 = 12;
    const REFERENCE_MEMBERS: [u32; 13] = [11, 22, 33, 44, 55, 66, 77, 88, 99, 111, 122, 133, 144];

    let make_bytes =
        |primary_class: &[u8; 3], paired_class: &[u8; 3], first_side_discriminant: u32| {
            let mut bytes = vec![0; REFERENCE_COUNT_OFFSET];
            bytes[0..4].copy_from_slice(&3u32.to_le_bytes());
            bytes[4..7].copy_from_slice(primary_class);
            bytes[7..11].copy_from_slice(&RECORD_INDEX.to_le_bytes());
            bytes[20..24].copy_from_slice(&1u32.to_le_bytes());
            bytes[27..31].copy_from_slice(&1u32.to_le_bytes());
            bytes[31..35].copy_from_slice(&2u32.to_le_bytes());
            bytes[35..39].copy_from_slice(&1u32.to_le_bytes());
            bytes[39] = 0;
            bytes[40] = 1;
            bytes[41] = 0;
            bytes[61..69].copy_from_slice(&1.0f64.to_le_bytes());

            let put_reference = |bytes: &mut [u8], offset: usize, record_index: u32| {
                bytes[offset] = 1;
                bytes[offset + 1..offset + 5].copy_from_slice(&record_index.to_le_bytes());
            };
            for (offset, record_index) in [(72, 55), (83, 66), (94, 22), (105, 111)] {
                put_reference(&mut bytes, offset, record_index);
            }
            bytes[116..120].copy_from_slice(&2u32.to_le_bytes());
            put_reference(&mut bytes, 120, 11);
            bytes[135..139].copy_from_slice(&1u32.to_le_bytes());
            bytes[139..143].copy_from_slice(&first_side_discriminant.to_le_bytes());
            put_reference(&mut bytes, 144, 33);
            put_reference(&mut bytes, 159, 44);
            bytes[175..179].copy_from_slice(&1u32.to_le_bytes());
            put_reference(&mut bytes, 179, 88);
            bytes[198..202].copy_from_slice(&1u32.to_le_bytes());
            put_reference(&mut bytes, 202, 66);
            let mut guid = Vec::new();
            lp_utf16(&mut guid, "00000000-0000-0000-0000-000000000000");
            bytes[213..289].copy_from_slice(&guid);

            bytes.extend_from_slice(
                &(u32::try_from(REFERENCE_MEMBERS.len()).expect("fixture value fits u32"))
                    .to_le_bytes(),
            );
            for record_index in REFERENCE_MEMBERS {
                let offset = bytes.len();
                bytes.resize(offset + 11, 0);
                put_reference(&mut bytes, offset, record_index);
            }
            bytes.extend_from_slice(&5u32.to_le_bytes());
            lp_utf16(&mut bytes, "Extrude");
            let mut tail = [0; 77];
            tail[0..4].copy_from_slice(&1u32.to_le_bytes());
            tail[31..35].copy_from_slice(&2u32.to_le_bytes());
            bytes.extend_from_slice(&tail);
            bytes.extend_from_slice(&3u32.to_le_bytes());
            bytes.extend_from_slice(paired_class);
            bytes.extend_from_slice(&RECORD_INDEX.to_le_bytes());
            assert_eq!(bytes.len(), FRAME_LENGTH + 11);
            bytes
        };

    let parse = |bytes: &[u8], class_tag: &str| {
        let header = DesignRecordHeader {
            id: "generated:scope-header#0".into(),
            record_index: RECORD_INDEX,
            class_tag: crate::records::references::DesignClassTag::try_from(class_tag.to_owned())
                .unwrap(),
            byte_offset: 0,
        };
        parse_parameter_scope(
            bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(bytes),
            header.record_index,
            &header.class_tag,
            header.byte_offset,
        )
        .unwrap()
        .expect("shifted reference-aware Extrude scope")
    };

    for (class_tag, primary_class, paired_class) in [
        ("357", b"357", b"258"),
        ("349", b"349", b"266"),
        ("397", b"397", b"262"),
    ] {
        let scope = parse(&make_bytes(primary_class, paired_class, 2), class_tag);
        assert_eq!(scope.frame_length(), u64_from_index(FRAME_LENGTH));
        assert_eq!(
            scope.reference_count_offset(),
            u64_from_index(REFERENCE_COUNT_OFFSET)
        );
        assert_eq!(
            scope.extrude_prologue(),
            Some(DesignExtrudePrologue::ShiftedReferenceAware {
                operation: DesignExtrudeOperation::Join,
                operation_offset: 27,
                direction_face_extend_values: [2, 1],
                side_extent_discriminators: [2, 0],
                side_extent_discriminator_offsets: [116, 288],
                extent: DesignExtrudeExtent::TwoSidedToFaces,
                direction_face_extend_offsets: [31, 35],
                direction_reversed: false,
                direction_reversed_offset: 39,
                solid_operation: true,
                solid_operation_offset: 40,
                start: DesignExtrudeStart::ProfilePlane,
                start_offset: 41,
            })
        );
    }

    let mut invalid_class_397 = make_bytes(b"397", b"262", 2);
    invalid_class_397[135..139].copy_from_slice(&2u32.to_le_bytes());
    let invalid_scope = parse_parameter_scope(
        &invalid_class_397,
        &crate::design::test_support::indexed_record_offsets_for_test(&invalid_class_397),
        RECORD_INDEX,
        &crate::records::references::DesignClassTag::try_from("397".to_owned()).unwrap(),
        0,
    )
    .unwrap()
    .expect("class-397 scope envelope remains parseable");
    assert!(invalid_scope.extrude_prologue().is_none());

    let mut invalid_tail = make_bytes(b"357", b"258", 2);
    invalid_tail[135..139].copy_from_slice(&0u32.to_le_bytes());
    let header = DesignRecordHeader {
        id: "generated:scope-header#0".into(),
        record_index: RECORD_INDEX,
        class_tag: crate::records::references::DesignClassTag::try_from("357".to_owned()).unwrap(),
        byte_offset: 0,
    };
    let invalid_scope = parse_parameter_scope(
        &invalid_tail,
        &crate::design::test_support::indexed_record_offsets_for_test(&invalid_tail),
        header.record_index,
        &header.class_tag,
        header.byte_offset,
    )
    .unwrap()
    .expect("scope envelope remains parseable");
    assert!(invalid_scope.extrude_prologue().is_none());

    let mut invalid_class = make_bytes(b"349", b"266", 2);
    invalid_class[FRAME_LENGTH + 4..FRAME_LENGTH + 7].copy_from_slice(b"259");
    let invalid_scope = parse_parameter_scope(
        &invalid_class,
        &crate::design::test_support::indexed_record_offsets_for_test(&invalid_class),
        RECORD_INDEX,
        &crate::records::references::DesignClassTag::try_from("349".to_owned()).unwrap(),
        0,
    )
    .unwrap()
    .expect("scope envelope remains parseable");
    assert!(invalid_scope.extrude_prologue().is_none());

    let prefix_length = 17;
    let mut nonzero_start = vec![0; prefix_length];
    nonzero_start.extend_from_slice(&make_bytes(b"349", b"266", 2));
    let nonzero_header = DesignRecordHeader {
        id: "generated:scope-header#nonzero".into(),
        record_index: RECORD_INDEX,
        class_tag: crate::records::references::DesignClassTag::try_from("349".to_owned()).unwrap(),
        byte_offset: u64_from_index(prefix_length),
    };
    let nonzero_scope = parse_parameter_scope(
        &nonzero_start,
        &crate::design::test_support::indexed_record_offsets_for_test(&nonzero_start),
        nonzero_header.record_index,
        &nonzero_header.class_tag,
        nonzero_header.byte_offset,
    )
    .unwrap()
    .expect("nonzero-start shifted reference-aware Extrude scope");
    assert_eq!(nonzero_scope.byte_offset(), u64_from_index(prefix_length));
    assert_eq!(
        nonzero_scope.reference_count_offset(),
        u64_from_index(prefix_length + REFERENCE_COUNT_OFFSET)
    );
    assert!(nonzero_scope.extrude_prologue().is_some());
}

#[test]
fn shifted_reference_aware_extrude_scope_decodes_516_byte_class_323_face_targets() {
    use crate::layout::shifted_reference_aware_extrude_class_323_tail as class_323_tail;
    use crate::layout::shifted_reference_aware_extrude_scope_prefix as layout;

    const FRAME_LENGTH: usize = 516;
    const RECORD_INDEX: u32 = 1535;
    const REFERENCE_MEMBERS: [u32; 11] = [11, 22, 33, 44, 55, 66, 77, 88, 99, 111, 122];

    let put_reference = |bytes: &mut [u8], offset: usize, record_index: u32| {
        bytes[offset] = 1;
        bytes[offset + 1..offset + 5].copy_from_slice(&record_index.to_le_bytes());
    };
    let mut bytes = vec![0; layout::REFERENCE_COUNT];
    bytes[0..4].copy_from_slice(&3u32.to_le_bytes());
    bytes[4..7].copy_from_slice(b"323");
    bytes[7..11].copy_from_slice(&RECORD_INDEX.to_le_bytes());
    bytes[layout::PREFIX_CONSTANT..layout::PREFIX_CONSTANT + 4]
        .copy_from_slice(&1u32.to_le_bytes());
    bytes[layout::OPERATION..layout::OPERATION + 4].copy_from_slice(&4u32.to_le_bytes());
    bytes[layout::DIRECTION..layout::DIRECTION + 4].copy_from_slice(&2u32.to_le_bytes());
    bytes[layout::FACE_EXTEND..layout::FACE_EXTEND + 4].copy_from_slice(&1u32.to_le_bytes());
    bytes[layout::GEOMETRY_KIND] = 1;
    for (component, value) in [0.0_f64, 0.0, 1.0].into_iter().enumerate() {
        let offset = layout::PROFILE_NORMAL + component * std::mem::size_of::<f64>();
        bytes[offset..offset + std::mem::size_of::<f64>()].copy_from_slice(&value.to_le_bytes());
    }
    for (slot, record_index) in [55, 66, 22, 111].into_iter().enumerate() {
        put_reference(
            &mut bytes,
            layout::REFERENCE_SLOTS + 3 + slot * 11,
            record_index,
        );
    }
    bytes[layout::FIRST_SIDE_EXTENT..layout::FIRST_SIDE_EXTENT + 4]
        .copy_from_slice(&2u32.to_le_bytes());
    put_reference(
        &mut bytes,
        layout::FIRST_SIDE_OWNER_REFERENCE,
        REFERENCE_MEMBERS[0],
    );
    bytes[layout::FIRST_SIDE_DISCRIMINANT..layout::FIRST_SIDE_DISCRIMINANT + 4]
        .copy_from_slice(&1u32.to_le_bytes());
    bytes[layout::FIRST_SIDE_PAYLOAD..layout::FIRST_SIDE_PAYLOAD + 4]
        .copy_from_slice(&2u32.to_le_bytes());
    put_reference(
        &mut bytes,
        layout::SECOND_SIDE_OFFSET_REFERENCE,
        REFERENCE_MEMBERS[2],
    );
    put_reference(
        &mut bytes,
        layout::SECOND_SIDE_TAPER_REFERENCE,
        REFERENCE_MEMBERS[3],
    );
    bytes[layout::PROFILE_GROUP_COUNT..layout::PROFILE_GROUP_COUNT + 4]
        .copy_from_slice(&1u32.to_le_bytes());
    put_reference(
        &mut bytes,
        layout::PROFILE_GROUP_REFERENCE,
        REFERENCE_MEMBERS[7],
    );
    bytes[class_323_tail::TRAILING_REFERENCE_COUNT..class_323_tail::TRAILING_REFERENCE_COUNT + 4]
        .copy_from_slice(&1u32.to_le_bytes());
    put_reference(&mut bytes, class_323_tail::TRAILING_REFERENCE, 9001);
    let mut guid = Vec::new();
    lp_utf16(&mut guid, "00000000-0000-0000-0000-000000000000");
    let guid_end = layout::SECOND_SIDE_EXTENT + 1;
    assert_eq!(guid.len(), guid_end - layout::BODY_GROUP_GUID_PREFIX);
    bytes[layout::BODY_GROUP_GUID_PREFIX..guid_end].copy_from_slice(&guid);

    bytes.extend_from_slice(
        &(u32::try_from(REFERENCE_MEMBERS.len()).expect("fixture value fits u32")).to_le_bytes(),
    );
    for record_index in REFERENCE_MEMBERS {
        let offset = bytes.len();
        bytes.resize(offset + 11, 0);
        put_reference(&mut bytes, offset, record_index);
    }
    bytes.extend_from_slice(&5u32.to_le_bytes());
    lp_utf16(&mut bytes, "Extrude");
    let mut tail = [0; 77];
    tail[0..4].copy_from_slice(&1u32.to_le_bytes());
    tail[31..35].copy_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&tail);
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"263");
    bytes.extend_from_slice(&RECORD_INDEX.to_le_bytes());
    assert_eq!(bytes.len(), FRAME_LENGTH + 11);

    let header = DesignRecordHeader {
        id: "generated:scope-header#0".into(),
        record_index: RECORD_INDEX,
        class_tag: crate::records::references::DesignClassTag::try_from("323".to_owned()).unwrap(),
        byte_offset: 0,
    };
    let scope = parse_parameter_scope(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        header.record_index,
        &header.class_tag,
        header.byte_offset,
    )
    .unwrap()
    .expect("shifted reference-aware class-323 Extrude scope");
    assert_eq!(scope.frame_length(), u64_from_index(FRAME_LENGTH));
    assert_eq!(
        scope.reference_count_offset(),
        u64_from_index(layout::REFERENCE_COUNT)
    );
    assert_eq!(
        scope.extrude_prologue(),
        Some(DesignExtrudePrologue::ShiftedReferenceAware {
            operation: DesignExtrudeOperation::NewBody,
            operation_offset: u64_from_index(layout::OPERATION),
            direction_face_extend_values: [2, 1],
            side_extent_discriminators: [2, 0],
            side_extent_discriminator_offsets: [
                u64_from_index(layout::FIRST_SIDE_EXTENT),
                u64_from_index(layout::SECOND_SIDE_EXTENT),
            ],
            extent: DesignExtrudeExtent::TwoSidedToFaces,
            direction_face_extend_offsets: [
                u64_from_index(layout::DIRECTION),
                u64_from_index(layout::FACE_EXTEND),
            ],
            direction_reversed: false,
            direction_reversed_offset: u64_from_index(layout::DIRECTION_REVERSED),
            solid_operation: true,
            solid_operation_offset: u64_from_index(layout::GEOMETRY_KIND),
            start: DesignExtrudeStart::ProfilePlane,
            start_offset: u64_from_index(layout::START_SUPPORT),
        })
    );

    let mut invalid_trailing_reference = bytes.clone();
    invalid_trailing_reference
        [class_323_tail::TRAILING_REFERENCE + 1..class_323_tail::TRAILING_REFERENCE + 5]
        .copy_from_slice(&REFERENCE_MEMBERS[5].to_le_bytes());
    let invalid_scope = parse_parameter_scope(
        &invalid_trailing_reference,
        &crate::design::test_support::indexed_record_offsets_for_test(&invalid_trailing_reference),
        header.record_index,
        &header.class_tag,
        header.byte_offset,
    )
    .unwrap()
    .expect("scope envelope remains parseable");
    assert!(invalid_scope.extrude_prologue().is_none());

    let mut invalid_class = bytes;
    invalid_class[FRAME_LENGTH + 4..FRAME_LENGTH + 7].copy_from_slice(b"259");
    let invalid_scope = parse_parameter_scope(
        &invalid_class,
        &crate::design::test_support::indexed_record_offsets_for_test(&invalid_class),
        header.record_index,
        &header.class_tag,
        header.byte_offset,
    )
    .unwrap()
    .expect("scope envelope remains parseable");
    assert!(invalid_scope.extrude_prologue().is_none());
}

#[test]
fn shifted_reference_aware_extrude_scope_decodes_485_byte_class_323_symmetric_through_all() {
    use crate::layout::shifted_reference_aware_extrude_class_323_symmetric_prefix as symmetric;
    use crate::layout::shifted_reference_aware_extrude_scope_prefix as layout;

    const FRAME_LENGTH: usize = 485;
    const RECORD_INDEX: u32 = 1535;
    const REFERENCE_MEMBERS: [u32; 10] = [11, 22, 33, 44, 55, 66, 77, 88, 99, 111];

    let put_reference = |bytes: &mut [u8], offset: usize, record_index: u32| {
        bytes[offset] = 1;
        bytes[offset + 1..offset + 5].copy_from_slice(&record_index.to_le_bytes());
    };
    let mut bytes = vec![0; symmetric::REFERENCE_COUNT];
    bytes[0..4].copy_from_slice(&3u32.to_le_bytes());
    bytes[4..7].copy_from_slice(b"323");
    bytes[7..11].copy_from_slice(&RECORD_INDEX.to_le_bytes());
    bytes[layout::PREFIX_CONSTANT..layout::PREFIX_CONSTANT + 4]
        .copy_from_slice(&1u32.to_le_bytes());
    bytes[layout::OPERATION..layout::OPERATION + 4].copy_from_slice(&2u32.to_le_bytes());
    bytes[layout::DIRECTION..layout::DIRECTION + 4].copy_from_slice(&3u32.to_le_bytes());
    bytes[layout::FACE_EXTEND..layout::FACE_EXTEND + 4].copy_from_slice(&0u32.to_le_bytes());
    bytes[layout::GEOMETRY_KIND] = 1;
    for (component, value) in [0.0_f64, 0.0, 1.0].into_iter().enumerate() {
        let offset = layout::PROFILE_NORMAL + component * std::mem::size_of::<f64>();
        bytes[offset..offset + std::mem::size_of::<f64>()].copy_from_slice(&value.to_le_bytes());
    }
    for (slot, record_index) in [55, 66, 22, 111].into_iter().enumerate() {
        put_reference(
            &mut bytes,
            layout::REFERENCE_SLOTS + 3 + slot * 11,
            record_index,
        );
    }
    bytes[symmetric::FIRST_SIDE_EXTENT..symmetric::FIRST_SIDE_EXTENT + 4]
        .copy_from_slice(&symmetric::FIRST_SIDE_EXTENT_VALUE.to_le_bytes());
    bytes[symmetric::SECOND_SIDE_EXTENT..symmetric::SECOND_SIDE_EXTENT + 4]
        .copy_from_slice(&symmetric::SECOND_SIDE_EXTENT_VALUE.to_le_bytes());
    put_reference(
        &mut bytes,
        symmetric::SYMMETRIC_EXTENT_REFERENCE,
        REFERENCE_MEMBERS[0],
    );
    bytes[symmetric::PROFILE_GROUP_COUNT..symmetric::PROFILE_GROUP_COUNT + 4]
        .copy_from_slice(&symmetric::PROFILE_GROUP_COUNT_VALUE.to_le_bytes());
    put_reference(
        &mut bytes,
        symmetric::PROFILE_GROUP_REFERENCE,
        REFERENCE_MEMBERS[2],
    );
    bytes[symmetric::TRAILING_REFERENCE_COUNT..symmetric::TRAILING_REFERENCE_COUNT + 4]
        .copy_from_slice(&symmetric::TRAILING_REFERENCE_COUNT_VALUE.to_le_bytes());
    put_reference(
        &mut bytes,
        symmetric::TRAILING_REFERENCE,
        REFERENCE_MEMBERS[3],
    );
    let mut guid = Vec::new();
    lp_utf16(&mut guid, "00000000-0000-0000-0000-000000000000");
    assert_eq!(
        guid.len(),
        symmetric::REFERENCE_COUNT_PADDING - symmetric::GUID_PREFIX
    );
    bytes[symmetric::GUID_PREFIX..symmetric::REFERENCE_COUNT_PADDING].copy_from_slice(&guid);

    bytes.extend_from_slice(
        &(u32::try_from(REFERENCE_MEMBERS.len()).expect("fixture value fits u32")).to_le_bytes(),
    );
    for record_index in REFERENCE_MEMBERS {
        let offset = bytes.len();
        bytes.resize(offset + 11, 0);
        put_reference(&mut bytes, offset, record_index);
    }
    bytes.extend_from_slice(&5u32.to_le_bytes());
    lp_utf16(&mut bytes, "Extrude");
    let mut tail = [0; 77];
    tail[0..4].copy_from_slice(&1u32.to_le_bytes());
    tail[31..35].copy_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&tail);
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"263");
    bytes.extend_from_slice(&RECORD_INDEX.to_le_bytes());
    assert_eq!(bytes.len(), FRAME_LENGTH + 11);

    let header = DesignRecordHeader {
        id: "generated:scope-header#0".into(),
        record_index: RECORD_INDEX,
        class_tag: crate::records::references::DesignClassTag::try_from("323".to_owned()).unwrap(),
        byte_offset: 0,
    };
    let parse = |bytes: &[u8]| {
        parse_parameter_scope(
            bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(bytes),
            header.record_index,
            &header.class_tag,
            header.byte_offset,
        )
        .unwrap()
        .expect("shifted reference-aware symmetric Extrude scope")
    };
    let scope = parse(&bytes);
    assert_eq!(scope.frame_length(), u64_from_index(FRAME_LENGTH));
    assert_eq!(
        scope.reference_count_offset(),
        u64_from_index(symmetric::REFERENCE_COUNT)
    );
    assert_eq!(
        scope.extrude_prologue(),
        Some(DesignExtrudePrologue::ShiftedReferenceAware {
            operation: DesignExtrudeOperation::Cut,
            operation_offset: u64_from_index(layout::OPERATION),
            direction_face_extend_values: [3, 0],
            side_extent_discriminators: [4, 4],
            side_extent_discriminator_offsets: [
                u64_from_index(symmetric::FIRST_SIDE_EXTENT),
                u64_from_index(symmetric::SECOND_SIDE_EXTENT),
            ],
            extent: DesignExtrudeExtent::SymmetricThroughAll,
            direction_face_extend_offsets: [
                u64_from_index(layout::DIRECTION),
                u64_from_index(layout::FACE_EXTEND)
            ],
            direction_reversed: false,
            direction_reversed_offset: u64_from_index(layout::DIRECTION_REVERSED),
            solid_operation: true,
            solid_operation_offset: u64_from_index(layout::GEOMETRY_KIND),
            start: DesignExtrudeStart::ProfilePlane,
            start_offset: u64_from_index(layout::START_SUPPORT),
        })
    );

    let mut invalid_extent = bytes.clone();
    invalid_extent[symmetric::SECOND_SIDE_EXTENT..symmetric::SECOND_SIDE_EXTENT + 4]
        .copy_from_slice(&3u32.to_le_bytes());
    assert!(parse(&invalid_extent).extrude_prologue().is_none());

    let mut invalid_trailing_reference = bytes.clone();
    invalid_trailing_reference
        [symmetric::TRAILING_REFERENCE + 1..symmetric::TRAILING_REFERENCE + 5]
        .copy_from_slice(&9001u32.to_le_bytes());
    assert!(parse(&invalid_trailing_reference)
        .extrude_prologue()
        .is_none());

    let mut invalid_class = bytes;
    invalid_class[FRAME_LENGTH + 4..FRAME_LENGTH + 7].copy_from_slice(b"259");
    assert!(parse(&invalid_class).extrude_prologue().is_none());
}
