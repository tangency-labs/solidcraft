//! Typed parameter contract and result schema for `scan.align` (#14353).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::types::IdOrName;

/// Align a point cloud or mesh occurrence(s) by defining a 3-2-1 source datum frame (primary
/// plane, in-plane secondary direction, and projected origin) and mapping it rigidly (`det = +1`)
/// onto the model origin frame or an explicit target frame.
///
/// Pass `preview: true` to compute the fit residuals, source/target frames, and rigid transform
/// without modifying the document.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct ScanAlignParams {
    /// Point cloud to align, by id or name. Give `cloud` or occurrence target keys, not both.
    #[serde(default)]
    pub cloud: Option<IdOrName>,
    /// Single occurrence to align, by id or name.
    #[serde(default)]
    pub occurrence: Option<IdOrName>,
    /// One or more occurrences to align, by id or name (1..64).
    #[serde(default)]
    pub occurrences: Option<Value>,
    /// Single occurrence path `[parent_id, ..., occurrence_id]` (1..64 ids).
    #[serde(default)]
    pub path: Option<Vec<u64>>,
    /// One or more occurrence paths `[[ids...], ...]` (1..64 paths).
    #[serde(default)]
    pub paths: Option<Vec<Vec<u64>>>,
    /// Primary plane on the scan:
    /// - `{"points": [[x,y,z], ...]}` (at least 3 noncollinear sample points for least-squares fit),
    /// - a plane name (`"XY"`, `"XZ"`, `"YZ"`, or a construction plane name),
    /// - `{"origin": [x,y,z], "normal": [x,y,z]}`, or
    /// - `{"face": [x,y,z]}` (a point on a planar face).
    pub plane: Value,
    /// Secondary direction (projected into the primary plane):
    /// - `{"from": [x,y,z], "to": [x,y,z]}` (two distinct points),
    /// - `{"vector": [x,y,z]}` (explicit non-zero vector),
    /// - `{"edge": [x,y,z]}` (a point on a straight model edge; curved edges are rejected), or
    /// - an axis name (`"X"`, `"Y"`, `"Z"`, `"-X"`, `"-Y"`, `"-Z"`, or a construction axis name).
    pub direction: Value,
    /// Explicit source frame origin `[x, y, z]` (or `"origin"` for `[0, 0, 0]`), projected
    /// orthogonally onto the primary plane.
    pub origin: Value,
    /// Target frame to align onto. Default: world origin frame (`XY` plane, `+X` direction,
    /// `[0, 0, 0]` origin). Accepts `"world"`, `"XY"|"XZ"|"YZ"`, a construction plane name, or
    /// `{"origin": [x,y,z], "x_axis": [x,y,z], "z_axis": [x,y,z]}`.
    #[serde(default)]
    pub target: Option<Value>,
    /// Reverse the primary plane normal (`Z`) while keeping a right-handed frame (`det = +1`).
    #[serde(default)]
    pub flip_normal: Option<bool>,
    /// Reverse the projected in-plane direction (`X`) while keeping a right-handed frame (`det = +1`).
    #[serde(default)]
    pub flip_direction: Option<bool>,
    /// Planarity and degeneracy tolerance in mm. Default: `1e-6`.
    #[serde(default)]
    pub tolerance: Option<f64>,
    /// `true` computes fit statistics and placement without changing the document. Default: `false`.
    #[serde(default)]
    pub preview: Option<bool>,
    /// For occurrence targets: `true` (default) captures the new placement in one undo step;
    /// `false` stores it as a pending move (like `occurrence.align`).
    #[serde(default)]
    pub capture: Option<bool>,
    /// For occurrence targets: allow aligning grounded occurrences. Default: `false`.
    #[serde(default)]
    pub move_grounded: Option<bool>,
}

/// Primary plane fit and unsigned residual statistics (in mm).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ScanPlaneFitInfo {
    /// Point on the plane (centroid of samples for fitted planes), in mm.
    pub origin: [f64; 3],
    /// Unit plane normal (before optional `flip_normal`).
    pub normal: [f64; 3],
    /// Number of sample points (`0` for an existing explicit plane).
    pub sample_count: usize,
    /// Root-mean-square of unsigned point-to-plane distances, in mm.
    pub rms_residual_mm: f64,
    /// Maximum absolute point-to-plane distance, in mm.
    pub max_abs_residual_mm: f64,
    /// Whether `max_abs_residual_mm <= tolerance`.
    pub is_planar: bool,
}

/// Explicit source origin and its orthogonal projection onto the primary plane (in mm).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ScanOriginInfo {
    /// Original picked/supplied origin point, in mm.
    pub raw_origin: [f64; 3],
    /// Orthogonal projection onto the primary plane, in mm.
    pub projected_origin: [f64; 3],
    /// Unsigned distance from `raw_origin` to the primary plane, in mm.
    pub off_plane_distance_mm: f64,
}

/// Right-handed orthonormal 3D frame (`x × y = z`, `det = +1`).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ScanFrameInfo {
    /// Frame origin, in mm.
    pub origin: [f64; 3],
    /// Unit X axis.
    pub x_axis: [f64; 3],
    /// Unit Y axis (`z_axis × x_axis`).
    pub y_axis: [f64; 3],
    /// Unit Z axis (plane normal).
    pub z_axis: [f64; 3],
}

/// What `scan.align` returns (for both `preview: true` and applied alignment).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ScanAlignResult {
    /// Whether this call only previewed without modifying the document.
    pub preview: bool,
    /// Target kind: `"cloud"` or `"occurrences"`.
    pub target_kind: String,
    /// Aligned point cloud id, when `target_kind == "cloud"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cloud: Option<u64>,
    /// Aligned occurrence ids, when `target_kind == "occurrences"`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub occurrences: Vec<u64>,
    /// Aligned occurrence paths, when `target_kind == "occurrences"`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub paths: Vec<Vec<u64>>,
    /// Human-readable names of everything that moved (or will move in preview).
    pub moved_names: Vec<String>,
    /// Shared child occurrence paths that also move in other instances of their parent component.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub shared: Vec<Vec<u64>>,
    /// For occurrence targets: whether the move was left pending (`capture: false`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pending: Option<bool>,
    /// Primary plane fit and unsigned residuals in mm.
    pub plane: ScanPlaneFitInfo,
    /// Raw and projected source origin in mm.
    pub origin: ScanOriginInfo,
    /// Resolved right-handed source frame.
    pub source_frame: ScanFrameInfo,
    /// Resolved right-handed target frame.
    pub target_frame: ScanFrameInfo,
    /// World-space proper rigid delta (column-major 4×4 `m[col][row]`, matching `solidcraft_doc::Mat`).
    pub world_transform: [[f64; 4]; 4],
    /// World-space proper rigid delta as 16 row-major numbers.
    pub world_matrix: Vec<f64>,
    /// For point clouds: the final composed placement (column-major 4×4 `m[col][row]`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub composed_transform: Option<[[f64; 4]; 4]>,
    /// For point clouds: the final composed placement matrix (16 row-major numbers).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub composed_matrix: Option<Vec<f64>>,
    /// Determinant of the 3×3 rotation matrix (guaranteed `+1.0` within tolerance).
    pub determinant: f64,
    /// Maximum absolute entry of `RᵀR − I`.
    pub orthogonality_error: f64,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}
