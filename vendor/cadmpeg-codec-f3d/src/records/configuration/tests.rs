// SPDX-License-Identifier: Apache-2.0

use super::{
    encode_configuration_payload, DesignConfiguration, DesignConfigurationWire,
    CONFIGURATION_CLONE_COUNT,
};
use crate::test_support::configuration_table::{object_target, table_document};
use serde_json::{json, Value};

fn wire(kind: &str, payload: &Value) -> Value {
    let name = if kind == "rule" {
        "entry.dsgcfgrule"
    } else {
        "entry.dsgcfg"
    };
    json!({
        "id": crate::ids::configuration_entry_id(
            name,
            &cadmpeg_ir::identity_component!("configuration"),
        ),
        "entry_name": name,
        "kind": kind,
        "payload": payload,
    })
}

fn active_row() -> Value {
    json!({"record_index": 349, "table_id": "1", "row_id": "r"})
}

fn populated() -> Value {
    table_document(
        "1",
        &[("H", object_target("202", "UserParameterValue"))],
        &[("r", "Row", vec![])],
        &["r"],
    )
}

#[test]
fn configuration_borrowed_wire_matches_owned_wire_bytes() {
    let mut with_row = wire("table", &populated());
    with_row["active_row"] = active_row();
    for value in [
        wire("table", &json!({})),
        wire("table", &json!({"before": [1, null], "after": false})),
        with_row,
        wire("rule", &json!({"when": {"x": [1, 2]}, "activate": "a"})),
    ] {
        let record: DesignConfiguration = serde_json::from_value(value.clone()).unwrap();
        let owned = DesignConfigurationWire::from(record.clone());
        assert_eq!(
            serde_json::to_vec(&record).unwrap(),
            serde_json::to_vec(&owned).unwrap()
        );
        assert_eq!(serde_json::to_value(&record).unwrap(), value);
    }
    let name = "a 😀# b.dsgcfg";
    let mut escaped = wire("table", &json!({}));
    escaped["entry_name"] = json!(name);
    escaped["id"] = json!(crate::ids::configuration_entry_id(
        name,
        &cadmpeg_ir::identity_component!("configuration")
    ));
    let record: DesignConfiguration = serde_json::from_value(escaped).unwrap();
    let owned = DesignConfigurationWire::from(record.clone());
    assert_eq!(
        serde_json::to_vec(&record).unwrap(),
        serde_json::to_vec(&owned).unwrap()
    );
}

#[test]
fn configuration_native_retained_limit_refuses_before_record_clone() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let record: DesignConfiguration = serde_json::from_value(wire("table", &populated())).unwrap();
    let arena_name = "design_configurations";
    let needed = serde_json::to_vec(&record).unwrap().len();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = u64::try_from(needed).unwrap() - 1;
    let (limited, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    CONFIGURATION_CLONE_COUNT.with(|count| count.set(0));
    let error = namespace
        .set_arena(&limited, arena_name, std::slice::from_ref(&record))
        .unwrap_err();
    CONFIGURATION_CLONE_COUNT.with(|count| assert_eq!(count.get(), 0));
    assert!(matches!(
        cadmpeg_core::CodecError::from(error),
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "serialize native record"
    ));

    let (service, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    namespace
        .set_arena(&service, arena_name, std::slice::from_ref(&record))
        .unwrap();
    assert_eq!(namespace.arenas()[arena_name].len(), 1);
}

#[test]
fn configuration_payload_is_retained_and_encoded_verbatim() {
    for payload in [
        json!({}),
        populated(),
        json!({"configurations": {"v": {}}, "active": "v"}),
        json!({"nested": [1, null, {"x": -0.0}], "text": "é"}),
    ] {
        let expected = wire("table", &payload.clone());
        let admitted: DesignConfiguration = serde_json::from_value(expected.clone()).unwrap();
        assert_eq!(serde_json::to_value(&admitted).unwrap(), expected);
        assert_eq!(
            serde_json::from_slice::<Value>(&encode_configuration_payload(&admitted).unwrap())
                .unwrap(),
            payload
        );
    }
}

#[test]
fn configuration_table_members_outside_the_table_form_are_counted() {
    let admitted: DesignConfiguration =
        serde_json::from_value(wire("table", &json!({"configurations": {}, "active": "v"})))
            .unwrap();
    assert!(admitted.tables().is_empty());
    assert_eq!(admitted.unknown_member_count(), 2);
    let admitted: DesignConfiguration =
        serde_json::from_value(wire("table", &populated())).unwrap();
    assert_eq!(admitted.tables().len(), 1);
    assert_eq!(admitted.unknown_member_count(), 0);
    let rule: DesignConfiguration = serde_json::from_value(wire(
        "rule",
        &json!({"when": "x", "activate": "y", "configurations": {}}),
    ))
    .unwrap();
    assert!(rule.tables().is_empty());
    assert_eq!(rule.unknown_member_count(), 1);
}

#[test]
fn configuration_active_row_belongs_to_a_table_document() {
    let mut value = wire("table", &populated());
    value["active_row"] = active_row();
    let record: DesignConfiguration = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(record.active_row().unwrap().row_id, "r");
    assert_eq!(serde_json::to_value(&record).unwrap(), value);

    let mut rule = wire("rule", &json!({}));
    rule["active_row"] = active_row();
    assert!(serde_json::from_value::<DesignConfiguration>(rule)
        .unwrap_err()
        .to_string()
        .contains("active row"));

    for (field, invalid) in [
        ("active_row", Value::Null),
        ("active_row", json!({"record_index": 1, "table_id": "1"})),
        (
            "active_row",
            json!({"record_index": 1, "table_id": "1", "row_id": "r", "extra": 0}),
        ),
        ("variant_order", json!([])),
    ] {
        let mut value = wire("table", &populated());
        value[field] = invalid;
        assert!(
            serde_json::from_value::<DesignConfiguration>(value).is_err(),
            "{field}"
        );
    }
}

mod admission;
