//! Bodies handed across in memory, as loose topology on exact geometry ([`ExactBody`]): what a
//! kernel's adapter gives the router and takes from it in place of a file, so no body is written
//! out as STEP and read back to cross (`docs/design/kernel-layer.md` §3).
//!
//! The record (`solidcore_record::brep::Brep`) stays the body format. An exact body is what a
//! record is assembled from: vertices, edges on their curves, faces on their surfaces with their
//! bounds, and shells, in the producer's own order, with the record's own curve, surface and
//! pcurve kinds. What only the record has (its canonical order, the radial order of an edge's
//! uses, each round face's NURBS form, tolerances measured on the geometry, seams on closed
//! surfaces) is made when solidcore assembles it, by the code that assembles every body read from
//! a file. Pcurves are optional: a use without one has it made, in closed form where the geometry
//! has one and fitted where it has none, and a producer that gives the ones it holds spares the
//! fitting.
//!
//! An exact body leaves a kernel with the history of the operation that made it in its own
//! indices ([`ExactExported`]); the router renumbers it into the record's ([`ExactHistory`]), so
//! an adapter never needs the record's order. Each face and edge also carries the producer's own
//! id, given back by record index, so an adapter can find its own entities in the record.

use serde::{Deserialize, Serialize};
use solidcore_record::brep::history::{EdgeOrigin, FaceProv};
use solidcore_record::brep::{Curve, Curve2, Surface};

/// A body as loose topology on exact geometry, in the producer's order. Millimetres and radians.
///
/// Indices are positions in this body's own lists. A body of several shells says how they make
/// regions by its [`ExactKind`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExactBody {
    /// The body's linear tolerance (mm): the record's, and every vertex's and edge's to start
    /// with. Where the geometry has a larger gap, the vertex or edge is given that gap.
    pub tolerance: f64,
    /// The points.
    pub vertices: Vec<[f64; 3]>,
    /// The edges.
    pub edges: Vec<ExactEdge>,
    /// The faces.
    pub faces: Vec<ExactFace>,
    /// The shells: each a list of faces (indices into [`ExactBody::faces`]), each face used on
    /// the side its [`ExactFace::forward`] says.
    pub shells: Vec<Vec<u32>>,
    /// How the shells make regions.
    pub kind: ExactKind,
}

/// How an [`ExactBody`]'s shells make regions.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExactKind {
    /// One closed shell, bounding the material.
    Solid,
    /// A solid with cavities: the first shell is the outer one, each other bounds a void. All
    /// closed.
    Voids,
    /// Shells apart: each closed shell bounds a solid of its own, and each open one is a sheet.
    Shells,
}

/// A bounded piece of a curve.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExactEdge {
    /// The producer's own id for the edge, given back by record index ([`ExactAssembled`]).
    pub id: u64,
    /// The curve, in its own parameter.
    pub curve: Curve,
    /// The edge's interval on the curve, min < max, from its start vertex (at min) to its end;
    /// `None` where it is where the vertices lie on the curve (the whole of a closed curve where
    /// both are one vertex).
    pub interval: Option<[f64; 2]>,
    /// `[start, end]` (indices into [`ExactBody::vertices`]); equal for a closed edge.
    pub vertices: [u32; 2],
}

/// A bounded piece of a surface.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExactFace {
    /// The producer's own id for the face, given back by record index ([`ExactAssembled`]).
    pub id: u64,
    /// The surface, in its own (u, v). A surface the record holds only with a net of its own
    /// (a ball patch, a blend by its definition) is given as that net.
    pub surface: Surface,
    /// The face's normal is the surface's natural normal (∂S/∂u × ∂S/∂v). In a closed shell the
    /// normals point out of the material.
    pub forward: bool,
    /// Its bounds, each relative to the surface's natural normal: seen from that normal, the
    /// outer bound runs counter-clockwise and inner ones clockwise.
    pub bounds: Vec<ExactBound>,
    /// The outer bound's index in [`ExactFace::bounds`], where the producer knows it; otherwise
    /// the pcurves decide.
    pub outer: Option<usize>,
}

/// A face's bound.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExactBound {
    /// A closed chain of edge uses, in traversal order.
    Edges {
        /// The uses.
        uses: Vec<ExactUse>,
    },
    /// One vertex: a pole, or an apex.
    Vertex {
        /// The vertex.
        vertex: u32,
    },
}

/// One use of an edge in a face's bound.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExactUse {
    /// The edge (an index into [`ExactBody::edges`]).
    pub edge: u32,
    /// The bound runs along the edge from its start vertex to its end.
    pub forward: bool,
    /// The edge in the face's (u, v), on the edge's interval and running min → max whatever
    /// `forward` says, where the producer holds it exactly; `None` where it doesn't, and the
    /// pcurve is made. A use whose pcurve has a closed form keeps that. On a face whose surface
    /// has its own NURBS form in the record (a cylinder, cone, sphere, torus or revolution) the
    /// record's are on that form, and made in closed form whatever is given here.
    pub pcurve: Option<Curve2>,
}

/// The history of the operation that made an exact body, in its own indices: as the record's
/// history (`solidcore_record::brep::history::History`), its origins on the operation's
/// operands in the operands' record indices.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExactHistory {
    /// How many operands the operation had.
    pub operands: usize,
    /// Where each face came from, by [`ExactBody::faces`] index. Where the record makes several
    /// faces of one (a face split at a seam), each is a split of what it was.
    pub faces: Vec<FaceProv>,
    /// Where each edge came from, by [`ExactBody::edges`] index. The faces an edge separates are
    /// the record's, so they are not given. An edge the record makes (a seam on a closed
    /// surface) lies within one face and comes from nothing.
    pub edges: Vec<Vec<EdgeOrigin>>,
}

/// A body as it leaves a kernel in memory: the exact body, and the operation's history over its
/// operands in the exact body's indices (`None` where the kernel can't report it exactly).
#[derive(Clone, Debug)]
pub struct ExactExported {
    /// The body.
    pub body: ExactBody,
    /// The operation's history, by the body's own indices.
    pub history: Option<ExactHistory>,
}

/// Where each item of a record assembled from an [`ExactBody`] came from, by record index.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ExactAssembled {
    /// Each record face's face in the exact body (an index into [`ExactBody::faces`]). A face
    /// the record split (at a seam) is listed for each of its pieces; a face no wider than the
    /// tolerance may be left out.
    pub faces: Vec<usize>,
    /// Each record edge's edge in the exact body, or `None` for one the record made (a seam):
    /// `None` for all of them where the record's edges could not be followed.
    pub edges: Vec<Option<usize>>,
}
