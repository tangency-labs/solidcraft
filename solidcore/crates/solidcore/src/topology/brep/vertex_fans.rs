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
//! | [`split_pinched_loops`] | none (not in the fork) |
//!
//! Deviations: the new vertices come back as a `Vec` rather than an out-array. A fan is
//! a `BTreeSet` of edge ids where the fork has a `std::set` of edge pointers; only
//! membership is read, so the order does not reach a result. The new vertex record is
//! built with its point and tolerance and appended to the vertex list as the fork's
//! `new`, `SetPoint`, `SetTolerance(.., FALSE, FALSE)` and `PostInsert` do; no editing
//! notification is sent for it (the fork's vertex is not on the B-rep's lists when it is
//! given its point), and none for the joined vertex's deletion, which the fork deletes
//! directly. Behaviour belongs to callers: on its own neither function is reached by an
//! upstream path; the fillet executive calls them (WR-665). No recursion; each fan walk is bounded
//! by the vertex's use count, as the fork's. [`split_pinched_loops`] is solidcore's own (not in
//! the fork): a face loop through one vertex twice cut there into two loops, so that the vertex
//! comes apart into one per fan; the cuts on a face are bounded by its edge uses.

use std::collections::{BTreeMap, BTreeSet};

use crate::error::topology::TopologyFailure;
use crate::error::{Failure, Result, SolidError, SM_ERR_NULL_POINTER};
use crate::kernel::Kernel;
use crate::math::Vec3;
use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
use crate::topology::edgeuse::p1 as eu1;
use crate::topology::{
    vertex, vertexuse, Brep, EdgeId, EdgeuseId, EdgeuseOwner, FaceId, FaceuseId, Loop, Loopuse,
    LoopuseId, LoopuseStart, Vertex, VertexId, VertexuseId,
};

const OP_SPLIT: &str = "topology::brep::vertex_fans::split_vertex_by_fans";
const OP_JOIN: &str = "topology::brep::vertex_fans::join_vertex_fans";
const OP_PINCH: &str = "topology::brep::vertex_fans::split_pinched_loops";

/// `pVertex->Remove(pVU)`.
fn remove_vertexuse(b: &mut Brep, v: VertexId, vu: VertexuseId, op: &'static str) -> Result<()> {
    if b.vertexuse(vu)?.vertex != Some(v) {
        return Err(SolidError::topology(
            op,
            TopologyFailure::Defect {
                why: "the vertex use to move is not on the vertex, though it is read from the \
                      vertex's uses",
            },
        ));
    }
    let index = b
        .vertex(v)?
        .vertexuses
        .iter()
        .position(|&id| id == vu)
        .ok_or_else(|| {
            SolidError::topology(
                op,
                TopologyFailure::Defect {
                    why: "the vertex use is not in its vertex's uses, though it names that vertex",
                },
            )
        })?;
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
        return Err(SolidError::topology(
            op,
            TopologyFailure::Defect {
                why: "the vertex use to insert is already on a vertex, though it was just taken \
                      off its own",
            },
        ));
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
/// `SM_ERR` (a [`TopologyFailure`](crate::error::topology::TopologyFailure)) when a fan walk takes more steps than the vertex
/// has uses; [`SolidError::null_pointer`] for an edge use without loop neighbour,
/// radial or edge; [`SolidError::NotFound`] for a dangling id.
pub fn split_vertex_by_fans(b: &mut Brep, v: VertexId) -> Result<Vec<VertexId>> {
    let null = || {
        Failure::new(
            OP_SPLIT,
            TopologyFailure::Defect {
                why: "a loop edge use has no loop neighbour, radial or edge, though the kernel \
                      links every loop edge use to them",
            },
        )
        .with_status(SM_ERR_NULL_POINTER)
        .into_error()
    };
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
                return Err(SolidError::topology(
                    OP_SPLIT,
                    TopologyFailure::Defect {
                        why: "a fan walk takes more steps than the vertex has uses, though each \
                              step passes two of its uses",
                    },
                ));
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
/// `SM_ERR_INVALID_INPUT` (a [`TopologyFailure`](crate::error::topology::TopologyFailure)) for the same vertex twice, a vertex
/// not in `b`, vertices apart or sharing an edge; [`SolidError::NotFound`] for a
/// dangling id.
pub fn join_vertex_fans(b: &mut Brep, survivor: VertexId, other: VertexId) -> Result<()> {
    let invalid = || SolidError::topology(OP_JOIN, TopologyFailure::VerticesNotJoinable);
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

/// (not upstream) Every face loop of `b` that passes through one vertex twice, cut there into
/// two loops, and the vertex split into one vertex per fan ([`split_vertex_by_fans`]), all at
/// its point; the new vertices are returned.
///
/// Such a loop is an outline that touches itself: a pad's corner on the edge of the face it
/// stands on, merged by the boolean into the vertex that splits that edge. Round the vertex the
/// face has two sectors, each between an edge of the outline and an edge of the pad's
/// footprint, and the body's other faces at the vertex lie between them in one fan. Cut, the
/// loop's two passes through the vertex are each joined to themselves: one loop runs on along
/// the outline (its two edges at the vertex now consecutive), the other round the footprint.
/// The fan then comes apart into two that share no edge: the outline's two edges with the face
/// across them, and the footprint's edges with the pad's faces. The body is the same point
/// set, its face's two loops now touching at a point. It is the working copy an offset builds
/// from, where each fan has an offset vertex of its own: the outline's two edges' offsets meet
/// in one point and the footprint's in another.
///
/// A pass through a seam's end (an edge at the vertex used twice in the loop) is no pinch. A
/// cut is made only where its two loops are oriented as the face needs, read from the signed
/// areas of their UV trim curves: from an outer loop one outer and one inner, from an inner
/// loop two inner ones. A pinch whose pieces are two outer loops (two lobes of a face meeting
/// at a point: two faces, not one face with two outer loops) or whose orientation the UV curves
/// do not decide is left as it is.
///
/// A pinch is cut only where the two sectors' offsets by `distance` meet in two points. The
/// offset of a vertex in a sector is fixed by the normals there of the faces bounding it, so
/// where the faces across the touch on one side have, at the vertex, the normals of those on
/// the other (a cylinder's side tangent to a block's side along the line where its rim's
/// circle touches the block's edge), both sectors' offsets meet in one point and the one
/// vertex stands for them: such a pinch is left as it is. The normals are compared with their
/// proved bounds, against the vertex's tolerance over `|distance|`.
///
/// # Errors
///
/// [`SolidError::NotFound`] for a dangling id; a [`TopologyFailure::Defect`] for a loop use
/// without mate, face use or vertex uses where the kernel links them, or a mate loop whose
/// edge uses do not run back along the loop's; those of the UV trim curves, of the face
/// normals at the vertex ([`crate::query::api_queries::face_normal_at_vertex`]) and of
/// [`split_vertex_by_fans`].
pub fn split_pinched_loops(k: &Kernel, b: &mut Brep, distance: f64) -> Result<Vec<VertexId>> {
    let mut made = Vec::new();
    for f in crate::topology::brep::p1::faces(b, None)? {
        let Some(fu) = b.face(f)?.faceuse else {
            continue;
        };
        // every cut makes one more loop, of one edge use or more: no more cuts than uses
        let mut budget = 0usize;
        for lu in eu1::faceuse_loopuses(b, fu, false, None)? {
            if matches!(b.loopuse(lu)?.start, LoopuseStart::Edgeuse(_)) {
                budget = budget.saturating_add(eu1::loopuse_edgeuses(b, lu)?.len());
            }
        }
        // the pinches left as they are, so that the search goes on past them
        let mut kept: Vec<(LoopuseId, VertexId)> = Vec::new();
        while let Some(pinch) = find_pinch(b, fu, &kept)? {
            if budget == 0 {
                return Err(pinch_defect(
                    "more pinches cut on a face than it has edge uses, though every cut makes \
                     one more loop of at least one of them",
                ));
            }
            budget -= 1;
            if sectors_offset_apart(k, b, f, &pinch, distance)?
                && cut_pinched_loop(k, b, f, fu, &pinch)?
            {
                b.refresh_cached_topology();
                made.extend(split_vertex_by_fans(b, pinch.vertex)?);
                b.refresh_cached_topology();
            } else {
                kept.push((pinch.loopuse, pinch.vertex));
            }
        }
    }
    Ok(made)
}

/// (not upstream) Whether a face loop of `b` passes through one vertex twice, not at a seam's
/// end: a loop [`split_pinched_loops`] would look at.
///
/// # Errors
///
/// As [`split_pinched_loops`]'s search: a dangling id, a loop edge use without vertex use or
/// edge.
pub fn has_pinched_loop(b: &Brep) -> Result<bool> {
    for f in crate::topology::brep::p1::faces(b, None)? {
        if let Some(fu) = b.face(f)?.faceuse {
            if find_pinch(b, fu, &[])?.is_some() {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

/// A loop through a vertex twice: the loop use (of the face's upward face use), its edge uses
/// in order, and the positions of the two that start at the vertex.
struct Pinch {
    loopuse: LoopuseId,
    edgeuses: Vec<EdgeuseId>,
    first: usize,
    second: usize,
    vertex: VertexId,
}

fn pinch_defect(why: &'static str) -> SolidError {
    Failure::new(OP_PINCH, TopologyFailure::Defect { why })
        .with_status(SM_ERR_NULL_POINTER)
        .into_error()
}

/// The first pass of a loop of `fu` through a vertex it passed before, not a seam's end and
/// not among `kept`.
fn find_pinch(b: &Brep, fu: FaceuseId, kept: &[(LoopuseId, VertexId)]) -> Result<Option<Pinch>> {
    for lu in eu1::faceuse_loopuses(b, fu, false, None)? {
        if !matches!(b.loopuse(lu)?.start, LoopuseStart::Edgeuse(_)) {
            continue;
        }
        let edgeuses = eu1::loopuse_edgeuses(b, lu)?;
        let mut starts = Vec::new();
        let mut edges = Vec::new();
        for &eu in &edgeuses {
            let vu = eu1::vertexuse(b, eu)?
                .ok_or_else(|| pinch_defect("a loop edge use has no vertex use"))?;
            starts.push(
                b.vertexuse(vu)?
                    .vertex
                    .ok_or_else(|| pinch_defect("a loop edge use's vertex use has no vertex"))?,
            );
            edges.push(eu1::edge(b, eu)?.ok_or_else(|| pinch_defect("an edge use has no edge"))?);
        }
        for (first, &v) in starts.iter().enumerate() {
            if kept.contains(&(lu, v)) {
                continue;
            }
            let Some(offset) = starts.iter().skip(first + 1).position(|&w| w == v) else {
                continue;
            };
            // a seam's end: an edge at the vertex used twice in the loop
            let seam_end = edges.iter().zip(&starts).enumerate().any(|(i, (e, &w))| {
                let ends_here = w == v || starts.get(i + 1).or_else(|| starts.first()) == Some(&v);
                ends_here && edges.iter().filter(|&g| g == e).count() > 1
            });
            if seam_end {
                continue;
            }
            return Ok(Some(Pinch {
                loopuse: lu,
                edgeuses,
                first,
                second: first + 1 + offset,
                vertex: v,
            }));
        }
    }
    Ok(None)
}

/// The orientation of the closed UV loop of `edgeuses` on their face (the certified signed
/// area of their UV trim curves, made where missing); `Unknown` where a curve cannot be had or
/// the area does not decide.
fn piece_orientation(k: &Kernel, b: &mut Brep, edgeuses: &[EdgeuseId]) -> Result<OrientType> {
    let mut curves = Vec::new();
    let mut senses = Vec::new();
    for &eu in edgeuses {
        let made = crate::topology::edgeuse::p3::get_or_create_uv_trim_curve(
            k, b, eu, false, false, true,
        )?;
        if made.status.is_err() {
            return Ok(OrientType::Unknown);
        }
        let Some(curve) = eu1::uv_trim_curve(b, eu)?.0 else {
            return Ok(OrientType::Unknown);
        };
        curves.push(curve.clone());
        senses.push(eu1::orientation(b, eu)? == OrientType::Same);
    }
    match crate::topology::face::p3::uv_loop_orientation(k, &curves, &senses, None) {
        Err(e) if e.is_unported_or_cancelled() => Err(e),
        Ok(o) => Ok(o),
        Err(_) => Ok(OrientType::Unknown),
    }
}

/// Links `cycle`'s edge uses into one loop, in order: each with its counter-clockwise
/// successor and clockwise predecessor.
fn link_cycle(b: &mut Brep, cycle: &[EdgeuseId]) -> Result<()> {
    for (i, &eu) in cycle.iter().enumerate() {
        let next = *cycle
            .get(i + 1)
            .or_else(|| cycle.first())
            .ok_or_else(|| pinch_defect("an empty loop piece"))?;
        let previous = *i
            .checked_sub(1)
            .map_or_else(|| cycle.last(), |p| cycle.get(p))
            .ok_or_else(|| pinch_defect("an empty loop piece"))?;
        let record = b.edgeuse_mut(eu)?;
        record.ccw = Some(next);
        record.cw = Some(previous);
    }
    Ok(())
}

/// The outward unit normals at `v`, each with its proved bound, of the faces other than `f`
/// across the first and the last of `edgeuses`, the edge uses of one side of a pinch at `v`.
fn faces_across(
    k: &Kernel,
    b: &mut Brep,
    f: FaceId,
    v: VertexId,
    edgeuses: &[EdgeuseId],
) -> Result<Vec<(Vec3, f64)>> {
    let mut faces: Vec<FaceId> = Vec::new();
    for eu in [edgeuses.first(), edgeuses.last()].into_iter().flatten() {
        let e = eu1::edge(b, *eu)?.ok_or_else(|| pinch_defect("an edge use has no edge"))?;
        for u in crate::topology::edge::edgeuses(b, e, None, None)? {
            if let Some(g) = eu1::face(b, u)? {
                if g != f && !faces.contains(&g) {
                    faces.push(g);
                }
            }
        }
    }
    let mut normals = Vec::new();
    for g in faces {
        // the face use on the body's outside: its normal faces the exterior
        let Some(gu) = crate::topology::face::p2::outer_faceuse(b, g)? else {
            continue;
        };
        let n = crate::query::api_queries::face_normal_at_vertex(k, b, v, g, gu)?;
        normals.push((n.value, n.bound));
    }
    Ok(normals)
}

/// Whether the two sectors of `pinch` on face `f` have their offsets by `distance` meet in
/// two points: whether a face across the touch on one side has, at the vertex, a normal no
/// face across it on the other side has (see [`split_pinched_loops`]).
fn sectors_offset_apart(
    k: &Kernel,
    b: &mut Brep,
    f: FaceId,
    pinch: &Pinch,
    distance: f64,
) -> Result<bool> {
    let (Some(piece), Some(head), Some(tail)) = (
        pinch.edgeuses.get(pinch.first..pinch.second),
        pinch.edgeuses.get(pinch.second..),
        pinch.edgeuses.get(..pinch.first),
    ) else {
        return Err(pinch_defect("a pinch's passes lie outside its loop"));
    };
    let rest: Vec<EdgeuseId> = head.iter().chain(tail).copied().collect();
    let one = faces_across(k, b, f, pinch.vertex, piece)?;
    let other = faces_across(k, b, f, pinch.vertex, &rest)?;
    if one.is_empty() || other.is_empty() {
        return Ok(true);
    }
    let tolerance = b.vertex(pinch.vertex)?.tolerance;
    let reach = distance.abs();
    // two faces' offsets by `distance` part at the vertex by at most |n_a - n_b| |distance|
    let same = |a: &(Vec3, f64), c: &(Vec3, f64)| {
        (a.0 - c.0).length() * reach <= tolerance + (a.1 + c.1) * reach
    };
    let covered =
        |xs: &[(Vec3, f64)], ys: &[(Vec3, f64)]| xs.iter().all(|x| ys.iter().any(|y| same(x, y)));
    Ok(!(covered(&one, &other) && covered(&other, &one)))
}

/// Cuts `pinch`'s loop (on face use `fu` of face `f`, and its mate on the other face use) at
/// its vertex into two loops, when they are oriented as the face needs (see
/// [`split_pinched_loops`]); whether it was cut.
fn cut_pinched_loop(
    k: &Kernel,
    b: &mut Brep,
    f: FaceId,
    fu: FaceuseId,
    pinch: &Pinch,
) -> Result<bool> {
    let (Some(piece), Some(head), Some(tail)) = (
        pinch.edgeuses.get(pinch.first..pinch.second),
        pinch.edgeuses.get(pinch.second..),
        pinch.edgeuses.get(..pinch.first),
    ) else {
        return Err(pinch_defect("a pinch's passes lie outside its loop"));
    };
    let piece = piece.to_vec();
    let rest: Vec<EdgeuseId> = head.iter().chain(tail).copied().collect();
    let (Some(&piece_start), Some(&rest_start)) = (piece.first(), rest.first()) else {
        return Ok(false);
    };
    let lu = pinch.loopuse;
    let record = *b.loopuse(lu)?;
    let lm = record
        .mate
        .ok_or_else(|| pinch_defect("a face's loop use has no mate"))?;
    let fm = b
        .loopuse(lm)?
        .faceuse
        .ok_or_else(|| pinch_defect("a mate loop use has no face use"))?;

    // the two pieces' orientations decide whether the cut is one the face can have
    let was_outer = record.orientation == OrientType::Same;
    let piece_o = piece_orientation(k, b, &piece)?;
    let rest_o = piece_orientation(k, b, &rest)?;
    let piece_outer = match (was_outer, piece_o, rest_o) {
        (true, OrientType::Same, OrientType::Opposite) => true,
        (true, OrientType::Opposite, OrientType::Same)
        | (false, OrientType::Opposite, OrientType::Opposite) => false,
        _ => return Ok(false),
    };

    // the mate loop runs back along the loop: the edge use after an edge use's mate is the
    // mate of the edge use before it
    let mut mates = BTreeMap::new();
    for &eu in &pinch.edgeuses {
        let mate = eu1::mate(b, eu)?.ok_or_else(|| pinch_defect("a loop edge use has no mate"))?;
        let before = eu1::cw_edgeuse(b, eu)?
            .ok_or_else(|| pinch_defect("a loop edge use has no clockwise neighbour"))?;
        let before_mate =
            eu1::mate(b, before)?.ok_or_else(|| pinch_defect("a loop edge use has no mate"))?;
        if eu1::loopuse(b, mate)? != Some(lm) || eu1::ccw_edgeuse(b, mate)? != Some(before_mate) {
            return Err(pinch_defect(
                "the mate loop's edge uses do not run back along the loop's, though a face's \
                 two loop uses are each other's reverse",
            ));
        }
        mates.insert(eu, mate);
    }
    let mate_of = |eu: &EdgeuseId| {
        mates
            .get(eu)
            .copied()
            .ok_or_else(|| pinch_defect("a loop edge use has no mate"))
    };
    let piece_mates: Vec<EdgeuseId> = piece.iter().rev().map(mate_of).collect::<Result<_>>()?;
    let rest_mates: Vec<EdgeuseId> = rest.iter().rev().map(mate_of).collect::<Result<_>>()?;

    // the piece's loop, with a loop use on each face use
    let new_loop = b.add_loop_record(Loop {
        face: f,
        edges: Vec::new(),
        loopuse: None,
    })?;
    let new_lu = b.add_loopuse_record(Loopuse {
        faceuse: Some(fu),
        loop_: Some(new_loop),
        mate: None,
        start: LoopuseStart::Edgeuse(Some(piece_start)),
        orientation: OrientType::Opposite,
    })?;
    let new_lm = b.add_loopuse_record(Loopuse {
        faceuse: Some(fm),
        loop_: Some(new_loop),
        mate: Some(new_lu),
        start: LoopuseStart::Edgeuse(Some(mate_of(&piece_start)?)),
        orientation: OrientType::Opposite,
    })?;
    b.loopuse_mut(new_lu)?.mate = Some(new_lm);
    b.loop_mut(new_loop)?.loopuse = Some(new_lu);

    for cycle in [&piece, &rest, &piece_mates, &rest_mates] {
        link_cycle(b, cycle)?;
    }
    for &eu in &piece {
        b.edgeuse_mut(eu)?.owner = EdgeuseOwner::Loopuse(Some(new_lu));
    }
    for &eu in &piece_mates {
        b.edgeuse_mut(eu)?.owner = EdgeuseOwner::Loopuse(Some(new_lm));
    }
    b.loopuse_mut(lu)?.start = LoopuseStart::Edgeuse(Some(rest_start));
    b.loopuse_mut(lm)?.start = LoopuseStart::Edgeuse(Some(mate_of(&rest_start)?));

    // the outer loop's uses labelled same and first in their face uses' lists, the others
    // opposite
    let (outer, inner) = if piece_outer {
        ([new_lu, new_lm], [lu, lm])
    } else {
        ([lu, lm], [new_lu, new_lm])
    };
    if was_outer {
        for side in outer {
            b.loopuse_mut(side)?.orientation = OrientType::Same;
        }
    }
    for side in inner {
        b.loopuse_mut(side)?.orientation = OrientType::Opposite;
    }
    for (face_use, new) in [(fu, new_lu), (fm, new_lm)] {
        let list = &mut b.faceuse_mut(face_use)?.loopuses;
        let ordered: Vec<LoopuseId> = if piece_outer {
            core::iter::once(new).chain(list.iter().copied()).collect()
        } else {
            list.iter().copied().chain(core::iter::once(new)).collect()
        };
        *list = ordered;
    }
    Ok(true)
}
