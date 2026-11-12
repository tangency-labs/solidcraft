//! The contracts of the 3D finishing operations of `cam.job` (docs/cam-jobs.md, "3D finishing
//! strategies"): constant-scallop offsets, steep and shallow finishing, pencil tracing of
//! creases and rest finishing after a larger tool. Their fields, like every operation's, refuse
//! keys they do not know.

use schemars::JsonSchema;
use serde::Deserialize;

use super::{BoundarySpec, Common, FeedsSpec, HeightsSpec};

/// The order offset passes run in.
#[derive(Clone, Copy, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DirectionSpec {
    /// From the boundary towards the middle.
    Inward,
    /// From the middle out to the boundary.
    Outward,
}

/// The passes on shallow ground.
#[derive(Clone, Copy, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ShallowSpec {
    /// Passes offset from the boundary and from the steep ground, a constant scallop apart.
    Scallop,
    /// Straight passes at `angle_deg`.
    Parallel,
}

operation! {
    /// 3D constant-scallop finishing: passes offset along the surface from the boundary, each a
    /// spacing from the last as measured over the surface, so the scallop is the same on steep
    /// and flat ground; finished with a pass along every sharp concave crease.
    ScallopOp {
        /// The Z levels: `bottom` is the floor, `top` is not used.
        pub heights: HeightsSpec,
        /// The ridge height to leave between passes, mm (needs a ball or bull nose); the
        /// passes are spaced for it, closer where the surface is concave. Give this or
        /// `stepover`.
        #[serde(default)]
        pub scallop_height: Option<f64>,
        /// Distance between passes along the surface, mm. Default: the tool's `stepover`, else
        /// 10% of its diameter.
        #[serde(default)]
        pub stepover: Option<f64>,
        /// `inward` (from the boundary) or `outward` (from the middle). Default: inward.
        #[serde(default)]
        pub direction: Option<DirectionSpec>,
        /// Where the cutter centre may go. Default: the part's extent.
        #[serde(default)]
        pub boundary: Option<BoundarySpec>,
        /// Material left on the surface, mm. Default: 0.
        #[serde(default)]
        pub stock_to_leave: Option<f64>,
        /// Spacing of the grid distances are measured on, and of the samples along a pass, mm.
        /// Default: a tenth of the tool's diameter.
        #[serde(default)]
        pub step: Option<f64>,
        /// Largest distance from the surface, mm. Default: the job's tolerance.
        #[serde(default)]
        pub tolerance: Option<f64>,
        /// Climb milling. Default: true.
        #[serde(default)]
        pub climb: Option<bool>,
    }
}

operation! {
    /// 3D steep and shallow finishing: z-levels where the ground is steeper than a threshold,
    /// offset or raster passes where it is shallower, overlapping at the edge between.
    SteepShallowOp {
        /// The Z levels: z-levels from `top` down to `bottom`; `bottom` is the floor too.
        pub heights: HeightsSpec,
        /// As `scallop`'s: the scallop height to space shallow passes by (and steep levels
        /// unless `stepdown`). Give this or `stepover`.
        #[serde(default)]
        pub scallop_height: Option<f64>,
        /// Distance between passes along the surface, mm. Default: the tool's `stepover`, else
        /// 10% of its diameter.
        #[serde(default)]
        pub stepover: Option<f64>,
        /// Ground steeper than this is steep, degrees from horizontal (1 to 89). Default: 40.
        #[serde(default)]
        pub threshold_deg: Option<f64>,
        /// How far each kind of pass runs on into the other's ground, mm. Default: a tenth of
        /// the tool's diameter.
        #[serde(default)]
        pub overlap: Option<f64>,
        /// Most height between z-levels, mm. Default: the spacing times the sine of the
        /// threshold, so levels are a spacing apart along the shallowest steep ground.
        #[serde(default)]
        pub stepdown: Option<f64>,
        /// `scallop` or `parallel` on shallow ground. Default: scallop.
        #[serde(default)]
        pub shallow: Option<ShallowSpec>,
        /// With `shallow: parallel`: the passes' direction, degrees from +X. Default: 0.
        #[serde(default)]
        pub angle_deg: Option<f64>,
        /// Where the cutter centre may go. Default: the part's extent.
        #[serde(default)]
        pub boundary: Option<BoundarySpec>,
        /// Material left on the surface, mm. Default: 0.
        #[serde(default)]
        pub stock_to_leave: Option<f64>,
        /// Spacing of the grid slopes and levels are found on, mm. Default: a tenth of the
        /// tool's diameter.
        #[serde(default)]
        pub step: Option<f64>,
        /// Largest distance from the surface, mm. Default: the job's tolerance.
        #[serde(default)]
        pub tolerance: Option<f64>,
        /// Climb milling. Default: true.
        #[serde(default)]
        pub climb: Option<bool>,
    }
}

operation! {
    /// 3D pencil finishing: along the concave creases where the cutter touches two faces at
    /// once, with offset passes beside them if asked.
    PencilOp {
        /// The Z levels: `bottom` is the floor, `top` is not used.
        pub heights: HeightsSpec,
        /// Least turn of the tip's path across a crease, degrees (1 to 170). Default: 30.
        #[serde(default)]
        pub min_angle_deg: Option<f64>,
        /// Offset passes on each side of every crease, 0 to 100. Default: 0.
        #[serde(default)]
        pub passes: Option<u32>,
        /// Distance between offset passes along the surface, mm. Default: the tool's
        /// `stepover`, else 10% of its diameter.
        #[serde(default)]
        pub stepover: Option<f64>,
        /// Creases shorter than this are left out, mm. Default: the tool's radius.
        #[serde(default)]
        pub min_length: Option<f64>,
        /// Where the cutter centre may go. Default: the part's extent.
        #[serde(default)]
        pub boundary: Option<BoundarySpec>,
        /// Material left on the surface, mm. Default: 0.
        #[serde(default)]
        pub stock_to_leave: Option<f64>,
        /// Spacing of the grid creases are looked for on, mm. Default: a tenth of the tool's
        /// diameter.
        #[serde(default)]
        pub step: Option<f64>,
        /// Largest distance from the surface, mm. Default: the job's tolerance.
        #[serde(default)]
        pub tolerance: Option<f64>,
        /// Climb milling. Default: true.
        #[serde(default)]
        pub climb: Option<bool>,
    }
}

operation! {
    /// 3D rest finishing: only where this tool reaches material a previous, larger tool left.
    RestFinishOp {
        /// The Z levels: `bottom` is the floor, `top` is not used.
        pub heights: HeightsSpec,
        /// The `number` of the tool that cut before (a tool of the job, of another shape).
        pub previous_tool: u32,
        /// Least material the previous tool left, square to the surface, for a place to be cut,
        /// mm. Default: twice the operation's tolerance.
        #[serde(default)]
        pub threshold: Option<f64>,
        /// As `scallop`'s. Give this or `stepover`.
        #[serde(default)]
        pub scallop_height: Option<f64>,
        /// Distance between passes along the surface, mm. Default: the tool's `stepover`, else
        /// 10% of its diameter.
        #[serde(default)]
        pub stepover: Option<f64>,
        /// How far round what is left the passes run on, mm. Default: a quarter of the tool's
        /// diameter.
        #[serde(default)]
        pub overlap: Option<f64>,
        /// Where the cutter centre may go. Default: the part's extent.
        #[serde(default)]
        pub boundary: Option<BoundarySpec>,
        /// Material left on the surface, by this tool and as the previous one left it, mm.
        /// Default: 0.
        #[serde(default)]
        pub stock_to_leave: Option<f64>,
        /// Spacing of the grid the two tools are compared on, mm. Default: a tenth of the
        /// tool's diameter.
        #[serde(default)]
        pub step: Option<f64>,
        /// Largest distance from the surface, mm. Default: the job's tolerance.
        #[serde(default)]
        pub tolerance: Option<f64>,
        /// Climb milling. Default: true.
        #[serde(default)]
        pub climb: Option<bool>,
    }
}
