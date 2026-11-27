//! Contracts for the commands in `cmd/stretch.rs`. Stretch replies with the plain feature reply,
//! [`super::features::FeatureResult`].

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::types::{LengthExpr, Point3, StringList};

/// Make a solid longer or shorter along one of its component's axes, keeping its details their
/// size (#20512). The body is cut across at a plane square to the direction, `at` in from the side
/// that stays put; the part beyond moves along the direction by `distance`, and the gap fills
/// with the body's own section there. A negative distance shrinks it by taking a slice out.
///
/// Whether the body exists and is a solid stay the handler's to check; whether the body is the
/// same shape all along the direction at the cut is the feature's, when it evaluates.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct StretchParams {
    /// The solid, by name. Default: the one selected body.
    #[serde(default)]
    pub body: Option<String>,
    /// Up to three stretches, each along a different axis, applied in order, instead of the one
    /// the fields below give.
    #[serde(default)]
    pub stretches: Option<Vec<AxisStretchParams>>,
    /// The way the moving side goes: `+x`, `-x`, `+y`, `-y`, `+z` or `-z`, in the body's
    /// component. The opposite side stays where it is. Needed without `stretches`.
    #[serde(default)]
    pub direction: Option<String>,
    /// A point on a face to measure the cut from, usually a flat face square to the direction:
    /// the cut is `at` past that face's level. Default: the side that stays put.
    #[serde(default)]
    pub from: Option<Point3>,
    /// How far the cut plane is along the direction from `from`, or from the side that stays put.
    /// Default: half the body's size along the direction (only without `from`; with it, 0).
    #[serde(default)]
    pub at: Option<LengthExpr>,
    /// How far the moving side goes: a stretch when positive, a shrink when negative. Needed
    /// without `stretches`.
    #[serde(default)]
    pub distance: Option<LengthExpr>,
    /// Change the sketch and Extrude that made the body instead of adding a Stretch step, when
    /// they give the same part. Default: false.
    #[serde(default)]
    pub edit_sketch: Option<bool>,
    /// The feature's name in the timeline.
    #[serde(default)]
    pub name: Option<String>,
    /// The bodies taking part, which every feature records; each an existing body or `*`. A
    /// stretch changes only `body`, whatever this lists.
    #[serde(default)]
    pub participants: Option<StringList>,
}

/// One stretch of [`StretchParams::stretches`]: the fields of a single stretch.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct AxisStretchParams {
    /// The way the moving side goes: `+x`, `-x`, `+y`, `-y`, `+z` or `-z`.
    pub direction: String,
    /// A point on a face to measure the cut from. Default: the side that stays put.
    #[serde(default)]
    pub from: Option<Point3>,
    /// How far the cut plane is along the direction from `from`, or from the side that stays put.
    /// Default: half the body's size along the direction (only without `from`; with it, 0).
    #[serde(default)]
    pub at: Option<LengthExpr>,
    /// How far the moving side goes: a stretch when positive, a shrink when negative.
    pub distance: LengthExpr,
}

/// What `solid.stretch` returns: the feature reply, how the stretch was made and, when the sketch
/// was asked for and not changed, why.
#[derive(Debug, Serialize, JsonSchema)]
pub struct StretchResult {
    #[serde(flatten)]
    pub feature: super::features::FeatureResult,
    /// `step`: a Stretch step was added; `sketch`: the sketch and Extrude that made the body
    /// were changed (then `feature` is that Extrude).
    pub made_by: String,
    /// The parameters changed, when the sketch was: each `{parameter, expression}`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub changed: Vec<ChangedParameter>,
    /// Why the sketch was not changed, when `edit_sketch` asked for it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sketch_not_edited: Option<String>,
}

/// A parameter Stretch changed, with its new expression.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ChangedParameter {
    pub parameter: String,
    pub expression: String,
}
