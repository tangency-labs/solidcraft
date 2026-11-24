//! Contracts and result types for the commands in `cmd/display.rs`.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Keep what is shown now, and how the bodies look, as a display state of the design. One undo
/// step. A state of the same name is replaced and keeps its place in the list. The camera is not
/// kept: named views do that.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct DisplaySaveParams {
    /// The state's name, at most 128 bytes once surrounding spaces are cut. Default: "Display
    /// State" and the first number no state's name has.
    #[serde(default)]
    pub name: Option<String>,
}

/// What `display.save` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct DisplaySaved {
    /// The name the state was kept under.
    pub state: String,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// Show what a display state shows and give the bodies the looks it kept, as one undo step
/// (what is hidden is undone like any `browser.visibility` change). Bodies the state does not
/// name are shown, and names of bodies that are gone are ignored. The camera does not move.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct DisplayApplyParams {
    /// The state to apply, by name.
    pub state: String,
}

/// What `display.apply` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct DisplayApplied {
    /// The state that was applied.
    pub state: String,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// Rename a display state. One undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct DisplayRenameParams {
    /// The state to rename, by name.
    pub state: String,
    /// The new name, 1 to 128 bytes once surrounding spaces are cut; no other state may have it.
    pub name: String,
}

/// What `display.rename` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct DisplayRenamed {
    /// The state's name now.
    pub state: String,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// Delete a display state. One undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct DisplayDeleteParams {
    /// The state to delete, by name.
    pub state: String,
}

/// What `display.delete` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct DisplayDeleted {
    /// The name of the state that is gone.
    pub deleted: String,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// `display.list` takes no parameters.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct DisplayListParams {}

/// A display state, as `display.list` gives it. Only its outline is described here: the reply
/// carries the design's own [`solidcraft_doc::display_state::NamedDisplayState`] values.
#[derive(Debug, Serialize, JsonSchema)]
pub struct DisplayStateEntry {
    /// The state's name.
    pub name: String,
    /// What is hidden or shown, as the design file's `visibility` lists.
    pub snapshot: serde_json::Value,
    /// How the bodies looked when it was saved; absent in a state that keeps no looks.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub appearances: Option<serde_json::Value>,
}

/// What `display.list` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct DisplayListResult {
    /// The design's display states, in the order they were made.
    #[schemars(with = "Vec<DisplayStateEntry>")]
    pub states: Vec<solidcraft_doc::display_state::NamedDisplayState>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// Leave the top isolation layer (`browser.visibility` with `isolate` adds one): what was shown
/// before it comes back exactly, and so does whatever was hidden by hand while it was active.
/// One undo step. Does nothing when not isolated.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct DisplayUnisolateParams {}

/// What `display.unisolate` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct DisplayUnisolated {
    /// Whether a layer was left (false: the design was not isolated).
    pub left: bool,
    /// How many layers are still active.
    pub level: usize,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// Leave every isolation layer: the view from before the first isolation comes back exactly.
/// One undo step. Does nothing when not isolated.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct DisplayUnisolateAllParams {}

/// What `display.unisolate_all` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct DisplayUnisolatedAll {
    /// Whether any layer was left (false: the design was not isolated).
    pub left: bool,
    /// How many layers are active: 0.
    pub level: usize,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}
