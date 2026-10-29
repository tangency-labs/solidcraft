use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BlendImport {
    pub path: String,
    #[serde(default)]
    pub objects: Vec<String>,
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BlendExport {
    pub path: String,
    #[serde(default)]
    pub features: Vec<u64>,
    pub name: Option<String>,
    #[serde(default)]
    pub surface: bool,
    pub tolerance: Option<f64>,
    pub operation: Option<String>,
    pub targets: Option<Vec<String>>,
}
#[derive(Serialize, JsonSchema)]
pub struct BlendObject {
    pub name: String,
    pub points: usize,
    pub faces: usize,
    pub levels: usize,
}
#[derive(Serialize, JsonSchema)]
pub struct BlendReply {
    pub path: String,
    pub objects: Vec<BlendObject>,
    pub rejected: Vec<String>,
    pub active: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub surface: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bytes: Option<usize>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Create {
    pub dimensions: Option<[f64; 3]>,
    pub segments: Option<[usize; 3]>,
    pub radii: Option<[f64; 2]>,
    pub path: Option<Vec<[f64; 3]>>,
    #[serde(default = "box_kind")]
    pub primitive: String,
    #[serde(default = "size")]
    pub size: f64,
    #[serde(default = "spans")]
    pub spans: usize,
    #[serde(default = "levels")]
    pub levels: usize,
}
fn box_kind() -> String {
    "box".into()
}
fn size() -> f64 {
    40.0
}
fn spans() -> usize {
    2
}
fn levels() -> usize {
    3
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Parameters {
    pub dimensions: Option<[f64; 3]>,
    pub segments: Option<[usize; 3]>,
    pub radii: Option<[f64; 2]>,
    pub path: Option<Vec<[f64; 3]>>,
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Pipe {
    pub sketch: Option<u64>,
    pub curves: Option<Vec<String>>,
    pub body: Option<String>,
    pub edges: Option<Vec<usize>>,
    pub diameter: Option<f64>,
    pub section_density: Option<usize>,
    pub path_samples: Option<usize>,
    pub overrides: Option<Vec<PipeOverride>>,
    #[serde(default)]
    pub regenerate: bool,
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PipeOverride {
    pub segment: String,
    pub diameter: Option<f64>,
    pub samples: Option<usize>,
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetVertices {
    pub vertices: Vec<usize>,
    pub positions: Vec<[f64; 3]>,
    #[serde(default = "yes")]
    pub checkpoint: bool,
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Transform {
    pub matrix: [[f64; 4]; 4],
    #[serde(default = "yes")]
    pub checkpoint: bool,
}
fn yes() -> bool {
    true
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Quality {
    pub levels: usize,
}
#[derive(Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProjectionTool {
    Flatten,
    Straighten,
}
#[derive(Default, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProjectionPoints {
    Control,
    #[default]
    Surface,
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Project {
    pub tool: ProjectionTool,
    pub vertices: Vec<usize>,
    #[serde(default)]
    pub points: ProjectionPoints,
    pub direction: Option<[f64; 3]>,
    pub origin: Option<[f64; 3]>,
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Symmetry {
    pub axis: Option<usize>,
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Empty {}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Commit {
    pub name: Option<String>,
    pub operation: Option<String>,
    #[serde(default)]
    pub targets: Vec<String>,
    pub tolerance: Option<f64>,
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Edit {
    pub feature: u64,
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Topology {
    pub tool: String,
    #[serde(default)]
    pub faces: Vec<usize>,
    #[serde(default)]
    pub vertices: Vec<usize>,
    #[serde(default)]
    pub edges: Vec<[usize; 2]>,
    #[serde(default)]
    pub displacement: [f64; 3],
    #[serde(default)]
    pub distance: f64,
    #[serde(default = "yes")]
    pub sharp: bool,
    #[serde(default)]
    pub fill_mode: FillMode,
}
#[derive(Default, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FillMode {
    #[default]
    Automatic,
    Single,
    Minimal,
    ReducedStar,
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InsertLoop {
    pub edge: [usize; 2],
    #[serde(default = "half")]
    pub fraction: f64,
}
fn half() -> f64 {
    0.5
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ToolPreview {
    pub pipe: Option<Pipe>,
    pub surface: Option<Surface>,
    pub matching: Option<Match>,
    pub topology: Option<Topology>,
    pub project: Option<Project>,
    pub unweld: Option<Unweld>,
    pub weld: Option<Weld>,
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Surface {
    pub partition: Option<bool>,
    pub body: String,
    pub face: usize,
    #[serde(default = "two")]
    pub segments: [usize; 2],
    #[serde(default)]
    pub continuity: usize,
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Match {
    #[serde(default)]
    pub adjacent_faces: bool,
    #[serde(default)]
    pub wire_from_face: bool,
    pub body: String,
    pub face: Option<usize>,
    pub edges: Option<Vec<usize>>,
    pub edge: [usize; 2],
    #[serde(default)]
    pub order: usize,
    pub position_mm: Option<f64>,
    pub tangent_deg: Option<f64>,
    pub curvature_relative: Option<f64>,
    pub tolerance: Option<f64>,
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Unweld {
    pub edges: Vec<[usize; 2]>,
    #[serde(default = "yes")]
    pub separate: bool,
    pub name: Option<String>,
}
#[derive(Clone, Copy, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum FormObject {
    Draft { part: u64 },
    Feature { feature: u64 },
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Weld {
    pub other: FormObject,
    pub first_edge: [usize; 2],
    pub second_edge: [usize; 2],
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Refine {
    pub faces: Vec<usize>,
    #[serde(default = "two")]
    pub counts: [usize; 2],
}
fn two() -> [usize; 2] {
    [2, 2]
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InsertEdge {
    pub face: usize,
    #[serde(default)]
    pub axis: usize,
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EdgePoint {
    pub edge: [usize; 2],
    pub fraction: f64,
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InsertPoints {
    pub points: Vec<EdgePoint>,
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExtrudeDrag {
    #[serde(default)]
    pub faces: Vec<usize>,
    #[serde(default)]
    pub edges: Vec<[usize; 2]>,
    pub displacement: [f64; 3],
    #[serde(default = "yes")]
    pub checkpoint: bool,
}
#[derive(Serialize, JsonSchema)]
pub struct Reply {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub imported: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pipe_segments: Option<Vec<serde_json::Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pipe: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub matching: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub match_report: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parts: Option<Vec<serde_json::Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub consumed_features: Option<Vec<u64>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub placement: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_preview: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hierarchy: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symmetry: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edge_creases: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vertex_creases: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parameters: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub editing_feature: Option<u64>,
    pub active: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub points: Option<Vec<[f64; 3]>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub faces: Option<Vec<Vec<usize>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub levels: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub triangles: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub undo: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub redo: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revision: Option<u64>,
}
