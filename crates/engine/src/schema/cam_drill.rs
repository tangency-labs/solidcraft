//! The `drill` operation of a CAM job (docs/cam-jobs.md, "Drilling"): holes at circle centres
//! and points of a sketch, or at explicit points, in the operation's frame.

use schemars::JsonSchema;
use serde::Deserialize;

use super::{Common, FeedsSpec, HeightsSpec};
use crate::schema::types::SketchRef;

/// How each hole is drilled. Every cycle is written as plain G0/G1 moves (and G4 for the
/// dwell), not as a canned cycle.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DrillCycleSpec {
    /// Feed to depth, rapid out.
    Simple,
    /// Feed to depth, pause `dwell_seconds`, rapid out.
    Dwell,
    /// Feed `peck` deeper at a time, rapid right out to the retract height after each peck,
    /// and back down to `peck_clearance` above the depth reached.
    Peck,
    /// Feed `peck` deeper at a time, backing off `back_off` after each to break the chip.
    ChipBreak,
}

operation! {
    /// Drilling: holes fed straight down the tool axis, with a drill or a flat end mill.
    DrillOp {
        /// A sketch whose circle centres and points are the holes (on a plane square to the
        /// tool axis). Give this or `points`.
        #[serde(default)]
        pub sketch: Option<SketchRef>,
        /// With `sketch`: drill at the centres of its circles (not construction ones).
        /// Default: true.
        #[serde(default)]
        pub circles: Option<bool>,
        /// With `sketch`: only circles of a diameter from the first to the second, mm.
        /// Default: every circle.
        #[serde(default)]
        pub diameter_range: Option<[f64; 2]>,
        /// With `sketch`: drill at its points that are not part of a curve. Default: true.
        #[serde(default)]
        pub sketch_points: Option<bool>,
        /// The holes [[x, y], ...] in the operation's XY. Give this or `sketch`.
        #[serde(default)]
        pub points: Option<Vec<[f64; 2]>>,
        /// `clearance`: travel between holes; `retract`: the R height, where feed begins;
        /// `top`: the hole's top, where pecks are counted from; `bottom`: its depth.
        pub heights: HeightsSpec,
        /// The cycle. Default: simple.
        #[serde(default)]
        pub cycle: Option<DrillCycleSpec>,
        /// With `peck` and `chip_break`: depth per peck, mm. Default: the tool's `stepdown`,
        /// else its diameter.
        #[serde(default)]
        pub peck: Option<f64>,
        /// With `peck`: how far above the depth reached the tool comes back down to at rapid,
        /// mm. Default: 0.5.
        #[serde(default)]
        pub peck_clearance: Option<f64>,
        /// With `chip_break`: how far the tool backs off after each peck, mm. Default: 0.5.
        #[serde(default)]
        pub back_off: Option<f64>,
        /// With `dwell`: the pause at depth, seconds. Default: 0.5.
        #[serde(default)]
        pub dwell_seconds: Option<f64>,
        /// `bottom` is where a drill reaches its full diameter: the tip goes deeper by the
        /// point's length (radius / tan(point angle / 2)). Default: false (`bottom` is the tip).
        #[serde(default)]
        pub to_full_diameter: Option<bool>,
    }
}
