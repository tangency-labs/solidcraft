// SPDX-License-Identifier: Apache-2.0
//! Records that two families own together.

#[test]
fn historical_binding_wire_rejects_partial_identity_and_orphan_states() {
    fn check<T>(base: &serde_json::Value)
    where
        T: serde::de::DeserializeOwned + serde::Serialize + std::fmt::Debug,
    {
        for binding in [
            serde_json::json!({}),
            serde_json::json!({"historical_entity_kind": "loop", "historical_entity_ref": 42}),
            serde_json::json!({"historical_entity_kind": "loop", "historical_entity_ref": 42, "historical_state_ids": [2, 3]}),
        ] {
            let mut wire = base.clone();
            wire.as_object_mut()
                .unwrap()
                .extend(binding.as_object().unwrap().clone());
            let value: T = serde_json::from_value(wire.clone()).unwrap();
            assert_eq!(serde_json::to_value(value).unwrap(), wire);
        }
        for binding in [
            serde_json::json!({"historical_entity_kind": "loop"}),
            serde_json::json!({"historical_entity_ref": 42}),
            serde_json::json!({"historical_state_ids": [2]}),
            serde_json::json!({"historical_entity_kind": "loop", "historical_state_ids": [2]}),
            serde_json::json!({"historical_entity_ref": 42, "historical_state_ids": [2]}),
        ] {
            let mut invalid = base.clone();
            invalid
                .as_object_mut()
                .unwrap()
                .extend(binding.as_object().unwrap().clone());
            let error = serde_json::from_value::<T>(invalid)
                .unwrap_err()
                .to_string();
            assert!(error.contains("historical_entity_kind"));
            assert!(error.contains("historical_entity_ref"));
            assert!(error.contains("historical_state_ids"));
        }
    }
    let mut member = serde_json::json!({
        "id": "member", "group_record_index": 1, "group_member_ordinal": 0,
        "record_index": 2, "byte_offset": 10, "class_tag": "346",
        "local_id": 17, "local_id_offset": 31,
        "asset_id": "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d", "asset_id_offset": 43,
        "context_id": "1b2c3d4e-5f6a-4b7c-8d9e-0f1a2b3c4d5e", "context_id_offset": 120,
        "tail_slot_present": false, "tail_slot_offset": 0,
        "next_record_index": 3, "next_byte_offset": 200
    });
    check::<crate::records::topology::extrude_selection::DesignExtrudeSelectionMember>(&member);
    for field in ["asset_id", "context_id"] {
        let mut invalid = member.clone();
        invalid[field] = serde_json::json!("asset");
        assert!(serde_json::from_value::<
            crate::records::topology::extrude_selection::DesignExtrudeSelectionMember,
        >(invalid)
        .expect_err("non-GUID selection identity")
        .to_string()
        .contains("GUID"));
    }
    for field in [
        "tail_slot_present",
        "tail_slot_offset",
        "next_record_index",
        "next_byte_offset",
    ] {
        member.as_object_mut().unwrap().remove(field);
    }
    member["scope_record_index"] = serde_json::json!(4);
    member["compact_layout"] = serde_json::json!(false);
    member["local_id_offset"] = serde_json::json!(34);
    member["asset_id_offset"] = serde_json::json!(52);
    member["context_id_offset"] = serde_json::json!(128);
    check::<crate::records::topology::edge_identity::DesignEdgeIdentityOperand>(&member);
    for (compact, local_id_offset) in [(true, 32), (true, 33), (false, 34)] {
        let mut framed = member.clone();
        framed["compact_layout"] = compact.into();
        framed["local_id_offset"] = local_id_offset.into();
        framed["asset_id_offset"] = (local_id_offset + 18).into();
        framed["context_id_offset"] = (local_id_offset + 94).into();
        let operand = serde_json::from_value::<
            crate::records::topology::edge_identity::DesignEdgeIdentityOperand,
        >(framed)
        .expect("edge-identity prologue framing");
        assert_eq!(operand.local_id_offset(), local_id_offset);
        assert_eq!(operand.layout().is_compact(), compact);
    }
    for (compact, local_id_offset) in [(false, 32), (false, 33), (true, 34), (false, 20)] {
        let mut framed = member.clone();
        framed["compact_layout"] = compact.into();
        framed["local_id_offset"] = local_id_offset.into();
        framed["asset_id_offset"] = (local_id_offset + 18).into();
        framed["context_id_offset"] = (local_id_offset + 94).into();
        assert!(
            serde_json::from_value::<
                crate::records::topology::edge_identity::DesignEdgeIdentityOperand,
            >(framed)
            .is_err(),
            "compact_layout {compact} with local_id_offset {local_id_offset}"
        );
    }
    for field in ["asset_id", "context_id"] {
        let mut invalid = member.clone();
        invalid[field] = serde_json::json!("asset");
        assert!(serde_json::from_value::<
            crate::records::topology::edge_identity::DesignEdgeIdentityOperand,
        >(invalid)
        .expect_err("non-GUID edge identity")
        .to_string()
        .contains("GUID"));
    }
}
