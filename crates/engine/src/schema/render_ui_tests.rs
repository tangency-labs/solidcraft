use serde_json::{Value, json};

use super::OutputSize;
use crate::Session;
use crate::schema::tests::{assert_keys, conforms};
use crate::schema::{ParamCheck, input_schema_for, issues, output_schema_for};

/// What the check finds in a call, in its order.
fn messages(params: &Value) -> Vec<String> {
    issues("render.output.size", params).into_iter().map(|i| i.message).collect()
}

/// What the command is refused with, as its caller reads it, with `param_check` at `level`.
fn refusal(level: ParamCheck, params: &Value) -> String {
    let mut s = Session { param_check: level, ..Session::default() };
    match s.execute("render.output.size", params) {
        Err(e) => e.to_string(),
        Ok(v) => panic!("{params} should be refused at {level:?}: {v}"),
    }
}

/// What the command answers, with `param_check` at `level`.
fn answer(level: ParamCheck, params: &Value) -> Value {
    let mut s = Session { param_check: level, ..Session::default() };
    s.execute("render.output.size", params).unwrap_or_else(|e| panic!("{params} should be answered at {level:?}: {e}"))
}

/// The two schemas, and the range they state. The command answers with what it was given, so one type
/// describes the call and the answer; `custom` is only ever answered with when it is true, so it is
/// not a required reply key.
#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<OutputSize>(), &["custom", "height", "width"]);
    for schema in [input_schema_for::<OutputSize>(), output_schema_for::<OutputSize>()] {
        let root = schema.as_value().clone();
        assert_eq!(root["additionalProperties"], json!(false), "the schema refuses unknown keys too");
        for side in ["width", "height"] {
            let side = &root["properties"][side];
            assert_eq!(side["type"], json!("integer"), "{side}");
            assert_eq!(side["minimum"], json!(1), "the smallest size the command takes");
            assert_eq!(side["maximum"], json!(16384), "the largest size the command takes");
        }
    }
    let root = output_schema_for::<OutputSize>().as_value().clone();
    let mut required: Vec<&str> = root["required"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
    required.sort_unstable();
    assert_eq!(required, ["height", "width"], "`custom` is absent rather than false");
}

/// The answer is the request: the two sizes at each end of the range, and `custom` echoed only when
/// it is true. Every answer matches the output schema, with the parameter check off and in strict.
#[test]
fn the_answer_is_the_request_at_every_parameter_check_level() {
    let out = output_schema_for::<OutputSize>().as_value().clone();
    for level in ParamCheck::ALL {
        for (request, reply) in [
            (json!({"width": 1, "height": 1}), json!({"width": 1, "height": 1})),
            (json!({"width": 16384, "height": 16384}), json!({"width": 16384, "height": 16384})),
            (json!({"width": 1920, "height": 1080}), json!({"width": 1920, "height": 1080})),
            (json!({"width": 1920, "height": 1080, "custom": false}), json!({"width": 1920, "height": 1080})),
            (json!({"width": 1920, "height": 1080, "custom": true}), json!({"width": 1920, "height": 1080, "custom": true})),
        ] {
            let a = answer(level, &request);
            assert_eq!(a, reply, "{request}");
            assert!(conforms(&out, &out, &a), "{a} doesn't match {out}");
        }
    }
}

/// A size outside the range is the handler's to refuse, so the call is refused with its message in
/// both modes: the check states the range for the caller and does not read it.
#[test]
fn a_size_outside_the_range_keeps_the_handlers_message() {
    for size in [
        json!({"width": 0, "height": 1080}),
        json!({"width": 1920, "height": 0}),
        json!({"width": 16385, "height": 1080}),
        json!({"width": 1920, "height": 16385}),
        json!({"width": 0, "height": 16385}),
    ] {
        let off = refusal(ParamCheck::Off, &size);
        assert!(off.contains("dimensions must be in 1..16384"), "{size}: {off}");
        assert!(messages(&size).is_empty(), "{size}: the check should have nothing to add");
        assert_eq!(refusal(ParamCheck::Strict, &size), off, "{size}");
    }
}

/// What the check words in its own way: a missing key, an unknown key, a value that is not a whole
/// number in range, and a call that is not an object. Each of these calls fails today as well, with
/// serde's wording; only the wording moves. Pinned here so that it is a decision and not a drift.
#[test]
fn the_check_words_its_own_refusals() {
    for (params, today, checked) in [
        (json!({}), "missing field `width`", "`width` is required"),
        (json!({"width": 1920}), "missing field `height`", "`height` is required"),
        (json!({"width": 1920, "height": 1080, "nope": 1}), "unknown field `nope`", "unknown key `nope`"),
        (
            json!({"width": 1920, "height": 1080, "custom": "yes"}),
            "invalid type: string \"yes\", expected a boolean",
            "`custom` must be true or false",
        ),
        (json!({"width": 1920.5, "height": 1080}), "invalid type: floating point", "`width` must be u32"),
        (json!({"width": -1, "height": 1080}), "invalid value: integer `-1`, expected u32", "`width`: invalid value: integer `-1`, expected u32"),
        (json!([]), "invalid length 0, expected struct OutputSize", "parameters must be an object"),
    ] {
        let off = refusal(ParamCheck::Off, &params);
        assert!(off.contains(today), "{params} with the check off: {off}");
        let strict = refusal(ParamCheck::Strict, &params);
        assert!(strict.contains(checked), "{params} in strict: {strict}");
    }
    assert_eq!(
        messages(&json!({"width": 1920, "height": 1080, "nope": 1})),
        ["unknown key `nope`", "`nope`: unknown field `nope`, expected one of `width`, `height`, `custom`"],
        "the check names the key, and its own reading of the call names it again"
    );
    assert_eq!(messages(&json!({})), ["`width` is required"]);
    // A size the command refuses is nothing the check finds; a call it takes is nothing it names.
    assert!(messages(&json!({"width": 0, "height": 0})).is_empty());
    assert!(messages(&json!({"width": 16384, "height": 16384, "custom": true})).is_empty());
}
