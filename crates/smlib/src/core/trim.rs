//! Untrim and extend on solidcore: the C API's `smlib_untrim_face` and `smlib_extend_sheet`,
//! ported from the fork's `capi/src/smlib_untrim.cpp` (`face_at`, `loop_distance`,
//! `copy_attributes`, `analytic`, `span_of`, `pieces_of`, `is_disc`, `rebuild_hole`) and
//! `capi/src/smlib_extend.cpp` (`on_side`, `eval`, `nearest_edge`, `is_boundary`, `side_edge`,
//! `across`, `cone_continued`, `same_patch`).
//!
//! A face restored towards its surface. Picked inner loops (holes) are taken away with
//! `SmFace::DeleteInnerLoop` on a copy where they are lamina; the outer boundary goes back to a
//! parameter box of the surface (`SmBrep::CreateFaceFromSurface`), and the holes kept are rebuilt
//! in that face from their own edges' curves and intervals (`MakeVertexLoop`, `MakeEdgeInFace`:
//! closing a loop splits off the face inside it, which is deleted). The new face and the rebuilt
//! edges take the attributes of the face and edges they stand for, so the history tags carry
//! through.
//!
//! A sheet is extended by SMLib's extension of its surface (`CreateExtendedSurface`: its end
//! spans continued, or a G1 or G2 run-off), the distance asked of it corrected until the
//! extension measures the distance across the side's middle; `CopyAndAddAnalytics` gives the
//! result back its analytic kind, and a cone or cylinder continued naturally stays the analytic
//! widened. The new face and each of its sides take the attributes of the face and side they
//! continue.
//!
//! An untrim of a face out of its body (`in_place` false) copies the face into a body of its own
//! through `SmBrep::CopyFaces` with the merge (solidcore#1872), which keeps its attributes.

use std::sync::PoisonError;

use solidcore::EntityRef;
use solidcore::curve::curve::Curve;
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::geom_utility::p2::ContinuityType;
use solidcore::geom::object::Shape;
use solidcore::geom::tol::USE_DEFAULT;
use solidcore::geom::vector2d::Point2;
use solidcore::geom::vector3d::{Point3, Vec3};
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_surface::p1::SurfParamType;
use solidcore::nurbs::b_spline_surface_n_lib::p2::EvaluateFlags;
use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
use solidcore::query::api_queries::{edge_closest_point, face_closest_point_in_shape, face_internal_point};
use solidcore::query::brep::{BrepQuery, LiveBrepObjects};
use solidcore::surface::cone::p1::Cone;
use solidcore::surface::surface::Surface;
use solidcore::surface::surface::SurfaceKind;
use solidcore::topology::brep::p1 as brep_p1;
use solidcore::topology::brep::p2::{CopyNotifyState, copy_source_ref};
use solidcore::topology::brep::p3::{DeleteFaceOptions, delete_face, new_brep, with_kernel_callbacks};
use solidcore::topology::brep::p5::{EdgeInFace, make_edge_in_face};
use solidcore::topology::brep::p7::copy_faces_from;
use solidcore::topology::brep::p8::create_face_from_surface;
use solidcore::topology::brep::p9::make_vertex_loop;
use solidcore::topology::edgeuse::p1 as eu_p1;
use solidcore::topology::edgeuse::p3::get_or_create_uv_trim_curve;
use solidcore::topology::face::{p1 as face_p1, p2 as face_p2, p4 as face_p4};
use solidcore::topology::loopuse::{loop_edges, loop_loopuses, notify_ref};
use solidcore::topology::topology_traverser::Marks;
use solidcore::topology::{Brep, BrepId, EdgeId, FaceId, LoopId, edge};

use super::blend::{p3, status};
use super::boolean::copy_of;
use super::sweep::build_faces;
use super::{Body, answer, run};
use crate::trim::Continuation;
use crate::{Error, Result};

/// `MAX_HOLE_POINTS`.
const MAX_HOLE_POINTS: usize = 10_000;

/// `SMLIB_ERR_EXCEPTION`'s stand-in: a rebuilt hole that did not close as the capi expects.
fn hole_refused() -> Error {
    Error::Exception
}

/// The capi's statuses as SolidCraft errors: an operation's own result.
type Out<T> = std::result::Result<T, Error>;

const ONE: BrepId = BrepId::from_index(0);

/// `face_at`: the face of `b` nearest `p`, within `tol` (`SmApiFaceClosestPoint`; a face whose
/// query fails is skipped).
fn face_at(k: &mut Kernel, b: &mut Brep, p: Point3, tol: f64) -> solidcore::Result<Option<FaceId>> {
    let all = brep_p1::faces(b, None)?;
    let mut shapes = Vec::with_capacity(all.len());
    for &f in &all {
        shapes.push(answer(Shape::new(k, &(ONE, &*b), &[], &[(ONE, f)], &[], &[]))?);
    }
    let query = BrepQuery::of_brep(b);
    let identity = query.effective_identity();
    let objects = LiveBrepObjects::new(b, &identity, None)?;
    let (mut best, mut d0) = (None, f64::INFINITY);
    for (&f, shape) in all.iter().zip(&shapes) {
        let Some(shape) = shape else { continue };
        if let Some(c) = answer(face_closest_point_in_shape(k, shape, &objects, (ONE, f), p))?
            && c.distance.is_finite()
            && c.distance < d0
        {
            d0 = c.distance;
            best = Some(f);
        }
    }
    Ok(best.filter(|_| d0 <= tol))
}

/// `loop_distance`: the distance from `p` to the nearest edge of loop `l`.
fn loop_distance(k: &Kernel, b: &Brep, l: LoopId, p: Point3) -> solidcore::Result<f64> {
    let mut best = f64::INFINITY;
    for e in loop_edges(b, l)?.into_iter().flatten() {
        if let Some(c) = answer(edge_closest_point(k, b, e, p))?
            && c.closest.distance.is_finite()
        {
            best = best.min(c.closest.distance);
        }
    }
    Ok(best)
}

/// `copy_attributes`: `to` (in `into`) takes a copy of each attribute of `from` (in `src`).
fn copy_attributes(src: &Brep, from: EntityRef, into: &mut Brep, to: EntityRef) -> solidcore::Result<()> {
    let copies: Vec<_> = src.entity_attributes(from).into_iter().map(|(_, a)| a.make_copy()).collect();
    for a in copies {
        into.add_attribute(to, a)?;
    }
    Ok(())
}

/// [`copy_attributes`] within one body.
fn copy_attributes_within(b: &mut Brep, from: EntityRef, to: EntityRef) -> solidcore::Result<()> {
    let copies: Vec<_> = b.entity_attributes(from).into_iter().map(|(_, a)| a.make_copy()).collect();
    for a in copies {
        b.add_attribute(to, a)?;
    }
    Ok(())
}

/// `span_of`: the box of the face's outer loop in the surface's parameters, the union of the
/// tight boxes of its edgeuses' UV trim curves (made where the face has none attached yet).
fn span_of(k: &Kernel, b: &mut Brep, outer: LoopId) -> solidcore::Result<Option<Extent2d>> {
    let (lu, _) = loop_loopuses(b, outer)?;
    let eus = eu_p1::loopuse_edgeuses(b, lu)?;
    let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
    let mut any = false;
    for eu in eus {
        let fetched = get_or_create_uv_trim_curve(k, b, eu, false, false, true)?;
        if fetched.status.is_err() {
            return Ok(None);
        }
        let (Some(uv), _) = eu_p1::uv_trim_curve(b, eu)? else { return Ok(None) };
        let Some(e) = answer(uv.calculate_tight_bounding_box(k, &uv.natural_interval()))? else { return Ok(None) };
        let (mn, mx) = (e.min(), e.max());
        lo = [lo[0].min(mn.x), lo[1].min(mn.y)];
        hi = [hi[0].max(mx.x), hi[1].max(mx.y)];
        any = true;
    }
    if !any || !(hi[0] > lo[0]) || !(hi[1] > lo[1]) || !(lo[0] + lo[1] + hi[0] + hi[1]).is_finite() {
        return Ok(None);
    }
    Ok(Some(Extent2d::from_bounds(lo[0], lo[1], hi[0], hi[1])))
}

/// One edge of a hole to rebuild: its edge, how the loop runs along it, its ends as the loop
/// runs, its curve and interval (`Piece`).
struct Piece {
    edge: EdgeId,
    orient: OrientType,
    from: Point3,
    to: Point3,
    curve: Curve,
    interval: solidcore::geom::extent1d::Extent1d,
}

/// `pieces_of`; `None` where the capi gives up.
fn pieces_of(b: &Brep, l: LoopId) -> solidcore::Result<Option<Vec<Piece>>> {
    let (lu, _) = loop_loopuses(b, l)?;
    let mut out = Vec::new();
    for eu in eu_p1::loopuse_edgeuses(b, lu)? {
        let Some(e) = eu_p1::edge(b, eu)? else { return Ok(None) };
        let ed = b.edge(e)?;
        let Some(curve) = ed.curve.clone() else { return Ok(None) };
        let (Some(v0), Some(v1)) = edge::start_end_vertices(b, e)? else { return Ok(None) };
        let same = eu_p1::orientation(b, eu)? == OrientType::Same;
        let (p0, p1) = (b.vertex(v0)?.point, b.vertex(v1)?.point);
        let (from, to) = if same { (p0, p1) } else { (p1, p0) };
        out.push(Piece { edge: e, orient: if same { OrientType::Same } else { OrientType::Opposite }, from, to, curve, interval: ed.interval });
    }
    Ok((!out.is_empty()).then_some(out))
}

/// `is_disc`: whether `f` is bounded by one loop of exactly the edges `made`.
fn is_disc(b: &Brep, f: FaceId, made: &[EdgeId]) -> solidcore::Result<bool> {
    let loops = face_p2::loops(b, f, false, None)?;
    let [l] = loops.as_slice() else { return Ok(false) };
    let edges: Vec<EdgeId> = loop_edges(b, *l)?.into_iter().flatten().collect();
    Ok(edges.len() == made.len() && edges.iter().all(|e| made.contains(e)))
}

/// `rebuild_hole`: the hole `pieces` (edges of `src`) rebuilt in `face` of `r`: vertices and
/// edges made in the face, the face the closed loop splits off inside it deleted. `face` is the
/// face that keeps the rest.
fn rebuild_hole(k: &mut Kernel, src: Option<&Brep>, r: &mut Brep, face: &mut FaceId, pieces: &[Piece]) -> solidcore::Result<Out<()>> {
    let Some(first_piece) = pieces.first() else { return Ok(Err(hole_refused())) };
    let editing = r.editing_enabled();
    r.set_editing_enabled(true);
    let tol = r.tolerance();
    // Each edit notifies through the kernel's callbacks with the body's own attributes
    // (`MakeEdgeInFace` does so itself), so the history tags follow.
    let vertex_loop = |k: &mut Kernel, r: &mut Brep, face: FaceId, at: Point3| {
        with_kernel_callbacks(k, r, |_, r, env| make_vertex_loop(r, face, at, OrientType::Opposite, env))
    };
    let done = (|| -> solidcore::Result<Out<()>> {
        let (_, first) = match vertex_loop(k, r, *face, first_piece.from) {
            Ok(v) => v,
            Err(e) => return Ok(Err(status(e))),
        };
        let mut at = first;
        let mut made = Vec::with_capacity(pieces.len());
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
            if let Err(e) = make_edge_in_face(k, r, true, Some(*face), Some(at), Some(next), curve, None, p.interval, p.orient, tol, &mut out, None) {
                return Ok(Err(status(e)));
            }
            let Some(e) = out.edge else { return Ok(Err(Error::Empty)) };
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
        let Some(split) = split else { return Ok(Err(hole_refused())) };
        // One of the two faces is the disc inside the hole: it goes, its edges stay on the other.
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
            let options = DeleteFaceOptions { delete_edges_vertices: false, ..DeleteFaceOptions::default() };
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

/// The orientation of `f`'s upward faceuse.
fn upward(b: &Brep, f: FaceId) -> solidcore::Result<Option<OrientType>> {
    Ok(match face_p2::upward_faceuse(b, f)? {
        Some(fu) => Some(b.faceuse(fu)?.orientation),
        None => None,
    })
}

/// `smlib_untrim_face`: the face of `body` nearest `point` (within `tol`) restored towards its
/// surface: the holes nearest `holes` (all of them for `None`) taken away and, with `outer`, its
/// outer boundary back on the surface's parameter box. The face alone, or with `in_place` the
/// whole body with it replaced.
pub(crate) fn untrim_face(body: &Body, point: [f64; 3], tol: f64, holes: Option<&[[f64; 3]]>, outer: bool, in_place: bool) -> Result<Body> {
    let n_holes = holes.map(<[_]>::len);
    if !point.iter().all(|x| x.is_finite())
        || !tol.is_finite()
        || !(tol > 0.0)
        || n_holes.is_some_and(|n| n > MAX_HOLE_POINTS)
        || holes.is_some_and(|h| !h.iter().flatten().all(|x| x.is_finite()))
        || (n_holes == Some(0) && !outer)
    {
        return Err(Error::Argument);
    }
    if body.0.is_poisoned() {
        return Err(Error::Internal("smlib_untrim_face: the body was left unfinished by an earlier panic".to_owned()));
    }
    let at = p3(point);
    run("smlib_untrim_face", |k| {
        let mut whole = {
            let g = body.0.lock().unwrap_or_else(PoisonError::into_inner);
            super::dump::input(k, "brep", &g);
            copy_of(k, &g, 1)?
        };
        // The face to untrim, in a body of its own (its hole edges lamina there) or in the copy
        // of the whole body.
        let (mut w, f) = if in_place {
            let f = face_at(k, &mut whole, at, tol)?;
            (whole, f)
        } else {
            let Some(src) = face_at(k, &mut whole, at, tol)? else { return Ok(Err(Error::NoMatch { point: 0 })) };
            let mut w = new_brep(k, USE_DEFAULT, USE_DEFAULT)?;
            w.set_tolerance(whole.tolerance());
            // Through SmMerge, with both bodies' attributes in the notifications, so the copy
            // carries its source's history tags (`SM_NO_COPY`).
            let copied = {
                let mut state = CopyNotifyState::with_attributes_of(&mut whole, &mut w)?;
                let this_face = |f: FaceId| notify_ref(EntityRef::Face(f));
                let copied = copy_faces_from(k, &mut whole, &[src], &mut w, true, &copy_source_ref, &this_face, &mut state.env(None));
                state.restore_attributes(&mut whole, &mut w)?;
                copied
            };
            let copied = match copied {
                Ok((c, _)) => c,
                Err(e) => return Ok(Err(status(e))),
            };
            // The capi's `copied.GetSize() != 1`: `SMLIB_ERR_EMPTY`.
            let [f] = copied.as_slice() else { return Ok(Err(Error::Empty)) };
            w.refresh_cached_topology();
            (w, Some(*f))
        };
        let Some(f) = f else { return Ok(Err(Error::NoMatch { point: 0 })) };
        let loops = face_p2::loops(&w, f, false, None)?;
        if loops.is_empty() {
            return Ok(Err(Error::Unsupported));
        }
        let inner = loops.get(1..).unwrap_or_default();
        // The holes taken away: all of them, or the one nearest each point.
        let mut removed: Vec<LoopId> = Vec::new();
        match holes {
            None => removed.extend_from_slice(inner),
            Some(points) => {
                for p in points {
                    let p = p3(*p);
                    let (mut best, mut d0) = (None, f64::INFINITY);
                    for &l in inner {
                        let d = loop_distance(k, &w, l, p)?;
                        if d < d0 {
                            d0 = d;
                            best = Some(l);
                        }
                    }
                    let Some(best) = best.filter(|_| d0 <= tol) else { return Ok(Err(Error::NoMatch { point: 0 })) };
                    if !removed.contains(&best) {
                        removed.push(best);
                    }
                }
            }
        }
        if !outer {
            for &l in &removed {
                // A hole whose edges another face of the body uses is no hole of a sheet.
                let deleted = face_p1::delete_inner_loop_with_kernel_callbacks(k, &mut w, f, l);
                if deleted.is_err() {
                    return Ok(Err(Error::Unsupported));
                }
            }
            w.refresh_cached_topology();
            return Ok(Ok(w));
        }
        // The outer boundary: the surface's parameter box (a free-form surface's own, an
        // analytic one's the span of the face).
        let Some(surf) = face_p2::surface(&w, f)?.cloned() else { return Ok(Err(Error::Unsupported)) };
        let mut bx = surf.natural_uv_domain_with_nurb(k)?;
        if !matches!(surf.kind(), SurfaceKind::BSpline(_)) {
            let Some(outer_loop) = loops.first() else { return Ok(Err(Error::Unsupported)) };
            match span_of(k, &mut w, *outer_loop)? {
                Some(s) => bx = s,
                None => return Ok(Err(Error::Unsupported)),
            }
        }
        let mut kept = Vec::new();
        for &l in inner {
            if removed.contains(&l) {
                continue;
            }
            match pieces_of(&w, l)? {
                Some(p) => kept.push(p),
                None => return Ok(Err(Error::Unsupported)),
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
        let target: &mut Brep = match r.as_mut() {
            Some(r) => r,
            None => &mut w,
        };
        let mut nf = match build_faces(target, |t, fenv| create_face_from_surface(k, t, Some(copy), &bx, fenv)) {
            Ok(nf) => nf,
            Err(e) => return Ok(Err(status(e))),
        };
        target.refresh_cached_topology();
        let _ = target;
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
        // The face faces the way the old one did.
        let target: &mut Brep = match r.as_mut() {
            Some(r) => r,
            None => &mut w,
        };
        let now = upward(target, nf)?;
        if let (Some(a), Some(b)) = (was, now)
            && a != b
            && let Err(e) = face_p1::flip_face_orientation(target, nf)
        {
            return Ok(Err(status(e)));
        }
        if let Some(r) = r {
            return Ok(Ok(r));
        }
        // The old face goes, with the edges and vertices only it used: an edit of the brep,
        // which SMLib allows only in edit mode.
        let editing = w.editing_enabled();
        w.set_editing_enabled(true);
        let deleted = with_kernel_callbacks(k, &mut w, |k, w, env| {
            let mut marks = Marks::default();
            let options = DeleteFaceOptions { delete_edges_vertices: true, ..DeleteFaceOptions::default() };
            delete_face(k, w, env, &mut marks, f, options)
        });
        w.set_editing_enabled(editing);
        w.refresh_cached_topology();
        if let Err(e) = deleted {
            return Ok(Err(status(e)));
        }
        Ok(Ok(w))
    })?
    .map(Body::new)
}

/// One side of a face's parameter box (`Side`): which, whether it is a u = const line (the face
/// grows in u), and whether it is at the box's max.
#[derive(Clone, Copy)]
struct Side {
    which: SurfParamType,
    u: bool,
    high: bool,
}

/// `SIDES`.
const SIDES: [Side; 4] = [
    Side { which: SurfParamType::UMin, u: true, high: false },
    Side { which: SurfParamType::UMax, u: true, high: true },
    Side { which: SurfParamType::VMin, u: false, high: false },
    Side { which: SurfParamType::VMax, u: false, high: true },
];

/// `SAMPLES`.
const SAMPLES: usize = 9;

/// `on_side`: the point of side `s` of `dom` at `t` in [0, 1] along it, in UV.
fn on_side(dom: &Extent2d, s: Side, t: f64) -> Point2 {
    let (lo, hi) = (dom.min(), dom.max());
    if s.u {
        Point2::new(if s.high { hi.x } else { lo.x }, lo.y + t * (hi.y - lo.y))
    } else {
        Point2::new(lo.x + t * (hi.x - lo.x), if s.high { hi.y } else { lo.y })
    }
}

/// `eval`: the point at `uv` with its u and v derivatives; `None` where it does not evaluate or
/// is not finite.
fn eval(k: &Kernel, surf: &Surface, uv: Point2) -> Option<(Vec3, Vec3, Vec3)> {
    let d = surf.evaluate(k, uv, 1, 1, EvaluateFlags::default()).ok()?;
    let p = *d.first()?.first()?;
    let du = *d.get(1)?.first()?;
    let dv = *d.first()?.get(1)?;
    (p.length().is_finite() && du.length().is_finite() && dv.length().is_finite()).then_some((p, du, dv))
}

fn point_at(k: &Kernel, surf: &Surface, uv: Point2) -> Option<Vec3> {
    eval(k, surf, uv).map(|(p, _, _)| p)
}

/// `edge_distance`.
fn edge_distance(k: &Kernel, b: &Brep, e: EdgeId, p: Vec3) -> f64 {
    match edge_closest_point(k, b, e, p) {
        Ok(c) if c.closest.distance.is_finite() => c.closest.distance,
        _ => f64::INFINITY,
    }
}

/// `nearest_edge`: the distance to the nearest of `edges`, with that edge.
fn nearest_edge(k: &Kernel, b: &Brep, edges: &[EdgeId], p: Vec3) -> (f64, Option<EdgeId>) {
    let (mut best, mut which) = (f64::INFINITY, None);
    for &e in edges {
        let d = edge_distance(k, b, e, p);
        if d < best {
            best = d;
            which = Some(e);
        }
    }
    (best, which)
}

/// `is_boundary`: whether side `s` of the face's box is a side of the face (its samples on the
/// face's edges), and whether it collapses to a point (a pole).
fn is_boundary(k: &Kernel, b: &Brep, surf: &Surface, dom: &Extent2d, s: Side, edges: &[EdgeId], tol: f64) -> (bool, bool) {
    let (Some(a), Some(z), Some(m)) =
        (point_at(k, surf, on_side(dom, s, 0.0)), point_at(k, surf, on_side(dom, s, 1.0)), point_at(k, surf, on_side(dom, s, 0.5)))
    else {
        return (false, false);
    };
    if (a - z).length() <= tol && (a - m).length() <= tol {
        return (true, true);
    }
    let all = (0..=SAMPLES).all(|i| match point_at(k, surf, on_side(dom, s, i as f64 / SAMPLES as f64)) {
        Some(p) => nearest_edge(k, b, edges, p).0 <= tol,
        None => false,
    });
    (all, false)
}

/// `side_edge`: the edge of `edges` side `s` of `dom` runs along (nearest the side's middle), or
/// `None` for a side collapsed to a point.
fn side_edge(k: &Kernel, b: &Brep, surf: &Surface, dom: &Extent2d, s: Side, edges: &[EdgeId], on: f64) -> Option<EdgeId> {
    let a = point_at(k, surf, on_side(dom, s, 0.0))?;
    let z = point_at(k, surf, on_side(dom, s, 1.0))?;
    let m = point_at(k, surf, on_side(dom, s, 0.5))?;
    if (a - z).length() <= on && (a - m).length() <= on {
        return None;
    }
    let (d, e) = nearest_edge(k, b, edges, m);
    e.filter(|_| d <= on)
}

/// `across`: how far `ext` (the face's surface extended past side `s` of its box `dom`, on the
/// same parameters) runs past that side, measured across it through the side's middle, by
/// 16-point Gauss-Legendre quadrature on 64 pieces. Negative when it can't be evaluated.
fn across(k: &Kernel, ext: &Surface, dom: &Extent2d, s: Side) -> f64 {
    const X: [f64; 8] = [
        0.0950125098376374,
        0.2816035507792589,
        0.4580167776572274,
        0.6178762444026438,
        0.755404408355003,
        0.8656312023878318,
        0.9445750230732326,
        0.9894009349916499,
    ];
    const W: [f64; 8] = [
        0.1894506104550685,
        0.1826034150449236,
        0.1691565193950025,
        0.1495959888165767,
        0.1246289712555339,
        0.0951585116824928,
        0.0622535239386479,
        0.0271524594117541,
    ];
    const PIECES: usize = 64;
    let Ok(bx) = ext.natural_uv_domain_with_nurb(k) else { return -1.0 };
    let mid = on_side(dom, s, 0.5);
    let from = if s.u { mid.x } else { mid.y };
    let to = match (s.u, s.high) {
        (true, true) => bx.max().x,
        (true, false) => bx.min().x,
        (false, true) => bx.max().y,
        (false, false) => bx.min().y,
    };
    let h = (to - from) / PIECES as f64;
    let mut length = 0.0;
    for piece in 0..PIECES {
        let c = from + (piece as f64 + 0.5) * h;
        for i in 0..16 {
            let (x, w) = if i < 8 { (-X[i], W[i]) } else { (X[i - 8], W[i - 8]) };
            let t = c + 0.5 * h * x;
            let uv = if s.u { Point2::new(t, mid.y) } else { Point2::new(mid.x, t) };
            let Some((_, du, dv)) = eval(k, ext, uv) else { return -1.0 };
            length += 0.5 * h.abs() * w * if s.u { du } else { dv }.length();
        }
    }
    length
}

/// `cone_continued`: a cone's or cylinder's face continued naturally `distance` past side `s` of
/// its box `dom`, as the analytic itself over a wider STEP domain (degrees round the axis,
/// height along it). `None` where the cone can't be widened so (more than a turn, or past the
/// apex).
fn cone_continued(k: &Kernel, surf: &Surface, dom: &Extent2d, s: Side, distance: f64) -> Option<Surface> {
    let SurfaceKind::Cone(cone) = surf.kind() else { return None };
    let mut cone: Cone = cone.as_ref().clone();
    let step = |uv: Point2| cone.convert_uv_from_nurbs_to_step(k, uv).ok();
    let (a, z, m) = (step(on_side(dom, s, 0.0))?, step(on_side(dom, s, 1.0))?, step(on_side(dom, s, 0.5))?);
    let (lo, hi) = (step(dom.min())?, step(dom.max())?);
    let semi = cone.canonical().semi_angle_deg;
    let mut bx = [[lo.x.min(hi.x), lo.x.max(hi.x)], [lo.y.min(hi.y), lo.y.max(hi.y)]];
    let eps = 1e-9 * (1.0 + m.x.abs() + m.y.abs());
    // The STEP coordinate constant along the side is the one the face grows in.
    let c = if (a.x - z.x).abs() <= eps {
        0
    } else if (a.y - z.y).abs() <= eps {
        1
    } else {
        return None;
    };
    let at = if c == 0 { m.x } else { m.y };
    let row = bx.get_mut(c)?;
    let end = if (at - row[1]).abs() <= (at - row[0]).abs() { 1 } else { 0 };
    let by = if c == 0 {
        let r = cone.radius(m.y);
        if !(r > 0.0) {
            return None;
        }
        (distance / r).to_degrees()
    } else {
        distance * semi.to_radians().cos()
    };
    if end == 1 {
        row[1] += by;
    } else {
        row[0] -= by;
    }
    if bx[0][1] - bx[0][0] > 360.0 + 1e-9 {
        return None;
    }
    let wider = Extent2d::from_bounds(bx[0][0], bx[1][0], bx[0][1], bx[1][1]);
    cone.adjust_step_uv_domain(k, &wider).ok()?;
    Some(Surface::from(cone))
}

/// `same_patch`: whether `p` and `q` span the same patch: their boxes' corners the same points,
/// in any order, within `tol`.
fn same_patch(k: &Kernel, p: &Surface, q: &Surface, tol: f64) -> bool {
    let (Ok(bp), Ok(bq)) = (p.natural_uv_domain_with_nurb(k), q.natural_uv_domain_with_nurb(k)) else { return false };
    let corners = |b: &Extent2d| {
        let (lo, hi) = (b.min(), b.max());
        [lo, Point2::new(hi.x, lo.y), hi, Point2::new(lo.x, hi.y)]
    };
    corners(&bp).into_iter().all(|u| {
        let Some(x) = point_at(k, p, u) else { return false };
        corners(&bq).into_iter().any(|v| point_at(k, q, v).is_some_and(|y| (x - y).length() <= tol))
    })
}

/// `extended_by`: the surface `surf` (whose face is its whole box `dom`) extended past side `s` as
/// `ct` says, so that it runs `distance` past that side measured across it ([`across`]), within
/// 1e-9 of it. `CreateExtendedSurface` takes its distance as an estimate, and how far it then runs
/// is increasing in what it is asked: a bracketed solve on that (each ask scaled by the distance
/// over what it gave until two asks fall either side, then Illinois false position). An extension
/// that moves the face's side off its parameters, or a target not met, is [`Error::NotExact`],
/// never the surface as it came out.
fn extended_by(k: &Kernel, surf: &Surface, dom: &Extent2d, s: Side, ct: ContinuityType, distance: f64, model_tol: f64) -> Result<Surface> {
    const BRACKET_TRIES: usize = 40;
    const SOLVE_TRIES: usize = 100;
    let tol = 1e-9 * distance;
    // How far asking `ask` runs, and the surface it makes.
    let try_ask = |ask: f64| -> Result<(Surface, f64)> {
        let tried = surf.create_extended_surface_dir(k, s.which, ask, ct).map_err(status)?;
        // The face's side stays where it was on the extended surface's parameters.
        let mid = on_side(dom, s, 0.5);
        let stays = matches!((point_at(k, surf, mid), point_at(k, &tried, mid)), (Some(was), Some(now)) if (was - now).length() <= model_tol);
        if !stays {
            return Err(Error::NotExact);
        }
        let got = across(k, &tried, dom, s);
        if got.is_finite() && got > 0.0 { Ok((tried, got)) } else { Err(Error::NotExact) }
    };
    // Asks that run short of and past `distance`, by f = got - distance.
    let (mut short, mut past) = (None, None);
    let mut ask = distance;
    for _ in 0..BRACKET_TRIES {
        let (made, got) = try_ask(ask)?;
        let f = got - distance;
        if f.abs() <= tol {
            return Ok(made);
        }
        if f < 0.0 {
            short = Some((ask, f));
        } else {
            past = Some((ask, f));
        }
        if short.is_some() && past.is_some() {
            break;
        }
        ask *= distance / got;
    }
    let (Some((mut lo, mut flo)), Some((mut hi, mut fhi))) = (short, past) else { return Err(Error::NotExact) };
    // Illinois: false position, halving the value at the end that stays when the same end moves
    // twice running.
    #[derive(PartialEq)]
    enum Moved {
        Neither,
        Lo,
        Hi,
    }
    let mut moved = Moved::Neither;
    for _ in 0..SOLVE_TRIES {
        let x = hi - fhi * (hi - lo) / (fhi - flo);
        if !(x > lo.min(hi) && x < lo.max(hi)) {
            break;
        }
        let (made, got) = try_ask(x)?;
        let f = got - distance;
        if f.abs() <= tol {
            return Ok(made);
        }
        if f < 0.0 {
            (lo, flo) = (x, f);
            if moved == Moved::Lo {
                fhi *= 0.5;
            }
            moved = Moved::Lo;
        } else {
            (hi, fhi) = (x, f);
            if moved == Moved::Hi {
                flo *= 0.5;
            }
            moved = Moved::Hi;
        }
    }
    Err(Error::NotExact)
}

/// `smlib_extend_sheet`: the one-face sheet `sheet`, whose face is its surface's whole parameter
/// box, continued `distance` past the side its edge nearest `point` (within `tol`) runs along,
/// as `how` says. A sheet of another shape is [`Error::Unsupported`]; an extension that cannot be
/// made `distance` long across the side (within 1e-9 of it) is [`Error::NotExact`].
pub(crate) fn extend_sheet(sheet: &Body, point: [f64; 3], tol: f64, distance: f64, how: Continuation) -> Result<Body> {
    if !point.iter().all(|x| x.is_finite()) || !tol.is_finite() || !(tol > 0.0) || !distance.is_finite() || !(distance > 0.0) {
        return Err(Error::Argument);
    }
    if sheet.0.is_poisoned() {
        return Err(Error::Internal("smlib_extend_sheet: the body was left unfinished by an earlier panic".to_owned()));
    }
    run("smlib_extend_sheet", |k| {
        let b = {
            let g = sheet.0.lock().unwrap_or_else(PoisonError::into_inner);
            super::dump::input(k, "sheet", &g);
            copy_of(k, &g, 1)?
        };
        let faces = brep_p1::faces(&b, None)?;
        let [f] = faces.as_slice() else { return Ok(Err(Error::Unsupported)) };
        let f = *f;
        let Some(surf) = face_p2::surface(&b, f)?.cloned() else { return Ok(Err(Error::Unsupported)) };
        if face_p2::loops(&b, f, false, None)?.len() != 1 {
            return Ok(Err(Error::Unsupported));
        }
        let edges = face_p4::edges(&b, f, None, false, None)?;
        let (d, picked) = nearest_edge(k, &b, &edges, p3(point));
        let Some(picked) = picked.filter(|_| d <= tol) else { return Ok(Err(Error::NoMatch { point: 0 })) };
        let model_tol = b.tolerance().max(1e-9);
        let on = 10.0 * model_tol;
        let dom = face_p2::uv_domain(&b, f)?;
        // Every side of the box is a side of the face (the face is its whole box), and the picked
        // edge is all of one side.
        let mut grow = None;
        for s in SIDES {
            let (boundary, collapsed) = is_boundary(k, &b, &surf, &dom, s, &edges, on);
            if !boundary {
                return Ok(Err(Error::Unsupported));
            }
            if collapsed {
                continue;
            }
            let all = (0..=SAMPLES).all(|i| match point_at(k, &surf, on_side(&dom, s, i as f64 / SAMPLES as f64)) {
                Some(p) => edge_distance(k, &b, picked, p) <= on,
                None => false,
            });
            if all {
                grow = Some(s);
            }
        }
        let Some(grow) = grow else { return Ok(Err(Error::Unsupported)) };
        // SMLib extends the whole surface; the face must be all of it.
        let natural = surf.natural_uv_domain_with_nurb(k)?;
        let (nl, nh, dl, dh) = (natural.min(), natural.max(), dom.min(), dom.max());
        let ptol = 1e-9 * 1f64.max((nh.x - nl.x).max(nh.y - nl.y));
        if (nl.x - dl.x).abs() > ptol || (nl.y - dl.y).abs() > ptol || (nh.x - dh.x).abs() > ptol || (nh.y - dh.y).abs() > ptol {
            return Ok(Err(Error::Unsupported));
        }
        let ct = match how {
            Continuation::Tangent => ContinuityType::G1,
            Continuation::Curvature => ContinuityType::G1G2,
            Continuation::Natural => ContinuityType::CInfinity,
        };
        // CreateExtendedSurface takes its distance as an estimate (a natural parabola falls 2.5%
        // short, a G2 run-off of a circle runs more than twice as far): the distance asked of it
        // is solved for until the extension measures `distance` across the side's middle.
        let extended = match extended_by(k, &surf, &dom, grow, ct, distance, model_tol) {
            Ok(e) => e,
            Err(e) => return Ok(Err(e)),
        };
        // A cone or cylinder continued naturally stays one, as the analytic widened, else SMLib
        // gives it back its analytic kind where it can.
        let grown = match how {
            Continuation::Natural => cone_continued(k, &surf, &dom, grow, distance).filter(|g| same_patch(k, g, &extended, 10.0 * model_tol)),
            _ => None,
        };
        let grown = match grown {
            Some(g) => g,
            None => match extended.copy_and_add_analytics(k) {
                Ok(g) => g,
                Err(e) => return Ok(Err(status(e))),
            },
        };
        let bx = grown.natural_uv_domain_with_nurb(k)?;
        let mut r = new_brep(k, USE_DEFAULT, USE_DEFAULT)?;
        r.set_tolerance(b.tolerance());
        let grown_eval = grown.copy(k)?;
        let nf = match build_faces(&mut r, |t, fenv| create_face_from_surface(k, t, Some(grown), &bx, fenv)) {
            Ok(nf) => nf,
            Err(e) => return Ok(Err(status(e))),
        };
        r.refresh_cached_topology();
        // The face and each side stand for the face and side they continue.
        copy_attributes(&b, EntityRef::Face(f), &mut r, EntityRef::Face(nf))?;
        let made = face_p4::edges(&r, nf, None, false, None)?;
        let new_on = 10.0 * r.tolerance().max(1e-9);
        for s in SIDES {
            let from = side_edge(k, &b, &surf, &dom, s, &edges, on);
            let to = side_edge(k, &r, &grown_eval, &bx, s, &made, new_on);
            if let (Some(from), Some(to)) = (from, to) {
                copy_attributes(&b, EntityRef::Edge(from), &mut r, EntityRef::Edge(to))?;
            }
        }
        // The new face faces the way the old one did.
        if let (Some(was), Some(now)) = (upward(&b, f)?, upward(&r, nf)?)
            && was != now
            && let Err(e) = face_p1::flip_face_orientation(&mut r, nf)
        {
            return Ok(Err(status(e)));
        }
        Ok(Ok(r))
    })?
    .map(Body::new)
}

/// A point inside each face, in the body's face order (`smlib_brep_face_points`:
/// `SmFace::CalculateAnInternalPoint`).
pub(crate) fn face_points(body: &Body) -> Result<Vec<[f64; 3]>> {
    super::with_body("smlib_brep_face_points", body, |k, b| {
        let faces = b.face_list().to_vec();
        faces.into_iter().map(|f| face_internal_point(k, b, f).map(|p| [p.x, p.y, p.z])).collect()
    })
}
