//! Contracts and result types for the commands in `cmd/edit.rs`.

use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value, json};

use super::check::{located, whole};
use super::types::{not_blank_pattern, present};

/// Assistant visibility is UI-owned and takes no parameters.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct AssistantParams {}

/// Headless sessions refuse Assistant display requests and produce no result.
#[derive(Debug, Serialize, JsonSchema)]
pub struct AssistantResult {}

/// Command search is UI-owned and takes no parameters.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct PaletteParams {}

/// Headless sessions refuse to open command search and produce no result.
#[derive(Debug, Serialize, JsonSchema)]
pub struct PaletteResult {}

/// `ui.toggle_visibility` takes nothing: it acts on the app's selection.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct ToggleVisibilityParams {}

/// Never produced: headless calls are refused, and the app acts on its own selection.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ToggleVisibilityResult {}

/// `edit.repeat` takes no parameters: it starts the last tool with its defaults.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct RepeatParams {}

/// What `edit.repeat` returns: nothing. Called through the engine it is always refused, since
/// only the app has a tool to start.
#[derive(Debug, Serialize, JsonSchema)]
pub struct RepeatResult {}

/// Display-only timeline playback requests. The connected UI owns the animation clock.
#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PlaybackAction {
    #[default]
    Toggle,
    Play,
    Stop,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct TimelinePlayParams {
    /// Omit to toggle; explicit play/stop requests are idempotent.
    #[serde(default)]
    pub action: PlaybackAction,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct TimelinePlayResult {
    /// Validated display request. A connected UI applies it; a headless engine has no clock.
    pub action: PlaybackAction,
}

/// `edit.undo` takes no parameters: it takes back the last step of the undo history.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct UndoParams {}

/// What `edit.undo` returns. With nothing to undo it is refused.
#[derive(Debug, Serialize, JsonSchema)]
pub struct UndoResult {
    /// The name of the step taken back, as the undo history labels it.
    pub undone: String,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// `edit.redo` takes no parameters: it does again the last step undone.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct RedoParams {}

/// What `edit.redo` returns. With nothing to redo it is refused.
#[derive(Debug, Serialize, JsonSchema)]
pub struct RedoResult {
    /// The name of the step done again, as the undo history labels it.
    pub redone: String,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// `edit.checkpoint` takes no parameters: it remembers the design, the active sketch and the
/// undo history, to go back to with `edit.restore_checkpoint`. The last 16 are kept, the oldest
/// dropped first.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct CheckpointParams {}

/// What `edit.checkpoint` and `edit.restore_checkpoint` return.
#[derive(Debug, Serialize, JsonSchema)]
pub struct CheckpointResult {
    /// The checkpoint's id.
    pub checkpoint: u64,
    /// `edit.restore_checkpoint` only: true when the design went back to it, false when it was
    /// dropped (`forget`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub restored: Option<bool>,
    /// True when restoring reattaches or removes a transient Form editor.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub form_restored: Option<bool>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// A checkpoint's id, as `edit.checkpoint` returns it: a whole number of at least 0.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CheckpointId(pub u64);

impl<'de> Deserialize<'de> for CheckpointId {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Value::deserialize(d)?.as_u64().map(CheckpointId).ok_or_else(|| D::Error::custom("must be an id from edit.checkpoint"))
    }
}

impl JsonSchema for CheckpointId {
    fn schema_name() -> Cow<'static, str> {
        "CheckpointId".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({"description": "A checkpoint's id, from edit.checkpoint.", "type": "integer", "minimum": 0})
    }
}

/// Go back to a checkpoint, undo history included; or, with `forget`, drop it without going
/// back. Going back uses it up and every later checkpoint with it: take a new one to go back
/// again. Forgetting drops only this one.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct RestoreCheckpointParams {
    /// The checkpoint, by the id `edit.checkpoint` returned.
    pub checkpoint: CheckpointId,
    /// True to drop the checkpoint without going back; later checkpoints stay. Default: false.
    #[serde(default)]
    pub forget: Option<bool>,
}

// The timeline editing commands (#12547): `timeline.compute_all`, `delete`, `rename`,
// `rollback`, `roll_to`, `dependents`, `suppress`, `edit` and `inspect`.
//
// The handlers read their parameters from the raw JSON, so each key here reads as its handler
// reads it and fails with the handler's message. Whether a feature, a folder or a name exists
// stays the handler's to check. A key the handler refuses when it is left out is optional in the
// type and required in the schema, so that the handler's message, not the check's "is
// required", is what a caller sees.

/// Adds `key` to the schema's `required` list: for a key the type has optional so that the
/// handler, not the check, refuses it when it's left out.
fn require(schema: &mut Schema, key: &str) {
    super::motion::require(schema, key);
}

/// Several keys of which the handler needs at least one: the schema lists each alone as enough.
fn require_any(schema: &mut Schema, keys: &[&str]) {
    let branches: Vec<Value> = keys.iter().map(|k| json!({"required": [k]})).collect();
    schema.insert("anyOf".into(), Value::Array(branches));
}

fn feature_required(schema: &mut Schema) {
    require(schema, "feature");
}

fn feature_and_name_required(schema: &mut Schema) {
    require(schema, "feature");
    require(schema, "name");
}

fn feature_and_set_required(schema: &mut Schema) {
    require(schema, "feature");
    require(schema, "set");
}

fn delete_needs_a_target(schema: &mut Schema) {
    require_any(schema, &["feature", "features", "groups"]);
}

fn suppress_needs_a_target(schema: &mut Schema) {
    require_any(schema, &["feature", "features", "groups"]);
}

fn inspect_needs_a_target(schema: &mut Schema) {
    require_any(schema, &["group", "feature"]);
}

/// The most features, or folders, a list may hold, as the handlers read it.
const MAX_LIST: usize = 10_000;

const FEATURE_MESSAGE: &str = "`feature` must be an id or name";

/// A feature by its id (a whole number) or its name, as `feature_id` reads it. A number that is
/// no id is the handler's to refuse ("no feature `1.5`").
#[derive(Clone, Debug, PartialEq)]
pub struct FeatureRef(pub Value);

impl<'de> Deserialize<'de> for FeatureRef {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match Value::deserialize(d)? {
            v @ (Value::Number(_) | Value::String(_)) => Ok(FeatureRef(v)),
            _ => Err(D::Error::custom(whole(FEATURE_MESSAGE))),
        }
    }
}

impl JsonSchema for FeatureRef {
    fn schema_name() -> Cow<'static, str> {
        "FeatureRef".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({"description": "A feature by id (a whole number) or by name.", "type": ["integer", "string"], "minimum": 0})
    }
}

/// `timeline.delete`'s `features`, as `params::name_or_id_values` reads it: one name, or a list of
/// names and ids. `null` is read as absent.
#[derive(Clone, Debug, PartialEq)]
pub struct FeatureKeys(pub Vec<Value>);

impl<'de> Deserialize<'de> for FeatureKeys {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let given = json!({ "features": Value::deserialize(d)? });
        match crate::params::name_or_id_values("", &given, "features") {
            Ok(keys) => Ok(FeatureKeys(keys.to_vec())),
            Err(crate::EngineError::BadParams { msg, .. }) => Err(D::Error::custom(whole(msg))),
            Err(e) => Err(D::Error::custom(whole(e))),
        }
    }
}

impl JsonSchema for FeatureKeys {
    fn schema_name() -> Cow<'static, str> {
        "FeatureKeys".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "One feature name, or a list of feature names and ids.",
            "anyOf": [
                {"type": "string"},
                {"type": "array", "items": {"type": ["integer", "string"], "minimum": 0}, "maxItems": MAX_LIST}
            ]
        })
    }
}

/// `timeline.suppress`'s `features`, as `feature_ids` reads it: a list of up to 10000 ids and
/// names, or one id or name.
#[derive(Clone, Debug, PartialEq)]
pub struct FeatureSelection(pub Vec<FeatureRef>);

impl<'de> Deserialize<'de> for FeatureSelection {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let one = |v: Value| match v {
            Value::Number(_) | Value::String(_) => Ok(FeatureRef(v)),
            _ => Err(D::Error::custom(whole(FEATURE_MESSAGE))),
        };
        match Value::deserialize(d)? {
            Value::Array(a) if a.len() <= MAX_LIST => a.into_iter().map(one).collect::<Result<_, _>>().map(FeatureSelection),
            Value::Array(_) => Err(D::Error::custom(whole("too many features"))),
            v @ (Value::Number(_) | Value::String(_)) => one(v).map(|f| FeatureSelection(vec![f])),
            _ => Err(D::Error::custom(whole("`features` must be a list of feature ids or names"))),
        }
    }
}

impl JsonSchema for FeatureSelection {
    fn schema_name() -> Cow<'static, str> {
        "FeatureSelection".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let one = generator.subschema_for::<FeatureRef>();
        json_schema!({
            "description": "One feature, or a list of features, by id or name.",
            "anyOf": [one, {"type": "array", "items": one, "maxItems": MAX_LIST}]
        })
    }
}

/// `groups`, as `group_ids` reads it: a list of up to 10000 timeline folder ids. `null` is
/// refused.
#[derive(Clone, Debug, PartialEq)]
pub struct FolderIds(pub Vec<u64>);

impl<'de> Deserialize<'de> for FolderIds {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match Value::deserialize(d)? {
            Value::Array(a) if a.len() > MAX_LIST => Err(D::Error::custom(whole("too many folders"))),
            Value::Array(a) => a
                .iter()
                .map(|v| v.as_u64().ok_or_else(|| D::Error::custom(whole("`groups` must contain folder ids"))))
                .collect::<Result<_, _>>()
                .map(FolderIds),
            _ => Err(D::Error::custom(whole("`groups` must be a list of timeline folder ids"))),
        }
    }
}

impl JsonSchema for FolderIds {
    fn schema_name() -> Cow<'static, str> {
        "FolderIds".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "Timeline folder ids.",
            "type": "array",
            "items": {"type": "integer", "minimum": 0},
            "maxItems": MAX_LIST
        })
    }
}

/// `timeline.inspect`'s `group`: one timeline folder id.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FolderId(pub u64);

impl<'de> Deserialize<'de> for FolderId {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Value::deserialize(d)?.as_u64().map(FolderId).ok_or_else(|| D::Error::custom(whole("`group` must be a timeline folder id")))
    }
}

impl JsonSchema for FolderId {
    fn schema_name() -> Cow<'static, str> {
        "FolderId".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({"description": "A timeline folder, by id.", "type": "integer", "minimum": 0})
    }
}

/// A feature's new name, as `timeline.rename` reads `name`: text that is 1 to 128 bytes long once
/// the whitespace around it is trimmed.
#[derive(Clone, Debug, PartialEq)]
pub struct NewName(pub String);

impl<'de> Deserialize<'de> for NewName {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match Value::deserialize(d)? {
            Value::String(s) if !s.trim().is_empty() && s.trim().len() <= 128 => Ok(NewName(s)),
            _ => Err(D::Error::custom(whole("`name` must be 1…128 characters"))),
        }
    }
}

impl JsonSchema for NewName {
    fn schema_name() -> Cow<'static, str> {
        "NewName".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "Text with something in it besides whitespace, at most 128 bytes long once the whitespace around it is trimmed (the bytes of its UTF-8, so fewer characters when some are not ASCII).",
            "type": "string",
            "pattern": not_blank_pattern()
        })
    }
}

/// Where `timeline.rollback` and `timeline.roll_to` put the marker: after this many features, or
/// at the end.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MarkerPosition {
    /// After the last feature: nothing is rolled back.
    End,
    /// After this many features. A number below 0 is the handler's to refuse, which names how
    /// many features there are.
    Count(f64),
}

impl<'de> Deserialize<'de> for MarkerPosition {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = Value::deserialize(d)?;
        if v.as_str().is_some_and(|t| t.trim().eq_ignore_ascii_case("end")) {
            return Ok(MarkerPosition::End);
        }
        v.as_f64()
            .or_else(|| v.as_str().and_then(|t| t.trim().parse::<f64>().ok()))
            .and_then(crate::params::finite)
            .map(MarkerPosition::Count)
            .ok_or_else(|| D::Error::custom(whole("`position` must be a number of features to keep active, or \"end\"")))
    }
}

impl JsonSchema for MarkerPosition {
    fn schema_name() -> Cow<'static, str> {
        "MarkerPosition".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "The number of features to keep active (0 or more), as a number or as text holding one, or \"end\" (any case) for all of them. A number below 0 is refused. A fraction is cut to a whole number, and a number of features or more is the end.",
            "anyOf": [
                {"type": "number", "minimum": 0, "exclusiveMaximum": 1e9},
                {"type": "string"}
            ]
        })
    }
}

/// A key that must be given as itself when present: `null` is refused with the handler's
/// message, which the value's own `Deserialize` gives.
fn marker_position<'de, D: Deserializer<'de>>(d: D) -> Result<Option<MarkerPosition>, D::Error> {
    match Value::deserialize(d)? {
        // The handler reads `null` as "end", as if `position` were left out.
        Value::Null => Ok(None),
        v => MarkerPosition::deserialize(v).map(Some).map_err(|e: serde_json::Error| D::Error::custom(e.to_string())),
    }
}

/// `timeline.edit`'s `set`: an object of the fields to change.
#[derive(Clone, Debug, PartialEq)]
pub struct SetFields(pub Map<String, Value>);

impl<'de> Deserialize<'de> for SetFields {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match Value::deserialize(d)? {
            Value::Object(o) => Ok(SetFields(o)),
            _ => Err(D::Error::custom(whole("`set` must be an object of fields"))),
        }
    }
}

impl JsonSchema for SetFields {
    fn schema_name() -> Cow<'static, str> {
        "SetFields".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "The fields to change, merged into the feature's definition as `timeline.inspect` returns it, e.g. {\"extent\": {\"distance\": \"30\"}} for an extrude or {\"angle\": \"90 deg\"} for a revolve. A dotted key names a nested field ({\"plane.distance\": \"8 mm\"} is {\"plane\": {\"distance\": \"8 mm\"}}). Expressions are text, or a number in mm or degrees. A field the definition doesn't have is refused.",
            "type": "object"
        })
    }
}

/// `timeline.compute_all` takes no parameters: it throws away every cached result and computes
/// the whole timeline again.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct TimelineComputeAllParams {}

/// What `timeline.compute_all` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct TimelineComputeAllResult {
    /// How many features the timeline holds.
    pub features: usize,
    /// How many of them failed to compute.
    pub errors: usize,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// Delete features, and the timeline folders given with their features, joints and nested
/// folders, as one undo step. A sketch takes the features built on
/// it with it, and a feature that owns a component takes the component's contents; later
/// features that fail without a deleted one stay and are named in `failing`. Give at least one of
/// `feature`, `features` and `groups`.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = delete_needs_a_target)]
pub struct TimelineDeleteParams {
    /// One feature, by id or name. `null` is refused.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "FeatureRef")]
    pub feature: Option<FeatureRef>,
    /// Features, by name or id: one name, or a list. `null` is read as absent.
    #[serde(default)]
    pub features: Option<FeatureKeys>,
    /// Timeline folders, by id. Each goes with all its features, joints and nested folders.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "FolderIds")]
    pub groups: Option<FolderIds>,
}

/// What `timeline.delete` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct TimelineDeleteResult {
    /// The features deleted, by id: those asked for, those that went with them and the features
    /// of the folders.
    pub deleted: Vec<u64>,
    /// The joints deleted with the folders, by id. Absent when there are none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub joints: Option<Vec<u64>>,
    /// The folders deleted, by id, nested ones included. Absent when there are none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub groups: Option<Vec<u64>>,
    /// Later features that worked before and fail now. Absent when there are none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failing: Option<Vec<FailingFeature>>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// A feature that failed once something was deleted.
#[derive(Debug, Serialize, JsonSchema)]
pub struct FailingFeature {
    /// The feature's name.
    pub feature: String,
    /// Why it fails.
    pub error: String,
}

/// Rename a feature; references to it (a sketch used by an extrude, a body name in a later
/// feature) follow. A construction plane's name may not be an origin plane's.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = feature_and_name_required)]
pub struct TimelineRenameParams {
    // Optional in the type and required in the schema (`feature_and_name_required`).
    /// The feature, by id or name.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "FeatureRef")]
    pub feature: Option<FeatureRef>,
    /// The new name. Another feature may not have it.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "NewName")]
    pub name: Option<NewName>,
}

/// What `timeline.rename` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct TimelineRenameResult {
    /// The feature, by id.
    pub feature: u64,
    /// Its name now, without the whitespace around it.
    pub name: String,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// The keys `timeline.rollback` and `timeline.roll_to` share, for one handler reads both.
/// `feature` is read first: when it is given, `position` is not read at all.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct MarkerKeys {
    /// Put the marker right after this feature, by id or name. When given, `position` is
    /// ignored; `null` is refused.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "FeatureRef")]
    pub feature: Option<FeatureRef>,
    /// How many features stay active, or "end". Default: end, also when `null`. Ignored when
    /// `feature` is given.
    #[serde(default, deserialize_with = "marker_position", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<MarkerPosition>")]
    pub position: Option<MarkerPosition>,
}

/// `value` as the marker commands' handler reads it: with `feature` given, `position` is not
/// looked at.
fn read_marker_keys<'de, D: Deserializer<'de>>(d: D) -> Result<MarkerKeys, D::Error> {
    let mut value = Value::deserialize(d)?;
    if let Value::Object(o) = &mut value
        && o.contains_key("feature")
    {
        o.remove("position");
    }
    serde_path_to_error::deserialize(&value).map_err(|e| D::Error::custom(located(&e)))
}

macro_rules! marker_params {
    ($(#[$doc:meta])* $name:ident, $summary:literal) => {
        $(#[$doc])*
        #[derive(Debug)]
        pub struct $name(pub MarkerKeys);

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                read_marker_keys(d).map($name)
            }
        }

        impl JsonSchema for $name {
            fn schema_name() -> Cow<'static, str> {
                stringify!($name).into()
            }
            fn json_schema(generator: &mut SchemaGenerator) -> Schema {
                let mut schema = MarkerKeys::json_schema(generator);
                schema.insert("description".into(), $summary.into());
                schema
            }
        }
    };
}

marker_params!(
    /// `timeline.rollback`'s parameters: [`MarkerKeys`].
    TimelineRollbackParams,
    "Move the history marker: the features after it are not built, as if rolled back, and the selection is cleared. Give `position`, the number of features to keep active (or \"end\"), or `feature` to put the marker right after one. `timeline.roll_to` does the same."
);
marker_params!(
    /// `timeline.roll_to`'s parameters: [`MarkerKeys`].
    TimelineRollToParams,
    "Move the history marker right after a feature, or to a number of features from the start, or to the end: the features after it are not built, and the selection is cleared. `timeline.rollback` does the same."
);

/// What `timeline.rollback` and `timeline.roll_to` return.
#[derive(Debug, Serialize, JsonSchema)]
pub struct TimelineMarkerResult {
    /// How many features are active now (before the marker); all of them at the end.
    pub marker: usize,
    /// How many features the timeline holds.
    pub features: usize,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What would go with a feature if it were deleted, and which later features would fail
/// without it. Nothing is changed.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = feature_required)]
pub struct TimelineDependentsParams {
    // Optional in the type and required in the schema (`feature_required`).
    /// The feature, by id or name.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "FeatureRef")]
    pub feature: Option<FeatureRef>,
}

/// What `timeline.dependents` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct TimelineDependentsResult {
    /// The names of the other features that deleting it would delete too.
    pub deleted_with_it: Vec<String>,
    /// The names of the features that work now and would fail without it.
    pub would_fail: Vec<String>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// Suppress features (and the joints of the folders given), or bring them back, as one undo
/// step. A suppressed feature is skipped when the design is built. Give at least one of
/// `feature`, `features` and `groups`.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = suppress_needs_a_target)]
pub struct TimelineSuppressParams {
    /// One feature, by id or name. `null` is read as absent.
    #[serde(default)]
    pub feature: Option<FeatureRef>,
    /// Features, by id or name: one, or a list. `null` is read as absent.
    #[serde(default)]
    pub features: Option<FeatureSelection>,
    /// Timeline folders, by id: every feature and joint in them and in their nested folders. With
    /// no `feature` or `features`, only those. `null` is refused.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "FolderIds")]
    pub groups: Option<FolderIds>,
    /// True to suppress, false to bring back. Default: toggle: all of them are suppressed unless
    /// all already are, then all are brought back. `null` is read as absent.
    #[serde(default, deserialize_with = "suppressed_flag", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<bool>")]
    pub suppressed: Option<bool>,
}

fn suppressed_flag<'de, D: Deserializer<'de>>(d: D) -> Result<Option<bool>, D::Error> {
    match Value::deserialize(d)? {
        Value::Null => Ok(None),
        Value::Bool(b) => Ok(Some(b)),
        _ => Err(D::Error::custom(whole("`suppressed` must be true or false"))),
    }
}

/// What `timeline.suppress` returns: one feature given alone is named by `feature`; folders add
/// `joints` and `groups`.
#[derive(Debug, Serialize, JsonSchema)]
pub struct TimelineSuppressResult {
    #[serde(flatten)]
    pub changed: SuppressChanged,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What a suppress changed.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum SuppressChanged {
    /// One feature, given as `feature`.
    One {
        /// The feature, by id.
        feature: u64,
        /// Whether it is suppressed now.
        suppressed: bool,
    },
    /// Features: `features` was given (not `null`), or more than one feature came out of the
    /// call, and no folders were given.
    Features {
        /// The features changed, by id.
        features: Vec<u64>,
        /// Whether they are suppressed now.
        suppressed: bool,
    },
    /// Timeline folders were given.
    Folders {
        /// The features changed, by id: those given and those of the folders.
        features: Vec<u64>,
        /// The joints changed, by id.
        joints: Vec<u64>,
        /// The folders given, by id.
        groups: Vec<u64>,
        /// Whether they are suppressed now.
        suppressed: bool,
    },
}

/// Change a feature's fields: `set` is merged into the feature's definition as
/// `timeline.inspect` returns it, and the design is rebuilt. The feature's type can't change. When the edited feature fails to build, the call returns
/// that build error.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = feature_and_set_required)]
pub struct TimelineEditParams {
    // Optional in the type and required in the schema (`feature_and_set_required`).
    /// The feature, by id or name.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "FeatureRef")]
    pub feature: Option<FeatureRef>,
    /// The fields to change.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "SetFields")]
    pub set: Option<SetFields>,
}

/// What `timeline.edit` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct TimelineEditResult {
    /// The feature, by id.
    pub feature: u64,
    /// How many features were rebuilt.
    pub recomputed: usize,
    /// How many features took an earlier result for the same inputs instead of being rebuilt.
    pub restored: usize,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// Describe a timeline folder (`group`), or a feature (`feature`): its definition, the fields
/// `timeline.edit` changes with their values, and its named inputs. Nothing is changed.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = inspect_needs_a_target)]
pub struct InspectKeys {
    /// A timeline folder, by id. When the key is given (`null` too, which is refused),
    /// `feature` is ignored.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "FolderId")]
    pub group: Option<FolderId>,
    /// A feature, by id or name. Without `group`, needed; `null` is refused.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "FeatureRef")]
    pub feature: Option<FeatureRef>,
}

/// `timeline.inspect`'s parameters: [`InspectKeys`], with `feature` not read when `group` is
/// given, as the handler does.
#[derive(Debug)]
pub struct TimelineInspectParams(pub InspectKeys);

impl<'de> Deserialize<'de> for TimelineInspectParams {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let mut value = Value::deserialize(d)?;
        if let Value::Object(o) = &mut value
            && o.contains_key("group")
        {
            o.remove("feature");
        }
        serde_path_to_error::deserialize(&value).map(TimelineInspectParams).map_err(|e| D::Error::custom(located(&e)))
    }
}

impl JsonSchema for TimelineInspectParams {
    fn schema_name() -> Cow<'static, str> {
        "TimelineInspectParams".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        InspectKeys::json_schema(generator)
    }
}

/// What `timeline.inspect` returns: a folder, or a feature.
#[derive(Debug, Serialize, JsonSchema)]
pub struct TimelineInspectResult {
    #[serde(flatten)]
    pub inspected: Inspected,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What was inspected.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum Inspected {
    /// A timeline folder, for `group`.
    Group {
        /// The folder, by id.
        group: u64,
        /// Its name.
        name: String,
        /// The folder that holds it; null at the timeline root.
        parent: Option<u64>,
        /// Its features, by id.
        features: Vec<u64>,
        /// Its joints, by id.
        joints: Vec<u64>,
    },
    /// A feature, for `feature`.
    Feature(Box<FeatureInfo>),
}

/// A feature as `timeline.inspect` describes it.
#[derive(Debug, Serialize, JsonSchema)]
pub struct FeatureInfo {
    /// The feature's id.
    pub id: u64,
    pub name: String,
    /// The feature's type key (as the command that made it names it).
    #[serde(rename = "type")]
    pub kind: String,
    /// The type, in words.
    pub type_name: String,
    pub suppressed: bool,
    /// Whether the feature is after the history marker, so it is not built.
    pub rolled_back: bool,
    /// The fields `timeline.edit`'s `set` changes, with their values now.
    #[schemars(schema_with = "definition_schema")]
    pub definition: Value,
    /// Fields left out of `definition` for their size, with what each holds.
    pub omitted: Vec<OmittedField>,
    /// The named inputs `parameters.change` can also set.
    pub inputs: Vec<FeatureInput>,
    /// Why the feature fails to build; null when it doesn't.
    pub error: Option<String>,
    /// A warning the feature built with; null when there is none.
    pub warning: Option<String>,
}

fn definition_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type": "object", "description": "The feature's definition: its fields by name."})
}

/// A field `timeline.inspect` left out of a definition.
#[derive(Debug, Serialize, JsonSchema)]
pub struct OmittedField {
    /// The field's name.
    pub field: String,
    /// What it holds.
    pub what: String,
}

/// One input of a feature.
#[derive(Debug, Serialize, JsonSchema)]
pub struct FeatureInput {
    /// What the input is, in words.
    pub label: String,
    /// The parameter it is bound to; null when none.
    pub param: Option<String>,
    /// Its expression.
    pub expression: String,
    /// The unit its number is read in.
    pub unit: String,
}

// The feature folder commands, Reorder, Move Timeline Items and Edit Feature (#13131):
// `timeline.group`, `ungroup`, `move_to_group`, `rename_group`, `reorder`, `reorder_many`,
// `move_many` and `redefine`.
//
// As above, each key reads as its handler reads it and fails with the handler's message; whether
// a feature, joint or folder exists, and whether a move would break the timeline, stays the
// handler's to check. A key a handler reads as text, a number or a flag, and otherwise takes as
// absent, is a plain optional field: the contract is the stricter of the two.

fn feature_and_position_required(schema: &mut Schema) {
    require(schema, "feature");
    require(schema, "position");
}

fn features_and_position_required(schema: &mut Schema) {
    require(schema, "features");
    require(schema, "position");
}

fn group_and_name_required(schema: &mut Schema) {
    require(schema, "group");
    require(schema, "name");
}

fn feature_and_command_required(schema: &mut Schema) {
    require(schema, "feature");
    require(schema, "command");
}

fn move_needs_an_item(schema: &mut Schema) {
    require_any(schema, &["features", "joints", "groups"]);
}

fn move_many_needs_an_item(schema: &mut Schema) {
    require_any(schema, &["features", "groups"]);
    require(schema, "position");
}

fn ungroup_needs_a_folder(schema: &mut Schema) {
    require_any(schema, &["group", "groups"]);
}

/// A list of features as `group_features` reads `features`: a list of up to 10000 ids and names.
/// `null` is refused. `MIN_ONE` is for the command that needs at least one.
#[derive(Clone, Debug, PartialEq)]
pub struct FeatureList<const MIN_ONE: bool = false>(pub Vec<FeatureRef>);

impl<'de, const MIN_ONE: bool> Deserialize<'de> for FeatureList<MIN_ONE> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let Value::Array(list) = Value::deserialize(d)? else {
            return Err(D::Error::custom(whole("`features` must be a list of feature ids or names")));
        };
        if list.is_empty() && MIN_ONE {
            return Err(D::Error::custom(whole("`features` must list at least one feature")));
        }
        if list.len() > MAX_LIST {
            return Err(D::Error::custom(whole("too many features")));
        }
        list.into_iter().map(FeatureRef::deserialize).collect::<Result<_, _>>().map(FeatureList).map_err(|e| D::Error::custom(e.to_string()))
    }
}

impl<const MIN_ONE: bool> JsonSchema for FeatureList<MIN_ONE> {
    fn schema_name() -> Cow<'static, str> {
        if MIN_ONE { "FeatureListNonEmpty" } else { "FeatureList" }.into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let one = generator.subschema_for::<FeatureRef>();
        json_schema!({
            "description": "Features, each by id or name.",
            "type": "array",
            "items": one,
            "minItems": usize::from(MIN_ONE),
            "maxItems": MAX_LIST
        })
    }
}

/// `joints`, as `group_joints` reads it: a list of up to 10000 joint ids and names. `null` is
/// refused.
#[derive(Clone, Debug, PartialEq)]
pub struct JointList(pub Vec<Value>);

impl<'de> Deserialize<'de> for JointList {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match Value::deserialize(d)? {
            Value::Array(list) if list.len() <= MAX_LIST => {
                if list.iter().all(|v| v.is_number() || v.is_string()) {
                    Ok(JointList(list))
                } else {
                    Err(D::Error::custom(whole("`joint` must be an id or a name")))
                }
            }
            _ => Err(D::Error::custom(whole("`joints` must list at most 10000 joint ids or names"))),
        }
    }
}

impl JsonSchema for JointList {
    fn schema_name() -> Cow<'static, str> {
        "JointList".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "Joints, each by id or name.",
            "type": "array",
            "items": {"type": ["integer", "string"], "minimum": 0},
            "maxItems": MAX_LIST
        })
    }
}

/// A new folder's name as `timeline.group` reads `name`: text, which once the whitespace around
/// it is trimmed is at most 128 bytes long. Nothing left means the default name.
#[derive(Clone, Debug, PartialEq)]
pub struct FolderName(pub String);

impl<'de> Deserialize<'de> for FolderName {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match Value::deserialize(d)? {
            Value::String(s) if s.trim().len() > 128 => Err(D::Error::custom(whole("`name` must be 1…128 characters"))),
            Value::String(s) => Ok(FolderName(s)),
            // The handler reads a name that isn't text as absent, so the folder gets the default
            // name: a call that works today, left alone here.
            _ => Ok(FolderName(String::new())),
        }
    }
}

impl JsonSchema for FolderName {
    fn schema_name() -> Cow<'static, str> {
        "FolderName".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "Text at most 128 bytes long once the whitespace around it is trimmed (the bytes of its UTF-8, so fewer characters when some are not ASCII). Blank text means the default name. Anything but text is read as no name.",
            "type": "string"
        })
    }
}

/// `timeline.group`'s `parent`: one timeline folder id.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ParentFolder(pub u64);

impl<'de> Deserialize<'de> for ParentFolder {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Value::deserialize(d)?.as_u64().map(ParentFolder).ok_or_else(|| D::Error::custom(whole("`parent` must be a timeline folder id")))
    }
}

impl JsonSchema for ParentFolder {
    fn schema_name() -> Cow<'static, str> {
        "ParentFolder".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({"description": "A timeline folder, by id.", "type": "integer", "minimum": 0})
    }
}

/// `timeline.ungroup`'s `groups`: a list of 1 to 10000 timeline folder ids.
#[derive(Clone, Debug, PartialEq)]
pub struct FoldersToRemove(pub Vec<u64>);

impl<'de> Deserialize<'de> for FoldersToRemove {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match Value::deserialize(d)? {
            Value::Array(a) if !a.is_empty() && a.len() <= MAX_LIST => a
                .iter()
                .map(|v| v.as_u64().ok_or_else(|| D::Error::custom(whole("`groups` must contain timeline folder ids"))))
                .collect::<Result<_, _>>()
                .map(FoldersToRemove),
            Value::Array(_) => Err(D::Error::custom(whole("`groups` must list 1…10000 timeline folder ids"))),
            _ => Err(D::Error::custom(whole("`groups` must be a list of timeline folder ids"))),
        }
    }
}

impl JsonSchema for FoldersToRemove {
    fn schema_name() -> Cow<'static, str> {
        "FoldersToRemove".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "Timeline folders, by id.",
            "type": "array",
            "items": {"type": "integer", "minimum": 0},
            "minItems": 1,
            "maxItems": MAX_LIST
        })
    }
}

/// A `position` as `params::num` reads it: a number of absolute value below 1e9. A number below
/// 0 is read as 0, and a fraction is cut to a whole number.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SlotNumber(pub f64);

impl<'de> Deserialize<'de> for SlotNumber {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Value::deserialize(d)?
            .as_f64()
            .and_then(crate::params::finite)
            .map(SlotNumber)
            .ok_or_else(|| D::Error::custom(whole("`position` must be a number")))
    }
}

impl JsonSchema for SlotNumber {
    fn schema_name() -> Cow<'static, str> {
        "SlotNumber".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({"type": "number", "exclusiveMinimum": -1e9, "exclusiveMaximum": 1e9})
    }
}

/// A flag the move commands read: `null` is read as absent, and anything but true or false is
/// refused with `message`.
fn move_flag<'de, D: Deserializer<'de>>(d: D, message: &str) -> Result<Option<bool>, D::Error> {
    match Value::deserialize(d)? {
        Value::Null => Ok(None),
        Value::Bool(b) => Ok(Some(b)),
        _ => Err(D::Error::custom(whole(message))),
    }
}

fn active_flag<'de, D: Deserializer<'de>>(d: D) -> Result<Option<bool>, D::Error> {
    move_flag(d, "`active` must be true or false")
}

fn allow_errors_flag<'de, D: Deserializer<'de>>(d: D) -> Result<Option<bool>, D::Error> {
    move_flag(d, "`allow_errors` must be true or false")
}

/// A feature command's id, as `timeline.redefine` reads `command`: text.
#[derive(Clone, Debug, PartialEq)]
pub struct CommandId(pub String);

impl<'de> Deserialize<'de> for CommandId {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match Value::deserialize(d)? {
            Value::String(s) => Ok(CommandId(s)),
            _ => Err(D::Error::custom(whole("`command` must name the feature's command"))),
        }
    }
}

impl JsonSchema for CommandId {
    fn schema_name() -> Cow<'static, str> {
        "CommandId".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "The id of the command that makes the feature (`solid.extrude`, `solid.fillet`, …). Only the commands that make features are taken.",
            "type": "string"
        })
    }
}

/// `timeline.redefine`'s `params`: the feature command's own parameters. The handler passes
/// whatever it is given on to that command, which checks it, so any value is read here.
#[derive(Clone, Debug, PartialEq)]
pub struct CommandParams(pub Value);

impl<'de> Deserialize<'de> for CommandParams {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Value::deserialize(d).map(CommandParams)
    }
}

impl JsonSchema for CommandParams {
    fn schema_name() -> Cow<'static, str> {
        "CommandParams".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "The parameters of the feature's command, as `describe_command` shows them for it: the feature is rebuilt from them. Keys it takes that this leaves out take their defaults, except that some are kept from the feature being edited: an extrude's or revolve's `convention` and an extrude's run-to body, a feature's `participants`, a frame member's `names` (when `body_name` is unchanged), a thread's old size, a perforation's outline and an emboss's face. Any value is read here and passed on, and the command refuses what is not its parameters. `name` is ignored: the feature keeps its name.",
            "type": "object"
        })
    }
}

/// Make a new timeline folder holding features, joints and other folders, inside another folder
/// or at the top level. The features and joints leave any other folder they were in. Each folder
/// must hold one unbroken run of features, so the features given must be next to each other in
/// the history. One undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct TimelineGroupParams {
    /// Features to put in the folder, by id or name. Default: none.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "FeatureList")]
    pub features: Option<FeatureList>,
    /// Joints to put in the folder, by id or name. Default: none.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "JointList")]
    pub joints: Option<JointList>,
    /// Folders to move into the new one, by id. Default: none.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "FolderIds")]
    pub groups: Option<FolderIds>,
    /// The folder's name. Default: `Folder` and the number of folders there will be.
    #[serde(default)]
    pub name: Option<FolderName>,
    /// The folder to put the new one in, by id. Default: the top level. `null` is read as absent.
    #[serde(default)]
    pub parent: Option<ParentFolder>,
}

/// What `timeline.group` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct TimelineGroupMade {
    /// The new folder, by id.
    pub group: u64,
    /// Its name, without the whitespace around it.
    pub name: String,
    /// The folder it is in, by id; `null` at the top level.
    pub parent: Option<u64>,
    /// The folders moved into it, by id.
    pub groups: Vec<u64>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// The keys `timeline.ungroup` reads. `groups` is read first: when it is given, `group` is not
/// read at all.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = ungroup_needs_a_folder)]
pub struct UngroupKeys {
    /// One folder to remove, by id. Ignored when `groups` is given; `null` is refused.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "FolderId")]
    pub group: Option<FolderId>,
    /// Folders to remove, by id: 1 to 10000. `null` is refused.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "FoldersToRemove")]
    pub groups: Option<FoldersToRemove>,
}

/// `timeline.ungroup`'s parameters: [`UngroupKeys`], read as the handler reads them, with
/// `group` not looked at when `groups` is given.
#[derive(Debug)]
pub struct TimelineUngroupParams(pub UngroupKeys);

impl<'de> Deserialize<'de> for TimelineUngroupParams {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let mut value = Value::deserialize(d)?;
        if let Value::Object(o) = &mut value
            && o.contains_key("groups")
        {
            o.remove("group");
        }
        serde_path_to_error::deserialize(&value).map(TimelineUngroupParams).map_err(|e| D::Error::custom(located(&e)))
    }
}

impl JsonSchema for TimelineUngroupParams {
    fn schema_name() -> Cow<'static, str> {
        "TimelineUngroupParams".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let mut schema = UngroupKeys::json_schema(generator);
        schema.insert(
            "description".into(),
            "Remove timeline folders. The features, joints and folders in them stay in the timeline, and go up to the folder that held the removed one, or to the top level. Give `groups`, or `group` for one. One undo step.".into(),
        );
        schema
    }
}

/// What `timeline.ungroup` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct TimelineUngrouped {
    /// The folders removed, by id, each once.
    pub groups: Vec<u64>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// Rename a timeline folder (the one that holds features and joints).
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = group_and_name_required)]
pub struct TimelineRenameGroupParams {
    // Optional in the type and required in the schema (`group_and_name_required`).
    /// The folder, by id.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "FolderId")]
    pub group: Option<FolderId>,
    /// The new name.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "NewName")]
    pub name: Option<NewName>,
}

/// What `timeline.rename_group` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct TimelineGroupRenamed {
    /// The folder, by id.
    pub group: u64,
    /// Its name now, without the whitespace around it.
    pub name: String,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// Move features, joints and folders into a timeline folder, or to the top level. They leave the
/// folders they were in, and each folder must still hold one unbroken run of features. Give at
/// least one of `features`, `joints` and `groups`. One undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = move_needs_an_item)]
pub struct TimelineMoveToGroupParams {
    /// Features to move, by id or name. Default: none.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "FeatureList")]
    pub features: Option<FeatureList>,
    /// Joints to move, by id or name. Default: none.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "JointList")]
    pub joints: Option<JointList>,
    /// Folders to move, by id. A folder can't go into itself or one of its own folders. Default:
    /// none.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "FolderIds")]
    pub groups: Option<FolderIds>,
    /// The folder to move them into, by id. Default: the top level, also when `null`.
    #[serde(default)]
    pub group: Option<FolderId>,
}

/// What `timeline.move_to_group` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct TimelineMovedToGroup {
    /// The features moved, by id.
    pub features: Vec<u64>,
    /// The joints moved, by id.
    pub joints: Vec<u64>,
    /// The folders moved, by id.
    pub groups: Vec<u64>,
    /// The folder they are in now, by id; `null` at the top level.
    pub group: Option<u64>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// Move features and folders (with everything in them) to a place in the timeline and into a
/// folder, together, as one block and one undo step. The block keeps the order its features had.
/// Give at least one of `features` and `groups`. Refused when a feature that worked would fail.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = move_many_needs_an_item)]
pub struct TimelineMoveManyParams {
    /// Features to move, by id or name. Default: none.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "FeatureList")]
    pub features: Option<FeatureList>,
    /// Folders to move, by id, with their features and nested folders. A folder can't go into
    /// itself or one of its own folders. Default: none.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "FolderIds")]
    pub groups: Option<FolderIds>,
    /// The folder to move them into, by id. Default: the top level, also when `null`.
    #[serde(default)]
    pub group: Option<FolderId>,
    // Optional in the type and required in the schema (`move_many_needs_an_item`).
    /// Where the block goes: the insertion slot among the timeline's features, counted before
    /// the block is taken out. A number below 0 is 0, a fraction is cut to a whole number, and a
    /// number past the end is the end.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "SlotNumber")]
    pub position: Option<SlotNumber>,
    /// True: the moved features end up before the history marker. False: behind it. Other
    /// features keep their state. Default: the marker stays where it is. `null` is read as
    /// absent.
    #[serde(default, deserialize_with = "active_flag", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<bool>")]
    pub active: Option<bool>,
}

/// What `timeline.move_many` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct TimelineMoved {
    /// The features moved, by id, in the order they now have: those given and those of the
    /// folders, each once.
    pub features: Vec<u64>,
    /// The folders moved, by id.
    pub groups: Vec<u64>,
    /// The folder they are in now, by id; `null` at the top level.
    pub group: Option<u64>,
    /// The slot asked for, as a whole number of at least 0.
    pub position: usize,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// A feature a move broke, which worked before.
#[derive(Debug, Serialize, JsonSchema)]
pub struct BrokenFeature {
    /// The feature's name.
    pub feature: String,
    /// Why it fails now.
    pub error: String,
}

// `timeline.reorder` reads `allow_errors` only once it knows the feature moves, which depends
// on the design, so the contract takes any value there and leaves a wrong one to the handler,
// which refuses it with its own message; reading it here would refuse a call that works.

/// Move one feature to another place in the timeline. Refused when a feature that worked would
/// fail, unless `allow_errors` is true. A feature in a timeline folder must stay next to the
/// others of its folder.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = feature_and_position_required)]
pub struct TimelineReorderParams {
    // Optional in the type and required in the schema (`feature_and_position_required`).
    /// The feature to move, by id or name.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "FeatureRef")]
    pub feature: Option<FeatureRef>,
    /// Its new index in the timeline, from 0. A number below 0 is 0, a fraction is cut to a whole
    /// number, and a number past the end is the last slot (the reply's `note` says so).
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "SlotNumber")]
    pub position: Option<SlotNumber>,
    /// True: the feature ends up before the history marker. False: behind it. Other features keep
    /// their state. Default: the marker stays where it is. `null` is read as absent.
    #[serde(default, deserialize_with = "active_flag", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<bool>")]
    pub active: Option<bool>,
    /// True: make the move even when it breaks features that worked; they are listed in `broken`.
    /// Default: false, the move is refused. `null` is read as absent. Anything but true, false
    /// or `null` is refused, except when `allow_errors` is not read: when the feature is already
    /// at `position` (and `active` is not given) nothing is moved, and a move that would break a
    /// timeline folder is refused first.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<bool>")]
    pub allow_errors: Option<Value>,
}

/// What `timeline.reorder` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct TimelineReordered {
    /// The feature, by id.
    pub feature: u64,
    /// Its index now.
    pub position: usize,
    /// Its index before.
    pub from: usize,
    /// Whether it changed place.
    pub moved: bool,
    /// Why nothing moved, or that a position past the end meant the last slot. Absent when there
    /// is nothing to say.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// With `allow_errors`: the features the move broke; empty when it broke none. Absent
    /// otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub broken: Option<Vec<BrokenFeature>>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// Move several features together to one place in the timeline, as one block that keeps the
/// order the features have there. Refused when a feature that worked would fail, unless
/// `allow_errors` is true.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = features_and_position_required)]
pub struct TimelineReorderManyParams {
    // Optional in the type and required in the schema (`features_and_position_required`).
    /// The features to move, by id or name: at least one.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "FeatureList<true>")]
    pub features: Option<FeatureList<true>>,
    /// Where the block goes: the insertion slot among the timeline's features, counted before the
    /// block is taken out. A number below 0 is 0, a fraction is cut to a whole number, and a
    /// number past the end is the end.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "SlotNumber")]
    pub position: Option<SlotNumber>,
    /// True: the moved features end up before the history marker. False: behind it. Other
    /// features keep their state. Default: the marker stays where it is. `null` is read as
    /// absent.
    #[serde(default, deserialize_with = "active_flag", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<bool>")]
    pub active: Option<bool>,
    /// True: make the move even when it breaks features that worked; they are listed in `broken`.
    /// Default: false, the move is refused. `null` is read as absent.
    #[serde(default, deserialize_with = "allow_errors_flag", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<bool>")]
    pub allow_errors: Option<bool>,
}

/// What `timeline.reorder_many` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct TimelineReorderedMany {
    /// The features moved, by id, in the order they now have.
    pub features: Vec<u64>,
    /// The slot asked for, at most the number of features the timeline holds.
    pub position: usize,
    /// With `allow_errors`: the features the move broke; empty when it broke none. Absent
    /// otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub broken: Option<Vec<BrokenFeature>>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// Rebuild a feature in place from its command and parameters, keeping its place in the
/// timeline, its name and its folder. The feature's type can't change. Only the commands that
/// make features are taken. When the edited feature fails to build, the call returns that error
/// and the design is left as it was.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = feature_and_command_required)]
pub struct TimelineRedefineParams {
    // Optional in the type and required in the schema (`feature_and_command_required`).
    /// The feature, by id or name.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "FeatureRef")]
    pub feature: Option<FeatureRef>,
    /// The command that makes the feature, as `timeline.inspect` and the timeline name it.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "CommandId")]
    pub command: Option<CommandId>,
    /// That command's parameters. Default: none.
    #[serde(default)]
    pub params: Option<CommandParams>,
}

/// What `timeline.redefine` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct TimelineRedefined {
    /// The feature, by id.
    pub feature: u64,
    /// How many features were rebuilt.
    pub recomputed: usize,
    /// How many features took an earlier result for the same inputs instead of being rebuilt.
    pub restored: usize,
    /// Whether the feature is after the history marker, so the edit is not built yet.
    pub rolled_back: bool,
    /// What the feature's command noticed building it. Absent when there is nothing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
    /// With `rolled_back`: that the feature is not built, and what to do about it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::{
        AssistantParams, CheckpointParams, CheckpointResult, FeatureKeys, FeatureRef, FeatureSelection, FolderId, FolderIds, MarkerPosition, NewName,
        PaletteParams, RedoParams, RedoResult, RepeatParams, RestoreCheckpointParams, SetFields, TimelineComputeAllParams, TimelineComputeAllResult,
        TimelineDeleteParams, TimelineDeleteResult, TimelineDependentsParams, TimelineDependentsResult, TimelineEditParams, TimelineEditResult,
        TimelineInspectParams, TimelineInspectResult, TimelineMarkerResult, TimelineRenameParams, TimelineRenameResult, TimelineRollToParams,
        TimelineRollbackParams, TimelineSuppressParams, TimelineSuppressResult, ToggleVisibilityParams, UndoParams, UndoResult,
    };
    use super::{
        TimelineGroupMade, TimelineGroupParams, TimelineGroupRenamed, TimelineMoveManyParams, TimelineMoveToGroupParams, TimelineMoved,
        TimelineMovedToGroup, TimelineRedefineParams, TimelineRedefined, TimelineRenameGroupParams, TimelineReorderManyParams, TimelineReorderParams,
        TimelineReordered, TimelineReorderedMany, TimelineUngroupParams, TimelineUngrouped,
    };
    use crate::Session;
    use crate::schema::tests::{assert_keys, assert_strict_keeps_todays_errors, conforms};
    use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

    #[test]
    fn schemas_and_the_key_check_agree() {
        assert_keys(input_schema_for::<RepeatParams>(), &[]);
        assert_keys(input_schema_for::<AssistantParams>(), &[]);
        assert_keys(input_schema_for::<PaletteParams>(), &[]);
        assert_keys(input_schema_for::<ToggleVisibilityParams>(), &[]);
        assert_keys(input_schema_for::<UndoParams>(), &[]);
        assert_keys(input_schema_for::<RedoParams>(), &[]);
        assert_keys(input_schema_for::<CheckpointParams>(), &[]);
        assert_keys(input_schema_for::<RestoreCheckpointParams>(), &["checkpoint", "forget"]);
        assert_keys(input_schema_for::<TimelineComputeAllParams>(), &[]);
        assert_keys(input_schema_for::<TimelineDeleteParams>(), &["feature", "features", "groups"]);
        assert_keys(input_schema_for::<TimelineRenameParams>(), &["feature", "name"]);
        assert_keys(input_schema_for::<TimelineRollbackParams>(), &["feature", "position"]);
        assert_keys(input_schema_for::<TimelineRollToParams>(), &["feature", "position"]);
        assert_keys(input_schema_for::<TimelineDependentsParams>(), &["feature"]);
        assert_keys(input_schema_for::<TimelineSuppressParams>(), &["feature", "features", "groups", "suppressed"]);
        assert_keys(input_schema_for::<TimelineEditParams>(), &["feature", "set"]);
        assert_keys(input_schema_for::<TimelineInspectParams>(), &["feature", "group"]);
        assert_keys(input_schema_for::<TimelineGroupParams>(), &["features", "groups", "joints", "name", "parent"]);
        assert_keys(input_schema_for::<TimelineUngroupParams>(), &["group", "groups"]);
        assert_keys(input_schema_for::<TimelineRenameGroupParams>(), &["group", "name"]);
        assert_keys(input_schema_for::<TimelineMoveToGroupParams>(), &["features", "group", "groups", "joints"]);
        assert_keys(input_schema_for::<TimelineMoveManyParams>(), &["active", "features", "group", "groups", "position"]);
        assert_keys(input_schema_for::<TimelineReorderParams>(), &["active", "allow_errors", "feature", "position"]);
        assert_keys(input_schema_for::<TimelineReorderManyParams>(), &["active", "allow_errors", "features", "position"]);
        assert_keys(input_schema_for::<TimelineRedefineParams>(), &["command", "feature", "params"]);
    }

    #[test]
    fn strict_keeps_todays_errors() {
        assert_strict_keeps_todays_errors(&[
            ("edit.restore_checkpoint", json!({"checkpoint": "first"})),
            ("edit.restore_checkpoint", json!({"checkpoint": -1})),
            ("timeline.delete", json!({})),
            ("timeline.delete", json!({"features": []})),
            ("timeline.delete", json!({"features": true})),
            ("timeline.delete", json!({"features": [true]})),
            ("timeline.delete", json!({"features": ["Nope"]})),
            ("timeline.delete", json!({"features": [-1]})),
            ("timeline.delete", json!({"feature": null})),
            ("timeline.delete", json!({"feature": [1]})),
            ("timeline.delete", json!({"feature": 99})),
            ("timeline.delete", json!({"feature": 1.5})),
            ("timeline.delete", json!({"features": vec![1; 10_001]})),
            ("timeline.delete", json!({"groups": null})),
            ("timeline.delete", json!({"groups": 3})),
            ("timeline.delete", json!({"groups": ["a"]})),
            ("timeline.delete", json!({"groups": [77]})),
            ("timeline.delete", json!({"groups": vec![1; 10_001]})),
            ("timeline.rename", json!({})),
            ("timeline.rename", json!({"name": "x"})),
            ("timeline.rename", json!({"feature": null, "name": "x"})),
            ("timeline.rename", json!({"feature": true, "name": "x"})),
            ("timeline.rename", json!({"feature": 1})),
            ("timeline.rename", json!({"feature": 1, "name": ""})),
            ("timeline.rename", json!({"feature": 1, "name": "   "})),
            ("timeline.rename", json!({"feature": 1, "name": 5})),
            ("timeline.rename", json!({"feature": 1, "name": null})),
            ("timeline.rename", json!({"feature": 1, "name": "x".repeat(129)})),
            ("timeline.rename", json!({"feature": "Nope", "name": "x"})),
            ("timeline.rollback", json!({"position": "start"})),
            ("timeline.rollback", json!({"position": true})),
            ("timeline.rollback", json!({"position": [1]})),
            ("timeline.rollback", json!({"feature": null})),
            ("timeline.rollback", json!({"feature": true, "position": 1})),
            ("timeline.rollback", json!({"feature": "Nope"})),
            ("timeline.roll_to", json!({"position": "first"})),
            ("timeline.roll_to", json!({"position": "NaN"})),
            ("timeline.roll_to", json!({"position": 1e12})),
            ("timeline.roll_to", json!({"feature": {"id": 1}})),
            ("timeline.roll_to", json!({"feature": 99})),
            ("timeline.dependents", json!({})),
            ("timeline.dependents", json!({"feature": null})),
            ("timeline.dependents", json!({"feature": false})),
            ("timeline.dependents", json!({"feature": "Nope"})),
            ("timeline.suppress", json!({})),
            ("timeline.suppress", json!({"feature": true})),
            ("timeline.suppress", json!({"features": {}})),
            ("timeline.suppress", json!({"features": [true]})),
            ("timeline.suppress", json!({"features": vec![1; 10_001]})),
            ("timeline.suppress", json!({"feature": 1, "suppressed": "yes"})),
            ("timeline.suppress", json!({"feature": 1, "suppressed": 1})),
            ("timeline.suppress", json!({"feature": 1, "groups": null})),
            ("timeline.suppress", json!({"feature": 1, "groups": "a"})),
            ("timeline.suppress", json!({"feature": 1, "groups": [true]})),
            ("timeline.suppress", json!({"feature": "Nope"})),
            ("timeline.edit", json!({})),
            ("timeline.edit", json!({"set": {}})),
            ("timeline.edit", json!({"feature": null, "set": {}})),
            ("timeline.edit", json!({"feature": 1})),
            ("timeline.edit", json!({"feature": 1, "set": null})),
            ("timeline.edit", json!({"feature": 1, "set": []})),
            ("timeline.edit", json!({"feature": 1, "set": "radius"})),
            ("timeline.inspect", json!({})),
            ("timeline.inspect", json!({"feature": null})),
            ("timeline.inspect", json!({"feature": [1]})),
            ("timeline.inspect", json!({"feature": "Nope"})),
            ("timeline.inspect", json!({"group": null})),
            ("timeline.inspect", json!({"group": "a"})),
            ("timeline.inspect", json!({"group": -1})),
            ("timeline.inspect", json!({"group": 1.5})),
            ("timeline.inspect", json!({"group": 99})),
            ("timeline.group", json!({"features": "A"})),
            ("timeline.group", json!({"features": null})),
            ("timeline.group", json!({"features": [null]})),
            ("timeline.group", json!({"features": [99]})),
            ("timeline.group", json!({"features": ["Nope"]})),
            ("timeline.group", json!({"features": vec![1; 10_001]})),
            ("timeline.group", json!({"joints": null})),
            ("timeline.group", json!({"joints": [true]})),
            ("timeline.group", json!({"joints": ["Nope"]})),
            ("timeline.group", json!({"groups": 3})),
            ("timeline.group", json!({"groups": null})),
            ("timeline.group", json!({"groups": ["a"]})),
            ("timeline.group", json!({"groups": [77]})),
            ("timeline.group", json!({"name": "x".repeat(129)})),
            ("timeline.group", json!({"parent": "a"})),
            ("timeline.group", json!({"parent": 77})),
            ("timeline.ungroup", json!({})),
            ("timeline.ungroup", json!({"group": null})),
            ("timeline.ungroup", json!({"group": "a"})),
            ("timeline.ungroup", json!({"group": 77})),
            ("timeline.ungroup", json!({"groups": []})),
            ("timeline.ungroup", json!({"groups": null})),
            ("timeline.ungroup", json!({"groups": 3})),
            ("timeline.ungroup", json!({"groups": ["a"]})),
            ("timeline.ungroup", json!({"groups": [77]})),
            ("timeline.ungroup", json!({"groups": vec![1; 10_001]})),
            ("timeline.rename_group", json!({})),
            ("timeline.rename_group", json!({"name": "x"})),
            ("timeline.rename_group", json!({"group": null, "name": "x"})),
            ("timeline.rename_group", json!({"group": 77, "name": "x"})),
            ("timeline.move_to_group", json!({})),
            ("timeline.move_to_group", json!({"features": []})),
            ("timeline.move_to_group", json!({"features": "A"})),
            ("timeline.move_to_group", json!({"features": [true]})),
            ("timeline.move_to_group", json!({"joints": [1.5]})),
            ("timeline.move_to_group", json!({"groups": ["a"]})),
            ("timeline.move_to_group", json!({"features": [1], "group": "a"})),
            ("timeline.move_to_group", json!({"features": [1], "group": 77})),
            ("timeline.move_many", json!({})),
            ("timeline.move_many", json!({"features": [1]})),
            ("timeline.move_many", json!({"features": [1], "position": "1"})),
            ("timeline.move_many", json!({"features": [1], "position": null})),
            ("timeline.move_many", json!({"features": [1], "position": 1e9})),
            ("timeline.move_many", json!({"features": [1], "position": 0, "active": "yes"})),
            ("timeline.move_many", json!({"features": [1], "position": 0, "group": "a"})),
            ("timeline.reorder", json!({})),
            ("timeline.reorder", json!({"feature": 1})),
            ("timeline.reorder", json!({"feature": null, "position": 0})),
            ("timeline.reorder", json!({"feature": 99, "position": 0})),
            ("timeline.reorder", json!({"feature": 1, "position": "0"})),
            ("timeline.reorder", json!({"feature": 1, "position": 0, "active": 1})),
            ("timeline.reorder_many", json!({})),
            ("timeline.reorder_many", json!({"features": null, "position": 0})),
            ("timeline.reorder_many", json!({"features": [], "position": 0})),
            ("timeline.reorder_many", json!({"features": 1, "position": 0})),
            ("timeline.reorder_many", json!({"features": [true], "position": 0})),
            ("timeline.reorder_many", json!({"features": [1]})),
            ("timeline.reorder_many", json!({"features": [1], "position": 0, "allow_errors": 0})),
            ("timeline.redefine", json!({})),
            ("timeline.redefine", json!({"feature": 1})),
            ("timeline.redefine", json!({"feature": 1, "command": 5})),
            ("timeline.redefine", json!({"feature": 1, "command": null})),
            ("timeline.redefine", json!({"command": "solid.box"})),
            ("timeline.redefine", json!({"feature": true, "command": "solid.box"})),
            ("timeline.redefine", json!({"feature": 1, "command": "solid.nope"})),
            ("timeline.redefine", json!({"feature": 1, "command": "file.save"})),
        ]);
    }

    /// Undo, redo, a checkpoint, going back to it and forgetting one, in Warn: each reply matches
    /// its output schema and carries no warning.
    #[test]
    fn replies_match_the_output_schema() {
        let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
        let mut run = |id: &str, p: Value| s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
        let matches = |schema: schemars::Schema, id: &str, reply: &Value| {
            let schema = schema.as_value();
            assert!(reply.get("param_warnings").is_none() && conforms(schema, schema, reply), "{id}: {reply} doesn't match {schema}");
        };
        run("solid.box", json!({"length": 10, "width": 10, "height": 10}));
        matches(output_schema_for::<UndoResult>(), "edit.undo", &run("edit.undo", json!({})));
        matches(output_schema_for::<RedoResult>(), "edit.redo", &run("edit.redo", json!({})));
        let taken = run("edit.checkpoint", json!({}));
        matches(output_schema_for::<CheckpointResult>(), "edit.checkpoint", &taken);
        run("solid.box", json!({"length": 5, "width": 5, "height": 5, "corner": [20, 0, 0]}));
        let back = run("edit.restore_checkpoint", json!({"checkpoint": taken["checkpoint"]}));
        assert_eq!(back["restored"], true, "{back}");
        matches(output_schema_for::<CheckpointResult>(), "edit.restore_checkpoint", &back);
        let again = run("edit.checkpoint", json!({}));
        let dropped = run("edit.restore_checkpoint", json!({"checkpoint": again["checkpoint"], "forget": true}));
        assert_eq!(dropped["restored"], false, "{dropped}");
        matches(output_schema_for::<CheckpointResult>(), "edit.restore_checkpoint", &dropped);
    }

    /// A part with a box, a sketch and an extrude of it, and a timeline folder holding the box.
    fn timeline_session(param_check: ParamCheck) -> (Session, u64) {
        let mut s = Session::default();
        let mut run = |id: &str, p: Value| s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
        run("solid.box", json!({"name": "Base", "length": 10, "width": 10, "height": 10}));
        run("sketch.create", json!({"plane": "XY"}));
        run("sketch.rectangle.center", json!({"center": [30, 0], "corner": [5, 5]}));
        run("sketch.finish", json!({}));
        run("solid.extrude", json!({"name": "Pad", "distance": 4, "operation": "new"}));
        let group = run("timeline.group", json!({"name": "Housing", "features": ["Base"]}))["group"].as_u64().unwrap_or_default();
        s.param_check = param_check;
        (s, group)
    }

    fn matches(schema: schemars::Schema, id: &str, reply: &Value) {
        let schema = schema.as_value();
        assert!(conforms(schema, schema, reply), "{id}: {reply} doesn't match {schema}");
        assert!(reply.get("param_warnings").is_none(), "{id}: {reply}");
    }

    fn issues(id: &str, p: Value) -> Vec<String> {
        crate::schema::issues(id, &p).into_iter().map(|i| i.message).collect()
    }

    /// Every form of every command, run for real in Warn: each reply matches its output schema and
    /// carries no warning, so the contract takes the call the handler takes.
    #[test]
    fn timeline_replies_match_the_output_schema() {
        let (mut s, group) = timeline_session(ParamCheck::Warn);
        let mut run = |id: &str, p: Value| s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));

        let r = run("timeline.compute_all", json!({}));
        assert_eq!((r["features"].as_u64(), r["errors"].as_u64()), (Some(3), Some(0)), "{r}");
        matches(output_schema_for::<TimelineComputeAllResult>(), "timeline.compute_all", &r);

        for p in [json!({"feature": "Base"}), json!({"feature": 1}), json!({"feature": "Pad"})] {
            let r = run("timeline.inspect", p);
            assert!(r["definition"].is_object() && r["inputs"].is_array(), "{r}");
            matches(output_schema_for::<TimelineInspectResult>(), "timeline.inspect", &r);
        }
        // With `group` the folder is described and `feature` is not read, whatever it holds.
        for p in [
            json!({"group": group}),
            json!({"group": group, "feature": true}),
            json!({"group": group, "feature": null}),
            json!({"group": group, "feature": "Nope"}),
        ] {
            let r = run("timeline.inspect", p);
            assert_eq!(r["name"], "Housing", "{r}");
            matches(output_schema_for::<TimelineInspectResult>(), "timeline.inspect", &r);
        }
        let r = run("timeline.inspect", json!({"feature": "Pad"}));
        assert!(r["inputs"].as_array().is_some_and(|i| !i.is_empty()) && r["error"].is_null(), "{r}");

        let r = run("timeline.dependents", json!({"feature": "Base"}));
        matches(output_schema_for::<TimelineDependentsResult>(), "timeline.dependents", &r);
        let r = run("timeline.dependents", json!({"feature": 2}));
        assert_eq!(r["deleted_with_it"], json!(["Pad"]), "{r}");
        matches(output_schema_for::<TimelineDependentsResult>(), "timeline.dependents", &r);

        // The marker: a count, a count as text, the end in any case, null, a feature (which
        // makes `position` unread, whatever it holds).
        for cmd in ["timeline.rollback", "timeline.roll_to"] {
            for p in [
                json!({"position": 1}),
                json!({"position": "2"}),
                json!({"position": " End "}),
                json!({"position": null}),
                json!({}),
                json!({"feature": "Base"}),
                json!({"feature": 1, "position": true}),
                json!({"feature": "Base", "position": "start"}),
                json!({"feature": "Base", "position": -4}),
                json!({"feature": "Base", "position": null}),
            ] {
                let r = run(cmd, p.clone());
                assert!(r["marker"].is_u64() && r["features"] == 3, "{cmd} {p}: {r}");
                matches(output_schema_for::<TimelineMarkerResult>(), cmd, &r);
            }
        }
        assert_eq!(run("timeline.roll_to", json!({"feature": "Base", "position": 7}))["marker"], 1);

        // Suppress: one feature, a list, a single name as `features`, null keys read as absent, folders.
        for (p, shape) in [
            (json!({"feature": "Pad"}), "feature"),
            (json!({"feature": "Pad", "features": null, "groups": [], "suppressed": null}), "feature"),
            (json!({"features": ["Pad"], "suppressed": true}), "features"),
            (json!({"features": "Pad", "suppressed": false}), "features"),
            (json!({"feature": "Pad", "features": ["Base"]}), "features"),
            (json!({"features": [1, "Pad"]}), "features"),
            (json!({"groups": [group], "feature": null, "features": null}), "groups"),
            (json!({"groups": [group], "suppressed": false}), "groups"),
            (json!({"feature": "Pad", "groups": [group], "suppressed": true}), "groups"),
        ] {
            let r = run("timeline.suppress", p.clone());
            assert_eq!((r.get("feature").is_some(), r.get("groups").is_some()), (shape == "feature", shape == "groups"), "{p}: {r}");
            matches(output_schema_for::<TimelineSuppressResult>(), "timeline.suppress", &r);
        }
        run("timeline.suppress", json!({"features": ["Base", "Pad"], "suppressed": false}));

        // Edit.
        let r = run("timeline.edit", json!({"feature": "Pad", "set": {"extent": {"distance": "7"}}}));
        assert_eq!(r["feature"], 3, "{r}");
        matches(output_schema_for::<TimelineEditResult>(), "timeline.edit", &r);
        let r = run("timeline.edit", json!({"feature": 3, "set": {}}));
        matches(output_schema_for::<TimelineEditResult>(), "timeline.edit", &r);

        // Rename, also to the name it has (a reply without a change), and with spaces around it.
        for (feature, name, now) in [(json!("Base"), "  Block ", "Block"), (json!("Block"), "Block", "Block"), (json!(1), "Base", "Base")] {
            let r = run("timeline.rename", json!({"feature": feature, "name": name}));
            assert_eq!((r["feature"].as_u64(), r["name"].as_str()), (Some(1), Some(now)), "{r}");
            matches(output_schema_for::<TimelineRenameResult>(), "timeline.rename", &r);
        }

        // Delete: by name, by id, a list, a folder, with a later feature left failing.
        let r = run("timeline.delete", json!({"feature": "Pad", "features": null, "groups": []}));
        assert!(r["deleted"] == json!([3]) && r.get("joints").is_none() && r.get("groups").is_none() && r.get("failing").is_none(), "{r}");
        matches(output_schema_for::<TimelineDeleteResult>(), "timeline.delete", &r);
        run("edit.undo", json!({}));
        let r = run("timeline.delete", json!({"features": "Pad"}));
        matches(output_schema_for::<TimelineDeleteResult>(), "timeline.delete", &r);
        run("edit.undo", json!({}));
        let r = run("timeline.delete", json!({"features": [3, "Base"]}));
        assert_eq!(r["deleted"], json!([3, 1]), "{r}");
        matches(output_schema_for::<TimelineDeleteResult>(), "timeline.delete", &r);
        run("edit.undo", json!({}));
        let r = run("timeline.delete", json!({"groups": [group]}));
        assert!(r["deleted"] == json!([1]) && r["groups"] == json!([group]), "{r}");
        matches(output_schema_for::<TimelineDeleteResult>(), "timeline.delete", &r);
        run("edit.undo", json!({}));
    }

    /// A delete that leaves a later feature failing names it in `failing`.
    #[test]
    fn delete_reply_with_failing_features_matches_the_output_schema() {
        let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
        let mut run = |id: &str, p: Value| s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
        run("solid.box", json!({"name": "A", "body_name": "Plate", "length": 10, "width": 10, "height": 10}));
        run("solid.box", json!({"name": "B", "body_name": "Other", "corner": [30, 0, 0], "length": 10, "width": 10, "height": 10}));
        run("solid.box", json!({"name": "C", "corner": [2, 2, 2], "length": 3, "width": 3, "height": 3, "operation": "cut", "targets": "Plate"}));
        let r = run("timeline.delete", json!({"feature": "A"}));
        assert!(r["failing"].as_array().is_some_and(|f| f.len() == 1 && f[0]["feature"] == "C" && f[0]["error"].is_string()), "{r}");
        matches(output_schema_for::<TimelineDeleteResult>(), "timeline.delete", &r);
    }

    /// A call that works today gets no warning and none that would be refused in strict: what the
    /// types accept is what the handlers read, case by case (the other forms are in
    /// `timeline_replies_match_the_output_schema`).
    #[test]
    fn the_types_read_each_key_as_the_handlers_do() {
        // A key a handler doesn't read in this case may hold anything.
        for (id, p) in [
            ("timeline.inspect", json!({"group": 1, "feature": {"anything": [1]}})),
            ("timeline.rollback", json!({"feature": 1, "position": {"anything": true}})),
            ("timeline.roll_to", json!({"feature": "Base", "position": [1, 2]})),
            ("timeline.suppress", json!({"feature": null, "features": null, "groups": [1], "suppressed": null})),
            ("timeline.delete", json!({"features": null, "groups": [1]})),
            ("timeline.rollback", json!({"position": null})),
            ("timeline.rollback", json!({"position": "7"})),
            ("timeline.rollback", json!({"position": -3})),
            ("timeline.rename", json!({"feature": 3.0, "name": "é".repeat(64)})),
        ] {
            assert_eq!(issues(id, p.clone()), Vec::<String>::new(), "{id} {p}");
        }
        // What the handlers refuse, with their messages, as the check words it.
        for (id, p, message) in [
            ("timeline.inspect", json!({"group": null}), "`group` must be a timeline folder id"),
            ("timeline.inspect", json!({"feature": null}), "`feature` must be an id or name"),
            ("timeline.rollback", json!({"feature": null, "position": 1}), "`feature` must be an id or name"),
            ("timeline.roll_to", json!({"position": "x"}), "`position` must be a number of features to keep active, or \"end\""),
            ("timeline.delete", json!({"feature": null}), "`feature` must be an id or name"),
            ("timeline.delete", json!({"groups": null}), "`groups` must be a list of timeline folder ids"),
            ("timeline.delete", json!({"features": [true]}), "`features` must list names or ids; the entry 1 is true"),
            ("timeline.delete", json!({"features": 4}), "`features` must be a list of names or ids; it is 4"),
            ("timeline.suppress", json!({"features": [null]}), "`feature` must be an id or name"),
            ("timeline.suppress", json!({"features": {}}), "`features` must be a list of feature ids or names"),
            ("timeline.suppress", json!({"feature": 1, "suppressed": 0}), "`suppressed` must be true or false"),
            ("timeline.rename", json!({"feature": 1, "name": " "}), "`name` must be 1…128 characters"),
            ("timeline.edit", json!({"feature": 1, "set": 1}), "`set` must be an object of fields"),
        ] {
            assert_eq!(issues(id, p.clone()), [message], "{id} {p}");
        }
        // Absent keys are left to the handlers' messages.
        for (id, p) in [
            ("timeline.rename", json!({})),
            ("timeline.edit", json!({})),
            ("timeline.dependents", json!({})),
            ("timeline.inspect", json!({})),
            ("timeline.delete", json!({})),
            ("timeline.suppress", json!({})),
        ] {
            assert_eq!(issues(id, p.clone()), Vec::<String>::new(), "{id} {p}");
        }
        // The types, on their own.
        let read = |v: Value| {
            (
                serde_json::from_value::<FeatureRef>(v.clone()).is_ok(),
                serde_json::from_value::<FeatureKeys>(v.clone()).is_ok(),
                serde_json::from_value::<FeatureSelection>(v.clone()).is_ok(),
                serde_json::from_value::<FolderIds>(v.clone()).is_ok(),
                serde_json::from_value::<FolderId>(v.clone()).is_ok(),
                serde_json::from_value::<NewName>(v.clone()).is_ok(),
                serde_json::from_value::<MarkerPosition>(v.clone()).is_ok(),
                serde_json::from_value::<SetFields>(v).is_ok(),
            )
        };
        assert_eq!(read(json!(3)), (true, false, true, false, true, false, true, false));
        assert_eq!(read(json!("Pad")), (true, true, true, false, false, true, false, false));
        assert_eq!(read(json!([1, "Pad"])), (false, true, true, false, false, false, false, false));
        assert_eq!(read(json!([1, 2])), (false, true, true, true, false, false, false, false));
        assert_eq!(read(json!({"a": 1})), (false, false, false, false, false, false, false, true));
        assert_eq!(read(json!(true)), (false, false, false, false, false, false, false, false));
        assert_eq!(serde_json::from_value::<MarkerPosition>(json!(" eNd ")).ok(), Some(MarkerPosition::End));
        assert_eq!(serde_json::from_value::<MarkerPosition>(json!("2.5")).ok(), Some(MarkerPosition::Count(2.5)));
        assert!(serde_json::from_value::<MarkerPosition>(json!(1e9)).is_err());
        assert!(serde_json::from_value::<NewName>(json!("x".repeat(129))).is_err());
        assert!(serde_json::from_value::<NewName>(json!(format!("  {}  ", "x".repeat(128)))).is_ok());
    }

    /// The schemas say what the handlers need: the keys they refuse a call without are required,
    /// and a call that works matches.
    #[test]
    fn the_schemas_take_what_the_handlers_read() {
        let takes = |schema: schemars::Schema, cases: &[(Value, bool)]| {
            let schema = schema.as_value();
            for (p, takes) in cases {
                assert_eq!(conforms(schema, schema, p), *takes, "{p}");
            }
        };
        takes(input_schema_for::<TimelineComputeAllParams>(), &[(json!({}), true)]);
        takes(
            input_schema_for::<TimelineDeleteParams>(),
            &[
                (json!({"feature": "Pad"}), true),
                (json!({"features": ["Pad", 2]}), true),
                (json!({"features": "Pad", "groups": [1]}), true),
                (json!({"groups": [1]}), true),
                (json!({}), false),
                (json!({"feature": true}), false),
                (json!({"features": [true]}), false),
                (json!({"groups": "a"}), false),
            ],
        );
        takes(
            input_schema_for::<TimelineRenameParams>(),
            &[
                (json!({"feature": "Pad", "name": "Boss"}), true),
                (json!({"feature": 2, "name": "Boss"}), true),
                (json!({"feature": 2}), false),
                (json!({"name": "x"}), false),
            ],
        );
        for schema in [input_schema_for::<TimelineRollbackParams>(), input_schema_for::<TimelineRollToParams>()] {
            takes(
                schema,
                &[
                    (json!({}), true),
                    (json!({"position": 2}), true),
                    (json!({"position": "end"}), true),
                    (json!({"feature": "Pad"}), true),
                    (json!({"position": -1}), false),
                    (json!({"position": true}), false),
                    (json!({"feature": true}), false),
                ],
            );
        }
        // `null` is read as absent for these, as the types read it.
        takes(
            input_schema_for::<TimelineSuppressParams>(),
            &[(json!({"feature": "Pad", "suppressed": null}), true), (json!({"feature": null, "features": null, "groups": [1]}), true)],
        );
        for schema in [input_schema_for::<TimelineRollbackParams>(), input_schema_for::<TimelineRollToParams>()] {
            takes(schema, &[(json!({"position": null}), true)]);
        }
        takes(input_schema_for::<TimelineDependentsParams>(), &[(json!({"feature": 2}), true), (json!({}), false)]);
        takes(
            input_schema_for::<TimelineSuppressParams>(),
            &[
                (json!({"feature": "Pad"}), true),
                (json!({"features": ["Pad", 1], "suppressed": true}), true),
                (json!({"groups": [3]}), true),
                (json!({}), false),
                (json!({"feature": "Pad", "suppressed": "yes"}), false),
            ],
        );
        takes(
            input_schema_for::<TimelineEditParams>(),
            &[
                (json!({"feature": "Pad", "set": {"a": 1}}), true),
                (json!({"feature": "Pad"}), false),
                (json!({"set": {}}), false),
                (json!({"feature": "Pad", "set": 1}), false),
            ],
        );
        takes(
            input_schema_for::<TimelineInspectParams>(),
            &[(json!({"feature": "Pad"}), true), (json!({"group": 1}), true), (json!({}), false), (json!({"group": "a"}), false)],
        );
    }

    /// Known gap, as in `component.replace_with_link` (`path` with `with`): the check leaves a key
    /// unread that the handler doesn't read in that case, but the published schema still types
    /// every key, so it describes the call without the unread key's value. Pinned so a change to
    /// either is deliberate.
    #[test]
    fn unread_keys_pass_the_check_but_not_the_schema() {
        for (id, p) in [
            ("timeline.inspect", json!({"group": 1, "feature": true})),
            ("timeline.rollback", json!({"feature": 1, "position": true})),
            ("timeline.roll_to", json!({"feature": 1, "position": [1]})),
        ] {
            assert_eq!(issues(id, p.clone()), Vec::<String>::new(), "{id} {p}");
        }
        let schema = input_schema_for::<TimelineInspectParams>();
        assert!(!conforms(schema.as_value(), schema.as_value(), &json!({"group": 1, "feature": true})));
    }

    /// A misspelt key still runs in Warn, and says so, with a reply that matches its schema.
    #[test]
    fn a_misspelt_key_is_reported_in_warn() {
        let (mut s, group) = timeline_session(ParamCheck::Warn);
        let mut run = |id: &str, p: Value, schema: schemars::Schema| {
            let r = s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
            let warnings = r["param_warnings"].as_array().map(|w| w.iter().filter_map(Value::as_str).collect::<Vec<_>>()).unwrap_or_default();
            assert_eq!(warnings.len(), 1, "{id}: {r}");
            assert!(warnings[0].starts_with("unknown key `"), "{id}: {warnings:?}");
            assert!(conforms(schema.as_value(), schema.as_value(), &r), "{id}: {r}");
        };
        run("timeline.compute_all", json!({"feture": 1}), output_schema_for::<TimelineComputeAllResult>());
        run("timeline.inspect", json!({"featur": "Base", "feature": "Base"}), output_schema_for::<TimelineInspectResult>());
        run("timeline.inspect", json!({"gruop": 1, "group": group}), output_schema_for::<TimelineInspectResult>());
        run("timeline.dependents", json!({"feature": "Base", "feaure": 1}), output_schema_for::<TimelineDependentsResult>());
        run("timeline.rollback", json!({"postion": 1}), output_schema_for::<TimelineMarkerResult>());
        run("timeline.roll_to", json!({"feature": "Base", "postion": 1}), output_schema_for::<TimelineMarkerResult>());
        run("timeline.suppress", json!({"feature": "Pad", "supressed": true}), output_schema_for::<TimelineSuppressResult>());
        run("timeline.edit", json!({"feature": "Pad", "sett": {}, "set": {}}), output_schema_for::<TimelineEditResult>());
        run("timeline.rename", json!({"feature": "Pad", "nme": "x", "name": "Boss"}), output_schema_for::<TimelineRenameResult>());
        run("timeline.delete", json!({"feature": "Boss", "feautres": []}), output_schema_for::<TimelineDeleteResult>());
    }

    /// Strict refuses a misspelt key before anything runs.
    #[test]
    fn strict_refuses_a_misspelt_key_and_changes_nothing() {
        let (mut s, _) = timeline_session(ParamCheck::Strict);
        let before = s.doc.to_json();
        for (id, p) in
            [("timeline.delete", json!({"feature": "Pad", "feautre": 1})), ("timeline.suppress", json!({"feature": "Pad", "supressed": true}))]
        {
            let e = s.execute(id, &p).unwrap_err().to_string();
            assert!(e.contains("unknown key"), "{e}");
            assert_eq!(s.doc.to_json(), before);
        }
    }

    /// A part with boxes A, B, C and D (ids 1 to 4, each apart from the others), a component and a
    /// joint of it, Hinge.
    fn folder_session(param_check: ParamCheck) -> Session {
        let mut s = Session::default();
        let mut run = |id: &str, p: Value| s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
        for (i, name) in ["A", "B", "C", "D"].into_iter().enumerate() {
            run("solid.box", json!({"name": name, "length": 5, "width": 5, "height": 5, "corner": [i as f64 * 20.0, 0, 0]}));
        }
        run("component.create", json!({"name": "Part"}));
        run("joint.as_built", json!({"type": "revolute", "a": 0, "b": "Part", "name": "Hinge"}));
        s.param_check = param_check;
        s
    }

    fn feature_ids(s: &Session) -> Vec<u64> {
        s.doc.features.iter().map(|f| f.id).collect()
    }

    /// Every form of the folder commands, Reorder, Move Timeline Items and Edit Feature, run for
    /// real in Warn: each reply matches its output schema and carries no warning, so the
    /// contract takes the call the handler takes.
    #[test]
    fn folder_and_reorder_replies_match_the_output_schema() {
        let mut s = folder_session(ParamCheck::Warn);
        let ids = feature_ids(&s);
        let (a, b, c, d) = (ids[0], ids[1], ids[2], ids[3]);
        let mut run = |id: &str, p: Value| s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));

        // New folders: nothing in them, features, joints, a name with spaces, a blank name,
        // `null` for the keys that read it as absent, and a folder in a folder.
        let r = run("timeline.group", json!({}));
        assert_eq!((r["name"].as_str(), r["parent"].is_null(), &r["groups"]), (Some("Folder1"), true, &json!([])), "{r}");
        matches(output_schema_for::<TimelineGroupMade>(), "timeline.group", &r);
        let empty = r["group"].as_u64().unwrap_or_default();
        let r = run("timeline.group", json!({"name": "  Pair ", "features": ["A", b], "joints": ["Hinge"], "groups": [empty], "parent": null}));
        assert_eq!((r["name"].as_str(), &r["groups"]), (Some("Pair"), &json!([empty])), "{r}");
        matches(output_schema_for::<TimelineGroupMade>(), "timeline.group", &r);
        let pair = r["group"].as_u64().unwrap_or_default();
        let r = run("timeline.group", json!({"name": "   ", "features": [], "joints": [], "groups": [], "parent": pair}));
        assert_eq!((r["name"].as_str(), &r["parent"]), (Some("Folder3"), &json!(pair)), "{r}");
        matches(output_schema_for::<TimelineGroupMade>(), "timeline.group", &r);
        let inner = r["group"].as_u64().unwrap_or_default();
        let r = run("timeline.group", json!({"name": null, "features": [c]}));
        matches(output_schema_for::<TimelineGroupMade>(), "timeline.group", &r);
        let third = r["group"].as_u64().unwrap_or_default();
        let r = run("timeline.group", json!({"name": "é".repeat(64), "features": ["D"]}));
        matches(output_schema_for::<TimelineGroupMade>(), "timeline.group", &r);
        let fourth = r["group"].as_u64().unwrap_or_default();

        // Rename: with spaces around the name.
        let r = run("timeline.rename_group", json!({"group": third, "name": "  Third "}));
        assert_eq!((r["group"].as_u64(), r["name"].as_str()), (Some(third), Some("Third")), "{r}");
        matches(output_schema_for::<TimelineGroupRenamed>(), "timeline.rename_group", &r);

        // Move to a folder: features, joints, folders, to the top level with `null` or without `group`.
        let r = run("timeline.move_to_group", json!({"features": [c], "group": pair}));
        assert_eq!((&r["features"], &r["joints"], &r["group"]), (&json!([c]), &json!([]), &json!(pair)), "{r}");
        matches(output_schema_for::<TimelineMovedToGroup>(), "timeline.move_to_group", &r);
        let r = run("timeline.move_to_group", json!({"joints": ["Hinge"], "group": third}));
        assert_eq!(r["joints"].as_array().map(Vec::len), Some(1), "{r}");
        matches(output_schema_for::<TimelineMovedToGroup>(), "timeline.move_to_group", &r);
        let r = run("timeline.move_to_group", json!({"groups": [third], "features": [], "group": null}));
        assert!(r["group"].is_null() && r["groups"] == json!([third]), "{r}");
        matches(output_schema_for::<TimelineMovedToGroup>(), "timeline.move_to_group", &r);
        let r = run("timeline.move_to_group", json!({"features": ["C", c], "groups": [], "joints": []}));
        assert!(r["group"].is_null() && r["features"] == json!([c]), "{r}");
        matches(output_schema_for::<TimelineMovedToGroup>(), "timeline.move_to_group", &r);

        // Move many: features, folders, a folder, a place, the marker; a slot as a negative or
        // fractional number, and `null` for what reads it as absent.
        let r = run("timeline.move_many", json!({"features": [d], "position": 0}));
        assert_eq!((&r["features"], &r["group"], &r["position"]), (&json!([d]), &Value::Null, &json!(0)), "{r}");
        matches(output_schema_for::<TimelineMoved>(), "timeline.move_many", &r);
        let r = run("timeline.move_many", json!({"features": ["A"], "position": 2.7, "group": null, "active": null}));
        assert_eq!(r["position"], 2, "{r}");
        matches(output_schema_for::<TimelineMoved>(), "timeline.move_many", &r);
        let r = run("timeline.move_many", json!({"features": ["B"], "position": -3, "group": fourth, "active": true}));
        assert_eq!(r["position"], 0, "{r}");
        matches(output_schema_for::<TimelineMoved>(), "timeline.move_many", &r);
        let r = run("timeline.move_many", json!({"groups": [fourth], "features": [], "position": 100, "active": true}));
        matches(output_schema_for::<TimelineMoved>(), "timeline.move_many", &r);
        let r = run("timeline.move_many", json!({"groups": [fourth], "group": inner, "position": 1, "active": true}));
        matches(output_schema_for::<TimelineMoved>(), "timeline.move_many", &r);
        run("timeline.rollback", json!({"position": "end"}));

        // Remove folders: one, a list (also with a `group` that is not read), a list with a
        // folder twice.
        let r = run("timeline.ungroup", json!({"group": inner}));
        assert_eq!(r["groups"], json!([inner]), "{r}");
        matches(output_schema_for::<TimelineUngrouped>(), "timeline.ungroup", &r);
        let r = run("timeline.ungroup", json!({"groups": [fourth, fourth], "group": {"anything": [null]}}));
        assert_eq!(r["groups"], json!([fourth]), "{r}");
        matches(output_schema_for::<TimelineUngrouped>(), "timeline.ungroup", &r);
        let r = run("timeline.ungroup", json!({"groups": [pair, empty, third]}));
        matches(output_schema_for::<TimelineUngrouped>(), "timeline.ungroup", &r);

        // Reorder: a move, one place with a note (nothing moved; past the end), the marker, and
        // `allow_errors` with and without something broken.
        let r = run("timeline.reorder", json!({"feature": "A", "position": 3}));
        assert_eq!((&r["position"], &r["moved"]), (&json!(3), &json!(true)), "{r}");
        assert!(r.get("note").is_none() && r.get("broken").is_none(), "{r}");
        matches(output_schema_for::<TimelineReordered>(), "timeline.reorder", &r);
        let r = run("timeline.reorder", json!({"feature": a, "position": 3.9, "allow_errors": "not read here"}));
        assert_eq!(r["moved"], false, "{r}");
        assert!(r["note"].as_str().is_some_and(|n| n.starts_with("nothing moved")), "{r}");
        matches(output_schema_for::<TimelineReordered>(), "timeline.reorder", &r);
        let r = run("timeline.reorder", json!({"feature": "B", "position": 99, "active": null, "allow_errors": null}));
        assert!(r["note"].as_str().is_some_and(|n| n.contains("past the end")) && r["moved"] == true, "{r}");
        matches(output_schema_for::<TimelineReordered>(), "timeline.reorder", &r);
        let r = run("timeline.reorder", json!({"feature": "B", "position": -5, "active": true, "allow_errors": true}));
        assert_eq!(r["broken"], json!([]), "{r}");
        matches(output_schema_for::<TimelineReordered>(), "timeline.reorder", &r);
        run("timeline.rollback", json!({"position": 2}));
        let r = run("timeline.reorder", json!({"feature": "B", "position": 3, "active": false, "allow_errors": false}));
        assert!(r.get("broken").is_none(), "{r}");
        matches(output_schema_for::<TimelineReordered>(), "timeline.reorder", &r);
        run("timeline.rollback", json!({"position": "end"}));

        // Reorder many: a block, with the marker, with `allow_errors`.
        let r = run("timeline.reorder_many", json!({"features": ["A", c], "position": 4}));
        assert_eq!(r["position"], 4, "{r}");
        assert!(r.get("broken").is_none(), "{r}");
        matches(output_schema_for::<TimelineReorderedMany>(), "timeline.reorder_many", &r);
        let r = run("timeline.reorder_many", json!({"features": [d], "position": 0, "active": true, "allow_errors": true}));
        assert_eq!(r["broken"], json!([]), "{r}");
        matches(output_schema_for::<TimelineReorderedMany>(), "timeline.reorder_many", &r);
        let r = run("timeline.reorder_many", json!({"features": [b, "A"], "position": 100.5, "active": null, "allow_errors": false}));
        assert_eq!(r["position"], 4, "{r}");
        matches(output_schema_for::<TimelineReorderedMany>(), "timeline.reorder_many", &r);

        // Edit Feature: params of the command, none that change anything, a name that is
        // ignored, and a feature after the history marker.
        let r = run("timeline.redefine", json!({"feature": "A", "command": "solid.box", "params": {"length": 7, "width": 5, "height": 5}}));
        assert_eq!((&r["feature"], &r["rolled_back"]), (&json!(a), &json!(false)), "{r}");
        assert!(r.get("note").is_none() && r.get("warning").is_none(), "{r}");
        matches(output_schema_for::<TimelineRedefined>(), "timeline.redefine", &r);
        let r = run(
            "timeline.redefine",
            json!({"feature": b, "command": "solid.box", "params": {"name": "Other", "length": 7, "width": 5, "height": 5, "corner": [20, 0, 0]}}),
        );
        matches(output_schema_for::<TimelineRedefined>(), "timeline.redefine", &r);
        run("timeline.rollback", json!({"position": 0}));
        let r = run("timeline.redefine", json!({"feature": "D", "command": "solid.box", "params": {"length": 9, "width": 5, "height": 5}}));
        assert_eq!(r["rolled_back"], true, "{r}");
        assert!(r["note"].as_str().is_some_and(|n| n.contains("after the history marker")), "{r}");
        matches(output_schema_for::<TimelineRedefined>(), "timeline.redefine", &r);
    }

    /// Reorder, Reorder Many and Edit Feature with something in the way: a move that is made
    /// anyway lists what it broke, and an edit that gives a warning says so.
    #[test]
    fn replies_with_broken_features_and_warnings_match_the_output_schema() {
        let (mut s, _) = timeline_session(ParamCheck::Warn);
        let mut run = |id: &str, p: Value| s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
        // The extrude can't come before its sketch.
        let r = run("timeline.reorder", json!({"feature": "Pad", "position": 1, "allow_errors": true}));
        assert!(r["broken"].as_array().is_some_and(|b| b.len() == 1 && b[0]["feature"] == "Pad" && b[0]["error"].is_string()), "{r}");
        matches(output_schema_for::<TimelineReordered>(), "timeline.reorder", &r);
        run("edit.undo", json!({}));
        let r = run("timeline.reorder_many", json!({"features": ["Pad"], "position": 1, "allow_errors": true}));
        assert!(r["broken"].as_array().is_some_and(|b| !b.is_empty()), "{r}");
        matches(output_schema_for::<TimelineReorderedMany>(), "timeline.reorder_many", &r);
    }

    /// A call that works today gets no warning and none that would be refused in strict: what the
    /// types accept is what the handlers read (the other forms are in
    /// `folder_and_reorder_replies_match_the_output_schema`).
    #[test]
    fn the_folder_types_read_each_key_as_the_handlers_do() {
        // Keys a handler doesn't read in this case may hold anything; `null` is read as absent
        // where the handler does so.
        for (id, p) in [
            ("timeline.ungroup", json!({"groups": [1], "group": null})),
            ("timeline.ungroup", json!({"groups": [1], "group": "x"})),
            ("timeline.group", json!({"name": "", "parent": null, "features": [1, "A", 1], "joints": [3, "H"], "groups": []})),
            ("timeline.group", json!({"name": 5, "features": [1]})),
            ("timeline.group", json!({"name": "x".repeat(128), "parent": 4})),
            ("timeline.group", json!({"name": format!("  {}  ", "x".repeat(128))})),
            ("timeline.move_to_group", json!({"features": [1], "group": null})),
            ("timeline.move_many", json!({"features": [1], "position": -1e8, "active": null, "group": null})),
            ("timeline.reorder", json!({"feature": 1.0, "position": 1e8, "allow_errors": null, "active": null})),
            ("timeline.reorder", json!({"feature": "A", "position": 0.5})),
            ("timeline.reorder", json!({"feature": 1, "position": 0, "allow_errors": "no"})),
            ("timeline.reorder_many", json!({"features": ["A", 2], "position": 0})),
            ("timeline.redefine", json!({"feature": 1, "command": "solid.box"})),
            ("timeline.redefine", json!({"feature": 1, "command": "solid.box", "params": null})),
            ("timeline.redefine", json!({"feature": 1, "command": "solid.box", "params": [1]})),
        ] {
            assert_eq!(issues(id, p.clone()), Vec::<String>::new(), "{id} {p}");
        }
        // What the handlers refuse, with their messages, as the check words it.
        for (id, p, message) in [
            ("timeline.group", json!({"features": "A"}), "`features` must be a list of feature ids or names"),
            ("timeline.group", json!({"features": null}), "`features` must be a list of feature ids or names"),
            ("timeline.group", json!({"features": [null]}), "`feature` must be an id or name"),
            ("timeline.group", json!({"features": vec![1; 10_001]}), "too many features"),
            ("timeline.group", json!({"joints": null}), "`joints` must list at most 10000 joint ids or names"),
            ("timeline.group", json!({"joints": [true]}), "`joint` must be an id or a name"),
            ("timeline.group", json!({"groups": 3}), "`groups` must be a list of timeline folder ids"),
            ("timeline.group", json!({"groups": [-1]}), "`groups` must contain folder ids"),
            ("timeline.group", json!({"name": "x".repeat(129)}), "`name` must be 1…128 characters"),
            ("timeline.group", json!({"parent": "a"}), "`parent` must be a timeline folder id"),
            ("timeline.ungroup", json!({"groups": []}), "`groups` must list 1…10000 timeline folder ids"),
            ("timeline.ungroup", json!({"groups": null}), "`groups` must be a list of timeline folder ids"),
            ("timeline.ungroup", json!({"groups": ["a"]}), "`groups` must contain timeline folder ids"),
            ("timeline.ungroup", json!({"group": null}), "`group` must be a timeline folder id"),
            ("timeline.rename_group", json!({"group": 1, "name": " "}), "`name` must be 1…128 characters"),
            ("timeline.rename_group", json!({"group": "a", "name": "x"}), "`group` must be a timeline folder id"),
            ("timeline.move_to_group", json!({"group": "a"}), "`group` must be a timeline folder id"),
            ("timeline.move_many", json!({"features": [1], "position": "1"}), "`position` must be a number"),
            ("timeline.move_many", json!({"features": [1], "position": 1, "active": "yes"}), "`active` must be true or false"),
            ("timeline.reorder", json!({"feature": 1, "position": 1e9}), "`position` must be a number"),
            ("timeline.reorder", json!({"feature": null, "position": 1}), "`feature` must be an id or name"),
            ("timeline.reorder_many", json!({"features": [], "position": 1}), "`features` must list at least one feature"),
            ("timeline.reorder_many", json!({"features": 1, "position": 1}), "`features` must be a list of feature ids or names"),
            ("timeline.redefine", json!({"feature": 1, "command": 5}), "`command` must name the feature's command"),
            ("timeline.redefine", json!({"feature": true, "command": "solid.box"}), "`feature` must be an id or name"),
        ] {
            assert_eq!(issues(id, p.clone()), [message], "{id} {p}");
        }
        // Absent keys are left to the handlers' messages.
        for (id, p) in [
            ("timeline.ungroup", json!({})),
            ("timeline.rename_group", json!({})),
            ("timeline.move_to_group", json!({})),
            ("timeline.move_many", json!({})),
            ("timeline.reorder", json!({})),
            ("timeline.reorder_many", json!({})),
            ("timeline.redefine", json!({})),
        ] {
            assert_eq!(issues(id, p.clone()), Vec::<String>::new(), "{id} {p}");
        }
    }

    /// The schemas say what the handlers need: the keys they refuse a call without are required,
    /// and a call that works matches.
    #[test]
    fn the_folder_schemas_take_what_the_handlers_read() {
        let takes = |schema: schemars::Schema, cases: &[(Value, bool)]| {
            let schema = schema.as_value();
            for (p, takes) in cases {
                assert_eq!(conforms(schema, schema, p), *takes, "{p}");
            }
        };
        takes(
            input_schema_for::<TimelineGroupParams>(),
            &[
                (json!({}), true),
                (json!({"name": "Box", "features": ["A", 2], "joints": ["H", 1], "groups": [3], "parent": 4}), true),
                (json!({"features": "A"}), false),
                (json!({"joints": null}), false),
                (json!({"parent": "x"}), false),
            ],
        );
        takes(
            input_schema_for::<TimelineUngroupParams>(),
            &[
                (json!({"group": 1}), true),
                (json!({"groups": [1, 2]}), true),
                (json!({}), false),
                (json!({"groups": []}), false),
                (json!({"group": null}), false),
            ],
        );
        takes(
            input_schema_for::<TimelineRenameGroupParams>(),
            &[
                (json!({"group": 1, "name": "x"}), true),
                (json!({"group": 1}), false),
                (json!({"name": "x"}), false),
                (json!({"group": 1, "name": ""}), true),
            ],
        );
        takes(
            input_schema_for::<TimelineMoveToGroupParams>(),
            &[
                (json!({"features": [1]}), true),
                (json!({"joints": ["H"], "group": null}), true),
                (json!({"groups": [2], "group": 3}), true),
                (json!({}), false),
                (json!({"group": 3}), false),
            ],
        );
        takes(
            input_schema_for::<TimelineMoveManyParams>(),
            &[
                (json!({"features": [1], "position": 2}), true),
                (json!({"groups": [1], "position": -2.5, "group": 3, "active": false}), true),
                (json!({"features": [1]}), false),
                (json!({"position": 1}), false),
                (json!({"features": [1], "position": 1e9}), false),
                (json!({"features": [1], "position": 1, "active": 1}), false),
            ],
        );
        takes(
            input_schema_for::<TimelineReorderParams>(),
            &[
                (json!({"feature": "A", "position": 2}), true),
                (json!({"feature": 1, "position": 2, "active": true, "allow_errors": false}), true),
                (json!({"feature": "A"}), false),
                (json!({"position": 2}), false),
                (json!({"feature": "A", "position": "2"}), false),
            ],
        );
        takes(
            input_schema_for::<TimelineReorderManyParams>(),
            &[
                (json!({"features": ["A", 2], "position": 0}), true),
                (json!({"features": [], "position": 0}), false),
                (json!({"features": ["A"]}), false),
                (json!({"position": 0}), false),
            ],
        );
        takes(
            input_schema_for::<TimelineRedefineParams>(),
            &[
                (json!({"feature": "A", "command": "solid.box", "params": {"length": 1}}), true),
                (json!({"feature": "A", "command": "solid.box"}), true),
                (json!({"feature": "A"}), false),
                (json!({"command": "solid.box"}), false),
                (json!({"feature": "A", "command": "solid.box", "params": 1}), false),
            ],
        );
    }

    /// A misspelt key still runs in Warn, and says so, with a reply that matches its schema.
    #[test]
    fn a_misspelt_folder_key_is_reported_in_warn() {
        let mut s = folder_session(ParamCheck::Warn);
        let mut run = |id: &str, p: Value, schema: schemars::Schema| {
            let r = s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
            let warnings = r["param_warnings"].as_array().map(|w| w.iter().filter_map(Value::as_str).collect::<Vec<_>>()).unwrap_or_default();
            assert_eq!(warnings.len(), 1, "{id}: {r}");
            assert!(warnings[0].starts_with("unknown key `"), "{id}: {warnings:?}");
            assert!(conforms(schema.as_value(), schema.as_value(), &r), "{id}: {r}");
            r
        };
        let made = run("timeline.group", json!({"name": "F", "feautres": ["A"], "features": ["A"]}), output_schema_for::<TimelineGroupMade>());
        let group = made["group"].as_u64().unwrap_or_default();
        run("timeline.rename_group", json!({"group": group, "nme": "x", "name": "G"}), output_schema_for::<TimelineGroupRenamed>());
        run("timeline.move_to_group", json!({"features": ["B"], "gruop": 1, "group": group}), output_schema_for::<TimelineMovedToGroup>());
        // `joints` is not read by `timeline.move_many`, so naming it is a mistake too.
        run("timeline.move_many", json!({"features": ["B"], "joints": ["Hinge"], "position": 1}), output_schema_for::<TimelineMoved>());
        run("timeline.reorder", json!({"feature": "B", "postion": 1, "position": 2}), output_schema_for::<TimelineReordered>());
        run("timeline.reorder_many", json!({"features": ["B"], "position": 0, "alow_errors": true}), output_schema_for::<TimelineReorderedMany>());
        run(
            "timeline.redefine",
            json!({"feature": "A", "command": "solid.box", "parmas": {}, "params": {"length": 6, "width": 5, "height": 5}}),
            output_schema_for::<TimelineRedefined>(),
        );
        run("timeline.ungroup", json!({"groups": [group], "grops": 1}), output_schema_for::<TimelineUngrouped>());
    }

    /// Strict refuses a misspelt key before anything runs, and a call with only the keys the
    /// handler reads goes through.
    #[test]
    fn strict_refuses_a_misspelt_folder_key_and_changes_nothing() {
        let mut s = folder_session(ParamCheck::Strict);
        let before = s.doc.to_json();
        for (id, p) in [
            ("timeline.group", json!({"features": ["A"], "feautres": 1})),
            ("timeline.ungroup", json!({"group": 1, "grop": 1})),
            ("timeline.rename_group", json!({"group": 1, "name": "x", "nme": 1})),
            ("timeline.move_to_group", json!({"features": ["A"], "gruop": 1})),
            ("timeline.move_many", json!({"features": ["A"], "position": 0, "joints": []})),
            ("timeline.reorder", json!({"feature": "A", "position": 2, "postion": 1})),
            ("timeline.reorder_many", json!({"features": ["A"], "position": 2, "alow_errors": true})),
            ("timeline.redefine", json!({"feature": "A", "command": "solid.box", "parmas": {}})),
        ] {
            let e = s.execute(id, &p).unwrap_err().to_string();
            assert!(e.contains("unknown key"), "{id}: {e}");
            assert_eq!(s.doc.to_json(), before, "{id}");
        }
        let r = s.execute("timeline.group", &json!({"name": "F", "features": ["A", "B"]})).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(r["name"], "F", "{r}");
    }
}
