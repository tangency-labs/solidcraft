//! Selection tools: nearest point / vertex within tolerance of a target, the edge
//! coincident with a curve between two vertices, and edge / face picking by point.
//!
//! Upstream: `source/SMLib/src/SmTools.cpp`, `source/SMLib/inc/SmTools.h`.
//!
//! | Rust                                   | Upstream                                  |
//! |----------------------------------------|-------------------------------------------|
//! | [`find_first_point_within_tol_to_pnt`] | `SmTools::FindFirstPointWithinTolToPnt`   |
//! | [`find_first_vertex_within_tol_to_pnt`]| `SmTools::FindFirstVertexWithinTolToPnt`  |
//! | [`find_coincident_edge`]               | `SmTools::FindCoincidentEdge`             |
//! | [`find_edge_proved_along`]             | none (own design)                         |
//! | [`find_edge`]                          | `SmTools::FindEdge`                       |
//! | [`find_face`]                          | `SmTools::FindFace`                       |
//! | [`PointMatch`]                         | the out-params of `FindFirstPointWithinTolToPnt` |
//! | [`VertexMatch`]                        | the out-params of `FindFirstVertexWithinTolToPnt` |
//! | [`CoincidentEdgeQuery`], [`CoincidentEdge`] | the in- and out-params of `FindCoincidentEdge` |
//! | [`EdgePick`]                           | the out-params of `FindEdge`              |
//!
//! Deviations:
//! - Out-params become return values. A target index outside the point array
//!   (upstream: out-of-bounds read) and an id that is not in the B-rep are
//!   [`SolidError::InvalidInput`] / [`SolidError::NotFound`].
//! - [`find_coincident_edge`] returns `None` when no edge is found (upstream sets
//!   `rbFound = FALSE` and `rdMaxDeviation = -SM_BIG_DOUBLE`, leaving
//!   `rbSameOrient` / `rlFoundIndex` unset or holding a rejected candidate's value).
//! - [`find_edge`] and [`find_face`] report "nothing found" (upstream `SM_ERR`) as
//!   `Ok(None)`. Faces are returned as [`FaceId`]s.
//! - **Edge and vertex queries** are those of `topology::edge` and `topology::vertex`.
//! - **Unported callees.** Exact B-rep cache dependencies of [`find_face`] return
//!   [`SolidError::Unported`] (see the `UNPORTED` markers).
//! - **Curve owner.** `pCurve->GetOwner()` of [`find_coincident_edge`]'s target is
//!   [`CoincidentEdgeQuery::curve_owner`]: a [`Curve`] names its owner by the caller's
//!   identity, which only the caller can map back to an edge.

use crate::curve::curve::Curve;
use crate::error::geom::GeomFailure;
use crate::error::{Result, SolidError, SM_ERR_NULL_POINTER};
use crate::geom::extent1d::Extent1d;
use crate::geom::global_solver::SolverOperation;
use crate::geom::object::ObjectRef;
use crate::geom::vector3d::sqrt;
use crate::intersect::curve_class::p1::SourceReach;
use crate::kernel::Kernel;
use crate::math::{Point3, Vec3, BIG_DOUBLE, EFF_ZERO};
use crate::nurbs::math::gamma;
use crate::surface::normal_cone::intersection_reach;
use crate::surface::surface::Surface;
use crate::topology::brep_cache::CacheObject;
use crate::topology::{Brep, BrepId, EdgeId, FaceId, VertexId};

/// `rlFoundIndex` of [`find_first_vertex_within_tol_to_pnt`] when no vertex is
/// within tolerance (upstream `99999`).
pub const NO_VERTEX_INDEX: usize = 99_999;

/// Sample count of the curve/edge distance check in [`find_coincident_edge`].
const COINCIDENCE_SAMPLES: u32 = 30;

/// Result of [`find_first_point_within_tol_to_pnt`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PointMatch {
    /// A point lies closer than the tolerance to the target (`rbIsFoundArg`).
    pub found: bool,
    /// Index of the point nearest the target (`rlFoundIndexArg`; `0` when the
    /// array holds only the target).
    pub index: usize,
    /// Distance from the target to the nearest point (`rdClosestDistanceArg`;
    /// `sqrt(SM_BIG_DOUBLE)` when there is no other point).
    pub closest_distance: f64,
    /// Distance from the target to the second nearest point
    /// (`rd2ndClosestDistanceArg`).
    pub second_closest_distance: f64,
}

/// Result of [`find_first_vertex_within_tol_to_pnt`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VertexMatch {
    /// A vertex lies within its tolerance plus the point tolerance (`rbIsFound`).
    pub found: bool,
    /// Index into the vertex list of the nearest qualifying vertex
    /// (`rlFoundIndex`; [`NO_VERTEX_INDEX`] when none qualifies).
    pub index: usize,
    /// Distance to the hit when the search stopped on an (almost) exact hit,
    /// otherwise the distance to the nearest vertex of the list
    /// (`rdClosestVertexDistance`; `SM_BIG_DOUBLE` for an empty list).
    pub distance: f64,
}

/// Target of [`find_coincident_edge`]: a curve between two known vertices.
#[derive(Debug, Clone, Copy)]
pub struct CoincidentEdgeQuery<'a> {
    /// Vertex at the curve start (`pV1`).
    pub start_vertex: VertexId,
    /// Vertex at the curve end (`pV2`).
    pub end_vertex: VertexId,
    /// The target curve (`pCurve`).
    pub curve: &'a Curve,
    /// The edge owning the target curve (`pCurve->GetOwner()` when it is an edge of the
    /// searched B-rep; `None` for a curve no searched edge owns). A [`Curve`] names its
    /// owner by the caller's identity, so the caller resolves it.
    pub curve_owner: Option<EdgeId>,
    /// Parameter range of the curve or its edge (`rCurveIvl`).
    pub interval: Extent1d,
    /// Tolerance of the curve and its end points (`d3dTolerance`).
    pub tolerance: f64,
}

/// A coincident edge found by [`find_coincident_edge`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CoincidentEdge {
    /// Index of the edge in the searched list (`rlFoundIndex`).
    pub index: usize,
    /// The curve runs the same way as the edge (`rbSameOrient`).
    pub same_orientation: bool,
    /// Largest sampled curve/edge distance (`rdMaxDeviation`).
    pub max_deviation: f64,
}

/// Result of [`find_edge`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EdgePick {
    /// Index of the nearest edge in `SmBrep::GetEdges` order (`rlEdgeIndex`).
    pub index: usize,
    /// Edge curve parameter of the closest point (`dParam`).
    pub parameter: f64,
}

/// Point drop result (`bGoodDrop`, drop parameter, drop distance of
/// `SmCurve::DropPoint`).
struct Drop {
    good: bool,
    parameter: f64,
    distance: f64,
}

/// Finds the point of `points` nearest to `points[target]` (`SmTools::FindFirstPointWithinTolToPnt`).
///
/// `found` is set when the nearest point lies closer than `tolerance`; a second
/// point at a similar distance (two loops touching at a vertex) is accepted too.
///
/// # Errors
/// [`SolidError::InvalidInput`] when `target` is not an index of `points`.
pub fn find_first_point_within_tol_to_pnt(
    points: &[Point3],
    target: usize,
    tolerance: f64,
) -> Result<PointMatch> {
    let pt = *points.get(target).ok_or(SolidError::InvalidInput {
        operation: "geom::tools::find_first_point_within_tol_to_pnt",
        reason: "target index outside the point array",
    })?;

    let mut closest = BIG_DOUBLE;
    let mut second = BIG_DOUBLE;
    let mut index = 0usize;

    for (i, &curr) in points.iter().enumerate() {
        if i == target {
            continue;
        }
        let vec: Vec3 = curr - pt;
        let len_sq = vec.length_squared();
        if closest > len_sq {
            second = closest;
            closest = len_sq;
            index = i;
        } else if second > len_sq {
            second = len_sq;
        }
    }

    let second = sqrt(second);
    let closest = sqrt(closest);

    // Upstream distinguishes a single hit (`closest < 0.5 * second`) from a
    // double hit, but accepts both (two loops may touch at a vertex).
    let found = closest < tolerance;

    Ok(PointMatch {
        found,
        index,
        closest_distance: closest,
        second_closest_distance: second,
    })
}

/// Finds the vertex of `vertices` nearest to `point` within the vertex's
/// tolerance plus `tolerance` (`SmTools::FindFirstVertexWithinTolToPnt`).
///
/// A vertex closer than `SM_EFF_ZERO` ends the search at once.
///
/// # Errors
/// [`SolidError::NotFound`] for a vertex id outside `brep`.
pub fn find_first_vertex_within_tol_to_pnt(
    brep: &Brep,
    vertices: &[VertexId],
    point: Point3,
    tolerance: f64,
) -> Result<VertexMatch> {
    let mut found = false;
    let mut index = NO_VERTEX_INDEX;

    let mut min_vertex_dist = BIG_DOUBLE;
    let mut min_qualifying_dist = BIG_DOUBLE;

    for (ii, &v) in vertices.iter().enumerate() {
        let tol = vertex_tolerance(brep, v)? + tolerance;
        let this_dist = point.distance(brep.vertex(v)?.point);

        if this_dist < EFF_ZERO {
            return Ok(VertexMatch {
                found: true,
                index: ii,
                distance: this_dist,
            });
        }

        if this_dist < tol {
            found = true;
            if this_dist < min_qualifying_dist {
                min_qualifying_dist = this_dist;
                index = ii;
            }
        }

        if this_dist < min_vertex_dist {
            min_vertex_dist = this_dist;
        }
    }

    Ok(VertexMatch {
        found,
        index,
        distance: min_vertex_dist,
    })
}

/// Searches `edges[start..]` for an edge between the query's vertices whose
/// geometry coincides with the query curve (`SmTools::FindCoincidentEdge`).
///
/// End points are matched by vertex identity; the edge's mid point is dropped
/// onto the curve and the curve/edge distance is sampled at 30 points. The edge
/// owning the curve is skipped. Returns the first coincident edge, or `None`.
///
/// # Errors
/// [`SolidError::Unported`] while edge geometry and curve evaluation are not
/// ported; errors of the edge / curve queries are propagated.
pub fn find_coincident_edge(
    k: &Kernel,
    brep: &Brep,
    query: &CoincidentEdgeQuery<'_>,
    edges: &[EdgeId],
    start: usize,
) -> Result<Option<CoincidentEdge>> {
    let curve_ivl = query.interval;
    let guess = curve_ivl.mid();
    let (v1, v2) = (query.start_vertex, query.end_vertex);

    for (ii, &e) in edges.iter().enumerate().skip(start) {
        if query.curve_owner == Some(e) {
            continue;
        }

        let matches = (edge_vertex(brep, e)? == Some(v1)
            && edge_other_vertex(brep, e, v1)? == Some(v2))
            || (edge_vertex(brep, e)? == Some(v2) && edge_other_vertex(brep, e, v2)? == Some(v1));
        if !matches {
            continue;
        }

        let tol = query.tolerance + edge_tolerance(brep, e)?;

        let edge_ivl = edge_interval(brep, e)?;
        let edge_mid_param = edge_ivl.evaluate(0.5);
        let mid_pt = edge_curve_point(k, brep, e, edge_mid_param)?;

        let drop = drop_point(k, query.curve, &curve_ivl, mid_pt, tol, guess)?;
        if !drop.good || drop.distance > tol {
            continue;
        }

        let edge_tangent = edge_curve_derivative(k, brep, e, edge_mid_param)?;
        let curve_tangent = curve_derivative(k, query.curve, drop.parameter)?;
        let same_orientation = curve_tangent.dot(edge_tangent) > 0.0;

        // the edge range in the curve's direction
        let other_range = if same_orientation {
            (edge_ivl.min(), edge_ivl.max())
        } else {
            (edge_ivl.max(), edge_ivl.min())
        };
        let dist = match curve_max_distance_between(
            k,
            query.curve,
            &curve_ivl,
            brep,
            e,
            other_range,
            COINCIDENCE_SAMPLES,
            tol,
        ) {
            Ok(d) => d,
            Err(
                err @ (SolidError::Unported { .. }
                | SolidError::Cancelled { .. }
                | SolidError::OutOfFuel { .. }),
            ) => return Err(err),
            Err(_) => continue,
        };
        if dist > tol {
            continue;
        }

        return Ok(Some(CoincidentEdge {
            index: ii,
            same_orientation,
            max_deviation: dist,
        }));
    }

    Ok(None)
}

/// Not upstream: the index of the first of `edges` between the query's vertices that is
/// proved to be the query's curve (docs/booleans.md, "One edge or two"): every point of
/// the curve over its interval within the query's tolerance plus the edge's of the edge's
/// curve over its interval, and every point of the edge's curve as near the query's, or in
/// the ball of one of the two vertices, which holds both curves' ends
/// ([`Curve::proved_together`]: an enclosure, where [`find_coincident_edge`] samples). A
/// vertex's ball is its tolerance about its point, or twice the distance from its point
/// to the query curve's end at it where that is more and stays clear of its far vertices
/// ([`vertex_balls`]): what the vertex has once the curve is an edge, whichever curve at
/// the vertex is asked first (docs/booleans.md, "What a vertex stands for"). A query
/// curve the proof cannot enclose ([`Curve::can_be_proved`]) is no edge's, decided before
/// anything is read. The edge owning the curve is skipped. A seam of
/// `face` (the face on both sides of it, [`crate::topology::edge::face_on_both_sides`])
/// bounds no material, and its tolerance adds nothing: the curve is the seam only within
/// the query's own reach (docs/booleans.md, "Seams and poles"), which for the
/// intersection curve of `face` with the surface `crossing` is its tolerance over the
/// sine of the two surfaces' least angle along it
/// ([`crate::surface::normal_cone::intersection_reach`]; without `crossing` the curve is
/// known to the query's tolerance). `None` where no edge is proved.
///
/// # Errors
///
/// Those of the edge and vertex queries and of the proof (its cancellation).
pub fn find_edge_proved_along(
    k: &Kernel,
    brep: &Brep,
    face: FaceId,
    query: &CoincidentEdgeQuery<'_>,
    edges: &[EdgeId],
    crossing: Option<&Surface>,
) -> Result<Option<usize>> {
    if !query.curve.can_be_proved() {
        return Ok(None);
    }
    let (v1, v2) = (query.start_vertex, query.end_vertex);
    // the curve's reach, read once and only for a seam
    let mut reach: Option<SourceReach> = None;
    // the two vertices' balls, read once and only where an edge runs between them
    let mut balls: Option<Vec<(Point3, f64)>> = None;
    for (ii, &e) in edges.iter().enumerate() {
        if query.curve_owner == Some(e) {
            continue;
        }
        let matches = (edge_vertex(brep, e)? == Some(v1)
            && edge_other_vertex(brep, e, v1)? == Some(v2))
            || (edge_vertex(brep, e)? == Some(v2) && edge_other_vertex(brep, e, v2)? == Some(v1));
        if !matches {
            continue;
        }
        // The edge's tolerance, at most the B-rep's: an edge's tolerance grows when a vertex
        // of it takes in another point (a merge already decided), and read back here it
        // decided the next edge by the merges before it. A 2 x 3 sheet shelled by exactly two
        // tolerances had its wall along y = 3, merged after the one along y = 0, made one
        // edge where y = 0 kept two (`tests/one_edge_or_two_symmetric_walls.rs`). The B-rep's
        // tolerance is the same for every edge of the operation.
        let edge_tol = edge_tolerance(brep, e)?.min(brep.tolerance());
        // A seam of `face` adds nothing (docs/booleans.md, "Seams and poles").
        let summed = query.tolerance + edge_tol;
        let tol = if crate::topology::edge::face_on_both_sides(brep, e, face)? {
            let known = if let Some(known) = reach {
                known
            } else {
                let own = crate::topology::face::p2::surface(brep, face)?;
                let known = match (crossing, own) {
                    (Some(other), Some(own)) => intersection_reach(
                        k,
                        query.curve,
                        &query.interval,
                        own,
                        other,
                        query.tolerance,
                    )?
                    .map_or(SourceReach::Unbounded, SourceReach::Across),
                    _ => SourceReach::Own,
                };
                reach = Some(known);
                known
            };
            known.contact_tolerance(query.tolerance, summed)
        } else {
            summed
        };
        let edge_ivl = edge_interval(brep, e)?;
        let ends = match balls.take() {
            Some(ends) => ends,
            None => vertex_balls(k, brep, query)?,
        };
        let together = query.curve.proved_together(
            k,
            &query.interval,
            edge_curve(brep, e)?,
            &edge_ivl,
            tol,
            &ends,
        )?;
        balls = Some(ends);
        if together {
            return Ok(Some(ii));
        }
    }
    Ok(None)
}

/// Not upstream: the balls of the query's two vertices for [`find_edge_proved_along`]: each
/// vertex's point with its tolerance, or with twice the distance from the point to the
/// query curve's end at it where that is more. A vertex stands for the end of each curve
/// at it (docs/booleans.md, "What a vertex stands for"): were the curve an edge of its
/// own, that end would have to lie within half the vertex's tolerance
/// (docs/validity-and-tolerances.md, section 5.2), so the vertex's ball is at least that
/// wide, whichever of the curves that end at the vertex is imprinted first. The end at a
/// vertex is the nearer of the curve's two (the two vertices differ), its distance taken
/// with its rounding. A cover is taken only while its ball stays clear of the vertex's far
/// vertices (the other end of the query curve and of each of its edges), each at its own
/// tolerance or cover, the two judged together
/// ([`covers_staying_clear`](crate::boolean::topology_intersector::p1::covers_staying_clear),
/// the rule the imprint's covers follow): a vertex cannot stand for both ends of a curve.
/// A cover that is not finite, or not clear, is not taken: the stored tolerance stays.
///
/// # Errors
///
/// Those of the vertex and edge queries and of the curve's evaluation.
fn vertex_balls(
    k: &Kernel,
    brep: &Brep,
    query: &CoincidentEdgeQuery<'_>,
) -> Result<Vec<(Point3, f64)>> {
    use crate::boolean::topology_intersector::p1::{covers_staying_clear, Growth};
    let curve_ends = [
        query.curve.evaluate_point(k, query.interval.min())?,
        query.curve.evaluate_point(k, query.interval.max())?,
    ];
    let ends = [query.start_vertex, query.end_vertex];
    let mut stored = Vec::new();
    let mut covers = Vec::new();
    for v in ends {
        let point = brep.vertex(v)?.point;
        let tolerance = vertex_tolerance(brep, v)?;
        stored.push((v, point, tolerance));
        let mut gap = f64::INFINITY;
        for end in curve_ends {
            let d = point.distance(end) + gamma(8) * (point.length() + end.length());
            if d < gap {
                gap = d;
            }
        }
        let cover = 2.0 * gap;
        if !(cover.is_finite() && cover > tolerance) {
            continue;
        }
        let mut fars: Vec<VertexId> = ends.iter().copied().filter(|&o| o != v).collect();
        for e in crate::topology::vertex::edges(brep, v, None)? {
            let start = crate::topology::edge::start_vertex(brep, e)?;
            let end = crate::topology::edge::end_vertex(brep, e)?;
            let far = if start == Some(v) { end } else { start };
            fars.extend(far.filter(|&f| f != v));
        }
        let mut reach = Vec::new();
        for f in fars {
            let far = brep.vertex(f)?;
            // the distance less its rounding: a lower bound
            let apart =
                point.distance(far.point) - gamma(8) * (point.length() + far.point.length());
            reach.push((Some(f), apart, far.tolerance));
        }
        covers.push(vec![Growth {
            vertex: v,
            tolerance: cover,
            reach,
        }]);
    }
    let taken = covers_staying_clear(&[], &covers);
    Ok(stored
        .into_iter()
        .map(|(v, point, tolerance)| {
            let ball = taken
                .iter()
                .find(|&&(w, _)| w == v)
                .map_or(tolerance, |&(_, cover)| cover);
            (point, ball)
        })
        .collect())
}

/// Finds the edge of `brep` nearest to `point` and the curve parameter of the
/// closest point on it (`SmTools::FindEdge`). There is no distance limit.
///
/// Returns `None` when no drop succeeds (upstream `SM_ERR`).
///
/// # Errors
/// Those of `SmBrep::GetEdges` and of the curve drops (`SER`); `SM_ERR_NULL_POINTER`
/// for an edge without curve (upstream dereferences it).
pub fn find_edge(k: &Kernel, brep: &Brep, point: Point3) -> Result<Option<EdgePick>> {
    let edges = brep_edges(brep)?;
    let mut best: Option<EdgePick> = None;
    let mut best_dist = BIG_DOUBLE;
    for (i, &e) in edges.iter().enumerate() {
        let ivl = edge_interval(brep, e)?;
        let drop = edge_drop_point(k, brep, e, &ivl, point, edge_tolerance(brep, e)? * 10.0)?;
        if drop.good && drop.distance < best_dist {
            best_dist = drop.distance;
            best = Some(EdgePick {
                index: i,
                parameter: drop.parameter,
            });
        }
    }
    Ok(best)
}

/// Finds a face of `brep` that `point` lies on (`SmTools::FindFace`): the first
/// face among the B-rep point-solve solutions (tolerance `100 *` B-rep tolerance).
///
/// Returns `None` when no solution is a face (upstream `SM_ERR`).
/// The caller supplies its kernel, B-rep id, object keys and intermediate owners;
/// owner keys must use the same mapping that assigned the geometry's owner links.
///
/// # Errors
/// Errors from B-rep cache construction and `BrepPointSolve`, including
/// explicit deferrals for their unfinished dependencies.
pub fn find_face(
    kernel: &Kernel,
    brep: &mut Brep,
    brep_id: BrepId,
    point: Point3,
    keys: &dyn Fn(CacheObject) -> ObjectRef,
    owner_of: &dyn Fn(ObjectRef) -> Option<ObjectRef>,
) -> Result<Option<FaceId>> {
    let tol = brep.tolerance() * 100.0;
    let solution_faces = brep_point_solve_faces(kernel, brep, brep_id, point, tol, keys, owner_of)?;
    Ok(solution_faces.into_iter().flatten().next())
}

// ---- calls into units not ported yet ------------------------------------------

/// `SmVertex::GetTolerance`.
fn vertex_tolerance(brep: &Brep, v: VertexId) -> Result<f64> {
    crate::topology::vertex::tolerance(brep, v)
}

/// `SmEdge::GetVertex`.
fn edge_vertex(brep: &Brep, e: EdgeId) -> Result<Option<VertexId>> {
    crate::topology::edge::vertex(brep, e)
}

/// `SmEdge::GetOtherVertex`.
fn edge_other_vertex(brep: &Brep, e: EdgeId, v: VertexId) -> Result<Option<VertexId>> {
    crate::topology::edge::other_vertex(brep, e, Some(v))
}

/// `SmEdge::GetTolerance`.
fn edge_tolerance(brep: &Brep, e: EdgeId) -> Result<f64> {
    crate::topology::edge::tolerance(brep, e)
}

/// `SmEdge::GetInterval`.
fn edge_interval(brep: &Brep, e: EdgeId) -> Result<Extent1d> {
    crate::topology::edge::interval(brep, e)
}

/// `pE->GetCurve()`, `NER` on a missing curve.
fn edge_curve(brep: &Brep, e: EdgeId) -> Result<&Curve> {
    crate::topology::edge::curve(brep, e)?
        .ok_or_else(|| SolidError::geom("geom::tools::edge_curve", GeomFailure::EdgeWithoutCurve))
}

/// `pE->GetCurve()->EvaluatePoint(t)` (`NER` on a missing curve).
fn edge_curve_point(k: &Kernel, brep: &Brep, e: EdgeId, t: f64) -> Result<Point3> {
    edge_curve(brep, e)?.evaluate_point(k, t)
}

/// First derivative from `pE->GetCurve()->Evaluate(t, 1, TRUE, ...)`.
fn edge_curve_derivative(k: &Kernel, brep: &Brep, e: EdgeId, t: f64) -> Result<Vec3> {
    let pv = edge_curve(brep, e)?.evaluate(k, t, 1, true, true)?;
    pv.get(1).copied().ok_or_else(|| {
        SolidError::geom_defect_with_status(
            "geom::tools::edge_curve_derivative",
            "Curve::evaluate returned no first derivative though one was asked for, against its contract of returning derivatives 0..=num_derivs",
            SM_ERR_NULL_POINTER,
        )
    })
}

/// First derivative from `pCurve->Evaluate(t, 1, TRUE, ...)`.
fn curve_derivative(k: &Kernel, curve: &Curve, t: f64) -> Result<Vec3> {
    let pv = curve.evaluate(k, t, 1, true, true)?;
    pv.get(1).copied().ok_or_else(|| {
        SolidError::geom_defect_with_status(
            "geom::tools::curve_derivative",
            "Curve::evaluate returned no first derivative though one was asked for, against its contract of returning derivatives 0..=num_derivs",
            SM_ERR_NULL_POINTER,
        )
    })
}

/// `pCurve->DropPoint(ivl, pt, NULL, tol, &guess, ...)` (local solve, `SM_SO_MINIMIZE`).
fn drop_point(
    k: &Kernel,
    curve: &Curve,
    ivl: &Extent1d,
    pt: Point3,
    tol: f64,
    guess: f64,
) -> Result<Drop> {
    let d = curve.drop_point(
        k,
        ivl,
        pt,
        None,
        tol,
        Some(guess),
        SolverOperation::Minimize,
    )?;
    Ok(Drop {
        good: d.success,
        parameter: d.parameter,
        distance: d.distance,
    })
}

/// `pE->GetCurve()->DropPoint(ivl, pt, NULL, tol, NULL, ...)` (global solve,
/// `SM_SO_MINIMIZE`).
fn edge_drop_point(
    k: &Kernel,
    brep: &Brep,
    e: EdgeId,
    ivl: &Extent1d,
    pt: Point3,
    tol: f64,
) -> Result<Drop> {
    let d =
        edge_curve(brep, e)?.drop_point(k, ivl, pt, None, tol, None, SolverOperation::Minimize)?;
    Ok(Drop {
        good: d.success,
        parameter: d.parameter,
        distance: d.distance,
    })
}

/// `pCurve->CurveMaxDistanceBetween(ivl, *pE->GetCurve(), t0, t1, samples, &tol,
/// dist, NULL, NULL)` (`NER` on a missing edge curve).
#[allow(clippy::too_many_arguments)]
fn curve_max_distance_between(
    k: &Kernel,
    curve: &Curve,
    ivl: &Extent1d,
    brep: &Brep,
    e: EdgeId,
    other_range: (f64, f64),
    samples: u32,
    tol: f64,
) -> Result<f64> {
    let other = edge_curve(brep, e)?;
    Ok(curve
        .curve_max_distance_between(
            k,
            ivl,
            other,
            other_range.0,
            other_range.1,
            u64::from(samples),
            Some(tol),
        )?
        .distance)
}

/// `SmBrep::GetEdges`.
fn brep_edges(brep: &Brep) -> Result<Vec<EdgeId>> {
    crate::topology::brep::p1::edges(brep, None)
}

/// `SmTopologySolver::BrepPointSolve(brep, pt, SM_SO_MINIMIZE, SM_SR_ALL, tol,
/// SM_BIG_DOUBLE, NULL, solutions)`: per solution, its first object if it is a face.
fn brep_point_solve_faces(
    kernel: &Kernel,
    brep: &mut Brep,
    brep_id: BrepId,
    pt: Point3,
    tol: f64,
    keys: &dyn Fn(CacheObject) -> ObjectRef,
    owner_of: &dyn Fn(ObjectRef) -> Option<ObjectRef>,
) -> Result<Vec<Option<FaceId>>> {
    use crate::geom::global_solver::{SolutionRequested, SolverOperation};
    use crate::intersect::topology_solver::p2::TopologySolveOptions;
    use crate::query::brep::{solve_mutable_brep_point, BrepIdentity};
    use crate::topology::brep_cache::{make_brep_cache, CacheSource};
    let cache = make_brep_cache(kernel, CacheSource::Brep(brep), keys)?;
    // the solve builds the face caches it reaches (`smsurf_GetSurfaceCache`), which
    // creates their UV trim curves on the B-rep
    let identity = BrepIdentity {
        brep_id,
        keys,
        owner_of,
    };
    let solutions = solve_mutable_brep_point(
        kernel,
        brep,
        &identity,
        &cache,
        pt,
        TopologySolveOptions {
            operation: SolverOperation::Minimize,
            requested: SolutionRequested::All,
            tolerance_3d: tol,
            best_answer_so_far_sq: BIG_DOUBLE,
            opt_vectors: &[],
        },
    )?;
    Ok(solutions
        .as_slice()
        .iter()
        .map(|s| match s.objects.first().copied().flatten() {
            Some(CacheObject::Face(f)) => Some(f),
            _ => None,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(x: f64, y: f64, z: f64) -> Point3 {
        Point3::new(x, y, z)
    }

    #[test]
    fn point_single_hit() {
        let pts = [p(0.0, 0.0, 0.0), p(5.0, 0.0, 0.0), p(0.001, 0.0, 0.0)];
        let m = find_first_point_within_tol_to_pnt(&pts, 0, 0.01).unwrap();
        assert!(m.found);
        assert_eq!(m.index, 2);
        assert!((m.closest_distance - 0.001).abs() < 1e-15);
        assert!((m.second_closest_distance - 5.0).abs() < 1e-15);
    }

    #[test]
    fn point_double_hit_is_accepted() {
        let pts = [p(0.0, 0.0, 0.0), p(0.001, 0.0, 0.0), p(-0.001, 0.0, 0.0)];
        let m = find_first_point_within_tol_to_pnt(&pts, 0, 0.01).unwrap();
        assert!(m.found);
        assert_eq!(m.index, 1);
        assert_eq!(m.closest_distance, m.second_closest_distance);
    }

    #[test]
    fn point_alone() {
        let m = find_first_point_within_tol_to_pnt(&[p(1.0, 2.0, 3.0)], 0, 1.0).unwrap();
        assert!(!m.found);
        assert_eq!(m.index, 0);
        assert_eq!(m.closest_distance, BIG_DOUBLE.sqrt());
        assert_eq!(m.second_closest_distance, BIG_DOUBLE.sqrt());
    }

    #[test]
    fn point_bad_target() {
        assert!(find_first_point_within_tol_to_pnt(&[p(1.0, 2.0, 3.0)], 1, 1.0).is_err());
        assert!(find_first_point_within_tol_to_pnt(&[], 0, 1.0).is_err());
    }

    #[test]
    fn vertex_empty_list() {
        let brep = Brep::new();
        let m = find_first_vertex_within_tol_to_pnt(&brep, &[], p(0.0, 0.0, 0.0), 1.0).unwrap();
        assert_eq!(
            m,
            VertexMatch {
                found: false,
                index: NO_VERTEX_INDEX,
                distance: BIG_DOUBLE
            }
        );
    }

    #[test]
    fn unported_paths() {
        let mut brep = Brep::new();
        let a = brep.add_vertex(p(0.0, 0.0, 0.0)).unwrap();
        let b = brep.add_vertex(p(1.0, 0.0, 0.0)).unwrap();
        let e = brep.add_edge(a, b).unwrap();
        // vertex tolerances are ported (topology.vertex): the vertex at the point is found
        let r = find_first_vertex_within_tol_to_pnt(&brep, &[a], p(0.0, 0.0, 0.0), 1.0).unwrap();
        assert!(r.found);
        assert_eq!(r.index, 0);
        // the edge's own line: skipped when it is the curve's owner
        let line = Curve::from(crate::curve::line::Line::from_points(
            p(0.0, 0.0, 0.0),
            p(1.0, 0.0, 0.0),
            3,
        ));
        brep.edge_mut(e).unwrap().curve = Some(line.clone());
        brep.edge_mut(e).unwrap().interval = Extent1d::from_min_max(0.0, 1.0);
        let k = Kernel::default();
        let mut q = CoincidentEdgeQuery {
            start_vertex: a,
            end_vertex: b,
            curve: &line,
            curve_owner: None,
            interval: Extent1d::from_min_max(0.0, 1.0),
            tolerance: 1e-6,
        };
        // the curve evaluations are ported: a bare edge (no edge uses) fails in
        // SmEdge::GetVertex, not on an unported callee
        let r = find_coincident_edge(&k, &brep, &q, &[e], 0);
        assert!(matches!(r, Err(SolidError::InvalidInput { .. })));
        assert_eq!(find_coincident_edge(&k, &brep, &q, &[e], 1).unwrap(), None);
        q.curve_owner = Some(e);
        assert_eq!(find_coincident_edge(&k, &brep, &q, &[e], 0).unwrap(), None);
        // the edge's line is the nearest (and only) edge: the drop finds its start
        let hit = find_edge(&k, &brep, p(0.0, 0.0, 0.0)).unwrap().unwrap();
        assert_eq!(hit.index, 0);
        assert!(hit.parameter.abs() < 1e-12);
        // the B-rep box is ported (query.brep): the cache builds and, without faces,
        // the solve finds none
        let r = find_face(
            &Kernel::default(),
            &mut brep,
            BrepId::from_index(7),
            p(0.0, 0.0, 0.0),
            &crate::topology::brep_cache::arena_identity,
            &|_| None,
        );
        assert!(matches!(r, Ok(None)), "{r:?}");
    }

    /// The unit box's edge from the origin to (1, 0, 0), its vertices and a face of it.
    fn box_edge(k: &mut Kernel) -> (Brep, EdgeId, VertexId, VertexId, FaceId) {
        let brep =
            crate::primitive::api_primitives::create_box(k, Point3::ZERO, 1.0, 1.0, 1.0).unwrap();
        let at = |v: VertexId, x: f64| {
            let q = brep.vertex(v).unwrap().point;
            q.distance(p(x, 0.0, 0.0)) < 1e-12
        };
        let e = crate::topology::brep::p1::edges(&brep, None)
            .unwrap()
            .into_iter()
            .find(|&e| {
                let s = crate::topology::edge::start_vertex(&brep, e)
                    .unwrap()
                    .unwrap();
                let t = crate::topology::edge::end_vertex(&brep, e)
                    .unwrap()
                    .unwrap();
                (at(s, 0.0) && at(t, 1.0)) || (at(s, 1.0) && at(t, 0.0))
            })
            .unwrap();
        let s = crate::topology::edge::start_vertex(&brep, e)
            .unwrap()
            .unwrap();
        let t = crate::topology::edge::end_vertex(&brep, e)
            .unwrap()
            .unwrap();
        let (a, b) = if at(s, 0.0) { (s, t) } else { (t, s) };
        let face = crate::topology::vertex::faces(&brep, a).unwrap()[0];
        (brep, e, a, b, face)
    }

    /// #19045 review: a query curve the proof cannot enclose (a composite curve, which does
    /// not even evaluate) is no edge's: the answer is no match, not the error of reading
    /// its ends for the vertices' balls.
    #[test]
    fn a_curve_the_proof_cannot_enclose_matches_no_edge() {
        let mut k = Kernel::default();
        let (brep, e, a, b, face) = box_edge(&mut k);
        let line = Curve::from(crate::curve::line::Line::from_points(
            p(0.0, 0.0, 0.0),
            p(1.0, 0.0, 0.0),
            3,
        ));
        let composite = Curve::from(
            crate::curve::composite_curve::p1::CompositeCurve::with_curves(
                3,
                vec![line],
                false,
                None,
                None,
            )
            .unwrap(),
        );
        let query = CoincidentEdgeQuery {
            start_vertex: a,
            end_vertex: b,
            curve: &composite,
            curve_owner: None,
            interval: Extent1d::from_min_max(0.0, 1.0),
            tolerance: 1e-6,
        };
        assert_eq!(
            find_edge_proved_along(&k, &brep, face, &query, &[e], None).unwrap(),
            None
        );
    }

    /// #19045 review: a vertex's ball covers the query curve's end only while it stays
    /// clear of its far vertices. A line 0.3 beside the box's unit edge, asked between the
    /// edge's two vertices, would give each a ball of 0.6: together they hold the whole
    /// edge and the whole line, which were then proved one edge though 0.3 apart. The two
    /// balls reach each other (0.6 + 0.6 > 1), so both keep their stored tolerance and the
    /// line is no edge's; the edge's own line still is.
    #[test]
    fn vertex_balls_that_reach_each_other_prove_no_edge() {
        let mut k = Kernel::default();
        let (brep, e, a, b, face) = box_edge(&mut k);
        let beside = Curve::from(crate::curve::line::Line::from_points(
            p(0.0, 0.3, 0.0),
            p(1.0, 0.3, 0.0),
            3,
        ));
        let mut query = CoincidentEdgeQuery {
            start_vertex: a,
            end_vertex: b,
            curve: &beside,
            curve_owner: None,
            interval: Extent1d::from_min_max(0.0, 1.0),
            tolerance: 1e-6,
        };
        let balls = vertex_balls(&k, &brep, &query).unwrap();
        for (ball, v) in balls.iter().zip([a, b]) {
            assert_eq!(ball.1, vertex_tolerance(&brep, v).unwrap());
        }
        assert_eq!(
            find_edge_proved_along(&k, &brep, face, &query, &[e], None).unwrap(),
            None
        );
        let own = Curve::from(crate::curve::line::Line::from_points(
            p(0.0, 0.0, 0.0),
            p(1.0, 0.0, 0.0),
            3,
        ));
        query.curve = &own;
        assert_eq!(
            find_edge_proved_along(&k, &brep, face, &query, &[e], None).unwrap(),
            Some(0)
        );
    }
}
