//! Contracts and result types for `drawing.dimension.from_model` (`cmd/drawing_model_dimensions.rs`,
//! #13054).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::drawing_view::ViewIdParam;

/// Bring the model's own dimensions into a view.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(deny_unknown_fields)]
pub struct FromModelParams {
    pub view: ViewIdParam,
    /// Only this sketch's dimensions, by id or name.
    #[serde(default, deserialize_with = "super::present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub sketch: Option<String>,
    /// Only this feature's own dimensions and its sketch's, by id or name.
    #[serde(default, deserialize_with = "super::present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub feature: Option<String>,
    /// Only dimensions of this body, by name.
    #[serde(default, deserialize_with = "super::present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub body: Option<String>,
}

/// What defines a model dimension. A closed list.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    Sketch,
    Feature,
}

impl SourceKind {
    /// Every kind, in the order the schema lists them.
    pub const ALL: [SourceKind; 2] = [SourceKind::Sketch, SourceKind::Feature];
}

/// A model dimension: a sketch and its constraint's id, or a feature and its parameter's label.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
pub struct ModelSource {
    pub kind: SourceKind,
    /// The sketch's feature id, for a sketch's dimension.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sketch: Option<u64>,
    /// The constraint's id, for a sketch's dimension.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub constraint: Option<String>,
    /// The feature's id, for a feature's parameter.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub feature: Option<u64>,
    /// The parameter's label, for a feature's parameter.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub param: Option<String>,
}

/// Why a model dimension wasn't brought in. A closed list, so a script can act on it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SkipReason {
    /// A dimension of the drawing already shows it.
    AlreadyShown,
    /// A kind this command doesn't bring in yet.
    NotSupported,
    /// A driven (reference) sketch dimension.
    Driven,
    /// The view doesn't look along the sketch's normal.
    NotFaceOn,
    /// What it measures isn't found as one edge, corner or face.
    NotFound,
    /// The view doesn't draw its body.
    NotDrawn,
    /// The view can't measure it so.
    Unmeasurable,
    /// What it found reads another value.
    ValueDiffers,
    /// A sketch in a component placed through an occurrence.
    InComponent,
}

impl SkipReason {
    /// Every reason, in the order the schema lists them.
    pub const ALL: [SkipReason; 9] = [
        SkipReason::AlreadyShown,
        SkipReason::NotSupported,
        SkipReason::Driven,
        SkipReason::NotFaceOn,
        SkipReason::NotFound,
        SkipReason::NotDrawn,
        SkipReason::Unmeasurable,
        SkipReason::ValueDiffers,
        SkipReason::InComponent,
    ];
}

/// A model dimension brought in.
#[derive(Debug, Serialize, JsonSchema)]
pub struct Added {
    /// The new dimension, by id.
    pub dimension: String,
    pub from: ModelSource,
}

/// A model dimension left out, and why.
#[derive(Debug, Serialize, JsonSchema)]
pub struct Skipped {
    pub from: ModelSource,
    pub reason: SkipReason,
    /// Why, in words.
    pub why: String,
    /// The dimension that already shows it, by id.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shown_as: Option<String>,
}

/// What `drawing.dimension.from_model` returns, each list in timeline order, then in the sketch's
/// own order of dimensions.
#[derive(Debug, Serialize, JsonSchema)]
pub struct FromModelResult {
    pub added: Vec<Added>,
    pub skipped: Vec<Skipped>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

#[cfg(test)]
#[path = "drawing_model_dimensions_tests.rs"]
mod tests;
