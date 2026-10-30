//! Contracts and result types for the commands in `cmd/drawing.rs` (#3875): the templates a drawing
//! starts from.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// List the templates a new drawing can start from.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct TemplateListParams {}

/// One template a drawing can start from.
#[derive(Debug, Serialize, JsonSchema)]
pub struct TemplateEntry {
    /// Its name, as `drawing.create`'s `template` takes it.
    pub name: String,
    /// What the app calls it.
    pub title: String,
    /// Whether `drawing.create` starts from it for this design when no template is named.
    pub default: bool,
}

/// What `drawing.template.list` returns: the built-in templates, ISO A4 to A0 for a part and for
/// an assembly.
#[derive(Debug, Serialize, JsonSchema)]
pub struct TemplateListResult {
    pub templates: Vec<TemplateEntry>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

#[cfg(test)]
#[path = "drawing_tests.rs"]
mod tests;
