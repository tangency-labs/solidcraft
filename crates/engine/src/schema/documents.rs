//! Contracts and result types for the commands that list the open designs and show one of them
//! (`document.list`, `document.activate`).
//!
//! The engine holds one design, so these describe what a command line or headless server answers
//! (its one design). The desktop window answers them for every design tab it has open.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// List the designs that are open, in tab order. Reads only.
#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DocumentListParams {}

/// One open design in `document.list`.
#[derive(Debug, Serialize, JsonSchema)]
pub struct OpenDocument {
    /// The design's id, the one `document.revision` gives. Kept while the design stays open,
    /// through saves and renames; another for each design started or opened.
    pub document_id: String,
    /// The design's name, as its tab shows it.
    pub name: String,
    /// The file the design is saved in, or null while it has none.
    pub path: Option<String>,
    /// Whether this is the design every other command acts on.
    pub active: bool,
    /// Whether the design has changes that are not saved.
    pub dirty: bool,
}

/// What `document.list` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct DocumentListResult {
    /// The open designs, in tab order.
    pub documents: Vec<OpenDocument>,
    /// The `document_id` of the design every other command acts on now.
    pub active_document_id: String,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// Show an open design in its tab and make it the one every other command acts on.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DocumentActivateParams {
    /// The design's id, as `document.list` or `file.new` with `new_tab` gives it.
    pub document_id: String,
}

/// What `document.activate` returns: the design now shown.
#[derive(Debug, Serialize, JsonSchema)]
pub struct DocumentActivateResult {
    /// The design now active.
    pub document_id: String,
    /// Its name.
    pub name: String,
    /// The file it is saved in, or null while it has none.
    pub path: Option<String>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}
