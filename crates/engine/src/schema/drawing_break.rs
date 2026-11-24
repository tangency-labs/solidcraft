//! Stateless W36 preview contracts. These are not persistent drawing records.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BreakBand {
    pub start: f64,
    pub end: f64,
    pub gap_mm: f64,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PreviewParams {
    /// Angle of all break lines, anticlockwise in radians.
    pub angle_rad: f64,
    /// Alignment/shortening axis, normalized by the map. Defaults to horizontal.
    #[serde(default = "horizontal_axis")]
    pub shortening_axis: [f64; 2],
    /// Paper:model scale, such as 1:2.
    pub scale: String,
    /// At most 64 disjoint bands along the cut normal; gaps are paper mm along the shortening axis.
    #[serde(default)]
    pub breaks: Vec<BreakBand>,
    /// At most 2,000 original straight segments. Curves must not be silently approximated.
    #[serde(default)]
    pub lines: Vec<[[f64; 2]; 2]>,
    /// At most 2,000 displayed points to map back for picking; gaps return null.
    #[serde(default)]
    pub paper_points: Vec<[f64; 2]>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct PreviewResult {
    pub normal: [f64; 2],
    pub shortening_axis: [f64; 2],
    pub shortening_mm: f64,
    pub breaks: Vec<BreakBand>,
    /// Displayed intervals along `normal`, rather than distances along `shortening_axis`.
    pub gaps: Vec<[f64; 2]>,
    pub lines: Vec<[[f64; 2]; 2]>,
    pub model_points: Vec<Option<[f64; 2]>>,
}

fn horizontal_axis() -> [f64; 2] {
    [1.0, 0.0]
}
