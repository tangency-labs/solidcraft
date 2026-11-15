//! Contracts and result types for the commands in `cmd/sketch_constraints.rs`: the curvature and
//! polygon constraints, the constrainer, Auto Constrain (three commands), Toggle Driven Dimension,
//! Toggle Radius / Diameter, the constraint glyphs and Snap Inference.
//!
//! The handlers read their parameters from the raw JSON, so each key here reads as its handler
//! reads it and fails with the handler's message. What only the design can tell (whether a
//! sketch, curve, point or dimension exists) stays the handler's to check. A key the handler
//! refuses when it is left out (`a`, `b`, `lines`, `entities`, `constraint`, `at`) is optional in
//! the type, so the handler keeps its own message, and required in the schema. A call with several
//! problems may get a different one first in strict than with the check off (the check goes key by
//! key, the handlers in their own order).

use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

use super::check::whole;
use super::config::NameList;
use super::motion::require;
use super::sketch_ends::SketchTarget;
use super::types::{Point2, SketchRef, present};
use crate::params;

/// Switch a circle's or an arc's dimension between its radius and its diameter.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct ToggleDiameterParams {
    /// The radius or diameter dimension's constraint id.
    #[serde(deserialize_with = "dimension_id")]
    pub constraint: String,
    /// `true` for a diameter, `false` for a radius (default: the other one).
    #[serde(default)]
    pub diameter: Option<bool>,
    /// The sketch (default: the one being edited).
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "SketchRef")]
    pub sketch: Option<SketchRef>,
}

/// A constraint id, with the handler's message for anything else.
fn dimension_id<'de, D: serde::Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    match Value::deserialize(d)? {
        Value::String(s) => Ok(s),
        _ => Err(serde::de::Error::custom("must be a dimension id")),
    }
}

/// What `sketch.toggle_diameter` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ToggleDiameterResult {
    /// The dimension's constraint id (unchanged).
    pub constraint: String,
    /// Whether it now holds the diameter.
    pub diameter: bool,
    /// The sketch's solve: `{solved, dof}`.
    pub sketch: Value,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// A curve's id, as `sketch.constraint.curvature` reads `a` and `b`: text, else its message.
#[derive(Clone, Debug, PartialEq)]
pub struct CurveId(pub String);

impl<'de> Deserialize<'de> for CurveId {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match Value::deserialize(d)? {
            Value::String(s) => Ok(CurveId(s)),
            _ => Err(D::Error::custom(whole("expected a curve id"))),
        }
    }
}

impl JsonSchema for CurveId {
    fn schema_name() -> Cow<'static, str> {
        "CurveId".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({"description": "A curve's id in the sketch, such as `c3`.", "type": "string"})
    }
}

/// A dimension's constraint id, as `sketch.toggle_driven` reads `constraint`.
#[derive(Clone, Debug, PartialEq)]
pub struct DimensionId(pub String);

impl<'de> Deserialize<'de> for DimensionId {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match Value::deserialize(d)? {
            Value::String(s) => Ok(DimensionId(s)),
            _ => Err(D::Error::custom("must be a dimension id")),
        }
    }
}

impl JsonSchema for DimensionId {
    fn schema_name() -> Cow<'static, str> {
        "DimensionId".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({"description": "A dimension's constraint id in the sketch.", "type": "string"})
    }
}

/// The point of `sketch.snap`'s `at`: `[x, y]` or `{x, y}` of numbers (below 1e9), with
/// `params::req_vec2`'s message.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClickPoint(pub solidcore_record::Vec2);

impl<'de> Deserialize<'de> for ClickPoint {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        params::vec2(&Value::deserialize(d)?).map(ClickPoint).ok_or_else(|| D::Error::custom("must be a point [x, y]"))
    }
}

impl JsonSchema for ClickPoint {
    fn schema_name() -> Cow<'static, str> {
        "ClickPoint".into()
    }
    fn inline_schema() -> bool {
        true
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        generator.subschema_for::<Point2>()
    }
}

/// The ids of the refusing-when-left-out keys are listed in the schema's `required`.
fn require_keys(schema: &mut Schema, keys: &[&str]) {
    for key in keys {
        require(schema, key);
    }
}

fn curvature_required(schema: &mut Schema) {
    require_keys(schema, &["a", "b"]);
}

fn polygon_required(schema: &mut Schema) {
    require_keys(schema, &["lines"]);
}

fn constrainer_required(schema: &mut Schema) {
    require_keys(schema, &["entities"]);
}

fn driven_required(schema: &mut Schema) {
    require_keys(schema, &["constraint"]);
}

fn snap_required(schema: &mut Schema) {
    require_keys(schema, &["at"]);
}

/// Make two curves meet with curvature continuity (G2): they must share an end point.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = curvature_required)]
pub struct CurvatureParams {
    // Optional in the type and required in the schema (`curvature_required`): left out, the
    // handler refuses it with its own message, not the check's "is required".
    /// The first curve's id.
    #[serde(default)]
    pub a: Option<CurveId>,
    /// The second curve's id.
    #[serde(default)]
    pub b: Option<CurveId>,
    /// The sketch (default: the one being edited).
    #[serde(default)]
    pub sketch: Option<SketchTarget>,
    /// `refuse` (any case) refuses the constraint when the others already determine it. Anything
    /// else keeps it, shown dimmed as redundant. Read only then.
    #[serde(default)]
    pub if_redundant: Option<String>,
}

/// Make a closed chain of lines a regular polygon: the sides equal and every corner on one
/// (construction) circle, which is added.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = polygon_required)]
pub struct PolygonParams {
    // Optional in the type and required in the schema, as `a` above.
    /// The ids of the lines of the closed chain: 3 to 200 of them, each a line whose end points
    /// are shared with its neighbours.
    #[serde(default)]
    pub lines: Option<NameList>,
    /// The sketch (default: the one being edited).
    #[serde(default)]
    pub sketch: Option<SketchTarget>,
}

/// Apply the constraint one or two sketch entities nearly have already: horizontal or vertical
/// for one line; for two, coincident, point on curve, collinear, parallel, perpendicular, equal,
/// concentric or tangent, whichever the geometry is closest to.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = constrainer_required)]
pub struct ConstrainerParams {
    // Optional in the type and required in the schema, as `a` above.
    /// One or two sketch entity ids (curves or points): one line, or two entities.
    #[serde(default)]
    pub entities: Option<NameList>,
    /// The sketch (default: the one being edited).
    #[serde(default)]
    pub sketch: Option<SketchTarget>,
    /// `refuse` (any case) refuses the constraint when the others already determine it. Anything
    /// else keeps it, shown dimmed as redundant. Read only then.
    #[serde(default)]
    pub if_redundant: Option<String>,
}

/// Infer the constraints the sketch's geometry nearly has (coincident points, horizontal and
/// vertical lines, tangent joints), then dimension what is left until it is fully constrained.
/// A constraint that removes no freedom or that the sketch could not solve with is dropped.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct AutoConstrainParams {
    /// How close two points must be to be made coincident, in mm (a number of 0 or less, or of
    /// 1e9 or more, is read as the default). Default: 0.01.
    #[serde(default)]
    pub tolerance: Option<f64>,
    /// How far a line may be from horizontal or vertical, or a joint from tangent, in degrees (a
    /// number of 0 or less, or of 1e9 or more, is read as the default). Default: 1.
    #[serde(default)]
    pub angle_tolerance: Option<f64>,
    /// Add dimensions (lengths, radii, diameters, distances from the datum). Default: true.
    #[serde(default)]
    pub dimensions: Option<bool>,
    /// The point the position dimensions are measured from: a point id or a name the sketch
    /// resolves (as `sketch.auto_constrain_from_datum` takes it). Default: the sketch origin.
    #[serde(default)]
    pub datum: Option<String>,
    /// The sketch (default: the one being edited).
    #[serde(default)]
    pub sketch: Option<SketchTarget>,
}

/// Auto Constrain with the position dimensions measured from a datum point instead of the
/// origin.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct AutoConstrainDatumParams {
    /// How close two points must be to be made coincident, in mm (a number of 0 or less, or of
    /// 1e9 or more, is read as the default). Default: 0.01.
    #[serde(default)]
    pub tolerance: Option<f64>,
    /// How far a line may be from horizontal or vertical, or a joint from tangent, in degrees (a
    /// number of 0 or less, or of 1e9 or more, is read as the default). Default: 1.
    #[serde(default)]
    pub angle_tolerance: Option<f64>,
    /// Add dimensions (lengths, radii, diameters, distances from the datum). Default: true.
    #[serde(default)]
    pub dimensions: Option<bool>,
    /// The point the position dimensions are measured from: a point id or a name the sketch
    /// resolves. Default: the sketch origin.
    #[serde(default)]
    pub datum: Option<String>,
    /// The sketch (default: the one being edited).
    #[serde(default)]
    pub sketch: Option<SketchTarget>,
}

/// Auto Constrain, then Finish Sketch: the sketch is closed when it is done.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct FinishAutoConstrainParams {
    /// How close two points must be to be made coincident, in mm (a number of 0 or less, or of
    /// 1e9 or more, is read as the default). Default: 0.01.
    #[serde(default)]
    pub tolerance: Option<f64>,
    /// How far a line may be from horizontal or vertical, or a joint from tangent, in degrees (a
    /// number of 0 or less, or of 1e9 or more, is read as the default). Default: 1.
    #[serde(default)]
    pub angle_tolerance: Option<f64>,
    /// Add dimensions (lengths, radii, diameters, distances from the datum). Default: true.
    #[serde(default)]
    pub dimensions: Option<bool>,
    /// The point the position dimensions are measured from: a point id or a name the sketch
    /// resolves. Default: the sketch origin.
    #[serde(default)]
    pub datum: Option<String>,
    /// The sketch (default: the one being edited).
    #[serde(default)]
    pub sketch: Option<SketchTarget>,
}

/// Make a dimension driven (it measures, and is no longer a parameter) or driving again (it gets
/// a parameter holding its current value).
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = driven_required)]
pub struct ToggleDrivenParams {
    // Optional in the type and required in the schema, as `a` above.
    /// The dimension's constraint id.
    #[serde(default)]
    pub constraint: Option<DimensionId>,
    /// `true` for driven, `false` for driving. Default: the other one from now.
    #[serde(default)]
    pub driven: Option<bool>,
    /// The sketch (default: the one being edited).
    #[serde(default)]
    pub sketch: Option<SketchTarget>,
}

/// List every constraint of a sketch with where its glyph is drawn (sketch coordinates), what it
/// is and whether it fails or is redundant, with the sketch's freedom. Changes nothing.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct GlyphsParams {
    /// The sketch (default: the one being edited).
    #[serde(default)]
    pub sketch: Option<SketchTarget>,
}

/// What a click at a point in the sketch being edited would snap to or infer, best first: a
/// point, a line's midpoint, a point on a curve, and horizontal or vertical alignment with the
/// previous click. Changes nothing.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = snap_required)]
pub struct SnapParams {
    // Optional in the type and required in the schema, as `a` above.
    /// The click, [x, y] in the sketch's coordinates, in mm.
    #[serde(default)]
    pub at: Option<ClickPoint>,
    /// The previous click, [x, y]: horizontal and vertical alignment with it is offered. Default:
    /// none.
    #[serde(default)]
    pub from: Option<Point2>,
    /// The snap distance in mm (a number of 0 or less, or of 1e9 or more, is read as the
    /// default). Default: 1.
    #[serde(default)]
    pub radius: Option<f64>,
}

/// What the curvature constraint returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ConstraintAdded {
    /// The new constraint's id.
    pub constraint: String,
    /// The sketch's solve: `{solved, dof}`.
    pub sketch: Value,
    /// `true` when the other constraints already determine it (present only then).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub redundant: bool,
    /// The constraints that already determine it, as "id (Name)" (present only when redundant).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub determined_by: Option<Vec<String>>,
    /// What the command added to its sketch, `{points, curves, constraints, dimensions}` (lists
    /// of ids): the session adds it to the reply of a sketch command, so the handler leaves it out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created: Option<Value>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What `sketch.constrainer` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ConstrainerResult {
    /// The new constraint's id.
    pub constraint: String,
    /// The sketch's solve: `{solved, dof}`.
    pub sketch: Value,
    /// `true` when the other constraints already determine it (present only then).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub redundant: bool,
    /// The constraints that already determine it, as "id (Name)" (present only when redundant).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub determined_by: Option<Vec<String>>,
    /// The kind of constraint applied, such as `Horizontal` or `Tangent`.
    #[serde(rename = "type")]
    pub kind: String,
    /// What the command added to its sketch, `{points, curves, constraints, dimensions}` (lists
    /// of ids): the session adds it to the reply of a sketch command, so the handler leaves it out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created: Option<Value>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What the polygon constraint returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct PolygonResult {
    /// The new constraints' ids: equal sides, then the corners on the circle.
    pub constraints: Vec<String>,
    /// The sketch's solve: `{solved, dof}`.
    pub sketch: Value,
    /// What the command added to its sketch, `{points, curves, constraints, dimensions}` (lists
    /// of ids): the session adds it to the reply of a sketch command, so the handler leaves it out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created: Option<Value>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What the Auto Constrain commands return.
#[derive(Debug, Serialize, JsonSchema)]
pub struct AutoConstrainResult {
    /// The ids of the constraints and dimensions it added.
    pub added: Vec<String>,
    /// The degrees of freedom left (0: fully constrained).
    pub dof: usize,
    /// The sketch's solve: `{solved, dof}`.
    pub sketch: Value,
    /// What the command added to its sketch, `{points, curves, constraints, dimensions}` (lists
    /// of ids): the session adds it to the reply of a sketch command, so the handler leaves it out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created: Option<Value>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What `sketch.toggle_driven` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ToggleDrivenResult {
    /// The dimension's constraint id (unchanged).
    pub constraint: String,
    /// Whether it is driven now.
    pub driven: bool,
    /// The sketch's solve: `{solved, dof}`.
    pub sketch: Value,
    /// What the command added to its sketch, `{points, curves, constraints, dimensions}` (lists
    /// of ids): the session adds it to the reply of a sketch command, so the handler leaves it out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created: Option<Value>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// One constraint of `sketch.glyphs`, with where its glyph goes.
#[derive(Debug, Serialize, JsonSchema)]
pub struct Glyph {
    /// The constraint's id.
    pub id: String,
    /// Its kind, such as `Horizontal` or `Radius`.
    pub name: String,
    /// Whether it is a dimension.
    pub dimension: bool,
    /// Whether it is driven (it measures; it does not constrain).
    pub driven: bool,
    /// A dimension's value (mm, or radians for an angle); `null` for another constraint.
    pub value: Option<f64>,
    /// The parameter a driving dimension takes its value from; `null` for none.
    pub param: Option<String>,
    /// Where to draw the glyph, [x, y] in sketch coordinates; `null` when it has no place.
    pub at: Option<[f64; 2]>,
    /// Whether it is one of the constraints the solver could not satisfy.
    pub failing: bool,
    /// Whether the others imply it (it removes no freedom) though it holds: shown dimmed.
    pub redundant: bool,
}

/// What `sketch.glyphs` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct GlyphsResult {
    /// The sketch's feature id.
    pub sketch: u64,
    /// Its constraints, in order.
    pub glyphs: Vec<Glyph>,
    /// The degrees of freedom left.
    pub dof: usize,
    /// Whether nothing is left free.
    pub fully_constrained: bool,
    /// The ids of the constraints that fail.
    pub conflicts: Vec<String>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// One thing a click would snap to or infer.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Snap {
    /// An existing point (an end point, a centre, the origin): the click is made coincident.
    Coincident {
        /// The point's id.
        point: String,
        /// Where it is, [x, y].
        at: [f64; 2],
    },
    /// The middle of a line.
    Midpoint {
        /// The line's id.
        curve: String,
        /// The middle, [x, y].
        at: [f64; 2],
    },
    /// The nearest place on a curve.
    OnCurve {
        /// The curve's id.
        curve: String,
        /// That place, [x, y].
        at: [f64; 2],
    },
    /// Level with the previous click (`from`).
    Horizontal {
        /// The click moved to that level, [x, y].
        at: [f64; 2],
    },
    /// Straight above or below the previous click (`from`).
    Vertical {
        /// The click moved to that line, [x, y].
        at: [f64; 2],
    },
}

/// What `sketch.snap` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct SnapResult {
    /// At most eight, best first: the first is what a click would use.
    pub snaps: Vec<Snap>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

#[cfg(test)]
#[path = "sketch_constraints_tests.rs"]
mod tests;
