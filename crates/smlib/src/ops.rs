//! SMLib's modelling operations: booleans, fillet, chamfer and shell. Each returns a new body
//! and leaves its inputs unchanged.
//!
//! Edges and faces are chosen by points, never by SMLib's internal order: each point selects the
//! edge (face) nearest to it, which must lie within `tolerance` of it and be nearer than any other
//! by more than the body's modelling tolerance ([`Error::NoMatch`], [`Error::Ambiguous`]).

use crate::{Brep, Error, Result, error, ffi, init};

/// How [`Brep::boolean`] combines two bodies.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BooleanOp {
    Union,
    /// The first body minus the second.
    Subtract,
    Intersect,
}

/// Most points one selection may hold (the C API's limit).
pub const MAX_POINTS: usize = 4096;

fn pick_error((s, at): ffi::ops::PickError) -> Error {
    match (s, at) {
        (ffi::ops::ERR_PICK_NONE, Some(point)) => Error::NoMatch { point },
        (ffi::ops::ERR_PICK_AMBIGUOUS, Some(point)) => Error::Ambiguous { point },
        (s, _) => error(s),
    }
}

impl Brep {
    /// `self` and `other` combined by `op`: `None` when the result is empty (disjoint bodies
    /// intersected, a body subtracted by one that encloses it). Several separate solids come back
    /// as one body, counted by [`Brep::solid_count`]. Tagged operands ([`Brep::tagged`]) give a
    /// result whose [`Brep::history`] says where each face and edge came from.
    pub fn boolean(&self, other: &Brep, op: BooleanOp) -> Result<Option<Brep>> {
        init()?;
        let op = match op {
            BooleanOp::Union => ffi::ops::BOOLEAN_UNION,
            BooleanOp::Subtract => ffi::ops::BOOLEAN_SUBTRACT,
            BooleanOp::Intersect => ffi::ops::BOOLEAN_INTERSECT,
        };
        match ffi::ops::boolean(&self.0, &other.0, op) {
            Ok(h) => Ok(Some(Brep(h))),
            Err(ffi::ERR_EMPTY) => Ok(None),
            Err(s) => Err(error(s)),
        }
    }

    /// The number of separate solid regions (voids not counted).
    pub fn solid_count(&self) -> Result<usize> {
        ffi::ops::solid_count(&self.0).map(|n| usize::try_from(n).unwrap_or(0)).map_err(error)
    }

    /// The edges nearest to `edges` (each within `tolerance`), with their tangent chains, rounded
    /// at `radius`.
    pub fn fillet(&self, edges: &[[f64; 3]], tolerance: f64, radius: f64) -> Result<Brep> {
        self.pick_op(ffi::ops::Pick::Fillet, edges, tolerance, radius, false)
    }

    /// The edges nearest to `edges` (each within `tolerance`), with their tangent chains, cut by
    /// an equal-distance chamfer `distance` back along each face (measured at each edge's
    /// midpoint), with bevelled corners.
    pub fn chamfer(&self, edges: &[[f64; 3]], tolerance: f64, distance: f64) -> Result<Brep> {
        self.pick_op(ffi::ops::Pick::Chamfer, edges, tolerance, distance, false)
    }

    /// Hollowed to a wall of `thickness` (negative: inside the original surface, positive:
    /// outside), open at the faces nearest to `open_faces` (each within `tolerance`); none gives
    /// a closed hollow body.
    pub fn shell(&self, open_faces: &[[f64; 3]], tolerance: f64, thickness: f64) -> Result<Brep> {
        self.pick_op(ffi::ops::Pick::Shell, open_faces, tolerance, thickness, true)
    }

    fn pick_op(&self, kind: ffi::ops::Pick, points: &[[f64; 3]], tolerance: f64, size: f64, allow_none: bool) -> Result<Brep> {
        init()?;
        if points.len() > MAX_POINTS || (points.is_empty() && !allow_none) {
            return Err(Error::Argument);
        }
        ffi::ops::pick_op(kind, &self.0, points, tolerance, size).map(Brep).map_err(pick_error)
    }
}

#[cfg(test)]
#[path = "ops_tests.rs"]
mod tests;
