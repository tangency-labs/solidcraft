//! Contracts for the surface edit commands in `cmd/surface.rs` (#12627): Patch, Stitch, Unstitch, Reverse
//! Normal, Thicken, Offset Surface, Untrim, Fair Surface, Extend, Trim and Surface Loft. Each adds a feature and returns
//! `add_feature`'s reply, a [`super::features::FeatureResult`] (Patch adds its crown and continuity,
//! a [`PatchResult`]).
//!
//! The handlers read their parameters from the raw JSON, so each key here reads as its handler
//! reads it and fails with the handler's message. Whether a body or a face exists, and whether a
//! value evaluates and fits the surface, stay the handlers' to check. Where a handler ignores a
//! key of the wrong type (a `chain` that isn't true or false, an item of `faces` that isn't a
//! point, which the handlers skip), the contract refuses it.

use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Value, json};
use solidcraft_geom::fill::{MAX_CROWN, MIN_CROWN};
use solidcraft_kernel::Construction;

use super::check::{truncate, whole};
use super::config::NameList;
use super::features::FeatureResult;
use super::motion::require;
use super::surface_boundary::message;
use super::types::{
    FeatureCommon, IdOrName, LengthExpr, OperationParam, PlaneParam, Point3, ProfilesParam, RailFitParam, SketchRef, TouchingParam, feature_common,
    guide_rails_problem, present,
};
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

/// Surface Extend's `edges`: 1 to 10 000 points, all with the handler's message. The handler
/// skips an item that isn't a point; the contract refuses it.
fn extend_edges<'de, D: Deserializer<'de>>(d: D) -> Result<Option<FacePoints>, D::Error> {
    match Value::deserialize(d)? {
        Value::Array(points) if !points.is_empty() && points.len() <= MAX_PICKS && points.iter().all(|p| params::vec3(p).is_some()) => {
            Ok(Some(FacePoints(points)))
        }
        _ => Err(D::Error::custom(whole("`edges` must list points on the edges to extend"))),
    }
}

fn extend_edges_schema(generator: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "description": "Points on the boundary edges to push out: 1 to 10 000.",
        "type": "array",
        "items": point_schema(generator),
        "minItems": 1,
        "maxItems": MAX_PICKS
    })
}

/// Surface Extend's `mode`, with the handler's message for an unknown one. The handler reads a
/// `mode` that isn't text as left out; the contract refuses it.
fn extend_mode<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    match Value::deserialize(d)? {
        Value::Null => Ok(None),
        Value::String(m) if ["natural", "tangent", "curvature"].contains(&m.trim()) => Ok(Some(m)),
        Value::String(m) => Err(D::Error::custom(whole(format!("`mode` must be natural, tangent or curvature, not `{}`", m.trim())))),
        _ => Err(D::Error::custom("must be natural, tangent or curvature")),
    }
}

fn extend_mode_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "description": "How the surface continues: natural, the surface itself continued; tangent, tangent to it across the edge (G1); curvature, keeping its curvature across the edge too (G2). Default: natural.",
        "type": "string",
        "enum": ["natural", "tangent", "curvature"]
    })
}

/// A face picked by a point on it, as `{body, point}`.
fn face_on_body(generator: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "type": "object",
        "properties": {
            "body": {"type": "string", "description": "The body, by name."},
            "point": point_schema(generator)
        },
        "required": ["body", "point"],
        "additionalProperties": false
    })
}

fn up_to_schema(generator: &mut SchemaGenerator) -> Schema {
    let face = face_on_body(generator);
    let plane = generator.subschema_for::<PlaneParam>();
    json_schema!({
        "description": "Extend up to this instead of by `distance`: `bounds`, back to the surface's own bounds (natural mode only); a face, its surface extended; or a plane.",
        "anyOf": [{"const": "bounds"}, face, plane]
    })
}

fn extend_required(schema: &mut Schema) {
    require(schema, "edges");
    schema.insert("anyOf".into(), json!([{"required": ["distance"]}, {"required": ["up_to"]}]));
}

/// Push boundary edges of a surface body out. One undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = extend_required)]
pub struct SurfaceExtendParams {
    /// The surface body, by name.
    #[serde(deserialize_with = "body_name")]
    pub body: String,
    // Optional in the type, required in the schema: left out, the handler refuses the call with
    // its own message.
    #[serde(default, deserialize_with = "extend_edges", skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "extend_edges_schema")]
    pub edges: Option<FacePoints>,
    /// How far the edges move.
    #[serde(default)]
    pub distance: Option<LengthExpr>,
    #[serde(default, deserialize_with = "extend_mode", skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "extend_mode_schema")]
    pub mode: Option<String>,
    // Kept as given: the handler reads its forms, with its messages.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "up_to_schema")]
    pub up_to: Option<Value>,
    /// True: the edges next to an extended edge are lengthened to meet it. False: the extension's
    /// sides run square to the edge. Default: true.
    #[serde(default)]
    pub neighbours: Option<bool>,
    #[serde(flatten, deserialize_with = "feature_common")]
    pub common: FeatureCommon,
}

/// A point or a list of points, as the handler's `keep_points` reads `key`, with its message.
/// `null` reads as left out.
fn keep_points<'de, D: Deserializer<'de>>(d: D, key: &str) -> Result<Option<Value>, D::Error> {
    let v = Value::deserialize(d)?;
    let fits = match &v {
        Value::Null => return Ok(None),
        v if params::vec3(v).is_some() => true,
        Value::Array(a) => !a.is_empty() && a.len() <= MAX_PICKS && a.iter().all(|p| params::vec3(p).is_some()),
        _ => false,
    };
    if fits { Ok(Some(v)) } else { Err(D::Error::custom(whole(format!("`{key}` must be [x, y, z] or a list of them")))) }
}

fn trim_keep<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Value>, D::Error> {
    keep_points(d, "keep")
}

fn trim_keep_with<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Value>, D::Error> {
    keep_points(d, "keep_with")
}

fn keep_points_schema(generator: &mut SchemaGenerator) -> Schema {
    let point = point_schema(generator);
    json_schema!({
        "description": "A point on the side to keep, or points on the regions to keep: the pieces that hang together once cut.",
        "anyOf": [point, {"type": "array", "items": point, "minItems": 1, "maxItems": MAX_PICKS}]
    })
}

/// A mutual trim's other surface, with the handler's message.
fn trim_with<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    match Value::deserialize(d)? {
        Value::Null => Ok(None),
        Value::String(name) => Ok(Some(name)),
        _ => Err(D::Error::custom(whole("`with` must name a surface body"))),
    }
}

fn trim_with_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "description": "Mutual trim: another surface body, one face each; each is cut where the other's surface (extended) meets it. Leave out `plane`, `tool` and `curves`.",
        "type": "string"
    })
}

fn trim_tool_schema(generator: &mut SchemaGenerator) -> Schema {
    let mut face = face_on_body(generator);
    face.insert("description".into(), json!("A face to cut with, its surface extended."));
    face
}

fn trim_curves_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "description": "Sketch curves to cut with.",
        "type": "object",
        "properties": {
            "sketch": {"description": "The sketch: its id or name.", "type": ["integer", "string"]},
            "curves": {"description": "Curve ids, chains meeting end to end. Default: every curve.", "type": "array", "items": {"type": "string"}, "maxItems": 1000},
            "direction": {"description": "`normal`, along the sketch's normal (the default), or a direction [x, y, z] to sweep the curves onto the surface along."}
        },
        "required": ["sketch"],
        "additionalProperties": false
    })
}

fn trim_required(schema: &mut Schema) {
    require(schema, "keep");
    schema.insert("anyOf".into(), json!([{"required": ["plane"]}, {"required": ["tool"]}, {"required": ["curves"]}, {"required": ["with"]}]));
}

/// Cut a surface body with a plane, a face or sketch curves and keep the side or regions asked
/// for, or trim two surfaces against each other. One undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = trim_required)]
pub struct SurfaceTrimParams {
    /// The surface body, by name.
    #[serde(deserialize_with = "body_name")]
    pub body: String,
    // Optional in the type, required in the schema: left out, the handler refuses the call with
    // its own message.
    #[serde(default, deserialize_with = "trim_keep", skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "keep_points_schema")]
    pub keep: Option<Value>,
    /// The plane to cut with.
    #[serde(default)]
    pub plane: Option<PlaneParam>,
    // `tool` and `curves` are kept as given: the handler reads them, with its messages.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "trim_tool_schema")]
    pub tool: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "trim_curves_schema")]
    pub curves: Option<Value>,
    #[serde(default, deserialize_with = "trim_with", skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "trim_with_schema")]
    pub with: Option<String>,
    #[serde(default, deserialize_with = "trim_keep_with", skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "keep_points_schema")]
    pub keep_with: Option<Value>,
    /// Mutual trim: a surface that stops short of the other is extended to meet it.
    #[serde(default)]
    pub extend: Option<bool>,
    /// Mutual trim: the kept pieces are sewn into `body`, and `with` is then gone.
    #[serde(default)]
    pub stitch: Option<bool>,
    #[serde(flatten, deserialize_with = "feature_common")]
    pub common: FeatureCommon,
}

/// `surface_loft`'s name for itself, for the readers the contract shares with it.
const LOFT: &str = "surface.loft";

/// What `surface_loft` says about one section before it looks a sketch up: its message, or `None`.
/// The handler skips an edge point that isn't a point, and ignores a `body` that isn't text and a
/// key a section doesn't take; the contract refuses those too.
fn loft_section_problem(section: &Value, index: usize) -> Option<String> {
    let body = section.get("body");
    let known: &[&str] = if body.is_some_and(Value::is_string) {
        let edges = section.get("edges").and_then(Value::as_array);
        if !edges.is_some_and(|e| (1..=MAX_PICKS).contains(&e.len()) && e.iter().all(|p| params::vec3(p).is_some())) {
            return Some(whole("`sections.edges` must list 1 to 10000 points on the edges of a chain"));
        }
        if section.get("sketch").is_some() {
            return Some(whole("a section is either a sketch chain or a body edge chain"));
        }
        &["body", "edges"]
    } else {
        if crate::cmd::sketch_key(section.get("sketch")).is_none() {
            return Some(whole("`sections.sketch` must be a sketch id or name"));
        }
        match params::string_list(LOFT, section, "curves") {
            Ok(curves) if curves.len() > 1000 => return Some(whole("at most 1000 curves a section")),
            Ok(_) => {}
            Err(e) => return Some(whole(message(e))),
        }
        if let Some(Err(e)) = section.get("profiles").map(|v| ProfilesParam::deserialize(v.clone())) {
            return Some(e.to_string());
        }
        if body.is_some() {
            return Some("`sections.body` must name a body".to_string());
        }
        &["sketch", "curves", "profiles"]
    };
    let key = section.as_object()?.keys().find(|k| !known.contains(&k.as_str()))?;
    let key = truncate(key, 64);
    Some(whole(format!("unknown key `{key}` in `sections[{index}]`: a section takes {}", known.join(", "))))
}

/// A loft's `sections`, read as `surface_loft` reads them before it looks anything up, with its
/// messages. Kept as given, for the handler. Whether the sketches, curves and bodies exist, and
/// whether the sections make a loft, are the handler's to check.
#[derive(Clone, Debug, PartialEq)]
pub struct LoftSections(pub Value);

impl<'de> Deserialize<'de> for LoftSections {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = Value::deserialize(d)?;
        let Some(sections) = v.as_array() else {
            return Err(D::Error::custom(whole("`sections` must list the sections in order")));
        };
        if !(2..=50).contains(&sections.len()) {
            return Err(D::Error::custom(whole("a loft needs 2…50 sections")));
        }
        match sections.iter().enumerate().find_map(|(i, section)| loft_section_problem(section, i)) {
            Some(why) => Err(D::Error::custom(why)),
            None => Ok(LoftSections(v)),
        }
    }
}

impl JsonSchema for LoftSections {
    fn schema_name() -> Cow<'static, str> {
        "LoftSections".into()
    }
    fn inline_schema() -> bool {
        true
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let sketch = generator.subschema_for::<SketchRef>();
        let profiles = generator.subschema_for::<ProfilesParam>();
        let point = point_schema(generator);
        json_schema!({
            "description": "The sections in order, 2 to 50, all open or all closed. Each is one chain: {\"sketch\", \"curves\"?, \"profiles\"?} (a profile without holes, or curves meeting end to end) or {\"body\", \"edges\"} (points on a chain of a body's edges; the face the chain bounds is the surface that end repeats).",
            "type": "array",
            "minItems": 2,
            "maxItems": 50,
            "items": {
                "anyOf": [
                    {
                        "type": "object",
                        "properties": {
                            "sketch": sketch,
                            "curves": {"anyOf": [{"type": "string"}, {"type": "array", "items": {"type": "string"}, "maxItems": 1000}, {"type": "null"}]},
                            "profiles": profiles
                        },
                        "required": ["sketch"],
                        "additionalProperties": false
                    },
                    {
                        "type": "object",
                        "properties": {
                            "body": {"type": "string"},
                            "edges": {"type": "array", "items": point, "minItems": 1, "maxItems": MAX_PICKS}
                        },
                        "required": ["body", "edges"],
                        "additionalProperties": false
                    }
                ]
            }
        })
    }
}

/// A loft's `continuity`: one word for each end, with the handler's messages.
fn loft_continuity<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Value>, D::Error> {
    let v = Value::deserialize(d)?;
    let Some(words) = v.as_array().filter(|a| a.len() == 2) else {
        return Err(D::Error::custom(whole("`continuity` must be two words, one per end")));
    };
    for word in words {
        params::continuity(LOFT, Some(word), "a `continuity` entry", Construction::Bridge).map_err(|e| D::Error::custom(whole(message(e))))?;
    }
    Ok(Some(v))
}

fn loft_continuity_schema(_: &mut SchemaGenerator) -> Schema {
    let word = json!({"type": "string", "pattern": "^\\s*[gG][0-3]\\s*$"});
    json_schema!({
        "description": "How much of the surface at the first and at the last section the loft repeats: g0 (the default), g1 the tangent plane, g2 also the curvature, g3 also how that curvature changes.",
        "type": "array",
        "items": word,
        "minItems": 2,
        "maxItems": 2
    })
}

/// A loft's `crown`: one number for each end, with the handler's messages; `null` is 0.
fn loft_crown<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Value>, D::Error> {
    let v = Value::deserialize(d)?;
    if v.is_null() {
        return Ok(None);
    }
    let Some(ends) = v.as_array().filter(|a| a.len() == 2) else {
        return Err(D::Error::custom(whole("`crown` must be two numbers, one per end")));
    };
    if ends.iter().all(|x| x.as_f64().is_some_and(|x| x.is_finite() && (MIN_CROWN..=MAX_CROWN).contains(&x))) {
        Ok(Some(v))
    } else {
        Err(D::Error::custom(whole("each `crown` entry must be between -0.9 (flattest) and 3 (fullest)")))
    }
}

fn loft_crown_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "description": "How full each end leaves, -0.9 flattest to 3 fullest. Default: 0 at both.",
        "type": ["array", "null"],
        "items": {"type": "number", "minimum": MIN_CROWN, "maximum": MAX_CROWN},
        "minItems": 2,
        "maxItems": 2
    })
}

/// A loft's guide `rails`, as `features::guide_rails` reads them before it looks the sketches up,
/// with its messages: a list of `{sketch, curves}`. Kept as given, for the handler, which also
/// checks how many rails a loft takes.
#[derive(Clone, Debug, PartialEq)]
pub struct LoftRails(pub Value);

impl<'de> Deserialize<'de> for LoftRails {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = Value::deserialize(d)?;
        match guide_rails_problem(&v, None, false) {
            Some(why) => Err(D::Error::custom(why)),
            None => Ok(LoftRails(v)),
        }
    }
}

impl JsonSchema for LoftRails {
    fn schema_name() -> Cow<'static, str> {
        "LoftRails".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let sketch = generator.subschema_for::<SketchRef>();
        json_schema!({
            "description": "Guide rails (the command checks how many a loft takes), each {\"sketch\": the sketch its curves are in, \"curves\": its curve ids in order, 1 to 1000 (one id may be given as text)}. Sketch sections only, both ends g0.",
            "type": "array",
            "minItems": 1,
            "items": {
                "type": "object",
                "properties": {
                    "sketch": sketch,
                    "curves": {"anyOf": [{"type": "string"}, {"type": "array", "items": {"type": "string"}, "minItems": 1, "maxItems": 1000}]}
                },
                "required": ["sketch", "curves"],
                "additionalProperties": false
            }
        })
    }
}

fn sections_required(schema: &mut Schema) {
    require(schema, "sections");
}

/// A surface body through sections in order, ruled between two and smooth through three or more,
/// with no caps. One undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = sections_required)]
pub struct SurfaceLoftParams {
    // Optional in the type, required in the schema: left out, the handler refuses the call with
    // its own message, which the contract gives `null` (`present`: it goes to `LoftSections`).
    #[serde(default, deserialize_with = "present")]
    #[schemars(with = "LoftSections")]
    pub sections: Option<LoftSections>,
    #[serde(default, deserialize_with = "loft_continuity", skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "loft_continuity_schema")]
    pub continuity: Option<Value>,
    #[serde(default, deserialize_with = "loft_crown", skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "loft_crown_schema")]
    pub crown: Option<Value>,
    #[serde(default)]
    pub rails: Option<LoftRails>,
    /// How the rails reshape the sections. Default: scale. Used only with `rails`.
    #[serde(default)]
    pub rail_fit: Option<RailFitParam>,
    #[serde(flatten, deserialize_with = "feature_common")]
    pub common: FeatureCommon,
}

/// What `surface_patch` says about itself, for the readers the contract shares with it.
const PATCH: &str = "surface.patch";

/// The continuity word a patch takes: g0 … g4, what `Construction::Patch` builds.
fn patch_word() -> Value {
    json!({"type": "string", "pattern": "^\\s*[gG][0-4]\\s*$"})
}

/// The words a patch's `continuity` may be, as the handler spells them out when it is neither.
fn patch_continuity_words() -> String {
    let (lowest, highest) = Construction::Patch.orders();
    solidcraft_kernel::Continuity::words(lowest, highest)
}

/// One `{edge, g}` list of a patch's `continuity`, read as `patch_continuity` reads it, with its
/// messages.
fn patch_continuity_edges(list: &Value) -> Result<(), String> {
    let list = list.as_array().ok_or_else(|| whole("`continuity` edges must be a list of {edge: [x,y,z], g}"))?;
    if list.len() > MAX_PICKS {
        return Err(whole("`continuity` lists at most 10000 edges"));
    }
    for e in list {
        if e.get("edge").and_then(params::vec3).is_none() {
            return Err(whole("a `continuity` entry's `edge` must be a point [x,y,z]"));
        }
        params::continuity(PATCH, e.get("g"), "a patch's continuity", Construction::Patch).map_err(|e| whole(message(e)))?;
    }
    Ok(())
}

/// A patch's `continuity`: one word for every edge, a list of `{edge, g}` for some edges (the
/// rest g0), or `{all, edges}`. Read as `patch_continuity` reads it, with its messages; kept as
/// given, for the handler. That a word only applies to a patch on a loop of edges is the
/// handler's to check.
fn patch_continuity<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Value>, D::Error> {
    let v = Value::deserialize(d)?;
    let word = |g: Option<&Value>| {
        params::continuity(PATCH, g, "a patch's continuity", Construction::Patch).map_err(|e| D::Error::custom(whole(message(e))))
    };
    match &v {
        Value::String(_) => {
            word(Some(&v))?;
        }
        Value::Array(_) => patch_continuity_edges(&v).map_err(D::Error::custom)?,
        Value::Object(fields) => {
            if let Some(all) = fields.get("all") {
                word(Some(all))?;
            }
            if let Some(edges) = fields.get("edges") {
                patch_continuity_edges(edges).map_err(D::Error::custom)?;
            }
        }
        _ => {
            return Err(D::Error::custom(whole(format!("`continuity` must be {}, or a list of {{edge, g}}", patch_continuity_words()))));
        }
    }
    Ok(Some(v))
}

fn patch_continuity_schema(generator: &mut SchemaGenerator) -> Schema {
    let word = patch_word();
    let entry = json!({
        "type": "object",
        "properties": {"edge": point_schema(generator), "g": word},
        "required": ["edge", "g"],
        "additionalProperties": false
    });
    json_schema!({
        "description": "How the patch meets the faces beside the loop (a patch on a loop of edges only): one word for every edge — g0 (the default), g1 their tangent planes, g2 also their curvature, g3 also its rate of change, g4 the change of that rate —, a list of {\"edge\": a point on it, \"g\": the word} for those edges (the rest g0), or {\"all\": the word, \"edges\": that list}.",
        "anyOf": [
            word,
            {"type": "array", "items": entry.clone(), "maxItems": MAX_PICKS},
            {"type": "object", "properties": {"all": word, "edges": {"type": "array", "items": entry, "maxItems": MAX_PICKS}}, "additionalProperties": false}
        ]
    })
}

/// A patch's `points`: at most 64 finite points, with the handler's messages. The handler reads
/// a `points` that isn't a list (or `null`) as left out; the contract refuses it.
fn patch_points<'de, D: Deserializer<'de>>(d: D) -> Result<Option<FacePoints>, D::Error> {
    let v = Value::deserialize(d)?;
    let Some(list) = v.as_array().filter(|a| a.len() <= solidcraft_geom::fill_general::MAX_FILL_POINTS) else {
        return Err(D::Error::custom(whole(format!("`points` must list at most {} points [x,y,z]", solidcraft_geom::fill_general::MAX_FILL_POINTS))));
    };
    if list.iter().all(|q| params::vec3(q).is_some_and(|q| q.is_finite())) {
        Ok(Some(FacePoints(list.clone())))
    } else {
        Err(D::Error::custom(whole("a patch point must be a finite [x,y,z]")))
    }
}

fn patch_points_schema(generator: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "description": "Points the patch passes through: at most 64 (a patch on a loop of edges only).",
        "type": "array",
        "items": point_schema(generator),
        "maxItems": solidcraft_geom::fill_general::MAX_FILL_POINTS
    })
}

/// A patch's `crown`: a number between -0.9 and 3, with the handler's message; `null` is refused
/// as the handler refuses it.
fn patch_crown<'de, D: Deserializer<'de>>(d: D) -> Result<Option<f64>, D::Error> {
    match Value::deserialize(d)? {
        v if v.as_f64().is_some_and(|x| (MIN_CROWN..=MAX_CROWN).contains(&x)) => Ok(v.as_f64()),
        _ => Err(D::Error::custom(whole("`crown` must be a number between -0.9 and 3"))),
    }
}

fn patch_crown_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "description": "How much fuller a fitted patch's middle is made, -0.9 flattest to 3 fullest (a patch on a loop of edges only). The bump vanishes on the loop. Default: 0.",
        "type": "number",
        "minimum": MIN_CROWN,
        "maximum": MAX_CROWN
    })
}

/// A patch's `edges`: points on a closed loop of a body's edges, at most 10 000, with the
/// handler's message for too many. The handler skips an item that isn't a point and reads an
/// `edges` that isn't a list as left out; the contract refuses both (`null` reads as left out,
/// as the handler reads it).
fn patch_edges<'de, D: Deserializer<'de>>(d: D) -> Result<Option<FacePoints>, D::Error> {
    match Value::deserialize(d)? {
        Value::Null => Ok(None),
        Value::Array(list) if list.len() > MAX_PICKS => Err(D::Error::custom(whole("too many edges"))),
        Value::Array(list) if list.iter().all(|p| params::vec3(p).is_some()) => Ok(Some(FacePoints(list))),
        _ => Err(D::Error::custom("must list points [x, y, z] on a closed loop of a body's edges")),
    }
}

fn patch_edges_schema(generator: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "description": "Points on a closed loop of a body's edges: one face on them, planar when the loop is, else a smooth B-spline surface. Up to 10 000. With `edges`, `sketch`, `profiles` and `profiles_3d` do nothing.",
        "anyOf": [
            {"type": "array", "items": point_schema(generator), "maxItems": MAX_PICKS},
            {"type": "null"}
        ]
    })
}

/// A patch's `profiles_3d`: 1 to 128 indices of the sketch's planar 3D loops, as listed by
/// `sketch.inspect`, read as `profiles_3d_param` reads it, with its messages.
fn patch_profiles_3d<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Value>, D::Error> {
    let v = Value::deserialize(d)?;
    let Some(list) = v.as_array() else {
        return Err(D::Error::custom(whole("`profiles_3d` must be a list of indices")));
    };
    if list.is_empty() || list.len() > 128 {
        return Err(D::Error::custom(whole("`profiles_3d` needs 1 to 128 indices")));
    }
    for i in list {
        let n = i.as_u64().ok_or_else(|| D::Error::custom(whole("`profiles_3d` indices must be whole numbers")))?;
        usize::try_from(n).map_err(|_| D::Error::custom(whole(format!("there is no profile {n}"))))?;
    }
    Ok(Some(v))
}

fn patch_profiles_3d_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "description": "Indices of the sketch's planar 3D-sketch loops to patch, each in its own plane, as `sketch.inspect` lists them under `profiles_3d`: 1 to 128. Without `profiles`, the sketch's own-plane profiles stay untouched.",
        "type": "array",
        "items": {"type": "integer", "minimum": 0},
        "minItems": 1,
        "maxItems": 128
    })
}

/// A planar face on a sketch's regions or on its planar 3D-sketch loops, or one face on a closed
/// loop of a body's edges. One undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct PatchParams {
    /// The sketch to patch, by id or name. Default: the active sketch, else the last sketch.
    /// Ignored when `edges` is given.
    #[serde(default, deserialize_with = "present")]
    #[schemars(with = "IdOrName")]
    pub sketch: Option<IdOrName>,
    /// Which of the sketch's planar regions to patch. Default: all of them — unless `profiles_3d`
    /// is given without `profiles`, which leaves the sketch's own-plane profiles untouched.
    #[serde(default, deserialize_with = "present")]
    #[schemars(with = "ProfilesParam")]
    pub profiles: Option<ProfilesParam>,
    #[serde(default, deserialize_with = "patch_profiles_3d", skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "patch_profiles_3d_schema")]
    pub profiles_3d: Option<Value>,
    #[serde(default, deserialize_with = "patch_edges", skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "patch_edges_schema")]
    pub edges: Option<FacePoints>,
    /// The body whose edges `edges` picks. Default: the only body, else the last one.
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default, deserialize_with = "patch_continuity", skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "patch_continuity_schema")]
    pub continuity: Option<Value>,
    #[serde(default, deserialize_with = "patch_points", skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "patch_points_schema")]
    pub points: Option<FacePoints>,
    #[serde(default, deserialize_with = "patch_crown", skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "patch_crown_schema")]
    pub crown: Option<f64>,
    #[serde(flatten, deserialize_with = "feature_common")]
    pub common: FeatureCommon,
}

/// What `surface.patch` returns: [`FeatureResult`]'s fields, plus the crown and continuity the
/// patch was built with.
#[derive(Debug, Serialize, JsonSchema)]
pub struct PatchResult {
    #[serde(flatten)]
    pub feature: FeatureResult,
    /// How much fuller a fitted patch's middle was made (the call's `crown`, 0 by default).
    pub crown: f64,
    /// How the patch meets the faces beside its loop: `all` when it is not g0, and the edges
    /// picked with their own `g`; `{}` when every edge is g0.
    #[schemars(schema_with = "patch_continuity_reply_schema")]
    pub continuity: solidcraft_doc::PatchContinuity,
}

fn patch_continuity_reply_schema(generator: &mut SchemaGenerator) -> Schema {
    let entry = json!({
        "type": "object",
        "properties": {"edge": point_schema(generator), "g": patch_word()},
        "required": ["edge", "g"]
    });
    json_schema!({
        "description": "How the patch meets the faces beside its loop: `all` when it is not g0, and the edges picked with their own `g`; `{}` when every edge is g0.",
        "type": "object",
        "properties": {"all": patch_word(), "edges": {"type": "array", "items": entry}}
    })
}

#[cfg(test)]
#[path = "surface_tests.rs"]
mod tests;
