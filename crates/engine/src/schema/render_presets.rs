//! Contracts and result types for the native studio and quality profile commands in
//! `cmd/render/studio_presets.rs` (#11766). The handlers read these same parameter types, so
//! each contract is exactly what its handler takes; like the handlers, a contract refuses
//! unknown keys.

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// `set`'s default: no change to the setup.
fn no_change() -> Value {
    serde_json::json!({})
}

/// `set`: changes to the setup, as `render.settings.set` takes them. `patched_settings` in
/// `cmd/render.rs` refuses anything but an object of at most 13 fields, with its own message.
fn set_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "description": "Changes to the setup before the studio is fitted, as `render.settings.set` takes them: an object of at most 13 setup fields. The setup must render with the GPU path tracer. Default: none.",
        "type": "object",
        "maxProperties": 13,
        "default": {}
    })
}

/// Fit a native studio to the visible product: its HDR environment, a ground, its light rig
/// when it has one, and its exposure. `render.preset.resolve` reports what would be saved,
/// without changing the design. `render.preset.apply` saves it, in one undo step, after that
/// review. The camera, the output size, sampling, optical bindings and the design itself are
/// kept. Needs a build with the bundled render library.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PresetParams {
    /// The native studio: `NATIVE-STUDIO-` and its number, as `render.library.list` with
    /// `kind: native_scenes` gives it.
    pub id: String,
    /// The render setup to fit it into, saved or new.
    pub name: String,
    #[serde(default = "no_change")]
    #[schemars(schema_with = "set_schema")]
    pub set: Value,
    /// The design's revision when the caller resolved the studio, as `render.preset.resolve`
    /// gives it. If the design changed since, the call is refused: resolve it again. Apply needs
    /// it; resolve takes it to check the same way. Default: no check.
    #[serde(default)]
    pub expected_revision: Option<u64>,
    /// Apply only: agree that the studio replaces the setup's lighting, environment and exposure.
    /// Apply refuses without it.
    #[serde(default)]
    pub replace_lighting_and_environment: bool,
    /// Apply only: agree that the studio replaces a stage the setup already has. Apply refuses
    /// without it when the resolved studio says `requires_stage_consent`.
    #[serde(default)]
    pub replace_stage: bool,
}

/// Work out the GPU path tracer's fixed sampling budget for a quality profile: the image size,
/// samples, depth and seed it asks for. Nothing is saved, and the design doesn't change. A
/// profile the GPU path tracer can't meet whole is refused, never silently reduced.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct QualityParams {
    /// The quality profile: `Q-` and its number, as `render.library.list` with `kind: quality`
    /// gives it.
    pub id: String,
    /// How to use it: only `fixed_samples_png`, a fixed budget for a PNG still.
    #[schemars(schema_with = "variant_schema")]
    pub variant: String,
    /// Agree that the budget leaves out what the profile asks for beyond a fixed budget (see
    /// `omitted` in the reply). Required as true.
    pub acknowledge_omissions: bool,
}

fn variant_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type": "string", "enum": ["fixed_samples_png"]})
}

/// What `render.preset.resolve` and `render.preset.apply` return: the fitted studio.
#[derive(Debug, Serialize, JsonSchema)]
pub struct StudioComposition {
    /// The native studio.
    pub id: String,
    /// The render setup.
    pub name: String,
    /// The design's revision the studio was fitted at: give it to `render.preset.apply` as
    /// `expected_revision`.
    pub revision: u64,
    /// The native studio's recipe, as `render.library.describe` gives it.
    pub source: Map<String, Value>,
    /// The setup with the studio fitted, as the render settings commands give it.
    pub setup: Value,
    /// The SHA-256 of the studio's HDR environment.
    pub resource_sha256: String,
    /// How the studio's light rig was fitted, as `render.library.studio.prepare` reports it;
    /// `null` for a studio without a rig.
    pub rig_projection: Value,
    /// Always `resolved_native_projection`.
    pub status: String,
    /// Whether the design changed: always false for resolve; for apply, false when the setup
    /// already was the studio.
    pub document_changed: bool,
    /// Always `gpu-path`: the studio renders with the GPU path tracer.
    pub executor: String,
    /// Always true: applying needs `replace_lighting_and_environment`.
    pub requires_lighting_consent: bool,
    /// Whether applying needs `replace_stage`: the setup has a stage, or the patch gives one.
    pub requires_stage_consent: bool,
    /// What the studio replaces in the setup.
    pub replaces: Vec<String>,
    /// What it keeps.
    pub preserves: Vec<String>,
    /// How the product's clearance from the stage and lights is checked.
    pub clearance: String,
    /// The studio's limits, in sentences.
    pub limitations: Vec<String>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What `render.quality.prepare` returns: the fixed budget, to give to the render commands.
#[derive(Debug, Serialize, JsonSchema)]
pub struct QualityPlan {
    /// The quality profile.
    pub id: String,
    /// Its name.
    pub name: Option<String>,
    /// Always `fixed_samples_png`.
    pub variant: String,
    /// Always `fixed_budget_projection_not_full_quality_profile`.
    pub status: String,
    /// The design's revision when the budget was prepared.
    pub revision: u64,
    /// Always false.
    pub document_changed: bool,
    /// Always true: the profile itself isn't changed.
    pub source_unchanged: bool,
    /// The render setup fields the budget sets.
    pub settings: QualitySize,
    /// The GPU path tracer's options the budget sets.
    pub gpu_options: QualityOptions,
    /// What the budget keeps as the setup has it.
    pub preserves: Vec<String>,
    /// What the profile asks for that a fixed budget leaves out.
    pub omitted: Vec<String>,
    /// The quality profile as its catalog holds it, unchanged.
    pub source: Map<String, Value>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// The image size of a quality budget (pixels).
#[derive(Debug, Serialize, JsonSchema)]
pub struct QualitySize {
    pub width: u32,
    pub height: u32,
}

/// The GPU path tracer's options of a quality budget.
#[derive(Debug, Serialize, JsonSchema)]
pub struct QualityOptions {
    /// Samples per pixel: 1 to 4096.
    pub samples: u32,
    /// The most bounces a path takes: 1 to 16.
    pub max_depth: u32,
    pub seed: u32,
    /// Whether the profile asks for denoising; `null` when it doesn't say.
    pub denoise: Option<bool>,
}

#[cfg(test)]
#[path = "render_presets_tests.rs"]
mod tests;
