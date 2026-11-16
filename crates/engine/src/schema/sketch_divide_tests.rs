use serde_json::{Value, json};

use super::{DivideParams, DivideResult};
use crate::Session;
use crate::schema::tests::{assert_keys, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

/// A sketch being edited, as the command needs: a line l1, a circle and an infinite line.
fn sketch_with_curves(s: &mut Session) {
    run(s, "sketch.create", json!({"plane": "XY", "name": "Curves"}));
    run(s, "sketch.line", json!({"points": [[0, 0], [30, 0]], "infer": false}));
    run(s, "sketch.circle.center", json!({"center": [50, 20], "radius": 8}));
}

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<DivideParams>(), &["curve", "curves", "segments", "sketch"]);
}

/// A call that fails today gets the same message in strict. The command runs only while a sketch
/// is being edited, so each call runs in one.
#[test]
fn strict_keeps_todays_errors() {
    let many = vec!["l1"; 10_001];
    for p in [
        json!({"curve": "l1"}),
        json!({"curve": "l1", "segments": null}),
        json!({"curve": "l1", "segments": 1}),
        json!({"curve": "l1", "segments": 501}),
        json!({"curve": "l1", "segments": 2.5}),
        json!({"curve": "l1", "segments": "3"}),
        json!({"segments": 3}),
        json!({"curve": 5, "segments": 3}),
        json!({"curve": [1], "segments": 3}),
        json!({"curves": {}, "segments": 3}),
        json!({"curves": many, "segments": 3}),
        json!({"curve": "nope", "segments": 3}),
        json!({"curve": "l1", "segments": 3, "sketch": true}),
        json!({"curve": "l1", "segments": 3, "sketch": "Nope"}),
    ] {
        let error = |level: ParamCheck| {
            let mut s = Session { param_check: level, ..Session::default() };
            sketch_with_curves(&mut s);
            s.execute("sketch.point.divide", &p).err().map(|e| e.to_string())
        };
        let off = error(ParamCheck::Off);
        assert!(off.is_some(), "{p} should fail today");
        assert_eq!(error(ParamCheck::Strict), off, "{p}");
    }
}

/// #15880: the reply of a real divide, as the output schema says, in Warn without warnings for
/// a good call and with them for a misspelt key; for an open curve and a closed one.
#[test]
fn divide_replies_with_its_schema() {
    let schema = output_schema_for::<DivideResult>();
    let matches = |r: &Value, warned: bool| {
        let schema = schema.as_value();
        assert!(conforms(schema, schema, r), "{r} doesn't match {schema}");
        assert_eq!(r.get("param_warnings").is_some(), warned, "{r}");
    };
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    sketch_with_curves(&mut s);
    let r = run(&mut s, "sketch.point.divide", json!({"curve": "l1", "segments": 4}));
    matches(&r, false);
    assert_eq!(r["points"].as_array().map(Vec::len), Some(3), "{r}");
    assert_eq!(r["constraints"].as_array().map(Vec::len), Some(3), "{r}");
    assert_eq!(r["created"]["points"].as_array().map(Vec::len), Some(3), "{r}");
    let r = run(&mut s, "sketch.point.divide", json!({"curves": ["c1"], "segments": 5, "sketch": "Curves"}));
    matches(&r, false);
    assert_eq!(r["points"].as_array().map(Vec::len), Some(5), "a closed curve gets one point per piece: {r}");
    matches(&run(&mut s, "sketch.point.divide", json!({"curve": "l1", "segments": 2, "segmnts": 3})), true);
}
