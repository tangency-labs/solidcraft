//! The wedge an equal-distance chamfer or a constant round takes off (or, on a concave edge,
//! fills) at a straight edge between two flat faces: for an application that builds the blend
//! running on past the edge's ends (an edge ending on a narrow round or cove the blend is
//! wider than) from the wedge's prism carried on along the edge's line, cut from the part on a
//! convex edge and added to it on a concave one.
//!
//! Not upstream (solidcore's). [`straight_wedge`] decides where that construction applies and
//! measures the wedge: the edge is a straight line between two vertices
//! ([`crate::curve::curve::Curve::is_linear`]), both its faces are planar and hold the edge's
//! line, they are not tangent to each other, and no other edge at either end of it is concave:
//! past each end the part falls away (a narrow round, a cove's edge), it does not rise into a
//! wall the carried-on blend would run under or into another inside corner. Whether the edge
//! is concave comes from [`is_concave_edge`]. The directions into the faces are exact, from
//! the faces' outward normals at the edge's middle ([`outward`]).

use crate::blend::carried_face::{plane_normal, SQUARE};
use crate::blend::chamfer_side::outward;
use crate::blend::fillet_solver::p5::is_concave_edge;
use crate::query::brep::{calculate_bounding_box, BoundingBoxOptions};
use crate::topology::edgeuse::p1 as eu1;
use crate::topology::{edge, vertex, EdgeId, FaceId, VertexId};
use crate::{Brep, Kernel, Point3, Result, Vec3};

/// The least sine between the two faces' normals for the edge to be a corner a chamfer cuts:
/// faces closer to tangent than this give a wedge too thin to cut.
pub const CORNER: f64 = 1e-6;

/// A straight edge between two flat faces, and the directions into each face square to it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StraightWedge {
    /// The edge.
    pub edge: EdgeId,
    /// The edge's start vertex.
    pub start: Point3,
    /// The edge's end vertex.
    pub end: Point3,
    /// The unit direction of the edge, from its start to its end.
    pub along: Vec3,
    /// For each face of the edge, the unit direction in its plane, square to the edge, that
    /// runs from the edge into the face: the way the chamfer's contact line on that face
    /// lies from the edge.
    pub into: [Vec3; 2],
    /// The faces' outward unit normals at the edge's middle, in the order of `into`.
    pub normals: [Vec3; 2],
    /// The faces, in the order of `into`.
    pub faces: [FaceId; 2],
    /// The edge's start and end vertices.
    pub vertices: [VertexId; 2],
    /// Whether the edge is concave: the faces meet in an inside corner, the wedge between
    /// them open air the blend fills; otherwise the wedge is the part's material it takes off.
    pub concave: bool,
}

/// The wedge of `e` ([`StraightWedge`]), or `None` where the construction does not apply: an
/// edge that is not a straight line between two vertices, a face of it that is not planar or
/// does not hold the edge's line, faces that meet (almost) tangentially, or another concave
/// edge at either end of it.
///
/// # Errors
/// The kernel's errors from the topology lookups and evaluations.
pub fn straight_wedge(k: &Kernel, b: &mut Brep, e: EdgeId) -> Result<Option<StraightWedge>> {
    let (Some(s), Some(t)) = (edge::start_vertex(b, e)?, edge::end_vertex(b, e)?) else {
        return Ok(None);
    };
    if s == t {
        return Ok(None);
    }
    let (start, end) = (vertex::point(b, s)?, vertex::point(b, t)?);
    let span = end - start;
    let length = span.length();
    if !(length > 0.0 && length.is_finite()) {
        return Ok(None);
    }
    let along = span * (1.0 / length);
    let Some(u0) = edge::blend_edgeuse(k, b, e)? else {
        return Ok(None);
    };
    let Some(u1) = eu1::radial(b, u0)? else {
        return Ok(None);
    };
    if u1 == u0 {
        return Ok(None);
    }
    let concave = is_concave_edge(k, b, e)?;
    let size = calculate_bounding_box(k, &mut *b, BoundingBoxOptions::default())?
        .size()
        .length();
    if !(size > 0.0 && size.is_finite()) {
        return Ok(None);
    }
    let tol = (size * 1e-9).max(1e-9);
    let straight = match edge::curve(b, e)? {
        Some(c) => c.is_linear(k, tol, false)?.linear,
        None => false,
    };
    if !straight {
        return Ok(None);
    }
    let mid = b.edge(e)?.interval.evaluate(0.5);
    let (f0, n0) = outward(k, b, u0, mid)?;
    let (f1, n1) = outward(k, b, u1, mid)?;
    if f0 == f1 {
        return Ok(None);
    }
    for (face, n) in [(f0, n0), (f1, n1)] {
        if plane_normal(k, b, face, tol)?.is_none() || n.dot(along).abs() > SQUARE {
            return Ok(None);
        }
    }
    // Each face lies, from a convex edge, against the other face's outward normal, and from a
    // concave edge along it: its direction in is its plane's direction square to the edge
    // that leans away from that normal, or toward it.
    let sine = n0.cross(n1).length();
    if sine.is_nan() || sine <= CORNER {
        return Ok(None);
    }
    let into_of = |n: Vec3, other: Vec3| -> Option<Vec3> {
        let across = n.cross(along);
        let len = across.length();
        if !(len > 0.0 && len.is_finite()) {
            return None;
        }
        let across = across * (1.0 / len);
        let lean = across.dot(other);
        if lean.abs() < CORNER {
            return None;
        }
        Some(if (lean < 0.0) == concave {
            -across
        } else {
            across
        })
    };
    let (Some(i0), Some(i1)) = (into_of(n0, n1), into_of(n1, n0)) else {
        return Ok(None);
    };
    for end in [s, t] {
        for other in vertex::edges(b, end, None)? {
            if other != e && is_concave_edge(k, b, other)? {
                return Ok(None);
            }
        }
    }
    Ok(Some(StraightWedge {
        edge: e,
        start,
        end,
        along,
        into: [i0, i1],
        normals: [n0, n1],
        faces: [f0, f1],
        vertices: [s, t],
        concave,
    }))
}
