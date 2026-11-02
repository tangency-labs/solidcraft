//! Contracts and result types for the commands in `cmd/motion.rs`.

use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

/// Find where each value of a joint can go before the parts it moves touch another body. The
/// design is not changed.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct FindLimitsParams {
    pub joint: JointRef,
    /// Which value (0 for a revolute's angle). Default: every value of the joint.
    #[serde(default)]
    pub index: Option<u64>,
    /// mm: each stop is bisected until no part moves more than this between the last clear value
    /// and the touching one (above 0, at most 1000). Default: 0.01.
    #[serde(default)]
    pub refine: Option<f64>,
    /// Between samples, in the value's unit (degrees for angles, mm for distances; above 0).
    /// Default: fine enough that no part passes through another between two.
    #[serde(default)]
    pub step: Option<f64>,
    /// The parts checked against each other. Default: every occurrence.
    #[serde(default, deserialize_with = "super::present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Vec<PartRef>")]
    pub occurrences: Option<Vec<PartRef>>,
    /// The parts that stay: each of `occurrences` is checked only against these.
    #[serde(default)]
    pub against: Option<Vec<PartRef>>,
    /// Check the pairs of the design's contact sets instead (not with `occurrences`).
    #[serde(default)]
    pub contact_sets: Option<bool>,
}

/// A joint by its id (a number) or its name.
#[derive(Debug)]
pub struct JointRef;

impl<'de> Deserialize<'de> for JointRef {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match Value::deserialize(d)? {
            Value::Number(_) | Value::String(_) => Ok(JointRef),
            _ => Err(D::Error::custom("must be an id or a name")),
        }
    }
}

impl JsonSchema for JointRef {
    fn schema_name() -> Cow<'static, str> {
        "JointRef".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({"description": "A joint's id (a number) or its name.", "type": ["number", "string"]})
    }
}

/// An occurrence by its id (a number) or its name.
#[derive(Debug)]
pub struct PartRef;

impl<'de> Deserialize<'de> for PartRef {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match Value::deserialize(d)? {
            Value::Number(_) | Value::String(_) => Ok(PartRef),
            _ => Err(D::Error::custom("an occurrence must be an id or a name")),
        }
    }
}

impl JsonSchema for PartRef {
    fn schema_name() -> Cow<'static, str> {
        "PartRef".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({"description": "An occurrence's id (a number) or its name.", "type": ["number", "string"]})
    }
}

/// What stopped one side of a sweep.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum Stop {
    /// Two parts touch.
    Contact {
        a: String,
        b: String,
        /// Where they touch (world, mm).
        at: [f64; 3],
    },
    /// `loop`: a closed loop can no longer close; `limit`: the limit of another of the joint's
    /// values, or of another joint through a motion link; `turn`: no contact within one turn, so the side is a turn from the other;
    /// `range`: no contact within the parts' reach (a distance), so the side has no limit.
    Other(Why),
}

/// Why a side stopped short of contact.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Why {
    Loop,
    Limit,
    Turn,
    Range,
}

/// One value's limits.
#[derive(Debug, Serialize, JsonSchema)]
pub struct FoundLimits {
    /// Which value of the joint.
    pub index: usize,
    /// Its name (angle, distance, …).
    pub dof: String,
    /// `deg` or `mm`.
    pub unit: String,
    /// Where it is now, which the search starts from.
    pub value: f64,
    /// The lowest value clear of contact, or `null` (`min_by` is `range`).
    pub min: Option<f64>,
    /// The highest value clear of contact, or `null` (`max_by` is `range`).
    pub max: Option<f64>,
    pub min_by: Stop,
    pub max_by: Stop,
    /// How far each way the search went: a turn (360) for an angle, the parts' reach for a
    /// distance.
    pub range: f64,
}

/// Two parts already passing into each other where the search starts.
#[derive(Debug, Serialize, JsonSchema)]
pub struct Overlap {
    pub a: String,
    pub b: String,
    /// How deep (mm).
    pub depth_mm: f64,
}

/// What `joint.find_limits` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct FindLimitsResult {
    pub joint: u64,
    pub name: String,
    pub values: Vec<FoundLimits>,
    pub refine_mm: f64,
    /// `mesh`: distances between the parts' tessellations.
    pub gap_method: String,
    /// How far those can be off: 0 when every face is flat.
    pub gap_tolerance_mm: f64,
    /// How many pairs of parts are checked.
    pub pairs: usize,
    /// Pairs of parts touching where the search starts: they stop it only once they pass into
    /// each other deeper than they start.
    pub touching_at_start: Vec<[String; 2]>,
    /// Pairs of those already passing into each other, deeper than a touch.
    pub overlapping_at_start: Vec<Overlap>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

#[cfg(test)]
#[path = "motion_tests.rs"]
mod tests;
