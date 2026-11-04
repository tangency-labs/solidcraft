// SPDX-License-Identifier: Apache-2.0

use crate::records::feature::extrude::{
    DesignExtrudeExtent, DesignExtrudeOperation, DesignExtrudePrologue, DesignExtrudeStart,
};
use crate::test_support::lp_utf16;

#[test]
fn legacy_class_397_symmetric_extrude_scope_decodes_473_byte_frame() {
    use crate::layout::legacy_class_397_symmetric_extrude_frame as layout;

    const RECORD_INDEX: u32 = 3970;
    const REFERENCE_MEMBERS: [u32; 8] = [11, 22, 33, 44, 55, 66, 77, 88];
    let mut bytes = vec![0; layout::LEN];
    bytes[0..4].copy_from_slice(&3u32.to_le_bytes());
    bytes[4..7].copy_from_slice(b"397");
    bytes[7..11].copy_from_slice(&RECORD_INDEX.to_le_bytes());
    bytes[layout::PREFIX_CONSTANT..layout::PREFIX_CONSTANT + 4]
        .copy_from_slice(&layout::PREFIX_CONSTANT_VALUE.to_le_bytes());
    bytes[layout::OPERATION..layout::OPERATION + 4].copy_from_slice(&2u32.to_le_bytes());
    bytes[layout::DIRECTION..layout::DIRECTION + 4]
        .copy_from_slice(&layout::DIRECTION_VALUE.to_le_bytes());
    bytes[layout::FACE_EXTEND..layout::FACE_EXTEND + 4]
        .copy_from_slice(&layout::FACE_EXTEND_VALUE.to_le_bytes());
    bytes[layout::GEOMETRY_KIND] = 1;
    bytes[layout::START_SUPPORT] = 1;
    bytes[layout::PROFILE_NORMAL + 8..layout::PROFILE_NORMAL + 16]
        .copy_from_slice(&1.0f64.to_le_bytes());

    let mut slot_offset = layout::REFERENCE_SLOTS;
    for (present, record_index) in [
        (true, 11u32),
        (true, 22u32),
        (true, 33u32),
        (true, 44u32),
        (false, 0u32),
        (true, 55u32),
        (false, 0u32),
    ] {
        if present {
            bytes[slot_offset] = 1;
            bytes[slot_offset + 1..slot_offset + 5].copy_from_slice(&record_index.to_le_bytes());
            slot_offset += 11;
        } else {
            slot_offset += 1;
        }
    }
    assert_eq!(slot_offset, layout::FIRST_SIDE_EXTENT);
    bytes[layout::FIRST_SIDE_EXTENT..layout::FIRST_SIDE_EXTENT + 4]
        .copy_from_slice(&1u32.to_le_bytes());
    bytes[layout::SECOND_SIDE_EXTENT..layout::SECOND_SIDE_EXTENT + 4]
        .copy_from_slice(&1u32.to_le_bytes());
    let mut guid = Vec::new();
    lp_utf16(&mut guid, "00000000-0000-0000-0000-000000000000");
    bytes[layout::GUID..layout::GUID + guid.len()].copy_from_slice(&guid);
    bytes[layout::REFERENCE_COUNT..layout::REFERENCE_COUNT + 4].copy_from_slice(
        &(u32::try_from(REFERENCE_MEMBERS.len()).expect("fixture value fits u32")).to_le_bytes(),
    );

    let prologue = super::super::legacy_class_397::exact_symmetric_extrude_prologue(
        &bytes,
        0,
        layout::LEN,
        "397",
        "262",
        layout::REFERENCE_COUNT,
        &REFERENCE_MEMBERS,
    )
    .expect("class-397 symmetric shifted Extrude prologue");
    assert_eq!(
        prologue,
        DesignExtrudePrologue::LegacyShifted {
            operation_prefix_marker_offset: None,
            operation: DesignExtrudeOperation::Cut,
            operation_offset: 27,
            direction_face_extend_values: [3, 2],
            side_extent_discriminators: [1, 1],
            side_extent_discriminator_offsets: [126, 139],
            extent: Some(DesignExtrudeExtent::SymmetricDistance),
            direction_face_extend_offsets: [31, 35],
            direction_reversed: false,
            direction_reversed_offset: 39,
            solid_operation: true,
            solid_operation_offset: 40,
            start: DesignExtrudeStart::OffsetProfilePlane,
            start_offset: 41,
        }
    );

    let mut invalid_side = bytes;
    invalid_side[layout::SECOND_SIDE_EXTENT..layout::SECOND_SIDE_EXTENT + 4]
        .copy_from_slice(&0u32.to_le_bytes());
    assert!(
        super::super::legacy_class_397::exact_symmetric_extrude_prologue(
            &invalid_side,
            0,
            layout::LEN,
            "397",
            "262",
            layout::REFERENCE_COUNT,
            &REFERENCE_MEMBERS,
        )
        .is_none()
    );
}
