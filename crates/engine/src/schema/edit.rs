//! Contracts and result types for the commands in `cmd/edit.rs`.

use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

use super::check::whole;
use crate::PickMode;

/// Assistant visibility is UI-owned and takes no parameters.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct AssistantParams {}

/// Headless sessions refuse Assistant display requests and produce no result.
#[derive(Debug, Serialize, JsonSchema)]
pub struct AssistantResult {}

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
/// back. Either uses it up and every later checkpoint with it: take a new one to go back again.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct RestoreCheckpointParams {
    /// The checkpoint, by the id `edit.checkpoint` returned.
    pub checkpoint: CheckpointId,
    /// True to drop the checkpoint (and every later one) without going back. Default: false.
    #[serde(default)]
    pub forget: Option<bool>,
}

/// Choose what a click on the model picks while no dialog or tool takes the pick: faces, edges
/// and vertices; the face's whole body; the feature that made it; the innermost placement or the
/// top-level component holding it; or sketch geometry only. A window setting: the design, its
/// history and the selection are untouched.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct SelectModeParams {
    /// The selection mode.
    #[serde(deserialize_with = "pick_mode")]
    #[schemars(schema_with = "pick_mode_schema")]
    pub mode: PickMode,
}

/// A selection mode, read by the handler's own reader.
fn pick_mode<'de, D: serde::Deserializer<'de>>(d: D) -> Result<PickMode, D::Error> {
    crate::cmd::pick_mode(&Value::deserialize(d)?).map_err(|e| serde::de::Error::custom(whole(e)))
}

/// The modes' ids, as one string enum.
fn pick_mode_schema(_: &mut SchemaGenerator) -> Schema {
    let ids: Vec<Value> = PickMode::ALL.iter().filter_map(|m| serde_json::to_value(m).ok()).collect();
    json_schema!({
        "description": "faces (faces, edges and vertices), bodies, features, parts (the innermost placement), components (the top-level placement) or sketch (sketch curves, points and profiles).",
        "type": "string",
        "enum": ids
    })
}

/// What `select.mode` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct SelectModeResult {
    /// The selection mode now on.
    #[schemars(schema_with = "pick_mode_schema")]
    pub mode: PickMode,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

#[cfg(test)]
#[path = "edit_tests.rs"]
mod tests;
