//! Contracts and result types for the named selection commands in `cmd/named_selection.rs`
//! (#18716): a selection kept under a name in the design, to select again in one click.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Keep a selection under a name: the items given, or what is selected now. One undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct SelectionSaveParams {
    /// The name, at most 128 characters once surrounding spaces are cut; no other named selection
    /// may have it. Default: "Selection", the next number, and what it holds ("Selection1 – 3
    /// components, 2 bodies").
    #[serde(default)]
    pub name: Option<String>,
    /// The items, as `select.set` takes them. Default: the current selection.
    #[serde(default)]
    pub items: Option<Vec<Value>>,
}

/// What `named_selection.save` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct SelectionSaved {
    /// The name the selection was kept under.
    pub selection: String,
    /// How many items it holds.
    pub count: usize,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// Select a named selection's items again, in place of the current selection. Items that no
/// longer exist are skipped. Not an undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct SelectionApplyParams {
    /// The named selection, by name.
    pub selection: String,
}

/// What `named_selection.apply` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct SelectionApplied {
    pub selection: String,
    /// How many items are selected now.
    pub selected: usize,
    /// How many of its items no longer exist and were skipped.
    pub skipped: usize,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// Replace a named selection's items with the items given, or with what is selected now. One
/// undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct SelectionUpdateParams {
    /// The named selection, by name.
    pub selection: String,
    /// The items, as `select.set` takes them. Default: the current selection.
    #[serde(default)]
    pub items: Option<Vec<Value>>,
}

/// Rename a named selection. One undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct SelectionRenameParams {
    /// The named selection, by name.
    pub selection: String,
    /// The new name, 1 to 128 characters once surrounding spaces are cut; no other named
    /// selection may have it.
    pub name: String,
}

/// What `named_selection.rename` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct SelectionRenamed {
    /// Its name now.
    pub selection: String,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// Delete a named selection; its items stay in the design. One undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct SelectionDeleteParams {
    /// The named selection, by name.
    pub selection: String,
}

/// What `named_selection.delete` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct SelectionDeleted {
    /// The name of the named selection that is gone.
    pub deleted: String,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// `named_selection.list` takes no parameters.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct SelectionListParams {}

/// A named selection, as `named_selection.list` gives it.
#[derive(Debug, Serialize, JsonSchema)]
pub struct SelectionEntry {
    pub name: String,
    /// Its items, as `select.set` takes them.
    pub items: Vec<Value>,
    /// Whether SolidCraft made the name (then it follows an update); left out when not.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub generated: bool,
}

/// What `named_selection.list` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct SelectionListResult {
    /// The design's named selections, in the order they were made.
    #[schemars(with = "Vec<SelectionEntry>")]
    pub selections: Vec<solidcraft_doc::NamedSelection>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}
