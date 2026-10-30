//! Contracts for explicit source-to-destination placement.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{Point2, Point3};

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Object {
    Body { name: String },
    Occurrence { path: Vec<u64> },
}

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct CopyParams {
    #[serde(default)]
    pub objects: Option<Vec<Object>>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct MoveParams {
    #[serde(default)]
    pub objects: Option<Vec<Object>>,
    pub from: Point3,
    pub to: Point3,
    #[serde(default)]
    pub copy: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum Anchor {
    Sketch(Point2),
    World(Point3),
}

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct PasteParams {
    pub from: Anchor,
    pub to: Anchor,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct CopyResult {
    pub copied: Vec<Object>,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum MoveResult {
    Feature(super::features::FeatureResult),
    Occurrence { occurrence: u64, component: Option<u64>, transform: Option<[[f64; 4]; 4]>, pending: Option<bool> },
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum PasteResult {
    Objects(MoveResult),
    Sketch { curves: Vec<String>, points: Vec<String>, constraints: usize, translate: [f64; 2], sketch: Value },
    Features { pasted: Vec<PastedFeature>, errors: Vec<FeatureError>, notices: Vec<String> },
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct PastedFeature {
    pub id: u64,
    pub name: String,
    pub from: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct FeatureError {
    pub feature: u64,
    pub error: String,
}
