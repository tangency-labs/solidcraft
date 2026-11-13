//! Contracts and result type for the render stage commands in `cmd/render/staging.rs` (#11455).
//! A stage is a set of render-only panels saved with a render setup. The handlers read these
//! same parameter types, so each contract is exactly what its handler takes; like the handlers,
//! a contract refuses unknown keys.

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Serialize};
use serde_json::Value;

fn patch() -> Value {
    serde_json::json!({})
}

fn set_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type": "object", "maxProperties": 13, "default": {}})
}

fn margin() -> f64 {
    0.25
}

fn gap() -> f64 {
    0.1
}

fn reflectance() -> [f64; 3] {
    [0.18; 3]
}

/// Fit one ground panel under the visible product, a little below its lowest point, and save
/// it with a render setup in one undo step. The setup must render with the CPU raytracer or the
/// GPU path tracer. The product's clearance from the panel is checked again on every render.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GroundParams {
    /// The render setup, saved or new.
    pub name: String,
    /// Changes to the setup before the ground is fitted, as `render.settings.set` takes them:
    /// an object of at most 13 setup fields. Default: none.
    #[serde(default = "patch")]
    #[schemars(schema_with = "set_schema")]
    pub set: Value,
    /// How far the ground reaches past the product on each side, as a share of the product's
    /// largest size: 0 to 10. Default: 0.25.
    #[serde(default = "margin")]
    #[schemars(range(min = 0.0, max = 10.0))]
    pub margin_fraction: f64,
    /// The gap between the product's lowest point and the ground (mm): 0.0001 to 1000.
    /// Default: 0.1.
    #[serde(default = "gap")]
    #[schemars(range(min = 0.0001, max = 1000.0))]
    pub gap_mm: f64,
    /// How much light the ground reflects, as linear red, green and blue: each 0 to 1.
    /// Default: 0.18 each.
    #[serde(default = "reflectance")]
    pub reflectance_linear: [f64; 3],
    /// Replace a stage the setup already has. Without it, such a setup is refused.
    #[serde(default)]
    pub replace_stage: bool,
}

/// Fit a floor and a wall, or a curved cyclorama of 24 flat strips, behind the visible product,
/// facing the setup's camera, and save it with the render setup in one undo step. The setup
/// must render with the CPU raytracer or the GPU path tracer. World +Z is up. This is a finite
/// background, not an infinite floor or a smooth CAD face.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BackdropParams {
    /// The render setup, saved or new.
    pub name: String,
    /// Changes to the setup before the backdrop is fitted, as `render.settings.set` takes them:
    /// an object of at most 13 setup fields. Default: none.
    #[serde(default = "patch")]
    #[schemars(schema_with = "set_schema")]
    pub set: Value,
    pub kind: BackdropKind,
    /// How much light the panels reflect, as linear red, green and blue: each 0 to 1.
    /// Default: 0.18 each.
    #[serde(default = "reflectance")]
    pub reflectance_linear: [f64; 3],
    /// Replace a stage the setup already has. Without it, such a setup is refused.
    #[serde(default)]
    pub replace_stage: bool,
    /// The design's revision when the caller last read it, as `document.inspect` gives it. If
    /// the design changed since, the call is refused: read it again and retry.
    pub expected_revision: u64,
}

/// The backdrop's shape.
#[derive(Clone, Copy, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BackdropKind {
    /// A floor and a flat wall.
    FloorWall,
    /// A floor that curves up into the wall, in 24 flat strips.
    Cyclorama,
}

/// Remove the stage from a render setup in one undo step. The design's geometry and the
/// setup's lighting stay as they are.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ClearParams {
    /// The render setup, saved or new.
    pub name: String,
    /// Changes to the setup made with the removal, as `render.settings.set` takes them:
    /// an object of at most 13 setup fields. Default: none.
    #[serde(default = "patch")]
    #[schemars(schema_with = "set_schema")]
    pub set: Value,
}

/// What `render.stage.ground`, `render.stage.backdrop` and `render.stage.clear` return.
#[derive(Debug, Serialize, JsonSchema)]
pub struct StageSaved {
    /// The render setup.
    pub name: String,
    /// The setup as saved, as the render settings commands give it.
    pub setup: Value,
    /// Always true: the design's bodies don't change.
    pub engineering_geometry_unchanged: bool,
    /// Always true: the panels exist only in renders.
    pub stage_only: bool,
    /// How the product's clearance from the panels is checked.
    pub clearance: String,
    /// A backdrop's shape: `finite_floor_and_wall` or `24_segment_polygonal_cyclorama`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub geometry_kind: Option<String>,
    /// A backdrop's part in the lighting.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub light_transport: Option<String>,
    /// A backdrop: always true, the setup's camera stays as it is.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub camera_preserved: Option<bool>,
    /// A backdrop's limits, in sentences.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limitations: Option<Vec<String>>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

#[cfg(test)]
#[path = "render_stage_tests.rs"]
mod tests;
