// SPDX-License-Identifier: Apache-2.0

use crate::records::feature::sheet_metal::{
    DesignEdgeFlangeEdge, DesignEdgeFlangeOperation, DesignEdgeFlangeSelection,
    DesignEdgeFlangeShape, DesignHemOperation,
};

fn edge_flange_wire_for_borrowed(mode: &str) -> serde_json::Value {
    let (owners, owners_by_edge) = match mode {
        "full_edge" => (serde_json::json!([]), serde_json::json!([])),
        "symmetric" => (serde_json::json!([50]), serde_json::json!([])),
        "two_sides" => (serde_json::json!([50, 51]), serde_json::json!([])),
        "symmetric_per_edge" => (serde_json::json!([50, 51]), serde_json::json!([])),
        "two_sides_per_edge" => (
            serde_json::json!([50, 51, 52, 53]),
            serde_json::json!([[50, 51], [52, 53]]),
        ),
        _ => unreachable!(),
    };
    serde_json::json!({
        "edge_wrapper_record_indices":[10,11], "edge_group_record_indices":[20,21],
        "edge_operand_record_indices":[23,24], "aggregate_group_record_index":30,
        "aggregate_operand_record_indices":[34,35], "height_owner_record_index":40,
        "angle_owner_record_index":41, "width_mode":mode,
        "width_distance_owner_record_indices":owners,
        "width_distance_owner_record_indices_by_edge":owners_by_edge,
        "settings_record_index":42, "bend_radius":0.25, "bend_radius_offset":50,
        "reference_side_code":4, "height_datum":"inner_faces", "bend_position":"adjacent"
    })
}

#[test]
fn edge_flange_operation_borrowed_wire_matches_owned_wire_bytes() {
    for mode in [
        "full_edge",
        "symmetric",
        "two_sides",
        "symmetric_per_edge",
        "two_sides_per_edge",
    ] {
        let wire = edge_flange_wire_for_borrowed(mode);
        let operation: DesignEdgeFlangeOperation = serde_json::from_value(wire).unwrap();
        let owned = super::DesignEdgeFlangeOperationSerde::from(operation.clone());
        assert_eq!(
            serde_json::to_vec(&operation).unwrap(),
            serde_json::to_vec(&owned).unwrap()
        );
    }
}

#[test]
fn edge_flange_operation_native_retained_limit_refuses_before_clone() {
    #[derive(serde::Serialize)]
    struct NestedRecord<'a> {
        id: &'static str,
        value: &'a DesignEdgeFlangeOperation,
    }
    let operation: DesignEdgeFlangeOperation =
        serde_json::from_value(edge_flange_wire_for_borrowed("two_sides_per_edge")).unwrap();
    let record = NestedRecord {
        id: "f3d:native:edge-flange#0",
        value: &operation,
    };
    crate::test_support::native_test::assert_borrowed_native_retained_limit(
        &record,
        "design_parameter_scopes",
        || super::EDGE_FLANGE_OPERATION_CLONE_COUNT.with(|count| count.set(0)),
        || super::EDGE_FLANGE_OPERATION_CLONE_COUNT.with(std::cell::Cell::get),
    );
}

#[test]
fn hem_operand_indices_derive_from_groups_and_reject_wire_disagreement() {
    let wire = serde_json::json!({
        "edge_wrapper_record_index":1, "edge_group_record_index":2, "edge_operand_record_index":5,
        "aggregate_group_record_index":6, "aggregate_operand_record_index":9,
        "parameter_owners":{"kind":"gap_length", "gap_owner_record_index":10, "length_owner_record_index":11},
        "settings_record_index":12, "bend_radius":0.25, "bend_radius_offset":100,
        "bend_position": "adjacent", "flipped": false
    });
    let mut flipped = wire.clone();
    flipped["flipped"] = true.into();
    let reversed: DesignHemOperation = serde_json::from_value(flipped.clone()).unwrap();
    assert!(reversed.flipped);
    assert_eq!(serde_json::to_value(reversed).unwrap(), flipped);
    for (field, value) in [
        ("flipped", serde_json::json!(2)),
        ("bend_position", serde_json::json!("unknown")),
        ("bend_position", serde_json::json!("tangent")),
        ("direction_reversal_byte", serde_json::json!(0)),
    ] {
        let mut invalid = wire.clone();
        invalid[field] = value;
        assert!(serde_json::from_value::<DesignHemOperation>(invalid).is_err());
    }
    let mut operation: DesignHemOperation = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(serde_json::to_value(operation).unwrap(), wire);
    for (group, operand) in [
        ("edge_group_record_index", "edge_operand_record_index"),
        (
            "aggregate_group_record_index",
            "aggregate_operand_record_index",
        ),
    ] {
        let mut invalid = wire.clone();
        invalid[group] = serde_json::json!(u32::MAX);
        invalid[operand] = serde_json::json!(u32::MAX);
        assert!(serde_json::from_value::<DesignHemOperation>(invalid).is_err());
    }
    operation.edge_group_record_index = 20_u32.try_into().unwrap();
    operation.aggregate_group_record_index = 30_u32.try_into().unwrap();
    assert_eq!(operation.edge_operand_record_index(), 23);
    assert_eq!(operation.aggregate_operand_record_index(), 33);
    let changed = serde_json::to_value(operation).unwrap();
    assert_eq!(changed["edge_operand_record_index"], 23);
    assert_eq!(changed["aggregate_operand_record_index"], 33);
    for field in [
        "edge_operand_record_index",
        "aggregate_operand_record_index",
    ] {
        let mut invalid = wire.clone();
        invalid[field] = serde_json::json!(99);
        assert!(serde_json::from_value::<DesignHemOperation>(invalid).is_err());
    }
}

#[test]
fn flange_selection_couples_single_edge_aggregate_and_preserves_multiple_operands() {
    assert!(
        DesignEdgeFlangeEdge::from_columns(vec![1], vec![u32::MAX], &[u32::MAX], vec![3]).is_err()
    );
    assert!(super::DesignRecipeGroupIndex::try_from(u32::MAX).is_err());
    assert_eq!(
        super::DesignRecipeGroupIndex::try_from(u32::MAX - 3)
            .unwrap()
            .operand(),
        u32::MAX
    );
    let mut edge = DesignEdgeFlangeEdge {
        wrapper: 10,
        group_record_index: 20_u32.try_into().unwrap(),
        aggregate_operand_record_index: 33,
    };
    assert_eq!(edge.operand_record_index(), 23);
    edge.group_record_index = 25_u32.try_into().unwrap();
    assert_eq!(edge.operand_record_index(), 28);
    assert!(DesignEdgeFlangeSelection::try_new(
        DesignEdgeFlangeShape::FullEdge {
            edges: vec![edge],
            height: crate::records::feature::sheet_metal::DesignEdgeFlangeHeightExtent::Distance
        },
        30
    )
    .is_ok());
    edge.aggregate_operand_record_index = 34;
    assert!(DesignEdgeFlangeSelection::try_new(
        DesignEdgeFlangeShape::FullEdge {
            edges: vec![edge],
            height: crate::records::feature::sheet_metal::DesignEdgeFlangeHeightExtent::Distance
        },
        30
    )
    .is_err());
    assert!(DesignEdgeFlangeSelection::try_new(
        DesignEdgeFlangeShape::FullEdge {
            edges: vec![
                edge,
                DesignEdgeFlangeEdge {
                    wrapper: 11,
                    group_record_index: 26_u32.try_into().unwrap(),
                    aggregate_operand_record_index: 35
                }
            ],
            height: crate::records::feature::sheet_metal::DesignEdgeFlangeHeightExtent::Distance
        },
        30
    )
    .is_ok());
    let wire = serde_json::json!({
        "edge_wrapper_record_indices":[10], "edge_group_record_indices":[20], "edge_operand_record_indices":[23],
        "aggregate_group_record_index":30, "aggregate_operand_record_indices":[33],
        "height_owner_record_index":40, "angle_owner_record_index":41, "width_mode":"full_edge",
        "width_distance_owner_record_indices":[], "settings_record_index":42, "bend_radius":0.25,
        "bend_radius_offset":50, "reference_side_code":4, "height_datum":"inner_faces", "bend_position":"adjacent"
    });
    assert!(serde_json::from_value::<DesignEdgeFlangeOperation>(wire.clone()).is_ok());
    for field in [
        "edge_operand_record_indices",
        "aggregate_operand_record_indices",
    ] {
        let mut invalid = wire.clone();
        invalid[field] = serde_json::json!([99]);
        assert!(serde_json::from_value::<DesignEdgeFlangeOperation>(invalid).is_err());
    }
    let mut multiple = wire;
    multiple["edge_wrapper_record_indices"] = serde_json::json!([10, 11]);
    multiple["edge_group_record_indices"] = serde_json::json!([20, 21]);
    multiple["edge_operand_record_indices"] = serde_json::json!([23, 24]);
    multiple["aggregate_operand_record_indices"] = serde_json::json!([34, 35]);
    assert!(serde_json::from_value::<DesignEdgeFlangeOperation>(multiple).is_ok());
}
