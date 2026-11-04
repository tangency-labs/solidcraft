// SPDX-License-Identifier: Apache-2.0

use crate::test_support::write_marked_reference;

const EPS_BEND_RADIUS: f64 = 1.0e-12;

/// Field values written into a synthetic single-edge `EdgeFlange` frame.
struct EdgeFlangeFixture {
    header_shift: usize,
    /// Width-distance parameter owners the edge-width mode adds to the table.
    width_count: usize,
    result_count: usize,
    bend_position: u32,
    height_datum: u32,
    reference_side: u32,
    bend_radius: f64,
    wrapper: u32,
    settings: u32,
    angle_owner: u32,
    height_owner: u32,
    aggregate_group: u32,
    edge_group: u32,
}

/// A synthetic frame plus the offsets the reader is expected to derive.
struct EdgeFlangeFrame {
    bytes: Vec<u8>,
    paired_at: usize,
    bend_radius_offset: u64,
}

#[test]
fn base_flange_scope_has_exact_profile_and_thickness_fields() {
    let mut bytes = vec![0; 416];
    bytes[73..77].copy_from_slice(&1u32.to_le_bytes());
    bytes[81] = 1;
    bytes[82..86].copy_from_slice(&266u32.to_le_bytes());
    bytes[92..96].copy_from_slice(&1u32.to_le_bytes());
    bytes[112] = 1;
    bytes[113..117].copy_from_slice(&263u32.to_le_bytes());
    bytes[123..131].copy_from_slice(&0.25f64.to_le_bytes());
    bytes[141..145].copy_from_slice(&1u32.to_le_bytes());
    bytes[145] = 1;
    bytes[146..150].copy_from_slice(&256u32.to_le_bytes());

    let operation = crate::design::decode::scopes::sheet_metal::exact_base_flange_operation(
        &bytes,
        0,
        416,
        &[256, 259, 263, 266],
    )
    .expect("fixed BaseFlange operation");
    assert_eq!(operation.thickness.get(), 0.25);
    assert_eq!(operation.thickness_offset, 123);
    assert_eq!(operation.profile_group_record_index, 256);
    assert_eq!(operation.profile_record_index, 259);
    assert_eq!(operation.thickness_record_index, 263);
    assert_eq!(operation.settings_record_index, 266);

    bytes[123..131].copy_from_slice(&0.0f64.to_le_bytes());
    assert!(
        crate::design::decode::scopes::sheet_metal::exact_base_flange_operation(
            &bytes,
            0,
            416,
            &[256, 259, 263, 266]
        )
        .is_none()
    );
}

#[test]
fn edge_flange_scope_resolves_every_role_from_its_marked_slot() {
    // Settings before edge/aggregate groups; roles come from marked slots, not table position.
    let references = [201, 204, 207, 218, 221, 240, 243, 251, 254];
    let frame = edge_flange_frame(&EdgeFlangeFixture {
        header_shift: 0,
        width_count: 1,
        result_count: 2,
        bend_position: 2,
        height_datum: 1,
        reference_side: 4,
        bend_radius: 0.25,
        wrapper: 201,
        settings: 207,
        angle_owner: 218,
        height_owner: 204,
        aggregate_group: 240,
        edge_group: 251,
    });

    let operation = crate::design::decode::scopes::sheet_metal::exact_edge_flange_operation(
        &frame.bytes,
        0,
        frame.paired_at,
        "414",
        "258",
        &references,
    )
    .expect("decode the exact edge flange operation")
    .expect("fixed EdgeFlange operation");
    assert_eq!(
        operation
            .selection
            .shape()
            .edges()
            .map(|edge| edge.wrapper)
            .collect::<Vec<_>>(),
        [201]
    );
    assert_eq!(
        operation
            .selection
            .shape()
            .edges()
            .map(|edge| edge.group_record_index.get())
            .collect::<Vec<_>>(),
        [251]
    );
    assert_eq!(
        operation
            .selection
            .shape()
            .edges()
            .map(crate::records::feature::sheet_metal::DesignEdgeFlangeEdge::operand_record_index)
            .collect::<Vec<_>>(),
        [254]
    );
    assert_eq!(operation.selection.aggregate_group_record_index(), 240);
    assert_eq!(
        operation
            .selection
            .shape()
            .edges()
            .map(|edge| edge.aggregate_operand_record_index)
            .collect::<Vec<_>>(),
        [243]
    );
    assert_eq!(operation.height_owner_record_index, 204);
    assert_eq!(operation.angle_owner_record_index, 218);
    assert_eq!(operation.settings_record_index, 207);
    assert!((operation.bend_radius.get() - 0.25).abs() < EPS_BEND_RADIUS);
    assert_eq!(operation.bend_radius_offset, frame.bend_radius_offset);
    assert_eq!(
        operation.bend_position,
        crate::records::feature::sheet_metal::DesignBendPosition::Inside
    );
    assert_eq!(
        operation.height_datum,
        crate::records::feature::sheet_metal::DesignSheetMetalHeightDatum::InnerFaces
    );
    // The one table entry no slot claims is the width-distance owner, which
    // makes this the symmetric edge-width mode.
    assert_eq!(
        operation
            .selection
            .shape()
            .owner_indices()
            .copied()
            .collect::<Vec<_>>(),
        vec![221]
    );
    assert_eq!(
        operation.selection.shape().mode(),
        crate::records::feature::sheet_metal::DesignEdgeWidthMode::Symmetric
    );
}

#[test]
fn edge_flange_scope_reads_the_shifted_header_form() {
    let references = [201, 204, 207, 218, 240, 243, 251, 254];
    for header_shift in [0usize, 4] {
        let frame = edge_flange_frame(&EdgeFlangeFixture {
            header_shift,
            width_count: 0,
            result_count: 1,
            bend_position: 3,
            height_datum: 2,
            reference_side: 4,
            bend_radius: 0.5,
            wrapper: 201,
            settings: 207,
            angle_owner: 218,
            height_owner: 204,
            aggregate_group: 240,
            edge_group: 251,
        });

        let operation = crate::design::decode::scopes::sheet_metal::exact_edge_flange_operation(
            &frame.bytes,
            0,
            frame.paired_at,
            "414",
            "258",
            &references,
        )
        .expect("decode the exact edge flange operation")
        .expect("fixed EdgeFlange operation");
        assert_eq!(
            operation.bend_position,
            crate::records::feature::sheet_metal::DesignBendPosition::Adjacent
        );
        assert_eq!(
            operation.height_datum,
            crate::records::feature::sheet_metal::DesignSheetMetalHeightDatum::OuterFaces
        );
        assert_eq!(
            operation.selection.shape().mode(),
            crate::records::feature::sheet_metal::DesignEdgeWidthMode::FullEdge
        );
        assert!(operation
            .selection
            .shape()
            .owner_indices()
            .copied()
            .collect::<Vec<_>>()
            .is_empty());
    }
}

#[test]
fn legacy_edge_flange_scope_reads_both_classed_single_edge_forms() {
    let references = [201, 204, 207, 218, 240, 243, 251, 254];
    for (class_tag, paired_class_tag) in [("325", "258"), ("334", "257")] {
        let frame = legacy_edge_flange_frame();
        let operation = crate::design::decode::scopes::sheet_metal::exact_edge_flange_operation(
            &frame.bytes,
            0,
            frame.paired_at,
            class_tag,
            paired_class_tag,
            &references,
        )
        .expect("decode the exact edge flange operation")
        .expect("legacy classed EdgeFlange operation");
        assert_eq!(
            operation
                .selection
                .shape()
                .edges()
                .map(|edge| edge.wrapper)
                .collect::<Vec<_>>(),
            [201]
        );
        assert_eq!(
            operation
                .selection
                .shape()
                .edges()
                .map(|edge| edge.group_record_index.get())
                .collect::<Vec<_>>(),
            [251]
        );
        assert_eq!(
            operation
                .selection
                .shape()
                .edges()
                .map(crate::records::feature::sheet_metal::DesignEdgeFlangeEdge::operand_record_index)
                .collect::<Vec<_>>(),
            [254]
        );
        assert_eq!(operation.selection.aggregate_group_record_index(), 240);
        assert_eq!(
            operation
                .selection
                .shape()
                .edges()
                .map(|edge| edge.aggregate_operand_record_index)
                .collect::<Vec<_>>(),
            [243]
        );
        assert_eq!(operation.height_owner_record_index, 204);
        assert_eq!(operation.angle_owner_record_index, 218);
        assert_eq!(operation.settings_record_index, 207);
        assert!((operation.bend_radius.get() - 0.254).abs() < EPS_BEND_RADIUS);
        assert_eq!(operation.bend_radius_offset, 138);
        assert_eq!(
            operation.bend_position,
            crate::records::feature::sheet_metal::DesignBendPosition::Inside
        );
        assert_eq!(
            operation.height_datum,
            crate::records::feature::sheet_metal::DesignSheetMetalHeightDatum::OuterFaces
        );
        assert_eq!(
            operation.selection.shape().mode(),
            crate::records::feature::sheet_metal::DesignEdgeWidthMode::FullEdge
        );
    }
}

#[test]
fn legacy_edge_flange_scope_reads_classed_full_edge_multi_edge_forms() {
    let references = [201, 204, 207, 210, 213, 216, 219, 222, 225, 228, 231, 234];
    for (class_tag, paired_class_tag) in [("325", "258"), ("334", "257"), ("364", "261")] {
        let frame = legacy_multi_edge_flange_frame();
        let operation = crate::design::decode::scopes::sheet_metal::exact_edge_flange_operation(
            &frame.bytes,
            0,
            frame.paired_at,
            class_tag,
            paired_class_tag,
            &references,
        )
        .expect("decode the exact edge flange operation")
        .expect("legacy classed multi-edge EdgeFlange operation");
        assert_eq!(
            operation
                .selection
                .shape()
                .edges()
                .map(|edge| edge.wrapper)
                .collect::<Vec<_>>(),
            [201, 210]
        );
        assert_eq!(
            operation
                .selection
                .shape()
                .edges()
                .map(|edge| edge.group_record_index.get())
                .collect::<Vec<_>>(),
            [204, 213]
        );
        assert_eq!(
            operation
                .selection
                .shape()
                .edges()
                .map(crate::records::feature::sheet_metal::DesignEdgeFlangeEdge::operand_record_index)
                .collect::<Vec<_>>(),
            [207, 216]
        );
        assert_eq!(operation.selection.aggregate_group_record_index(), 225);
        assert_eq!(
            operation
                .selection
                .shape()
                .edges()
                .map(|edge| edge.aggregate_operand_record_index)
                .collect::<Vec<_>>(),
            [228, 231]
        );
        assert_eq!(operation.height_owner_record_index, 219);
        assert_eq!(operation.angle_owner_record_index, 222);
        assert_eq!(operation.settings_record_index, 234);
        assert!((operation.bend_radius.get() - 0.254).abs() < EPS_BEND_RADIUS);
        assert_eq!(operation.bend_radius_offset, 165);
        assert_eq!(
            operation.bend_position,
            crate::records::feature::sheet_metal::DesignBendPosition::Inside
        );
        assert_eq!(
            operation.height_datum,
            crate::records::feature::sheet_metal::DesignSheetMetalHeightDatum::OuterFaces
        );
        assert_eq!(
            operation.selection.shape().mode(),
            crate::records::feature::sheet_metal::DesignEdgeWidthMode::FullEdge
        );
    }
}

#[test]
fn legacy_edge_flange_scope_reads_class364_per_edge_width_form() {
    let references = [
        201, 204, 207, 210, 213, 216, 219, 222, 225, 228, 231, 234, 237, 240,
    ];
    let frame = legacy_class364_per_edge_width_flange_frame();
    let operation = crate::design::decode::scopes::sheet_metal::exact_edge_flange_operation(
        &frame.bytes,
        0,
        frame.paired_at,
        "364",
        "261",
        &references,
    )
    .expect("decode the exact edge flange operation")
    .expect("legacy class-364 per-edge width EdgeFlange operation");
    assert_eq!(
        operation
            .selection
            .shape()
            .edges()
            .map(|edge| edge.wrapper)
            .collect::<Vec<_>>(),
        [201, 213]
    );
    assert_eq!(
        operation
            .selection
            .shape()
            .edges()
            .map(|edge| edge.group_record_index.get())
            .collect::<Vec<_>>(),
        [204, 216]
    );
    assert_eq!(
        operation
            .selection
            .shape()
            .edges()
            .map(crate::records::feature::sheet_metal::DesignEdgeFlangeEdge::operand_record_index)
            .collect::<Vec<_>>(),
        [207, 219]
    );
    assert_eq!(operation.selection.aggregate_group_record_index(), 231);
    assert_eq!(
        operation
            .selection
            .shape()
            .edges()
            .map(|edge| edge.aggregate_operand_record_index)
            .collect::<Vec<_>>(),
        [234, 237]
    );
    assert_eq!(operation.height_owner_record_index, 225);
    assert_eq!(operation.angle_owner_record_index, 228);
    assert_eq!(
        operation
            .selection
            .shape()
            .owner_indices()
            .copied()
            .collect::<Vec<_>>(),
        vec![210, 222]
    );
    assert_eq!(operation.settings_record_index, 240);
    assert_eq!(
        operation.selection.shape().mode(),
        crate::records::feature::sheet_metal::DesignEdgeWidthMode::SymmetricPerEdge
    );
    assert!((operation.bend_radius.get() - 0.254).abs() < EPS_BEND_RADIUS);
    assert_eq!(operation.bend_radius_offset, 165);
}

#[test]
fn legacy_edge_flange_scope_reads_class325_two_sided_per_edge_form() {
    let references = [
        201, 204, 207, 210, 213, 216, 219, 222, 225, 228, 231, 234, 237, 240, 243, 246,
    ];
    let frame = legacy_class325_two_sided_per_edge_flange_frame();
    for (class_tag, paired_class_tag) in [("325", "258"), ("334", "257")] {
        let operation = crate::design::decode::scopes::sheet_metal::exact_edge_flange_operation(
            &frame.bytes,
            0,
            frame.paired_at,
            class_tag,
            paired_class_tag,
            &references,
        )
        .expect("decode the exact edge flange operation")
        .expect("legacy two-sided per-edge EdgeFlange operation");
        assert_eq!(
            operation
                .selection
                .shape()
                .edges()
                .map(|edge| edge.wrapper)
                .collect::<Vec<_>>(),
            [201, 213]
        );
        assert_eq!(
            operation
                .selection
                .shape()
                .edges()
                .map(|edge| edge.group_record_index.get())
                .collect::<Vec<_>>(),
            [204, 216]
        );
        assert_eq!(
            operation
                .selection
                .shape()
                .edges()
                .map(crate::records::feature::sheet_metal::DesignEdgeFlangeEdge::operand_record_index)
                .collect::<Vec<_>>(),
            [207, 219]
        );
        assert_eq!(operation.selection.aggregate_group_record_index(), 231);
        assert_eq!(
            operation
                .selection
                .shape()
                .edges()
                .map(|edge| edge.aggregate_operand_record_index)
                .collect::<Vec<_>>(),
            [243, 246]
        );
        assert_eq!(
            operation
                .selection
                .shape()
                .owner_indices()
                .copied()
                .collect::<Vec<_>>(),
            vec![210, 222, 234, 237]
        );
        assert_eq!(
            match &operation.selection.shape() {
                crate::records::feature::sheet_metal::DesignEdgeFlangeShape::TwoSidesPerEdge {
                    edges,
                    ..
                } => edges.iter().map(|row| row.owners).collect::<Vec<_>>(),
                _ => Vec::new(),
            },
            vec![[210, 222], [234, 237]]
        );
        assert_eq!(operation.height_owner_record_index, 225);
        assert_eq!(operation.angle_owner_record_index, 228);
        assert_eq!(operation.settings_record_index, 240);
        assert_eq!(
            operation.selection.shape().mode(),
            crate::records::feature::sheet_metal::DesignEdgeWidthMode::TwoSidesPerEdge
        );
        assert!((operation.bend_radius.get() - 0.254).abs() < EPS_BEND_RADIUS);
        assert_eq!(operation.bend_radius_offset, 169);
    }
}

#[test]
fn legacy_edge_flange_scope_reads_class286_single_edge_form() {
    let references = [201, 204, 207, 218, 240, 243, 251, 254];
    let frame = legacy_class286_single_edge_flange_frame();
    let operation = crate::design::decode::scopes::sheet_metal::exact_edge_flange_operation(
        &frame.bytes,
        0,
        frame.paired_at,
        "286",
        "258",
        &references,
    )
    .expect("decode the exact edge flange operation")
    .expect("legacy class-286 EdgeFlange operation");
    assert_eq!(
        operation
            .selection
            .shape()
            .edges()
            .map(|edge| edge.wrapper)
            .collect::<Vec<_>>(),
        [201]
    );
    assert_eq!(
        operation
            .selection
            .shape()
            .edges()
            .map(|edge| edge.group_record_index.get())
            .collect::<Vec<_>>(),
        [251]
    );
    assert_eq!(
        operation
            .selection
            .shape()
            .edges()
            .map(crate::records::feature::sheet_metal::DesignEdgeFlangeEdge::operand_record_index)
            .collect::<Vec<_>>(),
        [254]
    );
    assert_eq!(operation.selection.aggregate_group_record_index(), 240);
    assert_eq!(
        operation
            .selection
            .shape()
            .edges()
            .map(|edge| edge.aggregate_operand_record_index)
            .collect::<Vec<_>>(),
        [243]
    );
    assert_eq!(operation.height_owner_record_index, 204);
    assert_eq!(operation.angle_owner_record_index, 218);
    assert_eq!(operation.settings_record_index, 207);
    assert!((operation.bend_radius.get() - 0.25).abs() < EPS_BEND_RADIUS);
    assert_eq!(operation.bend_radius_offset, 142);
    assert_eq!(
        operation.bend_position,
        crate::records::feature::sheet_metal::DesignBendPosition::Adjacent
    );
    assert_eq!(
        operation.height_datum,
        crate::records::feature::sheet_metal::DesignSheetMetalHeightDatum::OuterFaces
    );
    assert_eq!(
        operation.selection.shape().mode(),
        crate::records::feature::sheet_metal::DesignEdgeWidthMode::FullEdge
    );
    assert!(operation
        .selection
        .shape()
        .owner_indices()
        .copied()
        .collect::<Vec<_>>()
        .is_empty());
}

#[test]
fn legacy_edge_flange_scope_reads_class286_extended_two_sided_per_edge_form() {
    let references = [
        201, 204, 207, 210, 213, 216, 219, 222, 225, 228, 231, 234, 237, 240, 243, 246, 249, 252,
        255, 258, 261, 264, 267, 270, 273, 276, 279, 282,
    ];
    let frame = legacy_class286_extended_two_sided_per_edge_flange_frame();
    let operation = crate::design::decode::scopes::sheet_metal::exact_edge_flange_operation(
        &frame.bytes,
        0,
        frame.paired_at,
        "286",
        "258",
        &references,
    )
    .expect("decode the exact edge flange operation")
    .expect("legacy extended class-286 EdgeFlange operation");
    assert_eq!(
        operation
            .selection
            .shape()
            .edges()
            .map(|edge| edge.wrapper)
            .collect::<Vec<_>>(),
        [201, 213]
    );
    assert_eq!(
        operation
            .selection
            .shape()
            .edges()
            .map(|edge| edge.group_record_index.get())
            .collect::<Vec<_>>(),
        [204, 216]
    );
    assert_eq!(
        operation
            .selection
            .shape()
            .edges()
            .map(crate::records::feature::sheet_metal::DesignEdgeFlangeEdge::operand_record_index)
            .collect::<Vec<_>>(),
        [207, 219]
    );
    assert_eq!(operation.selection.aggregate_group_record_index(), 231);
    assert_eq!(
        operation
            .selection
            .shape()
            .edges()
            .map(|edge| edge.aggregate_operand_record_index)
            .collect::<Vec<_>>(),
        [279, 282]
    );
    assert_eq!(
        operation
            .selection
            .shape()
            .owner_indices()
            .copied()
            .collect::<Vec<_>>(),
        vec![210, 222, 234, 237]
    );
    assert_eq!(
        match &operation.selection.shape() {
            crate::records::feature::sheet_metal::DesignEdgeFlangeShape::TwoSidesPerEdge {
                edges,
                ..
            } => edges.iter().map(|row| row.owners).collect::<Vec<_>>(),
            _ => Vec::new(),
        },
        vec![[210, 222], [234, 237]]
    );
    assert_eq!(
        operation.auxiliary_reference_record_indices,
        [243, 246, 249, 252, 255, 258, 261, 264, 267, 270, 273, 276]
    );
    assert_eq!(
        operation.selection.shape().source(),
        crate::records::feature::sheet_metal::DesignEdgeFlangeWidthParameterSource::EdgeOffset
    );
    assert_eq!(
        operation.selection.shape().mode(),
        crate::records::feature::sheet_metal::DesignEdgeWidthMode::TwoSidesPerEdge
    );
    assert_eq!(operation.settings_record_index, 240);
    assert_eq!(operation.height_owner_record_index, 225);
    assert_eq!(operation.angle_owner_record_index, 228);
    assert!((operation.bend_radius.get() - 0.254).abs() < EPS_BEND_RADIUS);
    assert_eq!(operation.bend_radius_offset, 169);
}

#[test]
fn edge_flange_scope_refuses_a_frame_whose_group_operand_is_absent() {
    let references = [201, 204, 207, 218, 240, 243, 251, 255];
    let frame = edge_flange_frame(&EdgeFlangeFixture {
        header_shift: 0,
        width_count: 0,
        result_count: 1,
        bend_position: 1,
        height_datum: 2,
        reference_side: 4,
        bend_radius: 0.25,
        wrapper: 201,
        settings: 207,
        angle_owner: 218,
        height_owner: 204,
        aggregate_group: 240,
        edge_group: 251,
    });

    assert!(
        crate::design::decode::scopes::sheet_metal::exact_edge_flange_operation(
            &frame.bytes,
            0,
            frame.paired_at,
            "414",
            "258",
            &references,
        )
        .expect("decode the exact edge flange operation")
        .is_none()
    );
}

#[test]
fn edge_flange_scope_reads_the_single_edge_to_object_form() {
    use crate::records::feature::sheet_metal::DesignEdgeFlangeHeightExtent;

    let references = [201, 204, 207, 218, 221, 224, 240, 243, 251, 254, 270];
    for header_shift in [0usize, 4] {
        let frame = edge_flange_to_object_frame(header_shift);
        let operation = crate::design::decode::scopes::sheet_metal::exact_edge_flange_operation(
            &frame.bytes,
            0,
            frame.paired_at,
            "414",
            "258",
            &references,
        )
        .expect("decode the exact edge flange operation")
        .expect("fixed to-object EdgeFlange operation");
        assert_eq!(
            operation
                .selection
                .shape()
                .owner_indices()
                .copied()
                .collect::<Vec<_>>(),
            Vec::<u32>::new()
        );
        assert_eq!(
            operation
                .selection
                .shape()
                .edges()
                .map(|edge| edge.group_record_index.get())
                .collect::<Vec<_>>(),
            [251]
        );
        assert_eq!(
            operation
                .selection
                .shape()
                .edges()
                .map(crate::records::feature::sheet_metal::DesignEdgeFlangeEdge::operand_record_index)
                .collect::<Vec<_>>(),
            [254]
        );
        assert_eq!(
            operation.selection.shape().height(),
            DesignEdgeFlangeHeightExtent::ToObject {
                target_group_record_index: 221,
                target_operand_record_index: 224,
                offset_owner_record_index: 270,
                reference_record_indices: [469, 470],
            }
        );
    }
}

#[test]
fn edge_flange_scope_refuses_a_to_object_frame_with_a_table_reference_pair() {
    let mut frame = edge_flange_to_object_frame(0);
    frame.bytes[85 + 109 + 1..85 + 109 + 5].copy_from_slice(&270u32.to_le_bytes());
    assert!(
        crate::design::decode::scopes::sheet_metal::exact_edge_flange_operation(
            &frame.bytes,
            0,
            frame.paired_at,
            "414",
            "258",
            &[201, 204, 207, 218, 221, 224, 240, 243, 251, 254, 270],
        )
        .expect("decode the exact edge flange operation")
        .is_none()
    );
}

/// Build a single-edge `EdgeFlange` frame from the settled fixed-section layout.
///
/// Every offset is computed from the layout rather than counted by hand, so the
/// fixture stays correct when a field width changes.
fn edge_flange_frame(fixture: &EdgeFlangeFixture) -> EdgeFlangeFrame {
    let common = 85 + fixture.header_shift;
    let wrapper_at = common + 8;
    let settings_at = wrapper_at + 11;
    let datum_at = settings_at + 11;
    let angle_at = datum_at + 4;
    let height_at = angle_at + 11;
    let side_at = height_at + 11;
    let radius_at = side_at + 15;
    let result_count_at = radius_at + 14;
    let aggregate_at = radius_at + 22 + fixture.result_count * 15;
    let edge_group_at = aggregate_at + 27;
    let paired_at =
        493 + fixture.result_count * 15 + fixture.width_count * 11 + fixture.header_shift;

    let mut bytes = vec![0; paired_at.max(edge_group_at + 11)];
    bytes[common..common + 4].copy_from_slice(&fixture.bend_position.to_le_bytes());
    bytes[common + 4..common + 8].copy_from_slice(&1u32.to_le_bytes());
    write_marked_reference(&mut bytes, wrapper_at, fixture.wrapper);
    write_marked_reference(&mut bytes, settings_at, fixture.settings);
    bytes[datum_at..datum_at + 4].copy_from_slice(&fixture.height_datum.to_le_bytes());
    write_marked_reference(&mut bytes, angle_at, fixture.angle_owner);
    write_marked_reference(&mut bytes, height_at, fixture.height_owner);
    bytes[side_at..side_at + 4].copy_from_slice(&fixture.reference_side.to_le_bytes());
    bytes[radius_at..radius_at + 8].copy_from_slice(&fixture.bend_radius.to_le_bytes());
    let result_count = u32::try_from(fixture.result_count).expect("result count fits u32");
    bytes[result_count_at..result_count_at + 4].copy_from_slice(&result_count.to_le_bytes());
    write_marked_reference(&mut bytes, aggregate_at, fixture.aggregate_group);
    write_marked_reference(&mut bytes, edge_group_at, fixture.edge_group);

    EdgeFlangeFrame {
        bytes,
        paired_at,
        bend_radius_offset: u64::try_from(radius_at).expect("radius offset fits u64"),
    }
}

fn edge_flange_to_object_frame(header_shift: usize) -> EdgeFlangeFrame {
    let common = 85 + header_shift;
    let wrapper_at = common + 8;
    let settings_at = wrapper_at + 11;
    let datum_at = settings_at + 11;
    let angle_at = datum_at + 4;
    let height_at = angle_at + 11;
    let side_at = height_at + 11;
    let radius_at = side_at + 15;
    let target_group_at = common + 94;
    let target_reference_one_at = common + 109;
    let target_reference_two_at = common + 124;
    let aggregate_at = common + 143;
    let edge_group_at = common + 170;
    let paired_at = 576 + header_shift;
    let mut bytes = vec![0; paired_at];

    bytes[common..common + 4].copy_from_slice(&2u32.to_le_bytes());
    bytes[common + 4..common + 8].copy_from_slice(&1u32.to_le_bytes());
    write_marked_reference(&mut bytes, wrapper_at, 201);
    write_marked_reference(&mut bytes, settings_at, 207);
    bytes[datum_at..datum_at + 4].copy_from_slice(&2u32.to_le_bytes());
    write_marked_reference(&mut bytes, angle_at, 218);
    write_marked_reference(&mut bytes, height_at, 204);
    bytes[side_at..side_at + 4].copy_from_slice(&4u32.to_le_bytes());
    bytes[radius_at..radius_at + 8].copy_from_slice(&0.25f64.to_le_bytes());
    bytes[radius_at + 14..radius_at + 18].copy_from_slice(&1u32.to_le_bytes());
    write_marked_reference(&mut bytes, target_group_at, 221);
    bytes[common + 105..common + 109].copy_from_slice(&2u32.to_le_bytes());
    write_marked_reference(&mut bytes, target_reference_one_at, 469);
    bytes[common + 120..common + 124].copy_from_slice(&1u32.to_le_bytes());
    write_marked_reference(&mut bytes, target_reference_two_at, 470);
    bytes[common + 139..common + 143].copy_from_slice(&1u32.to_le_bytes());
    write_marked_reference(&mut bytes, aggregate_at, 240);
    bytes[common + 166..common + 170].copy_from_slice(&1u32.to_le_bytes());
    write_marked_reference(&mut bytes, edge_group_at, 251);

    EdgeFlangeFrame {
        bytes,
        paired_at,
        bend_radius_offset: u64::try_from(radius_at).expect("radius offset fits u64"),
    }
}

fn legacy_edge_flange_frame() -> EdgeFlangeFrame {
    let paired_at = 494;
    let mut bytes = vec![0; paired_at];
    bytes[76..80].copy_from_slice(&2u32.to_le_bytes());
    bytes[80..84].copy_from_slice(&1u32.to_le_bytes());
    write_marked_reference(&mut bytes, 84, 201);
    write_marked_reference(&mut bytes, 95, 207);
    bytes[106..110].copy_from_slice(&2u32.to_le_bytes());
    write_marked_reference(&mut bytes, 110, 218);
    write_marked_reference(&mut bytes, 121, 204);
    bytes[132..136].copy_from_slice(&4u32.to_le_bytes());
    bytes[138..146].copy_from_slice(&0.254f64.to_le_bytes());
    bytes[146..150].copy_from_slice(&2u32.to_le_bytes());
    write_marked_reference(&mut bytes, 150, 501);
    bytes[161..165].copy_from_slice(&1u32.to_le_bytes());
    write_marked_reference(&mut bytes, 165, 502);
    bytes[176..180].copy_from_slice(&0u32.to_le_bytes());
    bytes[180..184].copy_from_slice(&1u32.to_le_bytes());
    write_marked_reference(&mut bytes, 184, 240);
    write_marked_reference(&mut bytes, 207, 251);

    EdgeFlangeFrame {
        bytes,
        paired_at,
        bend_radius_offset: 138,
    }
}

fn legacy_multi_edge_flange_frame() -> EdgeFlangeFrame {
    use crate::layout::edge_flange_multi_edge_fixed_operation as layout;

    let paired_at = 591;
    let mut bytes = vec![0; paired_at];
    bytes[layout::BEND_POSITION..layout::BEND_POSITION + 4].copy_from_slice(&2u32.to_le_bytes());
    bytes[layout::EDGE_COUNT..layout::EDGE_COUNT + 4].copy_from_slice(&2u32.to_le_bytes());
    write_marked_reference(&mut bytes, layout::EDGE_WRAPPER_ONE_REFERENCE, 201);
    write_marked_reference(&mut bytes, layout::EDGE_WRAPPER_TWO_REFERENCE, 210);
    write_marked_reference(&mut bytes, layout::SETTINGS_REFERENCE, 234);
    bytes[layout::HEIGHT_DATUM..layout::HEIGHT_DATUM + 4].copy_from_slice(&2u32.to_le_bytes());
    write_marked_reference(&mut bytes, layout::ANGLE_OWNER_REFERENCE, 222);
    write_marked_reference(&mut bytes, layout::HEIGHT_OWNER_REFERENCE, 219);
    bytes[layout::REFERENCE_SIDE..layout::REFERENCE_SIDE + 4].copy_from_slice(&4u32.to_le_bytes());
    bytes[layout::INSIDE_BEND_RADIUS..layout::INSIDE_BEND_RADIUS + 8]
        .copy_from_slice(&0.254f64.to_le_bytes());
    bytes[layout::RESULT_COUNT..layout::RESULT_COUNT + 4].copy_from_slice(&3u32.to_le_bytes());
    write_marked_reference(&mut bytes, layout::RESULT_ONE_REFERENCE, 501);
    bytes[layout::RESULT_ONE_TRAILER..layout::RESULT_ONE_TRAILER + 4]
        .copy_from_slice(&1u32.to_le_bytes());
    write_marked_reference(&mut bytes, layout::RESULT_TWO_REFERENCE, 502);
    bytes[layout::RESULT_TWO_TRAILER..layout::RESULT_TWO_TRAILER + 4]
        .copy_from_slice(&1u32.to_le_bytes());
    write_marked_reference(&mut bytes, layout::RESULT_THREE_REFERENCE, 503);
    bytes[layout::RESULT_THREE_TRAILER..layout::RESULT_THREE_TRAILER + 4]
        .copy_from_slice(&0u32.to_le_bytes());
    bytes[layout::RESULT_SEPARATOR..layout::RESULT_SEPARATOR + 4]
        .copy_from_slice(&1u32.to_le_bytes());
    write_marked_reference(&mut bytes, layout::AGGREGATE_GROUP_REFERENCE, 225);
    write_marked_reference(&mut bytes, layout::EDGE_GROUP_ONE_REFERENCE, 204);
    write_marked_reference(&mut bytes, layout::EDGE_GROUP_TWO_REFERENCE, 213);

    EdgeFlangeFrame {
        bytes,
        paired_at,
        bend_radius_offset: u64::try_from(layout::INSIDE_BEND_RADIUS)
            .expect("bend radius offset fits u64"),
    }
}

fn legacy_class364_per_edge_width_flange_frame() -> EdgeFlangeFrame {
    use crate::layout::edge_flange_class364_per_edge_width_fixed_operation as layout;

    let paired_at = 643;
    let mut bytes = vec![0; paired_at];
    bytes[layout::BEND_POSITION..layout::BEND_POSITION + 4].copy_from_slice(&3u32.to_le_bytes());
    bytes[layout::EDGE_COUNT..layout::EDGE_COUNT + 4].copy_from_slice(&2u32.to_le_bytes());
    write_marked_reference(&mut bytes, layout::EDGE_WRAPPER_ONE_REFERENCE, 201);
    write_marked_reference(&mut bytes, layout::EDGE_WRAPPER_TWO_REFERENCE, 213);
    write_marked_reference(&mut bytes, layout::SETTINGS_REFERENCE, 240);
    bytes[layout::HEIGHT_DATUM..layout::HEIGHT_DATUM + 4].copy_from_slice(&2u32.to_le_bytes());
    write_marked_reference(&mut bytes, layout::ANGLE_OWNER_REFERENCE, 228);
    write_marked_reference(&mut bytes, layout::HEIGHT_OWNER_REFERENCE, 225);
    bytes[layout::REFERENCE_SIDE..layout::REFERENCE_SIDE + 4].copy_from_slice(&4u32.to_le_bytes());
    bytes[layout::INSIDE_BEND_RADIUS..layout::INSIDE_BEND_RADIUS + 8]
        .copy_from_slice(&0.254f64.to_le_bytes());
    bytes[layout::RESULT_COUNT..layout::RESULT_COUNT + 4].copy_from_slice(&5u32.to_le_bytes());
    for (ordinal, (record_index, trailer)) in
        [(501u32, 1u32), (502, 1), (503, 1), (504, 1), (505, 0)]
            .into_iter()
            .enumerate()
    {
        let result_offset = layout::RESULT_ONE_REFERENCE + ordinal * 15;
        write_marked_reference(&mut bytes, result_offset, record_index);
        bytes[layout::RESULT_ONE_TRAILER + ordinal * 15
            ..layout::RESULT_ONE_TRAILER + ordinal * 15 + 4]
            .copy_from_slice(&trailer.to_le_bytes());
    }
    bytes[layout::RESULT_SEPARATOR..layout::RESULT_SEPARATOR + 4]
        .copy_from_slice(&1u32.to_le_bytes());
    write_marked_reference(&mut bytes, layout::AGGREGATE_GROUP_REFERENCE, 231);
    write_marked_reference(&mut bytes, layout::EDGE_GROUP_ONE_REFERENCE, 204);
    write_marked_reference(&mut bytes, layout::EDGE_GROUP_TWO_REFERENCE, 216);

    EdgeFlangeFrame {
        bytes,
        paired_at,
        bend_radius_offset: 165,
    }
}

fn legacy_class325_two_sided_per_edge_flange_frame() -> EdgeFlangeFrame {
    use crate::layout::edge_flange_class325_334_two_sided_per_edge_fixed_operation as layout;

    let paired_at = 669;
    let mut bytes = vec![0; paired_at];
    bytes[layout::BEND_POSITION..layout::BEND_POSITION + 4].copy_from_slice(&3u32.to_le_bytes());
    bytes[layout::EDGE_COUNT..layout::EDGE_COUNT + 4].copy_from_slice(&2u32.to_le_bytes());
    write_marked_reference(&mut bytes, layout::EDGE_WRAPPER_ONE_REFERENCE, 201);
    write_marked_reference(&mut bytes, layout::EDGE_WRAPPER_TWO_REFERENCE, 213);
    write_marked_reference(&mut bytes, layout::SETTINGS_REFERENCE, 240);
    bytes[layout::HEIGHT_DATUM..layout::HEIGHT_DATUM + 4].copy_from_slice(&2u32.to_le_bytes());
    write_marked_reference(&mut bytes, layout::ANGLE_OWNER_REFERENCE, 228);
    write_marked_reference(&mut bytes, layout::HEIGHT_OWNER_REFERENCE, 225);
    bytes[layout::REFERENCE_SIDE..layout::REFERENCE_SIDE + 4].copy_from_slice(&4u32.to_le_bytes());
    bytes[layout::INSIDE_BEND_RADIUS..layout::INSIDE_BEND_RADIUS + 8]
        .copy_from_slice(&0.254f64.to_le_bytes());
    bytes[layout::RESULT_COUNT..layout::RESULT_COUNT + 4].copy_from_slice(&5u32.to_le_bytes());
    for (ordinal, (record_index, trailer)) in
        [(501u32, 1u32), (502, 1), (503, 1), (504, 1), (505, 0)]
            .into_iter()
            .enumerate()
    {
        let result_offset = layout::RESULT_ONE_REFERENCE + ordinal * 15;
        write_marked_reference(&mut bytes, result_offset, record_index);
        bytes[layout::RESULT_ONE_TRAILER + ordinal * 15
            ..layout::RESULT_ONE_TRAILER + ordinal * 15 + 4]
            .copy_from_slice(&trailer.to_le_bytes());
    }
    bytes[layout::RESULT_SEPARATOR..layout::RESULT_SEPARATOR + 4]
        .copy_from_slice(&1u32.to_le_bytes());
    write_marked_reference(&mut bytes, layout::AGGREGATE_GROUP_REFERENCE, 231);
    write_marked_reference(&mut bytes, layout::EDGE_GROUP_ONE_REFERENCE, 204);
    write_marked_reference(&mut bytes, layout::EDGE_GROUP_TWO_REFERENCE, 216);

    EdgeFlangeFrame {
        bytes,
        paired_at,
        bend_radius_offset: u64::try_from(layout::INSIDE_BEND_RADIUS)
            .expect("bend radius offset fits u64"),
    }
}

fn legacy_class286_single_edge_flange_frame() -> EdgeFlangeFrame {
    use crate::layout::edge_flange_class286_single_edge_fixed_operation as layout;

    let paired_at = 483;
    let mut bytes = vec![0; paired_at];
    bytes[layout::BEND_POSITION..layout::BEND_POSITION + 4].copy_from_slice(&3u32.to_le_bytes());
    bytes[layout::EDGE_COUNT..layout::EDGE_COUNT + 4].copy_from_slice(&1u32.to_le_bytes());
    write_marked_reference(&mut bytes, layout::EDGE_WRAPPER_REFERENCE, 201);
    write_marked_reference(&mut bytes, layout::SETTINGS_REFERENCE, 207);
    bytes[layout::HEIGHT_DATUM..layout::HEIGHT_DATUM + 4].copy_from_slice(&2u32.to_le_bytes());
    write_marked_reference(&mut bytes, layout::ANGLE_OWNER_REFERENCE, 218);
    write_marked_reference(&mut bytes, layout::HEIGHT_OWNER_REFERENCE, 204);
    bytes[layout::REFERENCE_SIDE..layout::REFERENCE_SIDE + 4].copy_from_slice(&4u32.to_le_bytes());
    bytes[layout::INSIDE_BEND_RADIUS..layout::INSIDE_BEND_RADIUS + 8]
        .copy_from_slice(&0.25f64.to_le_bytes());
    bytes[layout::RESULT_COUNT..layout::RESULT_COUNT + 4].copy_from_slice(&1u32.to_le_bytes());
    write_marked_reference(&mut bytes, layout::RESULT_REFERENCE, 501);
    bytes[layout::RESULT_TRAILER..layout::RESULT_TRAILER + 4].copy_from_slice(&0u32.to_le_bytes());
    bytes[layout::RESULT_SEPARATOR..layout::RESULT_SEPARATOR + 4]
        .copy_from_slice(&1u32.to_le_bytes());
    write_marked_reference(&mut bytes, layout::AGGREGATE_GROUP_REFERENCE, 240);
    write_marked_reference(&mut bytes, layout::EDGE_GROUP_REFERENCE, 251);

    EdgeFlangeFrame {
        bytes,
        paired_at,
        bend_radius_offset: u64::try_from(layout::INSIDE_BEND_RADIUS)
            .expect("bend radius offset fits u64"),
    }
}

fn legacy_class286_extended_two_sided_per_edge_flange_frame() -> EdgeFlangeFrame {
    use crate::layout::edge_flange_class286_two_sided_per_edge_fixed_operation as layout;

    let paired_at = 801;
    let mut bytes = vec![0; paired_at];
    bytes[layout::BEND_POSITION..layout::BEND_POSITION + 4].copy_from_slice(&3u32.to_le_bytes());
    bytes[layout::EDGE_COUNT..layout::EDGE_COUNT + 4].copy_from_slice(&2u32.to_le_bytes());
    write_marked_reference(&mut bytes, layout::EDGE_WRAPPER_ONE_REFERENCE, 201);
    write_marked_reference(&mut bytes, layout::EDGE_WRAPPER_TWO_REFERENCE, 213);
    write_marked_reference(&mut bytes, layout::SETTINGS_REFERENCE, 240);
    bytes[layout::HEIGHT_DATUM..layout::HEIGHT_DATUM + 4].copy_from_slice(&2u32.to_le_bytes());
    write_marked_reference(&mut bytes, layout::ANGLE_OWNER_REFERENCE, 228);
    write_marked_reference(&mut bytes, layout::HEIGHT_OWNER_REFERENCE, 225);
    bytes[layout::REFERENCE_SIDE..layout::REFERENCE_SIDE + 4].copy_from_slice(&4u32.to_le_bytes());
    bytes[layout::INSIDE_BEND_RADIUS..layout::INSIDE_BEND_RADIUS + 8]
        .copy_from_slice(&0.254f64.to_le_bytes());
    bytes[layout::RESULT_COUNT..layout::RESULT_COUNT + 4].copy_from_slice(&5u32.to_le_bytes());
    for (ordinal, (record_index, trailer)) in
        [(501u32, 1u32), (502, 1), (503, 1), (504, 1), (505, 0)]
            .into_iter()
            .enumerate()
    {
        let result_offset = layout::RESULT_ONE_REFERENCE + ordinal * 15;
        write_marked_reference(&mut bytes, result_offset, record_index);
        bytes[layout::RESULT_ONE_TRAILER + ordinal * 15
            ..layout::RESULT_ONE_TRAILER + ordinal * 15 + 4]
            .copy_from_slice(&trailer.to_le_bytes());
    }
    bytes[layout::RESULT_SEPARATOR..layout::RESULT_SEPARATOR + 4]
        .copy_from_slice(&1u32.to_le_bytes());
    write_marked_reference(&mut bytes, layout::AGGREGATE_GROUP_REFERENCE, 231);
    write_marked_reference(&mut bytes, layout::EDGE_GROUP_ONE_REFERENCE, 204);
    write_marked_reference(&mut bytes, layout::EDGE_GROUP_TWO_REFERENCE, 216);

    EdgeFlangeFrame {
        bytes,
        paired_at,
        bend_radius_offset: u64::try_from(layout::INSIDE_BEND_RADIUS)
            .expect("bend radius offset fits u64"),
    }
}

#[test]
fn legacy_edge_flange_result_table_rejects_each_duplicate_without_heap_growth() {
    use crate::layout::edge_flange_class325_334_two_sided_per_edge_fixed_operation as layout;
    let references = [
        201, 204, 207, 210, 213, 216, 219, 222, 225, 228, 231, 234, 237, 240, 243, 246,
    ];
    let frame = legacy_class325_two_sided_per_edge_flange_frame();
    let decode = |bytes: &[u8]| {
        crate::design::decode::scopes::sheet_metal::exact_edge_flange_operation(
            bytes,
            0,
            frame.paired_at,
            "325",
            "258",
            &references,
        )
        .expect("decode the exact edge flange operation")
    };
    assert!(decode(&frame.bytes).is_some());
    for ordinal in 1..5 {
        let mut bytes = frame.bytes.clone();
        write_marked_reference(&mut bytes, layout::RESULT_ONE_REFERENCE + ordinal * 15, 501);
        assert!(decode(&bytes).is_none(), "duplicate at result {ordinal}");
    }
}
