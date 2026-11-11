//! Contracts and result types for the commands in `cmd/sketch_ends.rs`: a sketch's loose ends,
//! and tying them (#10959).

use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

use super::check::whole;

/// The most names `params::string_list` takes, which the tie command reads its points with.
const MAX_POINTS: usize = 10_000;

/// A sketch, as `sketch_store::target` reads one: its feature id or its name, with its message.
/// Whether a sketch has that id or name is the handler's to check.
#[derive(Clone, Debug, PartialEq)]
pub struct SketchTarget(pub String);

impl<'de> Deserialize<'de> for SketchTarget {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match Value::deserialize(d)? {
            Value::Number(n) if n.is_u64() => Ok(SketchTarget(n.to_string())),
            Value::String(s) => Ok(SketchTarget(s)),
            _ => Err(D::Error::custom("must be an id or a name")),
        }
    }
}

impl JsonSchema for SketchTarget {
    fn schema_name() -> Cow<'static, str> {
        "SketchTarget".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "The sketch: its feature id or its name (default: the one being edited).",
            "anyOf": [{"type": "integer", "minimum": 0}, {"type": "string"}]
        })
    }
}

/// Point ids, as the tie command reads them: a list of ids (a single id isn't taken), at most
/// 10 000, with its messages.
#[derive(Clone, Debug, PartialEq)]
pub struct PointIds(pub Vec<String>);

impl<'de> Deserialize<'de> for PointIds {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let fail = || D::Error::custom("must be a list of point ids");
        let Value::Array(a) = Value::deserialize(d)? else {
            return Err(fail());
        };
        let ids = a.into_iter().map(|v| if let Value::String(s) = v { Some(s) } else { None }).collect::<Option<Vec<_>>>().ok_or_else(fail)?;
        if ids.len() > MAX_POINTS {
            return Err(D::Error::custom(whole(format!("`points` must list at most {} names", MAX_POINTS))));
        }
        Ok(PointIds(ids))
    }
}

impl JsonSchema for PointIds {
    fn schema_name() -> Cow<'static, str> {
        "PointIds".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "Point ids, such as `p3`.",
            "type": "array",
            "items": {"type": "string"},
            "maxItems": MAX_POINTS
        })
    }
}

/// The ends of a sketch's profile curves (not construction, projected or text curves) that can
/// make a profile fail.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct LooseEndsParams {
    #[serde(default)]
    pub sketch: Option<SketchTarget>,
}

/// Tie each untied end to what it touches: coincident with another curve's end, or on another
/// curve. One undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct TieLooseEndsParams {
    #[serde(default)]
    pub sketch: Option<SketchTarget>,
    /// The untied ends to tie, by point id (default: every untied end). An id that isn't an
    /// untied end is left as it is.
    #[serde(default)]
    pub points: Option<PointIds>,
}

/// What `sketch.loose_ends` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct LooseEnds {
    /// Ends that touch no other profile curve: no profile runs through them.
    pub open: Vec<OpenEnd>,
    /// Ends that touch another curve only by position: the profile closes now, but nothing keeps
    /// it closed when a dimension changes.
    pub untied: Vec<UntiedEnd>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// An end that touches nothing.
#[derive(Debug, Serialize, JsonSchema)]
pub struct OpenEnd {
    /// The end's point id.
    pub point: Option<String>,
    /// The profile curve it ends.
    pub curve: Option<String>,
    /// Where it is, in sketch coordinates.
    pub at: Option<[f64; 2]>,
}

/// An end that touches another curve only by position.
#[derive(Debug, Serialize, JsonSchema)]
pub struct UntiedEnd {
    /// The end's point id.
    pub point: Option<String>,
    /// The curve it ends.
    pub curve: Option<String>,
    /// Where it is, in sketch coordinates.
    pub at: Option<[f64; 2]>,
    /// What it touches: another curve's end, or a curve.
    pub touches: Touches,
}

/// What an untied end touches.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Touches {
    /// Another curve's end, by point id.
    Point(Option<String>),
    /// A curve, by id.
    Curve(Option<String>),
}

/// What `sketch.loose_ends.tie` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct TiedLooseEnds {
    /// The ids of the constraints added, one per end tied.
    pub constraints: Vec<String>,
    /// The sketch's solve: `{solved, dof}`.
    pub sketch: Value,
    /// The untied ends left.
    pub untied: Vec<UntiedEnd>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

#[cfg(test)]
#[path = "sketch_ends_tests.rs"]
mod tests;
