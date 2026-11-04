// SPDX-License-Identifier: Apache-2.0
//! Edge operand wire: the resolved axis is whole or absent.

fn edge_identity_operand(compact: bool, local_id_offset: u64) -> super::DesignEdgeIdentityOperand {
    let wire = serde_json::json!({
        "id": "identity", "scope_record_index": 4, "group_record_index": 1,
        "group_member_ordinal": 0, "record_index": 2, "byte_offset": 10,
        "class_tag": "346", "compact_layout": compact, "local_id": 17,
        "local_id_offset": local_id_offset,
        "asset_id": "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d",
        "asset_id_offset": local_id_offset + 18,
        "context_id": "1b2c3d4e-5f6a-4b7c-8d9e-0f1a2b3c4d5e",
        "context_id_offset": local_id_offset + 94
    });
    serde_json::from_value(wire).unwrap()
}

#[test]
fn edge_identity_operand_borrowed_wire_matches_owned_wire_bytes() {
    for (compact, offset) in [(false, 34), (true, 33), (true, 32)] {
        let mut operand = edge_identity_operand(compact, offset);
        for resolution in [None, Some("resolved#1".to_owned())] {
            operand.resolution_identity_id = resolution;
            operand.transition_edge_candidates = vec![7, 9];
            operand.resolved_edge_slots = vec![7];
            let owned = super::DesignEdgeIdentityOperandWire::from(operand.clone());
            assert_eq!(
                serde_json::to_vec(&operand).unwrap(),
                serde_json::to_vec(&owned).unwrap()
            );
        }
    }
}

#[test]
fn edge_identity_operand_native_retained_limit_refuses_before_clone() {
    #[derive(serde::Serialize)]
    struct NestedRecord<'a> {
        id: &'static str,
        value: &'a super::DesignEdgeIdentityOperand,
    }
    let operand = edge_identity_operand(false, 34);
    let record = NestedRecord {
        id: "f3d:native:edge-identity#0",
        value: &operand,
    };
    crate::test_support::native_test::assert_borrowed_native_retained_limit(
        &record,
        "design_parameter_scopes",
        || super::EDGE_IDENTITY_OPERAND_CLONE_COUNT.with(|count| count.set(0)),
        || super::EDGE_IDENTITY_OPERAND_CLONE_COUNT.with(std::cell::Cell::get),
    );
}

#[test]
fn edge_operand_wire_rejects_partial_resolved_axis() {
    let prefix = r#"{"id":"edge","scope_record_index":1,"scope_reference_ordinal":0,"record_index":2,"byte_offset":10,"class_tag":"346","paired_byte_offset":20,"paired_class_tag":"262","recipe_record_index":5,"recipe_record_byte_offset":30,"recipe_id":"recipe","recipe_prefix_offset":41,"recipe_prefix_bytes":"","recipe_references":[],"recipe_program_offset":50,"recipe_program":[]"#;
    let suffix = r#","next_record_index":4,"next_byte_offset":100}"#;
    let origin = serde_json::to_string(&cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0)).unwrap();
    let direction = serde_json::to_string(&cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0)).unwrap();
    for fields in [
        String::new(),
        format!(",\"resolved_axis_origin\":{origin},\"resolved_axis_direction\":{direction}"),
    ] {
        let wire = format!("{prefix}{fields}{suffix}");
        let operand: super::DesignEdgeOperand = serde_json::from_str(&wire).unwrap();
        assert_eq!(serde_json::to_string(&operand).unwrap(), wire);
    }
    for (field, value) in [
        ("resolved_axis_origin", origin),
        ("resolved_axis_direction", direction),
    ] {
        let invalid = format!("{prefix},\"{field}\":{value}{suffix}");
        let error = serde_json::from_str::<super::DesignEdgeOperand>(&invalid)
            .unwrap_err()
            .to_string();
        assert!(error.contains("resolved_axis_origin"));
        assert!(error.contains("resolved_axis_direction"));
    }
}

fn edge_operand_for_borrowed(axis: bool) -> super::DesignEdgeOperand {
    let mut wire = serde_json::json!({
        "id":"f3d:native:edge-operand#0", "scope_record_index":1,
        "scope_reference_ordinal":0, "record_index":2,"byte_offset":10,
        "class_tag":"346","paired_byte_offset":20,"paired_class_tag":"262",
        "recipe_record_index":5,"recipe_record_byte_offset":30,
        "recipe_id":"recipe","recipe_prefix_offset":41,"recipe_prefix_bytes":"",
        "recipe_references":[],"recipe_program_offset":50,"recipe_program":[],
        "next_record_index":4,"next_byte_offset":100
    });
    if axis {
        wire["resolved_axis_origin"] =
            serde_json::to_value(cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0)).unwrap();
        wire["resolved_axis_direction"] =
            serde_json::to_value(cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0)).unwrap();
    }
    serde_json::from_value(wire).unwrap()
}

#[test]
fn edge_operand_borrowed_wire_matches_owned_wire_bytes() {
    for axis in [false, true] {
        let operand = edge_operand_for_borrowed(axis);
        let owned = super::DesignEdgeOperandDraft::from(operand.clone());
        assert_eq!(
            serde_json::to_vec(&operand).unwrap(),
            serde_json::to_vec(&owned).unwrap()
        );
    }
}

#[test]
fn edge_operand_native_retained_limit_refuses_before_clone() {
    let operand = edge_operand_for_borrowed(true);
    crate::test_support::native_test::assert_borrowed_native_retained_limit(
        &operand,
        "design_edge_operands",
        || super::EDGE_OPERAND_CLONE_COUNT.with(|count| count.set(0)),
        || super::EDGE_OPERAND_CLONE_COUNT.with(std::cell::Cell::get),
    );
}
