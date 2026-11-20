//! Contracts and reply types for the read-only render information commands (#12832): Render
//! Backends and Inspect Render Scene in `cmd/render.rs`, and Layered Surface Aging and Lens and
//! Creative Effects in `cmd/render/effects.rs`. Each takes an empty object only, as its handler
//! does, and changes nothing.
//!
//! Values the render crate defines (a backend's description, a resource profile's limits, an
//! aging or look preset, the photographic defaults) are published as JSON objects: that crate
//! has no `JsonSchema`, and these commands describe them for reading, not for sending back.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// A JSON object whose fields the render crate defines.
pub type RenderObject = Map<String, Value>;

/// No parameters: an empty object, as the handlers take it.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct NoParams {}

/// A CPU render resource profile: its id and its image, geometry, memory and work limits.
#[derive(Debug, Serialize, JsonSchema)]
pub struct CpuResourceProfile {
    /// The profile's id (`compact`, `studio`, `workstation`).
    pub id: Value,
    pub limits: RenderObject,
}

/// What `render.backends` returns: metadata only; nothing renders and no GPU is probed.
#[derive(Debug, Serialize, JsonSchema)]
pub struct RenderBackends {
    /// The version of this reply's shape: 1.
    pub schema_version: u32,
    /// The backend a render uses unless a setup names another: `builtin`.
    pub default_backend: String,
    /// Each backend with its executors, their availability, profiles and limits.
    pub backends: Vec<RenderObject>,
    /// The CPU raytracer's resource profiles.
    pub cpu_resource_profiles: Vec<CpuResourceProfile>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// An axis-aligned box, in world millimetres.
#[derive(Debug, Serialize, JsonSchema)]
pub struct Bounds {
    pub min: [f64; 3],
    pub max: [f64; 3],
}

/// One body the renderer would draw.
#[derive(Debug, Serialize, JsonSchema)]
pub struct SceneBody {
    /// Its place in the capture order: `body_index` in the surface and optics commands.
    pub ordinal: usize,
    /// Its name: `body_name` in those commands.
    pub name: String,
    /// The stable definition, feature, component or occurrence target the saved bindings use for
    /// it, or `null` when it has none.
    pub stable_target: Option<Value>,
    /// How many triangles its render mesh has.
    pub triangles: usize,
    /// Its colour, [r, g, b] from 0 to 255.
    pub color: [u8; 3],
    /// How opaque it is: 1 is solid.
    pub opacity: f64,
    /// How many of its faces have an appearance of their own.
    pub face_appearances: usize,
    pub bounds: Bounds,
}

/// What `render.scene` returns: the product scene, without modelling overlays or mesh data.
#[derive(Debug, Serialize, JsonSchema)]
pub struct RenderSceneInfo {
    pub bodies: Vec<SceneBody>,
    /// The bounds of every body, or `null` when there is none.
    pub bounds: Option<Bounds>,
    /// Always false: modelling overlays (sketches, planes, handles) are never in the scene.
    pub overlays: bool,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What `render.aging.describe` returns: the surface aging presets and what aging does.
#[derive(Debug, Serialize, JsonSchema)]
pub struct AgingDescription {
    /// The most aging layers one job stacks.
    pub maximum_layers: u32,
    /// Where a render job keeps its aging stack.
    pub storage: String,
    /// The order the layers apply in.
    pub order: String,
    /// One preset layer for each kind of aging.
    pub presets: Vec<RenderObject>,
    /// Which bodies a layer may apply to.
    pub target: String,
    /// The space the aging patterns are laid out in.
    pub coordinate_space: String,
    /// What aging never changes.
    pub preserves: Vec<String>,
    pub limitations: Vec<String>,
    /// What to use for dust in the air instead.
    pub airborne: String,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What `render.effects.describe` returns: the creative looks and photographic defaults.
#[derive(Debug, Serialize, JsonSchema)]
pub struct EffectsDescription {
    /// The most look layers one job stacks.
    pub maximum_look_layers: u32,
    /// Where a render job keeps its finishing.
    pub storage: String,
    /// One layer for each kind of look, with its default settings.
    pub looks: Vec<RenderObject>,
    /// The photographic presentation's default settings.
    pub photographic_default: RenderObject,
    /// Where lens flare takes its light from.
    pub flare_source: String,
    /// What finishing never changes.
    pub preserves: Vec<String>,
    pub limitations: Vec<String>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

#[cfg(test)]
#[path = "render_info_tests.rs"]
mod tests;
