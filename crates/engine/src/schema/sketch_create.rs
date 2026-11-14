//! Contracts and result types for the commands in `cmd/sketch_create.rs`: the construction lines,
//! the spline edits, and (#12550) the tangent arcs and circles, slots, splines, conics, ellipses,
//! blend curves, line types and sketch text.
//!
//! The handlers read their parameters from the raw JSON, so each key here reads as its handler
//! reads it and fails with the handler's message. What only the design can tell (a curve or point
//! that does not exist, an expression that does not evaluate, a font that is not installed) stays
//! the handler's to check. A key the handler refuses when it is left out is optional in the type
//! and required in the schema, so that the handler's message, not the check's "is required", is
//! what a caller sees. A key the handler reads only in some cases (`diameter` when there is no
//! `radius`, `end` when there is no `sweep`, the keys of a text outside `edits`) is read here
//! only in those cases, so that a call that works is not warned about. A call with several
//! problems may get a different one first in strict than with the check off (the check goes key
//! by key, the handlers in their own order).

use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Value, json};
use solidcore_record::Vec2;

use super::check::{located, parse_issue, whole};
use super::motion::require;
use super::sketch_ends::SketchTarget;
use super::sketch_shapes::Created;
use super::types::{AngleExpr, LengthExpr, Point2, Point3, SketchRef, not_blank_pattern, present};
use crate::params;

/// Create a reference line in the active sketch: infinite, or a segment of a given length.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct ConstructionLineParams {
    /// The construction method.
    pub mode: Option<ConstructionMethod>,
    /// Point to pass through; for tangent_at it selects a contact on the curve.
    pub at: Value,
    /// Second point of a two-point construction line.
    pub through: Option<Value>,
    /// Point used to select the closest solution when several exist; for a line with a
    /// `length`, the side of `at` it runs towards.
    pub near: Option<[f64; 2]>,
    /// Zero-based index in the deterministic list of solutions.
    pub solution: Option<usize>,
    /// A reference line for parallel and perpendicular constructions.
    pub curve: Option<String>,
    /// Second reference curve for a common tangent or an angle bisector.
    pub curve2: Option<String>,
    /// Choose the other tangent when two solutions exist.
    pub alternate: Option<bool>,
    /// Direction measured counter-clockwise from sketch X, in degrees.
    pub angle: Option<AngleExpr>,
    /// A segment of this length from `at` instead of an infinite line, held by a length
    /// dimension. For the horizontal, vertical, angle, parallel and perpendicular methods.
    pub length: Option<LengthExpr>,
    /// Positive distance from the reference, on the side of `at`. For a parallel line that is
    /// the picked line; for a horizontal line, the sketch X axis; for a vertical line, the
    /// sketch Y axis. A typed value becomes a driving dimension.
    #[serde(default)]
    pub offset: Option<LengthExpr>,
    /// `false` makes a line with a `length` a normal sketch line, ready for a profile, instead
    /// of construction geometry (default `true`). An infinite line is always construction.
    pub construction: Option<bool>,
    /// The sketch (default: the one being edited).
    pub sketch: Option<SketchRef>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ConstructionMethod {
    Free,
    Horizontal,
    Vertical,
    Angle,
    Parallel,
    Perpendicular,
    TangentExternal,
    TangentAt,
    TangentTwo,
    Bisector,
}

/// The curves and constraints created, together with the sketch solve.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ConstructionLineResult {
    pub curves: Vec<String>,
    pub constraints: Vec<String>,
    pub sketch: Value,
    /// Everything the call added to the sketch: `{points, curves, constraints, dimensions}`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created: Option<Created>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// Reverse a spline's direction: its last point becomes its first, and the curve runs the other
/// way along the same path. Its points, and the constraints on them and on the curve, stay. Takes
/// fit point, control point and closed splines; refused for a projected spline and for one an
/// offset uses or makes.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct SplineReverseParams {
    /// The spline's curve id.
    #[serde(deserialize_with = "spline_id")]
    pub curve: String,
    /// The sketch (default: the one being edited).
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "SketchRef")]
    pub sketch: Option<SketchRef>,
}

/// Take one point out of a spline: it runs through (or over) its other points only, and the point
/// goes with the constraints on it unless another curve uses it. Takes fit point, control point
/// and closed splines; refused for a closed spline's closing point, for an end joined to other
/// geometry or held by a constraint, when fewer than 2 points would be left (3 on a closed
/// spline), and for a projected, offset or patterned spline.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct SplineRemovePointParams {
    /// The spline's curve id.
    #[serde(deserialize_with = "spline_id")]
    pub curve: String,
    /// One of the spline's points: a point id, or the curve's `start` or `end` (`s1.end`).
    #[serde(deserialize_with = "point_ref")]
    pub point: String,
    /// The sketch (default: the one being edited).
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "SketchRef")]
    pub sketch: Option<SketchRef>,
}

/// What `sketch.spline.remove_point` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct SplineRemovePointResult {
    /// The spline's curve id (unchanged).
    pub curve: String,
    /// The id the point had.
    pub point: String,
    /// Whether the point went too; `false` when another curve still uses it.
    pub deleted: bool,
    /// The sketch's solve: `{solved, dof}`.
    pub sketch: Value,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// A point reference, with the handler's message for anything else.
fn point_ref<'de, D: serde::Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    match Value::deserialize(d)? {
        Value::String(s) => Ok(s),
        _ => Err(serde::de::Error::custom("must be one of the spline's points")),
    }
}

/// A curve id, with the handler's message for anything else.
fn spline_id<'de, D: serde::Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    match Value::deserialize(d)? {
        Value::String(s) => Ok(s),
        _ => Err(serde::de::Error::custom("must be a spline id")),
    }
}

/// Give a fit point spline a tangent handle at each fit point, where the curve already runs.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct SplineHandlesParams {
    /// The spline's curve id.
    #[serde(deserialize_with = "spline_id")]
    pub curve: String,
    /// The sketch (default: the one being edited).
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "SketchRef")]
    pub sketch: Option<SketchRef>,
}

/// What `sketch.spline.handles` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct SplineHandlesResult {
    /// The spline's curve id (unchanged).
    pub curve: String,
    /// The handle point ids, in fit point order.
    pub handles: Vec<String>,
    /// The sketch's solve: `{solved, dof}`.
    pub sketch: Value,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// Remove a fit point spline's tangent handles: the curve runs through its fit points as if the
/// handles had never been moved.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct SplineResetHandlesParams {
    /// The spline's curve id.
    #[serde(deserialize_with = "spline_id")]
    pub curve: String,
    /// The sketch (default: the one being edited).
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "SketchRef")]
    pub sketch: Option<SketchRef>,
}

/// What `sketch.spline.reset_handles` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct SplineResetHandlesResult {
    /// The spline's curve id (unchanged).
    pub curve: String,
    /// The removed handle point ids, in fit point order.
    pub removed: Vec<String>,
    /// The sketch's solve: `{solved, dof}`.
    pub sketch: Value,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What `sketch.spline.reverse` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct SplineReverseResult {
    /// The spline's curve id (unchanged).
    pub curve: String,
    /// The sketch's solve: `{solved, dof}`.
    pub sketch: Value,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// Add a point to a fit point spline, where the curve passes nearest to it, between its
/// neighbours; or make it the spline's new first or last point with `end`. The curve still runs
/// through the new point and all its old ones, and the constraints on those stay. Refused for a
/// control point spline, and for an `end` joined to other geometry.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = insert_point_required)]
pub struct SplineInsertPointParams {
    /// The spline's curve id.
    #[serde(deserialize_with = "spline_id")]
    pub curve: String,
    // Optional in the type and required in the schema (`insert_point_required`): the handler's
    // message for a missing `at` is the one it gives for a point it cannot read, so it is the one
    // a caller should hear.
    /// Where the curve is to run through the new point: `[x, y]` or `{x, y}` in mm. Without `end`,
    /// the nearest place along the curve; with it, the new end's place.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "PointXY")]
    pub at: Option<PointXY>,
    /// `start` or `end`: `at` becomes the spline's new first or last point instead, and the curve
    /// runs on to it.
    #[serde(default, deserialize_with = "spline_end", skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "spline_end_schema")]
    pub end: Option<SplineEnd>,
    /// The sketch (default: the one being edited).
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "SketchRef")]
    pub sketch: Option<SketchRef>,
}

/// `at` is required in the schema, though the type leaves it to the handler (#16085).
fn insert_point_required(schema: &mut Schema) {
    require(schema, "at");
}

/// Which end of the spline `at` becomes the new point of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplineEnd {
    /// The new first point.
    Start,
    /// The new last point.
    End,
}

/// `end` as the handler reads it: `null` is as left out, and anything that is not `start` or
/// `end` is refused with its message.
fn spline_end<'de, D: Deserializer<'de>>(d: D) -> Result<Option<SplineEnd>, D::Error> {
    nullable(d, |v| match v.as_str() {
        Some("start") => Ok(SplineEnd::Start),
        Some("end") => Ok(SplineEnd::End),
        _ => refuse("`end` must be start or end"),
    })
}

fn spline_end_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"enum": ["start", "end"]})
}

/// What `sketch.spline.insert_point` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct SplineInsertPointResult {
    /// The new point's id.
    pub point: String,
    /// The spline's curve id (unchanged).
    pub curve: String,
    /// The sketch's solve: `{solved, dof}`.
    pub sketch: Value,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

// ---------------------------------------------------------------------------------------------
// The creation commands of `cmd/sketch_create.rs` (#12550)

/// A value a handler reads with `null` as absent: `read` turns it into `T`, or into the handler's
/// message, which is passed on whole.
fn nullable<'de, D: Deserializer<'de>, T>(d: D, read: impl FnOnce(Value) -> Result<T, String>) -> Result<Option<T>, D::Error> {
    match Value::deserialize(d)? {
        Value::Null => Ok(None),
        v => read(v).map(Some).map_err(|message| D::Error::custom(whole(message))),
    }
}

/// `message` as the `Err` of a reader.
fn refuse<T>(message: impl Into<String>) -> Result<T, String> {
    Err(message.into())
}

/// The error of a handler's reader, as the message it gives.
fn message_of(e: crate::EngineError) -> String {
    match e {
        crate::EngineError::BadParams { msg, .. } => msg,
        other => other.to_string(),
    }
}

/// `params::string_list`'s reading of `v` as the list `key`, with its messages.
fn name_list(v: &Value, key: &str) -> Result<Vec<String>, String> {
    let mut p = serde_json::Map::new();
    p.insert(key.to_string(), v.clone());
    params::string_list("", &Value::Object(p), key).map_err(message_of)
}

/// Adds properties to an object schema.
fn add_properties(schema: &mut Schema, properties: Value) {
    if let (Some(Value::Object(have)), Value::Object(more)) = (schema.get_mut("properties"), properties) {
        have.extend(more);
    }
}

/// A point `[x, y]` or `{x, y}` in mm, as `params::req_vec2` reads it: the handlers say `` `key`
/// must be a point [x, y] `` for a missing one too.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PointXY(pub Vec2);

impl<'de> Deserialize<'de> for PointXY {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        params::vec2(&Value::deserialize(d)?).map(PointXY).ok_or_else(|| D::Error::custom("must be a point [x, y]"))
    }
}

impl JsonSchema for PointXY {
    fn schema_name() -> Cow<'static, str> {
        "PointXY".into()
    }
    fn inline_schema() -> bool {
        true
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        generator.subschema_for::<Point2>()
    }
}

/// The forms of a point argument: [`SketchPoint`] and [`SnapPoint`] differ in the snaps.
fn point_arg_schema(generator: &mut SchemaGenerator, snaps: bool) -> Schema {
    let point = generator.subschema_for::<Point2>();
    let coordinate = generator.subschema_for::<LengthExpr>();
    let mut forms = vec![
        json!(point),
        json!({
            "description": "[x, y] where a coordinate may be an expression over the design's parameters, such as [\"w / 2\", 5]: a point that follows the parameters and is fixed; text naming no parameter, such as \"5\", is just that number, in the design's length unit (a JSON number is mm), and does not itself fix the point.",
            "type": "array", "items": coordinate, "minItems": 2, "maxItems": 2
        }),
        json!({"description": "An existing point's id such as \"l1.end\" or \"p3\", or a snap such as \"mid:l1\".", "type": "string"}),
        json!({"description": "An existing point by id.", "type": "object", "properties": {"ref": {"type": "string"}}, "required": ["ref"]}),
        json!({"description": "A point by place.", "type": "object", "properties": {"at": point}, "required": ["at"]}),
    ];
    let description = if snaps {
        let model = generator.subschema_for::<Point3>();
        forms.push(json!({"description": "A model vertex [x, y, z] in mm, projected into the sketch.", "type": "object", "properties": {"vertex": model}, "required": ["vertex"]}));
        forms.push(json!({"description": "A point on a model edge [x, y, z] in mm, projected into the sketch.", "type": "object", "properties": {"on_edge": model}, "required": ["on_edge"]}));
        "A sketch point: [x, y] in mm (a coordinate may be an expression), an existing point's id (\"l1.end\"), {ref} or {at}, a snap as text (\"mid:<line>\", \"on:<curve>:x,y\", \"x:<a>+<b>:x,y\", \"vertex:x,y,z\"), or a model {vertex} or {on_edge}."
    } else {
        "A sketch point: [x, y] in mm (a coordinate may be an expression over the design's parameters), an existing point's id such as \"l1.end\", or {ref} or {at}."
    };
    json_schema!({"description": description, "anyOf": forms})
}

/// A point argument as `sketch::parg` reads it: a place `[x, y]` (a coordinate may be an
/// expression), an existing point's id, `{ref}` or `{at}`. Kept as given, for the handler, which
/// has the design to find a point and evaluate an expression.
#[derive(Clone, Debug, PartialEq)]
pub struct SketchPoint(pub Value);

impl<'de> Deserialize<'de> for SketchPoint {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = Value::deserialize(d)?;
        crate::cmd::point_arg_form(&v, false).map(|()| SketchPoint(v)).map_err(|why| D::Error::custom(whole(why)))
    }
}

impl JsonSchema for SketchPoint {
    fn schema_name() -> Cow<'static, str> {
        "SketchPoint".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        point_arg_schema(generator, false)
    }
}

/// A point argument of a command that snaps its points (`sketch_project::snap_points`): a
/// [`SketchPoint`], or a model `{vertex}` or `{on_edge}`. Snaps given as text are text.
#[derive(Clone, Debug, PartialEq)]
pub struct SnapPoint(pub Value);

impl<'de> Deserialize<'de> for SnapPoint {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = Value::deserialize(d)?;
        crate::cmd::point_arg_form(&v, true).map(|()| SnapPoint(v)).map_err(|why| D::Error::custom(whole(why)))
    }
}

impl JsonSchema for SnapPoint {
    fn schema_name() -> Cow<'static, str> {
        "SnapPoint".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        point_arg_schema(generator, true)
    }
}

/// A width, as the slot commands read it: a number above 0.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SlotWidth(pub f64);

impl<'de> Deserialize<'de> for SlotWidth {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match Value::deserialize(d)?.as_f64().and_then(params::finite) {
            Some(w) if w > 1e-9 => Ok(SlotWidth(w)),
            _ => Err(D::Error::custom("must be positive")),
        }
    }
}

impl JsonSchema for SlotWidth {
    fn schema_name() -> Cow<'static, str> {
        "SlotWidth".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "The slot's width in mm: more than 0.000000001 and below 1 000 000 000.",
            "type": "number", "exclusiveMinimum": 1e-9, "exclusiveMaximum": 1e9
        })
    }
}

/// The schema of curve ids as `params::string_list` reads them: a list of ids, or one id when
/// one is enough, with the bounds on the list.
fn curve_ids_schema(description: &str, min: usize, max: usize) -> Schema {
    let list = json!({"type": "array", "items": {"type": "string"}, "minItems": min, "maxItems": max});
    let forms = if min <= 1 && max >= 1 { json!([{"type": "string"}, list]) } else { json!([list]) };
    json_schema!({"description": description, "anyOf": forms})
}

/// `N` curves, as the tangent circles read `curves`: an id or a list of exactly `N` ids. A
/// missing, `null` or wrongly long one is refused with the same message.
#[derive(Clone, Debug, PartialEq)]
pub struct CurveIds<const N: usize>(pub Vec<String>);

impl<'de, const N: usize> Deserialize<'de> for CurveIds<N> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let ids = name_list(&Value::deserialize(d)?, "curves").map_err(|m| D::Error::custom(whole(m)))?;
        if ids.len() == N { Ok(CurveIds(ids)) } else { Err(D::Error::custom(whole(format!("`curves` must list {N} curves")))) }
    }
}

impl<const N: usize> JsonSchema for CurveIds<N> {
    fn schema_name() -> Cow<'static, str> {
        format!("CurveIds{N}").into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        curve_ids_schema(&format!("Exactly {N} curves of the sketch (lines, circles or arcs), by id."), N, N)
    }
}

/// Lines, as the centerline and infinite line commands read `curves`: an id or a list of ids, at
/// least one.
#[derive(Clone, Debug, PartialEq)]
pub struct LineIds(pub Vec<String>);

impl<'de> Deserialize<'de> for LineIds {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let ids = name_list(&Value::deserialize(d)?, "curves").map_err(|m| D::Error::custom(whole(m)))?;
        if ids.is_empty() { Err(D::Error::custom(whole("`curves` must list lines"))) } else { Ok(LineIds(ids)) }
    }
}

impl JsonSchema for LineIds {
    fn schema_name() -> Cow<'static, str> {
        "LineIds".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        curve_ids_schema("The lines, by id: one id or a list of ids.", 1, params::MAX_STRING_LIST)
    }
}

/// `blend_curve`'s `continuity`, as `blend_order` reads it: g1 to g4 in either case, blank for g1.
#[derive(Clone, Debug, PartialEq)]
pub struct BlendContinuity(pub String);

impl<'de> Deserialize<'de> for BlendContinuity {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = Value::deserialize(d)?;
        if v.as_str().is_some_and(|s| s.trim().is_empty()) {
            return Ok(BlendContinuity(String::new()));
        }
        params::continuity("", Some(&v), "`continuity`", solidcraft_kernel::Construction::BlendCurve)
            .map(|g| BlendContinuity(g.as_str().to_string()))
            .map_err(|e| D::Error::custom(whole(message_of(e))))
    }
}

impl JsonSchema for BlendContinuity {
    fn schema_name() -> Cow<'static, str> {
        "BlendContinuity".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        let blank = super::types::whitespace_class();
        json_schema!({
            "description": "How the blend meets the two curves: g1 (the default, a tangent spline), g2, g3 or g4 (the curvature, its rate and the 4th derivative match too, as one Bézier of degree 5, 7 or 9). Either case; blank is g1.",
            "type": "string",
            "pattern": format!("^[{blank}]*([gG][1-4][{blank}]*)?$")
        })
    }
}

/// `periodic`, as `spline` reads it: true or false (`null` too is refused). Only a cubic fit
/// point spline (`FIT`) can be periodic; the control point splines refuse `true`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Periodic<const FIT: bool>(pub bool);

impl<'de, const FIT: bool> Deserialize<'de> for Periodic<FIT> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match Value::deserialize(d)? {
            Value::Bool(b) if FIT || !b => Ok(Periodic(b)),
            Value::Bool(_) => Err(D::Error::custom(whole("periodic mode requires a cubic fit spline"))),
            _ => Err(D::Error::custom(whole("periodic must be boolean"))),
        }
    }
}

impl<const FIT: bool> JsonSchema for Periodic<FIT> {
    fn schema_name() -> Cow<'static, str> {
        if FIT { "PeriodicFit" } else { "PeriodicNever" }.into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        if FIT {
            json_schema!({"description": "Close the spline smoothly through its points (needs at least 3 distinct points). Default: false.", "type": "boolean"})
        } else {
            json_schema!({"description": "Not available for a control point spline: only false is accepted. Default: false.", "type": "boolean", "const": false})
        }
    }
}

/// A spline's points, as `point_list` reads them: 2 to 500 point arguments, snaps included.
#[derive(Clone, Debug, PartialEq)]
pub struct SplinePoints(pub Vec<Value>);

impl<'de> Deserialize<'de> for SplinePoints {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let Value::Array(list) = Value::deserialize(d)? else {
            return Err(D::Error::custom(whole("`points` must be a list")));
        };
        if list.len() < 2 || list.len() > solidcraft_sketch::MAX_SPLINE_POINTS {
            return Err(D::Error::custom(whole("a spline needs 2 to 500 points")));
        }
        for v in &list {
            crate::cmd::point_arg_form(v, true).map_err(|why| D::Error::custom(whole(why)))?;
        }
        Ok(SplinePoints(list))
    }
}

impl JsonSchema for SplinePoints {
    fn schema_name() -> Cow<'static, str> {
        "SplinePoints".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let point = generator.subschema_for::<SnapPoint>();
        json_schema!({
            "description": "The spline's points, 2 to 500, each [x, y], a point id or a snap; consecutive points must differ.",
            "type": "array", "items": point, "minItems": 2, "maxItems": solidcraft_sketch::MAX_SPLINE_POINTS
        })
    }
}

/// Make a line through a midpoint: the line's middle is `mid` and one end is `end`, the other is
/// the mirror image, held by a midpoint constraint.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = midpoint_required)]
pub struct MidpointLineParams {
    // Optional in the type and required in the schema (`midpoint_required`): left out, the handler
    // refuses it with its own message.
    /// The line's middle: a point.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "SnapPoint")]
    pub mid: Option<SnapPoint>,
    /// One end of the line, [x, y] in mm.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "PointXY")]
    pub end: Option<PointXY>,
    /// Make it construction geometry. Default: false.
    #[serde(default)]
    pub construction: Option<bool>,
    /// The sketch (default: the one being edited).
    #[serde(default)]
    pub sketch: Option<SketchTarget>,
}

fn midpoint_required(schema: &mut Schema) {
    for key in ["mid", "end"] {
        require(schema, key);
    }
}

/// `sketch.arc.tangent`'s `end`: the id of an existing point, or a place [x, y].
#[derive(Clone, Debug, PartialEq)]
pub enum ArcEnd {
    /// The arc ends on this point.
    Point(String),
    /// A new point there.
    At(Vec2),
}

impl<'de> Deserialize<'de> for ArcEnd {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match Value::deserialize(d)? {
            Value::String(id) => Ok(ArcEnd::Point(id)),
            v => params::vec2(&v).map(ArcEnd::At).ok_or_else(|| D::Error::custom("must be a point [x, y]")),
        }
    }
}

impl JsonSchema for ArcEnd {
    fn schema_name() -> Cow<'static, str> {
        "ArcEnd".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let point = generator.subschema_for::<Point2>();
        json_schema!({
            "description": "Where the arc ends: [x, y] in mm (a new point), or an existing point's id such as \"l2.start\" (the arc ends on that point).",
            "anyOf": [point, {"type": "string"}]
        })
    }
}

/// Draw an arc that leaves the end of a line, arc or spline tangent to it, and ends at `end`,
/// held by a tangent constraint.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = arc_tangent_required)]
pub struct ArcTangentParams {
    /// Where the arc starts: the end point of a drawn line, arc or spline, by id (`l1.end`).
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "SketchPoint")]
    pub start: Option<SketchPoint>,
    /// Where it ends.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "ArcEnd")]
    pub end: Option<ArcEnd>,
    /// The curve to be tangent to. Default: the first drawn curve that ends at `start`.
    #[serde(default)]
    pub curve: Option<String>,
    /// The sketch (default: the one being edited).
    #[serde(default)]
    pub sketch: Option<SketchTarget>,
}

fn arc_tangent_required(schema: &mut Schema) {
    for key in ["start", "end"] {
        require(schema, key);
    }
}

/// A diameter, as `sketch.circle.two_tangent` reads it without a `radius`: a finite number.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Diameter(pub f64);

impl<'de> Deserialize<'de> for Diameter {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Value::deserialize(d)?.as_f64().and_then(params::finite).map(Diameter).ok_or_else(|| D::Error::custom(whole("needs `radius` or `diameter`")))
    }
}

impl JsonSchema for Diameter {
    fn schema_name() -> Cow<'static, str> {
        "Diameter".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({"description": "A diameter in mm.", "type": "number", "exclusiveMinimum": -1e9, "exclusiveMaximum": 1e9})
    }
}

/// Draw a circle tangent to two curves, with a radius or a diameter, near a point; the radius is
/// held by a dimension.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = two_tangent_required)]
pub struct TwoTangentCircleKeys {
    /// The radius: a number in mm or a length expression. Wins over `diameter`.
    #[serde(default)]
    pub radius: Option<LengthExpr>,
    /// The diameter in mm, a number; used when there is no `radius`.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Diameter")]
    pub diameter: Option<Diameter>,
    /// Which of the solutions: the circle whose centre is nearest this point, [x, y] in mm.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "PointXY")]
    pub near: Option<PointXY>,
    /// The two curves, by id: lines, circles or arcs of the sketch.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "CurveIds<2>")]
    pub curves: Option<CurveIds<2>>,
    /// The sketch (default: the one being edited).
    #[serde(default)]
    pub sketch: Option<SketchTarget>,
}

fn two_tangent_required(schema: &mut Schema) {
    for key in ["near", "curves"] {
        require(schema, key);
    }
    schema.insert("anyOf".into(), json!([{"required": ["radius"]}, {"required": ["diameter"]}]));
}

/// `sketch.circle.two_tangent`'s parameters: [`TwoTangentCircleKeys`], with `diameter` left unread
/// when there is a `radius`, and a `radius` that isn't one left unread otherwise, as the handler
/// does.
#[derive(Debug)]
pub struct TwoTangentCircleParams(pub TwoTangentCircleKeys);

impl<'de> Deserialize<'de> for TwoTangentCircleParams {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let mut value = Value::deserialize(d)?;
        let radius = params::expr(&value, "radius").is_some();
        if let Some(keys) = value.as_object_mut() {
            keys.remove(if radius { "diameter" } else { "radius" });
        }
        serde_path_to_error::deserialize(&value).map(TwoTangentCircleParams).map_err(|e| D::Error::custom(located(&e)))
    }
}

impl JsonSchema for TwoTangentCircleParams {
    fn schema_name() -> Cow<'static, str> {
        "TwoTangentCircleParams".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let mut schema = TwoTangentCircleKeys::json_schema(generator);
        schema.insert("title".into(), "TwoTangentCircleParams".into());
        schema
    }
}

/// Draw a circle tangent to three curves, near a point, held by three tangent constraints.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = three_tangent_required)]
pub struct ThreeTangentCircleParams {
    /// Which of the solutions: the circle whose centre is nearest this point, [x, y] in mm.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "PointXY")]
    pub near: Option<PointXY>,
    /// The three curves, by id: lines, circles or arcs of the sketch.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "CurveIds<3>")]
    pub curves: Option<CurveIds<3>>,
    /// The sketch (default: the one being edited).
    #[serde(default)]
    pub sketch: Option<SketchTarget>,
}

fn three_tangent_required(schema: &mut Schema) {
    for key in ["near", "curves"] {
        require(schema, key);
    }
}

/// Draw a slot from its centre and the centre of one end arc; the other end is the mirror
/// image. Four tangent constraints and an equal one hold it.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = slot_center_required)]
pub struct SlotCenterPointParams {
    /// The slot's centre, [x, y] in mm.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "PointXY")]
    pub center: Option<PointXY>,
    /// The centre of one end arc, [x, y] in mm; the other end is its mirror image through `center`.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "PointXY")]
    pub end: Option<PointXY>,
    /// The slot's width, in mm.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "SlotWidth")]
    pub width: Option<SlotWidth>,
    /// Join the new points to the existing points they land on. Default: true.
    #[serde(default)]
    pub connect: Option<bool>,
    /// The sketch (default: the one being edited).
    #[serde(default)]
    pub sketch: Option<SketchTarget>,
}

fn slot_center_required(schema: &mut Schema) {
    for key in ["center", "end", "width"] {
        require(schema, key);
    }
}

/// Draw a slot along an arc through three points on its centre arc. Returns `centre_arc`: the
/// construction arc along the slot's middle.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = arc_slot_three_required)]
pub struct ArcSlotThreePointParams {
    /// The centre arc's start, [x, y] in mm.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "PointXY")]
    pub start: Option<PointXY>,
    /// A point the centre arc passes through, [x, y] in mm.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "PointXY")]
    pub through: Option<PointXY>,
    /// The centre arc's end, [x, y] in mm.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "PointXY")]
    pub end: Option<PointXY>,
    /// The slot's width, in mm.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "SlotWidth")]
    pub width: Option<SlotWidth>,
    /// The sketch (default: the one being edited).
    #[serde(default)]
    pub sketch: Option<SketchTarget>,
}

fn arc_slot_three_required(schema: &mut Schema) {
    for key in ["start", "through", "end", "width"] {
        require(schema, key);
    }
}

/// The sweep of an arc slot, as `sketch.slot.arc_center` reads it: a number of degrees (`null`
/// is refused).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sweep(pub f64);

impl<'de> Deserialize<'de> for Sweep {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Value::deserialize(d)?.as_f64().and_then(params::finite).map(Sweep).ok_or_else(|| D::Error::custom(whole("`sweep` must be finite degrees")))
    }
}

impl JsonSchema for Sweep {
    fn schema_name() -> Cow<'static, str> {
        "Sweep".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({"description": "The sweep in degrees, counter-clockwise from `start`; the sketch refuses one that is not above 0 and below 360.", "type": "number", "exclusiveMinimum": -1e9, "exclusiveMaximum": 1e9})
    }
}

/// Draw a slot along an arc from its centre, the start of its centre arc and its end or sweep.
/// Returns `centre_arc`: the construction arc along the slot's middle.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = arc_slot_center_required)]
pub struct ArcSlotCenterKeys {
    /// The centre of the arcs, [x, y] in mm.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "PointXY")]
    pub center: Option<PointXY>,
    /// The centre arc's start, [x, y] in mm; its distance from `center` is the arc's radius.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "PointXY")]
    pub start: Option<PointXY>,
    /// The slot's width, in mm.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "SlotWidth")]
    pub width: Option<SlotWidth>,
    /// How far the centre arc turns counter-clockwise, in degrees (0 to 360, not including
    /// either). Wins over `end`, which is then not read.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Sweep")]
    pub sweep: Option<Sweep>,
    /// Without `sweep`: a point in the direction of the centre arc's end from `center` (the arc
    /// runs counter-clockwise from `start` to it), [x, y] in mm.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "PointXY")]
    pub end: Option<PointXY>,
    /// The sketch (default: the one being edited).
    #[serde(default)]
    pub sketch: Option<SketchTarget>,
}

fn arc_slot_center_required(schema: &mut Schema) {
    for key in ["center", "start", "width"] {
        require(schema, key);
    }
    schema.insert("anyOf".into(), json!([{"required": ["sweep"]}, {"required": ["end"]}]));
}

/// `sketch.slot.arc_center`'s parameters: [`ArcSlotCenterKeys`], with `end` left unread when
/// `sweep` is there (`null` included), as the handler does.
#[derive(Debug)]
pub struct ArcSlotCenterParams(pub ArcSlotCenterKeys);

impl<'de> Deserialize<'de> for ArcSlotCenterParams {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let mut value = Value::deserialize(d)?;
        if let Some(keys) = value.as_object_mut().filter(|o| o.contains_key("sweep")) {
            keys.remove("end");
        }
        serde_path_to_error::deserialize(&value).map(ArcSlotCenterParams).map_err(|e| D::Error::custom(located(&e)))
    }
}

impl JsonSchema for ArcSlotCenterParams {
    fn schema_name() -> Cow<'static, str> {
        "ArcSlotCenterParams".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let mut schema = ArcSlotCenterKeys::json_schema(generator);
        schema.insert("title".into(), "ArcSlotCenterParams".into());
        schema
    }
}

/// The minor radius of an ellipse, as `sketch.ellipse` reads it: a number of mm above 0.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MinorRadius(pub f64);

impl<'de> Deserialize<'de> for MinorRadius {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match Value::deserialize(d)?.as_f64().and_then(params::finite) {
            Some(r) if r > 1e-9 && r < 1e8 => Ok(MinorRadius(r)),
            _ => Err(D::Error::custom(whole("the minor radius must be positive"))),
        }
    }
}

impl JsonSchema for MinorRadius {
    fn schema_name() -> Cow<'static, str> {
        "MinorRadius".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({"description": "The minor radius in mm: above 0.000000001 and below 100 000 000.", "type": "number", "exclusiveMinimum": 1e-9, "exclusiveMaximum": 1e8})
    }
}

/// The point an ellipse passes through on its minor side, as `sketch.ellipse` reads `minor`
/// without a `minor_radius`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MinorPoint(pub Vec2);

impl<'de> Deserialize<'de> for MinorPoint {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        params::vec2(&Value::deserialize(d)?).map(MinorPoint).ok_or_else(|| D::Error::custom(whole("needs `minor` [x,y] or `minor_radius`")))
    }
}

impl JsonSchema for MinorPoint {
    fn schema_name() -> Cow<'static, str> {
        "MinorPoint".into()
    }
    fn inline_schema() -> bool {
        true
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        generator.subschema_for::<Point2>()
    }
}

/// `axes`, as `sketch.ellipse` reads it: true or false (`null` is the default).
fn axes_flag<'de, D: Deserializer<'de>>(d: D) -> Result<Option<bool>, D::Error> {
    nullable(d, |v| match v {
        Value::Bool(b) => Ok(b),
        _ => refuse("`axes` must be true or false"),
    })
}

/// Draw an ellipse from its centre, the end of its major axis and its minor radius or a point on
/// its minor side. By default it also gets its major and minor axes as construction lines, held
/// by midpoint, perpendicular and point-on-curve constraints.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = ellipse_required)]
pub struct EllipseKeys {
    /// The ellipse's centre, [x, y] in mm.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "PointXY")]
    pub center: Option<PointXY>,
    /// The end of the major axis, [x, y] in mm; not the centre.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "PointXY")]
    pub major: Option<PointXY>,
    /// The minor radius in mm. Wins over `minor`, which is then not read.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "MinorRadius")]
    pub minor_radius: Option<MinorRadius>,
    /// Without `minor_radius`: a point [x, y] in mm the ellipse passes through on its minor side;
    /// it must lie within the span of the major axis.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "MinorPoint")]
    pub minor: Option<MinorPoint>,
    /// Also add the major and minor axes as construction lines. Default: true.
    #[serde(default, deserialize_with = "axes_flag", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "bool")]
    pub axes: Option<bool>,
    /// Make it construction geometry. Default: false.
    #[serde(default)]
    pub construction: Option<bool>,
    /// The sketch (default: the one being edited).
    #[serde(default)]
    pub sketch: Option<SketchTarget>,
}

fn ellipse_required(schema: &mut Schema) {
    for key in ["center", "major"] {
        require(schema, key);
    }
    schema.insert("anyOf".into(), json!([{"required": ["minor"]}, {"required": ["minor_radius"]}]));
}

/// `sketch.ellipse`'s parameters: [`EllipseKeys`], with `minor` left unread when there is a
/// `minor_radius`, and a `minor_radius` that isn't a number left unread, as the handler does.
#[derive(Debug)]
pub struct EllipseParams(pub EllipseKeys);

impl<'de> Deserialize<'de> for EllipseParams {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let mut value = Value::deserialize(d)?;
        let radius = params::num(&value, "minor_radius").is_some();
        if let Some(keys) = value.as_object_mut() {
            keys.remove(if radius { "minor" } else { "minor_radius" });
        }
        serde_path_to_error::deserialize(&value).map(EllipseParams).map_err(|e| D::Error::custom(located(&e)))
    }
}

impl JsonSchema for EllipseParams {
    fn schema_name() -> Cow<'static, str> {
        "EllipseParams".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let mut schema = EllipseKeys::json_schema(generator);
        schema.insert("title".into(), "EllipseParams".into());
        schema
    }
}

/// The conic's `rho`, as `sketch.conic` reads it: a number above 0 and below 1 (one of 1e9 or
/// more in size is ignored, as if left out).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rho(pub f64);

impl<'de> Deserialize<'de> for Rho {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match Value::deserialize(d)? {
            Value::Number(n) => match n.as_f64().and_then(params::finite) {
                Some(r) if r > 1e-6 && r < 1.0 - 1e-6 => Ok(Rho(r)),
                Some(_) => Err(D::Error::custom(whole("`rho` must be between 0 and 1"))),
                None => Ok(Rho(0.5)),
            },
            _ => Err(D::Error::custom("must be a number between 0 and 1")),
        }
    }
}

impl JsonSchema for Rho {
    fn schema_name() -> Cow<'static, str> {
        "Rho".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "How full the curve is: above 0 and below 1. 0.5 is a parabola, less an ellipse, more a hyperbola. A number of 1e9 or more in size is ignored. Default: 0.5.",
            "type": "number", "exclusiveMinimum": 1e-6, "exclusiveMaximum": 1.0 - 1e-6
        })
    }
}

/// Draw a conic curve from its two ends and the apex where the tangents there meet.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = conic_required)]
pub struct ConicParams {
    /// The conic's start: a point.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "SnapPoint")]
    pub start: Option<SnapPoint>,
    /// The conic's end: a point, not the start.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "SnapPoint")]
    pub end: Option<SnapPoint>,
    /// The apex, where the tangents at both ends meet: a point, not the start or end.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "SnapPoint")]
    pub apex: Option<SnapPoint>,
    /// How full the curve is. Default: 0.5 (a parabola).
    #[serde(default)]
    pub rho: Option<Rho>,
    /// Make it construction geometry. Default: false.
    #[serde(default)]
    pub construction: Option<bool>,
    /// The sketch (default: the one being edited).
    #[serde(default)]
    pub sketch: Option<SketchTarget>,
}

fn conic_required(schema: &mut Schema) {
    for key in ["start", "end", "apex"] {
        require(schema, key);
    }
}

/// Draw a fit point spline: a cubic spline through points, optionally closed (periodic).
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = points_required)]
pub struct FitPointSplineParams {
    /// The points the spline passes through.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "SplinePoints")]
    pub points: Option<SplinePoints>,
    /// Close it smoothly through its points. Default: false.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Periodic<true>")]
    pub periodic: Option<Periodic<true>>,
    /// Make it construction geometry. Default: false.
    #[serde(default)]
    pub construction: Option<bool>,
    /// The sketch (default: the one being edited).
    #[serde(default)]
    pub sketch: Option<SketchTarget>,
}

/// Draw a control point spline: a cubic one (`sketch.spline.control_point`) or one of degree 5
/// (`sketch.spline.control_point_5`), shaped by its control points.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = points_required)]
pub struct ControlPointSplineParams {
    /// The control points.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "SplinePoints")]
    pub points: Option<SplinePoints>,
    /// Refused as true: only a cubic fit point spline can be periodic. Default: false.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Periodic<false>")]
    pub periodic: Option<Periodic<false>>,
    /// Make it construction geometry. Default: false.
    #[serde(default)]
    pub construction: Option<bool>,
    /// The sketch (default: the one being edited).
    #[serde(default)]
    pub sketch: Option<SketchTarget>,
}

fn points_required(schema: &mut Schema) {
    require(schema, "points");
}

/// Join the end points of two curves with a blend curve: a tangent spline (g1), or one Bézier that
/// also matches the curves' curvature and more (g2 to g4).
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = blend_required)]
pub struct BlendCurveParams {
    /// The end point of the first curve, by id (`l1.end`).
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "SketchPoint")]
    pub a: Option<SketchPoint>,
    /// The end point of the second curve, by id (`a2.start`); not the same place as `a`.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "SketchPoint")]
    pub b: Option<SketchPoint>,
    /// g1 (the default), g2, g3 or g4.
    #[serde(default)]
    pub continuity: Option<BlendContinuity>,
    /// The sketch (default: the one being edited).
    #[serde(default)]
    pub sketch: Option<SketchTarget>,
}

fn blend_required(schema: &mut Schema) {
    for key in ["a", "b"] {
        require(schema, key);
    }
}

/// Make lines centerlines (an axis: not in profiles), or ordinary lines again.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = curves_required)]
pub struct CenterlineParams {
    /// The lines, by id.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "LineIds")]
    pub curves: Option<LineIds>,
    /// True makes them centerlines, false ordinary lines. Default: each line switches.
    #[serde(default)]
    pub value: Option<bool>,
    /// The sketch (default: the one being edited).
    #[serde(default)]
    pub sketch: Option<SketchTarget>,
}

/// Make lines infinite lines (construction geometry drawn across the whole view through the
/// line's two points), or ordinary lines again.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = curves_required)]
pub struct InfiniteLineParams {
    /// The lines, by id.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "LineIds")]
    pub curves: Option<LineIds>,
    /// True makes them infinite, false ordinary lines. Default: each line switches.
    #[serde(default)]
    pub value: Option<bool>,
    /// The sketch (default: the one being edited).
    #[serde(default)]
    pub sketch: Option<SketchTarget>,
}

fn curves_required(schema: &mut Schema) {
    require(schema, "curves");
}

/// What the creation commands return: what they made and the sketch's solve.
#[derive(Debug, Serialize, JsonSchema)]
pub struct DrawResult {
    /// The ids of the curves made.
    pub curves: Vec<String>,
    /// The ids of the constraints made.
    pub constraints: Vec<String>,
    /// The sketch's solve: `{solved, dof}`.
    pub sketch: Value,
    /// What it added: `{points, curves, constraints, dimensions}`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created: Option<Value>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What `sketch.slot.arc_three_point` and `sketch.slot.arc_center` return.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ArcSlotResult {
    #[serde(flatten)]
    pub drawn: DrawResult,
    /// The id of the construction arc along the slot's middle.
    pub centre_arc: Option<String>,
    /// The point held halfway along the construction centre arc.
    pub middle: Option<String>,
}

/// What `sketch.centerline` and `sketch.infinite` return.
#[derive(Debug, Serialize, JsonSchema)]
pub struct LineTypeResult {
    /// How many lines were changed.
    pub changed: usize,
    /// The sketch's solve: `{solved, dof}`.
    pub sketch: Value,
    /// What it added: `{points, curves, constraints, dimensions}`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created: Option<Value>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

// ---------------------------------------------------------------------------------------------
// Sketch text

/// The words of a text, as `text_source` reads `text`: a string that isn't blank, of at most
/// `MAX_TEXT_CHARS` characters (`null` is as left out).
fn text_words<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    nullable(d, |v| match v {
        Value::String(t) if t.trim().is_empty() => refuse("`text` must not be empty"),
        Value::String(t) if t.chars().count() > solidcraft_sketch::MAX_TEXT_CHARS => refuse("text too long"),
        Value::String(t) => Ok(t),
        _ => refuse("`text` must be a string"),
    })
}

fn text_words_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type": "string", "pattern": not_blank_pattern(), "maxLength": solidcraft_sketch::MAX_TEXT_CHARS})
}

fn text_expression<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    nullable(d, |v| match v {
        Value::String(e) => Ok(e),
        _ => refuse("`text_expression` must be a string"),
    })
}

fn string_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type": "string"})
}

/// A number of a text, as `text_number` reads it: a finite number, or text that isn't blank (an
/// expression), at most 4096 bytes.
fn text_number_form(v: &Value) -> bool {
    match v {
        Value::Number(n) => n.as_f64().and_then(params::finite).is_some(),
        Value::String(e) => !e.trim().is_empty() && e.len() <= 4096,
        _ => false,
    }
}

/// A text's `at`: `[x, y]` with each a number or an expression, or `{x, y}` of numbers.
fn text_at<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Value>, D::Error> {
    nullable(d, |v| {
        let ok = match &v {
            Value::Array(a) => matches!(a.as_slice(), [x, y] if text_number_form(x) && text_number_form(y)),
            other => params::vec2(other).is_some(),
        };
        if ok { Ok(v) } else { refuse(crate::cmd::AT_MUST) }
    })
}

fn text_at_schema(generator: &mut SchemaGenerator) -> Schema {
    let coordinate = generator.subschema_for::<LengthExpr>();
    let point = generator.subschema_for::<Point2>();
    json_schema!({
        "anyOf": [{"type": "array", "items": coordinate, "minItems": 2, "maxItems": 2}, point]
    })
}

fn expression_string() -> Value {
    json!({"type": "string", "minLength": 1, "maxLength": 4096, "pattern": not_blank_pattern()})
}

/// A text's `height`: a number of mm above 0 and below 1e5, or an expression.
fn text_height<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Value>, D::Error> {
    nullable(d, |v| {
        if !text_number_form(&v) {
            return refuse("`height` must be a length in mm, or an expression");
        }
        match v.as_f64() {
            Some(h) if !(h > 1e-6 && h < 1e5) => refuse("`height` must be positive"),
            _ => Ok(v),
        }
    })
}

fn text_height_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"anyOf": [{"type": "number", "exclusiveMinimum": 1e-6, "exclusiveMaximum": 1e5}, expression_string()]})
}

/// A text's `angle`: a number of degrees, or an expression.
fn text_angle<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Value>, D::Error> {
    nullable(d, |v| if text_number_form(&v) { Ok(v) } else { refuse("`angle` must be an angle in degrees, or an expression") })
}

fn text_angle_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"anyOf": [{"type": "number", "exclusiveMinimum": -1e9, "exclusiveMaximum": 1e9}, expression_string()]})
}

fn text_font<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    nullable(d, |v| match v {
        Value::String(f) => Ok(f),
        _ => refuse("`font` must be a font family name or a font file path"),
    })
}

macro_rules! text_flag {
    ($name:ident, $key:literal) => {
        fn $name<'de, D: Deserializer<'de>>(d: D) -> Result<Option<bool>, D::Error> {
            nullable(d, |v| match v {
                Value::Bool(b) => Ok(b),
                _ => refuse(concat!("`", $key, "` must be true or false")),
            })
        }
    };
}

text_flag!(text_bold, "bold");
text_flag!(text_italic, "italic");
text_flag!(text_reverse, "reverse");

fn text_align<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    nullable(d, |v| match v {
        Value::String(a) if matches!(a.trim().to_ascii_lowercase().as_str(), "left" | "center" | "centre" | "right") => Ok(a),
        _ => refuse("`align` must be left, center or right"),
    })
}

/// A pattern for one of the words, whatever its case, with blanks around it.
fn word_pattern(words: &[&str]) -> String {
    let blank = super::types::whitespace_class();
    let case = |w: &str| w.chars().map(|c| format!("[{}{}]", c.to_ascii_uppercase(), c)).collect::<String>();
    format!("^[{blank}]*({})[{blank}]*$", words.iter().map(|w| case(w)).collect::<Vec<_>>().join("|"))
}

fn text_align_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type": "string", "pattern": word_pattern(&["left", "center", "centre", "right"])})
}

fn text_letter_spacing<'de, D: Deserializer<'de>>(d: D) -> Result<Option<f64>, D::Error> {
    nullable(d, |v| v.as_f64().and_then(params::finite).map_or_else(|| refuse("`letter_spacing` must be a length in mm"), Ok))
}

fn text_letter_spacing_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type": "number", "exclusiveMinimum": -1e9, "exclusiveMaximum": 1e9})
}

fn text_line_spacing<'de, D: Deserializer<'de>>(d: D) -> Result<Option<f64>, D::Error> {
    nullable(d, |v| v.as_f64().and_then(params::finite).filter(|s| *s > 0.0).map_or_else(|| refuse("`line_spacing` must be a positive factor"), Ok))
}

fn text_line_spacing_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type": "number", "exclusiveMinimum": 0, "exclusiveMaximum": 1e9})
}

fn text_path<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    nullable(d, |v| match v {
        Value::String(c) => Ok(c),
        _ => refuse("`path` must be a curve id"),
    })
}

/// What a text is, as `sketch.text` and `sketch.edit_text` read it (`null` is as left out for
/// each).
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct TextKeys {
    /// The words, at most 4096 characters, `\n` breaking lines; not blank. Not with
    /// `text_expression`. When editing, kept when left out.
    #[serde(default, deserialize_with = "text_words", skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "text_words_schema")]
    pub text: Option<String>,
    /// The words built from text parameters (`parameters.add` with unit text), number parameters
    /// and quoted texts joined by `+` (`"Label + '-L'"`, `"'W' + Width"`; `number(Width, 2)` is
    /// a number with 2 decimals). The text keeps it and follows the parameters; the reply's
    /// `exprs` returns it as `text`. Not with `text`. When editing, typed words replace it.
    #[serde(default, deserialize_with = "text_expression", skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "string_schema")]
    pub text_expression: Option<String>,
    /// The baseline start, [x, y] in mm, or {x, y}; in `[x, y]` each is a number or an expression
    /// over the design's parameters as a string (`"plate_width / 2"`), which the text keeps and
    /// follows. Needed to make a text; when editing, kept when left out (and moving a text
    /// replaces its expressions with numbers). Does not place a text on a `path`.
    #[serde(default, deserialize_with = "text_at", skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "text_at_schema")]
    pub at: Option<Value>,
    /// The capital height in mm (above 0, below 100 000), a number or an expression over the
    /// design's parameters. Default: 5. When editing, kept when left out.
    #[serde(default, deserialize_with = "text_height", skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "text_height_schema")]
    pub height: Option<Value>,
    /// The direction of the baseline, counter-clockwise from sketch X: a number of degrees or an
    /// expression. Default: 0. When editing, kept when left out. Does not turn a text on a `path`.
    #[serde(default, deserialize_with = "text_angle", skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "text_angle_schema")]
    pub angle: Option<Value>,
    /// An installed font's family or full name, or a .ttf or .otf file path (`~` and `%VAR%`
    /// expand); empty: the built-in Liberation Sans. Default: the built-in font. When editing,
    /// kept when left out.
    #[serde(default, deserialize_with = "text_font", skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "string_schema")]
    pub font: Option<String>,
    /// An installed family's bold face. Default: false. When editing, kept when left out.
    #[serde(default, deserialize_with = "text_bold", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "bool")]
    pub bold: Option<bool>,
    /// An installed family's italic face (without one, the text is slanted). Default: false. When
    /// editing, kept when left out.
    #[serde(default, deserialize_with = "text_italic", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "bool")]
    pub italic: Option<bool>,
    /// Run the text the other way along its `path`. Default: false. When editing, kept when left
    /// out.
    #[serde(default, deserialize_with = "text_reverse", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "bool")]
    pub reverse: Option<bool>,
    /// The lines against `at`, or along the path: left, center (or centre) or right, either case.
    /// Default: left. When editing, kept when left out.
    #[serde(default, deserialize_with = "text_align", skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "text_align_schema")]
    pub align: Option<String>,
    /// Extra space between letters, in mm. Default: 0. When editing, kept when left out.
    #[serde(default, deserialize_with = "text_letter_spacing", skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "text_letter_spacing_schema")]
    pub letter_spacing: Option<f64>,
    /// The line distance as a multiple of the font's (above 0). Default: 1. When editing, kept
    /// when left out.
    #[serde(default, deserialize_with = "text_line_spacing", skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "text_line_spacing_schema")]
    pub line_spacing: Option<f64>,
    /// A curve id of the sketch the baseline follows; empty: none. `at` and `angle` then do not
    /// place the text, and the curve becomes construction, a guide the letters stand on. When
    /// editing, kept when left out.
    #[serde(default, deserialize_with = "text_path", skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "string_schema")]
    pub path: Option<String>,
}

/// The text keys of `value`, with the handler's messages: `text` and `text_expression` together
/// are refused first, as `text_source` does before it reads either.
fn read_text_keys<E: serde::de::Error>(value: &Value) -> Result<TextKeys, E> {
    let given = |k: &str| value.get(k).is_some_and(|v| !v.is_null());
    if given("text") && given("text_expression") {
        return Err(E::custom(whole("give `text` or `text_expression`, not both")));
    }
    serde_path_to_error::deserialize(value).map_err(|e| E::custom(located(&e)))
}

/// Make text in the sketch being edited: outlines that stay linked to their words and style, so a
/// later `sketch.edit_text` can change them. Give `text` or `text_expression`, and `at`.
#[derive(Debug)]
pub struct SketchTextParams(pub TextKeys);

impl<'de> Deserialize<'de> for SketchTextParams {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let value = Value::deserialize(d)?;
        // First, as the handler does.
        if value.get("at").is_none_or(Value::is_null) {
            return Err(D::Error::custom(whole(crate::cmd::AT_MUST)));
        }
        read_text_keys(&value).map(SketchTextParams)
    }
}

impl JsonSchema for SketchTextParams {
    fn schema_name() -> Cow<'static, str> {
        "SketchTextParams".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let mut schema = TextKeys::json_schema(generator);
        schema.insert("title".into(), "SketchTextParams".into());
        schema.insert(
            "description".into(),
            "Make text in the sketch being edited: outlines that stay linked to their words and style, so a later sketch.edit_text can change them. Give `text` or `text_expression`, and `at`."
                .into(),
        );
        add_properties(&mut schema, sketch_property());
        require(&mut schema, "at");
        schema.insert("anyOf".into(), json!([{"required": ["text"]}, {"required": ["text_expression"]}]));
        schema
    }
}

/// One text of `sketch.edit_text`: its link or one of its curves (`entity`), and the keys that
/// change.
#[derive(Debug)]
pub struct TextEdit {
    /// The text's link id.
    pub link: Option<String>,
    /// One of its curves, used when there is no `link`.
    pub entity: Option<String>,
    /// What changes.
    pub keys: TextKeys,
}

impl<'de> Deserialize<'de> for TextEdit {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let value = Value::deserialize(d)?;
        // `str_` reads a key that isn't text as left out.
        let name = |k: &str| value.get(k).and_then(Value::as_str).map(str::to_string);
        let (link, entity) = (name("link"), name("entity"));
        if link.is_none() && entity.is_none() {
            return Err(D::Error::custom(whole("give `link` or `entity`")));
        }
        Ok(TextEdit { link, entity, keys: read_text_keys(&value)? })
    }
}

/// The texts go into the sketch being edited whatever `sketch` says; the session reads it only to
/// report what the call `created`, so a call that names it still runs without a warning.
fn sketch_property() -> Value {
    json!({"sketch": {"description": "Only for the reply's `created`, which compares this sketch (default: the one being edited); the text always goes into the sketch being edited.", "type": ["integer", "string"]}})
}

fn edit_target_properties() -> Value {
    json!({
        "link": {"description": "The text's link id (the `link` the text's reply names).", "type": "string"},
        "entity": {"description": "One of the text's curves, by id, when there is no `link`.", "type": "string"}
    })
}

impl JsonSchema for TextEdit {
    fn schema_name() -> Cow<'static, str> {
        "TextEdit".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let mut schema = TextKeys::json_schema(generator);
        schema.insert("title".into(), "TextEdit".into());
        schema.insert("description".into(), "One text's change: the text by `link` or `entity`, and the keys to change; the rest is kept.".into());
        add_properties(&mut schema, edit_target_properties());
        schema.insert("anyOf".into(), json!([{"required": ["link"]}, {"required": ["entity"]}]));
        schema
    }
}

/// Change texts: one by `link` (or `entity`, one of its curves) with the keys to change, or
/// several at once by `edits`, the design rebuilt once, all or nothing. What is left out stays; a
/// number replaces an expression, and so does moving the text. With `edits`, the other keys of the
/// call (but `live`) are not read.
#[derive(Debug)]
pub struct EditTextParams {
    /// The several texts, when the call gives `edits`.
    pub edits: Option<Vec<TextEdit>>,
    /// The one text, when it does not.
    pub single: Option<TextEdit>,
    /// One frame of an interactive change.
    pub live: Option<bool>,
}

impl<'de> Deserialize<'de> for EditTextParams {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let value = Value::deserialize(d)?;
        // `live` takes effect only as `true`; anything else but a flag is a mistake.
        let live = match value.get("live") {
            None | Some(Value::Null) => None,
            Some(Value::Bool(b)) => Some(*b),
            Some(_) => return Err(D::Error::custom(whole("`live` must be true or false"))),
        };
        match value.get("edits") {
            None | Some(Value::Null) => {
                let single = TextEdit::deserialize(&value).map_err(|e| D::Error::custom(e.to_string()))?;
                Ok(EditTextParams { edits: None, single: Some(single), live })
            }
            Some(Value::Array(edits)) => {
                if edits.is_empty() || edits.len() > crate::cmd::MAX_TEXT_EDITS {
                    return Err(D::Error::custom(whole(format!("`edits` must hold 1 to {} edits", crate::cmd::MAX_TEXT_EDITS))));
                }
                let edits = edits
                    .iter()
                    .map(|e| serde_path_to_error::deserialize::<_, TextEdit>(e).map_err(|e| D::Error::custom(whole(parse_issue(&e).message))))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(EditTextParams { edits: Some(edits), single: None, live })
            }
            Some(_) => Err(D::Error::custom(whole("`edits` must be a list of edits"))),
        }
    }
}

impl JsonSchema for EditTextParams {
    fn schema_name() -> Cow<'static, str> {
        "EditTextParams".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let item = generator.subschema_for::<TextEdit>();
        let mut schema = TextKeys::json_schema(generator);
        schema.insert("title".into(), "EditTextParams".into());
        schema.insert(
            "description".into(),
            "Change texts: one by `link` (or `entity`, one of its curves) with the keys to change, or several at once by `edits`, the design rebuilt once, all or nothing. What is left out stays; a number replaces an expression, and so does moving the text. With `edits`, the other keys of the call (but `live`) are not read."
                .into(),
        );
        add_properties(&mut schema, edit_target_properties());
        add_properties(&mut schema, sketch_property());
        add_properties(
            &mut schema,
            json!({
                "edits": {"description": "Several texts changed together: each is `link` or `entity` with the keys to change.", "type": "array", "items": item, "minItems": 1, "maxItems": crate::cmd::MAX_TEXT_EDITS},
                "live": {"description": "One frame of an interactive change: the sketch is solved and shown, the features after it stay as they were until `sketch.drag_end`. A call with `live: true` is not checked against this schema yet.", "type": "boolean"}
            }),
        );
        schema.insert("anyOf".into(), json!([{"required": ["link"]}, {"required": ["entity"]}, {"required": ["edits"]}]));
        schema
    }
}

/// The expressions behind a text's numbers and words.
#[derive(Debug, Default, Serialize, JsonSchema)]
pub struct TextExpressions {
    /// `at`'s x.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub x: Option<String>,
    /// `at`'s y.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub y: Option<String>,
    /// The height.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height: Option<String>,
    /// The angle.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub angle: Option<String>,
    /// The words (`text_expression`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

/// One text made or changed.
#[derive(Debug, Serialize, JsonSchema)]
pub struct TextChange {
    /// The text's link id.
    pub link: String,
    /// The ids of its curves.
    pub curves: Vec<String>,
    /// The curve the text follows that became construction, a guide the letters stand on; null
    /// when none did.
    pub guide: Option<String>,
    /// The expressions the text keeps; absent when it keeps none.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exprs: Option<TextExpressions>,
}

/// What `sketch.text` returns, and `sketch.edit_text` for one text.
#[derive(Debug, Serialize, JsonSchema)]
pub struct TextResult {
    #[serde(flatten)]
    pub change: TextChange,
    /// What it added: `{points, curves, constraints, dimensions}`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created: Option<Value>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What `sketch.edit_text` returns for `edits`.
#[derive(Debug, Serialize, JsonSchema)]
pub struct TextEditsResult {
    /// Each edit's result, in order.
    pub edits: Vec<TextChange>,
    /// What it added: `{points, curves, constraints, dimensions}`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created: Option<Value>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What `sketch.edit_text` returns: one text's result, or each edit's.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum EditTextResult {
    One(TextResult),
    Many(TextEditsResult),
}

#[cfg(test)]
#[path = "sketch_create_tests.rs"]
mod tests;
