//! Contract and reply type for Sweep Joint (`joint.sweep`, #12844), whose handler is `sweep` in
//! `cmd/motion_sweep.rs`. It drives one value of a joint from `from` toward `to` on a copy of the
//! design and stops at the first contact between the parts it checks.
//!
//! It takes the keys of Find Joint Limits (`FindLimitsParams` in `schema/motion.rs`), whose
//! `JointRef`, `PartRef` and `Stop` it reuses, plus `from`, `to`, `report_gaps` and `apply`. The
//! handler reads its parameters from the raw JSON, so `from`, `to`, `refine` and `step` read here
//! as it reads them and fail with its message. Whether the joint exists and moves, and whether
//! there are two parts to check, stay the handler's to check.

use std::borrow::Cow;
use std::collections::BTreeMap;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

use super::check::whole;
use super::motion::{JointRef, PartRef, Stop, require};

/// The largest `refine` the handler takes, in mm (`positive(p, "refine", 1e3, …)`).
const MAX_REFINE_MM: f64 = 1e3;
/// The largest `step` the handler takes, in the value's unit (`positive(p, "step", 1e9, …)`).
const MAX_STEP: f64 = 1e9;

/// Drive one value of a joint from `from` toward `to` on a copy of the design, and stop at the
/// first contact between the parts checked (bisected to `refine`), where a closed loop can't
/// close, or at a limit. Closed loops follow; contact sets don't stop it. The design is not
/// changed unless `apply` is true.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = from_and_to_required)]
pub struct SweepParams {
    pub joint: JointRef,
    /// Which value (0 for a revolute's angle). Default: 0.
    #[serde(default)]
    pub index: Option<u64>,
    // `from` and `to` are optional in the type and required in the schema
    // (`from_and_to_required`): left out, the handler refuses the call with its own message,
    // which names both, not the check's "is required".
    /// Where the sweep starts.
    #[serde(default, deserialize_with = "joint_value", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "JointValue")]
    pub from: Option<JointValue>,
    /// Where the sweep heads.
    #[serde(default, deserialize_with = "joint_value", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "JointValue")]
    pub to: Option<JointValue>,
    /// mm: the first contact is bisected until no part moves more than this between the last
    /// clear value and the touching one (above 0, at most 1000). Default: 0.01.
    #[serde(default, deserialize_with = "refine")]
    #[schemars(schema_with = "refine_schema")]
    pub refine: Option<f64>,
    /// Between samples, in the value's unit (degrees for angles, mm for distances; above 0, at
    /// most 1e9). Default: fine enough that no part passes through another between two.
    #[serde(default, deserialize_with = "step")]
    #[schemars(schema_with = "step_schema")]
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
    /// Report the nearest gap at every sample, and the nearest each part came to another
    /// (`min_gap`). Default: false.
    #[serde(default)]
    pub report_gaps: Option<bool>,
    /// Leave the design where the sweep stopped, in one undo step. Default: false: the design
    /// is not changed.
    #[serde(default)]
    pub apply: Option<bool>,
}

fn from_and_to_required(schema: &mut Schema) {
    require(schema, "from");
    require(schema, "to");
}

/// A joint value: a number (degrees for an angle, mm for a distance) or an expression, as
/// `cmd/joints.rs`'s `value` reads it. Whether an expression evaluates is the handler's to check.
#[derive(Debug)]
pub struct JointValue;

impl JsonSchema for JointValue {
    fn schema_name() -> Cow<'static, str> {
        "JointValue".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "A joint value: a number, in degrees for an angle and in mm for a distance, smaller than 1e9 in size, \
                or an expression (`\"90 deg\"`, `\"Opening\"`).",
            "type": ["number", "string"],
            "exclusiveMinimum": -1e9,
            "exclusiveMaximum": 1e9,
        })
    }
}

/// `from` or `to`: a number smaller than 1e9 in size, or text, with the handler's message for
/// anything else, `null` included.
fn joint_value<'de, D: Deserializer<'de>>(d: D) -> Result<Option<JointValue>, D::Error> {
    let v = Value::deserialize(d)?;
    let taken = match &v {
        Value::Number(n) => n.as_f64().is_some_and(|x| x.is_finite() && x.abs() < 1e9),
        Value::String(_) => true,
        _ => false,
    };
    if taken { Ok(Some(JointValue)) } else { Err(D::Error::custom(whole(format!("a joint value {}", crate::params::expr_problem(Some(&v)))))) }
}

/// A number above 0, at most `max`, or `null` (left out), as the handler's `positive` reads it,
/// with its message (the check names the key).
fn positive<E: serde::de::Error>(v: Value, max: f64) -> Result<Option<f64>, E> {
    match v {
        Value::Null => Ok(None),
        v => v
            .as_f64()
            .filter(|x| x.is_finite() && *x > 0.0 && *x <= max)
            .map(Some)
            .ok_or_else(|| E::custom(format!("must be a number above 0, at most {max}"))),
    }
}

fn refine<'de, D: Deserializer<'de>>(d: D) -> Result<Option<f64>, D::Error> {
    positive(Value::deserialize(d)?, MAX_REFINE_MM)
}

fn step<'de, D: Deserializer<'de>>(d: D) -> Result<Option<f64>, D::Error> {
    positive(Value::deserialize(d)?, MAX_STEP)
}

/// A number above 0, at most `max`, or `null`.
fn positive_schema(description: &str, max: f64) -> Schema {
    json_schema!({
        "description": description,
        "type": ["number", "null"],
        "exclusiveMinimum": 0,
        "maximum": max,
    })
}

fn refine_schema(_: &mut SchemaGenerator) -> Schema {
    positive_schema(
        "mm: the first contact is bisected until no part moves more than this between the last clear value and the touching one \
         (above 0, at most 1000). Default: 0.01.",
        MAX_REFINE_MM,
    )
}

fn step_schema(_: &mut SchemaGenerator) -> Schema {
    positive_schema(
        "Between samples, in the value's unit (degrees for angles, mm for distances; above 0, at most 1e9). Default: fine enough \
         that no part passes through another between two.",
        MAX_STEP,
    )
}

/// The nearest gap at one sample, with `report_gaps`.
#[derive(Debug, Serialize, JsonSchema)]
pub struct SampleGap {
    /// The smallest gap between two parts checked, in mm, or `null` when no two come near.
    pub min_gap_mm: Option<f64>,
    /// The two parts with that gap, or `null`.
    pub pair: Option<[String; 2]>,
}

/// One sample of the sweep.
#[derive(Debug, Serialize, JsonSchema)]
pub struct SweepSample {
    /// The joint value, in `unit`.
    pub value: f64,
    /// With `report_gaps` only.
    #[serde(flatten)]
    pub gap: Option<SampleGap>,
}

/// The nearest one part came to another along the sweep.
#[derive(Debug, Serialize, JsonSchema)]
pub struct NearestGap {
    /// The gap, in mm.
    pub mm: f64,
    /// The joint value where it was, in `unit`.
    pub at_value: f64,
    /// The other part.
    pub to: String,
}

/// What `joint.sweep` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct SweepResult {
    /// The joint's id.
    pub joint: u64,
    pub name: String,
    /// Which value was swept.
    pub index: usize,
    /// `deg` for an angle, `mm` for a distance: the unit of `from`, `to`, `stopped_at` and the
    /// samples.
    pub unit: &'static str,
    pub from: f64,
    pub to: f64,
    /// The last value clear of contact, or where a loop or limit stopped it; `null`: it reached
    /// `to`.
    pub stopped_at: Option<f64>,
    /// What stopped it (two parts touching, a loop or a limit), or `null`.
    pub stopped_by: Option<Stop>,
    /// The values tested, `from` excluded, before the bisection.
    pub samples: Vec<SweepSample>,
    pub refine_mm: f64,
    /// `mesh`: distances between the parts' tessellations.
    pub gap_method: &'static str,
    /// How far those can be off: 0 when every face is flat.
    pub gap_tolerance_mm: f64,
    /// How many pairs of parts were checked.
    pub pairs: usize,
    /// Pairs of parts touching at `from`, such as parts joined face to face: they stop it only
    /// once they pass into each other deeper than they start.
    pub touching_at_start: Vec<[String; 2]>,
    /// Whether the design was left where the sweep stopped (`apply`, and it moved).
    pub applied: bool,
    /// With `report_gaps` only: by part name, the nearest it came to another.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_gap: Option<BTreeMap<String, NearestGap>>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

#[cfg(test)]
#[path = "joint_sweep_tests.rs"]
mod tests;
