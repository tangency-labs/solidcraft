//! Contract and reply of `surface.offset_curve` (`cmd/surface_offset_curve.rs`, #22414): a curve
//! at a distance from an edge of a face, or from a curve lying on it, measured along the face.
//!
//! Whether the face, the edge or the sketch curve exist, and whether the curve lies on the face,
//! are the handler's (and the design's) to check.

use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

use super::check::whole;
use super::motion::require;
use super::sketch_project::LinkMade;
use super::types::Point3;
use crate::cmd::{RefForm, ref_form};

/// The handler's message for `from`.
pub(crate) const FROM_MESSAGE: &str =
    "`from` must be {edge: [x,y,z]} (an edge of the face) or {sketch, curve} (a curve of another sketch lying on the face)";

fn some<'de, D: Deserializer<'de>, T: Deserialize<'de>>(d: D) -> Result<Option<T>, D::Error> {
    T::deserialize(d).map(Some)
}

/// `surface.offset_curve`'s `from`: an edge `{edge: [x,y,z]}` or a sketch curve `{sketch, curve}`.
#[derive(Clone, Debug, PartialEq)]
pub struct OffsetFrom(pub Value);

impl<'de> Deserialize<'de> for OffsetFrom {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = Value::deserialize(d)?;
        match ref_form(&v) {
            Ok(RefForm::Edge { .. }) | Ok(RefForm::Sketch { curve: Some(_), .. }) => Ok(OffsetFrom(v)),
            _ => Err(D::Error::custom(whole(FROM_MESSAGE))),
        }
    }
}

impl JsonSchema for OffsetFrom {
    fn schema_name() -> Cow<'static, str> {
        "OffsetCurveFrom".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let point = generator.subschema_for::<Point3>();
        json_schema!({
            "description": "What to offset: {edge: [x, y, z]} (the face's edge through that point) or {sketch: id or name, curve: id} (a curve or 3D curve of another sketch lying on the face).",
            "anyOf": [
                {"type": "object", "properties": {"edge": point, "body": {"type": "string"}}, "required": ["edge"]},
                {"type": "object", "properties": {"sketch": {"type": ["string", "integer"]}, "curve": {"type": "string"}}, "required": ["sketch", "curve"]}
            ]
        })
    }
}

/// Add a curve to the active sketch at `distance` from an edge of a face, or from a curve lying on
/// the face, measured along the face (each point is that far along the straightest path on the
/// face that leaves the source square to it), as a linked 3D curve that follows the face and the
/// source. On a plane it is the ordinary offset; on a cylinder, cone or extrusion it is the offset
/// in the face unrolled flat, exactly. Where the offset would leave the face it stops at the
/// face's edge. One undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = required)]
pub struct OffsetCurveParams {
    /// A point [x, y, z] in mm on the face. It also picks the side to offset to when the face lies
    /// on both sides of the source (a curve inside the face); from an edge the curve goes into the
    /// face.
    #[serde(default, deserialize_with = "some", skip_serializing_if = "Option::is_none")]
    pub face: Option<Point3>,
    /// The face's body. Default: the body the point is on. Text only; anything else is read as
    /// left out.
    #[serde(default)]
    pub body: Option<String>,
    /// What to offset.
    #[serde(default, deserialize_with = "some", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "OffsetFrom")]
    pub from: Option<OffsetFrom>,
    /// How far along the face, in mm (more than 0).
    #[serde(default, deserialize_with = "some", skip_serializing_if = "Option::is_none")]
    pub distance: Option<f64>,
    /// How far (mm, more than 0) the fitted cubic B-spline may be from the true offset, measured
    /// between the points it is made through. Default: 0.01. A straight offset comes out exactly
    /// straight; a circular one is a spline within the tolerance.
    #[serde(default)]
    pub tolerance: Option<f64>,
}

fn required(schema: &mut Schema) {
    require(schema, "face");
    require(schema, "from");
    require(schema, "distance");
    // Both lengths are more than 0, as the handler checks.
    if let Some(props) = schema.get_mut("properties").and_then(Value::as_object_mut) {
        for key in ["distance", "tolerance"] {
            if let Some(p) = props.get_mut(key).and_then(Value::as_object_mut) {
                p.insert("exclusiveMinimum".into(), Value::from(0.0));
            }
        }
    }
}

/// What `surface.offset_curve` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct OffsetCurveResult {
    #[serde(flatten)]
    pub made: LinkMade,
    /// The largest distance (mm) found between the curve (a cubic B-spline through the exact
    /// offset points) and the true offset, measured between the points it was made through.
    pub deviation: f64,
    /// The face's boundary cut the offset short (it would have left the face).
    pub clipped: bool,
    /// What the command added to the sketch, as every sketch command reports it (the engine adds
    /// it): `{points, curves, constraints, dimensions}`, and `wires` when there are any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created: Option<Value>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}
