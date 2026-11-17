use serde_json::json;

use super::{LivingHingeParams, LivingHingeResult};
use crate::Session;
use crate::schema::tests::{assert_keys, conforms};
use crate::schema::{input_schema_for, output_schema_for};

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(
        input_schema_for::<LivingHingeParams>(),
        &[
            "anchor",
            "angle",
            "body_name",
            "body_names",
            "bridge",
            "clip",
            "margin",
            "name",
            "participants",
            "profiles",
            "row_spacing",
            "sketch",
            "slot_length",
            "slot_width",
            "targets",
        ],
    );
}

#[test]
fn the_contract_refuses_what_the_handler_refuses() {
    let schema = input_schema_for::<LivingHingeParams>();
    let root = schema.as_value();
    let ok = json!({"slot_length": 20, "bridge": 3, "row_spacing": 1.5, "clip": "shorten", "anchor": [0, 0, 0]});
    assert!(conforms(root, root, &ok));
    for bad in [json!({"bridge": 3, "row_spacing": 1.5}), json!({"slot_length": 20, "bridge": 3, "row_spacing": 1.5, "extra": 1})] {
        assert!(!conforms(root, root, &bad), "{bad}");
    }
    assert!(serde_json::from_value::<LivingHingeParams>(json!({"slot_length": 20, "bridge": 3, "row_spacing": 1.5, "clip": "x"})).is_err());
}

#[test]
fn the_reply_is_a_living_hinge_result() {
    let mut s = Session::default();
    for (id, p) in [
        ("solid.box", json!({"length": 120, "width": 80, "height": 3, "body_name": "Wall"})),
        ("sketch.create", json!({"plane": "XY", "offset": 3, "name": "HingeArea"})),
        ("sketch.rectangle.two_point", json!({"p0": [40, 0], "p1": [80, 80]})),
        ("sketch.finish", json!({})),
    ] {
        s.execute(id, &p).unwrap_or_else(|e| panic!("{id}: {e}"));
    }
    let reply = s
        .execute("solid.living_hinge", &json!({"sketch": "HingeArea", "slot_length": 20, "bridge": 3, "row_spacing": 1.5, "angle": "90 deg"}))
        .unwrap();
    let schema = output_schema_for::<LivingHingeResult>();
    let root = schema.as_value();
    assert!(conforms(root, root, &reply), "{reply}");
    assert!(reply["slots"].as_u64().unwrap() > 0, "{reply}");
}
