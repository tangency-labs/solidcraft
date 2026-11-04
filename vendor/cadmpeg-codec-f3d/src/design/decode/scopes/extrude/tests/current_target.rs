// SPDX-License-Identifier: Apache-2.0
use crate::layout::current_extrude_face_target_payload as target;
use crate::records::feature::extrude::{DesignExtrudeExtent, DesignExtrudePrologue};

#[test]
fn current_two_face_extent_uses_payload_field_before_terminal_zero() {
    let mut bytes = vec![0; 240];
    bytes[28..32].copy_from_slice(&4u32.to_le_bytes());
    bytes[32..36].copy_from_slice(&2u32.to_le_bytes());
    bytes[46..54].copy_from_slice(&1f64.to_le_bytes());
    let first = 77;
    bytes[first..first + 4].copy_from_slice(&2u32.to_le_bytes());
    bytes[first + target::TARGET_OPTION..first + target::TARGET_OPTION + 4]
        .copy_from_slice(&1u32.to_le_bytes());
    let second = first + target::SECOND_SIDE_EXTENT;
    bytes[second..second + 4].copy_from_slice(&2u32.to_le_bytes());
    let parse =
        |bytes: &[u8]| super::super::exact_current_extrude_prologue(bytes, 0, 220, &[], false);
    let parsed = parse(&bytes).unwrap();
    assert!(matches!(
        parsed,
        DesignExtrudePrologue::ReferenceAware {
            side_extent_discriminators: [2, 2],
            side_extent_discriminator_offsets: [77, 100],
            extent: DesignExtrudeExtent::TwoSidedToFaces,
            ..
        }
    ));
    bytes[second..second + 4].copy_from_slice(&1u32.to_le_bytes());
    assert!(parse(&bytes).is_none());
    bytes[second..second + 4].copy_from_slice(&2u32.to_le_bytes());
    bytes[first + target::TARGET_OPTION..first + target::TARGET_OPTION + 4].fill(0);
    assert!(parse(&bytes).is_none());
}

#[test]
fn offset_24_prologue_decodes_operation_and_distance() {
    let mut bytes = vec![0; 120];
    bytes[24..28].copy_from_slice(&4u32.to_le_bytes());
    bytes[28..32].copy_from_slice(&1u32.to_le_bytes());
    bytes[32..36].copy_from_slice(&2u32.to_le_bytes());
    bytes[37] = 1;
    bytes[42..50].copy_from_slice(&1f64.to_le_bytes());
    bytes[73..77].copy_from_slice(&1u32.to_le_bytes());
    let parse =
        |bytes: &[u8]| super::super::exact_current_extrude_prologue(bytes, 0, 100, &[], false);
    let parsed = parse(&bytes).unwrap();
    assert!(matches!(
        parsed,
        DesignExtrudePrologue::ReferenceAware {
            operation: crate::records::feature::extrude::DesignExtrudeOperation::NewBody,
            operation_offset: 24,
            extent: DesignExtrudeExtent::OneSidedDistance,
            ..
        }
    ));
}
