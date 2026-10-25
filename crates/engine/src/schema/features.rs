//! Contracts and result types for the commands in `cmd/features.rs`.
//!
//! Field doc comments are the schemas' descriptions. Keys the handlers read only in some
//! situations (`sketch` without `face`, the thin-wall keys with `thin: true`) are optional
//! fields whose descriptions say so; the contracts don't check the combinations, and the
//! handlers ignore those keys otherwise.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::types::{
    AngleExpr, ConventionParam, DirectionParam, ExtentKeyword, ExtrudeOperation, FacesParam, FromPoint, IdOrName, LengthExpr, OperationParam, Point3,
    PointExpr3, ProfilesParam, StringList, ThinSideParam, ToPoint, TouchingParam, present,
};

/// Add a box primitive.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct BoxParams {
    /// Size along X.
    pub length: LengthExpr,
    /// Size along Y.
    pub width: LengthExpr,
    /// Size along Z.
    pub height: LengthExpr,
    /// The box's minimum corner. Default: the origin. When both `corner` and `center` are given,
    /// `corner` is used.
    #[serde(default, deserialize_with = "present")]
    #[schemars(with = "PointExpr3")]
    pub corner: Option<PointExpr3>,
    /// The box's centre, instead of `corner`.
    #[serde(default, deserialize_with = "present")]
    #[schemars(with = "PointExpr3")]
    pub center: Option<PointExpr3>,
    /// How the box combines with existing bodies. Default: new.
    #[serde(default)]
    pub operation: Option<OperationParam>,
    /// The bodies a join, cut or intersect works on. Default: the bodies the box meets.
    #[serde(default)]
    pub targets: Option<StringList>,
    /// What a join makes of pieces that share no face. Default: separate.
    #[serde(default)]
    pub touching: Option<TouchingParam>,
    /// The bodies taking part (the same as `targets` for a primitive).
    #[serde(default)]
    pub participants: Option<StringList>,
    /// The feature's name in the timeline.
    #[serde(default)]
    pub name: Option<String>,
    /// The new body's name.
    #[serde(default)]
    pub body_name: Option<String>,
    /// Names for the new bodies, in order.
    #[serde(default)]
    pub body_names: Option<StringList>,
}

/// Extrude sketch profiles (or a planar body face) into a solid.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct ExtrudeParams {
    /// How far: the length one way; with `direction: symmetric`, the total thickness (per side
    /// with `convention: legacy_per_side`); with `distance2`, the first side's length. Required
    /// unless `through_all` or `to` is given.
    #[serde(default)]
    pub distance: Option<LengthExpr>,
    /// The sketch to extrude, by id or name. Default: the active sketch, else the last one.
    /// Ignored when `face` is given.
    #[serde(default, deserialize_with = "present")]
    #[schemars(with = "IdOrName")]
    pub sketch: Option<IdOrName>,
    /// Which of the sketch's profiles to extrude. Default: all of them. Ignored when `face` is
    /// given; not allowed with `thin`.
    #[serde(default, deserialize_with = "present")]
    #[schemars(with = "ProfilesParam")]
    pub profiles: Option<ProfilesParam>,
    /// Each index or point pick is exactly the region it names. Without it, a pick on a drawn
    /// profile that reference geometry (a face's projected outline) cuts into regions takes the
    /// whole drawn profile. The dialogs send `true`: they highlight the regions they pick.
    #[serde(default)]
    pub exact_regions: Option<bool>,
    /// A point on a planar body face to extrude instead of a sketch profile.
    #[serde(default)]
    pub face: Option<Point3>,
    /// Several coplanar faces of one body to extrude together, by a point on each. Not with
    /// `face`, `sketch` or `profiles`.
    #[serde(default, deserialize_with = "present")]
    #[schemars(with = "FacesParam")]
    pub faces: Option<FacesParam>,
    /// The body whose face to use, where faces of several bodies meet at `face` (or `faces`).
    #[serde(default)]
    pub face_body: Option<String>,
    /// With `face`, among placed instances: the body whose placement `face` is read in (read by
    /// the engine's frame mapping, before the command runs). Default: `face_body`, else the
    /// body nearest the point.
    #[serde(default)]
    pub body: Option<String>,
    /// Extrude through everything in the way.
    #[serde(default)]
    pub through_all: Option<bool>,
    /// `"through_all"`, the same as `through_all: true`.
    #[serde(default)]
    pub extent: Option<ExtentKeyword>,
    /// Draft angle of the side walls (positive: outward, so the far end is larger along the
    /// extrusion direction; negative: inward, so the far end is smaller).
    #[serde(default)]
    pub taper: Option<AngleExpr>,
    /// Which way to extrude. Default: positive.
    #[serde(default)]
    pub direction: Option<DirectionParam>,
    /// Two Sides: a second distance the other way from the first side, which may be
    /// `through_all` or `to`; with `direction: negative` the first side goes against the normal
    /// and `distance2` along it.
    #[serde(default)]
    pub distance2: Option<LengthExpr>,
    /// Start this far from the sketch plane, along its normal.
    #[serde(default)]
    pub start_offset: Option<LengthExpr>,
    /// How a symmetric distance is read. Default: total_distance.
    #[serde(default, deserialize_with = "present")]
    #[schemars(with = "ConventionParam")]
    pub convention: Option<ConventionParam>,
    /// Extrude up to this face, vertex or point (To Object).
    #[serde(default, deserialize_with = "present")]
    #[schemars(with = "ToPoint")]
    pub to: Option<ToPoint>,
    /// How far past `to` to go.
    #[serde(default)]
    pub to_offset: Option<LengthExpr>,
    /// Start at this face or point (From Object).
    #[serde(default, deserialize_with = "present")]
    #[schemars(with = "FromPoint")]
    pub from: Option<FromPoint>,
    /// How the extrusion combines with existing bodies. Default: new.
    #[serde(default)]
    pub operation: Option<ExtrudeOperation>,
    /// The bodies a join, cut or intersect works on. Default for a join from a body's face:
    /// that body.
    #[serde(default)]
    pub targets: Option<StringList>,
    /// What a join makes of pieces that share no face. Default: separate.
    #[serde(default)]
    pub touching: Option<TouchingParam>,
    /// The bodies taking part; overrides the default scope.
    #[serde(default)]
    pub participants: Option<StringList>,
    /// The feature's name in the timeline.
    #[serde(default)]
    pub name: Option<String>,
    /// The new body's name.
    #[serde(default)]
    pub body_name: Option<String>,
    /// Names for the new bodies, in order.
    #[serde(default)]
    pub body_names: Option<StringList>,
    /// A thin wall along sketch curves instead of solid profiles.
    // `skip_serializing_if` keeps schemars from writing `default: null`, which `bool` refuses.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "bool")]
    pub thin: Option<bool>,
    /// With `thin`: the curves the wall follows. Default: every drawn curve of the sketch.
    #[serde(default)]
    pub curves: Option<StringList>,
    /// With `thin`: the wall's thickness.
    #[serde(default)]
    pub thickness: Option<LengthExpr>,
    /// With `thin`: which side of the curves the wall lies on. Default: center.
    #[serde(default)]
    pub side: Option<ThinSideParam>,
    /// With `thin` and `side: two_sides`: the thickness outside.
    #[serde(default)]
    pub thickness2: Option<LengthExpr>,
}

/// Something a feature's result tells the user without stopping them, with an action to take.
#[derive(Debug, Serialize, JsonSchema)]
pub struct FeatureNotice {
    /// The text to show.
    pub message: String,
    /// How the bodies split off touched the others: `along_edge`, `at_point` or `touching` (along
    /// a line or at a point their boundaries don't hold).
    pub contact: String,
    /// The command that changes the feature as the notice offers.
    pub action: NoticeAction,
}

/// A command a notice offers to run.
#[derive(Debug, Serialize, JsonSchema)]
pub struct NoticeAction {
    /// The action's label.
    pub label: String,
    /// The command's id.
    pub command: String,
    /// Its parameters.
    pub params: Value,
}

/// What a feature command returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct FeatureResult {
    /// The new feature's id.
    pub feature: u64,
    /// The new feature's name.
    pub name: String,
    /// Every body after the feature.
    pub bodies: Vec<String>,
    /// The bodies the feature made.
    pub created_bodies: Vec<String>,
    /// Bodies that were there before and now have other geometry.
    pub changed_bodies: Vec<String>,
    /// Bodies that are gone.
    pub removed_bodies: Vec<String>,
    /// A warning from the evaluation (a Boolean that needed a retry, a reference found again
    /// elsewhere), or `null`.
    pub warning: Option<String>,
    /// A join that split pieces into bodies of their own because they touch the others only along
    /// an edge or at a point (its `touching: separate`): what to tell the user, and the edit that
    /// makes them one body instead. Left out otherwise, also when the join kept pieces that touch
    /// nothing in one body.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notice: Option<FeatureNotice>,
    /// How long the feature took to evaluate, in milliseconds.
    pub ms: Option<f64>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

#[cfg(test)]
#[path = "features_tests.rs"]
mod tests;
