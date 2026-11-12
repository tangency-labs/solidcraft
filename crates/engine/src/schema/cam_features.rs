//! Contracts and result types for `cam.features` in `cmd/cam_features.rs`: the machinable
//! features of a part, found on its exact B-rep. docs/cam-jobs.md describes it.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// List the machinable features of a part, found on the exact B-rep of its bodies: holes,
/// pockets and the planar faces facing the tool, each with a stable id (`hole:0`, `pocket:1`…)
/// a `cam.job` operation can name instead of a sketch or polygon.
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

/// What `cam.features` returns. Every coordinate of a feature is in work coordinates (the
/// design less `origin`), Z the tool axis, so its numbers go straight into an operation.
#[derive(Clone, Debug, Default, Serialize, JsonSchema)]
pub struct CamFeaturesReply {
    /// Work zero in design coordinates.
    pub origin: [f64; 3],
    /// The bodies looked at.
    pub bodies: Vec<String>,
    pub tolerance: f64,
    /// The setup: the tool coming down work Z.
    pub setups: Vec<SetupReport>,
    pub holes: Vec<HoleReport>,
    /// Holes of the same diameter, depth and kind, for one drilling operation each.
    pub hole_groups: Vec<HoleGroup>,
    pub pockets: Vec<PocketReport>,
    /// Planar faces facing the tool.
    pub faces: Vec<FaceReport>,
    /// What was not recognised.
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

/// A closed pocket: a planar floor below the faces round it, walled all round.
#[derive(Clone, Debug, Default, Serialize, JsonSchema)]
pub struct PocketReport {
    pub id: String,
    pub body: String,
    /// The floor's face (as `model.faces` numbers it), also listed under `faces` as `face_id`.
    pub floor_face: usize,
    pub face_id: String,
    pub floor_z: f64,
    /// The top of its highest wall.
    pub top_z: f64,
    pub depth: f64,
    /// The floor's outline, counter-clockwise, curves within the tolerance.
    pub outline: Vec<[f64; 2]>,
    pub area: f64,
    pub bounds: [[f64; 2]; 2],
    /// The smallest radius of the outline's inside corners (where the tool must fit); 0 when
    /// a corner is sharp.
    pub min_corner_radius: f64,
    /// Sharp inside corners (a cutter leaves its own radius there).
    pub sharp_corners: usize,
    /// Every wall is square to the floor (no draft or round at its foot): the outline is the
    /// pocket's all the way up. When false the outline is the floor's, inside the walls: a 2D
    /// pocket or contour on it leaves the walls.
    pub walls_vertical: bool,
    /// Bosses standing in it.
    pub islands: Vec<IslandReport>,
    /// An opening through the part (a slot, an obround, a window): no floor; `floor_z` is where
    /// its walls end and it is cut a little past them. `floor_face` is one of its walls.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub through: bool,
}

#[derive(Clone, Debug, Default, Serialize, JsonSchema)]
pub struct IslandReport {
    pub outline: Vec<[f64; 2]>,
    pub top_z: f64,
    pub area: f64,
}

/// A planar face facing the tool.
#[derive(Clone, Debug, Default, Serialize, JsonSchema)]
pub struct FaceReport {
    pub id: String,
    pub body: String,
    /// The face (as `model.faces` numbers it).
    pub face: usize,
    pub z: f64,
    pub area: f64,
    pub bounds: [[f64; 2]; 2],
    /// `top` (the highest), `plateau` (everything round it falls away), `step` (open on some
    /// sides), `island_top` (the top of a boss standing in a pocket), `pocket_floor`,
    /// `hole_floor` (a blind hole's floor or a counterbore's step).
    pub role: String,
    /// The pocket whose floor it is.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pocket: Option<String>,
    /// The face's outward normal in work coordinates.
    pub normal_work: [f64; 3],
}

#[cfg(test)]
#[path = "cam_features_tests.rs"]
mod tests;
