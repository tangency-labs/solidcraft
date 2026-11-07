//! Contracts and result types for `drawing.check` (`cmd/drawing_check.rs`, #13054).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Check the open drawing, or one of its sheets. Read-only.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(deny_unknown_fields)]
pub struct CheckParams {
    /// The sheet, by id or name (default: every sheet).
    #[serde(default, deserialize_with = "super::present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub sheet: Option<String>,
}

/// What a finding is. A closed list: a new kind is a change of this schema, so a script can rely
/// on the set.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FindingKind {
    /// A dimension, balloon or note whose reference isn't found.
    Lost,
    /// One whose reference is found more than once.
    Ambiguous,
    /// A dimension whose references are found but can't be measured.
    Unmeasurable,
    /// One whose reference is on a suppressed feature.
    Suppressed,
    /// A dimension or a view that can't be worked out.
    Failed,
    /// A dimension whose text hides its value, or a title block cell whose typed text hides one.
    Overridden,
    /// A view or dimension the sheet shows as it last was.
    OutOfDate,
    /// A view with nothing to measure.
    NothingShown,
    /// A title block field with no value.
    EmptyField,
    /// A parts list row no balloon points at.
    NotBallooned,
    /// A view outside its sheet's drawing space or over its title block, or another object off the
    /// paper.
    OutsideSheet,
}

impl FindingKind {
    /// Every kind, in the order the schema lists them.
    pub const ALL: &[FindingKind] = &[
        FindingKind::Lost,
        FindingKind::Ambiguous,
        FindingKind::Unmeasurable,
        FindingKind::Suppressed,
        FindingKind::Failed,
        FindingKind::Overridden,
        FindingKind::OutOfDate,
        FindingKind::NothingShown,
        FindingKind::EmptyField,
        FindingKind::NotBallooned,
        FindingKind::OutsideSheet,
    ];
}

/// What kind of record a finding names.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CheckedKind {
    View,
    Dimension,
    Balloon,
    Note,
    Table,
    Image,
    /// A sheet's title block: its id is the sheet's.
    TitleBlock,
}

/// The record a finding names.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
pub struct CheckedObject {
    pub kind: CheckedKind,
    pub id: String,
}

/// One thing that makes the drawing wrong or incomplete.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
pub struct Finding {
    pub kind: FindingKind,
    /// The sheet it is on, by id.
    pub sheet: String,
    pub object: CheckedObject,
    /// A title block finding's cell, by name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cell: Option<String>,
    /// An empty field, as `source.key` (`component.part_number`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
    /// A parts list row, by item (`1.2`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub item: Option<String>,
    /// Why, in words.
    pub why: String,
}

/// How many findings there are of one kind.
#[derive(Debug, Serialize, JsonSchema)]
pub struct KindCount {
    pub kind: FindingKind,
    pub count: usize,
}

/// What `drawing.check` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct CheckResult {
    /// In sheet order, then by the record each names.
    pub findings: Vec<Finding>,
    /// Each kind found, with how many, in the schema's order.
    pub counts: Vec<KindCount>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

#[cfg(test)]
#[path = "drawing_check_tests.rs"]
mod tests;
