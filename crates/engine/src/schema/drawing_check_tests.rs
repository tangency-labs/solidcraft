use serde_json::Value;

use super::{CheckParams, CheckResult, CoverageState, FindingKind};
use crate::schema::tests::assert_keys;
use crate::schema::{input_schema_for, output_schema_for};

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<CheckParams>(), &["sheet"]);
}

/// The reply's kinds and coverage states are closed lists: each schema node names every value of
/// its list, and nothing else.
#[test]
fn the_kinds_are_a_closed_list() {
    let schema = output_schema_for::<CheckResult>();
    let text = |v: Value| v.as_str().unwrap().to_string();
    let kinds: Vec<String> = FindingKind::ALL.iter().map(|k| text(serde_json::to_value(k).unwrap())).collect();
    let states: Vec<String> = CoverageState::ALL.iter().map(|k| text(serde_json::to_value(k).unwrap())).collect();
    for (def, mut all) in [("FindingKind", kinds), ("CoverageState", states)] {
        let node = &schema.as_value()["$defs"][def];
        let mut named: Vec<String> = Vec::new();
        names(node, &mut named);
        named.sort();
        all.sort();
        assert_eq!(named, all, "{def}: {node}");
    }
}

/// Every string a schema node allows by `enum` or `const`, however it nests them.
fn names(node: &Value, out: &mut Vec<String>) {
    match node {
        Value::Object(o) => {
            for (key, v) in o {
                match (key.as_str(), v) {
                    ("enum", Value::Array(list)) => out.extend(list.iter().filter_map(Value::as_str).map(str::to_string)),
                    ("const", Value::String(name)) => out.push(name.clone()),
                    _ => names(v, out),
                }
            }
        }
        Value::Array(list) => list.iter().for_each(|v| names(v, out)),
        _ => {}
    }
}
