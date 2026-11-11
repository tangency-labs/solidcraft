//! Manufacturing (CAM) foundation for SolidCraft: cutting tools, toolpaths, 2D contour and
//! pocket milling of planar regions, G-code output through declarative post processors, and a
//! G-code reader that checks what was written against the toolpaths it was written from.
//!
//! Everything here works in a setup's work coordinates: millimetres, feeds in mm/min. The 2D
//! operations have the tool axis along +Z. The crate knows nothing of documents or the B-rep
//! kernel; the engine hands it regions and surfaces.
//!
//! Beyond 2D: [`Surface`] holds a part's triangles, and the drop-cutter ([`drop_z`],
//! [`drop_along`]) lowers a flat, ball, bull nose or tapered (chamfer) [`Cutter`] onto them
//! along any axis, stopping where it first touches. A tip placed there cannot cut into the
//! surface, which is what the 3-axis surface operations ([`surface_ops`]) are built on: raster
//! finishing, layered roughing of a stock box and z-level finishing of steep walls. The 3D
//! finishing strategies ([`finish3d`]) place their passes by the shape of the part: constant
//! scallop, steep and shallow, pencil and rest finishing. Long planning can be stopped from
//! another thread ([`stop`]).
//!
//! Multi-axis toolpaths ([`Toolpath5`]) give the tool tip with the direction of the tool axis.
//! Machine kinematics ([`kinematics`]) turn them into the positions of a machine's linear and
//! rotary axes, for machines with up to two rotaries on the table or the head. Indexed (3+1 and
//! 3+2) work ([`indexed`]) runs a 3-axis toolpath in a tilted work plane, with the rotaries held
//! still.
//!
//! The region offsetting is our own: every boundary piece is offset, corners that open away
//! from the offset side get a joining arc, the pieces are cut where they cross, and a piece is
//! kept when its midpoint is no closer to the boundary than the offset distance. Lines and
//! arcs stay lines and arcs, so walls come out as straight moves and true arcs.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod chamfer;
mod check;
mod cutter;
mod dropcutter;
pub mod finish3d;
mod frame;
mod gcode;
pub mod indexed;
pub mod kinematics;
mod multiaxis;
mod offset;
mod ops;
mod par;
mod path;
mod piece;
mod post;
mod readback;
pub mod stop;
mod surface;
pub mod surface_ops;
mod tool;

pub use check::{lift_entries, verify};
pub use cutter::{Cone, Cutter};
pub use dropcutter::{drop_along, drop_triangle, drop_z};
pub use finish3d::{
    OffsetDirection, PencilParams, RestParams, ScallopParams, ShallowPattern, Spacing, SteepShallowParams, pencil, rest_finish, scallop,
    steep_shallow,
};
pub use frame::{Frame, angle_about, rotate, rotate_about};
pub use gcode::{Operation, Program, emit, parse, work_offset_code};
pub use multiaxis::{Move5, PathStats5, Pose, Toolpath5, slerp, verify5};
pub use offset::offset_regions;
pub use ops::{ContourParams, Cut, Feeds, Heights, PocketParams, Side, contour, pocket};
pub use path::{Move, PathStats, Toolpath, arc_sweep};
pub use post::{CommentStyle, LineNumbers, Post, Units};
pub use readback::{Readback, allowed_deviation, readback};
pub use surface::{Surface, barycentric, nearest_on_triangle, ray_triangle};
pub use surface_ops::{
    Boundary, DEFAULT_DESCENT_LIMIT, ENTRY_GAP, MAX_GRID_NODES, MAX_SAMPLES, ParallelParams, RoughingParams, WaterlineParams, parallel, roughing,
    waterline,
};
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
    /// The work was asked to stop ([`stop`]) before it was done: nothing was planned.
    #[error("stopped before it was done")]
    Stopped,
}

pub type Result<T> = std::result::Result<T, CamError>;

pub(crate) fn invalid<T>(why: impl Into<String>) -> Result<T> {
    Err(CamError::Invalid(why.into()))
}

#[cfg(test)]
mod finish3d_tests;
#[cfg(test)]
mod foundation_tests;
#[cfg(test)]
mod kinematics_tests;
#[cfg(test)]
mod surface_ops_tests;
#[cfg(test)]
mod tests;
