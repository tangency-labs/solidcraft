use std::collections::BTreeSet;

use serde_json::{Value, json};

use super::{InterferenceParams, InterferenceResult};
use crate::Session;
use crate::schema::tests::{assert_keys, assert_strict_keeps_todays_errors, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<InterferenceParams>(), &["bodies", "budget_s"]);
}

/// #12874: calls that fail today get the same message in strict: `string_list`'s for `bodies`
/// of the wrong shape (`NameList` gives them), the handler's for `budget_s` out of range, and
/// its own for a body the design lacks.
#[test]
fn strict_keeps_todays_errors() {
    let too_many = vec![json!("A"); 10_001];
    assert_strict_keeps_todays_errors(&[
        ("inspect.interference", json!({"bodies": 5})),
        ("inspect.interference", json!({"bodies": {"name": "A"}})),
        ("inspect.interference", json!({"bodies": ["A", 1]})),
        ("inspect.interference", json!({"bodies": too_many})),
        ("inspect.interference", json!({"bodies": ["Nope"]})),
        ("inspect.interference", json!({"budget_s": 0})),
        ("inspect.interference", json!({"budget_s": -1})),
        ("inspect.interference", json!({"budget_s": 86401})),
        ("inspect.interference", json!({"budget_s": "60"})),
        ("inspect.interference", json!({"budget_s": true})),
    ]);
}

/// #12874: the contract takes what the input schema allows: each call's check finds no issue
/// exactly when it matches the schema.
#[test]
fn the_check_takes_what_the_schema_allows() {
    let schema = input_schema_for::<InterferenceParams>();
    let schema = schema.as_value();
    for (p, takes) in [
        (json!({}), true),
        (json!({"bodies": "C1"}), true),
        (json!({"bodies": ["C1", "C2"]}), true),
        (json!({"bodies": []}), true),
        (json!({"bodies": null, "budget_s": null}), true),
        (json!({"budget_s": 0.5}), true),
        (json!({"budget_s": 86400}), true),
        (json!({"bodies": 5}), false),
        (json!({"bodies": ["C1", 1]}), false),
        (json!({"budget_s": 0}), false),
        (json!({"budget_s": 86401}), false),
        (json!({"budget_s": "60"}), false),
    ] {
        let issues = crate::schema::issues("inspect.interference", &p);
        assert_eq!(issues.is_empty(), takes, "{p}: {issues:?}");
        assert_eq!(conforms(schema, schema, &p), takes, "{p}");
    }
}

/// The keys of an object.
fn keys(v: &Value) -> BTreeSet<&str> {
    v.as_object().into_iter().flatten().map(|(k, _)| k.as_str()).collect()
}

/// #12874: Interference replies as its output schema says, in Warn without warnings, with the
/// keys it gave before: on two cylinders that overlap and one apart, checked together and in
/// pairs, with a time budget given.
#[test]
fn interference_replies_with_its_schema() {
    let mut s = Session::default();
    for p in [
        json!({"radius": 5, "height": 10, "base": [0, 0, 0], "body_name": "C1"}),
        json!({"radius": 5, "height": 10, "base": [7, 0, 0], "operation": "new", "body_name": "C2"}),
        json!({"radius": 5, "height": 10, "base": [40, 0, 0], "operation": "new", "body_name": "Far"}),
    ] {
        s.execute("solid.cylinder", &p).unwrap_or_else(|e| panic!("{p}: {e}"));
    }
    s.param_check = ParamCheck::Warn;
    let schema = output_schema_for::<InterferenceResult>();
    let schema = schema.as_value();
    let reply_keys = BTreeSet::from([
        "interferences",
        "count",
        "checked",
        "unchecked",
        "truncated",
        "skipped_pairs",
        "pairs_candidate",
        "pairs_cleared",
        "pairs_intersected",
        "pairs_not_checked",
        "budget_s",
        "budget_exceeded",
        "threads",
        "boolean_ms",
        "tessellate_ms",
        "slowest",
        "ms",
    ]);
    for p in [json!({}), json!({"bodies": "C1"}), json!({"bodies": ["C1", "C2"], "budget_s": 60}), json!({"bodies": ["C1", "Far"]})] {
        let r = s.execute("inspect.interference", &p).unwrap_or_else(|e| panic!("{p}: {e}"));
        assert!(conforms(schema, schema, &r), "{p}: {r} doesn't match {schema}");
        assert!(r.get("param_warnings").is_none(), "{p}: {r}");
        assert_eq!(keys(&r), reply_keys, "{p}: {r}");
    }
    let r = s.execute("inspect.interference", &json!({"bodies": ["C1", "C2"]})).unwrap_or_default();
    let hit = &r["interferences"][0];
    assert_eq!((&r["count"], &hit["a"], &hit["b"]), (&json!(1), &json!("C1"), &json!("C2")), "{r}");
    assert_eq!(keys(hit), BTreeSet::from(["a", "b", "volume_mm3", "area_mm2", "bbox", "center"]), "{r}");
    assert!(hit["volume_mm3"].as_f64().is_some_and(|v| v > 0.0) && keys(&hit["bbox"]) == BTreeSet::from(["min", "max"]), "{r}");
    let slowest = &r["slowest"][0];
    assert_eq!(keys(slowest), BTreeSet::from(["a", "b", "ms", "boolean_ms", "tessellate_ms"]), "{r}");
}
