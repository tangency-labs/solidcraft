//! The contract for Render Image and Check Render Request in `cmd/render/jobs.rs` (#13311). Both
//! commands take the same image request; `render.preflight` only checks it.
//!
//! The handler reads a request in one of two forms. With `progressive: true` it is the GPU form
//! (`ProgressiveStart`); otherwise it is the runtime form (`Start`), for the raster and CPU
//! raytrace image executors. Before parsing, the handler refuses the keys and values of the other
//! form with its own messages. The contract makes the same checks with the same messages, then
//! parses the request with the handler's own serde types ([`crate::cmd::read_render_request`]),
//! passing serde's message on whole. So strict refuses what the handler refuses, with today's
//! message, and only an unknown key is worded by the check. What the handler checks after parsing
//! (ranges, combinations of keys, the saved setup, the host) stays the handler's.
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

/// The GPU form's `raytrace` keys.
const GPU_RAYTRACE: &[&str] = &["dielectrics", "clear_coats", "textures"];

/// The keys the handler refuses in the runtime form before parsing it, whatever their value.
const GPU_CONTROLS: &[&str] = &["lens", "lighting", "tone_map", "supersampling", "denoise_settings", "denoise_diagnostics"];

const RUNTIME_GPU_MESSAGE: &str =
    "runtime GPU transmission, texture, lens, environment, lighting, tone-map, denoise and clamp_indirect controls require progressive rendering";
const PROGRESSIVE_CPU_MESSAGE: &str = "progressive rendering does not accept CPU raytrace, environment or LED options";
const WEIGHTS_MESSAGE: &str = "studio visibility weights must be finite in 0..1";
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
    let progressive = params.get("progressive").and_then(Value::as_bool) == Some(true);
    let present = |key: &str| params.get(key).is_some();
    let raytrace = params.get("raytrace").and_then(Value::as_object);
    let environment = params.get("environment");
    if progressive {
        if params.get("integrator").is_some_and(|v| v.as_str() != Some("raster"))
            || raytrace.is_some_and(|o| o.keys().any(|key| !GPU_RAYTRACE.contains(&key.as_str())))
            || RUNTIME_ONLY.iter().any(|key| present(key))
            || environment.is_some_and(|e| e.is_object() && e.get("kind").is_none())
        {
            return Err(PROGRESSIVE_CPU_MESSAGE.into());
        }
    } else if raytrace.is_some_and(|o| ["clear_coats", "textures"].iter().any(|key| o.contains_key(*key)))
        || environment.is_some_and(|e| e.get("kind").is_some())
        || GPU_CONTROLS.iter().any(|key| present(key))
        || params.get("denoise").is_some_and(|v| v != &json!(false))
        || params.get("clamp_indirect").is_some_and(|v| !v.is_null())
    {
        return Err(RUNTIME_GPU_MESSAGE.into());
    }
    crate::cmd::read_render_request(params.clone(), progressive).map_err(|e| e.to_string())?;
    // Parsed: `studio_weights` is three numbers or null, and `samples`, `max_depth` and `seed`
    // are whole numbers or null.
    let weights = params.get("studio_weights").and_then(Value::as_array).into_iter().flatten();
    if weights.filter_map(Value::as_f64).any(|w| !(0.0..=1.0).contains(&w)) {
        return Err(WEIGHTS_MESSAGE.into());
    }
    if !progressive && ["samples", "max_depth", "seed"].iter().any(|key| params.get(*key).is_some_and(|v| !v.is_null())) {
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
            options, and only `replace` and `studio_weights` may come with it. Not with `settings`.", "type": ["string", "null"]}),
        );
        put(
            "settings",
            section(
                "A whole render setup, as `render.settings.get` gives one, used without saving it; a setup with \
            a `job` as for `setup`. Not with `setup`. Without either: the default setup, seen from the current camera or fitted \
            to the product.",
            ),
        );
        put("replace", switch("Cancel the design's active and waiting render jobs before this one starts. Default: false."));
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
            samples?}` for an HDR saved in the design. With `progressive: true`: a GPU environment, by `kind`: `builtin_hdr`, \
            `hdr`, `embedded_hdr` or `sky`. Default: none.",
            ),
        );
        put(
            "aging",
            json!({"description": "Aging layers for the bodies, at most 8. CPU only with `integrator: raytrace`. \
            Default: none.", "type": "array", "items": {"type": "object"}, "maxItems": 8}),
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
        let runtime = json!({
            "properties": {
                "progressive": {"enum": [false]},
                "raytrace": {"not": any_of_keys(&["clear_coats", "textures"])},
                "environment": {
                    "type": ["object", "null"],
                    "properties": {
                        "path": {"type": "string"},
                        "resource": {"type": ["string", "null"]},
                        "expected_sha256": {"type": ["string", "null"]},
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
                "environment": {
                    "required": ["kind"],
                    "properties": {"kind": {"type": "string", "enum": ["builtin_hdr", "hdr", "embedded_hdr", "sky"]}},
                },
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
        })
    }
}

#[cfg(test)]
#[path = "render_start_tests.rs"]
mod tests;
