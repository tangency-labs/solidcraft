use std::collections::BTreeSet;

use serde_json::{Value, json};
use solidcraft_render::job::CpuHost;

use super::{GPU_ONLY, RUNTIME_ONLY, RenderRequest};
use crate::Session;
use crate::schema::check::schema_keys;
use crate::schema::tests::{assert_keys, assert_strict_keeps_todays_errors, conforms};
use crate::schema::{ParamCheck, input_schema_for};

const IDS: [&str; 2] = ["render.start", "render.preflight"];

/// #13311: `engine.describe` gives Render Image and Check Render Request their exact parameters.
#[test]
fn both_commands_describe_their_parameters() {
    let mut s = Session::default();
    for id in IDS {
        let described = s.execute("engine.describe", &json!({"command": id})).unwrap_or_else(|e| panic!("{e}"));
        assert!(described["input"].is_object(), "{id}: {described}");
        assert_eq!(described["input"].get("additionalProperties"), Some(&Value::Bool(false)), "{id}: {described}");
    }
}

/// The fields of the handler's reader for one form, from serde's message for an unknown key,
/// which lists them all.
fn handler_fields(progressive: bool) -> BTreeSet<String> {
    let error = crate::cmd::read_render_request(json!({"\u{1}": 0}), progressive).err().map(|e| e.to_string()).unwrap_or_default();
    let listed = error.split_once("expected one of ").map(|(_, fields)| fields).unwrap_or_default();
    let fields: BTreeSet<String> = listed.split(", ").map(|field| field.trim_matches('`').to_string()).collect();
    assert!(fields.len() > 10, "{error}");
    fields
}

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(
        input_schema_for::<RenderRequest>(),
        &[
            "adaptive",
            "aging",
            "backdrop",
            "caustics",
            "clamp_indirect",
            "denoise",
            "denoise_diagnostics",
            "denoise_settings",
            "environment",
            "finishing",
            "integrator",
            "led_samples",
            "led_strip",
            "lens",
            "lighting",
            "linear_exr",
            "max_depth",
            "metal_energy_compensation",
            "nested_media",
            "progressive",
            "raytrace",
            "replace",
            "samples",
            "seed",
            "settings",
            "setup",
            "studio_weights",
            "supersampling",
            "texture_filtering",
            "tone_map",
            "transmission_samples",
            "transparent_background",
            "volume",
        ],
    );
}

/// #13311: the contract's keys are the fields the handler reads, in either form, and each form's
/// schema refuses the keys only the other form reads. A field added to the handler's `Start` or
/// `ProgressiveStart` fails this test until the contract declares it.
#[test]
fn the_keys_are_the_fields_the_handler_reads() {
    let (runtime, gpu) = (handler_fields(false), handler_fields(true));
    let schema = input_schema_for::<RenderRequest>();
    let schema = schema.as_value();
    assert_eq!(schema_keys(schema), runtime.union(&gpu).cloned().collect::<BTreeSet<_>>());
    let set = |keys: &[&str]| keys.iter().map(|key| key.to_string()).collect::<BTreeSet<_>>();
    assert_eq!(set(GPU_ONLY), gpu.difference(&runtime).cloned().collect::<BTreeSet<_>>());
    assert_eq!(set(RUNTIME_ONLY), runtime.difference(&gpu).cloned().collect::<BTreeSet<_>>());
    let refused = |form: &Value| -> BTreeSet<String> {
        let keys = form["not"]["anyOf"].as_array().into_iter().flatten().filter_map(|k| k["required"][0].as_str());
        keys.map(str::to_string).collect()
    };
    assert_eq!(refused(&schema["anyOf"][0]), set(GPU_ONLY));
    assert_eq!(refused(&schema["anyOf"][1]), set(RUNTIME_ONLY));
}

/// #13311: a key of the other form gets the handler's message in strict: its own refusal for the
/// GPU controls and the LED options, serde's for the others.
#[test]
fn a_key_of_the_other_form_keeps_todays_error() {
    let mut cases: Vec<(&str, Value)> = GPU_ONLY.iter().map(|key| ("render.preflight", json!({*key: null}))).collect();
    cases.extend(RUNTIME_ONLY.iter().map(|key| ("render.preflight", json!({"progressive": true, *key: null}))));
    assert_strict_keeps_todays_errors(&cases);
}

/// #13311: calls that fail today get the same message in strict: the handler's own refusals of
/// the other form's values, a studio weight out of range and sampling controls without
/// `progressive`, and serde's message for a value of the wrong shape (the check passes it on
/// whole).
#[test]
fn strict_keeps_todays_errors() {
    let mut cases = Vec::new();
    for id in IDS {
        cases.extend([
            (id, json!({"progressive": "yes"})),
            (id, json!({"progressive": null})),
            (id, json!({"integrator": "gpu"})),
            (id, json!({"samples": 4})),
            (id, json!({"seed": -1})),
            (id, json!({"denoise": true})),
            (id, json!({"denoise": null})),
            (id, json!({"clamp_indirect": 2})),
            (id, json!({"raytrace": {"textures": []}})),
            (id, json!({"raytrace": {"no_such_option": 1}})),
            (id, json!({"environment": {"kind": "sky"}})),
            (id, json!({"environment": {"path": 5}})),
            (id, json!({"studio_weights": [0, 0.5, 2]})),
            (id, json!({"studio_weights": [0, 0.5]})),
            (id, json!({"setup": 5})),
            (id, json!({"settings": {"width": "wide"}})),
            (id, json!({"replace": "no"})),
            (id, json!({"aging": {}})),
            (id, json!({"led_samples": 1.5})),
            (id, json!({"progressive": true, "integrator": "raytrace"})),
            (id, json!({"progressive": true, "integrator": null})),
            (id, json!({"progressive": true, "raytrace": {"lights": []}})),
            (id, json!({"progressive": true, "raytrace": null})),
            (id, json!({"progressive": true, "environment": {"path": "studio.hdr"}})),
            (id, json!({"progressive": true, "environment": {"kind": "moon"}})),
            (id, json!({"progressive": true, "tone_map": "filmic"})),
            (id, json!({"progressive": true, "supersampling": -1})),
            (id, json!({"progressive": true, "studio_weights": [2, 0, 0]})),
            (id, json!({"progressive": true, "finishing": null})),
            (id, json!({"progressive": true, "denoise_settings": 3})),
            (id, json!({"progressive": true, "lens": [1, 2]})),
            (id, json!({"progressive": true, "settings": 5})),
            (id, json!({"progressive": true, "environment": {"kind": "builtin_hdr"}})),
            (id, json!({"progressive": true, "environment": {"kind": "sky", "assume_linear_srgb": true}})),
            (id, json!({"integrator": "raytrace", "environment": {"resource": "ABC"}})),
        ]);
    }
    assert_strict_keeps_todays_errors(&cases);
}

/// #13311: a call wrong in two ways gets the handler's first message in strict: `setup` with
/// `settings`, and a runtime key beside an inline job, come before the form's own checks, and
/// studio weights with the CPU ray tracer before sampling controls.
#[test]
fn strict_keeps_the_handlers_order() {
    let mut cases = Vec::new();
    for id in IDS {
        cases.extend([
            (id, json!({"setup": "Studio", "settings": {}, "samples": 4})),
            (id, json!({"setup": "Studio", "settings": {}, "progressive": true, "led_samples": 4})),
            (id, json!({"settings": {"job": {"method": "raster"}}, "samples": 4})),
            (id, json!({"settings": {"job": {"method": "raster"}}, "lens": null})),
            (id, json!({"settings": {"job": {"method": "raster"}}, "progressive": true})),
            (id, json!({"settings": {"job": {"method": "moon"}}, "samples": 4})),
            (id, json!({"integrator": "raytrace", "studio_weights": [1, 1, 1], "samples": 4})),
        ]);
    }
    assert_strict_keeps_todays_errors(&cases);
}

/// #13311: an HDR's `resource` and `expected_sha256` are 64 lowercase hex digits, as the
/// handler's `HdrId` reads them. `conforms` doesn't read patterns, so the pattern's text is
/// pinned here, in both forms' environments.
#[test]
fn an_hdr_digest_is_64_lowercase_hex_digits() {
    assert_eq!(super::DIGEST_PATTERN, "^[0-9a-f]{64}$");
    for (digest, takes) in [("0a".repeat(32), true), ("0A".repeat(32), false), ("0a".repeat(31), false), ("0g".repeat(32), false)] {
        let runtime = json!({"integrator": "raytrace", "environment": {"resource": digest}});
        let gpu = json!({"progressive": true, "environment": {"kind": "embedded_hdr", "resource": digest}});
        for (p, progressive) in [(runtime, false), (gpu, true)] {
            assert_eq!(crate::cmd::read_render_request(p.clone(), progressive).is_ok(), takes, "{p}");
            assert_eq!(crate::schema::issues("render.start", &p).is_empty(), takes, "{p}");
        }
    }
    let schema = input_schema_for::<RenderRequest>();
    let schema = schema.as_value();
    let runtime = &schema["anyOf"][0]["properties"]["environment"]["properties"];
    assert_eq!(runtime["resource"]["pattern"], super::DIGEST_PATTERN);
    assert_eq!(runtime["expected_sha256"]["pattern"], super::DIGEST_PATTERN);
    let gpu = schema["anyOf"][1]["properties"]["environment"]["anyOf"].as_array().cloned().unwrap_or_default();
    let patterns: Vec<&Value> = gpu.iter().flat_map(|form| [&form["properties"]["resource"], &form["properties"]["expected_sha256"]]).collect();
    assert_eq!(patterns.iter().filter(|key| key["pattern"] == super::DIGEST_PATTERN).count(), 2, "{gpu:?}");
}

/// #13311: an unknown key fails in every mode, as the handler's serde structs refuse it: with
/// serde's message with the check off, and in strict with the check's, which suggests a key.
#[test]
fn an_unknown_key_is_refused_in_every_mode() {
    for id in IDS {
        for p in [json!({"sampels": 4}), json!({"progressive": true, "sampels": 4})] {
            let error = |level: ParamCheck| Session { param_check: level, ..Session::default() }.execute(id, &p).err().map(|e| e.to_string());
            assert!(error(ParamCheck::Off).is_some_and(|e| e.contains("unknown field `sampels`")), "{id} {p}: {:?}", error(ParamCheck::Off));
            assert!(error(ParamCheck::Strict).is_some_and(|e| e.contains("did you mean `samples`?")), "{id} {p}: {:?}", error(ParamCheck::Strict));
        }
    }
}

/// #13311: the contract takes what the input schema allows: each call's check finds no issue
/// exactly when it matches the schema. (What the handler refuses after parsing, such as 0
/// samples or a setup that doesn't exist, both allow.)
#[test]
fn the_check_takes_what_the_schema_allows() {
    let cases = [
        (json!({}), true),
        (json!({"setup": "Studio", "replace": true, "studio_weights": [1, 0.5, 0]}), true),
        (json!({"settings": {"width": 16, "height": 16}, "transparent_background": true}), true),
        (json!({"setup": null, "settings": null, "studio_weights": null}), true),
        (json!({"progressive": false, "integrator": "raytrace", "raytrace": {}, "aging": []}), true),
        (json!({"integrator": "raytrace", "environment": {"path": "studio.hdr", "rotation_radians": 0.5, "samples": 64}}), true),
        (json!({"integrator": "raytrace", "led_strip": null, "led_samples": 16}), true),
        (json!({"denoise": false, "clamp_indirect": null, "samples": null, "max_depth": null, "seed": null, "environment": null}), true),
        (json!({"settings": {"width": 16, "height": 16}, "samples": 0}), false),
        (json!({"progressive": "yes"}), false),
        (json!({"progressive": null}), false),
        (json!({"integrator": "gpu"}), false),
        (json!({"samples": 4}), false),
        (json!({"denoise": true}), false),
        (json!({"clamp_indirect": 2}), false),
        (json!({"raytrace": {"textures": []}}), false),
        (json!({"environment": {"kind": "sky"}}), false),
        (json!({"environment": {"path": 5}}), false),
        (json!({"studio_weights": [0, 0.5, 2]}), false),
        (json!({"studio_weights": [0, 0.5]}), false),
        (json!({"replace": "no"}), false),
        (json!({"lens": null}), false),
        (json!({"finishing": {}}), false),
        (json!({"tone_map": "reinhard"}), false),
        (
            json!({"progressive": true, "integrator": "raster", "samples": 64, "max_depth": 8, "seed": 3, "denoise": true,
                "clamp_indirect": 4.5, "tone_map": "aces_fitted", "supersampling": 2, "transmission_samples": 4,
                "raytrace": {"dielectrics": [], "clear_coats": [], "textures": []}, "environment": null, "finishing": {},
                "denoise_settings": {}, "denoise_diagnostics": true, "linear_exr": true, "nested_media": true,
                "texture_filtering": true, "metal_energy_compensation": true, "lens": null, "lighting": null, "caustics": null,
                "volume": null, "adaptive": null, "backdrop": null, "aging": [], "studio_weights": [0, 0, 1]}),
            true,
        ),
        (json!({"setup": "Studio", "settings": {"width": 16}}), false),
        (json!({"setup": "Studio", "settings": null}), true),
        (json!({"progressive": true, "environment": {"kind": "sky"}}), true),
        (json!({"progressive": true, "environment": {"kind": "sky", "gradient": {"zenith": [0, 0, 1]}, "intensity": 2}}), true),
        (json!({"progressive": true, "environment": {"kind": "sky", "assume_linear_srgb": true}}), false),
        (json!({"progressive": true, "environment": {"kind": "builtin_hdr", "id": "studio", "rotation_rad": 0.5}}), true),
        (json!({"progressive": true, "environment": {"kind": "builtin_hdr"}}), false),
        (json!({"progressive": true, "environment": {"kind": "hdr", "path": "a.hdr", "expected_sha256": null, "assume_linear_srgb": true}}), true),
        (json!({"progressive": true, "environment": {"kind": "hdr"}}), false),
        (json!({"progressive": true, "environment": {"kind": "embedded_hdr", "resource": "0a".repeat(32)}}), true),
        (json!({"progressive": true, "environment": {"kind": "embedded_hdr", "path": "a.hdr"}}), false),
        (json!({"progressive": true, "environment": null}), true),
        (json!({"progressive": true, "samples": 0, "max_depth": 0}), true),
        (json!({"progressive": true, "tone_map": null, "settings": {"width": 16}}), true),
        (json!({"progressive": true, "integrator": "raytrace"}), false),
        (json!({"progressive": true, "integrator": null}), false),
        (json!({"progressive": true, "raytrace": {"lights": []}}), false),
        (json!({"progressive": true, "raytrace": null}), false),
        (json!({"progressive": true, "environment": {"path": "studio.hdr"}}), false),
        (json!({"progressive": true, "led_strip": null}), false),
        (json!({"progressive": true, "led_samples": 16}), false),
        (json!({"progressive": true, "tone_map": "filmic"}), false),
        (json!({"progressive": true, "supersampling": -1}), false),
        (json!({"progressive": true, "studio_weights": [2, 0, 0]}), false),
        (json!({"progressive": true, "finishing": null}), false),
        (json!({"progressive": true, "denoise_settings": 3}), false),
    ];
    let schema = input_schema_for::<RenderRequest>();
    let schema = schema.as_value();
    for id in IDS {
        for (p, takes) in &cases {
            assert_eq!(crate::schema::issues(id, p).is_empty(), *takes, "{id} {p}: {:?}", crate::schema::issues(id, p));
            assert_eq!(conforms(schema, schema, p), *takes, "{id} {p}");
        }
    }
}

/// #13311: in Warn, a request the contract takes runs as it did with the check off and gets no
/// warnings: Check Render Request's reply is the same, and Render Image queues a job. Among them
/// a saved setup with a job and an inline one, which the handler lowers itself.
#[test]
fn requests_the_contract_takes_run_as_before() {
    let session = |level: ParamCheck| {
        let mut s = Session { param_check: level, ..Session::default() };
        s.set_render_job_host(Some(CpuHost::cooperative())).unwrap_or_else(|e| panic!("{e}"));
        s.execute("solid.box", &json!({"length": 20, "width": 10, "height": 5})).unwrap_or_else(|e| panic!("{e}"));
        let raster = json!({"name": "Raster", "set": {"width": 16, "height": 16, "job": {"method": "raster"}}});
        s.execute("render.settings.set", &raster).unwrap_or_else(|e| panic!("{e}"));
        s
    };
    for p in [
        json!({}),
        json!({"settings": {"width": 16, "height": 16}, "replace": false, "transparent_background": true}),
        json!({"settings": {"width": 16, "height": 16}, "studio_weights": [1, 0.5, 0.25], "integrator": "raster"}),
        json!({"setup": "Raster", "replace": true}),
        json!({"settings": {"width": 16, "height": 16, "job": {"method": "raster", "transparent_background": true}}, "studio_weights": [1, 1, 1]}),
    ] {
        let off = session(ParamCheck::Off).execute("render.preflight", &p).unwrap_or_else(|e| panic!("{p}: {e}"));
        let warned = session(ParamCheck::Warn).execute("render.preflight", &p).unwrap_or_else(|e| panic!("{p}: {e}"));
        assert_eq!(warned, off, "{p}");
        assert!(warned.get("param_warnings").is_none(), "{p}: {warned}");
        let started = session(ParamCheck::Warn).execute("render.start", &p).unwrap_or_else(|e| panic!("{p}: {e}"));
        assert!(started["job"].is_object() && started.get("param_warnings").is_none(), "{p}: {started}");
    }
}
