//! The loop gaps an extended offset leaves and their closing edges (fork
//! tangency-labs/smlib 5d428ae, 7ff0a94, 34318f4, ccbf77a; the only path since #2981).
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
//! Not in the fork: `split_corner_vertices` (private, WR-657; the only path since
//! #2981), run by `close_loop_gaps` before it closes any gap.

use std::collections::{BTreeMap, BTreeSet};

use super::p1::{forget_seconds, OffsetExecutive, OffsetOperation};
use super::p2::OffsetNotifications;
use crate::curve::curve::p1::GlobalPointSolveOptions;
use crate::curve::curve::{Curve, CurveKind};
use crate::error::intersect::IntersectFailure;
use crate::error::offset::OffsetFailure;
use crate::error::{EntityRef, Failure, Result, Site, SolidError, TargetEdge, TargetFace};
use crate::geom::extent1d::Extent1d;
use crate::geom::extent2d::Extent2d;
use crate::geom::global_solver::{SolutionRequested, SolverOperation};
use crate::kernel::Kernel;
use crate::math::{max2, Point2, Point3, Vec2, Vec3, EFF_ZERO, EFF_ZERO_SQRT};
use crate::nurbs::b_spline_curve::BSplineCurve;
use crate::nurbs::b_spline_curve_n_lib::TrimMode;
use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
use crate::nurbs::math::gamma;
use crate::query::edge::closest_point;
use crate::surface::plane::Plane;
use crate::surface::surface::p2::SurfaceKind;
use crate::surface::surface::p4::{global_point_solve_starts, NoObject};
use crate::surface::surface::Surface;
use crate::topology::brep::p1 as bp1;
use crate::topology::brep::p3::{delete_face_with_body_identity, squeeze_edge, DeleteFaceOptions};
use crate::topology::brep::p9::make_vertex_split_edge_with_state;
use crate::topology::edge::{self, NotificationStores, NotifyState, SetCurveOptions};
use crate::topology::edgeuse::{p1 as eu, p3 as eu3};
use crate::topology::face::{p2 as face2, p3 as face3, p4 as face4};
use crate::topology::{vertexuse, Brep, EdgeId, EdgeuseId, FaceId, VertexId};

const OP: &str = "offset::offset_executive::step_gaps";

/// The gap search's passes per face (the fork's `lGuard`).
const MAX_GAP_PASSES: usize = 64;

/// The fraction of a gap's tolerance below which the bound on its straight bridge's distance
/// from the face stops tightening (the scale `curve_surface_gap_bound_within` asks for: a
/// hundredth of the tolerance the bound is held to).
const CHORD_BOUND_FLOOR: f64 = 1.0e-2;

fn null(reason: &'static str) -> SolidError {
    SolidError::invalid_input(OP, reason)
}

/// The gap search's failure `failure` (a refusal, or a defect: a reviewed one or one of the
/// search's named defects; status `SM_ERR`), where the search stood (`site`).
#[track_caller]
fn gap_failure(failure: OffsetFailure, site: Site) -> SolidError {
    Failure::new(OP, failure).or_at(site).into_error()
}

/// The larger tolerance of edge `e`'s end vertices (0 for one it lacks).
fn end_vertex_tolerance(b: &Brep, e: EdgeId) -> Result<f64> {
    let mut largest = 0.0_f64;
    for v in [edge::start_vertex(b, e)?, edge::end_vertex(b, e)?]
        .into_iter()
        .flatten()
    {
        largest = largest.max(b.vertex(v)?.tolerance);
    }
    Ok(largest)
}

/// The mate of an edge use.
fn required_mate(b: &Brep, u: EdgeuseId) -> Result<EdgeuseId> {
    eu::mate(b, u)?.ok_or_else(|| null("edge use without mate"))
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
    /// Not upstream: in a local operation, each kept edge ([`Self::keep_unmoved_edges`])
    /// is the original edge on both of its faces. Each offset face lifts its edges from
    /// its own UV trim curves, so the two offset edges of one kept edge differ from the
    /// original's 3D curve, and from each other, by up to the original edge's tolerance
    /// (the merge, which no longer intersects their surfaces, cannot join them). Both
    /// offset edges get the original edge's 3D curve, interval and tolerance, and their
    /// edge uses its edge uses' orientations and UV trim curves (the faces keep copies of
    /// their surfaces, so the UV curves hold), and their vertices the original vertices'
    /// points: the same edge, copied, with no new approximation. No tolerance is lowered:
    /// an edge or vertex takes the original's where that is larger than what earlier steps
    /// gave it, and a moved vertex then grows to cover every edge end at it (an edge
    /// intersected again beside a kept one ends where it was found, not at the original
    /// point). This runs after the faces' surfaces are extended, just before the merge.
    ///
    /// Before it, in every local operation (kept edges or not), the offset B-rep takes the
    /// input body's tolerance (owner rule: the result's tolerance is the input's), which
    /// bounds the original edges it now holds; the executive's own is half of it.
    ///
    /// # Errors
    ///
    /// [`SolidError::InvalidInput`] for missing topology; [`SolidError::PortInvariant`]
    /// for a kept edge without a 3D curve, or an edge use of one that has no offset, nor
    /// its mate, on a face whose offset is there; those of the tolerance, curve and vertex
    /// edits and the edge ends' evaluations.
    pub(super) fn restore_kept_edges(
        &mut self,
        kernel: &mut Kernel,
        original: &Brep,
        notifications: &mut OffsetNotifications<'_>,
    ) -> Result<()> {
        if self.operation() != OffsetOperation::LocalOperation {
            return Ok(());
        }
        let mut b = self
            .take_offset_brep()
            .ok_or_else(|| null("no offset B-rep"))?;
        let r = crate::topology::brep::p2::set_tolerance(
            kernel,
            &mut b,
            original.tolerance(),
            true,
            true,
        )
        .and_then(|()| self.restore_kept_edges_in(kernel, original, &mut b, notifications));
        self.set_offset_brep(b);
        r
    }

    fn restore_kept_edges_in(
        &self,
        kernel: &mut Kernel,
        original: &Brep,
        b: &mut Brep,
        notifications: &mut OffsetNotifications<'_>,
    ) -> Result<()> {
        // the offset vertices moved back to the original points
        let mut moved = BTreeSet::new();
        for &e in &self.kept_edges {
            // a kept edge is restored from its 3D curve; one without (a degenerate edge)
            // would keep its two lifted offsets apart
            let curve = edge::curve(original, e)?
                .ok_or_else(|| SolidError::port_invariant(OP, "a kept edge without a 3D curve"))?
                .clone();
            let interval = edge::interval(original, e)?;
            let tolerance = edge::tolerance(original, e)?;
            for u in edge::edgeuses(original, e, None, None)? {
                let Some(offset_u) = self.offset_edgeuse(u) else {
                    // of an edge use and its mate (the two sides of one face), one is related
                    // to its offset, which restores both; neither is, only when the face's
                    // offset closed to nothing and was deleted with its edges
                    // (`remove_closed_faces`): the edge has no offset on that face
                    let mate = required_mate(original, u)?;
                    if self.offset_edgeuse(mate).is_none()
                        && !self.offset_face_deleted(original, [u, mate])?
                    {
                        return Err(SolidError::port_invariant(
                            OP,
                            "a kept edge use, nor its mate, has an offset, and its face's \
                             offset is there",
                        ));
                    }
                    continue;
                };
                let offset_e =
                    eu::edge(b, offset_u)?.ok_or_else(|| null("edge use without edge"))?;
                let offset_mate = required_mate(b, offset_u)?;
                let orientation = eu::orientation(original, u)?;
                let opposite = match orientation {
                    OrientType::Same => OrientType::Opposite,
                    OrientType::Opposite => OrientType::Same,
                    OrientType::Unknown => OrientType::Unknown,
                };
                eu::set_orientation(b, offset_u, orientation)?;
                eu::set_orientation(b, offset_mate, opposite)?;
                set_edge_curve(kernel, b, offset_e, curve.clone(), notifications)?;
                edge::set_interval(b, offset_e, &interval)?;
                edge::set_tolerance(b, offset_e, tolerance, true, false)?;
                let uv = eu::uv_trim_curve(original, u)?.0.cloned();
                if let Some(uv) = uv {
                    eu::set_uv_trim_curve(
                        b,
                        offset_u,
                        eu::UvTrimCurveArg::New(Box::new(uv)),
                        0.0,
                        true,
                        None,
                    )?;
                }
                for (ou, au) in [(u, offset_u), (required_mate(original, u)?, offset_mate)] {
                    let v = edgeuse_vertex(original, ou)?;
                    let offset_v = edgeuse_vertex(b, au)?;
                    let point = original.vertex(v)?.point;
                    let key_of = notifications.result_key_of;
                    crate::topology::vertex::set_point(
                        b,
                        offset_v,
                        point,
                        &mut *notifications.records,
                        &mut *notifications.attributes,
                        None,
                        key_of(EntityRef::Vertex(offset_v)),
                        &|vu| key_of(EntityRef::Vertexuse(vu)),
                    )?;
                    let vertex_tolerance = original.vertex(v)?.tolerance;
                    crate::topology::vertex::set_tolerance(
                        b,
                        offset_v,
                        vertex_tolerance,
                        true,
                        false,
                    )?;
                    moved.insert(offset_v);
                }
            }
        }
        // each moved vertex covers the ends of its edges (the kept ones all restored now)
        for v in moved {
            let point = b.vertex(v)?.point;
            let mut reach = 0.0_f64;
            for e in crate::topology::vertex::edges(b, v, None)? {
                for u in edge::edgeuses(b, e, None, None)? {
                    if edgeuse_vertex(b, u)? == v {
                        reach = reach.max((point_at(kernel, b, u, 0.0)? - point).length());
                    }
                }
            }
            crate::topology::vertex::set_tolerance(b, v, reach, true, false)?;
        }
        Ok(())
    }

    /// Whether the offset face of the original face of edge uses `uses` (an edge use and
    /// its mate) was deleted: neither side's face use is related to an offset face any
    /// more ([`OffsetExecutive::remove_closed_faces`] forgets them).
    fn offset_face_deleted(&self, original: &Brep, uses: [EdgeuseId; 2]) -> Result<bool> {
        for u in uses {
            let fu = eu::faceuse(original, u)?.ok_or_else(|| null("edge use without face use"))?;
            if !self.faceuse_to_faces.seconds(fu).is_empty() {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// `CloseLoopGaps` (fork; run for every extended offset since #2981): where
    /// consecutive edge uses of an offset face no longer meet at their
    /// shared vertex (a local operation's step left an edge behind; a solid offset's two
    /// consecutive edges whose offsets run parallel), the edge whose end lies away from the
    /// vertex gets the straight segment to it (which must lie on the face's extended
    /// surface) and is split there, or is trimmed to it when the vertex moved into it.
    /// Each face is searched again after each gap closed.
    ///
    /// # Errors
    ///
    /// [`crate::error::offset::OffsetFailure::LoopGapEndsAmbiguous`] for a gap both or neither
    /// end of which is at the vertex,
    /// [`crate::error::offset::OffsetFailure::LoopGapPassLimit`] for a face still searched
    /// after [`MAX_GAP_PASSES`] passes, and
    /// [`crate::error::offset::OffsetFailure::SplitAtEdgeEnd`] where the bridged edge is not
    /// split at the joint (each a defect of the search, `SM_ERR`);
    /// [`crate::error::offset::OffsetFailure::LoopGapUncertified`] where the straight
    /// segment to the gap could not be established within tolerance;
    /// [`crate::error::offset::OffsetFailure::EdgeVanishes`] where the vertex moved to or past the edge's other
    /// end; those of the evaluations, solves, curve edits
    /// and the split; [`SolidError::InvalidInput`] for missing topology.
    ///
    /// In a local operation an edge closed to a point (fork ccbf77a, WR-566) is squeezed out
    /// instead, and once every gap is closed the faces that closed to nothing with such edges
    /// are deleted (see `remove_closed_faces`).
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
        let segments = r?;
        self.cap_vertex_gaps(kernel, &segments, notifications)
    }

    fn close_loop_gaps_in(
        &mut self,
        kernel: &mut Kernel,
        original: &Brep,
        b: &mut Brep,
        notifications: &mut OffsetNotifications<'_>,
    ) -> Result<Vec<GapSegment>> {
        let approx_tol = self.approximation_tol_3d();
        let mut segments: Vec<GapSegment> = Vec::new();
        // (WR-657) the corners whose offset planes no longer meet in one point
        self.split_corner_vertices(kernel, original, b, approx_tol, notifications)?;
        // (fork ccbf77a) each edge that closed to a point: its offset face and the original
        // face across it
        let mut closed_edges: Vec<(FaceId, Option<FaceId>)> = Vec::new();
        let closing = self.close_gaps_of_faces(
            kernel,
            original,
            b,
            approx_tol,
            notifications,
            &mut closed_edges,
            &mut segments,
        );
        if let Err(failure) = closing {
            // A cancelled pass is a cancellation, never a refusal.
            if failure.is_cancelled() {
                return Err(failure);
            }
            // (WR-566) a later gap failed after edges had closed to points: an edge closed
            // where no face (yet) closed to nothing is refused by name, ahead of the later
            // gap's failure. Mid-pass this can name a face (1043) that the remaining passes
            // would only have refused as their own failure (1001); either way it is refused.
            if !closed_edges.is_empty() {
                self.closed_faces(kernel, original, b, &closed_edges)?;
            }
            return Err(failure);
        }
        if !closed_edges.is_empty() {
            self.remove_closed_faces(kernel, original, b, &closed_edges, notifications)?;
        }
        let faces = bp1::faces(b, None)?;
        segments.retain(|s| faces.contains(&s.face));
        // a gap at a vertex of an open face is a step between that face's rim and the
        // offset faces, which the step walls close: no vertex polygon
        let mut kept: Vec<GapSegment> = Vec::new();
        for s in segments {
            let on_open = match s.vertex {
                Some(v) => on_a_face_of(original, v, self.shell_faces())?,
                None => false,
            };
            if !on_open {
                kept.push(s);
            }
        }
        Ok(kept)
    }

    /// The passes of [`OffsetExecutive::close_loop_gaps`] over the faces of `b`: each gap
    /// of each face's loops is closed (`close_loop_gap`), the edges closed to points noted
    /// in `closed_edges` and the straight segments added to the faces' loops in `segments`.
    ///
    /// # Errors
    ///
    /// Those of [`OffsetExecutive::close_loop_gaps`] before its faces are removed.
    #[allow(clippy::too_many_arguments)]
    fn close_gaps_of_faces(
        &mut self,
        kernel: &mut Kernel,
        original: &Brep,
        b: &mut Brep,
        approx_tol: f64,
        notifications: &mut OffsetNotifications<'_>,
        closed_edges: &mut Vec<(FaceId, Option<FaceId>)>,
        segments: &mut Vec<GapSegment>,
    ) -> Result<()> {
        for f in bp1::faces(b, None)? {
            // Each gap closed changes the loop: look again until none is left (each gap
            // is closed once).
            let mut passes = 0;
            let mut closed = true;
            while closed {
                closed = false;
                passes += 1;
                if passes > MAX_GAP_PASSES {
                    // one gap closed per pass: a face with this many gaps or more, or a gap
                    // not really closed found again; either is the search's limit, a defect
                    return Err(gap_failure(
                        OffsetFailure::LoopGapPassLimit {
                            face: self.original_face(original, f),
                            limit: MAX_GAP_PASSES,
                        },
                        self.gap_site(original, b, f, None, None),
                    ));
                }
                let fu =
                    face2::upward_faceuse(b, f)?.ok_or_else(|| null("face without face use"))?;
                let loopuses = b.faceuse(fu)?.loopuses.clone();
                'loops: for lu in loopuses {
                    let eus = eu::loopuse_edgeuses(b, lu)?;
                    for (kk, &this) in eus.iter().enumerate() {
                        // the next use round the loop (the first after the last)
                        let next = *eus
                            .get(kk + 1)
                            .or_else(|| eus.first())
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
                        // the original vertex the gap is the offset of: where the
                        // original edge use of `next` starts (the end of its mate's)
                        let vertex = match self.edgeuse_to_edgeuses.firsts(next).as_slice() {
                            [orig] => Some(edgeuse_vertex(original, *orig)?),
                            _ => match self
                                .edgeuse_to_edgeuses
                                .firsts(required_mate(b, next)?)
                                .as_slice()
                            {
                                [orig] => {
                                    Some(edgeuse_vertex(original, required_mate(original, *orig)?)?)
                                }
                                _ => None,
                            },
                        };
                        // (#21446 review) a face whose outline touches itself at the vertex
                        // is left to the steps before #21215 there
                        let pinched = match vertex {
                            Some(v0) => pinches_an_outline(original, v0)?,
                            None => false,
                        };
                        let segment = if end.distance(sv) <= tol && start.distance(sv) > tol {
                            self.close_loop_gap(
                                kernel,
                                original,
                                b,
                                f,
                                Gap {
                                    u: next,
                                    at_start: true,
                                    other: this,
                                    v,
                                    vertex_point: sv,
                                    tol,
                                    original_vertex: vertex,
                                    pinched,
                                },
                                notifications,
                                closed_edges,
                            )?
                        } else if start.distance(sv) <= tol && end.distance(sv) > tol {
                            self.close_loop_gap(
                                kernel,
                                original,
                                b,
                                f,
                                Gap {
                                    u: this,
                                    at_start: false,
                                    other: next,
                                    v,
                                    vertex_point: sv,
                                    tol,
                                    original_vertex: vertex,
                                    pinched,
                                },
                                notifications,
                                closed_edges,
                            )?
                        } else {
                            // both ends at the vertex, or neither: valid geometry near the
                            // tolerance (a gap between tol and 2 tol about the vertex) that the
                            // search does not handle, a defect
                            return Err(gap_failure(
                                OffsetFailure::LoopGapEndsAmbiguous {
                                    face: self.original_face(original, f),
                                    end_distance: end.distance(sv),
                                    start_distance: start.distance(sv),
                                    tolerance: tol,
                                },
                                self.gap_site(original, b, f, Some(next), Some(sv)),
                            ));
                        };
                        if let Some((from, to, bridge)) = segment {
                            // (#21215) the segment as the face's offset face use runs: its
                            // loop here is the upward one, which runs the other way where
                            // the input's face use is not on its surface's normal side
                            let segment = if !pinched && self.loops_against_input(original, f)? {
                                GapSegment {
                                    face: f,
                                    from: to,
                                    to: from,
                                    bridge: reversed(bridge)?,
                                    tol,
                                    vertex,
                                    pinched,
                                }
                            } else {
                                GapSegment {
                                    face: f,
                                    from,
                                    to,
                                    bridge,
                                    tol,
                                    vertex,
                                    pinched,
                                }
                            };
                            segments.push(segment);
                        }
                        closed = true;
                        break 'loops;
                    }
                }
            }
        }
        Ok(())
    }

    /// The check of [`OffsetExecutive::remove_closed_faces`]: the offset faces that closed
    /// to nothing with `closed_edges` (`IsDegenerate`), in the order they closed, and the
    /// same as a set; every closed edge must be one of theirs or lie across from one.
    ///
    /// # Errors
    ///
    /// [`crate::error::offset::OffsetFailure::EdgeVanishes`] for a closed edge of a face
    /// that did not close to nothing and across from none that did; those of
    /// `IsDegenerate` and the face queries.
    fn closed_faces(
        &self,
        kernel: &Kernel,
        original: &Brep,
        b: &mut Brep,
        closed_edges: &[(FaceId, Option<FaceId>)],
    ) -> Result<(Vec<FaceId>, BTreeSet<FaceId>)> {
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
            let explained = gone.contains(&face)
                || across.is_some_and(|a| gone_orig.contains(&a))
                || self.closed_on_both_sides(original, b, closed_edges, face, across)?;
            if !explained {
                return Err(self.edge_vanishes(original, face, None));
            }
        }
        Ok((gone_list, gone))
    }

    /// (#1126) Whether the edge of `face` closed to a point (the original face `across`
    /// beside it) is one edge closed on both its sides: `face`'s only closed edge, the
    /// face across having closed exactly one edge, the one across from `face`, and both
    /// faces keeping a loop of three edges or more. The point is then a vertex where the
    /// faces either side meet the two faces whose edge closed (an offset that just
    /// reaches where its neighbours converge). An edge of a face that closed more than
    /// once, or that leaves a face fewer than three edges, is no such vertex.
    ///
    /// # Errors
    ///
    /// Those of the face and loop queries.
    fn closed_on_both_sides(
        &self,
        original: &Brep,
        b: &Brep,
        closed_edges: &[(FaceId, Option<FaceId>)],
        face: FaceId,
        across: Option<FaceId>,
    ) -> Result<bool> {
        let Some(across) = across else {
            return Ok(false);
        };
        let original_of = |f: FaceId| -> Result<Option<FaceId>> {
            match self.faceuse_to_faces.firsts(f).as_slice() {
                [fu] => eu::faceuse_face(original, *fu),
                _ => Ok(None),
            }
        };
        let Some(this_orig) = original_of(face)? else {
            return Ok(false);
        };
        // the closed edges of the offset face across, and that face
        let mut across_closed = Vec::new();
        for &(other, other_across) in closed_edges {
            if other != face && original_of(other)? == Some(across) {
                across_closed.push((other, other_across));
            }
        }
        let once = closed_edges.iter().filter(|&&(f, _)| f == face).count() == 1;
        let [(other, Some(back))] = across_closed[..] else {
            return Ok(false);
        };
        if !once || back != this_orig {
            return Ok(false);
        }
        for f in [face, other] {
            if face4::edges(b, f, None, false, None)?.len() < 3 {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// The end of the fork's `CloseLoopGaps` (ccbf77a, WR-566): the offset faces that
    /// closed to nothing as their edges closed to points (`IsDegenerate`: a step's wall
    /// whose low top moved onto the high top's plane, its edges along the tops now
    /// coinciding) are deleted, in the order they closed, with their pairs in the
    /// executive's relations; the merge joins the faces either side along their
    /// coinciding edges. Every closed edge must be one of theirs or lie across from one of
    /// them (the sides' edges along the wall).
    ///
    /// # Errors
    ///
    /// [`crate::error::offset::OffsetFailure::EdgeVanishes`] for a closed edge of a face that did not close to
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
        let (gone_list, gone) = self.closed_faces(kernel, original, b, closed_edges)?;
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
    /// (see [`OffsetExecutive::close_loop_gaps`]). Returns the segment added to the face's
    /// loop, its ends as the loop runs and, where it is no straight segment, its curve
    /// (see [`Self::plane_section_bridge`]); `None` where an edge was trimmed instead.
    ///
    /// Not in the fork (#21215): where the gap's end lies on the other edge at the vertex,
    /// the two edges meet at the gap's end, and that edge is trimmed to it and the vertex
    /// moved there, instead of the gap's edge running on to the vertex and back along the
    /// other edge (a loop that doubles back on itself); not in a local operation, whose moved
    /// faces' edges can be left behind.
    #[allow(clippy::too_many_lines)] // one fork routine
    fn close_loop_gap(
        &mut self,
        kernel: &mut Kernel,
        original: &Brep,
        b: &mut Brep,
        f: FaceId,
        gap: Gap,
        notifications: &mut OffsetNotifications<'_>,
        closed_edges: &mut Vec<(FaceId, Option<FaceId>)>,
    ) -> Result<Option<(Point3, Point3, Option<BSplineCurve>)>> {
        let Gap {
            u,
            at_start,
            other,
            v,
            vertex_point,
            tol,
            original_vertex,
            pinched,
        } = gap;
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
                    // (fork ccbf77a, WR-566) the vertex moved to the edge's other end, where
                    // the edge's other vertex is: the edge closed to a point. That is sound
                    // only where a face closes to nothing with it, which `close_loop_gaps`
                    // checks once every gap is closed. The edge is squeezed out, its two
                    // vertices one, and noted with the original face across it. Only a
                    // local operation moves a wall's top onto another face's plane: in a
                    // shell or offset an edge closed to a point is where the result meets
                    // itself along a line (no sound solid), refused below by name.
                    if self.operation() == OffsetOperation::LocalOperation
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
                        return Ok(None);
                    }
                    // (fork 34318f4) elsewhere the vertex moved past the edge's end: the edge
                    // vanishes, the face turns inside out
                    let input_edge = self.original_edge(original, b, u).ok().flatten();
                    return Err(self.edge_vanishes(original, f, input_edge));
                }
                curve.trim(kernel, &mut keep, TrimMode::Notify)?;
                let new_ivl = curve.natural_interval();
                set_edge_curve(kernel, b, e, curve, notifications)?;
                edge::set_interval(b, e, &new_ivl)?;
                return Ok(None);
            }
        }

        // (#1126) The edge's two vertices met at the gap and its curve misses them: the
        // edge closed to a point where the faces either side meet (a draft that just
        // reaches where its neighbours converge). It is squeezed out like a closed edge
        // above; `closed_faces` takes it only where its edge on the face across closed
        // with it.
        if self.operation() == OffsetOperation::LocalOperation
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
            return Ok(None);
        }

        // (#21215) The gap's end on the other edge at the vertex: the two edges cross
        // there, so the other edge is trimmed back to the gap's end and the vertex moved
        // to it. Not in a local operation: there a moved face's edge can be left behind
        // at its old place (`a_far_top_on_a_unit_square_plane_is_refused_as_not_sound`),
        // its end on the other edge where the two do not meet.
        if self.operation() != OffsetOperation::LocalOperation
            && !pinched
            && self.trim_other_to_gap_end(
                kernel,
                original,
                b,
                f,
                other,
                (v, vertex_point),
                gap_end,
                tol,
                notifications,
            )?
        {
            return Ok(None);
        }

        // The straight segment must lie on the face's surface (as extended for the offset:
        // the face's own surface ends at its old boundary). Its far end is no farther past
        // the face than its length.
        let key = (notifications.result_key_of)(EntityRef::Face(f)).key.0;
        let margin = self
            .offset_extension_margin(f)
            .max(vertex_point.distance(gap_end));
        let ext = self
            .get_create_extended_surface(kernel, key, &surface, margin)?
            .clone();
        let ext_domain = ext.natural_uv_domain_with_nurb(kernel)?;
        let chord = BSplineCurve::create_line_segment(3, vertex_point, gap_end)?;
        let straight = chord_on_surface(kernel, &ext, &ext_domain, &chord, tol)?;

        // The segment, from the vertex to the gap end, joined at the gap end: straight
        // where that lies on the face's surface, else the face's section by the plane of
        // the offset face that holds both ends (#21215).
        let (mut segment, bridge) = if straight {
            (chord, None)
        } else if pinched {
            // a face's outline touches itself at the vertex: no section is sought there
            return Err(self.loop_gap_uncertified(original, f));
        } else {
            match self.plane_section_bridge(
                kernel,
                original,
                b,
                f,
                &ext,
                original_vertex,
                [vertex_point, gap_end],
                tol,
                notifications,
            )? {
                Some(c) => (c.clone(), Some(c)),
                None => {
                    // The bridge could not be established within tolerance. Refuse here,
                    // before the result is judged.
                    return Err(self.loop_gap_uncertified(original, f));
                }
            }
        };
        curve.join_with(kernel, !at_curve_start, &mut segment, true, Some(tol))?;

        // Where they join: the parameter of the gap end on the joined curve.
        let joined = curve.natural_interval();
        let joint = closest_on(kernel, &curve, &joined, gap_end, tol)?
            .ok_or_else(|| {
                gap_failure(
                    OffsetFailure::Defect {
                        why: "the nearest point of the gap end on the edge's curve, just joined \
                              through that end, was not found, though a point always has a \
                              nearest point on a bounded curve",
                    },
                    self.gap_site(original, b, f, Some(u), Some(gap_end)),
                )
            })?
            .0;
        // a bridge that is no straight segment meets the edge at a kink: its knot
        let joint = if bridge.is_some() {
            kink_at(&curve, joint, gap_end, tol)?
        } else {
            joint
        };

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
            // the joint lies within an end vertex's tolerance along the edge, which only
            // the gap's length (more than `tol`) was checked against: two checks of the
            // kernel's own edge disagree, a defect
            return Err(gap_failure(
                OffsetFailure::SplitAtEdgeEnd {
                    face: self.original_face(original, f),
                    vertex_tolerance: end_vertex_tolerance(b, e)?,
                },
                self.gap_site(original, b, f, Some(u), Some(gap_end)),
            ));
        }
        output.vertex.ok_or_else(|| null("split without vertex"))?;
        // the segment as the face's loop runs: from the vertex to the gap where the gap is
        // at the edge use's start, else from the gap to the vertex
        Ok(Some(if at_start {
            (vertex_point, gap_end, bridge)
        } else {
            (gap_end, vertex_point, reversed(bridge)?)
        }))
    }

    /// Whether offset face `f`'s upward loops run against the loops of the input's face use
    /// it is the offset of (#21215): the input's face use is not the upward one of its face,
    /// so the result's face use of `f` is the one below its surface, whose loops run the
    /// other way. The segments of faces whose surfaces face opposite ways then still run
    /// one way round a gap between them. `false` where `f` is the offset of no single input
    /// face use.
    ///
    /// # Errors
    ///
    /// Those of the topology queries.
    fn loops_against_input(&self, original: &Brep, f: FaceId) -> Result<bool> {
        let [fu] = self.faceuse_to_faces.firsts(f)[..] else {
            return Ok(false);
        };
        let Some(face) = eu::faceuse_face(original, fu)? else {
            return Ok(false);
        };
        Ok(face2::upward_faceuse(original, face)? != Some(fu))
    }

    /// `close_loop_gap`'s trim of the `other` edge use at the gap's vertex `v` (#21215): where
    /// `gap_end`, the end of the gap's edge away from the vertex, lies on `other`'s curve
    /// within `tol`, the two edges meet there, not at the vertex. `other`'s curve is trimmed
    /// so that its end at the vertex (the end nearer the vertex's point, where `other`'s
    /// use ends or starts within the gap's tolerance) is at `gap_end`, and the vertex moved
    /// to `gap_end`. Without the trim the gap's edge would run
    /// on to the vertex along `other` and the loop come back over it.
    ///
    /// Returns whether it trimmed; `false` where `gap_end` is not on `other`'s curve.
    ///
    /// # Errors
    ///
    /// [`crate::error::offset::OffsetFailure::EdgeVanishes`] where `gap_end` is at or past
    /// `other`'s far end; those of the curve edits and the vertex move.
    #[allow(clippy::too_many_arguments)]
    fn trim_other_to_gap_end(
        &mut self,
        kernel: &mut Kernel,
        original: &Brep,
        b: &mut Brep,
        f: FaceId,
        other: EdgeuseId,
        (v, vertex_point): (VertexId, Point3),
        gap_end: Point3,
        tol: f64,
        notifications: &mut OffsetNotifications<'_>,
    ) -> Result<bool> {
        let e = eu::edge(b, other)?.ok_or_else(|| null("edge use without edge"))?;
        let Some(trimmed) = edge::create_trimmed_nurbs_curve(kernel, b, e)? else {
            return Ok(false);
        };
        let CurveKind::BSpline(curve) = trimmed.kind() else {
            return Ok(false);
        };
        let mut curve = curve.clone();
        let ivl = curve.natural_interval();
        let Some((at, value)) = closest_on(kernel, &curve, &ivl, gap_end, tol)? else {
            return Ok(false);
        };
        if value > tol {
            return Ok(false);
        }
        // the curve's end at the vertex
        let at_curve_start = curve.evaluate_point(ivl.min())?.distance(vertex_point)
            <= curve.evaluate_point(ivl.max())?.distance(vertex_point);
        let mut keep = if at_curve_start {
            Extent1d::from_min_max(at, ivl.max())
        } else {
            Extent1d::from_min_max(ivl.min(), at)
        };
        if keep.length() <= EFF_ZERO * (1.0 + ivl.max_dimension()) {
            // the gap's end at the other edge's far end: that edge vanishes
            let input_edge = self.original_edge(original, b, other).ok().flatten();
            return Err(self.edge_vanishes(original, f, input_edge));
        }
        curve.trim(kernel, &mut keep, TrimMode::Notify)?;
        let new_ivl = curve.natural_interval();
        set_edge_curve(kernel, b, e, curve, notifications)?;
        edge::set_interval(b, e, &new_ivl)?;
        let key_of = notifications.result_key_of;
        crate::topology::vertex::set_point(
            b,
            v,
            gap_end,
            &mut *notifications.records,
            &mut *notifications.attributes,
            None,
            key_of(EntityRef::Vertex(v)),
            &|vu| key_of(EntityRef::Vertexuse(vu)),
        )?;
        Ok(true)
    }

    /// `close_loop_gap`'s bridge where the straight segment between `ends` (the vertex,
    /// then the gap's end) is not on the face's surface `ext` (as extended), #21215: the
    /// two ends are corners of the offset of `original_vertex` where the offsets of faces
    /// tangent along an edge cross a third face's offset at different points (a tube's
    /// rounded corner joined to a loft's at a crease: the tube's round and side and the
    /// loft's round and side meet the tube's side's offset at two points), and the gap runs
    /// along the third face. Where that face's surface is a plane holding both ends within
    /// `tol`, the bridge is the section of `ext` by that plane between the ends: an offset
    /// face of one of the faces at `original_vertex`, the plane extended as the face's own
    /// offset is. Its curve runs from the first end to the second, its ends within `tol` of
    /// them, and its control polygon lies within `tol` of the plane, which bounds the curve
    /// there.
    ///
    /// Returns `None` where no such plane holds both ends, or no single held open section
    /// through both ends is found ([`held_section`]); a plane whose section is empty
    /// (`IntersectFailure::NoIntersection`) is passed over for the next.
    ///
    /// # Errors
    ///
    /// Those of the topology queries, the extension, the curve solves and the trim; a
    /// cancelled intersection; [`OffsetFailure::LoopGapUncertified`] caused by the
    /// intersection's error where the intersection of a plane holding both ends fails other
    /// than by finding no section (whether that section is the bridge is not known).
    #[allow(clippy::too_many_arguments)]
    fn plane_section_bridge(
        &mut self,
        kernel: &mut Kernel,
        original: &Brep,
        b: &Brep,
        f: FaceId,
        ext: &Surface,
        original_vertex: Option<VertexId>,
        ends: [Point3; 2],
        tol: f64,
        notifications: &mut OffsetNotifications<'_>,
    ) -> Result<Option<BSplineCurve>> {
        let Some(v0) = original_vertex else {
            return Ok(None);
        };
        let at_vertex = crate::topology::vertex::faces(original, v0)?;
        let [from, to] = ends;
        for g in bp1::faces(b, None)? {
            if g == f {
                continue;
            }
            let mut of_vertex = false;
            for fu in self.faceuse_to_faces.firsts(g) {
                if eu::faceuse_face(original, fu)?.is_some_and(|o| at_vertex.contains(&o)) {
                    of_vertex = true;
                }
            }
            if !of_vertex {
                continue;
            }
            let Some(surface) = b.face(g)?.surface.as_ref() else {
                continue;
            };
            let SurfaceKind::Plane(plane) = surface.kind() else {
                continue;
            };
            let (origin, normal, _) = plane.is_planar();
            let Ok(normal) = normal.unitize() else {
                continue;
            };
            let off = |p: Point3| (p - origin).dot(normal).abs();
            if off(from) > tol || off(to) > tol {
                continue;
            }
            let surface = surface.clone();
            let key = (notifications.result_key_of)(EntityRef::Face(g)).key.0;
            let margin = self.offset_extension_margin(g).max(from.distance(to));
            let plane_ext = self
                .get_create_extended_surface(kernel, key, &surface, margin)?
                .clone();
            let (curves, status) = crate::intersect::api_intersectors::intersect_surfaces_within(
                kernel,
                ext,
                &plane_ext,
                Some(0.25 * tol),
            );
            if let Err(e) = status {
                if e.is_cancelled() {
                    return Err(e);
                }
                if is_no_section(&e) {
                    // no section of this plane: another plane may still hold both ends
                    continue;
                }
                // the intersection did not complete: whether this plane's section is the
                // bridge is not known, so the bridge is not established, for that cause
                return Err(Failure::new(
                    OP,
                    OffsetFailure::LoopGapUncertified {
                        face: self.original_face(original, f),
                    },
                )
                .caused_by(e)
                .into_error());
            }
            match held_section(kernel, &curves, ends, &off, tol)? {
                Held::One(piece) => return Ok(Some(piece)),
                Held::Two => return Ok(None),
                Held::None => {}
            }
        }
        Ok(None)
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

    /// `SplitEdgesAtStepCorners` (fork 7ff0a94, the WR-530 addendum's step walls, local
    /// operations only): every edge of
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
                    // kept `tol` from the ends by distance, the point is within an end
                    // vertex's tolerance along the edge: two checks of the kernel's own edge
                    // disagree, a defect
                    return Err(gap_failure(
                        OffsetFailure::SplitAtEdgeEnd {
                            face: None,
                            vertex_tolerance: end_vertex_tolerance(b, e)?,
                        },
                        Site::point(p),
                    ));
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

/// Whether the straight `chord` (a gap's bridge from the vertex to the gap's end) lies on
/// `ext` (the face's extended surface, its parameters `ext_domain`) within `tol` over its
/// whole length. Three points of it, at its quarters, are dropped onto the surface first:
/// one farther than `tol` rules the chord out. They cannot rule it in, since a surface can
/// meet the chord at its ends and at every probe and leave it between them; the chord is on
/// the surface only where a proved bound on its distance from it over the whole chord,
/// every rounding counted, is within `tol` (`curve_surface_gap_bound_within`; on a plane the
/// chord's control points bound it at once, so a chord on a plane stays straight). `false`
/// where the bound is not proved.
///
/// # Errors
///
/// Those of the probes' solves; the bound's unported and cancelled statuses.
fn chord_on_surface(
    kernel: &Kernel,
    ext: &Surface,
    ext_domain: &Extent2d,
    chord: &BSplineCurve,
    tol: f64,
) -> Result<bool> {
    let ivl = chord.natural_interval();
    for ii in 1..4 {
        let pnt = chord.evaluate_point(ivl.evaluate(f64::from(ii) / 4.0))?;
        let sols = global_point_solve_starts(
            kernel,
            ext,
            ext_domain,
            SolverOperation::Minimize,
            pnt,
            tol,
            SolutionRequested::Single,
        )?;
        if !sols.first().is_some_and(|s| s.solution_value <= tol) {
            return Ok(false);
        }
    }
    let bound = crate::query::curve_surface_gap::curve_surface_gap_bound_within(
        kernel,
        ext,
        ext_domain,
        chord,
        [ivl.min(), ivl.max()],
        CHORD_BOUND_FLOOR * tol,
        [tol, tol],
    )?;
    Ok(bound.is_some_and(|g| g <= tol))
}

/// Whether intersection error `e` is the typed empty result
/// (`IntersectFailure::NoIntersection`: the surfaces were intersected and have no section),
/// not a failure to intersect them.
fn is_no_section(e: &SolidError) -> bool {
    e.intersect_failure() == Some(&IntersectFailure::NoIntersection)
}

/// `plane_section_bridge`'s pick among the sections `curves` of a face's surface by a plane
/// (#21215): the one piece between `ends` that the plane holds. A candidate is an open
/// B-spline section with both ends within `tol` of it at distinct parameters; its piece
/// between them, run from the first end to the second, is held where its control polygon
/// lies within `tol` of the plane (`off`, the distance from it), which bounds the piece
/// there. A candidate the plane does not hold is passed over wherever it comes; two held
/// pieces leave the bridge ambiguous ([`Held::Two`]).
///
/// # Errors
///
/// Those of the evaluations, the closest point solves, the trim and the reversal.
fn held_section(
    kernel: &Kernel,
    curves: &[Curve],
    [from, to]: [Point3; 2],
    off: &dyn Fn(Point3) -> f64,
    tol: f64,
) -> Result<Held> {
    let mut found: Option<BSplineCurve> = None;
    for c in curves {
        let CurveKind::BSpline(section) = c.kind() else {
            continue;
        };
        let ivl = section.natural_interval();
        if section
            .evaluate_point(ivl.min())?
            .distance(section.evaluate_point(ivl.max())?)
            <= tol
        {
            // a closed section has two ways between the ends
            continue;
        }
        let (Some((t0, d0)), Some((t1, d1))) = (
            closest_on(kernel, section, &ivl, from, tol)?,
            closest_on(kernel, section, &ivl, to, tol)?,
        ) else {
            continue;
        };
        if d0 > tol || d1 > tol || (t1 - t0).abs() <= EFF_ZERO * (1.0 + ivl.max_dimension()) {
            continue;
        }
        let mut piece = section.clone();
        let mut keep = Extent1d::from_min_max(t0.min(t1), t0.max(t1));
        piece.trim(kernel, &mut keep, TrimMode::Notify)?;
        if t0 > t1 {
            let whole = piece.natural_interval();
            piece.reverse_parameterization(&whole)?;
        }
        let (polygon, _) = piece.control_polygon()?;
        if polygon.iter().any(|&p| off(p) > tol) {
            continue;
        }
        if found.is_some() {
            // two held pieces through both ends: none is the bridge
            return Ok(Held::Two);
        }
        found = Some(piece);
    }
    Ok(found.map_or(Held::None, Held::One))
}

/// What [`held_section`] found among a plane's sections.
#[derive(Debug)]
enum Held {
    /// No piece between the ends that the plane holds.
    None,
    /// The one held piece, from the first end to the second.
    One(BSplineCurve),
    /// Two held pieces or more: the bridge is ambiguous.
    Two,
}

/// The parameter to split the joined `curve` at near `param`, the nearest point of `joint`
/// on it: the knot there where the curve is only continuous (a joint of two curves meeting
/// at an angle) and whose point is within `tol` of `joint`, else `param`. A split beside the
/// knot would leave the kink inside one piece.
///
/// # Errors
///
/// Those of the knot query and the evaluation.
fn kink_at(curve: &BSplineCurve, param: f64, joint: Point3, tol: f64) -> Result<f64> {
    let degree = curve.degree()?;
    let (knots, multiplicities) = curve.knots(None)?;
    let mut best: Option<f64> = None;
    for (&k, &m) in knots.iter().zip(&multiplicities) {
        if m < degree || curve.evaluate_point(k)?.distance(joint) > tol {
            continue;
        }
        if best.is_none_or(|b| (k - param).abs() < (b - param).abs()) {
            best = Some(k);
        }
    }
    Ok(best.unwrap_or(param))
}

/// `curve` run the other way, keeping its interval.
fn reversed(curve: Option<BSplineCurve>) -> Result<Option<BSplineCurve>> {
    let Some(mut c) = curve else {
        return Ok(None);
    };
    let ivl = c.natural_interval();
    c.reverse_parameterization(&ivl)?;
    Ok(Some(c))
}

/// One gap of an offset face's loop at vertex `v` (at `vertex_point`), as
/// `close_loop_gaps` found it: the edge use `u` whose end lies away from the vertex (its
/// start where `at_start`), the `other` edge use at the vertex, the gap's tolerance and the
/// original vertex the gap is the offset of, where known.
#[derive(Clone, Copy)]
struct Gap {
    u: EdgeuseId,
    at_start: bool,
    other: EdgeuseId,
    v: VertexId,
    vertex_point: Point3,
    tol: f64,
    original_vertex: Option<VertexId>,
    /// Whether `original_vertex` pinches an input face's outline ([`pinches_an_outline`]):
    /// the #21215 steps are then left out.
    pinched: bool,
}

/// A segment `close_loop_gap` added to an offset face's loop: the face, the segment's ends
/// in the direction the face's loop runs, its curve in that direction where it is no
/// straight segment (a plane's section of the face's surface, #21215), the gap's tolerance,
/// and the original vertex the gap is at (where the gap's edge use has one original edge
/// use).
#[derive(Clone)]
struct GapSegment {
    face: FaceId,
    from: Point3,
    to: Point3,
    bridge: Option<BSplineCurve>,
    tol: f64,
    vertex: Option<VertexId>,
    /// Whether `vertex` pinches an input face's outline ([`pinches_an_outline`]).
    pinched: bool,
}

/// A polygon of [`OffsetExecutive::cap_vertex_gaps`]: its corners in order, each side's
/// curve where it is a curved segment (`None`: straight), its tolerance, its original
/// vertex and the edge it is closed along.
struct CapPolygon {
    corners: Vec<Point3>,
    sides: Vec<Option<BSplineCurve>>,
    tol: f64,
    vertex: Option<VertexId>,
    /// The straight edge the polygon is closed along, from its last corner to its first,
    /// where its segments do not close it.
    along: Option<EdgeId>,
    /// Whether a segment of the polygon is at a vertex that pinches an input face's outline:
    /// the polygon is then not taken into a face (`widen_face_over_polygon`).
    pinched: bool,
}

impl GapSegment {
    /// Whether the segment runs between `a` and `b`, either way, within its tolerance.
    fn joins(&self, a: Point3, b: Point3) -> bool {
        (self.from.distance(a) <= self.tol && self.to.distance(b) <= self.tol)
            || (self.from.distance(b) <= self.tol && self.to.distance(a) <= self.tol)
    }
}

impl OffsetExecutive {
    /// Not in the fork (solidcraft#17469): the offset of an original vertex whose faces'
    /// offsets do not meet in one point. Where they stand apart, `close_loop_gaps` closes
    /// each face's loop at the vertex with its own straight segment, and where the two
    /// offsets of an edge at the vertex end at different points (faces tangent along the
    /// edge, one of whose offsets meets its next face lower than the other's), the longer
    /// one runs on past the shorter. A plane and a cylinder tangent along an edge that
    /// ends where a cone stands on a ledge: the plane's offset meets the ledge's at the
    /// ledge's height, the cylinder's meets the cone's above it, and the cone's and the
    /// ledge's offsets are joined by segments down to the ledge's offset and along it to
    /// the plane's. The segments, and the stretch where the edge's longer offset runs on,
    /// bound a polygon no face fills, and the body built is open there. The vertex's
    /// offset is that polygon: the plane face it bounds, run against the faces' loops, which
    /// the merge joins to each face along its side.
    ///
    /// A side is a segment no other face borders (a segment of a step wall or one two faces
    /// share is no side) at a vertex on no open face (`shell_faces`: there the gap is a step
    /// from the open face's rim, which the step walls close); the segments are chained end to end, each end meeting exactly
    /// one other within the larger of the two segments' tolerances. A chain that does not
    /// close is closed by the straight stretch from its last end to its first only where an
    /// offset edge runs along all of it: a straight edge with both its ends within the
    /// chain's tolerance, the largest of its segments' (`along_an_edge`). A polygon of three or more corners on one plane within the
    /// chain's tolerance, twice its area more than that tolerance times its perimeter, is
    /// filled; any other chain is left as it is (the result is then judged as before). The
    /// face is kept out of the face extensions: it closes the gap exactly.
    ///
    /// # Errors
    ///
    /// Those of the topology queries, the closest point solves and the face's construction.
    fn cap_vertex_gaps(
        &mut self,
        kernel: &mut Kernel,
        segments: &[GapSegment],
        notifications: &mut OffsetNotifications<'_>,
    ) -> Result<()> {
        if segments.len() < 2 {
            return Ok(());
        }
        let mut polygons: Vec<CapPolygon> = Vec::new();
        {
            let b = self.offset_brep().ok_or_else(|| null("no offset B-rep"))?;
            let mut open: Vec<GapSegment> = Vec::new();
            for s in segments {
                if !bordered_elsewhere(b, s)? {
                    open.push(s.clone());
                }
            }
            let mut used: BTreeSet<usize> = BTreeSet::new();
            for (first, s) in open.iter().enumerate() {
                if !used.insert(first) {
                    continue;
                }
                // the polygon runs each segment backwards, from its `to` to its `from`:
                // `after` continues from `s`'s end, `before` leads into its start
                let mut after: Vec<GapSegment> = Vec::new();
                let mut before: Vec<GapSegment> = Vec::new();
                let (mut start, mut end) = (s.to, s.from);
                let mut closed = false;
                // two segments meet within the larger of their own tolerances
                let (mut first_tol, mut last_tol) = (s.tol, s.tol);
                while let Some(j) = only(&open, &used, |c| {
                    c.to.distance(end) <= max2(last_tol, c.tol)
                }) {
                    let Some(c) = open.get(j) else { break };
                    used.insert(j);
                    after.push(c.clone());
                    end = c.from;
                    last_tol = c.tol;
                    if end.distance(start) <= max2(first_tol, last_tol) {
                        closed = true;
                        break;
                    }
                }
                while !closed {
                    let Some(j) = only(&open, &used, |c| {
                        c.from.distance(start) <= max2(first_tol, c.tol)
                    }) else {
                        break;
                    };
                    let Some(c) = open.get(j) else { break };
                    used.insert(j);
                    before.push(c.clone());
                    start = c.to;
                    first_tol = c.tol;
                    closed = start.distance(end) <= max2(first_tol, last_tol);
                }
                let chain: Vec<GapSegment> = before
                    .into_iter()
                    .rev()
                    .chain(std::iter::once(s.clone()))
                    .chain(after)
                    .collect();
                // the polygon's own tolerance: the largest of its segments'
                let tol = chain.iter().fold(0.0, |t, c| max2(t, c.tol));
                let mut corners: Vec<Point3> = chain.iter().map(|c| c.to).collect();
                // side i runs from corner i to the next: segment i backwards
                let mut sides: Vec<Option<BSplineCurve>> = Vec::new();
                for c in &chain {
                    sides.push(reversed(c.bridge.clone())?);
                }
                let mut along = None;
                if !closed {
                    if chain.len() < 2 {
                        continue;
                    }
                    let Some(e) = along_an_edge(kernel, b, end, start, tol)? else {
                        continue;
                    };
                    corners.push(end);
                    sides.push(None);
                    along = Some(e);
                }
                let vertex = s
                    .vertex
                    .filter(|_| chain.iter().all(|c| c.vertex == s.vertex));
                let pinched = chain.iter().any(|c| c.pinched);
                if corners.len() >= 3 {
                    polygons.push(CapPolygon {
                        corners,
                        sides,
                        tol,
                        vertex,
                        along,
                        pinched,
                    });
                }
            }
        }
        for polygon in polygons {
            if let Some(e) = polygon.along.filter(|_| !polygon.pinched) {
                let mut b = self
                    .take_offset_brep()
                    .ok_or_else(|| null("no offset B-rep"))?;
                let r = Self::widen_face_over_polygon(kernel, &mut b, &polygon, e, notifications);
                self.set_offset_brep(b);
                if r? {
                    continue;
                }
            }
            self.cap_gap_polygon(kernel, &polygon, notifications)?;
        }
        Ok(())
    }

    /// Not in the fork (#21215): a polygon of [`Self::cap_vertex_gaps`] closed along the
    /// straight edge `along` of an offset face whose plane holds the whole polygon, its
    /// corners and its curved sides' control polygons within its tolerance, with one end of
    /// the closing stretch at an end vertex `w` of `along` and the other inside it. The
    /// polygon is then part of that face: the stretch of its boundary along `along` gives
    /// way to the polygon's other sides. The face's other edge at `w` runs on through them
    /// (joined to them and split at each corner), `w` moves to the stretch's other end and
    /// `along` is trimmed to it. A plane face of its own there would meet the face across a
    /// stretch where the polygon's curved side runs tangent into `along` (the section of a
    /// round's offset by the plane, where the round is tangent to a plane that crosses it
    /// along `along`), which the merge cannot join.
    ///
    /// Returns whether the face took the polygon; `false` leaves the polygon to its own face.
    ///
    /// # Errors
    ///
    /// Those of the topology queries, the curve joins and edits, the vertex move and the
    /// splits.
    #[allow(clippy::too_many_lines)] // one edit of one loop
    fn widen_face_over_polygon(
        kernel: &mut Kernel,
        b: &mut Brep,
        polygon: &CapPolygon,
        along: EdgeId,
        notifications: &mut OffsetNotifications<'_>,
    ) -> Result<bool> {
        let CapPolygon {
            corners,
            sides,
            tol,
            ..
        } = polygon;
        let tol = *tol;
        let (Some(&start), Some(&end)) = (corners.first(), corners.last()) else {
            return Ok(false);
        };
        if corners.len() < 3 || sides.len() != corners.len() {
            return Ok(false);
        }
        let [g] = crate::query::edge::edge_faces(b, along)?[..] else {
            return Ok(false);
        };
        let Some(surface) = b.face(g)?.surface.as_ref() else {
            return Ok(false);
        };
        let SurfaceKind::Plane(plane) = surface.kind() else {
            return Ok(false);
        };
        let (origin, normal, _) = plane.is_planar();
        let Ok(normal) = normal.unitize() else {
            return Ok(false);
        };
        let off = |p: Point3| (p - origin).dot(normal).abs();
        if corners.iter().any(|&p| off(p) > tol) {
            return Ok(false);
        }
        for curve in sides.iter().flatten() {
            let (points, _) = curve.control_polygon()?;
            if points.iter().any(|&p| off(p) > tol) {
                return Ok(false);
            }
        }
        // the stretch's end at an end vertex of `along`, and its other end inside it
        let ends = crate::query::edge::edge_vertices(b, along)?;
        let [v0, v1] = ends[..] else {
            return Ok(false);
        };
        let (p0, p1) = (b.vertex(v0)?.point, b.vertex(v1)?.point);
        let at_end = |p: Point3| {
            if p.distance(p0) <= tol {
                Some(v0)
            } else if p.distance(p1) <= tol {
                Some(v1)
            } else {
                None
            }
        };
        // the path from `w` to the stretch's other end over the polygon's other sides
        let n = corners.len();
        let side = |i: usize| -> Result<BSplineCurve> {
            match (
                sides.get(i),
                corners.get(i),
                corners.get(i.saturating_add(1)),
            ) {
                (Some(Some(c)), _, _) => Ok(c.clone()),
                (_, Some(&p), Some(&q)) => BSplineCurve::create_line_segment(3, p, q),
                _ => Err(null("polygon side without corners")),
            }
        };
        // the sides but the closing stretch, the last
        let sides_before_stretch = n.saturating_sub(1);
        let (w, inside, path, joints) = match (at_end(start), at_end(end)) {
            (Some(w), None) => {
                let mut path = Vec::new();
                for i in 0..sides_before_stretch {
                    path.push(side(i)?);
                }
                let joints: Vec<Point3> =
                    corners.iter().take(sides_before_stretch).copied().collect();
                (w, end, path, joints)
            }
            (None, Some(w)) => {
                let mut path = Vec::new();
                for i in (0..sides_before_stretch).rev() {
                    if let Some(c) = reversed(Some(side(i)?))? {
                        path.push(c);
                    }
                }
                let joints: Vec<Point3> = corners.iter().skip(1).rev().copied().collect();
                (w, start, path, joints)
            }
            _ => return Ok(false),
        };
        if inside.distance(p0) <= tol || inside.distance(p1) <= tol {
            return Ok(false);
        }
        let w_point = b.vertex(w)?.point;
        // the face's other edge at `w`
        let mut others = Vec::new();
        for e in face4::edges(b, g, None, false, None)? {
            if e != along && crate::query::edge::edge_vertices(b, e)?.contains(&w) {
                others.push(e);
            }
        }
        let [t] = others[..] else {
            return Ok(false);
        };
        let curve_of = |kernel: &Kernel, b: &mut Brep, e: EdgeId| -> Result<Option<BSplineCurve>> {
            let Some(trimmed) = edge::create_trimmed_nurbs_curve(kernel, b, e)? else {
                return Ok(None);
            };
            Ok(match trimmed.kind() {
                CurveKind::BSpline(c) => Some(c.clone()),
                _ => None,
            })
        };
        let (Some(mut joined), Some(mut trimmed_along)) =
            (curve_of(kernel, b, t)?, curve_of(kernel, b, along)?)
        else {
            return Ok(false);
        };
        // `along` keeps its part from `inside` to its far end
        let along_ivl = trimmed_along.natural_interval();
        let Some((at, value)) = closest_on(kernel, &trimmed_along, &along_ivl, inside, tol)? else {
            return Ok(false);
        };
        if value > tol {
            return Ok(false);
        }
        let along_starts_at_w = trimmed_along
            .evaluate_point(along_ivl.min())?
            .distance(w_point)
            <= trimmed_along
                .evaluate_point(along_ivl.max())?
                .distance(w_point);
        let mut keep = if along_starts_at_w {
            Extent1d::from_min_max(at, along_ivl.max())
        } else {
            Extent1d::from_min_max(along_ivl.min(), at)
        };
        if keep.length() <= EFF_ZERO * (1.0 + along_ivl.max_dimension()) {
            return Ok(false);
        }
        // the other edge runs on from `w` over the path
        let t_ivl = joined.natural_interval();
        let t_ends_at_w = joined.evaluate_point(t_ivl.max())?.distance(w_point)
            <= joined.evaluate_point(t_ivl.min())?.distance(w_point);
        for piece in path {
            if t_ends_at_w {
                let mut piece = piece;
                joined.join_with(kernel, true, &mut piece, false, Some(tol))?;
            } else if let Some(mut piece) = reversed(Some(piece))? {
                joined.join_with(kernel, false, &mut piece, true, Some(tol))?;
            }
        }
        trimmed_along.trim(kernel, &mut keep, TrimMode::Notify)?;
        let new_along = trimmed_along.natural_interval();
        set_edge_curve(kernel, b, along, trimmed_along, notifications)?;
        edge::set_interval(b, along, &new_along)?;
        let new_t = joined.natural_interval();
        set_edge_curve(kernel, b, t, joined, notifications)?;
        edge::set_interval(b, t, &new_t)?;
        let key_of = notifications.result_key_of;
        crate::topology::vertex::set_point(
            b,
            w,
            inside,
            &mut *notifications.records,
            &mut *notifications.attributes,
            None,
            key_of(EntityRef::Vertex(w)),
            &|vu| key_of(EntityRef::Vertexuse(vu)),
        )?;
        // a vertex at each corner the edge now runs through
        for joint in joints {
            let mut split = false;
            for e in face4::edges(b, g, None, false, None)? {
                if e == along {
                    continue;
                }
                let Some(c) = curve_of(kernel, b, e)? else {
                    continue;
                };
                let ivl = c.natural_interval();
                let Some((param, value)) = closest_on(kernel, &c, &ivl, joint, tol)? else {
                    continue;
                };
                let param = kink_at(&c, param, joint, tol)?;
                if value > tol
                    || c.evaluate_point(ivl.min())?.distance(joint) <= tol
                    || c.evaluate_point(ivl.max())?.distance(joint) <= tol
                {
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
                    param,
                    &mut state,
                    notifications.result_curve_ref,
                )?;
                split = output.split;
                break;
            }
            if !split {
                // the corner lies within an end vertex's tolerance along the edge, which the
                // checks above put farther than the tolerance from both its ends: two checks
                // of the kernel's own edge disagree, a defect
                return Err(gap_failure(
                    OffsetFailure::SplitAtEdgeEnd {
                        face: None,
                        vertex_tolerance: end_vertex_tolerance(b, t)?,
                    },
                    Site::default(),
                ));
            }
        }
        Ok(true)
    }

    /// One polygon of [`Self::cap_vertex_gaps`], its corners in order: its plane face,
    /// made when the corners (and a curved side's control polygon) lie on one plane within
    /// its tolerance and its area is clear of zero, and related to its original vertex where
    /// one is known. A side is straight, or the curve of the segment it is.
    fn cap_gap_polygon(
        &mut self,
        kernel: &mut Kernel,
        polygon: &CapPolygon,
        notifications: &mut OffsetNotifications<'_>,
    ) -> Result<()> {
        let CapPolygon {
            corners,
            sides: curved,
            tol,
            vertex,
            ..
        } = polygon;
        let (tol, vertex) = (*tol, *vertex);
        let Some(&origin) = corners.first() else {
            return Ok(());
        };
        let sides = || corners.iter().zip(corners.iter().cycle().skip(1));
        let has_curved = curved.iter().any(Option::is_some);
        // Newell's normal: twice the polygon's area along its normal. A curved side gives its
        // own part (`curved_area_vector`), and the area test takes the parts' rounding off:
        // the corners alone can be collinear while a curved side bounds an area (an arc and
        // its chord). A cap of straight sides keeps the corners' sum.
        let mut normal = Vec3::ZERO;
        let mut perimeter = 0.0;
        let mut rounding = 0.0;
        let mut straight_size = 0.0;
        for (k, (&p, &q)) in sides().enumerate() {
            if let Some(Some(curve)) = curved.get(k) {
                let Some(part) = curved_area_vector(origin, curve)? else {
                    // the side's part is not bounded: its area is not known
                    return Ok(());
                };
                normal += part.twice_area;
                perimeter += part.length;
                rounding += part.rounding;
            } else {
                normal += (p - origin).cross(q - origin);
                perimeter += p.distance(q);
                straight_size += (p - origin).length() * (q - origin).length();
            }
        }
        if has_curved {
            // the straight parts' rounding (a cross product of two differences each) and the
            // sum's, counted on the parts' sizes
            rounding = (rounding
                + gamma(corners.len().saturating_add(8))
                    * (straight_size + normal.length() + rounding))
                * (1.0 + gamma(2));
        }
        if normal.length() - rounding <= tol * perimeter {
            return Ok(());
        }
        let n = normal.unitize()?;
        if corners.iter().any(|&p| (p - origin).dot(n).abs() > tol) {
            return Ok(());
        }
        let mut boundary: Vec<Point3> = corners.clone();
        for curve in curved.iter().flatten() {
            let (points, _) = curve.control_polygon()?;
            if points.iter().any(|&p| (p - origin).dot(n).abs() > tol) {
                return Ok(());
            }
            boundary.extend(points);
        }
        let Some(&second) = corners.get(1) else {
            return Ok(());
        };
        // (with a curved side the corners need not span the plane: the first side's direction
        // is taken square to the boundary's normal)
        let x_axis = if has_curved {
            let along = second - origin;
            (along - n * along.dot(n)).unitize()?
        } else {
            (second - origin).unitize()?
        };
        let y_axis = n.cross(x_axis);
        let (mut u_min, mut u_max, mut v_min, mut v_max) = (0.0_f64, 0.0_f64, 0.0_f64, 0.0_f64);
        for &p in &boundary {
            let (u, v) = ((p - origin).dot(x_axis), (p - origin).dot(y_axis));
            u_min = u_min.min(u);
            u_max = u_max.max(u);
            v_min = v_min.min(v);
            v_max = v_max.max(v);
        }
        // the plane reaches past the polygon by its own size, as a face's replaced plane does
        let margin = max2(u_max - u_min, v_max - v_min);
        let domain = Extent2d::from_points(
            Point2::new(u_min - margin, v_min - margin),
            Point2::new(u_max + margin, v_max + margin),
        );
        let mut plane = Plane::new(origin, x_axis, y_axis, Vec2::new(1.0, 1.0), &domain);
        plane.make_nurb()?;
        let mut curves = Vec::new();
        for (k, (&p, &q)) in sides().enumerate() {
            curves.push(Curve::from(match curved.get(k) {
                Some(Some(curve)) => curve.clone(),
                _ => BSplineCurve::create_line_segment(3, p, q)?,
            }));
        }
        let orientations = vec![OrientType::Same; curves.len()];
        let mut stores = NotificationStores {
            records: &mut *notifications.records,
            attributes: &mut *notifications.attributes,
            key_of: notifications.result_key_of,
        };
        let face = self.make_cap_face(
            kernel,
            Surface::from(plane),
            curves,
            orientations,
            &[],
            &mut NotifyState::from_kernel_stores(&mut stores),
        )?;
        self.push_closing_face(face);
        if let Some(vertex) = vertex {
            self.relate_vertex_face(vertex, face);
        }
        Ok(())
    }
}

/// A curved side's part of a cap's Newell normal ([`curved_area_vector`]).
struct CurvedArea {
    /// `∫ (C(t) - O) × C'(t) dt` over the side: its part of twice the cap's area vector.
    twice_area: Vec3,
    /// A bound on the rounding of `twice_area`, every coordinate error counted.
    rounding: f64,
    /// The length of the side's control polygon, no less than the side's own length.
    length: f64,
}

/// The part of curved side `curve` (a non-rational 3D B-spline) of a cap's Newell normal
/// about `origin`: `∫ (C(t) - O) × C'(t) dt` over its whole interval. A closed boundary's
/// parts sum to twice its area vector (Green's theorem; a straight side `p q` gives
/// `(p - O) × (q - O)`, Newell's term). The curve, moved by `-O`, is cut into its Bézier pieces
/// (`decompose_bez`); a piece of degree `n` with control points `Q_i` gives
///
/// `∫ Q × Q' dt = Σ_i Σ_k ½ C(n, i) C(n - 1, k) / C(2n - 1, i + k) · Q_i × (Q_(k+1) - Q_k)`,
///
/// from `Q' = n Σ_k (Q_(k+1) - Q_k) b_k^(n-1)` and the integral of a product of Bernstein
/// polynomials, `∫ b_i^n b_k^(n-1) dt = C(n, i) C(n - 1, k) / (2n C(2n - 1, i + k))`. A
/// degree-1 piece gives Newell's term.
///
/// Rounding (`docs/definitions-and-certificates.md` §4.4): the move rounds each coordinate
/// once (`γ_1` of the largest moved coordinate `M`) and the cut's `I` knot insertions carry
/// `γ_(12 (I + 1))` of `2 M` (as `nurbs::crv_deviation` counts them), so with `e` their sum
/// each control point is within `2 e` of its exact place and each difference within `4 e`.
/// A term `c Q × D` is then off by at most `c (2 e (|D| + 4 e) + 4 e |Q|)`, and the products,
/// the coefficients and the sum round within `γ_(12 + terms)` of the terms' sizes
/// `c (|Q| + 2 e) (|D| + 4 e)`.
///
/// `None` for a rational or 2D curve, a degree past the binomial table, and numbers not
/// finite: the side's part is then not bounded.
///
/// # Errors
///
/// Those of the moved form's construction and the cut.
fn curved_area_vector(origin: Point3, curve: &BSplineCurve) -> Result<Option<CurvedArea>> {
    use crate::nurbs::bezier::binomial;
    use crate::nurbs::crv_basic::Curve as Nurb;
    use crate::nurbs::math::CPoint;

    let Ok(nurb) = curve.nurb_ref() else {
        return Ok(None);
    };
    if nurb.is_rational() || !nurb.is_3d() {
        return Ok(None);
    }
    let n = nurb.degree();
    if n == 0 {
        return Ok(None);
    }
    let moved = Nurb::new(
        nurb.cpts()
            .iter()
            .map(|c| CPoint::from_point(Vec3::new(c.x - origin.x, c.y - origin.y, c.z - origin.z)))
            .collect(),
        n,
        nurb.knots().to_vec(),
    )?;
    let largest = moved.cpts().iter().fold(0.0_f64, |m, c| {
        m.max(c.x.abs()).max(c.y.abs()).max(c.z.abs())
    });
    let pieces = moved.decompose_bez()?;
    // knots inserted: those of the pieces' joined knot vector, 2 (n + 1) + (s - 1) n, past
    // the curve's
    let joined = pieces
        .len()
        .saturating_sub(1)
        .saturating_mul(n)
        .saturating_add(2 * (n + 1));
    let inserted = joined.saturating_sub(moved.knots().len());
    let e =
        gamma(1) * largest + gamma(inserted.saturating_add(1).saturating_mul(12)) * 2.0 * largest;
    let point = |c: &CPoint| Vec3::new(c.x, c.y, c.z);
    let mut twice_area = Vec3::ZERO;
    let (mut error, mut size, mut terms) = (0.0_f64, 0.0_f64, 0_usize);
    let mut length = 0.0;
    for piece in &pieces {
        let q = piece.cpts();
        if q.len() != n.saturating_add(1) {
            return Ok(None);
        }
        for (a, b) in q.iter().zip(q.iter().skip(1)) {
            length += point(b).distance(point(a));
        }
        for (i, qi) in q.iter().enumerate() {
            for (k, (qk, qk1)) in q.iter().zip(q.iter().skip(1)).enumerate() {
                let (Ok(cn), Ok(cm), Ok(cd)) = (
                    binomial(n, i),
                    binomial(n.saturating_sub(1), k),
                    binomial((2 * n).saturating_sub(1), i.saturating_add(k)),
                ) else {
                    return Ok(None);
                };
                let c = 0.5 * cn * cm / cd;
                let (qv, d) = (point(qi), point(qk1) - point(qk));
                twice_area += qv.cross(d) * c;
                let (ql, dl) = (qv.length(), d.length());
                error += c * (2.0 * e * (dl + 4.0 * e) + 4.0 * e * ql);
                size += c * (ql + 2.0 * e) * (dl + 4.0 * e);
                terms += 1;
            }
        }
    }
    let rounding = (error + gamma(terms.saturating_add(12)) * size) * (1.0 + gamma(2));
    let finite = twice_area.length().is_finite() && rounding.is_finite() && length.is_finite();
    Ok(finite.then_some(CurvedArea {
        twice_area,
        rounding,
        length,
    }))
}

/// Whether a face other than `s`'s has an edge between `s`'s ends (a step wall's, or the
/// same segment of a neighbour): such a segment is no side of a vertex's polygon. Shared ends
/// are the filter; a curved segment (a bridge) is bordered only by an edge whose curve is also
/// proved within the segment's tolerance of the bridge over both whole curves, either way round
/// ([`curved_side_coincides`]): two curves with the same ends can bow apart.
fn bordered_elsewhere(b: &Brep, s: &GapSegment) -> Result<bool> {
    for &e in b.edge_list() {
        let ends = crate::query::edge::edge_vertices(b, e)?;
        let [v0, v1] = ends.as_slice() else {
            continue;
        };
        if s.joins(b.vertex(*v0)?.point, b.vertex(*v1)?.point)
            && crate::query::edge::edge_faces(b, e)?
                .iter()
                .any(|&f| f != s.face)
        {
            let coincides = match &s.bridge {
                None => true,
                Some(bridge) => curved_side_coincides(b, e, bridge, s.tol)?,
            };
            if coincides {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

/// Whether the curve of edge `e` over its interval and `bridge` over its own are proved
/// within `tol` of each other, every point of each near a point of the other
/// ([`crate::nurbs::crv_deviation::certified_hausdorff`] of their B-spline forms, a bound over
/// both whole curves with every rounding counted, as
/// [`crate::boolean::api_brep::coincidence_bound`] takes it for two B-splines), with `bridge`
/// run either way ([`exactly_reversed`]). `false` where no bound is proved (an edge without a
/// B-spline curve, a rational or 2D form, intervals that share no more than a point, a
/// reversal that would round): the segment then stays a side.
///
/// # Errors
///
/// Those of the edge lookup and of the reversed form's construction.
fn curved_side_coincides(b: &Brep, e: EdgeId, bridge: &BSplineCurve, tol: f64) -> Result<bool> {
    use crate::nurbs::crv_deviation::certified_hausdorff;

    let rec = b.edge(e)?;
    let Some(CurveKind::BSpline(edge_curve)) = rec.curve.as_ref().map(Curve::kind) else {
        return Ok(false);
    };
    let (Ok(edge_nurb), Ok(ahead)) = (edge_curve.nurb_ref(), bridge.nurb_ref()) else {
        return Ok(false);
    };
    let (Some(&lo), Some(&hi)) = (ahead.knots().first(), ahead.knots().last()) else {
        return Ok(false);
    };
    let along = [rec.interval.min(), rec.interval.max()];
    if certified_hausdorff(ahead, [lo, hi], edge_nurb, along).is_ok_and(|d| d <= tol) {
        return Ok(true);
    }
    let Some(back) = exactly_reversed(ahead)? else {
        return Ok(false);
    };
    Ok(certified_hausdorff(&back, [lo, hi], edge_nurb, along).is_ok_and(|d| d <= tol))
}

/// `c` run the other way on its own domain `[lo, hi]`: its control points in reverse order on
/// the knots `lo + hi - u` taken in reverse, the curve `C(lo + hi - t)`, where every one of
/// those knots is exact in floating point (the sum `lo + hi` and each difference, checked by
/// the error-free transformation `TwoSum`: Knuth, TAOCP vol. 2, §4.2.2); `None` where one would
/// round, so the reversed form is the same point set and no rounding goes uncounted.
///
/// # Errors
///
/// Those of the reversed form's construction.
fn exactly_reversed(
    c: &crate::nurbs::crv_basic::Curve,
) -> Result<Option<crate::nurbs::crv_basic::Curve>> {
    // `a + b` with its rounding error (TwoSum): exact where the error is zero
    let exact_sum = |x: f64, y: f64| -> Option<f64> {
        let s = x + y;
        let yy = s - x;
        let err = (x - (s - yy)) + (y - yy);
        (s.is_finite() && err == 0.0).then_some(s)
    };
    let (Some(&lo), Some(&hi)) = (c.knots().first(), c.knots().last()) else {
        return Ok(None);
    };
    let Some(sum) = exact_sum(lo, hi) else {
        return Ok(None);
    };
    let mut knots = Vec::new();
    for &u in c.knots().iter().rev() {
        let Some(r) = exact_sum(sum, -u) else {
            return Ok(None);
        };
        knots.push(r);
    }
    Ok(Some(crate::nurbs::crv_basic::Curve::new(
        c.cpts().iter().rev().copied().collect(),
        c.degree(),
        knots,
    )?))
}

/// Whether vertex `v` of the input `original` pinches the outline of a face at it: a loop of
/// the face passes through `v` twice, and not as a seam's end (the seam's edge in the loop
/// twice, once each way). There the face's outline touches itself (a pad's corner on its
/// face's rim), the walls offset from the two meet at that point, and the steps added for
/// #21215 (the trim to the gap's end, the plane's section as bridge, the loops run by the
/// input's face use and the widened plane face) are left out, so such a body is refused as
/// before rather than built into a body that is not a manifold solid.
///
/// # Errors
///
/// Those of the topology queries.
fn pinches_an_outline(original: &Brep, v: VertexId) -> Result<bool> {
    for f in crate::topology::vertex::faces(original, v)? {
        let Some(fu) = original.face(f)?.faceuse else {
            continue;
        };
        for lu in eu::faceuse_loopuses(original, fu, false, None)? {
            let mut starts: Vec<(Option<EdgeId>, VertexId)> = Vec::new();
            for u in eu::loopuse_edgeuses(original, lu)? {
                if let Some(vu) = eu::vertexuse(original, u)? {
                    if let Some(w) = vertexuse::vertex(original, vu)? {
                        starts.push((eu::edge(original, u)?, w));
                    }
                }
            }
            if starts.iter().filter(|(_, w)| *w == v).count() < 2 {
                continue;
            }
            let seam_end = starts.iter().any(|&(e, w)| {
                w == v && e.is_some() && starts.iter().filter(|(g, _)| *g == e).count() > 1
            });
            if !seam_end {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

/// Whether vertex `v` of `b` lies on one of `faces`: one of its edges borders one of them.
fn on_a_face_of(b: &Brep, v: VertexId, faces: &[FaceId]) -> Result<bool> {
    if faces.is_empty() {
        return Ok(false);
    }
    for &e in b.edge_list() {
        if crate::query::edge::edge_vertices(b, e)?.contains(&v)
            && crate::query::edge::edge_faces(b, e)?
                .iter()
                .any(|f| faces.contains(f))
        {
            return Ok(true);
        }
    }
    Ok(false)
}

/// The index of the one segment of `open` not `used` that `meets`, if exactly one does.
fn only(
    open: &[GapSegment],
    used: &BTreeSet<usize>,
    meets: impl Fn(&GapSegment) -> bool,
) -> Option<usize> {
    let mut found = open
        .iter()
        .enumerate()
        .filter(|(j, c)| !used.contains(j) && meets(c))
        .map(|(j, _)| j);
    let first = found.next()?;
    found.next().is_none().then_some(first)
}

/// The edge of `b` that runs along the straight stretch from `p` to `q`, if any: a straight edge
/// (its control points within `tol` of one line, so the whole curve is) with both ends of
/// the stretch within `tol` of it. The distance to a straight segment is convex, so the
/// whole stretch is then within `tol` of the edge.
fn along_an_edge(k: &Kernel, b: &Brep, p: Point3, q: Point3, tol: f64) -> Result<Option<EdgeId>> {
    for &e in b.edge_list() {
        let Some(curve) = b.edge(e)?.curve.as_ref() else {
            continue;
        };
        if !crate::topology::brep::trim_extend::is_straight(curve, tol)? {
            continue;
        }
        let near = |x: Point3| -> Result<bool> {
            Ok(closest_point(k, b, e, x)?.is_some_and(|cp| cp.distance <= tol))
        };
        if near(p)? && near(q)? {
            return Ok(Some(e));
        }
    }
    Ok(None)
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
    /// Not in the fork (the only path since #2981; WR-657, before the gaps are
    /// closed): a convex corner of the
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

    /// The caller's topology identified by the executive's provenance. A lookup that
    /// fails or has no unique source leaves that slot empty: diagnostic context never
    /// changes the operation's outcome or names an intermediate body's ids.
    fn gap_site(
        &self,
        original: &Brep,
        offset: &Brep,
        face: FaceId,
        edgeuse: Option<EdgeuseId>,
        at: Option<Point3>,
    ) -> Site {
        Site {
            edge: edgeuse.and_then(|u| TargetEdge::of_offset_edgeuse(self, original, offset, u)),
            face: TargetFace::of_offset_face(self, original, face),
            at,
            ..Site::default()
        }
    }

    /// The original face offset face `face` was made from, where it maps to one.
    /// This is diagnostic context only: an unreadable source names no caller face.
    pub(crate) fn original_face(&self, original: &Brep, face: FaceId) -> Option<FaceId> {
        match self.faceuse_to_faces.firsts(face).as_slice() {
            [faceuse] => eu::faceuse_face(original, *faceuse).ok().flatten(),
            _ => None,
        }
    }

    /// The refusal of an offset that moves a vertex of an edge of offset face `f` to or
    /// past the edge's other end ([`crate::error::offset::OffsetFailure::EdgeVanishes`]), naming `edge` of
    /// `original`, else the face of `original` that `f` comes from (error context only:
    /// a lookup that fails names nothing).
    fn edge_vanishes(&self, original: &Brep, f: FaceId, edge: Option<EdgeId>) -> SolidError {
        let face = if edge.is_none() {
            self.original_face(original, f)
        } else {
            None
        };
        Failure::new(OP, OffsetFailure::EdgeVanishes { edge, face }).into_error()
    }

    /// The refusal of a gap in the loops of offset face `f` that the straight bridge does
    /// not close ([`crate::error::offset::OffsetFailure::LoopGapUncertified`]), naming the
    /// face of `original` that `f` comes from (error context only: a lookup that fails or
    /// gives no single face names nothing).
    fn loop_gap_uncertified(&self, original: &Brep, f: FaceId) -> SolidError {
        let face = self.original_face(original, f);
        Failure::new(OP, OffsetFailure::LoopGapUncertified { face }).into_error()
    }

    /// The original edge offset edge use `u` (or its mate) was made from, where it maps
    /// to one.
    pub(crate) fn original_edge(
        &self,
        original: &Brep,
        b: &Brep,
        u: EdgeuseId,
    ) -> Result<Option<EdgeId>> {
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
    curve: impl Into<Curve>,
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
        edge::set_curve(b, e, Some(curve.into()), options, env, [None, None])?;
        let owner = (env.key_of)(EntityRef::Edge(e));
        if let Some(c) = b.edge_mut(e)?.curve.as_mut() {
            c.set_owner(Some(owner), None);
        }
        b.set_geometry_owner_key(EntityRef::Edge(e), Some(owner));
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::topology::{Edgeuse, EdgeuseOwner, Faceuse};

    fn mapped_gap() -> (
        OffsetExecutive,
        Brep,
        Brep,
        FaceId,
        EdgeuseId,
        FaceId,
        EdgeId,
    ) {
        let mut original = Brep::new();
        let shell = original.add_shell().unwrap();
        original.add_face(shell).unwrap();
        let input_face = original.add_face(shell).unwrap();
        let input_faceuse = original
            .add_faceuse_record(Faceuse {
                face: Some(input_face),
                ..Faceuse::default()
            })
            .unwrap();
        let vertex = original.add_vertex(Vec3::ZERO).unwrap();
        original.add_edge(vertex, vertex).unwrap();
        let input_edge = original.add_edge(vertex, vertex).unwrap();
        original.add_edgeuse_record(Edgeuse::default()).unwrap();
        let input_edgeuse = original
            .add_edgeuse_record(Edgeuse {
                edge: Some(input_edge),
                ..Edgeuse::default()
            })
            .unwrap();

        let mut offset = Brep::new();
        let shell = offset.add_shell().unwrap();
        let face = offset.add_face(shell).unwrap();
        let vertex = offset.add_vertex(Vec3::ZERO).unwrap();
        let edge = offset.add_edge(vertex, vertex).unwrap();
        let first = offset
            .add_edgeuse_record(Edgeuse {
                edge: Some(edge),
                owner: EdgeuseOwner::Shell(Some(shell)),
                ..Edgeuse::default()
            })
            .unwrap();
        let mate = offset
            .add_edgeuse_record(Edgeuse {
                edge: Some(edge),
                owner: EdgeuseOwner::Shell(Some(shell)),
                ..Edgeuse::default()
            })
            .unwrap();
        offset.edge_mut(edge).unwrap().edgeuses = vec![first, mate];
        // Input and intermediate ids deliberately differ, so copying the intermediate
        // entity into Site fails the regression.
        assert_ne!(face, input_face);
        assert_ne!(edge, input_edge);
        assert_ne!(first, input_edgeuse);
        let mut executive =
            OffsetExecutive::new(OffsetOperation::SolidOffset, 1.0, original.tolerance());
        executive.relate_faceuse(input_faceuse, face);
        executive.relate_edgeuses(input_edgeuse, first);
        (
            executive, original, offset, face, first, input_face, input_edge,
        )
    }

    /// A gap the search cannot close is a defect of the search (the gaps are in the offset
    /// loops the kernel built), naming the input's face and its numbers, with the status
    /// (`SM_ERR`) of the stage failure it replaced and where the search stood; a reviewed
    /// defect of the search keeps the same status and site.
    #[test]
    fn a_gap_the_search_cannot_close_is_a_defect_with_its_numbers_and_status() {
        let (executive, original, offset, face, edgeuse, input_face, input_edge) = mapped_gap();
        let point = Point3::new(1.0, 2.0, 3.0);
        let site = executive.gap_site(&original, &offset, face, Some(edgeuse), Some(point));
        let ambiguous = OffsetFailure::LoopGapEndsAmbiguous {
            face: executive.original_face(&original, face),
            end_distance: 2e-5,
            start_distance: 3e-5,
            tolerance: 1e-4,
        };
        let e = gap_failure(ambiguous, site);
        assert_eq!(e.offset_failure(), Some(&ambiguous));
        assert_eq!(e.kind(), crate::error::ErrorKind::StageFailed);
        assert!(e.is_defect());
        assert!(!e.is_refusal());
        assert_eq!(e.entity(), Some(EntityRef::Face(input_face)));
        assert_eq!(
            e.to_string(),
            format!(
                "{OP}: stage_failed: a gap in an offset face's loop has no one end at the vertex \
                 its edges share: its ends are 2e-5 and 3e-5 from it, against 1e-4 \
                 (face {}) at {}",
                input_face.index(),
                e.site().unwrap()
            )
        );
        let defect = gap_failure(OffsetFailure::Defect { why: "x" }, site);
        assert!(defect.is_defect());
        // the search's other named failures are defects too (#21059)
        for other in [
            OffsetFailure::LoopGapPassLimit {
                face: executive.original_face(&original, face),
                limit: MAX_GAP_PASSES,
            },
            OffsetFailure::SplitAtEdgeEnd {
                face: executive.original_face(&original, face),
                vertex_tolerance: 1e-5,
            },
        ] {
            let other = gap_failure(other, site);
            assert_eq!(other.kind(), crate::error::ErrorKind::StageFailed);
            assert!(other.is_defect() && !other.is_refusal(), "{other}");
            assert_eq!(other.upstream_status(), Some(crate::error::SM_ERR));
        }
        for failure in [&e, &defect] {
            assert_eq!(failure.operation(), OP);
            assert_eq!(failure.upstream_status(), Some(crate::error::SM_ERR));
            assert!(failure.is_status_answer());
            let site = failure.site().unwrap();
            assert_eq!(site.edge.map(TargetEdge::id), Some(input_edge));
            assert_eq!(site.face.map(TargetFace::id), Some(input_face));
            assert_eq!(site.at, Some(point));
        }
    }

    #[test]
    fn a_gap_failure_maps_an_edgeuse_mate_to_the_input_edge() {
        let (mut executive, original, offset, face, edgeuse, _, input_edge) = mapped_gap();
        let source = executive.edgeuse_to_edgeuses.firsts(edgeuse)[0];
        executive
            .edgeuse_to_edgeuses
            .remove_pair(source, edgeuse)
            .unwrap();
        let mate = required_mate(&offset, edgeuse).unwrap();
        executive.relate_edgeuses(source, mate);
        let site = executive.gap_site(&original, &offset, face, Some(edgeuse), None);
        assert_eq!(site.edge.map(TargetEdge::id), Some(input_edge));
    }

    #[test]
    fn a_gap_failure_never_guesses_among_input_entities() {
        let (mut executive, mut original, offset, face, edgeuse, _, _) = mapped_gap();
        let shell = original.add_shell().unwrap();
        let other_face = original.add_face(shell).unwrap();
        let other_faceuse = original
            .add_faceuse_record(Faceuse {
                face: Some(other_face),
                ..Faceuse::default()
            })
            .unwrap();
        executive.relate_faceuse(other_faceuse, face);
        let vertex = original.add_vertex(Vec3::ZERO).unwrap();
        let other_edge = original.add_edge(vertex, vertex).unwrap();
        let other_edgeuse = original
            .add_edgeuse_record(Edgeuse {
                edge: Some(other_edge),
                ..Edgeuse::default()
            })
            .unwrap();
        executive.relate_edgeuses(other_edgeuse, edgeuse);
        let point = Point3::new(1.0, 2.0, 3.0);
        let site = executive.gap_site(&original, &offset, face, Some(edgeuse), Some(point));
        assert!(site.edge.is_none());
        assert!(site.face.is_none());
        assert_eq!(site.at, Some(point));
    }

    use crate::nurbs::crv_basic::Curve as NurbCurve;
    use crate::nurbs::math::{CPoint, NOW};

    /// A non-rational 3D B-spline of degree `p` through control points `points` on `knots`.
    fn spline(points: &[[f64; 3]], p: usize, knots: &[f64]) -> BSplineCurve {
        let pw = points
            .iter()
            .map(|&[x, y, z]| CPoint::new(x, y, z, NOW))
            .collect();
        BSplineCurve::from_nurb(NurbCurve::new(pw, p, knots.to_vec()).unwrap(), 3, false)
    }

    /// The surface `z = q(x)`, `q(x) = x (x - 1/4) (x - 1/2) (x - 3/4) (x - 1)`, extruded
    /// along y over `[0, 1] x [-1, 1]`: one Bézier patch of degree 5 by 1, its control points'
    /// heights `q`'s Bernstein coefficients `b_i = Σ_(j <= i) C(i, j) / C(5, j) a_j`.
    fn quintic_extrusion() -> Surface {
        use crate::nurbs::b_spline_surface::BSplineSurface;
        use crate::nurbs::srf_basic::Surface as NurbSurface;
        // q's power coefficients a_0..a_5, from its roots
        let mut a = vec![1.0_f64];
        for r in [0.0, 0.25, 0.5, 0.75, 1.0] {
            let mut next = vec![0.0; a.len() + 1];
            for (j, &c) in a.iter().enumerate() {
                next[j + 1] += c;
                next[j] -= r * c;
            }
            a = next;
        }
        let choose = |n: usize, k: usize| -> f64 {
            (0..k).fold(1.0, |acc, i| acc * (n - i) as f64 / (i + 1) as f64)
        };
        let net: Vec<Vec<CPoint>> = (0..=5)
            .map(|i| {
                let b: f64 = (0..=i).map(|j| choose(i, j) / choose(5, j) * a[j]).sum();
                [-1.0, 1.0]
                    .iter()
                    .map(|&y| CPoint::new(i as f64 / 5.0, y, b, NOW))
                    .collect()
            })
            .collect();
        let nurb = NurbSurface::new(
            net,
            5,
            1,
            vec![0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0],
            vec![0.0, 0.0, 1.0, 1.0],
        )
        .unwrap();
        Surface::from(BSplineSurface::from_nurb(nurb, false))
    }

    /// Review r4674266684: the straight bridge along `z = 0` from `x = 0` to `x = 1` meets
    /// the quintic extrusion at both ends and at all three probes (its quarters, roots of
    /// `q`), yet leaves it between them by up to about `|q(0.1)| = 0.0035`: it is not on the
    /// surface. A chord along the extrusion's ruling at a root, and a chord in a plane, are.
    #[test]
    fn a_chord_on_a_surface_at_its_probes_only_is_not_a_straight_bridge() {
        let k = Kernel::default();
        let tol = 1.0e-3;
        let surface = quintic_extrusion();
        let domain = surface.natural_uv_domain_with_nurb(&k).unwrap();
        let chord = BSplineCurve::create_line_segment(
            3,
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
        )
        .unwrap();
        // every probe is on the surface: the probes alone took this chord
        for x in [0.25, 0.5, 0.75] {
            let sols = global_point_solve_starts(
                &k,
                &surface,
                &domain,
                SolverOperation::Minimize,
                Point3::new(x, 0.0, 0.0),
                tol,
                SolutionRequested::Single,
            )
            .unwrap();
            assert!(
                sols[0].solution_value <= 1.0e-9,
                "{x}: {}",
                sols[0].solution_value
            );
        }
        assert!(!chord_on_surface(&k, &surface, &domain, &chord, tol).unwrap());

        let ruling = BSplineCurve::create_line_segment(
            3,
            Point3::new(0.5, -0.5, 0.0),
            Point3::new(0.5, 0.5, 0.0),
        )
        .unwrap();
        assert!(chord_on_surface(&k, &surface, &domain, &ruling, tol).unwrap());

        let mut plane = Plane::new(
            Point3::new(0.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            Vec2::new(1.0, 1.0),
            &Extent2d::from_points(Point2::new(-2.0, -2.0), Point2::new(2.0, 2.0)),
        );
        plane.make_nurb().unwrap();
        let plane = Surface::from(plane);
        let plane_domain = plane.natural_uv_domain_with_nurb(&k).unwrap();
        assert!(chord_on_surface(&k, &plane, &plane_domain, &chord, tol).unwrap());
    }

    /// Review r4674266697: of a plane's sections through both ends, one held by the plane
    /// and one whose piece leaves it, the held one is the bridge in either order; two held
    /// pieces leave it ambiguous.
    #[test]
    fn a_section_the_plane_does_not_hold_never_makes_the_bridge_ambiguous() {
        let k = Kernel::default();
        let tol = 1.0e-4;
        let ends = [Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)];
        let off = |p: Point3| p.z.abs();
        let held = Curve::from(spline(
            &[[-0.5, 0.0, 0.0], [1.5, 0.0, 0.0]],
            1,
            &[0.0, 0.0, 1.0, 1.0],
        ));
        // through both ends, bowing up off the plane between them
        let leaving = Curve::from(spline(
            &[
                [-0.5, 0.0, 0.0],
                [0.0, 0.0, 0.0],
                [0.5, 0.0, 0.5],
                [1.0, 0.0, 0.0],
                [1.5, 0.0, 0.0],
            ],
            1,
            &[0.0, 0.0, 0.25, 0.5, 0.75, 1.0, 1.0],
        ));
        let other_held = Curve::from(spline(
            &[[-1.0, 0.0, 0.0], [2.0, 0.0, 0.0]],
            1,
            &[0.0, 0.0, 1.0, 1.0],
        ));
        for curves in [
            vec![held.clone(), leaving.clone()],
            vec![leaving.clone(), held.clone()],
        ] {
            let Held::One(piece) = held_section(&k, &curves, ends, &off, tol).unwrap() else {
                panic!("the held section is the bridge");
            };
            let ivl = piece.natural_interval();
            assert!(piece.evaluate_point(ivl.min()).unwrap().distance(ends[0]) <= tol);
            assert!(piece.evaluate_point(ivl.max()).unwrap().distance(ends[1]) <= tol);
        }
        assert!(matches!(
            held_section(&k, std::slice::from_ref(&leaving), ends, &off, tol).unwrap(),
            Held::None
        ));
        assert!(matches!(
            held_section(&k, &[held, leaving, other_held], ends, &off, tol).unwrap(),
            Held::Two
        ));
    }

    /// Review r4674266691: only the typed empty result passes a plane over; any other
    /// failure of the intersection is no "no section".
    #[test]
    fn only_an_empty_intersection_is_no_section() {
        let op = "test";
        assert!(is_no_section(&SolidError::intersect(
            op,
            IntersectFailure::NoIntersection
        )));
        assert!(!is_no_section(&SolidError::intersect(
            op,
            IntersectFailure::UvPolygonNotInDomain
        )));
        assert!(!is_no_section(&SolidError::invalid_input(
            op,
            "not an intersection"
        )));
    }

    /// Review r4674266706: a curved gap segment is bordered by an edge with the same ends
    /// only where the edge's curve runs along it (here the same arc, run the other way); a
    /// straight edge between the same ends, 0.1 from the arc's middle, is no border.
    #[test]
    fn a_curved_segment_is_bordered_only_by_an_edge_along_it() {
        let tol = 1.0e-4;
        let knots = [0.0, 0.0, 0.0, 1.0, 1.0, 1.0];
        let arc = spline(
            &[[0.0, 0.0, 0.0], [0.5, 0.2, 0.0], [1.0, 0.0, 0.0]],
            2,
            &knots,
        );
        let back = spline(
            &[[1.0, 0.0, 0.0], [0.5, 0.2, 0.0], [0.0, 0.0, 0.0]],
            2,
            &knots,
        );
        let chord = spline(
            &[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]],
            1,
            &[0.0, 0.0, 1.0, 1.0],
        );
        let mut b = Brep::new();
        let v0 = b.add_vertex(Vec3::new(0.0, 0.0, 0.0)).unwrap();
        let v1 = b.add_vertex(Vec3::new(1.0, 0.0, 0.0)).unwrap();
        let mut edge_of = |c: BSplineCurve| {
            let e = b.add_edge(v1, v0).unwrap();
            let rec = b.edge_mut(e).unwrap();
            rec.interval = c.natural_interval();
            rec.curve = Some(Curve::from(c));
            e
        };
        let (along, across) = (edge_of(back), edge_of(chord));
        assert!(curved_side_coincides(&b, along, &arc, tol).unwrap());
        assert!(!curved_side_coincides(&b, across, &arc, tol).unwrap());
    }

    /// Review r4674266724: an arc from `(0, 0)` to `(2, 0)` through height 1 (a parabola,
    /// control point `(1, 2)`), closed back along its chord, bounds `4/3` (two thirds of base
    /// times height), though its corners are collinear. A polyline's part is Newell's sum.
    #[test]
    fn a_curved_side_bounds_area_its_corners_do_not() {
        let origin = Point3::new(0.0, 0.0, 0.0);
        let arc = spline(
            &[[0.0, 0.0, 0.0], [1.0, 2.0, 0.0], [2.0, 0.0, 0.0]],
            2,
            &[0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        );
        let part = curved_area_vector(origin, &arc).unwrap().unwrap();
        // the arc runs clockwise about +z, and the chord back through the origin adds nothing
        assert!((part.twice_area - Vec3::new(0.0, 0.0, -8.0 / 3.0)).length() <= 1.0e-14);
        assert!(
            part.rounding > 0.0 && part.rounding < 1.0e-12,
            "{}",
            part.rounding
        );
        assert!(part.length >= 2.0 * 5.0_f64.sqrt() - 1.0e-12);
        let polyline = spline(
            &[[0.0, 0.0, 0.0], [1.0, 1.0, 0.0], [2.0, 0.0, 0.0]],
            1,
            &[0.0, 0.0, 0.5, 1.0, 1.0],
        );
        let part = curved_area_vector(origin, &polyline).unwrap().unwrap();
        assert!((part.twice_area - Vec3::new(0.0, 0.0, -2.0)).length() <= 1.0e-14);
    }
}
