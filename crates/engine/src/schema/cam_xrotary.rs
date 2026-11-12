//! Contracts of the 4th-axis operations about +X with files of their own: pockets and contours
//! wrapped onto a cylinder, and one 3-axis operation repeated at several A angles. Part of
//! `schema/cam.rs` (its `operation!` macro and shared types), kept in a file of its own.

use schemars::JsonSchema;
use serde::Deserialize;

use super::{Common, FeedsSpec, Op3Spec, SideSpec};
use crate::schema::SketchRef;

operation! {
    /// A 2D pocket wrapped onto a cylinder round the A axis: the profile is drawn unrolled (x
    /// along the axis, y the arc length round the cylinder of `radius`) and cleared in depth
    /// levels with the tool pointing at the axis, each level offset at its own radius.
    WrapPocketOp {
        /// The radius of the cylinder the profile is drawn on, mm.
        pub radius: f64,
        /// A sketch whose closed profiles are the pocket, its x along the axis and y the arc
        /// length round the cylinder (on a plane facing +Z). Give this or `polygon`.
        #[serde(default)]
        pub sketch: Option<SketchRef>,
        /// With `sketch`: which of its profiles, by index. Default: every drawn profile.
        #[serde(default)]
        pub profiles: Option<Vec<usize>>,
        /// One closed polygon [[x, arc length], ...] in unrolled coordinates.
        #[serde(default)]
        pub polygon: Option<Vec<[f64; 2]>>,
        /// The floor's depth from the cylinder, mm (negative into it).
        pub bottom: f64,
        /// The top of the material's depth from the cylinder, mm. Default: 0.
        #[serde(default)]
        pub top: Option<f64>,
        /// Most depth per level, mm. Default: the tool's `stepdown`, else half its diameter.
        #[serde(default)]
        pub stepdown: Option<f64>,
        /// Distance between passes at the cutting radius, mm, at most the tool diameter.
        /// Default: the tool's `stepover`, else 40% of its diameter.
        #[serde(default)]
        pub stepover: Option<f64>,
        /// How far above the cylinder rapids go round, mm. Default: 5.
        #[serde(default)]
        pub clearance: Option<f64>,
        /// How far above the cylinder feed begins, mm. Default: 1.
        #[serde(default)]
        pub retract: Option<f64>,
        /// Climb milling. Default: true.
        #[serde(default)]
        pub climb: Option<bool>,
        /// Material left on the walls, mm. Default: 0.
        #[serde(default)]
        pub stock_to_leave: Option<f64>,
        /// Steepest ramp into uncut material, degrees; 0 plunges. Default: 0.
        #[serde(default)]
        pub ramp_angle_deg: Option<f64>,
        /// Largest distance from the true wrapped profile, mm. Default: the job's tolerance.
        #[serde(default)]
        pub tolerance: Option<f64>,
        /// Largest turn of the tool axis in one move, degrees. Default: 2.
        #[serde(default)]
        pub max_angle_step_deg: Option<f64>,
    }
}

operation! {
    /// A 2D contour wrapped onto a cylinder round the A axis: the profile drawn unrolled, as
    /// for `wrap_pocket`, followed on the chosen side in depth levels, the tool radial.
    WrapContourOp {
        /// The radius of the cylinder the profile is drawn on, mm.
        pub radius: f64,
        /// A sketch whose closed profiles are followed, its x along the axis and y the arc
        /// length round the cylinder (on a plane facing +Z). Give this or `polygon`.
        #[serde(default)]
        pub sketch: Option<SketchRef>,
        /// With `sketch`: which of its profiles, by index. Default: every drawn profile.
        #[serde(default)]
        pub profiles: Option<Vec<usize>>,
        /// One closed polygon [[x, arc length], ...] in unrolled coordinates.
        #[serde(default)]
        pub polygon: Option<Vec<[f64; 2]>>,
        /// The last level's depth from the cylinder, mm (negative into it).
        pub bottom: f64,
        /// The top of the material's depth from the cylinder, mm. Default: 0.
        #[serde(default)]
        pub top: Option<f64>,
        /// Most depth per level, mm. Default: the tool's `stepdown`, else half its diameter.
        #[serde(default)]
        pub stepdown: Option<f64>,
        /// Which side the tool runs on. Default: outside.
        #[serde(default)]
        pub side: Option<SideSpec>,
        /// How far above the cylinder rapids go round, mm. Default: 5.
        #[serde(default)]
        pub clearance: Option<f64>,
        /// How far above the cylinder feed begins, mm. Default: 1.
        #[serde(default)]
        pub retract: Option<f64>,
        /// Climb milling. Default: true.
        #[serde(default)]
        pub climb: Option<bool>,
        /// Material left on the wall, mm. Default: 0.
        #[serde(default)]
        pub stock_to_leave: Option<f64>,
        /// Steepest ramp into each level along the profile, degrees; 0 plunges. Default: 0.
        #[serde(default)]
        pub ramp_angle_deg: Option<f64>,
        /// Largest distance from the true wrapped profile, mm. Default: the job's tolerance.
        #[serde(default)]
        pub tolerance: Option<f64>,
        /// Largest turn of the tool axis in one move, degrees. Default: 2.
        #[serde(default)]
        pub max_angle_step_deg: Option<f64>,
    }
}

/// One 3-axis operation run at several A angles (3+1), one indexed operation each, in the order
/// given: `a_deg` lists the angles, or `count` spreads that many evenly over a turn from
/// `start_deg`. Each is named `<name> A<angle>`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IndexedPatternOp {
    /// The operations' name, for an inner operation without one of its own (each angle's is
    /// it and ` A<angle>`).
    #[serde(default)]
    pub name: Option<String>,
    /// Plan and write this operation (`false`: kept in the job, turned off). Default: true.
    #[serde(default)]
    pub enabled: Option<bool>,
    /// The A angles, degrees (1 to 200 of them). Give this or `count`.
    #[serde(default)]
    pub a_deg: Option<Vec<f64>>,
    /// How many angles, evenly over a full turn (1 to 200). Give this or `a_deg`.
    #[serde(default)]
    pub count: Option<u32>,
    /// With `count`: the first angle, degrees. Default: 0.
    #[serde(default)]
    pub start_deg: Option<f64>,
    /// The 3-axis operation (`adaptive`, `adaptive_roughing`, `contour`, `pocket`, `parallel`,
    /// `roughing`, `waterline`, `scallop`, `steep_shallow`, `pencil`, `rest_finish`, `drill` or
    /// `chamfer`), its heights, polygon and boundary in the frame each angle brings under the
    /// tool.
    pub operation: Box<Op3Spec>,
}
