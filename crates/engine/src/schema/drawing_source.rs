//! Contracts and result types for the command in `cmd/drawing_source.rs` (#13029).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::drawing_view::ViewIdParam;

/// What `drawing.source.replace` takes: exactly one of `view`, `sheet` and `drawing`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(deny_unknown_fields)]
pub struct ReplaceParams {
    /// A base view: it and the views projected from it show `to`.
    #[serde(default, deserialize_with = "super::present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "ViewIdParam")]
    pub view: Option<ViewIdParam>,
    /// A sheet, by id or name: its base views, the component it documents and its parts lists.
    #[serde(default, deserialize_with = "super::present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub sheet: Option<String>,
    /// `true`: every sheet of the drawing, as `sheet` takes one.
    #[serde(default, deserialize_with = "super::present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "bool")]
    pub drawing: Option<bool>,
    /// The part to show instead.
    pub to: ToParam,
    /// `true` to answer without changing anything.
    #[serde(default, deserialize_with = "super::present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "bool")]
    pub dry_run: Option<bool>,
}

/// A component of the design, where it is placed.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(deny_unknown_fields)]
pub struct ToParam {
    /// Its id; 0 is the design itself.
    pub component: u64,
    /// The occurrences it is placed through, from the top. Left out, its only placement.
    #[serde(default, deserialize_with = "super::present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Vec<u64>")]
    pub path: Option<Vec<u64>>,
}

/// An annotation the replace keeps, rewritten onto the new part.
#[derive(Debug, Serialize, JsonSchema)]
pub struct KeptResult {
    pub id: String,
    /// dimension, note or balloon.
    pub kind: &'static str,
}

/// An annotation that doesn't come across: it keeps its old record, and the sheet doesn't draw it.
#[derive(Debug, Serialize, JsonSchema)]
pub struct LostResult {
    pub id: String,
    pub kind: &'static str,
    pub why: String,
}

/// A `hide` path under the old part, dropped from a view.
#[derive(Debug, Serialize, JsonSchema)]
pub struct DroppedHideResult {
    pub view: String,
    pub path: Vec<u64>,
}

/// What `drawing.source.replace` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ReplaceResult {
    /// The views now showing the new part: each base view, then the views projected from it.
    pub views: Vec<String>,
    pub kept: Vec<KeptResult>,
    pub lost: Vec<LostResult>,
    pub dropped_hide: Vec<DroppedHideResult>,
    /// The sheets whose documented component now is the new part.
    pub sheets: Vec<String>,
    /// The parts lists now listing the new part.
    pub parts_lists: Vec<String>,
    pub dry_run: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

#[cfg(test)]
#[path = "drawing_source_tests.rs"]
mod tests;
