//! Contract and result types for `inspect.parameter_sweep` in `cmd/parameter_sweep.rs` (#10977).

use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

use super::check::whole;
use super::types::{not_blank_pattern, present};

/// Values swept by one command, at most.
pub const MAX_VALUES: usize = 200;

/// Rebuild the design in a copy at each value of a parameter and report what fails, turns over
/// or loses its reference there, against the design as it is. The design itself isn't changed.
/// Give `values`, or `from` and `to`.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct ParameterSweepParams {
    /// The parameter, by name, as `parameters.list` lists it.
    #[serde(default, deserialize_with = "parameter_name")]
    #[schemars(required)]
    pub parameter: Option<String>,
    #[serde(default, deserialize_with = "present")]
    #[schemars(with = "SweepValues")]
    pub values: Option<SweepValues>,
    /// The first value of an even sweep (used only with `to`).
    #[serde(default, deserialize_with = "present")]
    #[schemars(with = "SweepBound")]
    pub from: Option<SweepBound>,
    /// The last value of an even sweep (used only with `from`).
    #[serde(default, deserialize_with = "present")]
    #[schemars(with = "SweepBound")]
    pub to: Option<SweepBound>,
    /// With `from` and `to`: how many steps between them (default 10), so `steps + 1` values.
    #[serde(default, deserialize_with = "steps")]
    #[schemars(schema_with = "steps_schema")]
    pub steps: Option<f64>,
}

/// The parameter's name, with the handler's message for anything that isn't text.
fn parameter_name<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    match Value::deserialize(d)? {
        Value::String(s) => Ok(Some(s)),
        _ => Err(D::Error::custom(whole("`parameter` is required (parameters.list)"))),
    }
}

/// `steps`, as the handler reads it: `null` is none, else a whole number 1 to 199.
fn steps<'de, D: Deserializer<'de>>(d: D) -> Result<Option<f64>, D::Error> {
    match Value::deserialize(d)? {
        Value::Null => Ok(None),
        v => v
            .as_f64()
            .filter(|k| k.fract() == 0.0 && *k >= 1.0 && *k < MAX_VALUES as f64)
            .map(Some)
            .ok_or_else(|| D::Error::custom(format!("must be a whole number 1…{}", MAX_VALUES - 1))),
    }
}

fn steps_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type": ["number", "null"], "minimum": 1, "maximum": MAX_VALUES - 1, "multipleOf": 1})
}

/// The values to sweep: 1 to 200, each a number or an expression (1 to 1000 bytes, not blank),
/// with the handler's messages. Each is set as the parameter's expression in turn.
#[derive(Clone, Debug, PartialEq)]
pub struct SweepValues(pub Vec<Value>);

impl<'de> Deserialize<'de> for SweepValues {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let Value::Array(list) = Value::deserialize(d)? else {
            return Err(D::Error::custom(whole("give `values`, or `from` and `to`")));
        };
        if list.is_empty() || list.len() > MAX_VALUES {
            return Err(D::Error::custom(whole(format!("`values` must list 1…{MAX_VALUES} values"))));
        }
        let usable = |v: &Value| match v {
            Value::Number(n) => n.as_f64().is_some_and(f64::is_finite),
            Value::String(x) => !x.trim().is_empty() && x.len() <= 1000,
            _ => false,
        };
        if !list.iter().all(usable) {
            return Err(D::Error::custom(whole("each value is a number or an expression")));
        }
        Ok(SweepValues(list))
    }
}

impl JsonSchema for SweepValues {
    fn schema_name() -> Cow<'static, str> {
        "SweepValues".into()
    }
    fn inline_schema() -> bool {
        true
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "The values to sweep, each a number or an expression (at most 1000 bytes, not blank), set as the parameter's expression in turn.",
            "type": "array",
            "items": {"anyOf": [{"type": "number"}, {"type": "string", "minLength": 1, "maxLength": 1000, "pattern": not_blank_pattern()}]},
            "minItems": 1,
            "maxItems": MAX_VALUES
        })
    }
}

/// An end of an even sweep: a number at most 1e9 in size, with the handler's message.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SweepBound(pub f64);

impl<'de> Deserialize<'de> for SweepBound {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Value::deserialize(d)?
            .as_f64()
            .filter(|x| x.is_finite() && x.abs() <= 1e9)
            .map(SweepBound)
            .ok_or_else(|| D::Error::custom("must be a number"))
    }
}

impl JsonSchema for SweepBound {
    fn schema_name() -> Cow<'static, str> {
        "SweepBound".into()
    }
    fn inline_schema() -> bool {
        true
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({"type": "number", "minimum": -1e9, "maximum": 1e9})
    }
}

/// What `inspect.parameter_sweep` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ParameterSweep {
    /// The parameter swept.
    pub parameter: String,
    /// One result per value, in order.
    pub results: Vec<SweepResult>,
    /// How many values show each kind of change.
    pub summary: SweepSummary,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// The design at one value: what changed against the design as it is, or why the parameter
/// takes no such value.
#[derive(Debug, Serialize, JsonSchema)]
pub struct SweepResult {
    /// The value: the number or expression given, or the number an even sweep reached.
    #[schemars(schema_with = "value_schema")]
    pub value: Value,
    /// How long the rebuild took (ms).
    pub ms: f64,
    /// Whether nothing failed, was lost or turned over at this value.
    pub ok: bool,
    /// Why the parameter takes no such value; then there are no changes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// What changed, when the parameter took the value.
    #[serde(flatten)]
    pub changes: Option<SweepChanges>,
}

fn value_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"anyOf": [{"type": "number"}, {"type": "string"}]})
}

/// What changed at one value against the design as it is.
#[derive(Debug, Default, Serialize, JsonSchema)]
pub struct SweepChanges {
    /// Features that build now and fail at this value.
    pub failed: Vec<FeatureFailed>,
    /// Features that warn at this value, with a warning they don't have now.
    pub warned: Vec<FeatureWarned>,
    /// Joints that lose an origin, or that the joint solve can no longer apply.
    pub joints_lost: Vec<JointLost>,
    /// Joints with an origin whose z axis turned more than 90° on its own part.
    pub joints_flipped: Vec<JointFlipped>,
    /// Sketches whose plane's normal turned more than 90°.
    pub sketches_flipped: Vec<SketchFlipped>,
    /// Sketches whose solve fails, or keeps a triangle's turn or an offset's side only by
    /// flipping it, where it doesn't now.
    pub sketches_failed: Vec<SketchFailed>,
}

/// A feature that fails at this value.
#[derive(Debug, Serialize, JsonSchema)]
pub struct FeatureFailed {
    pub feature: u64,
    pub name: String,
    pub error: String,
}

/// A feature that warns at this value.
#[derive(Debug, Serialize, JsonSchema)]
pub struct FeatureWarned {
    pub feature: u64,
    pub name: String,
    pub warning: String,
}

/// Which origin of a joint.
#[derive(Clone, Copy, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum JointSide {
    A,
    B,
}

/// A joint that loses an origin (`origin`), or that the joint solve can no longer apply (no
/// `origin`).
#[derive(Debug, Serialize, JsonSchema)]
pub struct JointLost {
    pub joint: u64,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub origin: Option<JointSide>,
    pub why: String,
}

/// A joint origin whose z axis turned over.
#[derive(Debug, Serialize, JsonSchema)]
pub struct JointFlipped {
    pub joint: u64,
    pub name: String,
    pub origin: JointSide,
    /// How far it turned, to 0.1°.
    pub degrees: f64,
}

/// A sketch whose plane turned over.
#[derive(Debug, Serialize, JsonSchema)]
pub struct SketchFlipped {
    pub sketch: u64,
    pub name: String,
    /// How far its normal turned, to 0.1°.
    pub degrees: f64,
}

/// A sketch whose solve fails or folds at this value.
#[derive(Debug, Serialize, JsonSchema)]
pub struct SketchFailed {
    pub sketch: u64,
    pub name: String,
    /// The constraints its solve can't meet.
    pub failing: Vec<String>,
    /// The constraints it meets only by flipping a triangle's turn or an offset's side.
    pub folded: Vec<String>,
}

/// How many values show each kind of change.
#[derive(Debug, Serialize, JsonSchema)]
pub struct SweepSummary {
    /// How many values were swept.
    pub values: usize,
    /// How many were ok.
    pub ok: usize,
    /// The slowest rebuild (ms).
    pub slowest_ms: f64,
    pub failed: usize,
    pub joints_lost: usize,
    pub joints_flipped: usize,
    pub sketches_flipped: usize,
    pub sketches_failed: usize,
}

#[cfg(test)]
#[path = "parameter_sweep_tests.rs"]
mod tests;
