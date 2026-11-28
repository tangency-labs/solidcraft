//! Contract for `surface.morph` in `cmd/surface_morph.rs`. It returns `add_feature`'s reply, a
//! [`super::features::FeatureResult`].

use schemars::JsonSchema;
use serde::de::Error as _;
use serde::{Deserialize, Deserializer};
use serde_json::Value;

use super::check::whole;
use super::types::{LengthExpr, NumberExpr, Point3, StringList};

/// Morph a solid or sheet body (or selected faces of it) by mapping a base reference surface onto
/// a target reference surface.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct MorphSurfaceParams {
    /// The solid or sheet body to morph, by name.
    #[serde(deserialize_with = "req_body")]
    pub body: String,
    /// Optional points on specific faces of `body` to morph. Omit or leave empty to morph the
    /// whole body.
    #[serde(default)]
    pub faces: Option<Vec<Point3>>,
    /// The body carrying the base reference surface, by name.
    #[serde(deserialize_with = "req_base")]
    pub base: String,
    /// Optional point picking a specific face of `base` (default: its first face).
    #[serde(default)]
    pub base_face: Option<Point3>,
    /// The body carrying the target (control) reference surface, by name.
    #[serde(deserialize_with = "req_control")]
    pub control: String,
    /// Optional point picking a specific face of `control` (default: its first face).
    #[serde(default)]
    pub control_face: Option<Point3>,
    /// Projection onto the base surface: `"normal"` (default, closest point) or `"direction"`.
    #[serde(default)]
    pub projection: Option<String>,
    /// Direction vector `[dx, dy, dz]` when `projection` is `"direction"`.
    #[serde(default)]
    pub direction: Option<Point3>,
    /// Offset mapping mode: `"normal_to_normal"` (default) or `"translate"`.
    #[serde(default)]
    pub map_mode: Option<String>,
    /// Deformation scale factor (default `1`; `0` is identity, `> 1` over-crowns).
    #[serde(default)]
    pub scale: Option<NumberExpr>,
    /// Surface/curve refitting tolerance (length, default `0.01 mm`).
    #[serde(default)]
    pub tolerance: Option<LengthExpr>,
    /// Reverse the normalized U parameter between base and target surfaces.
    #[serde(default)]
    pub flip_u: Option<bool>,
    /// Reverse the normalized V parameter between base and target surfaces.
    #[serde(default)]
    pub flip_v: Option<bool>,
    /// Swap normalized (U, V) between base and target surfaces.
    #[serde(default)]
    pub swap_uv: Option<bool>,
    /// Keep the original body and create a new morphed body instead of replacing in place.
    #[serde(default)]
    pub keep_original: Option<bool>,
    /// The feature's name in the timeline.
    #[serde(default)]
    pub name: Option<String>,
    /// Output body name when `keep_original` is true.
    #[serde(default)]
    pub body_name: Option<String>,
    /// Bodies taking part, recorded on the feature.
    #[serde(default)]
    pub participants: Option<StringList>,
}

fn req_body<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    match Value::deserialize(d)? {
        Value::String(s) => Ok(s),
        _ => Err(D::Error::custom(whole("`body` is required"))),
    }
}

fn req_base<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    match Value::deserialize(d)? {
        Value::String(s) => Ok(s),
        _ => Err(D::Error::custom(whole("`base` is required"))),
    }
}

fn req_control<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    match Value::deserialize(d)? {
        Value::String(s) => Ok(s),
        _ => Err(D::Error::custom(whole("`control` is required"))),
    }
}
