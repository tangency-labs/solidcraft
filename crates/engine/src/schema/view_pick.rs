//! `view.pick` without the app: world rays against the design's bodies.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// One world-space ray.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PickRay {
    /// Where the ray starts, in world millimetres.
    pub origin: [f64; 3],
    /// Its direction (any length but zero).
    pub dir: [f64; 3],
}

/// The rays to answer, and the app's pixel parameters, which only the running app can answer.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ViewPickParams {
    /// World rays, answered in order.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rays: Option<Vec<PickRay>>,
    /// Every body each ray meets, nearest first, not only the nearest.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub all: Option<bool>,
    /// Screenshot pixels: answered only by the running app.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pixels: Option<Vec<[f64; 2]>>,
    /// The pixels' frame (`window` or `model`): the running app's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frame: Option<String>,
    /// A model render's width for its pixels: the running app's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<f64>,
    /// A model render's height for its pixels: the running app's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<f64>,
    /// Edges and vertices near a pixel: the running app's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edges: Option<bool>,
    /// How near a pixel an edge counts: the running app's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edge_px: Option<f64>,
    /// Ghosted bodies: the running app's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub include_ghosted: Option<bool>,
}

/// One entry per ray, in order: `null` where the ray meets nothing, a hit
/// (`body`, `face`, `point`, `normal`, `depth`, with `edge` or `vertex` where the ray passes
/// through one, and `occurrence`, `occurrence_name`, `component` for a placed part's body), or
/// with `all` a list of hits nearest first.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct ViewPickResult {
    /// `rays`: the answers are of world rays, not of a view.
    pub frame: String,
    /// One entry per ray.
    pub hits: Vec<Value>,
}
