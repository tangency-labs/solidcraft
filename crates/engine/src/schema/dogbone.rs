//! Contract and result type for `solid.dogbone` (`cmd/dogbone.rs`).
use schemars::JsonSchema;
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use solidcore_record::Vec3;

use super::features::FeatureResult;
use super::types::{AngleExpr, AxisDir, DogboneSideParam, DogboneStyleParam, EdgeRefs, FaceRefs, FeatureCommon, LengthExpr, feature_common};

/// The bit's axis, read as [`AxisDir`] is (X, Y or Z in any case, or a non-zero `[x, y, z]`),
/// with the handler's message, which names `along`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AlongDir(pub Vec3);

/// What `solid.dogbone` says about an `along` it cannot read.
pub(crate) const ALONG_MESSAGE: &str = "`along` must be X, Y, Z or a non-zero [x, y, z]";

impl<'de> Deserialize<'de> for AlongDir {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        crate::cmd::axis_dir_form(&Value::deserialize(d)?).map(AlongDir).map_err(|_| D::Error::custom(super::check::whole(ALONG_MESSAGE)))
    }
}

/// Cut dogbone reliefs into inside corners. Give `edges`, `faces`, or both.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct DogboneParams {
    /// The inside corner edges to relieve: a point on each, or `{body, index}`.
    #[serde(default)]
    pub edges: Option<EdgeRefs>,
    /// Planar faces (a pocket's floor, a plate's face a slot runs through) whose inside corners,
    /// the straight edges along the face's normal at its corners, are relieved.
    #[serde(default)]
    pub faces: Option<FaceRefs>,
    /// The router bit's diameter: each relief is a round hole of this diameter. More than 0 and
    /// at most 100000 mm.
    pub diameter: LengthExpr,
    /// Default: normal.
    #[serde(default)]
    pub style: Option<DogboneStyleParam>,
    /// Minimal style only: how far short of the sharp corner the relief stops, at least 0 and at
    /// most 100000 mm. Default: 5% of the diameter.
    #[serde(default)]
    pub gap: Option<LengthExpr>,
    /// T-bone style only: which wall each relief bites. Default: longer.
    #[serde(default)]
    pub side: Option<DogboneSideParam>,
    /// The largest corner angle relieved, more than 0 deg and at most 180 deg: corners more open
    /// are skipped. Default: 90 deg (square and sharper corners).
    #[serde(default)]
    pub max_angle: Option<AngleExpr>,
    /// The bit's axis: picked edges not along it are skipped. Default: any direction (a picked
    /// face's corners are always along its normal).
    #[serde(default)]
    #[schemars(with = "Option<AxisDir>")]
    pub along: Option<AlongDir>,
    /// The body the edges and faces are on. Default: the one the picks find.
    #[serde(default)]
    pub body: Option<String>,
    #[serde(flatten, deserialize_with = "feature_common")]
    pub common: FeatureCommon,
}

/// An edge or face the dogbone skipped.
#[derive(Debug, Serialize, JsonSchema)]
pub struct DogboneSkip {
    /// A point on it: an edge's middle, a face's corner.
    pub at: [f64; 3],
    /// Why: convex (an outside corner), flat, too_open (more open than `max_angle`),
    /// gap_too_large, overhang (a blind end's face leans over the corner so far that no
    /// relief fits above it), too_long (an open end's face runs so nearly along the edge that the
    /// relief would run on past it by more than 1 km), not_straight, not_between_two_faces,
    /// not_planar, not_along_axis, no_corners (a picked face with none).
    pub reason: String,
}

/// What `solid.dogbone` returns: a feature command's reply, with the corners relieved and the
/// edges skipped.
#[derive(Debug, Serialize, JsonSchema)]
pub struct DogboneResult {
    #[serde(flatten)]
    pub feature: FeatureResult,
    /// How many corners were relieved.
    pub relieved: usize,
    /// The middle of each relieved corner edge.
    pub relieved_edges: Vec<[f64; 3]>,
    /// The edges and faces skipped, and why.
    pub skipped: Vec<DogboneSkip>,
}

#[cfg(test)]
#[path = "dogbone_tests.rs"]
mod tests;
