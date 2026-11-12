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
//! still. [`gcode5`] writes such toolpaths, continuous and indexed, as G-code for a 3-axis
//! machine with a rotary `A` axis, and [`readback5`] reads such a program back through the
//! machine's kinematics and holds it against the toolpaths it was written from.
//!
//! Collision checks ([`collision`]) work on any multi-axis toolpath: where the cutter cuts
//! into the part deeper than a tolerance (a gouge, measured square to the surface), and where
//! the tool's shank or holder ([`Tool::envelopes`]) comes closer to it than a clearance. A
//! toolpath whose holder would touch can be tilted clear ([`collision::tilt_away`]).
//!
//! On a 4th axis ([`rotary`]) the rotary axis is work X and the tool points at it. A flat
//! toolpath can be wrapped round a cylinder ([`wrap`]), a part turned on the axis can be
//! finished ([`rotary_finish`]) or roughed from round stock ([`rotary_roughing`]), and 2D
//! pockets and contours can be cut into a cylinder in depth levels ([`wrapped`]).
//!
//! Holes are drilled ([`drill`]) in four cycles written as plain moves, and checked against
//! the part with the drill's real point. A chamfer mill cuts chamfers and deburrs the top
//! edges of profiles ([`chamfer()`]), and the gouge check allows it the chamfer it is told to
//! cut ([`band_gouges`]).
//!
//! 2D contours and pockets take leads, entries, linking, holding tabs and finishing passes
//! ([`contour_with`], [`pocket_with`]). A lead into or out of a cut may be a vertical arc
//! ([`PlaneArc`]): [`gcode5`] writes it as a `G18` or `G19` arc, [`emit`] as lines within the
//! arc tolerance, and both readers read such arcs in their own plane. A planned path can be
//! written with fewer moves ([`compact`]: lines and arcs that stay within a tolerance of the
//! moves they replace), and its descent feeds worked out again for the coordinates as written
//! ([`limit_descents`]).
//!
//! Adaptive clearing ([`adaptive`]) clears a region level by level with the tool's engagement
//! (how much of its circumference is in material) held under a limit, for a mill that cannot
//! take a full-width cut. Given what is known of the stock (its extent, its top, a height map
//! of what earlier cuts left), it goes down in the open air beside it and clears only what is
//! still there; 3D adaptive roughing clears a stock box over a part's surface the same way.
//! [`engagement`] measures the engagement along a finished path from the material itself, not
//! from the planner's model, so a path can be checked against it.
//!
//! A job's raw material is a [`stock::Stock`]: a box, or a bar turned on the 4th axis. Facing
//! ([`stock::face`]) cuts its top flat, entering every level from beside it. [`rapids`] reads a
//! written program back through the machine's kinematics and holds every rapid against the
//! part, fixtures and the stock, and every position against the machine's travel.
//!
//! [`machine_time`] times a written program as the machine runs it: every axis held to its
//! own rate, with the accelerations where the machine gives them, spindle starts, tool changes
//! and stops for the operator. A [`setup_sheet`] tells the operator how to set the job up and
//! run it, as a self-contained HTML page or as Markdown.
//!
//! The stock-removal simulation ([`sim`]) is an independent check of a written program: it
//! reads the G-code as the controller runs it, sweeps the tool through a model of the stock
//! along every block, and holds what is left against the part. It reports cuts into the part
//! (gouges, measured in 3D), material left where the program cut, rapids that cut, and cuts at
//! the tool's full width. A 3-axis stock is a heightfield, with lines along X and Y beside it
//! for a part with faces under its own material; a 4th axis's stock is rays from the axis, with
//! the air an indexed cut leaves inside them. It also finds the material left over the whole
//! part. What a simulation finds can be kept as events at their move and place and merged by
//! kind ([`findings`]); the stock and the tool are drawn as meshes for a view ([`stock_mesh`],
//! [`tool_mesh`]).
//!
//! A post ([`Post`]) is data: besides its templates it holds switches for the program's
//! layout (a tool list, comment characters, blank lines, lines at every operation, a safe
//! retract to the machine's top, the tool length offset on the first Z move, modal motion
//! codes), all off by default.
//!
//! The region offsetting is our own: every boundary piece is offset, corners that open away
//! from the offset side get a joining arc, the pieces are cut where they cross, and a piece is
//! kept when its midpoint is no closer to the boundary than the offset distance. Lines and
//! arcs stay lines and arcs, so walls come out as straight moves and true arcs.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

pub mod adaptive;
mod chamfer;
mod check;
pub mod collision;
mod compact;
mod cutter;
pub mod drill;
mod dropcutter;
pub mod engagement;
pub mod findings;
pub mod finish3d;
mod frame;
mod gcode;
pub mod gcode5;
pub mod helical;
pub mod indexed;
pub mod keys;
pub mod kinematics;
mod leads;
pub mod machine_model;
pub mod machine_time;
mod multiaxis;
pub mod obstacle;
mod offset;
mod ops;
mod par;
mod path;
mod piece;
pub mod playback;
mod post;
pub mod rapids;
mod readback;
pub mod readback5;
pub mod rotary;
pub mod setup_sheet;
pub mod sim;
pub mod stock;
pub mod stock_mesh;
pub mod stop;
mod surface;
pub mod surface_ops;
mod tool;
pub mod tool_mesh;
pub mod workholding;
pub mod wrapped;

pub use chamfer::{ChamferGeometry, ChamferParams, DEFAULT_TIP_OFFSET, band_gouges, chamfer, chamfer_geometry};
pub use check::{lift_entries, verify};
pub use compact::{Compaction, compact, limit_descents};
pub use cutter::{Cone, Cutter};
pub use dropcutter::{drop_along, drop_triangle, drop_z};
pub use finish3d::{
    OffsetDirection, PencilParams, RestParams, ScallopParams, ShallowPattern, Spacing, SteepShallowParams, pencil, rest_finish, scallop,
    steep_shallow,
};
pub use frame::{Frame, angle_about, rotate, rotate_about};
pub use gcode::{Operation, Program, emit, parse, work_offset_code};
pub use leads::{
    ContourOptions, Entry, Lead, Linking, MAX_FINISH_PASSES, MAX_TABS, MIN_ARC_RADIUS, PocketOptions, TabLayout, Tabs, contour_with, pocket_with,
    tabs_fit,
};
pub use multiaxis::{Move5, Operation5, PathStats5, Pose, Program5, Toolpath5, slerp, verify5};
pub use offset::offset_regions;
pub use ops::{ContourParams, Cut, Feeds, Heights, PocketParams, Side, contour, pocket};
pub use path::{ArcPlane, Move, PathStats, PlaneArc, Toolpath, arc_sweep};
pub use post::{CommentStyle, LineNumbers, Post, SafeRetract, Units};
pub use readback::{Readback, allowed_deviation, readback};
pub use rotary::{
    DEFAULT_MAX_ANGLE_STEP, RotaryParams, RotaryPattern, RotaryRoughingParams, radial, rotary_finish, rotary_pose, rotary_roughing, wrap,
    wrap_with_step,
};
pub use surface::{Surface, barycentric, nearest_on_triangle, nearest_segments, ray_triangle, segment_triangle_distance};
pub use surface_ops::{
    Boundary, DEFAULT_DESCENT_LIMIT, ENTRY_GAP, MAX_GRID_NODES, MAX_SAMPLES, ParallelParams, RoughingParams, WaterlineParams, parallel, roughing,
    waterline,
};
pub use tool::{Envelope, HolderSection, Tool, ToolKind};

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
mod adaptive_tests;
#[cfg(test)]
mod chamfer_tests;
#[cfg(test)]
mod collision_tests;
#[cfg(test)]
mod drill_tests;
#[cfg(test)]
mod engagement_tests;
#[cfg(test)]
mod finish3d_tests;
#[cfg(test)]
mod foundation_tests;
#[cfg(test)]
mod gcode5_tests;
#[cfg(test)]
mod helical_tests;
#[cfg(test)]
mod kinematics_tests;
#[cfg(test)]
mod leads_tests;
#[cfg(test)]
mod machine_model_tests;
#[cfg(test)]
mod machine_time_tests;
#[cfg(test)]
mod playback_tests;
#[cfg(test)]
mod rapids_tests;
#[cfg(test)]
mod rotary_tests;
#[cfg(test)]
mod setup_sheet_tests;
#[cfg(test)]
mod sim_tests;
#[cfg(test)]
mod stock_tests;
#[cfg(test)]
mod surface_ops_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod workholding_tests;
#[cfg(test)]
mod wrapped_tests;
