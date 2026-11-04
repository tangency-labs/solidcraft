//! Contracts and result types for the 3D sketch constraint commands in `cmd/sketch_3d.rs`.

use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

use super::types::{SketchRef, present};

/// An axis of the sketch's 3D space: `x`, `y` or `z`, in any case.
#[derive(Clone, Copy, Debug)]
pub enum AxisName {
    X,
    Y,
    Z,
}

impl<'de> Deserialize<'de> for AxisName {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        match Value::deserialize(d)?.as_str().map(str::to_ascii_lowercase).as_deref() {
            Some("x") => Ok(AxisName::X),
            Some("y") => Ok(AxisName::Y),
            Some("z") => Ok(AxisName::Z),
            _ => Err(D::Error::custom("must be x, y or z")),
        }
    }
}

impl JsonSchema for AxisName {
    fn schema_name() -> Cow<'static, str> {
        "AxisName".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({"description": "x, y or z, in any case.", "type": "string", "pattern": "^[XxYyZz]$"})
    }
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

/// An origin plane of the sketch's 3D space: `xy`, `yz` or `xz`, in any case and either order, as
/// Parallel takes them.
#[derive(Clone, Copy, Debug)]
pub enum PlaneName {
    Xy,
    Yz,
    Xz,
}

impl<'de> Deserialize<'de> for PlaneName {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        match Value::deserialize(d)?.as_str().and_then(solidcraft_sketch::OriginPlane::from_name) {
            Some(solidcraft_sketch::OriginPlane::Xy) => Ok(PlaneName::Xy),
            Some(solidcraft_sketch::OriginPlane::Yz) => Ok(PlaneName::Yz),
            Some(solidcraft_sketch::OriginPlane::Xz) => Ok(PlaneName::Xz),
            None => Err(D::Error::custom("must be xy, yz or xz")),
        }
    }
}

impl JsonSchema for PlaneName {
    fn schema_name() -> Cow<'static, str> {
        "PlaneName".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "xy, yz or xz, in any case and either order.",
            "type": "string",
            "pattern": "^([XxYy][XxYy]|[YyZz][YyZz]|[XxZz][XxZz])$"
        })
    }
}

/// Hold 3D points on an origin plane.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct OnPlaneParams {
    /// A 3D point ref: `<curve>.start`, `.end`, `.through`, `.<n>`, or a 3D point's id.
    #[serde(default)]
    pub point: Option<String>,
    /// Several 3D point refs, each held on the plane.
    #[serde(default)]
    pub points: Option<Vec<String>>,
    /// The origin plane of the sketch's 3D space.
    pub plane: PlaneName,
    /// The sketch (default: the one being edited).
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "SketchRef")]
    pub sketch: Option<SketchRef>,
}

/// What `sketch.constraint.on_plane` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct OnPlaneResult {
    /// The constraint ids, one per point (an existing one when the point already had it).
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
