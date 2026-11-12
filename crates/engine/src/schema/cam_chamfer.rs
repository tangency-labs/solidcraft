//! The `chamfer` operation of a CAM job (docs/cam-jobs.md): a chamfer or deburr along the top
//! edges of closed profiles with a chamfer mill.

use schemars::JsonSchema;
use serde::Deserialize;

use super::{Common, FeedsSpec, HoleSpec, SideSpec};
use crate::schema::types::SketchRef;

/// The Z levels of a chamfer, in the operation's coordinates, mm: the depth follows from the
/// chamfer's width and the tool's angle.
#[derive(Clone, Copy, Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ChamferHeightsSpec {
    /// Where the tool travels between cuts.
    pub clearance: f64,
    /// Where rapid motion towards the material stops and feed begins.
    pub retract: f64,
    /// The top face whose edges are chamfered.
    pub top: f64,
}

operation! {
    /// A 2D chamfer or deburr with a chamfer mill along closed profiles' top edges, one pass deep.
    ChamferOp {
        /// The sketch whose closed profiles' edges are chamfered (on a plane square to the tool
        /// axis, facing it). Give this or `polygon`.
        #[serde(default)]
        pub sketch: Option<SketchRef>,
        /// With `sketch`: which of its profiles, by index. Default: every drawn profile.
        #[serde(default)]
        pub profiles: Option<Vec<usize>>,
        /// One closed polygon [[x, y], ...] in the operation's XY.
        #[serde(default)]
        pub polygon: Option<Vec<[f64; 2]>>,
        /// Round holes' rims as [{center, diameter}], each chamfered inside its hole (as `bore`
        /// takes holes). Give this, a `sketch` or a `polygon`.
        #[serde(default)]
        pub holes: Option<Vec<HoleSpec>>,
        /// The Z levels.
        pub heights: ChamferHeightsSpec,
        /// The chamfer's width across the top face (its horizontal leg), mm. Its depth down the
        /// wall is the width over the tangent of half the tool's angle (equal for a 90° tool).
        pub width: f64,
        /// How far the tip goes below the chamfer, mm, so the cone's side cuts all of it.
        /// Default: 0.2.
        #[serde(default)]
        pub tip_offset: Option<f64>,
        /// `outside` a part (the profile stays) or `inside` an opening. Default: outside; with
        /// `holes`, inside (and only inside).
        #[serde(default)]
        pub side: Option<SideSpec>,
        /// Climb milling. Default: true.
        #[serde(default)]
        pub climb: Option<bool>,
        /// Chord error on curved profiles, mm. Default: the job's tolerance.
        #[serde(default)]
        pub tolerance: Option<f64>,
    }
}
