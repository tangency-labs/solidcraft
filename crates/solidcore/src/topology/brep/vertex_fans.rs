//! Splitting a vertex where the body touches itself into one vertex per fan of faces,
//! and joining such vertices again (the fork's `SplitVertexByFans` / `JoinVertexFans`).
//!
//! Upstream: the fork tangency-labs/smlib `4ecfbab` (`source/SMLib/src/SmBrep.cpp`,
//! `source/SMLib/inc/SmBrep.h`); not in SMLib at the pin.
//!
//! | Rust | Upstream (fork) |
//! |------|-----------------|
//! | [`split_vertex_by_fans`] | `SmBrep::SplitVertexByFans` |
//! | [`join_vertex_fans`] | `SmBrep::JoinVertexFans` |
//!
//! Deviations: the new vertices come back as a `Vec` rather than an out-array. A fan is
//! a `BTreeSet` of edge ids where the fork has a `std::set` of edge pointers; only
//! membership is read, so the order does not reach a result. The new vertex record is
//! built with its point and tolerance and appended to the vertex list as the fork's
//! `new`, `SetPoint`, `SetTolerance(.., FALSE, FALSE)` and `PostInsert` do; no editing
//! notification is sent for it (the fork's vertex is not on the B-rep's lists when it is
//! given its point), and none for the joined vertex's deletion, which the fork deletes
//! directly. Behaviour belongs to callers: on its own neither function is reached by an
//! upstream path; the fillet executive calls them only with
//! `fixes.fillet_corner_per_vertex_fan` (WR-665). No recursion; each fan walk is bounded
//! by the vertex's use count, as the fork's.

use std::collections::{BTreeMap, BTreeSet};

use crate::error::{Result, SolidError, SM_ERR_INVALID_INPUT};
use crate::topology::edgeuse::p1 as eu1;
use crate::topology::{vertex, vertexuse, Brep, EdgeId, Vertex, VertexId, VertexuseId};

const OP_SPLIT: &str = "topology::brep::vertex_fans::split_vertex_by_fans";
const OP_JOIN: &str = "topology::brep::vertex_fans::join_vertex_fans";

/// `pVertex->Remove(pVU)`.
fn remove_vertexuse(b: &mut Brep, v: VertexId, vu: VertexuseId, op: &'static str) -> Result<()> {
    if b.vertexuse(vu)?.vertex != Some(v) {
        return Err(SolidError::sm_err(op));
    }
    let index = b
        .vertex(v)?
        .vertexuses
        .iter()
        .position(|&id| id == vu)
        .ok_or_else(|| SolidError::sm_err(op))?;
    b.vertex_mut(v)?.vertexuses.remove(index);
    b.vertexuse_mut(vu)?.vertex = None;
    Ok(())
}

/// `pVertex->PostInsert(pVU)`.
fn post_insert_vertexuse(
    b: &mut Brep,
    v: VertexId,
    vu: VertexuseId,
    op: &'static str,
) -> Result<()> {
    b.vertex(v)?;
    if b.vertexuse(vu)?.vertex.is_some() {
        return Err(SolidError::sm_err(op));
    }
    b.vertex_mut(v)?.vertexuses.push(vu);
    b.vertexuse_mut(vu)?.vertex = Some(v);
    Ok(())
}

/// `SmBrep::SplitVertexByFans` (the fork's, tangency-labs/smlib `4ecfbab`): a vertex
/// where the body touches itself (its edge uses form several fans, each a cone of faces
/// round the vertex sharing no edge with the others) split into one vertex per fan, all
/// at its point. The vertex keeps the fan of its first vertex use; the vertex uses of
/// each other fan move to a new vertex, returned in fan order. A vertex with one fan, a
/// vertex use not on a loop's edge use (a wire, a vertex loop or shell), or fans sharing
/// an edge is left as it is (no new vertices).
///
/// # Errors
///
/// `SM_ERR` ([`SolidError::Upstream`]) when a fan walk takes more steps than the vertex
/// has uses; [`SolidError::null_pointer`] for an edge use without loop neighbour,
/// radial or edge; [`SolidError::NotFound`] for a dangling id.
pub fn split_vertex_by_fans(b: &mut Brep, v: VertexId) -> Result<Vec<VertexId>> {
    let null = || SolidError::null_pointer(OP_SPLIT);
    let uses = vertex::vertexuses(b, v)?;
    let mut starts = Vec::new();
    for &vu in &uses {
        if vertexuse::vertexuse_type(b, vu)? != vertexuse::EDGEUSE_TYPE {
            return Ok(Vec::new());
        }
        let Some(eu) = vertexuse::edgeuse(b, vu)? else {
            return Ok(Vec::new());
        };
        if !eu1::is_loop_edgeuse(b, eu)? {
            return Ok(Vec::new());
        }
        starts.push(eu);
    }

    // the fans, as the edges each one's faces meet at the vertex
    let mut fans: Vec<BTreeSet<EdgeId>> = Vec::new();
    let mut fan_of: BTreeMap<EdgeId, usize> = BTreeMap::new();
    for &start in &starts {
        let start_edge = eu1::edge(b, start)?.ok_or_else(null)?;
        if fan_of.contains_key(&start_edge) {
            continue;
        }
        let mut fan = BTreeSet::new();
        let mut eu = start;
        let mut steps = 0usize;
        loop {
            // every step takes two vertex uses' edges: a fan is no longer than the
            // vertex's uses
            if steps > uses.len() {
                return Err(SolidError::sm_err(OP_SPLIT));
            }
            // the edge use before, ending at the vertex
            let incoming = eu1::cw_edgeuse(b, eu)?.ok_or_else(null)?;
            fan.insert(eu1::edge(b, eu)?.ok_or_else(null)?);
            fan.insert(eu1::edge(b, incoming)?.ok_or_else(null)?);
            // the next face's edge use from the vertex
            eu = eu1::radial(b, incoming)?.ok_or_else(null)?;
            if eu == start {
                break;
            }
            steps += 1;
        }
        if fan.iter().any(|edge| fan_of.contains_key(edge)) {
            // fans sharing an edge: not a vertex the body touches itself at
            return Ok(Vec::new());
        }
        for &edge in &fan {
            fan_of.insert(edge, fans.len());
        }
        fans.push(fan);
    }

    // every fan but the first on a vertex of its own
    let (point, tolerance) = {
        let record = b.vertex(v)?;
        (record.point, record.tolerance)
    };
    let mut made = Vec::new();
    for f in 1..fans.len() {
        let new = b.add_vertex_record(Vertex {
            point,
            ..Vertex::default()
        })?;
        vertex::set_tolerance(b, new, tolerance, false, false)?;
        for (&vu, &eu) in uses.iter().zip(&starts) {
            let edge = eu1::edge(b, eu)?.ok_or_else(null)?;
            // `sFanOf[edge]` (a missing key reads as fan 0)
            if fan_of.get(&edge).copied().unwrap_or(0) != f {
                continue;
            }
            remove_vertexuse(b, v, vu, OP_SPLIT)?;
            post_insert_vertexuse(b, new, vu, OP_SPLIT)?;
        }
        made.push(new);
    }
    Ok(made)
}

/// `SmBrep::JoinVertexFans` (the fork's, tangency-labs/smlib `4ecfbab`): the inverse of
/// [`split_vertex_by_fans`]. `other`'s vertex uses move onto `survivor` and `other` is
/// deleted. The two must be at one point (within the larger of their tolerances) and
/// share no edge.
///
/// # Errors
///
/// `SM_ERR_INVALID_INPUT` ([`SolidError::Upstream`]) for the same vertex twice, a vertex
/// not in `b`, vertices apart or sharing an edge; [`SolidError::NotFound`] for a
/// dangling id.
pub fn join_vertex_fans(b: &mut Brep, survivor: VertexId, other: VertexId) -> Result<()> {
    let invalid = || SolidError::Upstream {
        operation: OP_JOIN,
        code: SM_ERR_INVALID_INPUT,
        entity: None,
    };
    if survivor == other || b.vertex(survivor).is_err() || b.vertex(other).is_err() {
        return Err(invalid());
    }
    // smos_Max
    let (a, c) = (
        vertex::tolerance(b, survivor)?,
        vertex::tolerance(b, other)?,
    );
    let tol = if a > c { a } else { c };
    if vertex::point(b, survivor)?.distance(vertex::point(b, other)?) > tol {
        return Err(invalid());
    }
    let edges = vertex::edges(b, survivor, None)?;
    let other_edges = vertex::edges(b, other, None)?;
    if other_edges.iter().any(|edge| edges.contains(edge)) {
        return Err(invalid());
    }
    for vu in vertex::vertexuses(b, other)? {
        remove_vertexuse(b, other, vu, OP_JOIN)?;
        post_insert_vertexuse(b, survivor, vu, OP_JOIN)?;
    }
    b.remove_vertex_record(other)?;
    Ok(())
}
