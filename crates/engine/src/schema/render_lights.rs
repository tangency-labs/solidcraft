//! Portable read model for lights in a saved setup or an unsaved Render draft.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListParams {
    /// Saved setup name. Mutually exclusive with settings. Neither selects the default setup.
    pub setup: Option<String>,
    /// Complete RenderSetup snapshot, including an optional typed job. Never saved or applied.
    #[schemars(with = "Option<std::collections::BTreeMap<String, Value>>")]
    pub settings: Option<Value>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ListResult {
    pub lights: Vec<Light>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct Light {
    /// Family and zero-based source slot: directional:N, point:N, spot:N or area:N.
    /// Point and spot share their punctual-list index. Stable across numerical edits,
    /// not across family changes, list reordering, removal or rig replacement.
    pub id: String,
    /// English fallback for an unnamed light; clients translate name_key with number.
    pub name: String,
    pub name_key: String,
    /// One-based position in this list, for the Light {number} fallback.
    pub number: usize,
    /// Renderer-provided default/preset light, rather than an explicit saved source.
    pub implicit: bool,
    /// Relative linear intensity. For GPU radiance this is its largest RGB channel;
    /// colour * intensity reconstructs radiance. This is not a watts or Kelvin value.
    pub intensity: f64,
    /// Linear RGB, independent of display exposure and tone mapping.
    pub colour: [f64; 3],
    #[serde(flatten)]
    pub geometry: Geometry,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Geometry {
    Directional {
        /// World direction towards the source, as stored; need not be unit length.
        direction: [f64; 3],
        /// Present for the GPU finite-disk sun only, in radians.
        #[serde(skip_serializing_if = "Option::is_none")]
        angular_radius_rad: Option<f64>,
    },
    Point {
        position_mm: [f64; 3],
        reference_distance_mm: f64,
        minimum_distance_mm: f64,
    },
    Spot {
        position_mm: [f64; 3],
        reference_distance_mm: f64,
        minimum_distance_mm: f64,
        /// World emission direction FROM the source.
        emitted_axis: [f64; 3],
        inner_half_angle_radians: f64,
        outer_half_angle_radians: f64,
    },
    Area {
        position_mm: [f64; 3],
        /// Full world-space side vectors; U cross V is the emitting side.
        edge_u_mm: [f64; 3],
        edge_v_mm: [f64; 3],
    },
}

impl Geometry {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Directional { .. } => "directional",
            Self::Point { .. } => "point",
            Self::Spot { .. } => "spot",
            Self::Area { .. } => "area",
        }
    }
}

/// Ephemeral per-document light visibility. Never part of RenderSetup or a design.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Visibility {
    pub solo: Option<String>,
    pub muted: std::collections::BTreeSet<String>,
}

impl Visibility {
    pub fn visible(&self, id: &str) -> bool {
        !self.muted.contains(id) && self.solo.as_ref().is_none_or(|solo| solo == id)
    }

    pub fn active(&self) -> bool {
        self.solo.is_some() || !self.muted.is_empty()
    }
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SoloParams {
    pub id: Option<String>,
    pub solo: bool,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MuteParams {
    pub id: String,
    pub mute: bool,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LightPatch {
    pub kind: Option<String>,
    pub intensity_ev: Option<f64>,
    pub temperature_k: Option<f64>,
    pub softness: Option<f64>,
    pub azimuth_deg: Option<f64>,
    pub elevation_deg: Option<f64>,
    pub distance_mm: Option<f64>,
    pub size_mm: Option<f64>,
    pub aim_at: Option<[f64; 3]>,
    /// Full side lengths, preserving each existing edge's orientation.
    pub width_mm: Option<f64>,
    pub height_mm: Option<f64>,
    /// World-space fields, lowered to the existing renderer parameters.
    pub position_mm: Option<[f64; 3]>,
    pub direction: Option<[f64; 3]>,
    pub edge_u_mm: Option<[f64; 3]>,
    pub edge_v_mm: Option<[f64; 3]>,
    pub radiance: Option<[f64; 3]>,
    pub colour: Option<[f64; 3]>,
    pub reference_distance_mm: Option<f64>,
    pub minimum_distance_mm: Option<f64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EditParams {
    pub name: String,
    pub id: Option<String>,
    /// Complete unsaved setup, used as the edit's starting point without losing other controls.
    #[schemars(with = "Option<std::collections::BTreeMap<String, Value>>")]
    pub settings: Option<Value>,
    pub set: Option<LightPatch>,
    pub expected_revision: Option<u64>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct EditResult {
    pub id: Option<String>,
    pub name: String,
    pub setup: Value,
}
