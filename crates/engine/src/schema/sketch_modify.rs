//! Contracts and result types for the commands in `cmd/sketch_modify.rs`: `sketch.trim`,
//! `sketch.trim.path`, `sketch.break`, `sketch.extend`, `sketch.fillet`, the three
//! `sketch.chamfer.*`, `sketch.offset`, `sketch.offset.break`, `sketch.mirror`, `sketch.move`,
//! `sketch.scale` and the two `sketch.pattern.*` (#12551).
//!
//! The handlers read their parameters from the raw JSON, so each type here reads its keys by
//! calling the helpers the handlers call (`params::req_vec2`, `params::vec2_list`,
//! `params::string_list`, `alignment_param`…) on the same JSON, and a value they refuse fails
//! with the handler's own message. Whether a curve, a point or a sketch exists stays the
//! handler's to check, and so does whatever needs the design (an expression that has to
//! evaluate, a pattern being edited).
//!
//! A key the handler refuses when it is left out is optional in the type and required in the
//! schema, so that the handler's message, not the check's "is required", is what a caller sees.
//!
//! Several commands read different keys in different cases, and each type reads only the keys
//! its handler reads in the case the call is in, so a call that works today gets no warning:
//!
//! - `sketch.break` takes `points` when the key is there (even `null`, which the handler
//!   refuses) and otherwise `at`;
//! - `sketch.extend` is the two-curve form when `curves` is there, reading `curves` and `at` as
//!   a pair and refusing `curve`, and otherwise reads `curve` and `at` as one point;
//! - `sketch.fillet` and the chamfers take every corner of `points` when it is a list, each as a
//!   point reference or `{a, b}`, and otherwise read `point`, or `a` and `b` when `point` is not
//!   text. A `point` that is not text, a `points` that is not a list, a `pattern` that is not
//!   text and an offset's `direction` that is not text switch the handler into its other form
//!   instead of being refused, so the types leave them alone too;
//! - `sketch.offset` reads `symmetric` unless `direction` is `symmetric`;
//! - the pattern commands may leave out what the pattern they edit (`pattern`) already has.

use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Value, json};
use solidcore_record::Vec2;
use solidcraft_doc::Expr;
use solidcraft_geom::prelude::*;
use solidcraft_sketch::PatternAlignment;

use super::check::whole;
use super::motion::require;
use super::sketch_ends::SketchTarget;
use super::sketch_shapes::Created;
use super::types::{LengthExpr, Point2, StringList};
use crate::params::{self, expr, num, str_};

/// The most copies a pattern makes, as `sketch.pattern.*` and the handlers' limits have it.
pub(crate) const MAX_COPIES: usize = 500;
/// The most offsets `sketch.offset` stacks one behind the other in a single run. Each instance
/// is its own association, so this stays below the sketch's own caps on analytic offset groups
/// and free-form associations (both 128).
pub(crate) const MAX_INSTANCES: usize = 100;
/// The most positions in a trim path.
pub(crate) const MAX_PATH_POINTS: usize = 2_000;
/// The most positions `sketch.break` takes.
pub(crate) const MAX_BREAK_POINTS: usize = 1_000;
/// The most corners one fillet or chamfer takes.
const MAX_CORNERS: usize = 500;
/// The most curves one offset takes.
const MAX_OFFSET_CURVES: usize = 500;

type Read<T> = Result<T, String>;

/// The message of a handler's refusal.
fn message(e: crate::EngineError) -> String {
    match e {
        crate::EngineError::BadParams { msg, .. } => msg,
        other => other.to_string(),
    }
}

/// Parameters read in one go from the raw JSON, as the handler reads them: a refusal is the
/// handler's message, kept whole.
macro_rules! read_params {
    ($($ty:ty),+ $(,)?) => {$(
        impl<'de> Deserialize<'de> for $ty {
            fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                Self::read(&Value::deserialize(d)?).map_err(|m| D::Error::custom(whole(m)))
            }
        }
    )+};
}

/// `key` as text, or `msg`; `None` when it is left out.
fn text(p: &Value, key: &str, msg: &str) -> Read<Option<String>> {
    match p.get(key) {
        None => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.clone())),
        Some(_) => Err(msg.to_string()),
    }
}

/// `key` as a point `[x, y]`, or `msg`; `None` when it is left out.
fn point(p: &Value, key: &str, msg: &str) -> Read<Option<Vec2>> {
    match p.get(key) {
        None => Ok(None),
        Some(v) => params::vec2(v).map(Some).ok_or_else(|| msg.to_string()),
    }
}

/// A flag the handler reads with `bool_`, which takes a wrong type as absent: `null` is absent
/// and any other value that is not true or false is refused here.
fn flag(p: &Value, key: &str) -> Read<Option<bool>> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(b)) => Ok(Some(*b)),
        Some(_) => Err(format!("`{key}` must be true or false")),
    }
}

/// A number the handler reads with `num`, or `msg`; `None` when it is left out.
fn number(p: &Value, key: &str, msg: &str) -> Read<Option<f64>> {
    match p.get(key) {
        None => Ok(None),
        Some(_) => num(p, key).map(Some).ok_or_else(|| msg.to_string()),
    }
}

/// A list of names as `params::string_list` reads it (`null` is an empty list, which the
/// handler refuses with its own message); `None` when the key is left out.
fn names(p: &Value, key: &str) -> Read<Option<Vec<String>>> {
    match p.get(key) {
        None => Ok(None),
        Some(_) => params::string_list("", p, key).map(Some).map_err(message),
    }
}

/// The sketch the call is for, as `sketch_store::target` reads it: an id or a name, and `null`
/// is the one being edited.
fn sketch_of(p: &Value) -> Read<Option<SketchTarget>> {
    match p.get("sketch") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(n)) if n.is_u64() => Ok(Some(SketchTarget(n.to_string()))),
        Some(Value::String(s)) => Ok(Some(SketchTarget(s.clone()))),
        Some(_) => Err("`sketch` must be an id or a name".to_string()),
    }
}

/// A point `[x, y]` or the id of one, as `point_or_ref` reads `key`; `None` when it is left out.
fn point_or_id(p: &Value, key: &str) -> Read<Option<PointOrId>> {
    match p.get(key) {
        None => Ok(None),
        Some(Value::String(id)) => Ok(Some(PointOrId::Id(id.clone()))),
        Some(v) => params::vec2(v).map(PointOrId::At).map(Some).ok_or_else(|| format!("`{key}` must be [x, y] or a point id")),
    }
}

/// A point of the sketch: `[x, y]` (or `{x, y}`) in mm, or the id of an existing point.
#[derive(Clone, Debug, PartialEq)]
pub enum PointOrId {
    /// By its coordinates.
    At(Vec2),
    /// By its id (`p3`, `l1.end`). Whether the sketch has it is the handler's to check.
    Id(String),
}

impl JsonSchema for PointOrId {
    fn schema_name() -> Cow<'static, str> {
        "SketchPointOrId".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let at = generator.subschema_for::<Point2>();
        json_schema!({
            "description": "A point of the sketch: [x, y] or {\"x\", \"y\"} in mm, or the id of an existing point.",
            "anyOf": [at, {"type": "string"}]
        })
    }
}

/// An optional key's schema when `null` is not read as absent and a plain value is.
fn number_or_expression(description: &str) -> Schema {
    json_schema!({
        "description": description,
        "anyOf": [
            {"type": "number", "exclusiveMinimum": -1e9, "exclusiveMaximum": 1e9},
            {"type": "string"}
        ]
    })
}

fn coordinate_number(description: &str) -> Schema {
    json_schema!({"description": description, "type": "number", "exclusiveMinimum": -1e9, "exclusiveMaximum": 1e9})
}

fn require_all(schema: &mut Schema, keys: &[&str]) {
    for key in keys {
        require(schema, key);
    }
}

/// A schema's `anyOf` of the key sets a call may be built from (the first form that fits).
fn one_of_these(schema: &mut Schema, forms: &[&[&str]]) {
    let forms: Vec<Value> = forms.iter().map(|keys| json!({"required": keys})).collect();
    schema.insert("anyOf".into(), Value::Array(forms));
}

// ---------------------------------------------------------------------------------------------
// Corners

/// One corner of a fillet or chamfer: a point where two curves meet, or the two curves.
#[derive(Clone, Debug, PartialEq)]
pub enum Corner {
    /// The corner point's reference (`l1.end`).
    Point(String),
    /// The two curves that meet at the corner, by id.
    Lines { a: String, b: String },
}

impl Corner {
    /// The corner `p` names, as `corner` reads it: `point` when it is text, else `a` and `b`.
    fn read(p: &Value) -> Read<Corner> {
        if let Some(point) = str_(p, "point") {
            return Ok(Corner::Point(point.to_string()));
        }
        let line = |key: &str| str_(p, key).map(str::to_string).ok_or_else(|| format!("`{key}` must be a line"));
        Ok(Corner::Lines { a: line("a")?, b: line("b")? })
    }

    /// An entry of `points`, as `corner_arg` turns it into the parameters of one corner: an
    /// object with `a`, `b` or `point` as it is, anything else as a `point`.
    fn read_entry(entry: &Value) -> Read<Corner> {
        match entry {
            Value::Object(o) if o.contains_key("a") || o.contains_key("b") || o.contains_key("point") => Corner::read(entry),
            _ => Corner::read(&json!({ "point": entry })),
        }
    }
}

impl JsonSchema for Corner {
    fn schema_name() -> Cow<'static, str> {
        "SketchCorner".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "A corner: its point's reference (\"l1.end\"), or {\"point\"}, or {\"a\", \"b\"}, the two curves that meet there.",
            "anyOf": [
                {"type": "string"},
                {"type": "object", "properties": {"point": {"type": "string"}}, "required": ["point"]},
                {"type": "object", "properties": {"a": {"type": "string"}, "b": {"type": "string"}}, "required": ["a", "b"]}
            ]
        })
    }
}

/// The corners a fillet or chamfer works on: `points`, a list of corners, or the one corner
/// `point` or `a` and `b` name. Fillet and the chamfers flatten this into their parameters.
#[derive(Debug, JsonSchema)]
pub struct CornerSelection {
    /// A corner's point reference (`l1.end`): the point two curves share. Used when `points` is
    /// not a list; it takes the place of `a` and `b` when it is text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub point: Option<String>,
    /// One of the two curves that meet at the corner (used when `point` is not text). Two lines
    /// that share no end are first brought to where they meet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub a: Option<String>,
    /// The other curve of the corner (see `a`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub b: Option<String>,
    /// Several corners in one go, 1 to 500, all cut alike: each a point reference or `{a, b}`
    /// (an object with `point`, `a` or `b`). When it is a list, `point`, `a` and `b` are not
    /// used.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub points: Option<Vec<Corner>>,
}

impl CornerSelection {
    fn read(p: &Value) -> Read<CornerSelection> {
        let mut selection = CornerSelection { point: None, a: None, b: None, points: None };
        match p.get("points").and_then(Value::as_array) {
            Some(list) if !list.is_empty() && list.len() <= MAX_CORNERS => {
                selection.points = Some(list.iter().map(Corner::read_entry).collect::<Read<Vec<_>>>()?);
            }
            Some(_) => return Err(format!("`points` must list 1…{MAX_CORNERS} corners")),
            None => match Corner::read(p)? {
                Corner::Point(point) => selection.point = Some(point),
                Corner::Lines { a, b } => (selection.a, selection.b) = (Some(a), Some(b)),
            },
        }
        Ok(selection)
    }
}

/// The forms a corner is given in, for the schemas that flatten [`CornerSelection`].
fn corner_forms(schema: &mut Schema) {
    one_of_these(schema, &[&["point"], &["a", "b"], &["points"]]);
}

// ---------------------------------------------------------------------------------------------
// Trim, break, extend

/// Remove the piece of a curve between the nearest crossings around a point (a curve nothing
/// crosses goes whole). The cut ends are joined to the curve that cut them. Trimming a line that
/// has single-line offsets, or one that is such an offset, releases their linked extents into
/// the dimensions that held them. One undo step.
#[derive(Debug, JsonSchema)]
#[schemars(deny_unknown_fields, transform = trim_required)]
pub struct TrimParams {
    /// The curve to trim, by id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub curve: Option<String>,
    /// A point on the piece to remove, in mm.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Point2")]
    pub at: Option<Vec2>,
    /// The sketch (default: the one being edited).
    #[serde(default)]
    pub sketch: Option<SketchTarget>,
}

fn trim_required(schema: &mut Schema) {
    require_all(schema, &["curve", "at"]);
}

impl TrimParams {
    fn read(p: &Value) -> Read<Self> {
        let at = point(p, "at", "`at` must be a point [x, y]")?;
        let sketch = sketch_of(p)?;
        let curve = text(p, "curve", "`curve` must be a curve id")?;
        Ok(TrimParams { curve, at, sketch })
    }
}

/// Trim every piece a path drawn across the sketch crosses, as `sketch.trim` removes the piece
/// at each crossing, in the order the path crosses them. A piece crossed again after it is gone
/// is not trimmed twice. Projected geometry, infinite lines, offset outputs other than a single
/// line's, and pieces `sketch.trim` refuses are passed over (the reply's `skipped` says why).
/// One undo step; with `dry_run`, nothing changes.
#[derive(Debug, JsonSchema)]
#[schemars(deny_unknown_fields, transform = trim_path_required)]
pub struct TrimPathParams {
    /// The path: 2 to 2000 positions in mm.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Vec<Point2>", length(min = 2, max = 2000))]
    pub points: Option<Vec<Vec2>>,
    /// Report what would be trimmed without changing the design. Default: false.
    #[serde(default)]
    pub dry_run: Option<bool>,
    /// The sketch (default: the one being edited).
    #[serde(default)]
    pub sketch: Option<SketchTarget>,
}

fn trim_path_required(schema: &mut Schema) {
    require(schema, "points");
}

impl TrimPathParams {
    fn read(p: &Value) -> Read<Self> {
        let points = match p.get("points") {
            None => None,
            Some(_) => {
                let points = params::vec2_list("", p, "points", MAX_PATH_POINTS).map_err(message)?;
                if points.len() < 2 {
                    return Err("`points` needs at least two positions".to_string());
                }
                Some(points)
            }
        };
        let dry_run = flag(p, "dry_run")?;
        let sketch = sketch_of(p)?;
        Ok(TrimPathParams { points, dry_run, sketch })
    }
}

/// Split a curve in two or more, at the places other curves cross it (`at`) or at given
/// positions (`points`). At a point where the curve ends and another curve meets, the curve
/// gets its own end there and the two are no longer joined. One undo step.
#[derive(Debug, JsonSchema)]
#[schemars(deny_unknown_fields, transform = break_required)]
pub struct BreakParams {
    /// The curve to break, by id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub curve: Option<String>,
    /// A point near the place to break, in mm: the curve is split at the crossings either side
    /// of it. Used when `points` is not given.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Point2")]
    pub at: Option<Vec2>,
    /// Positions to break at, 1 to 1000, in mm, projected onto the curve; duplicates are
    /// merged. When this key is there, `at` is not used.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Vec<Point2>", length(max = 1000))]
    pub points: Option<Vec<Vec2>>,
    /// The sketch (default: the one being edited).
    #[serde(default)]
    pub sketch: Option<SketchTarget>,
}

fn break_required(schema: &mut Schema) {
    require(schema, "curve");
    one_of_these(schema, &[&["at"], &["points"]]);
}

impl BreakParams {
    fn read(p: &Value) -> Read<Self> {
        let points = match p.get("points") {
            Some(_) => Some(params::vec2_list("", p, "points", MAX_BREAK_POINTS).map_err(message)?),
            None => None,
        };
        let sketch = sketch_of(p)?;
        let curve = text(p, "curve", "`curve` must be a curve id")?;
        let at = if points.is_some() { None } else { point(p, "at", "`at` must be a point [x, y]")? };
        Ok(BreakParams { curve, at, points, sketch })
    }
}

/// Where `sketch.extend` takes its ends from: one point, or one per curve.
#[derive(Clone, Debug, PartialEq)]
pub enum ExtendAt {
    /// One curve: a point near the end to extend.
    End(Vec2),
    /// Two curves: one point per curve, naming the end of it to grow.
    Ends([Vec2; 2]),
}

impl JsonSchema for ExtendAt {
    fn schema_name() -> Cow<'static, str> {
        "ExtendAt".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let at = generator.subschema_for::<Point2>();
        json_schema!({
            "description": "With `curve`: a point near the end to extend, [x, y] in mm. With `curves`: one point per curve, [[x, y], [x, y]].",
            "anyOf": [at, {"type": "array", "items": at, "minItems": 2, "maxItems": 2}]
        })
    }
}

/// Extend a line or arc to the nearest curve in its way (`curve` and `at`), or grow two lines or
/// arcs until they meet and join them there (`curves`). A line with nothing in its way runs on
/// to the reach the preview shows; an arc grows along its circle and closes into a whole circle
/// when nothing stops it. One undo step.
#[derive(Debug, JsonSchema)]
#[schemars(deny_unknown_fields, transform = extend_required)]
pub struct ExtendParams {
    /// The curve to extend, by id. Not with `curves`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub curve: Option<String>,
    /// Two lines or arcs of the sketch to grow until they meet, 2 ids, joined where they meet.
    /// Not with `curve`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Vec<String>", length(min = 2, max = 2))]
    pub curves: Option<Vec<String>>,
    /// With `curve`: a point near the end to extend. With `curves`: one point per curve, the
    /// end to grow (default: the end of each nearest the meeting point).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "ExtendAt")]
    pub at: Option<ExtendAt>,
    /// The sketch (default: the one being edited).
    #[serde(default)]
    pub sketch: Option<SketchTarget>,
}

fn extend_required(schema: &mut Schema) {
    one_of_these(schema, &[&["curve", "at"], &["curves"]]);
    schema.insert("not".into(), json!({"required": ["curve", "curves"]}));
}

impl ExtendParams {
    fn read(p: &Value) -> Read<Self> {
        if let Some(list) = p.get("curves") {
            if p.get("curve").is_some() {
                return Err("give `curve` with `at` (one curve) or `curves` (two curves), not both".to_string());
            }
            let ids =
                list.as_array().filter(|a| a.len() == 2).and_then(|a| a.iter().map(|v| v.as_str().map(str::to_string)).collect::<Option<Vec<_>>>());
            let Some(ids) = ids else {
                return Err("`curves` must be a list of exactly two curve ids".to_string());
            };
            let at = match p.get("at") {
                None => None,
                Some(v) => {
                    let ends = v.as_array().filter(|a| a.len() == 2).and_then(|a| a.iter().map(params::vec2).collect::<Option<Vec<Vec2>>>());
                    match ends.map(<[Vec2; 2]>::try_from) {
                        Some(Ok(two)) => Some(ExtendAt::Ends(two)),
                        _ => return Err("with `curves`, `at` is one point per curve: [[x,y],[x,y]]".to_string()),
                    }
                }
            };
            let sketch = sketch_of(p)?;
            return Ok(ExtendParams { curve: None, curves: Some(ids), at, sketch });
        }
        let at = point(p, "at", "`at` must be a point [x, y]")?.map(ExtendAt::End);
        let sketch = sketch_of(p)?;
        let curve = text(p, "curve", "`curve` must be a curve id")?;
        Ok(ExtendParams { curve, curves: None, at, sketch })
    }
}

// ---------------------------------------------------------------------------------------------
// Fillet and chamfers

/// Round one or several corners of a sketch with an arc of one radius, tangent to both curves.
/// The corner is two lines, or a line and an arc, or two arcs, that meet at a point. Two lines
/// that share no end are first brought to where they meet (the nearer end of a line that stops
/// short moves there, and a line that runs past is cut there, losing its shorter piece). With
/// several corners the first arc carries the radius dimension and the rest are equal to it. A
/// corner that dimensions or constraints measure to stays as a virtual sharp corner on both
/// curves' extensions. An ellipse's axis line is never a side of a corner. One undo step.
#[derive(Debug, JsonSchema)]
#[schemars(deny_unknown_fields, transform = fillet_required)]
pub struct FilletParams {
    /// The radius: a number in mm or a length expression, above 0.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "LengthExpr")]
    pub radius: Option<Expr>,
    #[serde(flatten)]
    pub corners: CornerSelection,
    /// The sketch (default: the one being edited).
    #[serde(default)]
    pub sketch: Option<SketchTarget>,
}

fn fillet_required(schema: &mut Schema) {
    require(schema, "radius");
    corner_forms(schema);
}

impl FilletParams {
    fn read(p: &Value) -> Read<Self> {
        let radius = match p.get("radius") {
            None => None,
            Some(_) => Some(expr(p, "radius").ok_or_else(|| format!("`radius` {}", params::expr_problem(p.get("radius"))))?),
        };
        let corners = CornerSelection::read(p)?;
        let sketch = sketch_of(p)?;
        Ok(FilletParams { radius, corners, sketch })
    }
}

/// Cut one or several corners of two lines with a straight line, the same distance back along
/// both lines. Two lines that share no end are first brought to where they meet, as in
/// `sketch.fillet`. Every corner is cut alike, in one undo step, and a corner that cannot be
/// cut refuses them all. A corner that dimensions or constraints measure to stays as a virtual
/// sharp corner.
#[derive(Debug, JsonSchema)]
#[schemars(deny_unknown_fields, transform = chamfer_equal_required)]
pub struct ChamferEqualParams {
    /// How far back from the corner along each line, in mm (a number; an expression is not
    /// read).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "distance_schema")]
    pub distance: Option<f64>,
    #[serde(flatten)]
    pub corners: CornerSelection,
    /// The sketch (default: the one being edited).
    #[serde(default)]
    pub sketch: Option<SketchTarget>,
}

fn distance_schema(_: &mut SchemaGenerator) -> Schema {
    coordinate_number("A distance in mm, a number.")
}

fn chamfer_equal_required(schema: &mut Schema) {
    require(schema, "distance");
    corner_forms(schema);
}

impl ChamferEqualParams {
    fn read(p: &Value) -> Read<Self> {
        let distance = number(p, "distance", "`distance` must be a number")?;
        let corners = CornerSelection::read(p)?;
        let sketch = sketch_of(p)?;
        Ok(ChamferEqualParams { distance, corners, sketch })
    }
}

/// Cut one or several corners of two lines with a straight line that is `distance` back along
/// `a` and `distance2` back along `b`. A point's `a` is the first of its two lines in curve
/// order. Two lines that share no end are first brought to where they meet, as in
/// `sketch.fillet`. Every corner is cut alike, in one undo step, and a corner that cannot be
/// cut refuses them all.
#[derive(Debug, JsonSchema)]
#[schemars(deny_unknown_fields, transform = chamfer_two_required)]
pub struct ChamferTwoParams {
    /// How far back from the corner along `a` (the first line), in mm (a number).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "distance_schema")]
    pub distance: Option<f64>,
    /// How far back from the corner along `b` (the second line), in mm (a number).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "distance_schema")]
    pub distance2: Option<f64>,
    #[serde(flatten)]
    pub corners: CornerSelection,
    /// The sketch (default: the one being edited).
    #[serde(default)]
    pub sketch: Option<SketchTarget>,
}

fn chamfer_two_required(schema: &mut Schema) {
    require_all(schema, &["distance", "distance2"]);
    corner_forms(schema);
}

impl ChamferTwoParams {
    fn read(p: &Value) -> Read<Self> {
        let distance = number(p, "distance", "`distance` must be a number")?;
        let distance2 = number(p, "distance2", "`distance2` must be a number")?;
        let corners = CornerSelection::read(p)?;
        let sketch = sketch_of(p)?;
        Ok(ChamferTwoParams { distance, distance2, corners, sketch })
    }
}

/// Cut one or several corners of two lines with a straight line that is `distance` back along
/// `a` and meets `b` at `angle` from `a`. A point's `a` is the first of its two lines in curve
/// order. Two lines that share no end are first brought to where they meet, as in
/// `sketch.fillet`. Every corner is cut alike, in one undo step, and a corner that cannot be
/// cut refuses them all.
#[derive(Debug, JsonSchema)]
#[schemars(deny_unknown_fields, transform = chamfer_angle_required)]
pub struct ChamferDistanceAngleParams {
    /// How far back from the corner along `a`, in mm (a number).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "distance_schema")]
    pub distance: Option<f64>,
    /// The angle of the cut from `a`, in degrees (a number). It must leave room to meet the
    /// other line.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "angle_schema")]
    pub angle: Option<f64>,
    #[serde(flatten)]
    pub corners: CornerSelection,
    /// The sketch (default: the one being edited).
    #[serde(default)]
    pub sketch: Option<SketchTarget>,
}

fn angle_schema(_: &mut SchemaGenerator) -> Schema {
    coordinate_number("An angle in degrees, a number.")
}

fn chamfer_angle_required(schema: &mut Schema) {
    require_all(schema, &["distance", "angle"]);
    corner_forms(schema);
}

impl ChamferDistanceAngleParams {
    fn read(p: &Value) -> Read<Self> {
        let distance = number(p, "distance", "`distance` must be a number")?;
        let angle = number(p, "angle", "`angle` must be a number (degrees)")?;
        let corners = CornerSelection::read(p)?;
        let sketch = sketch_of(p)?;
        Ok(ChamferDistanceAngleParams { distance, angle, corners, sketch })
    }
}

// ---------------------------------------------------------------------------------------------
// Offset

/// Offset a chain of sketch curves by a distance, as new curves driven by one dimension (a
/// parameter, `param`). The chain is the connected curves through the ones picked (`chain`).
/// A corner is trimmed back to the crossing when the offset runs past it, and extended until the
/// offset curves intersect when the corner falls short. Each side is its own offset association,
/// listed in the reply's `sides`. One undo step.
#[derive(Debug, JsonSchema)]
#[schemars(deny_unknown_fields, transform = offset_required)]
pub struct OffsetParams {
    /// The curves to offset, 1 to 500 ids (one id is taken as a list of one).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "StringList")]
    pub curves: Option<Vec<String>>,
    /// The distance: a number in mm or a length expression (it creates an editable dimension).
    /// Positive is left of the chain, or toward `side`; negative is the other way.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "LengthExpr")]
    pub distance: Option<Expr>,
    /// A point on the side to offset toward, in mm. Not with `symmetric`.
    #[serde(default)]
    pub side: Option<Point2>,
    /// Offset the whole connected chain, ends that only coincide included. Default: true.
    #[serde(default)]
    pub chain: Option<bool>,
    /// Reverse the primary side. Default: false.
    #[serde(default)]
    pub flip: Option<bool>,
    /// `one_side` (the default) or `symmetric` (the same distance each way; no `side`). Read
    /// without regard to case. Text that is not one of these, and `negative`, `reverse` and
    /// `flip`, which mean nothing here, are refused; a value that is not text is ignored.
    #[serde(default)]
    #[schemars(schema_with = "direction_schema")]
    pub direction: Option<Value>,
    /// The same distance each way, as `direction: symmetric` does; read unless `direction` is
    /// `symmetric`. Default: false.
    #[serde(default)]
    pub symmetric: Option<bool>,
    /// An independent distance the other way, as for `solid.extrude`: a number in mm or a
    /// positive length expression. Not with `symmetric`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "LengthExpr")]
    pub distance2: Option<Expr>,
    /// How many offsets stand one behind the other, 1 to 100 (a whole number or a unitless
    /// expression). Each instance after the first offsets the one before it by the same
    /// distance again. Default: 1.
    #[serde(default)]
    #[schemars(schema_with = "instances_schema")]
    pub instances: Option<Value>,
    /// The sketch (default: the one being edited).
    #[serde(default)]
    pub sketch: Option<SketchTarget>,
}

fn direction_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"description": "one_side or symmetric, in any case.", "type": ["string", "null"]})
}

fn instances_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "description": "A whole number from 1 to 100, or a unitless expression for one.",
        "anyOf": [
            {"type": "number", "minimum": 1, "maximum": MAX_INSTANCES},
            {"type": "string"},
            {"type": "null"}
        ]
    })
}

fn offset_required(schema: &mut Schema) {
    require_all(schema, &["curves", "distance"]);
}

/// `key` as a length, as `offset_length` reads it: a number or a length expression.
fn length(p: &Value, key: &str) -> Read<Option<Expr>> {
    match p.get(key) {
        None => Ok(None),
        Some(_) => expr(p, key).map(Some).ok_or_else(|| format!("`{key}` must be a length expression")),
    }
}

impl OffsetParams {
    fn read(p: &Value) -> Read<Self> {
        let curves = names(p, "curves")?;
        if curves.as_ref().is_some_and(|c| c.is_empty() || c.len() > MAX_OFFSET_CURVES) {
            return Err(format!("`curves` must list 1…{MAX_OFFSET_CURVES} curves"));
        }
        let distance = length(p, "distance")?;
        let flip = flag(p, "flip")?;
        // The handler reads a `side` that is not a point as left out, so this does too.
        let side = p.get("side").and_then(params::vec2).map(Point2);
        let chain = flag(p, "chain")?;
        let direction = p.get("direction").filter(|v| !v.is_null()).cloned();
        let symmetric = match str_(p, "direction").map(str::to_ascii_lowercase).as_deref() {
            None | Some("positive") | Some("one_side") => flag(p, "symmetric")?,
            Some("symmetric") => None,
            Some("negative" | "reverse" | "flip") => {
                return Err("an offset has no negative direction: give a negative `distance`, or a `side` to offset toward".to_string());
            }
            Some(other) => return Err(format!("unknown direction `{other}` (one_side, symmetric)")),
        };
        let distance2 = length(p, "distance2")?;
        let instances = Self::instances(p)?;
        let sketch = sketch_of(p)?;
        Ok(OffsetParams { curves, distance, side, chain, flip, direction, symmetric, distance2, instances, sketch })
    }

    /// `instances`, as `offset_instances` reads it before it looks at the design: a number is a
    /// whole number from 1 to 100, and text is an expression the handler evaluates.
    fn instances(p: &Value) -> Read<Option<Value>> {
        let message = format!("`instances` must be a whole number from 1 to {MAX_INSTANCES}, or an expression for one");
        match p.get("instances") {
            None | Some(Value::Null) => Ok(None),
            Some(Value::Number(_)) => {
                let count = num(p, "instances").ok_or_else(|| message.clone())?;
                if count.fract() != 0.0 || !(1.0..=MAX_INSTANCES as f64).contains(&count) {
                    return Err(message);
                }
                Ok(p.get("instances").cloned())
            }
            Some(v) => expr(p, "instances").map(|_| Some(v.clone())).ok_or(message),
        }
    }
}

/// Break the association of an offset: its curves stay as ordinary geometry, with their points
/// free to edit, and the dimension that drove them goes. Downstream features that used the
/// offset's boundary keep that exact boundary. One undo step.
#[derive(Debug, JsonSchema)]
#[schemars(deny_unknown_fields, transform = offset_break_required)]
pub struct OffsetBreakParams {
    /// A curve the offset made, by id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub curve: Option<String>,
    /// The sketch (default: the one being edited).
    #[serde(default)]
    pub sketch: Option<SketchTarget>,
}

fn offset_break_required(schema: &mut Schema) {
    require(schema, "curve");
}

impl OffsetBreakParams {
    fn read(p: &Value) -> Read<Self> {
        let sketch = sketch_of(p)?;
        let curve = text(p, "curve", "`curve` must name an offset output")?;
        Ok(OffsetBreakParams { curve, sketch })
    }
}

// ---------------------------------------------------------------------------------------------
// Mirror, move, scale

/// The mirror line of `sketch.mirror`: a curve id, `x` or `y`, or an object naming an origin
/// axis.
#[derive(Clone, Debug, PartialEq)]
pub enum MirrorLine {
    /// A straight curve's id, or `x` / `y` for a sketch axis (a curve with that id wins).
    Curve(String),
    /// A reference as `sketch.project` takes one; only `{axis: X|Y|Z}`, an origin axis, works.
    Reference(Value),
}

impl JsonSchema for MirrorLine {
    fn schema_name() -> Cow<'static, str> {
        "MirrorLine".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "A straight curve's id, or x / y for a sketch axis, or {\"axis\": \"X\" | \"Y\" | \"Z\"} for an origin axis (its projection into the sketch plane; an axis perpendicular to the plane is refused).",
            "anyOf": [{"type": "string"}, {"type": "object", "properties": {"axis": {"type": "string"}}}]
        })
    }
}

/// Mirror sketch curves and points across a line, as new copies, and add symmetry constraints
/// between each point and its copy (a point on the line stays one point, held on it). Circles
/// copied are made equal to their originals. One undo step.
#[derive(Debug, JsonSchema)]
#[schemars(deny_unknown_fields, transform = mirror_required)]
pub struct MirrorParams {
    /// The curves and points to mirror, by id (one id is taken as a list of one).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "StringList")]
    pub entities: Option<Vec<String>>,
    /// The mirror line.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "MirrorLine")]
    pub line: Option<MirrorLine>,
    /// The sketch (default: the one being edited).
    #[serde(default)]
    pub sketch: Option<SketchTarget>,
}

fn mirror_required(schema: &mut Schema) {
    require_all(schema, &["entities", "line"]);
}

impl MirrorParams {
    fn read(p: &Value) -> Read<Self> {
        let line = match p.get("line") {
            None => None,
            Some(Value::String(id)) => Some(MirrorLine::Curve(id.clone())),
            Some(v @ Value::Object(_)) => Some(MirrorLine::Reference(v.clone())),
            Some(_) => return Err("`line` must be a curve id, or x / y for a sketch axis".to_string()),
        };
        let sketch = sketch_of(p)?;
        let entities = names(p, "entities")?;
        Ok(MirrorParams { entities, line, sketch })
    }
}

/// Move, turn or copy sketch curves and points: turned by `angle` about `center`, then moved by
/// `translate`. A rotation turns the constraints tied to the sketch axes when all their geometry
/// moves (horizontal and vertical relations and dimensions swap at odd quarter turns and stay
/// at half turns, line angles turn with the line) and removes the horizontal and vertical ones
/// that cannot hold at other angles. Picking a text outline or linked point moves its whole text
/// source once, and copies of text stay editable. One undo step.
#[derive(Debug, JsonSchema)]
#[schemars(deny_unknown_fields, transform = move_required)]
pub struct MoveParams {
    /// The curves and points to move or copy, by id (one id is taken as a list of one). With
    /// `copy`, also drawn 3D curves, which keep their exact arcs and height above the plane.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "StringList")]
    pub entities: Option<Vec<String>>,
    /// The move, `[dx, dy]` in mm. Default: none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Point2")]
    pub translate: Option<Vec2>,
    /// The turn, in degrees (a number). Default: 0.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "angle_schema")]
    pub angle: Option<f64>,
    /// What the turn is about: a point [x, y] in mm or a point's id. Default: the sketch origin.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "PointOrId")]
    pub center: Option<PointOrId>,
    /// Make copies and leave the originals where they are. Default: false.
    #[serde(default)]
    pub copy: Option<bool>,
    /// Turn the constraints tied to the sketch axes with the geometry; false keeps constraints
    /// unchanged. Default: true. Not used with `copy`.
    #[serde(default)]
    pub rotate_constraints: Option<bool>,
    /// The sketch (default: the one being edited).
    #[serde(default)]
    pub sketch: Option<SketchTarget>,
}

fn move_required(schema: &mut Schema) {
    require(schema, "entities");
}

impl MoveParams {
    fn read(p: &Value) -> Read<Self> {
        let translate = point(p, "translate", "`translate` must be a point [x, y]")?;
        let angle = number(p, "angle", "`angle` must be finite")?;
        let copy = flag(p, "copy")?;
        let rotate_constraints = flag(p, "rotate_constraints")?;
        let entities = names(p, "entities")?;
        let sketch = sketch_of(p)?;
        let center = point_or_id(p, "center")?;
        Ok(MoveParams { entities, translate, angle, center, copy, rotate_constraints, sketch })
    }
}

/// Scale sketch curves and points about a base point. Circles and arcs scale their radii; a
/// fixed or projected point cannot move. One undo step.
#[derive(Debug, JsonSchema)]
#[schemars(deny_unknown_fields, transform = scale_required)]
pub struct ScaleParams {
    /// The curves and points to scale, by id (one id is taken as a list of one).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "StringList")]
    pub entities: Option<Vec<String>>,
    /// What stays put: a point [x, y] in mm or a point's id. Default: the sketch origin.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "PointOrId")]
    pub base: Option<PointOrId>,
    /// The factor, a number above 1e-9 and below 1e6.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "factor_schema")]
    pub factor: Option<f64>,
    /// The sketch (default: the one being edited).
    #[serde(default)]
    pub sketch: Option<SketchTarget>,
}

fn factor_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"description": "A scale factor, a number.", "type": "number", "exclusiveMinimum": 1e-9, "exclusiveMaximum": 1e6})
}

fn scale_required(schema: &mut Schema) {
    require_all(schema, &["entities", "factor"]);
}

impl ScaleParams {
    fn read(p: &Value) -> Read<Self> {
        let factor = match p.get("factor") {
            None => None,
            Some(_) => Some(num(p, "factor").filter(|k| *k > 1e-9 && *k < 1e6).ok_or_else(|| "`factor` must be a positive number".to_string())?),
        };
        let sketch = sketch_of(p)?;
        let entities = names(p, "entities")?;
        let base = point_or_id(p, "base")?;
        Ok(ScaleParams { entities, base, factor, sketch })
    }
}

// ---------------------------------------------------------------------------------------------
// Patterns

/// A pattern quantity as the handler reads it before the design is looked at: a number, or text
/// that is an expression for one (evaluated by the handler).
fn quantity(p: &Value, key: &str, not_a_number: &str) -> Read<Option<Value>> {
    match p.get(key) {
        None => Ok(None),
        Some(Value::String(_)) => Ok(p.get(key).cloned()),
        Some(_) => num(p, key).map(|_| p.get(key).cloned()).ok_or_else(|| not_a_number.to_string()),
    }
}

/// A pattern's count (`count` or `count2`), as `count` reads it, but for the design.
fn pattern_count(p: &Value, key: &str) -> Read<Option<Value>> {
    let value = quantity(p, key, &format!("`{key}` must be a number or expression"))?;
    if let Some(Value::Number(n)) = &value {
        let n = n.as_f64().unwrap_or(f64::NAN);
        if !(1.0..=MAX_COPIES as f64).contains(&n) || n.fract() != 0.0 {
            return Err(format!("`{key}` must be a positive integer from 1 to {MAX_COPIES}"));
        }
    }
    Ok(value)
}

fn alignment(p: &Value, key: &str) -> Read<Option<PatternAlignment>> {
    match p.get(key) {
        None => Ok(None),
        Some(_) => crate::cmd::alignment_param(p, key, "").map(Some).map_err(message),
    }
}

fn reversal(p: &Value, key: &str) -> Read<Option<bool>> {
    match p.get(key) {
        None => Ok(None),
        Some(_) => crate::cmd::reversed_param(p, key, "").map(Some).map_err(message),
    }
}

/// `dir` or `dir2`, as the handler reads them: a finite, nonzero vector.
fn direction_vector(p: &Value, key: &str) -> Read<Option<Vec2>> {
    match p.get(key) {
        None => Ok(None),
        // `dir2: null` is left out; `dir: null` is refused, as the handler does.
        Some(Value::Null) if key == "dir2" => Ok(None),
        Some(v) => params::vec2(v).and_then(Vec2::normalized).map(Some).ok_or_else(|| format!("`{key}` must be a finite nonzero vector")),
    }
}

/// `direction` or `direction2`: the id of a sketch line, or left out (`null` too).
fn direction_line(p: &Value, key: &str) -> Read<Option<String>> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(id)) => Ok(Some(id.clone())),
        Some(_) => Err(format!("`{key}` must reference a nondegenerate sketch line")),
    }
}

/// The pattern being edited, as `str_` reads `pattern`: text. Anything else makes a new one.
fn pattern_id(p: &Value) -> Option<String> {
    str_(p, "pattern").map(str::to_string)
}

pub(super) fn alignment_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type": "string", "enum": ["left", "center", "right"]})
}

fn pattern_forms(circular: bool) -> &'static [&'static [&'static str]] {
    if circular { &[&["pattern"], &["entities", "center", "count"]] } else { &[&["pattern"], &["entities", "count", "spacing"]] }
}

/// Make copies of sketch curves and points evenly round a centre, as an associative pattern: the
/// copies follow the originals, and the pattern keeps its own constraints. The quantity includes
/// the original. With `pattern`, edit the pattern that id names instead of making a new one:
/// keys left out keep their values, surviving instance ids are kept, and the pattern's type
/// cannot change. A changed `entities` list rebuilds every copy with fresh ids. One undo step.
#[derive(Debug, JsonSchema)]
#[schemars(deny_unknown_fields, transform = circular_required)]
pub struct CircularPatternParams {
    /// The curves and points to copy, by id (one id is taken as a list of one). Left out only
    /// with `pattern`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "StringList")]
    pub entities: Option<Vec<String>>,
    /// The centre: a point [x, y] in mm (it makes a point you can constrain) or an existing
    /// point's id. Left out only with `pattern`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "PointOrId")]
    pub center: Option<PointOrId>,
    /// How many, the original included: a whole number from 2 to 500 or a unitless expression
    /// (kept as an expression). Left out only with `pattern`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "count_schema")]
    pub count: Option<Value>,
    /// The total angle, in degrees: a number or an expression, nonzero and at most 360 in size.
    /// A full turn spaces the copies angle/count apart; a partial one angle/(count-1). Default:
    /// 360.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "pattern_angle_schema")]
    pub angle: Option<Value>,
    /// Where the original stands among the copies: `left`, `center` or `right`. Center
    /// balances the copies round it. Default: right.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "alignment_schema")]
    pub alignment: Option<PatternAlignment>,
    /// Flip the direction, and the extra side for an even total with `center`. Default: false.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "bool")]
    pub reversed: Option<bool>,
    /// The id of an existing circular pattern to edit instead of making a new one.
    #[serde(default)]
    pub pattern: Option<String>,
    /// The sketch (default: the one being edited).
    #[serde(default)]
    pub sketch: Option<SketchTarget>,
}

fn count_schema(_: &mut SchemaGenerator) -> Schema {
    number_or_expression("A whole number from 1 to 500 (a circular pattern needs at least 2 to make a copy), or an expression for one.")
}

fn pattern_angle_schema(_: &mut SchemaGenerator) -> Schema {
    number_or_expression("An angle in degrees, or an expression for one.")
}

fn circular_required(schema: &mut Schema) {
    one_of_these(schema, pattern_forms(true));
}

impl CircularPatternParams {
    fn read(p: &Value) -> Read<Self> {
        let entities = names(p, "entities")?;
        let count = pattern_count(p, "count")?;
        let angle = match quantity(p, "angle", "`angle` must be finite")? {
            Some(Value::Number(n)) => {
                let total = n.as_f64().unwrap_or(f64::NAN);
                if !(total.abs() > 1e-9 && total.abs() <= 360.0) {
                    return Err("`angle` must be nonzero and at most 360 degrees".to_string());
                }
                Some(Value::Number(n))
            }
            other => other,
        };
        let center = point_or_id(p, "center").map_err(|_| "`center` must be [x,y] or a point ref".to_string())?;
        let alignment = alignment(p, "alignment")?;
        let reversed = reversal(p, "reversed")?;
        let pattern = pattern_id(p);
        let sketch = sketch_of(p)?;
        Ok(CircularPatternParams { entities, center, count, angle, alignment, reversed, pattern, sketch })
    }
}

/// Make copies of sketch curves and points in a grid, as an associative pattern: one row along
/// `direction` (or `dir`) and, with `count2`, more rows along `direction2` (or `dir2`). The
/// copies follow the originals, and the pattern keeps its own constraints. Quantities include the
/// original, at most 500 copies in all; a negative spacing reverses its direction. With
/// `pattern`, edit the pattern that id names instead of making a new one: keys left out keep
/// their values, surviving instance ids are kept, and the pattern's type cannot change. A changed
/// `entities` list rebuilds every copy with fresh ids. One undo step.
#[derive(Debug, JsonSchema)]
#[schemars(deny_unknown_fields, transform = rectangular_required)]
pub struct RectangularPatternParams {
    /// The curves and points to copy, by id (one id is taken as a list of one). Left out only
    /// with `pattern`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "StringList")]
    pub entities: Option<Vec<String>>,
    /// How many along the first direction, the original included: a whole number from 1 to 500
    /// or a unitless expression. Left out only with `pattern`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "count_schema")]
    pub count: Option<Value>,
    /// The distance between copies along the first direction, in mm: a number or a length
    /// expression. Left out only with `pattern`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "spacing_schema")]
    pub spacing: Option<Value>,
    /// How many along the second direction, a whole number from 1 to 500 or a unitless
    /// expression. Default: 1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "count_schema")]
    pub count2: Option<Value>,
    /// The distance between copies along the second direction, in mm. Default: `spacing`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "spacing_schema")]
    pub spacing2: Option<Value>,
    /// The first direction as a vector [x, y] (finite, not zero; it is normalised). Default:
    /// sketch x.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Point2")]
    pub dir: Option<Vec2>,
    /// The second direction as a vector [x, y]. Default: perpendicular to the first.
    #[serde(default)]
    pub dir2: Option<Point2>,
    /// A sketch line the first direction follows (a live reference), by id.
    #[serde(default)]
    pub direction: Option<String>,
    /// A sketch line the second direction follows (a live reference), by id.
    #[serde(default)]
    pub direction2: Option<String>,
    /// Where the original stands along the first direction: `left`, `center` or `right`.
    /// Default: right.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "alignment_schema")]
    pub alignment: Option<PatternAlignment>,
    /// The same along the second direction. Default: right.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "alignment_schema")]
    pub alignment2: Option<PatternAlignment>,
    /// Reverse the placement along the first direction, and the extra side of an even total with
    /// `center`. Default: false.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "bool")]
    pub reversed: Option<bool>,
    /// The same along the second direction. Default: false.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "bool")]
    pub reversed2: Option<bool>,
    /// The id of an existing rectangular pattern to edit instead of making a new one.
    #[serde(default)]
    pub pattern: Option<String>,
    /// The sketch (default: the one being edited).
    #[serde(default)]
    pub sketch: Option<SketchTarget>,
}

fn spacing_schema(_: &mut SchemaGenerator) -> Schema {
    number_or_expression("A distance in mm: a number or a length expression. Negative reverses the direction.")
}

fn rectangular_required(schema: &mut Schema) {
    one_of_these(schema, pattern_forms(false));
}

impl RectangularPatternParams {
    fn read(p: &Value) -> Read<Self> {
        let entities = names(p, "entities")?;
        let count = pattern_count(p, "count")?;
        let spacing = quantity(p, "spacing", "`spacing` must be finite")?;
        let count2 = pattern_count(p, "count2")?;
        let spacing2 = quantity(p, "spacing2", "`spacing2` must be finite")?;
        let dir = direction_vector(p, "dir")?;
        let dir2 = direction_vector(p, "dir2")?.map(Point2);
        let direction = direction_line(p, "direction")?;
        let direction2 = direction_line(p, "direction2")?;
        let alignment_first = alignment(p, "alignment")?;
        let alignment_second = alignment(p, "alignment2")?;
        let reversed = reversal(p, "reversed")?;
        let reversed2 = reversal(p, "reversed2")?;
        let pattern = pattern_id(p);
        let sketch = sketch_of(p)?;
        Ok(RectangularPatternParams {
            entities,
            count,
            spacing,
            count2,
            spacing2,
            dir,
            dir2,
            direction,
            direction2,
            alignment: alignment_first,
            alignment2: alignment_second,
            reversed,
            reversed2,
            pattern,
            sketch,
        })
    }
}

read_params!(
    TrimParams,
    TrimPathParams,
    BreakParams,
    ExtendParams,
    FilletParams,
    ChamferEqualParams,
    ChamferTwoParams,
    ChamferDistanceAngleParams,
    OffsetParams,
    OffsetBreakParams,
    MirrorParams,
    MoveParams,
    ScaleParams,
    CircularPatternParams,
    RectangularPatternParams,
);

// ---------------------------------------------------------------------------------------------
// Replies

/// What `sketch.trim` removed from a curve, and what became of it.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum TrimOutcome {
    /// Nothing crossed the curve: it went whole.
    Deleted {
        /// The curve that went.
        deleted: String,
        /// Single-line offsets whose linked extents the trim released.
        #[serde(skip_serializing_if = "Vec::is_empty")]
        offset_released: Vec<String>,
    },
    /// A free-form curve (a spline) was trimmed: it was replaced by pieces.
    Freeform {
        /// The curve that was trimmed.
        trimmed: String,
        /// The ids of the pieces that are left; `null` when nothing was left to cut at.
        pieces: Option<Vec<String>>,
        /// Single-line offsets whose linked extents the trim released.
        #[serde(skip_serializing_if = "Vec::is_empty")]
        offset_released: Vec<String>,
    },
    /// The piece went; the curve keeps its id and the rest is joined to what cut it.
    Trimmed {
        /// The curve that was trimmed.
        trimmed: String,
        /// Single-line offsets whose linked extents the trim released.
        #[serde(skip_serializing_if = "Vec::is_empty")]
        offset_released: Vec<String>,
    },
}

impl TrimOutcome {
    /// The outcome with the offsets whose linked extents the trim released.
    pub fn released(mut self, ids: Vec<String>) -> Self {
        match &mut self {
            TrimOutcome::Deleted { offset_released, .. }
            | TrimOutcome::Freeform { offset_released, .. }
            | TrimOutcome::Trimmed { offset_released, .. } => {
                *offset_released = ids;
            }
        }
        self
    }
}

/// What `sketch.trim` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct TrimResult {
    /// What was removed.
    pub result: TrimOutcome,
    /// The sketch's solve: `{solved, dof}`.
    pub sketch: Value,
    /// Everything the call added to the sketch: `{points, curves, constraints, dimensions}`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created: Option<Created>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What `sketch.trim.path` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct TrimPathResult {
    /// Each trim's result, in the order the path crossed the pieces.
    pub trimmed: Vec<TrimOutcome>,
    /// Each removed piece as a polyline of [x, y] points, in the same order.
    pub removed: Vec<Vec<[f64; 2]>>,
    /// Why crossings were passed over, each reason once.
    pub skipped: Vec<String>,
    /// The sketch's solve: `{solved, dof}`.
    pub sketch: Value,
    /// `true` for a `dry_run`, which changed nothing; left out otherwise.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dry_run: Option<bool>,
    /// Everything the call added to the sketch: `{points, curves, constraints, dimensions}`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created: Option<Created>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What `sketch.break` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct BreakResult {
    /// The curve's id and its new pieces' ids, in order along it.
    pub curves: Vec<String>,
    /// The sketch's solve: `{solved, dof}`.
    pub sketch: Value,
    /// Everything the call added to the sketch: `{points, curves, constraints, dimensions}`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created: Option<Created>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What `sketch.extend` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ExtendResult {
    /// What moved or was made: the id of the point the line's end moved to or the new curve's
    /// id for one curve; the meeting point's id for two.
    pub moved: String,
    /// The sketch's solve: `{solved, dof}`.
    pub sketch: Value,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What `sketch.fillet` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct FilletResult {
    /// The arcs made, one per corner, in order.
    pub curves: Vec<String>,
    /// The constraints added: the tangencies, and the radius dimension on the first arc with the
    /// others equal to it.
    pub constraints: Vec<String>,
    /// The parameter that holds the radius.
    pub param: String,
    /// The points of corners kept as virtual sharp corners.
    pub virtual_corners: Vec<String>,
    /// The constraints and dimensions that moved to a virtual corner.
    pub moved_to_virtual_corner: Vec<String>,
    /// The offsets whose corners were cut, released into ordinary curves: their curve ids.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub offset_released: Vec<String>,
    /// The sketch's solve: `{solved, dof}`.
    pub sketch: Value,
    /// Everything the call added to the sketch: `{points, curves, constraints, dimensions}`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created: Option<Created>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What the `sketch.chamfer.*` commands return.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ChamferResult {
    /// The lines made, one per corner, in order.
    pub curves: Vec<String>,
    /// The points of corners kept as virtual sharp corners.
    pub virtual_corners: Vec<String>,
    /// The constraints and dimensions that moved to a virtual corner.
    pub moved_to_virtual_corner: Vec<String>,
    /// The offsets whose corners were cut, released into ordinary curves: their curve ids.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub offset_released: Vec<String>,
    /// The sketch's solve: `{solved, dof}`.
    pub sketch: Value,
    /// Everything the call added to the sketch: `{points, curves, constraints, dimensions}`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created: Option<Created>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// One side of an offset.
#[derive(Debug, Serialize, JsonSchema)]
pub struct OffsetSide {
    /// The signed distance of this side, in mm.
    pub distance: f64,
    /// The curves it made.
    pub curves: Vec<String>,
    /// The parameter that drives it; `null` for a trimmed offset, which is not linked to its
    /// source and has no distance to drive.
    pub param: Option<String>,
    /// Which instance it belongs to, counting from 1.
    pub instance: usize,
    /// Its offset association, when the boundary has one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub association: Option<String>,
}

/// What `sketch.offset` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct OffsetResult {
    /// Every curve the offset made, side by side and instance by instance.
    pub curves: Vec<String>,
    /// Every constraint it added.
    pub constraints: Vec<String>,
    /// The sketch's solve: `{solved, dof}`.
    pub sketch: Value,
    /// Each side of each instance.
    pub sides: Vec<OffsetSide>,
    /// The parameter that drives the primary distance; `null` when only trimmed offsets were
    /// made.
    pub param: Option<String>,
    /// How many instances were made.
    pub instances: usize,
    /// The parameter that drives `distance2`, when it was given and drives something.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub param2: Option<String>,
    /// The offset association, when the offset made one side only and has one; with two sides
    /// or several instances each side of `sides` has its own.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub association: Option<String>,
    /// Set when a distance that names a parameter drives nothing, because the offset was trimmed
    /// and so is not linked to its source (#12294).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
    /// Everything the call added to the sketch: `{points, curves, constraints, dimensions}`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created: Option<Created>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What `sketch.offset.break` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct OffsetBreakResult {
    /// The curve that was named.
    pub curve: String,
    /// The sketch's solve: `{solved, dof}`.
    pub sketch: Value,
    /// Every curve that stopped being an offset output, when the association had several.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub curves: Option<Vec<String>>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What `sketch.mirror` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MirrorResult {
    /// The copies of the curves, in order.
    pub curves: Vec<String>,
    /// The constraints added: symmetry between each point and its copy, equal circles, and a
    /// shared point held on the line.
    pub constraints: Vec<String>,
    /// The sketch's solve: `{solved, dof}`.
    pub sketch: Value,
    /// Everything the call added to the sketch: `{points, curves, constraints, dimensions}`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created: Option<Created>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What `sketch.move` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MoveResult {
    /// The curves moved, or with `copy` the copies made, by id.
    pub curves: Vec<String>,
    /// The constraints tying the moved geometry to what stays that the move let go, as
    /// `{id, kind}`.
    pub released: Vec<Value>,
    /// The sketch's solve: `{solved, dof}`.
    pub sketch: Value,
    /// Everything the call added to the sketch: `{points, curves, constraints, dimensions}`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created: Option<Created>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What `sketch.scale` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ScaleResult {
    /// The curves scaled, by id (an ellipse's axes come with it).
    pub curves: Vec<String>,
    /// The sketch's solve: `{solved, dof}`.
    pub sketch: Value,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What the `sketch.pattern.*` commands return.
#[derive(Debug, Serialize, JsonSchema)]
pub struct PatternResult {
    /// The pattern's id: give it as `pattern` to edit it.
    pub pattern: String,
    /// A circular pattern's centre point id; `null` for a rectangular one.
    pub center: Option<String>,
    /// The curves of every instance.
    pub curves: Vec<String>,
    /// The constraints of every instance.
    pub constraints: Vec<String>,
    /// The sketch's solve: `{solved, dof}`.
    pub sketch: Value,
    /// Everything the call added to the sketch: `{points, curves, constraints, dimensions}`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created: Option<Created>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

#[cfg(test)]
#[path = "sketch_modify_tests.rs"]
mod tests;
