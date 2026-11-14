//! Contract and result type for the command in `cmd/sketch.rs`: `sketch.dimension`.
//!
//! The handler reads its parameters from the raw JSON, so each key here reads as the handler reads
//! it and fails with the handler's message. What only the design can tell (a sketch, curve or point
//! that exists, a model edge that projects to something, a name that is free, whether the
//! combination of entities can be dimensioned) stays the handler's to check. `entities` is
//! optional in the type, so the handler keeps its own message when it is left out, and required in
//! the schema.
//!
//! The handler reads some keys only in some cases, and so does the contract: a driven dimension
//! (`driven: true`) measures and has no value, so its `value` (apart from a definition, which
//! the handler refuses by itself), `scale_sketch`, `scale_about_origin` and `if_redundant` are not
//! read and not checked.
//! `if_redundant` is read only once the dimension turns out to be determined by the sketch.
//!
//! The handler reads a few keys more leniently than a type can (a `type` that is not text counts
//! as `auto`, a `driven` that is not true or false as false): the type refuses them, which is what
//! warn reports and strict refuses. A call with several problems may get a different one first in
//! strict than with the check off (the check goes key by key, the handler in its own order).

use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value};

use super::check::whole;
use super::motion::require;
use super::sketch_ends::SketchTarget;
use super::types::{LengthExpr, Point2, Point3};
use crate::cmd::{MAX_PROJECTED_ENTITIES, model_ref};
use crate::params::json_brief;

/// One or two entities to dimension: a curve or point id (`l1`, `p3`, `l1.start`), `x` or `y` (a
/// sketch axis), `mid:<line id>` (a line's midpoint), or a model edge `{edge: [x, y, z], body?}`
/// or vertex `{vertex: [x, y, z], body?}`, which the handler projects into the sketch first. One
/// id may be given as text rather than a list.
#[derive(Clone, Debug, PartialEq)]
pub struct DimensionEntities(pub Vec<Value>);

impl DimensionEntities {
    /// The entities `v` lists, or the handler's message. In the handler's order: a list of more
    /// than 1000 is refused before anything, then the entries that are not names, then the count.
    /// A model reference is left to the handler: it looks up the sketch before it reads the form
    /// of the reference, so a reference that is no form of one is its to refuse (with "no sketch
    /// is being edited" when there is no sketch, which only the session can tell).
    fn read(v: &Value) -> Result<DimensionEntities, String> {
        let list: Vec<Value> = match v {
            Value::String(_) => vec![v.clone()],
            Value::Array(list) => {
                if list.len() > MAX_PROJECTED_ENTITIES {
                    return Err(format!("`entities` may list at most {MAX_PROJECTED_ENTITIES} entities"));
                }
                if let Some((i, e)) = list.iter().enumerate().find(|(_, e)| !e.is_string() && !model_ref(e)) {
                    return Err(format!("`entities` must list names; the entry {} is {}", i + 1, json_brief(e)));
                }
                list.clone()
            }
            other => return Err(format!("`entities` must be a list of names; it is {}", json_brief(other))),
        };
        if list.is_empty() || list.len() > 2 {
            return Err("`entities` must list one or two sketch entities".into());
        }
        Ok(DimensionEntities(list))
    }
}

impl JsonSchema for DimensionEntities {
    fn schema_name() -> Cow<'static, str> {
        "DimensionEntities".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let point = generator.subschema_for::<Point3>();
        let model = |key: &str| {
            serde_json::json!({
                "type": "object",
                "properties": {key: point, "body": {"type": "string", "description": "The body's name. Default: any body. Text only; anything else is read as left out."}},
                "required": [key]
            })
        };
        let one = serde_json::json!({"anyOf": [{"type": "string"}, model("edge"), model("vertex")]});
        json_schema!({
            "description": "One or two entities: a curve or point id (`l1`, `p3`, `l1.start`, `c1.center`), `x` or `y` (a sketch axis), `mid:<line id>` (a line's midpoint), or a model edge `{edge: [x, y, z], body?}` or vertex `{vertex: [x, y, z], body?}`, projected into the sketch first and measured there. One id may be given as text instead of a list.",
            "anyOf": [{"type": "string"}, {"type": "array", "items": one, "minItems": 1, "maxItems": 2}]
        })
    }
}

/// Dimension one or two sketch entities: a length, distance, radius, diameter, angle or ellipse
/// half-axis, driving the sketch (it adds a parameter) or only measuring (`driven: true`). The
/// kind follows from the entities, `type` and where `text_at` is. One undo step.
#[derive(Debug, JsonSchema)]
#[schemars(deny_unknown_fields, transform = entities_required)]
pub struct SketchDimensionParams {
    // Optional in the type and required in the schema (`entities_required`): left out, the
    // handler refuses it with its own message, not the check's "is required".
    /// The entities to dimension, one or two.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "DimensionEntities")]
    pub entities: Option<DimensionEntities>,
    /// The sketch (default: the one being edited).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "SketchTarget")]
    pub sketch: Option<SketchTarget>,
    /// What to measure, in any case: auto (the default: from the entities and where `text_at`
    /// is), distance, horizontal (or distance_x), vertical (or distance_y), length, radius,
    /// diameter, major or minor (an ellipse's half-axis), angle, sweep (an arc's angle),
    /// arc_length (arclength), overall_length (a slot's overall length), offset. Other text is no
    /// known type: the entities' default measure (a length or a radius), with no choice by
    /// `text_at`.
    #[serde(default, rename = "type", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub kind: Option<String>,
    /// With type auto or distance: `horizontal` or `vertical` measures along that axis, as
    /// type does. Any other text is ignored.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub orientation: Option<String>,
    /// The dimension's value: a number (in the design's units, or degrees for an angle) or an
    /// expression such as `"width / 2"` or `"50 mm"`; or `name = expression` (also `#name =
    /// expression`), which adds the user parameter `name` and dimensions with it (refused when the
    /// name is taken or reserved; the reply's `defined` names it). Default: what the geometry
    /// measures now. Not read for a driven dimension, which has no value (a definition is refused
    /// there).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "LengthExpr")]
    pub value: Option<LengthExpr>,
    /// Where the dimension's text goes, in sketch coordinates [x, y] (mm): with type auto it also
    /// chooses what is measured (horizontal or vertical distance, an angle's sector, an ellipse's
    /// half-axis).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Point2")]
    pub text_at: Option<Point2>,
    /// True to place a driven (reference) dimension: it measures, has no parameter and does not
    /// constrain. Default: false.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "bool")]
    pub driven: Option<bool>,
    /// When this is the sketch's first length dimension and nothing in the sketch fixes a size
    /// (no other driving length dimension, nothing fixed or projected), scale the whole sketch
    /// to the value instead of distorting it (the reply's `scaled` is the factor). A free
    /// drawing uses its own centre. A drawing tied to the origin scales about the origin.
    /// Default: false. Not read for a driven dimension.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "bool")]
    pub scale_sketch: Option<bool>,
    /// Scale about the sketch origin even when the drawing is free. Default: false. Not read
    /// for a driven dimension.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "bool")]
    pub scale_about_origin: Option<bool>,
    /// For an ellipse with type auto: `major` or `minor`, the half-axis an earlier placement
    /// chose, kept while `text_at` is near the boundary between them (any case; other text is
    /// ignored).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub previous_axis: Option<String>,
    /// For an ellipse with type auto: true gives the half-axis `text_at` picks, false the minor
    /// radius wherever `text_at` is. Default: true.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "bool")]
    pub half_axes: Option<bool>,
    /// `driven` (any case): when the sketch already determines the dimension, place it as a
    /// driven (reference) dimension instead of refusing it; the reply then has `redundant` and
    /// `determined_by`. Any other text refuses it. Read only when the dimension turns out to be
    /// determined, and not for a driven dimension.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub if_redundant: Option<String>,
}

fn entities_required(schema: &mut Schema) {
    require(schema, "entities");
}

/// `null` is a key left out, as the handler's readers read it.
fn given<'a>(o: &'a Map<String, Value>, key: &str) -> Option<&'a Value> {
    o.get(key).filter(|v| !v.is_null())
}

/// A key's text, or its message.
fn text(o: &Map<String, Value>, key: &str) -> Result<Option<String>, String> {
    match given(o, key) {
        None => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.clone())),
        Some(_) => Err(format!("`{key}` must be text")),
    }
}

/// A key's true or false, or its message (`message`, when the handler has its own).
fn flag(o: &Map<String, Value>, key: &str, message: Option<&str>) -> Result<Option<bool>, String> {
    match given(o, key) {
        None => Ok(None),
        Some(Value::Bool(b)) => Ok(Some(*b)),
        Some(_) => Err(message.map_or_else(|| format!("`{key}` must be true or false"), str::to_string)),
    }
}

/// A key read by a type's own `Deserialize`, which says what is wrong ("must be …").
fn typed<'a, T: Deserialize<'a>>(o: &'a Map<String, Value>, key: &str) -> Result<Option<T>, String> {
    match given(o, key) {
        None => Ok(None),
        Some(v) => T::deserialize(v).map(Some).map_err(|e| format!("`{key}` {e}")),
    }
}

impl SketchDimensionParams {
    /// The keys the handler reads, in its order and in the cases it reads them.
    fn read(o: &Map<String, Value>) -> Result<SketchDimensionParams, String> {
        let entities = given(o, "entities").map(DimensionEntities::read).transpose()?;
        // `entities: null` and a missing one are both "must list one or two sketch entities".
        let kind = text(o, "type")?;
        let orientation = text(o, "orientation")?;
        let text_at = typed::<Point2>(o, "text_at")?;
        let driven = flag(o, "driven", None)?;
        let previous_axis = text(o, "previous_axis")?;
        let half_axes = flag(o, "half_axes", Some("`half_axes` must be true or false"))?;
        // The sketch is read after the entities; a model reference among them is left to the
        // handler, which looks the sketch up first (see `DimensionEntities::read`).
        let sketch = typed::<SketchTarget>(o, "sketch")?;
        let (mut value, mut scale_sketch, mut scale_about_origin, mut if_redundant) = (None, None, None, None);
        // A driven dimension measures: it has no value to read, nothing to scale and nothing it
        // could be redundant with.
        if driven != Some(true) {
            value = typed::<LengthExpr>(o, "value")?;
            scale_sketch = flag(o, "scale_sketch", None)?;
            scale_about_origin = flag(o, "scale_about_origin", None)?;
            if_redundant = text(o, "if_redundant")?;
        }
        Ok(SketchDimensionParams {
            entities,
            sketch,
            kind,
            orientation,
            value,
            text_at,
            driven,
            scale_sketch,
            scale_about_origin,
            previous_axis,
            half_axes,
            if_redundant,
        })
    }
}

impl<'de> Deserialize<'de> for SketchDimensionParams {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let o = Map::<String, Value>::deserialize(d)?;
        SketchDimensionParams::read(&o).map_err(|why| D::Error::custom(whole(why)))
    }
}

/// What `sketch.dimension` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct DimensionResult {
    /// The parameter that holds the dimension's value (`d1`, or the name a `name = expression`
    /// value defined). A driven dimension's is read-only: it reads what the dimension measures.
    pub param: Option<String>,
    /// `true` for a driven (reference) dimension, as asked or placed instead of a redundant one.
    /// Absent for a driving one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub driven: Option<bool>,
    /// What the dimension is: `LinearDimension`, `RadialDimension`, `DiameterDimension`,
    /// `AngularDimension`, `ArcLengthDimension` or `LinearDiameterDimension`.
    #[serde(rename = "type")]
    pub kind: String,
    /// The parameter's expression, as the design stores it, in the design's units (a driving
    /// dimension only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expression: Option<String>,
    /// What the dimension measures on the solved sketch, in `unit`.
    pub measured: f64,
    /// `mm` for a length or `deg` for an angle.
    pub unit: String,
    /// The sketch's solve: `{solved, dof}`.
    pub sketch: Value,
    /// The factor the whole sketch was scaled by (`scale_sketch`), when it was.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scaled: Option<f64>,
    /// For an ellipse: the half-axis the dimension measures, `major` or `minor`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub axis: Option<String>,
    /// The user parameter a `name = expression` value added.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub defined: Option<String>,
    /// `true` when the sketch already determined the dimension and `if_redundant` was `driven`:
    /// it was placed as a driven dimension.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub redundant: Option<bool>,
    /// With `redundant`: the constraints that determine it, as "id (Name)".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub determined_by: Option<Vec<String>>,
    /// What the command added to its sketch, `{points, curves, constraints, dimensions}` (lists
    /// of ids): the session adds it to the reply of a sketch command, so the handler leaves it out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created: Option<Value>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

#[cfg(test)]
#[path = "sketch_dimension_tests.rs"]
mod tests;
