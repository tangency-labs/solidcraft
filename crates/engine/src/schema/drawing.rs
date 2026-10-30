//! Contracts and result types for the commands in `cmd/drawing.rs` (#3875): the templates a drawing
//! starts from.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Give each sheet of the drawing open the paper, drawing border and title block of a template's
/// sheet, moving nothing.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct TemplateApplyParams {
    /// The template's name, as `drawing.template.list` lists it. Its sheet in each sheet's place
    /// is applied, its last for the sheets past its own.
    pub template: String,
    /// True to answer what would change without changing anything. Default: false.
    #[serde(default, deserialize_with = "super::present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "bool")]
    pub dry_run: Option<bool>,
}

/// A sheet a template changes, and what of it.
#[derive(Debug, Serialize, JsonSchema)]
pub struct SheetTemplateChange {
    pub sheet: String,
    /// It takes the template sheet's paper.
    pub paper: bool,
    /// It takes the template sheet's drawing border.
    pub border: bool,
    /// It takes the template sheet's title block.
    pub title_block: bool,
    /// The cells whose typed text is dropped: the new block has no text cell of their name.
    pub lost: Vec<String>,
}

/// What `drawing.template.apply` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct TemplateApplyResult {
    /// Each sheet that changes, in order.
    pub changed: Vec<SheetTemplateChange>,
    /// The views on a restyled sheet that fall outside its drawing space or over its title block.
    /// None is moved.
    pub outside: Vec<super::drawing_view::OutsideResult>,
    /// Whether the call only answered, changing nothing.
    pub dry_run: bool,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

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
