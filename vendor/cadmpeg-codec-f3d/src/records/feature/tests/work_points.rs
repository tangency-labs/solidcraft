// SPDX-License-Identifier: Apache-2.0

#[test]
fn work_point_rule_moves_resolved_input_without_cloning_carrier() {
    use crate::records::feature::work_geometry::{
        DesignWorkPointInput, DesignWorkPointInputCarrier, DesignWorkPointRule,
        WORK_GEOMETRY_CLONE_COUNT,
    };

    let input = DesignWorkPointInput::try_new(
        7,
        14,
        Some(Box::new(DesignWorkPointInputCarrier::EdgeRecipe {
            operand_id: "f3d:Design/BulkStream.dat:edge#7".to_owned(),
        })),
    )
    .unwrap();
    WORK_GEOMETRY_CLONE_COUNT.with(|count| count.set(0));
    let rule = DesignWorkPointRule::from_serialized(5, vec![input]).unwrap();
    assert!(matches!(
        rule.inputs()[0].carrier(),
        Some(DesignWorkPointInputCarrier::EdgeRecipe { operand_id })
            if operand_id == "f3d:Design/BulkStream.dat:edge#7"
    ));
    WORK_GEOMETRY_CLONE_COUNT.with(|count| assert_eq!(count.get(), 0));
}

#[test]
fn work_point_rules_preserve_supported_and_native_forms_without_aliases() {
    use crate::records::feature::work_geometry::DesignWorkPointRule;

    let input = serde_json::json!({"record_index": 2, "reference_offset": 10});
    for (kind, code, arity) in [
        ("circle_center", 5, 1),
        ("two_edge_intersection", 7, 2),
        ("three_plane_intersection", 8, 3),
        ("vertex", 10, 1),
        ("edge_plane_intersection", 14, 2),
        ("distance_on_edge", 20, 1),
    ] {
        let inputs = (0..arity).map(|_| input.clone()).collect::<Vec<_>>();
        let mut wire = serde_json::json!({"kind": kind});
        if arity == 1 {
            wire["input"] = input.clone();
        } else {
            wire["inputs"] = serde_json::json!(inputs);
        }
        let rule: DesignWorkPointRule =
            serde_json::from_value(wire.clone()).expect("supported rule");
        assert_eq!(rule.reference_type(), code);
        assert_eq!(serde_json::to_value(rule).expect("serialize rule"), wire);
        let alias = serde_json::json!({"kind": "native", "reference_type": code, "inputs": inputs});
        let error = serde_json::from_value::<DesignWorkPointRule>(alias)
            .expect_err("native alias rejected");
        assert!(error.to_string().contains("reference_type"));
    }
    for (code, inputs) in [
        (0, vec![]),
        (u32::MAX, vec![input.clone()]),
        (5, vec![input.clone(), input]),
    ] {
        let wire = serde_json::json!({"kind": "native", "reference_type": code, "inputs": inputs});
        let rule: DesignWorkPointRule =
            serde_json::from_value(wire.clone()).expect("unassigned native form");
        assert_eq!(
            serde_json::to_value(rule).expect("serialize native form"),
            wire
        );
    }
}

#[test]
fn work_point_plane_carrier_is_bound_to_its_input_frame() {
    use crate::records::feature::work_geometry::DesignWorkPointInput;
    let wire = serde_json::json!({
        "record_index": 7, "reference_offset": 0,
        "carrier": { "kind": "work_plane", "selection": {
            "class_tag": "300",
            "asset_id": "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d", "asset_id_offset": 32,
            "context_id": "1b2c3d4e-5f6a-4b7c-8d9e-0f1a2b3c4d5e", "context_id_offset": 108,
            "identity_record_index": 10, "identity_record_offset": 150,
            "primary_identity": 19, "primary_identity_offset": 171,
            "work_plane_scope_record_index": 20, "next_record_index": 11, "next_byte_offset": 179
        }}
    });
    let admitted: DesignWorkPointInput = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(serde_json::to_value(admitted).unwrap(), wire);
    for field in ["identity_record_index", "next_record_index"] {
        let mut invalid = wire.clone();
        invalid["carrier"]["selection"][field] = 12.into();
        assert!(serde_json::from_value::<DesignWorkPointInput>(invalid).is_err());
    }
}

#[test]
fn work_point_sketch_point_carrier_is_bound_to_its_input_frame() {
    use crate::records::feature::work_geometry::DesignWorkPointInput;
    let wire = serde_json::json!({
        "record_index": 7, "reference_offset": 0,
        "carrier": { "kind": "sketch_point", "selection": {
            "class_tag": "300",
            "asset_id": "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d", "asset_id_offset": 32,
            "context_id": "1b2c3d4e-5f6a-4b7c-8d9e-0f1a2b3c4d5e", "context_id_offset": 108,
            "identity_record_index": 10, "identity_record_offset": 150,
            "sketch_record_index": 19, "sketch_record_index_offset": 175,
            "point_persistent_id": 23, "point_persistent_id_offset": 183,
            "point_native_id": "f3d:native/BulkStream.dat:sketch-point#19",
            "next_record_index": 11, "next_byte_offset": 191
        }}
    });
    let admitted: DesignWorkPointInput = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(serde_json::to_value(admitted).unwrap(), wire);
    for field in ["identity_record_index", "next_record_index"] {
        let mut invalid = wire.clone();
        invalid["carrier"]["selection"][field] = 12.into();
        assert!(serde_json::from_value::<DesignWorkPointInput>(invalid).is_err());
    }
}
