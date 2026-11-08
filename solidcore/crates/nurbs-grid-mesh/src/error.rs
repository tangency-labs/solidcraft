//! Why a face was not meshed.

use std::fmt;

/// Why a surface was refused or a face was not meshed. Library code returns one of these instead
/// of panicking on input-derived data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MeshError {
    /// The surface's arrays disagree in size, a knot vector decreases or is empty over its
    /// domain, a value is not finite, or a weight is not positive. Names the check that failed.
    BadSurface(&'static str),
    /// A mesh option is out of range (the tolerance must be finite and positive).
    BadOptions(&'static str),
    /// No boundary loop with at least three points.
    NoLoops,
    /// A loop that still crosses no grid line after a line was put through it: its extent in
    /// (u, v) is below the rounding band, so it encloses nothing.
    LoopInsideCell,
    /// The loop pieces inside cell `(i, j)` did not close into polygons: the loops cross
    /// themselves or each other there, or run outside the grid.
    OpenCell {
        /// The cell's index along u.
        i: usize,
        /// The cell's index along v.
        j: usize,
    },
    /// More vertices than a `u32` index can name.
    TooLarge,
    /// A planar face could not be triangulated. The message names the failing loop.
    BadPlanar(String),
    /// An index fell outside an array. A bug in this crate, reported instead of panicking.
    Internal,
}

impl fmt::Display for MeshError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadSurface(what) => write!(f, "bad surface: {what}"),
            Self::BadOptions(what) => write!(f, "bad options: {what}"),
            Self::NoLoops => write!(f, "no boundary loop with at least three points"),
            Self::LoopInsideCell => write!(f, "a boundary loop encloses no area in (u, v)"),
            Self::OpenCell { i, j } => write!(f, "boundary pieces in cell ({i}, {j}) do not close"),
            Self::TooLarge => write!(f, "more vertices than a u32 index can name"),
            Self::BadPlanar(message) => write!(f, "planar triangulation: {message}"),
            Self::Internal => write!(f, "internal index error"),
        }
    }
}

impl std::error::Error for MeshError {}
