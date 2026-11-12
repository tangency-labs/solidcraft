//! Contracts and result types for `cam.features` (in `cmd/cam_features.rs`): the holes of a part
//! a 3-axis mill drills or bores from above, found on its exact B-rep. docs/cam-jobs.md
//! describes it.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// List the holes of a part a 3-axis mill drills or bores from above, found on the exact B-rep
/// of its bodies: round holes along work Z with their floors, counterbores, countersinks and
/// threads, each with a stable id (`hole:0`, `hole:1`…), and the holes grouped by size.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CamFeaturesParams {
    /// The bodies of the part, by name. Default: every visible body.
    #[serde(default)]
    pub bodies: Option<Vec<String>>,
    /// The design point that is work zero, [x, y, z] mm (as a job's `origin`). Default: the
    /// design's origin.
    #[serde(default)]
    pub origin: Option<[f64; 3]>,
    /// How close two sizes or positions must be to count as equal, mm, 0.0001 to 1 (as a job's
    /// `tolerance`). Default: 0.01.
    #[serde(default)]
    pub tolerance: Option<f64>,
}

/// What `cam.features` returns. Every coordinate of a feature is in work coordinates (the design
/// less `origin`), the tool along work Z.
#[derive(Clone, Debug, Default, Serialize, JsonSchema)]
pub struct CamFeaturesReply {
    /// Work zero in design coordinates.
    pub origin: [f64; 3],
    /// The bodies looked at.
    pub bodies: Vec<String>,
    pub tolerance: f64,
    /// The setup: the tool along work Z, from above.
    pub setups: Vec<SetupReport>,
    pub holes: Vec<HoleReport>,
    /// Holes of the same diameter, depth and kind, for one drilling operation each.
    pub hole_groups: Vec<HoleGroup>,
    /// What was not recognised, or not reachable along the axis.
    pub notes: Vec<String>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// One tool direction.
#[derive(Clone, Debug, Default, Serialize, JsonSchema)]
pub struct SetupReport {
    /// The tool axis (pointing from the part to the spindle) in work coordinates.
    pub tool_axis: [f64; 3],
    /// The part's highest Z in this setup's frame (an upper bound on curved surfaces).
    pub top_z: f64,
    /// The part's lowest Z in this setup's frame.
    pub bottom_z: f64,
    /// The part's extent in this frame's XY: [[low x, low y], [high x, high y]].
    pub bounds: [[f64; 2]; 2],
    /// The ids of the features machined from this side.
    pub features: Vec<String>,
}

/// A round hole along the tool axis.
#[derive(Clone, Debug, Default, Serialize, JsonSchema)]
pub struct HoleReport {
    pub id: String,
    pub body: String,
    /// The axis's position in the setup's XY.
    pub center: [f64; 2],
    /// Where the axis meets the hole's top, in work coordinates.
    pub center_work: [f64; 3],
    /// The tool axis of the setup in work coordinates.
    pub axis_work: [f64; 3],
    pub diameter: f64,
    /// Where the round wall starts (the face it opens in).
    pub top_z: f64,
    /// Where the round wall ends.
    pub bottom_z: f64,
    /// `top_z - bottom_z`.
    pub depth: f64,
    /// It comes out on the other side.
    pub through: bool,
    /// `through`, `flat` (a flat floor), `cone` (a drill point) or `round` (the wall runs on into
    /// a curved floor within its radius, a round at its foot or a ball end, below `bottom_z`).
    pub floor: String,
    /// The drill point's tip (with `floor: "cone"`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tip_z: Option<f64>,
    /// The larger hole above this one that is its counterbore.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub counterbore: Option<String>,
    /// The smaller hole this one is the counterbore of.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub counterbore_of: Option<String>,
    /// A countersink at the top: its largest diameter and included angle.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub countersink: Option<Countersink>,
    /// [lowest, highest] Z over which the round wall does not close all the way round: the hole
    /// breaks into another feature there (a slot across its foot). Absent when the wall is whole.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub breaks_out: Option<[f64; 2]>,
    /// The body's faces (as `model.faces` numbers them) that make the round wall.
    pub faces: Vec<usize>,
    /// The thread the design taps in it (a hole made with a thread): the hole is its tap drill.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread: Option<HoleThread>,
}

/// A tapped hole's thread, as the design gives it.
#[derive(Clone, Debug, Default, Serialize, JsonSchema)]
pub struct HoleThread {
    /// As its standard writes it, e.g. "M6".
    pub designation: String,
    /// The thread's form, e.g. `metric`.
    pub form: String,
    /// The major (nominal) diameter, mm.
    pub major_diameter: f64,
    /// Advance per turn, mm.
    pub pitch: f64,
    /// Grooves side by side (lead = starts x pitch).
    pub starts: u32,
    /// `right`, `left` or `both`.
    pub hand: String,
    /// Where the threaded length starts and ends, in the setup's Z.
    pub top_z: f64,
    pub bottom_z: f64,
    /// `top_z - bottom_z`.
    pub depth: f64,
}

#[derive(Clone, Debug, Default, Serialize, JsonSchema)]
pub struct Countersink {
    pub diameter: f64,
    pub angle_deg: f64,
}

#[derive(Clone, Debug, Default, Serialize, JsonSchema)]
pub struct HoleGroup {
    pub diameter: f64,
    pub depth: f64,
    pub through: bool,
    pub count: usize,
    pub ids: Vec<String>,
}

#[cfg(test)]
#[path = "cam_features_tests.rs"]
mod tests;
