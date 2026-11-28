//! Contracts and reply types for Motion Trace, Keep Motion Trace and Edit Motion Trace in
//! `cmd/motion.rs` and `cmd/motion_trace.rs` (part of #2829). A trace is the path a point on a
//! part takes while a motion study plays or one joint value is driven.
//!
//! The handlers read their parameters from the raw JSON, so each key here reads as its handler
//! reads it and fails with the handler's message. Whether the occurrence, study, joint or sketch
//! exists, whether `index` is below the joint's value count and whether `point_ref` resolves to
//! the traced point stay the handlers' to check. Two keys differ on purpose: an `index` that is
//! not a whole number of 0 or more, and a `sketch` that is neither an id nor a name, are refused
//! by the check in its own words. Where a handler ignores a key of the wrong type (a `name` that
//! isn't text, an `auto_update` that isn't true or false), the contract refuses it.

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

use solidcraft_geom::Vec3Geom as _;

use super::check::whole;
use super::joints::{JointOriginParam, JointValue};
use super::motion::{JointRef, require, some_study, study_ref_schema};
use super::motion_play::{ContactStop, LoopStop};

/// The most samples a trace takes, as `cmd/motion.rs`'s `MAX_TRACE`.
const MAX_SAMPLES: f64 = 500.0;
/// The most ids an occurrence path lists.
const MAX_PATH: usize = 64;
/// A traced point is read only while smaller than this in size (mm).
const MAX_POINT: f64 = 1e7;

fn occurrence_and_point_required(schema: &mut Schema) {
    require(schema, "occurrence");
    require(schema, "point");
}

fn sketch_required(schema: &mut Schema) {
    require(schema, "sketch");
}

/// The motion a trace samples: a study played from start to end, or one joint value driven from
/// `from` to `to`. Give `study` or `joint`, not both.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct TraceMotionParams {
    #[serde(default, deserialize_with = "some_study", skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "study_ref_schema")]
    pub study: Option<Value>,
    /// The joint to drive, with `from` and `to`.
    #[serde(default, deserialize_with = "some_joint", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "JointRef")]
    pub joint: Option<Value>,
    /// With `joint`: the value the motion starts at.
    #[serde(default, deserialize_with = "some_joint_value", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "JointValue")]
    pub from: Option<Value>,
    /// With `joint`: the value the motion ends at.
    #[serde(default, deserialize_with = "some_joint_value", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "JointValue")]
    pub to: Option<Value>,
    /// With `joint`: which of its values is driven (0 for a revolute's angle), below the joint's
    /// value count. Default: 0.
    #[serde(default)]
    pub index: Option<u64>,
    #[serde(default, deserialize_with = "some_samples", skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "samples_schema")]
    pub samples: Option<f64>,
}

/// Trace a point on a part as a motion study plays or a joint is driven, on a copy: the design is
/// not changed.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = occurrence_and_point_required)]
pub struct TraceParams {
    // `occurrence` and `point` are optional in the type and required in the schema: left out,
    // the handler refuses the call with its own message, not the check's "is required".
    #[serde(default, deserialize_with = "some_occurrence", skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "occurrence_schema")]
    pub occurrence: Option<Value>,
    #[serde(default, deserialize_with = "some_point", skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "point_schema")]
    pub point: Option<Value>,
    #[serde(flatten)]
    pub motion: TraceMotionParams,
}

/// Keep a trace as a spline in a new 3D sketch of the root component. One undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = occurrence_and_point_required)]
pub struct KeepTraceParams {
    // Required in the schema, optional in the type, as Motion Trace's.
    #[serde(default, deserialize_with = "some_occurrence", skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "occurrence_schema")]
    pub occurrence: Option<Value>,
    #[serde(default, deserialize_with = "some_point", skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "point_schema")]
    pub point: Option<Value>,
    #[serde(flatten)]
    pub motion: TraceMotionParams,
    /// The new sketch's name; blank text is read as no name. Default: "Motion Trace" and the
    /// first number no feature's name has.
    #[serde(default)]
    pub name: Option<String>,
    /// True: the spline follows later edits of the mechanism and of the motion. False: a fixed
    /// curve. Default: true.
    #[serde(default)]
    pub auto_update: Option<bool>,
    /// With `auto_update`: the traced point as a joint origin snap on the traced occurrence, kept
    /// in place of `point` (it must resolve to `point`). Default: `point` in the occurrence's
    /// coordinates.
    #[serde(default)]
    pub point_ref: Option<JointOriginParam>,
}

/// Change a kept associative trace's motion or sample count, or detach it, in one undo step.
/// `study` or `joint` replaces the motion; `from`, `to` and `index` alone change a joint motion's.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = sketch_required)]
pub struct EditTraceParams {
    // Required in the schema, optional in the type: left out, the handler says "give a sketch".
    #[serde(default, deserialize_with = "some_sketch", skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "sketch_schema")]
    pub sketch: Option<Value>,
    #[serde(flatten)]
    pub motion: TraceMotionParams,
    /// False: the trace stops following, and its spline stays a fixed curve. Default: true.
    #[serde(default)]
    pub auto_update: Option<bool>,
}

/// `occurrence`: an id or a name (0 or "root" is the root), or an occurrence path of ids.
fn some_occurrence<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Value>, D::Error> {
    match Value::deserialize(d)? {
        Value::Array(ids) if ids.len() > MAX_PATH => Err(D::Error::custom(whole(format!("an occurrence path has at most {MAX_PATH} ids")))),
        Value::Array(ids) if ids.iter().any(|v| v.as_u64().is_none()) => Err(D::Error::custom(whole("an occurrence path lists occurrence ids"))),
        v @ (Value::Number(_) | Value::String(_) | Value::Array(_)) => Ok(Some(v)),
        _ => Err(D::Error::custom(whole("an occurrence must be an id or a name"))),
    }
}

fn occurrence_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "description": "The part the point is on: an occurrence's id (a number) or name (0 or \"root\": the root), at its first placement, or an occurrence path of ids (one instance inside sub-assemblies, at most 64).",
        "anyOf": [
            {"type": ["number", "string"]},
            {"type": "array", "items": {"type": "integer", "minimum": 0}, "maxItems": MAX_PATH}
        ]
    })
}

/// `point`: `[x, y, z]` or `{x, y, z}` of finite numbers, smaller than 1e7 mm in size.
fn some_point<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Value>, D::Error> {
    let v = Value::deserialize(d)?;
    match crate::params::vec3(&v) {
        Some(at) if at.len() < MAX_POINT => Ok(Some(v)),
        _ => Err(D::Error::custom(whole("`point` must be [x, y, z], a world point"))),
    }
}

fn point_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "description": "The traced point, in world coordinates (mm) where the parts are placed now: [x, y, z] or {x, y, z}, smaller than 1e7 mm from the origin.",
        "anyOf": [
            {"type": "array", "items": {"type": "number"}, "minItems": 3, "maxItems": 3},
            {"type": "object", "properties": {"x": {"type": "number"}, "y": {"type": "number"}, "z": {"type": "number"}}, "required": ["x", "y", "z"]}
        ]
    })
}

/// `joint`: an id or a name, with `joints::joint_id`'s message for anything else, `null` included.
fn some_joint<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Value>, D::Error> {
    match Value::deserialize(d)? {
        v @ (Value::Number(_) | Value::String(_)) => Ok(Some(v)),
        _ => Err(D::Error::custom(whole("`joint` must be an id or a name"))),
    }
}

/// `from` and `to`: a joint value, with `joints::value`'s message for anything else.
fn some_joint_value<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Value>, D::Error> {
    match Value::deserialize(d)? {
        v @ Value::String(_) => Ok(Some(v)),
        Value::Number(n) if n.as_f64().is_some_and(|x| x.is_finite() && x.abs() < 1e9) => Ok(Some(Value::Number(n))),
        v => Err(D::Error::custom(whole(format!("a joint value {}", crate::params::expr_problem(Some(&v)))))),
    }
}

/// `samples`: a number from 2 to 500, with the handler's message for anything else, `null`
/// included.
fn some_samples<'de, D: Deserializer<'de>>(d: D) -> Result<Option<f64>, D::Error> {
    Value::deserialize(d)?
        .as_f64()
        .filter(|n| n.is_finite() && (2.0..=MAX_SAMPLES).contains(n))
        .map(Some)
        .ok_or_else(|| D::Error::custom(whole(format!("`samples` must be 2…{MAX_SAMPLES}"))))
}

fn samples_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "description": "How many points the motion is sampled at: 2 to 500 (a fraction rounds). Default: the study's steps + 1, or 100 for a joint.",
        "type": "number",
        "minimum": 2,
        "maximum": MAX_SAMPLES
    })
}

/// `sketch`: an id or a name. Anything else names no feature, so the handler's "missing".
fn some_sketch<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Value>, D::Error> {
    match Value::deserialize(d)? {
        v @ (Value::Number(_) | Value::String(_)) => Ok(Some(v)),
        _ => Err(D::Error::custom(whole("trace sketch is missing"))),
    }
}

fn sketch_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "description": "The sketch Keep Motion Trace made, by id (a number) or name, which must still have its associative trace.",
        "type": ["number", "string"]
    })
}

/// How a trace went, as both Motion Trace and Keep Motion Trace report it.
#[derive(Debug, Serialize, JsonSchema)]
pub struct TraceRun {
    /// The traced occurrence, by name ("root" for the root).
    pub occurrence: String,
    /// How many samples were taken.
    pub samples: usize,
    /// The sample where the motion stopped short, or `null`: the trace ends there.
    pub stopped_at: Option<usize>,
    /// Where a loop stopped the joints, or `null`.
    pub stopped_by_loop: Option<LoopStop>,
    /// Where contact stopped the joints, or `null`.
    pub stopped_by_contact: Option<ContactStop>,
}

/// What `motion.trace` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MotionTraced {
    #[serde(flatten)]
    pub run: TraceRun,
    /// The point's world positions, one per sample, in mm.
    pub points: Vec<[f64; 3]>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What `motion.trace.keep` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MotionTraceKept {
    #[serde(flatten)]
    pub run: TraceRun,
    /// The new sketch's id.
    pub sketch: u64,
    /// The new sketch's name.
    pub name: String,
    /// The spline's curve id.
    pub curves: Vec<String>,
    /// How many points the spline runs through: the samples, less those where the point stood
    /// still.
    pub points: usize,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What `motion.trace.edit` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MotionTraceEdited {
    /// The trace's sketch, by id.
    pub sketch: u64,
    /// Whether the trace still follows: false once it was detached.
    pub auto_update: bool,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

#[cfg(test)]
#[path = "motion_trace_tests.rs"]
mod tests;
