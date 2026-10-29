//! Contracts and result types for the commands in `cmd/edit.rs`.

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::check::whole;
use crate::PickMode;

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
