//! Untrim: a face restored towards its surface. The holes asked for are taken away
//! (`SmFace::DeleteInnerLoop`, on a copy where they are lamina); with `outer`, the outer boundary
//! goes back to a box of the surface's parameters (a free-form surface's own, an analytic one's
//! the box of the face's outer loop, the union of the kernel's boxes of its trims), and the holes
//! kept are rebuilt in that face from their own edges' curves and intervals (closing a loop splits
//! off the face inside it, which is deleted). The new face and the rebuilt edges carry the
//! attributes of the face and edges they stand for.
//!
//! The face and the holes are record indices ([`Order`]): a hole is named by any record edge of
//! its loop. Which face or hole a pick means is the application's. Ops sequences the kernel's
//! calls and compares what they return; it computes nothing.

use solidcore::curve::curve::Curve;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::tol::USE_DEFAULT;
use solidcore::geom::vector3d::Point3;
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
use solidcore::surface::surface::SurfaceKind;
use solidcore::topology::brep::p2::{copy_source_ref, CopyNotifyState};
use solidcore::topology::brep::p3::{
    brep_copy_mapped, delete_face, new_brep, with_kernel_callbacks, DeleteFaceOptions,
};
use solidcore::topology::brep::p5::{make_edge_in_face, EdgeInFace};
use solidcore::topology::brep::p7::copy_faces_from;
use solidcore::topology::brep::p9::make_vertex_loop;
use solidcore::topology::brep::sheet::face_on;
use solidcore::topology::edgeuse::p1 as eu_p1;
use solidcore::topology::face::{p1 as face_p1, p2 as face_p2};
use solidcore::topology::loopuse::{loop_edges, loop_loopuses, loop_uv_span, notify_ref};
use solidcore::topology::topology_traverser::Marks;
use solidcore::topology::{edge, Brep, EdgeId, FaceId, LoopId};
use solidcore::EntityRef;
use solidcore_convert::brep::Order;

use crate::body::{copy_attributes, copy_attributes_within, upward, Body};
use crate::error::status;
use crate::session::run_on;
use crate::split::copies_of;
use crate::{Error, Result};

/// The holes [`untrim_face`] takes away.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Holes<'a> {
    /// Every inner loop.
    All,
    /// The inner loop each record edge (an index into the body's [`Order`]) lies on.
    Along(&'a [usize]),
}

/// A rebuilt hole that did not close as one loop round one disc.
fn hole_refused() -> Error {
    Error::Exception
}

/// One edge of a hole to rebuild: its edge, how the loop runs along it, its ends as the loop
/// runs, its curve and interval.
struct Piece {
    edge: EdgeId,
    orient: OrientType,
    from: Point3,
    to: Point3,
    curve: Curve,
    interval: Extent1d,
}

/// The pieces of loop `l`; `None` for an edge with no curve or ends.
fn pieces_of(b: &Brep, l: LoopId) -> solidcore::Result<Option<Vec<Piece>>> {
    let (lu, _) = loop_loopuses(b, l)?;
    let mut out = Vec::new();
    for eu in eu_p1::loopuse_edgeuses(b, lu)? {
        let Some(e) = eu_p1::edge(b, eu)? else {
            return Ok(None);
        };
        let ed = b.edge(e)?;
        let Some(curve) = ed.curve.clone() else {
            return Ok(None);
        };
        let (Some(v0), Some(v1)) = edge::start_end_vertices(b, e)? else {
            return Ok(None);
        };
        let same = eu_p1::orientation(b, eu)? == OrientType::Same;
        let (p0, p1) = (b.vertex(v0)?.point, b.vertex(v1)?.point);
        let (from, to) = if same { (p0, p1) } else { (p1, p0) };
        out.push(Piece {
            edge: e,
            orient: if same {
                OrientType::Same
            } else {
                OrientType::Opposite
            },
            from,
            to,
            curve,
            interval: ed.interval,
        });
    }
    Ok((!out.is_empty()).then_some(out))
}

/// Whether `f` is bounded by one loop of exactly the edges `made`.
fn is_disc(b: &Brep, f: FaceId, made: &[EdgeId]) -> solidcore::Result<bool> {
    let loops = face_p2::loops(b, f, false, None)?;
    let [l] = loops.as_slice() else {
        return Ok(false);
    };
    let edges: Vec<EdgeId> = loop_edges(b, *l)?.into_iter().flatten().collect();
    Ok(edges.len() == made.len() && edges.iter().all(|e| made.contains(e)))
}

/// The hole `pieces` (edges of `src`, or of `r` itself for `None`) rebuilt in `face` of `r`:
/// vertices and edges made in the face, the face the closed loop splits off inside it deleted.
/// `face` becomes the face that keeps the rest.
fn rebuild_hole(
    k: &mut Kernel,
    src: Option<&Brep>,
    r: &mut Brep,
    face: &mut FaceId,
    pieces: &[Piece],
) -> solidcore::Result<Result<()>> {
    let Some(first_piece) = pieces.first() else {
        return Ok(Err(hole_refused()));
    };
    let editing = r.editing_enabled();
    r.set_editing_enabled(true);
    let tol = r.tolerance();
    // each edit notifies through the kernel's callbacks with the body's own attributes
    let vertex_loop = |k: &mut Kernel, r: &mut Brep, face: FaceId, at: Point3| {
        with_kernel_callbacks(k, r, |_, r, env| {
            make_vertex_loop(r, face, at, OrientType::Opposite, env)
        })
    };
    let done = (|| -> solidcore::Result<Result<()>> {
        let (_, first) = match vertex_loop(k, r, *face, first_piece.from) {
            Ok(v) => v,
            Err(e) => return Ok(Err(status(e))),
        };
        let mut at = first;
        let mut made = Vec::new();
        let mut split = None;
        for (i, p) in pieces.iter().enumerate() {
            let last = i + 1 == pieces.len();
            let next = if last {
                first
            } else {
                match vertex_loop(k, r, *face, p.to) {
                    Ok((_, v)) => v,
                    Err(e) => return Ok(Err(status(e))),
                }
            };
            let curve = p.curve.copy(k)?;
            let mut out = EdgeInFace::default();
            if let Err(e) = make_edge_in_face(
                k,
                r,
                true,
                Some(*face),
                Some(at),
                Some(next),
                curve,
                None,
                p.interval,
                p.orient,
                tol,
                &mut out,
                None,
            ) {
                return Ok(Err(status(e)));
            }
            let Some(e) = out.edge else {
                return Ok(Err(Error::Empty));
            };
            match src {
                Some(s) => copy_attributes(s, EntityRef::Edge(p.edge), r, EntityRef::Edge(e))?,
                None => copy_attributes_within(r, EntityRef::Edge(p.edge), EntityRef::Edge(e))?,
            }
            made.push(e);
            if let Some(nf) = out.face {
                if !last || split.is_some() {
                    return Ok(Err(hole_refused()));
                }
                split = Some(nf);
            }
            at = next;
        }
        let Some(split) = split else {
            return Ok(Err(hole_refused()));
        };
        // one of the two faces is the disc inside the hole: it goes, its edges stay on the other
        let disc = if is_disc(r, split, &made)? {
            split
        } else if is_disc(r, *face, &made)? {
            *face
        } else {
            return Ok(Err(hole_refused()));
        };
        let rest = if disc == split { *face } else { split };
        let deleted = with_kernel_callbacks(k, r, |k, r, env| {
            let mut marks = Marks::default();
            let options = DeleteFaceOptions {
                delete_edges_vertices: false,
                ..DeleteFaceOptions::default()
            };
            delete_face(k, r, env, &mut marks, disc, options)
        });
        if let Err(e) = deleted {
            return Ok(Err(status(e)));
        }
        *face = rest;
        Ok(Ok(()))
    })();
    r.set_editing_enabled(editing);
    r.refresh_cached_topology();
    done
}

/// Record face `face` of `body` (an index into `order`) restored towards its surface: the holes
/// `holes` names taken away and, with `outer`, its outer boundary back on its surface's
/// parameter box (a free-form surface's own, an analytic one's the box of the face's outer
/// loop). The face alone in a body of its own, or with `in_place` the whole body with it
/// replaced. The new face and its rebuilt edges carry the attributes of those they stand for;
/// the face faces the way it did. `body` is unchanged.
///
/// # Errors
/// - [`Error::Argument`]: a face or edge past the record's list, an edge on no hole of the
///   face, or no hole asked for without `outer`;
/// - [`Error::Unsupported`] (with what): a face with no loop or surface, a hole another face
///   uses, an outer loop with no box, a hole edge with no curve;
/// - [`Error::Empty`]: the face not copied out as one face;
/// - [`Error::Exception`]: a kept hole that does not close round one disc when rebuilt;
/// - the kernel's errors.
pub fn untrim_face(
    body: &Body,
    order: &Order,
    face: usize,
    holes: Holes<'_>,
    outer: bool,
    in_place: bool,
) -> Result<Body> {
    let src = *order.faces.get(face).ok_or(Error::Argument)?;
    let along: Option<Vec<EdgeId>> = match holes {
        Holes::All => None,
        Holes::Along(edges) => {
            if edges.is_empty() && !outer {
                return Err(Error::Argument);
            }
            let mut out = Vec::new();
            for &e in edges {
                out.push(*order.edges.get(e).ok_or(Error::Argument)?);
            }
            Some(out)
        }
    };
    run_on("untrim_face", &[("brep", body)], |k| {
        // the record's face and edges are the body's own: looked up in it, then mapped to
        // their copies (the copy numbers its entities as it builds them)
        let (mut whole, src, along) = {
            let g = body.lock();
            if !g.face_list().contains(&src) {
                return Ok(Err(Error::Argument));
            }
            let (whole, map) = brep_copy_mapped(k, &g)?;
            let face = |e| match e {
                EntityRef::Face(f) => Some(f),
                _ => None,
            };
            let edge = |e| match e {
                EntityRef::Edge(x) => Some(x),
                _ => None,
            };
            let Some([src]) = copies_of(&map, &[src], EntityRef::Face, face)
                .and_then(|v| <[FaceId; 1]>::try_from(v).ok())
            else {
                return Ok(Err(Error::Argument));
            };
            let along = match &along {
                None => None,
                Some(edges) => match copies_of(&map, edges, EntityRef::Edge, edge) {
                    Some(e) => Some(e),
                    None => return Ok(Err(Error::Argument)),
                },
            };
            (whole, src, along)
        };
        // the holes taken away, as places among the face's inner loops (a copy out of the body
        // keeps its loops' order): all of them, or the one each edge lies on
        let inner_of = |b: &Brep, f: FaceId| -> solidcore::Result<Vec<LoopId>> {
            Ok(face_p2::loops(b, f, false, None)?
                .into_iter()
                .skip(1)
                .collect())
        };
        let source_inner = inner_of(&whole, src)?;
        let mut removed_at: Vec<usize> = Vec::new();
        match &along {
            None => removed_at.extend(0..source_inner.len()),
            Some(edges) => {
                for &e in edges {
                    let mut hit = None;
                    for (i, &l) in source_inner.iter().enumerate() {
                        if loop_edges(&whole, l)?.into_iter().flatten().any(|x| x == e) {
                            hit = Some(i);
                        }
                    }
                    let Some(i) = hit else {
                        return Ok(Err(Error::Argument));
                    };
                    if !removed_at.contains(&i) {
                        removed_at.push(i);
                    }
                }
            }
        }
        // the face to untrim, in the copy of the whole body or in a body of its own (its hole
        // edges lamina there)
        let (mut w, f) = if in_place {
            (whole, src)
        } else {
            let mut w = new_brep(k, USE_DEFAULT, USE_DEFAULT)?;
            w.set_tolerance(whole.tolerance());
            // through the merge, with both bodies' attributes in the notifications, so the
            // copy carries what its source's face and edges carry
            let copied = {
                let mut state = CopyNotifyState::with_attributes_of(&mut whole, &mut w)?;
                let this_face = |f: FaceId| notify_ref(EntityRef::Face(f));
                let copied = copy_faces_from(
                    k,
                    &mut whole,
                    &[src],
                    &mut w,
                    true,
                    &copy_source_ref,
                    &this_face,
                    &mut state.env(None),
                );
                state.restore_attributes(&mut whole, &mut w)?;
                copied
            };
            let copied = match copied {
                Ok((c, _)) => c,
                Err(e) => return Ok(Err(status(e))),
            };
            let [f] = copied.as_slice() else {
                return Ok(Err(Error::Empty));
            };
            w.refresh_cached_topology();
            (w, *f)
        };
        let loops = face_p2::loops(&w, f, false, None)?;
        let Some((&outer_loop, inner)) = loops.split_first() else {
            return Ok(Err(Error::Unsupported("a face with no loop")));
        };
        if inner.len() != source_inner.len() {
            return Ok(Err(Error::Empty));
        }
        let removed: Vec<LoopId> = removed_at
            .iter()
            .filter_map(|&i| inner.get(i).copied())
            .collect();
        if !outer {
            for &l in &removed {
                // a hole whose edges another face of the body uses is no hole of a sheet
                if face_p1::delete_inner_loop_with_kernel_callbacks(k, &mut w, f, l).is_err() {
                    return Ok(Err(Error::Unsupported("a hole another face uses")));
                }
            }
            w.refresh_cached_topology();
            return Ok(Ok(w));
        }
        // the outer boundary: the surface's parameter box (a free-form surface's own, an
        // analytic one's the span of the face)
        let Some(surf) = face_p2::surface(&w, f)?.cloned() else {
            return Ok(Err(Error::Unsupported("a face with no surface")));
        };
        let bx = if matches!(surf.kind(), SurfaceKind::BSpline(_)) {
            surf.natural_uv_domain_with_nurb(k)?
        } else {
            match loop_uv_span(k, &mut w, outer_loop)? {
                Some(s) => s,
                None => return Ok(Err(Error::Unsupported("an outer loop with no box"))),
            }
        };
        let mut kept = Vec::new();
        for &l in inner {
            if removed.contains(&l) {
                continue;
            }
            match pieces_of(&w, l)? {
                Some(p) => kept.push(p),
                None => return Ok(Err(Error::Unsupported("a hole edge with no curve"))),
            }
        }
        let copy = surf.copy(k)?;
        let was = upward(&w, f)?;
        let mut r = if in_place {
            None
        } else {
            let mut r = new_brep(k, USE_DEFAULT, USE_DEFAULT)?;
            r.set_tolerance(w.tolerance());
            Some(r)
        };
        let made = face_on(k, r.as_mut().unwrap_or(&mut w), copy, &bx);
        let mut nf = match made {
            Ok(nf) => nf,
            Err(e) => return Ok(Err(status(e))),
        };
        for hole in &kept {
            let done = match r.as_mut() {
                Some(r) => rebuild_hole(k, Some(&w), r, &mut nf, hole)?,
                None => rebuild_hole(k, None, &mut w, &mut nf, hole)?,
            };
            if let Err(e) = done {
                return Ok(Err(e));
            }
        }
        match r.as_mut() {
            Some(r) => copy_attributes(&w, EntityRef::Face(f), r, EntityRef::Face(nf))?,
            None => copy_attributes_within(&mut w, EntityRef::Face(f), EntityRef::Face(nf))?,
        }
        // the face faces the way the old one did
        let target: &mut Brep = r.as_mut().unwrap_or(&mut w);
        if let (Some(a), Some(b)) = (was, upward(target, nf)?) {
            if a != b {
                if let Err(e) = face_p1::flip_face_orientation(target, nf) {
                    return Ok(Err(status(e)));
                }
            }
        }
        if let Some(r) = r {
            return Ok(Ok(r));
        }
        // the old face goes, with the edges and vertices only it used: an edit of the body,
        // which the kernel allows only in edit mode
        let editing = w.editing_enabled();
        w.set_editing_enabled(true);
        let deleted = with_kernel_callbacks(k, &mut w, |k, w, env| {
            let mut marks = Marks::default();
            let options = DeleteFaceOptions {
                delete_edges_vertices: true,
                ..DeleteFaceOptions::default()
            };
            delete_face(k, w, env, &mut marks, f, options)
        });
        w.set_editing_enabled(editing);
        w.refresh_cached_topology();
        if let Err(e) = deleted {
            return Ok(Err(status(e)));
        }
        Ok(Ok(w))
    })
    .and_then(|r| r.map(Body::new))
}

#[cfg(test)]
mod tests;
