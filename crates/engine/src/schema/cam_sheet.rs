//! The `summary` of a `cam.job` reply: what the machine operator has to set up, the same table
//! data as the job's setup sheet (docs/cam-jobs.md, "Setup sheet") in compact form, so an agent
//! can tell the user which tools to load, where work zero goes and what the program does.

use schemars::JsonSchema;
use serde::Serialize;

/// What to set up for a job.
#[derive(Debug, Default, Serialize, JsonSchema)]
pub struct SetupSummary {
    /// Where work zero (X0 Y0 Z0) is on the part, a sentence each.
    pub work_zero: Vec<String>,
    /// The part's lowest and highest [x, y, z] from work zero, mm.
    pub part_extent: [[f64; 3]; 2],
    /// The work offset the program runs in (`G54` ...), or what the post does instead.
    pub work_offset: String,
    /// Each rotary axis's convention (what it turns, which way positive goes, its travel), a
    /// sentence each; empty on a 3-axis machine.
    pub rotary: Vec<String>,
    /// The tools to load, by number.
    pub tools: Vec<SummaryTool>,
    /// The operations, in program order.
    pub operations: Vec<SummaryOperation>,
    pub totals: SummaryTotals,
    /// Each check the program was put through, with its result.
    pub checks: Vec<String>,
    /// What the operator should look out for: failed checks, the job's warnings, rotary turns
    /// past a full turn.
    pub warnings: Vec<String>,
}

/// A tool to load.
#[derive(Debug, Default, Serialize, JsonSchema)]
pub struct SummaryTool {
    pub number: u32,
    /// Its kind in words.
    pub kind: String,
    /// Cutting diameter and flute length, mm.
    pub diameter: f64,
    pub flute_length: f64,
    /// Tip to holder face, mm (when the tool says).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stickout: Option<f64>,
    /// The holder in words (`Ø34 x 4 + Ø34 x 18`), when the tool has holder data.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub holder: Option<String>,
    /// The lowest Z the program sends its tip to, in program coordinates, mm.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub z_min: Option<f64>,
    /// The operations that use it.
    pub operations: Vec<String>,
}

/// One operation.
#[derive(Debug, Default, Serialize, JsonSchema)]
pub struct SummaryOperation {
    pub name: String,
    pub kind: String,
    pub tool: u32,
    pub spindle_rpm: f64,
    /// Cutting and plunge feed, mm/min.
    pub feed: f64,
    pub plunge_feed: f64,
    /// The rotary angle or range it runs at (`A90°`, `A -180° to 180°`); absent on a 3-axis machine.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rotary: Option<String>,
    /// Distance at feed, mm, and time at feed, minutes.
    pub cut_length: f64,
    pub cut_minutes: f64,
}

/// The job's totals.
#[derive(Debug, Default, Serialize, JsonSchema)]
pub struct SummaryTotals {
    pub operations: usize,
    pub tools: usize,
    /// Tool changes, the first load included.
    pub tool_changes: usize,
    /// Distance at feed, mm.
    pub cut_length: f64,
    /// Time at feed as planned, minutes (rapids and tool changes not included).
    pub cut_minutes: f64,
    /// Time at feed as the controller times the written program, minutes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub program_minutes: Option<f64>,
}
