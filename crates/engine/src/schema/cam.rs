//! Contracts and result types for the commands in `cmd/cam.rs`: `cam.job`, `cam.post.list`,
//! `cam.post.get` and `cam.gcode.check`. The job is the CAM setup of a design (machine, post,
//! tools, operations), kept in a JSON file next to the design rather than in it;
//! docs/cam-jobs.md describes it in full.
//!
//! The handler reads its parameters through these same types, so what the contract accepts is
//! what the command accepts. Every object of the job refuses keys it does not know, so a
//! misspelt key is an error rather than a setting silently left at its default. The objects the
//! CAM crate defines (tools, a machine, a post) are taken as JSON here and read by the handler,
//! which refuses their unknown keys the same way.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::cam_adaptive::{AdaptiveOp, AdaptiveReport, AdaptiveRoughingOp};
use super::cam_rates::{MachineRatesSpec, MachineTimeReport};
use super::cam_stock::{FaceOp, RapidsReport, StockReport, StockSpec, TravelReport};
use super::types::SketchRef;

#[path = "cam_leads.rs"]
mod leads;
pub use leads::{ENTRY_DEG, EntrySpec, LeadSpec, LinkingSpec, MAX_ANGLE_DEG, MAX_TABS, MIN_ARC_RADIUS, TabsSpec};

/// Plan and check a CAM job and write its G-code: toolpaths for every operation, the program
/// written through the job's post for its machine, then read back and compared with the
/// toolpaths. Refuses, writing nothing, when the check fails, unless `allow_unsafe`. The job is
/// described in docs/cam-jobs.md.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CamJobParams {
    /// The job itself (see docs/cam-jobs.md). Give this or `job_path`.
    #[serde(default)]
    pub job: Option<CamJob>,
    /// A `.json` file holding the job. Relative `post_path` and `tool_library_path` in it are
    /// read from the file's folder.
    #[serde(default)]
    pub job_path: Option<String>,
    /// Where to write the G-code. Required unless `dry_run`.
    #[serde(default)]
    pub output: Option<String>,
    /// Where to write a self-contained HTML page showing the part and every toolpath (no
    /// network access). Default: no page.
    #[serde(default)]
    pub preview: Option<String>,
    /// Where to write the setup sheet for the machine operator: a `.html` page (self-contained,
    /// print styles) or a `.md` file with the work zero, the rotary convention, the tools to
    /// load, the operations in order, the checks and the warnings. Default: no sheet.
    #[serde(default)]
    pub setup_sheet: Option<String>,
    /// Plan and check the job, and report, but write no file; the report says whether the
    /// checks pass instead of refusing. Default: false.
    #[serde(default)]
    pub dry_run: Option<bool>,
    /// Write the program even when the read-back check fails (the report says so, and so does
    /// the program's first line). Default: false.
    #[serde(default)]
    pub allow_unsafe: Option<bool>,
    /// Write one program per tool instead of one program: each run of operations with the same
    /// tool is a program of its own, complete on its own (the safe start, the same work offset,
    /// the tool's length offset on its first Z, a header naming the tool, its stickout and its
    /// place in the order), named after `output` with its place and tool (`part-2-T6.ngc`). For
    /// a machine that does not stop at a tool change (`tool_change: ignored`). Default: false.
    #[serde(default)]
    pub split_by_tool: Option<bool>,
    /// Also simulate the written program removing material from stock taken from the part
    /// (`cam.simulate`): a gouge or a rapid through material fails the job like the other
    /// checks; material left on the part is a warning. Default: false.
    #[serde(default)]
    pub simulate: Option<bool>,
    /// With `simulate`: the simulation's cell size, mm (0.0001 to 10000). Default: five times
    /// the tolerance, coarser where the stock or the program is too large for that.
    #[serde(default)]
    pub simulate_resolution: Option<f64>,
}

/// What the machine does at a tool change (`T.. M6`): a job's `tool_change`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ToolChangeMode {
    /// An automatic tool changer: a program may use several tools.
    Auto,
    /// The machine stops the spindle and waits (the operator changes the tool, sets its length
    /// and presses Cycle Start): a program may use several tools.
    Stop,
    /// The controller ignores the tool change and runs on with the tool in the spindle: a
    /// program should use one tool (`split_by_tool`).
    Ignored,
}

/// A CAM job: what is machined, on which machine, through which post, with which tools.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CamJob {
    /// The program's name, written in its header. Default: the design's name.
    #[serde(default)]
    pub name: Option<String>,
    /// `3axis`, `4axis_a` (a rotary table about +X, the A axis, through work zero), or a whole
    /// machine as the CAM crate's `Machine` JSON. Give this or `machine_path`.
    #[serde(default)]
    pub machine: Option<MachineSpec>,
    /// The machine as the CAM crate's `Machine` JSON in a file, read from the job file's folder
    /// when relative. Give this or `machine`.
    #[serde(default)]
    pub machine_path: Option<String>,
    /// With `4axis_a`: the controller's positive A turns the table the other way. Default: false.
    #[serde(default)]
    pub a_reversed: Option<bool>,
    /// With `4axis_a`: the lowest and highest A the table reaches, in degrees. Default: it turns
    /// without end.
    #[serde(default)]
    pub a_limits_deg: Option<[f64; 2]>,
    /// A built-in post by name (`cam.post.list` lists them). Give this or `post_path`.
    #[serde(default)]
    pub post: Option<String>,
    /// A post as a JSON file (the CAM crate's `Post5`, as `cam.post.get` returns one).
    #[serde(default)]
    pub post_path: Option<String>,
    /// The work offset the program runs in, 1 to 6 (G54 to G59), for posts that write one.
    /// Default: 1.
    #[serde(default)]
    pub work_offset: Option<u8>,
    /// The design point that is work zero, [x, y, z] in mm. On a 4th axis the rotary axis is
    /// work X through work zero. Default: the design's origin.
    #[serde(default)]
    pub origin: Option<[f64; 3]>,
    /// How the part is turned for this setup, about `origin` (which is then work zero): a job
    /// that machines the back of a part (op2, after a flip) uses the same design as the job
    /// that machined its front. `"flip_x"` (half a turn about X: the bottom faces up, Y runs
    /// the other way), `"flip_y"` (half a turn about Y), `{"axis": [x, y, z], "angle_deg": a}`
    /// (a right-handed turn about that axis), or `{"sketch": name}` (work X, Y and Z along the
    /// sketch plane's X, Y and normal: the tool looks down on that plane). The operations'
    /// coordinates (polygons, heights, boundaries) and the `stock` are in the turned work
    /// coordinates. A 3-axis machine only: a 4th axis turns the part with its own angles.
    /// Default: none.
    #[serde(default)]
    pub orientation: Option<OrientationSpec>,
    /// The part's bodies by name: the surface 3D operations follow and every operation is
    /// checked for gouges against, drawn in the preview. Default: every visible body not in
    /// `fixtures`.
    #[serde(default)]
    pub bodies: Option<Vec<String>>,
    /// Bodies the tool must stay clear of (vises, clamps): checked for gouges and holder
    /// collisions, never machined. Default: none.
    #[serde(default)]
    pub fixtures: Option<Vec<String>>,
    /// The machining tolerance in mm, 0.0001 to 1: how far a toolpath may be from the profiles
    /// and surfaces it follows. 3D operations plan on the bodies tessellated within a quarter of
    /// it. Default: 0.01.
    #[serde(default)]
    pub tolerance: Option<f64>,
    /// The tools, as the CAM crate's `Tool` JSON (`number`, `kind`, `diameter`, `flute_length`,
    /// ...). Give this or `tool_library_path`.
    #[serde(default)]
    pub tools: Option<Vec<Map<String, Value>>>,
    /// A JSON file holding an array of tools, as `tools`.
    #[serde(default)]
    pub tool_library_path: Option<String>,
    /// The raw material, in work coordinates: `{box: {min, max}}`, `{cylinder: {radius, x_min,
    /// x_max}}` (a bar about work X) or `{around_part: {margin: [x, y, z], top?, bottom?}}`. No
    /// rapid may pass through it below its top, and `face` faces it. Default: none (rapids are
    /// checked against the part and fixtures only).
    #[serde(default)]
    pub stock: Option<StockSpec>,
    /// How far below the stock's bottom a cut through the stock (a cut-out, a through drill or
    /// bore) may go, mm, 0 to 25: the room a spoil board, or parallels clear of the cut, leave
    /// under it. With a workholding the cut is still checked against the parallels and the
    /// table. Default: 0 (no cut below the stock's bottom).
    #[serde(default)]
    pub breakthrough: Option<f64>,
    /// A machine model file (`solidcraft.machine_model`: the table, the spindle's nose and the
    /// machine's parts), read from the job file's folder when relative: where the workholding
    /// stands and how high the spindle goes. Default: none.
    #[serde(default)]
    pub machine_model_path: Option<String>,
    /// A workholding file (`solidcraft.workholding`: a vise, clamps, the table or a rotary unit),
    /// read from the job file's folder when relative. With a machine model, `cam.job` checks every
    /// move of the program against both: the cutter, shank, holder or a machine part running into
    /// it, and the spindle nose past the top of its travel, fail the job. Default: none.
    #[serde(default)]
    pub workholding_path: Option<String>,
    /// The workholding written into the job (the same object as a workholding file), instead of
    /// `workholding_path`. Default: none.
    #[serde(default)]
    pub workholding: Option<Map<String, Value>>,
    /// The operations, run and written in this order.
    pub operations: Vec<OperationSpec>,
    /// The machine's top rates (from its controller's settings): the reply then times the
    /// program as the machine runs it and warns about blocks the controller would slow. Give
    /// this or `machine_rates_path`. Default: none.
    #[serde(default)]
    pub machine_rates: Option<MachineRatesSpec>,
    /// A JSON file holding `machine_rates`, read from the job file's folder when relative.
    #[serde(default)]
    pub machine_rates_path: Option<String>,
    /// What the machine does at a tool change: `auto`, `stop` or `ignored`. With `ignored`, the
    /// post stops (`M0`) at each tool change after the first, and a program of several tools is
    /// warned about unless it is written one program per tool (`split_by_tool`). Default: not
    /// said (no warning).
    #[serde(default)]
    pub tool_change: Option<ToolChangeMode>,
}

/// How a job turns the part (`orientation`).
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum OrientationSpec {
    /// `"flip_x"` or `"flip_y"`.
    Named(String),
    /// A turn about an axis through `origin`.
    Turn { axis: [f64; 3], angle_deg: f64 },
    /// Work axes along a sketch's plane.
    Sketch { sketch: String },
}

/// A machine: a preset's name or the CAM crate's `Machine` JSON.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum MachineSpec {
    /// `3axis` or `4axis_a`.
    Preset(String),
    /// The CAM crate's `Machine` JSON: `name`, `rotaries`, `linear_limits?`, `work_origin?`.
    Custom(Map<String, Value>),
}

/// The Z levels of an operation, in work coordinates, mm.
#[derive(Clone, Copy, Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HeightsSpec {
    /// Where the tool travels between cuts.
    pub clearance: f64,
    /// Where rapid motion towards the material stops and feed begins.
    pub retract: f64,
    /// Top of the material to cut.
    pub top: f64,
    /// Final depth.
    pub bottom: f64,
}

/// Feed rates in mm/min.
#[derive(Clone, Copy, Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FeedsSpec {
    /// The cutting feed.
    pub cut: f64,
    /// The feed going down into the material: entries from above, ramps, plunges.
    pub plunge: f64,
    /// A 3D finishing pass's feed for moves inside it that go down the surface at least as
    /// steeply as its descent limit (a ball running down a roughed wall). Default: the cut
    /// feed; give the plunge feed to slow them (finishing straight from uncut stock).
    #[serde(default)]
    pub descent: Option<f64>,
}

/// Where a 3D operation's cutter centre may go, seen along the tool axis, in work coordinates.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BoundarySpec {
    /// The low and high corners [[x, y], [x, y]] of a rectangle. Default: the part's extent.
    #[serde(default)]
    pub rect: Option<[[f64; 2]; 2]>,
    /// A closed polygon [[x, y], ...] the centre must also stay inside.
    #[serde(default)]
    pub polygon: Option<Vec<[f64; 2]>>,
    /// With `polygon`: closed polygons inside it the centre stays out of (a flat top a ball
    /// finish leaves to a facing). Default: none.
    #[serde(default)]
    pub holes: Option<Vec<Vec<[f64; 2]>>>,
}

/// Which side of a profile a contour runs on.
#[derive(Clone, Copy, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SideSpec {
    /// Around the profile: the profile is the part that stays.
    Outside,
    /// Inside the profile: the profile is the opening.
    Inside,
    /// The tool centre on the profile.
    On,
}

/// How the passes of a rotary operation run.
#[derive(Clone, Copy, Debug, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum PatternSpec {
    /// Passes along X, at most `angle_step_deg` apart round the axis.
    Lines {
        /// Degrees between passes round the axis.
        angle_step_deg: f64,
        /// Every other pass runs back. Default: true.
        #[serde(default)]
        zigzag: Option<bool>,
    },
    /// One pass going round and round, `pitch` mm along +X per turn.
    Helix {
        /// Advance along X per turn, mm.
        pitch: f64,
    },
    /// Circumferential passes: a full turn of A at each X, at most `stepover` mm apart along X.
    Rings {
        /// Most distance between rings along X, mm.
        stepover: f64,
        /// Every other ring turns back, so A stays within a turn. Default: true.
        #[serde(default)]
        zigzag: Option<bool>,
    },
}

/// Declares an operation's struct with the fields every operation has, then its own.
macro_rules! operation {
    ($(#[$doc:meta])* $name:ident { $($(#[$fdoc:meta])* pub $field:ident : $ty:ty,)* }) => {
        $(#[$doc])*
        #[derive(Clone, Debug, Deserialize, JsonSchema)]
        #[serde(deny_unknown_fields)]
        pub struct $name {
            /// The operation's name, written as a comment in the program. Required, except
            /// inside `indexed` or `indexed_pattern`, which may carry it instead.
            #[serde(default)]
            pub name: String,
            /// Plan and write this operation. `false` keeps it in the job, turned off: it is not
            /// planned or written. Default: true.
            #[serde(default)]
            pub enabled: Option<bool>,
            /// The `number` of the tool it cuts with.
            pub tool: u32,
            /// Spindle speed, rev/min. Default: the tool's `spindle_rpm`.
            #[serde(default)]
            pub spindle_rpm: Option<f64>,
            /// Feed rates. Default: the tool's `feed` and `plunge_feed`.
            #[serde(default)]
            pub feeds: Option<FeedsSpec>,
            /// Coolant on while it cuts. Default: false.
            #[serde(default)]
            pub coolant: Option<bool>,
            /// Check this operation for gouges into the part. Turn it off only for a cut the
            /// model does not show (an engraving); the fixtures, holder and read-back checks
            /// still run. Default: true.
            #[serde(default)]
            pub gouge_check: Option<bool>,
            $($(#[$fdoc])* pub $field: $ty,)*
        }

        impl $name {
            pub fn common(&self) -> Common<'_> {
                Common {
                    name: &self.name,
                    tool: self.tool,
                    spindle_rpm: self.spindle_rpm,
                    feeds: self.feeds,
                    coolant: self.coolant.unwrap_or(false),
                    enabled: self.enabled.unwrap_or(true),
                    gouge_check: self.gouge_check.unwrap_or(true),
                }
            }
        }
    };
}

pub(crate) use operation;

/// What every operation has.
#[derive(Clone, Copy, Debug)]
pub struct Common<'a> {
    pub name: &'a str,
    pub tool: u32,
    pub spindle_rpm: Option<f64>,
    pub feeds: Option<FeedsSpec>,
    pub coolant: bool,
    /// Whether it is planned and written (`enabled`, true by default).
    pub enabled: bool,
    /// Whether it is checked for gouges into the part (`gouge_check`, true by default).
    pub gouge_check: bool,
}

operation! {
    /// A 2D contour round closed profiles, in depth passes.
    ContourOp {
        /// The sketch whose closed profiles are cut round (on a plane square to the tool axis,
        /// facing it). Give this or `polygon`.
        #[serde(default)]
        pub sketch: Option<SketchRef>,
        /// With `sketch`: which of its profiles, by index. Default: every drawn profile.
        #[serde(default)]
        pub profiles: Option<Vec<usize>>,
        /// One closed polygon [[x, y], ...] in work XY.
        #[serde(default)]
        pub polygon: Option<Vec<[f64; 2]>>,
        /// The Z levels.
        pub heights: HeightsSpec,
        /// Most depth per pass, mm. Default: the tool's `stepdown`, else half its diameter.
        #[serde(default)]
        pub stepdown: Option<f64>,
        /// Which side the tool runs on. Default: outside.
        #[serde(default)]
        pub side: Option<SideSpec>,
        /// Climb milling. Default: true.
        #[serde(default)]
        pub climb: Option<bool>,
        /// Material left on the wall, mm. Default: 0.
        #[serde(default)]
        pub stock_to_leave: Option<f64>,
        /// Steepest ramp into each pass along the profile, degrees; 0 plunges. Default: 0.
        #[serde(default)]
        pub ramp_angle_deg: Option<f64>,
        /// Chord error on curved profiles, mm. Default: the job's tolerance.
        #[serde(default)]
        pub tolerance: Option<f64>,
        /// A tangent arc into and out of each pass. Default: none.
        #[serde(default)]
        pub lead: Option<LeadSpec>,
        /// The radius of a vertical lead, mm: each pass is entered down a quarter arc in the
        /// vertical plane of its start's direction from this far above and behind its start (its
        /// `lead`'s, with one), and left up one beyond its end (`G18`/`G19` arcs where that
        /// direction runs along X or Y). It takes the place of `entry` (none, or a plunge).
        /// Default: none.
        #[serde(default)]
        pub vertical_lead: Option<f64>,
        /// How the tool goes down into each pass (instead of `ramp_angle_deg`). Default: the ramp
        /// angle's (0: plunge).
        #[serde(default)]
        pub entry: Option<EntrySpec>,
        /// Where the tool goes between passes it cannot link in the cut. Default: `clearance`.
        #[serde(default)]
        pub linking: Option<LinkingSpec>,
        /// Holding tabs (outside contours only). Default: none.
        #[serde(default)]
        pub tabs: Option<TabsSpec>,
        /// Passes round the final wall at full depth after the depth passes, which leave
        /// `finish_passes` × `finish_stepover` for them. Default: 0.
        #[serde(default)]
        pub finish_passes: Option<u32>,
        /// Distance between finishing passes, mm, at most half the tool's radius. Default: 5% of
        /// the tool's diameter.
        #[serde(default)]
        pub finish_stepover: Option<f64>,
        /// Go round the last pass once more. Default: false.
        #[serde(default)]
        pub spring_pass: Option<bool>,
    }
}

operation! {
    /// A 2D pocket: clears the inside of closed profiles, in depth passes.
    PocketOp {
        /// The sketch whose closed profiles are pocketed (on a plane square to the tool axis,
        /// facing it). Give this or `polygon`.
        #[serde(default)]
        pub sketch: Option<SketchRef>,
        /// With `sketch`: which of its profiles, by index. Default: every drawn profile.
        #[serde(default)]
        pub profiles: Option<Vec<usize>>,
        /// One closed polygon [[x, y], ...] in work XY.
        #[serde(default)]
        pub polygon: Option<Vec<[f64; 2]>>,
        /// The Z levels.
        pub heights: HeightsSpec,
        /// Most depth per pass, mm. Default: the tool's `stepdown`, else half its diameter.
        #[serde(default)]
        pub stepdown: Option<f64>,
        /// Distance between passes, mm, at most the tool diameter. Default: the tool's
        /// `stepover`, else 40% of its diameter.
        #[serde(default)]
        pub stepover: Option<f64>,
        /// Climb milling. Default: true.
        #[serde(default)]
        pub climb: Option<bool>,
        /// Material left on the walls, mm. Default: 0.
        #[serde(default)]
        pub stock_to_leave: Option<f64>,
        /// Steepest ramp into uncut material, degrees; 0 plunges. Default: 0.
        #[serde(default)]
        pub ramp_angle_deg: Option<f64>,
        /// Chord error on curved profiles, mm. Default: the job's tolerance.
        #[serde(default)]
        pub tolerance: Option<f64>,
        /// How the tool goes down into uncut material (instead of `ramp_angle_deg`): a ramp
        /// round the ring, or a helix. Default: the ramp angle's (0: plunge).
        #[serde(default)]
        pub entry: Option<EntrySpec>,
        /// Where the tool goes between rings it cannot link in the cut. Default: `clearance`.
        #[serde(default)]
        pub linking: Option<LinkingSpec>,
    }
}

#[path = "cam_helical.rs"]
mod helical;
pub use helical::{BoreOp, HoleSpec, ThreadMillOp};
#[path = "cam_chamfer.rs"]
mod chamfer;
pub use chamfer::{ChamferHeightsSpec, ChamferOp};
#[path = "cam_drill.rs"]
mod drill;
pub use drill::{DrillCycleSpec, DrillOp};

operation! {
    /// 3D raster finishing: passes side by side following the part's surface.
    ParallelOp {
        /// The Z levels: `bottom` is the floor, `top` is not used.
        pub heights: HeightsSpec,
        /// Distance between passes, mm. Default: the tool's `stepover`, else 10% of its diameter.
        #[serde(default)]
        pub stepover: Option<f64>,
        /// Direction of the passes, degrees counter-clockwise from +X. Default: 0.
        #[serde(default)]
        pub angle_deg: Option<f64>,
        /// Where the cutter centre may go. Default: the part's extent.
        #[serde(default)]
        pub boundary: Option<BoundarySpec>,
        /// Material left on the surface, mm. Default: 0.
        #[serde(default)]
        pub stock_to_leave: Option<f64>,
        /// Distance between samples along a pass before refinement, mm. Default: a quarter of
        /// the tool's diameter.
        #[serde(default)]
        pub step: Option<f64>,
        /// Largest distance from the surface, mm. Default: the job's tolerance.
        #[serde(default)]
        pub tolerance: Option<f64>,
        /// Every other pass runs back. Default: true.
        #[serde(default)]
        pub zigzag: Option<bool>,
        /// Moves inside a pass that go down at least this steeply, degrees from horizontal (0 to
        /// 90), run at the descent feed (`feeds.descent`); shallower ones at the cut feed.
        /// Default: 70.
        #[serde(default)]
        pub descent_limit_deg: Option<f64>,
    }
}

operation! {
    /// 3D roughing of a stock box in levels, staying `stock_to_leave` clear of the part.
    RoughingOp {
        /// The Z levels: `top` is the stock's top, `bottom` the lowest level.
        pub heights: HeightsSpec,
        /// Most material per level, mm. Default: the tool's `stepdown`, else half its diameter.
        #[serde(default)]
        pub stepdown: Option<f64>,
        /// Distance between passes, mm. Default: the tool's `stepover`, else 40% of its diameter.
        #[serde(default)]
        pub stepover: Option<f64>,
        /// Direction of the passes, degrees counter-clockwise from +X. Default: 0.
        #[serde(default)]
        pub angle_deg: Option<f64>,
        /// The stock's extent as the cutter centre may reach it. Default: the part's extent.
        #[serde(default)]
        pub boundary: Option<BoundarySpec>,
        /// Material left on the part, mm. Default: 0.25.
        #[serde(default)]
        pub stock_to_leave: Option<f64>,
        /// Distance between samples along a pass before refinement, mm. Default: a quarter of
        /// the tool's diameter.
        #[serde(default)]
        pub step: Option<f64>,
        /// Largest distance from the surface, mm. Default: the job's tolerance.
        #[serde(default)]
        pub tolerance: Option<f64>,
        /// Every other pass runs back. Default: true.
        #[serde(default)]
        pub zigzag: Option<bool>,
    }
}

operation! {
    /// 3D z-level finishing: round the part at each level, for steep walls.
    WaterlineOp {
        /// The Z levels: from `top` down to `bottom`, both included.
        pub heights: HeightsSpec,
        /// Most distance between levels, mm. Default: the tool's `stepdown`, else a quarter of
        /// its diameter.
        #[serde(default)]
        pub stepdown: Option<f64>,
        /// Where the cutter centre may go. Default: the part's extent and a little round it.
        #[serde(default)]
        pub boundary: Option<BoundarySpec>,
        /// Material left on the walls, mm. Default: 0.
        #[serde(default)]
        pub stock_to_leave: Option<f64>,
        /// Spacing of the grid the levels are traced on, mm. Default: a quarter of the tool's
        /// diameter.
        #[serde(default)]
        pub step: Option<f64>,
        /// How close to each level the loops are found, mm. Default: the job's tolerance.
        #[serde(default)]
        pub tolerance: Option<f64>,
        /// Climb milling. Default: true.
        #[serde(default)]
        pub climb: Option<bool>,
    }
}

operation! {
    /// 4th-axis finishing: the part turns on the A axis while the tool, radial, follows it.
    RotaryFinishOp {
        /// Lines along X, one helix, or rings round the axis.
        pub pattern: PatternSpec,
        /// Radius from the rotary axis rapids go round at, mm.
        pub clearance: f64,
        /// Radius the tool feeds in from, mm; must clear the part.
        pub retract: f64,
        /// Smallest radius the tip may reach, mm. Default: 0.
        #[serde(default)]
        pub floor: Option<f64>,
        /// From where to where along X, [min, max] mm. Default: the part's extent.
        #[serde(default)]
        pub range: Option<[f64; 2]>,
        /// Distance between samples along a pass before refinement, mm. Default: a quarter of
        /// the tool's diameter.
        #[serde(default)]
        pub step: Option<f64>,
        /// Largest distance from the surface, mm. Default: the job's tolerance.
        #[serde(default)]
        pub tolerance: Option<f64>,
        /// Material left on the surface, mm. Default: 0.
        #[serde(default)]
        pub stock_to_leave: Option<f64>,
        /// Largest turn of the tool axis in one cutting move, degrees. Default: 2.
        #[serde(default)]
        pub max_angle_step_deg: Option<f64>,
    }
}

operation! {
    /// 4th-axis roughing of round stock down to the part, in levels of radius.
    RotaryRoughingOp {
        /// Lines along X, one helix, or rings round the axis.
        pub pattern: PatternSpec,
        /// The round stock's radius, mm. Default: the job's `stock` (a cylinder's radius, or how
        /// far a box's farthest edge is from the A axis).
        #[serde(default)]
        pub stock_radius: Option<f64>,
        /// Radius from the rotary axis rapids go round at, mm.
        pub clearance: f64,
        /// Radius the tool feeds in from, mm; at or outside the stock.
        pub retract: f64,
        /// The last level's radius, mm. Default: 0.
        #[serde(default)]
        pub floor: Option<f64>,
        /// Most material per level, in radius, mm. Default: the tool's `stepdown`, else half its
        /// diameter.
        #[serde(default)]
        pub stepdown: Option<f64>,
        /// From where to where along X, [min, max] mm. Default: the part's extent.
        #[serde(default)]
        pub range: Option<[f64; 2]>,
        /// Distance between samples along a pass before refinement, mm. Default: a quarter of
        /// the tool's diameter.
        #[serde(default)]
        pub step: Option<f64>,
        /// Largest distance from the surface, mm. Default: the job's tolerance.
        #[serde(default)]
        pub tolerance: Option<f64>,
        /// Material left on the part, mm. Default: 0.25.
        #[serde(default)]
        pub stock_to_leave: Option<f64>,
        /// Largest turn of the tool axis in one cutting move, degrees. Default: 2.
        #[serde(default)]
        pub max_angle_step_deg: Option<f64>,
    }
}

operation! {
    /// A flat path wrapped round the rotary axis (engraving, slots round a cylinder).
    WrapOp {
        /// The radius of the cylinder the path is drawn on, mm.
        pub radius: f64,
        /// The flat path [[x, y, z], ...]: x along the axis, y the arc length round it at
        /// `radius` (mm), z the depth from the cylinder (0 on it, negative into it). Give this
        /// or `sketch` and `curve`.
        #[serde(default)]
        pub points: Option<Vec<[f64; 3]>>,
        /// A sketch whose curve `curve` is the path, its sketch x along the axis and y the arc
        /// length round it.
        #[serde(default)]
        pub sketch: Option<SketchRef>,
        /// With `sketch`: the curve's id.
        #[serde(default)]
        pub curve: Option<String>,
        /// With `sketch`: the depth of the whole path from the cylinder, mm (negative into it).
        /// Default: 0.
        #[serde(default)]
        pub depth: Option<f64>,
        /// How far above the cylinder the tool travels between cuts, mm. Default: 5.
        #[serde(default)]
        pub clearance: Option<f64>,
        /// How far above the cylinder feed begins, mm. Default: 1.
        #[serde(default)]
        pub retract: Option<f64>,
        /// Largest turn of the tool axis in one move, degrees. Default: 2.
        #[serde(default)]
        pub max_angle_step_deg: Option<f64>,
    }
}

// The 3D finishing strategies, declared with the macro above.
#[path = "cam_finish3d.rs"]
mod finish3d;
pub use finish3d::{DirectionSpec, PencilOp, RestFinishOp, ScallopOp, ShallowSpec, SteepShallowOp};
// Wrapped pockets and contours, and indexed patterns: their own file, inside this module for
// the `operation!` macro.
#[path = "cam_xrotary.rs"]
mod xrotary;
pub use xrotary::{IndexedPatternOp, WrapContourOp, WrapPocketOp};

/// A 3-axis operation, as an indexed operation runs it.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Op3Spec {
    Adaptive(AdaptiveOp),
    AdaptiveRoughing(AdaptiveRoughingOp),
    Contour(ContourOp),
    Pocket(PocketOp),
    Parallel(ParallelOp),
    Roughing(RoughingOp),
    Waterline(WaterlineOp),
    Scallop(ScallopOp),
    SteepShallow(SteepShallowOp),
    Pencil(PencilOp),
    RestFinish(RestFinishOp),
    Drill(DrillOp),
    Chamfer(ChamferOp),
}

impl Op3Spec {
    /// What every operation has.
    pub fn common(&self) -> Common<'_> {
        match self {
            Op3Spec::Adaptive(o) => o.common(),
            Op3Spec::AdaptiveRoughing(o) => o.common(),
            Op3Spec::Contour(o) => o.common(),
            Op3Spec::Pocket(o) => o.common(),
            Op3Spec::Parallel(o) => o.common(),
            Op3Spec::Roughing(o) => o.common(),
            Op3Spec::Waterline(o) => o.common(),
            Op3Spec::Scallop(o) => o.common(),
            Op3Spec::SteepShallow(o) => o.common(),
            Op3Spec::Pencil(o) => o.common(),
            Op3Spec::RestFinish(o) => o.common(),
            Op3Spec::Drill(o) => o.common(),
            Op3Spec::Chamfer(o) => o.common(),
        }
    }

    /// [`Op3Spec::common`] of the operation inside `indexed` or `indexed_pattern`: named `outer`
    /// when it has no name of its own, and turned off when either is.
    fn inner_common<'a>(&'a self, outer: Option<&'a String>, enabled: Option<bool>) -> Common<'a> {
        let mut c = self.common();
        if c.name.trim().is_empty()
            && let Some(n) = outer
        {
            c.name = n.as_str();
        }
        c.enabled &= enabled.unwrap_or(true);
        c
    }
}

/// A 3-axis operation run with the A axis held at `a_deg` (3+1): its heights, boundary and
/// polygon are in the frame that angle brings under the tool.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IndexedOp {
    /// The operation's name, for an inner operation without one of its own.
    #[serde(default)]
    pub name: Option<String>,
    /// Plan and write this operation (`false`: kept in the job, turned off). Default: true.
    #[serde(default)]
    pub enabled: Option<bool>,
    /// The A angle, degrees.
    pub a_deg: f64,
    /// The 3-axis operation (`adaptive`, `adaptive_roughing`, `contour`, `pocket`, `parallel`,
    /// `roughing`, `waterline`, `scallop`, `steep_shallow`, `pencil`, `rest_finish`, `drill` or
    /// `chamfer`).
    pub operation: Box<Op3Spec>,
}

/// One operation of a job, by `type`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OperationSpec {
    Contour(ContourOp),
    Pocket(PocketOp),
    Parallel(ParallelOp),
    Roughing(RoughingOp),
    Waterline(WaterlineOp),
    Scallop(ScallopOp),
    SteepShallow(SteepShallowOp),
    Pencil(PencilOp),
    RestFinish(RestFinishOp),
    Indexed(IndexedOp),
    RotaryFinish(RotaryFinishOp),
    RotaryRoughing(RotaryRoughingOp),
    Wrap(WrapOp),
    WrapPocket(WrapPocketOp),
    WrapContour(WrapContourOp),
    IndexedPattern(IndexedPatternOp),
    Drill(DrillOp),
    Chamfer(ChamferOp),
    Bore(BoreOp),
    ThreadMill(ThreadMillOp),
    Adaptive(AdaptiveOp),
    AdaptiveRoughing(AdaptiveRoughingOp),
    Face(FaceOp),
}

impl OperationSpec {
    /// What every operation has: an indexed one's inner operation's, named and turned on or off
    /// by the indexed one where that says.
    pub fn common(&self) -> Common<'_> {
        match self {
            OperationSpec::Contour(o) => o.common(),
            OperationSpec::Pocket(o) => o.common(),
            OperationSpec::Parallel(o) => o.common(),
            OperationSpec::Roughing(o) => o.common(),
            OperationSpec::Waterline(o) => o.common(),
            OperationSpec::Scallop(o) => o.common(),
            OperationSpec::SteepShallow(o) => o.common(),
            OperationSpec::Pencil(o) => o.common(),
            OperationSpec::RestFinish(o) => o.common(),
            OperationSpec::Indexed(o) => o.operation.inner_common(o.name.as_ref(), o.enabled),
            OperationSpec::RotaryFinish(o) => o.common(),
            OperationSpec::RotaryRoughing(o) => o.common(),
            OperationSpec::Wrap(o) => o.common(),
            OperationSpec::WrapPocket(o) => o.common(),
            OperationSpec::WrapContour(o) => o.common(),
            OperationSpec::IndexedPattern(o) => o.operation.inner_common(o.name.as_ref(), o.enabled),
            OperationSpec::Drill(o) => o.common(),
            OperationSpec::Chamfer(o) => o.common(),
            OperationSpec::Bore(o) => o.common(),
            OperationSpec::ThreadMill(o) => o.common(),
            OperationSpec::Adaptive(o) => o.common(),
            OperationSpec::AdaptiveRoughing(o) => o.common(),
            OperationSpec::Face(o) => o.common(),
        }
    }
}

/// What `cam.job` returns.
#[derive(Debug, Default, Serialize, JsonSchema)]
pub struct CamJobReply {
    /// The program's name.
    pub name: String,
    /// The machine's name.
    pub machine: String,
    /// The post's name.
    pub post: String,
    /// Every operation, in program order.
    pub operations: Vec<OperationReport>,
    /// Lines of G-code in the program.
    pub program_lines: usize,
    /// The first lines of the program.
    pub program_head: Vec<String>,
    /// The program read back and compared with the toolpaths.
    pub readback: ReadbackReport,
    /// The gouge check against the part and fixtures.
    pub gouge: GougeReport,
    /// Moves along which a tool's shank or holder comes closer to the part or fixtures than
    /// zero (tools with holder data only); `null` when no tool has holder data.
    pub holder_collisions: Option<usize>,
    /// Every rapid of the program as written against the part, fixtures and stock.
    pub rapids: RapidsReport,
    /// The linear axes against the machine's `linear_limits`; `null` for a machine without them.
    pub travel: Option<TravelReport>,
    /// The job's stock as checked against; `null` without one.
    pub stock: Option<StockReport>,
    /// The program checked against the job's workholding and machine model; absent without a
    /// workholding.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workholding: Option<HoldingReport>,
    /// Time at feed of the whole program, dwells included, minutes: as the machine runs it with
    /// the job's `machine_rates`, otherwise as written.
    pub cut_minutes: f64,
    /// Time at rapid with the machine's rapid rates, minutes; `null` without `machine_rates`.
    pub rapid_minutes: Option<f64>,
    /// The program timed against the job's `machine_rates`; `null` without them.
    pub machine_time: Option<MachineTimeReport>,
    /// Whether every check passed.
    pub passed: bool,
    /// The checks that failed.
    pub failures: Vec<String>,
    /// Whether this was a dry run (nothing written).
    pub dry_run: bool,
    /// The files written.
    pub outputs: Outputs,
    /// Anything else worth knowing: a post that writes no work offset, an output extension
    /// other than the post's, what the machine's rates say about the program.
    pub notes: Vec<String>,
    /// The stock-removal simulation of the written program, with `simulate`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub simulation: Option<super::cam_sim::CamSimulateReply>,
    /// What the operator sets up: the setup sheet's tables in compact form.
    pub summary: super::cam_sheet::SetupSummary,
    /// What to look at that does not fail the job (a program of several tools for a machine that
    /// ignores tool changes).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
    /// With `split_by_tool`: the programs in the order to run them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub programs: Vec<ProgramPart>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// One operation of a `cam.job` reply.
#[derive(Clone, Debug, Default, Serialize, JsonSchema)]
pub struct OperationReport {
    pub name: String,
    /// Its `type`.
    pub kind: String,
    /// The tool's number.
    pub tool: u32,
    /// Moves in its toolpath.
    pub moves: usize,
    /// Distance at feed, mm.
    pub cut_length: f64,
    /// Time at feed, minutes, a rotary operation's rewinds included.
    pub cut_minutes: f64,
    /// Times the post rewinds A in it to stay within its limits (a post with `rewind`); the time
    /// at feed they add is in `cut_minutes`.
    pub rewinds: usize,
    /// Largest angle between the tool axis and work +Z, degrees.
    pub max_tilt_deg: f64,
    /// The A angle it is indexed at, degrees (indexed operations only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub a_deg: Option<f64>,
    /// How deep its deepest gouge is in the tessellated part, mm (0: none deeper than the check
    /// allows).
    pub gouge: f64,
    /// Where that gouge is.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gouge_at: Option<GougeAt>,
    /// Moves with a holder collision.
    pub holder_collisions: usize,
    /// An adaptive operation's engagement, measured along its path, and the plain pocket it is
    /// compared with.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub adaptive: Option<AdaptiveReport>,
    pub warnings: Vec<String>,
}

/// The program as written against its toolpaths.
#[derive(Debug, Default, Serialize, JsonSchema)]
pub struct ReadbackReport {
    /// Largest distance between the tip as written and the toolpaths, mm.
    pub max_tip_deviation: f64,
    /// Largest angle between the tool axis as written and the toolpaths, degrees.
    pub max_axis_deviation_deg: f64,
    /// The deviation the post's precision accounts for, mm.
    pub allowed_tip: f64,
    /// The angle the post's precision accounts for, degrees.
    pub allowed_axis_deg: f64,
    /// Time at feed as the controller times the written program, minutes.
    pub cut_minutes: f64,
    pub pass: bool,
}

/// The gouge check.
#[derive(Debug, Default, Serialize, JsonSchema)]
pub struct GougeReport {
    /// The deepest gouge of any operation, mm into the tessellated part (0: none deeper than
    /// `allowed`): the deepest point of the part inside the cutter, measured to the cutter.
    pub worst: f64,
    /// The job's tolerance, mm.
    pub tolerance: f64,
    /// How far the tessellated part may be from the exact one, mm.
    pub tessellation: f64,
    /// Depths into the tessellated part up to this count as touching, mm: the tolerance less
    /// the tessellation, so no cut passed is deeper than the tolerance into the exact part.
    pub allowed: f64,
    /// Moves with a gouge.
    pub hits: usize,
    /// Where the deepest gouge is.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub at: Option<GougeAt>,
    /// Operations not checked against the part (`gouge_check: false`); they are still checked
    /// against the fixtures.
    pub skipped: Vec<String>,
    pub pass: bool,
}

/// Where a gouge is.
#[derive(Clone, Debug, Default, Serialize, JsonSchema)]
pub struct GougeAt {
    /// The operation's name.
    pub operation: String,
    /// The move of the operation's toolpath (counted from 0) along which it is.
    pub move_index: usize,
    /// The line of the program (counted from 1) that ends that move, when it is found.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<usize>,
    /// The tool tip at the deepest point, work coordinates, mm.
    pub xyz: [f64; 3],
}

/// The program checked against the workholding and the machine model.
#[derive(Clone, Debug, Default, Serialize, JsonSchema)]
pub struct HoldingReport {
    /// The workholding's name (its file's `name`, else its kind).
    pub name: String,
    /// `vise`, `clamps`, `table` or `rotary`.
    pub holding: String,
    /// The machine model's name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub machine_model: Option<String>,
    /// Moves checked.
    pub checked_moves: usize,
    /// What was found, each kind once per tool and operation (at most 20).
    pub hits: Vec<HoldingHitReport>,
    /// In a vise: how far the stock's top stands above the jaws, mm.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stock_above_jaws: Option<f64>,
    /// In a rotary unit: how the bar sits in the chuck, along work X.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bar_grip: Option<BarGripReport>,
    pub pass: bool,
    /// What is wrong with the setup itself (the stock wider than the vise opens, not gripped):
    /// each fails the job.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub setup: Vec<String>,
    /// What is doubtful about the setup for this job without failing it (the vise gripping too
    /// little of the stock for the job's deepest side cut), each with what to change; each is in
    /// the reply's `notes` too.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub setup_warnings: Vec<String>,
}

/// How a bar sits in a rotary unit's chuck, along work X (mm).
#[derive(Clone, Copy, Debug, Default, Serialize, JsonSchema)]
pub struct BarGripReport {
    /// The chuck's face.
    pub face_x: f64,
    /// The bar's end towards the chuck.
    pub held_end_x: f64,
    /// The bar's free end.
    pub free_end_x: f64,
    /// How much of the bar the jaws grip.
    pub in_jaws: f64,
    /// How far the bar stands out of the jaws to its free end.
    pub stands_out: f64,
    /// How far work zero (X0) is out from the chuck's face, towards the free end.
    pub zero_out: f64,
}

/// One kind of collision with the workholding, the table or the spindle's travel.
#[derive(Clone, Debug, Default, Serialize, JsonSchema)]
pub struct HoldingHitReport {
    /// `cutter-hits-vise`, `holder-hits-rotary`, `cutter-into-table`, `z-travel`, ...
    pub code: String,
    /// Whether it fails the job.
    pub error: bool,
    pub tool: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operation: Option<String>,
    /// The G-code line it first happens at.
    pub line: usize,
    /// Moves that have it.
    pub moves: usize,
    /// Where, work coordinates.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub at: Option<[f64; 3]>,
    pub message: String,
    pub fix: String,
}

/// The files `cam.job` wrote.
#[derive(Debug, Default, Serialize, JsonSchema)]
pub struct Outputs {
    /// The G-code file.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gcode: Option<String>,
    /// The HTML preview.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preview: Option<String>,
    /// The setup sheet.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub setup_sheet: Option<String>,
    /// With `split_by_tool`: every program written, in the order to run them (`gcode` is then
    /// left out).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub programs: Vec<String>,
}

/// One program of a job written one program per tool.
#[derive(Clone, Debug, Default, Serialize, JsonSchema)]
pub struct ProgramPart {
    /// Its place in the order, from 1.
    pub order: usize,
    /// The file written; `null` in a dry run.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    pub tool: u32,
    /// The tool's stickout from its holder, mm, when the tool gives it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stickout: Option<f64>,
    /// Its operations, in order.
    pub operations: Vec<String>,
    pub lines: usize,
    /// Time at feed, minutes.
    pub cut_minutes: f64,
}

/// List the built-in posts.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CamPostListParams {}

/// What `cam.post.list` returns.
#[derive(Debug, Default, Serialize, JsonSchema)]
pub struct CamPostListReply {
    /// Every built-in post.
    pub posts: Vec<PostEntry>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// A built-in post.
#[derive(Debug, Serialize, JsonSchema)]
pub struct PostEntry {
    pub name: String,
    pub description: String,
    /// Other names it is found by.
    pub aliases: Vec<String>,
}

/// Return a built-in post as JSON, to copy into a post file.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CamPostGetParams {
    /// The post's name or one of its other names (`cam.post.list`).
    pub name: String,
}

/// What `cam.post.get` returns.
#[derive(Debug, Default, Serialize, JsonSchema)]
pub struct CamPostGetReply {
    /// The post's name, as a job gives it.
    pub name: String,
    /// The post as the CAM crate's `Post5` JSON, to save as a job's `post_path`.
    pub post: Value,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// Read a G-code file and report what it does: lengths, time, the rotary range, inverse-time
/// blocks, and the first error the reader finds. Reads the subset of G-code SolidCraft's posts
/// write (absolute coordinates); other programs are read on a best-effort basis.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CamGcodeCheckParams {
    /// The G-code file.
    pub path: String,
    /// The rapid rate in mm/min, to estimate time at rapid. Default: rapids are not timed.
    #[serde(default)]
    pub rapid_rate: Option<f64>,
}

/// What `cam.gcode.check` returns.
#[derive(Debug, Default, Serialize, JsonSchema)]
pub struct CamGcodeCheckReply {
    pub path: String,
    /// Lines in the file.
    pub lines: usize,
    /// Motion blocks read (rapids, cuts, arcs, dwells).
    pub motion_blocks: usize,
    /// Distance of the linear axes at rapid, mm.
    pub rapid_length: f64,
    /// Distance of the linear axes at feed, mm.
    pub feed_length: f64,
    /// Time at feed, dwells included, minutes, as the controller times it.
    pub cut_minutes: f64,
    /// Time at rapid at `rapid_rate`, minutes (with `rapid_rate`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rapid_minutes: Option<f64>,
    /// Arcs (G2, G3).
    pub arcs: usize,
    /// Cuts in inverse time (G93).
    pub g93_blocks: usize,
    /// The rotary axes the program moves, each with its lowest and highest angle, degrees.
    pub rotary_ranges: Vec<RotaryRange>,
    /// Lowest and highest X, Y, Z reached, mm (absent with no motion).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bounds: Option<[[f64; 3]; 2]>,
    /// What the reader could not read (it stops at the first).
    pub errors: Vec<ParseErrorReport>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// A rotary axis's range in a program.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct RotaryRange {
    pub letter: String,
    pub min_deg: f64,
    pub max_deg: f64,
}

/// A line the reader could not read.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ParseErrorReport {
    /// The line, from 1 (0: not tied to a line).
    pub line: usize,
    pub message: String,
}

#[cfg(test)]
#[path = "cam_tests.rs"]
mod tests;
