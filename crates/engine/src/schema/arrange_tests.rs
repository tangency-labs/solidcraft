use serde_json::json;

use super::{ArrangeParams, ArrangeReply};
use crate::Session;
use crate::schema::tests::{assert_keys, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(
        input_schema_for::<ArrangeParams>(),
        &[
            "angle_step",
            "bodies",
            "bottoms",
            "copy",
            "improve",
            "lay_flat",
            "length",
            "margin",
            "method",
            "origin",
            "quantity",
            "sheet",
            "sheet_spacing",
            "sheets",
            "spacing",
            "width",
        ],
    );
}

#[test]
fn replies_match_the_output_schema() {
    let mut s = Session { param_check: ParamCheck::Warn, ..Default::default() };
    for (i, n) in ["A", "B"].iter().enumerate() {
        s.execute("solid.box", &json!({"length": 10, "width": 10, "height": 10, "corner": [i as f64 * 30.0, 0, 0], "body_name": n}))
            .unwrap_or_else(|e| panic!("{e}"));
    }
    let schema = output_schema_for::<ArrangeReply>();
    let schema = schema.as_value();
    for p in [
        json!({"bodies": ["A", "B"], "width": 100, "length": 100}),
        json!({"bodies": "A", "width": 15, "length": 15, "origin": [500, 500], "spacing": 0, "margin": 1, "angle_step": 0, "method": "rectangular", "improve": 2}),
        json!({"bodies": ["A", "B"], "width": 12, "length": 12}),
    ] {
        let r = s.execute("solid.arrange", &p).unwrap_or_else(|e| panic!("{p}: {e}"));
        assert!(conforms(schema, schema, &r), "{p}: {r}");
        assert!(r.get("param_warnings").is_none(), "{p}: {r}");
    }
}
