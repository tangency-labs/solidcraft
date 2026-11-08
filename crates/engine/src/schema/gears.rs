//! Contract and reply type for `gear.insert` in `cmd/gears.rs`: involute spur gears (#6796).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Insert an external involute spur gear as a new component: a tooth outline sketched on the XY
/// plane and extruded along +Z. One undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct GearInsertParams {
    /// Module in millimetres: pitch diameter divided by tooth count. Above 0.
    pub module: f64,
    /// Number of teeth, 6 to 1000.
    pub teeth: u32,
    /// Pressure angle in degrees, above 0 and at most 35 (default 20).
    #[serde(default)]
    pub pressure_angle: Option<f64>,
    /// Thickness of the gear along +Z in millimetres. Above 0 (default: the module times 10).
    #[serde(default)]
    pub face_width: Option<f64>,
    /// Profile shift coefficient (default 0).
    #[serde(default)]
    pub profile_shift: Option<f64>,
    /// Backlash in millimetres taken from the tooth thickness on the pitch circle (default 0).
    #[serde(default)]
    pub backlash: Option<f64>,
    /// Points along each tooth flank, 2 to 64 (default 8). More points follow the involute
    /// more closely and build more slowly.
    #[serde(default)]
    pub flank_points: Option<u32>,
    /// The component's name (default: `Spur Gear M2 Z20`).
    #[serde(default)]
    pub name: Option<String>,
}

/// What `gear.insert` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct InsertedGear {
    /// The new component's id.
    pub component: u64,
    /// Its occurrence's id.
    pub occurrence: u64,
    /// The component's name.
    pub name: String,
    /// Pitch diameter in millimetres.
    pub pitch_diameter: f64,
    /// Base circle diameter in millimetres.
    pub base_diameter: f64,
    /// Tip (outside) diameter in millimetres.
    pub tip_diameter: f64,
    /// Root diameter in millimetres.
    pub root_diameter: f64,
}
