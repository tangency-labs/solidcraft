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

use std::collections::BTreeSet;

use super::p1::{forget_seconds, OffsetExecutive};
use super::p2::OffsetNotifications;
use crate::curve::curve::p1::GlobalPointSolveOptions;
use crate::curve::curve::{Curve, CurveKind};
use crate::error::{EntityRef, Result, SolidError};
use crate::geom::extent1d::Extent1d;
use crate::geom::global_solver::{SolutionRequested, SolverOperation};
use crate::kernel::Kernel;
use crate::math::{max2, Point3, EFF_ZERO};
use crate::nurbs::b_spline_curve::BSplineCurve;
use crate::nurbs::b_spline_curve_n_lib::TrimMode;
use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
use crate::query::edge::closest_point;
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
pub(crate) const SM_ERR_OFFSET_EDGE_VANISHES: i32 = 1043;

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
