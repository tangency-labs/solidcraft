//! Contracts for the commands in `cmd/construct.rs`: construction axes, points and planes built
//! from model geometry. Their reply is a [`super::features::FeatureResult`].
//!
//! Every reference is a [`GeoRefParam`]; its description says the forms, and each key's says
//! what the evaluation needs it to be (a plane, a line, a point…). A reference of the wrong kind
//! is refused when the feature is evaluated, which fails the command.

use schemars::JsonSchema;
use serde::Deserialize;

use super::types::{AngleExpr, ConstructionCommon, GeoRefParam, NumberExpr, PlaneCommon, Point3, feature_common, present};

/// Add a construction axis along the axis of a cylinder, cone or torus face, or of a circular
/// edge.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct AxisThroughCylinderParams {
    /// The face or circular edge whose axis it is. Any line works: a straight edge or sketch
    /// line, X, Y, Z or a construction axis gives itself.
    pub face: GeoRefParam,
    #[serde(flatten, deserialize_with = "feature_common")]
    pub common: ConstructionCommon,
}

/// Add a construction axis square to a face, through a point.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct AxisNormalToFaceParams {
    /// The face: a plane, or a planar, cylindrical, conical, spherical or toroidal face.
    pub face: GeoRefParam,
    /// The point the axis goes through, square to the face where the face is nearest it: a
    /// vertex, a construction point, a sketch point, a fixed point, or the centre of a circular
    /// edge, a sketch circle or arc, or a spherical or toroidal face. Not on the axis of a round
    /// face.
    pub point: GeoRefParam,
    #[serde(flatten, deserialize_with = "feature_common")]
    pub common: ConstructionCommon,
}

/// Add a construction axis where two planes meet.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct AxisTwoPlanesParams {
    /// The first plane or planar face.
    pub a: GeoRefParam,
    /// The second plane or planar face, not parallel to the first.
    pub b: GeoRefParam,
    #[serde(flatten, deserialize_with = "feature_common")]
    pub common: ConstructionCommon,
}

/// Add a construction axis through two points, from `a` towards `b`.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct AxisTwoPointsParams {
    /// The first point: a vertex, a construction point, a sketch point, a fixed point, or the
    /// centre of a circular edge, a sketch circle or arc, or a spherical or toroidal face.
    pub a: GeoRefParam,
    /// The second point, somewhere else.
    pub b: GeoRefParam,
    #[serde(flatten, deserialize_with = "feature_common")]
    pub common: ConstructionCommon,
}

/// Add a construction axis along a straight edge or line.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct AxisEdgeParams {
    /// The line: a straight edge, a sketch line, X, Y, Z or a construction axis (a cylinder, cone
    /// or torus face gives its axis).
    pub edge: GeoRefParam,
    #[serde(flatten, deserialize_with = "feature_common")]
    pub common: ConstructionCommon,
}

/// Add a construction point at a vertex or point.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct PointVertexParams {
    /// The point: a vertex, a construction point, a sketch point, a fixed point, or the centre of
    /// a circular edge, a sketch circle or arc, or a spherical or toroidal face.
    pub point: GeoRefParam,
    #[serde(flatten, deserialize_with = "feature_common")]
    pub common: ConstructionCommon,
}

/// Add a construction point where two lines meet, or midway between their closest points.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct PointTwoEdgesParams {
    /// The first line: a straight edge, a sketch line, X, Y, Z or a construction axis (a circular
    /// edge or a cylinder, cone or torus face gives its axis).
    pub a: GeoRefParam,
    /// The second line, not parallel to the first.
    pub b: GeoRefParam,
    #[serde(flatten, deserialize_with = "feature_common")]
    pub common: ConstructionCommon,
}

/// Add a construction point where three planes meet.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct PointThreePlanesParams {
    /// The first plane or planar face.
    pub a: GeoRefParam,
    /// The second plane or planar face, not parallel to the first.
    pub b: GeoRefParam,
    /// The third plane or planar face, not parallel to the line where the first two meet.
    pub c: GeoRefParam,
    #[serde(flatten, deserialize_with = "feature_common")]
    pub common: ConstructionCommon,
}

/// Add a construction point at the centre of a circle, sphere or torus.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct PointCenterParams {
    /// A circular edge, a sketch circle or arc, or a spherical or toroidal face.
    pub of: GeoRefParam,
    #[serde(flatten, deserialize_with = "feature_common")]
    pub common: ConstructionCommon,
}

/// Add a construction point where a line meets a plane.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct PointEdgePlaneParams {
    /// The line: a straight edge, a sketch line, X, Y, Z or a construction axis (a circular edge
    /// or a cylinder, cone or torus face gives its axis), not parallel to the plane.
    pub edge: GeoRefParam,
    /// The plane or planar face.
    pub plane: GeoRefParam,
    #[serde(flatten, deserialize_with = "feature_common")]
    pub common: ConstructionCommon,
}

/// Add a construction point part way along a path.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct PointAlongPathParams {
    /// The path: an edge or a sketch curve, with some length.
    pub path: GeoRefParam,
    /// How far along the path, as a fraction of its length: 0 at its start, 1 at its end (a
    /// value outside is taken as the nearer end). Default: 0.5.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "NumberExpr")]
    pub t: Option<NumberExpr>,
    #[serde(flatten, deserialize_with = "feature_common")]
    pub common: ConstructionCommon,
}

/// Add a construction plane halfway between two planes: midway between parallel ones, else
/// through the line where they meet, halving the angle between them.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct MidplaneParams {
    /// The first plane or planar face; between parallel planes, the new plane faces as this one.
    pub a: GeoRefParam,
    /// The second plane or planar face.
    pub b: GeoRefParam,
    #[serde(flatten, deserialize_with = "feature_common")]
    pub common: PlaneCommon,
}

/// Add a construction plane through two lines in one plane.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct PlaneTwoEdgesParams {
    /// The first line: a straight edge, a sketch line, X, Y, Z or a construction axis (a circular
    /// edge or a cylinder, cone or torus face gives its axis).
    pub a: GeoRefParam,
    /// The second line, in one plane with the first but not on the same line.
    pub b: GeoRefParam,
    #[serde(flatten, deserialize_with = "feature_common")]
    pub common: PlaneCommon,
}

/// Add a construction plane containing an axis and a point off that axis.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct PlaneAxisPointParams {
    /// An origin axis, construction axis, straight edge or sketch line reference.
    pub axis: GeoRefParam,
    /// A vertex, sketch point, construction point or fixed point, not on the axis.
    pub point: GeoRefParam,
    #[serde(flatten, deserialize_with = "feature_common")]
    pub common: PlaneCommon,
}

/// Add a construction plane through three points.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct PlaneThreePointsParams {
    /// The first point (the plane's origin): a vertex, a construction point, a sketch point, a
    /// fixed point, or the centre of a circular edge, a sketch circle or arc, or a spherical or
    /// toroidal face.
    pub a: GeoRefParam,
    /// The second point.
    pub b: GeoRefParam,
    /// The third point, not on the line through the first two.
    pub c: GeoRefParam,
    #[serde(flatten, deserialize_with = "feature_common")]
    pub common: PlaneCommon,
}

/// Add a construction plane that touches a face at a point.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct PlaneTangentParams {
    /// The face: a plane, or a planar, cylindrical, conical, spherical or toroidal face.
    pub face: GeoRefParam,
    /// Where the plane touches the face, in mm: the point of the face nearest it. Not on the
    /// axis of a round face. Default, for a face picked by a point: that point; required
    /// otherwise.
    #[serde(default)]
    pub at: Option<Point3>,
    /// A plane or planar face to line the plane up with: it touches where the face turns
    /// towards the reference, parallel to it, on the side of `at`.
    #[serde(default)]
    pub reference: Option<GeoRefParam>,
    /// With `reference`, how far the plane is turned from it about the face's axis. Not on a
    /// sphere. Default: 0 deg.
    #[serde(default)]
    pub angle: Option<AngleExpr>,
    #[serde(flatten, deserialize_with = "feature_common")]
    pub common: PlaneCommon,
}

/// Add a construction plane square to a path, part way along it.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct PlaneAlongPathParams {
    /// The path: an edge or a sketch curve, with some length.
    pub path: GeoRefParam,
    /// How far along the path, as a fraction of its length: 0 at its start, 1 at its end (a
    /// value outside is taken as the nearer end). Default: 0.5.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "NumberExpr")]
    pub t: Option<NumberExpr>,
    #[serde(flatten, deserialize_with = "feature_common")]
    pub common: PlaneCommon,
}

/// Add a construction plane that holds a line and is square to a plane.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct PlanePerpendicularParams {
    /// The plane or planar face the new plane is square to.
    pub plane: GeoRefParam,
    /// The line it holds: a straight edge, a sketch line, X, Y, Z or a construction axis (a
    /// circular edge or a cylinder, cone or torus face gives its axis), not square to `plane`.
    pub line: GeoRefParam,
    #[serde(flatten, deserialize_with = "feature_common")]
    pub common: PlaneCommon,
}

#[cfg(test)]
#[path = "construct_tests.rs"]
mod tests;
