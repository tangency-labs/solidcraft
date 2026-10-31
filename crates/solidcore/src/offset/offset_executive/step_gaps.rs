//! The loop gaps an extended offset leaves and their closing edges (opt-in, fork
//! tangency-labs/smlib 5d428ae, 7ff0a94, 34318f4, ccbf77a).
//!
//! Upstream: the SolidCraft fork's `source/SMLib/src/SmOffsetExecutive.cpp` (not in the
//! pinned SMLib).
//!
//! | Rust                                               | Upstream (fork)                                    |
//! |----------------------------------------------------|----------------------------------------------------|
//! | [`OffsetExecutive::close_loop_gaps`]               | `SmOffsetExecutive::CloseLoopGaps`                 |
//! | `close_loop_gap` (private)                         | `SmOffsetExecutive::CloseLoopGap`                  |
//! | [`OffsetExecutive::split_edges_at_step_corners`]   | `SmOffsetExecutive::SplitEdgesAtStepCorners`       |
//!
//! Deviations: the offset B-rep is taken out of the executive for the edits and put back
//! on every return. `MakeVertexSplitEdge`'s `SM_ERR` (a split refused near an end) is a
//! successful Rust return with `split == false`; it fails here as the fork's `SER` does.
//! `GetCreateExtendedSurface` keys the face's surface by the result identity of its face,
//! as the executive's edge offsets do. The fork's `m_sClosedEdgeFaces` /
//! `m_sClosedEdgeAcross` (ccbf77a), filled by `CloseLoopGap` and read once at the end of
//! the same `CloseLoopGaps`, are one local list of pairs. Bounds: each face's gap search stops with `SM_ERR`
//! after 64 passes (the fork's guard); the other loops run over finite lists.
//!
//! Not in the fork: `split_corner_vertices` (private, opt-in
//! `Kernel::fixes.offset_splits_corner_vertices`, WR-657), run by `close_loop_gaps`
//! before it closes any gap.

use std::collections::{BTreeMap, BTreeSet};

use super::p1::{forget_seconds, OffsetExecutive};
use super::p2::OffsetNotifications;
use crate::curve::curve::p1::GlobalPointSolveOptions;
use crate::curve::curve::{Curve, CurveKind};
use crate::error::{EntityRef, Result, SolidError};
use crate::geom::extent1d::Extent1d;
use crate::geom::global_solver::{SolutionRequested, SolverOperation};
use crate::kernel::Kernel;
use crate::math::{max2, Point3, Vec3, EFF_ZERO, EFF_ZERO_SQRT};
use crate::nurbs::b_spline_curve::BSplineCurve;
use crate::nurbs::b_spline_curve_n_lib::TrimMode;
use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
use crate::query::edge::closest_point;
use crate::surface::surface::p2::SurfaceKind;
use crate::surface::surface::p4::{global_point_solve_starts, NoObject};
use crate::topology::brep::p1 as bp1;
use crate::topology::brep::p3::{delete_face_with_body_identity, squeeze_edge, DeleteFaceOptions};
use crate::topology::brep::p9::make_vertex_split_edge_with_state;
use crate::topology::edge::{self, NotificationStores, NotifyState, SetCurveOptions};
use crate::topology::edgeuse::{p1 as eu, p3 as eu3};
use crate::topology::face::{p2 as face2, p3 as face3, p4 as face4};
use crate::topology::{vertexuse, Brep, EdgeId, EdgeuseId, FaceId, VertexId};

const OP: &str = "offset::offset_executive::step_gaps";

/// `SM_ERR_OFFSET_EDGE_VANISHES` (the fork's `SmMessages.h`): an offset moves a vertex
/// along an edge past the edge's other end, a change of topology it does not make.
pub use crate::error::SM_ERR_OFFSET_EDGE_VANISHES;

/// The gap search's passes per face (the fork's `lGuard`).
const MAX_GAP_PASSES: usize = 64;

fn null(reason: &'static str) -> SolidError {
    SolidError::invalid_input(OP, reason)
}

fn sm_err() -> SolidError {
    SolidError::sm_err(OP)
}

/// The vertex of an edge use's vertex use (`GetVertexuse()->GetVertex()`).
fn edgeuse_vertex(b: &Brep, u: EdgeuseId) -> Result<VertexId> {
    let vu = eu::vertexuse(b, u)?.ok_or_else(|| null("edge use without vertex use"))?;
    vertexuse::vertex(b, vu)?.ok_or_else(|| null("vertex use without vertex"))
}

/// `pEU->NormalizedEvaluate(t, FALSE, rPoint)`.
fn point_at(k: &Kernel, b: &mut Brep, u: EdgeuseId, t: f64) -> Result<Point3> {
    Ok(eu3::normalized_evaluate(k, b, u, t, false, false, true)?.point)
}

/// The parameter and value of the first solution of `pCurve->GlobalPointSolve(ivl,
/// SM_SO_MINIMIZE, point, tol, NULL, NULL, SM_SR_SINGLE, ..)`.
fn closest_on(
    k: &Kernel,
    curve: &BSplineCurve,
    ivl: &Extent1d,
    point: Point3,
    tol: f64,
) -> Result<Option<(f64, f64)>> {
    let c = Curve::from(curve.clone());
    let sols = c.global_point_solve(
        k,
        ivl,
        SolverOperation::Minimize,
        point,
        tol,
        SolutionRequested::Single,
        &GlobalPointSolveOptions::<NoObject>::default(),
    )?;
    match sols.get(0) {
        Some(s) => Ok(Some((s.start.param(0)?, s.start.solution_value))),
        None => Ok(None),
    }
}

impl OffsetExecutive {
    /// `CloseLoopGaps` (fork, opt-in `Kernel::fixes.local_operation_step_walls` for a
    /// local operation, `Kernel::fixes.offset_closes_loop_gaps` for every extended
    /// offset): where consecutive edge uses of an offset face no longer meet at their
    /// shared vertex (a local operation's step left an edge behind; a solid offset's two
    /// consecutive edges whose offsets run parallel), the edge whose end lies away from the
    /// vertex gets the straight segment to it (which must lie on the face's extended
    /// surface) and is split there, or is trimmed to it when the vertex moved into it.
    /// Each face is searched again after each gap closed.
    ///
    /// # Errors
    ///
    /// `SM_ERR` for a gap neither end of which is at the vertex, a segment off the
    /// surface, or more than [`MAX_GAP_PASSES`] gaps on one face;
    /// [`SM_ERR_OFFSET_EDGE_VANISHES`] where the vertex moved to or past the edge's other
    /// end; those of the evaluations, solves, curve edits
    /// and the split; [`SolidError::InvalidInput`] for missing topology.
    ///
    /// With `Kernel::fixes.local_operation_closes_walls_to_nothing` (fork ccbf77a) an edge
    /// closed to a point is squeezed out instead, and once every gap is closed the faces
    /// that closed to nothing with such edges are deleted (see `remove_closed_faces`).
    pub(super) fn close_loop_gaps(
        &mut self,
        kernel: &mut Kernel,
        original: &Brep,
        notifications: &mut OffsetNotifications<'_>,
    ) -> Result<()> {
        let mut b = self
            .take_offset_brep()
            .ok_or_else(|| null("no offset B-rep"))?;
        let r = self.close_loop_gaps_in(kernel, original, &mut b, notifications);
        self.set_offset_brep(b);
        r
    }

    fn close_loop_gaps_in(
        &mut self,
        kernel: &mut Kernel,
        original: &Brep,
        b: &mut Brep,
        notifications: &mut OffsetNotifications<'_>,
    ) -> Result<()> {
        let approx_tol = self.approximation_tol_3d();
        // (WR-657, opt-in) the corners whose offset planes no longer meet in one point
        if kernel.fixes.offset_splits_corner_vertices {
            self.split_corner_vertices(kernel, original, b, approx_tol, notifications)?;
        }
        // (fork ccbf77a) each edge that closed to a point: its offset face and the original
        // face across it
        let mut closed_edges: Vec<(FaceId, Option<FaceId>)> = Vec::new();
        for f in bp1::faces(b, None)? {
            // Each gap closed changes the loop: look again until none is left (each gap
            // is closed once).
            let mut passes = 0;
            let mut closed = true;
            while closed {
                closed = false;
                passes += 1;
                if passes > MAX_GAP_PASSES {
                    return Err(sm_err());
                }
                let fu =
                    face2::upward_faceuse(b, f)?.ok_or_else(|| null("face without face use"))?;
                let loopuses = b.faceuse(fu)?.loopuses.clone();
                'loops: for lu in loopuses {
                    let eus = eu::loopuse_edgeuses(b, lu)?;
                    let num = eus.len();
                    for (kk, &this) in eus.iter().enumerate() {
                        let next = *eus
                            .get((kk + 1) % num)
                            .ok_or_else(|| null("loop without edge uses"))?;
                        if this == next {
                            continue;
                        }
                        let v = edgeuse_vertex(b, next)?;
                        let mate =
                            eu::mate(b, this)?.ok_or_else(|| null("edge use without mate"))?;
                        if edgeuse_vertex(b, mate)? != v {
                            continue;
                        }
                        let tol_this = b
                            .edge(eu::edge(b, this)?.ok_or_else(|| null("edge use without edge"))?)?
                            .tolerance;
                        let tol_next = b
                            .edge(eu::edge(b, next)?.ok_or_else(|| null("edge use without edge"))?)?
                            .tolerance;
                        let tol = 10.0 * max2(max2(tol_this, tol_next), approx_tol);
                        let end = point_at(kernel, b, this, 1.0)?;
                        let start = point_at(kernel, b, next, 0.0)?;
                        if end.distance(start) <= tol {
                            continue;
                        }
                        let sv = b.vertex(v)?.point;
                        if end.distance(sv) <= tol && start.distance(sv) > tol {
                            self.close_loop_gap(
                                kernel,
                                original,
                                b,
                                f,
                                next,
                                true,
                                sv,
                                tol,
                                notifications,
                                &mut closed_edges,
                            )?;
                        } else if start.distance(sv) <= tol && end.distance(sv) > tol {
                            self.close_loop_gap(
                                kernel,
                                original,
                                b,
                                f,
                                this,
                                false,
                                sv,
                                tol,
                                notifications,
                                &mut closed_edges,
                            )?;
                        } else {
                            return Err(sm_err());
                        }
                        closed = true;
                        break 'loops;
                    }
                }
            }
        }
        if !closed_edges.is_empty() {
            self.remove_closed_faces(kernel, original, b, &closed_edges, notifications)?;
        }
        Ok(())
    }

    /// The end of the fork's `CloseLoopGaps` (ccbf77a, opt-in
    /// `Kernel::fixes.local_operation_closes_walls_to_nothing`): the offset faces that
    /// closed to nothing as their edges closed to points (`IsDegenerate`: a step's wall
    /// whose low top moved onto the high top's plane, its edges along the tops now
    /// coinciding) are deleted, in the order they closed, with their pairs in the
    /// executive's relations; the merge joins the faces either side along their
    /// coinciding edges. Every closed edge must be one of theirs or lie across from one of
    /// them (the sides' edges along the wall).
    ///
    /// # Errors
    ///
    /// [`SM_ERR_OFFSET_EDGE_VANISHES`] for a closed edge of a face that did not close to
    /// nothing and across from none that did (a face turned inside out past it); those of
    /// `IsDegenerate`, the edge and face queries and `DeleteFace`.
    fn remove_closed_faces(
        &mut self,
        kernel: &mut Kernel,
        original: &Brep,
        b: &mut Brep,
        closed_edges: &[(FaceId, Option<FaceId>)],
        notifications: &mut OffsetNotifications<'_>,
    ) -> Result<()> {
        // offset faces closed to nothing, in the order they closed, and the original faces
        // they were offset from
        let mut gone_list: Vec<FaceId> = Vec::new();
        let mut gone: BTreeSet<FaceId> = BTreeSet::new();
        let mut gone_orig: BTreeSet<FaceId> = BTreeSet::new();
        for &(face, _) in closed_edges {
            if gone.contains(&face) || !face3::is_degenerate(kernel, b, face, false, None)? {
                continue;
            }
            gone.insert(face);
            gone_list.push(face);
            for fu in self.faceuse_to_faces.firsts(face) {
                if let Some(orig) = eu::faceuse_face(original, fu)? {
                    gone_orig.insert(orig);
                }
            }
        }
        for &(face, across) in closed_edges {
            let explained = gone.contains(&face) || across.is_some_and(|a| gone_orig.contains(&a));
            if !explained {
                return Err(SolidError::Upstream {
                    operation: OP,
                    code: SM_ERR_OFFSET_EDGE_VANISHES,
                    entity: None,
                });
            }
        }
        let mut gone_eus: BTreeSet<EdgeuseId> = BTreeSet::new();
        let body = notifications.result_brep;
        for &face in &gone_list {
            for e in face4::edges(b, face, None, false, None)? {
                gone_eus.extend(edge::edgeuses(b, e, None, None)?);
            }
            let mut stores = NotificationStores {
                records: &mut *notifications.records,
                attributes: &mut *notifications.attributes,
                key_of: notifications.result_key_of,
            };
            NotifyState::from_kernel_stores(&mut stores).with_kernel_env(
                kernel,
                |kernel, env| {
                    delete_face_with_body_identity(
                        kernel,
                        b,
                        env,
                        face,
                        DeleteFaceOptions {
                            delete_edges_vertices: true,
                            do_region_nesting: true,
                            region_to_keep: None,
                        },
                        body,
                    )
                },
            )?;
        }
        forget_seconds(&mut self.edgeuse_to_edgeuses, &gone_eus, None)?;
        self.forget_faces(&gone)
    }

    /// `CloseLoopGap(pFace, pEdgeuse, bAtStart, crVertexPoint, dTol)`: one gap closed
    /// (see [`OffsetExecutive::close_loop_gaps`]).
    #[allow(clippy::too_many_arguments, clippy::too_many_lines)] // one fork routine
    fn close_loop_gap(
        &mut self,
        kernel: &mut Kernel,
        original: &Brep,
        b: &mut Brep,
        f: FaceId,
        u: EdgeuseId,
        at_start: bool,
        vertex_point: Point3,
        tol: f64,
        notifications: &mut OffsetNotifications<'_>,
        closed_edges: &mut Vec<(FaceId, Option<FaceId>)>,
    ) -> Result<()> {
        let e = eu::edge(b, u)?.ok_or_else(|| null("edge use without edge"))?;
        let surface = b
            .face(f)?
            .surface
            .as_ref()
            .ok_or_else(|| null("face without surface"))?
            .clone();

        // The edge curve's end at the gap: its start where the edge use runs with it and
        // starts there.
        let same = eu::orientation(b, u)? == OrientType::Same;
        let at_curve_start = at_start == same;
        let trimmed = edge::create_trimmed_nurbs_curve(kernel, b, e)?
            .ok_or_else(|| null("edge without NURBS curve"))?;
        let mut curve = match trimmed.kind() {
            CurveKind::BSpline(c) => c.clone(),
            _ => return Err(null("trimmed NURBS curve is not a B-spline")),
        };
        let ivl = curve.natural_interval();
        let gap_end = curve.evaluate_point(if at_curve_start { ivl.min() } else { ivl.max() })?;

        // A vertex that moved along the edge into it (a step down): the edge is trimmed to
        // it.
        if let Some((at, value)) = closest_on(kernel, &curve, &ivl, vertex_point, tol)? {
            if value <= tol {
                let mut keep = if at_curve_start {
                    Extent1d::from_min_max(at, ivl.max())
                } else {
                    Extent1d::from_min_max(ivl.min(), at)
                };
                let long_enough = keep.length() > EFF_ZERO * (1.0 + ivl.max_dimension());
                if !long_enough {
                    // (fork ccbf77a, opt-in) the vertex moved to the edge's other end, where
                    // the edge's other vertex is: the edge closed to a point. That is sound
                    // only where a face closes to nothing with it, which `close_loop_gaps`
                    // checks once every gap is closed. The edge is squeezed out, its two
                    // vertices one, and noted with the original face across it.
                    if kernel.fixes.local_operation_closes_walls_to_nothing
                        && self.squeeze_closed_edge(
                            kernel,
                            original,
                            b,
                            f,
                            u,
                            e,
                            at_start,
                            vertex_point,
                            tol,
                            notifications,
                            closed_edges,
                        )?
                    {
                        return Ok(());
                    }
                    // (fork 34318f4) elsewhere the vertex moved past the edge's end: the edge
                    // vanishes, the face turns inside out
                    return Err(SolidError::Upstream {
                        operation: OP,
                        code: SM_ERR_OFFSET_EDGE_VANISHES,
                        entity: None,
                    });
                }
                curve.trim(kernel, &mut keep, TrimMode::Notify)?;
                let new_ivl = curve.natural_interval();
                set_edge_curve(kernel, b, e, curve, notifications)?;
                edge::set_interval(b, e, &new_ivl)?;
                return Ok(());
            }
        }

        // The straight segment must lie on the face's surface (as extended for the offset:
        // the face's own surface ends at its old boundary).
        let key = (notifications.result_key_of)(EntityRef::Face(f)).key.0;
        let ext = self
            .get_create_extended_surface(kernel, key, &surface)?
            .clone();
        let ext_domain = ext.natural_uv_domain_with_nurb(kernel)?;
        for ii in 1..4 {
            let w = f64::from(ii) / 4.0;
            let pnt = vertex_point + (gap_end - vertex_point) * w;
            let sols = global_point_solve_starts(
                kernel,
                &ext,
                &ext_domain,
                SolverOperation::Minimize,
                pnt,
                tol,
                SolutionRequested::Single,
            )?;
            let on = sols.first().is_some_and(|s| s.solution_value <= tol);
            if !on {
                return Err(sm_err());
            }
        }

        // The segment, from the vertex to the gap end, joined at the gap end.
        let mut segment = BSplineCurve::create_line_segment(3, vertex_point, gap_end)?;
        curve.join_with(kernel, !at_curve_start, &mut segment, true, Some(tol))?;

        // Where they join: the parameter of the gap end on the joined curve.
        let joined = curve.natural_interval();
        let joint = closest_on(kernel, &curve, &joined, gap_end, tol)?
            .ok_or_else(sm_err)?
            .0;

        // The edge takes the joined curve and is split at the joint.
        set_edge_curve(kernel, b, e, curve, notifications)?;
        edge::set_interval(b, e, &joined)?;
        let mut stores = NotificationStores {
            records: &mut *notifications.records,
            attributes: &mut *notifications.attributes,
            key_of: notifications.result_key_of,
        };
        let mut state = NotifyState::from_kernel_stores(&mut stores);
        let output = make_vertex_split_edge_with_state(
            kernel,
            b,
            e,
            joint,
            &mut state,
            notifications.result_curve_ref,
        )?;
        if !output.split {
            return Err(sm_err());
        }
        output.vertex.ok_or_else(|| null("split without vertex"))?;
        Ok(())
    }

    /// `CloseLoopGap`'s closed edge (fork ccbf77a): when the edge's other vertex is at
    /// `vertex_point`, the edge is squeezed into the vertex at the gap and noted with
    /// `face` and the original face across it (`None` where the edge use maps to no single
    /// original edge use); `false` (nothing done) otherwise.
    #[allow(clippy::too_many_arguments)] // one fork branch
    fn squeeze_closed_edge(
        &mut self,
        kernel: &mut Kernel,
        original: &Brep,
        b: &mut Brep,
        face: FaceId,
        u: EdgeuseId,
        e: EdgeId,
        at_start: bool,
        vertex_point: Point3,
        tol: f64,
        notifications: &mut OffsetNotifications<'_>,
        closed_edges: &mut Vec<(FaceId, Option<FaceId>)>,
    ) -> Result<bool> {
        let at = if at_start {
            edgeuse_vertex(b, u)?
        } else {
            edgeuse_vertex(
                b,
                eu::mate(b, u)?.ok_or_else(|| null("edge use without mate"))?,
            )?
        };
        let Some(far) = edge::other_vertex(b, e, Some(at))? else {
            return Ok(false);
        };
        if far == at || b.vertex(far)?.point.distance(vertex_point) > tol {
            return Ok(false);
        }
        let mut across = None;
        let mut orig_eus = self.edgeuse_to_edgeuses.firsts(u);
        if orig_eus.is_empty() {
            let mate = eu::mate(b, u)?.ok_or_else(|| null("edge use without mate"))?;
            orig_eus = self.edgeuse_to_edgeuses.firsts(mate);
        }
        if let [orig_eu] = orig_eus.as_slice() {
            let orig_edge =
                eu::edge(original, *orig_eu)?.ok_or_else(|| null("edge use without edge"))?;
            let orig_face = eu::face(original, *orig_eu)?;
            for of in edge::faces(original, orig_edge, None)? {
                if Some(of) != orig_face {
                    across = Some(of);
                }
            }
        }
        let mut stores = NotificationStores {
            records: &mut *notifications.records,
            attributes: &mut *notifications.attributes,
            key_of: notifications.result_key_of,
        };
        NotifyState::from_kernel_stores(&mut stores)
            .with_kernel_env(kernel, |kernel, env| squeeze_edge(kernel, b, env, e, at))?;
        closed_edges.push((face, across));
        Ok(true)
    }

    /// `SplitEdgesAtStepCorners` (fork 7ff0a94, opt-in
    /// `Kernel::fixes.local_operation_step_walls`, local operations only): every edge of
    /// the offset B-rep that runs past a corner of a step wall (within ten times its
    /// tolerance or the approximation tolerance, and not at its own ends) is split there,
    /// so that each part meets its own neighbour. In a local operation the only cap faces
    /// are step walls.
    ///
    /// # Errors
    ///
    /// `SM_ERR` for a refused split; those of the closest point and the split;
    /// [`SolidError::InvalidInput`] for missing topology.
    pub(super) fn split_edges_at_step_corners(
        &mut self,
        kernel: &mut Kernel,
        notifications: &mut OffsetNotifications<'_>,
    ) -> Result<()> {
        let mut b = self
            .take_offset_brep()
            .ok_or_else(|| null("no offset B-rep"))?;
        let r = self.split_edges_at_step_corners_in(kernel, &mut b, notifications);
        self.set_offset_brep(b);
        r
    }

    fn split_edges_at_step_corners_in(
        &mut self,
        kernel: &mut Kernel,
        b: &mut Brep,
        notifications: &mut OffsetNotifications<'_>,
    ) -> Result<()> {
        let approx_tol = self.approximation_tol_3d();
        let walls: Vec<FaceId> = self.edge_to_faces.all_seconds();
        let mut corners: Vec<VertexId> = Vec::new();
        for w in walls {
            for v in face4::vertices(b, w, None, false, None)? {
                if !corners.contains(&v) {
                    corners.push(v);
                }
            }
        }
        for v in corners {
            let p = b.vertex(v)?.point;
            let edges: Vec<EdgeId> = bp1::edges(b, None)?;
            for e in edges {
                let tol = 10.0 * max2(b.edge(e)?.tolerance, approx_tol);
                let start = edge::start_vertex(b, e)?.ok_or_else(|| null("edge without vertex"))?;
                let end = edge::end_vertex(b, e)?.ok_or_else(|| null("edge without vertex"))?;
                if b.vertex(start)?.point.distance(p) <= tol
                    || b.vertex(end)?.point.distance(p) <= tol
                {
                    continue;
                }
                let Some(cp) = closest_point(kernel, b, e, p)? else {
                    continue;
                };
                let near = cp.distance <= tol;
                if !near {
                    continue;
                }
                let mut stores = NotificationStores {
                    records: &mut *notifications.records,
                    attributes: &mut *notifications.attributes,
                    key_of: notifications.result_key_of,
                };
                let mut state = NotifyState::from_kernel_stores(&mut stores);
                let output = make_vertex_split_edge_with_state(
                    kernel,
                    b,
                    e,
                    cp.parameter,
                    &mut state,
                    notifications.result_curve_ref,
                )?;
                if !output.split {
                    return Err(sm_err());
                }
            }
        }
        Ok(())
    }
}

/// The point where three planes `(point, normal)` meet; `None` where two are parallel.
fn three_planes(planes: [(Point3, Vec3); 3]) -> Option<Point3> {
    let [(p1, n1), (p2, n2), (p3, n3)] = planes;
    let det = n1.dot(n2.cross(n3));
    if det.abs() < EFF_ZERO {
        return None;
    }
    let sum = n2.cross(n3) * n1.dot(p1) + n3.cross(n1) * n2.dot(p2) + n1.cross(n2) * n3.dot(p3);
    Some(sum * (1.0 / det))
}

/// How a corner of the original splits in the offset (see
/// [`OffsetExecutive::split_corner_vertices`]): the edges on face `b` end where `a`,
/// `b` and `c` meet (`t_b`), the others where `c`, `d` and `a` meet (`t_d`).
struct CornerSplit {
    b: FaceId,
    t_b: Point3,
    t_d: Point3,
}

/// The split of the original's vertex `v0` (see
/// [`OffsetExecutive::split_corner_vertices`]), `None` where `v0` is no convex corner
/// of four plane faces or its four offset planes still meet in one point (to `tol`).
fn corner_split(
    original: &Brep,
    b: &Brep,
    v0: VertexId,
    sheet_of: &BTreeMap<FaceId, FaceId>,
    tol: f64,
) -> Result<Option<CornerSplit>> {
    let mut edges = crate::topology::vertex::edges(original, v0, None)?;
    edges.sort_unstable();
    edges.dedup();
    if edges.len() != 4 {
        return Ok(None);
    }
    let p0 = original.vertex(v0)?.point;
    let mut faces: Vec<FaceId> = Vec::new();
    let mut dirs: Vec<Vec3> = Vec::new();
    let mut pairs: Vec<[FaceId; 2]> = Vec::new();
    for &e in &edges {
        let Some(other) = edge::other_vertex(original, e, Some(v0))? else {
            return Ok(None);
        };
        let Ok(dir) = (original.vertex(other)?.point - p0).unitize() else {
            return Ok(None);
        };
        let [f1, f2] = edge::faces(original, e, None)?[..] else {
            return Ok(None);
        };
        if f1 == f2 {
            return Ok(None);
        }
        for f in [f1, f2] {
            if !faces.contains(&f) {
                faces.push(f);
            }
        }
        dirs.push(dir);
        pairs.push([f1, f2]);
    }
    if faces.len() != 4 {
        return Ok(None);
    }
    // the offset planes, each normal turned out of the corner (the edges run into it)
    let inward = dirs
        .iter()
        .fold(Vec3::new(0.0, 0.0, 0.0), |sum, &d| sum + d);
    let mut planes: Vec<(Point3, Vec3)> = Vec::new();
    for f in &faces {
        let Some(sheet) = sheet_of.get(f) else {
            return Ok(None);
        };
        let Some(surface) = b.face(*sheet)?.surface.as_ref() else {
            return Ok(None);
        };
        let SurfaceKind::Plane(plane) = surface.kind() else {
            return Ok(None);
        };
        let (point, normal, _) = plane.is_planar();
        let side = normal.dot(inward);
        if side.abs() < EFF_ZERO {
            return Ok(None);
        }
        planes.push((point, if side > 0.0 { normal * -1.0 } else { normal }));
    }
    // a convex corner: every edge runs inside every face's plane
    if dirs
        .iter()
        .any(|&d| planes.iter().any(|&(_, n)| n.dot(d) > EFF_ZERO_SQRT))
    {
        return Ok(None);
    }
    // `a` and `c` across the corner (no edge between them), both ends inside the fourth
    // face
    let inside = |p: Point3, (at, n): (Point3, Vec3)| n.dot(p - at) < -tol;
    for (a, c, bb, d) in [
        (0, 1, 2, 3),
        (0, 2, 1, 3),
        (0, 3, 1, 2),
        (1, 2, 0, 3),
        (1, 3, 0, 2),
        (2, 3, 0, 1),
    ] {
        let (Some(&fa), Some(&fb), Some(&fc)) = (faces.get(a), faces.get(bb), faces.get(c)) else {
            return Ok(None);
        };
        let (Some(&pa), Some(&pb), Some(&pc), Some(&pd)) =
            (planes.get(a), planes.get(bb), planes.get(c), planes.get(d))
        else {
            return Ok(None);
        };
        if pairs.iter().any(|p| p.contains(&fa) && p.contains(&fc)) {
            continue;
        }
        let (Some(t_b), Some(t_d)) = (three_planes([pa, pb, pc]), three_planes([pc, pd, pa]))
        else {
            continue;
        };
        if inside(t_b, pd) && inside(t_d, pb) {
            return Ok(Some(CornerSplit { b: fb, t_b, t_d }));
        }
    }
    Ok(None)
}

/// The vertex the original's edges `e1` and `e2` share (one only).
fn common_vertex(original: &Brep, e1: EdgeId, e2: EdgeId) -> Result<Option<VertexId>> {
    let ends = |e| -> Result<[Option<VertexId>; 2]> {
        Ok([
            edge::start_vertex(original, e)?,
            edge::end_vertex(original, e)?,
        ])
    };
    let (a, c) = (ends(e1)?, ends(e2)?);
    let shared: Vec<VertexId> = a
        .iter()
        .flatten()
        .copied()
        .filter(|v| c.contains(&Some(*v)))
        .collect();
    Ok(match shared[..] {
        [v] => Some(v),
        _ => None,
    })
}

/// The ends of the straight edge `e` (its start and end), `None` where it is not
/// straight or closed.
fn straight_edge_ends(
    kernel: &mut Kernel,
    b: &mut Brep,
    e: EdgeId,
    tol: f64,
) -> Result<Option<(Point3, Point3)>> {
    if edge::start_vertex(b, e)? == edge::end_vertex(b, e)? {
        return Ok(None);
    }
    let Some(trimmed) = edge::create_trimmed_nurbs_curve(kernel, b, e)? else {
        return Ok(None);
    };
    let CurveKind::BSpline(curve) = trimmed.kind() else {
        return Ok(None);
    };
    if curve.is_line(5, tol)?.is_none() {
        return Ok(None);
    }
    let ivl = curve.natural_interval();
    Ok(Some((
        curve.evaluate_point(ivl.min())?,
        curve.evaluate_point(ivl.max())?,
    )))
}

/// One corner's planned edits (see [`OffsetExecutive::split_corner_vertices`]): the
/// offset edges' ends it moves (edge, its start or end, the new point) and the offset
/// vertices it moves.
#[derive(Default)]
struct CornerEdits {
    ends: Vec<(EdgeId, bool, Point3)>,
    vertices: Vec<(VertexId, Point3)>,
    /// One of its edges is not straight: the corner is left as it is.
    unusable: bool,
}

impl OffsetExecutive {
    /// Not in the fork (opt-in `Kernel::fixes.offset_splits_corner_vertices`, with
    /// `offset_closes_loop_gaps`, before the gaps are closed): a convex corner of the
    /// original where four plane faces meet, whose four offset planes no longer meet in
    /// one point, becomes an edge between the two faces across the corner from each
    /// other (`a` and `c`; the other two are `b` and `d`). Its ends are where `a`, `b`
    /// and `c` meet and where `c`, `d` and `a` meet, for whichever pair leaves both
    /// points inside the fourth face. The edge offsets end the four edges at the same
    /// pair of those points whichever way the corner splits: right for an outset of two
    /// chamfers meeting at a corner, outside the offset solid for the inset or a shell
    /// open at one of the four faces. Here each offset face's two edges at the corner
    /// are moved to their own ends of the split (along their lines) and the vertex
    /// between them to the nearer; `close_loop_gaps` then closes `a`'s and `c`'s
    /// loops with the straight edge between the two ends.
    ///
    /// The moves of every corner are planned first: an edge between two split corners
    /// moves at both ends. A corner one of whose edges would vanish (its ends meet or
    /// pass one another: more changes there than the one new edge, as where a small
    /// face is offset clear of the body) is left as it is, and the others planned again
    /// without it.
    ///
    /// # Errors
    ///
    /// Those of the topology queries and the curve evaluations and edits;
    /// [`SolidError::InvalidInput`] for missing topology.
    #[allow(clippy::too_many_lines)] // one pass over the offset faces, then the edits
    fn split_corner_vertices(
        &mut self,
        kernel: &mut Kernel,
        original: &Brep,
        b: &mut Brep,
        approx_tol: f64,
        notifications: &mut OffsetNotifications<'_>,
    ) -> Result<()> {
        // the offset face of each original face
        let mut sheet_of: BTreeMap<FaceId, FaceId> = BTreeMap::new();
        for f in bp1::faces(b, None)? {
            for fu in self.faceuse_to_faces.firsts(f) {
                if let Some(of) = eu::faceuse_face(original, fu)? {
                    sheet_of.insert(of, f);
                }
            }
        }
        let tol = 10.0 * max2(original.tolerance(), approx_tol);
        let mut corners: BTreeMap<VertexId, CornerSplit> = BTreeMap::new();
        for v0 in bp1::vertices(original, None)? {
            if let Some(split) = corner_split(original, b, v0, &sheet_of, tol)? {
                corners.insert(v0, split);
            }
        }
        if corners.is_empty() {
            return Ok(());
        }
        let mut plans: BTreeMap<VertexId, CornerEdits> = BTreeMap::new();
        let mut ends_of: BTreeMap<EdgeId, (Point3, Point3)> = BTreeMap::new();
        for f in bp1::faces(b, None)? {
            let Some(fu) = face2::upward_faceuse(b, f)? else {
                continue;
            };
            for lu in b.faceuse(fu)?.loopuses.clone() {
                let eus = eu::loopuse_edgeuses(b, lu)?;
                let num = eus.len();
                for (kk, &this) in eus.iter().enumerate() {
                    let next = *eus
                        .get((kk + 1) % num)
                        .ok_or_else(|| null("loop without edge uses"))?;
                    if this == next {
                        continue;
                    }
                    let v = edgeuse_vertex(b, next)?;
                    let mate = eu::mate(b, this)?.ok_or_else(|| null("edge use without mate"))?;
                    if edgeuse_vertex(b, mate)? != v {
                        continue;
                    }
                    let (Some(o_this), Some(o_next)) = (
                        self.original_edge(original, b, this)?,
                        self.original_edge(original, b, next)?,
                    ) else {
                        continue;
                    };
                    let Some(v0) = common_vertex(original, o_this, o_next)? else {
                        continue;
                    };
                    let Some(split) = corners.get(&v0) else {
                        continue;
                    };
                    let end_of = |e: EdgeId| -> Result<Point3> {
                        Ok(if edge::faces(original, e, None)?.contains(&split.b) {
                            split.t_b
                        } else {
                            split.t_d
                        })
                    };
                    let (t_this, t_next) = (end_of(o_this)?, end_of(o_next)?);
                    let plan = plans.entry(v0).or_default();
                    for (u, target) in [(this, t_this), (next, t_next)] {
                        let e = eu::edge(b, u)?.ok_or_else(|| null("edge use without edge"))?;
                        let Some(ends) = straight_edge_ends(kernel, b, e, tol)? else {
                            plan.unusable = true;
                            continue;
                        };
                        ends_of.insert(e, ends);
                        plan.ends
                            .push((e, edge::start_vertex(b, e)? == Some(v), target));
                    }
                    let here = b.vertex(v)?.point;
                    let anchor = if here.distance(t_this) <= here.distance(t_next) {
                        t_this
                    } else {
                        t_next
                    };
                    plan.vertices.push((v, anchor));
                }
            }
        }
        plans.retain(|_, plan| !plan.unusable);
        // drop the corners whose edges would vanish until none is left
        let lines = loop {
            let mut lines = ends_of.clone();
            for plan in plans.values() {
                for &(e, at_start, target) in &plan.ends {
                    if let Some(line) = lines.get_mut(&e) {
                        if at_start {
                            line.0 = target;
                        } else {
                            line.1 = target;
                        }
                    }
                }
            }
            let vanishing: BTreeSet<EdgeId> = lines
                .iter()
                .filter(|(e, (s, t))| {
                    ends_of.get(e).is_some_and(|&(s0, t0)| {
                        s.distance(*t) <= tol || (*t - *s).dot(t0 - s0) <= 0.0
                    })
                })
                .map(|(&e, _)| e)
                .collect();
            let before = plans.len();
            plans.retain(|_, plan| !plan.ends.iter().any(|(e, ..)| vanishing.contains(e)));
            if plans.len() == before {
                break lines;
            }
        };
        let mut moved: BTreeSet<EdgeId> = BTreeSet::new();
        for plan in plans.values() {
            for &(e, ..) in &plan.ends {
                if !moved.insert(e) {
                    continue;
                }
                let (Some(&(s, t)), Some(&(s0, t0))) = (lines.get(&e), ends_of.get(&e)) else {
                    continue;
                };
                if s.distance(s0) <= tol && t.distance(t0) <= tol {
                    continue;
                }
                let line = BSplineCurve::create_line_segment(3, s, t)?;
                let ivl = line.natural_interval();
                set_edge_curve(kernel, b, e, line, notifications)?;
                edge::set_interval(b, e, &ivl)?;
            }
            for &(v, point) in &plan.vertices {
                if b.vertex(v)?.point.distance(point) <= tol {
                    continue;
                }
                let key_of = notifications.result_key_of;
                crate::topology::vertex::set_point(
                    b,
                    v,
                    point,
                    &mut *notifications.records,
                    &mut *notifications.attributes,
                    None,
                    key_of(EntityRef::Vertex(v)),
                    &|vu| key_of(EntityRef::Vertexuse(vu)),
                )?;
            }
        }
        Ok(())
    }

    /// The original edge offset edge use `u` (or its mate) was made from, where it maps
    /// to one.
    fn original_edge(&self, original: &Brep, b: &Brep, u: EdgeuseId) -> Result<Option<EdgeId>> {
        let mut orig = self.edgeuse_to_edgeuses.firsts(u);
        if orig.is_empty() {
            let mate = eu::mate(b, u)?.ok_or_else(|| null("edge use without mate"))?;
            orig = self.edgeuse_to_edgeuses.firsts(mate);
        }
        match orig.as_slice() {
            [ou] => eu::edge(original, *ou),
            _ => Ok(None),
        }
    }
}

/// `pEdge->SetCurve(pCurve, TRUE); pCurve->SetOwner(pEdge)` on the offset B-rep.
fn set_edge_curve(
    kernel: &mut Kernel,
    b: &mut Brep,
    e: EdgeId,
    curve: BSplineCurve,
    notifications: &mut OffsetNotifications<'_>,
) -> Result<()> {
    let mut stores = NotificationStores {
        records: &mut *notifications.records,
        attributes: &mut *notifications.attributes,
        key_of: notifications.result_key_of,
    };
    let mut state = NotifyState::from_kernel_stores(&mut stores);
    let options = SetCurveOptions {
        delete_old_curve: true,
        dbg_warn_leaks: true,
        call_notify: true,
    };
    state.with_env(kernel, |env| {
        edge::set_curve(b, e, Some(Curve::from(curve)), options, env, [None, None])?;
        let owner = (env.key_of)(EntityRef::Edge(e));
        if let Some(c) = b.edge_mut(e)?.curve.as_mut() {
            c.set_owner(Some(owner), None);
        }
        b.set_geometry_owner_key(EntityRef::Edge(e), Some(owner));
        Ok(())
    })
}
