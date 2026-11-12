//! The adaptive clearing operations of a CAM job (`adaptive`, `adaptive_roughing`) and what
//! their report adds; the rest of the job's contract is in `cam.rs`.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::cam::{BoundarySpec, Common, FeedsSpec, HeightsSpec, operation};
use super::types::SketchRef;

operation! {
    /// 2D adaptive clearing of closed profiles: an engagement-limited path that never takes more
    /// of the tool's circumference than `max_engagement_deg` (outside its entry helix), in depth
    /// passes.
    AdaptiveOp {
        /// The sketch whose closed profiles are cleared (on a plane square to the tool axis,
        /// facing it). Give this or `polygon`.
        #[serde(default)]
        pub sketch: Option<SketchRef>,
        /// With `sketch`: which of its profiles, by index. Default: every drawn profile.
        #[serde(default)]
        pub profiles: Option<Vec<usize>>,
        /// One closed polygon [[x, y], ...] in the operation's XY.
        #[serde(default)]
        pub polygon: Option<Vec<[f64; 2]>>,
        /// The Z levels.
        pub heights: HeightsSpec,
        /// Most depth per pass, mm. Default: the tool's `stepdown`, else its diameter.
        #[serde(default)]
        pub stepdown: Option<f64>,
        /// The radial width of cut aimed at, mm. Default: the tool's `stepover`, else 10% of its
        /// diameter.
        #[serde(default)]
        pub optimal_load: Option<f64>,
        /// The least engagement wanted, degrees. Default: a third of the optimal load's.
        #[serde(default)]
        pub min_engagement_deg: Option<f64>,
        /// The most engagement taken outside the entry, degrees (5 to 180). Default: one and a
        /// half times the optimal load's, and at least 15° more than it.
        #[serde(default)]
        pub max_engagement_deg: Option<f64>,
        /// Diameter the tool's centre follows down the entry helix, mm, at most the tool's
        /// diameter. Default: 80% of the tool's diameter.
        #[serde(default)]
        pub helix_diameter: Option<f64>,
        /// Descent angle of the entry helix, degrees (0.5 to 45). Default: 2.
        #[serde(default)]
        pub ramp_angle_deg: Option<f64>,
        /// Climb milling. Default: true.
        #[serde(default)]
        pub climb: Option<bool>,
        /// Material left on the walls, mm. Default: 0.
        #[serde(default)]
        pub stock_to_leave: Option<f64>,
        /// Chord error on curved profiles, mm. Default: the job's tolerance.
        #[serde(default)]
        pub tolerance: Option<f64>,
        /// Also plan a plain offset pocket of the same profiles and report it beside the
        /// adaptive path. Default: true.
        #[serde(default)]
        pub compare: Option<bool>,
        /// Take the job's stock box as what there is to cut: the material is the profiles'
        /// part of it, the tool goes down in the open air beside the stock (also past a
        /// profile drawn round the stock) and cuts in from there, and levels above the stock's
        /// top are left out. false: the profiles are full of material and entered with a helix.
        /// Without a stock box, or seen other than straight down, the profiles are full of
        /// material. Default: true.
        #[serde(default)]
        pub from_stock: Option<bool>,
    }
}

operation! {
    /// 3D adaptive roughing of a stock box in levels, staying `stock_to_leave` clear of the part,
    /// with the engagement limited as in `adaptive`; entered from open air round the stock, or
    /// with a helix where the material is enclosed.
    AdaptiveRoughingOp {
        /// The Z levels: `top` is the stock's top, `bottom` the lowest level.
        pub heights: HeightsSpec,
        /// Most material per level, mm. Default: the tool's `stepdown`, else its diameter.
        #[serde(default)]
        pub stepdown: Option<f64>,
        /// The radial width of cut aimed at, mm. Default: the tool's `stepover`, else 10% of its
        /// diameter.
        #[serde(default)]
        pub optimal_load: Option<f64>,
        /// The least engagement wanted, degrees. Default: a third of the optimal load's.
        #[serde(default)]
        pub min_engagement_deg: Option<f64>,
        /// The most engagement taken outside an entry, degrees (5 to 180). Default as `adaptive`.
        #[serde(default)]
        pub max_engagement_deg: Option<f64>,
        /// Diameter of an entry helix, mm. Default: 80% of the tool's diameter.
        #[serde(default)]
        pub helix_diameter: Option<f64>,
        /// Descent angle of an entry helix, degrees. Default: 2.
        #[serde(default)]
        pub ramp_angle_deg: Option<f64>,
        /// Climb milling. Default: true.
        #[serde(default)]
        pub climb: Option<bool>,
        /// The stock's extent seen from above (`rect`, and a `polygon` it must also be inside).
        /// Default: the part's extent.
        #[serde(default)]
        pub boundary: Option<BoundarySpec>,
        /// Material left on the part, mm. Default: 0.25.
        #[serde(default)]
        pub stock_to_leave: Option<f64>,
        /// Add a level at every flat area of the part between the regular levels. Default: true.
        #[serde(default)]
        pub flats: Option<bool>,
        /// Step-up levels at most this far apart, mm: after each level the roughing steps back up
        /// toward the one above, cutting only where that level could not reach (shallow ground),
        /// so the terraces left there are at most this high. Default: none.
        #[serde(default)]
        pub step_up: Option<f64>,
        /// Chord error, mm. Default: the job's tolerance.
        #[serde(default)]
        pub tolerance: Option<f64>,
    }
}

/// What an adaptive operation adds to its report: the engagement measured along the path
/// written (against the material itself, not the planner's model), and the plain pocket it is
/// compared with.
#[derive(Clone, Debug, Default, Serialize, JsonSchema)]
pub struct AdaptiveReport {
    /// Whether the path's engagement was measured (a path too long to measure is not; the
    /// engagement figures are then 0 and say nothing).
    pub measured: bool,
    /// Largest engagement outside the entries, degrees.
    pub max_engagement_deg: f64,
    /// Largest engagement in the entry helices, degrees.
    pub max_entry_engagement_deg: f64,
    /// Mean engagement over the time the tool cuts, degrees.
    pub mean_engagement_deg: f64,
    /// Share of the cutting time outside entries at a full slot (170° or more): 0.
    pub full_slot_share: f64,
    /// Share of all the cutting time spent at a full slot in the entries.
    pub entry_slot_share: f64,
    /// The engagement limit asked for, degrees.
    pub limit_deg: f64,
    /// Levels cut.
    pub levels: usize,
    /// Entry helices.
    pub helices: usize,
    /// Times the tool lifts between cuts at a level.
    pub lifts: usize,
    /// Area of material within reach left uncut (narrow inside corners), mm², over the levels.
    pub left_area: f64,
    /// Area seen from above the tool cannot reach at all (corners tighter than its radius,
    /// places narrower than it), left for a smaller tool, mm² (2D only).
    pub corner_area: f64,
    /// The plain offset pocket of the same profiles at the operation's tool, stepover 40% of the
    /// diameter (2D only, with `compare`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pocket: Option<ComparedPath>,
}

/// Another strategy's path, measured the same way.
#[derive(Clone, Debug, Default, Serialize, JsonSchema)]
pub struct ComparedPath {
    pub max_engagement_deg: f64,
    pub full_slot_share: f64,
    /// Distance at feed, mm.
    pub cut_length: f64,
    /// Time at feed, minutes.
    pub cut_minutes: f64,
}
