//! Contracts and result types for the commands in `cmd/sketch_3d.rs`: `sketch.line3d`,
//! `sketch.spline3d`, `sketch.point3d` and `sketch.move3d`.
//!
//! The handlers read their parameters from the raw JSON, so each key here reads as its handler
//! reads it and fails with the handler's message. Whether the sketch has 3D Sketch on, a wire
//! exists or a point is in range stays the handler's to check, as does `move3d` being given
//! neither `to` nor `by`. A call with several problems may get a different one first in strict
//! than with the check off (the check goes key by key, the handlers in their own order).

use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Value, json};
use solidcore_record::Vec3;

use super::check::whole;
use super::motion::require;
use super::sketch_ends::SketchTarget;
use super::types::Point3;
use crate::params;

/// Draw 3D lines through world points, one line per segment, joined end to end. Needs 3D Sketch
/// on for the sketch (`sketch.options` with `sketch_3d: true`). One undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = points_required)]
pub struct Line3dParams {
    // Optional in the type and required in the schema (`points_required`): left out, the handler
    // refuses it with its own message, not the check's "is required".
    /// The world points [x, y, z] in mm, 2 to 500: one line per segment between neighbours.
    #[serde(default, deserialize_with = "some", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "DrawnPoints")]
    pub points: Option<DrawnPoints>,
    /// The sketch to draw in: its feature id or its name. Default: the one being edited.
    #[serde(default)]
    pub sketch: Option<SketchTarget>,
}

/// Draw a smooth 3D curve through world fit points. Needs 3D Sketch on for the sketch
/// (`sketch.options` with `sketch_3d: true`). One undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = points_required)]
pub struct Spline3dParams {
    // Optional in the type and required in the schema, as `Line3dParams::points`.
    /// The world fit points [x, y, z] in mm, 2 to 500, which the curve passes through.
    #[serde(default, deserialize_with = "some", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "DrawnPoints")]
    pub points: Option<DrawnPoints>,
    /// The sketch to draw in: its feature id or its name. Default: the one being edited.
    #[serde(default)]
    pub sketch: Option<SketchTarget>,
}

/// Draw a 3D point at a world point. Needs 3D Sketch on for the sketch (`sketch.options` with
/// `sketch_3d: true`). One undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = point_required)]
pub struct Point3dParams {
    // Optional in the type and required in the schema, as `Line3dParams::points`.
    /// The world point [x, y, z] in mm.
    #[serde(default, deserialize_with = "some", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "WorldPoint")]
    pub point: Option<WorldPoint>,
    /// The sketch to draw in: its feature id or its name. Default: the one being edited.
    #[serde(default)]
    pub sketch: Option<SketchTarget>,
}

/// Move one fit point of a drawn 3D curve or point, to a world point or by a world offset. The
/// points joined to it move too. One undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = move_required)]
pub struct Move3dParams {
    // Optional in the type; the schema requires `wire` or `point` (`move_required`).
    /// The drawn 3D curve or point to move, by id.
    #[serde(default, deserialize_with = "some", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "WireId")]
    pub wire: Option<WireId>,
    /// Or a sketch point on the plane instead of `wire`: `l1.end`, `p3` (a line's end, a fit
    /// point of a spline, a lone point). In a 3D sketch the lines and fit-point splines through it
    /// become drawn 3D curves first; arcs, circles and control-point splines stay on the plane.
    #[serde(default)]
    pub point: Option<String>,
    /// Which fit point of the curve (0 is the first), below 500. Default: 0.
    #[serde(default, deserialize_with = "some", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "FitIndex")]
    pub index: Option<FitIndex>,
    /// Where the fit point goes, as a world point [x, y, z] in mm. Wins over `by`. The schema
    /// requires `to` or `by`; the handler refuses a call with neither.
    #[serde(default, deserialize_with = "some", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "MoveTarget")]
    pub to: Option<MoveTarget>,
    /// How far the fit point moves, as a world offset [dx, dy, dz] in mm. Used when `to` is not
    /// given.
    #[serde(default, deserialize_with = "some", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "MoveTarget")]
    pub by: Option<MoveTarget>,
    /// The sketch the curve is in: its feature id or its name. Default: the one being edited.
    #[serde(default)]
    pub sketch: Option<SketchTarget>,
}

/// A key that, once given (`null` included), must parse as `T`, as the handlers read it.
fn some<'de, D: Deserializer<'de>, T: Deserialize<'de>>(d: D) -> Result<Option<T>, D::Error> {
    T::deserialize(d).map(Some)
}

fn points_required(schema: &mut Schema) {
    require(schema, "points");
}

fn point_required(schema: &mut Schema) {
    require(schema, "point");
}

fn move_required(schema: &mut Schema) {
    schema.insert(
        "allOf".into(),
        json!([
            {"anyOf": [{"required": ["wire"]}, {"required": ["point"]}]},
            {"anyOf": [{"required": ["to"]}, {"required": ["by"]}]},
        ]),
    );
}

/// The `points` of `sketch.line3d` and `sketch.spline3d`: a list of 2 to 500 points, as their
/// `points` reader takes it, with its messages.
#[derive(Clone, Debug, PartialEq)]
pub struct DrawnPoints(pub Vec<Vec3>);

impl<'de> Deserialize<'de> for DrawnPoints {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let max = solidcraft_sketch::MAX_FIT_POINTS;
        let Value::Array(a) = Value::deserialize(d)? else {
            return Err(D::Error::custom(whole("`points` must list [x,y,z] points")));
        };
        if a.len() < 2 || a.len() > max {
            return Err(D::Error::custom(whole(format!("`points` needs 2 to {max} points"))));
        }
        a.iter()
            .map(|v| params::vec3(v).ok_or_else(|| D::Error::custom(whole("a point must be [x, y, z]"))))
            .collect::<Result<_, _>>()
            .map(DrawnPoints)
    }
}

impl JsonSchema for DrawnPoints {
    fn schema_name() -> Cow<'static, str> {
        "DrawnPoints".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let point = generator.subschema_for::<Point3>();
        json_schema!({
            "description": "World points [x, y, z] in mm, 2 to 500.",
            "type": "array",
            "items": point,
            "minItems": 2,
            "maxItems": solidcraft_sketch::MAX_FIT_POINTS
        })
    }
}

/// The `point` of `sketch.point3d`: a [`Point3`], with the handler's message.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WorldPoint(pub Vec3);

impl<'de> Deserialize<'de> for WorldPoint {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        params::vec3(&Value::deserialize(d)?).map(WorldPoint).ok_or_else(|| D::Error::custom("must be a point [x, y, z]"))
    }
}

impl JsonSchema for WorldPoint {
    fn schema_name() -> Cow<'static, str> {
        "WorldPoint".into()
    }
    fn inline_schema() -> bool {
        true
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        generator.subschema_for::<Point3>()
    }
}

/// The `to` and `by` of `sketch.move3d`: a [`Point3`], with the handler's message.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MoveTarget(pub Vec3);

impl<'de> Deserialize<'de> for MoveTarget {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        params::vec3(&Value::deserialize(d)?).map(MoveTarget).ok_or_else(|| D::Error::custom("must be [x,y,z]"))
    }
}

impl JsonSchema for MoveTarget {
    fn schema_name() -> Cow<'static, str> {
        "MoveTarget".into()
    }
    fn inline_schema() -> bool {
        true
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        generator.subschema_for::<Point3>()
    }
}

/// The `wire` of `sketch.move3d`: text, as the handler reads it. Whether a drawn curve or point
/// has that id is the handler's to check.
#[derive(Clone, Debug, PartialEq)]
pub struct WireId(pub String);

impl<'de> Deserialize<'de> for WireId {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match Value::deserialize(d)? {
            Value::String(s) => Ok(WireId(s)),
            _ => Err(D::Error::custom("must be a 3D curve or point id")),
        }
    }
}

impl JsonSchema for WireId {
    fn schema_name() -> Cow<'static, str> {
        "WireId".into()
    }
    fn inline_schema() -> bool {
        true
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({"type": "string"})
    }
}

/// The `index` of `sketch.move3d`: a whole number below 500, as the handler reads it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FitIndex(pub usize);

impl<'de> Deserialize<'de> for FitIndex {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Value::deserialize(d)?
            .as_u64()
            .and_then(|v| usize::try_from(v).ok())
            .filter(|v| *v < solidcraft_sketch::MAX_FIT_POINTS)
            .map(FitIndex)
            .ok_or_else(|| D::Error::custom("must be a fit point index below 500"))
    }
}

impl JsonSchema for FitIndex {
    fn schema_name() -> Cow<'static, str> {
        "FitIndex".into()
    }
    fn inline_schema() -> bool {
        true
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({"type": "integer", "minimum": 0, "maximum": solidcraft_sketch::MAX_FIT_POINTS - 1})
    }
}

/// What `sketch.line3d` and `sketch.spline3d` return.
#[derive(Debug, Serialize, JsonSchema)]
pub struct Curves3dResult {
    /// The ids of the curves drawn: one per segment for a line, one for a spline.
    pub curves: Vec<String>,
    /// The sketch's solve: `{solved, dof}`.
    pub sketch: Value,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What `sketch.point3d` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct Point3dResult {
    /// The id of the point drawn.
    pub point: String,
    /// The sketch's solve: `{solved, dof}`.
    pub sketch: Value,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What `sketch.move3d` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct Move3dResult {
    /// How many fit points moved: the one asked for and the points joined to it.
    pub moved: usize,
    /// For a sketch `point`: the drawn 3D curves the lines and splines through it became.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lifted: Option<Vec<String>>,
    /// For a sketch `point`: the drawn curve the point is on now, and its fit point there.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wire: Option<String>,
    /// For a sketch `point`: which fit point of `wire` it is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index: Option<usize>,
    /// The sketch's solve: `{solved, dof}`.
    pub sketch: Value,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

#[cfg(test)]
#[path = "sketch_3d_tests.rs"]
mod tests;
