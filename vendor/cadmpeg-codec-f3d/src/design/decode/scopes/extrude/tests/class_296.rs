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
fn class_296_one_sided_to_face_extrude_scope_requires_exact_frame_shape() {
    use crate::layout::class_296_261_one_sided_to_face_extrude_prefix as layout;

    const RECORD_INDEX: u32 = 296_261;

    let make_bytes = |frame_length: usize, reference_count: usize| {
        let mut bytes = vec![0; layout::REFERENCE_COUNT + 4];
        bytes[0..4].copy_from_slice(&3u32.to_le_bytes());
        bytes[4..7].copy_from_slice(b"296");
        bytes[7..11].copy_from_slice(&RECORD_INDEX.to_le_bytes());
        bytes[layout::PREFIX_CONSTANT..layout::PREFIX_CONSTANT + 4]
            .copy_from_slice(&1u32.to_le_bytes());
        bytes[layout::OPERATION..layout::OPERATION + 4].copy_from_slice(&2u32.to_le_bytes());
        bytes[layout::DIRECTION..layout::DIRECTION + 4].copy_from_slice(&1u32.to_le_bytes());
        bytes[layout::FACE_EXTEND..layout::FACE_EXTEND + 4].copy_from_slice(&2u32.to_le_bytes());
        bytes[layout::DIRECTION_REVERSED] = 1;
        bytes[layout::GEOMETRY_KIND] = 1;
        bytes[layout::START_SUPPORT] = 0;
        bytes[layout::FIRST_SIDE_EXTENT..layout::FIRST_SIDE_EXTENT + 4]
            .copy_from_slice(&2u32.to_le_bytes());
        bytes[layout::SECOND_SIDE_EXTENT..layout::SECOND_SIDE_EXTENT + 4]
            .copy_from_slice(&0u32.to_le_bytes());
        bytes[layout::REFERENCE_COUNT..layout::REFERENCE_COUNT + 4].copy_from_slice(
            &(u32::try_from(reference_count).expect("fixture value fits u32")).to_le_bytes(),
        );
        for reference in 0..reference_count {
            bytes.push(1);
            bytes.extend_from_slice(
                &(RECORD_INDEX + 1 + u32::try_from(reference).expect("fixture value fits u32"))
                    .to_le_bytes(),
            );
            bytes.extend_from_slice(&[0; 6]);
        }
        bytes.extend_from_slice(&1u32.to_le_bytes());
        lp_utf16(&mut bytes, "Extrude");
        let mut tail = [0; 76];
        tail[0..4].copy_from_slice(&1u32.to_le_bytes());
        tail[30..34].copy_from_slice(&2u32.to_le_bytes());
        bytes.extend_from_slice(&tail);
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(b"261");
        bytes.extend_from_slice(&RECORD_INDEX.to_le_bytes());
        assert_eq!(bytes.len(), frame_length + 11);
        bytes
    };

    let parse_raw = |bytes: &[u8], class_tag: &str| {
        parse_parameter_scope(
            bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(bytes),
            RECORD_INDEX,
            &crate::records::references::DesignClassTag::try_from(class_tag.to_owned()).unwrap(),
            0,
        )
        .unwrap()
    };
    let parse = |bytes: &[u8], class_tag: &str| {
        parse_raw(bytes, class_tag).expect("class-296 one-sided-to-face scope envelope")
    };

    for (frame_length, reference_count) in [(440, 7), (462, 9), (473, 10)] {
        let scope = parse(&make_bytes(frame_length, reference_count), "296");
        assert_eq!(scope.frame_length(), u64_from_index(frame_length));
        assert_eq!(
            scope.reference_count_offset(),
            u64_from_index(layout::REFERENCE_COUNT)
        );
        assert_eq!(
            scope.extrude_prologue(),
            Some(DesignExtrudePrologue::LegacyShifted {
                operation_prefix_marker_offset: None,
                operation: DesignExtrudeOperation::Cut,
                operation_offset: u64_from_index(layout::OPERATION),
                direction_face_extend_values: [1, 2],
                side_extent_discriminators: [2, 0],
                side_extent_discriminator_offsets: [
                    u64_from_index(layout::FIRST_SIDE_EXTENT),
                    u64_from_index(layout::SECOND_SIDE_EXTENT),
                ],
                extent: Some(DesignExtrudeExtent::OneSidedToFace),
                direction_face_extend_offsets: [
                    u64_from_index(layout::DIRECTION),
                    u64_from_index(layout::FACE_EXTEND),
                ],
                direction_reversed: true,
                direction_reversed_offset: u64_from_index(layout::DIRECTION_REVERSED),
                solid_operation: true,
                solid_operation_offset: u64_from_index(layout::GEOMETRY_KIND),
                start: DesignExtrudeStart::ProfilePlane,
                start_offset: u64_from_index(layout::START_SUPPORT),
            })
        );
    }

    let mut invalid_member_count = make_bytes(440, 7);
    invalid_member_count[layout::REFERENCE_COUNT..layout::REFERENCE_COUNT + 4]
        .copy_from_slice(&8u32.to_le_bytes());
    assert!(parse_raw(&invalid_member_count, "296").is_none());

    let mut invalid_side = make_bytes(462, 9);
    invalid_side[layout::FIRST_SIDE_EXTENT..layout::FIRST_SIDE_EXTENT + 4]
        .copy_from_slice(&1u32.to_le_bytes());
    assert!(parse(&invalid_side, "296").extrude_prologue().is_none());

    let mut invalid_pair = make_bytes(473, 10);
    invalid_pair[473 + 4..473 + 7].copy_from_slice(b"260");
    assert!(parse(&invalid_pair, "296").extrude_prologue().is_none());

    assert!(parse(&make_bytes(462, 9), "414")
        .extrude_prologue()
        .is_none());
}

#[test]
fn class_296_symmetric_distance_extrude_scope_requires_exact_frame_shape() {
    use crate::layout::class_296_261_symmetric_distance_extrude_prefix as layout;

    const RECORD_INDEX: u32 = 296_450;
    const REFERENCE_MEMBERS: [u32; 7] = [
        296_451, 296_454, 296_457, 296_460, 296_463, 296_466, 296_469,
    ];

    let mut bytes = vec![0; layout::REFERENCE_COUNT + 4];
    bytes[0..4].copy_from_slice(&3u32.to_le_bytes());
    bytes[4..7].copy_from_slice(b"296");
    bytes[7..11].copy_from_slice(&RECORD_INDEX.to_le_bytes());
    bytes[layout::PREFIX_CONSTANT..layout::PREFIX_CONSTANT + 4]
        .copy_from_slice(&1u32.to_le_bytes());
    bytes[layout::OPERATION..layout::OPERATION + 4].copy_from_slice(&2u32.to_le_bytes());
    bytes[layout::DIRECTION..layout::DIRECTION + 4].copy_from_slice(&3u32.to_le_bytes());
    bytes[layout::FACE_EXTEND..layout::FACE_EXTEND + 4].copy_from_slice(&2u32.to_le_bytes());
    bytes[layout::DIRECTION_REVERSED] = 0;
    bytes[layout::GEOMETRY_KIND] = 1;
    bytes[layout::START_SUPPORT] = 0;
    bytes[layout::FIRST_SIDE_EXTENT..layout::FIRST_SIDE_EXTENT + 4]
        .copy_from_slice(&1u32.to_le_bytes());
    bytes[layout::SECOND_SIDE_EXTENT..layout::SECOND_SIDE_EXTENT + 4]
        .copy_from_slice(&0u32.to_le_bytes());
    bytes[layout::REFERENCE_COUNT..layout::REFERENCE_COUNT + 4].copy_from_slice(
        &(u32::try_from(REFERENCE_MEMBERS.len()).expect("fixture value fits u32")).to_le_bytes(),
    );
    for reference in REFERENCE_MEMBERS {
        bytes.push(1);
        bytes.extend_from_slice(&reference.to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
    }
    bytes.extend_from_slice(&1u32.to_le_bytes());
    lp_utf16(&mut bytes, "Extrude");
    let mut tail = [0; 76];
    tail[0..4].copy_from_slice(&1u32.to_le_bytes());
    tail[30..34].copy_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&tail);
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"261");
    bytes.extend_from_slice(&RECORD_INDEX.to_le_bytes());
    assert_eq!(bytes.len(), 450 + 11);

    let header = DesignRecordHeader {
        id: "generated:scope-header#class-296-symmetric".into(),
        record_index: RECORD_INDEX,
        class_tag: crate::records::references::DesignClassTag::try_from("296".to_owned()).unwrap(),
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
        .expect("class-296 symmetric-distance scope envelope")
    };
    let scope = parse(&bytes);
    assert_eq!(scope.frame_length(), 450);
    assert_eq!(
        scope.reference_count_offset(),
        u64_from_index(layout::REFERENCE_COUNT)
    );
    assert_eq!(
        scope
            .reference_members()
            .values()
            .copied()
            .collect::<Vec<_>>(),
        REFERENCE_MEMBERS
    );
    assert_eq!(
        scope.extrude_prologue(),
        Some(DesignExtrudePrologue::LegacyShifted {
            operation_prefix_marker_offset: None,
            operation: DesignExtrudeOperation::Cut,
            operation_offset: u64_from_index(layout::OPERATION),
            direction_face_extend_values: [3, 2],
            side_extent_discriminators: [1, 0],
            side_extent_discriminator_offsets: [
                u64_from_index(layout::FIRST_SIDE_EXTENT),
                u64_from_index(layout::SECOND_SIDE_EXTENT),
            ],
            extent: Some(DesignExtrudeExtent::SymmetricDistance),
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

    let mut invalid_extent = bytes.clone();
    invalid_extent[layout::FIRST_SIDE_EXTENT..layout::FIRST_SIDE_EXTENT + 4]
        .copy_from_slice(&2u32.to_le_bytes());
    assert!(parse(&invalid_extent).extrude_prologue().is_none());

    let mut invalid_direction = bytes.clone();
    invalid_direction[layout::DIRECTION..layout::DIRECTION + 4]
        .copy_from_slice(&1u32.to_le_bytes());
    assert!(parse(&invalid_direction).extrude_prologue().is_none());

    let mut invalid_pair = bytes;
    invalid_pair[450 + 4..450 + 7].copy_from_slice(b"260");
    assert!(parse(&invalid_pair).extrude_prologue().is_none());
}

#[test]
fn class_296_two_sided_to_faces_extrude_scope_requires_exact_frame_shape() {
    use crate::layout::class_296_261_two_sided_to_faces_extrude_prefix as layout;

    const RECORD_INDEX: u32 = 296_536;
    const REFERENCE_MEMBERS: [u32; 13] = [
        296_501, 296_504, 296_507, 296_510, 296_513, 296_516, 296_519, 296_522, 296_525, 296_528,
        296_531, 296_534, 296_537,
    ];
    const SLOT_REFERENCES: [u32; 4] = [296_513, 296_516, 296_519, 296_522];

    let mut bytes = vec![0; layout::REFERENCE_COUNT + 4];
    bytes[0..4].copy_from_slice(&3u32.to_le_bytes());
    bytes[4..7].copy_from_slice(b"296");
    bytes[7..11].copy_from_slice(&RECORD_INDEX.to_le_bytes());
    bytes[layout::PREFIX_CONSTANT..layout::PREFIX_CONSTANT + 4]
        .copy_from_slice(&1u32.to_le_bytes());
    bytes[layout::OPERATION..layout::OPERATION + 4].copy_from_slice(&1u32.to_le_bytes());
    bytes[layout::DIRECTION..layout::DIRECTION + 4].copy_from_slice(&2u32.to_le_bytes());
    bytes[layout::FACE_EXTEND..layout::FACE_EXTEND + 4].copy_from_slice(&2u32.to_le_bytes());
    bytes[layout::DIRECTION_REVERSED] = 0;
    bytes[layout::GEOMETRY_KIND] = 1;
    bytes[layout::START_SUPPORT] = 0;
    bytes[layout::PROFILE_NORMAL..layout::PROFILE_NORMAL + 8]
        .copy_from_slice(&1.0f64.to_le_bytes());
    bytes[layout::PROFILE_NORMAL + 8..layout::PROFILE_NORMAL + 16]
        .copy_from_slice(&0.0f64.to_le_bytes());
    bytes[layout::PROFILE_NORMAL + 16..layout::PROFILE_NORMAL + 24]
        .copy_from_slice(&0.0f64.to_le_bytes());
    let mut slot_offset = layout::REFERENCE_SLOTS;
    for (slot_ordinal, expected_present) in [false, false, false, true, true, true, true]
        .into_iter()
        .enumerate()
    {
        if expected_present {
            bytes[slot_offset] = 1;
            bytes[slot_offset + 1..slot_offset + 5]
                .copy_from_slice(&SLOT_REFERENCES[slot_ordinal - 3].to_le_bytes());
            slot_offset += 11;
        } else {
            slot_offset += 1;
        }
    }
    assert_eq!(slot_offset, layout::FIRST_SIDE_EXTENT);
    bytes[layout::FIRST_SIDE_EXTENT..layout::FIRST_SIDE_EXTENT + 4]
        .copy_from_slice(&2u32.to_le_bytes());
    bytes[layout::SECOND_SIDE_EXTENT..layout::SECOND_SIDE_EXTENT + 4]
        .copy_from_slice(&0u32.to_le_bytes());
    bytes[layout::REFERENCE_COUNT..layout::REFERENCE_COUNT + 4].copy_from_slice(
        &(u32::try_from(REFERENCE_MEMBERS.len()).expect("fixture value fits u32")).to_le_bytes(),
    );
    for reference in REFERENCE_MEMBERS {
        bytes.push(1);
        bytes.extend_from_slice(&reference.to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
    }
    bytes.extend_from_slice(&1u32.to_le_bytes());
    lp_utf16(&mut bytes, "Extrude");
    let mut tail = [0; 76];
    tail[0..4].copy_from_slice(&1u32.to_le_bytes());
    tail[30..34].copy_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&tail);
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"261");
    bytes.extend_from_slice(&RECORD_INDEX.to_le_bytes());
    assert_eq!(bytes.len(), 536 + 11);

    let header = DesignRecordHeader {
        id: "generated:scope-header#class-296-two-sided-to-faces".into(),
        record_index: RECORD_INDEX,
        class_tag: crate::records::references::DesignClassTag::try_from("296".to_owned()).unwrap(),
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
        .expect("class-296 two-sided-to-faces scope envelope")
    };
    let scope = parse(&bytes);
    assert_eq!(scope.frame_length(), 536);
    assert_eq!(
        scope.reference_count_offset(),
        u64_from_index(layout::REFERENCE_COUNT)
    );
    assert_eq!(
        scope
            .reference_members()
            .values()
            .copied()
            .collect::<Vec<_>>(),
        REFERENCE_MEMBERS
    );
    assert_eq!(
        scope.extrude_prologue(),
        Some(DesignExtrudePrologue::LegacyShifted {
            operation_prefix_marker_offset: None,
            operation: DesignExtrudeOperation::Join,
            operation_offset: u64_from_index(layout::OPERATION),
            direction_face_extend_values: [2, 2],
            side_extent_discriminators: [2, 0],
            side_extent_discriminator_offsets: [
                u64_from_index(layout::FIRST_SIDE_EXTENT),
                u64_from_index(layout::SECOND_SIDE_EXTENT),
            ],
            extent: Some(DesignExtrudeExtent::TwoSidedToFaces),
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

    let mut alternate_face_extend = bytes.clone();
    alternate_face_extend[layout::FACE_EXTEND..layout::FACE_EXTEND + 4]
        .copy_from_slice(&1u32.to_le_bytes());
    assert!(matches!(
        parse(&alternate_face_extend).extrude_prologue(),
        Some(DesignExtrudePrologue::LegacyShifted {
            direction_face_extend_values: [2, 1],
            extent: Some(DesignExtrudeExtent::TwoSidedToFaces),
            ..
        })
    ));

    let mut invalid_slot = bytes.clone();
    invalid_slot[layout::REFERENCE_SLOTS] = 1;
    assert!(parse(&invalid_slot).extrude_prologue().is_none());

    let mut invalid_profile_normal = bytes.clone();
    invalid_profile_normal[layout::PROFILE_NORMAL..layout::PROFILE_NORMAL + 8]
        .copy_from_slice(&2.0f64.to_le_bytes());
    assert!(parse(&invalid_profile_normal).extrude_prologue().is_none());

    let mut invalid_pair = bytes;
    invalid_pair[536 + 4..536 + 7].copy_from_slice(b"260");
    assert!(parse(&invalid_pair).extrude_prologue().is_none());
}

#[test]
fn class_296_legacy_one_sided_extrude_scopes_require_exact_frame_shape() {
    use crate::layout::class_296_261_legacy_extrude_prefix_scalar_at_54 as scalar_54;
    use crate::layout::class_296_261_legacy_extrude_prefix_scalar_at_70 as scalar_70;
    use crate::layout::class_296_261_legacy_one_sided_distance_tail as distance_tail;
    use crate::layout::class_296_261_legacy_one_sided_to_face_tail as to_face_tail;

    const RECORD_INDEX: u32 = 296_515;
    const TO_FACE_REFERENCES: [u32; 12] = [
        296_501, 296_504, 296_507, 296_510, 296_513, 296_516, 296_519, 296_522, 296_525, 296_528,
        296_531, 296_534,
    ];
    const DISTANCE_REFERENCES: [u32; 10] = [
        296_601, 296_604, 296_607, 296_610, 296_613, 296_616, 296_619, 296_622, 296_625, 296_628,
    ];

    let build = |frame_length: usize,
                 reference_count_offset: usize,
                 reference_members: &[u32],
                 slot_presence: [bool; 7],
                 slot_references: [u32; 4],
                 face_extend: u32,
                 first_extent: u32,
                 scalar_offset: usize,
                 scalar_value: f64,
                 direction_reversed: u8| {
        let mut bytes = vec![0; reference_count_offset + 4];
        bytes[0..4].copy_from_slice(&3u32.to_le_bytes());
        bytes[4..7].copy_from_slice(b"296");
        bytes[7..11].copy_from_slice(&RECORD_INDEX.to_le_bytes());
        bytes[scalar_54::PREFIX_CONSTANT..scalar_54::PREFIX_CONSTANT + 4]
            .copy_from_slice(&1u32.to_le_bytes());
        bytes[scalar_54::REFERENCE] = 1;
        bytes[scalar_54::REFERENCE + 1..scalar_54::REFERENCE + 5]
            .copy_from_slice(&reference_members[0].to_le_bytes());
        bytes[scalar_54::OPERATION..scalar_54::OPERATION + 4].copy_from_slice(&2u32.to_le_bytes());
        bytes[scalar_54::DIRECTION..scalar_54::DIRECTION + 4].copy_from_slice(&1u32.to_le_bytes());
        bytes[scalar_54::FACE_EXTEND..scalar_54::FACE_EXTEND + 4]
            .copy_from_slice(&face_extend.to_le_bytes());
        bytes[scalar_54::DIRECTION_REVERSED] = direction_reversed;
        bytes[scalar_54::GEOMETRY_KIND] = 1;
        bytes[scalar_54::START_SUPPORT] = 2;
        bytes[scalar_54::PROFILE_SCALAR_AT_54..scalar_54::PROFILE_SCALAR_AT_54 + 8]
            .copy_from_slice(&0.0f64.to_le_bytes());
        bytes[scalar_70::PROFILE_SCALAR_AT_70..scalar_70::PROFILE_SCALAR_AT_70 + 8]
            .copy_from_slice(&0.0f64.to_le_bytes());
        bytes[scalar_offset..scalar_offset + 8].copy_from_slice(&scalar_value.to_le_bytes());

        let mut slot_offset = scalar_54::REFERENCE_SLOTS;
        let mut slot_reference = slot_references.into_iter();
        for expected_present in slot_presence {
            if expected_present {
                bytes[slot_offset] = 1;
                bytes[slot_offset + 1..slot_offset + 5]
                    .copy_from_slice(&slot_reference.next().expect("slot reference").to_le_bytes());
                slot_offset += 11;
            } else {
                slot_offset += 1;
            }
        }
        assert_eq!(slot_offset, scalar_54::FIRST_SIDE_EXTENT);
        bytes[scalar_54::FIRST_SIDE_EXTENT..scalar_54::FIRST_SIDE_EXTENT + 4]
            .copy_from_slice(&first_extent.to_le_bytes());
        bytes[reference_count_offset - 4..reference_count_offset]
            .copy_from_slice(&0u32.to_le_bytes());
        bytes[reference_count_offset..reference_count_offset + 4].copy_from_slice(
            &(u32::try_from(reference_members.len()).expect("fixture value fits u32"))
                .to_le_bytes(),
        );

        for reference in reference_members {
            bytes.push(1);
            bytes.extend_from_slice(&reference.to_le_bytes());
            bytes.extend_from_slice(&[0; 6]);
        }
        bytes.extend_from_slice(&1u32.to_le_bytes());
        lp_utf16(&mut bytes, "Extrude");
        let mut tail = [0; 76];
        tail[0..4].copy_from_slice(&1u32.to_le_bytes());
        tail[30..34].copy_from_slice(&2u32.to_le_bytes());
        bytes.extend_from_slice(&tail);
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(b"261");
        bytes.extend_from_slice(&RECORD_INDEX.to_le_bytes());
        assert_eq!(bytes.len(), frame_length + 11);
        bytes
    };

    let header = DesignRecordHeader {
        id: "generated:scope-header#class-296-legacy-one-sided".into(),
        record_index: RECORD_INDEX,
        class_tag: crate::records::references::DesignClassTag::try_from("296".to_owned()).unwrap(),
        byte_offset: 0,
    };
    let prologue = |bytes: &[u8]| {
        parse_parameter_scope(
            bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(bytes),
            header.record_index,
            &header.class_tag,
            header.byte_offset,
        )
        .unwrap()
        .and_then(|scope| scope.extrude_prologue())
    };
    let assert_valid = |bytes: &[u8],
                        frame_length: usize,
                        reference_count_offset: usize,
                        reference_members: &[u32],
                        extent: DesignExtrudeExtent,
                        face_extend: u32,
                        direction_reversed: bool| {
        let scope = parse_parameter_scope(
            bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(bytes),
            header.record_index,
            &header.class_tag,
            header.byte_offset,
        )
        .unwrap()
        .expect("class-296 legacy one-sided scope");
        assert_eq!(scope.frame_length(), u64_from_index(frame_length));
        assert_eq!(
            scope.reference_count_offset(),
            u64_from_index(reference_count_offset)
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
                operation: DesignExtrudeOperation::Cut,
                operation_offset: u64_from_index(scalar_54::OPERATION),
                direction_face_extend_values: [1, face_extend],
                side_extent_discriminators: [
                    if matches!(extent, DesignExtrudeExtent::OneSidedToFace) {
                        2
                    } else {
                        1
                    },
                    0,
                ],
                side_extent_discriminator_offsets: [
                    u64_from_index(scalar_54::FIRST_SIDE_EXTENT),
                    if matches!(extent, DesignExtrudeExtent::OneSidedToFace) {
                        u64_from_index(to_face_tail::SECOND_SIDE_EXTENT)
                    } else {
                        u64_from_index(distance_tail::SECOND_SIDE_EXTENT)
                    },
                ],
                extent: Some(extent),
                direction_face_extend_offsets: [
                    u64_from_index(scalar_54::DIRECTION),
                    u64_from_index(scalar_54::FACE_EXTEND),
                ],
                direction_reversed,
                direction_reversed_offset: u64_from_index(scalar_54::DIRECTION_REVERSED),
                solid_operation: true,
                solid_operation_offset: u64_from_index(scalar_54::GEOMETRY_KIND),
                start: DesignExtrudeStart::FromFace,
                start_offset: u64_from_index(scalar_54::START_SUPPORT),
            })
        );
    };

    let to_face_scalar_54 = build(
        515,
        to_face_tail::REFERENCE_COUNT,
        &TO_FACE_REFERENCES,
        [true, false, false, true, false, true, true],
        [
            TO_FACE_REFERENCES[0],
            TO_FACE_REFERENCES[3],
            TO_FACE_REFERENCES[5],
            TO_FACE_REFERENCES[6],
        ],
        1,
        2,
        scalar_54::PROFILE_SCALAR_AT_54,
        1.0,
        1,
    );
    assert_valid(
        &to_face_scalar_54,
        515,
        to_face_tail::REFERENCE_COUNT,
        &TO_FACE_REFERENCES,
        DesignExtrudeExtent::OneSidedToFace,
        1,
        true,
    );

    let to_face_scalar_70 = build(
        515,
        to_face_tail::REFERENCE_COUNT,
        &TO_FACE_REFERENCES,
        [true, false, false, true, false, true, true],
        [
            TO_FACE_REFERENCES[0],
            TO_FACE_REFERENCES[3],
            TO_FACE_REFERENCES[5],
            TO_FACE_REFERENCES[6],
        ],
        1,
        2,
        scalar_70::PROFILE_SCALAR_AT_70,
        1.0,
        0,
    );
    assert_valid(
        &to_face_scalar_70,
        515,
        to_face_tail::REFERENCE_COUNT,
        &TO_FACE_REFERENCES,
        DesignExtrudeExtent::OneSidedToFace,
        1,
        false,
    );

    let distance_scalar_70 = build(
        483,
        distance_tail::REFERENCE_COUNT,
        &DISTANCE_REFERENCES,
        [true, false, true, true, false, true, false],
        [
            DISTANCE_REFERENCES[0],
            DISTANCE_REFERENCES[2],
            DISTANCE_REFERENCES[3],
            DISTANCE_REFERENCES[5],
        ],
        2,
        1,
        scalar_70::PROFILE_SCALAR_AT_70,
        -1.0,
        0,
    );
    assert_valid(
        &distance_scalar_70,
        483,
        distance_tail::REFERENCE_COUNT,
        &DISTANCE_REFERENCES,
        DesignExtrudeExtent::OneSidedDistance,
        2,
        false,
    );

    let mut invalid_profile = to_face_scalar_54.clone();
    invalid_profile[scalar_54::PROFILE_SCALAR_AT_54..scalar_54::PROFILE_SCALAR_AT_54 + 8]
        .copy_from_slice(&2.0f64.to_le_bytes());
    assert!(prologue(&invalid_profile).is_none());

    let mut invalid_scalar_lane = to_face_scalar_54.clone();
    invalid_scalar_lane[scalar_54::ZERO_AFTER_SCALAR_AT_54] = 1;
    assert!(prologue(&invalid_scalar_lane).is_none());

    let mut invalid_slot = to_face_scalar_54.clone();
    invalid_slot[scalar_54::REFERENCE_SLOTS] = 0;
    assert!(prologue(&invalid_slot).is_none());

    let mut invalid_face_extend = to_face_scalar_54.clone();
    invalid_face_extend[scalar_54::FACE_EXTEND..scalar_54::FACE_EXTEND + 4]
        .copy_from_slice(&2u32.to_le_bytes());
    assert!(prologue(&invalid_face_extend).is_none());

    let mut invalid_reference_count = to_face_scalar_54.clone();
    invalid_reference_count[to_face_tail::REFERENCE_COUNT..to_face_tail::REFERENCE_COUNT + 4]
        .copy_from_slice(&11u32.to_le_bytes());
    assert!(prologue(&invalid_reference_count).is_none());

    let mut invalid_pair = to_face_scalar_54;
    invalid_pair[515 + 4..515 + 7].copy_from_slice(b"260");
    assert!(prologue(&invalid_pair).is_none());
}
