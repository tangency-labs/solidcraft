use super::{AddParams, EditParams, MarkParams};
use crate::schema::input_schema_for;
use crate::schema::tests::assert_keys;
use serde_json::json;

#[test]
fn command_schemas_list_their_fields() {
    assert_keys(input_schema_for::<AddParams>(), &["edge", "overshoot_mm", "view"]);
    assert_keys(input_schema_for::<EditParams>(), &["mark", "overshoot_mm"]);
    assert_keys(input_schema_for::<MarkParams>(), &["mark"]);
}

#[test]
fn contracts_reject_unknown_fields_and_malformed_identifiers() {
    assert!(serde_json::from_value::<MarkParams>(json!({"mark": "not-an-id"})).is_err());
    assert!(serde_json::from_value::<MarkParams>(json!({"mark": 1})).is_err());
    assert!(serde_json::from_value::<MarkParams>(json!({"mark": "0000000000001", "extra": true})).is_err());
    assert!(serde_json::from_value::<EditParams>(json!({"mark": "0000000000001", "overshoot_mm": null})).is_err());
    assert!(serde_json::from_value::<AddParams>(json!({"view": "0000000000001", "edge": [0, 0, 0]})).is_err());
}

#[test]
fn centre_line_contracts_describe_references_and_reject_malformed_input() {
    use super::super::drawing_centre_line::{AddParams as LineAdd, EditParams as LineEdit, LineParams};
    assert_keys(input_schema_for::<LineAdd>(), &["a", "b", "overshoot_mm", "view"]);
    assert_keys(input_schema_for::<LineEdit>(), &["line", "overshoot_mm"]);
    assert_keys(input_schema_for::<LineParams>(), &["line"]);
    assert!(serde_json::from_value::<LineParams>(json!({"line":"not-an-id"})).is_err());
    assert!(serde_json::from_value::<LineParams>(json!({"line":"0000000000001","extra":true})).is_err());
    assert!(serde_json::from_value::<LineAdd>(json!({"view":"0000000000001","a":[0,0,0],"b":[1,0,0]})).is_err());
}
