//! The neutral move list: what CAM output and G-code both reduce to, and what the simulator reads.
//! Positions are in the workpiece frame, in millimetres. The tool axis is `[0, 0, 1]` for 3-axis.

use serde::{Deserialize, Serialize};

/// The tool of a move made before the first tool change (M6): none is selected yet. The simulator
/// reports such a move and does not cut it.
pub const NO_TOOL: u32 = u32::MAX;

/// A program: the moves in order, the tools they name, and what went wrong on the way.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Program {
    pub tools: Vec<Tool>,
    pub moves: Vec<Move>,
    pub diags: Vec<Diag>,
}

/// One tool-tip move. `tip` is where the tip ends; the start is the previous move's tip.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Move {
    pub kind: MoveKind,
    pub tip: [f64; 3],
    /// End tool axis, tip to spindle.
    pub axis: [f64; 3],
    /// End joint values, when the source knows them.
    pub joints: Option<Vec<f64>>,
    pub interp: Interp,
    pub tool: u32,
    /// Feed in mm/min (0 for rapids).
    pub feed: f64,
    /// Spindle speed in rpm (0 when stopped).
    pub spindle: f64,
    /// Source line (G-code) or CAM operation id.
    pub source: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum MoveKind {
    Rapid,
    Feed,
    /// The tool is placed at `tip` without cutting: the path from where it was is not known (after
    /// G28 or G30). The next move starts from `tip`.
    Jump,
    /// A circular or helical arc from the previous tip to `tip`, about `centre` with `normal`
    /// (seen from its tip, counter-clockwise when `ccw`). `turns` counts whole extra revolutions
    /// on top of the arc itself; the G-code subset always writes 0.
    Arc {
        centre: [f64; 3],
        normal: [f64; 3],
        ccw: bool,
        turns: u32,
    },
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Interp {
    #[default]
    Tcp,
    Joint,
    Slerp,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Tool {
    pub id: u32,
    pub shape: Shape,
    pub diameter: f64,
    pub flute_length: f64,
    /// Non-cutting profile as `[a, r]` pairs, `a` measured from the tip along the axis.
    pub shank: Vec<[f64; 2]>,
    pub holder: Vec<[f64; 2]>,
    pub stickout: f64,
}

/// Cutting shape, a body of revolution about the axis with the tip at the origin.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Shape {
    Flat,
    Ball,
    Bull { rc: f64 },
    Cone { tip_d: f64, angle: f64 },
    Taper { a0: f64, r0: f64, angle: f64 },
    Profile(Vec<[f64; 2]>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Severity {
    Error,
    Warning,
}

/// A problem found while reading or simulating a program. `col` is 0 when unknown.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diag {
    pub line: u32,
    pub col: u32,
    pub severity: Severity,
    pub msg: String,
}

impl Diag {
    pub fn error(line: u32, col: u32, msg: impl Into<String>) -> Diag {
        Diag { line, col, severity: Severity::Error, msg: msg.into() }
    }

    pub fn warning(line: u32, col: u32, msg: impl Into<String>) -> Diag {
        Diag { line, col, severity: Severity::Warning, msg: msg.into() }
    }

    pub fn is_error(&self) -> bool {
        self.severity == Severity::Error
    }
}
