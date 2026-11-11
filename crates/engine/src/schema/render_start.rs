//! The contract for Render Image and Check Render Request in `cmd/render/jobs.rs` (#13311). Both
//! commands take the same image request; `render.preflight` only checks it.
//!
//! The handler reads a request in one of two forms. With `progressive: true` it is the GPU form
//! (`ProgressiveStart`); otherwise it is the runtime form (`Start`), for the raster and CPU
//! raytrace image executors. Before parsing, the handler refuses `setup` with `settings`, an
//! inline job with runtime keys beside it, and the keys and values of the other form. The contract
//! calls the handler's own functions for these ([`crate::cmd::render_saved_job_refusal`],
//! [`crate::cmd::render_form_refusal`]), then parses the request with the handler's serde types
//! ([`crate::cmd::read_render_request`]), passing serde's message on whole. So strict refuses what
//! the handler refuses before it looks at the design, with today's message, and only an unknown
//! key is worded by the check. What the handler checks later (ranges, the saved setup, the host)
//! stays the handler's.
//!
//! Strict words two kinds of call differently from the handler, which still refuses them: a named
//! `setup` whose saved job is in the design, with a runtime key beside it (the handler names the
//! key, strict the key's own problem, such as sampling without `progressive`), and a request
//! wrong in two ways where one is an optics or clear coat binding in `settings` (the handler
//! names the binding first).
//!
//! The render crate has no `JsonSchema`. The schema types each top-level key and publishes the
//! render crate's option sections (`raytrace`, `settings`, `lens` and the others) as JSON objects.

use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer};
use serde_json::{Map, Value, json};

use super::check::whole;

/// The keys only the GPU form takes. The runtime form refuses them.
pub(crate) const GPU_ONLY: &[&str] = &[
    "adaptive",
    "backdrop",
    "caustics",
    "denoise_diagnostics",
    "denoise_settings",
    "finishing",
    "lens",
    "lighting",
    "linear_exr",
    "metal_energy_compensation",
    "nested_media",
    "supersampling",
    "texture_filtering",
    "tone_map",
    "transmission_samples",
    "volume",
];

/// The keys only the runtime form takes. The GPU form refuses them.
pub(crate) const RUNTIME_ONLY: &[&str] = &["led_samples", "led_strip"];

/// An HDR's SHA-256 as the handler reads one (`HdrId`): 64 lowercase hex digits.
pub(crate) const DIGEST_PATTERN: &str = "^[0-9a-f]{64}$";

const WEIGHTS_MESSAGE: &str = "studio visibility weights must be finite in 0..1";
const RAYTRACE_WEIGHTS_MESSAGE: &str = "studio_weights apply to raster or the implicit GPU studio; use explicit CPU light intensities";
const SAMPLING_MESSAGE: &str = "sampling controls require progressive rendering";

/// Render Image (`render.start`) and Check Render Request (`render.preflight`): an image request.
/// Nothing is read from it here; the handler reads the parameters itself.
#[derive(Debug)]
pub struct RenderRequest;

impl<'de> Deserialize<'de> for RenderRequest {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let params = Value::deserialize(d)?;
        read(&params).map_err(|message| D::Error::custom(whole(message)))?;
        Ok(RenderRequest)
    }
}

/// What `prepare` in `cmd/render/jobs.rs` refuses in `params` before it looks at the design, in its
/// order and with its messages.
fn read(params: &Value) -> Result<(), String> {
    if let Some(message) = crate::cmd::render_saved_job_refusal(params) {
        return Err(message);
    }
    let progressive = params.get("progressive").and_then(Value::as_bool) == Some(true);
    if let Some(message) = crate::cmd::render_form_refusal(params, progressive) {
        return Err(message.into());
    }
    crate::cmd::read_render_request(params.clone(), progressive).map_err(|e| e.to_string())?;
    // Parsed: `studio_weights` is three numbers or null, `integrator` is raster or raytrace, and
    // `samples`, `max_depth` and `seed` are whole numbers or null.
    let set = |key: &str| params.get(key).is_some_and(|v| !v.is_null());
    let weights = params.get("studio_weights").and_then(Value::as_array).into_iter().flatten();
    if weights.filter_map(Value::as_f64).any(|w| !(0.0..=1.0).contains(&w)) {
        return Err(WEIGHTS_MESSAGE.into());
    }
    if progressive {
        return Ok(());
    }
    if params.get("integrator").and_then(Value::as_str) == Some("raytrace") && set("studio_weights") {
        return Err(RAYTRACE_WEIGHTS_MESSAGE.into());
    }
    if ["samples", "max_depth", "seed"].into_iter().any(set) {
        return Err(SAMPLING_MESSAGE.into());
    }
    Ok(())
}

/// A whole number the handler reads as a `u32`, or `null` for its default.
fn count(description: &str) -> Value {
    json!({"description": description, "type": ["integer", "null"], "minimum": 0, "maximum": u32::MAX})
}

/// A section of options the render crate reads as an object, or `null` for none.
fn section(description: &str) -> Value {
    json!({"description": description, "type": ["object", "null"]})
}

/// A switch, false by default.
fn switch(description: &str) -> Value {
    json!({"description": description, "type": "boolean", "default": false})
}

/// `{"type": "object", "required": [key]}` for each key: with `not`, the keys an object may not have.
fn any_of_keys(keys: &[&str]) -> Value {
    json!({"anyOf": keys.iter().map(|key| json!({"type": "object", "required": [key]})).collect::<Vec<_>>()})
}

impl JsonSchema for RenderRequest {
    fn schema_name() -> Cow<'static, str> {
        "RenderRequest".into()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        let mut properties = Map::new();
        let mut put = |key: &str, schema: Value| properties.insert(key.into(), schema);
        put(
            "setup",
            json!({"description": "A saved render setup, by name. A setup with a saved `job` decides the method and its \
            options, and only `replace` and `studio_weights` may come with it (`studio_weights` only where its method takes \
            them). Not with `settings`.", "type": ["string", "null"]}),
        );
        put(
            "settings",
            section(
                "A whole render setup, as `render.settings.get` gives one, used without saving it; a setup with \
            a `job` as for `setup`. Not with `setup`. Without either: the default setup, seen from the current camera or fitted \
            to the product.",
            ),
        );
        put(
            "replace",
            switch(
                "Render Image: cancel the design's active and waiting render jobs before this one starts. Check Render \
            Request ignores it. Default: false.",
            ),
        );
        put("transparent_background", switch("Leave the background transparent. Default: false."));
        put(
            "studio_weights",
            json!({"description": "Three visibility weights, each from 0 to 1, for the studio's sources: with the \
            raster executor, or on the GPU with its implicit studio (no `lighting` and no environment). Never saved.",
            "type": ["array", "null"], "items": {"type": "number", "minimum": 0, "maximum": 1}, "minItems": 3, "maxItems": 3}),
        );
        put(
            "progressive",
            json!({"description": "`true`: the GPU form, rendered progressively by the GPU path tracer; the keys \
            marked GPU apply. `false` or left out: the runtime form, for the raster executor or, with `integrator: raytrace`, \
            the CPU ray tracer. Default: false.", "type": "boolean", "default": false}),
        );
        put(
            "integrator",
            json!({"description": "`raster` or `raytrace` (CPU ray tracing, in a build with it). With `progressive: \
            true`, only `raster`. Default: raster.", "type": "string", "enum": ["raster", "raytrace"], "default": "raster"}),
        );
        put(
            "raytrace",
            section(
                "Without `progressive`: the CPU ray tracer's options (lights, reflections, `path`, `progressive` \
            sampling, `output`, `budget` and others; see docs/render-jobs.md), with `integrator: raytrace`. With `progressive: \
            true`: the GPU's `dielectrics`, `clear_coats` and `textures` overrides, each a list.",
            ),
        );
        put(
            "environment",
            section(
                "Without `progressive`: a CPU HDR environment, with `integrator: raytrace`: `{path, \
            expected_sha256?, rotation_radians?, intensity?, samples?}` for a file or `{resource, rotation_radians?, intensity?, \
            samples?}` for an HDR saved in the design. With `progressive: true`: a GPU environment, by `kind`: \
            `{kind: builtin_hdr, id, rotation_rad?, intensity?}`, `{kind: hdr, path, expected_sha256?, rotation_rad?, \
            intensity?, assume_linear_srgb?}`, `{kind: embedded_hdr, resource, rotation_rad?, intensity?, assume_linear_srgb?}` \
            or `{kind: sky, gradient?, rotation_rad?, intensity?}`. An HDR's `resource` and `expected_sha256` are its SHA-256, \
            64 lowercase hex digits. Default: none.",
            ),
        );
        put(
            "aging",
            json!({"description": "Aging layers for the bodies, at most 8: with `integrator: raytrace` on the CPU, or on \
            the GPU with `progressive: true`; the raster executor refuses them. Default: none.",
            "type": "array", "items": {"type": "object"}, "maxItems": 8}),
        );
        put(
            "led_strip",
            section(
                "CPU only, with `integrator: raytrace`: a continuous LED strip, a rectangle in world mm \
            (`center_mm`, `length_mm`, `width_mm`, `length_axis`, `width_axis`, `color`, `intensity`), lit as area lights.",
            ),
        );
        put("led_samples", count("CPU only: the samples for the whole LED strip: 1, 4, 16 or 64. Needs `led_strip`. Default: 16."));
        put("denoise", switch("GPU: denoise the image. Without `progressive: true`, only `false`. Default: false."));
        put(
            "clamp_indirect",
            json!({"description": "GPU: cap each contribution from the first bounce on at this value, from \
            0.000001 to 1000000, keeping its hue. Biased. Without `progressive: true`, only `null`. Default: none.",
            "type": ["number", "null"]}),
        );
        put("samples", count("GPU: samples per pixel, from 1 to 65536. Without `progressive: true`, only `null`."));
        put("max_depth", count("GPU: the most bounces, from 1 to 64. Without `progressive: true`, only `null`."));
        put("seed", count("GPU: the random seed. Without `progressive: true`, only `null`."));
        put("supersampling", count("GPU: 1 or 2. At 2 the image is traced at twice the width and height and averaged down. Default: 1."));
        put(
            "denoise_settings",
            json!({"description": "GPU: the denoiser's `passes` (1 to 3), `strength` (0 to 1), \
            `firefly_suppression` and `optical_filter`.", "type": "object"}),
        );
        put("denoise_diagnostics", switch("GPU: keep the denoiser's guide mask, for `render.export` with `format: denoise_mask`. Default: false."));
        put(
            "caustics",
            section(
                "GPU: photon caustics: `photons`, `radius_mm`, `max_depth`, `seed` and the optional `area_lights`, \
            `environment`, `guided_emission` and `refresh_per_pass`. Default: none.",
            ),
        );
        put("nested_media", switch("GPU: trace strictly nested solids (glass in glass, an air bubble) as nested media. Default: false."));
        put(
            "texture_filtering",
            switch(
                "GPU: filter surface maps by the ray's footprint (mip levels and bounded anisotropy). \
            Default: false, which samples the finest level bilinearly.",
            ),
        );
        put("metal_energy_compensation", switch("GPU: restore the energy rough isotropic metals lose. Default: false."));
        put(
            "volume",
            section(
                "GPU: one homogeneous medium: `min_mm`, `max_mm`, `extinction_per_mm` (0 to 1000), \
            `scattering_albedo` [r, g, b] and `anisotropy` (-0.95 to 0.95). Default: none.",
            ),
        );
        put(
            "adaptive",
            section(
                "GPU: adaptive sampling: `pilot_samples`, `min_samples`, `relative_error`, `absolute_error` and \
            `coverage_error`. Default: none, the same samples for every pixel.",
            ),
        );
        put("transmission_samples", count("GPU: 1, 2 or 4 strata for the first Fresnel branch. Default: 1."));
        put(
            "finishing",
            json!({"description": "GPU: creative finishing, as Regrade Linear Master's `presentation` takes it: \
            `looks`, `photographic`, `bloom`, `rgb_gain`, `contrast`, `saturation` and `vignette`. Default: none.",
            "type": "object"}),
        );
        put("backdrop", section("GPU: a gradient behind the product, as Regrade Linear Master's `presentation` takes it. Default: none."));
        put("linear_exr", switch("GPU: keep the raw linear FLOAT32 master, for `render.export` with `format: exr`. Default: false."));
        put("lens", section("GPU: a thin lens, `{aperture_radius_mm, focus_distance_mm}`. Default: none, a pinhole camera."));
        put("lighting", section("GPU: studio lighting in place of the implicit studio. Default: none."));
        put(
            "tone_map",
            json!({"description": "GPU: the tone response, `reinhard` or `aces_fitted`. Default: reinhard.",
            "type": ["string", "null"], "enum": ["reinhard", "aces_fitted", null]}),
        );

        let null_only = json!({"type": "null"});
        let digest = json!({"type": "string", "pattern": DIGEST_PATTERN});
        let digest_or_null = json!({"type": ["string", "null"], "pattern": DIGEST_PATTERN});
        let runtime = json!({
            "properties": {
                "progressive": {"enum": [false]},
                "raytrace": {"not": any_of_keys(&["clear_coats", "textures"])},
                "environment": {
                    "type": ["object", "null"],
                    "properties": {
                        "path": {"type": "string"},
                        "resource": digest_or_null,
                        "expected_sha256": digest_or_null,
                        "rotation_radians": {"type": "number"},
                        "intensity": {"type": "number"},
                        "samples": {"type": "integer", "minimum": 0, "maximum": u32::MAX},
                    },
                    "additionalProperties": false,
                },
                "denoise": {"enum": [false]},
                "clamp_indirect": null_only,
                "samples": null_only,
                "max_depth": null_only,
                "seed": null_only,
            },
            "not": any_of_keys(GPU_ONLY),
        });
        let array = json!({"type": "array"});
        // The GPU environment, as `cmd/render/environment.rs` reads one: by `kind`, each with its keys.
        let gpu_environment = |kind: &str, required: &[&str], keys: Value| {
            let mut properties =
                json!({"kind": {"type": "string", "enum": [kind]}, "rotation_rad": {"type": "number"}, "intensity": {"type": "number"}});
            if let (Some(all), Some(keys)) = (properties.as_object_mut(), keys.as_object()) {
                all.extend(keys.clone());
            }
            let required: Vec<&str> = std::iter::once("kind").chain(required.iter().copied()).collect();
            json!({"type": "object", "required": required, "properties": properties, "additionalProperties": false})
        };
        let linear = json!({"type": "boolean"});
        let rgb = json!({"type": "array", "items": {"type": "number"}, "minItems": 3, "maxItems": 3});
        let gpu_environments = json!([
            {"type": "null"},
            gpu_environment("builtin_hdr", &["id"], json!({"id": {"type": "string"}})),
            gpu_environment("hdr", &["path"], json!({"path": {"type": "string"}, "expected_sha256": digest_or_null, "assume_linear_srgb": linear})),
            gpu_environment("embedded_hdr", &["resource"], json!({"resource": digest, "assume_linear_srgb": linear})),
            gpu_environment(
                "sky",
                &[],
                json!({"gradient": {"type": "object", "properties": {"zenith": rgb, "horizon": rgb, "ground": rgb}, "additionalProperties": false}}),
            ),
        ]);
        let gpu = json!({
            "required": ["progressive"],
            "properties": {
                "progressive": {"enum": [true]},
                "integrator": {"enum": ["raster"]},
                "raytrace": {
                    "type": "object",
                    "properties": {"dielectrics": array, "clear_coats": array, "textures": array},
                    "additionalProperties": false,
                },
                "environment": {"anyOf": gpu_environments},
            },
            "not": any_of_keys(RUNTIME_ONLY),
        });
        json_schema!({
            "description": "An image request. With `progressive: true` it is the GPU form, without it the runtime form; each \
                refuses the other form's keys. Render Image captures the product and queues a job; Check Render Request checks \
                the request and estimates its cost without queuing anything. Neither changes the design.",
            "type": "object",
            "properties": properties,
            "additionalProperties": false,
            "anyOf": [runtime, gpu],
            "not": {"required": ["setup", "settings"], "properties": {"setup": {"not": null_only}, "settings": {"not": null_only}}},
        })
    }
}

#[cfg(test)]
#[path = "render_start_tests.rs"]
mod tests;
