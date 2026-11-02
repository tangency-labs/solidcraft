//! Contracts for view-only surface analyses; the handlers validate domain ranges.
use super::{AxisDir, Point3, present};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct ZebraParams {
    /// Stripe frequency across the view, including fractional values. Default 12; 1…200.
    pub stripes: Option<f64>,
    pub clear: Option<bool>,
}
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct DraftParams {
    /// Nonzero pull direction. Default +Z; normalised by the handler.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "AxisDir")]
    pub pull: Option<AxisDir>,
    /// Threshold in degrees, 0 inclusive to 90 exclusive. Default 1.
    pub angle: Option<f64>,
    pub clear: Option<bool>,
}
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct EnvironmentParams {
    pub clear: Option<bool>,
}
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct AccessibilityParams {
    /// Nonzero direction the tool comes from. Default +Z.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "AxisDir")]
    pub direction: Option<AxisDir>,
    pub clear: Option<bool>,
}
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct CurvatureParams {
    /// Radius in mm shown red, greater than 1e-6 and less than 1e9.
    /// Default a tenth of the largest model diagonal, or 10 mm for an empty model.
    pub radius: Option<f64>,
    pub clear: Option<bool>,
}
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum EdgePick {
    Index { index: usize },
    Point(Point3),
}
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct ContinuityParams {
    /// Body name; otherwise visible bodies. At most 64 bodies in one call.
    pub body: Option<String>,
    /// At most 512 edge indices or points. Empty/absent measures all seams.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Vec<EdgePick>")]
    pub edges: Option<Vec<EdgePick>>,
    /// Integer stations per edge, 3…257. Default 33.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "f64")]
    pub samples: Option<f64>,
    /// Positional tolerance in mm, 0…1e9 exclusive. Default .001.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "f64")]
    pub gap_tolerance_mm: Option<f64>,
    /// Normal angle tolerance in degrees. Default 1.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "f64")]
    pub angle_tolerance_deg: Option<f64>,
    /// Curvature tolerance in inverse mm. Default .01.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "f64")]
    pub curvature_tolerance_per_mm: Option<f64>,
    /// Preserve other measured seams, replacing only matching body/edge records.
    pub add: Option<bool>,
    pub clear: Option<bool>,
}

#[derive(Serialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Analysis {
    Zebra { stripes: f64 },
    Draft { pull: [f64; 3], angle: f64 },
    Curvature { radius: f64 },
    Environment,
    Access { dir: [f64; 3] },
}
#[derive(Serialize, JsonSchema)]
pub struct AnalysisResult {
    pub analysis: Option<Analysis>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}
#[derive(Serialize, JsonSchema)]
pub struct Metric {
    pub value: f64,
    pub at: [f64; 3],
    pub edge_fraction: f64,
}
#[derive(Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    Smooth,
    CurvatureJump,
    Sharp,
    Gap,
}
#[derive(Serialize, JsonSchema)]
pub struct EdgeReport {
    pub edge: usize,
    pub length_mm: f64,
    pub samples: usize,
    pub gap_mm: Metric,
    pub normal_angle_deg: Metric,
    pub curvature_jump_per_mm: Metric,
    pub verdict: Verdict,
}
#[derive(Serialize, JsonSchema)]
pub struct BodyReport {
    pub body: String,
    pub edges: Vec<EdgeReport>,
    pub skipped: usize,
    pub unmeasured: usize,
}
#[derive(Serialize, JsonSchema)]
pub struct Tolerances {
    pub gap_mm: f64,
    pub angle_deg: f64,
    pub curvature_per_mm: f64,
}
#[derive(Serialize, JsonSchema)]
pub struct Summary {
    pub edges: usize,
    pub smooth: usize,
    pub curvature_jump: usize,
    pub sharp: usize,
    pub gap: usize,
    pub skipped: usize,
    pub unmeasured: usize,
    pub worst: Option<Verdict>,
}
#[derive(Serialize, JsonSchema)]
#[serde(untagged)]
pub enum ContinuityResult {
    Cleared {
        analysis: String,
        cleared: bool,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        param_warnings: Vec<String>,
    },
    Measured {
        analysis: String,
        samples: usize,
        tolerances: Tolerances,
        bodies: Vec<BodyReport>,
        summary: Summary,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        param_warnings: Vec<String>,
    },
}
