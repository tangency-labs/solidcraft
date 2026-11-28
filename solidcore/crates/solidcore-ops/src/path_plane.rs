//! The plane a sweep path lies in, within an angle, for a pipe or a pattern along a path whose
//! pieces are a body's edges or a sketch's curves (#16084). Solidcore decides it
//! ([`solidcore::query::path_plane::path_plane_normal`]); ops only passes the application's
//! plain numbers through.

pub use solidcore::query::path_plane::{PathPlanePiece, PieceFit};

/// The unit normal of a plane every piece of the path lies in, within `tol` radians: the axis of
/// its first arc, or else the turn between its first two directions of travel that are not
/// parallel, checked against every piece over its whole length (a free-form span by its control
/// points). A straight path gets one of the planes through its line. `None` for a path that
/// leaves every plane, has no direction, or has no pieces.
#[must_use]
pub fn plane_normal(pieces: &[PathPlanePiece], tol: f64) -> Option<[f64; 3]> {
    solidcore::query::path_plane::path_plane_normal(pieces, tol)
}
