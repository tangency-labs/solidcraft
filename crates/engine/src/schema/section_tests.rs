use serde_json::{Value, json};

use super::{SectionCurvesParams, SectionCurvesResult};
use crate::Session;
use crate::schema::tests::{assert_keys, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

const CMD: &str = "inspect.section_curves";

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<SectionCurvesParams>(), &["at", "bodies", "flip", "offset", "plane", "tolerance"]);
}

/// Every form of the call (a named plane with `at` or `offset`, an explicit plane, `flip`, one
/// body name or a list, a tolerance) replies as the output schema says, with lines, arcs and
/// splines among the segments, and none of these calls gets a warning.
#[test]
fn section_curves_replies_match_the_output_schema() {
    let schema = output_schema_for::<SectionCurvesResult>();
    let schema = schema.as_value();
    let required: Vec<&str> = schema.get("required").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).collect();
    assert!(!required.contains(&"param_warnings"));
    let mut s = Session::default();
    s.execute("solid.box", &json!({"length": 40, "width": 20, "height": 10, "body_name": "Plate"})).unwrap_or_else(|e| panic!("{e}"));
    s.execute("solid.cylinder", &json!({"radius": 10, "height": 40, "body_name": "Rod", "base": [100, 0, 0]})).unwrap_or_else(|e| panic!("{e}"));
    s.param_check = ParamCheck::Warn;
    let (sin, cos) = (30f64.to_radians().sin(), 30f64.to_radians().cos());
    let calls = [
        json!({"plane": "XY", "at": 5}),
        json!({"plane": "XY", "offset": "5 mm", "flip": true, "bodies": "Plate"}),
        json!({"plane": {"origin": [100, 0, 20], "normal": [0, sin, cos]}, "bodies": ["Rod"], "tolerance": 0.01}),
    ];
    let mut kinds = std::collections::BTreeSet::new();
    for p in calls {
        let r = s.execute(CMD, &p).unwrap_or_else(|e| panic!("{p}: {e}"));
        assert!(conforms(schema, schema, &r), "{r} doesn't match {schema}");
        assert!(r.get("param_warnings").is_none(), "{p}: {r}");
        for g in r["loops"].as_array().into_iter().flatten().flat_map(|l| l["segments"].as_array().into_iter().flatten()) {
            kinds.insert(g["type"].as_str().unwrap_or("?").to_string());
        }
    }
    assert_eq!(kinds.into_iter().collect::<Vec<_>>(), ["arc", "line", "spline"]);
    // A misspelt key in warn: the reply carries the warning and still matches.
    let r = s.execute(CMD, &json!({"plane": "XY", "at": 5, "tolerence": 0.01})).unwrap_or_else(|e| panic!("{e}"));
    assert!(r.get("param_warnings").is_some() && conforms(schema, schema, &r), "{r}");
}

/// In strict, an unknown key is refused under the command's own id before the cut runs.
#[test]
fn strict_refuses_an_unknown_key() {
    let mut s = Session { param_check: ParamCheck::Strict, ..Session::default() };
    s.execute("solid.box", &json!({"length": 40, "width": 20, "height": 10})).unwrap_or_else(|e| panic!("{e}"));
    assert!(s.execute(CMD, &json!({"plane": "XY", "at": 5})).is_ok());
    let e = s.execute(CMD, &json!({"plane": "XY", "at": 5, "tolerence": 0.01})).map(|_| ()).unwrap_err().to_string();
    assert!(e.starts_with("invalid parameters for `inspect.section_curves`: unknown key `tolerence`"), "{e}");
}
