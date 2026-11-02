//! Contracts and result types for the 3D sketch constraint commands in `cmd/sketch_3d.rs`.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::types::{SketchRef, present};

/// An axis of the sketch's 3D space.
#[derive(Clone, Copy, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AxisName {
    X,
    Y,
    Z,
}

/// Make drawn 3D lines parallel to an axis.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct AlongAxisParams {
    /// A drawn 3D line's id.
    #[serde(default)]
    pub line: Option<String>,
    /// Several drawn 3D lines' ids, each made parallel to the axis.
    #[serde(default)]
    pub lines: Option<Vec<String>>,
    /// The axis of the sketch's 3D space (its component's, for a sketch in a component).
    pub axis: AxisName,
    /// The sketch (default: the one being edited).
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "SketchRef")]
    pub sketch: Option<SketchRef>,
}

/// What `sketch.constraint.along_axis` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct AlongAxisResult {
    /// The constraint ids, one per line (an existing one when the line already had it).
    pub constraints: Vec<String>,
    /// The sketch's solve: `{solved, dof}`.
    pub sketch: Value,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

#[cfg(test)]
#[path = "sketch_3d_tests.rs"]
mod tests;
