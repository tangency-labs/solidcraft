//! Contracts and result types for the commands in `cmd/motion.rs`.

use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Value, json};

use super::check::whole;

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

// ---- Contact (#11949) ----
//
// The contact handlers read their parameters from the raw JSON, so each key here reads as its
// handler reads it and fails with the handler's message. Whether the occurrences and the set
// exist stays the handler's to check.

/// The most occurrences a contact set's list may name, as `cmd/motion.rs`'s `MAX_ITEMS`.
const MAX_OCCURRENCES: usize = 10_000;

/// Turn contact sets on or off: while on, driving a joint or playing a motion study stops where
/// bodies in one contact set would pass through each other. One undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct ContactEnableParams {
    /// Whether contact sets are on. Default: true.
    #[serde(default)]
    pub enabled: Option<bool>,
}

/// No parameters: `contact.enable_all` (every occurrence takes part in contact, as one set),
/// `contact.disable_all` (contact off), `contact.list` and `contact.check` (read only).
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct ContactNoParams {}

/// Make a contact set: occurrences whose bodies may touch but not pass through each other while
/// joints move. Contact sets are turned on. One undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = occurrences_required)]
pub struct ContactCreateParams {
    // Optional in the type and required in the schema (`occurrences_required`): left out, the
    // handler refuses it with its own message, not the check's "is required".
    #[serde(default, deserialize_with = "some_occurrences", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "OccurrenceKeys")]
    pub occurrences: Option<OccurrenceKeys>,
    /// The set's name. Default: "Contact Set" and the first number no set's name has.
    #[serde(default)]
    pub name: Option<String>,
}

/// Change a contact set: its occurrences, its name, or whether it's suppressed; or delete it. One
/// undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = set_required)]
pub struct ContactEditParams {
    // Required in the schema (`set_required`), optional in the type, as `occurrences` above.
    #[serde(default, deserialize_with = "contact_set", skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "contact_set_schema")]
    pub set: Option<Value>,
    /// The set's new occurrences, replacing them all.
    #[serde(default, deserialize_with = "some_occurrences", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "OccurrenceKeys")]
    pub occurrences: Option<OccurrenceKeys>,
    /// The set's new name; blank text keeps the name.
    #[serde(default)]
    pub name: Option<String>,
    /// Suppress the set (it stops nothing) or not.
    #[serde(default)]
    pub suppressed: Option<bool>,
    /// Delete the set instead; the other keys are then not used. Default: false.
    #[serde(default)]
    pub delete: Option<bool>,
}

/// A contact set's occurrences, as `cmd/motion.rs`'s `occurrences` reads them: a list of ids or
/// names, with its messages. An occurrence named twice counts once.
#[derive(Clone, Debug, PartialEq)]
pub struct OccurrenceKeys(pub Vec<Value>);

impl<'de> Deserialize<'de> for OccurrenceKeys {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        occurrence_list(Value::deserialize(d)?).map(OccurrenceKeys)
    }
}

impl JsonSchema for OccurrenceKeys {
    fn schema_name() -> Cow<'static, str> {
        "OccurrenceKeys".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "Two or more occurrences, each by id (a number) or name; not the root.",
            "type": "array",
            "items": {"type": ["number", "string"]},
            "minItems": 2,
            "maxItems": MAX_OCCURRENCES
        })
    }
}

/// A list of occurrences as `cmd/motion.rs`'s `occurrences` reads it, with its messages: at most
/// 10000, each an id or a name. Anything else, `null` included, isn't a list. Whether each
/// occurrence exists, and that none is the root, stay the handler's to check.
fn occurrence_list<E: serde::de::Error>(v: Value) -> Result<Vec<Value>, E> {
    match v {
        Value::Array(list) if list.len() > MAX_OCCURRENCES => Err(E::custom(whole("too many occurrences"))),
        Value::Array(list) if list.iter().all(|v| v.is_number() || v.is_string()) => Ok(list),
        Value::Array(_) => Err(E::custom(whole("an occurrence must be an id or a name"))),
        _ => Err(E::custom(whole("`occurrences` must list occurrences"))),
    }
}

/// The parts a collision check takes, `occurrences` or `against`, as `checked_pairs` in
/// `cmd/motion_analysis.rs` reads each: through `cmd/motion.rs`'s `occurrences`, so `against`'s
/// messages name `occurrences` too. Unlike [`OccurrenceKeys`], one part is enough: the handler
/// counts the two lists together.
#[derive(Clone, Debug, PartialEq)]
pub struct PartList(pub Vec<Value>);

impl<'de> Deserialize<'de> for PartList {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        occurrence_list(Value::deserialize(d)?).map(PartList)
    }
}

impl JsonSchema for PartList {
    fn schema_name() -> Cow<'static, str> {
        "PartList".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "Occurrences, each by id (a number) or name; not the root.",
            "type": "array",
            "items": {"type": ["number", "string"]},
            "maxItems": MAX_OCCURRENCES
        })
    }
}

/// `occurrences`: given, `null` included, it must be [`OccurrenceKeys`], as the handlers read it
/// whenever the key is there.
fn some_occurrences<'de, D: Deserializer<'de>>(d: D) -> Result<Option<OccurrenceKeys>, D::Error> {
    OccurrenceKeys::deserialize(d).map(Some)
}

/// `set`: an id or a name, with the handler's message for anything else, `null` included.
fn contact_set<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Value>, D::Error> {
    match Value::deserialize(d)? {
        v @ (Value::Number(_) | Value::String(_)) => Ok(Some(v)),
        _ => Err(D::Error::custom(whole("`set` must be an id or a name"))),
    }
}

fn contact_set_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"description": "The contact set, by id (a number) or name.", "type": ["number", "string"]})
}

/// Adds `key` to the schema's `required` list: for a key the type has optional so that the
/// handler, not the check, refuses it when it's left out.
pub(super) fn require(schema: &mut Schema, key: &str) {
    match schema.get_mut("required") {
        Some(Value::Array(required)) => {
            if !required.iter().any(|k| k == key) {
                required.push(key.into());
            }
        }
        _ => {
            schema.insert("required".into(), Value::Array(vec![key.into()]));
        }
    }
}

fn occurrences_required(schema: &mut Schema) {
    require(schema, "occurrences");
}

fn set_required(schema: &mut Schema) {
    require(schema, "set");
}

/// What `contact.enable_sets`, `contact.enable_all` and `contact.disable_all` return.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ContactSwitch {
    /// Whether contact is on.
    pub enabled: bool,
    /// From `contact.enable_all`: always true, every occurrence takes part.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub all: Option<bool>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What `contact.create` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ContactSetCreated {
    /// The new set's id.
    pub set: u64,
    pub name: String,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What `contact.edit` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ContactSetEdited {
    #[serde(flatten)]
    pub change: ContactSetChange,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What an edit did to the set.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum ContactSetChange {
    /// The set was deleted.
    Deleted {
        /// Always true.
        deleted: bool,
    },
    /// The set was changed.
    Changed {
        /// The set's id.
        set: u64,
    },
}

/// What `contact.list` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ContactSets {
    /// Whether contact is on.
    pub enabled: bool,
    /// Whether every occurrence takes part, as one set, instead of `sets`.
    pub all: bool,
    pub sets: Vec<ContactSetInfo>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// A contact set, as the design saves it.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ContactSetInfo {
    pub id: u64,
    pub name: String,
    /// Its occurrences' ids.
    pub occurrences: Vec<u64>,
    /// Present, and true, when the set is suppressed.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub suppressed: bool,
}

impl From<&solidcraft_doc::motion::ContactSet> for ContactSetInfo {
    fn from(set: &solidcraft_doc::motion::ContactSet) -> Self {
        ContactSetInfo { id: set.id, name: set.name.clone(), occurrences: set.occurrences.clone(), suppressed: set.suppressed }
    }
}

/// What `contact.check` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ContactCheck {
    /// The pairs of occurrences in one contact set whose bodies pass through each other now;
    /// none while contact is off.
    pub collisions: Vec<ContactCollision>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// Two occurrences whose bodies pass through each other.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ContactCollision {
    /// The occurrences' names.
    pub a: String,
    pub b: String,
    /// How deep (mm).
    pub depth_mm: f64,
}

// ---- Motion studies (#12155) ----
//
// `motion.study` reads its parameters from the raw JSON, so each key here reads as the handler
// reads it and fails with its message. Whether the study and each key's joint exist, whether the
// joint has that value, and whether an expression evaluates stay the handler's to check.

/// The most keys a motion study takes, and the most points one key takes.
const MAX_STUDY_KEYS: usize = 1000;
const MAX_KEY_POINTS: usize = 10_000;
/// A point's step, and a value given as a number, are read only while smaller than this in size.
const MAX_POINT_MAGNITUDE: f64 = 1e9;

/// Make a motion study, which drives joints along a timeline of steps: each key gives one value of
/// one joint at some steps, linear in between and held before the first and after the last. With
/// `study`, change that study instead. One undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct MotionStudyParams {
    /// A study to change, by id (a number) or name; left out, a new one is made.
    #[serde(default, deserialize_with = "some_study", skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "study_ref_schema")]
    pub study: Option<Value>,
    /// The study's name; blank text keeps it. Default for a new one: "Motion Study" and the first
    /// number no study's name has.
    #[serde(default)]
    pub name: Option<String>,
    /// How many steps the timeline has: 1 to 100 000 (a fraction rounds). Default for a new one: 100.
    #[serde(default, deserialize_with = "study_steps", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "f64", range(min = 1, max = 100_000))]
    pub steps: Option<f64>,
    #[serde(default, deserialize_with = "study_keys", skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "study_keys_schema")]
    pub keys: Option<Vec<Value>>,
}

/// List the design's motion studies, each with its keys in the units `motion.study` takes, and its
/// associative traces. Nothing is changed.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct MotionListParams {}

/// Delete a motion study. One undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = study_required)]
pub struct MotionStudyDeleteParams {
    // Required in the schema (`study_required`), optional in the type: left out, the handler
    // refuses it with its own message, not the check's "is required".
    #[serde(default, deserialize_with = "some_study", skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "study_ref_schema")]
    pub study: Option<Value>,
}

/// `study`: an id or a name, with the handler's message for anything else, `null` included.
pub(super) fn some_study<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Value>, D::Error> {
    match Value::deserialize(d)? {
        v @ (Value::Number(_) | Value::String(_)) => Ok(Some(v)),
        _ => Err(D::Error::custom(whole("`study` must be an id or a name"))),
    }
}

pub(super) fn study_ref_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "description": "The motion study, by id (a number) or name. A number is looked up as an id first, text as a name first.",
        "type": ["number", "string"]
    })
}

fn study_required(schema: &mut Schema) {
    require(schema, "study");
}

/// `steps`: a number from 1 to 100 000, with the handler's message for anything else, `null`
/// included.
fn study_steps<'de, D: Deserializer<'de>>(d: D) -> Result<Option<f64>, D::Error> {
    Value::deserialize(d)?
        .as_f64()
        .filter(|n| n.is_finite() && (1.0..=100_000.0).contains(n))
        .map(Some)
        .ok_or_else(|| D::Error::custom(whole("`steps` must be 1…100000")))
}

/// Why the handler refuses `key`'s shape before looking at the design: its joint, value index,
/// points and values, with the handler's messages.
fn study_key_problem(key: &Value) -> Option<String> {
    if !matches!(key.get("joint"), Some(Value::Number(_) | Value::String(_))) {
        return Some("`joint` must be an id or a name".into());
    }
    // The handler reads an index that isn't a whole number as 0; the contract refuses it.
    if key.get("index").is_some_and(|i| i.as_u64().is_none()) {
        return Some("a key's `index` must be a whole number of 0 or more".into());
    }
    let Some(points) = key.get("points").and_then(Value::as_array) else {
        return Some("each key needs `points`: [[step, value], …]".into());
    };
    if points.is_empty() || points.len() > MAX_KEY_POINTS {
        return Some("a key needs 1…10000 points".into());
    }
    for point in points {
        let step = point.get(0).and_then(Value::as_f64).filter(|x| x.is_finite() && x.abs() < MAX_POINT_MAGNITUDE);
        let Some(value) = point.get(1).filter(|_| step.is_some()) else {
            return Some("points are [step, value]".into());
        };
        let readable = match value {
            Value::Number(n) => n.as_f64().is_some_and(|x| x.is_finite() && x.abs() < MAX_POINT_MAGNITUDE),
            Value::String(_) => true,
            _ => false,
        };
        if !readable {
            return Some(format!("a joint value {}", crate::params::expr_problem(Some(value))));
        }
    }
    None
}

/// `keys`: a list of at most 1000 keys, each checked as [`study_key_problem`] says. Given, `null`
/// included, it must be a list, as the handler reads it.
fn study_keys<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Vec<Value>>, D::Error> {
    let Value::Array(keys) = Value::deserialize(d)? else {
        return Err(D::Error::custom(whole("`keys` must be a list")));
    };
    if keys.len() > MAX_STUDY_KEYS {
        return Err(D::Error::custom(whole("too many keys")));
    }
    if let Some(problem) = keys.iter().find_map(study_key_problem) {
        return Err(D::Error::custom(whole(problem)));
    }
    Ok(Some(keys))
}

/// The schema of `motion.study`'s `keys`. A point's two elements are typed apart
/// (`prefixItems`): the step must be a number, while the value may be a number or an
/// expression. Elements after those two are ignored by the handler, so they are left free.
fn study_keys_schema(_: &mut SchemaGenerator) -> Schema {
    let number = json!({"type": "number", "exclusiveMinimum": -MAX_POINT_MAGNITUDE, "exclusiveMaximum": MAX_POINT_MAGNITUDE});
    json_schema!({
        "description": "The study's keys, replacing any it has (at most 1000). Each drives one value of one joint: `joint` by id or name, `index` the value (0 for a revolute's angle; default 0), and `points` as [step, value] pairs (1 to 10 000), a value being degrees for an angle and mm for a distance, or an expression.",
        "type": "array",
        "maxItems": MAX_STUDY_KEYS,
        "items": {
            "type": "object",
            "required": ["joint", "points"],
            "properties": {
                "joint": {"type": ["number", "string"]},
                "index": {"type": "integer", "minimum": 0},
                "points": {
                    "type": "array",
                    "minItems": 1,
                    "maxItems": MAX_KEY_POINTS,
                    "items": {
                        "type": "array",
                        "minItems": 2,
                        "prefixItems": [
                            number.clone(),
                            {"anyOf": [number, {"type": "string"}]}
                        ]
                    }
                }
            }
        }
    })
}

/// What `motion.study` returns: the study made or changed.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MotionStudySaved {
    /// The study's id.
    pub study: u64,
    pub name: String,
    pub steps: u32,
    /// How many keys it has.
    pub keys: usize,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What `motion.list` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MotionStudies {
    pub studies: Vec<MotionStudyInfo>,
    /// The design's associative traces, each `{sketch, motion, samples, error}`: the sketch
    /// (by id) the trace draws into, the motion it follows (a joint's range or a study), how many
    /// samples it takes (`null`: automatic), and why it last failed (`null` when it didn't).
    #[schemars(with = "Vec<serde_json::Map<String, Value>>")]
    pub traces: Vec<Value>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// A motion study, in the form `motion.study` takes it back unchanged.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MotionStudyInfo {
    pub id: u64,
    pub name: String,
    pub steps: u32,
    pub keys: Vec<MotionStudyKeyInfo>,
}

/// One key of a study.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MotionStudyKeyInfo {
    /// The joint's id.
    pub joint: u64,
    /// Its name; its id, as text, when the joint is gone.
    pub joint_name: String,
    /// Which of its values.
    pub index: usize,
    /// `deg` for an angle, `mm` for a distance: the unit of the points' values.
    pub unit: String,
    /// [step, value] pairs.
    pub points: Vec<[f64; 2]>,
}

/// What `motion.delete` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MotionStudyDeleted {
    /// The deleted study's name.
    pub deleted: String,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

#[cfg(test)]
#[path = "motion_tests.rs"]
mod tests;
