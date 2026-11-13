//! Contracts and reply types for the commands in `cmd/sketch_divide.rs`: points that divide
//! sketch curves into equal lengths (#8969, #15880).

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Value, json};

use super::check::whole;
use super::frame::SketchSolve;
use super::motion::require;
use super::sketch_ends::SketchTarget;
use super::sketch_shapes::Created;
use crate::params::{self, num};

/// The most segments one curve is divided into, and the most curves one command divides.
/// `cmd/sketch_divide.rs` takes both from here.
pub(crate) const MAX_SEGMENTS: usize = 500;
pub(crate) const MAX_CURVES: usize = 200;

type Read<T> = Result<T, String>;

/// The message of a handler's refusal.
fn message(e: crate::EngineError) -> String {
    match e {
        crate::EngineError::BadParams { msg, .. } => msg,
        other => other.to_string(),
    }
}

/// `sketch.point.divide`: which curves, and into how many equal lengths each is divided. Which
/// curves the sketch has, whether one of them is an infinite line, how many points the call
/// would add and whether the sketch solves stay the handler's to check.
#[derive(Debug, JsonSchema)]
#[schemars(deny_unknown_fields, transform = divide_required)]
pub struct DivideParams {
    /// The curve to divide, by id. `curves` may name more: the handler joins the two lists,
    /// `curves` first, and divides each curve on its own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "curve_names_schema")]
    pub curve: Option<Vec<String>>,
    /// The curves to divide, at most 200 ids. With `curve`: both are read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "curve_names_schema")]
    pub curves: Option<Vec<String>>,
    /// How many equal lengths each curve is divided into: a whole number from 2 to 500. An open
    /// curve gets `segments` − 1 points, a closed one `segments`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "segments_schema")]
    pub segments: Option<f64>,
    /// The sketch (default: the one being edited).
    #[serde(default)]
    pub sketch: Option<SketchTarget>,
}

impl<'de> Deserialize<'de> for DivideParams {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Self::read(&Value::deserialize(d)?).map_err(|m| D::Error::custom(whole(m)))
    }
}

/// A call names its curves through one of these two keys, and `segments` is the one the handler
/// refuses when left out.
fn divide_required(schema: &mut Schema) {
    require(schema, "segments");
    schema.insert("anyOf".into(), json!([{"required": ["curve"]}, {"required": ["curves"]}]));
}

/// A curve id, or a list of them, as `params::string_list` reads one. The most names is what
/// the handler refuses more of; `null` names none, which is why it is allowed here and refused
/// by the reader below, with the handler's message.
fn curve_names_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "description": "A curve id, or a list of curve ids. `null` names none.",
        "anyOf": [
            {"type": "string"},
            {"type": "array", "items": {"type": "string"}, "maxItems": MAX_CURVES},
            {"type": "null"}
        ]
    })
}

/// A whole number in range. The range is what the handler refuses outside it; being whole is
/// what its message asks for, which a number schema cannot say.
fn segments_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "description": "A whole number of equal lengths, from 2 to 500.",
        "type": "number",
        "minimum": 2,
        "maximum": MAX_SEGMENTS
    })
}

/// `key` as `params::string_list` reads it: one id or a list of ids, `null` for none, with its
/// messages; `None` when the key is left out.
fn names(p: &Value, key: &str) -> Read<Option<Vec<String>>> {
    match p.get(key) {
        None => Ok(None),
        Some(_) => params::string_list("", p, key).map(Some).map_err(message),
    }
}

impl DivideParams {
    /// The call as the handler reads it, in its order, so a refusal is the handler's message.
    fn read(p: &Value) -> Read<Self> {
        let curves = names(p, "curves")?;
        let curve = names(p, "curve")?;
        let named = curves.as_ref().map_or(0, Vec::len) + curve.as_ref().map_or(0, Vec::len);
        if named == 0 {
            return Err("`curve` (or `curves`) must name the curves to divide".to_string());
        }
        if named > MAX_CURVES {
            return Err(format!("at most {MAX_CURVES} curves at once"));
        }
        let segments = match p.get("segments") {
            None => None,
            Some(_) => num(p, "segments")
                .filter(|n| n.fract() == 0.0 && *n >= 2.0 && *n <= MAX_SEGMENTS as f64)
                .map(Some)
                .ok_or_else(|| format!("`segments` must be a whole number from 2 to {MAX_SEGMENTS}"))?,
        };
        let sketch = match p.get("sketch") {
            None | Some(Value::Null) => None,
            Some(Value::Number(n)) if n.is_u64() => Some(SketchTarget(n.to_string())),
            Some(Value::String(s)) => Some(SketchTarget(s.clone())),
            Some(_) => return Err("`sketch` must be an id or a name".to_string()),
        };
        Ok(DivideParams { curve, curves, segments, sketch })
    }
}

/// What `sketch.point.divide` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct DivideResult {
    /// The new points' ids: each curve's points in order along it, the curves in the order they
    /// were named.
    pub points: Vec<String>,
    /// The constraints that hold each point at its share of its curve, in the same order.
    pub constraints: Vec<String>,
    /// The sketch's solve: `{solved, dof}`, or null when the edit did not solve it again.
    pub sketch: Option<SketchSolve>,
    /// Everything the call added to the sketch: `{points, curves, constraints, dimensions}`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created: Option<Created>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

#[cfg(test)]
#[path = "sketch_divide_tests.rs"]
mod tests;
