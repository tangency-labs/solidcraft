//! Exact model query contracts; coordinates are model-space millimetres.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FaceOutlinesParams {
    pub body: String,
    /// Current face indices from model.faces; omitted selects analytic planar faces.
    /// Indices must be refreshed after a model edit; they are not persistent references.
    #[schemars(length(max = 256))]
    pub faces: Option<Vec<FaceIndex>>,
    /// Position in the selected face list, not a kernel face index.
    #[serde(default)]
    pub offset: usize,
    /// Maximum faces per page, 1..64. A byte budget can shorten a page.
    #[serde(default = "page_size")]
    #[schemars(range(min = 1, max = 64))]
    pub limit: usize,
}
fn page_size() -> usize {
    64
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FaceIndex {
    pub index: usize,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct FaceOutlines {
    pub body: String,
    pub faces: Vec<FaceOutline>,
    /// Next position in the same selection; null means the page is complete.
    pub next: Option<usize>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct FaceOutline {
    pub index: usize,
    pub name: Option<String>,
    /// Whether the persistent name is exact, as in model.faces (not a mesh accuracy flag).
    pub exact: bool,
    pub plane: OutlinePlane,
    pub loops: Vec<OutlineRegion>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct OutlinePlane {
    pub origin: [f64; 3],
    pub normal: [f64; 3],
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct OutlineRegion {
    pub outer: Vec<OutlineSegment>,
    pub holes: Vec<Vec<OutlineSegment>>,
}

/// Analytic B-rep geometry. Arc sweep is right-handed about axis from start.
/// Loops follow the underlying surface's natural orientation, not necessarily its outward normal.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OutlineSegment {
    Line { a: [f64; 3], b: [f64; 3] },
    Arc { center: [f64; 3], radius: f64, axis: [f64; 3], start: [f64; 3], sweep_rad: f64 },
    Circle { center: [f64; 3], radius: f64, axis: [f64; 3], start: [f64; 3], sweep_rad: f64 },
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ClassifyPointParams {
    pub body: String,
    /// Finite model-space coordinates in millimetres.
    pub point: [f64; 3],
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ClassifiedPoint {
    pub body: String,
    pub class: PointClass,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PointClass {
    Inside,
    Outside,
    On,
}
