//! Contracts and result types for the commands in `cmd/pulleys.rs`: timing-belt pulleys (#20116).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The belt profile a pulley fits.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum BeltProfile {
    /// GT2, 2 mm pitch.
    Gt2,
    /// HTD 3M, 3 mm pitch.
    Htd3m,
    /// HTD 5M, 5 mm pitch.
    Htd5m,
}

/// Insert a timing-belt pulley as a new component at the origin, its axis along Z. One undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct PulleyInsertParams {
    pub profile: BeltProfile,
    /// The number of grooves.
    #[schemars(range(min = 10, max = 200))]
    pub teeth: u32,
    /// The belt's width in mm (default 6 for gt2, 9 for htd3m, 15 for htd5m). The toothed part is
    /// 1 mm wider so the belt runs free.
    #[serde(default)]
    #[schemars(range(min = 1.0, max = 100.0))]
    pub belt_width: Option<f64>,
    /// A flange at each end to keep the belt on (default true).
    #[serde(default)]
    pub flanges: Option<bool>,
    /// An axle hole's diameter in mm (default: no hole).
    #[serde(default)]
    #[schemars(range(min = 0.0))]
    pub bore_diameter: Option<f64>,
    /// The axle hole's centre along +X, in mm (default 0: on the axis).
    #[serde(default)]
    pub bore_offset: Option<f64>,
    /// The component's name (default: the pulley's, such as `GT2 Pulley 20T 6mm`).
    #[serde(default)]
    pub name: Option<String>,
}

/// What `pulley.insert` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct InsertedPulley {
    /// The new component's id.
    pub component: u64,
    /// Its occurrence's id.
    pub occurrence: u64,
    /// The component's name.
    pub name: String,
    /// The belt's pitch line: teeth × pitch / π (mm).
    pub pitch_diameter: f64,
    /// Across the lands between the grooves (mm).
    pub outside_diameter: f64,
    /// Along Z, flanges included (mm).
    pub width: f64,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}
