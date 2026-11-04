// SPDX-License-Identifier: Apache-2.0

use super::layout;
use crate::design::decode::scopes::sheet_metal::exact_edge_flange_operation;
use crate::test_support::write_marked_reference;

fn frame() -> (Vec<u8>, Vec<u32>) {
    let mut bytes = vec![0; layout::LEN];
    for (offset, value) in [
        (layout::BEND_POSITION, 2u32),
        (layout::EDGE_COUNT, 2),
        (layout::HEIGHT_DATUM, 2),
        (layout::REFERENCE_SIDE, 4),
        (layout::RESULT_COUNT, 3),
        (layout::RESULT_ONE_TRAILER, 1),
        (layout::RESULT_TWO_TRAILER, 1),
        (layout::RESULT_THREE_TRAILER, 0),
        (layout::RESULT_SEPARATOR, 1),
        (layout::EDGE_GROUP_COUNT, 2),
    ] {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    for (offset, index) in [
        (layout::EDGE_WRAPPER_ONE_REFERENCE, 51),
        (layout::EDGE_WRAPPER_TWO_REFERENCE, 61),
        (layout::SETTINGS_REFERENCE, 141),
        (layout::ANGLE_OWNER_REFERENCE, 91),
        (layout::HEIGHT_OWNER_REFERENCE, 81),
        (layout::RESULT_ONE_REFERENCE, 151),
        (layout::RESULT_TWO_REFERENCE, 161),
        (layout::RESULT_THREE_REFERENCE, 171),
        (layout::AGGREGATE_GROUP_REFERENCE, 101),
        (layout::EDGE_GROUP_ONE_REFERENCE, 52),
        (layout::EDGE_GROUP_TWO_REFERENCE, 62),
    ] {
        write_marked_reference(&mut bytes, offset, index);
    }
    bytes[layout::INSIDE_BEND_RADIUS..layout::INSIDE_BEND_RADIUS + 8]
        .copy_from_slice(&0.35f64.to_le_bytes());
    (
        bytes,
        vec![51, 52, 55, 61, 62, 65, 81, 91, 101, 104, 108, 141],
    )
}

#[test]
fn corner_flange_decodes_both_class_pairs_and_round_trips() {
    let (bytes, references) = frame();
    for class in ["361", "362"] {
        let operation =
            exact_edge_flange_operation(&bytes, 0, layout::LEN, class, "258", &references)
                .unwrap()
                .expect("two-edge operation");
        let edges = operation.selection.shape().edges().collect::<Vec<_>>();
        assert_eq!(edges.len(), 2);
        assert_eq!(edges[0].wrapper, 51);
        assert_eq!(edges[0].group_record_index.get(), 52);
        assert_eq!(edges[0].operand_record_index(), 55);
        assert_eq!(edges[0].aggregate_operand_record_index, 104);
        assert_eq!(edges[1].wrapper, 61);
        assert_eq!(edges[1].group_record_index.get(), 62);
        assert_eq!(edges[1].operand_record_index(), 65);
        assert_eq!(edges[1].aggregate_operand_record_index, 108);
        let wire = serde_json::to_value(&operation).unwrap();
        let decoded: crate::records::feature::sheet_metal::DesignEdgeFlangeOperation =
            serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(decoded).unwrap(), wire);
        let mut invalid = wire;
        invalid["reference_side_code"] = serde_json::json!(3);
        assert!(serde_json::from_value::<
            crate::records::feature::sheet_metal::DesignEdgeFlangeOperation,
        >(invalid)
        .is_err());
    }
}

#[test]
fn corner_flange_refuses_unproven_controls_counts_and_links() {
    let (bytes, references) = frame();
    for offset in [
        layout::BEND_POSITION,
        layout::EDGE_COUNT,
        layout::HEIGHT_DATUM,
        layout::REFERENCE_SIDE,
        layout::RESULT_COUNT,
        layout::RESULT_ONE_TRAILER,
        layout::RESULT_TWO_TRAILER,
        layout::RESULT_THREE_TRAILER,
        layout::RESULT_SEPARATOR,
        layout::EDGE_GROUP_COUNT,
    ] {
        let mut invalid = bytes.clone();
        invalid[offset..offset + 4].copy_from_slice(&7u32.to_le_bytes());
        assert!(
            exact_edge_flange_operation(&invalid, 0, layout::LEN, "361", "258", &references)
                .unwrap()
                .is_none(),
            "offset {offset}"
        );
    }
    for offset in [
        layout::EDGE_WRAPPER_ONE_REFERENCE,
        layout::EDGE_WRAPPER_TWO_REFERENCE,
        layout::SETTINGS_REFERENCE,
        layout::ANGLE_OWNER_REFERENCE,
        layout::HEIGHT_OWNER_REFERENCE,
        layout::AGGREGATE_GROUP_REFERENCE,
        layout::EDGE_GROUP_ONE_REFERENCE,
        layout::EDGE_GROUP_TWO_REFERENCE,
        layout::RESULT_ONE_REFERENCE,
        layout::RESULT_TWO_REFERENCE,
        layout::RESULT_THREE_REFERENCE,
    ] {
        let mut invalid = bytes.clone();
        invalid[offset] = 0;
        assert!(
            exact_edge_flange_operation(&invalid, 0, layout::LEN, "361", "258", &references)
                .unwrap()
                .is_none(),
            "offset {offset}"
        );
    }
    for (class, paired, length) in [
        ("360", "258", layout::LEN),
        ("361", "257", layout::LEN),
        ("361", "258", layout::LEN - 1),
    ] {
        assert!(
            exact_edge_flange_operation(&bytes, 0, length, class, paired, &references)
                .unwrap()
                .is_none()
        );
    }
    for count in [1u32, 3] {
        let mut invalid = bytes.clone();
        invalid[layout::EDGE_COUNT..layout::EDGE_COUNT + 4].copy_from_slice(&count.to_le_bytes());
        assert!(
            exact_edge_flange_operation(&invalid, 0, layout::LEN, "361", "258", &references)
                .unwrap()
                .is_none()
        );
    }
    let mut duplicate = references.clone();
    duplicate[10] = duplicate[9];
    assert!(
        exact_edge_flange_operation(&bytes, 0, layout::LEN, "361", "258", &duplicate)
            .unwrap()
            .is_none()
    );
    let mut unordered = references.clone();
    unordered.swap(9, 10);
    assert!(
        exact_edge_flange_operation(&bytes, 0, layout::LEN, "361", "258", &unordered)
            .unwrap()
            .is_none()
    );
    let mut duplicate_result = bytes.clone();
    write_marked_reference(&mut duplicate_result, layout::RESULT_TWO_REFERENCE, 151);
    assert!(exact_edge_flange_operation(
        &duplicate_result,
        0,
        layout::LEN,
        "361",
        "258",
        &references
    )
    .unwrap()
    .is_none());
    for (offset, length) in [
        (layout::SIDE_PADDING, layout::SIDE_PADDING_VALUE.len()),
        (layout::RADIUS_PADDING, layout::RADIUS_PADDING_VALUE.len()),
        (layout::GROUP_PADDING, layout::GROUP_PADDING_VALUE.len()),
    ] {
        for position in offset..offset + length {
            let mut invalid = bytes.clone();
            invalid[position] = 1;
            assert!(exact_edge_flange_operation(
                &invalid,
                0,
                layout::LEN,
                "361",
                "258",
                &references
            )
            .unwrap()
            .is_none());
        }
    }
    for radius in [0.0f64, -0.35, f64::NAN] {
        let mut invalid = bytes.clone();
        invalid[layout::INSIDE_BEND_RADIUS..layout::INSIDE_BEND_RADIUS + 8]
            .copy_from_slice(&radius.to_le_bytes());
        assert!(
            exact_edge_flange_operation(&invalid, 0, layout::LEN, "361", "258", &references)
                .unwrap()
                .is_none()
        );
    }
    let mut extra = references.clone();
    extra.push(181);
    assert!(
        exact_edge_flange_operation(&bytes, 0, layout::LEN, "361", "258", &extra)
            .unwrap()
            .is_none()
    );
    for position in 0..references.len() {
        let mut invalid = references.clone();
        invalid.remove(position);
        assert!(
            exact_edge_flange_operation(&bytes, 0, layout::LEN, "361", "258", &invalid)
                .unwrap()
                .is_none()
        );
    }
}
