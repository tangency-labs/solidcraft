//! Contracts and result types for the commands in `cmd/edit.rs`.

use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

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

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::{
        AssistantParams, CheckpointParams, CheckpointResult, RedoParams, RedoResult, RepeatParams, RestoreCheckpointParams, UndoParams, UndoResult,
    };
    use crate::Session;
    use crate::schema::tests::{assert_keys, assert_strict_keeps_todays_errors, conforms};
    use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

    #[test]
    fn schemas_and_the_key_check_agree() {
        assert_keys(input_schema_for::<RepeatParams>(), &[]);
        assert_keys(input_schema_for::<AssistantParams>(), &[]);
        assert_keys(input_schema_for::<UndoParams>(), &[]);
        assert_keys(input_schema_for::<RedoParams>(), &[]);
        assert_keys(input_schema_for::<CheckpointParams>(), &[]);
        assert_keys(input_schema_for::<RestoreCheckpointParams>(), &["checkpoint", "forget"]);
    }

    #[test]
    fn strict_keeps_todays_errors() {
        assert_strict_keeps_todays_errors(&[
            ("edit.restore_checkpoint", json!({"checkpoint": "first"})),
            ("edit.restore_checkpoint", json!({"checkpoint": -1})),
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
}
