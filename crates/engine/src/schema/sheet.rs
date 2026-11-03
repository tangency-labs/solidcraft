//! Contracts for the sheet metal feature commands in `cmd/sheet.rs` that have one. They return
//! [`FeatureResult`](super::features::FeatureResult).
//!
//! A sheet metal rule sets the sheet's thickness, K-factor and bend radius: the commands take
//! the rule's values, so those are no keys of these contracts.

use schemars::JsonSchema;
use serde::Deserialize;

use super::types::{AngleExpr, FeatureCommon, IdOrName, LengthExpr, StringList, feature_common, present};

/// Press formed beads (stiffening ribs) into a sheet made from surfaces (Convert or Thicken)
/// along straight sketch lines on a face of one of its planar panels: each keeps the sheet's
/// thickness, is in the folded solid, and is listed in the flat pattern and drawn in its DXF
/// (the flat blank stays flat).
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct BeadParams {
    /// The sketch holding the centrelines, by id or name: on a face of a planar panel of the
    /// sheet. Default: the active sketch, else the last one.
    #[serde(default, deserialize_with = "present")]
    #[schemars(with = "IdOrName")]
    pub sketch: Option<IdOrName>,
    /// The centrelines: straight sketch lines (curve ids), 1 to 200; each makes one bead from
    /// end to end (its ends are where the end ramps meet the face).
    pub curves: StringList,
    /// The cross-section: trapezoid (the only one so far). Default: trapezoid.
    #[serde(default)]
    pub shape: Option<String>,
    /// From the face the bead stands out of to its crown on that side, more than 0.
    pub depth: LengthExpr,
    /// Across, between the flanks' virtual sharps on the face it stands out of, more than 0.
    pub width: LengthExpr,
    /// The flanks' angle to the face, between 0 and 90 deg. Give it or `crown_width`, not both.
    /// Default: 45 deg.
    #[serde(default)]
    pub angle: Option<AngleExpr>,
    /// Across the crown, between the flanks' virtual sharps there: sets the angle instead.
    #[serde(default)]
    pub crown_width: Option<LengthExpr>,
    /// The inner radius of the bends at the base. Default: the rule's bend radius.
    #[serde(default)]
    pub base_radius: Option<LengthExpr>,
    /// The inner radius of the bends at the crown. Default: the rule's bend radius.
    #[serde(default)]
    pub crown_radius: Option<LengthExpr>,
    /// Along the centreline, how long each end's ramp is between its virtual sharps. Default: 3 ×
    /// the depth.
    #[serde(default)]
    pub end_length: Option<LengthExpr>,
    /// How near the bead may come to edges, cut-outs, bends and other beads before the feature
    /// warns. Default: 3 × the thickness.
    #[serde(default)]
    pub margin: Option<LengthExpr>,
    /// The bead stands out of the other face of the sheet. Default: false (out of the face the
    /// sketch is on).
    #[serde(default)]
    pub flip: Option<bool>,
    /// The sheet metal body. Default: the newest sheet made from surfaces the first centreline
    /// lies on.
    #[serde(default)]
    pub body: Option<String>,
    #[serde(flatten, deserialize_with = "feature_common")]
    pub common: FeatureCommon,
}

#[cfg(test)]
#[path = "sheet_tests.rs"]
mod tests;
