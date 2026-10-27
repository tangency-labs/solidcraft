//! Contracts for weldment mounting plates.

use schemars::JsonSchema;
use serde::Deserialize;

use super::{AngleExpr, FeatureCommon, LengthExpr, Point3, feature_common, present};

/// Outline of the plate. Dimensions are required for the chosen shape.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PlateOutline {
    #[default]
    Rectangle,
    Round,
}

/// A bolt pattern centred on the plate, turning with it. Holes affect the plate only.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(tag = "pattern", rename_all = "snake_case", deny_unknown_fields)]
pub enum BoltPattern {
    #[default]
    None,
    Rectangle {
        /// Through-hole diameter, a positive length.
        diameter: LengthExpr,
        /// Centre-to-centre spacing along the plate's X axis.
        pitch_x: LengthExpr,
        /// Centre-to-centre spacing along the plate's Y axis.
        pitch_y: LengthExpr,
    },
    Circle {
        /// Through-hole diameter, a positive length.
        diameter: LengthExpr,
        /// Diameter through the hole centres.
        pitch_diameter: LengthExpr,
        /// Number of equally spaced holes, from 2 to 128.
        #[schemars(range(min = 2, max = 128))]
        count: u32,
        /// Pattern rotation about the plate normal. Default: 0 degrees.
        #[serde(default)]
        angle: Option<AngleExpr>,
    },
}

/// Create a separate mounting plate centred on a persistently identified planar face.
/// The member is not modified. `participants` has no effect on the result.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct PlateParams {
    /// A world-space point on the planar support face, in millimetres.
    pub face: Point3,
    /// Exact world body name. Required with `index`; otherwise the face point must be unambiguous.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub body: Option<String>,
    /// Exact support face index at creation. Required with `body`; saved as a persistent identity.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "usize")]
    pub index: Option<usize>,
    /// Occurrence ids of the selected support, at most 64 nonzero ids.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Vec<u64>", length(max = 64))]
    pub path: Option<Vec<u64>>,
    /// Rectangle or round. Default: rectangle.
    #[serde(default)]
    pub shape: PlateOutline,
    /// Rectangle width along face X. Required for rectangle, omitted for round.
    #[serde(default)]
    pub width: Option<LengthExpr>,
    /// Rectangle height along face Y. Required for rectangle, omitted for round.
    #[serde(default)]
    pub height: Option<LengthExpr>,
    /// Round plate diameter. Required for round, omitted for rectangle.
    #[serde(default)]
    pub diameter: Option<LengthExpr>,
    /// Positive thickness extending outwards from the selected face.
    pub thickness: LengthExpr,
    /// Signed normal offset from the support face. Default: 0 mm.
    #[serde(default)]
    pub offset: Option<LengthExpr>,
    /// Offset along the support face X axis, before plate rotation. Default: 0 mm.
    #[serde(default)]
    pub x_offset: Option<LengthExpr>,
    /// Offset along the support face Y axis, before plate rotation. Default: 0 mm.
    #[serde(default)]
    pub y_offset: Option<LengthExpr>,
    /// Plate rotation about the outward face normal. Default: 0 degrees.
    #[serde(default)]
    pub angle: Option<AngleExpr>,
    /// Four-hole rectangular pattern, bolt circle, or none. Default: none.
    #[serde(default)]
    pub holes: BoltPattern,
    #[serde(flatten, deserialize_with = "feature_common")]
    pub common: FeatureCommon,
}

#[cfg(test)]
#[path = "frame_tests.rs"]
mod tests;
