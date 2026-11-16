//! Manufacturing grain command contracts.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum StockAxis {
    Length,
    Width,
    Thickness,
    None,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GrainEdge {
    /// Straight edge index from model.edges, belonging to the body being edited.
    pub edge: usize,
    /// When supplied, must identify the body being edited.
    pub body: Option<String>,
    /// Optional planar face index; the edge must be one of its boundary edges.
    pub face: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum GrainDirection {
    Axis(StockAxis),
    Vector([f64; 3]),
    Edge(GrainEdge),
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GrainParams {
    pub body: String,
    /// Stock axis, world direction vector, or an edge (optionally on a planar face).
    /// "none" clears grain, overriding a frame member's automatic grain.
    pub direction: GrainDirection,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GrainQueryParams {
    pub body: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct GrainResult {
    pub body: String,
    /// Unit direction in world coordinates, or null when the body has no grain.
    pub direction: Option<[f64; 3]>,
    /// Exact stock length along the grain in millimetres, or null without grain.
    pub stock_length: Option<f64>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GrainGuideParams {
    /// Omit to toggle. Initially off; a viewport setting, never saved in the design.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on: Option<bool>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{input_schema_for, tests::assert_keys};

    #[test]
    fn grain_schema_keys_match_the_contract() {
        assert_keys(input_schema_for::<GrainParams>(), &["body", "direction"]);
        assert_keys(input_schema_for::<GrainQueryParams>(), &["body"]);
        assert_keys(input_schema_for::<GrainGuideParams>(), &["on"]);
    }
}
