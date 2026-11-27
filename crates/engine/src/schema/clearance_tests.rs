use serde_json::{Value, json};

use super::{Clearance, ClearanceParams};
use crate::Session;
use crate::schema::tests::{assert_keys, assert_strict_keeps_todays_errors, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(
        input_schema_for::<ClearanceParams>(),
        &["against", "bodies", "clearance", "envelopes", "min_volume", "nearest", "skip_within", "tolerance"],
    );
}

/// The handler reads the numbers and `skip_within` before the bodies, so each of these fails
/// the same way on a design with no bodies.
#[test]
fn strict_keeps_todays_errors() {
    assert_strict_keeps_todays_errors(&[
        ("inspect.clearance", json!({"envelopes": "yes"})),
        ("inspect.clearance", json!({"min_volume": -1})),
        ("inspect.clearance", json!({"min_volume": "x"})),
        ("inspect.clearance", json!({"min_volume": 2e9})),
        ("inspect.clearance", json!({"clearance": 2e6})),
        ("inspect.clearance", json!({"tolerance": 2})),
        ("inspect.clearance", json!({"nearest": 1001})),
        ("inspect.clearance", json!({"skip_within": "x"})),
        ("inspect.clearance", json!({"skip_within": 5})),
        ("inspect.clearance", json!({"bodies": ["Nope"]})),
        ("inspect.clearance", json!({"against": "Nope"})),
    ]);
}

/// #11051: a clearance check replies as its output schema says, in Warn without warnings for
/// good calls and with them for a misspelt key: two overlapping boxes and a third 2 mm away,
/// checked with every key, and with `against` alone.
#[test]
fn a_clearance_check_replies_with_its_schema() {
    let schema = output_schema_for::<Clearance>();
    let matches = |r: &Value, warned: bool| {
        let schema = schema.as_value();
        assert!(conforms(schema, schema, r), "{r} doesn't match {schema}");
        assert_eq!(r.get("param_warnings").is_some(), warned, "{r}");
    };
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "A"}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [5, 5, 5], "body_name": "B", "operation": "new"}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [12, -20, 0], "body_name": "C", "operation": "new"}));
    let r = run(&mut s, "inspect.clearance", json!({}));
    matches(&r, false);
    assert_eq!(r["overlaps"].as_array().map(Vec::len), Some(1), "{r}");
    let r = run(
        &mut s,
        "inspect.clearance",
        json!({"bodies": ["A", "B"], "against": "C", "min_volume": 0.01, "clearance": 30, "tolerance": 1e-6, "nearest": 2, "skip_within": "occurrence"}),
    );
    matches(&r, false);
    assert!(r["near"].as_array().is_some_and(|n| !n.is_empty()) && r["nearest"]["A"].is_array(), "{r}");
    matches(&run(&mut s, "inspect.clearance", json!({"against": ["A"], "skip_within": null})), false);
    matches(&run(&mut s, "inspect.clearance", json!({"clearence": 3})), true);
}

/// #22050: unchecked entries with optional non-boolean `distance_mm` and mesh-derived `exact: false`
/// match the `inspect.clearance` output schema.
#[test]
fn unchecked_distance_and_exact_conform_to_output_schema() {
    use super::Unchecked;

    let schema = output_schema_for::<Clearance>();
    let schema = schema.as_value();
    let reply = Clearance {
        overlaps: Vec::new(),
        near: Vec::new(),
        nearest: Default::default(),
        unchecked: vec![
            Unchecked { a: "A".into(), b: Some("B".into()), reason: "relate".into(), distance_mm: Some(0.0), exact: None },
            Unchecked { a: "C".into(), b: Some("D".into()), reason: "gaps".into(), distance_mm: Some(0.25), exact: Some(false) },
            Unchecked { a: "Mesh".into(), b: None, reason: "mesh body: not checked".into(), distance_mm: None, exact: None },
        ],
        bodies: 4,
        pairs_checked: 2,
        pairs_reused: 0,
        pairs_pruned: 0,
        truncated: false,
        ms: 1.0,
        param_warnings: Vec::new(),
    };
    let v = serde_json::to_value(&reply).unwrap();
    assert!(conforms(schema, schema, &v), "{v} doesn't match {schema}");
}
