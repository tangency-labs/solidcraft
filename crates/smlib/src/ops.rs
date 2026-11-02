//! SMLib's modelling operations: booleans, fillet, chamfer, shell, offset faces and thicken.
//! Each returns a new body
//! and leaves its inputs unchanged.
//!
//! Edges and faces are chosen by points, never by SMLib's internal order: each point selects the
//! edge (face) nearest to it, which must lie within `tolerance` of it and be nearer than any other
//! by more than the body's modelling tolerance ([`Error::NoMatch`], [`Error::Ambiguous`]).

use crate::{Brep, Error, Result, core};

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

/// The second side of [`Brep::chamfer_sides`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ChamferSecond {
    /// The setback along the other face.
    Distance(f64),
    /// The angle (radians) the chamfer makes with the first face.
    Angle(f64),
}

/// Most bodies [`Brep::union_all`] takes.
pub const MAX_UNION_BREPS: usize = 100_000;

/// Most points one selection may hold.
pub const MAX_POINTS: usize = 4096;

/// One edge round an opened corner's hole ([`Brep::blend_setback_open`]): its index in the
/// sheet's edge order, the face beside it in its face order, and whether that face, run
/// counter-clockwise about its outward normal, runs the edge from its start vertex to its end.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HoleEdge {
    pub edge: usize,
    pub face: usize,
    pub forward: bool,
}

/// The edges round an opened corner's hole, in order: each starts where the one before it ends.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hole {
    pub edges: Vec<HoleEdge>,
}

impl Brep {
    /// `self` and `other` combined by `op`: `None` when the result is empty (disjoint bodies
    /// intersected, a body subtracted by one that encloses it). Several separate solids come back
    /// as one body, counted by [`Brep::solid_count`]. Tagged operands ([`Brep::tagged`]) give a
    /// result whose [`Brep::history`] says where each face and edge came from.
    pub fn boolean(&self, other: &Brep, op: BooleanOp) -> Result<Option<Brep>> {
        core::boolean::boolean(self.core(), other.core(), op).map(|r| r.map(Brep::made_core))
    }

    /// The union of `breps` in one call: they are unioned pairwise, level by level, so none takes
    /// part in more than about log2(n) unions. The result is as [`Brep::boolean`]'s; operands
    /// tagged each with its own number ([`Brep::tagged`]) give a result whose [`Brep::history`]
    /// names them. At least one body and at most [`MAX_UNION_BREPS`].
    pub fn union_all(breps: &[&Brep]) -> Result<Brep> {
        if breps.is_empty() || breps.len() > MAX_UNION_BREPS {
            return Err(Error::Argument);
        }
        let bodies: Vec<&core::Body> = breps.iter().map(|b| b.core()).collect();
        core::boolean::union_all(&bodies).map(Brep::made_core)
    }

    /// `self` and `other`, sheets of planar faces on one plane ([`Brep::planar_faces`]), combined
    /// as regions of that plane by `op`: `None` when the result is empty. The result's edges are
    /// the inputs' own curves, split where they cross.
    pub fn boolean2d(&self, other: &Brep, op: BooleanOp) -> Result<Option<Brep>> {
        core::boolean::boolean2d(self.core(), other.core(), op).map(|r| r.map(Brep::made_core))
    }

    /// `self` with each face `tool` meets cut along the intersection, the pieces kept as faces of
    /// one body: `None` when the tool meets no face. Tagged operands give the pieces' history
    /// (each piece is a split of the face it was cut from).
    pub fn imprint(&self, tool: &Brep) -> Result<Option<Brep>> {
        core::boolean::imprint(self.core(), tool.core()).map(|r| r.map(Brep::made_core))
    }

    /// The number of separate solid regions (voids not counted).
    pub fn solid_count(&self) -> Result<usize> {
        crate::core::solid_count(self.core())
    }

    /// The edges nearest to `edges` (each within `tolerance`), exactly those, rounded
    /// at `radius`.
    pub fn fillet(&self, edges: &[[f64; 3]], tolerance: f64, radius: f64) -> Result<Brep> {
        self.core_blend(Blend::Round, edges, &[radius], tolerance, &[], &[], core::blend::Corners::Filled)
    }

    /// The edges nearest to `edges` (each within `tolerance`), exactly those, cut by
    /// an equal-distance chamfer `distance` back along each face (measured at each edge's
    /// midpoint), with bevelled corners.
    pub fn chamfer(&self, edges: &[[f64; 3]], tolerance: f64, distance: f64) -> Result<Brep> {
        self.core_blend(Blend::Chamfer, edges, &[distance], tolerance, &[], &[], core::blend::Corners::Filled)
    }

    /// The edges nearest to `edges` (each within `tolerance`), exactly those, blended
    /// as `kind`, edge i at `sizes[i]`.
    pub fn blend(&self, kind: Blend, edges: &[[f64; 3]], sizes: &[f64], tolerance: f64) -> Result<Brep> {
        if edges.len() > MAX_POINTS || edges.is_empty() || sizes.len() != edges.len() {
            return Err(Error::Argument);
        }
        self.core_blend(kind, edges, sizes, tolerance, &[], &[], core::blend::Corners::Filled)
    }

    /// [`Brep::blend`], with whether each point picked an edge that a larger round, overflowing
    /// its faces, removed whole: that edge is gone and not rounded (SMLib rounds the edges whose
    /// blends overflow first, then what is left of the others).
    pub fn blend_dropping(&self, kind: Blend, edges: &[[f64; 3]], sizes: &[f64], tolerance: f64) -> Result<(Brep, Vec<bool>)> {
        if edges.len() > MAX_POINTS || edges.is_empty() || sizes.len() != edges.len() {
            return Err(Error::Argument);
        }
        core::blend::blend_dropping(self.core(), kind, edges, sizes, tolerance).map(|(b, d)| (Brep::made_core(b), d))
    }

    /// [`Brep::blend`] with corner setbacks: each of `corners` picks the vertex nearest it
    /// (within `tolerance`), a corner where every edge is blended, and that corner's patch starts
    /// `setbacks[j]` along each of its edges from the vertex (one distance per corner, as SMLib
    /// keeps it). A corner SMLib builds no setback on (some edge there not blended, a chamfer) is
    /// [`Error::Unsupported`]; a setback as long as an edge there, [`Error::NoRoom`].
    pub fn blend_setback(
        &self,
        kind: Blend,
        edges: &[[f64; 3]],
        sizes: &[f64],
        corners: &[[f64; 3]],
        setbacks: &[f64],
        tolerance: f64,
    ) -> Result<Brep> {
        if edges.len() > MAX_POINTS
            || edges.is_empty()
            || sizes.len() != edges.len()
            || corners.is_empty()
            || corners.len() > MAX_POINTS
            || setbacks.len() != corners.len()
        {
            return Err(Error::Argument);
        }
        self.core_blend(kind, edges, sizes, tolerance, corners, setbacks, core::blend::Corners::Filled)
    }

    /// [`Brep::blend_setback`] with each set-back corner left open: SMLib's patch between the
    /// corner's setback sections is removed, so the result is a sheet whose only free edges are
    /// the holes', one [`Hole`] per corner in `corners`' order (a vertex picked twice gives its
    /// hole twice). A corner whose patch SMLib does not build as a disc of faces of its own is
    /// [`Error::Unsupported`].
    pub fn blend_setback_open(
        &self,
        kind: Blend,
        edges: &[[f64; 3]],
        sizes: &[f64],
        corners: &[[f64; 3]],
        setbacks: &[f64],
        tolerance: f64,
    ) -> Result<(Brep, Vec<Hole>)> {
        if edges.len() > MAX_POINTS
            || edges.is_empty()
            || sizes.len() != edges.len()
            || corners.is_empty()
            || corners.len() > MAX_POINTS
            || setbacks.len() != corners.len()
        {
            return Err(Error::Argument);
        }
        core::blend::blend_holes(self.core(), kind, edges, sizes, tolerance, corners, setbacks, core::blend::Corners::Open)
            .map(|(b, holes)| (Brep::made_core(b), holes))
    }

    /// The one edge nearest to `edge` (within `tolerance`) rounded with a radius
    /// that varies along it: a clamped B-spline of `degree` (1 to 3) with `knots` and control
    /// `radii` over the position along the edge, 0 at its end nearest `start` and 1 at the other
    /// (the edge curve's parameter scaled to 0..1).
    pub fn fillet_variable(&self, edge: [f64; 3], tolerance: f64, start: [f64; 3], degree: usize, knots: &[f64], radii: &[f64]) -> Result<Brep> {
        core::blend::fillet_variable(self.core(), edge, tolerance, start, degree, knots, radii).map(Brep::made_core)
    }

    /// The edges nearest to `edges` (each within `tolerance`), exactly those, cut by
    /// a chamfer `distance` back along the face whose outward normal leans most along `first`,
    /// and by `second` on the other, with bevelled corners (setbacks measured at each edge's
    /// midpoint).
    pub fn chamfer_sides(&self, edges: &[[f64; 3]], tolerance: f64, distance: f64, second: ChamferSecond, first: [f64; 3]) -> Result<Brep> {
        if edges.is_empty() || edges.len() > MAX_POINTS {
            return Err(Error::Argument);
        }
        core::blend::chamfer_sides(self.core(), edges, tolerance, distance, second, first).map(Brep::made_core)
    }

    /// Hollowed to a wall of `thickness` (negative: inside the original surface, positive:
    /// outside), open at the faces nearest to `open_faces` (each within `tolerance`); none gives
    /// a closed hollow body. An inward wall that leaves no room inside is [`Error::NoRoom`]; one
    /// whose walls would just meet (a hole exactly twice the wall from a side) is
    /// [`Error::WallsTouch`].
    pub fn shell(&self, open_faces: &[[f64; 3]], tolerance: f64, thickness: f64) -> Result<Brep> {
        core::offset::shell(self.core(), open_faces, tolerance, thickness).map(Brep::made_core)
    }

    /// The solid with the faces nearest to `faces` (each within `tolerance`) moved `distance`
    /// along their outward normals (negative: into the body), onto their surfaces' offsets, the
    /// faces around them extended or trimmed to meet them. Faces moved in past all of the body
    /// behind them are [`Error::NoRoom`].
    pub fn offset_faces(&self, faces: &[[f64; 3]], tolerance: f64, distance: f64) -> Result<Brep> {
        core::offset::offset_faces(self.core(), faces, tolerance, distance).map(Brep::made_core)
    }

    /// A sheet thickened into a solid by `thickness` along its surfaces' normals (negative:
    /// against them), or by half each way when `symmetric`, with ruled walls along its open
    /// edges. A closed solid is [`Error::Argument`].
    pub fn thicken(&self, thickness: f64, symmetric: bool) -> Result<Brep> {
        core::offset::thicken(self.core(), thickness, symmetric).map(Brep::made_core)
    }

    /// This body with its topological edges and vertices removed (SMLib's
    /// `SmBrep::RemoveTopologicalEdgesAndVertices`): faces on one surface meeting along an edge
    /// become one, and two edges on one curve meeting at a vertex nothing else uses become one.
    /// The edges nearest to `keep` (each within `tolerance`) stay. The shape is unchanged, and
    /// SMLib's history tags follow the merge.
    pub fn merge_faces(&self, keep: &[[f64; 3]], tolerance: f64) -> Result<Brep> {
        if keep.len() > MAX_POINTS {
            return Err(Error::Argument);
        }
        core::merge::merge_faces(self.core(), keep, tolerance).map(Brep::made_core)
    }

    /// The flat faces nearest to `faces` (each within `tolerance`) tilted by `angle` radians
    /// (0 < |angle| < 89.94 degrees) about the line where each meets the neutral plane through
    /// `neutral_origin` with normal `neutral_normal`, a positive angle leaning them in going along
    /// `pull`; the faces around them extended or trimmed to meet them. A face that isn't flat, or
    /// isn't parallel to `pull`, is [`Error::Unsupported`].
    pub fn draft(
        &self,
        faces: &[[f64; 3]],
        tolerance: f64,
        neutral_origin: [f64; 3],
        neutral_normal: [f64; 3],
        pull: [f64; 3],
        angle: f64,
    ) -> Result<Brep> {
        if faces.is_empty() || faces.len() > MAX_POINTS {
            return Err(Error::Argument);
        }
        core::offset::draft(self.core(), faces, tolerance, neutral_origin, neutral_normal, pull, angle).map(Brep::made_core)
    }

    /// [`Brep::blend`] and its kin on solidcore (`core::blend::blend`).
    #[allow(clippy::too_many_arguments)]
    fn core_blend(
        &self,
        kind: Blend,
        edges: &[[f64; 3]],
        sizes: &[f64],
        tolerance: f64,
        corners: &[[f64; 3]],
        setbacks: &[f64],
        open: core::blend::Corners,
    ) -> Result<Brep> {
        if edges.len() > MAX_POINTS || edges.is_empty() {
            return Err(Error::Argument);
        }
        core::blend::blend(self.core(), kind, edges, sizes, tolerance, corners, setbacks, open).map(Brep::made_core)
    }
}

#[cfg(test)]
#[path = "ops_tests.rs"]
mod tests;
