//! Contracts and result types for the commands in `cmd/drawing_clipboard.rs` (#13065).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::Point2;
use super::drawing_source::{DroppedHideResult, KeptResult, LostResult, ToParam};
use super::drawing_table::TableIdParam;
use super::drawing_view::ViewIdParam;

/// What `drawing.copy` takes: exactly one of `views`, `sheets` and `tables`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(deny_unknown_fields)]
pub struct CopyParams {
    /// Views of one sheet, each with its dimensions, balloons and notes.
    #[serde(default, deserialize_with = "super::present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Vec<ViewIdParam>")]
    pub views: Option<Vec<ViewIdParam>>,
    /// Sheets, by id or name, each with everything on it.
    #[serde(default, deserialize_with = "super::present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Vec<String>")]
    pub sheets: Option<Vec<String>>,
    /// Parts lists and revision tables.
    #[serde(default, deserialize_with = "super::present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Vec<TableIdParam>")]
    pub tables: Option<Vec<TableIdParam>>,
}

/// What `drawing.copy` copied, by kind.
#[derive(Debug, Serialize, JsonSchema)]
pub struct CopyResult {
    pub sheets: Vec<String>,
    pub views: Vec<String>,
    pub dimensions: Vec<String>,
    pub balloons: Vec<String>,
    pub notes: Vec<String>,
    pub images: Vec<String>,
    pub tables: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What `drawing.paste` takes.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(deny_unknown_fields)]
pub struct PasteParams {
    /// The sheet copied views and tables go on, by id or name (default: the first). Copied sheets
    /// come whole, as new sheets.
    #[serde(default, deserialize_with = "super::present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub sheet: Option<String>,
    /// Where the centre of the first view `drawing.copy` listed goes, in paper millimetres; the
    /// others keep their spacing. Left out, every view keeps its place.
    #[serde(default, deserialize_with = "super::present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Point2")]
    pub position_mm: Option<Point2>,
    /// The part the pasted views show. Left out: the parts they showed, or this design itself for
    /// a copy from another design.
    #[serde(default, deserialize_with = "super::present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "ToParam")]
    pub to: Option<ToParam>,
    /// `true` to paste what another design copied last.
    #[serde(default, deserialize_with = "super::present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "bool")]
    pub from_design: Option<bool>,
}

/// One record the paste made, and the one it is a copy of.
#[derive(Debug, Serialize, JsonSchema)]
pub struct PastedResult {
    pub from: String,
    pub id: String,
    /// sheet, view, dimension, balloon, note, image file, image or table.
    pub kind: &'static str,
}

/// A copied record that isn't pasted, and why.
#[derive(Debug, Serialize, JsonSchema)]
pub struct LeftOutResult {
    pub id: String,
    pub kind: &'static str,
    pub why: &'static str,
}

/// What `drawing.paste` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct PasteResult {
    pub pasted: Vec<PastedResult>,
    /// With `to`, or from another design: the annotations moved onto the part, and those that keep
    /// their old record and aren't drawn.
    pub kept: Vec<KeptResult>,
    pub lost: Vec<LostResult>,
    pub dropped_hide: Vec<DroppedHideResult>,
    pub left_out: Vec<LeftOutResult>,
    pub warnings: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

#[cfg(test)]
#[path = "drawing_clipboard_tests.rs"]
mod tests;
