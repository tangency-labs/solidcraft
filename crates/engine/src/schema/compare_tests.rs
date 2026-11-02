use serde_json::{Value, json};

use super::{CompareParams, Comparison};
use crate::Session;
use crate::schema::tests::{assert_keys, assert_strict_keeps_todays_errors, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<CompareParams>(), &["a", "b", "color", "max_points", "max_regions", "spacing", "tolerance"]);
}

/// The handler reads the numbers and the colours before it looks for the bodies, so each of
/// these fails the same way on a design with none.
#[test]
fn strict_keeps_todays_errors() {
    assert_strict_keeps_todays_errors(&[
        ("inspect.compare", json!({"tolerance": 0})),
        ("inspect.compare", json!({"tolerance": "x"})),
        ("inspect.compare", json!({"spacing": 2000})),
        ("inspect.compare", json!({"max_points": 1})),
        ("inspect.compare", json!({"max_regions": 0})),
        ("inspect.compare", json!({"color": 5})),
        ("inspect.compare", json!({"color": "red"})),
        ("inspect.compare", json!({"color": {"only_in_a": "#zz0000"}})),
        ("inspect.compare", json!({"color": {"only_in_b": [1, 2]}})),
        ("inspect.compare", json!({"a": 5})),
        ("inspect.compare", json!({"a": "A"})),
        ("inspect.compare", json!({})),
        ("inspect.compare", json!({"a": "X", "b": "Y"})),
    ]);
}

/// #10994: a comparison replies as its output schema says, in Warn without warnings for good
/// calls and with them for a misspelt key: a box and its copy (identical, no regions), and the
/// copy with a boss, compared with every key and coloured both ways.
#[test]
fn a_comparison_replies_with_its_schema() {
    let schema = output_schema_for::<Comparison>();
    let matches = |r: &Value, warned: bool| {
        let schema = schema.as_value();
        assert!(conforms(schema, schema, r), "{r} doesn't match {schema}");
        assert_eq!(r.get("param_warnings").is_some(), warned, "{r}");
    };
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    run(&mut s, "solid.box", json!({"length": 40, "width": 30, "height": 10, "body_name": "A"}));
    run(&mut s, "solid.move", json!({"bodies": ["A"], "translate": [0, 0, 0], "copy": true}));
    let r = run(&mut s, "inspect.compare", json!({}));
    matches(&r, false);
    assert!(r["identical"] == true && r["regions"] == json!([]) && r.get("colored_faces").is_none(), "{r}");
    run(&mut s, "solid.box", json!({"length": 6, "width": 5, "height": 2, "corner": [5, 5, 10], "operation": "join", "targets": ["A (2)"]}));
    let r = run(
        &mut s,
        "inspect.compare",
        json!({"a": "A", "b": "A (2)", "tolerance": 0.01, "spacing": 0.5, "max_points": 1e6, "max_regions": 10, "color": true}),
    );
    matches(&r, false);
    assert_eq!((r["regions"][0]["side"].as_str(), r["identical"].as_bool()), (Some("only_in_b"), Some(false)), "{r}");
    assert!(r["colored_faces"].as_u64().is_some_and(|n| n > 0), "{r}");
    let r = run(&mut s, "inspect.compare", json!({"a": "A", "b": "A (2)", "color": {"only_in_b": "#123456", "only_in_a": null}}));
    matches(&r, false);
    matches(&run(&mut s, "inspect.compare", json!({"a": "A", "b": "A (2)", "color": false, "tolerence": 1})), true);
}
