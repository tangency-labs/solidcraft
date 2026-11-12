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

use super::types::SketchRef;

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
    /// Plan and check the job, and report, but write no file; the report says whether the
    /// checks pass instead of refusing. Default: false.
    #[serde(default)]
    pub dry_run: Option<bool>,
    /// Write the program even when the read-back check fails (the report says so, and so does
    /// the program's first line). Default: false.
    #[serde(default)]
    pub allow_unsafe: Option<bool>,
}

/// A CAM job: what is machined, on which machine, through which post, with which tools.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CamJob {
    /// The program's name, written in its header. Default: the design's name.
    #[serde(default)]
    pub name: Option<String>,
    /// `3axis`, `4axis_a` (a rotary table about +X, the A axis, through work zero), or a whole
    /// machine as the CAM crate's `Machine` JSON.
    #[serde(default)]
    pub machine: Option<MachineSpec>,
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
    /// The part's bodies by name, drawn in the preview. Default: every visible body.
    #[serde(default)]
    pub bodies: Option<Vec<String>>,
    /// The machining tolerance in mm, 0.0001 to 1: how far a toolpath may be from the profiles
    /// it follows. Default: 0.01.
    #[serde(default)]
    pub tolerance: Option<f64>,
    /// The tools, as the CAM crate's `Tool` JSON (`number`, `kind`, `diameter`, `flute_length`,
    /// ...). Give this or `tool_library_path`.
    #[serde(default)]
    pub tools: Option<Vec<Map<String, Value>>>,
    /// A JSON file holding an array of tools, as `tools`.
    #[serde(default)]
    pub tool_library_path: Option<String>,
    /// The operations, run and written in this order.
    pub operations: Vec<OperationSpec>,
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

/// Declares an operation's struct with the fields every operation has, then its own.
macro_rules! operation {
    ($(#[$doc:meta])* $name:ident { $($(#[$fdoc:meta])* pub $field:ident : $ty:ty,)* }) => {
        $(#[$doc])*
        #[derive(Clone, Debug, Deserialize, JsonSchema)]
        #[serde(deny_unknown_fields)]
        pub struct $name {
            /// The operation's name, written as a comment in the program.
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
                }
            }
        }
    };
}

/// What every operation has.
#[derive(Clone, Copy, Debug)]
pub struct Common<'a> {
    pub name: &'a str,
    pub tool: u32,
    pub spindle_rpm: Option<f64>,
    pub feeds: Option<FeedsSpec>,
    pub coolant: bool,
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
    }
}

/// One operation of a job, by `type`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OperationSpec {
    Contour(ContourOp),
    Pocket(PocketOp),
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
    /// Time at feed of the whole program, dwells included, minutes, as written.
    pub cut_minutes: f64,
    /// Whether every check passed.
    pub passed: bool,
    /// The checks that failed.
    pub failures: Vec<String>,
    /// Whether this was a dry run (nothing written).
    pub dry_run: bool,
    /// The files written.
    pub outputs: Outputs,
    /// Anything else worth knowing: a post that writes no work offset, an output extension
    /// other than the post's.
    pub notes: Vec<String>,
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
    /// Time at feed, minutes.
    pub cut_minutes: f64,
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

/// The files `cam.job` wrote.
#[derive(Debug, Default, Serialize, JsonSchema)]
pub struct Outputs {
    /// The G-code file.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gcode: Option<String>,
    /// The HTML preview.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preview: Option<String>,
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
