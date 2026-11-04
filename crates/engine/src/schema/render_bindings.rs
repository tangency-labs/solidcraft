//! Contracts and result types for the render binding commands (#11741): a body of a render setup
//! bound to a library surface's maps (`cmd/render/surface_bindings.rs`) or to optical numbers
//! (`cmd/render/optical_bindings.rs`). The handlers read these same parameter types, so each
//! contract is exactly what its handler takes; like the handlers, a contract refuses unknown
//! keys.
//!
//! Three parameters keep their own readers, and their schemas here are written by hand:
//! - `target` and `optics` are the doc crate's `RenderBodyTarget` and `OpticalMaterial`, which
//!   don't derive `JsonSchema` (the doc crate doesn't depend on schemars).
//! - `surface` is passed on whole to `render.library.surface.prepare`, which checks it.

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use solidcraft_doc::render_optics::OpticalMaterial;
use solidcraft_doc::render_surface::RenderBodyTarget;

/// `set`'s default: no change to the setup.
fn no_change() -> Value {
    serde_json::json!({})
}

/// `set`: changes to the setup, as `render.settings.set` takes them. `patched_settings` in
/// `cmd/render.rs` refuses anything but an object of at most 13 fields, with its own message.
fn set_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "description": "Changes to the setup made with the binding, as `render.settings.set` takes them: an object of at most 13 setup fields. Default: none.",
        "type": "object",
        "maxProperties": 13,
        "default": {}
    })
}

/// A body of the design, by the identity a render setup keeps for it: its definition, the
/// feature that made it, and the component occurrence it's in. The ids survive a rebuild, so a
/// binding follows the body, and never another body that took its place in the list.
fn target_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "description": "A body, by the identity a render setup keeps for it, as the setup's `surfaces` or `optics` list gives it. A component and its occurrence path are both 0 and empty for a body outside components.",
        "type": "object",
        "properties": {
            "definition": {"description": "The design or component definition that holds the body: 1 to 1024 bytes, without control characters.", "type": "string"},
            "feature": {"description": "The id of the feature that made the body: 1 or more.", "type": "integer", "minimum": 1},
            "component": {"description": "The id of the component the body is in, or 0 outside components.", "type": "integer", "minimum": 0},
            "occurrence_path": {"description": "The occurrence ids from the top of the design down to the body's occurrence, each 1 or more and none twice; empty outside components.", "type": "array", "items": {"type": "integer", "minimum": 1}}
        },
        "required": ["definition", "feature", "component", "occurrence_path"],
        "additionalProperties": false
    })
}

/// Optical numbers for a body, as `solidcraft_doc::render_optics::OpticalMaterial` reads them.
/// Every field has a default, so `{}` is clear glass with a refractive index of 1.5.
fn optics_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "description": "The body's optical numbers, for the CPU or GPU renderer. Each field has a default: `{}` is clear glass with a refractive index of 1.5. These are render-only numbers, not an engineering material.",
        "type": "object",
        "properties": {
            "ior": {"description": "Refractive index. Default: 1.5.", "type": "number"},
            "absorption_per_mm": {"description": "How much light the inside absorbs per mm, as linear red, green and blue: each 0 or more. Default: none.", "type": "array", "items": {"type": "number", "minimum": 0}, "minItems": 3, "maxItems": 3},
            "boundary": {"description": "How the body's surface bounds the medium: `thin_sheet` (a sheet of `thickness_mm`), `closed_box` or `closed_mesh` (the thickness comes from the geometry). Default: closed_mesh.", "type": "string", "enum": ["thin_sheet", "closed_box", "closed_mesh"]},
            "thickness_mm": {"description": "A thin sheet's thickness (mm); 0 for a closed boundary. Default: 0.", "type": "number", "minimum": 0},
            "shadow_transmission": {"description": "Let light through for shadows too. Default: false.", "type": "boolean"},
            "subsurface": {
                "description": "Light scattering inside the body, as in wax or skin; `null` for none. Default: none.",
                "anyOf": [
                    {
                        "type": "object",
                        "properties": {
                            "scattering_per_mm": {"description": "How often light scatters per mm: 0 to 100.", "type": "number", "minimum": 0, "maximum": 100},
                            "anisotropy": {"description": "Which way it scatters: -0.95 (back) to 0.95 (forward). Default: 0, every way alike.", "type": "number", "minimum": -0.95, "maximum": 0.95}
                        },
                        "required": ["scattering_per_mm"],
                        "additionalProperties": false
                    },
                    {"type": "null"}
                ]
            }
        },
        "additionalProperties": false
    })
}

/// `surface`: a library surface placed on a body, exactly as `render.library.surface.prepare`
/// takes it. That command isn't converted yet, so its keys are described here.
fn surface_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "description": "The library surface and where it goes on the body, as `render.library.surface.prepare` takes it.",
        "type": "object",
        "properties": {
            "id": {"description": "The surface: `SURF-` and its number, as `render.library.list` gives it.", "type": "string"},
            "body_index": {"description": "The body's place in the render scene, from 0, as `render.library.targets` gives it.", "type": "integer", "minimum": 0},
            "body_name": {"description": "The body's name at that place, as `render.library.targets` gives it; a body that moved in the list is refused.", "type": "string"},
            "origin": {"description": "Where the maps start, in mm, as [x, y, z]. Default: the origin.", "type": ["array", "null"], "items": {"type": "number"}, "minItems": 3, "maxItems": 3},
            "plane": {"description": "The plane the maps lie in: `xy`, `yz` or `zx`. Default: xy.", "type": "string", "enum": ["xy", "yz", "zx"]},
            "scale": {"description": "How much larger than authored the maps repeat: 0.001 to 1000. Default: 1.", "type": "number", "minimum": 0.001, "maximum": 1000}
        },
        "required": ["id", "body_index", "body_name"],
        "additionalProperties": false
    })
}

/// Bind a body of a render setup to a library surface's authored maps, for the CPU or GPU renderer,
/// in one undo step. The binding keeps the maps' exact source hashes and the body's identity,
/// mapped in the body's own millimetres, so it follows the body when it moves. Needs a build with
/// the bundled render library and a CPU or GPU setup.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SurfaceBindParams {
    /// The render setup, saved or new.
    pub name: String,
    #[schemars(schema_with = "surface_schema")]
    pub surface: Value,
    #[serde(default = "no_change")]
    #[schemars(schema_with = "set_schema")]
    pub set: Value,
    /// Replace a binding the body already has. Without it, such a body is refused.
    #[serde(default)]
    pub replace: bool,
}

/// Remove one body's surface binding from a render setup, in one undo step. The design's
/// geometry, and any render device, aren't needed.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SurfaceUnbindParams {
    /// The render setup.
    pub name: String,
    #[schemars(schema_with = "target_schema")]
    pub target: RenderBodyTarget,
    #[serde(default = "no_change")]
    #[schemars(schema_with = "set_schema")]
    pub set: Value,
}

/// Bind a body of a render setup to optical numbers (glass, tinted media, subsurface
/// scattering), for the CPU or GPU renderer, in one undo step. The design's geometry and engineering
/// materials don't change. The setup may render on the CPU or GPU.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OpticsBindParams {
    /// The render setup, saved or new.
    pub name: String,
    #[serde(default = "no_change")]
    #[schemars(schema_with = "set_schema")]
    pub set: Value,
    /// The body's place in the render scene, from 0, as `render.library.targets` gives it.
    pub body_index: usize,
    /// The body's name at that place; a body that moved in the list is refused.
    pub body_name: String,
    #[schemars(schema_with = "optics_schema")]
    pub optics: OpticalMaterial,
    /// The design's revision when the caller last read it, as `document.inspect` gives it. If
    /// the design changed since, the call is refused: read it again and retry.
    pub expected_revision: u64,
    /// Replace optics the body already has. Without it, such a body is refused.
    #[serde(default)]
    pub replace: bool,
}

/// Remove one body's optical binding from a saved render setup, in one undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OpticsUnbindParams {
    /// The saved render setup.
    pub name: String,
    #[schemars(schema_with = "target_schema")]
    pub target: RenderBodyTarget,
    /// The design's revision when the caller last read it, as `document.inspect` gives it. If
    /// the design changed since, the call is refused: read it again and retry.
    pub expected_revision: u64,
}

/// What `render.library.surface.bind` and `render.library.surface.unbind` return.
#[derive(Debug, Serialize, JsonSchema)]
pub struct SurfaceBindingSaved {
    /// The render setup.
    pub name: String,
    /// The setup as saved, as the render settings commands give it.
    pub setup: Value,
    /// Always `definition_local_mm`: the maps are placed in the body's own millimetres.
    pub mapping_space: String,
    /// Always true: the design's geometry and physical materials don't change.
    pub geometry_and_physical_materials_unchanged: bool,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What `render.optics.bind` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct OpticsBindingSaved {
    /// The render setup.
    pub name: String,
    /// The setup as saved, as the render settings commands give it.
    pub setup: Value,
    /// Always true: the design's geometry and engineering data don't change.
    pub geometry_and_engineering_unchanged: bool,
    /// What the image producer checks before it draws the body: closed topology and nested
    /// containment.
    pub certification: String,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What `render.optics.unbind` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct OpticsUnbound {
    /// The render setup.
    pub name: String,
    /// Whether the body had optics to remove; `false` leaves the setup as it was.
    pub removed: bool,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

#[cfg(test)]
#[path = "render_bindings_tests.rs"]
mod tests;
