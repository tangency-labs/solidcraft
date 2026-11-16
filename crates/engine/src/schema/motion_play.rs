//! Contracts and reply types for Play Motion Study and Export Motion Study in `cmd/motion.rs`
//! (#12668). A motion study drives an assembly's joints along a timeline of steps
//! ([`super::motion::MotionStudyParams`]).
//!
//! The handlers read their parameters from the raw JSON, so each key here reads as its handler
//! reads it and fails with the handler's message. Whether the study exists, and whether a path
//! may be written, stay the handlers' to check. Where a handler ignores a key of the wrong type
//! (a `quiet` that isn't true or false, a `path` that isn't text), the contract refuses it.

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

use super::check::whole;
use super::motion::{require, some_study, study_ref_schema};

/// The most samples Export Motion Study takes, as `cmd/motion.rs` reads `samples`.
const MAX_SAMPLES: f64 = 10_000.0;

fn study_required(schema: &mut Schema) {
    require(schema, "study");
}

fn study_and_step_required(schema: &mut Schema) {
    require(schema, "study");
    require(schema, "step");
}

/// Play a motion study: set its joints to the study's values at one step (contact sets stop them
/// where parts would pass through each other), and report where the parts went. One undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = study_and_step_required)]
pub struct PlayParams {
    // `study` and `step` are optional in the type and required in the schema: left out, the
    // handler refuses the call with its own message, not the check's "is required".
    #[serde(default, deserialize_with = "some_study", skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "study_ref_schema")]
    pub study: Option<Value>,
    #[serde(default, deserialize_with = "play_step", skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "play_step_schema")]
    pub step: Option<f64>,
    /// True: `positions` lists only the occurrences that moved. Default: false, every occurrence.
    #[serde(default)]
    pub quiet: Option<bool>,
}

/// `step`: a number, with the handler's message for anything else, `null` included.
fn play_step<'de, D: Deserializer<'de>>(d: D) -> Result<Option<f64>, D::Error> {
    Value::deserialize(d)?.as_f64().map(Some).ok_or_else(|| D::Error::custom(whole("`step` must be a number")))
}

fn play_step_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "description": "The step to play to: 0 to the study's `steps` (a number outside is taken to the nearer end; a fraction falls between two steps).",
        "type": "number"
    })
}

/// Export the occurrences' positions at evenly spaced steps of a motion study, as the reply or
/// written to a file. It plays the study on a copy: the design is not changed.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = study_required)]
pub struct ExportParams {
    // Required in the schema (`study_required`), optional in the type, as Play's.
    #[serde(default, deserialize_with = "some_study", skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "study_ref_schema")]
    pub study: Option<Value>,
    #[serde(default, deserialize_with = "export_samples", skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "export_samples_schema")]
    pub samples: Option<f64>,
    /// A file to write the samples to: CSV when it ends in `.csv` (one row per occurrence per
    /// sample), JSON otherwise. The reply then gives the path and the number of samples. Blank
    /// text is read as no path. Default: the reply carries the samples.
    #[serde(default)]
    pub path: Option<String>,
}

/// `samples`: a number from 2 to 10 000, with the handler's message for anything else, `null`
/// included.
fn export_samples<'de, D: Deserializer<'de>>(d: D) -> Result<Option<f64>, D::Error> {
    Value::deserialize(d)?
        .as_f64()
        .filter(|n| (2.0..=MAX_SAMPLES).contains(n))
        .map(Some)
        .ok_or_else(|| D::Error::custom(whole("`samples` must be 2…10000")))
}

fn export_samples_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "description": "How many samples, evenly spaced from the first step to the last: 2 to 10 000 (a fraction is cut to a whole number). Default: one per step, at most 10 000.",
        "type": "number",
        "minimum": 2,
        "maximum": MAX_SAMPLES
    })
}

/// One value of a joint, as `joint.list` gives it.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct NamedJointValue {
    /// Which value (`angle`, `distance`, …).
    pub name: String,
    pub value: f64,
    /// `deg` or `mm`.
    pub unit: String,
    /// The value's limits, [lowest, highest], or `null` when it has none.
    pub limits: Option<[f64; 2]>,
}

/// A joint's values after it was driven.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct DrivenJoint {
    /// The joint's id.
    pub joint: u64,
    /// Its values, or `null` when the joint is gone.
    pub values: Option<Vec<NamedJointValue>>,
}

/// Where contact stopped the joints: two parts touched.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct ContactStop {
    /// The two parts, by name.
    pub a: String,
    pub b: String,
    /// How far the joints got towards the values asked for, from 0 to 1.
    pub fraction: f64,
}

/// Where a closed loop of joints could no longer close.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct LoopStop {
    /// The joint whose loop opens first, by name.
    pub joint: String,
    /// How far the joints got towards the values asked for, from 0 to 1.
    pub fraction: f64,
}

/// What driving the joints did, as `cmd/motion.rs`'s `drive_with_contact` reports it.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct DriveReport {
    /// Each joint driven, with its values.
    pub values: Vec<DrivenJoint>,
    /// Where contact stopped them, or `null`.
    pub stopped_by_contact: Option<ContactStop>,
    /// Where a loop stopped them, or `null`.
    pub stopped_by_loop: Option<LoopStop>,
    /// How many of the joints' conditions the solver couldn't meet.
    pub conflicts: usize,
}

/// A placement: its origin and axes, in world coordinates (mm).
#[derive(Debug, Serialize, JsonSchema)]
pub struct Pose {
    pub origin: [f64; 3],
    pub x: [f64; 3],
    pub y: [f64; 3],
    pub z: [f64; 3],
}

/// Where an occurrence is placed.
#[derive(Debug, Serialize, JsonSchema)]
pub struct PartPose {
    /// The occurrence, by name.
    pub occurrence: String,
    pub pose: Pose,
}

/// A keyed joint that can move nothing, and why.
#[derive(Debug, Serialize, JsonSchema)]
pub struct UnmovedJoint {
    /// The joint's id.
    pub joint: u64,
    /// Its name, or `null` when the joint is gone.
    pub name: Option<String>,
    pub reason: String,
}

/// What `motion.play` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct StudyPlayed {
    #[serde(flatten)]
    pub drive: DriveReport,
    /// Whether any occurrence moved.
    pub moved: bool,
    /// Where the occurrences are now: every one, or with `quiet` only those that moved.
    pub positions: Vec<PartPose>,
    /// The keyed joints that can move nothing, such as a chain joined to nothing grounded.
    pub unmoved: Vec<UnmovedJoint>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// One sample of an exported study.
#[derive(Debug, Serialize, JsonSchema)]
pub struct StudySample {
    /// The step played to.
    pub step: f64,
    /// Where contact stopped the joints, or `null`.
    pub stopped_by_contact: Option<ContactStop>,
    /// Where a loop stopped them, or `null`.
    pub stopped_by_loop: Option<LoopStop>,
    /// Where every occurrence is at that step.
    pub positions: Vec<PartPose>,
}

/// The samples of an exported study, or the file they were written to.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum StudyExport {
    /// Without `path`: the samples themselves.
    Samples {
        /// The study's name.
        study: String,
        /// The study's number of steps.
        steps: u32,
        samples: Vec<StudySample>,
    },
    /// With `path`: the file written, and how many samples it holds.
    Written { path: String, samples: usize },
}

/// What `motion.export` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct StudyExported {
    #[serde(flatten)]
    pub export: StudyExport,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

#[cfg(test)]
#[path = "motion_play_tests.rs"]
mod tests;
