use serde_json::Value;

use super::{CheckParams, CheckResult, FindingKind};
use crate::schema::tests::assert_keys;
use crate::schema::{input_schema_for, output_schema_for};

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<CheckParams>(), &["sheet"]);
}

/// The reply's kinds are a closed list: the schema's kind names every kind, and nothing else.
#[test]
fn the_kinds_are_a_closed_list() {
    let schema = output_schema_for::<CheckResult>();
    let kind = &schema.as_value()["$defs"]["FindingKind"];
    let mut named = Vec::new();
    names(kind, &mut named);
    named.sort();
    let mut all: Vec<String> = FindingKind::ALL.iter().map(|k| serde_json::to_value(k).unwrap().as_str().unwrap().to_string()).collect();
    all.sort();
    assert_eq!(named, all, "{kind}");
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
