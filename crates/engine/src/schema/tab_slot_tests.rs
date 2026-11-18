use serde_json::json;

use super::{TabSlotParams, TabSlotResult};
use crate::Session;
use crate::schema::tests::{assert_keys, conforms};
use crate::schema::{input_schema_for, output_schema_for};

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(
        input_schema_for::<TabSlotParams>(),
        &[
            "body_name",
            "body_names",
            "clearance",
            "count",
            "end_distance",
            "name",
            "participants",
            "protrusion",
            "seed",
            "slot_plates",
            "spacing",
            "tab_plates",
            "tab_width",
            "width_jitter",
        ],
    );
}

#[test]
fn the_contract_refuses_what_the_handler_refuses() {
    let schema = input_schema_for::<TabSlotParams>();
    let root = schema.as_value();
    let ok = json!({"tab_plates": ["Wall", {"body": "Back", "near": [0, 0, 0]}], "slot_plates": ["Bottom"], "count": 3, "clearance": "0.1 mm"});
    assert!(conforms(root, root, &ok));
    for bad in [json!({"slot_plates": ["Bottom"]}), json!({"tab_plates": ["Wall"], "slot_plates": ["Bottom"], "extra": 1})] {
        assert!(!conforms(root, root, &bad), "{bad}");
    }
}

#[test]
fn the_reply_is_a_tab_slot_result() {
    let mut s = Session::default();
    for (id, p) in [
        ("solid.box", json!({"corner": [0, 0, 0], "length": 160, "width": 100, "height": 6, "body_name": "Bottom", "operation": "new"})),
        ("solid.box", json!({"corner": [20, 30, 6], "length": 120, "width": 6, "height": 60, "body_name": "Wall", "operation": "new"})),
    ] {
        s.execute(id, &p).unwrap_or_else(|e| panic!("{id}: {e}"));
    }
    let reply = s.execute("solid.tab_slot", &json!({"tab_plates": ["Wall"], "slot_plates": ["Bottom"], "count": 3})).unwrap();
    let schema = output_schema_for::<TabSlotResult>();
    let root = schema.as_value();
    assert!(conforms(root, root, &reply), "{reply}");
    assert_eq!(reply["tabs"].as_u64(), Some(3), "{reply}");
}
