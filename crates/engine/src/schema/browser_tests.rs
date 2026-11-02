use serde_json::{Value, json};

use super::{FindParams, FindResult};
use crate::Session;
use crate::browser_find::MAX_PATTERN_CHARS;
use crate::schema::tests::{assert_keys, assert_strict_keeps_todays_errors, conforms, sketch_rectangle};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<FindParams>(), &["kinds", "text"]);
}

#[test]
fn strict_keeps_todays_errors() {
    assert_strict_keeps_todays_errors(&[
        ("browser.find", json!({})),
        ("browser.find", json!({"text": "   "})),
        ("browser.find", json!({"text": "   ", "kinds": []})),
        ("browser.find", json!({"text": null, "kinds": null})),
        ("browser.find", json!({"text": 5})),
        ("browser.find", json!({"text": ["bolt"]})),
        ("browser.find", json!({"text": "a".repeat(MAX_PATTERN_CHARS + 1)})),
        ("browser.find", json!({"kinds": "body"})),
        ("browser.find", json!({"kinds": ["bodies"]})),
        ("browser.find", json!({"kinds": [1]})),
        ("browser.find", json!({"kinds": [null]})),
        ("browser.find", json!({"kinds": ["body", "body", "body", "body", "body", "body"]})),
    ]);
}

#[test]
fn replies_match_the_output_schema() {
    let schema = output_schema_for::<FindResult>();
    let schema = schema.as_value();
    let mut s = Session::default();
    sketch_rectangle(&mut s);
    s.execute("solid.box", &json!({"length": 5, "width": 5, "height": 5})).unwrap_or_else(|e| panic!("{e}"));
    s.param_check = ParamCheck::Warn;
    let forms = [
        json!({"text": "sketch"}),
        json!({"kinds": ["body"]}),
        json!({"text": "*1", "kinds": ["sketch", "body", "component", "construction", "joint"]}),
        json!({"text": "  box  ", "kinds": []}),
        json!({"text": "nothing is called this"}),
        json!({"text": "a".repeat(MAX_PATTERN_CHARS), "kinds": null}),
    ];
    for p in forms {
        let r = s.execute("browser.find", &p).unwrap_or_else(|e| panic!("{p}: {e}"));
        assert!(r.get("param_warnings").is_none(), "{p}: {r}");
        assert!(conforms(schema, schema, &r), "{r} doesn't match {schema}");
    }
    // A reply carrying warnings still matches.
    let r = s.execute("browser.find", &json!({"text": "box", "kind": "body"})).unwrap_or_else(|e| panic!("{e}"));
    assert!(r.get("param_warnings").is_some(), "{r}");
    assert!(conforms(schema, schema, &r), "{r} doesn't match {schema}");
    // The check itself catches a wrong type, a wrong item type and a missing key.
    let ok = json!({"found": [{"kind": "body", "name": "Box1", "key": "Box1", "path": [], "item": {"type": "body", "name": "Box1"}}], "total": 1, "incomplete": false});
    assert!(conforms(schema, schema, &ok));
    for (k, v) in [("total", json!("1")), ("found", json!([1])), ("incomplete", json!(0))] {
        let mut bad = ok.clone();
        bad[k] = v;
        assert!(!conforms(schema, schema, &bad), "{bad}");
    }
    let mut bad = ok.clone();
    bad["found"][0]["item"] = Value::from("Box1");
    assert!(!conforms(schema, schema, &bad), "{bad}");
    bad = ok.clone();
    bad.as_object_mut().map(|o| o.remove("total"));
    assert!(!conforms(schema, schema, &bad), "{bad}");
}
