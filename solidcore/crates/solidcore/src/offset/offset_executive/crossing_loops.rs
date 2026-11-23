//! Offset faces whose loops cross one another, rebuilt before the piecewise merge.
//!
//! Upstream: none at the pinned commit; ported from the SMLib fork (tangency-labs/smlib,
//! "SMLib: an offset face whose loops cross is rebuilt from the stretches that bound
//! it", `source/SMLib/src/SmOffsetExecutive.cpp`), tangency-labs/solidcore#854, WR-345.
//!
//! | Rust                                         | Fork                                           |
//! |----------------------------------------------|------------------------------------------------|
//! | [`OffsetExecutive::resolve_crossing_loops`]  | `SmOffsetExecutive::ResolveCrossingLoops`       |
//! | `resolve_crossing_loops_of_face` (private)   | `SmOffsetExecutive::ResolveCrossingLoopsOfFace` |
//! | `LoopStretch` (private)                      | `sm_LoopStretch`                               |
//! | `unresolved` (private)                       | `sm_UnresolvedCrossing`                        |
//! | `walls_touch` (private)                      | `sm_WallsTouch` (a759622)                      |
//! | `loop_directions_at` (private)               | `sm_LoopDirectionsAt` (a759622)                |
//! | `loop_touches_at` (private)                  | `sm_LoopTouchesAt` (a759622)                   |
//! | `LoopContact` (private)                      | `sm_LoopContact` (a759622)                     |
//! | `floor_is_cavity` (private)                  | `sm_FloorIsCavity` (1b016ca)                   |
//! | `outward_of_body` (private)                  | `sm_OutwardOfBody` (1b016ca)                   |
//!
//! Deviations: always on (it was the opt-in `Kernel::fixes.offset_resolve_crossing_loops`
//! until that flag was retired); called by `do_extended_offset`. The reason a crossing is
//! not resolved is the error's `operation` text instead of an `smos_ErrorMessage` report.
//! `SmBrep::MakeFaceFromLoopOnFace` is not ported: a face that comes apart is built with one
//! `MakeFaceWithCurves` per outer loop (each a sheet of its own, as the fork's
//! `SplitDisconnectedShells` leaves them), and one that comes apart and keeps holes as well
//! is not resolved (`SM_ERR_BAD_INTERSECTIONS`). No recursion; every loop is bounded by the
//! face's edge uses and crossings. The fork's later a759622 ("an offset whose walls would
//! meet is refused as such": every contact is examined before any crossing, and loops that
//! touch without crossing fail with `SM_ERR_OFFSET_WALLS_TOUCH`) is built in, not 6169ea1's
//! routine (contacts checked as they are found, a touch reported as whichever crossing
//! check fails first, `SM_ERR_BAD_INTERSECTIONS`).
//! The fork's 1b016ca (regions the loops wind round the other way under an opening are
//! kept, reversed, where they are the cavity's floor) is part of the routine (WR-345
//! addendum): it adds the original body to the routine's inputs for its point tests.
//!
//! Not in the fork: a loop of one edge use that no crossing cuts is not taken for a stretch
//! too short to build, though its closed curve's two ends meet (the fork refuses an uncut
//! hole circle beside a crossing as three loops at one point). Where the offset's results
//! are not merged (the local operation's included), a face whose loops cross is refused
//! ([`crate::error::offset::LoopCrossing::Unmerged`]): only the merge cuts the walls the
//! crossing loops come from where they pass through each other. In the local operation,
//! whose final gate refuses a body that is not sound, a face whose loops cross is rebuilt
//! where at every crossing the walls of the two loops leave it on opposite sides
//! (solidcraft#18374: a boss whose foot the drafted side now runs inside): they meet only
//! at the crossing, and each is split there before the stitch.
//!
//! Not in the fork (solidcraft#16983): a face of one loop is resolved as well. Where the
//! offsets of the faces either side of a face cross over each other (a wall thicker than
//! half a rib between them), the face's offset turns inside out, wholly (the rib's top) or
//! in part (the end face across the rib, whose loop then crosses itself). The loop is cut
//! where it crosses itself and closed into simple loops (`classify_one_loop`); those whose
//! signed area is negative are regions turned inside out, kept reversed where they are the
//! cavity's floor and dropped otherwise, by the same rule as fork 1b016ca's.
//! Source correspondence follows the face builder's input-curve owners, so a
//! shortened stretch retains its source without a nearest-midpoint selection.
//!
//! Not in the fork (solidcore#3329): such a region is the cavity's floor only under an
//! opening. One that lies within a wall (every stretch round it bounds an offset wall) is
//! dropped, and the walls across it, which end on its edges, are carried on past it by a
//! strip on each wall's surface; the merge cuts them where they meet and the rest goes
//! with the excess faces.

use std::collections::BTreeMap;

use super::p1::{OffsetExecutive, OffsetOperation};
use super::p2::OffsetNotifications;
use crate::curve::curve::p8::CurveIntersectOptions;
use crate::curve::curve::Curve;
use crate::error::{EntityRef, Result, SolidError, SM_ERR_ASSERT_FAILURE, SM_ERR_NULL_POINTER};
use crate::geom::extent1d::Extent1d;
use crate::geom::global_solver::{SolutionRequested, SolverOperation};
use crate::geom::object::NotifyOperation;
use crate::geom::point_set::PointContainment;
use crate::geom::solution_array::SolutionType;
use crate::geom::tol::ANG_TOL_DEG;
use crate::geom::tol::{brep_zone_tol_3d, SmallTopoType};
use crate::geom::vector2d::Point2;
use crate::intersect::point_class::{live_owned_surface_global_point_solve, PointClassType};
use crate::kernel::Kernel;
use crate::math::{Point3, Vec3};
use crate::nurbs::b_spline_curve::BSplineCurve;
use crate::nurbs::b_spline_curve_n_lib::TrimMode;
use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
use crate::nurbs::knots::Side;
use crate::query::brep::BrepQuery;
use crate::query::edge::closest_point;
use crate::query::r#loop::contains_uv_point;
use crate::surface::surface::exact_pieces::{common_trim_interval, trim_unsnapped};
use crate::surface::surface::p1::{map_to_2d, zone_tol_3d};
use crate::surface::surface::p4::NoObject;
use crate::topology::brep::p1 as bp1;
use crate::topology::brep::p3::{delete_face_with_body_identity, DeleteFaceOptions};
use crate::topology::brep::p7::{
    make_face_with_curve_owners, make_face_with_curves_with_kernel_context, FaceCurvesInput,
};
use crate::topology::brep::p9::make_vertex_split_edge_with_state;
use crate::topology::edge::{self, NotificationStores, NotifyState};
use crate::topology::edgeuse::{p1 as eu, p3 as eu3};
use crate::topology::face::p2 as face;
use crate::topology::face::p3 as face3;
use crate::topology::face::p3::uv_loop_orientation;
use crate::topology::topology;
use crate::topology::topology_traverser::face_edges;
use crate::topology::{loopuse, vertexuse, Brep, EdgeId, EdgeuseId, FaceId, LoopId, LoopuseStart};

const OP: &str = "offset::offset_executive::crossing_loops::resolve_crossing_loops";

use crate::error::offset::{LoopCrossing, OffsetFailure, WallContact};
use crate::error::Failure;
// `SM_ERR_ASSERT_FAILURE`.

/// A stretch of a loop of an offset face between two of its crossings with other loops
/// or the ends of its edge use, in the loop's direction (`sm_LoopStretch`).
#[derive(Debug, Clone)]
struct LoopStretch {
    /// `lLoop`: its loop, 0 the outer loop.
    loop_index: usize,
    /// `pEU`: the edge use (of the face's `SM_OT_SAME` face use) it is a stretch of.
    edgeuse: EdgeuseId,
    /// `pUV`: the index of that edge use's UV trim curve.
    uv: usize,
    /// `dLo`, `dHi`: its edge-parameter interval.
    lo: f64,
    hi: f64,
    /// `lStart`, `lEnd`: the nodes it starts and ends at, a crossing (`>= 0`) or the
    /// vertex ending edge use `k` of the face (`-1 - k`).
    start: i64,
    end: i64,
    /// `bKeep`: it bounds the face.
    keep: bool,
    /// `bFloor` (fork 1b016ca): it bounds a region the loops wind round the other way.
    floor: bool,
    /// `sMid`: its middle point.
    mid: Point3,
    /// (not upstream) its edge use runs with its edge.
    same: bool,
    /// (not upstream) of a face of one loop, the stretch it goes on to in the loop it
    /// closes into (`classify_one_loop`), so that two loops through one crossing stay apart.
    follows: Option<usize>,
}

/// A crossing of the loops of an offset face that is not resolved
/// (`sm_UnresolvedCrossing`): the offset fails with `SM_ERR_BAD_INTERSECTIONS` rather
/// than merge a face whose loops cross; `crossing` names the configuration. The face is
/// named by `resolve_crossing_loops`, which knows the input face it comes from.
fn unresolved(crossing: LoopCrossing) -> SolidError {
    Failure::new(
        OP,
        OffsetFailure::LoopsCross {
            face: None,
            crossing,
        },
    )
    .into_error()
}

/// Loops of an offset face that touch without crossing (`sm_WallsTouch`, fork a759622):
/// the offset walls they come from meet there, so the offset would pinch to nothing
/// along a line (or over a stretch). No face is bounded there, and the offset is refused
/// with `SM_ERR_OFFSET_WALLS_TOUCH`; `contact` names the configuration.
fn walls_touch(contact: WallContact) -> SolidError {
    Failure::new(
        OP,
        OffsetFailure::WallsTouch {
            face: None,
            contact,
        },
    )
    .into_error()
}

/// Where edge uses `a` and `b` of different loops meet (`sm_LoopContact`, fork a759622):
/// their indices and their edge parameters there.
#[derive(Debug, Clone, Copy)]
struct LoopContact {
    a: usize,
    b: usize,
    t_a: f64,
    t_b: f64,
}

/// The first derivative of `curve` at `t` (`Evaluate(t, 1, from_left, aEval)`, `aEval[1]`).
fn curve_tangent(k: &Kernel, curve: &Curve, t: f64, from_left: bool) -> Result<Vec3> {
    let eval = curve.evaluate(k, t, 1, from_left, true)?;
    Ok(*present(eval.get(1), EVAL_1ST)?)
}

/// The directions in which a loop (its edge uses `loop_edgeuses`) leaves the point `p`
/// of its edge use `on` (at edge parameter `t`) (`sm_LoopDirectionsAt`, fork a759622):
/// both ways along `on`'s curve where `p` lies inside the edge; where `p` is a vertex of
/// the loop (within `tol_3d`), along each of the loop's edges from it.
fn loop_directions_at(
    k: &Kernel,
    b: &Brep,
    loop_edgeuses: &[EdgeuseId],
    on: EdgeuseId,
    t: f64,
    p: Point3,
    tol_3d: f64,
) -> Result<Vec<Vec3>> {
    let mut dirs = Vec::new();
    let on_edge = edge_of(b, on)?;
    if p.distance(vertex_point(b, edge::start_vertex(b, on_edge)?)?) > tol_3d
        && p.distance(vertex_point(b, edge::end_vertex(b, on_edge)?)?) > tol_3d
    {
        let d = curve_tangent(k, edge_curve(b, on_edge)?, t, true)?;
        dirs.push(d);
        dirs.push(-d);
        return Ok(dirs);
    }
    for &edgeuse in loop_edgeuses {
        let e = edge_of(b, edgeuse)?;
        let ivl = b.edge(e)?.interval;
        if p.distance(vertex_point(b, edge::start_vertex(b, e)?)?) <= tol_3d {
            dirs.push(curve_tangent(k, edge_curve(b, e)?, ivl.min(), true)?);
        }
        if p.distance(vertex_point(b, edge::end_vertex(b, e)?)?) <= tol_3d {
            dirs.push(-curve_tangent(k, edge_curve(b, e)?, ivl.max(), false)?);
        }
    }
    Ok(dirs)
}

/// Whether loop B touches loop A where they meet (`sm_LoopTouchesAt`, fork a759622;
/// `normal` the face's normal there, `dirs_a` and `dirs_b` the directions each leaves
/// that point in): it does when it leaves along one of A's directions (they are tangent
/// there, to SMLib's angle tolerance for parallel vectors), or when both its directions
/// lie on the same side of A, so that it comes up to A and goes back without passing
/// through it. A loop passing the point more than once is not told apart (`false`).
fn loop_touches_at(dirs_a: &[Vec3], dirs_b: &[Vec3], normal: Vec3) -> Result<bool> {
    let ([a0, a1], [b0, b1]) = (dirs_a, dirs_b) else {
        return Ok(false);
    };
    for &db in &[*b0, *b1] {
        for &da in &[*a0, *a1] {
            let (parallel, anti) = db.parallel_sense(da, ANG_TOL_DEG);
            if parallel && !anti.unwrap_or(false) {
                return Ok(true);
            }
        }
    }
    // The side of A from dirs_a[0] counterclockwise about the normal to dirs_a[1], and
    // where each of B's lies.
    let mut side = normal.ccw_angle_between(*a0, *a1)?;
    if side <= 0.0 {
        side += 2.0 * core::f64::consts::PI;
    }
    let mut on_side = [false; 2];
    for (slot, &db) in on_side.iter_mut().zip(&[*b0, *b1]) {
        let mut ang = normal.ccw_angle_between(*a0, db)?;
        if ang < 0.0 {
            ang += 2.0 * core::f64::consts::PI;
        }
        *slot = ang < side;
    }
    Ok(on_side[0] == on_side[1])
}

/// What [`cut_at_crossings_refusing_touch`] reads of `ResolveCrossingLoopsOfFace`'s
/// locals: the face's edge uses per loop, every edge use in loop order with its loop and
/// UV trim curve, where each loop's edge uses start, and the 3D and UV tolerances.
struct CrossingInputs<'a> {
    /// (not upstream, solidcraft#16983) a face of one loop: the contacts sought are that
    /// loop's with itself, of edge uses away from the vertices they share.
    one_loop: bool,
    loop_edgeuses: &'a [Vec<EdgeuseId>],
    loop_of: &'a [usize],
    edgeuses: &'a [EdgeuseId],
    uvs: &'a [Curve],
    first_of_loop: &'a [usize],
    tol_3d: f64,
    uv_tol: f64,
}

/// The crossings of the loops of offset face `f` as fork a759622 finds them, cutting each
/// edge use's `cuts` there and returning their number: every contact of edge uses of
/// different loops is found first (in UV space, as `SmFace` test #38 finds them), loops
/// that touch at any of them are refused (`SM_ERR_OFFSET_WALLS_TOUCH`), then a crossing
/// at a loop vertex is refused (`SM_ERR_BAD_INTERSECTIONS`) and the rest cut the loops.
fn cut_at_crossings_refusing_touch(
    kernel: &Kernel,
    b: &Brep,
    f: FaceId,
    inp: &CrossingInputs<'_>,
    cuts: &mut [Vec<(f64, i64)>],
) -> Result<i64> {
    let (edgeuses, uvs, loop_of, tol_3d) = (inp.edgeuses, inp.uvs, inp.loop_of, inp.tol_3d);
    // Where edge uses of different loops meet, in UV space; of a face of one loop, where
    // its edge uses meet away from the vertices they share.
    let mut contacts: Vec<LoopContact> = Vec::new();
    for aa in 0..edgeuses.len() {
        let first_b = if inp.one_loop {
            aa + 1
        } else {
            *at(inp.first_of_loop, *at(loop_of, aa)? + 1)?
        };
        for bb in first_b..edgeuses.len() {
            let (uv_a, uv_b) = (at(uvs, aa)?, at(uvs, bb)?);
            let solutions = uv_a.global_curve_intersect(
                kernel,
                &uv_a.natural_interval(),
                uv_b,
                &uv_b.natural_interval(),
                inp.uv_tol,
                &CurveIntersectOptions::<NoObject>::default(),
            )?;
            let shared = if inp.one_loop {
                shared_vertex_points(b, *at(edgeuses, aa)?, *at(edgeuses, bb)?)?
            } else {
                Vec::new()
            };
            for solution in solutions.as_slice() {
                // a single touch at a shared vertex is the loop's own corner; a run along
                // from it still goes on to the refusal below
                if !shared.is_empty() && solution.solution_type == SolutionType::SingleValue {
                    let e_a = edge_of(b, *at(edgeuses, aa)?)?;
                    let p = edge_curve(b, e_a)?.evaluate_point(kernel, solution.start.param(0)?)?;
                    if shared.iter().any(|v| p.distance(*v) <= tol_3d) {
                        continue;
                    }
                }
                if solution.solution_type != SolutionType::SingleValue {
                    return Err(walls_touch(WallContact::RunAlong));
                }
                contacts.push(LoopContact {
                    a: aa,
                    b: bb,
                    t_a: solution.start.param(0)?,
                    t_b: solution.start.param(1)?,
                });
            }
        }
    }
    if contacts.is_empty() {
        return Ok(0);
    }

    // Loops that touch without crossing bound no face there, however the rest of it is
    // resolved.
    let surface = present(b.face(f)?.surface.as_ref(), FACE_NO_SURFACE)?;
    for c in &contacts {
        let (e_a, e_b) = (
            edge_of(b, *at(edgeuses, c.a)?)?,
            edge_of(b, *at(edgeuses, c.b)?)?,
        );
        let p_a = edge_curve(b, e_a)?.evaluate_point(kernel, c.t_a)?;
        let p_b = edge_curve(b, e_b)?.evaluate_point(kernel, c.t_b)?;
        let uv3 = at(uvs, c.a)?.evaluate_point(kernel, c.t_a)?;
        let normal =
            surface.evaluate_normal(kernel, Point2::new(uv3.x, uv3.y), Side::Left, Side::Left)?;
        let dirs_a = loop_directions_at(
            kernel,
            b,
            at(inp.loop_edgeuses, *at(loop_of, c.a)?)?,
            *at(edgeuses, c.a)?,
            c.t_a,
            p_a,
            tol_3d,
        )?;
        let dirs_b = loop_directions_at(
            kernel,
            b,
            at(inp.loop_edgeuses, *at(loop_of, c.b)?)?,
            *at(edgeuses, c.b)?,
            c.t_b,
            p_b,
            tol_3d,
        )?;
        if loop_touches_at(&dirs_a, &dirs_b, normal)? {
            return Err(walls_touch(WallContact::Touch));
        }
    }

    // The crossings: where each edge use is cut.
    let mut num_crossings: i64 = 0;
    for c in &contacts {
        let (e_a, e_b) = (
            edge_of(b, *at(edgeuses, c.a)?)?,
            edge_of(b, *at(edgeuses, c.b)?)?,
        );
        let p_a = edge_curve(b, e_a)?.evaluate_point(kernel, c.t_a)?;
        let p_b = edge_curve(b, e_b)?.evaluate_point(kernel, c.t_b)?;
        if p_a.distance(vertex_point(b, edge::start_vertex(b, e_a)?)?) <= tol_3d
            || p_a.distance(vertex_point(b, edge::end_vertex(b, e_a)?)?) <= tol_3d
            || p_b.distance(vertex_point(b, edge::start_vertex(b, e_b)?)?) <= tol_3d
            || p_b.distance(vertex_point(b, edge::end_vertex(b, e_b)?)?) <= tol_3d
        {
            return Err(unresolved(LoopCrossing::AtLoopVertex));
        }
        at_mut(cuts, c.a)?.push((c.t_a, num_crossings));
        at_mut(cuts, c.b)?.push((c.t_b, num_crossings));
        num_crossings += 1;
    }
    Ok(num_crossings)
}

/// (not upstream, solidcraft#16983) The points of the vertices edge uses `a` and `b`
/// share (consecutive edge uses of a loop meet at one, a loop of two at both).
fn shared_vertex_points(b: &Brep, a: EdgeuseId, bb: EdgeuseId) -> Result<Vec<Point3>> {
    let (e_a, e_b) = (edge_of(b, a)?, edge_of(b, bb)?);
    let ends_a = [edge::start_vertex(b, e_a)?, edge::end_vertex(b, e_a)?];
    let ends_b = [edge::start_vertex(b, e_b)?, edge::end_vertex(b, e_b)?];
    let mut points = Vec::new();
    for v in ends_a.into_iter().flatten() {
        if ends_b.contains(&Some(v)) {
            points.push(b.vertex(v)?.point);
        }
    }
    Ok(points)
}

/// (not upstream, solidcraft#16983) Sorts the stretches of a face of one loop into the
/// loops they close into, and marks each kept (`keep`) or wound the other way (`floor`).
/// The loop is cut where it crosses itself (`stretches`, in its order, each crossing
/// passed twice); at a crossing a stretch goes on to the stretch leaving it on the other
/// pass, so that each loop closed is simple, and each is kept where its certified signed
/// area in UV ([`uv_loop_orientation`]) is positive and wound the other way where it is
/// negative: a region of the face turned inside out, where the offsets of the faces
/// beside it crossed over each other (a wall thicker than half a rib). A loop that
/// crosses nowhere is one loop. Whether the loops so closed nest is not examined: the
/// offset of one loop turned inside out in places makes them side by side.
///
/// # Errors
///
/// [`LoopCrossing::StretchUnclassified`] for a loop whose area has no certified sign or
/// a stretch with no stretch to go on to; those of the trims.
fn classify_one_loop(kernel: &Kernel, stretches: &mut [LoopStretch], uvs: &[Curve]) -> Result<()> {
    let n = stretches.len();
    // each stretch's successor: past a vertex of the loop the stretch after it, at a
    // crossing the stretch leaving it on the other pass
    let mut next = Vec::new();
    // the stretch after each in the loop's order: the next one, the first after the last
    let followers = (1..n).chain(core::iter::once(0));
    for (stretch, follower) in stretches.iter().zip(followers) {
        if stretch.end < 0 {
            next.push(follower);
            continue;
        }
        let other = (0..n)
            .find(|&tt| tt != follower && stretches.get(tt).is_some_and(|t| t.start == stretch.end))
            .ok_or_else(|| unresolved(LoopCrossing::StretchUnclassified))?;
        next.push(other);
    }
    let mut done = vec![false; n];
    for seed in 0..n {
        if *at(&done, seed)? {
            continue;
        }
        let mut chain = Vec::new();
        let mut ss = seed;
        while !*at(&done, ss)? {
            *at_mut(&mut done, ss)? = true;
            chain.push(ss);
            ss = *at(&next, ss)?;
        }
        if ss != seed {
            return Err(unresolved(LoopCrossing::StretchUnclassified));
        }
        let mut pieces = Vec::new();
        let mut senses = Vec::new();
        for &tt in &chain {
            let stretch = at(stretches, tt)?;
            let mut uv = at(uvs, stretch.uv)?.copy(kernel)?;
            let mut ivl = Extent1d::from_min_max(stretch.lo, stretch.hi);
            uv.trim(kernel, &mut ivl, TrimMode::Notify)?;
            pieces.push(uv);
            senses.push(stretch.same);
        }
        let orientation = uv_loop_orientation(kernel, &pieces, &senses, None)?;
        let (keep, floor) = match orientation {
            OrientType::Same => (true, false),
            OrientType::Opposite => (false, true),
            OrientType::Unknown => return Err(unresolved(LoopCrossing::StretchUnclassified)),
        };
        for &tt in &chain {
            let follows = Some(*at(&next, tt)?);
            let stretch = at_mut(stretches, tt)?;
            stretch.keep = keep;
            stretch.floor = floor;
            stretch.follows = follows;
        }
    }
    Ok(())
}

/// The UV trim curve of an edge use does not run over its edge's interval: a defect, with
/// the status `SM_ERR_ASSERT_FAILURE` of the check it replaces.
#[track_caller]
fn trim_interval_mismatch() -> SolidError {
    Failure::new(
        OP,
        OffsetFailure::Defect {
            why: "an offset edge use's UV trim curve does not span its edge's interval, though \
                  the kernel keeps an edge use's trim curve over its edge's interval",
        },
    )
    .with_status(SM_ERR_ASSERT_FAILURE)
    .into_error()
}

/// A defect of the offset's own data (`why` says what is missing), with the status
/// `SM_ERR_NULL_POINTER` of the `NER` it replaces.
#[track_caller]
fn defect(why: &'static str) -> SolidError {
    Failure::new(OP, OffsetFailure::Defect { why })
        .with_status(SM_ERR_NULL_POINTER)
        .into_error()
}

/// `value`, or the defect `why` ([`defect`]): a link or entity of the offset B-rep, or of
/// the routine's own data, that is not there.
#[track_caller]
fn present<T>(value: Option<T>, why: &'static str) -> Result<T> {
    match value {
        Some(v) => Ok(v),
        None => Err(defect(why)),
    }
}

/// Why an index into the routine's own lists is past their end.
const INDEX_PAST_END: &str = "an index into the crossings' own lists is past their end, though \
                              every index is taken from those lists";

/// A wall strip's result: a strip that cannot be made is `None`, as [`crate::error::ignored`]
/// reads it. A defect (the kernel's fault, [`SolidError::is_defect`]) is passed on with its
/// reason, never read as a strip that cannot be made
/// (`solidcore/docs/failure-codes-and-reproduction.md` §6, rule 5).
fn strip_result<T>(r: Result<T>) -> Result<Option<T>> {
    match r {
        Err(e) if e.is_defect() => Err(e),
        r => crate::error::ignored(r),
    }
}

/// Why a curve's evaluation to the first derivative gave no tangent (the words of
/// `intersect::curve_class::p1`).
const EVAL_1ST: &str = "the curve's evaluation to the first derivative returned fewer than two \
                        vectors, though it returns the point and each derivative asked for";

#[track_caller]
fn at<T>(v: &[T], i: usize) -> Result<&T> {
    present(v.get(i), INDEX_PAST_END)
}

#[track_caller]
fn at_mut<T>(v: &mut [T], i: usize) -> Result<&mut T> {
    present(v.get_mut(i), INDEX_PAST_END)
}

#[track_caller]
fn index_i64(i: usize) -> Result<i64> {
    match i64::try_from(i) {
        Ok(n) => Ok(n),
        Err(_) => Err(defect(
            "an edge use's index does not fit a node number, though a list holds no more \
             entries than an i64 counts",
        )),
    }
}

/// Why the offset B-rep is not there.
const NO_OFFSET: &str = "the offset B-rep is not there, though the executive builds it before \
                         it resolves crossing loops";
/// Why an offset face has no surface.
const FACE_NO_SURFACE: &str =
    "an offset face has no surface, though the offset gives every face it builds one";
/// Why an offset face has no face use.
const FACE_NO_FACEUSE: &str =
    "an offset face has no face use, though the kernel makes every face with its face uses";
/// Why a face use has no mate.
const FU_NO_MATE: &str = "a face use has no mate, though the kernel makes face uses in mated pairs";
/// Why a vertex loop has no vertex use.
const VLOOP_NO_VU: &str = "a vertex loop use has no vertex use, though the kernel starts every \
                           vertex loop use with one";
/// Why a vertex use has no vertex.
const VU_NO_VERTEX: &str =
    "a vertex use has no vertex, though the kernel links every vertex use to its vertex";
/// Why a loop use has no loop.
const LU_NO_LOOP: &str =
    "a loop use has no loop, though the kernel links every loop use to its loop";
/// Why an edge use has no UV trim curve.
const EU_NO_UV: &str = "an edge use has no UV trim curve right after the curve was made for it \
                        without an error";
/// Why an edge use has no edge.
const EU_NO_EDGE: &str =
    "an edge use has no edge, though the kernel links every edge use to its edge";
/// Why an edge use has no mate.
const EU_NO_MATE: &str =
    "an edge use has no mate, though the kernel makes edge uses in mated pairs";
/// Why an edge use has no face use.
const EU_NO_FU: &str = "an edge use of an offset face has no face use, though the kernel makes \
                        every edge use of a face in a loop of its face use";
/// Why an edge has no edge use.
const EDGE_NO_EU: &str =
    "an offset edge has no edge use, though the kernel makes every edge with its uses";
/// Why an offset edge has no curve.
const EDGE_NO_CURVE: &str =
    "an offset edge has no curve, though the offset gives every edge it builds one";
/// Why an edge has no vertex at an end.
const EDGE_NO_VERTEX: &str =
    "an offset edge has no vertex at an end, though the kernel bounds every edge by its vertices";
/// Why a face just built is not returned.
const NO_NEW_FACE: &str = "the face built from the kept stretches was not returned, though its \
                           construction succeeded";

/// The vertex node of edge use `k`: `-1 - k`.
fn vertex_node(k: usize) -> Result<i64> {
    Ok(-1 - index_i64(k)?)
}

#[track_caller]
fn edge_of(b: &Brep, edgeuse: EdgeuseId) -> Result<EdgeId> {
    present(eu::edge(b, edgeuse)?, EU_NO_EDGE)
}

#[track_caller]
fn edge_curve(b: &Brep, e: EdgeId) -> Result<&Curve> {
    present(b.edge(e)?.curve.as_ref(), EDGE_NO_CURVE)
}

#[track_caller]
fn vertex_point(b: &Brep, v: Option<crate::topology::VertexId>) -> Result<Point3> {
    Ok(b.vertex(present(v, EDGE_NO_VERTEX)?)?.point)
}

/// (not in the fork, solidcraft#18374) A crossing of the loops of an offset face: where it
/// is, and the two edge uses of the face it cuts.
struct Crossing {
    point: Point3,
    uv: Point2,
    edgeuses: [EdgeuseId; 2],
}

/// (not in the fork, solidcraft#18374) The crossings `cuts` holds (per edge use of
/// `edgeuses`, whose UV trim curves are `uvs`: its parameter and crossing there), each at
/// the point of the first edge use cut there.
fn crossings_of(
    kernel: &Kernel,
    b: &Brep,
    edgeuses: &[EdgeuseId],
    uvs: &[Curve],
    cuts: &[Vec<(f64, i64)>],
) -> Result<Vec<Crossing>> {
    let mut found: BTreeMap<i64, (Point3, Point2, Vec<EdgeuseId>)> = BTreeMap::new();
    for (gg, edge_cuts) in cuts.iter().enumerate() {
        let edgeuse = *at(edgeuses, gg)?;
        for &(t, crossing) in edge_cuts {
            let entry = match found.entry(crossing) {
                std::collections::btree_map::Entry::Occupied(o) => o.into_mut(),
                std::collections::btree_map::Entry::Vacant(v) => {
                    let p = edge_curve(b, edge_of(b, edgeuse)?)?.evaluate_point(kernel, t)?;
                    let uv = at(uvs, gg)?.evaluate_point(kernel, t)?;
                    v.insert((p, Point2::new(uv.x, uv.y), Vec::new()))
                }
            };
            entry.2.push(edgeuse);
        }
    }
    found
        .into_values()
        .map(|(point, uv, uses)| match uses.as_slice() {
            [a, bb] => Ok(Crossing {
                point,
                uv,
                edgeuses: [*a, *bb],
            }),
            _ => Err(unresolved(LoopCrossing::Unmerged)),
        })
        .collect()
}

/// (not in the fork) Whether a stretch whose ends are `distance` apart is too short to
/// build (`LoopCrossing::ThreeAtOnePoint`): its ends lie within `tol_3d`, unless it starts
/// and ends at the same node, a vertex (`< 0`). That is a loop of one edge use that no
/// crossing cuts, whole and as it was built, whose closed curve's two ends meet. A stretch
/// that ends at a crossing, or runs between two vertices, is tested as before. A NaN
/// (not in the fork, solidcore#3329) The parameters `[lo, hi]` of wall edge `wall` over the
/// stretch from `from` through `mid` to `to` (its points on the face across): each end's
/// closest parameter, taken at the other end of the edge's interval where it lies at one end
/// of a closed edge (a seam), so that the interval holds the middle's. `None` where an end
/// or the middle is not on the edge within `tol_3d`, or no such interval is found.
fn stretch_on_wall(
    kernel: &Kernel,
    b: &Brep,
    wall: EdgeId,
    from: Point3,
    mid: Point3,
    to: Point3,
    tol_3d: f64,
) -> Result<Option<(f64, f64)>> {
    let mut at = [0.0; 3];
    for (slot, p) in at.iter_mut().zip([from, mid, to]) {
        let Some(cp) = closest_point(kernel, b, wall, p)? else {
            return Ok(None);
        };
        if cp.distance.is_nan() || cp.distance > tol_3d {
            return Ok(None);
        }
        *slot = cp.parameter;
    }
    let [t0, tm, t1] = at;
    let ivl = b.edge(wall)?.interval;
    let closed = edge::start_vertex(b, wall)? == edge::end_vertex(b, wall)?;
    let eps = 1.0e-9 * (1.0 + ivl.max().abs().max(ivl.min().abs()));
    let ends = |t: f64| -> Vec<f64> {
        if closed && (t - ivl.min()).abs() <= eps {
            vec![t, ivl.max()]
        } else if closed && (t - ivl.max()).abs() <= eps {
            vec![t, ivl.min()]
        } else {
            vec![t]
        }
    };
    let mut best: Option<(f64, f64)> = None;
    for a in ends(t0) {
        for z in ends(t1) {
            let (lo, hi) = (a.min(z), a.max(z));
            if lo < tm && tm < hi && best.is_none_or(|(l, h)| hi - lo < h - l) {
                best = Some((lo, hi));
            }
        }
    }
    Ok(best)
}

/// distance or tolerance is too short, the whole loop's included.
fn stretch_too_short(from: i64, to: i64, distance: f64, tol_3d: f64) -> bool {
    let whole_loop = from < 0 && from == to;
    match distance.partial_cmp(&tol_3d) {
        None => true,
        Some(core::cmp::Ordering::Greater) => false,
        Some(_) => !whole_loop,
    }
}

impl OffsetExecutive {
    /// `ResolveCrossingLoops` (fork, not upstream): rebuilds every offset face whose
    /// loops cross one another. Each loop of an offset face is made of the offsets of
    /// its edges and is trimmed only where its own consecutive edges meet; nothing trims
    /// one loop of a face against another, so an inner loop whose offset grows past the
    /// outer loop's crosses it. Such a face is not valid (`SmFace` test #38) and the
    /// merge cannot use it.
    ///
    /// # Errors
    ///
    /// `SM_ERR_BAD_INTERSECTIONS` for crossings this does not resolve (see
    /// `resolve_crossing_loops_of_face`); `SM_ERR_OFFSET_WALLS_TOUCH` for loops that touch without crossing; those of the
    /// evaluations, intersections and face construction.
    pub(super) fn resolve_crossing_loops(
        &mut self,
        kernel: &mut Kernel,
        original: &mut Brep,
        notifications: &mut OffsetNotifications<'_>,
    ) -> Result<()> {
        let mut offset = present(self.take_offset_brep(), NO_OFFSET)?;
        let status = bp1::faces(&offset, None).and_then(|faces| {
            faces.into_iter().try_for_each(|f| {
                self.resolve_crossing_loops_of_face(kernel, original, &mut offset, f, notifications)
                    .map_err(|e| self.on_input_face(original, f, e))
            })
        });
        self.set_offset_brep(offset);
        status
    }

    /// `e`, a refusal of offset face `f`, naming the face of `original` that
    /// `f` comes from where the refusal names no face and `f` comes from exactly one
    /// (not upstream, #2615: error context only; any other error is returned as it is).
    fn on_input_face(&self, original: &Brep, f: FaceId, e: SolidError) -> SolidError {
        let SolidError::Failed(mut failure) = e else {
            return e;
        };
        let crate::error::Reason::Offset(refusal) = failure.reason else {
            return SolidError::Failed(failure);
        };
        // a failed routine's status (not a refusal) is returned as it is, as it was before
        // the routine described it
        if refusal.replaces_upstream_status() {
            return SolidError::Failed(failure);
        }
        let input_face = match self.faceuse_to_faces.firsts(f).as_slice() {
            [fu] => eu::faceuse_face(original, *fu).ok().flatten(),
            _ => None,
        };
        if let Some(face) = input_face {
            failure.reason = crate::error::Reason::Offset(refusal.on_face(face));
            failure.entity = failure.entity.or(Some(EntityRef::Face(face)));
        }
        SolidError::Failed(failure)
    }

    /// `ResolveCrossingLoopsOfFace` (fork, not upstream): when the loops of offset face
    /// `f` cross one another, replaces it with the face(s) of its surface inside the
    /// outer loop and outside every inner one, bounded by the stretches of the loops
    /// between their crossings. (not in the fork, solidcraft#16983) A face of one loop
    /// that crosses itself, or winds round the other way, is replaced by the regions of
    /// it that wind round the right way (see the module notes).
    ///
    /// The crossings are found in the face's UV space as `SmFace` test #38 finds them;
    /// every loop is cut there. A stretch of the outer loop bounds the face where it lies
    /// outside every inner loop, a stretch of an inner loop where it lies inside the
    /// outer loop and outside the other inner ones (`SmLoop::ContainsUVPoint` at its
    /// middle). The kept stretches, in their loops' directions, close into loops at the
    /// crossings; a new loop holding a stretch of the old outer loop bounds the face
    /// from outside. The face is rebuilt from each stretch's edge curve and UV trim
    /// curve trimmed to it, on a copy of its surface; the new faces, edges and vertices
    /// take the attributes and offset-map places of those they come from.
    #[allow(clippy::too_many_lines)] // one fork routine
    pub(super) fn resolve_crossing_loops_of_face(
        &mut self,
        kernel: &mut Kernel,
        original: &mut Brep,
        b: &mut Brep,
        f: FaceId,
        notifications: &mut OffsetNotifications<'_>,
    ) -> Result<()> {
        // (the face's tolerance model: SmTol::GetZoneTol3d reads the surface's face)
        let (zone_tol, uv_tol) = {
            let surface = present(b.face(f)?.surface.as_ref(), FACE_NO_SURFACE)?;
            let zone_tol = zone_tol_3d(kernel, surface)?;
            let uv_mid = surface.natural_uv_domain_with_nurb(kernel)?.mid();
            (zone_tol, map_to_2d(kernel, zone_tol, uv_mid, surface)?)
        };
        let mut fu = present(face::upward_faceuse(b, f)?, FACE_NO_FACEUSE)?;
        if eu::faceuse_orientation(b, fu)? != OrientType::Same {
            fu = present(eu::faceuse_mate(b, fu)?, FU_NO_MATE)?;
        }

        // The face's edge loops (the outer loop first) and the points of its vertex loops.
        let mut loops: Vec<LoopId> = Vec::new();
        let mut loop_edgeuses: Vec<Vec<EdgeuseId>> = Vec::new();
        let mut loop_points: Vec<Point3> = Vec::new();
        for lu in eu::faceuse_loopuses(b, fu, false, None)? {
            if let LoopuseStart::Vertexuse(_) = b.loopuse(lu)?.start {
                let vus = loopuse::vertexuses(b, lu, None)?;
                let vu = present(vus.first().copied().flatten(), VLOOP_NO_VU)?;
                let v = present(vertexuse::vertex(b, vu)?, VU_NO_VERTEX)?;
                loop_points.push(b.vertex(v)?.point);
                continue;
            }
            loops.push(present(b.loopuse(lu)?.loop_, LU_NO_LOOP)?);
            loop_edgeuses.push(eu::loopuse_edgeuses(b, lu)?);
        }
        // (not upstream, solidcraft#16983) a face of one loop: the loop crosses itself, or
        // winds round the other way, where the face turned inside out
        let one_loop = loops.len() == 1;
        if loops.is_empty() {
            return Ok(());
        }
        // (not upstream) only a face with an edge whose offset is inverted turns inside
        // out (`TrimmedEdge`); every other face of one loop is left as it is
        if one_loop && !self.inverted_faces.contains(&f) {
            return Ok(());
        }

        // Every edge use, in loop order, with its UV trim curve.
        let mut loop_of: Vec<usize> = Vec::new();
        let mut edgeuses: Vec<EdgeuseId> = Vec::new();
        let mut uvs: Vec<Curve> = Vec::new();
        let mut first_of_loop: Vec<usize> = Vec::new();
        let mut tol_3d = self.approximation_tol_3d();
        for (ll, list) in loop_edgeuses.iter().enumerate() {
            first_of_loop.push(edgeuses.len());
            for &edgeuse in list {
                eu3::get_or_create_uv_trim_curve(kernel, b, edgeuse, false, false, true)?.status?;
                let uv = present(eu::uv_trim_curve(b, edgeuse)?.0.cloned(), EU_NO_UV)?;
                let e = edge_of(b, edgeuse)?;
                if !uv.natural_interval().are_equal(&b.edge(e)?.interval, 0.0) {
                    return Err(trim_interval_mismatch());
                }
                loop_of.push(ll);
                edgeuses.push(edgeuse);
                uvs.push(uv);
                tol_3d = tol_3d.max(b.edge(e)?.tolerance);
            }
        }
        first_of_loop.push(edgeuses.len());
        tol_3d *= 10.0;

        // The crossings of edge uses of different loops, in UV space (as SmFace test
        // #38 finds them).
        // per edge use: (parameter, crossing)
        let mut cuts: Vec<Vec<(f64, i64)>> = vec![Vec::new(); edgeuses.len()];
        // Fork a759622: every contact is looked at before any crossing is resolved
        // or refused.
        let num_crossings = cut_at_crossings_refusing_touch(
            kernel,
            b,
            f,
            &CrossingInputs {
                one_loop,
                loop_edgeuses: &loop_edgeuses,
                loop_of: &loop_of,
                edgeuses: &edgeuses,
                uvs: &uvs,
                first_of_loop: &first_of_loop,
                tol_3d,
                uv_tol,
            },
            &mut cuts,
        )?;
        if num_crossings == 0 {
            // (not upstream) a loop crossing nowhere bounds the face unless it winds round
            // the other way: the whole face turned inside out
            let inverted = one_loop
                && face3::compute_loop_orientation(kernel, b, f, *at(&loops, 0)?, None, false)?
                    == OrientType::Opposite;
            if !inverted {
                return Ok(());
            }
        }

        // (not in the fork) Rebuilding this face trims it alone. Where its loops cross, an
        // offset wall a crossing loop's edges bound (the offset of the face across that
        // edge) is cut there only by the merge (`PiecewiseMerge` with merged results). The
        // unmerged path only stitches the offset faces, so where two such walls pass
        // through each other they stay whole: the stitch encloses no region (the face
        // rebuilt) or the face keeps loops that cross (`SmFace` test #38, the face left as
        // it is). Body type does not tell where that happens: an open sheet with a hole
        // through a face of its own has the same two walls as a solid. An unmerged offset
        // whose face's loops cross is therefore refused, unless it is the local operation
        // and at every crossing the walls of the two loops leave the face on opposite
        // sides of it (solidcraft#18374:
        // a boss on a face whose drafted side now runs inside the boss's foot): there the
        // two walls meet only at the crossing, on their edges, and each wall's edge is
        // split at it (`split_walls_at_crossings`), so the stitch joins every stretch to
        // the wall it bounds. With the results merged the same offset is built either way.
        // The local operation (always unmerged) is no exception: its offset faces are
        // stitched alike before they are moved into the body.
        if !self.merge_results() {
            // (a face turned inside out whole, crossing nowhere, comes of walls that
            // passed each other; only the local operation's final gate holds the stitched
            // body to a sound solid, so the other unmerged offsets stay refused)
            let crossings = crossings_of(kernel, b, &edgeuses, &uvs, &cuts)?;
            if self.operation() != OffsetOperation::LocalOperation
                || crossings.is_empty()
                || !self.walls_part_at_crossings(kernel, original, b, f, &crossings, tol_3d)?
            {
                return Err(unresolved(LoopCrossing::Unmerged));
            }
            self.split_walls_at_crossings(
                kernel,
                original,
                b,
                f,
                &crossings,
                tol_3d,
                notifications,
            )?;
            // the face is rebuilt from its stretches, each edge new (cut at a crossing, or
            // run the other way round a region turned over): no edge of it is kept as it
            // was ([`OffsetExecutive::keep_unmoved_edges`])
            for &edgeuse in &edgeuses {
                for offset_use in [Some(edgeuse), eu::mate(b, edgeuse)?].into_iter().flatten() {
                    for source in self.edgeuse_to_edgeuses.firsts(offset_use) {
                        let Some(e) = eu::edge(original, source)? else {
                            continue;
                        };
                        if self.kept_edges.remove(&e) {
                            for u in edge::edgeuses(original, e, None, None)? {
                                self.kept_edgeuses.remove(&u);
                            }
                        }
                    }
                }
            }
        }

        // Until the merge every offset face is a sheet of its own: its edges bound it
        // alone.
        for &edgeuse in &edgeuses {
            if !eu::edge_is_lamina(b, edge_of(b, edgeuse)?)? {
                return Err(unresolved(LoopCrossing::SharesEdge));
            }
        }

        // Cut every edge use at its crossings into stretches, in the loop's direction.
        let mut stretches: Vec<LoopStretch> = Vec::new();
        for (gg, &edgeuse) in edgeuses.iter().enumerate() {
            let e = edge_of(b, edgeuse)?;
            let same = eu::orientation(b, edgeuse)? == OrientType::Same;
            let ivl = b.edge(e)?.interval;
            let edge_cuts = at_mut(&mut cuts, gg)?;
            edge_cuts.sort_by(|x, y| x.0.total_cmp(&y.0).then(x.1.cmp(&y.1)));
            if !same {
                edge_cuts.reverse();
            }
            let edge_cuts = edge_cuts.clone();
            let loop_index = *at(&loop_of, gg)?;
            let first = *at(&first_of_loop, loop_index)?;
            let prev = if gg == first {
                *at(&first_of_loop, loop_index + 1)? - 1
            } else {
                gg - 1
            };
            let mut from = if same { ivl.min() } else { ivl.max() };
            let mut from_node = vertex_node(prev)?;
            for cc in 0..=edge_cuts.len() {
                let (to, to_node) = match edge_cuts.get(cc) {
                    Some(&(t, crossing)) => (t, crossing),
                    None => (if same { ivl.max() } else { ivl.min() }, vertex_node(gg)?),
                };
                let lo = from.min(to);
                let hi = from.max(to);
                let curve = edge_curve(b, e)?;
                let from_pt = curve.evaluate_point(kernel, lo)?;
                let to_pt = curve.evaluate_point(kernel, hi)?;
                if hi.partial_cmp(&lo) != Some(core::cmp::Ordering::Greater)
                    || stretch_too_short(from_node, to_node, from_pt.distance(to_pt), tol_3d)
                {
                    return Err(unresolved(LoopCrossing::ThreeAtOnePoint));
                }
                let mid = curve.evaluate_point(kernel, 0.5 * (lo + hi))?;
                stretches.push(LoopStretch {
                    loop_index,
                    edgeuse,
                    uv: gg,
                    lo,
                    hi,
                    start: from_node,
                    end: to_node,
                    keep: false,
                    floor: false,
                    mid,
                    same,
                    follows: None,
                });
                from = to;
                from_node = to_node;
            }
        }

        // Which stretches bound the face: of the outer loop, those outside every inner
        // loop; of an inner loop, those inside the outer loop and outside the other
        // inner ones. (not upstream) Of a face of one loop, those of the loops it closes
        // into that wind round the right way (`classify_one_loop`).
        if one_loop {
            classify_one_loop(kernel, &mut stretches, &uvs)?;
        } else {
            for ss in 0..stretches.len() {
                let stretch = at(&stretches, ss)?;
                let uv3 = at(&uvs, stretch.uv)?
                    .evaluate_point(kernel, 0.5 * (stretch.lo + stretch.hi))?;
                let uv = Point2::new(uv3.x, uv3.y);
                let own_loop = stretch.loop_index;
                let mut keep = true;
                let mut in_outer = own_loop == 0;
                let mut in_inner = 0_usize;
                for (ll, &l) in loops.iter().enumerate() {
                    if ll == own_loop {
                        continue;
                    }
                    let found = contains_uv_point(kernel, b, l, None, uv, zone_tol)?.containment;
                    if found != PointContainment::Inside && found != PointContainment::Outside {
                        return Err(unresolved(LoopCrossing::StretchUnclassified));
                    }
                    let wanted = if ll == 0 {
                        PointContainment::Inside
                    } else {
                        PointContainment::Outside
                    };
                    if found != wanted {
                        keep = false;
                    }
                    if ll == 0 {
                        in_outer = found == PointContainment::Inside;
                    } else if found == PointContainment::Inside {
                        in_inner += 1;
                    }
                }
                let stretch = at_mut(&mut stretches, ss)?;
                stretch.keep = keep;
                // Fork 1b016ca: the stretches round a region the loops wind round once the
                // other way: a hole's stretch outside the outer loop and every other hole,
                // and the outer loop's stretch inside one hole (the region of a hole that ran
                // out past the moved outer loop).
                stretch.floor = if own_loop != 0 {
                    !in_outer && in_inner == 0
                } else {
                    in_inner == 1
                };
            }
        }

        // Fork 1b016ca: a region the loops wind round the other way is the offset face
        // turned inside out there, cavity on its other side (a lid shelled open at a
        // rib's outer side standing just in from the skirt's outer wall: the skirt top's
        // offset has the rib's foot as a hole, whose side along the open face stays while
        // the outer loop's side along the wall moves past it; between them, under the
        // opening, lies the cavity's floor). Dropped, the cavity was left open there.
        // Such a region is kept, as a reversed face, exactly when the cavity lies on its
        // other side (`floor_is_cavity`); otherwise it bounds nothing and is dropped as
        // before. Chain the floor stretches into loops.
        let mut next_floor: Vec<Option<usize>> = vec![None; stretches.len()];
        for ss in 0..stretches.len() {
            if !at(&stretches, ss)?.floor {
                continue;
            }
            let end = at(&stretches, ss)?.end;
            let follows = at(&stretches, ss)?.follows;
            for (tt, other) in stretches.iter().enumerate() {
                if other.floor && other.start == end && follows.is_none_or(|f| f == tt) {
                    *at_mut(&mut next_floor, ss)? = Some(tt);
                }
            }
            if at(&next_floor, ss)?.is_none() {
                at_mut(&mut stretches, ss)?.floor = false;
            }
        }
        let mut floor_loops: Vec<Vec<usize>> = Vec::new();
        {
            let mut seen: Vec<bool> = vec![false; stretches.len()];
            for ss in 0..stretches.len() {
                if !at(&stretches, ss)?.floor || *at(&seen, ss)? {
                    continue;
                }
                let mut chain = Vec::new();
                let mut closed = false;
                let mut tt = Some(ss);
                while let Some(t) = tt {
                    if !at(&stretches, t)?.floor {
                        break;
                    }
                    if *at(&seen, t)? {
                        closed = t == ss;
                        break;
                    }
                    *at_mut(&mut seen, t)? = true;
                    chain.push(t);
                    tt = *at(&next_floor, t)?;
                }
                if closed {
                    floor_loops.push(chain);
                } else {
                    for &t in &chain {
                        at_mut(&mut stretches, t)?.floor = false;
                    }
                }
            }
        }

        // Close the kept stretches into loops: each goes on to the one kept stretch that
        // starts where it ends (at a crossing, the stretch of the other loop that bounds
        // the face).
        let mut next: Vec<Option<usize>> = vec![None; stretches.len()];
        let mut prev_count: Vec<usize> = vec![0; stretches.len()];
        for (ss, stretch) in stretches.iter().enumerate() {
            if !stretch.keep {
                continue;
            }
            for (tt, other) in stretches.iter().enumerate() {
                if !other.keep
                    || other.start != stretch.end
                    || stretch.follows.is_some_and(|f| f != tt)
                {
                    continue;
                }
                if at(&next, ss)?.is_some() {
                    return Err(unresolved(LoopCrossing::LeaveMoreThanOneWay));
                }
                *at_mut(&mut next, ss)? = Some(tt);
                *at_mut(&mut prev_count, tt)? += 1;
            }
            if at(&next, ss)?.is_none() {
                return Err(unresolved(LoopCrossing::LeaveNoWay));
            }
        }
        let mut new_loops: Vec<Vec<usize>> = Vec::new();
        let mut new_loop_is_outer: Vec<bool> = Vec::new();
        let mut done: Vec<bool> = vec![false; stretches.len()];
        for ss in 0..stretches.len() {
            if !at(&stretches, ss)?.keep || *at(&done, ss)? {
                continue;
            }
            let mut chain = Vec::new();
            let mut outer = false;
            let mut tt = ss;
            while !*at(&done, tt)? {
                if *at(&prev_count, tt)? != 1 {
                    return Err(unresolved(LoopCrossing::ArriveMoreThanOneWay));
                }
                *at_mut(&mut done, tt)? = true;
                chain.push(tt);
                if at(&stretches, tt)?.loop_index == 0 {
                    outer = true;
                }
                tt = present(
                    *at(&next, tt)?,
                    "a kept stretch has no next stretch, though each kept stretch was given \
                     one or refused as leaving no way",
                )?;
            }
            new_loops.push(chain);
            new_loop_is_outer.push(outer);
        }

        // Outer loops first (MakeFaceWithCurves takes the first loop for the outer one).
        let outer_loops: Vec<usize> = (0..new_loops.len())
            .filter(|&ll| new_loop_is_outer.get(ll).copied().unwrap_or(false))
            .collect();
        let hole_loops: Vec<usize> = (0..new_loops.len())
            .filter(|&ll| !new_loop_is_outer.get(ll).copied().unwrap_or(false))
            .collect();
        // (deviation: SmBrep::MakeFaceFromLoopOnFace is not ported, so a face that
        // comes apart is made one face per outer loop, and only without holes)
        if outer_loops.len() > 1 && !hole_loops.is_empty() {
            return Err(unresolved(LoopCrossing::ComesApartAroundHole));
        }
        let face_loops: Vec<Vec<usize>> = if outer_loops.len() > 1 {
            outer_loops.iter().map(|&ll| vec![ll]).collect()
        } else if outer_loops.is_empty() {
            Vec::new()
        } else {
            vec![outer_loops.iter().chain(&hole_loops).copied().collect()]
        };

        // The face(s) the old one becomes; none when no stretch of its outer loop bounds
        // it.
        let uv_domain = b.face(f)?.uv_domain;
        let mut new_faces: Vec<FaceId> = Vec::new();
        let mut edge_sources: BTreeMap<EdgeId, EdgeuseId> = BTreeMap::new();
        for order in &face_loops {
            let mut input = FaceCurvesInput {
                loop_points: if new_faces.is_empty() {
                    loop_points.clone()
                } else {
                    Vec::new()
                },
                ..FaceCurvesInput::default()
            };
            let mut curves_3d = Vec::new();
            let mut uv_curves = Vec::new();
            let mut curve_sources = Vec::new();
            for &ll in order {
                let chain = at(&new_loops, ll)?;
                input.loops.push(chain.len());
                for &ss in chain {
                    let stretch = at(&stretches, ss)?;
                    let e = edge_of(b, stretch.edgeuse)?;
                    let mut curve_3d = edge_curve(b, e)?.copy(kernel)?;
                    let mut uv_curve = at(&uvs, stretch.uv)?.copy(kernel)?;
                    let interval = common_trim_interval(
                        kernel,
                        &[&curve_3d, &uv_curve],
                        Extent1d::from_min_max(stretch.lo, stretch.hi),
                        OP,
                    )?;
                    trim_unsnapped(kernel, &mut curve_3d, interval, OP)?;
                    trim_unsnapped(kernel, &mut uv_curve, interval, OP)?;
                    curves_3d.push(Some(curve_3d));
                    curve_sources.push(stretch.edgeuse);
                    uv_curves.push(uv_curve);
                    input
                        .orientations
                        .push(eu::orientation(b, stretch.edgeuse)?);
                }
            }
            input.curves_3d = Some(curves_3d);
            input.uv_curves = Some(uv_curves);
            let new_surface =
                present(b.face(f)?.surface.as_ref(), FACE_NO_SURFACE)?.copy(kernel)?;
            let region = b.infinite_region();
            let key_of = notifications.result_key_of;
            let mut stores = NotificationStores {
                records: &mut *notifications.records,
                attributes: &mut *notifications.attributes,
                key_of,
            };
            let mut owners = Vec::new();
            let made = NotifyState::from_kernel_stores(&mut stores).with_kernel_env(
                kernel,
                |kernel, env| {
                    make_face_with_curve_owners(
                        kernel,
                        b,
                        region,
                        input,
                        new_surface,
                        &uv_domain,
                        OrientType::Same,
                        Some(true),
                        &|face| key_of(EntityRef::Face(face)),
                        &mut owners,
                        env,
                    )
                },
            )?;
            if owners.len() != curve_sources.len() {
                return Err(unresolved(LoopCrossing::StretchUnclassified));
            }
            for (owner, source) in owners.into_iter().zip(curve_sources) {
                // Discarded degenerate slots own no edge. Every retained edge
                // is still required to have a source below.
                let Some(owner) = owner else { continue };
                if let Some(previous) = edge_sources.insert(owner, source) {
                    if edge_of(b, previous)? != edge_of(b, source)? {
                        return Err(unresolved(LoopCrossing::StretchUnclassified));
                    }
                }
            }
            new_faces.push(present(made.face, NO_NEW_FACE)?);
        }

        // Fork 1b016ca: the regions the loops wind round the other way that are cavity,
        // each a face on the surface turned over (the loop runs round the region the
        // other way).
        let mut walled: Vec<Vec<usize>> = Vec::new();
        for chain in &floor_loops {
            let mut input = FaceCurvesInput::default();
            input.loops.push(chain.len());
            let mut curves_3d = Vec::new();
            let mut uv_curves = Vec::new();
            let mut curve_sources = Vec::new();
            for &ss in chain {
                let stretch = at(&stretches, ss)?;
                let e = edge_of(b, stretch.edgeuse)?;
                let mut curve_3d = edge_curve(b, e)?.copy(kernel)?;
                let mut uv_curve = at(&uvs, stretch.uv)?.copy(kernel)?;
                let interval = common_trim_interval(
                    kernel,
                    &[&curve_3d, &uv_curve],
                    Extent1d::from_min_max(stretch.lo, stretch.hi),
                    OP,
                )?;
                trim_unsnapped(kernel, &mut curve_3d, interval, OP)?;
                trim_unsnapped(kernel, &mut uv_curve, interval, OP)?;
                curves_3d.push(Some(curve_3d));
                curve_sources.push(stretch.edgeuse);
                uv_curves.push(uv_curve);
                input
                    .orientations
                    .push(eu::orientation(b, stretch.edgeuse)?);
            }
            // (not in the fork, solidcore#3329) of an offset that opens faces (a shell), only a
            // region under an opening can be the cavity's floor; then a point inside the
            // region, just off it on the side the cavity would lie on. An offset that opens
            // nothing (a draft's local operation, solidcraft#18374) has no opening to ask
            // about: the point alone says.
            let opens = !self.shell_faces().is_empty();
            if opens && !self.under_an_opening(original, b, &stretches, chain)? {
                // the region lies within a wall: the walls across it are carried on past it so
                // the merge cuts them where they meet
                walled.push(chain.clone());
                continue;
            }
            if !self.floor_is_cavity(kernel, original, b, f, &stretches, chain)? {
                continue;
            }
            input.curves_3d = Some(curves_3d);
            input.uv_curves = Some(uv_curves);
            let new_surface =
                present(b.face(f)?.surface.as_ref(), FACE_NO_SURFACE)?.copy(kernel)?;
            let region = b.infinite_region();
            let key_of = notifications.result_key_of;
            let mut stores = NotificationStores {
                records: &mut *notifications.records,
                attributes: &mut *notifications.attributes,
                key_of,
            };
            let mut owners = Vec::new();
            let made = NotifyState::from_kernel_stores(&mut stores).with_kernel_env(
                kernel,
                |kernel, env| {
                    make_face_with_curve_owners(
                        kernel,
                        b,
                        region,
                        input,
                        new_surface,
                        &uv_domain,
                        OrientType::Opposite,
                        Some(true),
                        &|face| key_of(EntityRef::Face(face)),
                        &mut owners,
                        env,
                    )
                },
            )?;
            if owners.len() != curve_sources.len() {
                return Err(unresolved(LoopCrossing::StretchUnclassified));
            }
            for (owner, source) in owners.into_iter().zip(curve_sources) {
                // Discarded degenerate slots own no edge. Every retained edge
                // is still required to have a source below.
                let Some(owner) = owner else { continue };
                if let Some(previous) = edge_sources.insert(owner, source) {
                    if edge_of(b, previous)? != edge_of(b, source)? {
                        return Err(unresolved(LoopCrossing::StretchUnclassified));
                    }
                }
            }
            new_faces.push(present(made.face, NO_NEW_FACE)?);
            // its edges come from them
            for &ss in chain {
                at_mut(&mut stretches, ss)?.keep = true;
            }
        }

        for chain in &walled {
            self.extend_walls_across(kernel, original, b, f, &stretches, chain, notifications)?;
        }

        // The new faces, edges and vertices take the attributes of the face, edges and
        // vertices they come from, and the old edge uses' places in the edge use map go
        // to the new ones.
        let key_of = notifications.result_key_of;
        for &new_face in &new_faces {
            let mut stores = NotificationStores {
                records: &mut *notifications.records,
                attributes: &mut *notifications.attributes,
                key_of,
            };
            NotifyState::from_kernel_stores(&mut stores).with_kernel_env(kernel, |_, env| {
                topology::notify(
                    env.records,
                    env.attributes,
                    env.context.as_deref_mut(),
                    NotifyOperation::Copy,
                    key_of(EntityRef::Face(f)),
                    Some(key_of(EntityRef::Face(new_face))),
                    None,
                    None,
                )
            })?;
            for new_edge in face_edges(b, new_face)? {
                // The builder reports the edge that owns each input curve slot;
                // provenance is that slot's source, not a nearest midpoint.
                let old_eu = *present(
                    edge_sources.get(&new_edge),
                    "an edge of a rebuilt face has no source edge use, though the face builder \
                     reports the slot that owns every edge it makes",
                )?;
                let old_edge = edge_of(b, old_eu)?;
                let new_vertices = [
                    edge::start_vertex(b, new_edge)?,
                    edge::end_vertex(b, new_edge)?,
                ];
                let old_vertices = [
                    edge::start_vertex(b, old_edge)?,
                    edge::end_vertex(b, old_edge)?,
                ];
                let mut pairs = vec![(EntityRef::Edge(old_edge), EntityRef::Edge(new_edge))];
                for &new_v in &new_vertices {
                    for &old_v in &old_vertices {
                        let new_v = present(new_v, EDGE_NO_VERTEX)?;
                        let old_v = present(old_v, EDGE_NO_VERTEX)?;
                        if b.vertex(new_v)?.point.distance(b.vertex(old_v)?.point) <= tol_3d {
                            pairs.push((EntityRef::Vertex(old_v), EntityRef::Vertex(new_v)));
                        }
                    }
                }
                let mut stores = NotificationStores {
                    records: &mut *notifications.records,
                    attributes: &mut *notifications.attributes,
                    key_of,
                };
                NotifyState::from_kernel_stores(&mut stores).with_kernel_env(
                    kernel,
                    |_, env| {
                        for &(old, new) in &pairs {
                            topology::notify(
                                env.records,
                                env.attributes,
                                env.context.as_deref_mut(),
                                NotifyOperation::Copy,
                                key_of(old),
                                Some(key_of(new)),
                                None,
                                None,
                            )?;
                        }
                        Ok(())
                    },
                )?;
                let new_eus = b.edge(new_edge)?.edgeuses.clone();
                for old in [Some(old_eu), eu::mate(b, old_eu)?] {
                    let old = present(old, EU_NO_MATE)?;
                    let old_orientation =
                        eu::faceuse_orientation(b, present(eu::faceuse(b, old)?, EU_NO_FU)?)?;
                    let originals = self.edgeuse_to_edgeuses.firsts(old);
                    for &new_eu in &new_eus {
                        let new_fu = present(eu::faceuse(b, new_eu)?, EU_NO_FU)?;
                        if eu::faceuse_orientation(b, new_fu)? != old_orientation {
                            continue;
                        }
                        for &original in &originals {
                            self.edgeuse_to_edgeuses.relate_pair(original, new_eu)?;
                        }
                    }
                }
            }
        }

        // The new faces take the old one's places in the face maps; the old face, its
        // edge uses and their pairs go.
        for original in self.faceuse_to_faces.firsts(f) {
            self.faceuse_to_faces.remove_pair(original, f)?;
            for &new_face in &new_faces {
                self.faceuse_to_faces.relate_pair(original, new_face)?;
            }
        }
        for original in self.edge_to_faces.firsts(f) {
            self.edge_to_faces.remove_pair(original, f)?;
            for &new_face in &new_faces {
                self.edge_to_faces.relate_pair(original, new_face)?;
            }
        }
        for original in self.vertex_to_faces.firsts(f) {
            self.vertex_to_faces.remove_pair(original, f)?;
            for &new_face in &new_faces {
                self.vertex_to_faces.relate_pair(original, new_face)?;
            }
        }
        for blended in self.blend_to_faces.seconds(f) {
            self.blend_to_faces.remove_pair(f, blended)?;
            for &new_face in &new_faces {
                self.blend_to_faces.relate_pair(new_face, blended)?;
            }
        }
        for blend in self.blend_to_faces.firsts(f) {
            self.blend_to_faces.remove_pair(blend, f)?;
            for &new_face in &new_faces {
                self.blend_to_faces.relate_pair(blend, new_face)?;
            }
        }
        for &edgeuse in &edgeuses {
            for old in [Some(edgeuse), eu::mate(b, edgeuse)?] {
                let old = present(old, EU_NO_MATE)?;
                for original in self.edgeuse_to_edgeuses.firsts(old) {
                    self.edgeuse_to_edgeuses.remove_pair(original, old)?;
                }
            }
        }
        let body = notifications.result_brep;
        let mut stores = NotificationStores {
            records: &mut *notifications.records,
            attributes: &mut *notifications.attributes,
            key_of,
        };
        NotifyState::from_kernel_stores(&mut stores).with_kernel_env(kernel, |kernel, env| {
            delete_face_with_body_identity(
                kernel,
                b,
                env,
                f,
                DeleteFaceOptions {
                    delete_edges_vertices: true,
                    do_region_nesting: true,
                    region_to_keep: None,
                },
                body,
            )
        })?;
        Ok(())
    }

    /// (not in the fork, solidcraft#18374) The edges of the other offset faces across the
    /// edge of offset edge use `edgeuse` of face `f`: the offsets, on the faces beside it,
    /// of the original edge it comes from (the walls that edge bounds).
    fn walls_across(
        &self,
        original: &Brep,
        b: &Brep,
        f: FaceId,
        edgeuse: EdgeuseId,
    ) -> Result<Vec<EdgeId>> {
        let mut walls = Vec::new();
        for offset_use in [Some(edgeuse), eu::mate(b, edgeuse)?].into_iter().flatten() {
            for source in self.edgeuse_to_edgeuses.firsts(offset_use) {
                let Some(e) = eu::edge(original, source)? else {
                    continue;
                };
                for across in edge::edgeuses(original, e, None, None)? {
                    for wall_use in self.edgeuse_to_edgeuses.seconds(across) {
                        let wall = edge_of(b, wall_use)?;
                        if eu::face(b, wall_use)? != Some(f) && !walls.contains(&wall) {
                            walls.push(wall);
                        }
                    }
                }
            }
        }
        Ok(walls)
    }

    /// (not in the fork, solidcraft#18374) Whether at every crossing of `crossings` the
    /// walls across the two edge uses it cuts leave offset face `f` on opposite sides of
    /// its surface: each wall's binormal at the crossing (into the wall, square to its
    /// edge) points along the face's normal there for one edge use's walls and against it
    /// for the other's. Two such walls meet only at the crossing, on their edges, so the
    /// stitch closes the body there once their edges are split; walls on one side pass
    /// through each other, which only the merge cuts. An edge use with no offset face across
    /// it (a sheet's open edge) does not tell: a thickened sheet's wall is built along it
    /// later, so it is not proved apart.
    ///
    /// # Errors
    ///
    /// Those of the closest points, binormals and normals.
    fn walls_part_at_crossings(
        &self,
        kernel: &Kernel,
        original: &Brep,
        b: &mut Brep,
        f: FaceId,
        crossings: &[Crossing],
        tol_3d: f64,
    ) -> Result<bool> {
        for crossing in crossings {
            let normal = present(b.face(f)?.surface.as_ref(), FACE_NO_SURFACE)?.evaluate_normal(
                kernel,
                crossing.uv,
                Side::Left,
                Side::Left,
            )?;
            let mut sides = [0.0_f64; 2];
            for (side, &edgeuse) in sides.iter_mut().zip(&crossing.edgeuses) {
                let walls = self.walls_across(original, b, f, edgeuse)?;
                if walls.is_empty() {
                    return Ok(false);
                }
                for wall in walls {
                    let Some(cp) = closest_point(kernel, b, wall, crossing.point)? else {
                        return Ok(false);
                    };
                    if !(cp.distance.is_finite() && cp.distance <= tol_3d) {
                        return Ok(false);
                    }
                    let wall_use = present(b.edge(wall)?.edgeuses.first().copied(), EDGE_NO_EU)?;
                    let into = eu3::evaluate_binormal(
                        kernel,
                        b,
                        wall_use,
                        cp.parameter,
                        false,
                        &eu3::BinormalOptions::default(),
                    )?
                    .binormal
                    .dot(normal);
                    // every wall across one edge use leaves the face the same way
                    if !(into.is_finite() && into != 0.0 && *side * into >= 0.0) {
                        return Ok(false);
                    }
                    *side = into;
                }
            }
            if !(sides[0].is_finite() && sides[1].is_finite() && sides[0] * sides[1] < 0.0) {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// (not in the fork, solidcraft#18374) Splits each wall across the edge uses a
    /// crossing of offset face `f` cuts (`walls_across`) at the crossing, where it does not
    /// end there already, so that the stretches either side of the crossing each meet an
    /// edge of their own.
    ///
    /// # Errors
    ///
    /// [`LoopCrossing::Unmerged`] where a wall does not pass the crossing; those of the
    /// closest points and the splits.
    fn split_walls_at_crossings(
        &mut self,
        kernel: &mut Kernel,
        original: &Brep,
        b: &mut Brep,
        f: FaceId,
        crossings: &[Crossing],
        tol_3d: f64,
        notifications: &mut OffsetNotifications<'_>,
    ) -> Result<()> {
        for crossing in crossings {
            for &edgeuse in &crossing.edgeuses {
                // the faces of the walls across it; after a split the crossing may lie on
                // either part, so each face's edges are searched
                let mut wall_faces: Vec<FaceId> = Vec::new();
                for wall in self.walls_across(original, b, f, edgeuse)? {
                    for wf in edge::faces(b, wall, None)? {
                        if !wall_faces.contains(&wf) {
                            wall_faces.push(wf);
                        }
                    }
                }
                for wf in wall_faces {
                    let mut through = None;
                    for e in face_edges(b, wf)? {
                        let Some(cp) = closest_point(kernel, b, e, crossing.point)? else {
                            continue;
                        };
                        if !(cp.distance.is_finite() && cp.distance <= tol_3d) {
                            continue;
                        }
                        let ends = [edge::start_vertex(b, e)?, edge::end_vertex(b, e)?];
                        if ends.iter().any(|&v| {
                            vertex_point(b, v).is_ok_and(|p| p.distance(crossing.point) <= tol_3d)
                        }) {
                            // the wall ends at the crossing already
                            through = None;
                            break;
                        }
                        through = Some((e, cp.parameter));
                    }
                    let Some((e, t)) = through else {
                        continue;
                    };
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
                        t,
                        &mut state,
                        notifications.result_curve_ref,
                    )?;
                    if !output.split {
                        return Err(unresolved(LoopCrossing::Unmerged));
                    }
                }
            }
        }
        Ok(())
    }

    /// `sm_OutwardOfBody` (fork 1b016ca): the sign (`+1` or `-1`) that turns
    /// `surface_normal`, the normal of offset face `offset_face` at `at`, to the body's
    /// outward normal at the face it was offset or copied from (the normal of that face's
    /// face use the offset was built from, which points into the region it bounds: out
    /// of the body where that is void); `None` (unknown) for a face with no face of the
    /// body behind it (caps, walls built along edges), or where the body's face is not
    /// found within twice the wall or has no normal.
    fn outward_of_body(
        &self,
        kernel: &Kernel,
        original: &mut Brep,
        offset_face: FaceId,
        at: Point3,
        surface_normal: Vec3,
        wall: f64,
    ) -> Result<Option<f64>> {
        let Some(&fu) = self.faceuse_to_faces.firsts(offset_face).first() else {
            return Ok(None);
        };
        let orig = present(
            original.faceuse(fu)?.face,
            "a face use of the body has no face, though the kernel links every face use to \
             its face",
        )?;
        let Some(shell) = original.faceuse(fu)?.shell else {
            return Ok(None);
        };
        let Some(region) = original.shell(shell)?.region else {
            return Ok(None);
        };
        // the body as the caller gave it
        let Some(surface) = face::surface(original, orig)? else {
            return Err(SolidError::offset(
                OP,
                OffsetFailure::FaceHasNoSurface { face: orig },
            ));
        };
        let surface = surface.clone_for_evaluation()?;
        let domain = surface.natural_uv_domain_with_nurb(kernel)?;
        let solutions = live_owned_surface_global_point_solve(
            kernel,
            &surface,
            original,
            orig,
            &domain,
            SolverOperation::Minimize,
            at,
            2.0 * wall,
            SolutionRequested::Single,
        )?;
        let Some(first) = solutions.first() else {
            return Ok(None);
        };
        let uv = Point2::new(first.param(0)?, first.param(1)?);
        let Ok(mut normal) = surface
            .evaluate_normal(kernel, uv, Side::Left, Side::Left)?
            .unitize()
        else {
            return Ok(None);
        };
        // the face use's normal points into the region it bounds: out of the body where
        // that is void
        if eu::faceuse_orientation(original, fu)? != OrientType::Same {
            normal *= -1.0;
        }
        if original.region(region)?.is_void() == 0 {
            normal *= -1.0;
        }
        Ok(Some(if normal.dot(surface_normal) >= 0.0 {
            1.0
        } else {
            -1.0
        }))
    }

    /// (not in the fork, solidcore#3329) Whether the region of an offset face that the
    /// stretches `chain` wind round the other way lies under an opening: one of its
    /// stretches is the offset of an edge of a face the shell opens. Such a stretch does
    /// not move with the wall across it (there is none: the opening), so the loop it is on
    /// can run past the other loop's moved side with the cavity reaching through between
    /// them (fork 1b016ca's floor). Where every stretch bounds an offset wall, the loops
    /// cross because those walls pass through each other (the merge cuts them where they
    /// do), and the region they wind round the other way lies within one of the walls: not
    /// cavity. [`Self::floor_is_cavity`]'s point just off such a region can miss the wall
    /// it lies within, whose face ends at the region's own level (a concave round's offset
    /// running past the moved outer wall below it, solidcore#3329), and the region was kept
    /// as a fold-back sliver of floor. Such a region is dropped and the walls across it
    /// carried on until they meet ([`Self::extend_walls_across`]).
    ///
    /// # Errors
    ///
    /// Those of the topology queries.
    fn under_an_opening(
        &self,
        original: &Brep,
        b: &Brep,
        stretches: &[LoopStretch],
        chain: &[usize],
    ) -> Result<bool> {
        let open = self.shell_faces();
        for &ss in chain {
            let edgeuse = at(stretches, ss)?.edgeuse;
            for offset_use in [Some(edgeuse), eu::mate(b, edgeuse)?].into_iter().flatten() {
                for source in self.edgeuse_to_edgeuses.firsts(offset_use) {
                    let Some(e) = eu::edge(original, source)? else {
                        continue;
                    };
                    for across in edge::edgeuses(original, e, None, None)? {
                        if eu::face(original, across)?.is_some_and(|g| open.contains(&g)) {
                            return Ok(true);
                        }
                    }
                }
            }
        }
        Ok(false)
    }

    /// (not in the fork, solidcore#3329) Carries each wall across a region of offset face
    /// `f` that its loops wind round the other way and that lies within a wall
    /// ([`Self::under_an_opening`] false) on past the region: on a copy of the wall's
    /// surface, a strip beyond the stretch of the wall's edge that bounds the region, as
    /// wide as the offset's wall. Where every stretch bounds an offset wall, the walls
    /// end on the region's edges and do not meet; carried on, they pass through each
    /// other above the region and the merge cuts them there (the strip is related to its
    /// wall, which it continues, so the two are not intersected). What lies past the cut
    /// is within the wall of the original and goes with the excess faces.
    ///
    /// # Errors
    ///
    /// Those of the evaluations, point solves and face construction.
    #[allow(clippy::too_many_arguments)] // the crossing routine's state
    fn extend_walls_across(
        &mut self,
        kernel: &mut Kernel,
        original: &Brep,
        b: &mut Brep,
        f: FaceId,
        stretches: &[LoopStretch],
        chain: &[usize],
        notifications: &mut OffsetNotifications<'_>,
    ) -> Result<()> {
        let reach = self.offset_distance().abs();
        let tol_3d = self.approximation_tol_3d().max(b.tolerance()) * 10.0;
        for &ss in chain {
            let stretch = at(stretches, ss)?;
            let (from, to) = {
                let curve = edge_curve(b, edge_of(b, stretch.edgeuse)?)?;
                (
                    curve.evaluate_point(kernel, stretch.lo)?,
                    curve.evaluate_point(kernel, stretch.hi)?,
                )
            };
            for wall in self.walls_across(original, b, f, stretch.edgeuse)? {
                let Some((lo, hi)) =
                    stretch_on_wall(kernel, b, wall, from, stretch.mid, to, tol_3d)?
                else {
                    continue;
                };
                for wf in edge::faces(b, wall, None)? {
                    // a strip that cannot be made leaves the walls apart, which the
                    // shell's gate refuses; a defect is passed on
                    let strip = strip_result(self.wall_strip(
                        kernel,
                        b,
                        wf,
                        wall,
                        lo,
                        hi,
                        reach,
                        notifications,
                    ))?
                    .flatten();
                    if let Some(strip) = strip {
                        for original_use in self.faceuse_to_faces.firsts(wf) {
                            self.faceuse_to_faces.relate_pair(original_use, strip)?;
                        }
                        self.wall_extensions.push((wf, strip));
                    }
                }
            }
        }
        Ok(())
    }

    /// (not in the fork, solidcore#3329) The strip that carries wall face `wf` on past
    /// `[lo, hi]` of its edge `wall`, `reach` wide, on a copy of its surface: in UV, the
    /// edge use's trim curve over `[lo, hi]`, the same moved off the face by the UV step
    /// that measures `reach` on the surface at the middle, and the two lines between their
    /// ends. `None` where the face has no edge use of `wall` or the step is not found.
    #[allow(clippy::too_many_arguments)] // the crossing routine's state
    fn wall_strip(
        &mut self,
        kernel: &mut Kernel,
        b: &mut Brep,
        wf: FaceId,
        wall: EdgeId,
        lo: f64,
        hi: f64,
        reach: f64,
        notifications: &mut OffsetNotifications<'_>,
    ) -> Result<Option<FaceId>> {
        let mut fu = present(face::upward_faceuse(b, wf)?, FACE_NO_FACEUSE)?;
        if eu::faceuse_orientation(b, fu)? != OrientType::Same {
            fu = present(eu::faceuse_mate(b, fu)?, FU_NO_MATE)?;
        }
        let Some(edgeuse) = b
            .edge(wall)?
            .edgeuses
            .iter()
            .copied()
            .find(|&u| eu::faceuse(b, u).ok().flatten() == Some(fu))
        else {
            return Ok(None);
        };
        eu3::get_or_create_uv_trim_curve(kernel, b, edgeuse, false, false, true)?.status?;
        let uv = present(eu::uv_trim_curve(b, edgeuse)?.0.cloned(), EU_NO_UV)?;
        let same = eu::orientation(b, edgeuse)? == OrientType::Same;
        // the wall's surface carried on past its sides, its parameters kept (a bounded
        // surface can end where the wall does)
        let surface = super::reach::extended_by_margin(
            kernel,
            present(b.face(wf)?.surface.as_ref(), FACE_NO_SURFACE)?,
            2.0 * reach,
        )?;
        let uv_at = |t: f64| -> Result<Point2> {
            let p = uv.evaluate_point(kernel, t)?;
            Ok(Point2::new(p.x, p.y))
        };
        // the edge use runs from `a` to `z` with the face on its left; the strip lies on
        // its right
        let (a, z) = if same { (lo, hi) } else { (hi, lo) };
        let mid = 0.5 * (lo + hi);
        let dt = 1.0e-3 * (hi - lo);
        let m = uv_at(mid)?;
        let ahead = uv_at(if same { mid + dt } else { mid - dt })?;
        let behind = uv_at(if same { mid - dt } else { mid + dt })?;
        let (du, dv) = (ahead.x - behind.x, ahead.y - behind.y);
        let len = (du * du + dv * dv).sqrt();
        if !(len.is_finite() && len > 0.0) {
            return Ok(None);
        }
        // right of the direction of travel
        let (nu, nv) = (dv / len, -du / len);
        let p = surface.evaluate_point(kernel, m)?;
        let h = 1.0e-6 * (1.0 + m.x.abs().max(m.y.abs()));
        let q = surface.evaluate_point(kernel, Point2::new(m.x + h * nu, m.y + h * nv))?;
        let per_uv = p.distance(q) / h;
        if !(per_uv.is_finite() && per_uv > 0.0) {
            return Ok(None);
        }
        let step = reach / per_uv;
        let (su, sv) = (nu * step, nv * step);
        let start = uv_at(a)?;
        let end = uv_at(z)?;
        let lift = |p: Point2| Point3::new(p.x + su, p.y + sv, 0.0);
        let flat = |p: Point2| Point3::new(p.x, p.y, 0.0);

        let mut along = uv.copy(kernel)?;
        along.trim(
            kernel,
            &mut Extent1d::from_min_max(lo, hi),
            TrimMode::Notify,
        )?;
        let mut beyond = along.copy(kernel)?;
        beyond.transform(
            kernel,
            &crate::geom::axis2_placement::Axis2Placement::new(
                Point3::new(su, sv, 0.0),
                Vec3::new(1.0, 0.0, 0.0),
                Vec3::new(0.0, 1.0, 0.0),
            ),
            None,
        )?;
        let mut along_3d = edge_curve(b, wall)?.copy(kernel)?;
        along_3d.trim(
            kernel,
            &mut Extent1d::from_min_max(lo, hi),
            TrimMode::Notify,
        )?;
        let mut rail_out = Curve::from(BSplineCurve::create_line_segment(
            2,
            flat(start),
            lift(start),
        )?);
        let mut rail_back =
            Curve::from(BSplineCurve::create_line_segment(2, lift(end), flat(end))?);
        let (back, on) = if same {
            (OrientType::Opposite, OrientType::Same)
        } else {
            (OrientType::Same, OrientType::Opposite)
        };
        let mut uv_domain = b.face(wf)?.uv_domain;
        for p in [start, end] {
            uv_domain.add_point(Point2::new(p.x + su, p.y + sv));
        }
        // the new sides on the surface (the stretch keeps its edge's curve)
        let tol = self.approximation_tol_3d();
        let mut lifted = Vec::new();
        for c in [&mut rail_out, &mut beyond, &mut rail_back] {
            c.convert_to_2d(kernel)?;
            let ivl = c.natural_interval();
            let on = surface
                .lift_curve(
                    kernel,
                    &uv_domain,
                    &c.clone_for_evaluation()?,
                    &ivl,
                    tol,
                    false,
                )?
                .curve;
            lifted.push(Some(on));
        }
        let mut curves_3d = vec![Some(along_3d)];
        curves_3d.extend(lifted);
        // the edge's stretch run back (z to a), out at a, beyond (a to z), back at z
        let input = FaceCurvesInput {
            loops: vec![4],
            curves_3d: Some(curves_3d),
            uv_curves: Some(vec![along, rail_out, beyond, rail_back]),
            orientations: vec![back, OrientType::Same, on, OrientType::Same],
            loop_points: Vec::new(),
        };
        let region = b.infinite_region();
        let key_of = notifications.result_key_of;
        let mut stores = NotificationStores {
            records: &mut *notifications.records,
            attributes: &mut *notifications.attributes,
            key_of,
        };
        let made = make_face_with_curves_with_kernel_context(
            kernel,
            b,
            region,
            input,
            surface,
            &uv_domain,
            OrientType::Same,
            None,
            &|face| key_of(EntityRef::Face(face)),
            &mut stores,
        )?;
        Ok(made.face)
    }

    /// `sm_FloorIsCavity` (fork 1b016ca): whether the region of offset face `f` that the
    /// stretches `chain` wind round the other way is the cavity's face, reversed. The
    /// cavity would lie on the side of the region the face's own cavity side is not, so
    /// the point just beyond the region on that side must be cavity by the offset's own
    /// rule: inside the body, and on the cavity side of every other offset face whose
    /// wall it lies within (its foot on that face is inside the face, less than the wall
    /// from it). A face's cavity side is opposite the body's outward normal at the face
    /// it was offset from ([`Self::outward_of_body`]).
    fn floor_is_cavity(
        &self,
        kernel: &Kernel,
        original: &mut Brep,
        b: &mut Brep,
        f: FaceId,
        stretches: &[LoopStretch],
        chain: &[usize],
    ) -> Result<bool> {
        let wall = self.offset_distance();
        let surface = present(face::surface(b, f)?, FACE_NO_SURFACE)?.clone_for_evaluation()?;
        // a point of the region: the middle of its stretches' middles, on the surface
        let mut mid = Point3::new(0.0, 0.0, 0.0);
        for &ss in chain {
            mid += at(stretches, ss)?.mid - Point3::new(0.0, 0.0, 0.0);
        }
        #[allow(clippy::cast_precision_loss)] // a loop's stretch count
        let count = chain.len() as f64;
        mid = Point3::new(0.0, 0.0, 0.0) + (mid - Point3::new(0.0, 0.0, 0.0)) * (1.0 / count);
        let domain = surface.natural_uv_domain_with_nurb(kernel)?;
        let solutions = live_owned_surface_global_point_solve(
            kernel,
            &surface,
            b,
            f,
            &domain,
            SolverOperation::Minimize,
            mid,
            wall,
            SolutionRequested::Single,
        )?;
        let Some(first) = solutions.first() else {
            return Ok(false);
        };
        let uv = Point2::new(first.param(0)?, first.param(1)?);
        let p = surface.evaluate_point(kernel, uv)?;
        let Ok(n) = surface
            .evaluate_normal(kernel, uv, Side::Left, Side::Left)?
            .unitize()
        else {
            return Ok(false);
        };
        let Some(sign) = self.outward_of_body(kernel, original, f, p, n, wall)? else {
            return Ok(false);
        };
        // the face's cavity side is against the body's outward normal; the region's the
        // other one
        let step = 1.0e-3 * wall;
        let q = p + n * (sign * step);
        let zone = brep_zone_tol_3d(kernel, original, SmallTopoType::NormalSize);
        let classified =
            BrepQuery::of_brep(original).point_3d_classify(kernel, original, q, zone, true)?;
        let in_body = classified.point_class() == PointClassType::Region
            && match classified.object() {
                Some(EntityRef::Region(r)) => original.region(r)?.is_void() == 0,
                _ => false,
            };
        if !in_body {
            return Ok(false);
        }
        for other in bp1::faces(b, None)? {
            if other == f {
                continue;
            }
            let (other_surface, other_domain) = {
                let record = b.face(other)?;
                let Some(os) = record.surface.as_ref() else {
                    continue;
                };
                (os.clone_for_evaluation()?, record.uv_domain)
            };
            let solutions = live_owned_surface_global_point_solve(
                kernel,
                &other_surface,
                b,
                other,
                &other_domain,
                SolverOperation::Minimize,
                q,
                wall,
                SolutionRequested::Single,
            )?;
            let Some(first) = solutions.first() else {
                continue;
            };
            if first.solution_value.partial_cmp(&wall) != Some(core::cmp::Ordering::Less) {
                continue;
            }
            let other_uv = Point2::new(first.param(0)?, first.param(1)?);
            // its foot inside the face
            let loops = face::loops(b, other, false, None)?;
            let mut inside = !loops.is_empty();
            let other_zone = zone_tol_3d(kernel, &other_surface)?;
            for (ll, &l) in loops.iter().enumerate() {
                if !inside {
                    break;
                }
                let found =
                    contains_uv_point(kernel, b, l, None, other_uv, other_zone)?.containment;
                let wanted = if ll == 0 {
                    PointContainment::Inside
                } else {
                    PointContainment::Outside
                };
                if found != wanted {
                    inside = false;
                }
            }
            if !inside {
                continue;
            }
            let on = other_surface.evaluate_point(kernel, other_uv)?;
            let Ok(other_normal) = other_surface
                .evaluate_normal(kernel, other_uv, Side::Left, Side::Left)?
                .unitize()
            else {
                continue;
            };
            let Some(other_sign) =
                self.outward_of_body(kernel, original, other, on, other_normal, wall)?
            else {
                continue;
            };
            // the cavity side is against the outward normal
            if (q - on).dot(other_normal * other_sign) >= 0.0 {
                return Ok(false);
            }
        }
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The length test still refuses what it is for: a stretch ending at a crossing, or
    /// running between two vertices, whose ends lie within the tolerance, and NaN. Only an
    /// uncut loop of one edge use (the same vertex node at both ends) passes over it.
    #[test]
    fn only_an_uncut_loop_of_one_edge_passes_the_length_test() {
        let tol = 0.01;
        // a loop of one closed edge use, uncut: its ends meet
        assert!(!stretch_too_short(-3, -3, 0.0, tol));
        // a real defect still caught: ends at crossings, a crossing and a vertex, two
        // vertices of an open edge, all within the tolerance
        assert!(stretch_too_short(0, 1, 0.005, tol));
        assert!(stretch_too_short(-3, 2, 0.01, tol));
        assert!(stretch_too_short(2, -3, 0.0, tol));
        assert!(stretch_too_short(-2, -3, 0.005, tol));
        assert!(stretch_too_short(0, 1, f64::NAN, tol));
        // NaN is refused before the whole-loop exemption
        assert!(stretch_too_short(-3, -3, f64::NAN, tol));
        assert!(stretch_too_short(-3, -3, 0.0, f64::NAN));
        // long enough
        assert!(!stretch_too_short(0, 1, 0.02, tol));
        assert!(!stretch_too_short(-2, -3, 0.02, tol));
    }

    /// The helpers' defects keep the status of the check each replaces and say their own
    /// reason: the trim interval mismatch `SM_ERR_ASSERT_FAILURE`, a missing link, a list
    /// index past the end and a missing tangent `SM_ERR_NULL_POINTER`; all are defects of
    /// this routine with no entity.
    #[test]
    fn the_helpers_defects_keep_the_status_they_replace() {
        use crate::error::ErrorKind;
        let short = [0_u8];
        let cases = [
            (
                trim_interval_mismatch(),
                SM_ERR_ASSERT_FAILURE,
                "an offset edge use's UV trim curve does not span its edge's interval, though \
                 the kernel keeps an edge use's trim curve over its edge's interval",
            ),
            (
                present(None::<u8>, FU_NO_MATE).unwrap_err(),
                SM_ERR_NULL_POINTER,
                FU_NO_MATE,
            ),
            (
                at(&short, 1).unwrap_err(),
                SM_ERR_NULL_POINTER,
                INDEX_PAST_END,
            ),
            (
                present(None::<u8>, EVAL_1ST).unwrap_err(),
                SM_ERR_NULL_POINTER,
                EVAL_1ST,
            ),
        ];
        for (error, status, why) in cases {
            assert_eq!(error.upstream_status(), Some(status), "{error:?}");
            assert_eq!(error.kind(), ErrorKind::StageFailed, "{error:?}");
            assert!(error.is_defect(), "{error:?}");
            assert_eq!(error.operation(), OP, "{error:?}");
            assert_eq!(error.entity(), None, "{error:?}");
            assert_eq!(
                error.offset_failure(),
                Some(&OffsetFailure::Defect { why }),
                "{error:?}"
            );
        }
        assert_eq!(at(&short, 0).ok(), Some(&0));
        assert_eq!(present(Some(7_u8), FU_NO_MATE).ok(), Some(7));
    }

    /// A wall strip's defect reaches the caller with its reason; a strip that cannot be made
    /// for any other reason is skipped, and a strip that is made is kept.
    #[test]
    fn a_wall_strip_defect_is_passed_on_and_other_failures_skip_the_strip() {
        let defect = strip_result::<u8>(present(None, FACE_NO_FACEUSE)).unwrap_err();
        assert!(defect.is_defect(), "{defect:?}");
        assert_eq!(
            defect.offset_failure(),
            Some(&OffsetFailure::Defect {
                why: FACE_NO_FACEUSE
            })
        );
        let failed = SolidError::invalid_input(OP, "no step found");
        assert!(!failed.is_defect());
        assert_eq!(strip_result::<u8>(Err(failed)).ok(), Some(None));
        assert_eq!(strip_result(Ok(7_u8)).ok(), Some(Some(7)));
    }
}
