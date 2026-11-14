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
    /// A dimension, centre annotation or view that can't be worked out.
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
    CentreLine,
    CentreMark,
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

/// Whether the drawing dimensions or notes a feature. A closed list, as [`FindingKind`] is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CoverageState {
    /// A dimension or note points at a face it made.
    Covered,
    /// It made a face a view draws, named from exact history, and nothing points at one.
    NotCovered,
    /// The faces it made that a view draws aren't named from exact history.
    Unknown,
    /// It makes no face a view draws, or it is suppressed or rolled back.
    NotApplicable,
}

impl CoverageState {
    /// Every state, in the order the schema lists them.
    pub const ALL: [CoverageState; 4] = [CoverageState::Covered, CoverageState::NotCovered, CoverageState::Unknown, CoverageState::NotApplicable];
}

/// One feature of the design, and whether the drawing dimensions or notes it. This is coverage:
/// a dimension or a note points at it, not that it is fully dimensioned.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
pub struct Coverage {
    pub feature: u64,
    /// Its name in the timeline.
    pub name: String,
    pub state: CoverageState,
    /// The dimensions and notes that point at a face it made.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub by: Vec<CheckedObject>,
    /// Why it is unknown or not applicable, in words.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
}

/// How many features are in one state.
#[derive(Debug, Serialize, JsonSchema)]
pub struct StateCount {
    pub state: CoverageState,
    pub count: usize,
}

/// What `drawing.check` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct CheckResult {
    /// The sheet checked, by id, when the request named one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sheet: Option<String>,
    /// In sheet order, then by the record each names.
    pub findings: Vec<Finding>,
    /// Each kind found, with how many, in the schema's order.
    pub counts: Vec<KindCount>,
    /// Each feature of the design, in timeline order. Not findings: a script reads the two apart.
    pub coverage: Vec<Coverage>,
    /// Each state found, with how many, in the schema's order.
    pub coverage_counts: Vec<StateCount>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

#[cfg(test)]
#[path = "drawing_check_tests.rs"]
mod tests;
