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
    AngleExpr, ConventionParam, DirectionParam, DraftCornerParam, FeatureCommon, IdOrName, LengthExpr, ProfilesParam, RevolveAxis, RevolveDirection,
    ToPoint, feature_common, present,
};

/// Extrude a sketch's curves or profiles into a surface body: walls only, no caps.
///
/// The extent keys `solid.extrude` has beyond these (`through_all`, `extent` and `from`) are left
/// out: a surface extrude refuses them as "not supported yet".
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct SurfaceExtrudeParams {
    /// How far: the length one way; with `direction: symmetric`, the total (per side with
    /// `convention: legacy_per_side`); with `distance2`, the first side's length. Required
    /// unless `to` is given.
    #[serde(default)]
    pub distance: Option<LengthExpr>,
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
    /// Draft angle of the walls, the profile being the section at the start (positive: outward,
    /// so the far end is larger; negative: inward). Closed profiles and closed loops of curves
    /// only; a profile's holes lean the other way, as on `solid.extrude`'s solid, whose side walls
    /// these are. One side only, and not with `to`.
    #[serde(default)]
    pub taper: Option<AngleExpr>,
    /// How the taper closes a sharp corner of the profile.
    #[serde(default, deserialize_with = "present")]
    #[schemars(with = "DraftCornerParam")]
    pub corner: Option<DraftCornerParam>,
    /// How a symmetric distance is read. Default: total_distance.
    #[serde(default, deserialize_with = "present")]
    #[schemars(with = "ConventionParam")]
    pub convention: Option<ConventionParam>,
    /// End at this face, vertex or point (To Object). A curved or slanted face trims the walls
    /// where they meet its surface.
    #[serde(default, deserialize_with = "present")]
    #[schemars(with = "ToPoint")]
    pub to: Option<ToPoint>,
    /// How far past `to` to go: to a point or a flat face square to the extrusion only.
    #[serde(default)]
    pub to_offset: Option<LengthExpr>,
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
    /// Each face faces the side Surface Extrude's would: the curve's direction crossed with the
    /// sketch normal, so out of a profile. Default: true. False builds each face along its
    /// surface's own normal, as surface revolves saved before it keep.
    #[serde(default)]
    pub faced: Option<bool>,
    #[serde(flatten, deserialize_with = "feature_common")]
    pub common: FeatureCommon,
}

#[cfg(test)]
#[path = "surface_profile_tests.rs"]
mod tests;
