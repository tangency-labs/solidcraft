//! SMLib's modelling operations: booleans, fillet, chamfer, shell, offset faces and thicken.
//! Each returns a new body
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

/// What [`Brep::blend`] makes of each selected edge.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Blend {
    /// A circular cross-section at a constant radius ([`Brep::fillet`]).
    Round,
    /// A curvature-continuous (G2) cross-section between the contact lines of the round of
    /// that radius.
    Curvature,
    /// A circular cross-section of constant width from contact line to contact line: the size
    /// is that width.
    Chord,
    /// An equal-distance chamfer ([`Brep::chamfer`]): the size is the setback along each face.
    Chamfer,
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

    /// `self` with each face `tool` meets cut along the intersection, the pieces kept as faces of
    /// one body: `None` when the tool meets no face. Tagged operands give the pieces' history
    /// (each piece is a split of the face it was cut from).
    pub fn imprint(&self, tool: &Brep) -> Result<Option<Brep>> {
        init()?;
        match ffi::ops::imprint(&self.0, &tool.0) {
            Ok(h) => Ok(Some(Brep(h))),
            Err(ffi::ops::ERR_NO_CUT) => Ok(None),
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

    /// The edges nearest to `edges` (each within `tolerance`), with their tangent chains, blended
    /// as `kind`, edge i (and its chain) at `sizes[i]`.
    pub fn blend(&self, kind: Blend, edges: &[[f64; 3]], sizes: &[f64], tolerance: f64) -> Result<Brep> {
        init()?;
        if edges.len() > MAX_POINTS || edges.is_empty() || sizes.len() != edges.len() {
            return Err(Error::Argument);
        }
        let kind = match kind {
            Blend::Round => ffi::ops::BLEND_ROUND,
            Blend::Curvature => ffi::ops::BLEND_CURVATURE,
            Blend::Chord => ffi::ops::BLEND_CHORD,
            Blend::Chamfer => ffi::ops::BLEND_CHAMFER,
        };
        ffi::ops::blend(kind, &self.0, edges, sizes, tolerance).map(Brep).map_err(pick_error)
    }

    /// The one edge nearest to `edge` (within `tolerance`, no tangent chain) rounded with a radius
    /// that varies along it: a clamped B-spline of `degree` (1 to 3) with `knots` and control
    /// `radii` over the position along the edge, 0 at its end nearest `start` and 1 at the other
    /// (the edge curve's parameter scaled to 0..1).
    pub fn fillet_variable(&self, edge: [f64; 3], tolerance: f64, start: [f64; 3], degree: usize, knots: &[f64], radii: &[f64]) -> Result<Brep> {
        init()?;
        let degree = i32::try_from(degree).map_err(|_| Error::Argument)?;
        ffi::ops::fillet_variable(&self.0, &edge, tolerance, &start, degree, knots, radii).map(Brep).map_err(|s| match s {
            ffi::ops::ERR_PICK_NONE => Error::NoMatch { point: 0 },
            ffi::ops::ERR_PICK_AMBIGUOUS => Error::Ambiguous { point: 0 },
            s => error(s),
        })
    }

    /// Hollowed to a wall of `thickness` (negative: inside the original surface, positive:
    /// outside), open at the faces nearest to `open_faces` (each within `tolerance`); none gives
    /// a closed hollow body. An inward wall that leaves no room inside is [`Error::NoRoom`].
    pub fn shell(&self, open_faces: &[[f64; 3]], tolerance: f64, thickness: f64) -> Result<Brep> {
        self.pick_op(ffi::ops::Pick::Shell, open_faces, tolerance, thickness, true)
    }

    /// The solid with the faces nearest to `faces` (each within `tolerance`) moved `distance`
    /// along their outward normals (negative: into the body), onto their surfaces' offsets, the
    /// faces around them extended or trimmed to meet them. Faces moved in past all of the body
    /// behind them are [`Error::NoRoom`].
    pub fn offset_faces(&self, faces: &[[f64; 3]], tolerance: f64, distance: f64) -> Result<Brep> {
        self.pick_op(ffi::ops::Pick::OffsetFaces, faces, tolerance, distance, false)
    }

    /// A sheet thickened into a solid by `thickness` along its surfaces' normals (negative:
    /// against them), or by half each way when `symmetric`, with ruled walls along its open
    /// edges. A closed solid is [`Error::Argument`].
    pub fn thicken(&self, thickness: f64, symmetric: bool) -> Result<Brep> {
        init()?;
        ffi::ops::thicken(&self.0, thickness, symmetric).map(Brep).map_err(error)
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
