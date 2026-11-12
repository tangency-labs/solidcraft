//! Contracts and result types for `cam.features` in `cmd/cam_features.rs`: the machinable
//! features of a part, found on its exact B-rep. docs/cam-jobs.md describes it.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The direction(s) the tool comes from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AxisSpec {
    /// A 3-axis mill: the tool comes down along work -Z only.
    #[default]
    Z,
    /// A 4th axis about work X (the A axis): every face square to X can be turned up under the
    /// tool, each at the A angle that brings its normal up.
    A,
}

/// List the machinable features of a part, found on the exact B-rep of its bodies: holes,
/// pockets, planar faces facing the tool, outer profiles, fillets and chamfers on top edges,
/// other 3D surfaces and (with `axis: "a"`) turned diameters, each with a stable id
/// (`hole:0`, `pocket:1`…) a `cam.job` operation can name instead of a sketch or polygon.
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
    /// `z` (3-axis) or `a` (a 4th axis about work X). Default: `z`.
    #[serde(default)]
    pub axis: Option<AxisSpec>,
    /// How close two sizes or positions must be to count as equal, mm, 0.0001 to 1 (as a job's
    /// `tolerance`). Default: 0.01.
    #[serde(default)]
    pub tolerance: Option<f64>,
}

/// What `cam.features` returns. Every coordinate of a feature is in its setup's frame: work
/// coordinates (the design less `origin`) turned by the setup's A angle, so its Z is the tool
/// axis and its numbers go straight into an operation of that setup.
#[derive(Clone, Debug, Default, Serialize, JsonSchema)]
pub struct CamFeaturesReply {
    /// `z` or `a`.
    pub axis: AxisSpec,
    /// Work zero in design coordinates.
    pub origin: [f64; 3],
    /// The bodies looked at.
    pub bodies: Vec<String>,
    pub tolerance: f64,
    /// One setup per tool direction: `a_deg` 0 first (with `axis: "z"` the only one).
    pub setups: Vec<SetupReport>,
    pub holes: Vec<HoleReport>,
    /// Holes of the same diameter, depth and kind, for one drilling operation each.
    pub hole_groups: Vec<HoleGroup>,
    pub pockets: Vec<PocketReport>,
    /// Planar faces facing the tool.
    pub faces: Vec<FaceReport>,
    /// Outer profiles to contour round.
    pub profiles: Vec<ProfileReport>,
    /// Rounds on top edges.
    pub fillets: Vec<EdgeBreakReport>,
    /// Chamfers on top edges.
    pub chamfers: Vec<EdgeBreakReport>,
    /// Other curved areas the tool reaches from above (3D finishing).
    pub surfaces: Vec<SurfaceReport>,
    /// Diameters turned about the A axis (`axis: "a"` only).
    pub turned: Vec<TurnedReport>,
    /// Pockets wrapped round the A axis (`axis: "a"` only): a floor on a cylinder about X,
    /// below the part's surface over part of a turn, its walls radial or square to X.
    pub wrapped: Vec<WrappedReport>,
    /// What was not recognised, or not reachable along the axis.
    pub notes: Vec<String>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// One tool direction.
#[derive(Clone, Debug, Default, Serialize, JsonSchema)]
pub struct SetupReport {
    /// The A angle that brings this side up, degrees (0 with `axis: "z"`).
    pub a_deg: f64,
    /// The tool axis (pointing from the part to the spindle) in work coordinates.
    pub tool_axis: [f64; 3],
    /// The part's highest Z in this setup's frame (an upper bound on curved surfaces).
    pub top_z: f64,
    /// The part's lowest Z in this setup's frame.
    pub bottom_z: f64,
    /// The part's extent in this frame's XY: [[low x, low y], [high x, high y]].
    pub bounds: [[f64; 2]; 2],
    /// With `axis: "a"`: the largest distance of the part from the A axis.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub swing_radius: Option<f64>,
    /// The ids of the features machined from this side.
    pub features: Vec<String>,
}

/// A round hole along the tool axis.
#[derive(Clone, Debug, Default, Serialize, JsonSchema)]
pub struct HoleReport {
    pub id: String,
    /// The setup's A angle (with `axis: "a"`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub a_deg: Option<f64>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub a_deg: Option<f64>,
    pub count: usize,
    pub ids: Vec<String>,
}

/// A closed pocket: a planar floor below the faces round it, walled all round.
#[derive(Clone, Debug, Default, Serialize, JsonSchema)]
pub struct PocketReport {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub a_deg: Option<f64>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub a_deg: Option<f64>,
    pub body: String,
    /// The face (as `model.faces` numbers it).
    pub face: usize,
    pub z: f64,
    pub area: f64,
    pub bounds: [[f64; 2]; 2],
    /// `top` (the highest), `plateau` (everything round it falls away), `step` (open on some
    /// sides), `island_top` (the top of a boss standing in a pocket), `pocket_floor`, `hole_floor`
    /// (a blind hole's floor or a counterbore's step),
    /// `wrapped_wall` (a wall of a pocket wrapped round the A axis, which `pocket` names).
    pub role: String,
    /// The pocket whose floor it is.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pocket: Option<String>,
    /// The face's outward normal in work coordinates.
    pub normal_work: [f64; 3],
}

/// The outline of a face whose walls fall straight away all round: what a contour runs
/// outside of, from `top_z` down to `bottom_z`.
#[derive(Clone, Debug, Default, Serialize, JsonSchema)]
pub struct ProfileReport {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub a_deg: Option<f64>,
    pub body: String,
    pub top_z: f64,
    pub bottom_z: f64,
    pub depth: f64,
    pub outline: Vec<[f64; 2]>,
    pub area: f64,
    pub bounds: [[f64; 2]; 2],
    /// The smallest radius of the outline's notches (where a tool running outside must fit);
    /// absent when it has none.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_concave_radius: Option<f64>,
    /// The face it is the outline of (`face:k`).
    pub face_id: String,
}

/// A fillet or chamfer on top edges.
#[derive(Clone, Debug, Default, Serialize, JsonSchema)]
pub struct EdgeBreakReport {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub a_deg: Option<f64>,
    pub body: String,
    pub faces: Vec<usize>,
    /// A fillet's radius.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub radius: Option<f64>,
    /// A chamfer's angle from the tool axis, degrees.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub angle_deg: Option<f64>,
    pub top_z: f64,
    pub bottom_z: f64,
    pub bounds: [[f64; 2]; 2],
}

/// Curved faces for 3D finishing.
#[derive(Clone, Debug, Default, Serialize, JsonSchema)]
pub struct SurfaceReport {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub a_deg: Option<f64>,
    pub body: String,
    pub faces: Vec<usize>,
    /// The kinds of surface: `sphere`, `torus`, `cylinder`, `cone`, `freeform`, `plane`
    /// (sloped).
    pub kinds: Vec<String>,
    /// The smallest radius of curvature of its analytic faces (a ball's radius should not
    /// exceed it in the concave ones); absent for free-form faces only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_radius: Option<f64>,
    pub top_z: f64,
    pub bottom_z: f64,
    pub bounds: [[f64; 2]; 2],
    /// Its faces the tool axis cannot see all of (undercuts): a face facing down with material
    /// under it, or a face the part overhangs. A 3D finish from above leaves them; cut them from
    /// an index on the 4th axis, a side setup, or with a tool that reaches under.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub undercut_faces: Vec<usize>,
}

/// A diameter turned about the A axis.
#[derive(Clone, Debug, Default, Serialize, JsonSchema)]
pub struct TurnedReport {
    pub id: String,
    pub body: String,
    pub diameter: f64,
    pub x_min: f64,
    pub x_max: f64,
    pub faces: Vec<usize>,
    /// A lobe (a cam's) on this diameter: the sides at A angles (`face:k`, `fillet:k`...) that
    /// stand out of it within its X range. One rotary feature: its turning and finishing name this
    /// id.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub lobe: Vec<String>,
    /// With a lobe: its largest radius from the A axis, the largest of `profile`'s.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub radius_max: Option<f64>,
    /// With a lobe: `[a_deg, radius]` every 15 degrees of A, at the middle of its sides along X:
    /// turned to A `a_deg` the part's top there is at Z `radius`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub profile: Vec<[f64; 2]>,
}

/// A pocket wrapped round the A axis: its floor a cylinder about X below the part's surface,
/// over part of a turn, cut with `wrap_pocket` (and its walls with `wrap_contour`).
#[derive(Clone, Debug, Default, Serialize, JsonSchema)]
pub struct WrappedReport {
    pub id: String,
    pub body: String,
    /// The floor's face.
    pub floor_face: usize,
    /// The walls' faces: their face reports (role `wrapped_wall`) name this pocket.
    pub walls: Vec<usize>,
    /// The radius of the part's surface the pocket is cut into: the wrap's `radius`.
    pub radius: f64,
    /// The floor's radius.
    pub floor_radius: f64,
    /// `radius - floor_radius`: the wrap's `bottom` is its negative.
    pub depth: f64,
    pub x_min: f64,
    pub x_max: f64,
    /// The angles (A, degrees) the pocket runs between, `a_min_deg < a_max_deg`.
    pub a_min_deg: f64,
    pub a_max_deg: f64,
    pub span_deg: f64,
    /// The floor's outline unrolled at `radius`: [x, arc length round it] (y = radius × A in
    /// radians), the wrap's `polygon`; counter-clockwise.
    pub outline: Vec<[f64; 2]>,
}

#[cfg(test)]
#[path = "cam_features_tests.rs"]
mod tests;
