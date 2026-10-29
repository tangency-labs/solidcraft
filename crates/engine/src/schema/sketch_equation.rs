//! Contracts and result types for the commands in `cmd/sketch_equation.rs`.

use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Deserializer, Serialize, de::Error as _};
use serde_json::Value;

use super::types::{SketchRef, present};

/// A formula or a range end: an expression (text) or a number.
#[derive(Clone, Debug, PartialEq)]
pub struct Formula(pub String);

impl<'de> Deserialize<'de> for Formula {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match Value::deserialize(d)? {
            Value::String(s) if !s.trim().is_empty() => Ok(Formula(s)),
            Value::Number(n) => match n.as_f64().and_then(crate::params::finite) {
                Some(v) => Ok(Formula(format!("{v}"))),
                None => Err(D::Error::custom("must be a finite number")),
            },
            _ => Err(D::Error::custom("must be a formula (a string) or a number")),
        }
    }
}

impl JsonSchema for Formula {
    fn schema_name() -> Cow<'static, str> {
        "Formula".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "An expression in the design's expression language and parameters, such as \"r * cos(t)\" or \"2 * pi\", or a number.",
            "anyOf": [
                {"type": "number", "exclusiveMinimum": -1e9, "exclusiveMaximum": 1e9},
                {"type": "string", "minLength": 1, "maxLength": 1024, "pattern": "\\S"}
            ]
        })
    }
}

/// How far (mm) the curve may stray from its formulas.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tolerance(pub f64);

impl<'de> Deserialize<'de> for Tolerance {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Value::deserialize(d)?
            .as_f64()
            .filter(|t| t.is_finite() && (1e-4..=10.0).contains(t))
            .map(Tolerance)
            .ok_or_else(|| D::Error::custom("must be a number of mm from 0.0001 to 10"))
    }
}

impl JsonSchema for Tolerance {
    fn schema_name() -> Cow<'static, str> {
        "EquationTolerance".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "How far (mm) the curve may stray from its formulas: 0.0001 to 10. Default 0.01.",
            "type": "number",
            "minimum": 0.0001,
            "maximum": 10.0
        })
    }
}

/// An equation curve's id.
#[derive(Clone, Debug, PartialEq)]
pub struct CurveId(pub String);

impl<'de> Deserialize<'de> for CurveId {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match Value::deserialize(d)? {
            Value::String(s) => Ok(CurveId(s)),
            _ => Err(D::Error::custom("must be an equation curve's id")),
        }
    }
}

impl JsonSchema for CurveId {
    fn schema_name() -> Cow<'static, str> {
        "EquationCurveId".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({"description": "An equation curve's id (a curve id of the sketch).", "type": "string"})
    }
}

/// Draw a curve from formulas: x(t) and y(t) over a range of t, or y = f(x) over a range of x.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct EquationCurveParams {
    /// x(t), a formula of t. Leave it out with `explicit: true`.
    #[serde(default)]
    pub x: Option<Formula>,
    /// y(t), a formula of t; or f(x), a formula of x, with `explicit: true`. Required.
    #[serde(default)]
    pub y: Option<Formula>,
    /// The explicit form, y = f(x): `y` is a formula of x and `x` is left out. Default false.
    #[serde(default)]
    pub explicit: Option<bool>,
    /// Where the variable starts: a number, an angle ("360 deg" is 2·pi) or a length (in the
    /// design's unit). Required.
    #[serde(default)]
    pub start: Option<Formula>,
    /// Where the variable ends, as `start`. Required.
    #[serde(default)]
    pub end: Option<Formula>,
    /// How far (mm) the curve may stray from its formulas. Default 0.01.
    #[serde(default)]
    pub tolerance: Option<Tolerance>,
    /// Make it construction geometry. Default false.
    #[serde(default)]
    pub construction: Option<bool>,
    /// The sketch (default: the one being edited).
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "SketchRef")]
    pub sketch: Option<SketchRef>,
}

/// Change an equation curve's formulas, range or tolerance.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct EquationEditParams {
    /// The equation curve. Required.
    #[serde(default)]
    pub curve: Option<CurveId>,
    /// x(t). Kept when left out; `explicit: true` drops it.
    #[serde(default)]
    pub x: Option<Formula>,
    /// y(t), or f(x) for an explicit curve. Kept when left out.
    #[serde(default)]
    pub y: Option<Formula>,
    /// The explicit form, y = f(x). Kept when left out; false needs `x` (given or kept).
    #[serde(default)]
    pub explicit: Option<bool>,
    /// Where the variable starts. Kept when left out.
    #[serde(default)]
    pub start: Option<Formula>,
    /// Where the variable ends. Kept when left out.
    #[serde(default)]
    pub end: Option<Formula>,
    /// How far (mm) the curve may stray from its formulas. Kept when left out.
    #[serde(default)]
    pub tolerance: Option<Tolerance>,
    /// The sketch (default: the one being edited).
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "SketchRef")]
    pub sketch: Option<SketchRef>,
}

/// What `sketch.equation_curve` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct EquationCurveResult {
    /// The new curve's id, as a list (as other drawing commands return it).
    pub curves: Vec<String>,
    /// The new curve's id.
    pub curve: String,
    /// How many points the spline goes through (a closed curve's start counted once).
    pub points: usize,
    /// The curve ends where it starts and closes smoothly there.
    pub closed: bool,
    /// The largest distance (mm) found between the spline and the formulas.
    pub deviation: f64,
    /// The sketch's solve: `{solved, dof}`.
    pub sketch: Value,
    /// What it added: `{points, curves, constraints, dimensions}`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created: Option<Value>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What `sketch.equation_curve.edit` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct EquationEditResult {
    /// The curve's id (unchanged).
    pub curve: String,
    /// How many points the spline now goes through (a closed curve's start counted once).
    pub points: usize,
    /// The curve ends where it starts and closes smoothly there.
    pub closed: bool,
    /// The largest distance (mm) found between the spline and the formulas.
    pub deviation: f64,
    /// The sketch's solve: `{solved, dof}`.
    pub sketch: Value,
    /// What the edit added (the new points in between): `{points, curves, constraints, dimensions}`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created: Option<Value>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

#[cfg(test)]
#[path = "sketch_equation_tests.rs"]
mod tests;
