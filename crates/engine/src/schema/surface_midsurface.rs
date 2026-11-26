//! Contracts and the analysis reply for Midsurface (`surface.midsurface`,
//! `surface.midsurface_analyze`) in `cmd/surface.rs` (#11439). `surface.midsurface` replies with
//! the plain feature reply, [`super::features::FeatureResult`], whose `warning` says what the
//! sheets leave out; `surface.midsurface_analyze` replies with [`MidsurfaceAnalysis`].
//!
//! Whether the body exists, the points lie on its faces, two faces pair, and the expressions
//! evaluate are the handler's to check.

use schemars::JsonSchema;
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

use super::check::whole;
use super::types::{FeatureCommon, LengthExpr, Point3, feature_common};

/// The handler's message for `body` when it isn't text.
fn body<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    match Value::deserialize(d)? {
        Value::String(s) => Ok(s),
        _ => Err(D::Error::custom(whole("`body` is required"))),
    }
}

/// Make the mid-surfaces of a thin-walled solid: for each pair of faces that face each other
/// across material (flat walls and round walls), the surface halfway between them, joined where
/// walls meet, holes through both faces carried. Without `pairs`, the pairs the analysis is sure
/// of are built (see `surface.midsurface_analyze`); pairs that need a decision are left out unless
/// `include_review_pairs`. The feature stores the pairs chosen, including a chosen round pair that
/// is not built yet (its `warning` names it on every rebuild, and an edit that makes its two faces
/// cover the same part of the wall builds it): a rebuild chooses no other pairs, and checks the
/// stored ones again (a pair that no longer faces across a wall within the thickness limits makes
/// the rebuild fail). The reply's `warning` lists what is not covered. A new body: with `stitch`
/// one, a solid where the sewn sheets close up and otherwise a surface body; without it a surface
/// body per sheet, or per piece of a sheet a joint cut in pieces. One undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct MidsurfaceParams {
    /// The solid, by name.
    #[serde(deserialize_with = "body")]
    pub body: String,
    /// The thickest wall to pair across. Default: the analysis's suggestion (the thickest
    /// confident wall thickness found, half as much again, below the next one).
    #[serde(default)]
    pub max_thickness: Option<LengthExpr>,
    /// The thinnest wall to pair across. Default: 0.
    #[serde(default)]
    pub min_thickness: Option<LengthExpr>,
    /// Walls whose span is less than this many times their thickness need a decision (chunky,
    /// not shell-like). Default: 5. With `pairs`, it only decides which of the pairs not chosen
    /// the `warning` says need a decision.
    #[serde(default)]
    pub min_aspect: Option<f64>,
    /// The pairs to build, each two points `[[x,y,z], [x,y,z]]`, one on each face (as
    /// `surface.midsurface_analyze` lists them), 1 to 10,000 of them. Default: the confident
    /// pairs.
    #[serde(default)]
    #[schemars(length(min = 1, max = 10000))]
    pub pairs: Option<Vec<[Point3; 2]>>,
    /// Points on faces whose pairs are not built, 10,000 at most. Default: none.
    #[serde(default)]
    #[schemars(length(max = 10000))]
    pub exclude: Option<Vec<Point3>>,
    /// Also build the pairs that need a decision (chunky, partial, competing, blocked). A round
    /// wall whose two faces do not cover the same part of the wall (partial, or an opening in one
    /// face only) is not built even then: the reply's `warning` names its faces. Default: false.
    /// Ignored with `pairs`.
    #[serde(default)]
    pub include_review_pairs: Option<bool>,
    /// Carry holes through both faces into the sheets. Default: true. A round wall's holes cannot
    /// be filled yet: false with a built round pair that has holes refuses the build, whatever
    /// their size; keep the holes, or leave that pair out (`exclude`).
    #[serde(default)]
    pub carry_holes: Option<bool>,
    /// Fill holes no wider than this. Default: 0 (keep all). Any size above 0 with a built round
    /// pair that has holes refuses the build, even when every hole is wider: keep all holes, or
    /// leave that pair out (`exclude`).
    #[serde(default)]
    pub suppress_holes_below: Option<LengthExpr>,
    /// Keep the solid. Default: true.
    #[serde(default)]
    pub keep_original: Option<bool>,
    /// Sew the sheets where they meet into one body (edges where three sheets meet stay open): a
    /// solid where the sewn sheets close up, as a sealed hollow box's six walls do, otherwise a
    /// surface body. False: a surface body per sheet, and one per piece of a sheet that a joint
    /// cut in pieces across a carried hole. Default: true.
    #[serde(default)]
    pub stitch: Option<bool>,
    #[serde(flatten, deserialize_with = "feature_common")]
    pub common: FeatureCommon,
}

/// Propose the mid-surface pairs of a thin-walled solid, without changing the design: each pair
/// of faces that face each other across material, with its thickness, area, aspect, confidence
/// and notes; every other face with the reason it is in no pair (a rim of a wall is accounted
/// for); the wall thicknesses found, with a limit suggested for each; and what building the
/// confident pairs would leave out.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct MidsurfaceAnalyzeParams {
    /// The solid, by name.
    #[serde(deserialize_with = "body")]
    pub body: String,
    /// The thickest wall to pair across. Default: the suggestion from a first look at the body.
    #[serde(default)]
    pub max_thickness: Option<LengthExpr>,
    /// The thinnest wall to pair across. Default: 0.
    #[serde(default)]
    pub min_thickness: Option<LengthExpr>,
    /// Walls whose span is less than this many times their thickness need a decision. Default: 5.
    #[serde(default)]
    pub min_aspect: Option<f64>,
}

/// One proposed pair of faces.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MidsurfacePair {
    /// The two faces' indices in the body.
    pub faces: [usize; 2],
    /// A point inside each face: what `pairs` takes to build this pair.
    pub points: [[f64; 3]; 2],
    /// The wall's thickness, mm.
    pub thickness: f64,
    /// The sheet's area before its joints are made, mm².
    pub area: f64,
    /// The share of the smaller face the other covers, 0 to 1.
    pub coverage: f64,
    /// The square root of the area over the thickness: below `min_aspect`, not shell-like.
    pub aspect: f64,
    /// `high` (built by default) or `review` (needs a decision: see `notes`).
    pub confidence: String,
    /// Whether the wall is round (two coaxial cylinders).
    pub round: bool,
    /// Openings that do not go straight through (in one face only, a boss's foot or a pocket; or
    /// closed or narrowing between the faces, a counterbored bore) the sheet is filled over, mm².
    pub filled_over: f64,
    /// Why the pair needs a decision, in words.
    pub notes: Vec<String>,
}

/// A face in no pair.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MidsurfaceLeftover {
    /// The face's index in the body.
    pub face: usize,
    /// `rim` (the thin end of a paired wall: accounted for), `not_plane_or_cylinder`,
    /// `out_of_range` (a face looks back, `gap` mm away, past `max_thickness`), `too_thin` (a face
    /// looks back nearer than `min_thickness`), `no_overlap` (a face looks back within the limits
    /// but the two do not overlap), or `no_facing_face`.
    pub reason: String,
    /// For `out_of_range`, `too_thin` and `no_overlap`: how far away the facing face is, mm.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gap: Option<f64>,
    /// For `rim`: the pairs (indices into `pairs`) whose walls hold it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pairs: Vec<usize>,
}

/// A wall thickness found among the confident pairs.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MidsurfaceCluster {
    /// The thickness, mm (the thinnest in the cluster).
    pub thickness: f64,
    /// The thickest in the cluster, mm.
    pub up_to: f64,
    /// The pairs' total area, mm².
    pub area: f64,
    /// How many pairs.
    pub pairs: usize,
    /// A `max_thickness` that takes this wall thickness and thinner ones, and no thicker one.
    pub suggested_limit: f64,
}

/// A face that building the confident pairs leaves uncovered.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MidsurfaceUncovered {
    /// The face's index in the body.
    pub face: usize,
    /// Why, in words.
    pub why: String,
}

/// Where each face goes when the confident pairs are built.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MidsurfaceCompleteness {
    /// How many faces the body has.
    pub faces: usize,
    /// Faces in the confident pairs.
    pub paired: usize,
    /// Faces that are the thin ends of those walls.
    pub rims: usize,
    /// Every other face, with its reason: nothing is left out silently.
    pub uncovered: Vec<MidsurfaceUncovered>,
}

/// What `surface.midsurface_analyze` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MidsurfaceAnalysis {
    /// The body analysed.
    pub body: String,
    /// The thickness limit the analysis used: the thickest wall it pairs across, mm (the given
    /// `max_thickness` or the suggestion), not a measured thickness (see `pairs`).
    pub max_thickness: f64,
    /// The thickness limit the analysis used: the thinnest wall it pairs across, mm.
    pub min_thickness: f64,
    /// The pairs, in the order `rim` leftovers refer to.
    pub pairs: Vec<MidsurfacePair>,
    /// The faces in no pair, each with its reason; rims (the thin ends of paired walls) among
    /// them are accounted for by their pairs.
    pub leftovers: Vec<MidsurfaceLeftover>,
    /// The wall thicknesses found, thinnest first.
    pub clusters: Vec<MidsurfaceCluster>,
    /// What building the confident pairs would cover.
    pub completeness: MidsurfaceCompleteness,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

#[cfg(test)]
#[path = "surface_midsurface_tests.rs"]
mod tests;
