//! Contracts for frame member creation.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::Deserialize;

use super::{AngleExpr, FeatureCommon, LengthExpr, ProfilesParam, SketchRef, feature_common};

#[cfg(test)]
#[path = "frame_tests.rs"]
mod tests;

/// A parametric library profile. `frame.profile.list` describes each kind's dimensions.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ProfileParams {
    pub kind: String,
    #[serde(flatten)]
    pub dimensions: BTreeMap<String, LengthExpr>,
}

/// Structural members on selected sketch lines or circular arcs. Each selected curve makes
/// one member, unless `continuous` requests one open tangent chain. With no selection,
/// only drawn straight lines are used, preserving the existing default. Participants have
/// no effect: this feature creates new bodies.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct MemberParams {
    pub sketch: SketchRef,
    /// Curve ids, each once, at most 1000. For a continuous member, required in path order;
    /// individual curves are reversed as necessary to connect their ends.
    #[serde(default)]
    pub lines: Option<Vec<String>>,
    /// One physical member along the selected curves. Default false. The curves must form
    /// an open connected tangent line/arc chain, including exact spatial circular arcs.
    #[serde(default)]
    pub continuous: Option<bool>,
    /// Library profile, mutually exclusive with `profile_sketch`.
    #[serde(default)]
    pub profile: Option<ProfileParams>,
    /// Section sketch, mutually exclusive with `profile`. Its nested material must be one piece.
    #[serde(default)]
    pub profile_sketch: Option<SketchRef>,
    #[serde(default)]
    pub profiles: Option<ProfilesParam>,
    /// Profile point on the path: origin (default), center, centroid, top_left, top,
    /// top_right, left, right, bottom_left, bottom or bottom_right.
    #[serde(default)]
    pub anchor: Option<String>,
    /// Initial section rotation. Default zero; the section then follows each bend without
    /// resetting its orientation to world up at each segment.
    #[serde(default)]
    pub angle: Option<AngleExpr>,
    #[serde(default)]
    pub mirror: Option<bool>,
    /// Pinned body names by curve id; a continuous member uses the stable key `$path`.
    #[serde(default)]
    pub names: Option<BTreeMap<String, String>>,
    #[serde(flatten, deserialize_with = "feature_common")]
    pub common: FeatureCommon,
}
