//! Environment editor commands; saved values retain the existing render setup format.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EnvironmentParams {
    pub name: String,
    /// Complete numerical draft, validated before applying the requested edit.
    #[serde(default = "empty_patch")]
    pub set: Value,
    /// Finite degrees within +/- one million; wrapped to [-180, 180).
    pub rotation_deg: Option<f64>,
    /// Linear intensity: 0..64 for CPU and 0..100 for GPU.
    pub intensity: Option<f64>,
}

fn empty_patch() -> Value {
    serde_json::json!({})
}

#[derive(Serialize, JsonSchema)]
pub struct EnvironmentResult {
    pub name: String,
    pub setup: Value,
}

/// Shared clear-coat numbers; the handler validates the renderer's exact domain.
#[derive(Clone, Copy, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct CoatNumbers {
    pub weight: f64,
    pub roughness: f64,
    pub ior: f64,
    /// Metallic paint flakes under the coat; absent for none.
    pub flakes: Option<FlakeNumbers>,
}
impl Default for CoatNumbers {
    fn default() -> Self {
        let coat = solidcraft_doc::render_coat::CoatMaterial::default();
        Self { weight: coat.weight, roughness: coat.roughness, ior: coat.ior, flakes: None }
    }
}
impl CoatNumbers {
    pub fn material(self) -> solidcraft_doc::render_coat::CoatMaterial {
        solidcraft_doc::render_coat::CoatMaterial {
            weight: self.weight,
            roughness: self.roughness,
            ior: self.ior,
            flakes: self.flakes.map(FlakeNumbers::material),
        }
    }
}
/// Paint flake numbers (docs/render-flakes.md); the handler validates their ranges.
#[derive(Clone, Copy, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct FlakeNumbers {
    /// Share of the surface covered, 0..1.
    pub coverage: f64,
    /// About the width of one flake, 0.005..10 mm.
    pub size_mm: f64,
    /// Tilt from the surface as a share of 45 degrees, 0..1.
    pub spread: f64,
    /// Depth of the deepest flake in the paint, 0..1.
    pub depth: f64,
    /// sRGB color.
    pub color: [u8; 3],
    pub metallic: f64,
    /// Perceptual roughness, 0.045..1.
    pub roughness: f64,
}
impl Default for FlakeNumbers {
    fn default() -> Self {
        let f = solidcraft_doc::render_coat::FlakeMaterial::default();
        Self {
            coverage: f.coverage,
            size_mm: f.size_mm,
            spread: f.spread,
            depth: f.depth,
            color: f.color,
            metallic: f.metallic,
            roughness: f.roughness,
        }
    }
}
impl FlakeNumbers {
    pub fn material(self) -> solidcraft_doc::render_coat::FlakeMaterial {
        let Self { coverage, size_mm, spread, depth, color, metallic, roughness } = self;
        solidcraft_doc::render_coat::FlakeMaterial { coverage, size_mm, spread, depth, color, metallic, roughness }
    }
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CoatBindParams {
    pub name: String,
    /// Numerical shot snapshot; cannot replace existing coat bindings through this patch.
    #[serde(default = "empty_patch")]
    pub set: Value,
    pub body_index: usize,
    pub body_name: String,
    pub coat: CoatNumbers,
    pub expected_revision: u64,
    #[serde(default)]
    pub replace: bool,
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CoatUnbindParams {
    pub name: String,
    /// Exact saved definition/feature/component/occurrence_path, checked by the document type.
    #[schemars(with = "std::collections::BTreeMap<String, Value>")]
    pub target: Value,
    pub expected_revision: u64,
}
#[derive(Serialize, JsonSchema)]
pub struct CoatBindResult {
    pub name: String,
    pub setup: Value,
    pub geometry_and_engineering_unchanged: bool,
}
#[derive(Serialize, JsonSchema)]
pub struct CoatUnbindResult {
    pub name: String,
    pub removed: bool,
}
