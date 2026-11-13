//! Contracts and result types for the commands in `cmd/point_cloud.rs` that have them.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::types::IdOrName;

/// Read a point cloud's file again from where it was inserted. The points, colours, segments,
/// triangles and properties are replaced; the name, placement, units, point size and colouring
/// are kept, and so are the filters on properties the file still has. The file is read under the
/// limit the cloud was inserted with, so a file that grew is thinned only past that limit.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct PointCloudReloadParams {
    /// The point cloud, by id or name.
    pub cloud: IdOrName,
}

/// What `pointcloud.reload` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct PointCloudReloadResult {
    /// The cloud's id.
    pub cloud: u64,
    /// How many points it has now.
    pub points: usize,
    /// How many segments it has now.
    pub segments: usize,
    /// How many triangles it has now.
    pub triangles: usize,
    /// What could not be read from the file, or was thinned out of it.
    pub warnings: Vec<String>,
}

/// Delete a point cloud. Its file is left where it is; only the design forgets the cloud.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct PointCloudDeleteParams {
    /// The point cloud, by id or name.
    pub cloud: IdOrName,
}

/// What `pointcloud.delete` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct PointCloudDeleteResult {
    /// The cloud that was deleted, by id.
    pub deleted: u64,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// `pointcloud.list` takes no parameters.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct PointCloudListParams {}

/// What `pointcloud.list` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct PointCloudListResult {
    /// Every point cloud in the design.
    pub clouds: Vec<PointCloudInfo>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// One point cloud, as the design keeps it.
#[derive(Debug, Serialize, JsonSchema)]
pub struct PointCloudInfo {
    /// The cloud's id.
    pub cloud: u64,
    /// Its name in the design.
    pub name: String,
    /// The file it was read from (its name only).
    pub file: String,
    /// The path `pointcloud.reload` reads it again from; empty when the file was not kept.
    pub source: String,
    /// How many points it has now.
    pub points: usize,
    /// How many segments it has.
    pub segments: usize,
    /// How many triangles it has.
    pub triangles: usize,
    /// How many points the file had (more than `points` when the cloud was thinned coming in).
    pub source_points: u64,
    /// The file's length unit: `mm`, `cm`, `m`, `in` or `ft`.
    pub units: String,
    /// Whether it is drawn.
    pub visible: bool,
    /// Size of a point on screen (pixels).
    pub point_size: f64,
    /// How the points are coloured: `file`, `single`, `height` or a property's name.
    pub color_by: String,
    /// The one colour of every point, with `color_by: single`; null means the plain one.
    pub color: Option<[f32; 3]>,
    /// Whether the file carries a colour of each point.
    pub colored: bool,
    /// The properties the file carries, the point coordinates first.
    pub properties: Vec<PointCloudProperty>,
    /// The filters on its properties: only what every filter keeps is shown.
    pub filters: Vec<PointCloudFilter>,
    /// The colour ramp's range now, exact over what the filters keep; null when it is not a ramp.
    pub color_range: Option<[f64; 2]>,
    /// The ramp's own range, or the automatic one by height; null when it is worked out.
    pub height_range: Option<[f64; 2]>,
    /// Whether the ramp's range is worked out from the points.
    pub height_auto: bool,
    /// The box round the cloud in the design, `[[x, y, z], [x, y, z]]` in mm; null for no points.
    pub bounds: Option<[[f64; 3]; 2]>,
    /// Where the cloud sits: a rigid row-major 3×4 matrix, 12 numbers, row by row.
    pub matrix: Vec<f64>,
}

/// One property a point cloud's file carries.
#[derive(Debug, Serialize, JsonSchema)]
pub struct PointCloudProperty {
    /// Its name; `x`, `y` and `z` are the coordinates of each point in the design.
    pub name: String,
    /// Whether its values are whole numbers, which a colour ramp takes as a palette.
    pub integer: bool,
    /// Its smallest and largest value, the coordinates in the design; null when it has no values.
    pub span: Option<[f64; 2]>,
    /// The names of whole-number values, `[number, name]`, as the file gives them; null when it
    /// gives none.
    pub names: Option<Vec<(i32, String)>>,
}

/// A filter on one of a point cloud's properties.
#[derive(Debug, Serialize, JsonSchema)]
pub struct PointCloudFilter {
    /// The property it filters.
    pub property: String,
    #[serde(flatten)]
    pub keep: PointCloudFilterKeeps,
}

/// The values a filter keeps.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum PointCloudFilterKeeps {
    /// Whose value lies in the range, both ends included.
    Range { range: [f64; 2] },
    /// Which are one of these whole numbers, for a property with named values.
    Values { values: Vec<i32> },
}

#[cfg(test)]
#[path = "point_cloud_tests.rs"]
mod tests;
