//! Contract and result type for `sketch.draw.dimensioned` in `cmd/sketch_dimensioned.rs` (#13313).
//!
//! The handler reads its parameters from the raw JSON, so each key here reads as the handler
//! reads it and fails with the handler's message. `command` and `params` are optional in the
//! type and required in the schema, so that the handler's message, not the check's "is required",
//! is what a caller sees when they are left out. Which roles `dimensions` may name depends on the
//! creator, and whether a value can be evaluated or is in range depends on the design: both stay
//! the handler's to check. A call with several problems may get a different one first in strict
//! than with the check off (the check goes key by key, the handler in its own order).

use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value, json};

use super::check::whole;
use super::motion::require;
use super::types::{AngleExpr, LengthExpr};
use crate::params;

/// The sketch creators `sketch.draw.dimensioned` can dimension: those `dimension_roles` lists.
pub const CREATORS: &[&str] = &[
    "sketch.line",
    "sketch.line.midpoint",
    "sketch.rectangle.two_point",
    "sketch.rectangle.center",
    "sketch.rectangle.three_point",
    "sketch.rectangle.rounded",
    "sketch.rectangle.edge_center",
    "sketch.circle.center",
    "sketch.circle.two_point",
    "sketch.circle.three_point",
    "sketch.arc.center_point",
    "sketch.ellipse",
    "sketch.slot.center_to_center",
    "sketch.slot.overall",
    "sketch.slot.center_point",
    "sketch.slot.arc_center",
    "sketch.slot.arc_three_point",
    "sketch.polygon.inscribed",
    "sketch.polygon.circumscribed",
    "sketch.polygon.edge",
];

/// A sketch creator as `command` names it: one `dimension_roles` knows.
#[derive(Clone, Debug, PartialEq)]
pub struct Creator(pub String);

impl<'de> Deserialize<'de> for Creator {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match Value::deserialize(d)? {
            Value::String(s) if crate::dimension_roles(&s).is_some() => Ok(Creator(s)),
            Value::String(_) => Err(D::Error::custom(whole("unsupported sketch creator"))),
            _ => Err(D::Error::custom(whole("`command` must name a supported sketch creator"))),
        }
    }
}

impl JsonSchema for Creator {
    fn schema_name() -> Cow<'static, str> {
        "Creator".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "A sketch creation command whose shape can be dimensioned.",
            "type": "string",
            "enum": CREATORS
        })
    }
}

/// The creator's own parameters: an object (`null` is refused).
#[derive(Clone, Debug, PartialEq)]
pub struct CreatorParams(pub Map<String, Value>);

impl<'de> Deserialize<'de> for CreatorParams {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match Value::deserialize(d)? {
            Value::Object(o) => Ok(CreatorParams(o)),
            _ => Err(D::Error::custom(whole("`params` must be an object"))),
        }
    }
}

impl JsonSchema for CreatorParams {
    fn schema_name() -> Cow<'static, str> {
        "CreatorParams".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "The parameters of the creator `command` names, as that command takes them. The creator checks them.",
            "type": "object"
        })
    }
}

/// The sizes to place, by role. Each value is a number or an expression of at most 4096
/// characters, as `params::expr` reads it (`null` is refused).
#[derive(Clone, Debug, PartialEq)]
pub struct DimensionValues(pub Map<String, Value>);

impl<'de> Deserialize<'de> for DimensionValues {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let Value::Object(o) = Value::deserialize(d)? else {
            return Err(D::Error::custom(whole("`dimensions` must be an object")));
        };
        for value in o.values() {
            if params::expr(&json!({"value": value}), "value").is_none_or(|e| e.len() > 4096) {
                return Err(D::Error::custom(whole("dimension values must be bounded expressions or finite numbers")));
            }
        }
        Ok(DimensionValues(o))
    }
}

impl JsonSchema for DimensionValues {
    fn schema_name() -> Cow<'static, str> {
        "DimensionValues".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let length = generator.subschema_for::<LengthExpr>();
        let angle = generator.subschema_for::<AngleExpr>();
        json_schema!({
            "description": "The sizes to place, each by its role: a number (in the design's unit) or an expression, or `name = expression`, which also defines the user parameter `name`. The roles a creator takes are its own: a line or midpoint line takes `length` and `angle`; a rectangle `width` and `height`, an edge-centre rectangle `length` (its first edge) and `width`; a circle `diameter` or `radius` (not both); an arc by centre `radius` and `sweep`; an ellipse `major_radius` and `minor_radius`; a centre-to-centre, overall or centre-point slot `length` and `width`; a centre arc slot `radius`, `sweep` and `width`; a three-point arc slot `width`; an inscribed or circumscribed polygon `radius`; an edge polygon `radius` or `length` (not both). A role the creator does not take is refused.",
            "type": "object",
            "properties": {
                "length": length,
                "width": length,
                "height": length,
                "diameter": length,
                "radius": length,
                "major_radius": length,
                "minor_radius": length,
                "angle": angle,
                "sweep": angle
            },
            "additionalProperties": length
        })
    }
}

/// Draw a shape and dimension it in one undo step. `command` names the sketch creator and
/// `params` its parameters; `dimensions` gives the sizes to place, by role, as driving
/// dimensions. With `as_drawn` every other size of the shape is placed at its size as drawn
/// (a size the sketch already determines is left out); with `locate` the shape is then placed
/// from the sketch origin with horizontal and vertical distances, only where each takes freedom
/// away. Either one makes `dimensions` optional. A size the sketch already has is placed as a
/// driven (reference) dimension rather than refused.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = draw_required)]
pub struct DrawDimensionedParams {
    /// The sketch creation command to run: one of the listed creators.
    #[serde(default, deserialize_with = "command")]
    #[schemars(with = "Creator")]
    pub command: Option<Creator>,
    /// The creator's own parameters, as it takes them (`sketch` among them, to draw in a sketch
    /// other than the active one). A `sweep` dimension replaces the creator's `sweep` and removes
    /// its `end`.
    #[serde(default, deserialize_with = "creator_params")]
    #[schemars(with = "CreatorParams")]
    pub params: Option<CreatorParams>,
    /// The sizes to place, by role. Required unless `as_drawn` or `locate` is true.
    #[serde(default, deserialize_with = "dimension_values")]
    #[schemars(with = "DimensionValues")]
    pub dimensions: Option<DimensionValues>,
    /// Also dimension every other size of the shape at its size as drawn. A radius or diameter
    /// given takes the place of the other (as an edge polygon's side length does its radius); a
    /// circle otherwise takes its diameter. Default: false.
    #[serde(default)]
    pub as_drawn: Option<bool>,
    /// Then place the shape from the sketch origin until nothing of it is left free: horizontal
    /// and vertical distances from the origin to its points, centres first, each kept only where
    /// it removes freedom, so a shape anchored to the origin or to placed geometry gets none. A
    /// point on an axis goes onto that axis instead of getting a distance of 0. Default: false.
    #[serde(default)]
    pub locate: Option<bool>,
}

fn command<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Creator>, D::Error> {
    Creator::deserialize(d).map(Some)
}

fn creator_params<'de, D: Deserializer<'de>>(d: D) -> Result<Option<CreatorParams>, D::Error> {
    CreatorParams::deserialize(d).map(Some)
}

fn dimension_values<'de, D: Deserializer<'de>>(d: D) -> Result<Option<DimensionValues>, D::Error> {
    DimensionValues::deserialize(d).map(Some)
}

/// `command` and `params` are always needed; `dimensions` unless `as_drawn` or `locate` is true.
fn draw_required(schema: &mut Schema) {
    require(schema, "command");
    require(schema, "params");
    schema.insert(
        "anyOf".into(),
        json!([
            {"required": ["dimensions"]},
            {"required": ["as_drawn"], "properties": {"as_drawn": {"enum": [true]}}},
            {"required": ["locate"], "properties": {"locate": {"enum": [true]}}}
        ]),
    );
}

/// What `sketch.draw.dimensioned` returns: what the creator returned, then the dimensions placed.
#[derive(Debug, Serialize, JsonSchema)]
#[schemars(transform = creator_reply)]
pub struct DrawDimensionedResult {
    /// What the creator returned, as that command returns it (`curves`, `constraints`, `sketch`,
    /// `created`, and for some creators `sides`, `corners`, `centre_arc` or `support_circle`),
    /// except `sketch`: it is the sketch's solve after the dimensions and any `locate` placement,
    /// not the creator's own.
    #[serde(flatten)]
    pub drawn: Map<String, Value>,
    /// The `sketch.dimension` reply of each dimension placed, by role.
    pub dimensions: Map<String, Value>,
    /// The names of the user parameters a typed `name = expression` defined. Only when there
    /// are any.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub defined: Vec<String>,
    /// With `locate`: the `sketch.dimension` or `sketch.constraint.coincident` replies of what
    /// placed the shape, in order (empty when nothing was left to place).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub located: Option<Vec<Value>>,
    /// In the warn parameter-check mode: what the call's parameters got wrong. When the call
    /// itself is fine, the creator's own warnings about `params` are kept here.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

impl DrawDimensionedResult {
    /// The creator's reply `drawn` with the dimensions placed. A key the creator's reply and this
    /// result both have takes this result's value, as the handler always set them over it.
    pub fn new(drawn: Value, dimensions: Map<String, Value>, defined: Vec<String>, located: Option<Vec<Value>>) -> Self {
        let mut drawn = match drawn {
            Value::Object(o) => o,
            _ => Map::new(),
        };
        let mut param_warnings = Vec::new();
        if let Some(Value::Array(warnings)) = drawn.get("param_warnings")
            && warnings.iter().all(Value::is_string)
        {
            param_warnings = warnings.iter().filter_map(|w| w.as_str().map(str::to_string)).collect();
            drawn.remove("param_warnings");
        }
        drawn.remove("dimensions");
        if !defined.is_empty() {
            drawn.remove("defined");
        }
        if located.is_some() {
            drawn.remove("located");
        }
        DrawDimensionedResult { drawn, dimensions, defined, located, param_warnings }
    }
}

/// The keys most creators' replies have, which depend on the creator (none is always there).
fn creator_reply(schema: &mut Schema) {
    let strings = json!({"type": "array", "items": {"type": "string"}});
    let properties = json!({
        "curves": strings,
        "constraints": strings,
        "sketch": {"type": "object", "description": "The sketch's solve after the dimensions and any `locate` placement: `{solved, dof}`."},
        "created": {"type": "object", "description": "What the creator added: `{points, curves, constraints, dimensions}`."},
        "sides": {"type": "object", "description": "A rectangle's sides {bottom, right, top, left}, line ids."},
        "corners": {"type": "object", "description": "A rectangle's corners {bottom_left, bottom_right, top_right, top_left}, point ids."},
        "centre_arc": {"type": "string", "description": "A centre arc slot's middle arc."},
        "support_circle": {"type": "string", "description": "A polygon's construction circle."}
    });
    if let (Some(Value::Object(existing)), Value::Object(extra)) = (schema.get_mut("properties"), properties) {
        for (k, v) in extra {
            existing.entry(k).or_insert(v);
        }
    }
}

#[cfg(test)]
#[path = "sketch_dimensioned_tests.rs"]
mod tests;
