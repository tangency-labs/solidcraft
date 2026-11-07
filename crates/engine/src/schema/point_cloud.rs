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

#[cfg(test)]
#[path = "point_cloud_tests.rs"]
mod tests;
