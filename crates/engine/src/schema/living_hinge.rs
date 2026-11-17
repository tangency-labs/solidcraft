//! Contract of `solid.living_hinge` (`cmd/living_hinge.rs`).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::features::FeatureResult;
use super::types::{AngleExpr, FeatureCommon, IdOrName, LengthExpr, PointExpr3, ProfilesParam, StringList, feature_common, present};

/// What happens to slots the edges cross.
#[derive(Clone, Copy, Debug, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HingeClipParam {
    /// Of two neighbouring rows, the one whose slot centre is nearer the plate's edge runs out
    /// through it; the rest are shortened.
    RunOut,
    /// No slot runs out: a solid border along the plate's edge.
    Shorten,
    /// Only whole slots are cut.
    Whole,
}

/// The pattern's keys, shared by every form of the command.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct HingePattern {
    /// A slot's length tip to tip, as cut: more than `slot_width` by at least 0.001 mm.
    pub slot_length: LengthExpr,
    /// The material between two slots in one row, at least 0.001 mm.
    pub bridge: LengthExpr,
    /// Centre line to centre line between rows: more than `slot_width` by at least 0.001 mm.
    pub row_spacing: LengthExpr,
    /// The cut's width, about a laser's kerf, at least 0.001 mm. Default: 0.2 mm.
    #[serde(default)]
    pub slot_width: Option<LengthExpr>,
    /// The slots' direction, which is the bend axis, from the sketch's X axis. Default: 0.
    #[serde(default)]
    pub angle: Option<AngleExpr>,
    /// A slot of row 0 is centred on this point, projected onto the sketch plane. Default: the
    /// pattern centred on the area.
    #[serde(default, deserialize_with = "present")]
    #[schemars(with = "PointExpr3")]
    pub anchor: Option<PointExpr3>,
    /// Slots the edges cross. Default: run_out.
    #[serde(default)]
    pub clip: Option<HingeClipParam>,
    /// The least material between a slot's end and every edge of the area and its keep-outs,
    /// 0 or more. Default: 0.
    #[serde(default)]
    pub margin: Option<LengthExpr>,
}

/// Cut rows of staggered slots through a plate inside a sketch's closed profiles, so a laser-cut
/// plywood, MDF or acrylic plate can bend there. A profile's outer loop is the area; a loop inside
/// it is a keep-out no slot crosses. The slots are cut in one operation, a few hundred at most
/// (the command help names the limit); a sheet metal body is refused. The faces cut have no
/// stable names yet.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct LivingHingeParams {
    /// The sketch holding the area, by id or name. Default: the active sketch, else the last one.
    #[serde(default, deserialize_with = "present")]
    #[schemars(with = "IdOrName")]
    pub sketch: Option<IdOrName>,
    /// Which of the sketch's profiles hold the area and its keep-outs. Default: every drawn
    /// profile, a loop inside another being its keep-out.
    #[serde(default, deserialize_with = "present")]
    #[schemars(with = "ProfilesParam")]
    pub profiles: Option<ProfilesParam>,
    #[serde(flatten)]
    pub pattern: HingePattern,
    /// The plate to cut. Default: the bodies of the active component (or `participants`).
    #[serde(default)]
    pub targets: Option<StringList>,
    #[serde(flatten, deserialize_with = "feature_common")]
    pub common: FeatureCommon,
}

/// What `solid.living_hinge` returns: the feature, and what its evaluation cut.
#[derive(Debug, Serialize, JsonSchema)]
pub struct LivingHingeResult {
    #[serde(flatten)]
    pub feature: FeatureResult,
    /// Slots cut.
    pub slots: usize,
    /// Rows holding at least one slot.
    pub rows: usize,
    /// Slots cut as the lattice has them.
    pub whole: usize,
    /// Slots shortened by an edge.
    pub shortened: usize,
    /// Slots that run out through the plate's edge.
    pub run_out: usize,
    /// Lattice slots that met the material and were left out.
    pub left_out: usize,
    /// The slots' centre lines, summed (mm).
    pub cut_length_mm: f64,
    /// The slots' outlines, summed (mm).
    pub outline_length_mm: f64,
    /// The area the slots open (mm²).
    pub open_area_mm2: f64,
    /// The band's width across the rows (mm).
    pub band_width_mm: f64,
}

#[cfg(test)]
#[path = "living_hinge_tests.rs"]
mod tests;
