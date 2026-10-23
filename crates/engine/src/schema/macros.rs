//! Contracts and reply types for the commands in `cmd/macros.rs` (PR #5651).
//!
//! The macro recorder keeps its state on the session, so most of these commands take nothing
//! and reply with the recorder's state. `macro.play` takes the macro to run (source, a saved
//! name or a path — at most one, which the handler checks) and the values for its `@param`
//! declarations; the values' kinds are the macro's own dialect, so the contract types the
//! object and the handler checks each value against its `@param`.

use schemars::{JsonSchema, Schema};
use serde::Deserialize;
use serde::Serialize;
use serde_json::{Map, Value};

/// Adds `keys` to the schema's `required` list: for a key the type has optional so that the
/// handler, not the check, refuses it when it's left out.
fn require_keys(schema: &mut Schema, keys: &[&str]) {
    let mut required = match schema.get("required") {
        Some(Value::Array(required)) => required.clone(),
        _ => Vec::new(),
    };
    for key in keys {
        if !required.iter().any(|k| k == key) {
            required.push((*key).into());
        }
    }
    schema.insert("required".into(), Value::Array(required));
}

/// A command that takes no parameters: `macro.pause`, `macro.resume`, `macro.stop`,
/// `macro.clear` and `macro.list` read nothing.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct MacroNoParams {}

/// Start recording the commands that change the design (`macro.record`).
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct MacroRecordParams {
    /// The macro's name: a header comment on the recorded source and Save's default file
    /// name. Default: the design's name.
    #[serde(default)]
    pub name: Option<String>,
}

/// Play a macro (`macro.play`): its source, or where to read it, and its parameter values.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct MacroPlayParams {
    /// Rhai source to play. Give at most one of `source`, `name` and `path`; give none to
    /// play the loaded macro.
    #[serde(default)]
    pub source: Option<String>,
    /// A saved macro's file name in the macros folder (as `macro.list` reports them).
    #[serde(default)]
    pub name: Option<String>,
    /// Any `.rhai` file's path.
    #[serde(default)]
    pub path: Option<String>,
    /// Values for the macro's `@param` declarations, `{name: value}`. Each value is checked
    /// against its parameter's kind (text, int, number, length, angle, bool, choice) by the
    /// handler, which says what a bad one is.
    #[serde(default)]
    pub params: Option<Map<String, Value>>,
}

/// Load a macro file (`macro.open`). `path` is optional in the type so that the handler's
/// message, not the check's "is required", is what a caller sees when it is left out.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = open_path_required)]
pub struct MacroOpenParams {
    /// A `.rhai` file's path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

fn open_path_required(schema: &mut Schema) {
    require_keys(schema, &["path"]);
}

/// Write the recording to a file (`macro.save`).
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct MacroSaveParams {
    /// A file name in the macros folder, ending in `.rhai`. Default: the macro's name.
    #[serde(default)]
    pub name: Option<String>,
    /// Any `.rhai` file's path. Give a name or a path, not both.
    #[serde(default)]
    pub path: Option<String>,
}

/// Delete a saved macro (`macro.delete`). `name` is optional in the type so that the
/// handler's message is what a caller sees when it is left out.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = delete_name_required)]
pub struct MacroDeleteParams {
    /// A saved macro's file name (see `macro.list`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

fn delete_name_required(schema: &mut Schema) {
    require_keys(schema, &["name"]);
}

/// What `macro.record`, `macro.pause` and `macro.resume` return: the recorder's state.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MacroState {
    /// Whether a recording is running.
    pub recording: bool,
    /// Whether the recording is paused (it captures nothing while paused).
    pub paused: bool,
    /// Commands captured so far.
    pub steps: usize,
}

/// What `macro.stop` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MacroStopped {
    /// Always false: the recording has ended.
    pub recording: bool,
    /// Always false.
    pub paused: bool,
    /// Commands captured; Play runs them, Save writes them.
    pub steps: usize,
    /// Whether the recording ended at the step limit rather than at Stop.
    pub capped: bool,
}

/// What `macro.play` returns when the macro ran; a failed play rolls back and errors.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MacroPlayed {
    /// Always true.
    pub ran: bool,
    /// `run(...)` calls the source holds.
    pub steps: usize,
}

/// What `macro.open` returns; the macro also becomes the loaded one.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MacroOpened {
    /// The `@name` header, or empty.
    pub name: String,
    /// `run(...)` calls the file holds.
    pub steps: usize,
    /// The `@param` declarations, in order.
    pub params: Vec<crate::cmd::macros::MacroParam>,
}

/// What `macro.save` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MacroSaved {
    /// The file written.
    pub path: String,
    /// `run(...)` calls written.
    pub steps: usize,
}

/// What `macro.clear` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MacroCleared {
    /// Always true.
    pub cleared: bool,
}

/// What `macro.list` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MacroListed {
    /// Whether a recording is running.
    pub recording: bool,
    /// Whether the recording is paused.
    pub paused: bool,
    /// Whether the last recording ended at the step limit.
    pub capped: bool,
    /// Commands captured in the current or loaded macro.
    pub steps: usize,
    /// The macros folder, or null where the system has none (the browser).
    pub dir: Option<String>,
    /// The saved macros, by name.
    pub macros: Vec<MacroEntry>,
}

/// One saved macro, as `macro.list` reports it.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MacroEntry {
    /// The file name, ending in `.rhai`.
    pub name: String,
    /// The file's size in bytes.
    pub bytes: u64,
}

/// What `macro.delete` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MacroDeleted {
    /// The file name deleted.
    pub deleted: String,
}

#[cfg(test)]
#[path = "macros_tests.rs"]
mod tests;
