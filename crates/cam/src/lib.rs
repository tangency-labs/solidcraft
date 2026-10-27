//! Manufacturing (CAM) foundation for SolidCraft: cutting tools, toolpaths, 2D contour and
//! pocket milling of planar regions, G-code output through declarative post processors, and a
//! G-code reader that checks what was written against the toolpaths it was written from.
//!
//! Everything here works in a setup's work coordinates: millimetres, tool axis along +Z, feeds
//! in mm/min. The crate knows nothing of documents or the B-rep kernel; the engine hands it
//! regions. Machine presets, an MTConnect plugin host, FOCAS/THINC protocol stubs, and
//! Inspection Plus review snippets sit beside the mill posts: they do not store toolpaths on
//! the design.
//!
//! The region offsetting is our own: every boundary piece is offset, corners that open away
//! from the offset side get a joining arc, the pieces are cut where they cross, and a piece is
//! kept when its midpoint is no closer to the boundary than the offset distance. Lines and
//! arcs stay lines and arcs, so walls come out as straight moves and true arcs.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod check;
mod focas;
mod gcode;
mod machine;
mod mtconnect;
mod offset;
mod ops;
mod path;
mod piece;
mod post;
mod probe;
mod readback;
mod thinc;
mod tool;

pub use check::verify;
pub use focas::FocasBackend;
pub use gcode::{Operation, Program, emit, parse, work_offset_code};
pub use machine::{
    Availability, FakeMachineHost, MachineAxes, MachineBackend, MachineBackendDescriptor, MachineError, MachineHost, MachineIntegration,
    MachineIntegrationKind, MachineLiveStatus, MachineProfile, MachineResult, ProbeCapability, ProbeMacroStyle, SpindleTaper, ToolChanger,
    ToolChangerKind, preset_machines,
};
pub use mtconnect::{FixedHttpGet, HttpGet, MtConnectBackend, StdHttpGet, parse_mtconnect_current, sample_haas_current_xml};
pub use offset::offset_regions;
pub use ops::{ContourParams, Cut, Feeds, Heights, PocketParams, Side, contour, pocket};
pub use path::{Move, PathStats, Toolpath, arc_sweep};
pub use post::{CommentStyle, LineNumbers, Post, Units};
pub use probe::{ProbeCycle, ProbeSnippetRequest, emit_snippet, style_for_post};
pub use readback::{Readback, allowed_deviation, readback};
pub use thinc::ThincBackend;
pub use tool::{Tool, ToolKind, cantilever_stiffness_n_per_um, tooth_passing_hz, wall_stiffness_n_per_um, youngs_modulus_gpa};

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
