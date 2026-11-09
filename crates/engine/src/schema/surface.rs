//! Contracts for the surface edit commands in `cmd/surface.rs` (#12627): Stitch, Unstitch, Reverse
//! Normal, Thicken, Offset Surface, Untrim and Fair Surface. Each adds a feature and returns
//! `add_feature`'s reply, a [`super::features::FeatureResult`].
//!
//! The handlers read their parameters from the raw JSON, so each key here reads as its handler
//! reads it and fails with the handler's message. Whether a body or a face exists, and whether a
//! value evaluates and fits the surface, stay the handlers' to check. Where a handler ignores a
//! key of the wrong type (a `chain` that isn't true or false, an item of `faces` that isn't a
//! point, which the handlers skip), the contract refuses it.

use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer};
use serde_json::Value;

use super::check::whole;
use super::config::NameList;
use super::motion::require;
use super::types::{FeatureCommon, LengthExpr, OperationParam, Point3, TouchingParam, feature_common, present};
use crate::params::{self, MAX_STRING_LIST};

/// The most faces, or holes, one call may pick, as `cmd/surface.rs` reads them.
const MAX_PICKS: usize = 10_000;

/// A point's schema, as [`Point3`] publishes it: [x, y, z] or {x, y, z}, in mm.
fn point_schema(generator: &mut SchemaGenerator) -> Schema {
    generator.subschema_for::<Point3>()
}

/// `body` for Unstitch, Reverse Normal and Fair Surface: text, with the handler's message for
/// anything else (`body_faces` reads a body that isn't text as left out).
fn body_name<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    match Value::deserialize(d)? {
        Value::String(name) => Ok(name),
        _ => Err(D::Error::custom(whole("`body` is required"))),
    }
}

/// Points on faces of one body, at most 10 000: the faces Unstitch takes out, Reverse Normal turns
/// over or Fair Surface fairs. The handlers skip an item that isn't a point, and read `faces`
/// that isn't a list as none; the contract refuses both.
#[derive(Clone, Debug, PartialEq)]
pub struct FacePoints(pub Vec<Value>);

impl<'de> Deserialize<'de> for FacePoints {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match Value::deserialize(d)? {
            Value::Array(points) if points.len() > MAX_PICKS => Err(D::Error::custom(whole("too many faces"))),
            Value::Array(points) if points.iter().all(|p| params::vec3(p).is_some()) => Ok(FacePoints(points)),
            _ => Err(D::Error::custom("must list points [x, y, z] on the body's faces")),
        }
    }
}

impl JsonSchema for FacePoints {
    fn schema_name() -> Cow<'static, str> {
        "FacePoints".into()
    }
    fn inline_schema() -> bool {
        true
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        json_schema!({"type": "array", "items": point_schema(generator), "maxItems": MAX_PICKS})
    }
}

/// The handlers' message for `bodies` with no body in it.
const BODIES_MESSAGE: &str = "`bodies` must list bodies";

/// `bodies` for Stitch and Thicken: a name or a list of 1 to 10 000 names, read as
/// `params::string_list` reads it (through [`NameList`]), with its messages. `string_list` reads
/// `null` as no names, which the handlers then refuse; the contract refuses `null` and an empty
/// list with that message.
fn surface_bodies<'de, D: Deserializer<'de>>(d: D) -> Result<Option<NameList>, D::Error> {
    let v = Value::deserialize(d)?;
    let names = if v.is_null() { NameList::default() } else { NameList::deserialize(v).map_err(D::Error::custom)? };
    if names.0.is_empty() { Err(D::Error::custom(whole(BODIES_MESSAGE))) } else { Ok(Some(names)) }
}

/// The schema of [`surface_bodies`]. Not `NameList`'s own, which allows the empty list.
fn bodies_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "description": "The surface bodies, by name: one, or a list of 1 to 10 000.",
        "anyOf": [
            {"type": "string"},
            {"type": "array", "items": {"type": "string"}, "minItems": 1, "maxItems": MAX_STRING_LIST}
        ]
    })
}

fn bodies_required(schema: &mut Schema) {
    require(schema, "bodies");
}

/// Sew surface bodies into the first one along the edges they share; a solid when they close. One
/// undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = bodies_required)]
pub struct StitchParams {
    // Optional in the type, required in the schema: left out, the handler refuses the call with
    // its own message ("`bodies` must list bodies"), which the contract gives `null` and `[]`.
    #[serde(default, deserialize_with = "surface_bodies")]
    #[schemars(schema_with = "bodies_schema")]
    pub bodies: Option<NameList>,
    /// How far apart two edges may be and still be sewn together. Default: 0, edges that meet.
    #[serde(default)]
    pub tolerance: Option<LengthExpr>,
    #[serde(flatten, deserialize_with = "feature_common")]
    pub common: FeatureCommon,
}

/// Take faces out of a body, each a surface body of its own; the rest stay together under the
/// body's name. A solid comes apart into surfaces. One undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct UnstitchParams {
    /// The body, by name.
    #[serde(deserialize_with = "body_name")]
    pub body: String,
    /// Points on the faces to take out. Default: every face, each its own surface body.
    #[serde(default)]
    pub faces: Option<FacePoints>,
    #[serde(flatten, deserialize_with = "feature_common")]
    pub common: FeatureCommon,
}

/// Turn faces of a surface body over, so their normals point the other way. One undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct ReverseNormalParams {
    /// The surface body, by name.
    #[serde(deserialize_with = "body_name")]
    pub body: String,
    /// Points on the faces to turn over. Default: every face.
    #[serde(default)]
    pub faces: Option<FacePoints>,
    #[serde(flatten, deserialize_with = "feature_common")]
    pub common: FeatureCommon,
}

/// Give surface bodies a thickness, making solids, and combine them with other bodies. One undo
/// step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = bodies_required)]
pub struct ThickenParams {
    // Required in the schema (`bodies_required`), optional in the type, as Stitch's.
    #[serde(default, deserialize_with = "surface_bodies")]
    #[schemars(schema_with = "bodies_schema")]
    pub bodies: Option<NameList>,
    /// How thick, along each surface's normal; negative: against it.
    pub thickness: LengthExpr,
    /// True: half the thickness each side of the surface. Default: false.
    #[serde(default)]
    pub symmetric: Option<bool>,
    /// How the thickened bodies combine with existing ones. Default: new.
    #[serde(default)]
    pub operation: Option<OperationParam>,
    /// The bodies a join, cut or intersect works on, by name. Default: every body in the active
    /// component.
    #[serde(default)]
    pub targets: Option<NameList>,
    /// What a join makes of pieces that share no face. Default: separate.
    #[serde(default)]
    pub touching: Option<TouchingParam>,
    #[serde(flatten, deserialize_with = "feature_common")]
    pub common: FeatureCommon,
}

/// Points on the faces to offset, 1 to 10 000, with the handler's message for anything else.
/// The handler skips an item that isn't a point and refuses the call only when none is left; the
/// contract refuses any such item.
#[derive(Clone, Debug, PartialEq)]
pub struct OffsetFaces(pub Vec<Value>);

/// The handler's message for `faces` it can't use.
const OFFSET_FACES_MESSAGE: &str = "`faces` must list points on the faces to offset";

impl<'de> Deserialize<'de> for OffsetFaces {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match Value::deserialize(d)? {
            Value::Array(points) if (1..=MAX_PICKS).contains(&points.len()) && points.iter().all(|p| params::vec3(p).is_some()) => {
                Ok(OffsetFaces(points))
            }
            _ => Err(D::Error::custom(whole(OFFSET_FACES_MESSAGE))),
        }
    }
}

impl JsonSchema for OffsetFaces {
    fn schema_name() -> Cow<'static, str> {
        "OffsetFaces".into()
    }
    fn inline_schema() -> bool {
        true
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "Points on the faces to offset: 1 to 10 000.",
            "type": "array",
            "items": point_schema(generator),
            "minItems": 1,
            "maxItems": MAX_PICKS
        })
    }
}

fn faces_required(schema: &mut Schema) {
    require(schema, "faces");
}

/// Copy faces at a distance along their normals, as a new surface body (a solid where its faces
/// close). Planes and cylinders move exactly; other faces are fitted. One undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = faces_required)]
pub struct OffsetParams {
    // Optional in the type, required in the schema: left out, the handler refuses the call with
    // its own message, which the contract gives `null` and an empty list (`present`: a `null`
    // goes to `OffsetFaces`, which refuses it).
    #[serde(default, deserialize_with = "present")]
    #[schemars(with = "OffsetFaces")]
    pub faces: Option<OffsetFaces>,
    /// The body the faces are on, by name. Default: the body at the points.
    #[serde(default)]
    pub body: Option<String>,
    /// How far, along the faces' normals; negative: against them; 0 copies the faces.
    pub distance: LengthExpr,
    /// True: each point picks its face's whole chain of smoothly joined faces. Default: false.
    #[serde(default)]
    pub chain: Option<bool>,
    /// How far a fitted face may stray from the true offset. Default: 0.01 mm.
    #[serde(default)]
    pub tolerance: Option<LengthExpr>,
    #[serde(flatten, deserialize_with = "feature_common")]
    pub common: FeatureCommon,
}

/// Untrim's `face`: `{body, point}`, a point on a face of a body, read as the handler reads it,
/// with its messages. Other keys in it are read past.
#[derive(Clone, Debug, PartialEq)]
pub struct UntrimFace(pub Value);

impl<'de> Deserialize<'de> for UntrimFace {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let face = Value::deserialize(d)?;
        if !face.get("body").is_some_and(Value::is_string) {
            return Err(D::Error::custom(whole("`face.body` must name a body")));
        }
        if face.get("point").and_then(params::vec3).is_none() {
            return Err(D::Error::custom(whole("`face.point` must be [x, y, z] on the face")));
        }
        Ok(UntrimFace(face))
    }
}

impl JsonSchema for UntrimFace {
    fn schema_name() -> Cow<'static, str> {
        "UntrimFace".into()
    }
    fn inline_schema() -> bool {
        true
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "The face to untrim: `body`, by name, and `point`, a point on the face.",
            "type": "object",
            "required": ["body", "point"],
            "properties": {"body": {"type": "string"}, "point": point_schema(generator)}
        })
    }
}

/// Untrim's `holes`: `"all"`, or points on the inner loops to remove, read as the handler reads
/// them, with its messages. `null` is left out (`Option`), as the handler reads it: every hole.
#[derive(Clone, Debug, PartialEq)]
pub struct UntrimHoles(pub Value);

impl<'de> Deserialize<'de> for UntrimHoles {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match Value::deserialize(d)? {
            v @ Value::String(_) if v == "all" => Ok(UntrimHoles(v)),
            Value::Array(points) if points.len() > MAX_PICKS => Err(D::Error::custom(whole("too many holes"))),
            Value::Array(points) if points.iter().all(|p| params::vec3(p).is_some()) => Ok(UntrimHoles(Value::Array(points))),
            Value::Array(_) => Err(D::Error::custom(whole("`holes` must list points [x, y, z] on the inner loops"))),
            _ => Err(D::Error::custom(whole("`holes` must be \"all\" or a list of points [x, y, z] on the inner loops"))),
        }
    }
}

impl JsonSchema for UntrimHoles {
    fn schema_name() -> Cow<'static, str> {
        "UntrimHoles".into()
    }
    fn inline_schema() -> bool {
        true
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "The holes to remove: \"all\" (the default), or points on their inner loops (at most 10 000). An empty list removes none, for `outer` alone.",
            "anyOf": [
                {"type": "string", "enum": ["all"]},
                {"type": "array", "items": point_schema(generator), "maxItems": MAX_PICKS},
                {"type": "null"}
            ]
        })
    }
}

fn face_required(schema: &mut Schema) {
    require(schema, "face");
}

/// Remove a face's holes, or restore its outer boundary, as a surface body; the face keeps its
/// name. A solid's face is copied out. One undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = face_required)]
pub struct UntrimParams {
    // Optional in the type, required in the schema: left out, the handler refuses the call with
    // its own message ("`face` is required: {body, point}"). A `null` goes to `UntrimFace`
    // (`present`), which refuses it as the handler does.
    #[serde(default, deserialize_with = "present")]
    #[schemars(with = "UntrimFace")]
    pub face: Option<UntrimFace>,
    #[serde(default)]
    pub holes: Option<UntrimHoles>,
    /// True: restore the outer boundary to the surface's natural bounds (a free-form surface's
    /// own, an analytic surface's span of the face). Default: false.
    #[serde(default)]
    pub outer: Option<bool>,
    /// True: untrim a surface body's face where it is, not as a copy. A solid's face is always
    /// copied out. Default: false.
    #[serde(default)]
    pub in_place: Option<bool>,
    #[serde(flatten, deserialize_with = "feature_common")]
    pub common: FeatureCommon,
}

/// Fair Surface's `strength`: above 0 and at most 1, with the handler's message for a number out
/// of range. The handler ignores one that isn't a number; the contract refuses it.
fn fair_strength<'de, D: Deserializer<'de>>(d: D) -> Result<Option<f64>, D::Error> {
    match Value::deserialize(d)? {
        Value::Null => Ok(None),
        Value::Number(n) => n
            .as_f64()
            .filter(|x| *x > 0.0 && *x <= 1.0)
            .map(Some)
            .ok_or_else(|| D::Error::custom(whole("`strength` must be greater than 0 and at most 1 (the whole fairing step)"))),
        _ => Err(D::Error::custom("must be a number")),
    }
}

fn fair_strength_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "description": "How much of the full fairing step each iteration takes: above 0, at most 1. Default: 1.",
        "type": ["number", "null"],
        "exclusiveMinimum": 0,
        "maximum": 1
    })
}

/// Fair Surface's `iterations`: 1 to 1000 (a fraction is cut to a whole number), with the
/// handler's message for a number out of range. The handler ignores one that isn't a number; the
/// contract refuses it.
fn fair_iterations<'de, D: Deserializer<'de>>(d: D) -> Result<Option<f64>, D::Error> {
    match Value::deserialize(d)? {
        Value::Null => Ok(None),
        Value::Number(n) => {
            n.as_f64().filter(|x| (1.0..=1000.0).contains(x)).map(Some).ok_or_else(|| D::Error::custom(whole("`iterations` must be 1 to 1000")))
        }
        _ => Err(D::Error::custom("must be a number")),
    }
}

fn fair_iterations_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "description": "How many fairing steps: 1 to 1000. Default: 1.",
        "type": ["number", "null"],
        "minimum": 1,
        "maximum": 1000
    })
}

/// Fair a surface body's free-form faces: smooth their control points so curvature changes
/// evenly, staying within `tolerance` of where they are. One undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct FairParams {
    /// The surface body, by name.
    #[serde(deserialize_with = "body_name")]
    pub body: String,
    /// Points on the faces to fair. Default: every face.
    #[serde(default)]
    pub faces: Option<FacePoints>,
    #[serde(default, deserialize_with = "fair_strength")]
    #[schemars(schema_with = "fair_strength_schema")]
    pub strength: Option<f64>,
    #[serde(default, deserialize_with = "fair_iterations")]
    #[schemars(schema_with = "fair_iterations_schema")]
    pub iterations: Option<f64>,
    /// True: hold each face's boundary row of control points and the row next to it, so the face
    /// keeps meeting its neighbours in position and tangent. Default: true.
    #[serde(default)]
    pub keep_edges: Option<bool>,
    /// How far a control point may move. Default: a thousandth of the body's size.
    #[serde(default)]
    pub tolerance: Option<LengthExpr>,
    #[serde(flatten, deserialize_with = "feature_common")]
    pub common: FeatureCommon,
}

#[cfg(test)]
#[path = "surface_tests.rs"]
mod tests;
