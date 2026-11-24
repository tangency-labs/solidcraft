//! Contract and reply type for `gear.insert` in `cmd/gears.rs`: involute spur gears (#6796).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Insert an involute spur gear, external or internal, as a new component: a tooth outline sketched on
/// the XY plane and extruded along +Z. One undo step.
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
    /// Clearance coefficient from 0 to 0.6 of the module (default 0.25): the roots lie `module × (1 + clearance - profile_shift)` inside the pitch circle.
    #[serde(default)]
    pub clearance: Option<f64>,
    /// `thinner` (default): the backlash is taken off the teeth. `thicker`: it is added to them, for a mate that carries the whole clearance.
    #[serde(default)]
    pub backlash_direction: Option<String>,
    /// An internal gear: a ring whose teeth point inward (default false). Profile shift is not used.
    #[serde(default)]
    pub internal: Option<bool>,
    /// Internal gears only: ring thickness outside the tooth roots in millimetres (default: twice the module).
    #[serde(default)]
    pub rim_thickness: Option<f64>,
    /// External gears only: diameter of an axle hole through the centre in millimetres (default: none).
    #[serde(default)]
    pub bore_diameter: Option<f64>,
    /// External gears only: how far the axle hole's centre is from the gear's centre along +X, in
    /// millimetres (default 0). The hole, with this offset, must stay inside 90% of the root circle.
    #[serde(default)]
    pub bore_offset: Option<f64>,
    /// Helix angle on the pitch circle in degrees, from -45 to 45 (default 0: straight teeth).
    /// The teeth turn over the face width; a positive angle is a right-hand helix.
    #[serde(default)]
    pub helix_angle: Option<f64>,
    /// External helical gears only: two mirrored halves, each half the face width (default false).
    #[serde(default)]
    pub double_helical: Option<bool>,
    /// The component's name (default: `Spur Gear M2 Z20`, or `Internal Gear M2 Z40`).
    #[serde(default)]
    pub name: Option<String>,
}

/// Change an inserted gear's values and rebuild its teeth in place; the component, its placement
/// and its joints stay. One undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct GearEditParams {
    /// The gear's component, by id or name.
    pub component: serde_json::Value,
    /// New module in millimetres.
    #[serde(default)]
    pub module: Option<f64>,
    /// New number of teeth, 6 to 1000.
    #[serde(default)]
    pub teeth: Option<u32>,
    /// New pressure angle in degrees.
    #[serde(default)]
    pub pressure_angle: Option<f64>,
    /// New thickness along +Z in millimetres.
    #[serde(default)]
    pub face_width: Option<f64>,
    /// New profile shift coefficient.
    #[serde(default)]
    pub profile_shift: Option<f64>,
    /// New backlash in millimetres.
    #[serde(default)]
    pub backlash: Option<f64>,
    /// New clearance coefficient from 0 to 0.6 of the module : the roots lie `module × (1 + clearance - profile_shift)` inside the pitch circle.
    #[serde(default)]
    pub clearance: Option<f64>,
    /// `thinner`: the backlash is taken off the teeth. `thicker`: it is added to them, for a mate that carries the whole clearance.
    #[serde(default)]
    pub backlash_direction: Option<String>,
    /// New points per flank, 2 to 64.
    #[serde(default)]
    pub flank_points: Option<u32>,
    /// Make it an internal gear (true) or an external one (false).
    #[serde(default)]
    pub internal: Option<bool>,
    /// New ring thickness outside the tooth roots of an internal gear, in millimetres.
    #[serde(default)]
    pub rim_thickness: Option<f64>,
    /// New diameter of the axle hole through an external gear in millimetres; 0 removes it.
    #[serde(default)]
    pub bore_diameter: Option<f64>,
    /// New offset of the axle hole along +X in millimetres.
    #[serde(default)]
    pub bore_offset: Option<f64>,
    /// New helix angle in degrees; 0 makes the teeth straight.
    #[serde(default)]
    pub helix_angle: Option<f64>,
    /// Make it a double helical gear (true) or not (false).
    #[serde(default)]
    pub double_helical: Option<bool>,
}

/// What `gear.insert` and `gear.edit` return.
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
    /// Tooth tip diameter in millimetres: the outside of an external gear, the inside of an internal one.
    pub tip_diameter: f64,
    /// Tooth root diameter in millimetres.
    pub root_diameter: f64,
}
