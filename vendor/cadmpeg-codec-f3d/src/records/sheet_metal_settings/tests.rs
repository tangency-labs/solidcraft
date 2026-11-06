// SPDX-License-Identifier: Apache-2.0

#[test]
fn settings_wire_couples_version_arity_and_source_identity() {
    use super::DesignSheetMetalSettings;
    for (version, count, tail) in [
        ("v2", 7, None),
        ("v3", 9, Some("0000")),
        ("v3", 9, Some("010000")),
    ] {
        let mut wire = serde_json::json!({
            "id": "f3d:Design1/BulkStream.dat:design-sheet-metal-settings#20",
            "record_index": 50, "byte_offset": 20, "paired_byte_offset": 100,
            "mask": 240, "inputs": {"version": version, "slots": vec![None::<u64>; count]},
            "rule_record_index": 51, "presentation_record_index": 52,
            "owner_record_index": 53, "context_record_index": 54,
        });
        if let Some(tail) = tail {
            wire["inputs"]["rule_reference_tail"] = serde_json::json!(tail);
        }
        let record: DesignSheetMetalSettings = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(record).unwrap(), wire);
        let mut invalid = wire.clone();
        invalid["inputs"]["rule_reference_tail"] = serde_json::json!("000000");
        assert!(serde_json::from_value::<DesignSheetMetalSettings>(invalid).is_err());
        if tail.is_some() {
            let mut invalid = wire.clone();
            invalid["inputs"]
                .as_object_mut()
                .unwrap()
                .remove("rule_reference_tail");
            assert!(serde_json::from_value::<DesignSheetMetalSettings>(invalid).is_err());
        }
        for (field, value) in [
            ("byte_offset", serde_json::json!(21)),
            ("paired_byte_offset", serde_json::json!(20)),
            ("unexpected", serde_json::json!(0)),
        ] {
            let mut invalid = wire.clone();
            invalid[field] = value;
            assert!(serde_json::from_value::<DesignSheetMetalSettings>(invalid).is_err());
        }
        let mut invalid = wire;
        invalid["inputs"]["slots"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::Value::Null);
        assert!(serde_json::from_value::<DesignSheetMetalSettings>(invalid).is_err());
    }
}
