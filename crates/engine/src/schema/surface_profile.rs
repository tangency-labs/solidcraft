//! Contracts for the commands in `cmd/surface.rs` that make a surface body from a sketch's curves
//! and profiles: Surface Extrude and Surface Revolve. Both reply with the plain feature reply,
//! [`super::features::FeatureResult`].
//!
//! Both take the sketch's curves (open or closed chains) and profiles (their boundaries) as
//! `solid.extrude` takes profiles; with neither, every profile. Whether the sketch, a curve or a
//! profile exists is the handler's to check.

use schemars::JsonSchema;
use serde::Deserialize;

use super::config::NameList;
use super::types::{
    AngleExpr, ConventionParam, DirectionParam, FeatureCommon, IdOrName, LengthExpr, ProfilesParam, RevolveAxis, RevolveDirection, feature_common,
    present,
};

/// Extrude a sketch's curves or profiles into a surface body: walls only, no caps.
///
/// The extent keys `solid.extrude` has beyond these (`through_all`, `extent`, `taper`, `corner`,
/// `to`, `to_offset` and `from`) are left out: a surface extrude refuses the first ones as "not
/// supported yet" and does nothing with the others.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct SurfaceExtrudeParams {
    /// How far: the length one way; with `direction: symmetric`, the total (per side with
    /// `convention: legacy_per_side`); with `distance2`, the first side's length.
    pub distance: LengthExpr,
    /// The sketch, by id or name. Default: the active sketch, else the last one.
    #[serde(default, deserialize_with = "present")]
    #[schemars(with = "IdOrName")]
    pub sketch: Option<IdOrName>,
    /// The curves to extrude, by id: open or closed chains of curves meeting end to end.
    #[serde(default)]
    pub curves: Option<NameList>,
    /// The profiles whose boundaries to extrude, as `solid.extrude` picks them. Default: every
    /// profile when no `curves` are given; none when they are.
    #[serde(default, deserialize_with = "present")]
    #[schemars(with = "ProfilesParam")]
    pub profiles: Option<ProfilesParam>,
    /// Each index or point pick is exactly the region it names. Without it, a pick on a drawn
    /// profile that reference geometry (a face's projected outline) cuts into regions takes the
    /// whole drawn profile.
    #[serde(default)]
    pub exact_regions: Option<bool>,
    /// Which way to extrude. Default: positive.
    #[serde(default)]
    pub direction: Option<DirectionParam>,
    /// Two Sides: a second distance on the side `direction` leaves free. With
    /// `direction: negative` the first side goes against the normal and `distance2` along it.
    #[serde(default)]
    pub distance2: Option<LengthExpr>,
    /// Start this far from the sketch plane, along its normal.
    #[serde(default)]
    pub start_offset: Option<LengthExpr>,
    /// How a symmetric distance is read. Default: total_distance.
    #[serde(default, deserialize_with = "present")]
    #[schemars(with = "ConventionParam")]
    pub convention: Option<ConventionParam>,
    #[serde(flatten, deserialize_with = "feature_common")]
    pub common: FeatureCommon,
}

/// Revolve a sketch's curves or profiles about an axis into a surface body: no caps, and a solid
/// where a full turn closes it.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct SurfaceRevolveParams {
    /// The axis to turn about.
    pub axis: RevolveAxis,
    /// The sketch, by id or name. Default: the active sketch, else the last one. A bare curve id
    /// in `axis` is a line of this sketch.
    #[serde(default, deserialize_with = "present")]
    #[schemars(with = "IdOrName")]
    pub sketch: Option<IdOrName>,
    /// The curves to revolve, by id: open or closed chains of curves meeting end to end.
    #[serde(default)]
    pub curves: Option<NameList>,
    /// The profiles whose boundaries to revolve, as `solid.extrude` picks them. Default: every
    /// profile when no `curves` are given; none when they are.
    #[serde(default, deserialize_with = "present")]
    #[schemars(with = "ProfilesParam")]
    pub profiles: Option<ProfilesParam>,
    /// Each index or point pick is exactly the region it names. Without it, a pick on a drawn
    /// profile that reference geometry (a face's projected outline) cuts into regions takes the
    /// whole drawn profile.
    #[serde(default)]
    pub exact_regions: Option<bool>,
    /// How far to turn. Default: 360 deg.
    #[serde(default)]
    pub angle: Option<AngleExpr>,
    /// Two Sides: how far to turn the other way.
    #[serde(default)]
    pub angle2: Option<AngleExpr>,
    /// Split `angle` evenly either side of the sketch. Default: false.
    #[serde(default)]
    pub symmetric: Option<bool>,
    /// `symmetric` is the same as `symmetric: true`.
    #[serde(default)]
    pub direction: Option<RevolveDirection>,
    #[serde(flatten, deserialize_with = "feature_common")]
    pub common: FeatureCommon,
}

#[cfg(test)]
#[path = "surface_profile_tests.rs"]
mod tests;
