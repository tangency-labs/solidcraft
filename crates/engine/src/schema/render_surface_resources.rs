//! Explicit import of opaque appearance maps into a design-owned render setup.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

fn patch() -> Value {
    serde_json::json!({})
}
fn strength() -> f64 {
    1.0
}

#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TextureFile {
    /// Scoped local image path, read once and never saved in the design.
    pub path: String,
    /// Optional lowercase SHA-256 from an earlier selection or inspection.
    pub expected_sha256: Option<String>,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TextureFiles {
    /// Encoded sRGB color; normalized before interpolation.
    pub base_color_srgb: Option<TextureFile>,
    /// OpenGL tangent normal; linear data, never a color image.
    pub normal_opengl: Option<TextureFile>,
    pub roughness_linear: Option<TextureFile>,
    pub metallic_linear: Option<TextureFile>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Provenance {
    pub author: String,
    /// Inert source locator; importing never fetches this address.
    pub source: String,
    /// User-declared license or ownership; does not certify permission.
    pub license: String,
}

#[derive(Clone, Copy, Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SurfaceFrame {
    pub origin: [f64; 3],
    /// One physical repeat in world millimetres.
    pub u: [f64; 3],
    pub v: [f64; 3],
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ImportParams {
    pub name: String,
    #[serde(default = "patch")]
    pub set: Value,
    pub body_index: usize,
    pub body_name: String,
    pub frame: SurfaceFrame,
    pub maps: TextureFiles,
    pub provenance: Provenance,
    #[serde(default = "strength")]
    pub normal_strength: f64,
    #[serde(default)]
    pub flip_v: bool,
    #[serde(default)]
    pub replace: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListParams {}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RemoveParams {
    pub sha256: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ImportResult {
    pub name: String,
    pub setup: Value,
    pub resources: Vec<String>,
    pub self_contained: bool,
    pub mapping_space: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ResourceInfo {
    pub sha256: String,
    pub encoded_bytes: usize,
    pub dimensions: [u32; 2],
    pub provenance: Value,
    pub users: Vec<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ListResult {
    pub resources: Vec<ResourceInfo>,
    pub encoded_bytes: usize,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct RemoveResult {
    pub sha256: String,
    pub removed: bool,
}
