use serde_json::{Value, json};

use super::{CloseResultParams, OutputSize, RESULT_VIEW_FORMS, ResultViewName, ResultViewParams, ResultViewResult};
use crate::Session;
use crate::schema::render_jobs::Job;
use crate::schema::tests::{assert_keys, conforms};
use crate::schema::{ParamCheck, input_schema_for, issues, output_schema_for};

fn matches(schema: schemars::Schema, r: &Value) {
    let schema = schema.as_value();
    assert!(conforms(schema, schema, r), "{r} doesn't match {schema}");
}

/// `node`, or the definition it points at: a property that is only a name's schema is usually a
/// `$ref` to one.
fn resolved<'a>(root: &'a Value, node: &'a Value) -> &'a Value {
    node.get("$ref")
        .and_then(Value::as_str)
        .and_then(|r| r.strip_prefix("#/$defs/"))
        .and_then(|name| root.get("$defs").and_then(|defs| defs.get(name)))
        .unwrap_or(node)
}

/// The names `node` allows, whether they are listed as an `enum` or spelled one `const` at a time
/// (schemars uses the second form once a name carries its own description).
fn allowed_names(node: &Value) -> Vec<&str> {
    node["enum"]
        .as_array()
        .into_iter()
        .flatten()
        .chain(node["oneOf"].as_array().into_iter().flatten())
        .filter_map(|name| name.get("const").or(Some(name)).and_then(Value::as_str))
        .collect()
}

/// What the command is refused with, as its caller reads it, with `param_check` at `level`.
fn view_refusal(level: ParamCheck, id: &str, params: Value) -> String {
    let mut s = Session { param_check: level, ..Session::default() };
    s.execute(id, &params).err().unwrap_or_else(|| panic!("{id} {params} should be refused at {level:?}")).to_string()
}

/// The two contracts' keys, and the reply's: `view` always, `job` only for the guide mask.
#[test]
fn result_view_schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<ResultViewParams>(), &["view"]);
    assert_keys(input_schema_for::<CloseResultParams>(), &[]);

    // The five names, and nothing else, in the call and in the reply.
    for schema in [input_schema_for::<ResultViewParams>(), output_schema_for::<ResultViewResult>()] {
        let root = schema.as_value().clone();
        let view = resolved(&root, &root["properties"]["view"]);
        let mut listed = allowed_names(view);
        listed.sort_unstable();
        assert_eq!(listed, ["actual", "denoise_mask", "fit", "image", "model"], "{view}");
    }
    // The same five the handler reads, and nothing else: the schema and the handler's own reading
    // of the name cannot drift apart.
    for name in ["model", "image", "fit", "actual", "denoise_mask"] {
        assert!(ResultViewName::parse(name).is_some(), "{name}");
    }
    for not in ["", " ", "unknown", "Model", "DEN", "denoise", "denoise-mask", "model "] {
        assert_eq!(ResultViewName::parse(not), None, "{not}");
    }

    let root = output_schema_for::<ResultViewResult>().as_value().clone();
    let keys: Vec<&String> = root["properties"].as_object().unwrap().keys().collect();
    assert_eq!(keys, ["job", "view"], "the schema's keys, in its own order");
    assert_eq!(root["required"], json!(["view"]), "only the view is always there");
    // `job` is the identity the render commands already describe, and it is absent rather than null.
    let job = &root["properties"]["job"];
    assert!(conforms(&root, job, &json!({"session": 1, "serial": 2, "generation": 3})));
    for not in [json!({"session": 1, "serial": 2}), json!("job 2"), json!(7), Value::Null] {
        assert!(!conforms(&root, job, &not), "{not}");
    }
}

/// A name that is not one of the five is the handler's to refuse, so the call is refused with its
/// message in both modes, and the check adds nothing in front of it.
#[test]
fn a_view_that_is_not_one_of_the_five_keeps_the_handlers_message() {
    for view in ["", " ", "unknown", "Model", "DEN", "denoise", "denoise-mask", "denoise_mask "] {
        let params = json!({"view": view});
        let off = view_refusal(ParamCheck::Off, "render.result.view", params.clone());
        assert!(off.contains(RESULT_VIEW_FORMS), "{params}: {off}");
        assert_eq!(view_refusal(ParamCheck::Strict, "render.result.view", params.clone()), off, "{params}");
    }
}

/// What the check words in its own way: a missing key, an unknown key, a value that is not text, a
/// non-object, and a key for a command that takes none. Each of these calls fails today as well,
/// with the handler's or serde's wording; only the wording moves. Pinned here so that it is a
/// decision and not a drift. A name that is not one of the five is not one of them: the handler
/// refuses it in both modes (see the test above).
#[test]
fn the_check_words_its_own_result_view_refusals() {
    for (id, params, today, checked) in [
        ("render.result.view", json!({}), "missing field `view`", "`view` is required"),
        ("render.result.view", json!({"view": "model", "nope": 1}), "unknown field `nope`", "unknown key `nope`"),
        ("render.result.view", json!({"view": 5}), "invalid type: integer `5`, expected a string", "`view` must be text"),
        ("render.result.view", json!({"view": null}), "invalid type: null, expected a string", "`view` must be text"),
        ("render.result.view", json!([]), "invalid length 0, expected struct ResultViewParams with 1 element", "parameters must be an object"),
        ("render.result.close", json!({"view": "model"}), "expected no parameters", "unknown key `view`"),
    ] {
        let off = view_refusal(ParamCheck::Off, id, params.clone());
        assert!(off.contains(today), "{id} {params} with the check off: {off}");
        let strict = view_refusal(ParamCheck::Strict, id, params.clone());
        assert!(strict.contains(checked), "{id} {params} in strict: {strict}");
    }
}

/// Every call that needs no retained result answers with the view it now shows and nothing else, at
/// every parameter-check level, and the answer matches the output schema. `render.result.close` is
/// the same answer for `model`; the guide mask is refused before any answer is built.
#[test]
fn the_two_commands_answer_as_their_output_schema_says() {
    let out = output_schema_for::<ResultViewResult>();
    for level in ParamCheck::ALL {
        let mut s = Session { param_check: level, ..Session::default() };
        for view in ["model", "image", "fit", "actual"] {
            let r = s.execute("render.result.view", &json!({"view": view})).unwrap_or_else(|e| panic!("{view}: {e}"));
            assert_eq!(r, json!({"view": view}), "{view}");
            matches(out.clone(), &r);
        }
        for params in [json!({}), Value::Null] {
            let r = s.execute("render.result.close", &params).unwrap_or_else(|e| panic!("{params}: {e}"));
            assert_eq!(r, json!({"view": "model"}), "{params}");
            matches(out.clone(), &r);
        }
        // The guide mask needs a selected completed result: refused, so there is no answer to check.
        let e = s.execute("render.result.view", &json!({"view": "denoise_mask"})).unwrap_err().to_string();
        assert!(e.contains("no completed result is selected"), "{e}");
    }
}

/// The one answer that carries a second key: the guide mask names the retained result whose mask it
/// shows, with the identity the caller already has from `render.start`, as the render commands
/// already describe it. The Render workspace reads that key back as a job identity.
#[test]
fn a_guide_mask_answer_carries_the_result_identity() {
    let mut s = Session::default();
    s.set_render_job_host(Some(solidcraft_render::job::CpuHost::cooperative())).unwrap();
    s.execute("render.settings.set", &json!({"name": "Tiny", "set": {"width": 8, "height": 8}})).unwrap();
    let started = s.execute("render.start", &json!({"setup": "Tiny"})).unwrap();
    let job: Job = serde_json::from_value(started["job"].clone()).unwrap();
    let r = serde_json::to_value(ResultViewResult { view: ResultViewName::DenoiseMask, job: Some(job) }).unwrap();
    assert_eq!(r, json!({"view": "denoise_mask", "job": started["job"]}));
    matches(output_schema_for::<ResultViewResult>(), &r);
    assert_eq!(serde_json::to_value(job).unwrap(), started["job"], "the identity round-trips as the caller has it");
    // Without a mask there is no `job` key at all, and the four other views never carry one.
    for view in [ResultViewName::Model, ResultViewName::Image, ResultViewName::Fit, ResultViewName::Actual] {
        let r = serde_json::to_value(ResultViewResult { view, job: None }).unwrap();
        assert_eq!(r, json!({"view": serde_json::to_value(view).unwrap()}));
        matches(output_schema_for::<ResultViewResult>(), &r);
    }
}

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
