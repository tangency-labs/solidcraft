//! Manufacturing (CAM) foundation for SolidCraft: cutting tools, toolpaths, 2D contour and
//! pocket milling of planar regions, machine code through declarative post processors (G-code
//! today), and a reader that checks what was written against the toolpaths it was written from.
//!
//! Everything here works in a setup's work coordinates: millimetres, tool axis along +Z, feeds
//! in mm/min. The crate knows nothing of documents or the B-rep kernel; the engine hands it
//! regions.
//!
//! raygeo is the motion authority: it plans and generates every cut, walks the requested cut
//! direction, feeds the moves, and links its passes machinist-readably (SafeZ travel). This
//! crate prepares the regions, verifies every path on its own terms, and owns the machine
//! output: a format-tagged [`program`] behind a [`MachineOutput`] seam, with the G-code post
//! as its first backend. Its own exact line-and-arc offsetting survives as the oracle: the
//! translator clamps clearing moves against it, and it prepares the stock to leave.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod check;
mod gcode;
mod offset;
mod ops;
mod params;
mod path;
mod piece;
mod planner;
mod post;
mod program;
mod readback;
mod tool;

pub use check::verify;
pub use gcode::{emit, parse, work_offset_code};
pub use offset::offset_regions;
pub use ops::{adaptive, contour, pocket};
pub use params::{AdaptiveParams, ContourParams, Cut, Feeds, Heights, PocketParams, Side};
pub use path::{Move, PathStats, Toolpath, arc_sweep};
pub use post::{CommentStyle, LineNumbers, Post, Units};
pub use program::{MachineCode, MachineOutput, Operation, Program};
pub use readback::{Readback, allowed_deviation, readback};
pub use tool::{Tool, ToolKind};

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum CamError {
    #[error("invalid value: {0}")]
    Invalid(String),
    #[error("not supported yet: {0}")]
    Unsupported(String),
    #[error("too large: {0}")]
    TooLarge(String),
    /// A toolpath that breaks a safety rule (a rapid through material, a cut below the
    /// bottom). Operations check their own output, so this reports a defect rather than bad input.
    #[error("unsafe toolpath: {0}")]
    Unsafe(String),
    #[error("G-code line {0}: {1}")]
    Parse(usize, String),
}

pub type Result<T> = std::result::Result<T, CamError>;

pub(crate) fn invalid<T>(why: impl Into<String>) -> Result<T> {
    Err(CamError::Invalid(why.into()))
}

#[cfg(test)]
mod tests;
