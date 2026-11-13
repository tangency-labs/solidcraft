use serde_json::{Value, json};

use super::{DivideParams, DivideResult, MAX_CURVES};
use crate::Session;
use crate::schema::tests::{assert_keys, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

/// A sketch being edited, as this command needs: a line and a circle, so an open and a closed
/// curve are both there to divide.
fn divided_sketch(s: &mut Session) {
    run(s, "sketch.create", json!({"plane": "XY", "name": "Divide"}));
    run(s, "sketch.line", json!({"points": [[0, 0], [100, 0]], "infer": false, "connect": false, "ids": ["l"]}));
    run(s, "sketch.circle.center", json!({"center": [0, 40], "radius": 10, "id": "c"}));
}

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<DivideParams>(), &["curve", "curves", "segments", "sketch"]);
}

/// The schema takes the calls the handler takes and says which keys it cannot do without. It
/// cannot say that `segments` is a whole number or that `null` names no curves, so those are
/// allowed here and refused by the contract and the handler, with the handler's own message.
#[test]
fn the_schema_takes_what_the_handler_reads() {
    let takes = |schema: schemars::Schema, cases: &[(Value, bool)]| {
        let schema = schema.as_value();
        for (p, takes) in cases {
            assert_eq!(conforms(schema, schema, p), *takes, "{p}");
        }
    };
    takes(
        input_schema_for::<DivideParams>(),
        &[
            (json!({"curve": "l", "segments": 4}), true),
            (json!({"curves": ["l", "c"], "segments": 4, "sketch": "Divide"}), true),
            (json!({"curves": "c", "segments": 2}), true),
            (json!({"curve": "l", "curves": ["c"], "segments": 2}), true),
            (json!({"curve": "l", "segments": 2.0}), true),
            (json!({"curve": "l", "segments": 2.5}), true),
            (json!({"curve": null, "curves": null, "segments": 4}), true),
            (json!({"segments": 4}), false),
            (json!({"curve": "l"}), false),
            (json!({"curve": "l", "segments": 1}), false),
            (json!({"curve": "l", "segments": 501}), false),
            (json!({"curve": "l", "segments": "4"}), false),
            (json!({"curve": 5, "segments": 4}), false),
            (json!({"curves": [1], "segments": 4}), false),
            (json!({"curve": "l", "segments": 4, "sketch": 3}), true),
            (json!({"curve": "l", "segments": 4, "sketch": null}), true),
            (json!({"curve": "l", "segments": 4, "sketch": true}), false),
            (json!({"curve": "l", "segments": 4, "sketch": -1}), false),
        ],
    );
}

/// A call that fails today gets the same message in strict. The command runs only while a sketch
/// is being edited, so each call runs in one.
#[test]
fn strict_keeps_todays_errors() {
    let many: Vec<String> = (0..=MAX_CURVES).map(|i| format!("c{i}")).collect();
    for p in [
        json!({}),
        json!({"segments": 4}),
        json!({"curve": null, "curves": null, "segments": 4}),
        json!({"curve": "l"}),
        json!({"curve": "l", "segments": null}),
        json!({"curve": "l", "segments": 1}),
        json!({"curve": "l", "segments": 501}),
        json!({"curve": "l", "segments": 2.5}),
        json!({"curve": "l", "segments": "4"}),
        json!({"curve": 5, "segments": 4}),
        json!({"curves": [1], "segments": 4}),
        json!({"curves": many, "segments": 4}),
        json!({"curve": "nope", "segments": 4}),
        json!({"curve": "l", "segments": 4, "sketch": true}),
        json!({"curve": "l", "segments": 4, "sketch": -1}),
        json!({"curve": "l", "segments": 4, "sketch": "Nope"}),
    ] {
        let error = |level: ParamCheck| {
            let mut s = Session { param_check: level, ..Session::default() };
            divided_sketch(&mut s);
            s.execute("sketch.point.divide", &p).err().map(|e| e.to_string())
        };
        let off = error(ParamCheck::Off);
        assert!(off.is_some(), "{p} should fail today");
        assert_eq!(error(ParamCheck::Strict), off, "{p}");
    }
}

/// #15880: the reply as its output schema says, in Warn without warnings for a good call and
/// with them for a misspelt key: an open curve gets `segments` − 1 points and a closed one
/// `segments`, in the order the curves were named, and `created` is what the call added.
#[test]
fn divide_replies_with_its_schema() {
    let schema = output_schema_for::<DivideResult>();
    let matches = |r: &Value, warned: bool| {
        let schema = schema.as_value();
        assert!(conforms(schema, schema, r), "{r} doesn't match {schema}");
        assert_eq!(r.get("param_warnings").is_some(), warned, "{r}");
    };
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    divided_sketch(&mut s);

    let r = run(&mut s, "sketch.point.divide", json!({"curves": ["l", "c"], "segments": 4}));
    matches(&r, false);
    let points: Vec<Value> = r["points"].as_array().cloned().unwrap_or_default();
    assert_eq!(points.len(), 7, "three on the line, four round the circle: {r}");
    assert_eq!(r["constraints"].as_array().map(Vec::len), Some(7), "{r}");
    assert_eq!(r["created"]["points"].as_array(), Some(&points), "{r}");
    assert_eq!(r["created"]["constraints"], r["constraints"], "{r}");
    assert_eq!(r["sketch"]["solved"], json!(true), "{r}");
    assert!(r["sketch"]["dof"].is_number(), "{r}");

    // A single curve by `curve`, and the sketch by name: the same reply shape, one curve's worth.
    let r = run(&mut s, "sketch.point.divide", json!({"curve": "c", "segments": 3, "sketch": "Divide"}));
    matches(&r, false);
    assert_eq!(r["points"].as_array().map(Vec::len), Some(3), "a closed curve gets as many points as pieces: {r}");

    // A misspelt key is a warning, and the call still runs: the line gets two points.
    let r = run(&mut s, "sketch.point.divide", json!({"curve": "l", "segments": 3, "segements": 3}));
    matches(&r, true);
    assert_eq!(r["points"].as_array().map(Vec::len), Some(2), "an open curve gets one point fewer than pieces: {r}");
}
