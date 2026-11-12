//! Contracts and result types for the CAM simulation view's commands (`cam.view.*`, in
//! `cmd/cam_view.rs`; the state in `cam_view.rs`): a job's program played move by move on the
//! simulated stock, with the tool and its holder. docs/cam-jobs.md describes them.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::cam::CamJob;
use super::cam_sim::SimStockReport;

/// Load a job's program into the simulation view: the job planned and written in memory (as
/// `cam.job` with `dry_run`), or a G-code file of it; the stock, and the timeline of moves,
/// operations and tool changes. The view starts at the program's start; nothing is saved in the
/// design.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CamViewLoadParams {
    /// The job (as `cam.job` takes it). Give this or `job_path`.
    #[serde(default)]
    pub job: Option<CamJob>,
    /// A `.json` file holding the job.
    #[serde(default)]
    pub job_path: Option<String>,
    /// A G-code file to play instead of the job's own program (in the job's work coordinates);
    /// its operations are then one per tool change.
    #[serde(default)]
    pub program: Option<String>,
    /// The stock's cell size in mm. Default: five times the job's tolerance, coarser when that
    /// would be more than four million cells.
    #[serde(default)]
    pub resolution: Option<f64>,
    /// Play the whole program through once at load (every finding known at once), then go back
    /// to the start. Default: false (the stock is cut as the view plays).
    #[serde(default)]
    pub run_to_end: Option<bool>,
    /// What colours the cut stock and the toolpaths: `tool` (default) or `operation`.
    #[serde(default)]
    pub colour_by: Option<ColourBy>,
}

/// What colours the cut stock and the toolpaths.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ColourBy {
    #[default]
    Tool,
    Operation,
}

/// What `cam.view.load` returns.
#[derive(Debug, Default, Serialize, JsonSchema)]
pub struct CamViewLoadReply {
    /// Moves (motion blocks) in the timeline.
    pub moves: usize,
    /// Lines of the program.
    pub lines: usize,
    /// The program's length in time, seconds (with acceleration when the rates give it).
    pub duration_s: f64,
    /// Whether the moves are timed with acceleration.
    pub accelerated: bool,
    pub operations: Vec<ViewOperation>,
    pub tool_changes: Vec<ViewToolChange>,
    /// The stock the simulation starts from.
    pub stock: SimStockReport,
    /// The cell size, mm.
    pub resolution: f64,
    pub cells: usize,
    /// What is known to be wrong already (with `run_to_end`, all).
    pub findings: Vec<ViewFinding>,
    pub notes: Vec<String>,
    pub warnings: Vec<String>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// A run of moves of one operation.
#[derive(Debug, Default, Serialize, JsonSchema)]
pub struct ViewOperation {
    /// Its place in `operations` (what `cam.view.seek {op}` takes as a number).
    pub index: usize,
    pub name: String,
    pub tool: u32,
    /// Its moves: `first_move` to before `end_move`.
    pub first_move: usize,
    pub end_move: usize,
    pub start_s: f64,
    pub end_s: f64,
}

/// A tool change: the first move with the new tool.
#[derive(Debug, Default, Serialize, JsonSchema)]
pub struct ViewToolChange {
    #[serde(rename = "move")]
    pub at_move: usize,
    pub line: usize,
    pub tool: u32,
}

/// One thing found, at its move and place.
#[derive(Debug, Default, Serialize, JsonSchema)]
pub struct ViewFinding {
    /// Stable kebab-case kind: `rapid-in-stock`, `gouge`, `cut-spindle-off`, `shank-rubs`,
    /// `holder-hits-stock`, ...
    pub code: String,
    /// `error`, `warning` or `info`.
    pub severity: String,
    /// The move it happens in.
    #[serde(rename = "move", skip_serializing_if = "Option::is_none")]
    pub at_move: Option<usize>,
    /// The G-code line.
    pub line: usize,
    /// The operation (a place in `operations`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub op: Option<usize>,
    /// Where, work coordinates.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub at: Option<[f64; 3]>,
    /// How deep, mm (0 when it has no depth).
    pub depth: f64,
    pub message: String,
}

/// Move the simulation view's tool (and the stock cut with it) to a place in the program. Give
/// exactly one of `move`, `line`, `op`, `time_s`, `percent`, `tool_change`, `finding` and `by`.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CamViewSeekParams {
    /// To the end of this move (from 0).
    #[serde(default, rename = "move")]
    pub at_move: Option<f64>,
    /// To the end of the last move at or before this G-code line (from 1).
    #[serde(default)]
    pub line: Option<f64>,
    /// To an operation: its name or its place in `operations`; `at` its `start` (default) or `end`.
    #[serde(default)]
    pub op: Option<OpRef>,
    #[serde(default)]
    pub at: Option<OpEnd>,
    /// To this time from the start, seconds.
    #[serde(default)]
    pub time_s: Option<f64>,
    /// To this share of the program's time, 0 to 100.
    #[serde(default)]
    pub percent: Option<f64>,
    /// To the start of the first move with the tool of this tool change (from 0).
    #[serde(default)]
    pub tool_change: Option<f64>,
    /// To the end of the move of this finding (from 0, in the order of `cam.view.state`'s
    /// `findings: all`).
    #[serde(default)]
    pub finding: Option<f64>,
    /// This many moves on (negative: back); with `until`, on (back) to the next (last)
    /// `tool_change`, `op` start or `finding`.
    #[serde(default)]
    pub by: Option<f64>,
    #[serde(default)]
    pub until: Option<SeekUntil>,
}

/// An operation by name or place.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum OpRef {
    Index(f64),
    Name(String),
}

#[derive(Clone, Copy, Debug, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OpEnd {
    Start,
    End,
}

#[derive(Clone, Copy, Debug, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SeekUntil {
    ToolChange,
    Op,
    Finding,
}

/// Start playing the simulation view (a window plays it on at `speed`; headless it is a flag:
/// seek to move on).
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CamViewPlayParams {
    /// Times the program's estimated real time, above 0 to 1000. Default: 1 (or as before).
    #[serde(default)]
    pub speed: Option<f64>,
    /// Moves a second instead of real time, above 0 to 1000000.
    #[serde(default)]
    pub moves_per_s: Option<f64>,
    /// Start again at the end. Default: as before (off).
    #[serde(default, rename = "loop")]
    pub looping: Option<bool>,
}

/// No parameters.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CamViewEmptyParams {}

/// What the simulation view shows. Every switch left out stays as it is.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ViewLayers {
    /// The cuts' path.
    pub toolpaths: bool,
    /// The rapids' path.
    pub rapids: bool,
    /// The path still to come (dim).
    pub remaining_path: bool,
    pub stock: bool,
    /// The part (the design's bodies).
    pub part: bool,
    pub tool: bool,
    pub holder: bool,
    pub gouges: bool,
    pub collisions: bool,
    /// Stock coloured by how far it is from the part.
    pub deviation: bool,
}

impl Default for ViewLayers {
    fn default() -> ViewLayers {
        ViewLayers {
            toolpaths: true,
            rapids: true,
            remaining_path: true,
            stock: true,
            part: false,
            tool: true,
            holder: true,
            gouges: true,
            collisions: true,
            deviation: false,
        }
    }
}

/// Switch layers of the simulation view on or off.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CamViewShowParams {
    #[serde(default)]
    pub toolpaths: Option<bool>,
    #[serde(default)]
    pub rapids: Option<bool>,
    #[serde(default)]
    pub remaining_path: Option<bool>,
    #[serde(default)]
    pub stock: Option<bool>,
    #[serde(default)]
    pub part: Option<bool>,
    #[serde(default)]
    pub tool: Option<bool>,
    #[serde(default)]
    pub holder: Option<bool>,
    #[serde(default)]
    pub gouges: Option<bool>,
    #[serde(default)]
    pub collisions: Option<bool>,
    #[serde(default)]
    pub deviation: Option<bool>,
    #[serde(default)]
    pub colour_by: Option<ColourBy>,
}

/// What `cam.view.show` returns.
#[derive(Debug, Default, Serialize, JsonSchema)]
pub struct CamViewShowReply {
    pub layers: ViewLayers,
    pub colour_by: ColourBy,
}

/// How many findings `cam.view.state` lists.
#[derive(Clone, Copy, Debug, Default, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FindingsListing {
    None,
    /// Counts by code only (default).
    #[default]
    Summary,
    /// Every finding so far too (up to `limit`).
    All,
}

/// The simulation view's state.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CamViewStateParams {
    #[serde(default)]
    pub findings: Option<FindingsListing>,
    /// Most findings listed, 1 to 10000. Default: 100.
    #[serde(default)]
    pub limit: Option<f64>,
}

/// The tool in the spindle.
#[derive(Debug, Default, Serialize, JsonSchema)]
pub struct ViewTool {
    pub number: u32,
    /// `flat_endmill`, `ball_endmill`, ...
    pub kind: String,
    pub diameter: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stickout: Option<f64>,
}

/// Where the tool is.
#[derive(Debug, Default, Serialize, JsonSchema)]
pub struct ViewPosition {
    /// The tip, work coordinates, mm.
    pub tip: [f64; 3],
    /// Up the tool, a unit vector.
    pub axis: [f64; 3],
    /// The machine's axes as the program writes them.
    pub machine: ViewMachineWords,
}

#[derive(Debug, Default, Serialize, JsonSchema)]
pub struct ViewMachineWords {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    /// The rotary, degrees (a 4th axis only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub a_deg: Option<f64>,
}

#[derive(Debug, Default, Serialize, JsonSchema)]
pub struct ViewOpRef {
    pub index: usize,
    pub name: String,
}

/// Findings so far, by code.
#[derive(Debug, Default, Serialize, JsonSchema)]
pub struct FindingsSoFar {
    pub errors: usize,
    pub warnings: usize,
    pub infos: usize,
    pub by_code: Vec<FindingCount>,
}

#[derive(Debug, Default, Serialize, JsonSchema)]
pub struct FindingCount {
    pub code: String,
    pub severity: String,
    pub count: usize,
    /// The first line it happens at.
    pub first_line: usize,
    /// The first lines (up to 50).
    pub lines: Vec<usize>,
    pub message: String,
}

/// What `cam.view.state`, `seek`, `play` and `pause` return.
#[derive(Debug, Default, Serialize, JsonSchema)]
pub struct CamViewStateReply {
    /// The move the tool is in (from 0) and the share of it done (0 to 1).
    #[serde(rename = "move")]
    pub at_move: usize,
    pub moves: usize,
    pub fraction: f64,
    /// Its G-code line and text.
    pub line: usize,
    pub gcode: String,
    /// `rapid`, `feed`, `arc` or `plunge`.
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub op: Option<ViewOpRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool: Option<ViewTool>,
    pub position: ViewPosition,
    /// The feed (mm/min) of a cut.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub feed: Option<f64>,
    /// The spindle's speed when the program has said (0: stopped).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rpm: Option<f64>,
    pub time_s: f64,
    pub duration_s: f64,
    pub percent: f64,
    /// Material removed so far, mm³.
    pub removed_volume: f64,
    pub playing: bool,
    pub speed: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub moves_per_s: Option<f64>,
    #[serde(rename = "loop")]
    pub looping: bool,
    pub layers: ViewLayers,
    pub findings_so_far: FindingsSoFar,
    /// With `findings: all`: every finding so far (up to `limit`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub findings: Option<Vec<ViewFinding>>,
    /// Moves whose findings are known (the simulation has cut that far).
    pub checked_moves: usize,
    pub keyframes: usize,
    pub keyframe_bytes: usize,
    /// Changes with every change of the view (for a window's caches).
    pub frame: u64,
}

/// What `cam.view.close` returns.
#[derive(Debug, Default, Serialize, JsonSchema)]
pub struct CamViewCloseReply {
    pub closed: bool,
}

/// Write the stock as the simulation view has cut it so far to an STL file.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CamViewExportStockParams {
    /// The `.stl` file to write.
    pub path: String,
}

#[derive(Debug, Default, Serialize, JsonSchema)]
pub struct CamViewExportStockReply {
    pub path: String,
    pub triangles: usize,
}
