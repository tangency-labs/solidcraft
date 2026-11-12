//! The helical operations of a CAM job, `bore` and `thread_mill` (docs/cam-jobs.md): a child
//! of `schema/cam.rs`, whose `operation!` macro gives them the keys every operation has.

use schemars::JsonSchema;
use serde::Deserialize;

use super::{Common, FeedsSpec, HeightsSpec, SketchRef};

/// A circular hole given by its centre and diameter.
#[derive(Clone, Copy, Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HoleSpec {
    /// The centre [x, y] in the operation's XY, mm.
    pub center: [f64; 2],
    /// The finished diameter, mm.
    pub diameter: f64,
}

operation! {
    /// Helical bores of circular holes: a lead-in inside the hole, a helix down at a pitch or
    /// ramp angle, a flat circle at the bottom and a lead-out; several rings for holes wider
    /// than two tool diameters, and finishing passes at full diameter.
    BoreOp {
        /// A sketch whose circles are the holes (on a plane square to the operation's Z, either
        /// way up; its construction circles are left out). Give this or `holes`.
        #[serde(default)]
        pub sketch: Option<SketchRef>,
        /// With `sketch`: only circles whose diameter is in [smallest, largest], mm. Default:
        /// every circle.
        #[serde(default)]
        pub diameter_range: Option<[f64; 2]>,
        /// The holes as [{center, diameter}].
        #[serde(default)]
        pub holes: Option<Vec<HoleSpec>>,
        /// The Z levels: `top` is the hole's top, `bottom` its floor.
        pub heights: HeightsSpec,
        /// Down this far each turn, mm. Give this or `ramp_angle_deg`.
        #[serde(default)]
        pub pitch: Option<f64>,
        /// The helix's slope at the tool centre, degrees (above 0, at most 45). Default: 2,
        /// the pitch at most `stepdown`.
        #[serde(default)]
        pub ramp_angle_deg: Option<f64>,
        /// With a ramp angle: the most the helix goes down each turn, mm. Default: the tool's
        /// `stepdown`, else a tenth of its diameter.
        #[serde(default)]
        pub stepdown: Option<f64>,
        /// Most distance between the rings of a wide hole, mm, at most the tool diameter.
        /// Default: the tool's `stepover`, else 40% of its diameter.
        #[serde(default)]
        pub stepover: Option<f64>,
        /// Climb milling (counter-clockwise inside the hole). Default: true.
        #[serde(default)]
        pub climb: Option<bool>,
        /// Material the roughing helix leaves on the wall, mm. Default: 0.1 with finishing
        /// passes, else 0.
        #[serde(default)]
        pub stock_to_leave: Option<f64>,
        /// Passes at the full diameter after roughing (0 to 100); after the first they are
        /// spring passes. Default: 0.
        #[serde(default)]
        pub finish_passes: Option<u32>,
    }
}

operation! {
    /// Thread milling round holes (internal) or bosses (external) with a thread mill: a
    /// helix at exactly one pitch per turn, in radial passes, with an optional spring pass.
    ThreadMillOp {
        /// A sketch whose circles' centres are the threads' axes (on a plane facing +Z of the
        /// operation; construction circles left out). Give this or `centers`.
        #[serde(default)]
        pub sketch: Option<SketchRef>,
        /// With `sketch`: only circles whose diameter is in [smallest, largest], mm.
        #[serde(default)]
        pub diameter_range: Option<[f64; 2]>,
        /// The threads' axes as [[x, y], ...] in the operation's XY.
        #[serde(default)]
        pub centers: Option<Vec<[f64; 2]>>,
        /// The Z levels: the thread runs from `top` down to `bottom` (its length).
        pub heights: HeightsSpec,
        /// Advance per turn, mm (1 for M6x1).
        pub pitch: f64,
        /// The thread's major (nominal) diameter, mm (6 for M6).
        pub thread_diameter: f64,
        /// Inside a hole; false: round a boss. Default: true.
        #[serde(default)]
        pub internal: Option<bool>,
        /// A right-hand thread. Default: true.
        #[serde(default)]
        pub right_hand: Option<bool>,
        /// Climb milling. Default: true.
        #[serde(default)]
        pub climb: Option<bool>,
        /// The thread's radial depth, mm. Default: the ISO metric depth for the pitch
        /// (0.5413 × pitch inside, 0.6134 × pitch outside).
        #[serde(default)]
        pub thread_depth: Option<f64>,
        /// Radial passes the depth is split into (1 to 100). Default: 1.
        #[serde(default)]
        pub radial_passes: Option<u32>,
        /// One more pass at full depth for the tool's spring. Default: false.
        #[serde(default)]
        pub spring_pass: Option<bool>,
    }
}
