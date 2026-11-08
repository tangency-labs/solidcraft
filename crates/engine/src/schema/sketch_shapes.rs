//! Contracts and result types for the drawing commands in `cmd/sketch.rs`: a line, a point, the
//! circles and the arcs (#12953), the rectangles and the polygons (#13196).

use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Value, json};
use solidcore_record::Vec2;

use super::check::whole;
use super::sketch_ends::SketchTarget;
use super::types::{LengthExpr, Point2, Point3, present};
use crate::params;

/// What the coordinates of a point typed as text must be.
const POINT_MUST: &str = "a point's coordinates must be numbers or expressions over the design's parameters";
/// The most points a line takes.
const MAX_LINE_POINTS: usize = 10_000;

/// The forms of a point argument: [`SketchPoint`] and [`SnapPoint`] differ in the snaps.
fn point_arg_schema(generator: &mut SchemaGenerator, snaps: bool) -> Schema {
    let point = generator.subschema_for::<Point2>();
    let coordinate = generator.subschema_for::<LengthExpr>();
    let mut forms = vec![
        json!(point),
        json!({
            "description": "[x, y] where a coordinate may be an expression over the design's parameters, such as [\"w / 2\", 5]: a point that follows the parameters.",
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

/// A point as `params::req_vec2` reads it: `[x, y]` or `{x, y}` of numbers, with no expression, id
/// or snap (the 2-point circle's `p0` and `p1`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlainPoint(pub Vec2);

impl<'de> Deserialize<'de> for PlainPoint {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        params::vec2(&Value::deserialize(d)?).map(PlainPoint).ok_or_else(|| D::Error::custom("must be a point [x, y]"))
    }
}

impl JsonSchema for PlainPoint {
    fn schema_name() -> Cow<'static, str> {
        "PlainPoint".into()
    }
    fn inline_schema() -> bool {
        true
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        generator.subschema_for::<Point2>()
    }
}

/// The points of a line: a list of [`SnapPoint`]s, 2 to 10 000.
#[derive(Clone, Debug, PartialEq)]
pub struct LinePoints(pub Vec<Value>);

impl<'de> Deserialize<'de> for LinePoints {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let Value::Array(list) = Value::deserialize(d)? else {
            return Err(D::Error::custom(whole("`points` must be a list")));
        };
        if list.len() < 2 || list.len() > MAX_LINE_POINTS {
            return Err(D::Error::custom(whole("a line needs 2 or more points")));
        }
        for v in &list {
            crate::cmd::point_arg_form(v, true).map_err(|why| D::Error::custom(whole(why)))?;
        }
        Ok(LinePoints(list))
    }
}

impl JsonSchema for LinePoints {
    fn schema_name() -> Cow<'static, str> {
        "LinePoints".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let point = generator.subschema_for::<SnapPoint>();
        json_schema!({
            "description": "The points the line runs through, in order: 2 to 10 000. Each is a SnapPoint.",
            "type": "array", "items": point, "minItems": 2, "maxItems": MAX_LINE_POINTS
        })
    }
}

/// Whether `v` is a snap that `sketch.point` takes as one, and if so whether it is well formed
/// (what the design makes of it, such as a line that is not there, stays the handler's).
fn snap_form(v: &Value) -> Option<bool> {
    let text = v.as_str();
    let snapped =
        text.is_some_and(|t| ["mid:", "on:", "vertex:"].iter().any(|k| t.starts_with(k))) || v.get("vertex").is_some() || v.get("on_edge").is_some();
    if !snapped {
        return None;
    }
    let numbers = |t: &str| -> Vec<f64> { t.split(',').filter_map(|x| x.trim().parse().ok()).collect() };
    let finite3 = |n: &[f64]| matches!(n, [x, y, z] if x.is_finite() && y.is_finite() && z.is_finite());
    let vertex =
        v.get("vertex").and_then(params::vec3).is_some() || text.and_then(|t| t.strip_prefix("vertex:")).is_some_and(|t| finite3(&numbers(t)));
    let edge = v.get("on_edge").and_then(params::vec3).is_some();
    let mid = text.is_some_and(|t| t.starts_with("mid:"));
    let on = text
        .and_then(|t| t.strip_prefix("on:"))
        .and_then(|t| t.rsplit_once(':'))
        .is_some_and(|(_, xy)| matches!(numbers(xy)[..], [x, y] if x.is_finite() && y.is_finite()));
    Some(vertex || edge || mid || on)
}

/// `sketch.point`'s `point`: `[x, y]` (numbers, or text expressions over the design's parameters),
/// `{x, y}`, or a snap that makes the point. An existing point's id is not taken.
#[derive(Clone, Debug, PartialEq)]
pub struct PointOrSnap(pub Value);

impl<'de> Deserialize<'de> for PointOrSnap {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = Value::deserialize(d)?;
        match snap_form(&v) {
            Some(true) => return Ok(PointOrSnap(v)),
            Some(false) => {
                return Err(D::Error::custom(whole(format!(
                    "`point` {v} is not a snap it takes: \"mid:<line>\", \"on:<curve>:x,y\", \"vertex:x,y,z\", {{vertex: [x,y,z]}} or {{on_edge: [x,y,z]}}"
                ))));
            }
            None => {}
        }
        if let Some([x, y]) = v.as_array().map(Vec::as_slice)
            && (x.is_string() || y.is_string())
        {
            // The coordinates `sketch::expr_point` reads: a finite number, or text that isn't blank.
            let coordinate = |c: &Value| match c {
                Value::Number(n) => n.as_f64().and_then(params::finite).is_some(),
                Value::String(e) => !e.trim().is_empty() && e.len() <= 4096,
                _ => false,
            };
            return if coordinate(x) && coordinate(y) { Ok(PointOrSnap(v)) } else { Err(D::Error::custom(whole(POINT_MUST))) };
        }
        if params::vec2(&v).is_some() { Ok(PointOrSnap(v)) } else { Err(D::Error::custom("must be a point [x, y]")) }
    }
}

impl JsonSchema for PointOrSnap {
    fn schema_name() -> Cow<'static, str> {
        "PointOrSnap".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let point = generator.subschema_for::<Point2>();
        let coordinate = generator.subschema_for::<LengthExpr>();
        let model = generator.subschema_for::<Point3>();
        json_schema!({
            "description": "A point to make: [x, y] in mm (a coordinate may be an expression over the design's parameters: the point then follows them), or a snap that makes it and holds it: \"mid:<line>\" (the line's midpoint), \"on:<curve>:x,y\" (on the curve, near x,y), \"vertex:x,y,z\" or {vertex: [x,y,z]} (a model vertex, projected) or {on_edge: [x,y,z]} (a point on a model edge, projected). An existing point's id is not taken.",
            "anyOf": [
                point,
                {"type": "array", "items": coordinate, "minItems": 2, "maxItems": 2},
                {"type": "string", "pattern": "^(mid:|on:|vertex:)"},
                {"type": "object", "properties": {"vertex": model}, "required": ["vertex"]},
                {"type": "object", "properties": {"on_edge": model}, "required": ["on_edge"]}
            ]
        })
    }
}

/// An id a drawing command gives its curve or point, as `named_id` reads it: text that can be
/// written into a face name whole.
#[derive(Clone, Debug, PartialEq)]
pub struct DrawId(pub String);

/// `id`, or an entry of `ids`, as `named_id` refuses it (the entry's position is not named).
fn id_problem(id: &str) -> Option<String> {
    (!solidcraft_doc::persistent_name::text_fits_name(id))
        .then(|| format!("the id `{id}` cannot be written into a name whole; give an id without `|`, `,`, `~`, `#` or `]`"))
}

impl<'de> Deserialize<'de> for DrawId {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let Value::String(id) = Value::deserialize(d)? else {
            return Err(D::Error::custom("must be text"));
        };
        match id_problem(&id) {
            Some(why) => Err(D::Error::custom(whole(why))),
            None => Ok(DrawId(id)),
        }
    }
}

impl JsonSchema for DrawId {
    fn schema_name() -> Cow<'static, str> {
        "DrawId".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "An id for the new geometry, such as \"base\". It may not hold `|`, `,`, `~`, `#` or `]`, which a face name made from it would read as its own.",
            "type": "string"
        })
    }
}

/// A line's `ids`, as `params::string_list` and `named_id` read them: a list of ids (one id is a
/// list of one), at most 10 000.
#[derive(Clone, Debug, PartialEq)]
pub struct DrawIds(pub Vec<String>);

impl<'de> Deserialize<'de> for DrawIds {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = Value::deserialize(d)?;
        let ids = match params::string_list("", &json!({"ids": v}), "ids") {
            Ok(ids) => ids,
            Err(crate::EngineError::BadParams { msg, .. }) => return Err(D::Error::custom(whole(msg))),
            Err(e) => return Err(D::Error::custom(whole(e))),
        };
        if let Some(why) = ids.iter().find_map(|id| id_problem(id)) {
            return Err(D::Error::custom(whole(why)));
        }
        Ok(DrawIds(ids))
    }
}

impl JsonSchema for DrawIds {
    fn schema_name() -> Cow<'static, str> {
        "DrawIds".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "Ids for the lines, in order: the first for the first segment and so on. One id is a list of one. Segments past the end of the list get an id of the sketch's own. An id may not hold `|`, `,`, `~`, `#` or `]`.",
            "anyOf": [{"type": "string"}, {"type": "array", "items": {"type": "string"}, "maxItems": 10000}]
        })
    }
}

/// A circle's `radius` or `diameter`, as `radius_arg` and `circle_size` read it: a number, or text
/// that is an expression over the design's parameters (a dimension driven by them). Whether it is
/// positive, and whether an expression evaluates, is the handler's to check.
fn size_form(v: &Value, key: &str) -> Result<(), String> {
    match v {
        Value::Number(n) if n.as_f64().and_then(params::finite).is_some() => Ok(()),
        Value::String(_) if params::expr(&json!({ "v": v }), "v").is_some() => Ok(()),
        Value::String(_) => Err(format!("`{key}` {}", params::expr_problem(Some(v)))),
        _ => Err(format!("`{key}` must be a number")),
    }
}

macro_rules! circle_size {
    ($name:ident, $key:literal, $doc:literal) => {
        #[doc = $doc]
        #[derive(Clone, Debug, PartialEq)]
        pub struct $name(pub Value);

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                let v = Value::deserialize(d)?;
                size_form(&v, $key).map(|()| $name(v)).map_err(|why| D::Error::custom(whole(why)))
            }
        }

        impl JsonSchema for $name {
            fn schema_name() -> Cow<'static, str> {
                stringify!($name).into()
            }
            fn inline_schema() -> bool {
                true
            }
            fn json_schema(generator: &mut SchemaGenerator) -> Schema {
                generator.subschema_for::<LengthExpr>()
            }
        }
    };
}

circle_size!(CircleRadius, "radius", "A circle's `radius`: a number or an expression.");
circle_size!(CircleDiameter, "diameter", "A circle's `diameter`: a number or an expression.");

/// An arc's `sweep`, as `params::num` reads it: a number below 1e9 in size. That it is between
/// -360 and 360 and not 0 is the handler's to check.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sweep(pub f64);

impl<'de> Deserialize<'de> for Sweep {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Value::deserialize(d)?.as_f64().and_then(params::finite).map(Sweep).ok_or_else(|| D::Error::custom("must be a number of degrees"))
    }
}

impl JsonSchema for Sweep {
    fn schema_name() -> Cow<'static, str> {
        "Sweep".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "The angle the arc turns through, in degrees: positive counter-clockwise from `start`, negative clockwise. Not 0, and less than 360 in size.",
            "type": "number", "exclusiveMinimum": -360, "exclusiveMaximum": 360
        })
    }
}

/// Draw a line through two or more points, or a closed polyline.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct LineParams {
    /// The sketch to draw in: its feature id or name. Default: the one being edited.
    #[serde(default)]
    pub sketch: Option<SketchTarget>,
    /// The points to run through, in order: one segment between each two. A point that is an
    /// existing point's id (or `{ref}`) shares that point, so the line starts or ends there.
    pub points: LinePoints,
    /// Close the line with a segment from the last point back to the first. Default: false.
    #[serde(default)]
    pub closed: Option<bool>,
    /// Draw construction geometry, which is not part of a profile. Default: false.
    #[serde(default)]
    pub construction: Option<bool>,
    /// Ids for the segments, in order. Default: ids of the sketch's own.
    #[serde(default)]
    pub ids: Option<DrawIds>,
    /// Add a horizontal or vertical constraint to a segment that is drawn exactly so, and a
    /// tangent to the arc or a perpendicular to the line that it starts from. Default: false.
    #[serde(default)]
    pub infer: Option<bool>,
    /// Make a new point that lands exactly on an existing point (the origin too) coincident with
    /// it. Default: true.
    #[serde(default)]
    pub connect: Option<bool>,
}

/// Add a point to the sketch, or a point made by a snap that holds it.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct PointParams {
    /// The sketch to draw in: its feature id or name. Default: the one being edited.
    #[serde(default)]
    pub sketch: Option<SketchTarget>,
    /// Where the point goes, or the snap that makes it.
    pub point: PointOrSnap,
    /// An id for the point. Default: one of the sketch's own. Ignored when `point` is a snap.
    #[serde(default)]
    pub id: Option<DrawId>,
    /// Make the new point coincident with an existing point it lands exactly on (the origin
    /// too). Default: true. Ignored when `point` is a snap.
    #[serde(default)]
    pub connect: Option<bool>,
}

/// Draw a circle from its centre and its radius or diameter.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct CircleCenterParams {
    /// The sketch to draw in: its feature id or name. Default: the one being edited.
    #[serde(default)]
    pub sketch: Option<SketchTarget>,
    /// The centre. A point's id or `{ref}` makes that point the centre, so the circle follows it.
    pub center: SnapPoint,
    /// The radius, in mm: a number, or an expression over the design's parameters, which makes
    /// the radius a dimension driven by them. Wins over `diameter`. Give one of the two.
    #[serde(default)]
    pub radius: Option<CircleRadius>,
    /// The diameter, in mm, as `radius` is given. Used when `radius` is not.
    #[serde(default)]
    pub diameter: Option<CircleDiameter>,
    /// A point the circle passes through, kept on it. With a placed centre and an existing
    /// point, that point and `radius` both fix the radius.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "SnapPoint")]
    pub through: Option<SnapPoint>,
    /// An id for the circle. Default: one of the sketch's own.
    #[serde(default)]
    pub id: Option<DrawId>,
    /// Draw construction geometry, which is not part of a profile. Default: false.
    #[serde(default)]
    pub construction: Option<bool>,
    /// Make a new point that lands exactly on an existing point (the origin too) coincident with
    /// it. Default: true.
    #[serde(default)]
    pub connect: Option<bool>,
}

/// Draw a circle from the two ends of a diameter.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct CircleTwoPointParams {
    /// The sketch to draw in: its feature id or name. Default: the one being edited.
    #[serde(default)]
    pub sketch: Option<SketchTarget>,
    /// One end of the diameter, as [x, y] or {x, y} in mm. An expression, a point's id or a
    /// snap is not taken.
    pub p0: PlainPoint,
    /// The other end of the diameter. Not the same point as `p0`.
    pub p1: PlainPoint,
    /// Draw construction geometry, which is not part of a profile. Default: false.
    #[serde(default)]
    pub construction: Option<bool>,
    /// Make a new point that lands exactly on an existing point (the origin too) coincident with
    /// it. Default: true.
    #[serde(default)]
    pub connect: Option<bool>,
}

/// Draw a circle through three points.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct CircleThreePointParams {
    /// The sketch to draw in: its feature id or name. Default: the one being edited.
    #[serde(default)]
    pub sketch: Option<SketchTarget>,
    /// A point on the circle. One that is an existing point's id stays on the circle.
    pub p0: SnapPoint,
    /// A second point on the circle.
    pub p1: SnapPoint,
    /// A third point on the circle. The three may not be in a line.
    pub p2: SnapPoint,
    /// Draw construction geometry, which is not part of a profile. Default: false.
    #[serde(default)]
    pub construction: Option<bool>,
    /// Make a new point that lands exactly on an existing point (the origin too) coincident with
    /// it. Default: true.
    #[serde(default)]
    pub connect: Option<bool>,
}

/// Draw an arc from its two ends and a point on it.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct ArcThreePointParams {
    /// The sketch to draw in: its feature id or name. Default: the one being edited.
    #[serde(default)]
    pub sketch: Option<SketchTarget>,
    /// One end of the arc.
    pub start: SnapPoint,
    /// The other end of the arc.
    pub end: SnapPoint,
    /// A point the arc passes through, between the ends. One that is an existing point's id
    /// stays on the arc. The three may not be in a line.
    pub through: SnapPoint,
    /// An id for the arc. Default: one of the sketch's own.
    #[serde(default)]
    pub id: Option<DrawId>,
    /// Draw construction geometry, which is not part of a profile. Default: false.
    #[serde(default)]
    pub construction: Option<bool>,
    /// Make a new point that lands exactly on an existing point (the origin too) coincident with
    /// it. Default: true.
    #[serde(default)]
    pub connect: Option<bool>,
}

/// Draw an arc round a centre, from a start point to an end point or through a sweep.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct ArcCenterPointParams {
    /// The sketch to draw in: its feature id or name. Default: the one being edited.
    #[serde(default)]
    pub sketch: Option<SketchTarget>,
    /// The centre of the arc.
    pub center: SnapPoint,
    /// Where the arc starts. Its distance from `center` is the radius.
    pub start: SnapPoint,
    /// A point in the direction of the arc's end from `center`: the arc ends on the ray through
    /// it, at the radius. Give `end` or `sweep`; `sweep` wins.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "SnapPoint")]
    pub end: Option<SnapPoint>,
    /// With `end`: go clockwise from `start` to `end`. Default: false, counter-clockwise.
    #[serde(default)]
    pub clockwise: Option<bool>,
    /// The angle to turn through from `start`, in degrees, instead of `end`.
    #[serde(default)]
    pub sweep: Option<Sweep>,
    /// An id for the arc. Default: one of the sketch's own.
    #[serde(default)]
    pub id: Option<DrawId>,
    /// Draw construction geometry, which is not part of a profile. Default: false.
    #[serde(default)]
    pub construction: Option<bool>,
    /// Make a new point that lands exactly on an existing point (the origin too) coincident with
    /// it. Default: true.
    #[serde(default)]
    pub connect: Option<bool>,
}

/// What a command added to its sketch: the ids that the sketch has now and did not before.
#[derive(Debug, Default, Serialize, JsonSchema)]
pub struct Created {
    /// Point ids.
    pub points: Vec<String>,
    /// Curve ids.
    pub curves: Vec<String>,
    /// Constraint ids, dimensions not among them.
    pub constraints: Vec<String>,
    /// Dimension ids.
    pub dimensions: Vec<String>,
    /// The ids of 3D curves, when the command made any.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub wires: Vec<String>,
}

/// What a drawing command returns for a line, a circle or an arc.
#[derive(Debug, Serialize, JsonSchema)]
pub struct DrawResult {
    /// The ids of the curves drawn, in order.
    pub curves: Vec<String>,
    /// The ids of the constraints added besides the curves' own: a coincident for a point that
    /// landed on an existing one, a point on the curve for a point that was given by id, a radius
    /// for a size given as an expression, what `infer` added.
    pub constraints: Vec<String>,
    /// The sketch's solve: `{solved, dof}`.
    pub sketch: Value,
    /// Everything the call added to the sketch: `{points, curves, constraints, dimensions}`, and
    /// `wires` when there are any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created: Option<Created>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What `sketch.point` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct PointResult {
    /// The id of the point made.
    pub point: String,
    /// The ids of the constraints added: a coincident with the existing point it landed on.
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

/// A rectangle corner as `sketch::corner_arg` reads it: `[x, y]` or `{x, y}` of numbers, or
/// `[x, y]` with a coordinate typed as an expression over the design's parameters. Not a point's
/// id or a snap. Kept as given, for the handler, which evaluates the expressions.
#[derive(Clone, Debug, PartialEq)]
pub struct CornerPoint(pub Value);

impl<'de> Deserialize<'de> for CornerPoint {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = Value::deserialize(d)?;
        if params::vec2(&v).is_some() {
            return Ok(CornerPoint(v));
        }
        // `corner_arg` reads a pair with a coordinate typed as text as `sketch::expr_point` does,
        // which is `point_arg_form`'s expression branch; anything else as `params::req_vec2`.
        let typed = v.as_array().is_some_and(|a| a.len() == 2 && a.iter().any(Value::is_string));
        if typed {
            return crate::cmd::point_arg_form(&v, false).map(|()| CornerPoint(v)).map_err(|why| D::Error::custom(whole(why)));
        }
        Err(D::Error::custom("must be a point [x, y]"))
    }
}

impl JsonSchema for CornerPoint {
    fn schema_name() -> Cow<'static, str> {
        "CornerPoint".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let point = generator.subschema_for::<Point2>();
        let coordinate = generator.subschema_for::<LengthExpr>();
        json_schema!({
            "description": "A corner: [x, y] or {x, y} in mm, or [x, y] where a coordinate is an expression over the design's parameters, such as [\"w / 2\", 5]: the corner then follows the parameters. Not a point's id or a snap.",
            "anyOf": [
                point,
                {"type": "array", "items": coordinate, "minItems": 2, "maxItems": 2}
            ]
        })
    }
}

/// A number as `params::num` reads it (a polygon's `radius`, `diameter`, `sides` and `angle`): a
/// finite number, never an expression.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlainNumber(pub f64);

impl<'de> Deserialize<'de> for PlainNumber {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Value::deserialize(d)?.as_f64().and_then(params::finite).map(PlainNumber).ok_or_else(|| D::Error::custom("must be a number"))
    }
}

impl JsonSchema for PlainNumber {
    fn schema_name() -> Cow<'static, str> {
        "PlainNumber".into()
    }
    fn inline_schema() -> bool {
        true
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({"type": "number"})
    }
}

/// Draw an axis-aligned rectangle from two opposite corners.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct RectTwoPointParams {
    /// The sketch to draw in: its feature id or name. Default: the one being edited.
    #[serde(default)]
    pub sketch: Option<SketchTarget>,
    /// One corner. A coordinate typed as an expression holds the corner where the parameters put it.
    pub p0: CornerPoint,
    /// The opposite corner. It must differ from `p0` in both x and y.
    pub p1: CornerPoint,
    /// Draw construction geometry, which is not part of a profile. Default: false.
    #[serde(default)]
    pub construction: Option<bool>,
    /// Make a new point that lands exactly on an existing point (the origin too) coincident with
    /// it. Default: true.
    #[serde(default)]
    pub connect: Option<bool>,
}

/// Draw a rectangle from one edge and a point on the opposite edge, at any angle.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct RectThreePointParams {
    /// The sketch to draw in: its feature id or name. Default: the one being edited.
    #[serde(default)]
    pub sketch: Option<SketchTarget>,
    /// One end of the first edge, as [x, y] or {x, y} in mm. An expression, a point's id or a
    /// snap is not taken.
    pub p0: PlainPoint,
    /// The other end of the first edge. Not the same point as `p0`.
    pub p1: PlainPoint,
    /// A point on the opposite edge: its distance from the first edge is the rectangle's width.
    /// Not on the first edge's line.
    pub p2: PlainPoint,
    /// Draw construction geometry, which is not part of a profile. Default: false.
    #[serde(default)]
    pub construction: Option<bool>,
    /// Make a new point that lands exactly on an existing point (the origin too) coincident with
    /// it. Default: true.
    #[serde(default)]
    pub connect: Option<bool>,
}

/// Draw an axis-aligned rectangle from its centre and one corner, with construction diagonals
/// that hold the centre.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct RectCenterParams {
    /// The sketch to draw in: its feature id or name. Default: the one being edited.
    #[serde(default)]
    pub sketch: Option<SketchTarget>,
    /// The centre. A coordinate typed as an expression holds the centre where the parameters put it.
    pub center: CornerPoint,
    /// One corner, in any quadrant round the centre. It must differ from `center` in both x and y.
    pub corner: CornerPoint,
    /// Draw construction geometry, which is not part of a profile. Default: false.
    #[serde(default)]
    pub construction: Option<bool>,
    /// Make a new point that lands exactly on an existing point (the origin too) coincident with
    /// it. Default: true.
    #[serde(default)]
    pub connect: Option<bool>,
}

/// Draw an axis-aligned rectangle from two opposite corners, its four corners rounded alike, as
/// one undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct RectRoundedParams {
    /// The sketch to draw in: its feature id or name. Default: the one being edited.
    #[serde(default)]
    pub sketch: Option<SketchTarget>,
    /// One corner of the rectangle before rounding, as `sketch.rectangle.two_point` takes it.
    pub p0: CornerPoint,
    /// The opposite corner. It must differ from `p0` in both x and y.
    pub p1: CornerPoint,
    /// The corner radius, in mm: a number or an expression over the design's parameters. Positive
    /// and less than half the shorter side. One radius dimension holds it; the other arcs are
    /// equal to it.
    pub radius: LengthExpr,
    /// Draw construction geometry, which is not part of a profile. Default: false.
    #[serde(default)]
    pub construction: Option<bool>,
    /// Make a new point that lands exactly on an existing point (the origin too) coincident with
    /// it. Default: true.
    #[serde(default)]
    pub connect: Option<bool>,
}

/// Draw a regular polygon whose corners lie on a circle round its centre.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct PolygonInscribedParams {
    /// The sketch to draw in: its feature id or name. Default: the one being edited.
    #[serde(default)]
    pub sketch: Option<SketchTarget>,
    /// The centre, as [x, y] or {x, y} in mm. An expression, a point's id or a snap is not taken.
    pub center: PlainPoint,
    /// The distance from the centre to each corner, in mm. Positive. Wins over `diameter`. Give
    /// one of the two.
    #[serde(default)]
    pub radius: Option<PlainNumber>,
    /// Twice the distance from the centre to each corner, in mm. Used when `radius` is not.
    #[serde(default)]
    pub diameter: Option<PlainNumber>,
    /// The number of sides: a whole number from 3 to 200. Default: 6.
    #[serde(default)]
    pub sides: Option<PlainNumber>,
    /// The direction of the first corner from the centre, in degrees counter-clockwise from the
    /// sketch's x axis. Default: 90 (straight up).
    #[serde(default)]
    pub angle: Option<PlainNumber>,
    /// Make a new point that lands exactly on an existing point (the origin too) coincident with
    /// it. Default: true.
    #[serde(default)]
    pub connect: Option<bool>,
}

/// Draw a regular polygon whose sides touch a circle round its centre.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct PolygonCircumscribedParams {
    /// The sketch to draw in: its feature id or name. Default: the one being edited.
    #[serde(default)]
    pub sketch: Option<SketchTarget>,
    /// The centre, as [x, y] or {x, y} in mm. An expression, a point's id or a snap is not taken.
    pub center: PlainPoint,
    /// The distance from the centre to the middle of each side, in mm. Positive. Wins over
    /// `diameter`. Give one of the two.
    #[serde(default)]
    pub radius: Option<PlainNumber>,
    /// Twice the distance from the centre to the middle of each side (the width across flats), in
    /// mm. Used when `radius` is not.
    #[serde(default)]
    pub diameter: Option<PlainNumber>,
    /// The number of sides: a whole number from 3 to 200. Default: 6.
    #[serde(default)]
    pub sides: Option<PlainNumber>,
    /// The direction of the middle of the first side from the centre, in degrees counter-clockwise
    /// from the sketch's x axis. Default: 90 (straight up).
    #[serde(default)]
    pub angle: Option<PlainNumber>,
    /// Make a new point that lands exactly on an existing point (the origin too) coincident with
    /// it. Default: true.
    #[serde(default)]
    pub connect: Option<bool>,
}

/// Draw a regular polygon from one of its sides: the polygon lies to the left of the side from
/// `p0` to `p1`.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct PolygonEdgeParams {
    /// The sketch to draw in: its feature id or name. Default: the one being edited.
    #[serde(default)]
    pub sketch: Option<SketchTarget>,
    /// One end of the side, as [x, y] or {x, y} in mm. An expression, a point's id or a snap is
    /// not taken.
    pub p0: PlainPoint,
    /// The other end of the side. Not the same point as `p0`.
    pub p1: PlainPoint,
    /// The number of sides: a whole number from 3 to 200. Default: 6.
    #[serde(default)]
    pub sides: Option<PlainNumber>,
    /// Make a new point that lands exactly on an existing point (the origin too) coincident with
    /// it. Default: true.
    #[serde(default)]
    pub connect: Option<bool>,
}

/// An axis-aligned rectangle's sides by place, as line ids.
#[derive(Debug, Default, Serialize, JsonSchema)]
pub struct RectSides {
    /// The bottom side (least y).
    pub bottom: Option<String>,
    /// The right side (greatest x).
    pub right: Option<String>,
    /// The top side (greatest y).
    pub top: Option<String>,
    /// The left side (least x).
    pub left: Option<String>,
}

/// An axis-aligned rectangle's corners by place, as point ids.
#[derive(Debug, Default, Serialize, JsonSchema)]
pub struct RectCorners {
    /// The corner with the least x and y.
    pub bottom_left: Option<String>,
    /// The corner with the greatest x and least y.
    pub bottom_right: Option<String>,
    /// The corner with the greatest x and y.
    pub top_right: Option<String>,
    /// The corner with the least x and greatest y.
    pub top_left: Option<String>,
}

/// What `sketch.rectangle.two_point` and `sketch.rectangle.center` return.
#[derive(Debug, Serialize, JsonSchema)]
pub struct RectResult {
    /// The ids of the four sides, counter-clockwise from the bottom side.
    pub curves: Vec<String>,
    /// The ids of the constraints added: horizontal and vertical sides, a coincident for a corner
    /// that landed on an existing point, the centre rectangle's midpoint.
    pub constraints: Vec<String>,
    /// The sketch's solve: `{solved, dof}`.
    pub sketch: Value,
    /// The sides by place, so a caller can dimension them without inspecting the sketch.
    pub sides: RectSides,
    /// The corners by place.
    pub corners: RectCorners,
    /// Everything the call added to the sketch: `{points, curves, constraints, dimensions}`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created: Option<Created>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What `sketch.rectangle.rounded` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct RoundedRectResult {
    /// The ids of the four sides, then the four corner arcs.
    pub curves: Vec<String>,
    /// The ids of the constraints added: the rectangle's, then the fillet's (tangencies and the
    /// arcs equal to the first).
    pub constraints: Vec<String>,
    /// The sketch's solve after the rounding: `{solved, dof}`.
    pub sketch: Value,
    /// The sides by place. They end at the arcs now; there are no sharp corners to name.
    pub sides: RectSides,
    /// The corner arcs: bottom-left, bottom-right, top-right, top-left.
    pub arcs: Vec<String>,
    /// The parameter that holds the corner radius.
    pub param: String,
    /// Everything the call added to the sketch: `{points, curves, constraints, dimensions}`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created: Option<Created>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What the polygon commands return.
#[derive(Debug, Serialize, JsonSchema)]
pub struct PolygonResult {
    /// The ids of the sides, in order round the polygon.
    pub curves: Vec<String>,
    /// The ids of the constraints added: equal sides, the corners on their construction circle
    /// (and, round a circumscribed polygon, its tangent side), a coincident for a corner that
    /// landed on an existing point.
    pub constraints: Vec<String>,
    /// The sketch's solve: `{solved, dof}`.
    pub sketch: Value,
    /// The id of the construction circle that holds the polygon to its centre: through the
    /// corners of an inscribed or edge polygon, touching the sides of a circumscribed one.
    pub support_circle: String,
    /// Everything the call added to the sketch: `{points, curves, constraints, dimensions}`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created: Option<Created>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

#[cfg(test)]
#[path = "sketch_shapes_tests.rs"]
mod tests;
