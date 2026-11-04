// SPDX-License-Identifier: Apache-2.0
use super::super::{exact_class_338_two_sided_distance_extrude_prologue, exact_extrude_extent};
use crate::records::feature::extrude::DesignExtrudeExtent;
use crate::records::feature::extrude::DesignExtrudeOperation;

#[test]
fn extrude_extent_tuple_is_one_admission_key() {
    let accepted = [
        (1, [1, 0], DesignExtrudeExtent::OneSidedDistance),
        (1, [2, 0], DesignExtrudeExtent::OneSidedToFace),
        (1, [3, 0], DesignExtrudeExtent::OneSidedThroughNext),
        (1, [4, 0], DesignExtrudeExtent::OneSidedThroughAll),
        (2, [2, 0], DesignExtrudeExtent::TwoSidedToFaces),
        (2, [1, 1], DesignExtrudeExtent::TwoSidedDistance),
        (3, [1, 0], DesignExtrudeExtent::SymmetricDistance),
        (3, [4, 4], DesignExtrudeExtent::SymmetricThroughAll),
    ];
    for (direction, side_extent_discriminators, expected) in accepted {
        assert_eq!(
            exact_extrude_extent(direction, side_extent_discriminators),
            Some(expected)
        );
    }

    for (direction, side_extent_discriminators) in [
        (0, [1, 0]),
        (1, [1, 1]),
        (1, [4, 4]),
        (2, [1, 0]),
        (2, [4, 4]),
        (3, [1, 1]),
    ] {
        assert_eq!(
            exact_extrude_extent(direction, side_extent_discriminators),
            None
        );
    }
}

#[test]
fn class_338_two_sided_distance_requires_its_null_scope_scalar_lane() {
    let mut bytes = vec![0; 503];
    let frame = crate::layout::legacy_class_338_two_sided_distance_extrude_frame::LEN;
    let paired_at = frame;

    let put_u32 = |bytes: &mut [u8], at: usize, value: u32| {
        bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
    };
    let put_f64 = |bytes: &mut [u8], at: usize, value: f64| {
        bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
    };
    let put_marked_reference = |bytes: &mut [u8], at: usize, record_index: u32| {
        bytes[at] = 1;
        put_u32(bytes, at + 1, record_index);
    };

    put_u32(&mut bytes, 20, 1);
    put_u32(&mut bytes, 27, 2);
    put_u32(&mut bytes, 31, 2);
    put_u32(&mut bytes, 35, 0);
    bytes[39] = 0;
    bytes[40] = 1;
    bytes[41] = 1;
    put_f64(&mut bytes, 45, 1.0);
    put_f64(&mut bytes, 53, 0.0);
    put_f64(&mut bytes, 61, 0.0);
    bytes[139] = 1;
    put_marked_reference(&mut bytes, 149, 4);
    put_u32(&mut bytes, 165, 1);
    put_marked_reference(&mut bytes, 169, 5);
    put_u32(&mut bytes, 188, 1);
    put_marked_reference(&mut bytes, 192, 6);

    let guid = "00000000-0000-0000-0000-000000000000";
    put_u32(
        &mut bytes,
        203,
        u32::try_from(guid.encode_utf16().count()).expect("fixture value fits u32"),
    );
    for (ordinal, code_unit) in guid.encode_utf16().enumerate() {
        bytes[207 + ordinal * 2..209 + ordinal * 2].copy_from_slice(&code_unit.to_le_bytes());
    }
    put_u32(&mut bytes, 282, 10);
    bytes[paired_at + 4..paired_at + 7].copy_from_slice(b"262");

    let parsed = exact_class_338_two_sided_distance_extrude_prologue(
        &bytes,
        0,
        paired_at,
        "338",
        "262",
        282,
        &[4, 5, 6, 7, 8, 9, 10, 11, 12, 13],
    )
    .expect("class-338 frame should satisfy its exact admission grammar");
    assert_eq!(parsed.operation(), DesignExtrudeOperation::Cut);
    assert_eq!(parsed.extent(), Some(DesignExtrudeExtent::TwoSidedDistance));
    assert!(!parsed.direction_reversed());
    assert!(parsed.solid_operation());

    bytes[139] = 0;
    assert!(exact_class_338_two_sided_distance_extrude_prologue(
        &bytes,
        0,
        paired_at,
        "338",
        "262",
        282,
        &[4, 5, 6, 7, 8, 9, 10, 11, 12, 13],
    )
    .is_none());
}
