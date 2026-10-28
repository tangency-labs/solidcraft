//! Contracts and result types for the commands in `cmd/features.rs`.
//!
//! Field doc comments are the schemas' descriptions. Keys the handlers read only in some
//! situations (`sketch` without `face`, the thin-wall keys with `thin: true`) are optional
//! fields whose descriptions say so; the contracts don't check the combinations, and the
//! handlers ignore those keys otherwise.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::types::{
    AngleExpr, AxisDir, ConventionParam, DirectionParam, ExtentKeyword, ExtrudeOperation, FacesParam, FeatureCommon, FromPoint, IdOrName, LengthExpr,
    NumberExpr, OperationParam, Point3, PointExpr3, ProfilesParam, ScaleFactors, SketchRef, StringList, ThinSideParam, ToPoint, feature_common,
    present,
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
    /// The bodies a join, cut or intersect works on. Default: every body in the active
    /// component (an intersect removes those the box doesn't meet). Ignored when
    /// `participants` lists bodies.
    #[serde(default)]
    pub targets: Option<StringList>,
    /// The bodies a join, cut or intersect works on, in place of `targets`.
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

/// Add a cylinder primitive.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct CylinderParams {
    /// The radius. Give it or `diameter`; when both are given, `radius` is used.
    #[serde(default)]
    pub radius: Option<LengthExpr>,
    /// The diameter, instead of `radius`.
    #[serde(default)]
    pub diameter: Option<LengthExpr>,
    /// The height, along `axis`.
    pub height: LengthExpr,
    /// The centre of the bottom face. Default: the origin.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "PointExpr3")]
    pub base: Option<PointExpr3>,
    /// The direction from the bottom to the top. Default: Z.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "AxisDir")]
    pub axis: Option<AxisDir>,
    /// Up to two straight edges of the face it stands on, each `{edge: [x, y, z], distance: expr}`
    /// (as `solid.hole`'s): the base centre goes that far from each edge's line, in the face's
    /// plane, and follows the edges. With a numeric `base` only.
    #[serde(default)]
    #[schemars(with = "Option<Vec<serde_json::Value>>")]
    pub references: Option<serde_json::Value>,
    /// How the cylinder combines with existing bodies. Default: new.
    #[serde(default)]
    pub operation: Option<OperationParam>,
    /// The bodies a join, cut or intersect works on. Default: every body in the active
    /// component (an intersect removes those the cylinder doesn't meet).
    /// Ignored when `participants` lists bodies.
    #[serde(default)]
    pub targets: Option<StringList>,
    #[serde(flatten, deserialize_with = "feature_common")]
    pub common: FeatureCommon,
}

/// Add a sphere primitive.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct SphereParams {
    /// The radius. Give it or `diameter`; when both are given, `radius` is used.
    #[serde(default)]
    pub radius: Option<LengthExpr>,
    /// The diameter, instead of `radius`.
    #[serde(default)]
    pub diameter: Option<LengthExpr>,
    /// The centre. Default: the origin.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "PointExpr3")]
    pub center: Option<PointExpr3>,
    /// How the sphere combines with existing bodies. Default: new.
    #[serde(default)]
    pub operation: Option<OperationParam>,
    /// The bodies a join, cut or intersect works on. Default: every body in the active
    /// component (an intersect removes those the sphere doesn't meet).
    /// Ignored when `participants` lists bodies.
    #[serde(default)]
    pub targets: Option<StringList>,
    #[serde(flatten, deserialize_with = "feature_common")]
    pub common: FeatureCommon,
}

/// Add a torus primitive, around the Z axis.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct TorusParams {
    /// The major radius: from the centre to the middle of the ring.
    pub major: LengthExpr,
    /// The minor radius: of the ring's cross-section, above 0 and less than `major`.
    pub minor: LengthExpr,
    /// The centre. Default: the origin.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "PointExpr3")]
    pub center: Option<PointExpr3>,
    /// How the torus combines with existing bodies. Default: new.
    #[serde(default)]
    pub operation: Option<OperationParam>,
    /// The bodies a join, cut or intersect works on. Default: every body in the active
    /// component (an intersect removes those the torus doesn't meet).
    /// Ignored when `participants` lists bodies.
    #[serde(default)]
    pub targets: Option<StringList>,
    #[serde(flatten, deserialize_with = "feature_common")]
    pub common: FeatureCommon,
}

/// Add a cone or truncated cone: the base circle and, `height` along `axis`, the top circle (a
/// point when its radius is 1e-6 mm or less). Its radii, height and base are within 1e7 mm.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct ConeParams {
    /// The base circle's radius, above 1e-6 mm. Give it or `diameter`; when both are given,
    /// `radius` is used.
    #[serde(default)]
    pub radius: Option<LengthExpr>,
    /// The base circle's diameter, instead of `radius`.
    #[serde(default)]
    pub diameter: Option<LengthExpr>,
    /// The top circle's radius, 0 or more; up to 1e-6 mm makes a full cone ending in a point.
    /// When both are given, `top_radius` is used. Default: 0.
    #[serde(default)]
    pub top_radius: Option<LengthExpr>,
    /// The top circle's diameter, instead of `top_radius`.
    #[serde(default)]
    pub top_diameter: Option<LengthExpr>,
    /// The top's distance from the base along `axis`, more than 1e-6 mm either way; a negative
    /// height goes the other way.
    pub height: LengthExpr,
    /// The centre of the base circle. Default: the origin.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "PointExpr3")]
    pub base: Option<PointExpr3>,
    /// The direction `height` is measured along: from the base towards the top when `height` is
    /// positive. Default: Z.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "AxisDir")]
    pub axis: Option<AxisDir>,
    /// How the cone combines with existing bodies. Default: new.
    #[serde(default)]
    pub operation: Option<OperationParam>,
    /// The bodies a join, cut or intersect works on. Default: every body in the active
    /// component (an intersect removes those the cone doesn't meet).
    /// Ignored when `participants` lists bodies.
    #[serde(default)]
    pub targets: Option<StringList>,
    #[serde(flatten, deserialize_with = "feature_common")]
    pub common: FeatureCommon,
}

/// Add a solid tube along a path of sketch curves.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct PipeParams {
    /// The sketch holding the path, by id or name.
    pub path_sketch: SketchRef,
    /// The path's curve ids, in order along the path.
    pub path: StringList,
    /// The tube's outside diameter.
    pub diameter: LengthExpr,
    /// The wall thickness, for a hollow tube: above 0 and less than half `diameter`. Default:
    /// solid.
    #[serde(default)]
    pub wall: Option<LengthExpr>,
    /// How the pipe combines with existing bodies. Default: new.
    #[serde(default)]
    pub operation: Option<OperationParam>,
    /// The bodies a join, cut or intersect works on; given, they win over `participants`.
    /// Default: `participants`, else every body in the active component (an intersect removes
    /// those the pipe doesn't meet), or for a cut or intersect where it has none, every body.
    #[serde(default)]
    pub targets: Option<StringList>,
    #[serde(flatten, deserialize_with = "feature_common")]
    pub common: FeatureCommon,
}

/// Add a box around bodies, larger by a margin on every side.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct BoundingSolidParams {
    /// The bodies to enclose (`participants` doesn't change which). Default: all of them.
    #[serde(default)]
    pub bodies: Option<StringList>,
    /// How much larger than the bodies on every side. Default: 0.
    #[serde(default)]
    pub margin: Option<LengthExpr>,
    #[serde(flatten, deserialize_with = "feature_common")]
    pub common: FeatureCommon,
}

/// Scale bodies about a point.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct ScaleParams {
    /// The bodies to scale.
    pub bodies: StringList,
    /// The same factor in every direction, above 1e-9 and below 1e6. Give it or `factors`.
    #[serde(default)]
    pub factor: Option<NumberExpr>,
    /// Different factors along X, Y and Z, each above 1e-9 and below 1e6. When given, these set
    /// the scale; a `factor` given too has no effect but must still evaluate.
    #[serde(default)]
    pub factors: Option<ScaleFactors>,
    /// The point that stays where it is, in mm. Default: the origin.
    #[serde(default)]
    pub origin: Option<Point3>,
    // Not `FeatureCommon`: a scale makes no body, so `body_name` and `body_names` would do nothing.
    // `participants` stays because `timeline.redefine` passes a feature's own back.
    /// The feature's name in the timeline.
    #[serde(default)]
    pub name: Option<String>,
    /// The bodies taking part, which every feature records; each an existing body or `*`. A
    /// scale changes only `bodies`, whatever this lists.
    #[serde(default)]
    pub participants: Option<StringList>,
}

/// A rigid matrix, in row-major order, with translation in its last column.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum MoveMatrix {
    Flat3([f64; 12]),
    Flat4([f64; 16]),
    Rows3([[f64; 4]; 3]),
    Rows4([[f64; 4]; 4]),
}

/// Move or copy bodies, rotating about an origin before translating, in world coordinates.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct MoveParams {
    /// Bodies to move or copy.
    pub bodies: StringList,
    /// Translation after rotation; default zero. Coordinates are lengths.
    #[serde(default, deserialize_with = "present")]
    #[schemars(with = "PointExpr3")]
    pub translate: Option<PointExpr3>,
    /// Rotation pivot; default world origin. Coordinates are lengths.
    #[serde(default, deserialize_with = "present")]
    #[schemars(with = "PointExpr3")]
    pub origin: Option<PointExpr3>,
    /// Non-zero rotation direction; default Z.
    #[serde(default, deserialize_with = "present")]
    #[schemars(with = "Point3")]
    pub axis: Option<Point3>,
    /// Angle expression; default zero.
    #[serde(default, deserialize_with = "present")]
    #[schemars(with = "AngleExpr")]
    pub angle: Option<AngleExpr>,
    /// Rigid matrix instead of translate, origin, axis and angle. Scales, shears and mirrors are refused.
    #[serde(default, deserialize_with = "present")]
    #[schemars(with = "MoveMatrix")]
    pub matrix: Option<MoveMatrix>,
    /// With matrix: positive rotation tolerance, at most 1e-3; default 1e-9.
    #[serde(default)]
    pub tolerance: Option<f64>,
    /// Keep the source bodies and transform new copies; default false.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "bool")]
    pub copy: Option<bool>,
    #[serde(flatten, deserialize_with = "super::feature_common")]
    pub common: super::FeatureCommon,
}

/// A body feature, or the repeated placements moved together (and any root bodies moved).
#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum MoveResult {
    Feature(FeatureResult),
    Placements {
        placements: Vec<u64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        bodies: Option<FeatureResult>,
    },
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
    /// How long the feature took to evaluate, in milliseconds.
    pub ms: Option<f64>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

#[cfg(test)]
#[path = "features_tests.rs"]
mod tests;
