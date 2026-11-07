//! Contracts and reply types for the motion analysis commands in `cmd/motion_analysis.rs`
//! (#12852): Plot Motion Study (`motion.plot`) and Motion Interference (`motion.interference`).
//! Both play a motion study sample by sample on a copy of the design; neither changes it.
//!
//! The handlers read their parameters from the raw JSON, so each key here reads as its handler
//! reads it and fails with its message. `study` reuses the motion study commands' reader, and
//! the parts Motion Interference checks, `occurrences` and `against`, read as its
//! `checked_pairs` reads them ([`PartList`]). Whether the study, joints and occurrences exist,
//! and whether there are parts to check, stay the handlers' to check.

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value, json};
use solidcraft_geom::Vec3Geom;

use super::check::whole;
use super::motion::{JointRef, PartList, require, some_study, study_ref_schema};

/// The most samples either command takes (`MAX_SAMPLES` in `cmd/motion_analysis.rs`).
const MAX_SAMPLES: f64 = 1000.0;
/// The fewest samples a plot takes: three, so it has an acceleration.
const MIN_PLOT_SAMPLES: f64 = 3.0;
/// The fewest samples an interference check takes.
const MIN_INTERFERENCE_SAMPLES: f64 = 2.0;
/// The most joints a plot lists (`MAX_SERIES`).
const MAX_JOINTS: usize = 256;
/// The longest and shortest `duration` a plot takes, in seconds.
const MIN_DURATION_S: f64 = 1e-3;
const MAX_DURATION_S: f64 = 1e9;
/// The longest `budget_s` an interference check takes: a day (`MAX_BUDGET_S`).
const MAX_BUDGET_S: f64 = 86_400.0;
/// The most ids an occurrence path lists, and the largest size of a point's coordinates
/// (`traced_point` in `cmd/motion.rs`).
const MAX_PATH_IDS: usize = 64;
const MAX_POINT_MM: f64 = 1e7;

/// Play a motion study on a copy of the design and plot each joint value, with its velocity
/// and acceleration (finite differences between samples), and optionally the path of a point of
/// a part. The study plays as `motion.play` plays it, contact sets included. The design is not
/// changed.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = plot_keys)]
pub struct PlotParams {
    // Required in the schema (`plot_keys`), optional in the type: left out, the handler refuses
    // it with its own message, not the check's "is required".
    #[serde(default, deserialize_with = "some_study", skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "study_ref_schema")]
    pub study: Option<Value>,
    /// The joints to plot, each by id or name: every value of each. Default (or `null`): every
    /// joint that isn't suppressed.
    #[serde(default, deserialize_with = "plotted_joints")]
    #[schemars(schema_with = "joints_schema")]
    pub joints: Option<Vec<JointRef>>,
    /// The part whose point to follow: an occurrence by id or name, or a path of occurrence ids.
    /// Given with `point`.
    #[serde(default, deserialize_with = "followed_occurrence", skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "occurrence_schema")]
    pub occurrence: Option<Value>,
    /// The point to follow, [x, y, z] in world mm where the part is now. Given with
    /// `occurrence`.
    #[serde(default, deserialize_with = "followed_point", skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "point_schema")]
    pub point: Option<Value>,
    /// How many samples, from 3 to 1000 (rounded). Default: the study's steps + 1.
    #[serde(default, deserialize_with = "plot_samples", skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "plot_samples_schema")]
    pub samples: Option<f64>,
    /// The seconds the whole study takes, 0.001 or more: velocities and accelerations are then
    /// per second, and the reply has `time`. Default (or `null`): per step.
    #[serde(default, deserialize_with = "duration")]
    #[schemars(schema_with = "duration_schema")]
    pub duration: Option<f64>,
}

fn plot_keys(schema: &mut Schema) {
    require(schema, "study");
    // `occurrence` and `point` come together. `conforms` in the tests doesn't read this; the
    // handler refuses one without the other.
    schema.insert("dependentRequired".into(), json!({"occurrence": ["point"], "point": ["occurrence"]}));
}

/// Play a motion study on a copy of the design and find the pairs of parts that pass into each
/// other along it, with the steps where they overlap. Contact sets don't stop it. Parts are
/// tested at the samples only: one that passes wholly through another between two samples is
/// missed. The design is not changed.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = study_required)]
pub struct InterferenceParams {
    #[serde(default, deserialize_with = "some_study", skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "study_ref_schema")]
    pub study: Option<Value>,
    /// The parts checked against each other (two or more, or one with `against`). Default:
    /// every occurrence.
    #[serde(default, deserialize_with = "super::present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "PartList")]
    pub occurrences: Option<PartList>,
    /// The parts that must be missed: each of `occurrences` is checked only against these.
    /// Default (or `null`): none.
    #[serde(default)]
    pub against: Option<PartList>,
    /// Check the pairs of the design's contact sets instead (not with `occurrences`).
    #[serde(default)]
    pub contact_sets: Option<bool>,
    /// How many samples, from 2 to 1000 (rounded). Default: the study's steps + 1.
    #[serde(default, deserialize_with = "interference_samples", skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "interference_samples_schema")]
    pub samples: Option<f64>,
    /// The seconds the check may take (above 0, at most 86400), looked at between samples, for
    /// a mechanism that needs longer than the fixed work budget. Default (or `null`): that
    /// budget.
    #[serde(default, deserialize_with = "budget_s")]
    #[schemars(schema_with = "budget_s_schema")]
    pub budget_s: Option<f64>,
}

fn study_required(schema: &mut Schema) {
    require(schema, "study");
}

/// `joints`: a list of at most 256 joints, each an id or a name, or `null`, with the handler's
/// messages (`plotted` and `joint_id`).
fn plotted_joints<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Vec<JointRef>>, D::Error> {
    match Value::deserialize(d)? {
        Value::Null => Ok(None),
        Value::Array(list) if list.len() <= MAX_JOINTS => {
            if list.iter().all(|v| v.is_number() || v.is_string()) {
                Ok(Some(list.iter().map(|_| JointRef).collect()))
            } else {
                Err(D::Error::custom(whole("`joint` must be an id or a name")))
            }
        }
        _ => Err(D::Error::custom(format!("must list at most {MAX_JOINTS} joints"))),
    }
}

fn joints_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "description": "The joints to plot, each by id (a number) or name: every value of each. Default (or `null`): every joint \
            that isn't suppressed. At most 256 joints, and 256 joint values in all.",
        "type": ["array", "null"],
        "items": {"type": ["number", "string"]},
        "maxItems": MAX_JOINTS,
    })
}

/// `occurrence`: an id or a name, or a path of at most 64 occurrence ids, with the handler's
/// messages (`traced_point` and `occurrence_id`), `null` included.
fn followed_occurrence<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Value>, D::Error> {
    let v = Value::deserialize(d)?;
    let problem = match &v {
        Value::Number(_) | Value::String(_) => None,
        Value::Array(ids) if ids.len() > MAX_PATH_IDS => Some("an occurrence path has at most 64 ids"),
        Value::Array(ids) if !ids.iter().all(|id| id.as_u64().is_some()) => Some("an occurrence path lists occurrence ids"),
        Value::Array(_) => None,
        _ => Some("an occurrence must be an id or a name"),
    };
    match problem {
        Some(message) => Err(D::Error::custom(whole(message))),
        None => Ok(Some(v)),
    }
}

fn occurrence_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "description": "The part whose point to follow: an occurrence by id (a number) or name (`root` or 0 for the design's own \
            bodies), or a path of at most 64 occurrence ids from the top. Given with `point`.",
        "anyOf": [
            {"type": ["number", "string"]},
            {"type": "array", "items": {"type": "integer", "minimum": 0}, "maxItems": MAX_PATH_IDS},
        ],
    })
}

/// `point`: [x, y, z] or {x, y, z}, finite and nearer the origin than 1e7 mm, as `params::vec3`
/// and `traced_point` read it, with its message (the check names the key).
fn followed_point<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Value>, D::Error> {
    let v = Value::deserialize(d)?;
    match crate::params::vec3(&v) {
        Some(p) if p.len() < MAX_POINT_MM => Ok(Some(v)),
        _ => Err(D::Error::custom("must be [x, y, z], a world point")),
    }
}

fn point_schema(_: &mut SchemaGenerator) -> Schema {
    let coordinate = json!({"type": "number", "exclusiveMinimum": -1e9, "exclusiveMaximum": 1e9});
    json_schema!({
        "description": "The point to follow, in world mm where the part is now: [x, y, z] or {x, y, z}, nearer the origin than 1e7 mm. \
            Given with `occurrence`.",
        "anyOf": [
            {"type": "array", "items": coordinate, "minItems": 3, "maxItems": 3},
            {"type": "object", "properties": {"x": coordinate, "y": coordinate, "z": coordinate}, "required": ["x", "y", "z"]},
        ],
    })
}

/// `samples`: a number from `min` to 1000, with the handler's message (`samples`), `null`
/// included.
fn samples_from<E: serde::de::Error>(v: &Value, min: f64) -> Result<Option<f64>, E> {
    v.as_f64().filter(|n| n.is_finite() && *n >= min && *n <= MAX_SAMPLES).map(Some).ok_or_else(|| E::custom(format!("must be {min}…{MAX_SAMPLES}")))
}

fn plot_samples<'de, D: Deserializer<'de>>(d: D) -> Result<Option<f64>, D::Error> {
    samples_from(&Value::deserialize(d)?, MIN_PLOT_SAMPLES)
}

fn interference_samples<'de, D: Deserializer<'de>>(d: D) -> Result<Option<f64>, D::Error> {
    samples_from(&Value::deserialize(d)?, MIN_INTERFERENCE_SAMPLES)
}

fn samples_schema(min: f64) -> Schema {
    json_schema!({
        "description": format!("How many samples, from {min} to 1000 (rounded to a whole number). Default: the study's steps + 1."),
        "type": "number",
        "minimum": min,
        "maximum": MAX_SAMPLES,
    })
}

fn plot_samples_schema(_: &mut SchemaGenerator) -> Schema {
    samples_schema(MIN_PLOT_SAMPLES)
}

fn interference_samples_schema(_: &mut SchemaGenerator) -> Schema {
    samples_schema(MIN_INTERFERENCE_SAMPLES)
}

/// `duration`: seconds from 0.001 to 1e9, or `null`, with the handler's message.
fn duration<'de, D: Deserializer<'de>>(d: D) -> Result<Option<f64>, D::Error> {
    match Value::deserialize(d)? {
        Value::Null => Ok(None),
        v => v
            .as_f64()
            .filter(|s| (MIN_DURATION_S..=MAX_DURATION_S).contains(s))
            .map(Some)
            .ok_or_else(|| D::Error::custom("must be seconds, 0.001 or more")),
    }
}

fn duration_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "description": "The seconds the whole study takes, 0.001 or more (at most 1e9): velocities and accelerations are then per \
            second, and the reply has `time`. Default (or `null`): per step.",
        "type": ["number", "null"],
        "minimum": MIN_DURATION_S,
        "maximum": MAX_DURATION_S,
    })
}

/// `budget_s`: seconds above 0, at most a day, or `null`, with the handler's message (it reads
/// the number with `params::num`, which takes it finite and smaller than 1e9 in size).
fn budget_s<'de, D: Deserializer<'de>>(d: D) -> Result<Option<f64>, D::Error> {
    match Value::deserialize(d)? {
        Value::Null => Ok(None),
        v => v
            .as_f64()
            .filter(|s| s.is_finite() && *s > 0.0 && *s <= MAX_BUDGET_S)
            .map(Some)
            .ok_or_else(|| D::Error::custom(format!("must be a number of seconds above 0, at most {MAX_BUDGET_S}"))),
    }
}

fn budget_s_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "description": "The seconds the check may take (above 0, at most 86400), looked at between samples, so the last sample may \
            run past it. Default (or `null`): a fixed work budget.",
        "type": ["number", "null"],
        "exclusiveMinimum": 0,
        "maximum": MAX_BUDGET_S,
    })
}

/// Where a study stopped short, and why: shared by both replies.
#[derive(Debug, Serialize, JsonSchema)]
pub struct StudyStop {
    /// The sample it stopped at, or `null` when it played to the end.
    pub stopped_at: Option<usize>,
    /// The joint whose closed loop could no longer close (`{joint}`), or `null`.
    pub stopped_by_loop: Option<Map<String, Value>>,
}

/// One joint value plotted.
#[derive(Debug, Serialize, JsonSchema)]
pub struct PlotSeries {
    /// The joint's id.
    pub joint: u64,
    pub name: Option<String>,
    /// Which of its values.
    pub index: usize,
    /// The value's name (`angle`, `distance`, …).
    pub dof: Option<&'static str>,
    /// `deg` for an angle (unwrapped, so it runs on past a turn), `mm` for a distance.
    pub unit: &'static str,
    /// `unit` per second with a `duration`, else per step.
    pub velocity_unit: String,
    pub acceleration_unit: String,
    /// At each sample.
    pub value: Vec<f64>,
    pub velocity: Vec<f64>,
    pub acceleration: Vec<f64>,
}

/// The path of the point a plot follows.
#[derive(Debug, Serialize, JsonSchema)]
pub struct PlottedPoint {
    /// The part's name (`root` for the design's own bodies).
    pub occurrence: String,
    /// `mm`.
    pub unit: &'static str,
    pub velocity_unit: String,
    pub acceleration_unit: String,
    /// Where it is at each sample, [x, y, z] in world mm.
    pub position: Vec<[f64; 3]>,
    pub velocity: Vec<[f64; 3]>,
    pub acceleration: Vec<[f64; 3]>,
    /// The length of `velocity` at each sample.
    pub speed: Vec<f64>,
    /// The length of `acceleration` at each sample.
    pub acceleration_norm: Vec<f64>,
}

/// What `motion.plot` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MotionPlot {
    /// The study's name.
    pub study: String,
    /// Its steps.
    pub steps: u32,
    /// How many samples were taken: fewer than asked when the study stopped short.
    pub samples: usize,
    /// The study step of each sample.
    pub step: Vec<f64>,
    /// With a `duration` only: the time of each sample, in seconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time: Option<Vec<f64>>,
    pub joints: Vec<PlotSeries>,
    /// With `occurrence` and `point` only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub point: Option<PlottedPoint>,
    #[serde(flatten)]
    pub stop: StudyStop,
    /// Where two parts in a contact set touched and stopped it, or `null`.
    pub stopped_by_contact: Option<Map<String, Value>>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// One pair of parts that pass into each other along the study.
#[derive(Debug, Serialize, JsonSchema)]
pub struct CollidingPair {
    /// The two parts, the older occurrence first.
    pub a: String,
    pub b: String,
    /// The first and last study steps where they overlap.
    pub first_step: Option<f64>,
    pub last_step: Option<f64>,
    /// The step ranges where they overlap, [from, to].
    pub steps: Vec<[f64; 2]>,
    /// How far into each other they go at most, in mm: an estimate from points of the bodies'
    /// contact meshes, not exact.
    pub depth_mm: f64,
    /// The first step where they share the most volume, or `null` with `not_measured`.
    pub deepest_step: Option<f64>,
    /// The most volume they share at a sample, in mm³, worked out exactly from the bodies, or
    /// `null` with `not_measured` (never an estimate).
    pub volume_mm3: Option<f64>,
    /// Why the volume wasn't worked out at every sample they overlap, or `null`.
    pub not_measured: Option<String>,
}

/// What `motion.interference` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MotionInterference {
    /// The study's name.
    pub study: String,
    /// Its steps.
    pub steps: u32,
    /// Every sample was checked and no two parts collide. Never true when `truncated`.
    pub clear: bool,
    /// In the order they first collide.
    pub collisions: Vec<CollidingPair>,
    /// How many pairs of parts were checked.
    pub pairs: usize,
    /// How many samples were checked.
    pub samples: usize,
    /// The work budget, or `budget_s`, ran out before the last sample.
    pub truncated: bool,
    /// When `truncated`: the last step checked.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checked_to: Option<f64>,
    /// The `budget_s` given.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub budget_s: Option<f64>,
    #[serde(flatten)]
    pub stop: StudyStop,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

#[cfg(test)]
#[path = "motion_analysis_tests.rs"]
mod tests;
