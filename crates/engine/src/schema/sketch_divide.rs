//! Contract and result type for Divide Curve, `cmd/sketch_divide.rs` (#15880).

use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Value, json};

use super::config::NameList;
use super::sketch_ends::SketchTarget;
use super::sketch_shapes::Created;
use super::types::present;

/// Most segments one curve is divided into.
const MAX_SEGMENTS: u64 = 500;

/// How many pieces each curve is divided into: a whole number from 2 to 500, as the handler reads
/// it, with its message.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Segments(pub u64);

impl<'de> Deserialize<'de> for Segments {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match Value::deserialize(d)?.as_f64() {
            Some(n) if n.fract() == 0.0 && (2.0..=MAX_SEGMENTS as f64).contains(&n) => Ok(Segments(n as u64)),
            _ => Err(D::Error::custom(format!("must be a whole number from 2 to {MAX_SEGMENTS}"))),
        }
    }
}

impl JsonSchema for Segments {
    fn schema_name() -> Cow<'static, str> {
        "Segments".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({"description": "A whole number from 2 to 500.", "type": "integer", "minimum": 2, "maximum": MAX_SEGMENTS})
    }
}

/// Marks `segments` required. The type holds it as optional (`#[schemars(required)]` doesn't do
/// this for a key with `serde(default)`), so that a call that leaves it out reaches the handler,
/// which refuses it with its own message.
fn segments_required(schema: &mut Schema) {
    schema.insert("required".into(), json!(["segments"]));
}

/// Place points that divide curves into pieces of equal length.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = segments_required)]
pub struct DivideParams {
    /// The sketch: its feature id or name. Default: the one being edited.
    #[serde(default)]
    pub sketch: Option<SketchTarget>,
    /// A curve to divide, by id. At least one of `curve` and `curves` must name a curve.
    #[serde(default)]
    pub curve: Option<NameList>,
    /// Curves to divide, by id; each is divided on its own, at most 200 in all.
    #[serde(default)]
    pub curves: Option<NameList>,
    /// The pieces each curve is divided into. An open curve gets this many minus one points, a
    /// closed one this many.
    #[serde(default, deserialize_with = "present")]
    #[schemars(with = "Segments")]
    pub segments: Option<Segments>,
}

/// What `sketch.point.divide` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct DivideResult {
    /// The ids of the points added, per curve in order.
    pub points: Vec<String>,
    /// The ids of the constraints that hold them at their share of the length, one per point.
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
#[path = "sketch_divide_tests.rs"]
mod tests;
